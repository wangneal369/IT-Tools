//! #21 正则表达式测试器（RegexTester）。
//!
//! 透明双引擎：常规模式自动使用线性时间 `regex`；检测到环视 / 反向引用时
//! 自动回退到 `fancy-regex`（除非 `strict_linear_only` 开启）。flags
//! `i/m/s/x/U/g`，返回匹配区间、捕获组表与替换预览。
//!
//! ReDoS 护栏：实时 200,000 / 手动 1,000,000 回溯预算、`CancelFlag`  cooperative
//! 取消、匹配输出上限 5,000。灾难性回溯（如 `(a+)+b` 对 `a*30!`）由 fancy
//! 引擎的 `backtrack_limit` 拦截，返回 [`ToolErrorKind::UnsupportedInput`]。

use fancy_regex::{Captures, RegexBuilder as FancyBuilder};
use regex::{Regex, RegexBuilder};

use crate::{CancelFlag, ToolError, ToolErrorKind};

/// 实时回溯预算。
pub const LIVE_BACKTRACK_LIMIT: usize = 200_000;
/// 手动运行回溯预算。
pub const MANUAL_BACKTRACK_LIMIT: usize = 1_000_000;
/// 渲染匹配条数上限。
pub const MATCH_OUTPUT_CAP: usize = 5_000;

/// 正则标志。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_snake_case)] // `U`（ungreedy）是规格约定的标志名
pub struct RegexFlags {
    pub i: bool,
    pub m: bool,
    pub s: bool,
    pub x: bool,
    pub U: bool,
    pub g: bool,
}

/// 使用的引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Linear,
    Backtracking,
}

/// 单个匹配结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexMatch {
    pub start: usize,
    pub end: usize,
    pub text: String,
    /// 捕获组表：索引（1-based）→ 内容；未参与匹配的组为 `None`。
    pub groups: Vec<(usize, Option<String>)>,
}

/// 正则执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexResult {
    pub engine: Engine,
    pub matches: Vec<RegexMatch>,
    /// 替换预览（当提供 `replacement` 时）。
    pub replacement_preview: Option<String>,
    /// 是否因输出上限而截断。
    pub truncated: bool,
}

/// 检测「重复组内再含量词」的嵌套重复结构（典型 ReDoS 形态，如 `(a+)+b`）。
pub fn has_nested_quantifier(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut escaped = false;
    let mut in_class = false;
    let mut group_stack: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if in_class {
            if b == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'[' => in_class = true,
            b'(' => group_stack.push(false),
            b')' => {
                let inner_repeat = group_stack.pop().unwrap_or(false);
                if inner_repeat && matches!(bytes.get(i + 1), Some(b'+') | Some(b'*') | Some(b'{'))
                {
                    return true;
                }
            }
            b'+' | b'*' => {
                if let Some(inner) = group_stack.last_mut() {
                    *inner = true;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// 判断模式是否必须使用回溯引擎（环视或反向引用）。
pub fn needs_backtracking(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut escaped = false;
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            // \1..\9 反向引用（保守判断）。
            if b.is_ascii_digit() && b != b'0' {
                return true;
            }
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if in_class {
            if b == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if b == b'[' {
            in_class = true;
        } else if b == b'('
            && (pattern[i..].starts_with("(?=")
                || pattern[i..].starts_with("(?!")
                || pattern[i..].starts_with("(?<=")
                || pattern[i..].starts_with("(?<!"))
        {
            return true;
        }
        i += 1;
    }
    false
}

fn escape_pattern_for_linear(pattern: &str) -> Result<String, ToolError> {
    // 线性引擎不支持环视 / 反向引用，直接拒绝（由调用方保证已分流）。
    if needs_backtracking(pattern) {
        return Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            "该模式包含环视或反向引用，线性引擎不支持；请关闭严格安全模式以启用回溯引擎",
        ));
    }
    Ok(pattern.to_owned())
}

fn build_linear(pattern: &str, flags: &RegexFlags) -> Result<Regex, ToolError> {
    let pattern = escape_pattern_for_linear(pattern)?;
    RegexBuilder::new(&pattern)
        .case_insensitive(flags.i)
        .multi_line(flags.m)
        .dot_matches_new_line(flags.s)
        .ignore_whitespace(flags.x)
        .swap_greed(flags.U)
        .unicode(true)
        .build()
        .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("正则表达式无效：{e}")))
}

fn build_fancy(
    pattern: &str,
    flags: &RegexFlags,
    budget: usize,
) -> Result<fancy_regex::Regex, ToolError> {
    FancyBuilder::new(pattern)
        .backtrack_limit(budget)
        .case_insensitive(flags.i)
        .multi_line(flags.m)
        .dot_matches_new_line(flags.s)
        .verbose_mode(flags.x)
        .build()
        .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("正则表达式无效：{e}")))
}

/// 执行正则测试。
///
/// `manual` 为 `true` 时使用 1M 预算（手动运行），否则使用 200k（实时）。
pub fn test_regex(
    pattern: &str,
    text: &str,
    flags: &RegexFlags,
    replacement: Option<&str>,
    strict_linear_only: bool,
    manual: bool,
    cancel: &CancelFlag,
) -> Result<RegexResult, ToolError> {
    let fancy = needs_backtracking(pattern);
    if fancy && strict_linear_only {
        return Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            "严格安全模式禁止环视 / 反向引用等回溯特性",
        )
        .with_hint("关闭严格安全模式，或改写为线性等价表达式"));
    }
    if cancel.is_cancelled() {
        return Err(ToolError::new(ToolErrorKind::Cancelled, "正则测试已取消"));
    }

    // 嵌套量词（如 `(a+)+b`）是指数回溯形态：fancy 可能用线性 delegate
    // 优化而绕过 backtrack_limit。对较长输入主动拒绝，避免病态耗时。
    if has_nested_quantifier(pattern) && text.len() > 16 {
        return Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            "检测到嵌套量词且输入较长，存在灾难性回溯（ReDoS）风险，已拒绝执行",
        )
        .with_hint("改写为无嵌套量词的等价表达式，或缩短输入后重试"));
    }

    let budget = if manual {
        MANUAL_BACKTRACK_LIMIT
    } else {
        LIVE_BACKTRACK_LIMIT
    };
    let mut matches = Vec::new();
    let mut truncated = false;

    if !fancy {
        let re = build_linear(pattern, flags)?;
        for m in re.find_iter(text) {
            if cancel.is_cancelled() {
                return Err(ToolError::new(ToolErrorKind::Cancelled, "正则测试已取消"));
            }
            if matches.len() >= MATCH_OUTPUT_CAP {
                truncated = true;
                break;
            }
            matches.push(RegexMatch {
                start: m.start(),
                end: m.end(),
                text: m.as_str().to_owned(),
                groups: Vec::new(),
            });
            if !flags.g {
                break;
            }
        }
        // 捕获组表（取首个匹配，避免重复开销）。
        if let Some(caps) = re.captures(text)
            && let Some(first) = matches.first_mut()
        {
            first.groups = (1..caps.len())
                .map(|i| (i, caps.get(i).map(|m| m.as_str().to_owned())))
                .collect();
        }
    } else {
        let re = build_fancy(pattern, flags, budget)?;
        let caps_iter = re.captures_iter(text);
        for caps_result in caps_iter {
            if cancel.is_cancelled() {
                return Err(ToolError::new(ToolErrorKind::Cancelled, "正则测试已取消"));
            }
            let caps: Captures<'_, str> = caps_result.map_err(|e| {
                ToolError::new(
                    ToolErrorKind::UnsupportedInput,
                    format!("正则回溯超出预算（疑似灾难性回溯）：{e}"),
                )
                .with_hint("减少输入长度、优化表达式，或使用手动运行以提高预算")
            })?;
            if matches.len() >= MATCH_OUTPUT_CAP {
                truncated = true;
                break;
            }
            let whole = caps
                .get(0)
                .map(|m| (m.start(), m.end(), m.as_str().to_owned()));
            if let Some((start, end, text)) = whole {
                matches.push(RegexMatch {
                    start,
                    end,
                    text,
                    groups: (1..caps.len())
                        .map(|i| (i, caps.get(i).map(|m| m.as_str().to_owned())))
                        .collect(),
                });
            }
            if !flags.g {
                break;
            }
        }
    }

    let replacement_preview = replacement.map(|rep| {
        if !fancy {
            let re = build_linear(pattern, flags).expect("已成功构建");
            if flags.g {
                re.replace_all(text, rep).into_owned()
            } else {
                re.replace(text, rep).into_owned()
            }
        } else {
            let re = build_fancy(pattern, flags, budget).expect("已成功构建");
            if flags.g {
                re.replace_all(text, rep).into_owned()
            } else {
                re.replace(text, rep).into_owned()
            }
        }
    });

    Ok(RegexResult {
        engine: if fancy {
            Engine::Backtracking
        } else {
            Engine::Linear
        },
        matches,
        replacement_preview,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_engine_basic_flags_and_captures() {
        let flags = RegexFlags {
            i: true,
            g: true,
            ..Default::default()
        };
        let result = test_regex(
            r"(\w+)@(\w+)",
            "A@b c@d",
            &flags,
            Some("$1"),
            false,
            false,
            &CancelFlag::new(),
        )
        .unwrap();
        assert_eq!(result.engine, Engine::Linear);
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].start, 0);
        assert_eq!(result.matches[0].groups.len(), 2);
        assert_eq!(result.matches[0].groups[0].1.as_deref(), Some("A"));
        assert_eq!(result.replacement_preview.as_deref(), Some("A c"));
    }

    #[test]
    fn lookaround_uses_backtracking_engine() {
        assert!(needs_backtracking(r"(?=foo)bar"));
        let result = test_regex(
            r"(?<=a)b",
            "ab",
            &RegexFlags::default(),
            None,
            false,
            false,
            &CancelFlag::new(),
        )
        .unwrap();
        assert_eq!(result.engine, Engine::Backtracking);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].start, 1);
    }

    #[test]
    fn strict_linear_only_rejects_lookaround() {
        let err = test_regex(
            r"(?=a)a",
            "a",
            &RegexFlags::default(),
            None,
            true,
            false,
            &CancelFlag::new(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn catastrophic_backtracking_returns_graceful_error() {
        // (a+)+b 对 a*30! 是经典指数回溯；fancy 的 backtrack_limit 必须在
        // 有限时间内拦截，而不能挂起测试线程。
        let text = "a".repeat(30) + "!";
        let result = test_regex(
            r"^(a+)+b$",
            &text,
            &RegexFlags::default(),
            None,
            false,
            false,
            &CancelFlag::new(),
        );
        assert!(result.is_err(), "灾难性回溯必须返回错误而不是挂起");
        assert_eq!(result.unwrap_err().kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn cancel_flag_is_respected() {
        let flag = CancelFlag::new();
        flag.cancel();
        let err = test_regex(
            "a",
            "aaa",
            &RegexFlags::default(),
            None,
            false,
            false,
            &flag,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Cancelled);
    }

    #[test]
    fn match_output_cap() {
        let result = test_regex(
            "a",
            &"a".repeat(MATCH_OUTPUT_CAP + 10),
            &RegexFlags {
                g: true,
                ..Default::default()
            },
            None,
            false,
            false,
            &CancelFlag::new(),
        )
        .unwrap();
        assert_eq!(result.matches.len(), MATCH_OUTPUT_CAP);
        assert!(result.truncated);
    }
}

//! #17 文本对比（TextDiff）。
//!
//! 基于 `similar` 提供行级并排 / 行内 hunk、忽略空白 / 大小写、折叠未变上下文、
//! 词级高亮与 Unified Diff 导出。护栏：> 1 MB 或 > 20,000 行自动关闭词级高亮，
//! > 10 MB 直接拒绝（[`ToolErrorKind::SizeLimitExceeded`]）。

use similar::{ChangeTag, DiffOp, TextDiff, TextDiffConfig, WhitespaceMode};

use crate::{CancelFlag, ToolError, ToolErrorKind};

/// 词级高亮自动关闭阈值：1 MB。
pub const WORD_DIFF_SIZE_LIMIT: usize = 1024 * 1024;
/// 词级高亮自动关闭阈值：20,000 行。
pub const WORD_DIFF_LINE_LIMIT: usize = 20_000;
/// 输入硬上限：10 MB。
pub const TEXT_DIFF_HARD_LIMIT: usize = 10 * 1024 * 1024;

/// 对比选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDiffOptions {
    /// 忽略空白差异（空白折叠）。
    pub ignore_whitespace: bool,
    /// 忽略大小写差异。
    pub ignore_case: bool,
    /// 折叠未变化行的上下文半径（`None` 表示保留全部上下文）。
    pub context_lines: Option<usize>,
    /// 是否请求词级高亮（超大输入会被自动禁用，见 [`TextDiffResult::word_diff_enabled`]）。
    pub word_diff: bool,
}

impl Default for TextDiffOptions {
    fn default() -> Self {
        Self {
            ignore_whitespace: false,
            ignore_case: false,
            context_lines: Some(3),
            word_diff: true,
        }
    }
}

/// 单侧行内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub tag: ChangeTag,
    pub text: String,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
}

/// 并排视图的一行：左侧 / 右侧同时保留，便于 UI 两栏渲染。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

/// 词级高亮片段：`true` 表示该片段是差异部分。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSpan {
    pub emphasized: bool,
    pub text: String,
}

/// 行内视图：一行 + 该行的词级高亮片段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRow {
    pub tag: ChangeTag,
    pub text: String,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub spans: Vec<WordSpan>,
}

/// 文本对比结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDiffResult {
    pub side_by_side: Vec<SideBySideRow>,
    pub inline: Vec<InlineRow>,
    pub word_diff_enabled: bool,
    pub additions: usize,
    pub deletions: usize,
}

/// 执行文本对比。
///
/// `cancel` 在逐行构建结果前检查一次，用于配合后台任务的标签换代取消。
pub fn diff_text(
    old: &str,
    new: &str,
    options: &TextDiffOptions,
    cancel: &CancelFlag,
) -> Result<TextDiffResult, ToolError> {
    if old.len() > TEXT_DIFF_HARD_LIMIT || new.len() > TEXT_DIFF_HARD_LIMIT {
        return Err(ToolError::new(
            ToolErrorKind::SizeLimitExceeded,
            format!(
                "文本对比输入超过 10 MB 硬上限（左 {} 字节，右 {} 字节）",
                old.len(),
                new.len()
            ),
        )
        .with_hint("请拆分文件或仅比较感兴趣的部分"));
    }
    if cancel.is_cancelled() {
        return Err(ToolError::new(ToolErrorKind::Cancelled, "文本对比已取消"));
    }

    let large = old.len() > WORD_DIFF_SIZE_LIMIT
        || new.len() > WORD_DIFF_SIZE_LIMIT
        || old.lines().count() > WORD_DIFF_LINE_LIMIT
        || new.lines().count() > WORD_DIFF_LINE_LIMIT;
    let word_diff_enabled = options.word_diff && !large;

    let mut config = TextDiffConfig::default();
    if options.ignore_whitespace {
        config.whitespace_mode(WhitespaceMode::IgnoreAll);
    }
    let old_owned;
    let new_owned;
    let (old_text, new_text): (&str, &str) = if options.ignore_case {
        old_owned = old.to_lowercase();
        new_owned = new.to_lowercase();
        (&old_owned, &new_owned)
    } else {
        (old, new)
    };

    let diff: TextDiff<'_, '_, str> = config.diff_lines(old_text, new_text);

    let mut side_by_side = Vec::new();
    let mut inline = Vec::new();
    let mut additions = 0usize;
    let mut deletions = 0usize;

    let grouped: Vec<Vec<DiffOp>> = match options.context_lines {
        Some(radius) => diff
            .grouped_ops(radius)
            .into_iter()
            .filter(|ops| !ops.is_empty())
            .collect(),
        None => {
            let ops = diff.ops();
            if ops.is_empty() {
                Vec::new()
            } else {
                vec![ops.to_vec()]
            }
        }
    };

    for group in &grouped {
        for op in group {
            for change in diff.iter_changes(op) {
                let tag = change.tag();
                let text = change
                    .value()
                    .trim_end_matches('\n')
                    .trim_end_matches('\r')
                    .to_owned();
                let old_index = change.old_index();
                let new_index = change.new_index();
                let line = DiffLine {
                    tag,
                    text: text.clone(),
                    old_index,
                    new_index,
                };
                match tag {
                    ChangeTag::Insert => {
                        additions += 1;
                        side_by_side.push(SideBySideRow {
                            left: None,
                            right: Some(line),
                        });
                    }
                    ChangeTag::Delete => {
                        deletions += 1;
                        side_by_side.push(SideBySideRow {
                            left: Some(line),
                            right: None,
                        });
                    }
                    ChangeTag::Equal => {
                        side_by_side.push(SideBySideRow {
                            left: Some(line.clone()),
                            right: Some(line),
                        });
                    }
                }
                inline.push(InlineRow {
                    tag,
                    text,
                    old_index,
                    new_index,
                    spans: Vec::new(),
                });
            }
        }
    }

    // 词级高亮：对每个 Replace hunk 取对应的旧 / 新文本做一次词级细化，
    // 将强调片段分别挂到该 hunk 覆盖的删除行与新增行上。
    if word_diff_enabled {
        let old_lines: Vec<&str> = old_text.lines().collect();
        let new_lines: Vec<&str> = new_text.lines().collect();
        for op in grouped.iter().flat_map(|g| g.iter()) {
            let (old_index, old_len, new_index, new_len) = match *op {
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => (old_index, old_len, new_index, new_len),
                _ => continue,
            };
            let before = old_lines
                .iter()
                .skip(old_index)
                .take(old_len)
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let after = new_lines
                .iter()
                .skip(new_index)
                .take(new_len)
                .copied()
                .collect::<Vec<_>>()
                .join(" ");
            let word_diff: TextDiff<'_, '_, str> =
                TextDiffConfig::default().diff_words(&before, &after);
            let spans: Vec<WordSpan> = word_diff
                .iter_all_changes()
                .take(4096)
                .map(|change| WordSpan {
                    emphasized: change.tag() != ChangeTag::Equal,
                    text: change.value().to_string(),
                })
                .collect();
            for row in inline.iter_mut() {
                if row
                    .old_index
                    .is_some_and(|i| (old_index..old_index + old_len).contains(&i))
                    || row
                        .new_index
                        .is_some_and(|i| (new_index..new_index + new_len).contains(&i))
                {
                    row.spans = spans.clone();
                }
            }
        }
    }

    Ok(TextDiffResult {
        side_by_side,
        inline,
        word_diff_enabled,
        additions,
        deletions,
    })
}

/// 导出 Unified Diff 文本。
pub fn export_unified_diff(
    old: &str,
    new: &str,
    options: &TextDiffOptions,
) -> Result<String, ToolError> {
    if old.len() > TEXT_DIFF_HARD_LIMIT || new.len() > TEXT_DIFF_HARD_LIMIT {
        return Err(ToolError::new(
            ToolErrorKind::SizeLimitExceeded,
            "文本超过 10 MB 硬上限，无法导出 Unified Diff",
        ));
    }
    let mut config = TextDiffConfig::default();
    if options.ignore_whitespace {
        config.whitespace_mode(WhitespaceMode::IgnoreAll);
    }
    let old_owned;
    let new_owned;
    let (old_text, new_text): (&str, &str) = if options.ignore_case {
        old_owned = old.to_lowercase();
        new_owned = new.to_lowercase();
        (&old_owned, &new_owned)
    } else {
        (old, new)
    };
    let diff: TextDiff<'_, '_, str> = config.diff_lines(old_text, new_text);
    let mut formatter = diff.unified_diff();
    formatter
        .context_radius(options.context_lines.unwrap_or(3))
        .header("a", "b");
    Ok(formatter.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_line_additions_and_deletions() {
        let result = diff_text(
            "a\nb\nc\n",
            "a\nx\nc\nd\n",
            &TextDiffOptions::default(),
            &CancelFlag::new(),
        )
        .unwrap();
        assert_eq!(result.additions, 2);
        assert_eq!(result.deletions, 1);
        assert!(result.word_diff_enabled);
        assert!(result.inline.iter().any(|r| r.tag == ChangeTag::Insert));
        assert!(result.inline.iter().any(|r| r.tag == ChangeTag::Delete));
    }

    #[test]
    fn ignore_case_and_whitespace() {
        let opts = TextDiffOptions {
            ignore_case: true,
            ignore_whitespace: true,
            ..Default::default()
        };
        let result = diff_text(
            "Hello World\n",
            "hello  world  \n",
            &opts,
            &CancelFlag::new(),
        )
        .unwrap();
        assert_eq!(result.additions, 0);
        assert_eq!(result.deletions, 0);
    }

    #[test]
    fn collapses_unchanged_context() {
        let old: String = (0..20).map(|i| format!("line {i}\n")).collect();
        let mut new = old.clone();
        new.push_str("extra\n");
        let result =
            diff_text(&old, &new, &TextDiffOptions::default(), &CancelFlag::new()).unwrap();
        assert!(
            result.side_by_side.len() < 21,
            "上下文应被折叠：{}",
            result.side_by_side.len()
        );
        assert_eq!(result.additions, 1);
    }

    #[test]
    fn large_input_disables_word_diff_and_hard_limit_rejects() {
        let opts = TextDiffOptions::default();
        let big = "word ".repeat(300_000);
        let result = diff_text(&big, &big, &opts, &CancelFlag::new()).unwrap();
        assert!(!result.word_diff_enabled);

        let too_big = "x".repeat(TEXT_DIFF_HARD_LIMIT + 1);
        let err = diff_text(&too_big, "", &opts, &CancelFlag::new()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
    }

    #[test]
    fn cancellation_is_respected() {
        let flag = CancelFlag::new();
        flag.cancel();
        let err = diff_text("a", "b", &TextDiffOptions::default(), &flag).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Cancelled);
    }

    #[test]
    fn unified_diff_export() {
        let text = export_unified_diff("a\nb\n", "a\nc\n", &TextDiffOptions::default()).unwrap();
        assert!(text.contains("--- "));
        assert!(text.contains("+++ "));
        assert!(text.contains("-b"));
        assert!(text.contains("+c"));
    }
}

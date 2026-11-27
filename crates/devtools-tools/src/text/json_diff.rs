//! #18 JSON 语义对比（JsonDiff）。
//!
//! 忽略键序与空白噪声；数组默认按序比较，可选无序；输出结构化 JSON Pointer
//! 差异列表（[`DiffEntry::Added`] / [`Removed`] / [`Modified`]）、键排序规范化
//! 双栏字符串与纯文本差异报告。

use serde_json::Value;

use crate::{ToolError, ToolErrorKind};

/// JSON 对比选项。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsonDiffOptions {
    /// 数组按无序集合比较（默认 `false`，即按序）。
    pub unordered_arrays: bool,
}

/// 结构化差异条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEntry {
    Added {
        pointer: String,
        value: Value,
    },
    Removed {
        pointer: String,
        value: Value,
    },
    Modified {
        pointer: String,
        old: Value,
        new: Value,
    },
}

impl DiffEntry {
    pub fn pointer(&self) -> &str {
        match self {
            Self::Added { pointer, .. }
            | Self::Removed { pointer, .. }
            | Self::Modified { pointer, .. } => pointer,
        }
    }
}

/// 双栏规范化对比结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonDiffResult {
    pub entries: Vec<DiffEntry>,
    /// 键按字母序排序后的左侧 pretty JSON。
    pub left_canonical: String,
    /// 键按字母序排序后的右侧 pretty JSON。
    pub right_canonical: String,
    /// 纯文本差异报告。
    pub report: String,
}

/// 解析两份 JSON 并执行语义对比。
pub fn diff_json(
    left: &str,
    right: &str,
    options: &JsonDiffOptions,
) -> Result<JsonDiffResult, ToolError> {
    let left_value: Value = serde_json::from_str(left).map_err(|e| {
        ToolError::new(ToolErrorKind::InvalidSyntax, format!("左侧 JSON 无效：{e}"))
            .with_hint("检查括号、逗号与引号是否配对")
    })?;
    let right_value: Value = serde_json::from_str(right).map_err(|e| {
        ToolError::new(ToolErrorKind::InvalidSyntax, format!("右侧 JSON 无效：{e}"))
    })?;

    let mut entries = Vec::new();
    compare(&left_value, &right_value, "", options, &mut entries);

    let left_canonical = canonical_pretty(&left_value);
    let right_canonical = canonical_pretty(&right_value);
    let report = render_report(&entries);

    Ok(JsonDiffResult {
        entries,
        left_canonical,
        right_canonical,
        report,
    })
}

fn escape_token(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

fn compare(
    left: &Value,
    right: &Value,
    pointer: &str,
    options: &JsonDiffOptions,
    out: &mut Vec<DiffEntry>,
) {
    if left == right {
        return;
    }
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => {
            for (key, lval) in l {
                let child = format!("{pointer}/{}", escape_token(key));
                match r.get(key) {
                    None => out.push(DiffEntry::Removed {
                        pointer: child,
                        value: lval.clone(),
                    }),
                    Some(rval) => compare(lval, rval, &child, options, out),
                }
            }
            for (key, rval) in r {
                if !l.contains_key(key) {
                    let child = format!("{pointer}/{}", escape_token(key));
                    out.push(DiffEntry::Added {
                        pointer: child,
                        value: rval.clone(),
                    });
                }
            }
        }
        (Value::Array(l), Value::Array(r)) if options.unordered_arrays => {
            // 无序模式：按元素值多重集比较，剩余元素记为增删。
            let mut unmatched_right: Vec<(usize, &Value)> = r.iter().enumerate().collect();
            for (i, lval) in l.iter().enumerate() {
                if let Some(pos) = unmatched_right.iter().position(|(_, rv)| *rv == lval) {
                    let (_, rval) = unmatched_right.remove(pos);
                    compare(lval, rval, &format!("{pointer}/{i}"), options, out);
                } else {
                    out.push(DiffEntry::Removed {
                        pointer: format!("{pointer}/{i}"),
                        value: lval.clone(),
                    });
                }
            }
            for (idx, rval) in unmatched_right {
                out.push(DiffEntry::Added {
                    pointer: format!("{pointer}/{idx}"),
                    value: rval.clone(),
                });
            }
        }
        (Value::Array(l), Value::Array(r)) => {
            let max = l.len().max(r.len());
            for i in 0..max {
                let child = format!("{pointer}/{i}");
                match (l.get(i), r.get(i)) {
                    (Some(a), Some(b)) => compare(a, b, &child, options, out),
                    (Some(a), None) => out.push(DiffEntry::Removed {
                        pointer: child,
                        value: a.clone(),
                    }),
                    (None, Some(b)) => out.push(DiffEntry::Added {
                        pointer: child,
                        value: b.clone(),
                    }),
                    (None, None) => unreachable!(),
                }
            }
        }
        _ => out.push(DiffEntry::Modified {
            pointer: if pointer.is_empty() {
                "/".to_owned()
            } else {
                pointer.to_owned()
            },
            old: left.clone(),
            new: right.clone(),
        }),
    }
}

/// 键按字母序递归排序 + 2 空格 pretty 输出，消除键序 / 空白噪声。
pub fn canonical_pretty(value: &Value) -> String {
    fn sort_keys(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                let mut out = serde_json::Map::new();
                for key in keys {
                    out.insert(key.clone(), sort_keys(&map[key]));
                }
                Value::Object(out)
            }
            Value::Array(arr) => Value::Array(arr.iter().map(sort_keys).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string_pretty(&sort_keys(value)).unwrap_or_else(|_| "{}".to_owned())
}

fn render_report(entries: &[DiffEntry]) -> String {
    if entries.is_empty() {
        return "两个 JSON 语义一致（忽略键序与空白）。\n".to_owned();
    }
    let mut out = String::new();
    for entry in entries {
        match entry {
            DiffEntry::Added { pointer, value } => {
                out.push_str(&format!("+ {pointer}: {value}\n"));
            }
            DiffEntry::Removed { pointer, value } => {
                out.push_str(&format!("- {pointer}: {value}\n"));
            }
            DiffEntry::Modified { pointer, old, new } => {
                out.push_str(&format!("~ {pointer}: {old} -> {new}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_key_order_and_whitespace() {
        let a = r#"{ "b": 2, "a": [1, 2] }"#;
        let b = "{\"a\":[1,2],\n\"b\":2}";
        let result = diff_json(a, b, &JsonDiffOptions::default()).unwrap();
        assert!(result.entries.is_empty());
        assert!(result.report.contains("语义一致"));
        assert_eq!(result.left_canonical, result.right_canonical);
    }

    #[test]
    fn reports_added_removed_modified_with_json_pointer() {
        let result = diff_json(
            r#"{"a":1,"b":{"c":2},"gone":true}"#,
            r#"{"a":9,"b":{"c":2,"d":3}}"#,
            &JsonDiffOptions::default(),
        )
        .unwrap();
        let pointers: Vec<&str> = result.entries.iter().map(DiffEntry::pointer).collect();
        assert!(pointers.contains(&"/a"));
        assert!(pointers.contains(&"/b/d"));
        assert!(pointers.contains(&"/gone"));
        assert!(result.report.contains("- /gone"));
        assert!(result.report.contains("+ /b/d"));
    }

    #[test]
    fn array_order_matters_unless_unordered() {
        let a = r#"{"x":[1,2]}"#;
        let b = r#"{"x":[2,1]}"#;
        assert!(
            !diff_json(a, b, &JsonDiffOptions::default())
                .unwrap()
                .entries
                .is_empty()
        );
        let unordered = JsonDiffOptions {
            unordered_arrays: true,
        };
        assert!(diff_json(a, b, &unordered).unwrap().entries.is_empty());
    }

    #[test]
    fn pointer_escapes_tilde_and_slash() {
        let result = diff_json(
            r#"{"a/b":{"~c":1}}"#,
            r#"{"a/b":{"~c":2}}"#,
            &JsonDiffOptions::default(),
        )
        .unwrap();
        assert!(result.entries.iter().any(|e| e.pointer() == "/a~1b/~0c"));
    }

    #[test]
    fn invalid_json_is_syntax_error() {
        let err = diff_json("{", "{}", &JsonDiffOptions::default()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}

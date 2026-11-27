//! #22 JSONPath 查询测试器（JsonPathTester）。
//!
//! 严格 RFC 9535（`serde_json_path 0.7`）。前置拦截未闭合 `\u` 转义，规避
//! 上游 `#124` panic；返回规范化 JSON Pointer 路径列表与匹配值数组双视图；
//! 明确区分合法 0 命中（`Ok(empty)`）与语法错误（`Err`）。

use serde_json::Value;
use serde_json_path::{JsonPath, LocatedNodeList};

use crate::{ToolError, ToolErrorKind};

/// 单个匹配结果：规范化路径 + 值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPathMatch {
    /// RFC 6901 JSON Pointer 形式。
    pub pointer: String,
    /// `serde_json_path` 的规范化路径显示形式。
    pub normalized: String,
    pub value: Value,
}

/// 查询结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPathResult {
    pub matches: Vec<JsonPathMatch>,
    /// 合法 0 命中时为 `true`（与语法错误相区分）。
    pub empty_result: bool,
}

/// 前置校验：拦截未闭合的 `\u` 转义序列（上游 #124 panic 防护）。
///
/// 检测字符串字面量中的 `\u` 后不足 4 位十六进制，或 `\u{...}` 未闭合。
/// 同时拦截裸 `\u` 位于表达式结尾的情况。
pub fn validate_unicode_escapes(path: &str) -> Result<(), ToolError> {
    let bytes = path.as_bytes();
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            if b == b'u' {
                // \uXXXX：必须紧跟 4 位十六进制。
                let digits = &bytes[i + 1..];
                if digits.len() < 4 || !digits[..4].iter().all(u8::is_ascii_hexdigit) {
                    return Err(ToolError::new(
                        ToolErrorKind::InvalidSyntax,
                        format!("未闭合的 \\u 转义序列（位置 {}）", i + 1),
                    )
                    .with_hint("补全为 \\uXXXX（4 位十六进制）"));
                }
            }
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_string {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = !in_string;
        }
        i += 1;
    }
    if escaped {
        // 以反斜杠结尾同样会让上游解析器产生未闭合转义。
        return Err(ToolError::new(
            ToolErrorKind::InvalidSyntax,
            "表达式以未闭合的反斜杠结尾",
        ));
    }
    Ok(())
}

/// 执行 JSONPath 查询。
///
/// 先做 `\u` 前置校验，再解析（语法错误 → `Err`），再查询；合法 0 命中返回
/// `Ok(JsonPathResult { matches: [], empty_result: true })`。
pub fn query_json_path(json: &str, path: &str) -> Result<JsonPathResult, ToolError> {
    validate_unicode_escapes(path)?;

    let value: Value = serde_json::from_str(json)
        .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("JSON 无效：{e}")))?;

    let parsed = JsonPath::parse(path).map_err(|e| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!(
                "JSONPath 语法错误（位置 {}）：{}",
                e.position(),
                e.message()
            ),
        )
        .with_hint("严格遵循 RFC 9535，例如 $.store.book[0].title")
    })?;

    let located: LocatedNodeList<'_> = parsed.query_located(&value);
    let matches: Vec<JsonPathMatch> = located
        .iter()
        .map(|node| JsonPathMatch {
            pointer: node.location().to_json_pointer(),
            normalized: node.location().to_string(),
            value: node.node().clone(),
        })
        .collect();

    let empty_result = matches.is_empty();
    Ok(JsonPathResult {
        matches,
        empty_result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
        "store": {
            "book": [
                {"title": "A", "price": 8.95},
                {"title": "B", "price": 12.99}
            ],
            "bicycle": {"color": "red"}
        },
        "expensive": 10
    }"#;

    #[test]
    fn valid_query_and_zero_match_are_distinguished() {
        let result = query_json_path(JSON, "$.store.book[*].title").unwrap();
        assert_eq!(result.matches.len(), 2);
        assert_eq!(result.matches[0].pointer, "/store/book/0/title");
        assert!(!result.empty_result);

        let empty = query_json_path(JSON, "$.store.book[?(@.price > 100)].title").unwrap();
        assert!(empty.matches.is_empty());
        assert!(empty.empty_result);
    }

    #[test]
    fn syntax_error_is_err() {
        let err = query_json_path(JSON, "$.store[").unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn unclosed_unicode_escape_guard() {
        // 上游 serde_json_path #124：未闭合 \u 会 panic；必须在解析前拦截。
        assert!(validate_unicode_escapes(r#"$["\u12"]"#).is_err());
        assert!(validate_unicode_escapes(r#"$["\uZZZZ"]"#).is_err());
        assert!(validate_unicode_escapes(r#"$["\u1"]"#).is_err());
        let err = query_json_path(JSON, r#"$["\u12"]"#).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        assert!(err.message.contains("\\u"));
        // 合法转义不拦截。
        assert!(validate_unicode_escapes(r#"$["A"]"#).is_ok());
    }

    #[test]
    fn all_five_rfc9535_functions() {
        // match / search：字符串正则逻辑函数。
        let m = query_json_path(JSON, "$.store.book[?match(@.title, 'A')]").unwrap();
        assert_eq!(m.matches.len(), 1);
        let s = query_json_path(JSON, "$.store.book[?search(@.title, 'B')]").unwrap();
        assert_eq!(s.matches.len(), 1);

        // length：字符串 / 数组长度。
        let l = query_json_path(JSON, "$.store.book[?length(@.title) == 1]").unwrap();
        assert_eq!(l.matches.len(), 2);

        // count：节点集合计数。
        let c = query_json_path(JSON, "$.store.book[?count(@.title) == 1]").unwrap();
        assert_eq!(c.matches.len(), 2);

        // value：ValueType 转 LogicalType 判定。
        let v = query_json_path(JSON, "$.store.book[?value(@.price) > 10]").unwrap();
        assert_eq!(v.matches.len(), 1);
    }
}

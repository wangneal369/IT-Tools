//! #11 JSON 格式化与压缩。
//!
//! `serde_json (preserve_order)` 保序输出；美化 / 压缩、2/4/Tab 缩进、
//! 可选键字母序排序（默认保序），并返回结构化行列 `SourceSpan` 错误。

use serde::Serialize as _;
use serde_json::{Map, Value};

use crate::util::{invalid_syntax, size_limit};

use devtools_core::{SourceSpan, ToolError};

/// 缩进风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonIndent {
    Two,
    Four,
    Tab,
}

impl JsonIndent {
    fn bytes(self) -> &'static [u8] {
        match self {
            JsonIndent::Two => b"  ",
            JsonIndent::Four => b"    ",
            JsonIndent::Tab => b"\t",
        }
    }
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFormatterRequest {
    pub input: String,
    /// `true` 美化，`false` 压缩到单行。
    pub pretty: bool,
    pub indent: JsonIndent,
    /// 递归按键字母序排序；默认保留输入顺序。
    pub sort_keys: bool,
}

/// 结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonFormatterResponse {
    pub output: String,
    /// 输出是否为压缩结果。
    pub minified: bool,
}

/// 执行 JSON 格式化 / 压缩。
pub fn run(req: &JsonFormatterRequest) -> Result<JsonFormatterResponse, ToolError> {
    const MAX_INPUT: usize = 8 * 1024 * 1024;
    if req.input.len() > MAX_INPUT {
        return Err(size_limit("输入超过 8 MB 上限"));
    }
    let mut value: Value = serde_json::from_str(&req.input).map_err(|err| {
        let span = SourceSpan::new(
            err.line().max(1),
            err.column().max(1),
            err.line().max(1),
            err.column().max(1),
        );
        invalid_syntax(format!("JSON 语法错误：{err}")).with_span(span)
    })?;
    if req.sort_keys {
        value = sort_keys(value);
    }
    let output = if req.pretty {
        to_pretty(&value, req.indent)?
    } else {
        serde_json::to_string(&value)
            .map_err(|err| invalid_syntax(format!("JSON 序列化失败：{err}")))?
    };
    Ok(JsonFormatterResponse {
        output,
        minified: !req.pretty,
    })
}

fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            let mut sorted = Map::new();
            for key in keys {
                let child = map.get(&key).cloned().unwrap_or(Value::Null);
                sorted.insert(key, sort_keys(child));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

fn to_pretty(value: &Value, indent: JsonIndent) -> Result<String, ToolError> {
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.bytes());
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|err| invalid_syntax(format!("JSON 序列化失败：{err}")))?;
    String::from_utf8(out).map_err(|_| invalid_syntax("JSON 序列化产生了非法 UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str) -> JsonFormatterRequest {
        JsonFormatterRequest {
            input: input.to_string(),
            pretty: true,
            indent: JsonIndent::Two,
            sort_keys: false,
        }
    }

    #[test]
    fn key_order_is_preserved_by_default() {
        let resp = run(&request(r#"{"z":1,"a":2,"m":3}"#)).unwrap();
        let z = resp.output.find("\"z\"").unwrap();
        let a = resp.output.find("\"a\"").unwrap();
        let m = resp.output.find("\"m\"").unwrap();
        assert!(z < a && a < m, "{}", resp.output);
    }

    #[test]
    fn sort_keys_is_alphabetical_and_recursive() {
        let mut req = request(r#"{"z":{"b":1,"a":2},"a":3}"#);
        req.sort_keys = true;
        let resp = run(&req).unwrap();
        assert!(resp.output.find("\"a\": 3").unwrap() < resp.output.find("\"z\"").unwrap());
        assert!(resp.output.find("\"a\": 2").unwrap() < resp.output.find("\"b\": 1").unwrap());
    }

    #[test]
    fn indentation_styles() {
        let mut req = request(r#"{"a":{"b":1}}"#);
        req.indent = JsonIndent::Four;
        assert!(run(&req).unwrap().output.contains("\n        \"b\""));
        req.indent = JsonIndent::Tab;
        assert!(run(&req).unwrap().output.contains("\n\t\t\"b\""));
        req.indent = JsonIndent::Two;
        assert!(run(&req).unwrap().output.contains("\n    \"b\""));
    }

    #[test]
    fn minify_produces_single_line() {
        let mut req = request("{\n  \"a\": [1, 2],\n  \"b\": true\n}");
        req.pretty = false;
        let resp = run(&req).unwrap();
        assert_eq!(resp.output, r#"{"a":[1,2],"b":true}"#);
        assert!(resp.minified);
    }

    #[test]
    fn syntax_error_has_span() {
        let err = run(&request("{\n  \"a\": 1,\n}")).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        assert_eq!(err.span.unwrap().line, 3);
    }

    #[test]
    fn large_input_is_rejected() {
        let mut req = request("{}");
        req.input = " ".repeat(8 * 1024 * 1024 + 1);
        let err = run(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
    }
}

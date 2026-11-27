//! #1 数据格式互转：JSON / YAML / TOML / XML。
//!
//! 统一以 [`serde_json::Value`] 为中间模型：
//! - YAML 多文档合并为数组（单文档保持原结构）；
//! - XML 采用 `@attr`（属性）与 `#text`（文本）约定；
//! - TOML 输出严格拒绝 `null` 与顶层非表结构，并给出可操作提示。

use std::fmt::Write as _;

use serde::Serialize as _;
use serde_json::{Map, Value};
use toml::Value as TomlValue;

use crate::util::{invalid_syntax, size_limit, unsupported};

use devtools_core::{SourceSpan, ToolError};

/// 输入/输出数据格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFormat {
    Json,
    Yaml,
    Toml,
    Xml,
}

impl DataFormat {
    pub const fn label(self) -> &'static str {
        match self {
            DataFormat::Json => "JSON",
            DataFormat::Yaml => "YAML",
            DataFormat::Toml => "TOML",
            DataFormat::Xml => "XML",
        }
    }
}

/// 输出缩进风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Two,
    Four,
}

impl IndentStyle {
    const fn spaces(self) -> usize {
        match self {
            IndentStyle::Two => 2,
            IndentStyle::Four => 4,
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            IndentStyle::Two => b"  ",
            IndentStyle::Four => b"    ",
        }
    }
}

/// 单次转换请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFormatConverterRequest {
    pub input: String,
    pub from: DataFormat,
    pub to: DataFormat,
    /// 输出缩进（JSON / YAML / XML）。
    pub indent: IndentStyle,
    /// 递归按字母序排列对象键。
    pub sort_keys: bool,
}

/// 转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFormatConverterResponse {
    pub output: String,
}

/// 转换输入格式到输出格式。
pub fn convert(req: &DataFormatConverterRequest) -> Result<DataFormatConverterResponse, ToolError> {
    const MAX_INPUT: usize = 8 * 1024 * 1024;
    if req.input.len() > MAX_INPUT {
        return Err(size_limit("输入超过 8 MB 上限"));
    }
    let mut value = parse(req.from, &req.input)?;
    if req.sort_keys {
        value = sort_keys(value);
    }
    let output = serialize(req.to, &value, req.indent)?;
    Ok(DataFormatConverterResponse { output })
}

fn parse(format: DataFormat, input: &str) -> Result<Value, ToolError> {
    match format {
        DataFormat::Json => parse_json(input),
        DataFormat::Yaml => parse_yaml(input),
        DataFormat::Toml => parse_toml(input),
        DataFormat::Xml => parse_xml(input),
    }
}

fn parse_json(input: &str) -> Result<Value, ToolError> {
    serde_json::from_str(input).map_err(|err| {
        let span = SourceSpan::new(
            err.line().max(1),
            err.column().max(1),
            err.line().max(1),
            err.column().max(1),
        );
        invalid_syntax(format!("JSON 语法错误：{err}")).with_span(span)
    })
}

fn parse_yaml(input: &str) -> Result<Value, ToolError> {
    let mut documents: Vec<Value> = serde_saphyr::from_str_multiple(input)
        .map_err(|err| invalid_syntax(format!("YAML 语法错误：{err}")))?;
    if documents.len() > 1 {
        return Ok(Value::Array(documents));
    }
    // 单文档（含空文档）直接使用；`from_str_multiple` 会跳过 null 文档。
    Ok(documents.pop().unwrap_or(Value::Null))
}

fn parse_toml(input: &str) -> Result<Value, ToolError> {
    let parsed: TomlValue =
        toml::from_str(input).map_err(|err| invalid_syntax(format!("TOML 语法错误：{err}")))?;
    toml_to_json(parsed)
}

fn parse_xml(input: &str) -> Result<Value, ToolError> {
    let doc = roxmltree::Document::parse(input).map_err(|err| {
        let pos = err.pos();
        let span = SourceSpan::new(
            (pos.row as usize).max(1),
            (pos.col as usize).max(1),
            (pos.row as usize).max(1),
            (pos.col as usize).max(1),
        );
        invalid_syntax(format!("XML 语法错误：{err}")).with_span(span)
    })?;
    let root = doc.root_element();
    let mut map = Map::new();
    map.insert(root.tag_name().name().to_string(), element_to_json(root)?);
    Ok(Value::Object(map))
}

fn element_to_json(node: roxmltree::Node<'_, '_>) -> Result<Value, ToolError> {
    let attributes: Vec<(String, Value)> = node
        .attributes()
        .map(|attr| {
            (
                format!("@{}", attr.name()),
                Value::String(attr.value().to_string()),
            )
        })
        .collect();
    let children: Vec<roxmltree::Node<'_, '_>> =
        node.children().filter(|n| n.is_element()).collect();
    let text = node
        .children()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect::<String>();

    if attributes.is_empty() && children.is_empty() {
        return Ok(Value::String(text.trim().to_string()));
    }

    let mut map = Map::new();
    for (key, value) in attributes {
        map.insert(key, value);
    }
    if !text.trim().is_empty() {
        map.insert("#text".to_string(), Value::String(text.trim().to_string()));
    }
    for child in children {
        let name = child.tag_name().name().to_string();
        let value = element_to_json(child)?;
        match map.get_mut(&name) {
            Some(Value::Array(items)) => items.push(value),
            Some(existing) => {
                let previous = existing.take();
                *existing = Value::Array(vec![previous, value]);
            }
            None => {
                map.insert(name, value);
            }
        }
    }
    Ok(Value::Object(map))
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

fn serialize(format: DataFormat, value: &Value, indent: IndentStyle) -> Result<String, ToolError> {
    match format {
        DataFormat::Json => to_json_string(value, Some(indent.bytes())),
        DataFormat::Yaml => to_yaml_string(value, indent.spaces()),
        DataFormat::Toml => to_toml_string(value),
        DataFormat::Xml => to_xml_string(value, indent.spaces()),
    }
}

fn to_json_string(value: &Value, indent: Option<&'static [u8]>) -> Result<String, ToolError> {
    let mut out = Vec::new();
    match indent {
        None => serde_json::to_writer(&mut out, value),
        Some(indent) => {
            let formatter = serde_json::ser::PrettyFormatter::with_indent(indent);
            let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
            value.serialize(&mut serializer)
        }
    }
    .map_err(|err| invalid_syntax(format!("JSON 序列化失败：{err}")))?;
    String::from_utf8(out).map_err(|_| invalid_syntax("JSON 序列化产生了非法 UTF-8"))
}

fn to_yaml_string(value: &Value, indent_step: usize) -> Result<String, ToolError> {
    let mut options = serde_saphyr::SerializerOptions::default();
    options.indent_step = indent_step;
    serde_saphyr::to_string_with_options(value, options)
        .map_err(|err| invalid_syntax(format!("YAML 序列化失败：{err}")))
}

fn to_toml_string(value: &Value) -> Result<String, ToolError> {
    if let Some(path) = find_null(value, "$") {
        return Err(unsupported(format!("TOML 不支持 null：{path} 的值为 null"))
            .with_hint("请删除该键、改为空字符串，或先转换为 JSON/YAML"));
    }
    if !value.is_object() {
        return Err(unsupported("TOML 顶层必须是表（对象）")
            .with_hint("TOML 不支持顶层数组或标量；请用 JSON/YAML，或把数据包在一个键下面"));
    }
    let toml_value = json_to_toml(value.clone())?;
    toml::to_string_pretty(&toml_value)
        .map_err(|err| invalid_syntax(format!("TOML 序列化失败：{err}")))
}

fn find_null(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::Null => Some(path.to_string()),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| find_null(item, &format!("{path}[{index}]"))),
        Value::Object(map) => map
            .iter()
            .find_map(|(key, item)| find_null(item, &format!("{path}.{key}"))),
        _ => None,
    }
}

fn json_to_toml(value: Value) -> Result<TomlValue, ToolError> {
    Ok(match value {
        Value::Null => {
            return Err(unsupported("TOML 不支持 null").with_hint("请删除该键或改为空字符串"));
        }
        Value::Bool(flag) => TomlValue::Boolean(flag),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                TomlValue::Integer(int)
            } else if let Some(float) = number.as_f64() {
                if !float.is_finite() {
                    return Err(unsupported("TOML 无法精确表示非有限浮点数")
                        .with_hint("请把 NaN/Infinity 改为字符串表示"));
                }
                TomlValue::Float(float)
            } else {
                return Err(unsupported(format!("无法转换为 TOML 数字：{number}")));
            }
        }
        Value::String(text) => TomlValue::String(text),
        Value::Array(items) => TomlValue::Array(
            items
                .into_iter()
                .map(json_to_toml)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(map) => {
            let mut table = toml::map::Map::new();
            for (key, item) in map {
                table.insert(key, json_to_toml(item)?);
            }
            TomlValue::Table(table)
        }
    })
}

fn toml_to_json(value: TomlValue) -> Result<Value, ToolError> {
    Ok(match value {
        TomlValue::String(text) => Value::String(text),
        TomlValue::Integer(int) => Value::Number(int.into()),
        TomlValue::Float(float) => {
            if let Some(number) = serde_json::Number::from_f64(float) {
                Value::Number(number)
            } else {
                return Err(unsupported("TOML 浮点数 inf/nan 无法表示为 JSON 数字")
                    .with_hint("请将该值改为字符串后再转换"));
            }
        }
        TomlValue::Boolean(flag) => Value::Bool(flag),
        TomlValue::Datetime(datetime) => Value::String(datetime.to_string()),
        TomlValue::Array(items) => Value::Array(
            items
                .into_iter()
                .map(toml_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        TomlValue::Table(table) => {
            let mut map = Map::new();
            for (key, item) in table {
                map.insert(key, toml_to_json(item)?);
            }
            Value::Object(map)
        }
    })
}

fn to_xml_string(value: &Value, indent_width: usize) -> Result<String, ToolError> {
    let root = value.as_object().ok_or_else(|| {
        unsupported("XML 输出要求顶层为对象").with_hint("请用 { \"root\": ... } 包裹数据")
    })?;
    if root.len() != 1 {
        return Err(unsupported("XML 输出要求顶层对象恰好包含一个根元素")
            .with_hint("请确保 JSON 顶层只有一个键作为根元素名"));
    }
    let (name, content) = root.iter().next().expect("length checked above");
    validate_xml_name(name)?;
    let indent = " ".repeat(indent_width);
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_element(&mut out, name, content, 0, &indent)?;
    Ok(out)
}

fn validate_xml_name(name: &str) -> Result<(), ToolError> {
    let valid = !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.')
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    if valid {
        Ok(())
    } else {
        Err(unsupported(format!("`{name}` 不是合法的 XML 元素名")))
    }
}

fn write_element(
    out: &mut String,
    name: &str,
    content: &Value,
    depth: usize,
    indent: &str,
) -> Result<(), ToolError> {
    match content {
        Value::Array(items) => {
            for item in items {
                write_element(out, name, item, depth, indent)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            let mut attributes = Vec::new();
            let mut text: Option<&str> = None;
            let mut children = Vec::new();
            for (key, value) in map {
                if let Some(attr) = key.strip_prefix('@') {
                    attributes.push((attr, value));
                } else if key == "#text" {
                    text = Some(match value {
                        Value::String(s) => s.as_str(),
                        other => {
                            return Err(unsupported(format!(
                                "`#text` 必须是字符串，实际为 {other}"
                            )));
                        }
                    });
                } else {
                    children.push((key.as_str(), value));
                }
            }
            write_indent(out, depth, indent);
            let _ = write!(out, "<{name}");
            for (attr, value) in attributes {
                validate_xml_name(attr)?;
                let raw = value_as_text(value)
                    .ok_or_else(|| unsupported(format!("属性 `@{attr}` 的值必须是标量")))?;
                let _ = write!(out, " {attr}=\"{}\"", escape_xml(&raw));
            }
            if children.is_empty() && text.is_none() {
                out.push_str(" />\n");
                return Ok(());
            }
            out.push('>');
            if let Some(text) = text {
                out.push_str(&escape_xml(text));
                if children.is_empty() {
                    let _ = writeln!(out, "</{name}>");
                    return Ok(());
                }
            }
            out.push('\n');
            for (child_name, child) in children {
                validate_xml_name(child_name)?;
                write_element(out, child_name, child, depth + 1, indent)?;
            }
            write_indent(out, depth, indent);
            let _ = writeln!(out, "</{name}>");
            Ok(())
        }
        other => {
            write_indent(out, depth, indent);
            let raw = value_as_text(other).unwrap_or_default();
            let _ = writeln!(out, "<{name}>{}</{name}>", escape_xml(&raw));
            Ok(())
        }
    }
}

fn value_as_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => Some(String::new()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) => Some(text.clone()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn write_indent(out: &mut String, depth: usize, indent: &str) {
    for _ in 0..depth {
        out.push_str(indent);
    }
}

fn escape_xml(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn convert_str(from: DataFormat, to: DataFormat, input: &str) -> Result<String, ToolError> {
        convert(&DataFormatConverterRequest {
            input: input.to_string(),
            from,
            to,
            indent: IndentStyle::Two,
            sort_keys: false,
        })
        .map(|resp| resp.output)
    }

    #[test]
    fn json_to_yaml_preserves_structure_and_indent() {
        let output = convert_str(
            DataFormat::Json,
            DataFormat::Yaml,
            r#"{"name":"devtools","nested":{"port":8080},"tags":["a","b"]}"#,
        )
        .unwrap();
        assert!(output.contains("name: devtools"), "{output}");
        assert!(output.contains("port: 8080"), "{output}");
        assert!(output.contains("- a"), "{output}");
    }

    #[test]
    fn yaml_multiple_documents_merge_into_array() {
        let yaml = "---\na: 1\n---\nb: 2\n";
        let output = convert_str(DataFormat::Yaml, DataFormat::Json, yaml).unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let array = value.as_array().expect("multi-doc merges to array");
        assert_eq!(array.len(), 2);
        assert_eq!(array[0]["a"], 1);
        assert_eq!(array[1]["b"], 2);
    }

    #[test]
    fn yaml_single_document_stays_object() {
        let output = convert_str(DataFormat::Yaml, DataFormat::Json, "a: 1\nb: true\n").unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert!(value.is_object());
        assert_eq!(value["b"], true);
    }

    #[test]
    fn toml_rejects_null_with_path_and_hint() {
        let err =
            convert_str(DataFormat::Json, DataFormat::Toml, r#"{"a":{"b":null}}"#).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        assert!(err.message.contains("null"), "{}", err.message);
        assert!(err.message.contains("$.a.b"), "{}", err.message);
        assert!(err.hint.is_some());
    }

    #[test]
    fn toml_rejects_top_level_array() {
        let err = convert_str(DataFormat::Json, DataFormat::Toml, "[1,2,3]").unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        assert!(err.message.contains("顶层"), "{}", err.message);
        assert!(err.hint.is_some());
    }

    #[test]
    fn toml_round_trip_preserves_values() {
        let toml_input = "title = \"demo\"\n[server]\nport = 8080\n";
        let json = convert_str(DataFormat::Toml, DataFormat::Json, toml_input).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["server"]["port"], 8080);
        let back = convert_str(DataFormat::Json, DataFormat::Toml, &json).unwrap();
        assert!(back.contains("title = \"demo\""), "{back}");
        assert!(back.contains("port = 8080"), "{back}");
    }

    #[test]
    fn xml_attributes_and_text_use_sigils() {
        let xml = r#"<book id="7"><title>Rust</title><tag>a</tag><tag>b</tag><empty/></book>"#;
        let json = convert_str(DataFormat::Xml, DataFormat::Json, xml).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["book"]["@id"], "7");
        assert_eq!(value["book"]["title"], "Rust");
        assert_eq!(value["book"]["tag"][1], "b");
        assert_eq!(value["book"]["empty"], "");
    }

    #[test]
    fn xml_text_node_and_round_trip() {
        let input = r##"{"p":{"@class":"lead","#text":"hello & <world>"}}"##;
        let xml = convert_str(DataFormat::Json, DataFormat::Xml, input).unwrap();
        assert!(xml.contains("class=\"lead\""), "{xml}");
        assert!(xml.contains("hello &amp; &lt;world&gt;"), "{xml}");
        let back = convert_str(DataFormat::Xml, DataFormat::Json, &xml).unwrap();
        let value: Value = serde_json::from_str(&back).unwrap();
        assert_eq!(value["p"]["#text"], "hello & <world>");
    }

    #[test]
    fn xml_requires_single_root() {
        let err = convert_str(DataFormat::Json, DataFormat::Xml, r#"{"a":1,"b":2}"#).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn sort_keys_is_recursive() {
        let output = convert(&DataFormatConverterRequest {
            input: r#"{"z":1,"a":{"y":2,"b":3}}"#.to_string(),
            from: DataFormat::Json,
            to: DataFormat::Json,
            indent: IndentStyle::Two,
            sort_keys: true,
        })
        .unwrap()
        .output;
        let z = output.find("\"z\"").unwrap();
        let a = output.find("\"a\"").unwrap();
        assert!(a < z, "top-level keys sorted: {output}");
        let b = output.find("\"b\"").unwrap();
        let y = output.find("\"y\"").unwrap();
        assert!(b < y, "nested keys sorted: {output}");
    }

    #[test]
    fn json_syntax_error_carries_span() {
        let err = convert_str(DataFormat::Json, DataFormat::Yaml, "{\n  \"a\": 1,\n}").unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        let span = err.span.expect("span");
        assert_eq!(span.line, 3);
    }

    #[test]
    fn four_space_indent_is_honoured() {
        let output = convert(&DataFormatConverterRequest {
            input: r#"{"a":{"b":1}}"#.to_string(),
            from: DataFormat::Json,
            to: DataFormat::Json,
            indent: IndentStyle::Four,
            sort_keys: false,
        })
        .unwrap()
        .output;
        assert!(output.contains("\n    \"a\""), "{output}");
        assert!(output.contains("\n        \"b\""), "{output}");
    }
}

//! #5 JSON 转表格 / CSV。
//!
//! 容错接受对象数组、单个对象与标量数组；对象按键点号拍平（分隔符可配），
//! 数组/嵌套对象序列化为紧凑 JSON 字符串。预览严格截断到 50,000 单元格，
//! 导出始终为全量 CSV（可选 UTF-8 BOM）/ TSV / Markdown。

use indexmap::IndexMap;
use serde_json::Value;

use crate::util::{invalid_syntax, size_limit};

use devtools_core::{SourceSpan, ToolError};

/// 预览单元格上限。
pub const PREVIEW_CELL_LIMIT: usize = 50_000;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableExportFormat {
    Csv,
    Tsv,
    Markdown,
}

/// 单次转换请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonToTableRequest {
    pub input: String,
    /// 是否按键点号拍平嵌套对象。
    pub flatten: bool,
    /// 拍平分隔符。
    pub separator: String,
    pub export_format: TableExportFormat,
    /// 仅 CSV 生效：写入 UTF-8 BOM。
    pub bom: bool,
}

/// 表格预览。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub truncated: bool,
}

/// 转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonToTableResponse {
    pub preview: DataTable,
    /// 全量列名。
    pub columns: Vec<String>,
    /// 全量行数。
    pub total_rows: usize,
    /// 全量单元格数。
    pub total_cells: usize,
    /// 预览是否被截断。
    pub preview_truncated: bool,
    /// 全量导出内容。
    pub export: String,
}

/// 执行 JSON → 表格转换。
pub fn convert(req: &JsonToTableRequest) -> Result<JsonToTableResponse, ToolError> {
    const MAX_INPUT: usize = 8 * 1024 * 1024;
    if req.input.len() > MAX_INPUT {
        return Err(size_limit("输入超过 8 MB 上限"));
    }
    let value: Value = serde_json::from_str(&req.input).map_err(|err| {
        let span = SourceSpan::new(
            err.line().max(1),
            err.column().max(1),
            err.line().max(1),
            err.column().max(1),
        );
        invalid_syntax(format!("JSON 语法错误：{err}")).with_span(span)
    })?;

    let records = extract_records(&value, req.flatten, &req.separator);
    let columns = collect_columns(&records);
    let total_rows = records.len();
    let total_cells = total_rows.saturating_mul(columns.len());

    let preview_limit = if columns.is_empty() {
        0
    } else {
        (PREVIEW_CELL_LIMIT / columns.len()).max(1)
    };
    let preview_rows: Vec<Vec<String>> = records
        .iter()
        .take(preview_limit)
        .map(|record| {
            columns
                .iter()
                .map(|column| record.get(column).cloned().unwrap_or_default())
                .collect()
        })
        .collect();
    let preview_truncated = total_rows > preview_rows.len();

    let export = export(
        &columns,
        &records,
        req.export_format,
        req.bom && req.export_format == TableExportFormat::Csv,
    )?;

    Ok(JsonToTableResponse {
        preview: DataTable {
            columns: columns.clone(),
            rows: preview_rows,
            truncated: preview_truncated,
        },
        columns,
        total_rows,
        total_cells,
        preview_truncated,
        export,
    })
}

fn extract_records(value: &Value, flatten: bool, separator: &str) -> Vec<IndexMap<String, String>> {
    match value {
        Value::Array(items) => items
            .iter()
            .map(|item| record_from_value(item, flatten, separator))
            .collect(),
        other => vec![record_from_value(other, flatten, separator)],
    }
}

fn record_from_value(value: &Value, flatten: bool, separator: &str) -> IndexMap<String, String> {
    let mut record = IndexMap::new();
    match value {
        Value::Object(map) if flatten => {
            for (key, child) in map {
                flatten_into(child, key, separator, &mut record);
            }
        }
        Value::Object(_) => {
            record.insert("value".to_string(), compact_json(value));
        }
        other => {
            record.insert("value".to_string(), scalar_to_string(other));
        }
    }
    record
}

fn flatten_into(value: &Value, path: &str, separator: &str, out: &mut IndexMap<String, String>) {
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                out.insert(path.to_string(), "{}".to_string());
            } else {
                for (key, child) in map {
                    let child_path = format!("{path}{separator}{key}");
                    flatten_into(child, &child_path, separator, out);
                }
            }
        }
        Value::Array(_) => {
            out.insert(path.to_string(), compact_json(value));
        }
        other => {
            out.insert(path.to_string(), scalar_to_string(other));
        }
    }
}

fn scalar_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn collect_columns(records: &[IndexMap<String, String>]) -> Vec<String> {
    let mut columns: IndexMap<&str, ()> = IndexMap::new();
    for record in records {
        for key in record.keys() {
            columns.insert(key.as_str(), ());
        }
    }
    columns.into_keys().map(str::to_string).collect()
}

fn export(
    columns: &[String],
    records: &[IndexMap<String, String>],
    format: TableExportFormat,
    bom: bool,
) -> Result<String, ToolError> {
    match format {
        TableExportFormat::Csv => export_delimited(columns, records, b',', bom),
        TableExportFormat::Tsv => export_delimited(columns, records, b'\t', false),
        TableExportFormat::Markdown => Ok(export_markdown(columns, records)),
    }
}

fn export_delimited(
    columns: &[String],
    records: &[IndexMap<String, String>],
    delimiter: u8,
    bom: bool,
) -> Result<String, ToolError> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    writer
        .write_record(columns)
        .map_err(|err| invalid_syntax(format!("CSV 写入失败：{err}")))?;
    for record in records {
        writer
            .write_record(
                columns
                    .iter()
                    .map(|column| record.get(column).map(String::as_str).unwrap_or("")),
            )
            .map_err(|err| invalid_syntax(format!("CSV 写入失败：{err}")))?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|err| invalid_syntax(format!("CSV 写入失败：{err}")))?;
    let mut text =
        String::from_utf8(bytes).map_err(|_| invalid_syntax("CSV 导出产生了非法 UTF-8"))?;
    if bom {
        text.insert(0, '\u{FEFF}');
    }
    Ok(text)
}

fn export_markdown(columns: &[String], records: &[IndexMap<String, String>]) -> String {
    let mut out = String::new();
    out.push('|');
    for column in columns {
        out.push(' ');
        out.push_str(&escape_markdown(column));
        out.push_str(" |");
    }
    out.push('\n');
    out.push('|');
    for _ in columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for record in records {
        out.push('|');
        for column in columns {
            out.push(' ');
            out.push_str(&escape_markdown(
                record.get(column).map(String::as_str).unwrap_or(""),
            ));
            out.push_str(" |");
        }
        out.push('\n');
    }
    out
}

fn escape_markdown(cell: &str) -> String {
    cell.replace('|', "\\|")
        .replace('\n', "<br>")
        .replace('\r', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str, format: TableExportFormat) -> JsonToTableRequest {
        JsonToTableRequest {
            input: input.to_string(),
            flatten: true,
            separator: ".".to_string(),
            export_format: format,
            bom: false,
        }
    }

    #[test]
    fn array_of_objects_unions_columns_in_first_seen_order() {
        let resp = convert(&request(
            r#"[{"a":1,"b":2},{"b":3,"c":4}]"#,
            TableExportFormat::Csv,
        ))
        .unwrap();
        assert_eq!(resp.columns, ["a", "b", "c"]);
        assert_eq!(resp.total_rows, 2);
        assert_eq!(resp.preview.rows[0], ["1", "2", ""]);
        assert_eq!(resp.preview.rows[1], ["", "3", "4"]);
    }

    #[test]
    fn nested_objects_are_flattened_with_separator() {
        let resp = convert(&request(
            r#"[{"user":{"name":"neal","tags":["a","b"]}}]"#,
            TableExportFormat::Csv,
        ))
        .unwrap();
        assert_eq!(resp.columns, ["user.name", "user.tags"]);
        assert_eq!(resp.preview.rows[0][0], "neal");
        assert_eq!(resp.preview.rows[0][1], r#"["a","b"]"#);
    }

    #[test]
    fn flatten_can_be_disabled() {
        let mut req = request(r#"[{"user":{"name":"neal"}}]"#, TableExportFormat::Csv);
        req.flatten = false;
        let resp = convert(&req).unwrap();
        assert_eq!(resp.columns, ["value"]);
        assert_eq!(resp.preview.rows[0][0], r#"{"user":{"name":"neal"}}"#);
    }

    #[test]
    fn custom_separator_is_honoured() {
        let mut req = request(r#"[{"a":{"b":1}}]"#, TableExportFormat::Csv);
        req.separator = "__".to_string();
        let resp = convert(&req).unwrap();
        assert_eq!(resp.columns, ["a__b"]);
    }

    #[test]
    fn scalar_array_uses_single_value_column() {
        let resp = convert(&request("[1,2,3]", TableExportFormat::Csv)).unwrap();
        assert_eq!(resp.columns, ["value"]);
        assert_eq!(resp.total_rows, 3);
        assert_eq!(resp.preview.rows[2][0], "3");
    }

    #[test]
    fn single_object_becomes_one_row() {
        let resp = convert(&request(r#"{"id":7,"ok":true}"#, TableExportFormat::Csv)).unwrap();
        assert_eq!(resp.total_rows, 1);
        assert_eq!(resp.preview.rows[0], ["7", "true"]);
    }

    #[test]
    fn preview_is_capped_but_export_is_complete() {
        let mut items = String::from("[");
        for row in 0..6_000 {
            if row > 0 {
                items.push(',');
            }
            items.push('{');
            for col in 0..10 {
                if col > 0 {
                    items.push(',');
                }
                items.push_str(&format!("\"c{col}\":{row}"));
            }
            items.push('}');
        }
        items.push(']');
        let resp = convert(&request(&items, TableExportFormat::Csv)).unwrap();
        assert_eq!(resp.total_cells, 60_000);
        assert!(resp.preview_truncated);
        assert_eq!(resp.preview.rows.len(), 5_000);
        assert!(resp.preview.truncated);
        assert_eq!(resp.export.lines().count(), 6_001, "header + all rows");
    }

    #[test]
    fn csv_quotes_and_escapes() {
        let resp = convert(&request(
            r#"[{"a":"x,y","b":"line1\nline2"}]"#,
            TableExportFormat::Csv,
        ))
        .unwrap();
        assert!(resp.export.contains("\"x,y\""), "{}", resp.export);
    }

    #[test]
    fn bom_is_optional_and_csv_only() {
        let mut req = request(r#"[{"a":1}]"#, TableExportFormat::Csv);
        req.bom = true;
        let resp = convert(&req).unwrap();
        assert!(resp.export.starts_with('\u{FEFF}'));

        let mut req = request(r#"[{"a":1}]"#, TableExportFormat::Tsv);
        req.bom = true;
        let resp = convert(&req).unwrap();
        assert!(!resp.export.starts_with('\u{FEFF}'));
    }

    #[test]
    fn tsv_uses_tabs() {
        let resp = convert(&request(r#"[{"a":1,"b":2}]"#, TableExportFormat::Tsv)).unwrap();
        assert!(resp.export.contains("a\tb"), "{}", resp.export);
    }

    #[test]
    fn markdown_escapes_pipes_and_newlines() {
        let resp = convert(&request(
            r#"[{"a":"x|y","b":"line1\nline2"}]"#,
            TableExportFormat::Markdown,
        ))
        .unwrap();
        assert!(resp.export.contains("x\\|y"), "{}", resp.export);
        assert!(resp.export.contains("line1<br>line2"), "{}", resp.export);
        assert!(resp.export.contains("| --- |"), "{}", resp.export);
    }

    #[test]
    fn invalid_json_reports_span() {
        let err = convert(&request("[{\"a\":}", TableExportFormat::Csv)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        assert!(err.span.is_some());
    }

    #[test]
    fn empty_array_yields_empty_table() {
        let resp = convert(&request("[]", TableExportFormat::Csv)).unwrap();
        assert!(resp.columns.is_empty());
        assert_eq!(resp.total_rows, 0);
        assert!(!resp.preview_truncated);
    }
}

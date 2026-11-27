//! 分类 3：格式化（工具 #11–#13）。

pub mod json;
pub mod markup;
pub mod sql;

pub use json::{JsonFormatterRequest, JsonFormatterResponse, JsonIndent};
pub use markup::{MarkupIndent, MarkupLanguage, XmlHtmlFormatterRequest, XmlHtmlFormatterResponse};
pub use sql::{SqlFormatterRequest, SqlFormatterResponse, SqlIndent, SqlKeywordCase, SqlLayout};

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 3] = [
    ToolId::JsonFormatter,
    ToolId::SqlFormatter,
    ToolId::XmlHtmlFormatter,
];

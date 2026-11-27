//! 分类 1：转换（工具 #1–#5）。
//!
//! 全部为零 I/O 纯计算内核，输出强类型响应。

pub mod color;
pub mod data_format;
pub mod datetime;
pub mod json_table;
pub mod number_base;

pub use color::{
    ColorConverterRequest, ColorConverterResponse, ContrastReport, contrast_ratio,
    relative_luminance,
};
pub use data_format::{
    DataFormat, DataFormatConverterRequest, DataFormatConverterResponse, IndentStyle,
};
pub use datetime::{DateTimeConverterRequest, DateTimeConverterResponse, TimestampUnit};
pub use json_table::{DataTable, JsonToTableRequest, JsonToTableResponse, TableExportFormat};
pub use number_base::{NumberBaseConverterRequest, NumberBaseConverterResponse, SignedMode};

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 5] = [
    ToolId::DataFormatConverter,
    ToolId::DateTimeConverter,
    ToolId::NumberBaseConverter,
    ToolId::ColorConverter,
    ToolId::JsonToTable,
];

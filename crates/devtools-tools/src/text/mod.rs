//! 分类 5「文本对比」：#17–#20 纯计算内核。

pub mod analysis;
pub mod diff;
pub mod json_diff;
pub mod list_compare;

pub use analysis::*;
pub use diff::*;
pub use json_diff::*;
pub use list_compare::*;

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 4] = [
    ToolId::TextDiff,
    ToolId::JsonDiff,
    ToolId::TextAnalysis,
    ToolId::ListCompare,
];

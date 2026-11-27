//! 分类 6「调试开发」：#21–#24 纯计算内核。

pub mod chmod_calculator;
pub mod cron_parser;
pub mod json_path_tester;
pub mod regex_tester;

pub use chmod_calculator::*;
pub use cron_parser::*;
pub use json_path_tester::*;
pub use regex_tester::*;

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 4] = [
    ToolId::RegexTester,
    ToolId::JsonPathTester,
    ToolId::CronParser,
    ToolId::ChmodCalculator,
];

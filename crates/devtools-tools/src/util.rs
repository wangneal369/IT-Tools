//! 各工具内核共享的小型工具函数。
//!
//! 部分辅助函数只被特定分类使用，未启用对应 feature 时会暂时闲置。
#![allow(dead_code)]

use devtools_core::{CancelFlag, SourceSpan, ToolError, ToolErrorKind};

/// 协作取消检查：已请求取消时返回 `ToolError::Cancelled`。
pub(crate) fn check_cancel(flag: Option<&CancelFlag>) -> Result<(), ToolError> {
    if flag.is_some_and(CancelFlag::is_cancelled) {
        Err(ToolError::new(ToolErrorKind::Cancelled, "操作已取消"))
    } else {
        Ok(())
    }
}

pub(crate) fn invalid_syntax(message: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorKind::InvalidSyntax, message)
}

pub(crate) fn unsupported(message: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorKind::UnsupportedInput, message)
}

pub(crate) fn size_limit(message: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorKind::SizeLimitExceeded, message)
}

pub(crate) fn internal(message: impl Into<String>) -> ToolError {
    ToolError::new(ToolErrorKind::Internal, message)
}

/// 把字节偏移换算为 1-based 行列号（用于缺少结构化位置的解析器）。
pub(crate) fn byte_offset_to_span(input: &str, offset: usize) -> SourceSpan {
    let offset = offset.min(input.len());
    let mut line = 1usize;
    let mut column = 1usize;
    for ch in input[..offset].chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    SourceSpan::new(line, column, line, column)
}

/// 按行处理输入，保留空行与行尾风格（`\n` 归一化后输出 `\n`）。
pub(crate) fn process_lines<F>(input: &str, mut f: F) -> Result<String, ToolError>
where
    F: FnMut(&str, usize) -> Result<String, ToolError>,
{
    let mut out = Vec::new();
    for (index, line) in input.lines().enumerate() {
        out.push(f(line, index)?);
    }
    // `lines()` 对以换行符结尾的输入会丢弃末尾空行；显式还原结尾换行。
    let mut joined = out.join("\n");
    if input.ends_with('\n') && !input.is_empty() {
        joined.push('\n');
    }
    Ok(joined)
}

//! Structured tool errors and cooperative cancellation (ADR-0004).
//!
//! Tool kernels are pure synchronous/streaming functions with no async runtime.
//! Long-running work cooperatively checks a shared [`CancelFlag`]; failures
//! surface as a structured [`ToolError`] with an optional [`SourceSpan`].

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// A 1-based line/column span locating an error in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceSpan {
    /// 1-based start line.
    pub line: usize,
    /// 1-based start column.
    pub column: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// 1-based end column (inclusive).
    pub end_column: usize,
}

impl SourceSpan {
    pub const fn new(line: usize, column: usize, end_line: usize, end_column: usize) -> Self {
        Self {
            line,
            column,
            end_line,
            end_column,
        }
    }
}

/// The category of a tool failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolErrorKind {
    /// 输入语法错误（保留上一份有效输出并标记 `Stale`）。
    InvalidSyntax,
    /// 输入类型或操作不受支持。
    UnsupportedInput,
    /// 超过大小上限。
    SizeLimitExceeded,
    /// 用户取消或标签会话换代。
    Cancelled,
    /// 底层 I/O 错误。
    Io,
    /// 未预期的内部错误。
    Internal,
}

impl fmt::Display for ToolErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ToolErrorKind::InvalidSyntax => "invalid syntax",
            ToolErrorKind::UnsupportedInput => "unsupported input",
            ToolErrorKind::SizeLimitExceeded => "size limit exceeded",
            ToolErrorKind::Cancelled => "cancelled",
            ToolErrorKind::Io => "io error",
            ToolErrorKind::Internal => "internal error",
        })
    }
}

/// A structured tool error with an optional source location and recovery hint.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind}: {message}")]
pub struct ToolError {
    pub kind: ToolErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
    pub hint: Option<String>,
}

impl ToolError {
    pub fn new(kind: ToolErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            span: None,
            hint: None,
        }
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// A cooperative cancellation flag shared between the UI shell and a running
/// background tool kernel (ADR-0004).
///
/// Cheap to clone; all clones observe the same underlying atomic flag.
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation across all clones.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Reset the flag for reuse.
    pub fn reset(&self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_flag_clones_share_state_and_reset() {
        let flag = CancelFlag::new();
        let worker_handle = flag.clone();
        assert!(!worker_handle.is_cancelled());
        flag.cancel();
        assert!(worker_handle.is_cancelled());
        worker_handle.reset();
        assert!(!flag.is_cancelled());
    }

    #[test]
    fn tool_error_displays_kind_and_message() {
        let err = ToolError::new(ToolErrorKind::InvalidSyntax, "unexpected token")
            .with_span(SourceSpan::new(1, 5, 1, 8))
            .with_hint("check trailing comma");
        assert_eq!(err.to_string(), "invalid syntax: unexpected token");
        assert_eq!(err.span, Some(SourceSpan::new(1, 5, 1, 8)));
        assert_eq!(err.hint.as_deref(), Some("check trailing comma"));
    }
}

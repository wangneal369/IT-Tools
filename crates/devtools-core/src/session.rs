//! In-memory-only session and clipboard fingerprint types (ADR-0005).
//!
//! These types represent a tab's authoritative live state and the clipboard
//! deduplication fingerprint. They are strictly process-memory-only: the V1
//! persistence boundary forbids serializing tab sessions, tool payloads, or
//! clipboard fingerprints to disk. That invariant is enforced at compile time
//! via negative-trait assertions below.

use std::path::PathBuf;
use std::sync::Arc;

use crate::payload::RawPayload;

/// A unique, process-local identifier for a tool tab session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabSessionId(pub u64);

/// A lightweight summary of a tab session for the tab bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSessionSummary {
    pub id: TabSessionId,
    /// 工具标识。
    pub tool_id: crate::tool::ToolId,
    /// 实例序号（同工具多开）。
    pub instance: u32,
}

/// The authoritative in-memory state of one open tool tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabSessionRecord {
    pub id: TabSessionId,
    pub tool_id: crate::tool::ToolId,
    /// 当前输入载荷（共享快照，不落盘）。
    pub input: Option<RawPayload>,
    /// 单调递增的代次，配合 `CancelFlag` 防乱序回写（ADR-0004）。
    pub generation: u64,
}

/// Clipboard deduplication fingerprint (变更序号 + 内容摘要)。
///
/// Used to suppress self-copy echoes and avoid re-detecting unchanged content;
/// it never persists past process exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFingerprint {
    /// 剪贴板变更序号。
    pub sequence: u64,
    /// 内容摘要（仅驻内存）。
    pub digest: u64,
    /// 首个文件的路径摘要（若为文件载荷）。
    pub file_hint: Option<Arc<PathBuf>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::{ImagePayload, RelayPayload, ToolLaunchInput};
    use static_assertions::assert_not_impl_any;

    // Compile-time negative assertions: none of these types may ever gain a
    // serde Serialize or DeserializeOwned implementation (ADR-0005 hard redline).
    assert_not_impl_any!(TabSessionId: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(TabSessionSummary: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(TabSessionRecord: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(ClipboardFingerprint: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(ImagePayload: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(RawPayload: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(ToolLaunchInput: serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl_any!(RelayPayload: serde::Serialize, serde::de::DeserializeOwned);

    #[test]
    fn session_id_is_ordinal_and_copy() {
        let a = TabSessionId(1);
        let b = TabSessionId(2);
        assert!(a < b);
        let c = a;
        assert_eq!(c, a);
    }
}

//! In-memory payload and tool-launch contracts (ADR-0004).
//!
//! These types carry the raw clipboard/file data and the output of one tool into
//! another. They are strictly in-memory: the V1 persistence boundary forbids any
//! tool payload from ever touching disk (ADR-0005), so none of them implement
//! `Serialize`.

use std::path::PathBuf;
use std::sync::Arc;

use crate::data_type::DataType;
use crate::detect::{MatchedDataChain, ParsedHint};

/// A decoded image snapshot shared without copying (ADR-0004 `Arc` snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagePayload {
    pub width: u32,
    pub height: u32,
    /// Encoded image bytes (PNG/JPEG/…), shared by `Arc` to avoid large copies.
    pub bytes: Arc<[u8]>,
}

/// The raw clipboard/file payload, read in `Files > Image > Text` priority order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawPayload {
    Files(Vec<PathBuf>),
    Image(ImagePayload),
    Text(Arc<str>),
}

/// The strongly-typed input handed to a tool session when it opens or receives a
/// relay (ADR-0004 `accept_input`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLaunchInput {
    pub payload: RawPayload,
    pub chain: MatchedDataChain,
    pub hint: Option<ParsedHint>,
}

/// The single-hop relay output a tool advertises for "发送到 ▸" (tool relay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayPayload {
    pub data_type: DataType,
    pub payload: RawPayload,
    pub chain: MatchedDataChain,
}

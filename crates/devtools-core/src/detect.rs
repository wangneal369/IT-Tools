//! Smart-detection result contracts (ADR-0003 / ADR-0004).
//!
//! The `devtools-detect` DAG produces a [`MatchedDataChain`] from the clipboard
//! root down to the leaf data type, together with a [`ParsedHint`] that drives
//! the top-bar banner, command-palette ranking, and sidebar highlighting.

use crate::data_type::{Confidence, DataType};

/// A parsed detection conclusion: a data type and how confident we are in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHint {
    pub data_type: DataType,
    pub confidence: Confidence,
}

impl ParsedHint {
    pub const fn new(data_type: DataType, confidence: Confidence) -> Self {
        Self {
            data_type,
            confidence,
        }
    }
}

/// One node in a hierarchical detection chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedDataNode {
    pub data_type: DataType,
    pub confidence: Confidence,
}

impl MatchedDataNode {
    pub const fn new(data_type: DataType, confidence: Confidence) -> Self {
        Self {
            data_type,
            confidence,
        }
    }
}

/// The ordered chain of matched nodes, from the clipboard root to the leaf type.
///
/// Example: `Text -> Json -> JsonArray`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchedDataChain {
    pub nodes: Vec<MatchedDataNode>,
}

impl MatchedDataChain {
    pub fn new(nodes: Vec<MatchedDataNode>) -> Self {
        Self { nodes }
    }

    /// The deepest (most specific) matched node, i.e. the detected leaf type.
    pub fn leaf(&self) -> Option<&MatchedDataNode> {
        self.nodes.last()
    }

    /// The root node (clipboard root), if any.
    pub fn root(&self) -> Option<&MatchedDataNode> {
        self.nodes.first()
    }
}

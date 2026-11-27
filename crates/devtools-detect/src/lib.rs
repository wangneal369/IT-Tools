//! 纯 Rust 智能识别 DAG：Files > Image > Text，载荷与指纹仅驻内存。
//!
//! 调用方在后台执行本同步内核，并在读取正文前检查平台隐私标记。
//! 引擎不读取系统剪贴板、不创建线程、不记录历史，也不依赖工具内核。

mod engine;
mod fingerprint;
mod privacy;
mod text;

pub use devtools_core;
pub use engine::{
    DETECTORS, DetectionEngine, DetectionMode, DetectionResult, DetectionStatus, DetectorSpec,
    FULL_TEXT_LIMIT, HARD_TIMEOUT, IMAGE_LIMIT, MAGIC_PREFIX_LIMIT, Recommendation, SOFT_TARGET,
    STREAMING_FILE_LIMIT, TEXT_LIMIT,
};
pub use fingerprint::{ClipboardDeduplicator, DedupDecision};
pub use privacy::{PrivacyAssessment, assess_privacy, redacted_preview};

#[cfg(test)]
mod tests;

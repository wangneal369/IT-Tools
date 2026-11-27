//! OS integration and atomic TOML config I/O (ADR-0005).
//!
//! This crate is zero-GPUI. M1 ships the config persistence boundary: platform
//! path resolution, schema-versioned loading with corruption recovery, and
//! atomic write via same-directory temp file + rename. Tray/hotkey/single-instance
//! IPC and auto-launch land in M2.

mod paths;
mod store;

pub use paths::ConfigPaths;
pub use store::{ConfigStore, ConfigStoreError, LoadOutcome};

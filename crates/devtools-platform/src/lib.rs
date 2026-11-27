//! OS integration and atomic TOML config I/O (ADR-0002 / ADR-0005).
//!
//! This crate is strictly zero-GPUI and zero-tokio. It owns everything the UI
//! cannot own itself:
//!
//! - [`ConfigStore`] — the `config.toml` / `window-state.toml` persistence boundary.
//! - [`ipc`] — per-user single-instance local socket with newline framing.
//! - [`tray`] — system tray (Windows/macOS `tray-icon`+`muda`, Linux pure-Rust `ksni`).
//! - [`hotkey`] — global shortcut parsing, registration and live re-binding.
//! - [`autostart`] — OS login-item adapter (never persisted to `config.toml`).
//! - [`service`] — assembles all of the above and funnels events into
//!   [`bridge::PlatformCommand`]s for the GPUI workbench.

pub mod autostart;
pub mod bridge;
pub mod clipboard;
pub mod hotkey;
pub mod ipc;
mod paths;
pub mod service;
mod store;
pub mod tray;

pub use autostart::{AutostartAdapter, AutostartError};
pub use bridge::{PlatformBridge, PlatformCommand};
pub use hotkey::{HotkeyStatus, ShortcutError, parse_shortcut, platform_default_shortcut};
pub use ipc::{Instance, InstanceGuard, IpcCommand, IpcError, IpcName, IpcServer};
pub use paths::ConfigPaths;
pub use service::{PlatformService, PlatformStartup};
pub use store::{ConfigStore, ConfigStoreError, LoadOutcome};
pub use tray::{TrayError, TrayHandle};

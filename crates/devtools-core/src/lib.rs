//! DevTools domain contracts (ADR-0004).
//!
//! This crate is the zero-heavy-dependency core of the 7-crate workspace. It
//! defines the closed tool/category/data-type contracts, the persistence
//! whitelist models ([`AppSettings`] / [`WindowState`]), the structured error and
//! cancellation primitives, and the strictly in-memory session/clipboard types.
//!
//! Only `serde` and `thiserror` are pulled in; everything else is `std`.

pub mod data_type;
pub mod detect;
pub mod error;
pub mod payload;
pub mod session;
pub mod settings;
pub mod tool;

pub use data_type::{Confidence, DataType};
pub use detect::{MatchedDataChain, MatchedDataNode, ParsedHint};
pub use error::{CancelFlag, SourceSpan, ToolError, ToolErrorKind};
pub use payload::{ImagePayload, RawPayload, RelayPayload, ToolLaunchInput};
pub use session::{ClipboardFingerprint, TabSessionId, TabSessionRecord, TabSessionSummary};
pub use settings::{
    APP_SETTINGS_SCHEMA_VERSION, AppSettings, AutoJumpMode, CloseBehavior, EditorSettings,
    HotkeySettings, LayoutSettings, MacosDockPolicy, MinimizeBehavior, NavigationSettings,
    PlatformSettings, ResidencySettings, SmartDetectionSettings, TabBarMode, Theme,
    WINDOW_STATE_SCHEMA_VERSION, WakePlacement, WindowGeometry, WindowState,
};
pub use tool::{Category, ToolCapabilities, ToolDescriptor, ToolId, UiTrack};

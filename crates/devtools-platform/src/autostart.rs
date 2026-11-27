//! OS login-item adapter (ADR-0005).
//!
//! The operating system is the single source of truth for "launch at login":
//! this adapter only queries and mutates the OS registration. Nothing is ever
//! written to `config.toml`, so a change made from Task Manager / System
//! Settings / macOS Login Items is immediately reflected in the app.

use auto_launch::{AutoLaunch, AutoLaunchBuilder};

/// App name registered with the OS login items.
pub const APP_NAME: &str = "DevTools";
/// Login start is always silent: it must never flash a window (ADR-0002).
pub const AUTOSTART_ARGS: &[&str] = &["--silent"];

#[derive(Debug, thiserror::Error)]
pub enum AutostartError {
    #[error("autostart is unavailable: {0}")]
    Unavailable(String),
    #[error("autostart operation failed: {0}")]
    Operation(String),
}

/// Thin, testable wrapper around `auto-launch`.
pub struct AutostartAdapter {
    inner: Result<AutoLaunch, String>,
}

impl AutostartAdapter {
    /// Bind to the currently running executable.
    pub fn for_current_exe() -> Self {
        Self {
            inner: build_for_current_exe(),
        }
    }

    pub fn available(&self) -> bool {
        self.inner.is_ok()
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        match &self.inner {
            Ok(_) => None,
            Err(reason) => Some(reason),
        }
    }

    /// Query the OS. Never cached: the OS may change behind our back.
    pub fn is_enabled(&self) -> Result<bool, AutostartError> {
        match &self.inner {
            Ok(auto) => auto
                .is_enabled()
                .map_err(|err| AutostartError::Operation(err.to_string())),
            Err(reason) => Err(AutostartError::Unavailable(reason.clone())),
        }
    }

    /// Enable/disable the OS login item and report the observed final state so
    /// the UI never shows an optimistic checkbox.
    pub fn set_enabled(&self, enabled: bool) -> Result<bool, AutostartError> {
        let auto = match &self.inner {
            Ok(auto) => auto,
            Err(reason) => return Err(AutostartError::Unavailable(reason.clone())),
        };
        let result = if enabled {
            auto.enable()
        } else {
            auto.disable()
        };
        result.map_err(|err| AutostartError::Operation(err.to_string()))?;
        auto.is_enabled()
            .map_err(|err| AutostartError::Operation(err.to_string()))
    }
}

fn build_for_current_exe() -> Result<AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|err| format!("current_exe failed: {err}"))?;
    let exe = exe.to_string_lossy().into_owned();

    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(APP_NAME)
        .set_app_path(&exe)
        .set_args(AUTOSTART_ARGS);

    #[cfg(target_os = "windows")]
    builder.set_windows_enable_mode(auto_launch::WindowsEnableMode::CurrentUser);
    #[cfg(target_os = "linux")]
    builder.set_linux_launch_mode(auto_launch::LinuxLaunchMode::XdgAutostart);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);

    builder
        .build()
        .map_err(|err| format!("auto-launch build failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binds_to_the_test_executable_without_touching_the_os() {
        let adapter = AutostartAdapter::for_current_exe();
        assert!(adapter.available(), "{:?}", adapter.unavailable_reason());
        assert!(adapter.unavailable_reason().is_none());
    }

    #[test]
    fn silent_flag_is_the_only_login_argument() {
        assert_eq!(AUTOSTART_ARGS, &["--silent"]);
    }
}

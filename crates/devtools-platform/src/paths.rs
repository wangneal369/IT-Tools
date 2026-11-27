//! Platform-standard config directory resolution (ADR-0005).

use std::path::{Path, PathBuf};

/// Resolved on-disk locations for DevTools config state.
///
/// Production uses `directories::ProjectDirs` under the reverse-DNS identifier
/// `io.github.wangneal369.devtools`; tests use [`ConfigPaths::from_dir`] for a
/// deterministic sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPaths {
    config_dir: PathBuf,
}

impl ConfigPaths {
    /// The platform-standard config directory, or `None` when no home directory
    /// could be resolved (rare; e.g. a misconfigured `$HOME` on Linux).
    pub fn standard() -> Option<Self> {
        directories::ProjectDirs::from("io.github", "wangneal369", "devtools").map(|dirs| Self {
            config_dir: dirs.config_dir().to_path_buf(),
        })
    }

    /// Resolve against an explicit directory (deterministic tests).
    pub fn from_dir(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    /// The directory that holds `config.toml` and `window-state.toml`.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Path to `config.toml` (user preferences).
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Path to `window-state.toml` (window geometry + tab skeleton).
    pub fn window_state_file(&self) -> PathBuf {
        self.config_dir.join("window-state.toml")
    }
}

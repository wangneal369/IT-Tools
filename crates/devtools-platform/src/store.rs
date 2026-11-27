//! Atomic TOML config I/O with corruption recovery (ADR-0005).
//!
//! `config.toml` and `window-state.toml` are written atomically: serialize to a
//! unique same-directory temp file, `sync_all()`, then atomically replace the
//! destination (`std::fs::rename` uses `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING)`
//! on Windows in Rust 1.74+). On load, a malformed or semantically invalid file
//! is renamed to `*.invalid-<timestamp>` without clobbering existing backups and
//! the caller falls back to defaults; a file with a newer `schema_version` is
//! left byte-for-byte untouched (`UnsupportedFutureVersion`).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use devtools_core::{
    APP_SETTINGS_SCHEMA_VERSION, AppSettings, WINDOW_STATE_SCHEMA_VERSION, WindowState,
};

use crate::paths::ConfigPaths;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
static BACKUP_SEQ: AtomicU64 = AtomicU64::new(0);

trait StoredConfig: Serialize + DeserializeOwned + Default {
    const SCHEMA_VERSION: u32;
    const FILE_NAME: &'static str;

    fn validate(&self) -> Result<(), String>;
}

impl StoredConfig for AppSettings {
    const SCHEMA_VERSION: u32 = APP_SETTINGS_SCHEMA_VERSION;
    const FILE_NAME: &'static str = "config.toml";

    fn validate(&self) -> Result<(), String> {
        AppSettings::validate(self)
    }
}

impl StoredConfig for WindowState {
    const SCHEMA_VERSION: u32 = WINDOW_STATE_SCHEMA_VERSION;
    const FILE_NAME: &'static str = "window-state.toml";

    fn validate(&self) -> Result<(), String> {
        WindowState::validate(self)
    }
}

/// The observable result of loading a config document at the persistence boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadOutcome<T> {
    /// Loaded and validated successfully.
    Loaded(T),
    /// The file does not exist yet; factory defaults are returned without writing.
    NotFoundDefault(T),
    /// The file was corrupt or invalid; it was backed up to `backup_path` and
    /// `value` carries factory defaults for cold start.
    RecoveredFromInvalid {
        value: T,
        backup_path: PathBuf,
        reason: String,
    },
    /// The file was written by a newer app version; it is left untouched.
    UnsupportedFutureVersion {
        found: u32,
        supported: u32,
        path: PathBuf,
    },
}

impl<T> LoadOutcome<T> {
    /// Extract the usable value for `Loaded`, `NotFoundDefault`, or `RecoveredFromInvalid`.
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Loaded(v) | Self::NotFoundDefault(v) => Some(v),
            Self::RecoveredFromInvalid { value, .. } => Some(value),
            Self::UnsupportedFutureVersion { .. } => None,
        }
    }
}

/// Errors raised while loading (backup/IO failure) or saving a config document.
#[derive(Debug, thiserror::Error)]
pub enum ConfigStoreError {
    #[error("failed to serialize config: {0}")]
    Serialize(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Narrow persistence boundary exposing only the two V1 whitelisted files.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    paths: ConfigPaths,
}

impl ConfigStore {
    pub fn new(paths: ConfigPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &ConfigPaths {
        &self.paths
    }

    pub fn load_settings(&self) -> Result<LoadOutcome<AppSettings>, ConfigStoreError> {
        self.load::<AppSettings>()
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), ConfigStoreError> {
        self.save(settings)
    }

    pub fn load_window_state(&self) -> Result<LoadOutcome<WindowState>, ConfigStoreError> {
        self.load::<WindowState>()
    }

    pub fn save_window_state(&self, state: &WindowState) -> Result<(), ConfigStoreError> {
        self.save(state)
    }

    fn load<T: StoredConfig>(&self) -> Result<LoadOutcome<T>, ConfigStoreError> {
        let path = self.paths.config_dir().join(T::FILE_NAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(LoadOutcome::NotFoundDefault(T::default()));
            }
            Err(e) => {
                return self.quarantine(&path, e.to_string());
            }
        };

        // Probe schema_version first so future versions are never modified or backed up.
        match toml::from_str::<VersionProbe>(&text) {
            Err(e) => return self.quarantine(&path, e.to_string()),
            Ok(probe) if probe.schema_version > T::SCHEMA_VERSION => {
                return Ok(LoadOutcome::UnsupportedFutureVersion {
                    found: probe.schema_version,
                    supported: T::SCHEMA_VERSION,
                    path,
                });
            }
            Ok(_) => {}
        }

        match toml::from_str::<T>(&text) {
            Ok(value) => match value.validate() {
                Ok(()) => Ok(LoadOutcome::Loaded(value)),
                Err(reason) => self.quarantine(&path, reason),
            },
            Err(e) => self.quarantine(&path, e.to_string()),
        }
    }

    fn save<T: StoredConfig>(&self, value: &T) -> Result<(), ConfigStoreError> {
        let text = toml::to_string_pretty(value)
            .map_err(|e| ConfigStoreError::Serialize(e.to_string()))?;
        let path = self.paths.config_dir().join(T::FILE_NAME);
        atomic_write(&path, text.as_bytes())
    }

    fn quarantine<T: StoredConfig>(
        &self,
        path: &Path,
        reason: String,
    ) -> Result<LoadOutcome<T>, ConfigStoreError> {
        let backup_path = reserve_invalid_backup_path(path)?;
        if let Err(source) = fs::rename(path, &backup_path) {
            let _ = fs::remove_file(&backup_path);
            return Err(ConfigStoreError::Io {
                path: backup_path,
                source,
            });
        }
        Ok(LoadOutcome::RecoveredFromInvalid {
            value: T::default(),
            backup_path,
            reason,
        })
    }
}

#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u32,
}

/// Reserve a collision-free `*.invalid-<timestamp>[-<seq>]` backup target via
/// `create_new(true)` so concurrent or same-millisecond corruptions never clobber
/// an earlier backup.
fn reserve_invalid_backup_path(path: &Path) -> Result<PathBuf, ConfigStoreError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_owned());

    for attempt in 0..1024u32 {
        let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
        let candidate = if attempt == 0 && seq == 0 {
            path.with_file_name(format!("{file_name}.invalid-{timestamp}"))
        } else {
            path.with_file_name(format!("{file_name}.invalid-{timestamp}-{seq}"))
        };
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                drop(file);
                return Ok(candidate);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(ConfigStoreError::Io {
                    path: candidate,
                    source,
                });
            }
        }
    }

    Err(ConfigStoreError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exhausted unique invalid backup names",
        ),
    })
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.toml".to_owned());
    path.with_file_name(format!(
        ".{file_name}.{}.{nanos}.{seq}.tmp",
        std::process::id()
    ))
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), ConfigStoreError> {
    let dir = path.parent().ok_or_else(|| ConfigStoreError::Io {
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "config path has no parent"),
    })?;
    fs::create_dir_all(dir).map_err(|source| ConfigStoreError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    let tmp = unique_temp_path(path);
    let write_result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()
    })();
    if let Err(source) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigStoreError::Io { path: tmp, source });
    }

    if let Err(source) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(ConfigStoreError::Io {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::{
        AutoJumpMode, MacosDockPolicy, TabBarMode, Theme, ToolId, WakePlacement, WindowGeometry,
    };

    fn sandbox() -> (tempfile::TempDir, ConfigStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ConfigStore::new(ConfigPaths::from_dir(dir.path().to_path_buf()));
        (dir, store)
    }

    fn assert_no_stray_tmp_files(dir: &Path) {
        let stray: Vec<_> = fs::read_dir(dir)
            .expect("read dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(stray.is_empty(), "stray temp files left behind: {stray:?}");
    }

    #[test]
    fn missing_files_return_defaults_without_creating_files() {
        let (dir, store) = sandbox();
        let settings = store.load_settings().expect("load settings");
        assert!(matches!(settings, LoadOutcome::NotFoundDefault(s) if s == AppSettings::default()));
        let win = store.load_window_state().expect("load window state");
        assert!(matches!(win, LoadOutcome::NotFoundDefault(w) if w == WindowState::default()));
        assert!(!store.paths().config_file().exists());
        assert!(!store.paths().window_state_file().exists());
        assert_no_stray_tmp_files(dir.path());
    }

    #[test]
    fn settings_roundtrip_and_on_disk_whitelist_exclude_auto_launch_and_recents() {
        let (_dir, store) = sandbox();
        let mut settings = AppSettings {
            theme: Theme::Dark,
            ..AppSettings::default()
        };
        settings.editor.font_size = 16;
        settings.editor.ligatures = true;
        settings.editor.monospace_font = Some("Cascadia Code".to_owned());
        settings.layout.tab_bar_mode = TabBarMode::SingleRowScroll;
        settings.navigation.favorites = vec![ToolId::JsonFormatter, ToolId::JwtTool];
        settings.residency.wake_placement = WakePlacement::RememberLastPosition;
        settings.smart_detection.auto_jump = AutoJumpMode::HomeOnly;
        settings.platform.macos_dock_policy = MacosDockPolicy::MenuBarOnly;

        store.save_settings(&settings).expect("save");
        let raw = fs::read_to_string(store.paths().config_file()).expect("read raw toml");
        let doc: toml::Value = toml::from_str(&raw).expect("parse toml");
        let root = doc.as_table().expect("root table");
        assert!(
            !root.contains_key("auto_launch"),
            "auto_launch must not persist"
        );
        assert!(!root.contains_key("recents"), "recents must not persist");
        let nav = root
            .get("navigation")
            .and_then(toml::Value::as_table)
            .expect("navigation table");
        assert!(
            !nav.contains_key("recents"),
            "navigation.recents list must not persist"
        );
        assert!(
            !root
                .get("residency")
                .and_then(toml::Value::as_table)
                .expect("residency table")
                .contains_key("auto_launch"),
            "residency.auto_launch must not persist"
        );

        let loaded = store
            .load_settings()
            .expect("load")
            .into_value()
            .expect("v");
        assert_eq!(loaded, settings);
    }

    #[test]
    fn overwrite_replaces_atomically_and_leaves_no_temp_files() {
        let (dir, store) = sandbox();
        let first = AppSettings {
            theme: Theme::Dark,
            ..AppSettings::default()
        };
        store.save_settings(&first).expect("first save");

        let second = AppSettings {
            theme: Theme::Light,
            ..AppSettings::default()
        };
        store.save_settings(&second).expect("second save");

        let loaded = store
            .load_settings()
            .expect("load")
            .into_value()
            .expect("v");
        assert_eq!(loaded.theme, Theme::Light);
        assert_no_stray_tmp_files(dir.path());
    }

    #[test]
    fn corrupt_toml_is_backed_up_preserving_bytes_and_consecutive_corruptions_do_not_clobber() {
        let (dir, store) = sandbox();
        let path = store.paths().config_file();

        fs::write(&path, "bad toml {{{").expect("write first corrupt");
        let first_backup = match store.load_settings().expect("load first") {
            LoadOutcome::RecoveredFromInvalid {
                value,
                backup_path,
                reason,
            } => {
                assert_eq!(value, AppSettings::default());
                assert!(!reason.is_empty());
                assert!(!path.exists());
                assert_eq!(fs::read_to_string(&backup_path).unwrap(), "bad toml {{{");
                let name = backup_path.file_name().unwrap().to_string_lossy();
                assert!(name.starts_with("config.toml.invalid-"), "got {name}");
                backup_path
            }
            other => panic!("expected RecoveredFromInvalid, got {other:?}"),
        };

        fs::write(&path, "second bad toml").expect("write second corrupt");
        let second_backup = match store.load_settings().expect("load second") {
            LoadOutcome::RecoveredFromInvalid { backup_path, .. } => {
                assert_ne!(backup_path, first_backup);
                assert_eq!(fs::read_to_string(&backup_path).unwrap(), "second bad toml");
                backup_path
            }
            other => panic!("expected second RecoveredFromInvalid, got {other:?}"),
        };

        assert_eq!(fs::read_to_string(&first_backup).unwrap(), "bad toml {{{");
        assert!(second_backup.exists());
        assert_no_stray_tmp_files(dir.path());
    }

    #[test]
    fn truncated_or_semantically_invalid_config_triggers_recovery_instead_of_partial_merge() {
        let (_dir, store) = sandbox();
        let path = store.paths().config_file();

        // Truncated file with only schema_version must NOT silently fill missing sections.
        fs::write(&path, "schema_version = 1\n").expect("write truncated");
        assert!(matches!(
            store.load_settings().expect("load truncated"),
            LoadOutcome::RecoveredFromInvalid { .. }
        ));

        // Semantically out-of-range font_size must trigger recovery.
        let mut bad = AppSettings::default();
        bad.editor.font_size = 0;
        let bad_toml = toml::to_string_pretty(&bad).unwrap();
        fs::write(&path, bad_toml).expect("write bad semantics");
        assert!(matches!(
            store.load_settings().expect("load bad semantics"),
            LoadOutcome::RecoveredFromInvalid { .. }
        ));
    }

    #[test]
    fn future_schema_version_is_rejected_without_touching_file() {
        let (dir, store) = sandbox();
        let path = store.paths().config_file();
        let future = "schema_version = 99\nfuture_flag = true\n";
        fs::write(&path, future).expect("write future");

        match store.load_settings().expect("load future") {
            LoadOutcome::UnsupportedFutureVersion {
                found,
                supported,
                path: p,
            } => {
                assert_eq!(found, 99);
                assert_eq!(supported, APP_SETTINGS_SCHEMA_VERSION);
                assert_eq!(p, path);
            }
            other => panic!("expected UnsupportedFutureVersion, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&path).unwrap(), future);
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".invalid-"))
            .collect();
        assert!(backups.is_empty());
        assert_no_stray_tmp_files(dir.path());
    }

    #[test]
    fn legacy_config_without_locale_still_loads() {
        let (_dir, store) = sandbox();
        let path = store.paths().config_file();
        fs::create_dir_all(path.parent().unwrap()).expect("create dir");

        // Hand-written config.toml from before M2 added `locale`.
        let legacy = toml::to_string_pretty(&AppSettings::default())
            .expect("serialize")
            .lines()
            .filter(|line| !line.starts_with("locale ="))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, legacy).expect("write legacy");

        let loaded = store
            .load_settings()
            .expect("load legacy")
            .into_value()
            .expect("value");
        assert_eq!(loaded.locale, devtools_core::Locale::ZhCn);
    }

    #[test]
    fn window_state_roundtrip_and_creates_nested_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("nested").join("devtools");
        let store = ConfigStore::new(ConfigPaths::from_dir(nested.clone()));

        let state = WindowState {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            geometry: Some(WindowGeometry {
                x: 40,
                y: 60,
                width: 1200,
                height: 760,
            }),
            maximized: true,
            tab_skeleton: vec![ToolId::JsonFormatter, ToolId::JwtTool],
            active_tab_index: Some(1),
        };
        store.save_window_state(&state).expect("save");
        let loaded = store
            .load_window_state()
            .expect("load")
            .into_value()
            .expect("value");
        assert_eq!(loaded, state);
        assert_no_stray_tmp_files(&nested);
    }
}

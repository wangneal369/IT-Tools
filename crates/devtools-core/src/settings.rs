//! Persistence whitelist models (ADR-0005 / spec §5).
//!
//! These are the ONLY shell-level structures allowed to reach disk, split across
//! two files: `config.toml` ([`AppSettings`], user preferences) and
//! `window-state.toml` ([`WindowState`], automatically recorded window state).
//!
//! `auto_launch` is deliberately absent: the OS registration
//! (`auto-launch::is_enabled()`) is the single source of truth, so a boolean
//! copy in `config.toml` would drift (ADR-0005).
//!
//! Top-level models intentionally omit `#[serde(default)]` and require explicit
//! [`AppSettings::validate`] / [`WindowState::validate`] checks so truncated or
//! semantically broken TOML triggers corruption recovery instead of silent
//! partial-field guessing.

use serde::{Deserialize, Serialize};

use crate::tool::ToolId;

/// Schema version of `config.toml`.
pub const APP_SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Schema version of `window-state.toml`.
pub const WINDOW_STATE_SCHEMA_VERSION: u32 = 1;

const MIN_FONT_SIZE: u16 = 8;
const MAX_FONT_SIZE: u16 = 48;
const MIN_WINDOW_WIDTH: u32 = 320;
const MIN_WINDOW_HEIGHT: u32 = 240;

/// 主题：跟随系统 / 暗色 / 亮色（默认跟随系统）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

/// 代码编辑器外观（对齐 Issue #5 承诺的三项可配置字体偏好）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorSettings {
    pub font_size: u16,
    pub ligatures: bool,
    /// `None` 表示使用内置 JetBrains Mono；`Some` 为用户指定的本机等宽字体名。
    pub monospace_font: Option<String>,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            font_size: 14,
            ligatures: false,
            monospace_font: None,
        }
    }
}

/// 页签栏模式：多行等宽网格（默认）↔ 单行融合滚动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TabBarMode {
    #[default]
    MultiRowGrid,
    SingleRowScroll,
}

/// 工作台布局（侧边栏折叠、页签栏模式）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutSettings {
    pub sidebar_collapsed: bool,
    pub tab_bar_mode: TabBarMode,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            sidebar_collapsed: false,
            tab_bar_mode: TabBarMode::MultiRowGrid,
        }
    }
}

/// 导航偏好（收藏、最近使用显示开关、标签骨架恢复开关）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationSettings {
    /// 收藏工具的有序 `ToolId` 列表（侧边栏 / 首页 / 托盘菜单同序同步）。
    pub favorites: Vec<ToolId>,
    /// 是否在 UI 显示内存态「最近使用」（列表本体不落盘）。
    pub show_recents: bool,
    /// 标签骨架恢复开关（默认关；仅恢复空标签实例，不恢复输入/输出）。
    pub tab_skeleton_restore: bool,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            show_recents: true,
            tab_skeleton_restore: false,
        }
    }
}

/// 快捷键配置（全局唤起 + 工作台内智能粘贴/命令面板）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotkeySettings {
    /// 全局唤起热键；`None` 表示用户显式禁用，默认按平台回填。
    pub global_toggle: Option<String>,
    /// 工作台内显式智能粘贴快捷键。
    pub smart_paste: String,
    /// 快速命令面板快捷键。
    pub command_palette: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        let default_global = if cfg!(target_os = "macos") {
            "Control+Option+Space"
        } else {
            "Ctrl+Alt+Space"
        };
        let default_paste = if cfg!(target_os = "macos") {
            "Cmd+Shift+V"
        } else {
            "Ctrl+Shift+V"
        };
        let default_palette = if cfg!(target_os = "macos") {
            "Cmd+K"
        } else {
            "Ctrl+K"
        };
        Self {
            global_toggle: Some(default_global.to_owned()),
            smart_paste: default_paste.to_owned(),
            command_palette: default_palette.to_owned(),
        }
    }
}

/// 关窗行为（默认收起到托盘）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CloseBehavior {
    #[default]
    DismissToTray,
    Quit,
}

/// 最小化行为（默认原生最小化，可选收起到托盘）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MinimizeBehavior {
    #[default]
    NativeMinimize,
    DismissToTray,
}

/// 窗口唤起位置策略（默认跟随当前鼠标光标所在屏幕，对齐 Issue #6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WakePlacement {
    #[default]
    FollowCursorScreen,
    RememberLastPosition,
}

/// 常驻后台语义（关窗 / 最小化 / Esc / 唤起位置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidencySettings {
    pub close: CloseBehavior,
    pub minimize: MinimizeBehavior,
    pub esc_to_tray: bool,
    pub wake_placement: WakePlacement,
}

impl Default for ResidencySettings {
    fn default() -> Self {
        Self {
            close: CloseBehavior::DismissToTray,
            minimize: MinimizeBehavior::NativeMinimize,
            esc_to_tray: true,
            wake_placement: WakePlacement::FollowCursorScreen,
        }
    }
}

/// 智能识别唯一高置信度命中时的自动跳转模式（默认关，对齐 Issue #8）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AutoJumpMode {
    #[default]
    Off,
    HomeOnly,
    Always,
}

/// 智能识别开关与隐私写剪贴板行为。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SmartDetectionSettings {
    pub enabled: bool,
    pub auto_jump: AutoJumpMode,
    /// 敏感工具输出默认写入「排除剪贴板历史」标记。
    pub exclude_sensitive_from_clipboard_history: bool,
}

impl Default for SmartDetectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_jump: AutoJumpMode::Off,
            exclude_sensitive_from_clipboard_history: true,
        }
    }
}

/// macOS Dock 图标策略三态（默认混合模式：仅开窗时显示 Dock，对齐 Issue #6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MacosDockPolicy {
    #[default]
    ActiveWindowOnly,
    MenuBarOnly,
    Always,
}

/// 平台差异化偏好。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformSettings {
    pub macos_dock_policy: MacosDockPolicy,
    /// Linux 无 SNI 托盘宿主时默认关窗退出（`false`）；高级用户可强制常驻（`true`）。
    pub linux_force_tray_without_sni: bool,
    /// Wayland portal 热键只读展示文案缓存（非秘密）。
    pub wayland_portal_shortcut_label: Option<String>,
}

impl Default for PlatformSettings {
    fn default() -> Self {
        Self {
            macos_dock_policy: MacosDockPolicy::ActiveWindowOnly,
            linux_force_tray_without_sni: false,
            wayland_portal_shortcut_label: None,
        }
    }
}

/// 应用配置（`config.toml`，`schema_version = 1`）。
///
/// 仅承载显式白名单内的用户偏好，绝不包含任何工具载荷、会话、剪贴板内容或 `auto_launch`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    pub schema_version: u32,
    pub theme: Theme,
    pub editor: EditorSettings,
    pub layout: LayoutSettings,
    pub navigation: NavigationSettings,
    pub hotkeys: HotkeySettings,
    pub residency: ResidencySettings,
    pub smart_detection: SmartDetectionSettings,
    pub platform: PlatformSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: APP_SETTINGS_SCHEMA_VERSION,
            theme: Theme::System,
            editor: EditorSettings::default(),
            layout: LayoutSettings::default(),
            navigation: NavigationSettings::default(),
            hotkeys: HotkeySettings::default(),
            residency: ResidencySettings::default(),
            smart_detection: SmartDetectionSettings::default(),
            platform: PlatformSettings::default(),
        }
    }
}

impl AppSettings {
    /// Semantic validation executed right at the persistence boundary.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version == 0 {
            return Err("schema_version must be >= 1".to_owned());
        }
        if !(MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&self.editor.font_size) {
            return Err(format!(
                "editor.font_size {} is out of range ({MIN_FONT_SIZE}..={MAX_FONT_SIZE})",
                self.editor.font_size
            ));
        }
        Ok(())
    }
}

/// 窗口几何（逻辑像素）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 窗口状态（`window-state.toml`，`schema_version = 1`）。
///
/// 仅记录程序自动维护的窗口几何与可选标签骨架恢复的有序 `ToolId` 列表及活跃索引。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowState {
    pub schema_version: u32,
    pub geometry: Option<WindowGeometry>,
    pub maximized: bool,
    /// 开启标签骨架恢复时记录的有序空标签工具标识列表。
    pub tab_skeleton: Vec<ToolId>,
    pub active_tab_index: Option<usize>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            geometry: None,
            maximized: false,
            tab_skeleton: Vec::new(),
            active_tab_index: None,
        }
    }
}

impl WindowState {
    /// Semantic validation executed right at the persistence boundary.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version == 0 {
            return Err("schema_version must be >= 1".to_owned());
        }
        if let Some(geom) = self.geometry
            && (geom.width < MIN_WINDOW_WIDTH || geom.height < MIN_WINDOW_HEIGHT)
        {
            return Err(format!(
                "window geometry {}x{} below minimum {}x{}",
                geom.width, geom.height, MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT
            ));
        }
        if let Some(idx) = self.active_tab_index
            && idx >= self.tab_skeleton.len()
        {
            return Err(format!(
                "active_tab_index {idx} out of bounds for tab_skeleton len {}",
                self.tab_skeleton.len()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_match_v1_spec_invariants() {
        let settings = AppSettings::default();
        assert_eq!(settings.schema_version, 1);
        assert_eq!(settings.theme, Theme::System);
        assert_eq!(settings.editor.font_size, 14);
        assert!(!settings.editor.ligatures);
        assert!(settings.editor.monospace_font.is_none());
        assert_eq!(settings.layout.tab_bar_mode, TabBarMode::MultiRowGrid);
        assert!(!settings.navigation.tab_skeleton_restore);
        assert_eq!(
            settings.residency.wake_placement,
            WakePlacement::FollowCursorScreen
        );
        assert_eq!(
            settings.residency.minimize,
            MinimizeBehavior::NativeMinimize
        );
        assert_eq!(settings.smart_detection.auto_jump, AutoJumpMode::Off);
        assert!(
            settings
                .smart_detection
                .exclude_sensitive_from_clipboard_history
        );
        assert_eq!(
            settings.platform.macos_dock_policy,
            MacosDockPolicy::ActiveWindowOnly
        );
        assert!(!settings.platform.linux_force_tray_without_sni);
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn semantic_validation_rejects_out_of_bounds_values() {
        let mut settings = AppSettings::default();
        settings.editor.font_size = 0;
        assert!(settings.validate().is_err());

        let mut state = WindowState {
            active_tab_index: Some(0),
            ..WindowState::default()
        };
        assert!(state.validate().is_err());

        state.tab_skeleton = vec![ToolId::JsonFormatter];
        assert!(state.validate().is_ok());
    }
}

//! Window-independent workbench state (ADR-0002).
//!
//! Every open tab, the active selection, favorites, in-memory recents and the
//! lifecycle phase live in this `Entity`, which survives window destruction and
//! `WindowingRequest::Headless`. Windows are disposable projections.

use std::time::{Duration, Instant};

use devtools_core::{
    AppSettings, Category, CloseBehavior, ToolId, WINDOW_STATE_SCHEMA_VERSION, WindowGeometry,
    WindowState,
};
use devtools_platform::{ConfigStore, PlatformCommand, PlatformService};
use gpui_kit::*;

use crate::tool_view::{ToolViewContext, ToolViewRegistry};
use crate::window::{self, set_macos_policy};

/// Grace period before Warm Standby decays into Deep Sleep / Headless (spec §3.1).
pub const WARM_STANDBY_GRACE: Duration = Duration::from_secs(60);
/// Geometry writes are throttled while a user drags/resizes the window.
const GEOMETRY_PERSIST_INTERVAL: Duration = Duration::from_millis(750);

/// A process-local identity for one open tab instance. Two instances of the
/// same tool have distinct ids and hold distinct in-memory state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabInstanceId(pub u64);

/// What a tab shows.
pub enum TabKind {
    /// Workbench home (categories, favorites, recents). Always tab index 0.
    Home,
    /// One tool instance rendered by the tool-view registry.
    Tool {
        tool_id: ToolId,
        /// 1-based instance number shown next to the tool name (`#1`, `#2`).
        instance: u32,
        view: AnyView,
    },
}

pub struct TabEntry {
    pub id: TabInstanceId,
    pub kind: TabKind,
}

impl TabEntry {
    pub fn is_home(&self) -> bool {
        matches!(self.kind, TabKind::Home)
    }
}

/// Three-state residency model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// A window exists and is interactive.
    Active,
    /// Window destroyed, GPU retained; fast wake within the grace period.
    WarmStandby,
    /// GPU detached; only tray, hotkey and IPC remain.
    DeepSleep,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "活跃",
            Self::WarmStandby => "热待机",
            Self::DeepSleep => "深休眠",
        }
    }
}

/// What the home tab is currently showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeView {
    Overview,
    Category(Category),
}

/// In-memory-only recent tools (never persisted; cleared on process exit).
#[derive(Debug, Default, Clone)]
pub struct Recents {
    tools: Vec<ToolId>,
}

impl Recents {
    pub const LIMIT: usize = 5;

    pub fn push(&mut self, tool: ToolId) {
        self.tools.retain(|existing| *existing != tool);
        self.tools.insert(0, tool);
        self.tools.truncate(Self::LIMIT);
    }

    pub fn iter(&self) -> impl Iterator<Item = ToolId> + '_ {
        self.tools.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Everything needed to construct the workbench.
pub struct WorkbenchInit {
    pub store: ConfigStore,
    pub settings: AppSettings,
    pub settings_notice: Option<String>,
    pub window_state: WindowState,
    pub platform: PlatformService,
}

pub struct Workbench {
    pub(crate) store: ConfigStore,
    pub(crate) settings: AppSettings,
    pub(crate) settings_notice: Option<String>,
    pub(crate) window_state: WindowState,
    pub(crate) platform: PlatformService,
    pub(crate) platform_task: Option<Task<()>>,
    pub(crate) tabs: Vec<TabEntry>,
    pub(crate) active: usize,
    next_tab_id: u64,
    pub(crate) recents: Recents,
    pub(crate) home_view: HomeView,
    pub(crate) settings_open: bool,
    pub(crate) autostart_state: Option<Result<bool, String>>,
    pub(crate) active_window: Option<AnyWindowHandle>,
    pub(crate) phase: Phase,
    pub(crate) sleep_timer: Option<Task<()>>,
    pub(crate) registry: ToolViewRegistry,
    pub(crate) status_line: String,
    /// Kept alive for the process lifetime so the app-level key interceptor
    /// (`Ctrl/Cmd+W`, `Esc`) stays registered.
    pub(crate) key_interceptor: Option<Subscription>,
    last_geometry_persist: Option<Instant>,
}

impl Workbench {
    /// Build the workbench, start draining platform commands and restore the
    /// optional (empty) tab skeleton.
    pub fn new(init: WorkbenchInit, cx: &mut App) -> Entity<Self> {
        let WorkbenchInit {
            store,
            settings,
            settings_notice,
            window_state,
            platform,
        } = init;

        let entity = cx.new(|_| Self {
            store,
            settings,
            settings_notice,
            window_state,
            platform,
            platform_task: None,
            tabs: vec![TabEntry {
                id: TabInstanceId(0),
                kind: TabKind::Home,
            }],
            active: 0,
            next_tab_id: 1,
            recents: Recents::default(),
            home_view: HomeView::Overview,
            settings_open: false,
            autostart_state: None,
            active_window: None,
            phase: Phase::DeepSleep,
            sleep_timer: None,
            registry: ToolViewRegistry::new(),
            status_line: "就绪".to_owned(),
            key_interceptor: None,
            last_geometry_persist: None,
        });

        entity.update(cx, |workbench, cx| {
            workbench.spawn_platform_listener(cx);
            workbench.restore_tab_skeleton(cx);
        });
        entity
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    pub fn settings_notice(&self) -> Option<&str> {
        self.settings_notice.as_deref()
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn platform(&self) -> &PlatformService {
        &self.platform
    }

    pub fn platform_mut(&mut self) -> &mut PlatformService {
        &mut self.platform
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_tab(&self) -> &TabEntry {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    pub fn home_view(&self) -> HomeView {
        self.home_view
    }

    pub fn recents(&self) -> &Recents {
        &self.recents
    }

    pub fn is_settings_open(&self) -> bool {
        self.settings_open
    }

    pub fn status_line(&self) -> &str {
        &self.status_line
    }

    pub fn autostart_state(&self) -> Option<&Result<bool, String>> {
        self.autostart_state.as_ref()
    }

    pub fn is_favorite(&self, tool: ToolId) -> bool {
        self.settings.navigation.favorites.contains(&tool)
    }

    /// Register an M4 tool view, replacing the placeholder for `tool`.
    pub fn register_tool_view(
        &mut self,
        tool: ToolId,
        factory: Box<dyn crate::tool_view::ToolViewFactory>,
    ) {
        self.registry.register(tool, factory);
    }

    // ---------------------------------------------------------------- tabs

    /// Open a fresh instance of `tool`; the same tool may be open many times.
    pub fn open_tool(&mut self, tool: ToolId, cx: &mut Context<Self>) {
        let instance = self
            .tabs
            .iter()
            .filter(|tab| matches!(tab.kind, TabKind::Tool { tool_id, .. } if tool_id == tool))
            .count() as u32
            + 1;
        let id = TabInstanceId(self.next_tab_id);
        self.next_tab_id += 1;

        let view = self.registry.create(
            ToolViewContext {
                tool_id: tool,
                instance,
                tab_id: id,
            },
            cx.weak_entity(),
            cx,
        );

        self.tabs.push(TabEntry {
            id,
            kind: TabKind::Tool {
                tool_id: tool,
                instance,
                view,
            },
        });
        self.active = self.tabs.len() - 1;
        self.recents.push(tool);
        self.status_line = format!("已打开 {} #{}", crate::labels::tool_name(tool), instance);
        cx.notify();
    }

    pub fn open_category(&mut self, category: Category, cx: &mut Context<Self>) {
        self.home_view = HomeView::Category(category);
        self.active = 0;
        self.status_line = format!("分类：{}", crate::labels::category_label(category));
        cx.notify();
    }

    pub fn show_home(&mut self, cx: &mut Context<Self>) {
        self.home_view = HomeView::Overview;
        self.active = 0;
        cx.notify();
    }

    /// Select a tab; closing the last tool tab always returns to Home.
    pub fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active = index;
            cx.notify();
        }
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index == 0 || index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.active = self.active.saturating_sub(usize::from(index < self.active));
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        cx.notify();
    }

    /// `Ctrl/Cmd+W`: close the active tab, keeping at least Home.
    pub fn close_active_tab(&mut self, cx: &mut Context<Self>) {
        if self.active == 0 {
            self.status_line = "首页不可关闭".to_owned();
            cx.notify();
            return;
        }
        let index = self.active;
        self.close_tab(index, cx);
    }

    pub fn toggle_favorite(&mut self, tool: ToolId, cx: &mut Context<Self>) {
        let favorites = &mut self.settings.navigation.favorites;
        if let Some(pos) = favorites.iter().position(|existing| *existing == tool) {
            favorites.remove(pos);
            self.status_line = format!("已取消收藏 {}", crate::labels::tool_name(tool));
        } else {
            favorites.push(tool);
            self.status_line = format!("已收藏 {}", crate::labels::tool_name(tool));
        }
        self.persist_settings();
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.settings.layout.sidebar_collapsed = !self.settings.layout.sidebar_collapsed;
        self.persist_settings();
        cx.notify();
    }

    pub fn toggle_tab_bar_mode(&mut self, cx: &mut Context<Self>) {
        self.settings.layout.tab_bar_mode = match self.settings.layout.tab_bar_mode {
            devtools_core::TabBarMode::MultiRowGrid => devtools_core::TabBarMode::SingleRowScroll,
            devtools_core::TabBarMode::SingleRowScroll => devtools_core::TabBarMode::MultiRowGrid,
        };
        self.persist_settings();
        cx.notify();
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = true;
        self.refresh_autostart_state();
        cx.notify();
    }

    pub fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
    }

    // -------------------------------------------------------- settings

    /// Save `config.toml` atomically; failures surface in the settings banner.
    pub fn persist_settings(&mut self) {
        match self.store.save_settings(&self.settings) {
            Ok(()) => self.settings_notice = None,
            Err(err) => {
                self.settings_notice = Some(format!("设置保存失败（内存态继续生效）：{err}"))
            }
        }
    }

    pub fn set_theme(&mut self, theme: devtools_core::Theme, cx: &mut Context<Self>) {
        self.settings.theme = theme;
        self.persist_settings();
        cx.notify();
    }

    pub fn set_locale(&mut self, locale: devtools_core::Locale, cx: &mut Context<Self>) {
        self.settings.locale = locale;
        self.persist_settings();
        cx.notify();
    }

    pub fn set_close_behavior(&mut self, behavior: CloseBehavior, cx: &mut Context<Self>) {
        self.settings.residency.close = behavior;
        self.persist_settings();
        cx.notify();
    }

    pub fn set_smart_detection(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.smart_detection.enabled = enabled;
        self.persist_settings();
        cx.notify();
    }

    pub fn set_show_recents(&mut self, show: bool, cx: &mut Context<Self>) {
        self.settings.navigation.show_recents = show;
        self.persist_settings();
        cx.notify();
    }

    pub fn set_ligatures(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.settings.editor.ligatures = enabled;
        self.status_line = if enabled {
            "已启用等宽连字（M4 编辑器接入后生效）".to_owned()
        } else {
            "已关闭等宽连字".to_owned()
        };
        self.persist_settings();
        cx.notify();
    }

    /// Clamp to the schema-valid range, persist, and report the applied value.
    pub fn adjust_font_size(&mut self, delta: i32, cx: &mut Context<Self>) {
        let current = i32::from(self.settings.editor.font_size);
        let next = (current + delta).clamp(8, 48) as u16;
        self.settings.editor.font_size = next;
        self.persist_settings();
        cx.notify();
    }

    /// Apply a new global hotkey. A rejected binding rolls the settings field
    /// back so the UI never disagrees with the OS.
    pub fn apply_hotkey(&mut self, shortcut: Option<String>, cx: &mut Context<Self>) {
        let previous = self.settings.hotkeys.global_toggle.clone();
        let status = self.platform.apply_hotkey(shortcut.as_deref());
        match status {
            devtools_platform::HotkeyStatus::Active { .. } => {
                self.settings.hotkeys.global_toggle = shortcut;
                self.status_line = status.label();
                self.persist_settings();
            }
            devtools_platform::HotkeyStatus::Disabled => {
                self.settings.hotkeys.global_toggle = None;
                self.status_line = status.label();
                self.persist_settings();
            }
            devtools_platform::HotkeyStatus::Degraded { reason, .. } => {
                // Roll back to the previously registered binding.
                self.status_line = format!("热键未生效，已回滚：{reason}");
                if previous != shortcut {
                    self.settings.hotkeys.global_toggle = previous;
                }
            }
        }
        cx.notify();
    }

    pub fn refresh_autostart_state(&mut self) {
        self.autostart_state = Some(
            self.platform
                .autostart()
                .is_enabled()
                .map_err(|err| err.to_string()),
        );
    }

    /// Toggle the OS login item; the OS is the only source of truth.
    pub fn toggle_autostart(&mut self, cx: &mut Context<Self>) {
        let current = matches!(self.autostart_state, Some(Ok(true)));
        let state = self
            .platform
            .autostart()
            .set_enabled(!current)
            .map_err(|err| err.to_string());
        if let Err(err) = &state {
            self.status_line = format!("开机自启设置失败：{err}");
        } else {
            self.status_line = "开机自启已同步到操作系统".to_owned();
        }
        self.autostart_state = Some(state);
        cx.notify();
    }

    pub fn set_status(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.status_line = message.into();
        cx.notify();
    }

    // ---------------------------------------------------- persistence

    pub fn note_geometry(&mut self, geometry: WindowGeometry, maximized: bool) {
        self.window_state.geometry = Some(geometry);
        self.window_state.maximized = maximized;
        let due = self
            .last_geometry_persist
            .is_none_or(|last| last.elapsed() >= GEOMETRY_PERSIST_INTERVAL);
        if due {
            self.persist_window_state_now();
            self.last_geometry_persist = Some(Instant::now());
        }
    }

    /// Persist geometry + optional tab skeleton atomically.
    pub fn persist_window_state_now(&mut self) {
        let skeleton: Vec<ToolId> = if self.settings.navigation.tab_skeleton_restore {
            self.tabs
                .iter()
                .filter_map(|tab| match tab.kind {
                    TabKind::Tool { tool_id, .. } => Some(tool_id),
                    TabKind::Home => None,
                })
                .collect()
        } else {
            Vec::new()
        };

        let active_tab_index = {
            let mut tool_index = 0usize;
            let mut found = None;
            for (index, tab) in self.tabs.iter().enumerate() {
                if tab.is_home() {
                    continue;
                }
                if index == self.active {
                    found = Some(tool_index);
                    break;
                }
                tool_index += 1;
            }
            found.filter(|index| *index < skeleton.len())
        };

        let next = WindowState {
            schema_version: WINDOW_STATE_SCHEMA_VERSION,
            geometry: self.window_state.geometry,
            maximized: self.window_state.maximized,
            tab_skeleton: skeleton,
            active_tab_index,
        };

        if next.validate().is_err() {
            return;
        }
        if let Err(err) = self.store.save_window_state(&next) {
            self.settings_notice = Some(format!("窗口状态保存失败：{err}"));
        } else {
            self.window_state = next;
        }
    }

    /// Restore only empty tool tabs (never input/output/options).
    pub fn restore_tab_skeleton(&mut self, cx: &mut Context<Self>) {
        if !self.settings.navigation.tab_skeleton_restore {
            return;
        }
        let skeleton = self.window_state.tab_skeleton.clone();
        if skeleton.is_empty() {
            return;
        }
        for tool in skeleton {
            self.open_tool(tool, cx);
        }
        if let Some(active) = self.window_state.active_tab_index
            && active + 1 < self.tabs.len()
        {
            self.active = active + 1;
        }
    }

    // ------------------------------------------------------- lifecycle

    /// Whether closing the window should quit instead of hiding to the tray.
    pub fn close_should_quit(&self) -> bool {
        if self.settings.residency.close == CloseBehavior::Quit {
            return true;
        }
        // Without a tray (e.g. Linux without an SNI host) hiding would make the
        // app unreachable, unless the user forced residency.
        !self.platform.tray_available() && !self.settings.platform.linux_force_tray_without_sni
    }

    /// Called from `on_window_should_close` after the window was removed.
    pub fn on_close_requested(
        &mut self,
        geometry: WindowGeometry,
        maximized: bool,
        cx: &mut Context<Self>,
    ) {
        self.active_window = None;
        self.window_state.geometry = Some(geometry);
        self.window_state.maximized = maximized;
        self.persist_window_state_now();

        if self.close_should_quit() {
            self.quit_now(cx);
        } else {
            self.enter_warm_standby(cx);
        }
    }

    pub fn enter_warm_standby(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::WarmStandby;
        set_macos_policy(cx, false);
        self.sleep_timer = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor().timer(WARM_STANDBY_GRACE).await;
            cx.update(|cx| {
                if let Some(workbench) = weak.upgrade() {
                    workbench.update(cx, |workbench, cx| {
                        if workbench.phase == Phase::WarmStandby
                            && workbench.active_window.is_none()
                        {
                            workbench.enter_deep_sleep(cx);
                        }
                    });
                }
            });
        }));
        cx.notify();
    }

    pub fn enter_deep_sleep(&mut self, cx: &mut Context<Self>) {
        self.sleep_timer = None;
        if cx.graphical_environment().is_none() {
            self.phase = Phase::DeepSleep;
            cx.notify();
            return;
        }
        let task = cx.request_windowing(WindowingRequest::Headless);
        self.sleep_timer = Some(cx.spawn(async move |weak, cx| {
            let result = task.await;
            if let Err(err) = &result {
                eprintln!("[devtools] request_windowing(Headless) failed: {err:#}");
            }
            cx.update(|cx| {
                if let Some(workbench) = weak.upgrade() {
                    workbench.update(cx, |workbench, cx| {
                        workbench.phase = if result.is_ok() {
                            Phase::DeepSleep
                        } else {
                            Phase::WarmStandby
                        };
                        cx.notify();
                    });
                }
            });
        }));
    }

    /// Ordered quit (ADR-0002): cancel timers, destroy windows, drop tray /
    /// hotkey / IPC, then quit.
    pub fn quit_now(&mut self, cx: &mut Context<Self>) {
        self.sleep_timer = None;
        if let Some(handle) = self.active_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
        self.persist_window_state_now();
        self.platform.shutdown();
        cx.quit();
    }

    fn spawn_platform_listener(&mut self, cx: &mut Context<Self>) {
        let Some(receiver) = self.platform.take_commands() else {
            return;
        };
        self.platform_task = Some(cx.spawn(async move |weak, cx| {
            while let Ok(command) = receiver.recv().await {
                let alive = cx.update(|cx| {
                    let Some(workbench) = weak.upgrade() else {
                        return false;
                    };
                    match command {
                        PlatformCommand::Toggle => window::smart_toggle(&workbench, cx),
                        PlatformCommand::Show => window::show(&workbench, cx, false),
                        PlatformCommand::OpenSettings => {
                            workbench.update(cx, |workbench, cx| workbench.open_settings(cx));
                            window::show(&workbench, cx, true);
                        }
                        PlatformCommand::Quit => window::ordered_quit(&workbench, cx),
                    }
                    true
                });
                if !alive {
                    break;
                }
            }
        }));
    }
}

/// Clamp recorded geometry to the schema's minimum so a restored window is
/// never rejected by `WindowState::validate`.
pub fn clamp_geometry(geometry: WindowGeometry) -> WindowGeometry {
    WindowGeometry {
        x: geometry.x,
        y: geometry.y,
        width: geometry.width.max(320),
        height: geometry.height.max(240),
    }
}

/// Convert a GPUI window-bounds rectangle into persisted logical geometry.
pub fn geometry_from_bounds(bounds: WindowBounds) -> WindowGeometry {
    let inner = bounds.get_bounds();
    clamp_geometry(WindowGeometry {
        x: f32::from(inner.origin.x).round() as i32,
        y: f32::from(inner.origin.y).round() as i32,
        width: f32::from(inner.size.width).max(1.0).round() as u32,
        height: f32::from(inner.size.height).max(1.0).round() as u32,
    })
}

/// Rebuild `WindowBounds` from restored state, defaulting to a centered window.
pub fn window_bounds_from_state(state: &WindowState, cx: &App) -> WindowBounds {
    let Some(geometry) = state.geometry else {
        return WindowBounds::centered(None, size(px(1100.0), px(720.0)), cx);
    };
    let geometry = clamp_geometry(geometry);
    let bounds = Bounds {
        origin: point(px(geometry.x as f32), px(geometry.y as f32)),
        size: size(px(geometry.width as f32), px(geometry.height as f32)),
    };
    if state.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

#[cfg(test)]
mod tests {
    // NOTE: no `use super::*` here — `gpui_kit::*` also exports GPUI's `test`
    // macro, which would shadow the builtin `#[test]` attribute.
    use super::{Recents, clamp_geometry};
    use devtools_core::{ToolId, WindowGeometry, WindowState};

    #[test]
    fn recents_are_unique_newest_first_and_capped() {
        let mut recents = Recents::default();
        for tool in ToolId::ALL.into_iter().take(7) {
            recents.push(tool);
        }
        assert_eq!(recents.iter().count(), Recents::LIMIT);
        assert_eq!(recents.iter().next(), Some(ToolId::ALL[6]));

        recents.push(ToolId::ALL[2]);
        assert_eq!(recents.iter().next(), Some(ToolId::ALL[2]));
        assert_eq!(
            recents
                .iter()
                .filter(|tool| *tool == ToolId::ALL[2])
                .count(),
            1
        );
    }

    #[test]
    fn geometry_clamp_keeps_window_state_valid() {
        let tiny = WindowGeometry {
            x: -20,
            y: 10,
            width: 5,
            height: 1,
        };
        let clamped = clamp_geometry(tiny);
        assert_eq!(clamped.width, 320);
        assert_eq!(clamped.height, 240);
        assert!(
            WindowState {
                geometry: Some(clamped),
                ..WindowState::default()
            }
            .validate()
            .is_ok()
        );
    }
}

//! Application boot: GPUI application construction and the two-stage headless
//! contract (ADR-0002).
//!
//! MUST use `application().with_quit_mode(QuitMode::Explicit)` and start
//! `--silent` via `WindowingRequest::Headless`; never call
//! `gpui_kit::platform::headless()` (permanently unswitchable).

use devtools_core::{AppSettings, WindowState};
use devtools_platform::{ConfigStore, Instance, PlatformService, PlatformStartup};
use gpui_kit::*;

use crate::shell::WorkbenchView;
use crate::window;
use crate::workspace::{Phase, Workbench, WorkbenchInit};

/// What the process should show once GPUI is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    /// Normal launch: open the workbench window.
    ShowWindow,
    /// `--settings`: open the window with the settings drawer visible.
    ShowSettings,
    /// `--silent`: zero windows, headless residency.
    Silent,
}

/// Root owner for the window-independent [`Workbench`] entity.
///
/// `--silent` starts with zero windows, so no `WorkbenchView` holds a strong
/// reference yet; without this global the entity would be dropped immediately
/// and tray / hotkey / IPC commands would have no receiver. Keeping one strong
/// handle in an app global keeps the workbench alive for the whole process.
struct WorkbenchRoot {
    /// Owned only to keep the entity alive; never read.
    _entity: Entity<Workbench>,
}

impl Global for WorkbenchRoot {}

/// Everything `devtools-app` resolved before starting GPUI.
pub struct AppBoot {
    pub store: ConfigStore,
    pub settings: AppSettings,
    pub settings_notice: Option<String>,
    pub window_state: WindowState,
    pub instance: Instance,
    pub start: StartMode,
}

/// Build and run the GPUI application.
pub fn run(boot: AppBoot) {
    let AppBoot {
        store,
        settings,
        settings_notice,
        window_state,
        instance,
        start,
    } = boot;

    let initial_windowing = match start {
        StartMode::Silent => WindowingRequest::Headless,
        StartMode::ShowWindow | StartMode::ShowSettings => {
            WindowingRequest::Windowed(GraphicalEnvironment::detect())
        }
    };
    let global_shortcut = settings.hotkeys.global_toggle.clone();

    gpui_kit::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_windowing(initial_windowing)
        .run(move |cx| {
            gpui_kit::init(cx);

            let platform = PlatformService::start(PlatformStartup {
                instance,
                global_shortcut,
            });

            let workbench = Workbench::new(
                WorkbenchInit {
                    store,
                    settings,
                    settings_notice,
                    window_state,
                    platform,
                },
                cx,
            );

            cx.set_global(WorkbenchRoot {
                _entity: workbench.clone(),
            });
            install_key_interceptor(&workbench, cx);

            match start {
                StartMode::ShowWindow => window::show(&workbench, cx, false),
                StartMode::ShowSettings => window::show(&workbench, cx, true),
                StartMode::Silent => {
                    // Headless: tray, hotkey and IPC stay alive, no window.
                    window::set_macos_policy(cx, false);
                    workbench.update(cx, |state, _| state.phase = Phase::DeepSleep);
                }
            }

            // Keep the root view type linked even before its first window.
            let _ = std::any::TypeId::of::<WorkbenchView>();
        });
}

fn install_key_interceptor(workbench: &Entity<Workbench>, cx: &mut App) {
    let weak = workbench.downgrade();
    let subscription = cx.intercept_keystrokes(move |event, _window, cx| {
        let keystroke = &event.keystroke;
        let secondary = keystroke.modifiers.secondary(cx);

        if secondary && keystroke.key.eq_ignore_ascii_case("w") {
            if let Some(workbench) = weak.upgrade() {
                workbench.update(cx, |state, cx| state.close_active_tab(cx));
            }
            cx.stop_propagation();
            return;
        }

        if keystroke.key.eq_ignore_ascii_case("escape") {
            if let Some(workbench) = weak.upgrade() {
                let settings_open = workbench.read(cx).is_settings_open();
                if settings_open {
                    workbench.update(cx, |state, cx| state.close_settings(cx));
                } else if workbench.read(cx).active_window.is_some() {
                    window::hide_to_tray(&workbench, cx);
                }
            }
            cx.stop_propagation();
        }
    });

    workbench.update(cx, |state, _| state.key_interceptor = Some(subscription));
}

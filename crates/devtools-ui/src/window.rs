//! Window creation, dismissal and two-stage headless transitions (ADR-0002).
//!
//! GPUI has no per-window hide primitive on any platform, so "send to tray"
//! destroys the window and keeps the workbench entity alive; after the 60 s
//! grace period the whole graphical environment is detached with
//! `WindowingRequest::Headless`.

use gpui_kit::*;

use crate::shell::WorkbenchView;
use crate::workspace::{Phase, Workbench, geometry_from_bounds, window_bounds_from_state};

/// Raise the window and focus it; creates or re-attaches it as needed.
pub fn show(workbench: &Entity<Workbench>, cx: &mut App, open_settings: bool) {
    workbench.update(cx, |state, cx| {
        state.sleep_timer = None;
        if open_settings {
            state.open_settings(cx);
        }
        cx.notify();
    });

    set_macos_policy(cx, true);

    if let Some(handle) = workbench.read(cx).active_window {
        let focused = handle
            .update(cx, |_, window, cx| {
                window.activate_window();
                cx.activate(true);
            })
            .is_ok();
        if focused {
            workbench.update(cx, |state, cx| {
                state.phase = Phase::Active;
                cx.notify();
            });
            return;
        }
        // The window was destroyed behind our back.
        workbench.update(cx, |state, _| state.active_window = None);
    }

    let needs_reattach =
        cx.graphical_environment().is_none() || workbench.read(cx).phase() == Phase::DeepSleep;
    if needs_reattach {
        let task = cx.request_windowing(WindowingRequest::Windowed(GraphicalEnvironment::detect()));
        let weak = workbench.downgrade();
        cx.spawn(async move |cx| {
            let result = task.await;
            cx.update(|cx| {
                if let Err(err) = result {
                    eprintln!("[devtools] request_windowing(Windowed) failed: {err:#}");
                }
                if let Some(workbench) = weak.upgrade() {
                    create_main_window(&workbench, cx);
                }
            });
        })
        .detach();
    } else {
        create_main_window(workbench, cx);
    }
}

/// Smart toggle: hide only when the window is currently the foreground window.
pub fn smart_toggle(workbench: &Entity<Workbench>, cx: &mut App) {
    let is_foreground = workbench
        .read(cx)
        .active_window
        .and_then(|handle| {
            handle
                .update(cx, |_, window, _| window.is_window_active())
                .ok()
        })
        .unwrap_or(false);

    if is_foreground {
        hide_to_tray(workbench, cx);
    } else {
        show(workbench, cx, false);
    }
}

/// Destroy the window and enter Warm Standby, persisting geometry first.
pub fn hide_to_tray(workbench: &Entity<Workbench>, cx: &mut App) {
    let handle = workbench.read(cx).active_window;
    let mut geometry = None;
    let mut maximized = false;

    if let Some(handle) = handle {
        let _ = handle.update(cx, |_, window, _| {
            geometry = Some(geometry_from_bounds(window.window_bounds()));
            maximized = window.is_maximized();
            window.remove_window();
        });
    }

    workbench.update(cx, |state, cx| {
        state.active_window = None;
        if let Some(geometry) = geometry {
            state.window_state.geometry = Some(geometry);
            state.window_state.maximized = maximized;
        }
        state.persist_window_state_now();
        state.enter_warm_standby(cx);
    });
}

/// Ordered quit: timers → window → tray/hotkey/IPC → `cx.quit()`.
pub fn ordered_quit(workbench: &Entity<Workbench>, cx: &mut App) {
    workbench.update(cx, |state, cx| state.quit_now(cx));
}

fn create_main_window(workbench: &Entity<Workbench>, cx: &mut App) {
    let bounds = {
        let state = workbench.read(cx);
        window_bounds_from_state(&state.window_state, cx)
    };

    let options = WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("DevTools".into()),
            appears_transparent: false,
            ..Default::default()
        }),
        window_background: WindowBackgroundAppearance::Opaque,
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        window_min_size: Some(size(px(320.0), px(240.0))),
        ..Default::default()
    };

    let state_for_close = workbench.clone();
    let state_for_view = workbench.clone();
    let result = gpui_kit::open_window(options, cx, move |window, cx| {
        let close_state = state_for_close.clone();
        // Zed #66369 guardrail: return false and explicitly remove the window.
        window.on_window_should_close(cx, move |window, cx| {
            let geometry = geometry_from_bounds(window.window_bounds());
            let maximized = window.is_maximized();
            window.remove_window();
            close_state.update(cx, |state, cx| {
                state.on_close_requested(geometry, maximized, cx);
            });
            false
        });

        cx.new(|cx| WorkbenchView::new(state_for_view.clone(), window, cx))
    });

    match result {
        Ok((handle, _view)) => {
            cx.activate(true);
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            workbench.update(cx, |state, cx| {
                state.active_window = Some(handle);
                state.phase = Phase::Active;
                cx.notify();
            });
        }
        Err(err) => {
            eprintln!("[devtools] failed to open main window: {err:#}");
            workbench.update(cx, |state, cx| {
                state.set_status(format!("无法打开主窗口：{err:#}"), cx);
            });
        }
    }
}

pub(crate) fn set_macos_policy(cx: &mut App, regular: bool) {
    #[cfg(target_os = "macos")]
    {
        cx.set_activation_policy(if regular {
            ActivationPolicy::Regular
        } else {
            ActivationPolicy::Accessory
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cx, regular);
    }
}

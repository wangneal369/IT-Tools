//! Minimal runnable Windows GPUI prototype for Issue #12:
//! Validates ADR-0001 (`gpui-kit` / `gpui-pre 0.3.14`) + ADR-0002 two-stage headless residency:
//!   Active <-> Warm Standby (remove_window + QuitMode::Explicit) <-> Deep Sleep (WindowingRequest::Headless / detach_gpu)
//!   + tray-icon 0.27 / muda 0.22 nested menu + global-hotkey 0.10 + single-instance Named Pipe IPC.

mod ipc;
mod metrics;
mod tray_hotkey;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::ipc::{AcquireOutcome, IpcCommand};
use crate::metrics::{ResourceSnapshot, compute_cpu_pct, snapshot_now};
use crate::tray_hotkey::{SystemEvent, TrayAndHotkeyHandles, init_tray_and_hotkey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Active,
    WarmStandby,
    DeepSleepHeadless,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Active => "Active (Windowed)",
            Self::WarmStandby => "Warm Standby (0 Windows, GPU Retained)",
            Self::DeepSleepHeadless => "Deep Sleep (Headless, GPU Detached)",
        }
    }
}

#[derive(Debug, Clone)]
struct CliOptions {
    ipc_cmd: IpcCommand,
    grace_secs: u64,
    bench_mode: bool,
    stress_cycles: usize,
    quit_after_stress: bool,
}

impl CliOptions {
    fn parse() -> Self {
        let mut args = std::env::args().skip(1);
        let mut opts = Self {
            ipc_cmd: IpcCommand::Show,
            grace_secs: 60,
            bench_mode: false,
            stress_cycles: 0,
            quit_after_stress: false,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--silent" => opts.ipc_cmd = IpcCommand::Silent,
                "--toggle" => opts.ipc_cmd = IpcCommand::Toggle,
                "--show" => opts.ipc_cmd = IpcCommand::Show,
                "--quit-running" => opts.ipc_cmd = IpcCommand::Quit,
                "--bench" => opts.bench_mode = true,
                "--quit-after" => opts.quit_after_stress = true,
                "--grace" => {
                    if let Some(v) = args.next().and_then(|s| s.parse::<u64>().ok()) {
                        opts.grace_secs = v;
                    }
                }
                "--stress" => {
                    if let Some(v) = args.next().and_then(|s| s.parse::<usize>().ok()) {
                        opts.stress_cycles = v;
                    }
                }
                _ => {}
            }
        }
        opts
    }
}

#[derive(Debug, Clone)]
struct MockTab {
    name: &'static str,
    payload: String,
    edits: u64,
}

/// Authoritative window-independent state surviving window destruction & Headless GPU detach.
struct ResidencyState {
    phase: Phase,
    grace_secs: u64,
    hotkey_status: String,
    tabs: Vec<MockTab>,
    active_tab: usize,
    saved_bounds: Option<WindowBounds>,
    active_window: Option<AnyWindowHandle>,
    sleep_timer: Option<Task<()>>,
    transition_count: u64,
    wake_warm_ms: Vec<f64>,
    wake_deep_ms: Vec<f64>,
    last_snapshot: ResourceSnapshot,
    event_log: Vec<String>,
    pending_wake_start: Option<(Instant, Phase)>,
    wake_notify_tx: Option<async_channel::Sender<f64>>,
    action_tx: async_channel::Sender<AppAction>,
}

#[derive(Debug, Clone)]
enum AppAction {
    DismissToWarmStandby,
    EnterDeepSleepNow,
    WakeWindow {
        select_tool: Option<&'static str>,
        reason: &'static str,
    },
    SmartToggle {
        reason: &'static str,
    },
    RunStress {
        cycles: usize,
        quit_after: bool,
    },
    QuitApp,
}

impl ResidencyState {
    fn log(&mut self, msg: impl Into<String>) {
        let s = msg.into();
        eprintln!("[proto] {s}");
        self.event_log.push(s);
        if self.event_log.len() > 24 {
            let drain = self.event_log.len() - 24;
            self.event_log.drain(0..drain);
        }
    }

    fn select_or_add_tool(&mut self, name: &'static str) {
        if let Some(idx) = self.tabs.iter().position(|t| t.name == name) {
            self.active_tab = idx;
        } else {
            self.tabs.push(MockTab {
                name,
                payload: format!("{{\"tool\": \"{name}\", \"preserved\": true}}"),
                edits: 1,
            });
            self.active_tab = self.tabs.len() - 1;
        }
    }
}

/// Thin GPUI view projected from `Entity<ResidencyState>` — destroyed on dismiss, rebuilt on wake.
struct ProtoWorkbenchView {
    state: Entity<ResidencyState>,
}

impl Render for ProtoWorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state_read = self.state.read(cx);
        let phase = state_read.phase;
        let grace = state_read.grace_secs;
        let hk_status = state_read.hotkey_status.clone();
        let transitions = state_read.transition_count;
        let snap = state_read.last_snapshot;
        let tabs = state_read.tabs.clone();
        let active_tab = state_read.active_tab;
        let warm_p95 = percentile(&state_read.wake_warm_ms, 95.0);
        let deep_p95 = percentile(&state_read.wake_deep_ms, 95.0);
        let warm_n = state_read.wake_warm_ms.len();
        let deep_n = state_read.wake_deep_ms.len();
        let logs: Vec<String> = state_read.event_log.iter().rev().take(8).cloned().collect();
        let action_tx = state_read.action_tx.clone();
        let state_entity = self.state.clone();

        // Measure first-frame wake latency right after window creation.
        if state_read.pending_wake_start.is_some() {
            let entity_for_frame = self.state.clone();
            window.on_next_frame(move |_window, cx| {
                entity_for_frame.update(cx, |st, cx| {
                    if let Some((start, from_phase)) = st.pending_wake_start.take() {
                        let ms = start.elapsed().as_secs_f64() * 1000.0;
                        match from_phase {
                            Phase::WarmStandby => st.wake_warm_ms.push(ms),
                            Phase::DeepSleepHeadless => st.wake_deep_ms.push(ms),
                            Phase::Active => {}
                        }
                        st.last_snapshot = snapshot_now();
                        st.log(format!(
                            "Wake ready from {:?} in {:.2} ms (RSS {:.1} MB, VRAM {:.1} MB, threads {})",
                            from_phase,
                            ms,
                            st.last_snapshot.working_set_mb,
                            st.last_snapshot.gpu_dedicated_mb,
                            st.last_snapshot.thread_count
                        ));
                        if let Some(tx) = st.wake_notify_tx.take() {
                            let _ = tx.try_send(ms);
                        }
                        cx.notify();
                    }
                });
            });
        }

        let current_tab = tabs.get(active_tab).cloned();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x141517))
            .text_color(rgb(0xe8eaed))
            .p_4()
            .gap_3()
            .child(
                // Top header bar
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(0x2a2d34))
                    .pb_2()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("DevTools Two-Stage Headless Residency Prototype (#12)"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(0x9aa0a6))
                                    .child(format!(
                                        "Phase: {} | Transitions: {} | Grace: {}s | Hotkey: {}",
                                        phase.label(),
                                        transitions,
                                        grace,
                                        hk_status
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("btn-dismiss")
                                    .label("收起到热待机")
                                    .on_click({
                                        let tx = action_tx.clone();
                                        move |_, _, _| {
                                            let _ = tx.try_send(AppAction::DismissToWarmStandby);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-sleep")
                                    .label("立即深休眠")
                                    .on_click({
                                        let tx = action_tx.clone();
                                        move |_, _, _| {
                                            let _ = tx.try_send(AppAction::EnterDeepSleepNow);
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-stress")
                                    .primary()
                                    .label("10 轮压力测试")
                                    .on_click({
                                        let tx = action_tx.clone();
                                        move |_, _, _| {
                                            let _ = tx.try_send(AppAction::RunStress {
                                                cycles: 10,
                                                quit_after: false,
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("btn-quit")
                                    .danger()
                                    .label("彻底退出")
                                    .on_click({
                                        let tx = action_tx.clone();
                                        move |_, _, _| {
                                            let _ = tx.try_send(AppAction::QuitApp);
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                // Tab bar (proves window-independent TabSession survival)
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(tabs.iter().enumerate().map(|(i, tab)| {
                        let is_active = i == active_tab;
                        let st = state_entity.clone();
                        div()
                            .id(("tab", i))
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if is_active {
                                rgb(0x273449)
                            } else {
                                rgb(0x1c1e22)
                            })
                            .border_1()
                            .border_color(if is_active {
                                rgb(0x5e9eff)
                            } else {
                                rgb(0x2a2d34)
                            })
                            .text_xs()
                            .child(format!("{} (#{} edits)", tab.name, tab.edits))
                            .on_click(move |_, _, cx| {
                                st.update(cx, |s, cx| {
                                    s.active_tab = i;
                                    cx.notify();
                                });
                            })
                    })),
            )
            .child(
                // Active mock tool session box
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x1c1e22))
                    .border_1()
                    .border_color(rgb(0x2a2d34))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().text_sm().child(format!(
                                "权威内存 TabSession（关窗/深休眠不丢失）：{}",
                                current_tab.as_ref().map(|t| t.name).unwrap_or("-")
                            )))
                            .child(
                                Button::new("btn-mutate")
                                    .label("修改内存会话 (+1)")
                                    .on_click({
                                        let st = state_entity.clone();
                                        move |_, _, cx| {
                                            st.update(cx, |s, cx| {
                                                if let Some(t) = s.tabs.get_mut(s.active_tab) {
                                                    t.edits += 1;
                                                    t.payload = format!(
                                                        "{{\"tool\": \"{}\", \"edits\": {}, \"in_memory_only\": true}}",
                                                        t.name, t.edits
                                                    );
                                                }
                                                s.last_snapshot = snapshot_now();
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .p_2()
                            .rounded_sm()
                            .bg(rgb(0x111214))
                            .text_xs()
                            .text_color(rgb(0xa8d1ff))
                            .child(
                                current_tab
                                    .as_ref()
                                    .map(|t| t.payload.clone())
                                    .unwrap_or_default(),
                            ),
                    ),
            )
            .child(
                // Live metrics strip
                div()
                    .flex()
                    .gap_4()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x1c1e22))
                    .border_1()
                    .border_color(rgb(0x2a2d34))
                    .text_xs()
                    .child(format!("RSS: {:.1} MB", snap.working_set_mb))
                    .child(format!("Private: {:.1} MB", snap.private_mb))
                    .child(format!("GPU Dedicated: {:.1} MB", snap.gpu_dedicated_mb))
                    .child(format!("GPU Shared: {:.1} MB", snap.gpu_shared_mb))
                    .child(format!("Threads: {}", snap.thread_count))
                    .child(format!("Warm Wake P95 (n={}): {:.1} ms", warm_n, warm_p95))
                    .child(format!("Deep Wake P95 (n={}): {:.1} ms", deep_n, deep_p95)),
            )
            .child(
                // Recent lifecycle event log
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0x111214))
                    .border_1()
                    .border_color(rgb(0x2a2d34))
                    .text_xs()
                    .text_color(rgb(0x9aa0a6))
                    .child("Lifecycle Event Log (newest first):")
                    .children(logs.into_iter().map(|line| div().child(line))),
            )
    }
}

fn percentile(vals: &[f64], p: f64) -> f64 {
    if vals.is_empty() {
        return 0.0;
    }
    let mut sorted = vals.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn open_or_focus_window(
    state: &Entity<ResidencyState>,
    select_tool: Option<&'static str>,
    reason: &'static str,
    cx: &mut App,
) {
    let from_phase = state.read(cx).phase;

    if let Some(tool) = select_tool {
        state.update(cx, |s, cx| {
            s.select_or_add_tool(tool);
            cx.notify();
        });
    }

    // Cancel pending WarmStandby -> DeepSleep timer immediately.
    state.update(cx, |s, _| {
        s.sleep_timer = None;
    });

    // If already Active with a live window, just activate & focus it.
    if let Some(handle) = state.read(cx).active_window {
        let updated = handle.update(cx, |_, window, cx| {
            cx.activate(true);
            window.activate_window();
        });
        if updated.is_ok() {
            state.update(cx, |s, cx| {
                s.log(format!("Focused existing Active window ({reason})"));
                cx.notify();
            });
            return;
        }
    }

    let wake_start = Instant::now();

    // If currently Headless, re-attach GPU first via WindowingRequest::Windowed.
    if cx.graphical_environment().is_none() || from_phase == Phase::DeepSleepHeadless {
        let task = cx.request_windowing(WindowingRequest::Windowed(GraphicalEnvironment::detect()));
        let state_weak = state.downgrade();
        cx.spawn(async move |cx| {
            let res = task.await;
            let _ = cx.update(|cx| {
                if let Err(err) = res {
                    eprintln!("[proto] request_windowing(Windowed) returned: {err:#}");
                }
                if let Some(st) = state_weak.upgrade() {
                    create_main_window(&st, wake_start, from_phase, reason, cx);
                }
            });
        })
        .detach();
    } else {
        create_main_window(state, wake_start, from_phase, reason, cx);
    }
}

fn create_main_window(
    state: &Entity<ResidencyState>,
    wake_start: Instant,
    from_phase: Phase,
    reason: &'static str,
    cx: &mut App,
) {
    let saved_bounds = state.read(cx).saved_bounds;
    let window_bounds = saved_bounds.unwrap_or_else(|| {
        WindowBounds::centered(None, size(px(900.0), px(580.0)), cx)
    });

    let options = WindowOptions {
        window_bounds: Some(window_bounds),
        titlebar: Some(TitlebarOptions {
            title: Some("DevTools Residency Prototype (#12)".into()),
            appears_transparent: false,
            ..Default::default()
        }),
        window_background: WindowBackgroundAppearance::Opaque,
        focus: true,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };

    let state_clone = state.clone();
    let res = gpui_kit::open_window(options, cx, move |window, cx| {
        let st_close = state_clone.clone();
        // Guardrail for Zed #66369: return false and explicitly call window.remove_window().
        window.on_window_should_close(cx, move |window, cx| {
            let bounds = window.window_bounds();
            let tx = st_close.update(cx, |s, _| {
                s.saved_bounds = Some(bounds);
                s.active_window = None;
                s.action_tx.clone()
            });
            window.remove_window();
            let _ = tx.try_send(AppAction::DismissToWarmStandby);
            false
        });

        cx.new(|_| ProtoWorkbenchView {
            state: state_clone.clone(),
        })
    });

    match res {
        Ok((handle, _view)) => {
            cx.activate(true);
            let _ = handle.update(cx, |_, window, _| {
                window.activate_window();
            });
            state.update(cx, |s, cx| {
                s.active_window = Some(handle);
                s.phase = Phase::Active;
                s.transition_count += 1;
                s.pending_wake_start = Some((wake_start, from_phase));
                s.last_snapshot = snapshot_now();
                s.log(format!("Opened window via {reason} (from {:?})", from_phase));
                cx.notify();
            });
        }
        Err(e) => {
            state.update(cx, |s, cx| {
                s.log(format!("ERROR opening window: {e:#}"));
                cx.notify();
            });
        }
    }
}

fn dismiss_to_warm_standby(state: &Entity<ResidencyState>, cx: &mut App) {
    // Destroy window if still open.
    if let Some(handle) = state.update(cx, |s, _| s.active_window.take()) {
        let _ = handle.update(cx, |_, window, cx| {
            let bounds = window.window_bounds();
            state.update(cx, |s, _| s.saved_bounds = Some(bounds));
            window.remove_window();
        });
    }

    let grace_secs = state.read(cx).grace_secs;
    let state_weak = state.downgrade();
    let timer = if grace_secs > 0 {
        Some(cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_secs(grace_secs))
                .await;
            let _ = cx.update(|cx| {
                if let Some(st) = state_weak.upgrade() {
                    if st.read(cx).phase == Phase::WarmStandby {
                        enter_deep_sleep_headless(&st, cx);
                    }
                }
            });
        }))
    } else {
        None
    };

    state.update(cx, |s, _| {
        s.phase = Phase::WarmStandby;
        s.transition_count += 1;
        s.sleep_timer = timer;
        s.last_snapshot = snapshot_now();
        s.log(format!(
            "Entered Warm Standby (grace {}s, RSS {:.1} MB, VRAM {:.1} MB, threads {})",
            grace_secs,
            s.last_snapshot.working_set_mb,
            s.last_snapshot.gpu_dedicated_mb,
            s.last_snapshot.thread_count
        ));
    });
}

fn enter_deep_sleep_headless(state: &Entity<ResidencyState>, cx: &mut App) {
    // Ensure zero windows exist before detaching GPU.
    if let Some(handle) = state.update(cx, |s, _| s.active_window.take()) {
        let _ = handle.update(cx, |_, window, cx| {
            let bounds = window.window_bounds();
            state.update(cx, |s, _| s.saved_bounds = Some(bounds));
            window.remove_window();
        });
    }

    state.update(cx, |s, _| {
        s.sleep_timer = None;
    });

    if cx.graphical_environment().is_none() {
        state.update(cx, |s, _| {
            s.phase = Phase::DeepSleepHeadless;
        });
        return;
    }

    let task = cx.request_windowing(WindowingRequest::Headless);
    let state_weak = state.downgrade();
    cx.spawn(async move |cx| {
        let res = task.await;
        let _ = cx.update(|cx| {
            if let Some(st) = state_weak.upgrade() {
                st.update(cx, |s, _| match res {
                    Ok(()) => {
                        s.phase = Phase::DeepSleepHeadless;
                        s.transition_count += 1;
                        s.last_snapshot = snapshot_now();
                        s.log(format!(
                            "Entered Deep Sleep Headless (detach_gpu ok, RSS {:.1} MB, VRAM {:.1} MB, threads {})",
                            s.last_snapshot.working_set_mb,
                            s.last_snapshot.gpu_dedicated_mb,
                            s.last_snapshot.thread_count
                        ));
                    }
                    Err(err) => {
                        s.log(format!("request_windowing(Headless) err: {err:#}"));
                    }
                });
            }
        });
    })
    .detach();
}

fn smart_toggle(state: &Entity<ResidencyState>, reason: &'static str, cx: &mut App) {
    let active_win = state.read(cx).active_window;
    let is_foreground = active_win
        .and_then(|h| h.update(cx, |_, window, _| window.is_window_active()).ok())
        .unwrap_or(false);

    if is_foreground {
        dismiss_to_warm_standby(state, cx);
    } else {
        open_or_focus_window(state, None, reason, cx);
    }
}

fn ordered_quit(
    state: &Entity<ResidencyState>,
    handles: &Rc<RefCell<Option<TrayAndHotkeyHandles>>>,
    cx: &mut App,
) {
    eprintln!("[proto] Ordered quit starting (Zed #66382 guardrail)...");
    state.update(cx, |s, _| {
        s.sleep_timer = None;
    });
    if let Some(handle) = state.update(cx, |s, _| s.active_window.take()) {
        let _ = handle.update(cx, |_, window, _| {
            window.remove_window();
        });
    }
    // Drop tray icon & global hotkey handles explicitly before cx.quit().
    handles.borrow_mut().take();
    eprintln!("[proto] Handles dropped, calling cx.quit()");
    cx.quit();
}

async fn sample_phase_metrics(
    cx: &mut AsyncApp,
    label: &str,
    settle_ms: u64,
    sample_secs: u64,
) -> (ResourceSnapshot, f64, f64) {
    cx.background_executor()
        .timer(Duration::from_millis(settle_ms))
        .await;
    let start_snap = snapshot_now();
    let t0 = Instant::now();
    cx.background_executor()
        .timer(Duration::from_secs(sample_secs))
        .await;
    let end_snap = snapshot_now();
    let elapsed = t0.elapsed().as_secs_f64();
    let (cpu_1c, cpu_tot) = compute_cpu_pct(start_snap.cpu_time_100ns, end_snap.cpu_time_100ns, elapsed);
    println!(
        "BENCH_PHASE|{label}|cpu_1core={cpu_1c:.3}%|cpu_total={cpu_tot:.3}%|rss_mb={:.2}|private_mb={:.2}|vram_ded_mb={:.2}|vram_shared_mb={:.2}|threads={}",
        end_snap.working_set_mb,
        end_snap.private_mb,
        end_snap.gpu_dedicated_mb,
        end_snap.gpu_shared_mb,
        end_snap.thread_count
    );
    (end_snap, cpu_1c, cpu_tot)
}

async fn wait_for_wake(
    state: &Entity<ResidencyState>,
    reason: &'static str,
    cx: &mut AsyncApp,
) -> f64 {
    let (tx, rx) = async_channel::bounded::<f64>(1);
    let _ = cx.update(|cx| {
        state.update(cx, |s, _| {
            s.wake_notify_tx = Some(tx);
        });
        open_or_focus_window(state, None, reason, cx);
    });
    rx.recv().await.unwrap_or(-1.0)
}

fn spawn_stress_task(
    state: Entity<ResidencyState>,
    handles: Rc<RefCell<Option<TrayAndHotkeyHandles>>>,
    cycles: usize,
    quit_after: bool,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        eprintln!("[proto] Starting stress loop: {cycles} cycles...");
        let mut warm_latencies = Vec::new();
        let mut deep_latencies = Vec::new();

        for i in 1..=cycles {
            // 1) Active -> Warm Standby -> Wake
            let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            let w_ms = wait_for_wake(&state, "stress-warm-wake", cx).await;
            warm_latencies.push(w_ms);
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;

            // 2) Active -> Warm Standby -> Deep Sleep (Headless) -> Wake
            let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let _ = cx.update(|cx| enter_deep_sleep_headless(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(180))
                .await;
            let d_ms = wait_for_wake(&state, "stress-deep-wake", cx).await;
            deep_latencies.push(d_ms);
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;

            if i % 5 == 0 || i == cycles {
                let snap = snapshot_now();
                println!(
                    "STRESS_PROGRESS|cycle={i}/{cycles}|warm_last={w_ms:.2}ms|deep_last={d_ms:.2}ms|rss_mb={:.2}|vram_mb={:.2}|threads={}",
                    snap.working_set_mb, snap.gpu_dedicated_mb, snap.thread_count
                );
            }
        }

        // Verify authoritative state survived intact across all cycles.
        let tab_ok = cx.update(|cx| {
            let s = state.read(cx);
            s.tabs.len() >= 3 && s.tabs[0].payload.contains("preserved")
        });

        println!(
            "STRESS_SUMMARY|cycles={cycles}|state_preserved={tab_ok}|warm_p50={:.2}ms|warm_p95={:.2}ms|warm_max={:.2}ms|deep_p50={:.2}ms|deep_p95={:.2}ms|deep_max={:.2}ms",
            percentile(&warm_latencies, 50.0),
            percentile(&warm_latencies, 95.0),
            percentile(&warm_latencies, 100.0),
            percentile(&deep_latencies, 50.0),
            percentile(&deep_latencies, 95.0),
            percentile(&deep_latencies, 100.0),
        );

        if quit_after {
            let _ = cx.update(|cx| ordered_quit(&state, &handles, cx));
        }
    })
    .detach();
}

fn spawn_benchmark_suite(
    state: Entity<ResidencyState>,
    handles: Rc<RefCell<Option<TrayAndHotkeyHandles>>>,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        println!("BENCH_START|pid={}", std::process::id());
        // Disable automatic grace timer interference during controlled benchmark phases.
        let _ = cx.update(|cx| {
            state.update(cx, |s, _| s.grace_secs = 0);
        });

        // Phase 1: Active idle measurement
        let _ = sample_phase_metrics(cx, "active_idle", 1500, 4).await;

        // Phase 2: Warm Standby measurement
        let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
        let _ = sample_phase_metrics(cx, "warm_standby", 1500, 5).await;

        // Phase 3: Deep Sleep (Headless) measurement
        let _ = cx.update(|cx| enter_deep_sleep_headless(&state, cx));
        let _ = sample_phase_metrics(cx, "deep_sleep_headless", 1500, 6).await;

        // Phase 4: Second-instance IPC wake from Deep Sleep (spawns self with --show)
        let exe = std::env::current_exe().expect("current_exe");
        let ipc_t0 = Instant::now();
        let (tx, rx) = async_channel::bounded::<f64>(1);
        let _ = cx.update(|cx| {
            state.update(cx, |s, _| s.wake_notify_tx = Some(tx));
        });
        let exe_clone = exe.clone();
        let child_out = cx
            .background_executor()
            .spawn(async move {
                std::process::Command::new(exe_clone)
                    .arg("--show")
                    .output()
            })
            .await;
        let ipc_wake_ms = rx.recv().await.unwrap_or(-1.0);
        let ipc_total_ms = ipc_t0.elapsed().as_secs_f64() * 1000.0;
        let (ipc_exit_ok, ipc_stdout) = match child_out {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
            ),
            Err(e) => (false, format!("spawn err: {e}")),
        };
        let is_focused_after_ipc = cx.update(|cx| {
            state
                .read(cx)
                .active_window
                .and_then(|h| h.update(cx, |_, w, _| w.is_window_active()).ok())
                .unwrap_or(false)
        });
        println!(
            "BENCH_IPC_WAKE|exit_ok={ipc_exit_ok}|focused={is_focused_after_ipc}|window_wake_ms={ipc_wake_ms:.2}|end_to_end_ms={ipc_total_ms:.2}|child_msg={ipc_stdout}"
        );

        // Phase 5: 20-cycle high-frequency lifecycle stress test (20 warm wakes + 20 deep wakes = 40 rebuilds)
        let mut warm_latencies = Vec::with_capacity(20);
        let mut deep_latencies = Vec::with_capacity(20);
        for i in 1..=20 {
            let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            warm_latencies.push(wait_for_wake(&state, "bench-warm-wake", cx).await);
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;

            let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let _ = cx.update(|cx| enter_deep_sleep_headless(&state, cx));
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            deep_latencies.push(wait_for_wake(&state, "bench-deep-wake", cx).await);
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;

            if i % 5 == 0 {
                let s = snapshot_now();
                println!(
                    "BENCH_STRESS_TICK|cycle={i}/20|rss_mb={:.2}|private_mb={:.2}|vram_ded_mb={:.2}|threads={}",
                    s.working_set_mb, s.private_mb, s.gpu_dedicated_mb, s.thread_count
                );
            }
        }

        // Post-stress Deep Sleep memory leak check
        let _ = cx.update(|cx| dismiss_to_warm_standby(&state, cx));
        cx.background_executor()
            .timer(Duration::from_millis(100))
            .await;
        let _ = cx.update(|cx| enter_deep_sleep_headless(&state, cx));
        let _ = sample_phase_metrics(cx, "post_stress_deep_sleep", 1500, 4).await;

        let state_ok = cx.update(|cx| {
            let s = state.read(cx);
            s.tabs.len() == 3 && s.tabs[0].payload.contains("preserved")
        });

        println!(
            "BENCH_LATENCY|state_preserved={state_ok}|warm_n={}|warm_p50={:.2}ms|warm_p95={:.2}ms|warm_max={:.2}ms|deep_n={}|deep_p50={:.2}ms|deep_p95={:.2}ms|deep_max={:.2}ms",
            warm_latencies.len(),
            percentile(&warm_latencies, 50.0),
            percentile(&warm_latencies, 95.0),
            percentile(&warm_latencies, 100.0),
            deep_latencies.len(),
            percentile(&deep_latencies, 50.0),
            percentile(&deep_latencies, 95.0),
            percentile(&deep_latencies, 100.0),
        );

        println!("BENCH_DONE|calling_ordered_quit");
        let _ = cx.update(|cx| ordered_quit(&state, &handles, cx));
    })
    .detach();
}

fn main() {
    let opts = CliOptions::parse();

    // Step 1: Single-instance check BEFORE initializing GPUI (ADR-0004 fast-exit contract).
    let _instance_guard = match ipc::acquire_or_forward(opts.ipc_cmd) {
        Ok(AcquireOutcome::Primary(guard)) => guard,
        Ok(AcquireOutcome::ForwardedToExisting {
            primary_pid,
            allow_fg_ok,
        }) => {
            println!(
                "IPC_FORWARDED|cmd={:?}|primary_pid={primary_pid}|allow_fg_ok={allow_fg_ok}",
                opts.ipc_cmd
            );
            return;
        }
        Ok(AcquireOutcome::SilentNoopExisting) => {
            println!("IPC_SILENT_NOOP");
            return;
        }
        Err(e) => {
            eprintln!("[proto] Single-instance error: {e}");
            std::process::exit(1);
        }
    };

    let start_silent = opts.ipc_cmd == IpcCommand::Silent;
    let initial_windowing = if start_silent {
        WindowingRequest::Headless
    } else {
        WindowingRequest::Windowed(GraphicalEnvironment::detect())
    };

    let (sys_tx, sys_rx) = async_channel::unbounded::<SystemEvent>();
    let (ipc_tx, ipc_rx) = async_channel::unbounded::<IpcCommand>();
    ipc::spawn_ipc_server(ipc_tx);

    // Bridge IPC thread commands into the unified SystemEvent channel.
    {
        let sys_tx_ipc = sys_tx.clone();
        std::thread::Builder::new()
            .name("devtools-proto-ipc-bridge".into())
            .spawn(move || {
                while let Ok(cmd) = ipc_rx.recv_blocking() {
                    let _ = sys_tx_ipc.try_send(SystemEvent::Ipc(cmd));
                }
            })
            .expect("spawn ipc bridge");
    }

    let app = gpui_kit::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_windowing(initial_windowing);

    app.run(move |cx| {
        gpui_kit::init(cx);

        let handles_cell: Rc<RefCell<Option<TrayAndHotkeyHandles>>> = Rc::new(RefCell::new(None));
        let hotkey_status = match init_tray_and_hotkey(sys_tx.clone(), true) {
            Ok(h) => {
                let s = h.hotkey_status.clone();
                *handles_cell.borrow_mut() = Some(h);
                s
            }
            Err(e) => {
                eprintln!("[proto] Tray/hotkey init error: {e}");
                format!("Init failed: {e}")
            }
        };

        let (action_tx, action_rx) = async_channel::unbounded::<AppAction>();
        let initial_phase = if start_silent {
            Phase::DeepSleepHeadless
        } else {
            Phase::WarmStandby
        };

        let state = cx.new(|_| ResidencyState {
            phase: initial_phase,
            grace_secs: opts.grace_secs,
            hotkey_status,
            tabs: vec![
                MockTab {
                    name: "JSON 格式化与压缩",
                    payload: "{\"status\":\"preserved\",\"edits\":1}".into(),
                    edits: 1,
                },
                MockTab {
                    name: "JWT 解析与签发",
                    payload: "{\"alg\":\"HS256\",\"sub\":\"in-memory-session\"}".into(),
                    edits: 1,
                },
                MockTab {
                    name: "日期与时间戳转换",
                    payload: "1732684800 -> 2024-11-27T05:20:00Z".into(),
                    edits: 1,
                },
            ],
            active_tab: 0,
            saved_bounds: None,
            active_window: None,
            sleep_timer: None,
            transition_count: 0,
            wake_warm_ms: Vec::new(),
            wake_deep_ms: Vec::new(),
            last_snapshot: snapshot_now(),
            event_log: vec![format!("Process started (pid={})", std::process::id())],
            pending_wake_start: None,
            wake_notify_tx: None,
            action_tx: action_tx.clone(),
        });

        // Unified ForegroundExecutor consumer for OS events (tray, menu, global hotkey, IPC).
        {
            let tx = action_tx.clone();
            cx.spawn(async move |_cx| {
                while let Ok(ev) = sys_rx.recv().await {
                    let action = match ev {
                        SystemEvent::TrayLeftClick => AppAction::SmartToggle {
                            reason: "tray-left-click",
                        },
                        SystemEvent::MenuToggle => AppAction::SmartToggle {
                            reason: "tray-menu-toggle",
                        },
                        SystemEvent::MenuOpenTool(name) => AppAction::WakeWindow {
                            select_tool: Some(name),
                            reason: "tray-submenu",
                        },
                        SystemEvent::MenuDeepSleepNow => AppAction::EnterDeepSleepNow,
                        SystemEvent::MenuStressTest => AppAction::RunStress {
                            cycles: 10,
                            quit_after: false,
                        },
                        SystemEvent::MenuQuit => AppAction::QuitApp,
                        SystemEvent::GlobalHotkeyToggle => AppAction::SmartToggle {
                            reason: "global-hotkey",
                        },
                        SystemEvent::Ipc(IpcCommand::Show) => AppAction::WakeWindow {
                            select_tool: None,
                            reason: "ipc-show",
                        },
                        SystemEvent::Ipc(IpcCommand::Toggle) => AppAction::SmartToggle {
                            reason: "ipc-toggle",
                        },
                        SystemEvent::Ipc(IpcCommand::Silent) => continue,
                        SystemEvent::Ipc(IpcCommand::Quit) => AppAction::QuitApp,
                    };
                    let _ = tx.try_send(action);
                }
            })
            .detach();
        }

        // AppAction dispatcher on GPUI ForegroundExecutor.
        {
            let state_act = state.clone();
            let handles_act = handles_cell.clone();
            cx.spawn(async move |cx| {
                while let Ok(act) = action_rx.recv().await {
                    let state_c = state_act.clone();
                    let handles_c = handles_act.clone();
                    let _ = cx.update(|cx| match act {
                        AppAction::DismissToWarmStandby => dismiss_to_warm_standby(&state_c, cx),
                        AppAction::EnterDeepSleepNow => enter_deep_sleep_headless(&state_c, cx),
                        AppAction::WakeWindow {
                            select_tool,
                            reason,
                        } => open_or_focus_window(&state_c, select_tool, reason, cx),
                        AppAction::SmartToggle { reason } => smart_toggle(&state_c, reason, cx),
                        AppAction::RunStress { cycles, quit_after } => {
                            spawn_stress_task(state_c, handles_c, cycles, quit_after, cx);
                        }
                        AppAction::QuitApp => ordered_quit(&state_c, &handles_c, cx),
                    });
                }
            })
            .detach();
        }

        if !start_silent {
            open_or_focus_window(&state, None, "cold-start", cx);
        } else {
            eprintln!("[proto] Started in --silent Deep Sleep (Headless) mode.");
        }

        if opts.bench_mode {
            spawn_benchmark_suite(state.clone(), handles_cell.clone(), cx);
        } else if opts.stress_cycles > 0 {
            spawn_stress_task(
                state.clone(),
                handles_cell.clone(),
                opts.stress_cycles,
                opts.quit_after_stress,
                cx,
            );
        }
    });
}

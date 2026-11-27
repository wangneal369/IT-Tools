//! System tray (`tray-icon 0.27` + `muda 0.22` with nested submenu) and global hotkey
//! (`global-hotkey 0.10`), bridged via `async_channel` so native callbacks never borrow GPUI `App`.
//! Throwaway prototype for Issue #12.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone)]
pub enum SystemEvent {
    TrayLeftClick,
    MenuToggle,
    MenuOpenTool(&'static str),
    MenuDeepSleepNow,
    MenuStressTest,
    MenuQuit,
    GlobalHotkeyToggle,
    Ipc(crate::ipc::IpcCommand),
}

pub struct TrayAndHotkeyHandles {
    pub _tray: TrayIcon,
    pub _hotkey_manager: Option<GlobalHotKeyManager>,
    pub hotkey_status: String,
}

fn make_proto_icon() -> Icon {
    let w = 32u32;
    let h = 32u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            let border = x < 3 || x >= w - 3 || y < 3 || y >= h - 3;
            let glyph = (x >= 9 && x <= 12 && y >= 8 && y <= 23)
                || (x >= 13 && x <= 21 && (y >= 8 && y <= 11 || y >= 20 && y <= 23))
                || (x >= 20 && x <= 23 && y >= 11 && y <= 20);
            let (r, g, b, a) = if glyph {
                (245, 247, 250, 255)
            } else if border {
                (94, 158, 255, 255)
            } else {
                (26, 29, 36, 255)
            };
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = a;
        }
    }
    Icon::from_rgba(rgba, w, h).expect("create tray RGBA icon")
}

/// Must be called on the GPUI main thread (inside `Application::run`).
pub fn init_tray_and_hotkey(
    tx: async_channel::Sender<SystemEvent>,
    register_hotkey: bool,
) -> Result<TrayAndHotkeyHandles, String> {
    let menu = Menu::new();
    let item_toggle = MenuItem::with_id("toggle", "显示 / 隐藏主窗口 (Ctrl+Alt+Space)", true, None);

    // Nested submenu to exercise Windows `TrackPopupMenu` nested modal loop without re-entrancy panic.
    let sub_fav = Submenu::new("收藏工具", true);
    let fav_json = MenuItem::with_id("tool_json", "JSON 格式化与压缩", true, None);
    let fav_jwt = MenuItem::with_id("tool_jwt", "JWT 解析与签发", true, None);
    let fav_ts = MenuItem::with_id("tool_ts", "日期与时间戳转换", true, None);
    let _ = sub_fav.append_items(&[&fav_json, &fav_jwt, &fav_ts]);

    let item_sleep = MenuItem::with_id("deep_sleep", "立即进入深休眠 (Headless)", true, None);
    let item_stress = MenuItem::with_id("stress", "运行 10 轮生命周期压力测试", true, None);
    let item_quit = MenuItem::with_id("quit", "彻底退出", true, None);

    let sep1 = PredefinedMenuItem::separator();
    let sep2 = PredefinedMenuItem::separator();
    menu.append_items(&[
        &item_toggle,
        &sub_fav,
        &sep1,
        &item_sleep,
        &item_stress,
        &sep2,
        &item_quit,
    ])
    .map_err(|e| format!("menu append failed: {e}"))?;

    // Set non-blocking callback handlers (never borrow GPUI App directly!).
    {
        let tx = tx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tx.try_send(SystemEvent::TrayLeftClick);
            }
        }));
    }
    {
        let tx = tx.clone();
        MenuEvent::set_event_handler(Some(std::sync::Arc::new(move |event: MenuEvent| {
            let mapped = match event.id().0.as_str() {
                "toggle" => Some(SystemEvent::MenuToggle),
                "tool_json" => Some(SystemEvent::MenuOpenTool("JSON 格式化与压缩")),
                "tool_jwt" => Some(SystemEvent::MenuOpenTool("JWT 解析与签发")),
                "tool_ts" => Some(SystemEvent::MenuOpenTool("日期与时间戳转换")),
                "deep_sleep" => Some(SystemEvent::MenuDeepSleepNow),
                "stress" => Some(SystemEvent::MenuStressTest),
                "quit" => Some(SystemEvent::MenuQuit),
                _ => None,
            };
            if let Some(ev) = mapped {
                let _ = tx.try_send(ev);
            }
        })));
    }

    let tray = TrayIconBuilder::new()
        .with_tooltip("DevTools Residency Prototype (#12)")
        .with_icon(make_proto_icon())
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|e| format!("TrayIconBuilder failed: {e}"))?;

    // Register Ctrl+Alt+Space global hotkey (optional in automated bench if desired).
    let (hotkey_manager, hotkey_status) = if register_hotkey {
        match GlobalHotKeyManager::new() {
            Ok(mgr) => {
                let hk = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
                let hk_id = hk.id();
                match mgr.register(hk) {
                    Ok(()) => {
                        let tx_hk = tx.clone();
                        GlobalHotKeyEvent::set_event_handler(Some(move |ev: GlobalHotKeyEvent| {
                            if ev.id == hk_id && ev.state == HotKeyState::Pressed {
                                let _ = tx_hk.try_send(SystemEvent::GlobalHotkeyToggle);
                            }
                        }));
                        (Some(mgr), "Ctrl+Alt+Space registered".to_string())
                    }
                    Err(e) => (
                        Some(mgr),
                        format!("AlreadyRegistered/Error (non-fatal): {e}"),
                    ),
                }
            }
            Err(e) => (None, format!("GlobalHotKeyManager init failed: {e}")),
        }
    } else {
        (None, "Disabled by CLI flag".to_string())
    };

    Ok(TrayAndHotkeyHandles {
        _tray: tray,
        _hotkey_manager: hotkey_manager,
        hotkey_status,
    })
}

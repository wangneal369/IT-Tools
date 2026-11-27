//! System tray (spec §3.1).
//!
//! Windows/macOS use `tray-icon` + `muda`; Linux uses the same crate with the
//! pure-Rust `ksni` (StatusNotifierItem) backend, never GTK3/libappindicator.
//! Native callbacks only push onto the platform command channel (ADR-0002):
//! they never borrow GPUI state from inside a native modal loop.

use std::sync::Arc;

use async_channel::Sender;
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::bridge::PlatformCommand;

const MENU_TOGGLE: &str = "devtools.toggle";
const MENU_SETTINGS: &str = "devtools.settings";
const MENU_QUIT: &str = "devtools.quit";

#[derive(Debug, thiserror::Error)]
pub enum TrayError {
    #[error("tray menu setup failed: {0}")]
    Menu(String),
    #[error("tray icon creation failed: {0}")]
    Icon(String),
}

/// Owns the tray icon; dropping (or [`TrayHandle::shutdown`]) removes it from
/// the tray before the application exits (Zed #66382 guardrail).
pub struct TrayHandle {
    tray: Option<TrayIcon>,
}

impl TrayHandle {
    pub fn shutdown(&mut self) {
        if let Some(tray) = self.tray.take() {
            let _ = tray.set_visible(false);
            drop(tray);
        }
    }
}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Build the tray icon and wire its menu/click events into `sender`.
///
/// Must be called on the UI main thread.
pub fn build(sender: Sender<PlatformCommand>) -> Result<TrayHandle, TrayError> {
    let menu = Menu::new();
    let toggle = MenuItem::with_id(MENU_TOGGLE, "打开 / 隐藏 DevTools", true, None);
    let settings = MenuItem::with_id(MENU_SETTINGS, "设置...", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit = MenuItem::with_id(MENU_QUIT, "退出 DevTools", true, None);

    menu.append_items(&[&toggle, &settings, &separator, &quit])
        .map_err(|err| TrayError::Menu(err.to_string()))?;

    {
        let sender = sender.clone();
        MenuEvent::set_event_handler(Some(Arc::new(move |event: MenuEvent| {
            let command = match event.id().0.as_str() {
                MENU_TOGGLE => Some(PlatformCommand::Toggle),
                MENU_SETTINGS => Some(PlatformCommand::OpenSettings),
                MENU_QUIT => Some(PlatformCommand::Quit),
                _ => None,
            };
            if let Some(command) = command {
                let _ = sender.try_send(command);
            }
        })));
    }

    {
        let sender = sender.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = sender.try_send(PlatformCommand::Toggle);
            }
        }));
    }

    let tray = TrayIconBuilder::new()
        .with_tooltip("DevTools")
        .with_icon(devtools_icon()?)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
        .map_err(|err| TrayError::Icon(err.to_string()))?;

    Ok(TrayHandle { tray: Some(tray) })
}

/// A 32×32 in-memory icon (accent frame + `{}` glyph), avoiding any image
/// decoding dependency and any asset lookup at runtime.
fn devtools_icon() -> Result<Icon, TrayError> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let index = ((y * SIZE + x) * 4) as usize;
            let frame = !(3..SIZE - 3).contains(&x) || !(3..SIZE - 3).contains(&y);
            let left_brace = (9..=12).contains(&x) && (9..=22).contains(&y) && x + y >= 20;
            let right_brace = (19..=22).contains(&x) && (9..=22).contains(&y) && x + y <= 43;
            let glyph = left_brace || right_brace;
            let (r, g, b) = if glyph {
                (245, 247, 250)
            } else if frame {
                (94, 158, 255)
            } else {
                (26, 29, 36)
            };
            rgba[index] = r;
            rgba[index + 1] = g;
            rgba[index + 2] = b;
            rgba[index + 3] = 255;
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).map_err(|err| TrayError::Icon(err.to_string()))
}

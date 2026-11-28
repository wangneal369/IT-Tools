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

const TRAY_ICON_SIZE: u32 = 64;
const TRAY_ICON_RGBA: &[u8] = include_bytes!("../../../assets/icons/tray-64.rgba");

/// Embedded 64×64 pre-rendered RGBA app icon (`assets/icons/devtools.svg`),
/// keeping crisp HiDPI rendering without any runtime image decoder dependency.
fn devtools_icon() -> Result<Icon, TrayError> {
    Icon::from_rgba(TRAY_ICON_RGBA.to_vec(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
        .map_err(|err| TrayError::Icon(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{TRAY_ICON_RGBA, TRAY_ICON_SIZE};

    #[test]
    fn embedded_tray_icon_has_expected_rgba_dimensions_and_alpha() {
        let expected_len = (TRAY_ICON_SIZE * TRAY_ICON_SIZE * 4) as usize;
        assert_eq!(TRAY_ICON_RGBA.len(), expected_len);
        // Rounded squircle corners must stay transparent.
        assert_eq!(TRAY_ICON_RGBA[3], 0);
        // Center pixel must be opaque.
        let center = (((TRAY_ICON_SIZE / 2) * TRAY_ICON_SIZE + (TRAY_ICON_SIZE / 2)) * 4) as usize;
        assert_eq!(TRAY_ICON_RGBA[center + 3], 255);
    }
}

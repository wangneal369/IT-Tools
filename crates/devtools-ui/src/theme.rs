//! Warm-charcoal dark / soft-paper light palettes (spec §3.2).
//!
//! `Theme::System` follows the window appearance reported by the platform.

use devtools_core::Theme;
use gpui_kit::{Window, WindowAppearance};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: u32,
    pub rail: u32,
    pub panel: u32,
    pub panel_alt: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_soft: u32,
    pub danger: u32,
}

pub const DARK: Palette = Palette {
    bg: 0x141517,
    rail: 0x181a1e,
    panel: 0x1c1e22,
    panel_alt: 0x111214,
    border: 0x2a2d34,
    text: 0xe8eaed,
    muted: 0x9aa0a6,
    accent: 0x5e9eff,
    accent_soft: 0x273449,
    danger: 0xe5676f,
};

pub const LIGHT: Palette = Palette {
    bg: 0xf6f5f2,
    rail: 0xefeee9,
    panel: 0xffffff,
    panel_alt: 0xfbfaf7,
    border: 0xd9d6cd,
    text: 0x23262b,
    muted: 0x6b7078,
    accent: 0x2f6fd0,
    accent_soft: 0xdde8fa,
    danger: 0xc0392b,
};

/// Resolve the configured theme against the current window appearance.
pub fn resolve(theme: Theme, window: &Window) -> Palette {
    match theme {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
        Theme::System => match window.appearance() {
            WindowAppearance::Light | WindowAppearance::VibrantLight => LIGHT,
            _ => DARK,
        },
    }
}

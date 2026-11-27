//! Warm-charcoal dark / soft-paper light palettes (spec §3.2) and GPUI
//! component theme synchronization.
//!
//! `Theme::System` follows the window appearance reported by the platform, and
//! [`sync_gpui_theme`] keeps `gpui_kit::component::theme::Theme` (`Editor`,
//! `Input`, `ColorPicker`, scrollbars, and `mono_font_size`) aligned with the
//! workbench shell.

use std::sync::Arc;

use devtools_core::{AppSettings, Theme};
use gpui_kit::component::theme::ThemeMode;
use gpui_kit::{App, Hsla, Window, WindowAppearance, px, rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub is_dark: bool,
    pub bg: u32,
    pub chrome: u32,
    pub rail: u32,
    pub panel: u32,
    pub panel_alt: u32,
    pub hover: u32,
    pub active: u32,
    pub border: u32,
    pub border_strong: u32,
    pub text: u32,
    pub muted: u32,
    pub subtle: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_soft: u32,
    pub success: u32,
    pub success_soft: u32,
    pub warning: u32,
    pub warning_soft: u32,
    pub danger: u32,
    pub danger_soft: u32,
}

pub const DARK: Palette = Palette {
    is_dark: true,
    bg: 0x1a1c20,
    chrome: 0x141619,
    rail: 0x15171b,
    panel: 0x202329,
    panel_alt: 0x16181c,
    hover: 0x272b33,
    active: 0x2e333d,
    border: 0x2a2e37,
    border_strong: 0x393f4b,
    text: 0xe6e8ec,
    muted: 0x9aa0ab,
    subtle: 0x6b7280,
    accent: 0x3b82f6,
    accent_hover: 0x5292f8,
    accent_soft: 0x1e2d45,
    success: 0x3fb950,
    success_soft: 0x182d20,
    warning: 0xd4a04a,
    warning_soft: 0x332815,
    danger: 0xe5534b,
    danger_soft: 0x361c1e,
};

pub const LIGHT: Palette = Palette {
    is_dark: false,
    bg: 0xf6f7f9,
    chrome: 0xeeeff3,
    rail: 0xf1f2f6,
    panel: 0xffffff,
    panel_alt: 0xf2f4f8,
    hover: 0xe9edf4,
    active: 0xdfe5f0,
    border: 0xe1e5ec,
    border_strong: 0xcbd2df,
    text: 0x1f2328,
    muted: 0x57606a,
    subtle: 0x86909c,
    accent: 0x2563eb,
    accent_hover: 0x1d4ed8,
    accent_soft: 0xe4edff,
    success: 0x1a7f37,
    success_soft: 0xdcfce7,
    warning: 0x9a6700,
    warning_soft: 0xfef3c7,
    danger: 0xcf222e,
    danger_soft: 0xfee2e2,
};

/// Resolve the configured theme against a platform [`WindowAppearance`].
pub fn resolve_with_appearance(theme: Theme, appearance: WindowAppearance) -> Palette {
    match theme {
        Theme::Dark => DARK,
        Theme::Light => LIGHT,
        Theme::System => match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => LIGHT,
            _ => DARK,
        },
    }
}

/// Resolve the configured theme against the current window appearance.
pub fn resolve(theme: Theme, window: &Window) -> Palette {
    resolve_with_appearance(theme, window.appearance())
}

/// Synchronize `gpui_kit::component::theme::Theme` with [`AppSettings`] so
/// `Editor`, `Input`, `ColorPicker`, and scrollbars match the shell's dark/light
/// surface palette and configured `mono_font_size`.
pub fn sync_gpui_theme(settings: &AppSettings, appearance: Option<WindowAppearance>, cx: &mut App) {
    if !cx.has_global::<gpui_kit::component::theme::Theme>() {
        return;
    }
    let effective = appearance.unwrap_or_else(|| cx.window_appearance());
    let palette = resolve_with_appearance(settings.theme, effective);
    let desired_mode = if palette.is_dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    let desired_mono = px(f32::from(settings.editor.font_size));

    if gpui_kit::component::theme::Theme::global(cx).mode != desired_mode {
        gpui_kit::component::theme::Theme::change(desired_mode, None, cx);
    }

    gpui_kit::component::theme::Theme::update(cx, |theme| {
        theme.font_size = px(13.0);
        theme.mono_font_size = desired_mono;
        if let Some(family) = &settings.editor.monospace_font
            && !family.trim().is_empty()
        {
            theme.mono_font_family = family.clone().into();
        }
        theme.radius = px(6.0);
        theme.radius_lg = px(8.0);
        theme.focus_ring = false;

        theme.background = rgb(palette.panel).into();
        theme.foreground = rgb(palette.text).into();
        theme.popover = rgb(palette.panel).into();
        theme.popover_foreground = rgb(palette.text).into();
        theme.border = rgb(palette.border).into();
        theme.input = rgb(palette.border_strong).into();
        theme.ring = rgb(palette.accent).into();
        theme.caret = rgb(palette.accent).into();
        theme.primary = rgb(palette.accent).into();
        theme.primary_foreground = rgb(0xffffff).into();
        theme.secondary = rgb(palette.panel_alt).into();
        theme.secondary_foreground = rgb(palette.text).into();
        theme.muted = rgb(palette.panel_alt).into();
        theme.muted_foreground = rgb(palette.muted).into();
        theme.accent = rgb(palette.hover).into();
        theme.accent_foreground = rgb(palette.text).into();
        theme.danger = rgb(palette.danger).into();
        theme.danger_foreground = rgb(0xffffff).into();
        theme.scrollbar = Hsla::transparent_black();

        let mut highlight = (*theme.highlight_theme).clone();
        highlight.style.editor_background = Some(rgb(palette.panel).into());
        theme.highlight_theme = Arc::new(highlight);
    });
}

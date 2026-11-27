//! Warm-charcoal dark / soft-paper light palettes (spec §3.2) and GPUI
//! component theme synchronization.
//!
//! `Theme::System` follows the window appearance reported by the platform, and
//! [`sync_gpui_theme`] keeps `gpui_kit::component::theme::Theme` (`Editor`,
//! `Input`, `ColorPicker`, scrollbars, and `mono_font_size`) aligned with the
//! workbench shell.

use std::sync::{Arc, OnceLock};

use devtools_core::{AppSettings, Theme};
use gpui_kit::component::highlighter::HighlightTheme;
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
        theme.highlight_theme = highlight_theme(palette.is_dark);
    });
}

/// Curated warm-charcoal Dark and soft-paper Light syntax highlighting themes
/// tuned for DevTools' surface palette (`#202329` / `#ffffff`) and V1 languages
/// (JSON, YAML, TOML, SQL, HTML/XML, Diff, Markdown, CSS).
pub fn highlight_theme(is_dark: bool) -> Arc<HighlightTheme> {
    if is_dark {
        static DARK_SYNTAX: OnceLock<Arc<HighlightTheme>> = OnceLock::new();
        DARK_SYNTAX
            .get_or_init(|| {
                let parsed: HighlightTheme = serde_json::from_str(DARK_HIGHLIGHT_JSON)
                    .expect("bundled dark syntax highlight theme must be valid JSON");
                Arc::new(parsed)
            })
            .clone()
    } else {
        static LIGHT_SYNTAX: OnceLock<Arc<HighlightTheme>> = OnceLock::new();
        LIGHT_SYNTAX
            .get_or_init(|| {
                let parsed: HighlightTheme = serde_json::from_str(LIGHT_HIGHLIGHT_JSON)
                    .expect("bundled light syntax highlight theme must be valid JSON");
                Arc::new(parsed)
            })
            .clone()
    }
}

const DARK_HIGHLIGHT_JSON: &str = r##"{
  "name": "DevTools Warm Charcoal",
  "appearance": "dark",
  "style": {
    "editor.foreground": "#e6e8ec",
    "editor.background": "#202329",
    "editor.gutter.background": "#1b1e24",
    "editor.active_line.background": "#262b33",
    "editor.line_number": "#6b7280",
    "editor.active_line_number": "#e6e8ec",
    "editor.invisible": "#6b728066",
    "conflict": "#e5534b",
    "created": "#3fb950",
    "created.background": "#182d20",
    "deleted": "#f87171",
    "deleted.background": "#361c1e",
    "error": "#e5534b",
    "error.background": "#361c1e",
    "error.border": "#e5534b",
    "hint": "#93c5fd",
    "modified": "#d4a04a",
    "warning": "#d4a04a",
    "syntax": {
      "attribute": { "color": "#fbbf24" },
      "boolean": { "color": "#fb923c" },
      "comment": { "color": "#6b7280", "font_style": "italic" },
      "comment.doc": { "color": "#7b8494", "font_style": "italic" },
      "constant": { "color": "#fb923c" },
      "constructor": { "color": "#f472b6" },
      "embedded": { "color": "#e6e8ec" },
      "emphasis": { "font_style": "italic" },
      "emphasis.strong": { "font_weight": 700 },
      "enum": { "color": "#f472b6" },
      "function": { "color": "#60a5fa" },
      "keyword": { "color": "#c084fc" },
      "label": { "color": "#f472b6" },
      "link_text": { "color": "#60a5fa" },
      "link_uri": { "color": "#7dd3fc", "font_style": "italic" },
      "number": { "color": "#fbbf24" },
      "operator": { "color": "#94a3b8" },
      "preproc": { "color": "#c084fc" },
      "property": { "color": "#7dd3fc" },
      "punctuation": { "color": "#8b95a7" },
      "punctuation.bracket": { "color": "#9aa4b6" },
      "punctuation.delimiter": { "color": "#8b95a7" },
      "punctuation.list_marker": { "color": "#60a5fa" },
      "punctuation.special": { "color": "#c084fc" },
      "string": { "color": "#86efac" },
      "string.escape": { "color": "#34d399" },
      "string.regex": { "color": "#34d399" },
      "string.special": { "color": "#fde047" },
      "string.special.symbol": { "color": "#fbbf24" },
      "tag": { "color": "#60a5fa" },
      "tag.doctype": { "color": "#94a3b8" },
      "text.code.span": { "color": "#86efac" },
      "text.literal": { "color": "#86efac" },
      "title": { "color": "#7dd3fc", "font_weight": 600 },
      "type": { "color": "#f472b6" },
      "variable": { "color": "#e6e8ec" },
      "variable.special": { "color": "#f472b6" }
    }
  }
}"##;

const LIGHT_HIGHLIGHT_JSON: &str = r##"{
  "name": "DevTools Soft Paper",
  "appearance": "light",
  "style": {
    "editor.foreground": "#1f2328",
    "editor.background": "#ffffff",
    "editor.gutter.background": "#f8f9fb",
    "editor.active_line.background": "#f1f5f9",
    "editor.line_number": "#86909c",
    "editor.active_line_number": "#1f2328",
    "editor.invisible": "#86909c66",
    "conflict": "#cf222e",
    "created": "#1a7f37",
    "created.background": "#dcfce7",
    "deleted": "#cf222e",
    "deleted.background": "#fee2e2",
    "error": "#cf222e",
    "error.background": "#fee2e2",
    "error.border": "#cf222e",
    "hint": "#2563eb",
    "modified": "#9a6700",
    "warning": "#9a6700",
    "syntax": {
      "attribute": { "color": "#8250df" },
      "boolean": { "color": "#cf222e" },
      "comment": { "color": "#6e7781", "font_style": "italic" },
      "comment.doc": { "color": "#6e7781", "font_style": "italic" },
      "constant": { "color": "#cf222e" },
      "constructor": { "color": "#953800" },
      "embedded": { "color": "#1f2328" },
      "emphasis": { "font_style": "italic" },
      "emphasis.strong": { "font_weight": 700 },
      "enum": { "color": "#953800" },
      "function": { "color": "#0550ae" },
      "keyword": { "color": "#8250df" },
      "label": { "color": "#8250df" },
      "link_text": { "color": "#0550ae" },
      "link_uri": { "color": "#0969da", "font_style": "italic" },
      "number": { "color": "#953800" },
      "operator": { "color": "#57606a" },
      "preproc": { "color": "#8250df" },
      "property": { "color": "#0550ae" },
      "punctuation": { "color": "#57606a" },
      "punctuation.bracket": { "color": "#424a53" },
      "punctuation.delimiter": { "color": "#57606a" },
      "punctuation.list_marker": { "color": "#0550ae" },
      "punctuation.special": { "color": "#8250df" },
      "string": { "color": "#116329" },
      "string.escape": { "color": "#0a7f3f" },
      "string.regex": { "color": "#0a7f3f" },
      "string.special": { "color": "#953800" },
      "string.special.symbol": { "color": "#953800" },
      "tag": { "color": "#116329" },
      "tag.doctype": { "color": "#6e7781" },
      "text.code.span": { "color": "#8250df" },
      "text.literal": { "color": "#116329" },
      "title": { "color": "#0550ae", "font_weight": 600 },
      "type": { "color": "#953800" },
      "variable": { "color": "#1f2328" },
      "variable.special": { "color": "#cf222e" }
    }
  }
}"##;

#[cfg(test)]
mod tests {
    use super::highlight_theme;
    use gpui_kit::component::theme::ThemeMode;

    #[test]
    fn dark_and_light_syntax_themes_load_with_core_token_colors() {
        for (is_dark, expected_mode) in [(true, ThemeMode::Dark), (false, ThemeMode::Light)] {
            let theme = highlight_theme(is_dark);
            assert_eq!(theme.appearance, expected_mode);
            assert!(theme.style.editor_background.is_some());
            assert!(theme.style.editor_gutter_background.is_some());
            for token in [
                "property",
                "string",
                "number",
                "boolean",
                "keyword",
                "tag",
                "attribute",
                "comment",
                "punctuation",
            ] {
                assert!(
                    theme.style.syntax.style(token).is_some(),
                    "missing syntax style for {token} in dark={is_dark}"
                );
            }
        }
    }
}

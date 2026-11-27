//! Global shortcut registration and live re-binding (spec §3.1).
//!
//! The shortcut string stored in `AppSettings.hotkeys.global_toggle` is parsed
//! into a `global-hotkey` binding. Registration failures (Wayland, another app
//! owning the chord, no display server) never panic and never leave the UI
//! claiming a working shortcut: they surface as [`HotkeyStatus::Degraded`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_channel::Sender;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::bridge::PlatformCommand;

/// Default global toggle on Windows/Linux (spec §3.1).
pub const DEFAULT_GLOBAL_SHORTCUT: &str = "Ctrl+Alt+Space";
/// Default global toggle on macOS (spec §3.1).
pub const DEFAULT_GLOBAL_SHORTCUT_MACOS: &str = "Control+Option+Space";

/// The platform default patch for the global toggle shortcut.
pub const fn platform_default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        DEFAULT_GLOBAL_SHORTCUT_MACOS
    } else {
        DEFAULT_GLOBAL_SHORTCUT
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutError {
    Empty,
    MissingKey,
    NoModifier,
    UnknownModifier(String),
    UnknownKey(String),
}

impl std::fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "快捷键为空"),
            Self::MissingKey => write!(f, "缺少主按键"),
            Self::NoModifier => write!(f, "至少需要一个修饰键（禁止单键 / 媒体键）"),
            Self::UnknownModifier(value) => write!(f, "未知修饰键: {value}"),
            Self::UnknownKey(value) => write!(f, "未知按键: {value}"),
        }
    }
}

impl std::error::Error for ShortcutError {}

/// A validated user-facing shortcut string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedShortcut {
    pub modifiers: Modifiers,
    pub code: Code,
}

impl ParsedShortcut {
    pub fn to_hotkey(self) -> HotKey {
        HotKey::new(Some(self.modifiers), self.code)
    }
}

/// Parse strings such as `Ctrl+Alt+Space`, `Control+Option+Space`,
/// `Cmd+Shift+K` or `Super+F5`.
pub fn parse_shortcut(value: &str) -> Result<ParsedShortcut, ShortcutError> {
    let tokens: Vec<&str> = value
        .split('+')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() {
        return Err(ShortcutError::Empty);
    }
    if tokens.len() == 1 {
        return Err(ShortcutError::NoModifier);
    }

    let (key_token, modifier_tokens) = tokens.split_last().expect("non-empty");
    let mut modifiers = Modifiers::empty();
    for token in modifier_tokens {
        let parsed = match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Modifiers::CONTROL,
            "alt" | "option" | "opt" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            "cmd" | "command" | "meta" | "super" | "win" | "system" => Modifiers::META,
            _ => return Err(ShortcutError::UnknownModifier((*token).to_owned())),
        };
        modifiers |= parsed;
    }
    if modifiers.is_empty() {
        return Err(ShortcutError::NoModifier);
    }

    let code =
        parse_code(key_token).ok_or_else(|| ShortcutError::UnknownKey((*key_token).to_owned()))?;
    Ok(ParsedShortcut { modifiers, code })
}

fn parse_code(token: &str) -> Option<Code> {
    let lower = token.to_ascii_lowercase();
    if lower.len() == 1 {
        let ch = lower.chars().next()?;
        if ch.is_ascii_alphabetic() {
            return letter_code(ch);
        }
        if ch.is_ascii_digit() {
            return digit_code(ch);
        }
    } else if let Some(number) = lower.strip_prefix('f')
        && let Ok(n) = number.parse::<u8>()
    {
        return function_code(n);
    }

    Some(match lower.as_str() {
        "space" => Code::Space,
        "tab" => Code::Tab,
        "enter" | "return" => Code::Enter,
        "esc" | "escape" => Code::Escape,
        "backspace" => Code::Backspace,
        "delete" | "del" => Code::Delete,
        "insert" | "ins" => Code::Insert,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" | "page-up" | "pgup" => Code::PageUp,
        "pagedown" | "page-down" | "pgdn" => Code::PageDown,
        "up" | "arrowup" => Code::ArrowUp,
        "down" | "arrowdown" => Code::ArrowDown,
        "left" | "arrowleft" => Code::ArrowLeft,
        "right" | "arrowright" => Code::ArrowRight,
        "-" | "minus" => Code::Minus,
        "=" | "equal" => Code::Equal,
        "," | "comma" => Code::Comma,
        "." | "period" => Code::Period,
        "/" | "slash" => Code::Slash,
        ";" | "semicolon" => Code::Semicolon,
        "'" | "quote" => Code::Quote,
        "[" | "bracketleft" => Code::BracketLeft,
        "]" | "bracketright" => Code::BracketRight,
        "\\" | "backslash" => Code::Backslash,
        "`" | "backquote" => Code::Backquote,
        _ => return None,
    })
}

fn letter_code(ch: char) -> Option<Code> {
    Some(match ch {
        'a' => Code::KeyA,
        'b' => Code::KeyB,
        'c' => Code::KeyC,
        'd' => Code::KeyD,
        'e' => Code::KeyE,
        'f' => Code::KeyF,
        'g' => Code::KeyG,
        'h' => Code::KeyH,
        'i' => Code::KeyI,
        'j' => Code::KeyJ,
        'k' => Code::KeyK,
        'l' => Code::KeyL,
        'm' => Code::KeyM,
        'n' => Code::KeyN,
        'o' => Code::KeyO,
        'p' => Code::KeyP,
        'q' => Code::KeyQ,
        'r' => Code::KeyR,
        's' => Code::KeyS,
        't' => Code::KeyT,
        'u' => Code::KeyU,
        'v' => Code::KeyV,
        'w' => Code::KeyW,
        'x' => Code::KeyX,
        'y' => Code::KeyY,
        'z' => Code::KeyZ,
        _ => return None,
    })
}

fn digit_code(ch: char) -> Option<Code> {
    Some(match ch {
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        '9' => Code::Digit9,
        _ => return None,
    })
}

fn function_code(n: u8) -> Option<Code> {
    Some(match n {
        1 => Code::F1,
        2 => Code::F2,
        3 => Code::F3,
        4 => Code::F4,
        5 => Code::F5,
        6 => Code::F6,
        7 => Code::F7,
        8 => Code::F8,
        9 => Code::F9,
        10 => Code::F10,
        11 => Code::F11,
        12 => Code::F12,
        13 => Code::F13,
        14 => Code::F14,
        15 => Code::F15,
        16 => Code::F16,
        17 => Code::F17,
        18 => Code::F18,
        19 => Code::F19,
        20 => Code::F20,
        21 => Code::F21,
        22 => Code::F22,
        23 => Code::F23,
        24 => Code::F24,
        _ => return None,
    })
}

/// Observed state of global shortcut registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyStatus {
    /// Registered and listening.
    Active { shortcut: String },
    /// User explicitly disabled the global shortcut.
    Disabled,
    /// Requested shortcut could not be registered; the reason is shown in the
    /// settings panel banner.
    Degraded { requested: String, reason: String },
}

impl HotkeyStatus {
    pub fn is_degraded(&self) -> bool {
        matches!(self, Self::Degraded { .. })
    }

    /// Short, UI-ready description.
    pub fn label(&self) -> String {
        match self {
            Self::Active { shortcut } => format!("{shortcut} 已注册"),
            Self::Disabled => "全局热键已关闭".to_owned(),
            Self::Degraded { requested, reason } => {
                format!("{requested} 注册失败：{reason}")
            }
        }
    }
}

/// Owns the platform hotkey manager and the currently registered binding.
pub struct GlobalHotkey {
    manager: Option<GlobalHotKeyManager>,
    current: Option<(String, HotKey)>,
    current_id: Arc<AtomicU32>,
    sender: Sender<PlatformCommand>,
    status: HotkeyStatus,
    last_error: Option<String>,
}

impl GlobalHotkey {
    pub fn new(sender: Sender<PlatformCommand>) -> Self {
        Self {
            manager: None,
            current: None,
            current_id: Arc::new(AtomicU32::new(0)),
            sender,
            status: HotkeyStatus::Disabled,
            last_error: None,
        }
    }

    pub fn status(&self) -> &HotkeyStatus {
        &self.status
    }

    /// Most recent registration error even when a previous binding stayed active.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Register `shortcut`, replacing the previous binding on success.
    ///
    /// On failure the previous binding stays active, `last_error` records the
    /// reason for the settings banner, and a `Degraded` status is returned so
    /// the settings UI can roll its field back.
    pub fn apply(&mut self, shortcut: Option<&str>) -> HotkeyStatus {
        let Some(shortcut) = shortcut else {
            self.unregister_current();
            self.status = HotkeyStatus::Disabled;
            self.last_error = None;
            return self.status.clone();
        };

        if let Some(reason) = wayland_degradation() {
            self.unregister_current();
            self.last_error = Some(reason.clone());
            self.status = HotkeyStatus::Degraded {
                requested: shortcut.to_owned(),
                reason,
            };
            return self.status.clone();
        }

        let parsed = match parse_shortcut(shortcut) {
            Ok(parsed) => parsed,
            Err(err) => {
                let reason = err.to_string();
                self.last_error = Some(reason.clone());
                return HotkeyStatus::Degraded {
                    requested: shortcut.to_owned(),
                    reason,
                };
            }
        };

        let hotkey = parsed.to_hotkey();
        if let Some((_, active_hotkey)) = &self.current
            && *active_hotkey == hotkey
            && !self.status.is_degraded()
        {
            // Semantically identical chord (e.g. `Control+Option+Space` vs
            // `Ctrl+Alt+Space`): keep the OS registration and refresh the label.
            self.current = Some((shortcut.to_owned(), hotkey));
            self.status = HotkeyStatus::Active {
                shortcut: shortcut.to_owned(),
            };
            self.last_error = None;
            return self.status.clone();
        }

        if self.manager.is_none() {
            match GlobalHotKeyManager::new() {
                Ok(manager) => self.manager = Some(manager),
                Err(err) => {
                    let reason = format!("热键管理器初始化失败: {err}");
                    self.last_error = Some(reason.clone());
                    return HotkeyStatus::Degraded {
                        requested: shortcut.to_owned(),
                        reason,
                    };
                }
            }
        }

        let id = hotkey.id;
        let Some(manager) = self.manager.as_ref() else {
            unreachable!("manager was just created");
        };
        if let Err(err) = manager.register(hotkey) {
            let reason = format!("已被其他程序占用或被系统拒绝: {err}");
            self.last_error = Some(reason.clone());
            return HotkeyStatus::Degraded {
                requested: shortcut.to_owned(),
                reason,
            };
        }

        self.unregister_current();

        self.current_id.store(id, Ordering::SeqCst);
        let sender = self.sender.clone();
        let current_id = self.current_id.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id == current_id.load(Ordering::SeqCst) && event.state == HotKeyState::Pressed
            {
                let _ = sender.try_send(PlatformCommand::Toggle);
            }
        }));

        self.current = Some((shortcut.to_owned(), hotkey));
        self.status = HotkeyStatus::Active {
            shortcut: shortcut.to_owned(),
        };
        self.last_error = None;
        self.status.clone()
    }

    /// Unregister the live binding (used by ordered quit).
    pub fn shutdown(&mut self) {
        self.unregister_current();
        self.manager = None;
        self.status = HotkeyStatus::Disabled;
        self.last_error = None;
    }

    fn unregister_current(&mut self) {
        if let (Some(manager), Some((_, hotkey))) = (self.manager.as_ref(), self.current.take()) {
            let _ = manager.unregister(hotkey);
        }
        self.current_id.store(0, Ordering::SeqCst);
    }
}

/// Pure Wayland session check: if `WAYLAND_DISPLAY` is set OR
/// `XDG_SESSION_TYPE=wayland`, X11 global-hotkey registration must be skipped
/// even when XWayland also sets `DISPLAY` (spec §3.1).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn detect_wayland_degradation(
    wayland_display: Option<&str>,
    session_type: Option<&str>,
) -> Option<String> {
    let has_wayland_display = wayland_display.is_some_and(|v| !v.trim().is_empty());
    let is_wayland_session = session_type.is_some_and(|v| v.trim().eq_ignore_ascii_case("wayland"));
    if has_wayland_display || is_wayland_session {
        Some(
            "Wayland 会话下 X11 全局热键不可用，请通过系统快捷键绑定 `devtools --toggle` 或使用托盘唤起"
                .to_owned(),
        )
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn wayland_degradation() -> Option<String> {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    detect_wayland_degradation(wayland.as_deref(), session.as_deref())
}

#[cfg(not(target_os = "linux"))]
fn wayland_degradation() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_friendly_shortcut_dialects() {
        let parsed = parse_shortcut("Ctrl+Alt+Space").expect("windows/linux default");
        assert_eq!(parsed.modifiers, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(parsed.code, Code::Space);

        let mac = parse_shortcut("Control+Option+Space").expect("macOS default");
        assert_eq!(mac.modifiers, Modifiers::CONTROL | Modifiers::ALT);

        let cmd = parse_shortcut("Cmd+Shift+K").expect("cmd dialect");
        assert_eq!(cmd.modifiers, Modifiers::META | Modifiers::SHIFT);
        assert_eq!(cmd.code, Code::KeyK);

        let spaced = parse_shortcut("ctrl + shift + f5").expect("loose spacing / case");
        assert_eq!(spaced.modifiers, Modifiers::CONTROL | Modifiers::SHIFT);
        assert_eq!(spaced.code, Code::F5);
    }

    #[test]
    fn rejects_single_key_and_unknown_tokens() {
        assert_eq!(parse_shortcut("F5"), Err(ShortcutError::NoModifier));
        assert_eq!(parse_shortcut("   "), Err(ShortcutError::Empty));
        assert!(matches!(
            parse_shortcut("Hyper+K"),
            Err(ShortcutError::UnknownModifier(_))
        ));
        assert!(matches!(
            parse_shortcut("Ctrl+NotAKey"),
            Err(ShortcutError::UnknownKey(_))
        ));
    }

    #[test]
    fn parsed_shortcut_builds_a_hotkey_with_a_stable_id() {
        let a = parse_shortcut("Ctrl+Alt+Space").unwrap().to_hotkey();
        let b = parse_shortcut("Ctrl+Alt+Space").unwrap().to_hotkey();
        assert_eq!(a, b);
        assert_eq!(a.mods, Modifiers::CONTROL | Modifiers::ALT);
    }

    #[test]
    fn status_labels_surface_degradation() {
        let status = HotkeyStatus::Degraded {
            requested: "Ctrl+Alt+Space".to_owned(),
            reason: "conflict".to_owned(),
        };
        assert!(status.is_degraded());
        assert!(status.label().contains("conflict"));
    }

    #[test]
    fn wayland_degrades_even_when_xwayland_sets_display() {
        assert!(detect_wayland_degradation(Some("wayland-0"), Some("wayland")).is_some());
        assert!(detect_wayland_degradation(Some("wayland-0"), Some("x11")).is_some());
        assert!(detect_wayland_degradation(None, Some("Wayland")).is_some());
        assert!(detect_wayland_degradation(None, Some("x11")).is_none());
        assert!(detect_wayland_degradation(Some(""), None).is_none());
    }

    #[test]
    fn shortcut_aliases_produce_equal_hotkeys() {
        let win = parse_shortcut("Ctrl+Alt+Space").unwrap().to_hotkey();
        let mac = parse_shortcut("Control+Option+Space").unwrap().to_hotkey();
        assert_eq!(win, mac);
    }

    #[test]
    fn invalid_rebind_records_last_error_without_panicking() {
        let (tx, _rx) = async_channel::unbounded();
        let mut hk = GlobalHotkey::new(tx);
        let status = hk.apply(Some("SingleKeyOnly"));
        assert!(status.is_degraded());
        assert!(hk.last_error().is_some());
        let cleared = hk.apply(None);
        assert_eq!(cleared, HotkeyStatus::Disabled);
        assert!(hk.last_error().is_none());
    }
}

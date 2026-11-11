//! Shortcut parsing. The stored form is the string `global-hotkey` accepts,
//! which looks like `PrintScreen` or `Meta+Shift+Digit2`.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    /// Canonical string, e.g. `Meta+Shift+Digit2`.
    pub canonical: String,
}

impl Shortcut {
    pub fn parse(input: &str) -> Result<Self, String> {
        let canonical = normalize(input);
        HotKey::from_str(&canonical).map_err(|err| format!("{err}"))?;
        Ok(Self { canonical })
    }

    pub fn global(&self) -> Result<HotKey, String> {
        HotKey::from_str(&self.canonical).map_err(|err| format!("{err}"))
    }

    pub fn display(&self) -> String {
        display_canonical(&self.canonical)
    }

    pub fn is_print_screen(&self) -> bool {
        self.canonical.split('+').any(|part| part == "PrintScreen")
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

pub fn default_shortcut() -> Shortcut {
    #[cfg(target_os = "windows")]
    let raw = "PrintScreen";
    #[cfg(target_os = "macos")]
    let raw = "Meta+Shift+Digit2";
    #[cfg(all(unix, not(target_os = "macos")))]
    let raw = "Control+Shift+Digit2";
    Shortcut::parse(raw).unwrap_or(Shortcut {
        canonical: raw.to_string(),
    })
}

fn normalize(input: &str) -> String {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    let mut key: Option<String> = None;
    for raw in input.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        let token = part.to_ascii_lowercase().replace(' ', "");
        match token.as_str() {
            "cmd" | "command" | "meta" | "super" | "win" | "windows" => meta = true,
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            "printscreen" | "prtsc" | "prtscn" | "snapshot" => key = Some("PrintScreen".into()),
            other => key = Some(canonicalize_key(other)),
        }
    }
    let mut parts = Vec::new();
    if ctrl {
        parts.push("Control".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    if meta {
        parts.push("Meta".to_string());
    }
    if let Some(key) = key {
        parts.push(key);
    }
    parts.join("+")
}

fn canonicalize_key(token: &str) -> String {
    if token.len() == 1 {
        let ch = token.chars().next().unwrap();
        if ch.is_ascii_digit() {
            return format!("Digit{ch}");
        }
        if ch.is_ascii_alphabetic() {
            return format!("Key{}", ch.to_ascii_uppercase());
        }
    }
    match token {
        "esc" | "escape" => "Escape".into(),
        "enter" | "return" => "Enter".into(),
        "space" => "Space".into(),
        "tab" => "Tab".into(),
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "minus" | "-" => "Minus".into(),
        "equal" | "=" => "Equal".into(),
        other => {
            // Digit2, KeyA, F5 already in keyboard-types form, possibly lowercased.
            if let Some(rest) = other.strip_prefix("digit") {
                format!("Digit{rest}")
            } else if let Some(rest) = other.strip_prefix("key") {
                format!("Key{}", rest.to_ascii_uppercase())
            } else if let Some(rest) = other.strip_prefix('f') {
                if rest.chars().all(|c| c.is_ascii_digit()) {
                    format!("F{rest}")
                } else {
                    other.to_string()
                }
            } else {
                other.to_string()
            }
        }
    }
}

fn display_canonical(canonical: &str) -> String {
    canonical
        .split('+')
        .map(|part| match part {
            "Meta" => {
                if cfg!(target_os = "macos") {
                    "⌘"
                } else if cfg!(target_os = "windows") {
                    "Win"
                } else {
                    "Super"
                }
            }
            "Control" => {
                if cfg!(target_os = "macos") {
                    "⌃"
                } else {
                    "Ctrl"
                }
            }
            "Alt" => {
                if cfg!(target_os = "macos") {
                    "⌥"
                } else {
                    "Alt"
                }
            }
            "Shift" => {
                if cfg!(target_os = "macos") {
                    "⇧"
                } else {
                    "Shift"
                }
            }
            "PrintScreen" => "Print Screen",
            "Digit0" | "Digit1" | "Digit2" | "Digit3" | "Digit4" | "Digit5" | "Digit6"
            | "Digit7" | "Digit8" | "Digit9" => part.trim_start_matches("Digit"),
            other => other.strip_prefix("Key").unwrap_or(other),
        })
        .collect::<Vec<_>>()
        .join(if cfg!(target_os = "macos") { "" } else { "+" })
}

pub fn shortcut_from_parts(
    key: &str,
    command: bool,
    control: bool,
    shift: bool,
    alt: bool,
) -> Result<Shortcut, String> {
    let mut parts = Vec::new();
    if control {
        parts.push("Control");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    if command && !control {
        // egui's `modifiers.command` is Cmd on macOS and Ctrl on Windows/Linux.
        if cfg!(target_os = "macos") {
            parts.push("Meta");
        } else if !parts.contains(&"Control") {
            parts.push("Control");
        }
    }
    if command && control && cfg!(target_os = "macos") {
        parts.push("Meta");
    }
    parts.push(key);
    Shortcut::parse(&parts.join("+"))
}

/// Map the keys the settings recorder understands onto `global-hotkey` codes.
pub fn code_name_from_egui(name: &str) -> Option<&'static str> {
    Some(match name {
        "PrintScreen" => "PrintScreen",
        "Escape" => "Escape",
        "Enter" => "Enter",
        "Space" => "Space",
        "Tab" => "Tab",
        "Minus" => "Minus",
        "Equals" => "Equal",
        "A" => "KeyA",
        "B" => "KeyB",
        "C" => "KeyC",
        "D" => "KeyD",
        "E" => "KeyE",
        "F" => "KeyF",
        "G" => "KeyG",
        "H" => "KeyH",
        "I" => "KeyI",
        "J" => "KeyJ",
        "K" => "KeyK",
        "L" => "KeyL",
        "M" => "KeyM",
        "N" => "KeyN",
        "O" => "KeyO",
        "P" => "KeyP",
        "Q" => "KeyQ",
        "R" => "KeyR",
        "S" => "KeyS",
        "T" => "KeyT",
        "U" => "KeyU",
        "V" => "KeyV",
        "W" => "KeyW",
        "X" => "KeyX",
        "Y" => "KeyY",
        "Z" => "KeyZ",
        "0" => "Digit0",
        "1" => "Digit1",
        "2" => "Digit2",
        "3" => "Digit3",
        "4" => "Digit4",
        "5" => "Digit5",
        "6" => "Digit6",
        "7" => "Digit7",
        "8" => "Digit8",
        "9" => "Digit9",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "ArrowUp" => "ArrowUp",
        "ArrowDown" => "ArrowDown",
        "ArrowLeft" => "ArrowLeft",
        "ArrowRight" => "ArrowRight",
        _ => return None,
    })
}

/// Used by the Windows low-level hook to decide if Print Screen should be swallowed.
pub fn print_screen_modifiers(shortcut: &Shortcut) -> (bool, bool, bool, bool) {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    for part in shortcut.canonical.split('+') {
        match part {
            "Control" => ctrl = true,
            "Alt" => alt = true,
            "Shift" => shift = true,
            "Meta" => meta = true,
            _ => {}
        }
    }
    (ctrl, alt, shift, meta)
}

#[allow(dead_code)]
fn _code_ref(code: Code, mods: Modifiers) -> HotKey {
    HotKey::new(Some(mods), code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_parse() {
        let shortcut = default_shortcut();
        shortcut
            .global()
            .expect("default hotkey is accepted by global-hotkey");
        #[cfg(target_os = "linux")]
        {
            assert_eq!(shortcut.canonical, "Control+Shift+Digit2");
            assert!(shortcut.display().contains("2"));
        }
    }

    #[test]
    fn friendly_names_normalize() {
        let shortcut = Shortcut::parse("ctrl+shift+2").unwrap();
        assert_eq!(shortcut.canonical, "Control+Shift+Digit2");
        let print = Shortcut::parse("PrtSc").unwrap();
        assert_eq!(print.canonical, "PrintScreen");
        assert!(print.is_print_screen());
    }

    #[test]
    fn command_shift_two_is_meta_on_the_canonical_form() {
        let shortcut = Shortcut::parse("Command+Shift+2").unwrap();
        assert_eq!(shortcut.canonical, "Shift+Meta+Digit2");
        assert!(shortcut.global().is_ok());
    }
}

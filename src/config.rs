//! User settings. Stored as JSON under the platform config directory.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::hotkey::{Shortcut, default_shortcut};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// `None` means "use the platform default".
    pub hotkey: Option<String>,
    pub save_to_disk: bool,
    /// `None` means `~/Pictures/Screenshots`.
    pub save_dir: Option<PathBuf>,
    pub launch_at_login: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: None,
            save_to_disk: true,
            save_dir: None,
            launch_at_login: true,
        }
    }
}

impl Config {
    pub fn shortcut(&self) -> Shortcut {
        self.hotkey
            .as_deref()
            .and_then(|raw| Shortcut::parse(raw).ok())
            .unwrap_or_else(default_shortcut)
    }

    pub fn set_shortcut(&mut self, shortcut: &Shortcut) {
        self.hotkey = Some(shortcut.canonical.clone());
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stillshot")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn load() -> (Config, bool) {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(config) => (config, false),
            Err(err) => {
                crate::log::log(format!("config parse failed, using defaults: {err}"));
                (Config::default(), false)
            }
        },
        Err(_) => (Config::default(), true),
    }
}

pub fn save(config: &Config) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let text = serde_json::to_string_pretty(config)?;
    fs::write(config_path(), text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_hotkey_uses_the_platform_default() {
        let config = Config::default();
        assert!(
            config.shortcut().global().is_ok()
                || cfg!(not(any(
                    target_os = "linux",
                    target_os = "windows",
                    target_os = "macos"
                )))
        );
        assert!(!config.shortcut().canonical.is_empty());
    }

    #[test]
    fn roundtrip_json() {
        let config = Config {
            hotkey: Some("Alt+Shift+KeyS".into()),
            save_to_disk: false,
            save_dir: Some(PathBuf::from("/tmp/shots")),
            launch_at_login: false,
        };
        let text = serde_json::to_string(&config).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.shortcut().canonical, "Alt+Shift+KeyS");
        assert!(!back.save_to_disk);
    }
}

//! What the user has chosen, kept between runs.
//!
//! One file, a handful of keys, and a deliberate default: **nothing here is
//! switched on**. The wizard works with no network at all — game lists and
//! cover art come from caches Steam and Playnite have already written — and it
//! stays that way until someone opts in.
//!
//! The file lives beside the artwork cache: `%LOCALAPPDATA%\PC-Cartridge-System`
//! on Windows, `~/.local/state/pc-cartridge-system` elsewhere.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Look artwork up on SteamGridDB. Off unless the user says otherwise: it
    /// is the only thing in the project that talks to the network.
    pub steamgriddb_enabled: bool,
    /// A personal SteamGridDB API key. Their v2 API refuses unauthenticated
    /// requests, so the integration does nothing useful without one.
    pub steamgriddb_api_key: String,
}

impl Settings {
    /// The key to send, or nothing when the integration is off or unkeyed.
    pub fn steamgriddb_key(&self) -> Option<&str> {
        if !self.steamgriddb_enabled {
            return None;
        }
        let key = self.steamgriddb_api_key.trim();
        (!key.is_empty()).then_some(key)
    }
}

/// Where the settings file lives. `PC_CARTRIDGE_CONFIG_DIR` overrides it, which
/// is what the tests use.
pub fn settings_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PC_CARTRIDGE_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                return PathBuf::from(local).join("PC-Cartridge-System");
            }
        }
        PathBuf::from(".")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("pc-cartridge-system")
    }
}

fn settings_path() -> PathBuf {
    settings_dir().join("settings.json")
}

/// Read the settings. A missing or unreadable file is not an error — it means
/// the defaults, which are what a fresh install runs on.
pub fn load() -> Settings {
    load_from(&settings_path())
}

pub fn load_from(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    save_to(&settings_path(), settings)
}

pub fn save_to(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("could not encode the settings: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_install_is_offline() {
        let settings = Settings::default();
        assert!(!settings.steamgriddb_enabled);
        assert!(settings.steamgriddb_key().is_none());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let scratch = crate::testutil::Scratch::new("settings");
        let path = scratch.join("settings.json");

        // Nothing written yet: the defaults stand rather than an error.
        assert_eq!(load_from(&path), Settings::default());

        let chosen = Settings {
            steamgriddb_enabled: true,
            steamgriddb_api_key: "  abc123  ".to_string(),
        };
        save_to(&path, &chosen).unwrap();

        let read_back = load_from(&path);
        assert_eq!(read_back, chosen);
        // Whitespace around a pasted key is the user's, not theirs to debug.
        assert_eq!(read_back.steamgriddb_key(), Some("abc123"));
    }

    #[test]
    fn a_key_is_only_offered_when_the_lookup_is_on() {
        let off = Settings {
            steamgriddb_enabled: false,
            steamgriddb_api_key: "abc123".to_string(),
        };
        assert!(off.steamgriddb_key().is_none());

        let on_but_unkeyed = Settings {
            steamgriddb_enabled: true,
            steamgriddb_api_key: "   ".to_string(),
        };
        assert!(on_but_unkeyed.steamgriddb_key().is_none());
    }

    #[test]
    fn nonsense_in_the_file_falls_back_to_the_defaults() {
        let scratch = crate::testutil::Scratch::new("settings-junk");
        let path = scratch.join("settings.json");
        std::fs::write(&path, b"{ not json at all").unwrap();
        assert_eq!(load_from(&path), Settings::default());
    }
}

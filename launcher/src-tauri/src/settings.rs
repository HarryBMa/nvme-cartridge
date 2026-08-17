//! `settings.conf`, shared with the shell tooling.
//!
//! The file is a flat `KEY=value` list. `MODE` is the auto-launch toggle that
//! menu option 3 of `cartridge-linux.sh` / `cartridge-windows.ps1` flips, and it
//! is honoured here with the same meaning: when it is not `running`, a cartridge
//! must never start a game on its own.

use std::path::PathBuf;

pub const SETTINGS_FILE_NAME: &str = "settings.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Cartridges may auto-launch.
    Running,
    /// Cartridges are detected but nothing starts without a click.
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub mode: Mode,
    /// Whether inserting a cartridge should press Play for the user, or just
    /// present the launcher and wait.
    pub autolaunch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // Deliberately conservative: an unreadable or absent settings file
        // shows the launcher but never launches by itself. This mirrors the
        // shell helper, which treats a missing file as blocked.
        Settings {
            mode: Mode::Stopped,
            autolaunch: false,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        crate::trust::config_dir().map(|d| d.join(SETTINGS_FILE_NAME))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Settings::default();
        };
        let Ok(raw) = std::fs::read_to_string(path) else {
            return Settings::default();
        };
        Self::parse(&raw)
    }

    pub fn parse(raw: &str) -> Self {
        let mut settings = Settings::default();
        for line in raw.lines() {
            // Strip the UTF-8 BOM PowerShell's `Out-File` likes to add.
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_ascii_uppercase().as_str() {
                "MODE" => {
                    settings.mode = if value.eq_ignore_ascii_case("running") {
                        Mode::Running
                    } else {
                        Mode::Stopped
                    };
                    // The shell tooling has no separate auto-launch key; MODE is
                    // the toggle, so track it unless AUTOLAUNCH overrides below.
                    settings.autolaunch = settings.mode == Mode::Running;
                }
                "AUTOLAUNCH" => {
                    settings.autolaunch =
                        matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes");
                }
                _ => {}
            }
        }
        settings
    }

    /// A cartridge may only start a game unattended when both the shared MODE
    /// toggle and the launcher's own preference agree.
    pub fn may_autolaunch(&self) -> bool {
        self.mode == Mode::Running && self.autolaunch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_keys_fail_closed() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert!(!Settings::parse("").may_autolaunch());
        assert_eq!(Settings::parse("").mode, Mode::Stopped);
    }

    #[test]
    fn reads_the_shell_toggle() {
        assert_eq!(Settings::parse("MODE=running").mode, Mode::Running);
        assert!(Settings::parse("MODE=running").may_autolaunch());
        assert_eq!(Settings::parse("MODE=stopped").mode, Mode::Stopped);
        assert!(!Settings::parse("MODE=stopped").may_autolaunch());
    }

    #[test]
    fn tolerates_powershell_bom_and_casing() {
        let s = Settings::parse("\u{feff}mode=RUNNING\r\n");
        assert_eq!(s.mode, Mode::Running);
    }

    #[test]
    fn autolaunch_can_be_held_back_while_mode_runs() {
        let s = Settings::parse("MODE=running\nAUTOLAUNCH=false\n");
        assert_eq!(s.mode, Mode::Running);
        assert!(!s.may_autolaunch());
    }

    #[test]
    fn autolaunch_cannot_override_a_stopped_mode() {
        // MODE is the master switch; the launcher must not be able to defeat it.
        let s = Settings::parse("MODE=stopped\nAUTOLAUNCH=true\n");
        assert!(!s.may_autolaunch());
    }
}

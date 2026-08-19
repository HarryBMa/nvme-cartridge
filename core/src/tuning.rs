//! Windows settings that cost a cartridge real performance, and undoing them.
//!
//! Two of them matter enough to offer:
//!
//!   * **Defender scans the whole game folder** the first time anything reads
//!     it. On 60 GB of freshly copied files that is minutes of contention on a
//!     link that is already the slowest part of the machine.
//!   * **Search indexing** crawls the volume for the same reason, in the
//!     background, whenever it is plugged in.
//!
//! Both are opt-in, both are per-cartridge, and both can be put back — a tool
//! that quietly weakens someone's malware scanning and cannot undo it is not
//! a tool anyone should trust. Nothing here runs unless it is asked for.
//!
//! A third setting is worth changing and is deliberately *not* automated:
//! Windows sets removable drives to "Quick removal", which turns write caching
//! off. "Better performance" is the right choice for a cartridge that is always
//! ejected properly — which this project's launcher does — but the supported
//! way to set it is Device Manager, and the registry keys behind it are
//! per-device and undocumented. It is in the README as a manual step instead of
//! being guessed at here.

/// What was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tweak {
    /// Stop Defender's real-time scanner walking the cartridge.
    DefenderExclusion,
    /// Stop Search indexing the volume.
    SearchIndexing,
}

impl Tweak {
    pub fn describe(self, applying: bool) -> &'static str {
        match (self, applying) {
            (Tweak::DefenderExclusion, true) => "Excluded the cartridge from Defender scanning.",
            (Tweak::DefenderExclusion, false) => "Defender scans the cartridge again.",
            (Tweak::SearchIndexing, true) => "Search indexing switched off for the cartridge.",
            (Tweak::SearchIndexing, false) => "Search indexing switched back on.",
        }
    }
}

/// The PowerShell for one tweak, in one direction.
///
/// Built as a pure function so the commands can be read and tested on any
/// machine, rather than only being visible when they run on Windows.
pub fn script_for(tweak: Tweak, drive: &str, applying: bool) -> Result<String, String> {
    let letter = drive_letter(drive)?;
    Ok(match (tweak, applying) {
        (Tweak::DefenderExclusion, true) => {
            format!("Add-MpPreference -ExclusionPath '{letter}:\\'")
        }
        (Tweak::DefenderExclusion, false) => {
            format!("Remove-MpPreference -ExclusionPath '{letter}:\\'")
        }
        (Tweak::SearchIndexing, applying) => format!(
            "Get-CimInstance -ClassName Win32_Volume -Filter \"DriveLetter='{letter}:'\" | \
             Set-CimInstance -Property @{{IndexingEnabled=${}}}",
            if applying { "false" } else { "true" }
        ),
    })
}

/// Everything a run would do, for showing before anything is changed.
///
/// The user sees the actual commands. This is elevated, it touches malware
/// scanning, and "trust me" is not good enough.
pub fn plan(drive: &str, tweaks: &[Tweak], applying: bool) -> Result<Vec<String>, String> {
    tweaks
        .iter()
        .map(|tweak| script_for(*tweak, drive, applying))
        .collect()
}

/// A drive letter, from whatever form the window sent.
fn drive_letter(drive: &str) -> Result<char, String> {
    let trimmed = drive.trim().trim_end_matches('\\').trim_end_matches(':');
    let mut chars = trimmed.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_alphabetic() => Ok(letter.to_ascii_uppercase()),
        _ => Err(format!("{drive} is not a drive letter")),
    }
}

/// Apply (or undo) the tweaks. Windows only; elsewhere there is nothing to do.
#[cfg(windows)]
pub fn apply(drive: &str, tweaks: &[Tweak], applying: bool) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for tweak in tweaks {
        let script = script_for(*tweak, drive, applying)?;
        // Each tweak is elevated on its own, so the wizard itself never has to
        // run as administrator.
        let status = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden \
                     -ArgumentList '-NoProfile','-NonInteractive','-Command',\"{}\"",
                    script.replace('"', "`\"")
                ),
            ])
            .status()
            .map_err(|e| format!("could not run PowerShell: {e}"))?;

        if !status.success() {
            return Err(format!(
                "{} did not go through. Nothing else was changed.",
                tweak.describe(applying)
            ));
        }
        done.push(tweak.describe(applying).to_string());
    }
    Ok(done)
}

#[cfg(not(windows))]
pub fn apply(_drive: &str, _tweaks: &[Tweak], _applying: bool) -> Result<Vec<String>, String> {
    Err("These settings only exist on Windows.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_letters_are_accepted_in_the_forms_the_window_sends() {
        assert_eq!(drive_letter("D:\\").unwrap(), 'D');
        assert_eq!(drive_letter("D:").unwrap(), 'D');
        assert_eq!(drive_letter("d").unwrap(), 'D');
        assert!(drive_letter("/run/media/harry/CINDER").is_err());
        assert!(drive_letter("").is_err());
    }

    #[test]
    fn every_tweak_has_an_exact_opposite() {
        for tweak in [Tweak::DefenderExclusion, Tweak::SearchIndexing] {
            let on = script_for(tweak, "D:\\", true).unwrap();
            let off = script_for(tweak, "D:\\", false).unwrap();
            assert_ne!(on, off, "{tweak:?} undoes to the same command");
            assert!(on.contains("D:"), "{on}");
            assert!(off.contains("D:"), "{off}");
        }
    }

    #[test]
    fn the_defender_exclusion_is_added_and_removed_by_path() {
        assert_eq!(
            script_for(Tweak::DefenderExclusion, "D:\\", true).unwrap(),
            "Add-MpPreference -ExclusionPath 'D:\\'"
        );
        assert_eq!(
            script_for(Tweak::DefenderExclusion, "D:\\", false).unwrap(),
            "Remove-MpPreference -ExclusionPath 'D:\\'"
        );
    }

    #[test]
    fn indexing_is_turned_off_and_on_for_that_volume_only() {
        let off = script_for(Tweak::SearchIndexing, "E:", true).unwrap();
        assert!(off.contains("DriveLetter='E:'"), "{off}");
        assert!(off.contains("IndexingEnabled=$false"), "{off}");

        let on = script_for(Tweak::SearchIndexing, "E:", false).unwrap();
        assert!(on.contains("IndexingEnabled=$true"), "{on}");
    }

    #[test]
    fn the_plan_is_the_commands_themselves() {
        let commands = plan(
            "D:\\",
            &[Tweak::DefenderExclusion, Tweak::SearchIndexing],
            true,
        )
        .unwrap();
        assert_eq!(commands.len(), 2);
        assert!(commands[0].starts_with("Add-MpPreference"));
        // A bad drive fails the whole plan rather than half of it.
        assert!(plan("nonsense", &[Tweak::DefenderExclusion], true).is_err());
    }
}

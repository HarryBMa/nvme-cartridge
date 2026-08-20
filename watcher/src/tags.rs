//! Virtual cartridges: a tag stands in for a drive.
//!
//! A GamePak carries the game. A tag only points at one that is already
//! installed — so a tag is a directory holding the same `cartridge.conf` a real
//! cartridge would, and nothing else:
//!
//! ```text
//! ~/.local/state/pc-gamepak/tags/04A224B2C31D80/cartridge.conf
//!                                               /cover.jpg
//! ```
//!
//! That is deliberate rather than convenient. The launcher takes a path and
//! reads a manifest out of it; it has never cared whether the path was a mount
//! point, so a tag needs no launcher changes at all — only somewhere to point.
//!
//! The directory is named after the tag's UID. There is no wizard step for this
//! yet, so the way to learn a UID is to tap the tag: an unknown one is logged
//! with the exact directory name to create.

use std::path::{Path, PathBuf};

/// Files that mark a directory as a cartridge, in the launcher's own order of
/// preference.
const MARKERS: [&str; 2] = ["cartridge.conf", "autorun.inf"];

/// Where virtual cartridges live.
///
/// Beside the settings and the artwork cache, because it is the same kind of
/// thing: state belonging to this user, on this machine. Mirrors
/// `gamepak_core::settings::settings_dir` rather than calling it — the watcher
/// does not link core, and one `join` is a cheaper price than serde and ureq
/// resident for a whole login session.
pub fn tags_dir() -> PathBuf {
    config_dir().join("tags")
}

fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PC_GAMEPAK_CONFIG_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }

    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            if !local.trim().is_empty() {
                return PathBuf::from(local).join("PC-GamePak");
            }
        }
        PathBuf::from(".")
    }

    #[cfg(not(windows))]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("pc-gamepak")
    }
}

/// A tag's UID in the one form this crate uses: uppercase hex, nothing else.
///
/// Readers disagree about presentation — `04:a2:24`, `04 A2 24`, `04-a2-24` —
/// and a directory somebody typed by hand will disagree again. Reducing both
/// sides to their hex digits makes those the same tag.
///
/// It also makes the UID safe to use as a path component, which matters,
/// because a UID is bytes off an untrusted card: after this it cannot contain a
/// separator, a `..`, or anything else with meaning to the filesystem.
pub fn normalise(uid: &str) -> String {
    uid.chars()
        .filter(char::is_ascii_hexdigit)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// Format the bytes a reader returned as a UID.
pub fn format_uid(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(
            char::from_digit((byte >> 4) as u32, 16)
                .unwrap_or('0')
                .to_ascii_uppercase(),
        );
        out.push(
            char::from_digit((byte & 0xF) as u32, 16)
                .unwrap_or('0')
                .to_ascii_uppercase(),
        );
    }
    out
}

/// The virtual cartridge for this tag, if one has been set up.
pub fn resolve(uid: &str) -> Option<PathBuf> {
    resolve_in(&tags_dir(), uid)
}

/// As `resolve`, against a given directory. Split out so it can be tested
/// without a home directory to write to.
pub fn resolve_in(root: &Path, uid: &str) -> Option<PathBuf> {
    let wanted = normalise(uid);
    if wanted.is_empty() {
        return None;
    }

    // The exact name first, which is what the log tells people to create.
    let direct = root.join(&wanted);
    if is_cartridge(&direct) {
        return Some(direct);
    }

    // Then anything that is the same UID written differently. A directory named
    // `04:a2:24:b2` is a reasonable thing to have typed, and refusing it would
    // only be pedantry.
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if normalise(name) == wanted && is_cartridge(&entry.path()) {
            return Some(entry.path());
        }
    }

    None
}

/// Is there actually a cartridge in this directory?
///
/// A tag pointing at an empty folder opens a launcher with nothing to show, so
/// it is treated as an unknown tag instead.
pub fn is_cartridge(dir: &Path) -> bool {
    MARKERS.iter().any(|name| dir.join(name).is_file())
}

/// Should the tag reader be started at all?
///
/// Off unless asked for. A reader is hardware most people do not have, and
/// somebody who does have one probably bought it for something else — their
/// bank card, their office door — so connecting to whatever is sitting on it is
/// not a thing to do uninvited. Creating the tags directory is the invitation;
/// `PC_GAMEPAK_NFC=on`/`off` overrides it either way.
pub fn nfc_wanted() -> bool {
    match std::env::var("PC_GAMEPAK_NFC") {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "yes" | "true"
        ),
        Err(_) => tags_dir().is_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of our own, so these tests do not need a home.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gamepak-tags-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("scratch directory");
        dir
    }

    fn tag(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("tag directory");
        std::fs::write(dir.join("cartridge.conf"), "title=God of War\n").expect("manifest");
        dir
    }

    #[test]
    fn a_uid_is_reduced_to_its_hex_digits() {
        assert_eq!(normalise("04:a2:24:b2"), "04A224B2");
        assert_eq!(normalise("04 A2 24 B2"), "04A224B2");
        assert_eq!(normalise("04-a2-24-b2"), "04A224B2");
        assert_eq!(normalise("04A224B2"), "04A224B2");
    }

    #[test]
    fn a_uid_can_never_reach_out_of_the_tags_directory() {
        // The UID is bytes off a card somebody else could have written, so the
        // interesting cases are the ones that would escape.
        assert_eq!(normalise("../../etc/passwd"), "ECAD");
        assert_eq!(normalise("/root"), "");
        assert_eq!(normalise(".."), "");
        assert_eq!(normalise("zz//zz"), "");

        let root = scratch("escape");
        assert_eq!(resolve_in(&root, ".."), None);
        assert_eq!(resolve_in(&root, ""), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tag_resolves_to_its_directory() {
        let root = scratch("resolve");
        let expected = tag(&root, "04A224B2");

        assert_eq!(resolve_in(&root, "04A224B2"), Some(expected.clone()));
        // However the reader chose to punctuate it.
        assert_eq!(resolve_in(&root, "04:a2:24:b2"), Some(expected.clone()));

        // And a tag nobody has set up is simply unknown.
        assert_eq!(resolve_in(&root, "DEADBEEF"), None);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_directory_named_the_long_way_round_is_still_that_tag() {
        let root = scratch("punctuated");
        let expected = tag(&root, "04:a2:24:b2");
        assert_eq!(resolve_in(&root, "04A224B2"), Some(expected));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_empty_tag_directory_is_not_a_cartridge() {
        let root = scratch("empty");
        std::fs::create_dir_all(root.join("04A224B2")).expect("bare directory");

        // Opening a launcher on a folder with no manifest would show a window
        // with nothing in it, which is worse than saying the tag is unknown.
        assert_eq!(resolve_in(&root, "04A224B2"), None);

        std::fs::write(root.join("04A224B2").join("autorun.inf"), "[autorun]\n").expect("autorun");
        assert!(resolve_in(&root, "04A224B2").is_some());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bytes_become_the_directory_name_the_log_asks_for() {
        assert_eq!(format_uid(&[0x04, 0xA2, 0x24, 0xB2]), "04A224B2");
        assert_eq!(format_uid(&[0x00, 0x0F]), "000F");
        assert_eq!(format_uid(&[]), "");
    }
}

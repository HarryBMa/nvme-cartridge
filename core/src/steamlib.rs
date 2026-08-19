//! Putting a Steam game on the cartridge so Steam plays it *from* the cartridge.
//!
//! Copying `steamapps/common/<game>` onto a drive achieves nothing on its own:
//! `steam://rungameid/<appid>` asks Steam to launch whatever copy Steam knows
//! about, and Steam only knows about folders listed in its own
//! `config/libraryfolders.vdf`. So a cartridge that really holds the game needs
//! three things:
//!
//!   1. `<drive>/steamapps/common/<installdir>` — the game files;
//!   2. `<drive>/steamapps/appmanifest_<appid>.acf` — how Steam recognises it;
//!   3. the drive listed in Steam's `libraryfolders.vdf`.
//!
//! Step 3 edits Steam's own configuration, so the original is copied to
//! `libraryfolders.vdf.bak-cartridge` first and Steam must be closed — it
//! rewrites that file from memory on exit and would undo the change.

use std::path::{Path, PathBuf};

use crate::steam::{self, Kv};

#[derive(Debug)]
pub enum LibraryError {
    SteamNotFound,
    SteamRunning,
    GameNotFound(String),
    NotEnoughSpace { needed: u64, free: u64 },
    Io(String),
}

impl std::fmt::Display for LibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LibraryError::SteamNotFound => write!(f, "Could not find the Steam installation."),
            LibraryError::SteamRunning => write!(
                f,
                "Close Steam first. It rewrites libraryfolders.vdf when it exits, \
                 which would undo the cartridge being registered."
            ),
            LibraryError::GameNotFound(name) => {
                write!(f, "Could not find the installed files for {name}.")
            }
            LibraryError::NotEnoughSpace { needed, free } => write!(
                f,
                "The game needs {} but the cartridge only has {} free.",
                crate::format::human_bytes(*needed),
                crate::format::human_bytes(*free)
            ),
            LibraryError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Where a game's files live, and how big they are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGame {
    pub app_id: String,
    pub name: String,
    /// `…/steamapps/common/<installdir>`
    pub install_path: PathBuf,
    /// `…/steamapps/appmanifest_<appid>.acf`
    pub manifest_path: PathBuf,
    pub size_on_disk: u64,
}

/// Find where Steam actually installed an app, across every library folder.
pub fn locate(steam_root: &Path, app_id: &str) -> Option<InstalledGame> {
    for library in steam::library_paths(steam_root) {
        let manifest_path = library.join(format!("appmanifest_{app_id}.acf"));
        let Ok(text) = std::fs::read_to_string(&manifest_path) else {
            continue;
        };
        let kv = steam::parse_keyvalues(&text);
        let Some(state) = kv.get("AppState") else {
            continue;
        };

        let install_dir = state
            .get("installdir")
            .and_then(Kv::as_str)
            .map(str::to_string)?;
        let install_path = library.join("common").join(&install_dir);
        if !install_path.is_dir() {
            continue;
        }

        return Some(InstalledGame {
            app_id: app_id.to_string(),
            name: state
                .get("name")
                .and_then(Kv::as_str)
                .unwrap_or(&install_dir)
                .to_string(),
            install_path,
            manifest_path,
            size_on_disk: state
                .get("SizeOnDisk")
                .and_then(Kv::as_str)
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(0),
        });
    }
    None
}

/// Add a path to Steam's `libraryfolders.vdf`, if it is not already there.
///
/// Returns the file that was written, or `None` when the drive was already
/// registered.
pub fn register_library(steam_root: &Path, drive: &Path) -> Result<Option<PathBuf>, LibraryError> {
    let vdf = steam_root.join("config/libraryfolders.vdf");
    let text = std::fs::read_to_string(&vdf)
        .map_err(|e| LibraryError::Io(format!("{}: {e}", vdf.display())))?;

    let drive_str = drive.to_string_lossy().to_string();
    if library_paths_in(&text)
        .iter()
        .any(|p| paths_match(p, &drive_str))
    {
        return Ok(None);
    }

    // Keep a copy before touching Steam's own configuration.
    let backup = vdf.with_extension("vdf.bak-cartridge");
    if !backup.exists() {
        std::fs::copy(&vdf, &backup)
            .map_err(|e| LibraryError::Io(format!("could not back up {}: {e}", vdf.display())))?;
    }

    let updated = append_library_entry(&text, &drive_str);
    std::fs::write(&vdf, updated)
        .map_err(|e| LibraryError::Io(format!("could not write {}: {e}", vdf.display())))?;

    Ok(Some(vdf))
}

/// The `path` of every entry in a `libraryfolders.vdf`.
pub fn library_paths_in(text: &str) -> Vec<String> {
    let kv = steam::parse_keyvalues(text);
    let Some(folders) = kv.get("libraryfolders") else {
        return Vec::new();
    };

    folders
        .entries()
        .iter()
        .filter_map(|(key, value)| match value {
            Kv::Map(_) => value.get("path").and_then(Kv::as_str).map(str::to_string),
            Kv::Str(s) if key.chars().all(|c| c.is_ascii_digit()) => Some(s.clone()),
            Kv::Str(_) => None,
        })
        .collect()
}

/// Compare library paths the way Steam does: separators and trailing slashes
/// are noise, and Windows paths are case-insensitive.
fn paths_match(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(a) == norm(b)
}

/// Insert a new numbered entry into `libraryfolders.vdf`.
///
/// Written as a text edit rather than by re-serialising the parsed tree: Steam's
/// file carries per-app size bookkeeping that we have no business rewriting, so
/// the safest change is the smallest one.
pub fn append_library_entry(text: &str, drive: &str) -> String {
    let next_index = library_paths_in(text).len();
    // Steam escapes backslashes in this file.
    let escaped = drive.replace('\\', "\\\\");

    let entry = format!(
        "\t\"{next_index}\"\n\t{{\n\t\t\"path\"\t\t\"{escaped}\"\n\t\t\"label\"\t\t\"{CARTRIDGE_LABEL}\"\n\t\t\"contentid\"\t\t\"0\"\n\t\t\"totalsize\"\t\t\"0\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n"
    );

    // Insert before the final closing brace of the libraryfolders block.
    match text.rfind('}') {
        Some(index) => {
            let mut out = String::with_capacity(text.len() + entry.len());
            out.push_str(&text[..index]);
            out.push_str(&entry);
            out.push_str(&text[index..]);
            out
        }
        // No structure to preserve: write a whole file.
        None => format!("\"libraryfolders\"\n{{\n{entry}}}\n"),
    }
}

/// Stamped on the library entries this tool adds.
///
/// Steam shows it in the library-folder list, and it is how `unregister_library`
/// knows which entries are ours. Entries are never removed automatically: a
/// cartridge is *supposed* to be unplugged most of the time, so a missing folder
/// is the normal state, not stale cruft.
pub const CARTRIDGE_LABEL: &str = "PC GamePak";

/// Whether `drive` is currently listed in Steam's library folders.
pub fn is_registered(steam_root: &Path, drive: &Path) -> bool {
    let vdf = steam_root.join("config/libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(vdf) else {
        return false;
    };
    let drive_str = drive.to_string_lossy().to_string();
    library_paths_in(&text)
        .iter()
        .any(|p| paths_match(p, &drive_str))
}

/// Remove a cartridge from Steam's library folders.
///
/// Only ever called from an explicit "unregister" action — this is for a
/// cartridge that has been reformatted or repurposed, where the entry would
/// otherwise point at something that is never coming back.
pub fn unregister_library(steam_root: &Path, drive: &Path) -> Result<bool, LibraryError> {
    if steam_is_running() {
        return Err(LibraryError::SteamRunning);
    }

    let vdf = steam_root.join("config/libraryfolders.vdf");
    let text = std::fs::read_to_string(&vdf)
        .map_err(|e| LibraryError::Io(format!("{}: {e}", vdf.display())))?;

    let drive_str = drive.to_string_lossy().to_string();
    let Some(updated) = remove_library_entry(&text, &drive_str) else {
        return Ok(false);
    };

    let backup = vdf.with_extension("vdf.bak-cartridge");
    if !backup.exists() {
        std::fs::copy(&vdf, &backup)
            .map_err(|e| LibraryError::Io(format!("could not back up {}: {e}", vdf.display())))?;
    }
    std::fs::write(&vdf, updated)
        .map_err(|e| LibraryError::Io(format!("could not write {}: {e}", vdf.display())))?;
    Ok(true)
}

/// Cut the entry whose `path` matches, and renumber the ones after it.
///
/// Text surgery rather than re-serialising the parsed tree, for the same reason
/// as `append_library_entry`: Steam keeps per-app bookkeeping in this file that
/// we have no business rewriting.
pub fn remove_library_entry(text: &str, drive: &str) -> Option<String> {
    let paths = library_paths_in(text);
    let index = paths.iter().position(|p| paths_match(p, drive))?;

    // Find the `"<index>"` key at entry level, then its brace-matched block.
    let key = format!("\"{index}\"");
    let key_at = text.find(&key)?;
    let open = text[key_at..].find('{')? + key_at;

    let mut depth = 0;
    let mut close = None;
    let bytes = text.as_bytes();
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                // Skip string contents so braces inside them do not count.
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;

    // Take the whole line the key sits on, through the closing brace's line.
    let start = text[..key_at].rfind('\n').map(|n| n + 1).unwrap_or(0);
    let end = text[close..]
        .find('\n')
        .map(|n| close + n + 1)
        .unwrap_or(text.len());

    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..start]);
    out.push_str(&text[end..]);

    // Renumber the entries after the hole so the keys stay sequential.
    for old in (index + 1)..paths.len() {
        out = out.replacen(&format!("\"{old}\"\n"), &format!("\"{}\"\n", old - 1), 1);
    }
    Some(out)
}

/// Whether Steam is currently running.
pub fn steam_is_running() -> bool {
    #[cfg(windows)]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq steam.exe", "/NH"])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains("steam.exe")
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("pgrep")
            .args(["-x", "steam"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Copy a directory tree, reporting bytes copied as it goes.
///
/// A game is tens of gigabytes, so the callback lets the window show progress
/// instead of appearing to hang.
pub fn copy_tree(from: &Path, to: &Path, progress: &mut dyn FnMut(u64)) -> std::io::Result<u64> {
    // The running total is threaded through the recursion rather than
    // accumulated per level, so `progress` always receives the total copied so
    // far. Reporting a per-directory subtotal would make a progress bar jump
    // backwards every time the walk entered a new folder.
    let mut total = 0u64;
    copy_into(from, to, &mut total, progress)?;
    Ok(total)
}

fn copy_into(
    from: &Path,
    to: &Path,
    total: &mut u64,
    progress: &mut dyn FnMut(u64),
) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;

    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let source = entry.path();
        let destination = to.join(entry.file_name());

        if file_type.is_dir() {
            copy_into(&source, &destination, total, progress)?;
        } else if file_type.is_file() {
            let bytes = std::fs::copy(&source, &destination)?;
            *total += bytes;
            progress(*total);
        }
        // Symlinks are skipped: WinBtrfs on Windows does not reliably support
        // them, and Steam games on Windows do not rely on them.
    }

    Ok(())
}

/// Total size of a directory tree.
pub fn tree_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => tree_size(&entry.path()),
            Ok(t) if t.is_file() => entry.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/harry/.local/share/Steam"
		"label"		""
		"apps"
		{
			"367520"		"9106886656"
		}
	}
}
"#;

    #[test]
    fn reads_existing_library_paths() {
        assert_eq!(
            library_paths_in(SAMPLE),
            vec!["/home/harry/.local/share/Steam"]
        );
    }

    #[test]
    fn appends_a_new_entry_and_keeps_the_old_one() {
        let updated = append_library_entry(SAMPLE, "/run/media/harry/CINDER");
        let paths = library_paths_in(&updated);
        assert_eq!(
            paths,
            vec!["/home/harry/.local/share/Steam", "/run/media/harry/CINDER"]
        );
        // The existing app bookkeeping must survive the edit untouched.
        assert!(updated.contains("\"367520\"\t\t\"9106886656\""));
        // And the result must still parse as one libraryfolders block.
        assert_eq!(
            steam::parse_keyvalues(&updated)
                .get("libraryfolders")
                .map(|f| f.entries().len()),
            Some(2)
        );
    }

    #[test]
    fn appends_windows_paths_with_escaped_separators() {
        let updated = append_library_entry(SAMPLE, "D:\\SteamLibrary");
        assert!(updated.contains("D:\\\\SteamLibrary"), "{updated}");
        // Round-trips back to the unescaped form.
        assert!(library_paths_in(&updated).contains(&"D:\\SteamLibrary".to_string()));
    }

    #[test]
    fn recognises_an_already_registered_drive() {
        let updated = append_library_entry(SAMPLE, "/run/media/harry/CINDER");
        let existing = library_paths_in(&updated);
        assert!(existing
            .iter()
            .any(|p| paths_match(p, "/run/media/harry/CINDER")));
        // Trailing slash and separator style must not fool it.
        assert!(existing
            .iter()
            .any(|p| paths_match(p, "/run/media/harry/CINDER/")));
        assert!(!existing
            .iter()
            .any(|p| paths_match(p, "/run/media/harry/OTHER")));
    }

    #[test]
    fn windows_path_comparison_ignores_case_and_separators() {
        assert!(paths_match("D:\\SteamLibrary", "d:/steamlibrary"));
        assert!(paths_match("D:\\SteamLibrary\\", "D:\\SteamLibrary"));
        assert!(!paths_match("D:\\SteamLibrary", "E:\\SteamLibrary"));
    }

    #[test]
    fn writes_a_whole_file_when_there_is_nothing_to_preserve() {
        let out = append_library_entry("", "/mnt/cart");
        assert_eq!(library_paths_in(&out), vec!["/mnt/cart"]);
    }

    #[test]
    fn locates_an_installed_game_across_libraries() {
        let scratch = crate::testutil::Scratch::new("locate");
        let second = scratch.join("second");
        std::fs::create_dir_all(scratch.join("steamapps/common")).unwrap();
        std::fs::create_dir_all(second.join("steamapps/common/Hollow Knight")).unwrap();

        std::fs::write(
            scratch.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} }}",
                second.display()
            ),
        )
        .unwrap();
        std::fs::write(
            second.join("steamapps/appmanifest_367520.acf"),
            r#""AppState" { "appid" "367520" "name" "Hollow Knight"
                "installdir" "Hollow Knight" "StateFlags" "4" "SizeOnDisk" "9106886656" }"#,
        )
        .unwrap();

        let game = locate(scratch.path(), "367520").expect("found in the second library");
        assert_eq!(game.name, "Hollow Knight");
        assert_eq!(game.size_on_disk, 9_106_886_656);
        assert_eq!(
            game.install_path,
            second.join("steamapps/common/Hollow Knight")
        );

        assert_eq!(locate(scratch.path(), "999999"), None);
    }

    #[test]
    fn does_not_locate_a_game_whose_files_are_gone() {
        let scratch = crate::testutil::Scratch::new("missing-files");
        std::fs::create_dir_all(scratch.join("steamapps")).unwrap();
        std::fs::write(
            scratch.join("steamapps/appmanifest_1.acf"),
            r#""AppState" { "appid" "1" "name" "Ghost" "installdir" "Ghost" "StateFlags" "4" }"#,
        )
        .unwrap();
        // The manifest exists but common/Ghost does not.
        assert_eq!(locate(scratch.path(), "1"), None);
    }

    #[test]
    fn copies_a_tree_and_reports_progress() {
        let scratch = crate::testutil::Scratch::new("copy");
        let from = scratch.join("from");
        let to = scratch.join("to");
        std::fs::create_dir_all(from.join("bin/data")).unwrap();
        std::fs::write(from.join("game.exe"), vec![b'x'; 100]).unwrap();
        std::fs::write(from.join("bin/lib.dll"), vec![b'y'; 250]).unwrap();
        std::fs::write(from.join("bin/data/pack.bin"), vec![b'z'; 650]).unwrap();

        let mut reports = Vec::new();
        let copied = copy_tree(&from, &to, &mut |n| reports.push(n)).unwrap();

        assert_eq!(copied, 1000);
        assert_eq!(tree_size(&to), 1000);
        assert!(to.join("bin/data/pack.bin").is_file());
        // Progress is reported per file and never goes backwards.
        assert_eq!(reports.len(), 3);
        assert!(reports.windows(2).all(|w| w[1] >= w[0]), "{reports:?}");
        assert_eq!(*reports.last().unwrap(), 1000);
    }

    #[test]
    fn tree_size_of_a_missing_directory_is_zero() {
        assert_eq!(tree_size(Path::new("/definitely/not/here")), 0);
    }
}

#[cfg(test)]
mod unregister_tests {
    use super::*;

    const TWO: &str = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"/home/harry/.local/share/Steam"
		"label"		""
		"apps"
		{
			"367520"		"9106886656"
		}
	}
	"1"
	{
		"path"		"/run/media/harry/CINDER"
		"label"		"PC GamePak"
		"apps"
		{
		}
	}
	"2"
	{
		"path"		"/mnt/games/SteamLibrary"
		"label"		""
		"apps"
		{
			"1145360"		"15204593664"
		}
	}
}
"#;

    #[test]
    fn new_entries_carry_the_cartridge_label() {
        let updated = append_library_entry(TWO, "/run/media/harry/HOLLOW");
        assert!(updated.contains(CARTRIDGE_LABEL), "{updated}");
    }

    #[test]
    fn removes_the_matching_entry_and_keeps_the_others() {
        let out = remove_library_entry(TWO, "/run/media/harry/CINDER").expect("found");
        let paths = library_paths_in(&out);
        assert_eq!(
            paths,
            vec!["/home/harry/.local/share/Steam", "/mnt/games/SteamLibrary"]
        );
        // The surviving libraries keep their app bookkeeping untouched.
        assert!(out.contains("\"367520\"\t\t\"9106886656\""));
        assert!(out.contains("\"1145360\"\t\t\"15204593664\""));
        // And the keys are renumbered, so no gap is left behind.
        assert!(out.contains("\"1\"") && !out.contains("\"2\""), "{out}");
    }

    #[test]
    fn removing_the_last_entry_works() {
        let out = remove_library_entry(TWO, "/mnt/games/SteamLibrary").expect("found");
        assert_eq!(
            library_paths_in(&out),
            vec!["/home/harry/.local/share/Steam", "/run/media/harry/CINDER"]
        );
    }

    #[test]
    fn a_drive_that_is_not_listed_is_left_alone() {
        assert_eq!(remove_library_entry(TWO, "/run/media/harry/NOPE"), None);
    }

    #[test]
    fn the_result_still_parses_as_one_block() {
        let out = remove_library_entry(TWO, "/run/media/harry/CINDER").unwrap();
        let parsed = steam::parse_keyvalues(&out);
        assert_eq!(
            parsed.get("libraryfolders").map(|f| f.entries().len()),
            Some(2)
        );
    }

    #[test]
    fn round_trips_through_add_and_remove() {
        let added = append_library_entry(TWO, "/run/media/harry/HOLLOW");
        assert_eq!(library_paths_in(&added).len(), 4);
        let removed = remove_library_entry(&added, "/run/media/harry/HOLLOW").unwrap();
        assert_eq!(library_paths_in(&removed), library_paths_in(TWO));
    }
}

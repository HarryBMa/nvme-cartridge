//! Reading a cartridge: the manifest at its root, and the cover art beside it.
//!
//! Split out of the Tauri binary so it can be tested without a webview. Pure
//! std + serde.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One game entry inside a multi-game bundle cartridge.
#[derive(Serialize, Clone)]
pub struct GameEntry {
    pub title: String,
    pub executable: String,
    /// Cover as a `data:` URI, or empty string if none.
    pub cover: String,
    /// Absolute path to the cover image, or empty string.
    pub cover_path: String,
}

#[derive(Serialize, Clone)]
pub struct CartridgeInfo {
    /// Display title of the game / collection.
    pub title: String,
    /// Absolute path to the cover image, or empty string if none. Shown in the
    /// details sheet; never sent back to the backend.
    pub cover_path: String,
    /// The cover as a `data:` URI, or empty string if there is none.
    ///
    /// Inlined rather than served over the asset protocol: cartridge mount
    /// points are arbitrary, so a scope wide enough to serve them would be wide
    /// enough to serve anything on the machine.
    pub cover: String,
    /// The value from the `executable` / `open` key — either a URI or a
    /// relative path on the cartridge. For bundles this is the first game's
    /// executable so keyboard shortcuts (Enter = play) still work.
    pub executable: String,
    /// The root path of the cartridge drive as supplied by the caller.
    pub drive_path: String,
    /// True when the game's files live on the cartridge itself, rather than the
    /// cartridge being a key that points at an installed copy.
    ///
    /// The launcher uses this to make Eject ask twice: pulling a drive that a
    /// running game is reading from is a different mistake to pulling one that
    /// only holds a text file.
    pub holds_game: bool,
    /// True when this is a multi-game bundle with a `[collection]` + `[game]`
    /// format. Single-game cartridges have `false` here and an empty `games`
    /// vec for full backwards compatibility.
    pub is_bundle: bool,
    /// Individual games, populated only for bundles (`is_bundle == true`).
    pub games: Vec<GameEntry>,
}

/// Does this cartridge carry the game, or just point at it?
pub fn holds_game(root: &Path) -> bool {
    // Written by the wizard's "copy the game" step, and the same layout Steam
    // uses for any library folder.
    root.join("steamapps").join("common").is_dir()
}

/// Largest cover we will base64 into the webview. A cartridge is not a trusted
/// input, and a 200 MB "cover" should fail rather than be inlined.
pub const MAX_COVER_BYTES: u64 = 8 * 1024 * 1024;

// --------------------------------------------------------------------------
// Inline INI / conf file parser
//
// Handles both Windows autorun.inf ([section] key=value) and our flat
// cartridge.conf (key=value, no sections required).
// --------------------------------------------------------------------------

pub type IniMap = HashMap<String, HashMap<String, String>>;

pub fn parse_ini(content: &str) -> IniMap {
    let mut map: IniMap = HashMap::new();
    let mut current_section = String::from("general");

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                current_section = line[1..end].trim().to_lowercase();
            }
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_lowercase();
            let val = line[eq + 1..].trim().to_string();
            map.entry(current_section.clone())
                .or_default()
                .insert(key, val);
        }
    }
    map
}

pub fn ini_get<'a>(map: &'a IniMap, section: &str, key: &str) -> Option<&'a String> {
    map.get(section)?.get(key)
}

/// Extract every `[game]` section in order, preserving duplicates.
///
/// A regular `IniMap` / `HashMap` collapses repeated sections; this returns
/// each `[game]` block as its own key→value map so a multi-game bundle is
/// parsed correctly.
pub fn parse_game_sections(content: &str) -> Vec<HashMap<String, String>> {
    let mut result: Vec<HashMap<String, String>> = Vec::new();
    let mut in_game = false;
    let mut current: HashMap<String, String> = HashMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                let section = line[1..end].trim().to_lowercase();
                if in_game && !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                } else if in_game {
                    current.clear();
                }
                in_game = section == "game";
            }
            continue;
        }
        if in_game {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_lowercase();
                let val = line[eq + 1..].trim().to_string();
                current.insert(key, val);
            }
        }
    }
    if in_game && !current.is_empty() {
        result.push(current);
    }
    result
}

// --------------------------------------------------------------------------
// Helper: read cartridge metadata
//
// Priority:
//   1. cartridge.conf  (our own flat format — either the legacy single-game
//      key=value style, or the new [collection] + [game] bundle format)
//   2. autorun.inf     (classic Windows autorun, section [autorun])
// --------------------------------------------------------------------------

pub fn read_cartridge_info(drive_path: &str) -> Result<CartridgeInfo, String> {
    let root = Path::new(drive_path);

    // ---- Try cartridge.conf first ----
    let conf_path = root.join("cartridge.conf");
    if conf_path.exists() {
        let content = std::fs::read_to_string(&conf_path)
            .map_err(|e| format!("Failed to read cartridge.conf: {e}"))?;

        let ini = parse_ini(&content);
        let game_sections = parse_game_sections(&content);

        // ---- Bundle format: one or more [game] sections ----
        if !game_sections.is_empty() {
            let title = ini_get(&ini, "collection", "title")
                .cloned()
                .unwrap_or_else(|| "Game Collection".to_string());

            let cover_rel = ini_get(&ini, "collection", "cover")
                .cloned()
                .unwrap_or_default();
            let cover_path = resolve_cover(root, &cover_rel);

            let games: Vec<GameEntry> = game_sections
                .iter()
                .map(|g| {
                    let game_title = g
                        .get("title")
                        .cloned()
                        .unwrap_or_else(|| "Unknown Game".to_string());
                    let game_exec = g.get("executable").cloned().unwrap_or_default();
                    let game_cover_rel = g.get("cover").cloned().unwrap_or_default();
                    let game_cover_path = resolve_cover(root, &game_cover_rel);
                    GameEntry {
                        title: game_title,
                        executable: game_exec,
                        cover: cover_as_data_uri(&game_cover_path),
                        cover_path: game_cover_path,
                    }
                })
                .collect();

            // Expose the first game's executable as the primary so keyboard
            // shortcuts (Enter = play) still work for bundles.
            let executable = games
                .first()
                .map(|g| g.executable.clone())
                .unwrap_or_default();

            return Ok(CartridgeInfo {
                title,
                cover: cover_as_data_uri(&cover_path),
                cover_path,
                executable,
                drive_path: drive_path.to_string(),
                holds_game: holds_game(root),
                is_bundle: true,
                games,
            });
        }

        // ---- Single-game format: flat key=value (backwards compatible) ----
        let executable = ini_get(&ini, "general", "executable")
            .cloned()
            .unwrap_or_default();

        let title = ini_get(&ini, "general", "title")
            .cloned()
            .unwrap_or_else(|| "Unknown Game".to_string());

        let cover_rel = ini_get(&ini, "general", "cover")
            .cloned()
            .unwrap_or_default();
        let cover_path = resolve_cover(root, &cover_rel);

        return Ok(CartridgeInfo {
            title,
            cover: cover_as_data_uri(&cover_path),
            cover_path,
            executable,
            drive_path: drive_path.to_string(),
            holds_game: holds_game(root),
            is_bundle: false,
            games: Vec::new(),
        });
    }

    // ---- Fall back to autorun.inf ----
    let autorun_path = root.join("autorun.inf");
    if autorun_path.exists() {
        let content = std::fs::read_to_string(&autorun_path)
            .map_err(|e| format!("Failed to read autorun.inf: {e}"))?;

        let ini = parse_ini(&content);

        // `open=` and `shellexecute=` are deliberately not used as the Play
        // target. Windows has ignored them on non-optical media since Windows 7
        // — they are the original autorun malware vector. A drive that has only
        // an autorun.inf can still be shown in the launcher (label + cover art),
        // but Play will be disabled because there is no executable to run.
        let title = ini_get(&ini, "autorun", "label")
            .cloned()
            .unwrap_or_else(|| "Unknown Game".to_string());

        let icon_rel = ini_get(&ini, "autorun", "icon")
            .cloned()
            .unwrap_or_default();
        let cover_path = resolve_cover(root, &icon_rel);

        return Ok(CartridgeInfo {
            title,
            cover: cover_as_data_uri(&cover_path),
            cover_path,
            // Empty: autorun.inf's open= and shellexecute= are not used as the
            // play target; Play is intentionally disabled for autorun-only drives.
            executable: String::new(),
            drive_path: drive_path.to_string(),
            holds_game: holds_game(root),
            is_bundle: false,
            games: Vec::new(),
        });
    }

    Err(format!(
        "No cartridge.conf or autorun.inf found in {drive_path}"
    ))
}

/// Resolve a relative cover image path, falling back to common filenames.
pub fn resolve_cover(root: &Path, rel: &str) -> String {
    if !rel.is_empty() {
        if let Some(p) = join_within(root, rel) {
            if p.is_file() {
                return p.to_string_lossy().to_string();
            }
        }
    }
    find_cover_image(root)
}

/// Join a cartridge-supplied relative path onto the drive root, refusing
/// anything that would leave the drive.
///
/// `cover=` comes out of a file on a volume someone else may have written, so
/// `..\..\Users\me\.ssh\id_rsa` has to be rejected rather than read and handed
/// to the webview.
fn join_within(root: &Path, rel: &str) -> Option<PathBuf> {
    use std::path::Component;

    let candidate = Path::new(rel);
    // Absolute paths and drive-qualified paths (`C:\…`) are never relative to
    // this cartridge.
    if candidate.is_absolute() || rel.contains(':') {
        return None;
    }

    let mut out = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            // Any climb out, and any root or drive prefix, disqualifies it.
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    Some(root.join(out))
}

/// Read a cover image and encode it as a `data:` URI. Any failure yields an
/// empty string: a missing or oversized cover is not a reason to refuse the
/// cartridge, the placeholder just stays.
pub fn cover_as_data_uri(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let p = Path::new(path);
    match std::fs::metadata(p) {
        Ok(meta) if meta.len() > MAX_COVER_BYTES => return String::new(),
        Ok(_) => {}
        Err(_) => return String::new(),
    }
    let Ok(bytes) = std::fs::read(p) else {
        return String::new();
    };

    let mime = match p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        _ => "image/png",
    };

    format!("data:{mime};base64,{}", base64_encode(&bytes))
}

/// Look for common cover image filenames in the root of the cartridge.
pub fn find_cover_image(root: &Path) -> String {
    let candidates = [
        "cover.png",
        "cover.jpg",
        "cover.jpeg",
        "cover.webp",
        "poster.png",
        "poster.jpg",
        "box.png",
        "box.jpg",
    ];
    for name in &candidates {
        let p = root.join(name);
        if p.is_file() {
            return p.to_string_lossy().to_string();
        }
    }
    String::new()
}

/// Pull the value of `--drive` out of an argument list.
///
/// Accepts `--drive X` and `--drive=X`. Returns an empty string when absent,
/// which the frontend reports as "no cartridge" rather than guessing.
pub fn drive_from_args<I: Iterator<Item = String>>(args: I) -> String {
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--drive" {
            return args.next().unwrap_or_default();
        }
        if let Some(value) = arg.strip_prefix("--drive=") {
            return value.to_string();
        }
    }
    String::new()
}

/// Minimal base64 encoder — avoids adding a `base64` crate dependency.
pub fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 {
            chunk[1] as usize
        } else {
            0
        };
        let b2 = if chunk.len() > 2 {
            chunk[2] as usize
        } else {
            0
        };
        out.push(CHARS[(b0 >> 2) & 0x3F] as char);
        out.push(CHARS[((b0 << 4) | (b1 >> 4)) & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            CHARS[((b1 << 2) | (b2 >> 6)) & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[b2 & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_cartridge_conf() {
        let scratch = crate::testutil::Scratch::new("conf");
        std::fs::write(
            scratch.join("cartridge.conf"),
            "title=Hollow Knight\nexecutable=steam://rungameid/367520\n",
        )
        .unwrap();

        let info = read_cartridge_info(scratch.path().to_str().unwrap()).unwrap();
        assert_eq!(info.title, "Hollow Knight");
        assert_eq!(info.executable, "steam://rungameid/367520");
        assert!(!info.holds_game);
        assert!(!info.is_bundle);
        assert!(info.games.is_empty());
    }

    #[test]
    fn reads_a_bundle_cartridge_conf() {
        let scratch = crate::testutil::Scratch::new("bundle");
        std::fs::write(
            scratch.join("cartridge.conf"),
            "[collection]\ntitle=God of War Collection\ncover=collection.jpg\n\n\
             [game]\ntitle=God of War 2018\nexecutable=steam://rungameid/310970\n\n\
             [game]\ntitle=God of War Ragnarök\nexecutable=steam://rungameid/1476670\n",
        )
        .unwrap();

        let info = read_cartridge_info(scratch.path().to_str().unwrap()).unwrap();
        assert!(info.is_bundle);
        assert_eq!(info.title, "God of War Collection");
        assert_eq!(info.games.len(), 2);
        assert_eq!(info.games[0].title, "God of War 2018");
        assert_eq!(info.games[0].executable, "steam://rungameid/310970");
        assert_eq!(info.games[1].title, "God of War Ragnarök");
        assert_eq!(info.games[1].executable, "steam://rungameid/1476670");
        // Primary executable is the first game's for keyboard shortcuts.
        assert_eq!(info.executable, "steam://rungameid/310970");
    }

    #[test]
    fn parse_game_sections_preserves_order() {
        let content = "[collection]\ntitle=Test\n\n\
                       [game]\ntitle=A\nexecutable=steam://1\n\n\
                       [game]\ntitle=B\nexecutable=steam://2\n";
        let sections = parse_game_sections(content);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].get("title").unwrap(), "A");
        assert_eq!(sections[1].get("title").unwrap(), "B");
    }

    #[test]
    fn single_game_conf_is_not_a_bundle() {
        let scratch = crate::testutil::Scratch::new("single");
        std::fs::write(
            scratch.join("cartridge.conf"),
            "title=Cyberpunk 2077\nexecutable=steam://rungameid/1091500\n",
        )
        .unwrap();

        let info = read_cartridge_info(scratch.path().to_str().unwrap()).unwrap();
        assert!(!info.is_bundle);
        assert!(info.games.is_empty());
    }

    #[test]
    fn a_cartridge_carrying_the_game_says_so() {
        let scratch = crate::testutil::Scratch::new("holds");
        std::fs::write(
            scratch.join("cartridge.conf"),
            "title=X\nexecutable=steam://rungameid/1\n",
        )
        .unwrap();
        assert!(
            !read_cartridge_info(scratch.path().to_str().unwrap())
                .unwrap()
                .holds_game
        );

        std::fs::create_dir_all(scratch.join("steamapps/common/X")).unwrap();
        assert!(
            read_cartridge_info(scratch.path().to_str().unwrap())
                .unwrap()
                .holds_game
        );
    }

    #[test]
    fn autorun_supplies_a_label_but_never_an_executable() {
        let scratch = crate::testutil::Scratch::new("autorun");
        std::fs::write(
            scratch.join("autorun.inf"),
            "[autorun]\r\nlabel=Legacy Disc\r\nicon=cover.ico\r\nopen=evil.exe\r\n",
        )
        .unwrap();
        let info = read_cartridge_info(scratch.path().to_str().unwrap()).unwrap();
        assert_eq!(info.title, "Legacy Disc");
        // open= and shellexecute= must never reach the launcher as a play target.
        assert!(
            info.executable.is_empty(),
            "executable should be empty, got {:?}",
            info.executable
        );
    }

    #[test]
    fn a_volume_with_neither_file_is_not_a_cartridge() {
        let scratch = crate::testutil::Scratch::new("empty");
        assert!(read_cartridge_info(scratch.path().to_str().unwrap()).is_err());
    }

    #[test]
    fn cover_paths_stay_on_the_cartridge() {
        let root = Path::new("/media/x");
        assert_eq!(
            join_within(root, "art/cover.png"),
            Some(root.join("art/cover.png"))
        );
        for bad in [
            "../../../etc/passwd",
            "/etc/passwd",
            "C:\\Windows\\SAM",
            "..",
        ] {
            assert_eq!(join_within(root, bad), None, "{bad}");
        }
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn drive_from_args_reads_both_forms() {
        let v = |a: &[&str]| drive_from_args(a.iter().map(|s| s.to_string()));
        assert_eq!(v(&["--drive", "/run/media/h/CART"]), "/run/media/h/CART");
        assert_eq!(v(&["--drive=D:\\"]), "D:\\");
        assert_eq!(v(&["--create"]), "");
        assert_eq!(v(&["--drive"]), "");
    }
}

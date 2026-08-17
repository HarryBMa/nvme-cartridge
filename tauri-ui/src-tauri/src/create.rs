//! The create-cartridge wizard: turn a drive into a cartridge.
//!
//! Writes two files to the drive root — `cartridge.conf` and `cover.jpg` — and
//! nothing else. Everything it needs is already on the machine: the game list
//! comes from Steam's own manifests and the art from Steam's cache, so a
//! cartridge can be made with no network at all.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::drives::{self, TargetDrive};
use crate::steam;

/// Largest cover we will copy onto a cartridge.
const MAX_COVER_BYTES: u64 = 8 * 1024 * 1024;

/// URI schemes Play is allowed to hand to the OS.
const ALLOWED_SCHEMES: [&str; 8] = [
    "steam://",
    "heroic://",
    "gog://",
    "epic://",
    "playnite://",
    "lutris://",
    "http://",
    "https://",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGameInfo {
    pub app_id: String,
    pub name: String,
    pub size_on_disk: u64,
    pub has_cover: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeRequest {
    /// Target drive root. Re-checked here against the allowed list.
    pub drive_path: String,
    pub title: String,
    pub executable: String,
    /// Steam app id, when the cover should be copied from Steam's cache.
    #[serde(default)]
    pub app_id: Option<String>,
    /// Absolute path to a user-chosen cover image instead.
    #[serde(default)]
    pub cover_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeResult {
    pub conf_path: String,
    pub cover_written: bool,
    /// Anything worth telling the user that is not an outright failure.
    pub warnings: Vec<String>,
}

/// Games the wizard can offer.
pub fn steam_games() -> Result<Vec<SteamGameInfo>, String> {
    let root = steam::steam_root().ok_or_else(|| {
        "Could not find a Steam installation. Set STEAM_ROOT if it is somewhere unusual, \
         or enter the game details by hand."
            .to_string()
    })?;

    let games = steam::installed_games(&root);
    if games.is_empty() {
        return Err(format!(
            "Found Steam at {} but no fully installed games in it.",
            root.display()
        ));
    }

    Ok(games
        .into_iter()
        .map(|g| SteamGameInfo {
            app_id: g.app_id,
            name: g.name,
            size_on_disk: g.size_on_disk,
            has_cover: g.cover_path.is_some(),
        })
        .collect())
}

/// The cached cover for one app, as a data URI. Loaded one at a time: base64ing
/// a whole library at once would be tens of megabytes of IPC.
pub fn steam_cover(app_id: &str) -> String {
    if !is_numeric(app_id) {
        return String::new();
    }
    let Some(root) = steam::steam_root() else {
        return String::new();
    };
    let Some(path) = steam::find_cover(&root, app_id) else {
        return String::new();
    };
    read_as_data_uri(&path).unwrap_or_default()
}

pub fn target_drives() -> Vec<TargetDrive> {
    drives::list_drives()
}

/// Write the cartridge.
pub fn create_cartridge(request: &CartridgeRequest) -> Result<CartridgeResult, String> {
    let mut warnings = Vec::new();

    // Never trust the frontend's idea of where to write. Re-derive the allowed
    // set and require an exact match: this is the only code in the project that
    // creates files outside its own config directory.
    let root = resolve_target(&request.drive_path)?;

    let title = sanitize_conf_value(&request.title);
    if title.is_empty() {
        return Err("Give the cartridge a title.".into());
    }

    let executable = sanitize_conf_value(&request.executable);
    validate_executable(&executable, &root)?;

    // Copy the art first: if that fails we would rather not have written a conf
    // pointing at a cover that is not there.
    let cover_written = match write_cover(&root, request) {
        Ok(written) => written,
        Err(e) => {
            warnings.push(format!("Cover art was not copied: {e}"));
            false
        }
    };

    let conf = render_cartridge_conf(&title, &executable, cover_written.then_some("cover.jpg"));
    let conf_path = root.join("cartridge.conf");

    std::fs::write(&conf_path, conf)
        .map_err(|e| format!("Could not write {}: {e}", conf_path.display()))?;

    if !cover_written {
        warnings.push(
            "No cover art on the cartridge. The launcher will show a placeholder.".to_string(),
        );
    }

    Ok(CartridgeResult {
        conf_path: conf_path.to_string_lossy().into_owned(),
        cover_written,
        warnings,
    })
}

/// Check the requested drive is one we are actually willing to write to.
fn resolve_target(requested: &str) -> Result<PathBuf, String> {
    if requested.trim().is_empty() {
        return Err("Choose a drive first.".into());
    }
    let requested_path = Path::new(requested);

    let allowed = drives::list_drives();
    let matched = allowed
        .iter()
        .any(|drive| Path::new(&drive.path) == requested_path);

    if !matched {
        return Err(format!(
            "{requested} is not a removable drive this tool will write to. \
             Re-scan and pick a drive from the list."
        ));
    }
    if !requested_path.is_dir() {
        return Err(format!("{requested} is not there any more."));
    }
    Ok(requested_path.to_path_buf())
}

/// Copy the chosen art to `<drive>/cover.jpg`. Returns whether anything was
/// written.
fn write_cover(root: &Path, request: &CartridgeRequest) -> Result<bool, String> {
    let source = match (&request.cover_source, &request.app_id) {
        // An explicit file the user picked wins.
        (Some(path), _) if !path.trim().is_empty() => PathBuf::from(path),
        (_, Some(app_id)) if is_numeric(app_id) => {
            let steam_root = steam::steam_root()
                .ok_or_else(|| "no Steam installation to take the cover from".to_string())?;
            match steam::find_cover(&steam_root, app_id) {
                Some(p) => p,
                None => return Ok(false), // Steam simply has not cached one
            }
        }
        _ => return Ok(false),
    };

    let meta = std::fs::metadata(&source)
        .map_err(|e| format!("{}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    if meta.len() > MAX_COVER_BYTES {
        return Err(format!(
            "{} is {:.1} MB; the limit is {} MB",
            source.display(),
            meta.len() as f64 / 1_048_576.0,
            MAX_COVER_BYTES / 1_048_576
        ));
    }

    let destination = root.join("cover.jpg");
    std::fs::copy(&source, &destination)
        .map_err(|e| format!("could not write {}: {e}", destination.display()))?;
    Ok(true)
}

/// Strip anything that would corrupt the `key=value` file.
///
/// Newlines are the one that matters: a title containing one could otherwise
/// append an `executable=` line of its own choosing.
pub fn sanitize_conf_value(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A cartridge may name a known URI scheme, or a file on the cartridge itself.
pub fn validate_executable(executable: &str, root: &Path) -> Result<(), String> {
    if executable.is_empty() {
        return Err("Set what Play should start.".into());
    }

    let lower = executable.to_lowercase();
    if ALLOWED_SCHEMES.iter().any(|s| lower.starts_with(s)) {
        return Ok(());
    }

    // Anything with a scheme we do not know is refused rather than written out
    // and handed to the shell later.
    if let Some(colon) = executable.find(':') {
        let looks_like_scheme = executable[..colon]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.');
        // "D:\Games\x.exe" is a Windows path, not a scheme.
        let is_drive_letter = colon == 1;
        if looks_like_scheme && !is_drive_letter && executable[colon..].starts_with("://") {
            return Err(format!(
                "{executable} uses a scheme this launcher will not open. \
                 Supported: {}",
                ALLOWED_SCHEMES.join(", ")
            ));
        }
    }

    // Otherwise it has to be a relative path that stays on the cartridge.
    let candidate = Path::new(executable);
    if candidate.is_absolute() || executable.contains(':') {
        return Err(
            "A program has to live on the cartridge, so use a path relative to its root."
                .to_string(),
        );
    }
    use std::path::Component;
    for component in candidate.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err("The program path must not leave the cartridge.".to_string());
        }
    }

    if !root.join(candidate).exists() {
        return Err(format!("{executable} is not on the cartridge yet."));
    }
    Ok(())
}

/// Render the conf file, with a header explaining where it came from.
pub fn render_cartridge_conf(title: &str, executable: &str, cover: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("# PC Cartridge System\n");
    out.push_str("# Written by the create-cartridge wizard. Safe to edit by hand.\n");
    out.push('\n');
    out.push_str(&format!("title={title}\n"));
    out.push_str(&format!("executable={executable}\n"));
    if let Some(cover) = cover {
        out.push_str(&format!("cover={cover}\n"));
    }
    out
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn read_as_data_uri(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_COVER_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let mime = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "png" => "image/png",
        "webp" => "image/webp",
        _ => "image/jpeg",
    };
    Some(format!("data:{mime};base64,{}", crate::base64_encode(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_values_that_would_corrupt_the_file() {
        // The injection case: a newline in the title must not create a key.
        assert_eq!(
            sanitize_conf_value("Doom\nexecutable=evil.exe"),
            "Doom executable=evil.exe"
        );
        assert_eq!(sanitize_conf_value("  Hollow   Knight  "), "Hollow Knight");
        assert_eq!(sanitize_conf_value("Tabs\there"), "Tabs here");
        assert_eq!(sanitize_conf_value("\r\n\t "), "");
    }

    #[test]
    fn renders_a_conf_that_round_trips() {
        let conf = render_cartridge_conf("Hollow Knight", "steam://rungameid/367520", Some("cover.jpg"));
        assert!(conf.contains("title=Hollow Knight\n"));
        assert!(conf.contains("executable=steam://rungameid/367520\n"));
        assert!(conf.contains("cover=cover.jpg\n"));

        // No cover key when there is no cover.
        let bare = render_cartridge_conf("X", "steam://rungameid/1", None);
        assert!(!bare.contains("cover="));
    }

    #[test]
    fn accepts_the_known_uri_schemes() {
        let root = Path::new("/media/x");
        for good in [
            "steam://rungameid/367520",
            "heroic://launch/gog/1207658921",
            "STEAM://rungameid/1",
            "https://example.com/play",
        ] {
            assert!(validate_executable(good, root).is_ok(), "{good}");
        }
    }

    #[test]
    fn refuses_unknown_schemes() {
        let root = Path::new("/media/x");
        for bad in ["file:///etc/passwd", "javascript://alert(1)", "ftp://host/x"] {
            assert!(validate_executable(bad, root).is_err(), "{bad}");
        }
    }

    #[test]
    fn refuses_programs_that_are_not_on_the_cartridge() {
        let root = Path::new("/media/x");
        for bad in [
            "/usr/bin/bash",
            "../../../usr/bin/bash",
            "C:\\Windows\\System32\\cmd.exe",
            "Game/../../escape.exe",
        ] {
            assert!(validate_executable(bad, root).is_err(), "{bad}");
        }
        assert!(validate_executable("", root).is_err());
    }

    #[test]
    fn accepts_a_program_that_exists_on_the_cartridge() {
        let dir = std::env::temp_dir().join(format!("cart-exe-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Game/bin")).unwrap();
        std::fs::write(dir.join("Game/bin/start.sh"), b"#!/bin/sh\n").unwrap();

        assert!(validate_executable("Game/bin/start.sh", &dir).is_ok());
        assert!(validate_executable("Game/bin/missing.sh", &dir).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn refuses_targets_that_are_not_removable_drives() {
        // The wizard writes files, so this is the guard that matters most.
        for bad in ["/", "/home", "/etc", "/usr/local", ""] {
            assert!(
                resolve_target(bad).is_err(),
                "{bad} must never be a write target"
            );
        }
    }
}

//! The create-cartridge wizard: turn a drive into a cartridge.
//!
//! The full build, in order, each step optional except the last two:
//!
//!   1. format the drive to exFAT              (opt in, destructive)
//!   2. copy the game onto it and register the
//!      cartridge as a Steam library           (opt in, slow)
//!   3. copy the cover art
//!   4. write cartridge.conf                   (always)
//!   5. write autorun.inf for the drive's name and icon in Explorer
//!
//! Game lists come from Playnite when it is installed — it aggregates Steam,
//! GOG, Epic, Xbox, emulators and anything added by hand — and from Steam's own
//! manifests otherwise, which is also the only option on Linux.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::drives::{self, TargetDrive};
use crate::{autorun, format, playnite, portable, sgdb, steam, steamlib};

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

/// Where a game in the list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Library {
    Steam,
    Playnite,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    /// Steam app id, or Playnite GUID.
    pub id: String,
    pub name: String,
    pub library: Library,
    /// "Steam", "GOG", "Epic" … only Playnite reports this.
    pub source: String,
    pub size_on_disk: u64,
    pub has_cover: bool,
    /// What Play will start.
    pub executable: String,
    /// True when the game's files can be copied onto the cartridge.
    pub can_copy: bool,
}

/// One game's metadata when creating a multi-game bundle cartridge.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleGameRequest {
    pub title: String,
    pub executable: String,
    /// Steam app id, when the cover should come from Steam.
    #[serde(default)]
    pub app_id: Option<String>,
    /// Playnite GUID, when the cover should come from Playnite's cache.
    #[serde(default)]
    pub playnite_id: Option<String>,
    /// Absolute path to a user-chosen cover image for this game.
    #[serde(default)]
    pub cover_source: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeRequest {
    /// Target drive root. Re-checked here against the allowed list.
    pub drive_path: String,
    pub title: String,
    pub executable: String,
    /// Steam app id, when the cover and files should come from Steam.
    #[serde(default)]
    pub app_id: Option<String>,
    /// Playnite GUID, when the cover should come from Playnite's cache.
    #[serde(default)]
    pub playnite_id: Option<String>,
    /// Absolute path to a user-chosen cover image instead.
    #[serde(default)]
    pub cover_source: Option<String>,
    /// Format to exFAT first. `format_confirmation` must match the drive's
    /// current label or nothing happens.
    #[serde(default)]
    pub format_drive: bool,
    #[serde(default)]
    pub format_label: Option<String>,
    #[serde(default)]
    pub format_confirmation: Option<String>,
    /// Copy the game's files onto the cartridge.
    ///
    /// For a Steam game that also registers the drive as a Steam library. For
    /// anything else the folder is copied and `executable` is rewritten to point
    /// inside it.
    #[serde(default)]
    pub copy_game: bool,
    /// Which file inside the copied folder Play should start, relative to the
    /// game's install directory. Only used for non-Steam games.
    #[serde(default)]
    pub copy_executable: Option<String>,
    /// When set, create a multi-game bundle cartridge. `title` becomes the
    /// collection title; the top-level `executable` field is ignored (each
    /// game in the vec carries its own).
    #[serde(default)]
    pub games: Option<Vec<BundleGameRequest>>,
    /// Absolute path to the collection's cover image for a bundle.
    #[serde(default)]
    pub collection_cover_source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub step: &'static str,
    pub message: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CartridgeResult {
    pub conf_path: String,
    pub cover_written: bool,
    pub autorun_written: bool,
    pub icon: Option<String>,
    pub formatted: bool,
    pub game_copied: bool,
    pub bytes_copied: u64,
    pub registered_with_steam: bool,
    /// Where the game was copied to, relative to the cartridge root.
    pub game_folder: Option<String>,
    pub warnings: Vec<String>,
}

/// Every game the wizard can offer, Playnite first.
///
/// `playnite_root_override` lets the wizard pass a user-supplied path when
/// auto-discovery did not find Playnite. Corresponds to the `PLAYNITE_ROOT`
/// environment variable, but can be set per-invocation without touching the
/// environment.
pub fn list_games(playnite_root_override: Option<&str>) -> Result<Vec<GameInfo>, String> {
    let mut out = Vec::new();
    let mut problems = Vec::new();

    match playnite_games(playnite_root_override) {
        Ok(mut games) => out.append(&mut games),
        Err(e) => problems.push(e),
    }

    // Steam is still listed even with Playnite present: Playnite only knows
    // about games it has imported, and its export can be stale.
    match steam_games() {
        Ok(games) => {
            // Playnite's entry wins when both know a game, because it carries
            // the source label and launches through Playnite.
            let known: Vec<String> = out.iter().map(|g| g.name.to_lowercase()).collect();
            out.extend(
                games
                    .into_iter()
                    .filter(|g| !known.contains(&g.name.to_lowercase())),
            );
        }
        Err(e) => problems.push(e),
    }

    if out.is_empty() {
        return Err(if problems.is_empty() {
            "No installed games found.".to_string()
        } else {
            problems.join(" ")
        });
    }

    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(out)
}

fn playnite_games(playnite_root_override: Option<&str>) -> Result<Vec<GameInfo>, String> {
    let root = playnite_root_override
        .map(PathBuf::from)
        .or_else(playnite::playnite_root)
        .ok_or_else(|| "Playnite not found.".to_string())?;
    let exports = playnite::find_exports(&root);
    if exports.is_empty() {
        return Err(format!(
            "Playnite is installed at {} but has no JSON library export. \
             Install a JSON library exporter extension and run it.",
            root.display()
        ));
    }

    // Newest export wins, since several extensions may have written one.
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for path in exports {
        let modified = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            newest = Some((modified, path));
        }
    }
    let (_, path) = newest.expect("exports was not empty");

    let games = playnite::import_from(&path).map_err(|e| e.to_string())?;

    Ok(games
        .into_iter()
        .map(|g| GameInfo {
            executable: g.launch_uri(),
            has_cover: g
                .cover
                .as_deref()
                .and_then(|c| playnite::resolve_cover(&root, c))
                .is_some(),
            // Anything Playnite knows the install directory for can be copied
            // wholesale; Play then points at a file on the cartridge.
            can_copy: g.install_dir.is_some(),
            // Walk the install directory to report a real size. This is done
            // eagerly because the list is already being built; individual games
            // are typically 1-100 GB so the cost is paid once per wizard open.
            // If the directory has gone missing (e.g., game was uninstalled but
            // the export is stale) the size stays 0 rather than failing the
            // whole list.
            size_on_disk: g
                .install_dir
                .as_deref()
                .map(portable::tree_size_of)
                .unwrap_or(0),
            id: g.id,
            name: g.name,
            library: Library::Playnite,
            source: g.source,
        })
        .collect())
}

fn steam_games() -> Result<Vec<GameInfo>, String> {
    let root = steam::steam_root().ok_or_else(|| {
        "Could not find a Steam installation. Set STEAM_ROOT if it is somewhere unusual."
            .to_string()
    })?;

    let games = steam::installed_games(&root);
    if games.is_empty() {
        return Err(format!(
            "Found Steam at {} but no fully installed games.",
            root.display()
        ));
    }

    Ok(games
        .into_iter()
        .map(|g| GameInfo {
            executable: format!("steam://rungameid/{}", g.app_id),
            has_cover: g.cover_path.is_some(),
            can_copy: true,
            id: g.app_id,
            name: g.name,
            library: Library::Steam,
            source: "Steam".to_string(),
            size_on_disk: g.size_on_disk,
        })
        .collect())
}

/// Cover art for one game, as a data URI. Loaded one at a time: base64ing a
/// whole library at once would be tens of megabytes of IPC.
pub fn game_cover(library: Library, id: &str) -> String {
    let path = match library {
        Library::Steam => {
            if !is_numeric(id) {
                return String::new();
            }
            steam::steam_root()
                .and_then(|root| steam::find_cover(&root, id))
                .or_else(|| sgdb::last_used_artwork(&format!("steam:{id}")))
        }
        Library::Playnite => playnite::playnite_root()
            .and_then(|root| {
                let exports = playnite::find_exports(&root);
                exports
                    .iter()
                    .filter_map(|p| playnite::import_from(p).ok())
                    .flatten()
                    .find(|g| g.id == id)
                    .and_then(|g| g.cover)
                    .and_then(|c| playnite::resolve_cover(&root, &c))
            })
            .or_else(|| sgdb::last_used_artwork(&format!("playnite:{id}"))),
    };

    path.and_then(|p| read_as_data_uri(&p)).unwrap_or_default()
}

pub fn target_drives() -> Vec<TargetDrive> {
    drives::list_drives()
}

/// Whether this drive is currently listed as a Steam library folder.
pub fn steam_registration(drive_path: &str) -> bool {
    steam::steam_root()
        .map(|root| steamlib::is_registered(&root, Path::new(drive_path)))
        .unwrap_or(false)
}

/// Take a cartridge back out of Steam's library list.
///
/// For a cartridge that has been reformatted or repurposed. Entries are never
/// removed automatically — a cartridge is meant to spend most of its life
/// unplugged, so a missing folder is normal rather than stale.
pub fn unregister_from_steam(drive_path: &str) -> Result<bool, String> {
    let root = steam::steam_root().ok_or_else(|| "Could not find Steam.".to_string())?;
    steamlib::unregister_library(&root, Path::new(drive_path)).map_err(|e| e.to_string())
}

/// Describe what formatting a drive would destroy.
pub fn format_plan(path: &str) -> Result<format::FormatPlan, String> {
    format::plan(path).map_err(|e| e.to_string())
}

/// Build the cartridge.
pub fn create_cartridge(
    request: &CartridgeRequest,
    progress: &mut dyn FnMut(Progress),
) -> Result<CartridgeResult, String> {
    let mut warnings = Vec::new();
    let mut result = CartridgeResult {
        conf_path: String::new(),
        cover_written: false,
        autorun_written: false,
        icon: None,
        formatted: false,
        game_copied: false,
        bytes_copied: 0,
        registered_with_steam: false,
        game_folder: None,
        warnings: Vec::new(),
    };

    // Never trust the window's idea of where to write. The allowed set is
    // re-derived and an exact match required.
    let root = resolve_target(&request.drive_path)?;

    let title = sanitize_conf_value(&request.title);
    if title.is_empty() {
        return Err("Give the cartridge a title.".into());
    }

    // ---- 1. Format ------------------------------------------------------
    if request.format_drive {
        let label = request
            .format_label
            .clone()
            .unwrap_or_else(|| default_label(&title));
        let confirmation = request.format_confirmation.clone().unwrap_or_default();

        progress(Progress {
            step: "format",
            message: format!("Formatting {} to exFAT…", request.drive_path),
            done_bytes: 0,
            total_bytes: 0,
        });

        format::format_exfat(&request.drive_path, &label, &confirmation)
            .map_err(|e| e.to_string())?;
        result.formatted = true;

        // The mount point may take a moment to come back after mkfs.
        wait_for_mount(&root);
    }

    // ---- 2. Bundle mode: write each game, then bail early ----------------
    if let Some(bundle_games) = &request.games {
        if !bundle_games.is_empty() {
            // Validate every game's executable before writing anything.
            for g in bundle_games {
                let exec = sanitize_conf_value(&g.executable);
                validate_executable(&exec, &root)?;
            }

            // Collection cover.
            progress(Progress {
                step: "cover",
                message: "Copying collection cover art…".to_string(),
                done_bytes: 0,
                total_bytes: 0,
            });
            let coll_cover_src = request
                .collection_cover_source
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    request
                        .cover_source
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                });
            let coll_cover_dest = match write_cover_path(&root, coll_cover_src, "collection") {
                Ok(p) => p,
                Err(e) => {
                    warnings.push(format!("Collection cover art was not copied: {e}"));
                    None
                }
            };
            result.cover_written = coll_cover_dest.is_some();

            // Per-game covers.
            let mut game_tuples: Vec<(String, String, Option<String>)> = Vec::new();
            for (i, g) in bundle_games.iter().enumerate() {
                let game_title = sanitize_conf_value(&g.title);
                let game_exec = sanitize_conf_value(&g.executable);
                let game_cover_src = g
                    .cover_source
                    .as_deref()
                    .filter(|s| !s.trim().is_empty());
                let stem = format!("cover_{i}");
                let game_cover_name =
                    write_cover_path(&root, game_cover_src, &stem)
                        .unwrap_or(None)
                        .and_then(|p| {
                            p.file_name()
                                .and_then(|n| n.to_str())
                                .map(str::to_string)
                        });
                game_tuples.push((game_title, game_exec, game_cover_name));
            }

            // cartridge.conf in bundle format.
            let coll_cover_name = coll_cover_dest
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str());
            let tuples_ref: Vec<(&str, &str, Option<&str>)> = game_tuples
                .iter()
                .map(|(t, e, c)| (t.as_str(), e.as_str(), c.as_deref()))
                .collect();
            let conf = render_bundle_conf(&title, coll_cover_name, &tuples_ref);
            let conf_path = root.join("cartridge.conf");
            std::fs::write(&conf_path, conf)
                .map_err(|e| format!("Could not write {}: {e}", conf_path.display()))?;
            result.conf_path = conf_path.to_string_lossy().into_owned();

            // autorun.inf.
            progress(Progress {
                step: "autorun",
                message: "Naming the drive…".to_string(),
                done_bytes: 0,
                total_bytes: 0,
            });
            match autorun::write_autorun(&root, &title, coll_cover_dest.as_deref()) {
                Ok(icon) => {
                    result.autorun_written = true;
                    result.icon = icon;
                }
                Err(e) => warnings.push(format!("autorun.inf was not written: {e}")),
            }

            if !result.cover_written {
                warnings.push(
                    "No collection cover art on the cartridge. \
                     The launcher will show a placeholder."
                        .to_string(),
                );
            }

            result.warnings = warnings;
            return Ok(result);
        }
    }

    // ---- 2b. Copy the game (single-game only) ----------------------------
    //
    // Before validating the executable, because a generic copy *creates* the
    // file that Play will point at: the target does not exist on the cartridge
    // until the folder has been copied across.
    let mut executable = sanitize_conf_value(&request.executable);

    if request.copy_game {
        match copy_game(request, &root, progress) {
            Ok(Some(copied)) => {
                result.game_copied = true;
                result.bytes_copied = copied.bytes;
                result.registered_with_steam = copied.registered_with_steam;
                result.game_folder = copied.folder.clone();
                // A generic copy replaces the launch target with a path on the
                // cartridge; a Steam copy keeps its steam:// URI.
                if let Some(on_cartridge) = copied.executable {
                    executable = on_cartridge;
                }
            }
            Ok(None) => {}
            Err(e) => {
                // The cartridge is still worth finishing without the files.
                warnings.push(format!("The game was not copied: {e}"));
            }
        }
    }

    // ---- 3. Check what Play will start -----------------------------------
    validate_executable(&executable, &root)?;

    // ---- 4. Cover art ---------------------------------------------------
    progress(Progress {
        step: "cover",
        message: "Copying cover art…".to_string(),
        done_bytes: 0,
        total_bytes: 0,
    });

    let cover_destination = match write_cover(&root, request) {
        Ok(path) => path,
        Err(e) => {
            warnings.push(format!("Cover art was not copied: {e}"));
            None
        }
    };
    result.cover_written = cover_destination.is_some();

    // ---- 5. cartridge.conf ----------------------------------------------
    let conf = render_cartridge_conf(
        &title,
        &executable,
        cover_destination
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str()),
    );
    let conf_path = root.join("cartridge.conf");
    std::fs::write(&conf_path, conf)
        .map_err(|e| format!("Could not write {}: {e}", conf_path.display()))?;
    result.conf_path = conf_path.to_string_lossy().into_owned();

    // ---- 6. autorun.inf --------------------------------------------------
    progress(Progress {
        step: "autorun",
        message: "Naming the drive…".to_string(),
        done_bytes: 0,
        total_bytes: 0,
    });

    match autorun::write_autorun(&root, &title, cover_destination.as_deref()) {
        Ok(icon) => {
            result.autorun_written = true;
            result.icon = icon;
            if result.cover_written && result.icon.is_none() {
                warnings.push(
                    "Explorer needs an .ico for a custom drive icon and the cover is a JPEG, \
                     so the drive keeps its default icon. Drop a cover.ico on the cartridge \
                     to change that."
                        .to_string(),
                );
            }
        }
        Err(e) => warnings.push(format!("autorun.inf was not written: {e}")),
    }

    if !result.cover_written {
        warnings.push(
            "No cover art on the cartridge. The launcher will show a placeholder.".to_string(),
        );
    }

    result.warnings = warnings;
    Ok(result)
}

/// What a copy produced.
struct Copied {
    bytes: u64,
    /// Set when the launch target moved onto the cartridge.
    executable: Option<String>,
    /// Where the files landed, relative to the cartridge root.
    folder: Option<String>,
    registered_with_steam: bool,
}

/// Copy the game, by whichever route suits where it came from.
///
/// Steam games go through the library mechanism, because `steam://rungameid`
/// launches whatever copy Steam knows about and a loose folder would be ignored.
/// Everything else is a plain folder copy with Play pointed inside it, which is
/// the simpler and more honest arrangement — the cartridge really does carry the
/// game, with no launcher in the middle.
fn copy_game(
    request: &CartridgeRequest,
    root: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Option<Copied>, String> {
    if request.app_id.as_deref().is_some_and(is_numeric) {
        return copy_steam_game(request, root, progress);
    }
    copy_portable_game(request, root, progress)
}

/// Copy a self-contained game folder onto the cartridge.
fn copy_portable_game(
    request: &CartridgeRequest,
    root: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Option<Copied>, String> {
    let Some(source) = portable_source(request)? else {
        return Ok(None);
    };

    if !source.is_dir() {
        return Err(format!("{} is not there any more", source.display()));
    }

    let title = sanitize_conf_value(&request.title);
    let folder_name = portable::safe_folder_name(&title);
    // Everything the wizard copies lives under Games/, so the cartridge root
    // stays readable next to cartridge.conf and the cover.
    let relative_folder = format!("Games/{folder_name}");
    let destination = root.join("Games").join(&folder_name);

    let total = portable::tree_size_of(&source);
    let free = drives::list_drives()
        .into_iter()
        .find(|d| Path::new(&d.path) == root)
        .map(|d| d.free_bytes)
        .unwrap_or(0);
    if total > free {
        return Err(steamlib::LibraryError::NotEnoughSpace {
            needed: total,
            free,
        }
        .to_string());
    }

    // Decide what Play will run *before* copying, so a bad choice costs nothing.
    let chosen = choose_portable_executable(request, &source, &title)?;

    progress(Progress {
        step: "copy",
        message: format!("Copying {title}…"),
        done_bytes: 0,
        total_bytes: total,
    });

    let name = title.clone();
    let bytes = steamlib::copy_tree(&source, &destination, &mut |done| {
        progress(Progress {
            step: "copy",
            message: format!("Copying {name}…"),
            done_bytes: done,
            total_bytes: total,
        });
    })
    .map_err(|e| format!("{}: {e}", source.display()))?;

    Ok(Some(Copied {
        bytes,
        executable: Some(format!("{relative_folder}/{chosen}")),
        folder: Some(relative_folder),
        registered_with_steam: false,
    }))
}

/// Where a non-Steam game's files currently live.
fn portable_source(request: &CartridgeRequest) -> Result<Option<PathBuf>, String> {
    let Some(playnite_id) = request.playnite_id.as_deref() else {
        return Ok(None);
    };
    let root = playnite::playnite_root()
        .ok_or_else(|| "could not find Playnite, so there is no install directory".to_string())?;

    let game = playnite::find_exports(&root)
        .iter()
        .filter_map(|p| playnite::import_from(p).ok())
        .flatten()
        .find(|g| g.id == playnite_id)
        .ok_or_else(|| "that game is no longer in the Playnite export".to_string())?;

    match game.install_dir {
        Some(dir) => Ok(Some(dir)),
        None => Err("Playnite does not record an install directory for it".to_string()),
    }
}

/// The path inside the game folder that Play should start.
///
/// The caller's choice is honoured if it is a real file that stays inside the
/// folder; otherwise the best-ranked candidate is used.
fn choose_portable_executable(
    request: &CartridgeRequest,
    source: &Path,
    title: &str,
) -> Result<String, String> {
    if let Some(chosen) = request.copy_executable.as_deref().map(str::trim) {
        if !chosen.is_empty() {
            // The window supplied this, so it is checked the same way a
            // cartridge-supplied path would be.
            let relative = chosen.replace('\\', "/");
            if relative
                .split('/')
                .any(|part| part == ".." || part.is_empty())
                || Path::new(&relative).is_absolute()
                || relative.contains(':')
            {
                return Err(format!("{chosen} is not a path inside the game folder"));
            }
            if !source.join(&relative).is_file() {
                return Err(format!("{chosen} is not in the game folder"));
            }
            return Ok(relative);
        }
    }

    let play_action = playnite_play_action(request);
    portable::find_executables(source, title, play_action.as_deref())
        .into_iter()
        .next()
        .map(|c| c.relative)
        .ok_or_else(|| {
            "no program found in the game folder, so there would be nothing for Play to start"
                .to_string()
        })
}

fn playnite_play_action(request: &CartridgeRequest) -> Option<String> {
    let playnite_id = request.playnite_id.as_deref()?;
    let root = playnite::playnite_root()?;
    playnite::find_exports(&root)
        .iter()
        .filter_map(|p| playnite::import_from(p).ok())
        .flatten()
        .find(|g| g.id == playnite_id)
        .and_then(|g| g.play_action)
}

/// Candidates for what Play should start, best guess first.
pub fn executable_choices(playnite_id: &str) -> Result<Vec<portable::Candidate>, String> {
    let root = playnite::playnite_root().ok_or_else(|| "Could not find Playnite.".to_string())?;
    let game = playnite::find_exports(&root)
        .iter()
        .filter_map(|p| playnite::import_from(p).ok())
        .flatten()
        .find(|g| g.id == playnite_id)
        .ok_or_else(|| "That game is no longer in the Playnite export.".to_string())?;

    let dir = game
        .install_dir
        .ok_or_else(|| "Playnite does not record an install directory for it.".to_string())?;

    Ok(portable::find_executables(
        &dir,
        &game.name,
        game.play_action.as_deref(),
    ))
}

/// Copy a Steam game onto the cartridge and register it as a Steam library.
///
/// Returns the bytes copied and whether Steam was told about the drive, or
/// `None` when this cartridge is not a copyable Steam game.
fn copy_steam_game(
    request: &CartridgeRequest,
    root: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Option<Copied>, String> {
    let Some(app_id) = request.app_id.as_deref().filter(|id| is_numeric(id)) else {
        return Ok(None);
    };

    let steam_root =
        steam::steam_root().ok_or_else(|| steamlib::LibraryError::SteamNotFound.to_string())?;

    // Steam rewrites libraryfolders.vdf from memory when it exits, so a
    // registration made now would be silently undone.
    if steamlib::steam_is_running() {
        return Err(steamlib::LibraryError::SteamRunning.to_string());
    }

    let game = steamlib::locate(&steam_root, app_id)
        .ok_or_else(|| steamlib::LibraryError::GameNotFound(request.title.clone()).to_string())?;

    let total = if game.size_on_disk > 0 {
        game.size_on_disk
    } else {
        steamlib::tree_size(&game.install_path)
    };

    // Check space before starting a copy that could run for many minutes.
    let free = drives::list_drives()
        .into_iter()
        .find(|d| Path::new(&d.path) == root)
        .map(|d| d.free_bytes)
        .unwrap_or(0);
    if total > free {
        return Err(steamlib::LibraryError::NotEnoughSpace {
            needed: total,
            free,
        }
        .to_string());
    }

    let install_dir_name = game
        .install_path
        .file_name()
        .ok_or_else(|| "the game's install directory has no name".to_string())?;
    let destination = root.join("steamapps/common").join(install_dir_name);

    progress(Progress {
        step: "copy",
        message: format!("Copying {}…", game.name),
        done_bytes: 0,
        total_bytes: total,
    });

    let name = game.name.clone();
    let copied = steamlib::copy_tree(&game.install_path, &destination, &mut |done| {
        progress(Progress {
            step: "copy",
            message: format!("Copying {name}…"),
            done_bytes: done,
            total_bytes: total,
        });
    })
    .map_err(|e| format!("{}: {e}", game.install_path.display()))?;

    // The manifest is how Steam recognises the game in this library.
    let manifest_destination = root.join("steamapps").join(
        game.manifest_path
            .file_name()
            .ok_or_else(|| "the manifest has no filename".to_string())?,
    );
    std::fs::create_dir_all(root.join("steamapps"))
        .and_then(|_| std::fs::copy(&game.manifest_path, &manifest_destination).map(|_| ()))
        .map_err(|e| format!("could not copy the app manifest: {e}"))?;

    progress(Progress {
        step: "register",
        message: "Registering the cartridge with Steam…".to_string(),
        done_bytes: copied,
        total_bytes: total,
    });

    let registered = steamlib::register_library(&steam_root, root)
        .map_err(|e| e.to_string())?
        .is_some();

    Ok(Some(Copied {
        bytes: copied,
        // Steam launches by app id wherever the library lives, so the launch
        // target is unchanged.
        executable: None,
        folder: Some("steamapps/common".to_string()),
        registered_with_steam: registered,
    }))
}

/// After a format the mount point can briefly disappear.
fn wait_for_mount(root: &Path) {
    for _ in 0..40 {
        if root.is_dir() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// A default exFAT volume name derived from the title.
pub fn default_label(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let truncated: String = cleaned.chars().take(11).collect();
    let trimmed = truncated.trim().to_uppercase();
    if trimmed.is_empty() {
        "CARTRIDGE".to_string()
    } else {
        trimmed
    }
}

/// Check the requested drive is one we are actually willing to write to.
fn resolve_target(requested: &str) -> Result<PathBuf, String> {
    if requested.trim().is_empty() {
        return Err("Choose a drive first.".into());
    }
    let requested_path = Path::new(requested);

    let matched = drives::list_drives()
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

/// Copy the chosen art to the cartridge. Returns where it landed.
fn write_cover(root: &Path, request: &CartridgeRequest) -> Result<Option<PathBuf>, String> {
    let source = match (&request.cover_source, &request.app_id, &request.playnite_id) {
        (Some(path), _, _) if !path.trim().is_empty() => PathBuf::from(path),
        (_, Some(app_id), _) if is_numeric(app_id) => {
            let local =
                steam::steam_root().and_then(|steam_root| steam::find_cover(&steam_root, app_id));
            match local.or_else(|| sgdb::last_used_artwork(&format!("steam:{app_id}"))) {
                Some(p) => p,
                None => return Ok(None),
            }
        }
        (_, _, Some(playnite_id)) => {
            let root_dir = playnite::playnite_root()
                .ok_or_else(|| "no Playnite installation to take the cover from".to_string())?;
            let found = playnite::find_exports(&root_dir)
                .iter()
                .filter_map(|p| playnite::import_from(p).ok())
                .flatten()
                .find(|g| &g.id == playnite_id)
                .and_then(|g| g.cover)
                .and_then(|c| playnite::resolve_cover(&root_dir, &c));
            match found {
                Some(p) => p,
                None => match sgdb::last_used_artwork(&format!("playnite:{playnite_id}")) {
                    Some(p) => p,
                    None => return Ok(None),
                },
            }
        }
        _ => return Ok(None),
    };

    let meta = std::fs::metadata(&source).map_err(|e| format!("{}: {e}", source.display()))?;
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

    // Keep the source's extension so the launcher picks the right MIME type.
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = root.join(format!("cover.{extension}"));

    std::fs::copy(&source, &destination)
        .map_err(|e| format!("could not write {}: {e}", destination.display()))?;
    Ok(Some(destination))
}

/// Copy an image from an explicit `source_path` to `<root>/<stem>.<ext>`.
///
/// Used for bundle covers (collection + per-game) where the source is already
/// a concrete path rather than being looked up by app id or Playnite id.
/// Returns `None` (not an error) when `source_path` is absent.
fn write_cover_path(
    root: &Path,
    source_path: Option<&str>,
    stem: &str,
) -> Result<Option<PathBuf>, String> {
    let src_str = match source_path {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Ok(None),
    };
    let source = Path::new(src_str);
    let meta =
        std::fs::metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
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
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = root.join(format!("{stem}.{extension}"));
    std::fs::copy(source, &destination)
        .map_err(|e| format!("could not write {}: {e}", destination.display()))?;
    Ok(Some(destination))
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
        let is_drive_letter = colon == 1;
        if looks_like_scheme && !is_drive_letter && executable[colon..].starts_with("://") {
            return Err(format!(
                "{executable} uses a scheme this launcher will not open. Supported: {}",
                ALLOWED_SCHEMES.join(", ")
            ));
        }
    }

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

/// Render the bundle (multi-game) conf, with a `[collection]` header and one
/// `[game]` block per game.
pub fn render_bundle_conf(
    collection_title: &str,
    collection_cover: Option<&str>,
    games: &[(&str, &str, Option<&str>)],
) -> String {
    let mut out = String::new();
    out.push_str("# PC Cartridge System\n");
    out.push_str("# Written by the create-cartridge wizard. Safe to edit by hand.\n");
    out.push('\n');
    out.push_str("[collection]\n");
    out.push_str(&format!("title={collection_title}\n"));
    if let Some(cover) = collection_cover {
        out.push_str(&format!("cover={cover}\n"));
    }
    out.push('\n');
    for (title, executable, cover) in games {
        out.push_str("[game]\n");
        out.push_str(&format!("title={title}\n"));
        out.push_str(&format!("executable={executable}\n"));
        if let Some(c) = cover {
            out.push_str(&format!("cover={c}\n"));
        }
        out.push('\n');
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
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    };
    Some(format!(
        "data:{mime};base64,{}",
        crate::cartridge::base64_encode(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_values_that_would_corrupt_the_file() {
        assert_eq!(
            sanitize_conf_value("Doom\nexecutable=evil.exe"),
            "Doom executable=evil.exe"
        );
        assert_eq!(sanitize_conf_value("  Hollow   Knight  "), "Hollow Knight");
        assert_eq!(sanitize_conf_value("\r\n\t "), "");
    }

    #[test]
    fn renders_a_conf_that_round_trips() {
        let conf = render_cartridge_conf(
            "Hollow Knight",
            "steam://rungameid/367520",
            Some("cover.jpg"),
        );
        assert!(conf.contains("title=Hollow Knight\n"));
        assert!(conf.contains("executable=steam://rungameid/367520\n"));
        assert!(conf.contains("cover=cover.jpg\n"));
        assert!(!render_cartridge_conf("X", "steam://rungameid/1", None).contains("cover="));
    }

    #[test]
    fn accepts_known_schemes_including_playnite() {
        let root = Path::new("/media/x");
        for good in [
            "steam://rungameid/367520",
            "playnite://playnite/start/2b3c4d5e-1111-2222-3333-444455556666",
            "heroic://launch/gog/1207658921",
            "https://example.com/play",
        ] {
            assert!(validate_executable(good, root).is_ok(), "{good}");
        }
    }

    #[test]
    fn refuses_unknown_schemes_and_off_cartridge_programs() {
        let root = Path::new("/media/x");
        for bad in [
            "file:///etc/passwd",
            "javascript://alert(1)",
            "/usr/bin/bash",
            "../../../usr/bin/bash",
            "C:\\Windows\\System32\\cmd.exe",
            "",
        ] {
            assert!(validate_executable(bad, root).is_err(), "{bad}");
        }
    }

    #[test]
    fn refuses_targets_that_are_not_removable_drives() {
        for bad in ["/", "/home", "/etc", "/usr/local", ""] {
            assert!(resolve_target(bad).is_err(), "{bad} must never be a target");
        }
    }

    #[test]
    fn creating_a_cartridge_on_a_bad_target_writes_nothing() {
        let request = CartridgeRequest {
            drive_path: "/".to_string(),
            title: "Evil".to_string(),
            executable: "steam://rungameid/1".to_string(),
            format_drive: true,
            ..Default::default()
        };
        let mut seen = Vec::new();
        let err = create_cartridge(&request, &mut |p| seen.push(p)).unwrap_err();
        assert!(err.contains("not a removable drive"), "{err}");
        // Crucially, it bailed before the format step emitted anything.
        assert!(seen.is_empty(), "{seen:?}");
    }

    #[test]
    fn a_supplied_executable_must_stay_inside_the_game_folder() {
        let scratch = crate::testutil::Scratch::new("chosen");
        scratch.write("Game.exe", b"x");
        scratch.write("bin/run.exe", b"x");

        let pick = |chosen: &str| {
            let request = CartridgeRequest {
                title: "Game".into(),
                copy_executable: Some(chosen.to_string()),
                ..Default::default()
            };
            choose_portable_executable(&request, scratch.path(), "Game")
        };

        assert_eq!(pick("Game.exe").unwrap(), "Game.exe");
        // Backslashes are normalised, since the window may send either.
        assert_eq!(pick("bin\\run.exe").unwrap(), "bin/run.exe");

        // The window is not trusted with a path any more than a cartridge is.
        for bad in [
            "../../../etc/passwd",
            "bin/../../escape.exe",
            "/usr/bin/bash",
            "C:\\Windows\\System32\\cmd.exe",
            "missing.exe",
        ] {
            assert!(pick(bad).is_err(), "{bad} should have been refused");
        }
    }

    #[test]
    fn without_a_choice_the_best_candidate_is_used() {
        let scratch = crate::testutil::Scratch::new("auto");
        scratch.write("unins000.exe", b"x");
        scratch.write("Hollow Knight.exe", b"x");

        let request = CartridgeRequest {
            title: "Hollow Knight".into(),
            ..Default::default()
        };
        assert_eq!(
            choose_portable_executable(&request, scratch.path(), "Hollow Knight").unwrap(),
            "Hollow Knight.exe"
        );
    }

    #[test]
    fn a_folder_with_nothing_runnable_is_an_error_not_a_guess() {
        let scratch = crate::testutil::Scratch::new("norun");
        scratch.write("data.pak", b"x");
        let request = CartridgeRequest {
            title: "Empty".into(),
            ..Default::default()
        };
        let err = choose_portable_executable(&request, scratch.path(), "Empty").unwrap_err();
        assert!(err.contains("nothing for Play to start"), "{err}");
    }

    #[test]
    fn derives_a_valid_exfat_label_from_a_title() {
        assert_eq!(default_label("Hollow Knight"), "HOLLOW KNIG");
        assert_eq!(default_label("Cinder & Salt"), "CINDER SALT");
        assert_eq!(default_label("!!!"), "CARTRIDGE");
        assert_eq!(default_label(""), "CARTRIDGE");
        // Whatever it produces must pass the formatter's own check.
        for title in ["Hollow Knight", "Cinder & Salt", "!!!", "", "A"] {
            let label = default_label(title);
            assert!(
                format::check_label(&label).is_ok(),
                "{title:?} gave unusable label {label:?}"
            );
        }
    }
}

//! The create-cartridge wizard: turn a drive into a cartridge.
//!
//! The full build, in order, each step optional except the last two:
//!
//!   1. format the drive to btrfs or exFAT     (opt in, destructive)
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
use crate::{autorun, format, health, playnite, portable, sgdb, steam, steamlib, trim, verify};

/// Past this, a DRAM-less drive behind a USB bridge has little room for its
/// garbage collector to work in. Matches health.rs, which says the same thing
/// about a cartridge that is merely plugged in.
const CROWDED_PERCENT: u8 = 85;

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

/// The installed games, and why any library is missing from them.
///
/// The two are returned together because a partial list is still worth showing.
/// Steam almost always answers, so returning an error the moment Playnite fails
/// would throw away a usable list — but reporting only the games silently drops
/// the reason the other library is absent, which reads as "Playnite is not
/// supported" rather than "Playnite needs an exporter extension".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameList {
    pub games: Vec<GameInfo>,
    /// One line per library that could not be read. Empty on a clean run.
    pub problems: Vec<String>,
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
    /// Format the drive first. `format_confirmation` must match the drive's
    /// current label or nothing happens.
    #[serde(default)]
    pub format_drive: bool,
    #[serde(default)]
    pub format_filesystem: Option<format::Filesystem>,
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
    /// Close Steam if it is in the way.
    ///
    /// Steam holds its library list in memory and writes it out when it exits,
    /// so rewriting a cartridge it knows about means closing it first. With this
    /// set, Steam is asked to close itself and waited for; without it, the build
    /// stops and says so before anything has been changed.
    #[serde(default)]
    pub close_steam: bool,
    /// Read the cartridge back after copying and check every file against the
    /// sum taken as it was written. One extra pass over the drive, so it is
    /// asked for rather than assumed.
    #[serde(default)]
    pub verify_copy: bool,
    /// Ask the drive to release freed blocks once everything is written.
    ///
    /// Only worth it when the cartridge was not formatted first — mkfs already
    /// discards the whole volume — and it needs the same authentication prompt
    /// formatting does, so it is never implied.
    #[serde(default)]
    pub trim_after_write: bool,
}

impl CartridgeRequest {
    /// A single-game view of this request, so the copy helpers never have to
    /// know about bundles. The drive and the copy settings are shared; the
    /// game's own identity replaces the collection's.
    fn for_game(&self, game: &BundleGameRequest) -> CartridgeRequest {
        CartridgeRequest {
            title: game.title.clone(),
            executable: game.executable.clone(),
            app_id: game.app_id.clone(),
            playnite_id: game.playnite_id.clone(),
            cover_source: game.cover_source.clone(),
            games: None,
            // The format runs once, before any game is copied.
            format_drive: false,
            // Which file to start is picked per game by the ranking in
            // portable.rs: one dropdown per game would be more wizard than the
            // choice is worth.
            copy_executable: None,
            ..self.clone()
        }
    }
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
    pub formatted_filesystem: Option<format::Filesystem>,
    pub game_copied: bool,
    pub bytes_copied: u64,
    pub registered_with_steam: bool,
    /// Where the game was copied to, relative to the cartridge root.
    pub game_folder: Option<String>,
    /// How full the cartridge ended up, 0-100.
    pub used_percent: u8,
    /// What came of the TRIM, when one was asked for.
    pub trim: Option<String>,
    /// What came of the integrity check, when one was asked for.
    pub verified: Option<String>,
    /// True when Steam was closed to get at its library list.
    pub steam_closed: bool,
    /// True when a stale entry for this drive was taken out of that list.
    pub steam_entry_removed: bool,
    pub warnings: Vec<String>,
}

/// Every game the wizard can offer, Playnite first.
///
/// `playnite_root_override` lets the wizard pass a user-supplied path when
/// auto-discovery did not find Playnite. Corresponds to the `PLAYNITE_ROOT`
/// environment variable, but can be set per-invocation without touching the
/// environment.
pub fn list_games(playnite_root_override: Option<&str>) -> Result<GameList, String> {
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
    Ok(GameList {
        games: out,
        problems,
    })
}

fn playnite_games(playnite_root_override: Option<&str>) -> Result<Vec<GameInfo>, String> {
    let root = playnite_root_override
        .map(PathBuf::from)
        .or_else(playnite::playnite_root)
        .ok_or_else(|| "Playnite not found.".to_string())?;
    let (_, games) = playnite::import_newest_in(&root).map_err(|e| match e {
        playnite::ImportError::NotFound => format!(
            "Playnite is installed at {} but has no JSON library export. \
             Install a JSON library exporter extension and run it.",
            root.display()
        ),
        other => other.to_string(),
    })?;

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
        formatted_filesystem: None,
        game_copied: false,
        bytes_copied: 0,
        registered_with_steam: false,
        game_folder: None,
        used_percent: 0,
        trim: None,
        verified: None,
        steam_closed: false,
        steam_entry_removed: false,
        warnings: Vec::new(),
    };

    // Never trust the window's idea of where to write. The allowed set is
    // re-derived and an exact match required.
    let root = resolve_target(&request.drive_path)?;

    // Every file copied across, with the sum taken as it was written. Only
    // filled in when the build was asked to check its own work.
    let mut written: Vec<verify::FileDigest> = Vec::new();

    let title = sanitize_conf_value(&request.title);
    if title.is_empty() {
        return Err("Give the cartridge a title.".into());
    }

    // ---- 0. Steam's library list ----------------------------------------
    //
    // Rewriting a cartridge invalidates whatever Steam believed about it: the
    // game that entry describes is about to be formatted over, replaced, or
    // simply not be a Steam game any more. Left alone, Steam goes on listing a
    // game that is no longer there and offering to play it.
    //
    // This runs before anything is written, so a Steam that will not close
    // costs nothing — the alternative is discovering it after the drive has
    // already been formatted.
    let prepared = prepare_steam(request, &root, progress)?;
    result.steam_closed = prepared.closed;
    result.steam_entry_removed = prepared.entry_removed;
    warnings.extend(prepared.warnings);

    // ---- 1. Format ------------------------------------------------------
    if request.format_drive {
        let filesystem = request.format_filesystem.unwrap_or_default();
        let label = request
            .format_label
            .clone()
            .unwrap_or_else(|| default_label_for(filesystem, &title));
        let confirmation = request.format_confirmation.clone().unwrap_or_default();

        progress(Progress {
            step: "format",
            message: format!(
                "Formatting {} to {}…",
                request.drive_path,
                match filesystem {
                    format::Filesystem::Btrfs => "btrfs",
                    format::Filesystem::Exfat => "exFAT",
                }
            ),
            done_bytes: 0,
            total_bytes: 0,
        });

        format::format_drive(&request.drive_path, filesystem, &label, &confirmation)
            .map_err(|e| e.to_string())?;
        result.formatted = true;
        result.formatted_filesystem = Some(filesystem);

        // The mount point may take a moment to come back after mkfs.
        wait_for_mount(&root);
    }

    // ---- 2. Bundle mode: build every game, then bail early ---------------
    if let Some(bundle_games) = request.games.as_deref().filter(|g| !g.is_empty()) {
        // Nothing is written until every launch target that already exists
        // checks out. A copy creates its own target, so those are checked once
        // the files are there.
        if !request.copy_game {
            for game in bundle_games {
                validate_executable(&sanitize_conf_value(&game.executable), &root)?;
            }
        }

        // ---- Copy the games ----------------------------------------------
        //
        // Each game is copied as if it were the only one on the cartridge: the
        // copy helpers take a single-game view of the request and never learn
        // that this is a bundle.
        let mut entries: Vec<(String, String, Option<String>)> = Vec::new();

        for game in bundle_games {
            let game_title = sanitize_conf_value(&game.title);
            if game_title.is_empty() {
                return Err("Every game in a collection needs a title.".into());
            }
            let mut executable = sanitize_conf_value(&game.executable);

            if request.copy_game {
                let job = request.for_game(game);
                match copy_game(&job, &root, progress) {
                    Ok(Some(copied)) => {
                        result.game_copied = true;
                        result.bytes_copied += copied.bytes;
                        result.registered_with_steam |= copied.registered_with_steam;
                        if result.game_folder.is_none() {
                            result.game_folder = copied.folder.clone();
                        }
                        written.extend(copied.digests);
                        // A generic copy moves the launch target onto the
                        // cartridge; a Steam copy keeps its steam:// URI.
                        if let Some(on_cartridge) = copied.executable {
                            executable = on_cartridge;
                        }
                    }
                    Ok(None) => {}
                    // The cartridge is still worth finishing without the files.
                    Err(e) => warnings.push(format!("{game_title} was not copied: {e}")),
                }
                validate_executable(&executable, &root)?;
            }

            entries.push((game_title, executable, None));
        }

        // ---- Per-game cover art -------------------------------------------
        progress(Progress {
            step: "cover",
            message: "Copying cover art…".to_string(),
            done_bytes: 0,
            total_bytes: 0,
        });

        for (index, (game, entry)) in bundle_games.iter().zip(entries.iter_mut()).enumerate() {
            let source = match cover_source(
                game.cover_source.as_deref(),
                game.app_id.as_deref(),
                game.playnite_id.as_deref(),
            ) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(e) => {
                    warnings.push(format!("No cover for {}: {e}", entry.0));
                    continue;
                }
            };
            match copy_cover(&source, &root, &format!("cover_{index}")) {
                Ok(name) => entry.2 = Some(name),
                Err(e) => warnings.push(format!("Cover art for {} was not copied: {e}", entry.0)),
            }
        }

        // ---- The collection's own art -------------------------------------
        //
        // Whatever the wizard chose; failing that the first game's, so a
        // collection is never blank.
        let collection_art = match cover_source(
            request
                .collection_cover_source
                .as_deref()
                .or(request.cover_source.as_deref()),
            None,
            None,
        ) {
            Ok(found) => found,
            Err(e) => {
                warnings.push(format!("Collection cover art was not copied: {e}"));
                None
            }
        }
        .or_else(|| {
            let first = bundle_games.first()?;
            cover_source(
                first.cover_source.as_deref(),
                first.app_id.as_deref(),
                first.playnite_id.as_deref(),
            )
            .ok()
            .flatten()
        });

        let collection_cover = match collection_art {
            Some(source) => match copy_cover(&source, &root, "collection") {
                Ok(name) => Some(root.join(name)),
                Err(e) => {
                    warnings.push(format!("Collection cover art was not copied: {e}"));
                    None
                }
            },
            None => None,
        };
        result.cover_written = collection_cover.is_some();

        // ---- cartridge.conf -----------------------------------------------
        let tuples: Vec<(&str, &str, Option<&str>)> = entries
            .iter()
            .map(|(t, e, c)| (t.as_str(), e.as_str(), c.as_deref()))
            .collect();
        let conf = render_bundle_conf(
            &title,
            collection_cover
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            &tuples,
        );
        let conf_path = root.join("cartridge.conf");
        std::fs::write(&conf_path, conf)
            .map_err(|e| format!("Could not write {}: {e}", conf_path.display()))?;
        result.conf_path = conf_path.to_string_lossy().into_owned();

        // ---- autorun.inf ---------------------------------------------------
        progress(Progress {
            step: "autorun",
            message: "Naming the drive…".to_string(),
            done_bytes: 0,
            total_bytes: 0,
        });
        match autorun::write_autorun(&root, &title, collection_cover.as_deref()) {
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

        finish(
            request,
            &root,
            written,
            &mut result,
            &mut warnings,
            progress,
        );
        result.warnings = warnings;
        return Ok(result);
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
                written.extend(copied.digests);
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

    finish(
        request,
        &root,
        written,
        &mut result,
        &mut warnings,
        progress,
    );
    result.warnings = warnings;
    Ok(result)
}

/// The last thing every build does, whatever shape the cartridge was.
///
/// Releases freed blocks if asked, then reports how full the drive ended up —
/// which matters more here than on an internal disk, because a DRAM-less drive
/// behind a USB bridge has no host memory to lean on and needs the room.
fn finish(
    request: &CartridgeRequest,
    root: &Path,
    written: Vec<verify::FileDigest>,
    result: &mut CartridgeResult,
    warnings: &mut Vec<String>,
    progress: &mut dyn FnMut(Progress),
) {
    if request.verify_copy && !written.is_empty() {
        let manifest = verify::Manifest { files: written };
        let total = manifest.total_bytes();

        progress(Progress {
            step: "verify",
            message: "Reading the cartridge back…".to_string(),
            done_bytes: 0,
            total_bytes: total,
        });

        let problems = verify::verify(root, &manifest, &mut |done, total| {
            progress(Progress {
                step: "verify",
                message: "Reading the cartridge back…".to_string(),
                done_bytes: done,
                total_bytes: total,
            });
        });

        let files = manifest.files.len();
        if problems.is_empty() {
            result.verified = Some(format!(
                "Checked all {files} files against what was written; every one matches."
            ));
            // Left on the cartridge so it can be checked again later, on a
            // machine that no longer has the original.
            if let Err(e) = verify::write_manifest(root, &manifest) {
                warnings.push(format!("The file list was not saved to the cartridge: {e}"));
            }
        } else {
            // Named individually up to a point: "3 files are wrong" is not
            // actionable, and the first few are usually the whole story.
            let named: Vec<String> = problems
                .iter()
                .take(5)
                .map(verify::Problem::describe)
                .collect();
            let more = problems.len().saturating_sub(named.len());
            let tail = if more > 0 {
                format!(" (and {more} more)")
            } else {
                String::new()
            };
            let message = format!(
                "The copy did not survive: {}{tail}. Copy it again, and if it keeps happening \
                 try another cable or port before blaming the drive.",
                named.join("; ")
            );
            result.verified = Some(message.clone());
            warnings.push(message);
        }
    }

    if request.trim_after_write {
        progress(Progress {
            step: "trim",
            message: "Releasing free space back to the drive…".to_string(),
            done_bytes: 0,
            total_bytes: 0,
        });
        let outcome = trim::trim(&root.to_string_lossy());
        result.trim = Some(outcome.message());
    }

    let health = health::inspect(&root.to_string_lossy());
    result.used_percent = health.used_percent;
    if health.used_percent >= CROWDED_PERCENT {
        warnings.push(format!(
            "The cartridge is {}% full. These drives have no DRAM of their own and cannot \
             borrow host memory over USB, so keeping 15% or so spare is what keeps random \
             reads quick.",
            health.used_percent
        ));
    }
}

/// Will this build put the drive into Steam's library list?
///
/// True only when files are actually being copied *and* at least one of them is
/// a Steam game — both shapes of request can carry one, a single game at the top
/// level and a collection per game.
fn will_register_with_steam(request: &CartridgeRequest) -> bool {
    if !request.copy_game {
        return false;
    }
    if request.app_id.as_deref().is_some_and(is_numeric) {
        return true;
    }
    request.games.as_deref().is_some_and(|games| {
        games
            .iter()
            .any(|game| game.app_id.as_deref().is_some_and(is_numeric))
    })
}

/// What had to happen to Steam before the cartridge could be written.
struct SteamPrepared {
    closed: bool,
    entry_removed: bool,
    warnings: Vec<String>,
}

/// Get Steam out of the way, and take this drive out of its library list.
///
/// Both halves are conditional: there is nothing to do unless Steam is
/// installed and either already knows about this drive or is about to.
fn prepare_steam(
    request: &CartridgeRequest,
    root: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<SteamPrepared, String> {
    let mut prepared = SteamPrepared {
        closed: false,
        entry_removed: false,
        warnings: Vec::new(),
    };

    let Some(steam_root) = steam::steam_root() else {
        return Ok(prepared);
    };

    let registered = steamlib::is_registered(&steam_root, root);
    // A Steam copy will want to add this drive at the end, which is the same
    // file and so the same requirement.
    if !registered && !will_register_with_steam(request) {
        return Ok(prepared);
    }

    if steamlib::steam_is_running() {
        if !request.close_steam {
            return Err(
                "Steam is running, and it rewrites its library list from memory when it exits, \
                 so anything changed now would be undone. Close Steam, or tick \
                 “Close Steam if it is in the way”."
                    .to_string(),
            );
        }

        progress(Progress {
            step: "steam",
            message: "Asking Steam to close…".to_string(),
            done_bytes: 0,
            total_bytes: 0,
        });

        match steamlib::shutdown_steam(&steam_root) {
            steamlib::Shutdown::NotRunning => {}
            steamlib::Shutdown::Exited => prepared.closed = true,
            steamlib::Shutdown::StillRunning(why) => return Err(why),
        }
    }

    // Now that it is closed, drop the entry describing what used to be here.
    // A Steam copy adds a fresh one when it finishes; anything else leaves the
    // drive out of the list, which is correct, because it no longer holds the
    // game Steam thought it did.
    if registered {
        progress(Progress {
            step: "steam",
            message: "Taking the old cartridge out of Steam's library list…".to_string(),
            done_bytes: 0,
            total_bytes: 0,
        });

        match steamlib::unregister_library(&steam_root, root) {
            Ok(true) => prepared.entry_removed = true,
            Ok(false) => {}
            Err(e) => prepared
                .warnings
                .push(format!("Steam's library list was not updated: {e}")),
        }
    }

    Ok(prepared)
}

/// What a copy produced.
struct Copied {
    bytes: u64,
    /// Set when the launch target moved onto the cartridge.
    executable: Option<String>,
    /// Where the files landed, relative to the cartridge root.
    folder: Option<String>,
    registered_with_steam: bool,
    /// Every file that was written, and its sum, when the copy was asked to
    /// keep track. Empty otherwise.
    digests: Vec<crate::verify::FileDigest>,
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
    let mut digests = request.verify_copy.then(|| verify::Digests::new(root));
    let bytes =
        steamlib::copy_tree_digesting(&source, &destination, digests.as_mut(), &mut |done| {
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
        digests: digests.map(|d| d.into_manifest().files).unwrap_or_default(),
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
    let mut digests = request.verify_copy.then(|| verify::Digests::new(root));
    let copied = steamlib::copy_tree_digesting(
        &game.install_path,
        &destination,
        digests.as_mut(),
        &mut |done| {
            progress(Progress {
                step: "copy",
                message: format!("Copying {name}…"),
                done_bytes: done,
                total_bytes: total,
            });
        },
    )
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
        digests: digests.map(|d| d.into_manifest().files).unwrap_or_default(),
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

/// A default volume name derived from the title, for the default filesystem.
pub fn default_label(title: &str) -> String {
    default_label_for(format::Filesystem::default(), title)
}

/// A default volume name derived from the title.
///
/// Kept within the chosen filesystem's own limit, which is what makes the
/// difference visible: exFAT allows 11 characters, so *Hollow Knight* becomes
/// *Hollow Knig*, while btrfs has room for the whole thing.
pub fn default_label_for(filesystem: format::Filesystem, title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // 64 is well inside btrfs's 256 and long enough for any game's name; a
    // volume label the width of a sentence helps nobody.
    let limit = filesystem.label_limit().min(64);
    let truncated: String = cleaned.chars().take(limit).collect();
    let trimmed = truncated.trim().to_string();
    if trimmed.is_empty() {
        "Cartridge".to_string()
    } else {
        trimmed
    }
}

/// Check the requested drive is one we are actually willing to write to.
pub(crate) fn resolve_target(requested: &str) -> Result<PathBuf, String> {
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
    let Some(source) = cover_source(
        request.cover_source.as_deref(),
        request.app_id.as_deref(),
        request.playnite_id.as_deref(),
    )?
    else {
        return Ok(None);
    };
    copy_cover(&source, root, "cover").map(|name| Some(root.join(name)))
}

/// Where the art for one game currently lives.
///
/// A path chosen in the wizard wins; otherwise it comes from Steam's cache,
/// Playnite's, or the last artwork downloaded for this game, whichever the game
/// came from. `Ok(None)` means there is simply no art to copy, which is not an
/// error.
fn cover_source(
    chosen: Option<&str>,
    app_id: Option<&str>,
    playnite_id: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    if let Some(path) = chosen.map(str::trim).filter(|p| !p.is_empty()) {
        return Ok(Some(PathBuf::from(path)));
    }
    if let Some(app_id) = app_id.filter(|id| is_numeric(id)) {
        let local = steam::steam_root().and_then(|root| steam::find_cover(&root, app_id));
        return Ok(local.or_else(|| sgdb::last_used_artwork(&format!("steam:{app_id}"))));
    }
    if let Some(playnite_id) = playnite_id {
        let root_dir = playnite::playnite_root()
            .ok_or_else(|| "no Playnite installation to take the cover from".to_string())?;
        let found = playnite::find_exports(&root_dir)
            .iter()
            .filter_map(|p| playnite::import_from(p).ok())
            .flatten()
            .find(|g| g.id == playnite_id)
            .and_then(|g| g.cover)
            .and_then(|c| playnite::resolve_cover(&root_dir, &c));
        return Ok(found.or_else(|| sgdb::last_used_artwork(&format!("playnite:{playnite_id}"))));
    }
    Ok(None)
}

/// Copy art onto the cartridge as `<stem>.<its extension>`.
///
/// Returns the name relative to the cartridge root, which is what goes into
/// cartridge.conf.
pub(crate) fn copy_cover(source: &Path, root: &Path, stem: &str) -> Result<String, String> {
    let meta = std::fs::metadata(source).map_err(|e| format!("{}: {e}", source.display()))?;
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
    let relative = format!("{stem}.{extension}");

    std::fs::copy(source, root.join(&relative))
        .map_err(|e| format!("could not write {}: {e}", root.join(&relative).display()))?;
    Ok(relative)
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
    out.push_str("# PC GamePak\n");
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
    out.push_str("# PC GamePak\n");
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

/// A name for a cartridge carrying several games, from what they are called.
///
/// Sequels usually share their opening words — *God of War* and *God of War
/// Ragnarök* — so the shared run becomes the collection's name. When the titles
/// share nothing worth using, the count says what it is instead. The wizard
/// offers this; the user can always type their own.
pub fn suggest_collection_name(titles: &[String]) -> String {
    let cleaned: Vec<Vec<String>> = titles
        .iter()
        .map(|t| {
            sanitize_conf_value(t)
                .split_whitespace()
                .map(str::to_string)
                .collect()
        })
        .filter(|words: &Vec<String>| !words.is_empty())
        .collect();

    let Some(first) = cleaned.first() else {
        return "Collection".to_string();
    };
    if cleaned.len() == 1 {
        return first.join(" ");
    }

    // The longest run of opening words every title agrees on, compared without
    // case but kept in the first title's casing.
    let mut shared: Vec<&str> = Vec::new();
    for (index, word) in first.iter().enumerate() {
        let agreed = cleaned[1..].iter().all(|words| {
            words
                .get(index)
                .is_some_and(|other| other.to_lowercase() == word.to_lowercase())
        });
        if !agreed {
            break;
        }
        shared.push(word);
    }

    // One short shared word ("The", "Halo") names nothing on its own.
    let worth_using = shared.len() >= 2
        || shared
            .first()
            .is_some_and(|w| w.chars().count() >= 4 && !is_stop_word(w));

    if worth_using {
        format!("{} Collection", shared.join(" "))
    } else {
        format!("{} and {} more", first.join(" "), cleaned.len() - 1)
    }
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "the" | "a" | "an" | "of" | "and" | "for" | "in" | "on"
    )
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
    fn steam_is_only_disturbed_when_its_library_list_is_going_to_change() {
        // Not copying anything: Steam has no idea this is happening.
        let mut request = CartridgeRequest {
            app_id: Some("367520".into()),
            copy_game: false,
            ..Default::default()
        };
        assert!(!will_register_with_steam(&request));

        // Copying a Steam game does add the drive to the list.
        request.copy_game = true;
        assert!(will_register_with_steam(&request));

        // A GOG or itch game is copied without Steam ever hearing about it.
        request.app_id = Some("b7f3-tunic".into());
        assert!(!will_register_with_steam(&request));
        request.app_id = None;
        assert!(!will_register_with_steam(&request));

        // A collection counts if any one of its games is a Steam game.
        request.games = Some(vec![
            BundleGameRequest {
                playnite_id: Some("b7f3-tunic".into()),
                ..Default::default()
            },
            BundleGameRequest {
                app_id: Some("1593500".into()),
                ..Default::default()
            },
        ]);
        assert!(will_register_with_steam(&request));

        // And a collection of non-Steam games does not.
        request.games = Some(vec![BundleGameRequest {
            playnite_id: Some("b7f3-tunic".into()),
            ..Default::default()
        }]);
        assert!(!will_register_with_steam(&request));
    }

    #[test]
    fn a_per_game_request_keeps_the_drive_but_never_the_format() {
        let request = CartridgeRequest {
            drive_path: "/media/cart".into(),
            title: "God of War Collection".into(),
            collection_cover_source: Some("/pictures/collection.png".into()),
            format_drive: true,
            format_confirmation: Some("CART".into()),
            copy_game: true,
            copy_executable: Some("wrong-game.exe".into()),
            ..Default::default()
        };
        let game = BundleGameRequest {
            title: "God of War".into(),
            executable: "steam://rungameid/1593500".into(),
            app_id: Some("1593500".into()),
            ..Default::default()
        };

        let job = request.for_game(&game);
        assert_eq!(job.drive_path, "/media/cart");
        assert!(job.copy_game);
        assert_eq!(job.title, "God of War");
        assert_eq!(job.app_id.as_deref(), Some("1593500"));
        // Formatting once per game would wipe whatever was copied before it.
        assert!(!job.format_drive);
        // And the collection's own fields must not leak into a game: the
        // single-game executable pick belongs to a different game entirely.
        assert!(job.copy_executable.is_none());
        assert!(job.games.is_none());
    }

    #[test]
    fn art_is_copied_under_the_stem_it_was_given() {
        let scratch = crate::testutil::Scratch::new("cover-copy");
        scratch.write("source.PNG", b"not really a png");
        let root = scratch.path().join("cartridge");
        std::fs::create_dir_all(&root).unwrap();

        let name = copy_cover(&scratch.path().join("source.PNG"), &root, "cover_1").unwrap();

        // Lowercased extension, because that name goes straight into
        // cartridge.conf for the launcher to resolve.
        assert_eq!(name, "cover_1.png");
        assert!(root.join("cover_1.png").is_file());
    }

    #[test]
    fn art_that_is_missing_or_oversized_is_refused() {
        let scratch = crate::testutil::Scratch::new("cover-refuse");
        let root = scratch.path().join("cartridge");
        std::fs::create_dir_all(&root).unwrap();

        assert!(copy_cover(&scratch.path().join("nothing.jpg"), &root, "cover").is_err());

        scratch.write("huge.jpg", &vec![0u8; (MAX_COVER_BYTES + 1) as usize]);
        let err = copy_cover(&scratch.path().join("huge.jpg"), &root, "cover").unwrap_err();
        assert!(err.contains("the limit is"), "{err}");
        assert!(!root.join("cover.jpg").exists());
    }

    #[test]
    fn a_chosen_cover_beats_the_libraries() {
        // Nothing is looked up when the wizard supplied a path, so this holds
        // on a machine with neither Steam nor Playnite installed.
        let chosen = cover_source(Some("/pictures/gow.png"), Some("1593500"), None).unwrap();
        assert_eq!(chosen, Some(PathBuf::from("/pictures/gow.png")));

        // Blank counts as unset rather than as a path.
        assert!(cover_source(Some("   "), None, None).unwrap().is_none());
        assert!(cover_source(None, None, None).unwrap().is_none());
    }

    #[test]
    fn a_collection_is_named_after_what_the_games_share() {
        assert_eq!(
            suggest_collection_name(&["God of War".into(), "God of War Ragnarök".into(),]),
            "God of War Collection"
        );
        assert_eq!(
            suggest_collection_name(&["Mass Effect 2".into(), "Mass Effect 3".into()]),
            "Mass Effect Collection"
        );
        // Compared without case, but written in the first title's casing.
        assert_eq!(
            suggest_collection_name(&["Halo 3".into(), "HALO 3 ODST".into()]),
            "Halo 3 Collection"
        );
    }

    #[test]
    fn titles_with_nothing_in_common_are_counted_instead() {
        let name =
            suggest_collection_name(&["Hollow Knight".into(), "Hades".into(), "Tunic".into()]);
        assert_eq!(name, "Hollow Knight and 2 more");

        // A single shared stop word names nothing on its own.
        let name = suggest_collection_name(&["The Witness".into(), "The Talos Principle".into()]);
        assert_eq!(name, "The Witness and 1 more");
    }

    #[test]
    fn naming_a_collection_copes_with_nothing_to_go_on() {
        assert_eq!(suggest_collection_name(&["Solo".into()]), "Solo");
        assert_eq!(suggest_collection_name(&[]), "Collection");
        assert_eq!(suggest_collection_name(&["   ".into()]), "Collection");
    }

    #[test]
    fn what_the_wizard_writes_for_a_bundle_is_what_the_launcher_reads() {
        // The round trip is the property that matters: the writer and the
        // reader are in different modules and could drift apart.
        let scratch = crate::testutil::Scratch::new("bundle-round-trip");
        let root = scratch.path();
        scratch.write("gow.png", b"pretend png");

        let art = copy_cover(&root.join("gow.png"), root, "cover_0").unwrap();
        let conf = render_bundle_conf(
            "God of War Collection",
            Some("collection.jpg"),
            &[
                (
                    "God of War",
                    "steam://rungameid/1593500",
                    Some(art.as_str()),
                ),
                ("God of War Ragnarök", "steam://rungameid/2322010", None),
            ],
        );
        std::fs::write(root.join("cartridge.conf"), conf).unwrap();

        let info = crate::cartridge::read_cartridge_info(root.to_str().unwrap()).unwrap();

        assert!(info.is_bundle);
        assert_eq!(info.title, "God of War Collection");
        assert_eq!(info.games.len(), 2);
        assert_eq!(info.games[0].title, "God of War");
        assert_eq!(info.games[1].executable, "steam://rungameid/2322010");
        assert!(
            info.games[0].cover_path.ends_with("cover_0.png"),
            "{:?}",
            info.games[0].cover_path
        );
        // Enter still plays something: the first game is the primary target.
        assert_eq!(info.executable, "steam://rungameid/1593500");
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
    fn derives_a_label_the_chosen_filesystem_will_accept() {
        use format::Filesystem;

        // exFAT is the default, and its 11-character limit is the tight one.
        assert_eq!(default_label("Hollow Knight"), "Hollow Knig");
        assert_eq!(default_label("Cinder & Salt"), "Cinder Salt");
        assert_eq!(default_label("!!!"), "Cartridge");
        assert_eq!(default_label(""), "Cartridge");

        // btrfs has room for the whole name.
        assert_eq!(
            default_label_for(Filesystem::Btrfs, "Hollow Knight"),
            "Hollow Knight"
        );

        // Whatever it produces must pass the formatter's own check, for the
        // filesystem it was derived for.
        for filesystem in [Filesystem::Exfat, Filesystem::Btrfs] {
            for title in [
                "Hollow Knight",
                "Cinder & Salt",
                "!!!",
                "",
                "A",
                &"A".repeat(300),
            ] {
                let label = default_label_for(filesystem, title);
                assert!(
                    format::check_label_for(filesystem, &label).is_ok(),
                    "{title:?} on {filesystem:?} gave unusable label {label:?}"
                );
            }
        }
    }
}

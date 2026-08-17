// PC Cartridge Launcher — Tauri 2.0 backend
//
// Exposes three commands to the frontend:
//   parse_cartridge(drive_path)              -> CartridgeInfo (cover included)
//   launch_game(executable, drive_path)      -> ()
//   eject_drive(drive_path)                  -> ()
//
// There is deliberately no command that takes a path to read. An earlier
// read_image_as_data_uri(path) let the webview turn any file on the system into
// a data URI; the cover is now read here, from a path this file derives and
// confines to the cartridge itself.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// --------------------------------------------------------------------------
// Data types
// --------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct CartridgeInfo {
    /// Display title of the game / cartridge.
    title: String,
    /// Absolute path to the cover image, or empty string if none. Shown in the
    /// details sheet; never sent back to the backend.
    cover_path: String,
    /// The cover as a `data:` URI, or empty string if there is none.
    ///
    /// Inlined rather than served over the asset protocol: cartridge mount
    /// points are arbitrary, so a scope wide enough to serve them would be wide
    /// enough to serve anything on the machine.
    cover: String,
    /// The value from the `executable` / `open` key — either a URI or a
    /// relative path on the cartridge.
    executable: String,
    /// The root path of the cartridge drive as supplied by the caller.
    drive_path: String,
}

/// Largest cover we will base64 into the webview. A cartridge is not a trusted
/// input, and a 200 MB "cover" should fail rather than be inlined.
const MAX_COVER_BYTES: u64 = 8 * 1024 * 1024;

// --------------------------------------------------------------------------
// Inline INI / conf file parser
//
// Handles both Windows autorun.inf ([section] key=value) and our flat
// cartridge.conf (key=value, no sections required).
// --------------------------------------------------------------------------

type IniMap = HashMap<String, HashMap<String, String>>;

fn parse_ini(content: &str) -> IniMap {
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

fn ini_get<'a>(map: &'a IniMap, section: &str, key: &str) -> Option<&'a String> {
    map.get(section)?.get(key)
}

// --------------------------------------------------------------------------
// Helper: read cartridge metadata
//
// Priority:
//   1. cartridge.conf  (our own flat format, section "general" or none)
//   2. autorun.inf     (classic Windows autorun, section [autorun])
// --------------------------------------------------------------------------

fn read_cartridge_info(drive_path: &str) -> Result<CartridgeInfo, String> {
    let root = Path::new(drive_path);

    // ---- Try cartridge.conf first ----
    let conf_path = root.join("cartridge.conf");
    if conf_path.exists() {
        let content = std::fs::read_to_string(&conf_path)
            .map_err(|e| format!("Failed to read cartridge.conf: {e}"))?;

        let ini = parse_ini(&content);

        // cartridge.conf may have no section header — values land in "general"
        let executable = ini_get(&ini, "general", "executable")
            .cloned()
            .unwrap_or_default();

        let title = ini_get(&ini, "general", "title")
            .cloned()
            .unwrap_or_else(|| "Unknown Game".to_string());

        let cover_rel = ini_get(&ini, "general", "cover").cloned().unwrap_or_default();
        let cover_path = resolve_cover(root, &cover_rel);

        return Ok(CartridgeInfo {
            title,
            cover: cover_as_data_uri(&cover_path),
            cover_path,
            executable,
            drive_path: drive_path.to_string(),
        });
    }

    // ---- Fall back to autorun.inf ----
    let autorun_path = root.join("autorun.inf");
    if autorun_path.exists() {
        let content = std::fs::read_to_string(&autorun_path)
            .map_err(|e| format!("Failed to read autorun.inf: {e}"))?;

        let ini = parse_ini(&content);

        let executable = ini_get(&ini, "autorun", "open")
            .or_else(|| ini_get(&ini, "autorun", "shellexecute"))
            .cloned()
            .unwrap_or_default();

        let title = ini_get(&ini, "autorun", "label")
            .cloned()
            .unwrap_or_else(|| "Unknown Game".to_string());

        let icon_rel = ini_get(&ini, "autorun", "icon").cloned().unwrap_or_default();
        let cover_path = resolve_cover(root, &icon_rel);

        return Ok(CartridgeInfo {
            title,
            cover: cover_as_data_uri(&cover_path),
            cover_path,
            executable,
            drive_path: drive_path.to_string(),
        });
    }

    Err(format!(
        "No cartridge.conf or autorun.inf found in {drive_path}"
    ))
}

/// Resolve a relative cover image path, falling back to common filenames.
fn resolve_cover(root: &Path, rel: &str) -> String {
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
fn cover_as_data_uri(path: &str) -> String {
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
fn find_cover_image(root: &Path) -> String {
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

// --------------------------------------------------------------------------
// Tauri commands
// --------------------------------------------------------------------------

/// Parse the cartridge at `drive_path` and return metadata.
#[tauri::command]
fn parse_cartridge(drive_path: String) -> Result<CartridgeInfo, String> {
    read_cartridge_info(&drive_path)
}

/// The cartridge this window was started for.
///
/// The frontend asks for this rather than reading a query string: the window is
/// declared in tauri.conf.json and loads `index.html` with no parameters, so
/// there is nothing in the URL to read.
#[tauri::command]
fn drive_path() -> String {
    drive_from_args(std::env::args().skip(1))
}

/// Pull the value of `--drive` out of an argument list.
///
/// Accepts `--drive X` and `--drive=X`. Returns an empty string when absent,
/// which the frontend reports as "no cartridge" rather than guessing.
fn drive_from_args<I: Iterator<Item = String>>(args: I) -> String {
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
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
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

/// Launch the game.
/// `executable` can be a URI (steam://, heroic://, ...) or a path relative
/// to `drive_path`.
#[tauri::command]
fn launch_game(executable: String, drive_path: String) -> Result<(), String> {
    if executable.is_empty() {
        return Err("No executable configured for this cartridge".into());
    }

    let known_schemes = [
        "steam://",
        "heroic://",
        "gog://",
        "epic://",
        "playnite://",
        "lutris://",
        "http://",
        "https://",
    ];
    let is_uri = known_schemes
        .iter()
        .any(|s| executable.to_lowercase().starts_with(s));

    if is_uri {
        open_uri(&executable)
    } else {
        let full_path = PathBuf::from(&drive_path).join(&executable);
        if !full_path.exists() {
            return Err(format!("Executable not found: {}", full_path.display()));
        }
        #[cfg(target_os = "windows")]
        {
            Command::new(&full_path)
                .current_dir(full_path.parent().unwrap_or(Path::new(".")))
                .spawn()
                .map_err(|e| format!("Failed to launch {}: {e}", full_path.display()))?;
        }
        #[cfg(not(target_os = "windows"))]
        {
            Command::new("bash")
                .arg(&full_path)
                .current_dir(full_path.parent().unwrap_or(Path::new(".")))
                .spawn()
                .map_err(|e| format!("Failed to launch {}: {e}", full_path.display()))?;
        }
        Ok(())
    }
}

fn open_uri(uri: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/c", "start", "", uri])
            .spawn()
            .map_err(|e| format!("Failed to open URI {uri}: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(uri)
            .spawn()
            .map_err(|e| format!("Failed to open URI {uri}: {e}"))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(uri)
            .spawn()
            .map_err(|e| format!("Failed to open URI {uri}: {e}"))?;
        Ok(())
    }
}

/// Safely eject the cartridge drive.
#[tauri::command]
fn eject_drive(drive_path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        eject_windows(&drive_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        eject_linux(&drive_path)
    }
}

#[cfg(target_os = "windows")]
fn eject_windows(drive_path: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME};

    let letter = drive_path.trim_end_matches('\\').trim_end_matches('/');
    let volume_path = format!("\\\\.\\{letter}");

    let wide: Vec<u16> = OsStr::new(&volume_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            0,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return eject_windows_mountvol(drive_path);
    }

    let mut bytes_returned: u32 = 0;

    let locked = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_LOCK_VOLUME,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    if locked == 0 {
        unsafe { CloseHandle(handle) };
        return eject_windows_mountvol(drive_path);
    }

    let dismounted = unsafe {
        DeviceIoControl(
            handle,
            FSCTL_DISMOUNT_VOLUME,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        )
    };

    unsafe { CloseHandle(handle) };

    if dismounted != 0 {
        Ok(())
    } else {
        eject_windows_mountvol(drive_path)
    }
}

#[cfg(target_os = "windows")]
fn eject_windows_mountvol(drive_path: &str) -> Result<(), String> {
    let letter = drive_path.trim_end_matches('\\').trim_end_matches('/');
    let status = Command::new("mountvol")
        .args([letter, "/P"])
        .status()
        .map_err(|e| format!("mountvol failed: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "mountvol /P returned exit code {:?}",
            status.code()
        ))
    }
}

#[cfg(not(target_os = "windows"))]
fn eject_linux(drive_path: &str) -> Result<(), String> {
    let findmnt = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", drive_path])
        .output()
        .map_err(|e| format!("findmnt failed: {e}"))?;

    let device = String::from_utf8_lossy(&findmnt.stdout)
        .trim()
        .to_string();

    if device.is_empty() {
        return Err(format!("Cannot find block device for {drive_path}"));
    }

    let unmount = Command::new("udisksctl")
        .args(["unmount", "-b", &device, "--no-user-interaction"])
        .status()
        .map_err(|e| format!("udisksctl unmount failed: {e}"))?;

    if !unmount.success() {
        let _ = Command::new("umount").arg(&device).status();
    }

    let parent = get_parent_device(&device);
    let _ = Command::new("udisksctl")
        .args(["power-off", "-b", &parent, "--no-user-interaction"])
        .status();

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn get_parent_device(partition: &str) -> String {
    let out = Command::new("lsblk")
        .args(["-no", "PKNAME", partition])
        .output();
    if let Ok(o) = out {
        let parent_name = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !parent_name.is_empty() {
            return format!("/dev/{parent_name}");
        }
    }
    partition.to_string()
}

// --------------------------------------------------------------------------
// Entry point
// --------------------------------------------------------------------------

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            drive_path,
            parse_cartridge,
            launch_game,
            eject_drive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}

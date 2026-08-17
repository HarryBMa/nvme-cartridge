//! Finding drives the wizard is allowed to write a cartridge to.
//!
//! The filtering matters more than the listing: this tool writes files, so the
//! system disk and every ordinary system mount must never appear as a target.
//! On Unix that means only the automount directories a desktop uses for
//! removable media are considered.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// A drive the wizard can offer as a cartridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetDrive {
    /// Mount point / drive root: `/run/media/you/CART` or `D:\`.
    pub path: String,
    /// Short name for the list.
    pub label: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// True when there is already a cartridge.conf here, so the wizard can warn
    /// before overwriting someone else's cartridge.
    pub has_cartridge: bool,
}

/// Directories a Linux desktop automounts removable media into.
#[cfg(all(unix, not(target_os = "macos")))]
const AUTOMOUNT_ROOTS: [&str; 3] = ["/media", "/run/media", "/mnt"];

/// Whether a mount point may be offered as a cartridge target.
///
/// Deliberately a allowlist of automount locations rather than a denylist of
/// system paths: a new system mount showing up should fail closed.
pub fn is_writable_target(mount: &Path) -> bool {
    #[cfg(windows)]
    {
        // Drive letters are filtered by drive type instead; see list_drives.
        let _ = mount;
        true
    }
    #[cfg(target_os = "macos")]
    {
        mount.starts_with("/Volumes") && mount != Path::new("/Volumes")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        AUTOMOUNT_ROOTS.iter().any(|root| {
            let root = Path::new(root);
            // Must be *inside* an automount root, never the root itself.
            mount.starts_with(root) && mount != root
        })
    }
}

/// Filesystems that can never hold a cartridge.
pub fn is_pseudo_filesystem(fs: &str) -> bool {
    matches!(
        fs.to_ascii_lowercase().as_str(),
        "tmpfs"
            | "devtmpfs"
            | "proc"
            | "sysfs"
            | "cgroup"
            | "cgroup2"
            | "overlay"
            | "squashfs"
            | "autofs"
            | "devpts"
            | "mqueue"
            | "hugetlbfs"
            | "tracefs"
            | "debugfs"
            | "fusectl"
            | "configfs"
            | "securityfs"
            | "pstore"
            | "efivarfs"
            | "binfmt_misc"
            // read-only media cannot be written to
            | "iso9660"
            | "udf"
            // network shares are not cartridges
            | "nfs"
            | "nfs4"
            | "cifs"
            | "smbfs"
    )
}

/// One line of `/proc/mounts`, already unescaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub device: String,
    pub mount: PathBuf,
    pub fs_type: String,
    pub read_only: bool,
}

/// Parse `/proc/mounts`.
pub fn parse_proc_mounts(text: &str) -> Vec<MountEntry> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let device = fields.next()?;
            let mount = fields.next()?;
            let fs_type = fields.next()?;
            let options = fields.next().unwrap_or("");
            Some(MountEntry {
                device: unescape_mount(device),
                mount: PathBuf::from(unescape_mount(mount)),
                fs_type: fs_type.to_string(),
                read_only: options.split(',').any(|o| o == "ro"),
            })
        })
        .collect()
}

/// `/proc/mounts` escapes spaces and friends as octal.
fn unescape_mount(field: &str) -> String {
    if !field.contains('\\') {
        return field.to_string();
    }
    let bytes = field.as_bytes();
    let mut out = String::with_capacity(field.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() {
            let digits = &field[i + 1..i + 4];
            if let Ok(code) = u8::from_str_radix(digits, 8) {
                out.push(code as char);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Parse `df -P -k` output, returning (total_bytes, free_bytes).
///
/// `-P -k` rather than GNU's `-B1 --output=`: the POSIX form is understood by
/// coreutils, busybox and macOS alike.
pub fn parse_df(output: &str) -> Option<(u64, u64)> {
    for line in output.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }
        // Normally "Filesystem 1024-blocks Used Available …", but df wraps long
        // device names onto their own line, leaving the numbers first. Detect
        // which by whether the first field is a number.
        let numbers = if fields[0].parse::<u64>().is_ok() {
            &fields[..]
        } else {
            &fields[1..]
        };
        if numbers.len() < 3 {
            continue;
        }
        // blocks, used, available
        if let (Ok(total), Ok(avail)) = (numbers[0].parse::<u64>(), numbers[2].parse::<u64>()) {
            return Some((total * 1024, avail * 1024));
        }
    }
    None
}

/// Enumerate candidate cartridge drives.
pub fn list_drives() -> Vec<TargetDrive> {
    #[cfg(windows)]
    {
        windows_impl::list()
    }
    #[cfg(not(windows))]
    {
        unix_impl::list()
    }
}

/// True when this drive root already holds a cartridge.
fn has_cartridge(root: &Path) -> bool {
    root.join("cartridge.conf").is_file()
}

#[cfg(not(windows))]
mod unix_impl {
    use super::*;
    use std::process::Command;

    pub fn list() -> Vec<TargetDrive> {
        let Ok(text) = std::fs::read_to_string("/proc/mounts") else {
            // macOS has no /proc/mounts; fall back to listing /Volumes.
            return list_volumes_dir();
        };

        parse_proc_mounts(&text)
            .into_iter()
            .filter(|entry| {
                !entry.read_only
                    && !is_pseudo_filesystem(&entry.fs_type)
                    && is_writable_target(&entry.mount)
            })
            .map(|entry| describe(&entry.mount))
            .filter(|drive| drive.total_bytes > 0)
            .collect()
    }

    /// macOS path: every mounted volume appears under /Volumes.
    fn list_volumes_dir() -> Vec<TargetDrive> {
        let Ok(entries) = std::fs::read_dir("/Volumes") else {
            return Vec::new();
        };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir() && is_writable_target(p))
            .map(|p| describe(&p))
            .filter(|drive| drive.total_bytes > 0)
            .collect()
    }

    fn describe(mount: &Path) -> TargetDrive {
        let (total, free) = df(mount).unwrap_or((0, 0));
        TargetDrive {
            label: mount
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| mount.to_string_lossy().into_owned()),
            path: mount.to_string_lossy().into_owned(),
            total_bytes: total,
            free_bytes: free,
            has_cartridge: super::has_cartridge(mount),
        }
    }

    fn df(mount: &Path) -> Option<(u64, u64)> {
        let out = Command::new("df").arg("-P").arg("-k").arg(mount).output().ok()?;
        if !out.status.success() {
            return None;
        }
        parse_df(&String::from_utf8_lossy(&out.stdout))
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW,
    };

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    pub fn list() -> Vec<TargetDrive> {
        let mask = unsafe { GetLogicalDrives() };
        let mut out = Vec::new();

        for bit in 0..26u32 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let letter = (b'A' + bit as u8) as char;
            let root = format!("{letter}:\\");
            let wide = wide(&root);

            // A USB-C NVMe enclosure usually reports FIXED, not REMOVABLE, so
            // both are offered. Network drives and optical media are not.
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if kind != DRIVE_REMOVABLE && kind != DRIVE_FIXED {
                continue;
            }

            // Never offer the volume Windows itself booted from.
            if is_system_drive(letter) {
                continue;
            }

            let (total, free) = match disk_space(&wide) {
                Some(sizes) => sizes,
                None => continue, // no media in the slot
            };
            if total == 0 {
                continue;
            }

            let label = volume_label(&wide).unwrap_or_default();
            let path = Path::new(&root).to_path_buf();

            out.push(TargetDrive {
                label: if label.is_empty() {
                    format!("{letter}:")
                } else {
                    format!("{label} ({letter}:)")
                },
                path: root,
                total_bytes: total,
                free_bytes: free,
                has_cartridge: super::has_cartridge(&path),
            });
        }

        out
    }

    /// The drive holding Windows, from %SystemDrive% (e.g. "C:").
    fn is_system_drive(letter: char) -> bool {
        std::env::var("SystemDrive")
            .ok()
            .and_then(|s| s.chars().next())
            .map(|c| c.eq_ignore_ascii_case(&letter))
            .unwrap_or(letter == 'C')
    }

    fn disk_space(wide_root: &[u16]) -> Option<(u64, u64)> {
        let mut free_to_caller: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wide_root.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        (ok != 0).then_some((total, free_to_caller))
    }

    fn volume_label(wide_root: &[u16]) -> Option<String> {
        let mut name = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                wide_root.as_ptr(),
                name.as_mut_ptr(),
                name.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            )
        };
        if ok == 0 {
            return None;
        }
        let len = name.iter().position(|&c| c == 0).unwrap_or(0);
        Some(String::from_utf16_lossy(&name[..len]))
    }

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proc_mounts() {
        let text = "\
/dev/sda2 / ext4 rw,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/sdb1 /run/media/harry/CINDER exfat rw,nosuid,nodev,relatime,uid=1000 0 0
/dev/sr0 /media/harry/DVD iso9660 ro,nosuid,nodev,relatime 0 0
";
        let mounts = parse_proc_mounts(text);
        assert_eq!(mounts.len(), 4);
        assert_eq!(mounts[0].mount, Path::new("/"));
        assert_eq!(mounts[2].fs_type, "exfat");
        assert_eq!(mounts[2].mount, Path::new("/run/media/harry/CINDER"));
        assert!(!mounts[2].read_only);
        assert!(mounts[3].read_only);
    }

    #[test]
    fn unescapes_octal_in_mount_points() {
        let text = "/dev/sdb1 /run/media/harry/MY\\040CART exfat rw 0 0\n";
        let mounts = parse_proc_mounts(text);
        assert_eq!(mounts[0].mount, Path::new("/run/media/harry/MY CART"));
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn only_automounted_media_is_a_writable_target() {
        // The whole safety property of the wizard.
        for good in [
            "/media/harry/CART",
            "/run/media/harry/CART",
            "/mnt/cartridge",
        ] {
            assert!(is_writable_target(Path::new(good)), "{good} should be allowed");
        }
        for bad in [
            "/",
            "/home",
            "/home/harry",
            "/usr",
            "/etc",
            "/boot/efi",
            "/var/lib/docker",
            // the automount roots themselves are not drives
            "/media",
            "/run/media",
            "/mnt",
            // near-misses that must not slip through prefix matching
            "/mnt-backup",
            "/media-server/data",
        ] {
            assert!(!is_writable_target(Path::new(bad)), "{bad} must be refused");
        }
    }

    #[test]
    fn rejects_pseudo_and_readonly_filesystems() {
        for fs in ["tmpfs", "proc", "sysfs", "overlay", "iso9660", "nfs4", "cifs"] {
            assert!(is_pseudo_filesystem(fs), "{fs}");
        }
        for fs in ["ext4", "exfat", "vfat", "ntfs", "ntfs3", "btrfs", "xfs", "f2fs"] {
            assert!(!is_pseudo_filesystem(fs), "{fs} should be usable");
        }
        // Case from /proc/mounts is not guaranteed.
        assert!(is_pseudo_filesystem("TmpFS"));
    }

    #[test]
    fn parses_df_output() {
        let out = "\
Filesystem     1024-blocks     Used Available Capacity Mounted on
/dev/sdb1        124993536 30000000  94993536      25% /run/media/harry/CINDER
";
        let (total, free) = parse_df(out).unwrap();
        assert_eq!(total, 124_993_536 * 1024);
        assert_eq!(free, 94_993_536 * 1024);
    }

    #[test]
    fn parses_df_with_a_wrapped_device_name() {
        // Long device names push the numbers onto the next line.
        let out = "\
Filesystem                1024-blocks     Used Available Capacity Mounted on
/dev/mapper/a-very-long-device-name
                            124993536 30000000  94993536      25% /mnt/cart
";
        let (total, free) = parse_df(out).unwrap();
        assert_eq!(total, 124_993_536 * 1024);
        assert_eq!(free, 94_993_536 * 1024);
    }

    #[test]
    fn df_garbage_is_none_not_a_panic() {
        assert_eq!(parse_df(""), None);
        assert_eq!(parse_df("Filesystem 1024-blocks\n"), None);
        assert_eq!(parse_df("header\nnot numbers at all here\n"), None);
    }

    #[test]
    fn listing_drives_does_not_panic() {
        // Smoke test against the live mount table.
        let _ = list_drives();
    }
}

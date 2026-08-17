//! Cartridge detection.
//!
//! The launcher polls the mount table instead of subscribing to udev/WMI. That
//! sounds lazy, but it is the honest choice here:
//!
//! * A cartridge is only interesting once the desktop automounter has actually
//!   mounted it, which is strictly later than the kernel-level insert event. The
//!   shell helper already sleeps in a `findmnt` loop for exactly this reason.
//! * The same code path then works on Linux, Windows and macOS.
//! * The dock in this project holds 2.5" SATA/NVMe drives, which report
//!   `removable = 0`. Trusting the removable flag would miss every cartridge, so
//!   detection keys off "a volume appeared, and it has a manifest at its root".
//!
//! A one-second poll of the mount table is a few microseconds of work.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crate::manifest::MANIFEST_NAME;

pub const POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// A mounted volume that looks like it could be a cartridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Mount point: `/run/media/user/CART` or `D:\`.
    pub mount: PathBuf,
    /// Backing device where the platform exposes one (`/dev/sdb1`).
    pub device: Option<String>,
    /// Volume label as the OS reports it.
    pub label: Option<String>,
    pub file_system: Option<String>,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub removable: bool,
}

impl Volume {
    pub fn manifest_path(&self) -> PathBuf {
        self.mount.join(MANIFEST_NAME)
    }

    /// Short label for the data plate: the drive letter on Windows, the last
    /// path segment elsewhere.
    pub fn short_name(&self) -> String {
        if cfg!(windows) {
            return self.mount.to_string_lossy().trim_end_matches('\\').to_string();
        }
        self.mount
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.mount.to_string_lossy().into_owned())
    }
}

#[derive(Debug, Clone)]
pub enum VolumeEvent {
    Inserted(Volume),
    Removed(PathBuf),
}

/// Enumerate volumes that are plausible cartridge carriers.
pub fn list_volumes() -> Vec<Volume> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|disk| {
            let name = disk.name().to_string_lossy().into_owned();
            let fs = disk.file_system().to_string_lossy().into_owned();
            Volume {
                mount: disk.mount_point().to_path_buf(),
                // On Linux `name` is the device node; on Windows it is the label.
                device: name.starts_with("/dev/").then(|| name.clone()),
                label: (!name.is_empty() && !name.starts_with("/dev/")).then_some(name),
                file_system: (!fs.is_empty()).then_some(fs),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
                removable: disk.is_removable(),
            }
        })
        .filter(is_candidate_mount)
        .collect()
}

/// Reject the volumes that can never be a cartridge, cheaply, before touching
/// the filesystem.
///
/// This deliberately does *not* test `removable`: a 2.5" SATA SSD in a hot-swap
/// dock reports itself as fixed.
fn is_candidate_mount(volume: &Volume) -> bool {
    if volume.total_bytes == 0 {
        return false;
    }
    if let Some(fs) = &volume.file_system {
        if is_pseudo_filesystem(fs) {
            return false;
        }
    }
    is_user_mount_path(&volume.mount)
}

fn is_pseudo_filesystem(fs: &str) -> bool {
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
            | "fuse.portal"
            | "fuse.gvfsd-fuse"
            | "nfs"
            | "nfs4"
            | "cifs"
            | "smbfs"
            | "iso9660"
            | "udf"
    )
}

/// Where a desktop automounter puts a freshly inserted drive.
///
/// On Windows every volume is a drive-letter root, and we cannot narrow further
/// without excluding legitimate cartridges, so the manifest check does the real
/// filtering there. On Unix, restricting to the automount directories keeps the
/// root filesystem and every system mount out of the picture.
fn is_user_mount_path(mount: &Path) -> bool {
    if cfg!(windows) {
        return true;
    }
    if cfg!(target_os = "macos") {
        return mount.starts_with("/Volumes");
    }
    const AUTOMOUNT_ROOTS: [&str; 4] = ["/media", "/run/media", "/mnt", "/media/user"];
    AUTOMOUNT_ROOTS
        .iter()
        .any(|root| mount.starts_with(root) && mount != Path::new(root))
}

/// Spawn the poller. Volumes already mounted when the launcher starts are
/// reported as insertions on the first tick, so plugging a cartridge in before
/// login still shows the launcher.
pub fn watch() -> Receiver<VolumeEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("cartridge-volume-watcher".into())
        .spawn(move || {
            let mut known: HashMap<PathBuf, Volume> = HashMap::new();
            loop {
                let current: HashMap<PathBuf, Volume> = list_volumes()
                    .into_iter()
                    .map(|v| (v.mount.clone(), v))
                    .collect();

                for (mount, volume) in &current {
                    if !known.contains_key(mount) {
                        if tx.send(VolumeEvent::Inserted(volume.clone())).is_err() {
                            return;
                        }
                    }
                }
                for mount in known.keys() {
                    if !current.contains_key(mount) {
                        if tx.send(VolumeEvent::Removed(mount.clone())).is_err() {
                            return;
                        }
                    }
                }

                known = current;
                std::thread::sleep(POLL_INTERVAL);
            }
        })
        .expect("spawn volume watcher");
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(mount: &str, fs: &str, total: u64) -> Volume {
        Volume {
            mount: PathBuf::from(mount),
            device: Some("/dev/sdb1".into()),
            label: Some("CART".into()),
            file_system: Some(fs.into()),
            total_bytes: total,
            available_bytes: total / 2,
            removable: false,
        }
    }

    #[test]
    fn accepts_automounted_data_volumes_even_when_not_removable() {
        // The case that matters: a docked SATA SSD reporting removable = 0.
        assert!(is_candidate_mount(&volume(
            "/run/media/harry/CART",
            "exfat",
            512_000_000_000
        )));
        assert!(is_candidate_mount(&volume("/media/harry/CART", "ext4", 1_000)));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rejects_system_and_pseudo_mounts() {
        assert!(!is_candidate_mount(&volume("/", "ext4", 500_000)));
        assert!(!is_candidate_mount(&volume("/home", "ext4", 500_000)));
        assert!(!is_candidate_mount(&volume("/run/user/1000/doc", "tmpfs", 500)));
        assert!(!is_candidate_mount(&volume("/media/harry/CD", "iso9660", 700)));
        // A bare automount root is not itself a cartridge.
        assert!(!is_candidate_mount(&volume("/media", "ext4", 500_000)));
    }

    #[test]
    fn rejects_zero_sized_volumes() {
        assert!(!is_candidate_mount(&volume("/media/harry/CART", "ext4", 0)));
    }

    #[test]
    fn manifest_path_sits_at_the_volume_root() {
        let v = volume("/run/media/harry/CART", "exfat", 1_000);
        assert_eq!(
            v.manifest_path(),
            Path::new("/run/media/harry/CART/cartridge.toml")
        );
    }

    #[test]
    #[cfg(not(windows))]
    fn short_name_is_the_last_path_segment() {
        assert_eq!(volume("/run/media/harry/CART", "exfat", 1).short_name(), "CART");
    }

    #[test]
    fn enumerating_real_volumes_does_not_panic() {
        // Smoke test against the live mount table.
        let _ = list_volumes();
    }
}

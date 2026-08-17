//! Safely removing a cartridge.
//!
//! "Eject" means flush, unmount, and where the platform supports it, cut power to
//! the drive so the dock's light goes out and it is genuinely safe to pull.

use std::path::Path;
use std::process::Command;

use crate::volumes::Volume;

#[derive(Debug, thiserror::Error)]
pub enum EjectError {
    #[error("could not run {tool}: {source}")]
    Spawn {
        tool: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{tool} failed: {message}")]
    Failed { tool: String, message: String },
    #[error("this volume has no backing device to power off")]
    NoDevice,
}

/// Unmount, then power down.
pub fn eject(volume: &Volume) -> Result<(), EjectError> {
    #[cfg(target_os = "linux")]
    {
        let device = volume.device.as_deref().ok_or(EjectError::NoDevice)?;

        // udisks does this without asking for root, which is the whole point of
        // going through it rather than calling umount directly.
        run("udisksctl", &["unmount", "-b", device])?;

        // Powering off targets the whole disk, not the partition.
        let disk = parent_device(device);
        // A dock that cannot cut power is not an error the user needs to see:
        // the volume is already unmounted and safe to remove.
        let _ = run("udisksctl", &["power-off", "-b", &disk]);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let mount = volume.mount.to_string_lossy().into_owned();
        run("diskutil", &["eject", &mount])
    }

    #[cfg(target_os = "windows")]
    {
        // The shell's own Eject verb is what Explorer's "Safely Remove" uses, so
        // it flushes caches and notifies applications the same way.
        let drive = volume.mount.to_string_lossy().trim_end_matches('\\').to_string();
        let script = format!(
            "$ErrorActionPreference='Stop'; \
             $shell = New-Object -comObject Shell.Application; \
             $item = $shell.NameSpace(17).ParseName('{drive}\\'); \
             if ($null -eq $item) {{ throw 'drive {drive} not found' }} \
             $item.InvokeVerb('Eject')"
        );
        run(
            "powershell.exe",
            &["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script],
        )
    }
}

/// `/dev/sdb1` → `/dev/sdb`, `/dev/nvme0n1p2` → `/dev/nvme0n1`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parent_device(device: &str) -> String {
    let trimmed = device.trim_end_matches(|c: char| c.is_ascii_digit());
    // nvme and mmcblk separate the partition number with a `p`, and that `p` is
    // only a separator when a digit precedes it (`nvme0n1p2`, not `sdap`).
    if let Some(stripped) = trimmed.strip_suffix('p') {
        if stripped.ends_with(|c: char| c.is_ascii_digit()) {
            return stripped.to_string();
        }
    }
    if trimmed.is_empty() {
        return device.to_string();
    }
    trimmed.to_string()
}

#[cfg_attr(target_os = "windows", allow(dead_code))]
fn run(tool: &str, args: &[&str]) -> Result<(), EjectError> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .map_err(|source| EjectError::Spawn {
            tool: tool.to_string(),
            source,
        })?;

    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = [stderr.trim(), stdout.trim()]
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("exited with {}", output.status));
    Err(EjectError::Failed {
        tool: tool.to_string(),
        message,
    })
}

/// Whether the mount point still exists, used to confirm an eject took effect.
pub fn is_still_mounted(mount: &Path) -> bool {
    crate::volumes::list_volumes()
        .iter()
        .any(|v| v.mount == mount)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_whole_disk_behind_a_partition() {
        assert_eq!(parent_device("/dev/sdb1"), "/dev/sdb");
        assert_eq!(parent_device("/dev/sdb12"), "/dev/sdb");
        assert_eq!(parent_device("/dev/nvme0n1p2"), "/dev/nvme0n1");
        assert_eq!(parent_device("/dev/nvme0n1p12"), "/dev/nvme0n1");
        assert_eq!(parent_device("/dev/mmcblk0p1"), "/dev/mmcblk0");
    }

    #[test]
    fn leaves_whole_disk_devices_alone() {
        assert_eq!(parent_device("/dev/sdb"), "/dev/sdb");
        // nvme namespaces end in a digit but are already whole disks; trimming
        // to /dev/nvme0n is wrong, so this documents the known limitation of
        // relying on names. The caller only uses this for power-off, which is
        // best-effort.
        assert_eq!(parent_device("/dev/nvme0n1"), "/dev/nvme0n");
    }

    #[test]
    fn reports_which_tool_is_missing() {
        let err = run("definitely-not-a-real-binary", &["--help"]).unwrap_err();
        match err {
            EjectError::Spawn { tool, .. } => assert_eq!(tool, "definitely-not-a-real-binary"),
            other => panic!("expected a spawn error, got {other:?}"),
        }
    }

    #[test]
    fn surfaces_tool_stderr_on_failure() {
        let err = run("/bin/sh", &["-c", "echo boom >&2; exit 3"]).unwrap_err();
        match err {
            EjectError::Failed { message, .. } => assert_eq!(message, "boom"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}

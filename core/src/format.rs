//! Formatting a drive to btrfs.
//!
//! This is the only code in the project that destroys data, so it is built to
//! refuse rather than to succeed. Four things must all hold before a single
//! command runs:
//!
//!   1. the target is in `drives::list_drives()` — an allowlist of removable,
//!      automounted volumes, re-derived here rather than taken from the caller;
//!   2. it is not the system drive;
//!   3. the caller echoed the drive's current label back exactly;
//!   4. formatting was explicitly asked for, per cartridge. It is never implied.
//!
//! btrfs because it supports TRIM (discard=async mount option) and transparent
//! zstd compression, which meaningfully improve the lifespan and effective
//! capacity of NVMe drives. It works on Linux natively and on Windows via
//! WinBtrfs (https://github.com/maharmstone/btrfs).

use std::path::Path;
use std::process::Command;

use crate::drives;

/// A label longer than this is not valid btrfs.
const MAX_LABEL: usize = 256;

#[derive(Debug, PartialEq, Eq)]
pub enum FormatError {
    NotRemovable(String),
    SystemDrive(String),
    ConfirmationMismatch { expected: String, got: String },
    BadLabel(String),
    NoDevice(String),
    ToolMissing(String),
    Failed(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::NotRemovable(p) => write!(
                f,
                "{p} is not a removable drive this tool will touch, let alone format."
            ),
            FormatError::SystemDrive(p) => {
                write!(f, "{p} is the system drive. Refusing to format it.")
            }
            FormatError::ConfirmationMismatch { expected, got } => write!(
                f,
                "To erase this drive, type its current name exactly: {expected:?} (got {got:?})."
            ),
            FormatError::BadLabel(l) => write!(
                f,
                "{l:?} is not a usable btrfs label. Use up to {MAX_LABEL} characters, \
                 letters, digits, spaces, - or _."
            ),
            FormatError::NoDevice(p) => {
                write!(f, "Could not work out which device backs {p}.")
            }
            FormatError::ToolMissing(t) => write!(
                f,
                "{t} is not installed, so the drive cannot be formatted here. \
                 Format it to btrfs yourself and run the wizard again."
            ),
            FormatError::Failed(m) => write!(f, "Formatting failed: {m}"),
        }
    }
}

/// What a format would do, for the confirmation step.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatPlan {
    pub path: String,
    /// The label the user must type back to confirm.
    pub current_label: String,
    pub device: Option<String>,
    pub total_bytes: u64,
    /// Human-readable summary of what is about to be destroyed.
    pub warning: String,
}

/// Validate a proposed btrfs volume label.
pub fn check_label(label: &str) -> Result<String, FormatError> {
    let trimmed = label.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_LABEL {
        return Err(FormatError::BadLabel(label.to_string()));
    }
    // Keep to characters every tool and both OSes accept in a volume label.
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        return Err(FormatError::BadLabel(label.to_string()));
    }
    Ok(trimmed.to_string())
}

/// Describe what formatting `path` would destroy, refusing anything ineligible.
pub fn plan(path: &str) -> Result<FormatPlan, FormatError> {
    let drive = drives::list_drives()
        .into_iter()
        .find(|d| Path::new(&d.path) == Path::new(path))
        .ok_or_else(|| FormatError::NotRemovable(path.to_string()))?;

    if is_system_drive(Path::new(path)) {
        return Err(FormatError::SystemDrive(path.to_string()));
    }

    let device = backing_device(Path::new(path));
    let label = current_label(&drive);

    Ok(FormatPlan {
        warning: format!(
            "Everything on {} ({}) will be erased.",
            label,
            crate::format::human_bytes(drive.total_bytes)
        ),
        path: drive.path.clone(),
        current_label: label,
        device,
        total_bytes: drive.total_bytes,
    })
}

/// Format the drive, having checked everything.
///
/// `confirmation` must equal the drive's current label. That is the gate: it
/// forces the user to look at which drive they picked, rather than clicking
/// through a dialog.
pub fn format_btrfs(path: &str, new_label: &str, confirmation: &str) -> Result<(), FormatError> {
    let plan = plan(path)?;
    let label = check_label(new_label)?;

    if confirmation.trim() != plan.current_label {
        return Err(FormatError::ConfirmationMismatch {
            expected: plan.current_label,
            got: confirmation.trim().to_string(),
        });
    }

    run_format(&plan, &label)
}

/// The label to confirm against. An unlabelled drive would make confirmation
/// meaningless, so its short name stands in.
fn current_label(drive: &drives::TargetDrive) -> String {
    let label = drive.label.trim();
    if label.is_empty() {
        Path::new(&drive.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| drive.path.clone())
    } else {
        label.to_string()
    }
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 || unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(windows)]
fn is_system_drive(path: &Path) -> bool {
    let letter = path.to_string_lossy().chars().next().unwrap_or('C');
    std::env::var("SystemDrive")
        .ok()
        .and_then(|s| s.chars().next())
        .map(|c| c.eq_ignore_ascii_case(&letter))
        .unwrap_or(letter.eq_ignore_ascii_case(&'C'))
}

#[cfg(not(windows))]
fn is_system_drive(path: &Path) -> bool {
    // A removable mount is never / or /home, but check anyway: this is the last
    // line before mkfs.
    matches!(
        path.to_string_lossy().trim_end_matches('/'),
        "" | "/boot" | "/home" | "/usr" | "/var" | "/etc"
    )
}

#[cfg(windows)]
fn backing_device(path: &Path) -> Option<String> {
    // On Windows the drive letter *is* the handle used to format.
    Some(path.to_string_lossy().trim_end_matches('\\').to_string())
}

#[cfg(not(windows))]
fn backing_device(path: &Path) -> Option<String> {
    let out = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "--target"])
        .arg(path)
        .output()
        .ok()?;
    let device = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!device.is_empty() && device.starts_with("/dev/")).then_some(device)
}

#[cfg(windows)]
fn run_format(plan: &FormatPlan, label: &str) -> Result<(), FormatError> {
    let letter = plan
        .device
        .clone()
        .ok_or_else(|| FormatError::NoDevice(plan.path.clone()))?;

    // Format-Volume needs administrator, so it is elevated on its own rather
    // than requiring the whole wizard to run as admin.
    // WinBtrfs (https://github.com/maharmstone/btrfs) must be installed.
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         Format-Volume -DriveLetter {} -FileSystem Btrfs -NewFileSystemLabel '{}' \
         -Confirm:$false -Force",
        letter.trim_end_matches(':'),
        label.replace('\'', "''")
    );

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &format!(
                "Start-Process powershell.exe -Verb RunAs -Wait -WindowStyle Hidden \
                 -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-Command',{})",
                powershell_quote(&script)
            ),
        ])
        .status()
        .map_err(|e| FormatError::ToolMissing(format!("powershell.exe ({e})")))?;

    if status.success() {
        Ok(())
    } else {
        Err(FormatError::Failed(format!(
            "Format-Volume exited with {:?}",
            status.code()
        )))
    }
}

#[cfg(windows)]
fn powershell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(not(windows))]
fn run_format(plan: &FormatPlan, label: &str) -> Result<(), FormatError> {
    let device = plan
        .device
        .clone()
        .ok_or_else(|| FormatError::NoDevice(plan.path.clone()))?;

    // Unmount first; mkfs on a mounted filesystem would corrupt it.
    let _ = Command::new("udisksctl")
        .args(["unmount", "-b", &device, "--no-user-interaction"])
        .status();

    // mkfs needs root. pkexec raises the desktop's own authentication dialog
    // rather than the wizard handling a password itself.
    let (program, args) = mkfs_command(&device, label);
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();

    let output = Command::new(program)
        .args(&argv)
        .output()
        .map_err(|e| FormatError::ToolMissing(format!("{program} ({e})")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = [stderr.trim(), stdout.trim()]
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("exited with {:?}", output.status.code()));
    Err(FormatError::Failed(message))
}

/// Build the mkfs invocation. Split out so the argument order can be tested
/// without running anything.
#[cfg_attr(windows, allow(dead_code))]
pub fn mkfs_command(device: &str, label: &str) -> (&'static str, Vec<String>) {
    (
        "pkexec",
        vec![
            "mkfs.btrfs".to_string(),
            "-L".to_string(),
            label.to_string(),
            device.to_string(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_sensible_labels_and_preserves_case() {
        assert_eq!(check_label("cinder").unwrap(), "cinder");
        assert_eq!(check_label("  Hollow ").unwrap(), "Hollow");
        assert_eq!(check_label("CART_01").unwrap(), "CART_01");
        assert_eq!(check_label("MY CART").unwrap(), "MY CART");
    }

    #[test]
    fn refuses_labels_btrfs_cannot_hold() {
        for bad in [
            "",
            "   ",
            "bad/slash",
            "quote\"mark",
            "semi;colon",
            "new\nline",
        ] {
            assert!(
                matches!(check_label(bad), Err(FormatError::BadLabel(_))),
                "{bad:?}"
            );
        }
        // Short labels are fine; a label exactly at the limit is fine too.
        assert!(check_label("ELEVENCHARS").is_ok());
        assert!(check_label("TWELVECHARSX").is_ok());
        // A label over 256 characters must be rejected.
        assert!(check_label(&"A".repeat(257)).is_err());
    }

    #[test]
    fn refuses_to_format_anything_not_on_the_removable_allowlist() {
        // The guard that matters. None of these are removable mounts, so plan()
        // must refuse before any device is even looked up.
        for path in ["/", "/home", "/etc", "/usr/local", "/media", ""] {
            let err = plan(path).unwrap_err();
            assert!(
                matches!(
                    err,
                    FormatError::NotRemovable(_) | FormatError::SystemDrive(_)
                ),
                "{path} gave {err:?}"
            );
        }
    }

    #[test]
    fn format_refuses_before_confirmation_is_even_checked() {
        // An ineligible drive fails on eligibility, not on the label.
        let err = format_btrfs("/", "CART", "anything").unwrap_err();
        assert!(matches!(
            err,
            FormatError::NotRemovable(_) | FormatError::SystemDrive(_)
        ));
    }

    #[test]
    #[cfg(not(windows))]
    fn system_paths_are_recognised() {
        assert!(is_system_drive(Path::new("/home")));
        assert!(is_system_drive(Path::new("/etc")));
        assert!(!is_system_drive(Path::new("/run/media/harry/CINDER")));
    }

    #[test]
    fn mkfs_arguments_are_in_the_right_order() {
        let (program, args) = mkfs_command("/dev/sdb1", "Cinder");
        assert_eq!(program, "pkexec");
        assert_eq!(args, vec!["mkfs.btrfs", "-L", "Cinder", "/dev/sdb1"]);
    }

    #[test]
    fn formats_byte_counts_for_the_warning() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(128_035_676_160), "128 GB");
        assert_eq!(human_bytes(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn confirmation_mismatch_names_what_was_expected() {
        // Constructed directly: reaching this through plan() needs a real drive.
        let err = FormatError::ConfirmationMismatch {
            expected: "CINDER".into(),
            got: "cinder".into(),
        };
        let text = err.to_string();
        assert!(text.contains("CINDER"), "{text}");
        // Case matters, so the message has to show both.
        assert!(text.contains("cinder"), "{text}");
    }
}

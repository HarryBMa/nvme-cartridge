//! SHA-256 trust list, sharing the exact file and format the shell installers
//! already use: one lowercase hex digest per line.
//!
//! * Linux/macOS `~/.config/pc-cartridge-system/trusted_scripts.sha256`
//! * Windows     `%LOCALAPPDATA%\PC-Cartridge-System\trusted_scripts.sha256`
//!
//! Using the same file means a cartridge trusted with `cartridge-linux.sh`
//! menu option 2 is already trusted here, and vice versa.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::manifest::LaunchTarget;

pub const TRUST_FILE_NAME: &str = "trusted_scripts.sha256";

/// Where the shell scripts keep their state. Matches
/// `linux/cartridge-launcher-helper.sh` and `windows/cartridge-monitoring.ps1`.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        dirs::data_local_dir().map(|d| d.join("PC-Cartridge-System"))
    }
    #[cfg(not(windows))]
    {
        dirs::config_dir().map(|d| d.join("pc-cartridge-system"))
    }
}

pub fn trust_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join(TRUST_FILE_NAME))
}

/// Whether the cartridge in front of the user is allowed to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Trust {
    /// Steam hand-off: nothing executable is named, so no digest is needed.
    NotRequired,
    /// Digest is on the list.
    Verified { digest: String },
    /// Digest is absent. Play must stay disabled.
    Untrusted { digest: String },
    /// We could not hash what we were asked to run.
    Unreadable { reason: String },
}

impl Trust {
    pub fn allows_launch(&self) -> bool {
        matches!(self, Trust::NotRequired | Trust::Verified { .. })
    }

    pub fn digest(&self) -> Option<&str> {
        match self {
            Trust::Verified { digest } | Trust::Untrusted { digest } => Some(digest),
            _ => None,
        }
    }
}

/// Hash a file the way `sha256sum` and PowerShell's `Get-FileHash` do, so the
/// digests are byte-identical to the ones the shell tooling writes.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Evaluate a launch target against the trust list.
///
/// A `script` target is judged by the digest of the script, which is what the
/// existing shell helpers already record. A `command` target names an arbitrary
/// program with arbitrary arguments, so the thing that has to be pinned is the
/// manifest that spelled it out — otherwise editing `cartridge.toml` would
/// silently change what a trusted cartridge runs.
pub fn evaluate(target: &LaunchTarget, manifest_path: &Path, trusted: &[String]) -> Trust {
    let to_hash = match target {
        LaunchTarget::Steam { .. } => return Trust::NotRequired,
        LaunchTarget::Script(path) => path.as_path(),
        LaunchTarget::Command(_) => manifest_path,
    };

    match hash_file(to_hash) {
        Ok(digest) => {
            if trusted.iter().any(|t| t.eq_ignore_ascii_case(&digest)) {
                Trust::Verified { digest }
            } else {
                Trust::Untrusted { digest }
            }
        }
        Err(e) => Trust::Unreadable {
            reason: format!("{}: {e}", to_hash.display()),
        },
    }
}

/// Read the digest list. A missing file is an empty list, not an error — that is
/// simply a system where nothing has been trusted yet.
pub fn load_trusted() -> Vec<String> {
    let Some(path) = trust_file() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_trust_list(&raw)
}

pub fn parse_trust_list(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        // `sha256sum` writes "<digest>  <path>"; keep only the digest so a file
        // produced either way parses.
        .filter_map(|line| line.split_whitespace().next())
        .filter(|token| token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

/// Append a digest to the trust list, creating the file if needed.
///
/// Only ever called from an explicit click on "Trust this cartridge", never as
/// part of showing one.
pub fn trust_digest(digest: &str) -> std::io::Result<()> {
    use std::io::Write;

    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a SHA-256 digest",
        ));
    }
    let digest = digest.to_ascii_lowercase();
    if load_trusted().iter().any(|t| *t == digest) {
        return Ok(());
    }

    let path = trust_file().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no config directory")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{digest}")
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_match_sha256sum() {
        let dir = std::env::temp_dir().join(format!("cart-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("launch.sh");
        std::fs::write(&file, b"abc").unwrap();

        // Known SHA-256 of "abc".
        assert_eq!(
            hash_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_both_trust_file_dialects() {
        let raw = "\
# comment
BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD

e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  /media/x/launch.sh
not-a-digest
";
        let list = parse_trust_list(raw);
        assert_eq!(
            list,
            vec![
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ]
        );
    }

    #[test]
    fn script_trust_follows_the_digest() {
        let dir = std::env::temp_dir().join(format!("cart-trust-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("launch.sh");
        std::fs::write(&script, b"abc").unwrap();
        let manifest = dir.join("cartridge.toml");
        std::fs::write(&manifest, b"[cartridge]\n").unwrap();

        let target = LaunchTarget::Script(script.clone());
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        let verified = evaluate(&target, &manifest, &[digest.to_string()]);
        assert_eq!(verified, Trust::Verified { digest: digest.into() });
        assert!(verified.allows_launch());

        let untrusted = evaluate(&target, &manifest, &[]);
        assert!(!untrusted.allows_launch());
        assert_eq!(untrusted.digest(), Some(digest));

        // Editing the script must revoke trust.
        std::fs::write(&script, b"abcd").unwrap();
        assert!(!evaluate(&target, &manifest, &[digest.to_string()]).allows_launch());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn command_trust_pins_the_manifest_not_the_program() {
        let dir = std::env::temp_dir().join(format!("cart-cmd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("cartridge.toml");
        std::fs::write(&manifest, b"abc").unwrap();

        let target = LaunchTarget::Command(vec!["/usr/bin/foo".into()]);
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(evaluate(&target, &manifest, &[digest.to_string()]).allows_launch());

        // Rewriting the manifest to run something else must revoke trust.
        std::fs::write(&manifest, b"abcd").unwrap();
        assert!(!evaluate(&target, &manifest, &[digest.to_string()]).allows_launch());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn steam_targets_bypass_the_list_entirely() {
        let target = LaunchTarget::Steam {
            app_id: "367520".into(),
            big_picture: false,
        };
        let trust = evaluate(&target, Path::new("/nonexistent"), &[]);
        assert_eq!(trust, Trust::NotRequired);
        assert!(trust.allows_launch());
    }

    #[test]
    fn unreadable_targets_do_not_launch() {
        let target = LaunchTarget::Script("/definitely/not/here.sh".into());
        let trust = evaluate(&target, Path::new("/nope"), &[]);
        assert!(matches!(trust, Trust::Unreadable { .. }));
        assert!(!trust.allows_launch());
    }
}

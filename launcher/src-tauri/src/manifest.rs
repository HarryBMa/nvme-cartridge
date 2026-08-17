//! Cartridge metadata: the `cartridge.toml` manifest that sits at the root of a
//! cartridge, plus a read-only compatibility shim for legacy `autorun.inf`.
//!
//! Why not `autorun.inf`?
//!
//! * Windows has ignored the `open=` / `shellexecute=` keys on non-optical media
//!   since Windows 7 (KB967940), so it cannot do the one thing it looks like it
//!   does. Anything built on it needs a watcher anyway.
//! * It is the single most abused autorun vector in Windows history; writing new
//!   tooling that treats it as a source of executable intent is a bad trade.
//! * INI has no arrays, no types and no agreed escaping, so argument vectors and
//!   nested data have to be smuggled through string parsing.
//!
//! `cartridge.toml` is typed, has real arrays, round-trips through serde, and is
//! trivially diffable in a pull request. `autorun.inf` is still read when no
//! manifest exists, but only for the cosmetic `label` and `icon` keys — its
//! `open`/`shellexecute` keys are deliberately never honoured.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MANIFEST_NAME: &str = "cartridge.toml";
pub const LEGACY_AUTORUN_NAME: &str = "autorun.inf";

/// Largest manifest / artwork we are willing to pull off an untrusted volume.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_ARTWORK_BYTES: u64 = 12 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("no {MANIFEST_NAME} or {LEGACY_AUTORUN_NAME} at the volume root")]
    NotFound,
    #[error("{MANIFEST_NAME} is {0} bytes, refusing to parse anything over {MAX_MANIFEST_BYTES}")]
    TooLarge(u64),
    #[error("{MANIFEST_NAME} is not valid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

/// The on-disk manifest, exactly as authored.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Manifest {
    pub cartridge: CartridgeMeta,
    #[serde(default)]
    pub launch: LaunchSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CartridgeMeta {
    pub title: String,
    /// Studio, publisher or any second line under the title.
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub edition: Option<String>,
    #[serde(default)]
    pub year: Option<u16>,
    /// Free-form: stamped on the data plate the way a real cartridge is.
    #[serde(default)]
    pub serial: Option<String>,
    /// Cover art, relative to the volume root. Absolute paths are rejected.
    #[serde(default)]
    pub artwork: Option<String>,
    /// Optional transparent title treatment drawn over the art.
    #[serde(default)]
    pub logo: Option<String>,
    /// `#rrggbb` accent. Sampled from the artwork when absent.
    #[serde(default)]
    pub accent: Option<String>,
}

/// How the cartridge wants to be played. Exactly one target may be set.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LaunchSpec {
    /// Steam app id. Handed to `steam://rungameid/<id>`.
    #[serde(default)]
    pub steam: Option<String>,
    /// Script at the volume root, gated on the SHA-256 trust list.
    #[serde(default)]
    pub script: Option<String>,
    /// Raw argv, gated on the SHA-256 of the manifest itself.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    /// Ask Steam for Big Picture before handing over the app id.
    #[serde(default)]
    pub big_picture: bool,
}

/// A validated launch target. Constructing one of these is what proves the
/// manifest asked for exactly one unambiguous thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// Steam app id, already checked to be digits only.
    Steam { app_id: String, big_picture: bool },
    /// Path to a script on the cartridge, already checked to stay on the volume.
    Script(PathBuf),
    /// Argv with a non-empty program.
    Command(Vec<String>),
}

impl LaunchTarget {
    /// Stable discriminant for the UI.
    pub fn kind(&self) -> &'static str {
        match self {
            LaunchTarget::Steam { .. } => "steam",
            LaunchTarget::Script(_) => "script",
            LaunchTarget::Command(_) => "command",
        }
    }

    /// One-line human summary for the data plate.
    pub fn summary(&self) -> String {
        match self {
            LaunchTarget::Steam { app_id, .. } => format!("steam://rungameid/{app_id}"),
            LaunchTarget::Script(p) => p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string()),
            LaunchTarget::Command(argv) => argv.join(" "),
        }
    }

    /// Steam hand-off is a URL to a program the user already trusts with an
    /// argument we have proven is numeric, so it needs no trust entry. Anything
    /// that can name an executable does.
    pub fn requires_trust(&self) -> bool {
        !matches!(self, LaunchTarget::Steam { .. })
    }
}

impl Manifest {
    /// Read and validate the manifest for a mounted volume, falling back to
    /// `autorun.inf` for cosmetics only.
    pub fn load(root: &Path) -> Result<Self, ManifestError> {
        let path = root.join(MANIFEST_NAME);
        match std::fs::metadata(&path) {
            Ok(meta) => {
                if meta.len() > MAX_MANIFEST_BYTES {
                    return Err(ManifestError::TooLarge(meta.len()));
                }
                let raw = std::fs::read_to_string(&path)?;
                let manifest: Manifest = toml::from_str(&raw)?;
                manifest.validate()?;
                Ok(manifest)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::from_legacy_autorun(root),
            Err(e) => Err(e.into()),
        }
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.cartridge.title.trim().is_empty() {
            return Err(ManifestError::Invalid(
                "cartridge.title must not be empty".into(),
            ));
        }
        if let Some(accent) = &self.cartridge.accent {
            if !is_hex_colour(accent) {
                return Err(ManifestError::Invalid(format!(
                    "cartridge.accent must look like #rrggbb, got {accent:?}"
                )));
            }
        }
        // Surfaces the "exactly one target" rule at load time rather than at the
        // moment the user hits Play.
        self.launch.target(Path::new("/"))?;
        Ok(())
    }

    /// Minimal `[autorun]` reader. Only `label` and `icon` are honoured; `open`
    /// and `shellexecute` are ignored on purpose.
    fn from_legacy_autorun(root: &Path) -> Result<Self, ManifestError> {
        let path = root.join(LEGACY_AUTORUN_NAME);
        let raw = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(ManifestError::NotFound)
            }
            Err(e) => return Err(e.into()),
        };
        // autorun.inf is historically Windows-1252; lossy UTF-8 is close enough
        // for a label and keeps us from dragging in an encoding crate.
        let text = String::from_utf8_lossy(&raw);

        let mut in_autorun = false;
        let mut label = None;
        let mut icon = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_autorun = line[1..line.len() - 1].eq_ignore_ascii_case("autorun");
                continue;
            }
            if !in_autorun {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim().to_ascii_lowercase().as_str() {
                "label" if !value.is_empty() => label = Some(value.to_string()),
                // `icon` may carry a trailing resource index, e.g. `cover.ico,0`.
                "icon" if !value.is_empty() => {
                    icon = Some(value.split(',').next().unwrap_or(value).trim().to_string())
                }
                _ => {}
            }
        }

        let title = label.ok_or(ManifestError::NotFound)?;
        Ok(Manifest {
            cartridge: CartridgeMeta {
                title,
                subtitle: Some("Legacy autorun.inf".into()),
                edition: None,
                year: None,
                serial: None,
                artwork: icon,
                logo: None,
                accent: None,
            },
            launch: LaunchSpec::default(),
        })
    }

    /// Resolve the artwork to bytes plus a MIME type, ready to be inlined as a
    /// data URI. Returns `Ok(None)` when the manifest names no artwork.
    pub fn read_artwork(&self, root: &Path) -> Result<Option<(Vec<u8>, &'static str)>, ManifestError> {
        let Some(rel) = self.cartridge.artwork.as_deref() else {
            return Ok(None);
        };
        read_media(root, rel).map(Some)
    }

    pub fn read_logo(&self, root: &Path) -> Result<Option<(Vec<u8>, &'static str)>, ManifestError> {
        let Some(rel) = self.cartridge.logo.as_deref() else {
            return Ok(None);
        };
        read_media(root, rel).map(Some)
    }
}

impl LaunchSpec {
    /// Validate that the manifest names exactly one launch target and that the
    /// target itself is well formed.
    pub fn target(&self, root: &Path) -> Result<LaunchTarget, ManifestError> {
        let set = [
            self.steam.is_some(),
            self.script.is_some(),
            self.command.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count();

        if set == 0 {
            return Err(ManifestError::Invalid(
                "[launch] needs one of steam, script or command".into(),
            ));
        }
        if set > 1 {
            return Err(ManifestError::Invalid(
                "[launch] must name exactly one of steam, script or command".into(),
            ));
        }

        if let Some(app_id) = &self.steam {
            let app_id = app_id.trim();
            // The whole reason Steam launches skip the trust list: the argument
            // cannot be anything but digits, so there is no room for an
            // injected path or flag.
            if app_id.is_empty() || !app_id.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ManifestError::Invalid(format!(
                    "launch.steam must be a numeric app id, got {app_id:?}"
                )));
            }
            return Ok(LaunchTarget::Steam {
                app_id: app_id.to_string(),
                big_picture: self.big_picture,
            });
        }

        if let Some(script) = &self.script {
            let path = resolve_on_volume(root, script)?;
            return Ok(LaunchTarget::Script(path));
        }

        let argv = self.command.clone().unwrap_or_default();
        if argv.is_empty() || argv[0].trim().is_empty() {
            return Err(ManifestError::Invalid(
                "launch.command must start with a program to run".into(),
            ));
        }
        Ok(LaunchTarget::Command(argv))
    }
}

fn read_media(root: &Path, rel: &str) -> Result<(Vec<u8>, &'static str), ManifestError> {
    let path = resolve_on_volume(root, rel)?;
    let meta = std::fs::metadata(&path)?;
    if meta.len() > MAX_ARTWORK_BYTES {
        return Err(ManifestError::TooLarge(meta.len()));
    }
    let mime = mime_for(&path).ok_or_else(|| {
        ManifestError::Invalid(format!("unsupported image type: {}", path.display()))
    })?;
    Ok((std::fs::read(&path)?, mime))
}

/// Join a manifest-supplied relative path onto the volume root, rejecting
/// absolute paths and anything that climbs out of the volume.
fn resolve_on_volume(root: &Path, rel: &str) -> Result<PathBuf, ManifestError> {
    use std::path::Component;

    let rel_path = Path::new(rel);
    if rel_path.is_absolute() || rel.contains(':') {
        return Err(ManifestError::Invalid(format!(
            "{rel:?} must be relative to the cartridge root"
        )));
    }

    let mut out = PathBuf::new();
    for component in rel_path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ManifestError::Invalid(format!(
                    "{rel:?} must not escape the cartridge root"
                )))
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ManifestError::Invalid(format!(
                    "{rel:?} must be relative to the cartridge root"
                )))
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(ManifestError::Invalid(format!("{rel:?} is not a path")));
    }
    Ok(root.join(out))
}

fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        // .ico shows up via legacy autorun.inf and every webview renders it.
        "ico" => "image/x-icon",
        _ => return None,
    })
}

fn is_hex_colour(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_src: &str) -> Result<Manifest, ManifestError> {
        let m: Manifest = toml::from_str(toml_src)?;
        m.validate()?;
        Ok(m)
    }

    #[test]
    fn parses_a_full_manifest() {
        let m = parse(
            r##"
            [cartridge]
            title = "Cinder & Salt"
            subtitle = "Longwave Industries"
            year = 2026
            serial = "LW-0117-A"
            artwork = "art/cover.png"
            accent = "#e8a13a"

            [launch]
            steam = "367520"
            big_picture = true
            "##,
        )
        .expect("valid manifest");

        assert_eq!(m.cartridge.title, "Cinder & Salt");
        assert_eq!(m.cartridge.year, Some(2026));
        assert_eq!(
            m.launch.target(Path::new("/mnt/cart")).unwrap(),
            LaunchTarget::Steam {
                app_id: "367520".into(),
                big_picture: true
            }
        );
    }

    #[test]
    fn steam_launches_need_no_trust_entry_but_scripts_do() {
        let steam = LaunchTarget::Steam {
            app_id: "1".into(),
            big_picture: false,
        };
        assert!(!steam.requires_trust());
        assert!(LaunchTarget::Script("/mnt/cart/launch.sh".into()).requires_trust());
        assert!(LaunchTarget::Command(vec!["/bin/sh".into()]).requires_trust());
    }

    #[test]
    fn rejects_non_numeric_app_ids() {
        // The guard that keeps `steam` off the trust list honest.
        for bad in ["367520; rm -rf /", "../../etc", "", "abc", "36 7520"] {
            let spec = LaunchSpec {
                steam: Some(bad.to_string()),
                ..Default::default()
            };
            assert!(
                spec.target(Path::new("/mnt/cart")).is_err(),
                "app id {bad:?} should have been rejected"
            );
        }
    }

    #[test]
    fn rejects_ambiguous_and_empty_launch_blocks() {
        let both = LaunchSpec {
            steam: Some("10".into()),
            script: Some("launch.sh".into()),
            ..Default::default()
        };
        assert!(both.target(Path::new("/mnt/cart")).is_err());
        assert!(LaunchSpec::default().target(Path::new("/mnt/cart")).is_err());
    }

    #[test]
    fn script_paths_stay_on_the_volume() {
        let root = Path::new("/media/user/CART");
        let ok = LaunchSpec {
            script: Some("bin/launch.sh".into()),
            ..Default::default()
        };
        assert_eq!(
            ok.target(root).unwrap(),
            LaunchTarget::Script(root.join("bin/launch.sh"))
        );

        for bad in [
            "../../../home/user/.bashrc",
            "/etc/passwd",
            "C:\\Windows\\System32\\cmd.exe",
        ] {
            let spec = LaunchSpec {
                script: Some(bad.to_string()),
                ..Default::default()
            };
            assert!(
                spec.target(root).is_err(),
                "script path {bad:?} should have been rejected"
            );
        }
    }

    #[test]
    fn artwork_paths_are_confined_too() {
        assert!(resolve_on_volume(Path::new("/mnt/c"), "art/../../secret.png").is_err());
        assert_eq!(
            resolve_on_volume(Path::new("/mnt/c"), "./art/cover.png").unwrap(),
            Path::new("/mnt/c/art/cover.png")
        );
    }

    #[test]
    fn rejects_a_bad_accent_and_an_empty_title() {
        assert!(parse("[cartridge]\ntitle=\"X\"\naccent=\"tomato\"\n[launch]\nsteam=\"1\"").is_err());
        assert!(parse("[cartridge]\ntitle=\"   \"\n[launch]\nsteam=\"1\"").is_err());
    }

    #[test]
    fn legacy_autorun_gives_cosmetics_only() {
        let dir = std::env::temp_dir().join(format!("cart-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(LEGACY_AUTORUN_NAME),
            "[autorun]\r\nlabel=Hollow Reach\r\nicon=cover.ico,0\r\nopen=evil.exe\r\n",
        )
        .unwrap();

        let m = Manifest::load(&dir).unwrap();
        assert_eq!(m.cartridge.title, "Hollow Reach");
        assert_eq!(m.cartridge.artwork.as_deref(), Some("cover.ico"));
        // `open=` must never become a launch target.
        assert!(m.launch.command.is_none());
        assert!(m.launch.script.is_none());
        assert!(m.launch.target(&dir).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_manifest_is_not_found() {
        let dir = std::env::temp_dir().join(format!("cart-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            Manifest::load(&dir),
            Err(ManifestError::NotFound)
        ));
        std::fs::remove_dir_all(&dir).ok();
    }
}

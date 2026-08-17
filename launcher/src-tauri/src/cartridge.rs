//! Turning a mounted volume into the thing the launcher window draws.
//!
//! Kept free of Tauri types so it can be unit-tested on its own.

use std::path::PathBuf;

use serde::Serialize;

use crate::manifest::{Manifest, ManifestError};
use crate::settings::Settings;
use crate::trust::{self, Trust};
use crate::volumes::Volume;

/// Everything the window needs, in one payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cartridge {
    /// Stable within a session; the mount path is the identity of a cartridge.
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub edition: Option<String>,
    pub year: Option<u16>,
    pub serial: Option<String>,
    pub accent: Option<String>,
    /// Cover art as a `data:` URI.
    ///
    /// Inlining rather than serving the file keeps the asset-protocol scope shut:
    /// cartridge mount paths are arbitrary, so a scope wide enough to serve them
    /// would be wide enough to serve anything.
    pub artwork: Option<String>,
    pub logo: Option<String>,

    pub mount: String,
    pub drive: String,
    pub volume_label: Option<String>,
    pub device: Option<String>,
    pub file_system: Option<String>,
    pub total_bytes: u64,
    pub available_bytes: u64,

    pub launch_kind: &'static str,
    pub launch_summary: String,
    pub trust: Trust,
    /// False when the trust list or settings say Play must not fire.
    pub can_play: bool,
    /// True when this cartridge would have started on its own.
    pub autolaunch: bool,
}

/// Build the view for a volume, or explain why the volume is not a cartridge.
pub fn inspect(volume: &Volume, settings: &Settings) -> Result<Cartridge, ManifestError> {
    let manifest = Manifest::load(&volume.mount)?;
    let target = manifest.launch.target(&volume.mount)?;

    let manifest_path = volume.manifest_path();
    let trusted = trust::load_trusted();
    let trust = trust::evaluate(&target, &manifest_path, &trusted);

    let artwork = manifest
        .read_artwork(&volume.mount)
        .ok()
        .flatten()
        .map(|(bytes, mime)| data_uri(&bytes, mime));
    let logo = manifest
        .read_logo(&volume.mount)
        .ok()
        .flatten()
        .map(|(bytes, mime)| data_uri(&bytes, mime));

    let can_play = trust.allows_launch();

    Ok(Cartridge {
        id: volume.mount.to_string_lossy().into_owned(),
        title: manifest.cartridge.title.trim().to_string(),
        subtitle: clean(manifest.cartridge.subtitle),
        edition: clean(manifest.cartridge.edition),
        year: manifest.cartridge.year,
        serial: clean(manifest.cartridge.serial),
        accent: manifest.cartridge.accent.clone(),
        artwork,
        logo,

        mount: volume.mount.to_string_lossy().into_owned(),
        drive: volume.short_name(),
        volume_label: volume.label.clone(),
        device: volume.device.clone(),
        file_system: volume.file_system.clone(),
        total_bytes: volume.total_bytes,
        available_bytes: volume.available_bytes,

        launch_kind: target.kind(),
        launch_summary: target.summary(),
        trust,
        can_play,
        autolaunch: can_play && settings.may_autolaunch(),
    })
}

/// What the launcher needs to remember about a shown cartridge in order to act
/// on Play and Eject later, without re-reading the volume.
#[derive(Debug, Clone)]
pub struct Session {
    pub volume: Volume,
    pub manifest_path: PathBuf,
    pub target: crate::manifest::LaunchTarget,
    pub trust: Trust,
}

pub fn session(volume: &Volume) -> Result<Session, ManifestError> {
    let manifest = Manifest::load(&volume.mount)?;
    let target = manifest.launch.target(&volume.mount)?;
    let manifest_path = volume.manifest_path();
    let trust = trust::evaluate(&target, &manifest_path, &trust::load_trusted());
    Ok(Session {
        volume: volume.clone(),
        manifest_path,
        target,
        trust,
    })
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn data_uri(bytes: &[u8], mime: &str) -> String {
    format!("data:{mime};base64,{}", base64_encode(bytes))
}

/// Standard base64, no line breaks. Small enough not to justify a dependency.
fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        out.push(TABLE[(n >> 18 & 0x3f) as usize] as char);
        out.push(TABLE[(n >> 12 & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    struct TempCart(PathBuf);

    impl TempCart {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "cart-view-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempCart(dir)
        }
        fn volume(&self) -> Volume {
            Volume {
                mount: self.0.clone(),
                device: Some("/dev/sdb1".into()),
                label: Some("CART".into()),
                file_system: Some("exfat".into()),
                total_bytes: 512_110_190_592,
                available_bytes: 96_000_000_000,
                removable: false,
            }
        }
        fn write(&self, rel: &str, contents: &[u8]) {
            let path = self.0.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
    }

    impl Drop for TempCart {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    #[test]
    fn builds_a_view_from_a_steam_cartridge() {
        let cart = TempCart::new("steam");
        cart.write(
            "cartridge.toml",
            br##"
[cartridge]
title = "Cinder & Salt"
subtitle = "Longwave Industries"
year = 2026
serial = "LW-0117-A"
accent = "#e8a13a"

[launch]
steam = "367520"
"##,
        );

        let view = inspect(&cart.volume(), &Settings::parse("MODE=running")).unwrap();
        assert_eq!(view.title, "Cinder & Salt");
        assert_eq!(view.launch_kind, "steam");
        assert_eq!(view.launch_summary, "steam://rungameid/367520");
        assert_eq!(view.trust, Trust::NotRequired);
        assert!(view.can_play);
        // MODE=running plus a trusted target is the one case that auto-plays.
        assert!(view.autolaunch);
        assert_eq!(view.total_bytes, 512_110_190_592);
    }

    #[test]
    fn a_stopped_mode_shows_the_cartridge_but_never_auto_plays() {
        let cart = TempCart::new("stopped");
        cart.write(
            "cartridge.toml",
            b"[cartridge]\ntitle = \"X\"\n\n[launch]\nsteam = \"1\"\n",
        );
        let view = inspect(&cart.volume(), &Settings::parse("MODE=stopped")).unwrap();
        assert!(view.can_play, "the button still works when clicked");
        assert!(!view.autolaunch, "but nothing fires on its own");
    }

    #[test]
    fn an_untrusted_script_cartridge_cannot_play() {
        let cart = TempCart::new("untrusted");
        cart.write(
            "cartridge.toml",
            b"[cartridge]\ntitle = \"Y\"\n\n[launch]\nscript = \"launch.sh\"\n",
        );
        cart.write("launch.sh", b"#!/bin/bash\necho hi\n");

        let view = inspect(&cart.volume(), &Settings::parse("MODE=running")).unwrap();
        assert_eq!(view.launch_kind, "script");
        assert!(matches!(view.trust, Trust::Untrusted { .. }));
        assert!(!view.can_play);
        assert!(!view.autolaunch);
        // The digest is surfaced so the UI can show what to trust.
        assert_eq!(view.trust.digest().map(str::len), Some(64));
    }

    #[test]
    fn artwork_becomes_a_data_uri_and_missing_art_is_not_fatal() {
        let cart = TempCart::new("art");
        cart.write(
            "cartridge.toml",
            b"[cartridge]\ntitle=\"Z\"\nartwork=\"art/cover.png\"\n\n[launch]\nsteam=\"1\"\n",
        );
        cart.write("art/cover.png", b"\x89PNG\r\n\x1a\n");
        let view = inspect(&cart.volume(), &Settings::default()).unwrap();
        assert_eq!(
            view.artwork.as_deref(),
            Some("data:image/png;base64,iVBORw0KGgo=")
        );

        // Now point at art that does not exist: still a valid cartridge.
        cart.write(
            "cartridge.toml",
            b"[cartridge]\ntitle=\"Z\"\nartwork=\"art/gone.png\"\n\n[launch]\nsteam=\"1\"\n",
        );
        let view = inspect(&cart.volume(), &Settings::default()).unwrap();
        assert!(view.artwork.is_none());
        assert_eq!(view.title, "Z");
    }

    #[test]
    fn blank_optional_fields_are_dropped_rather_than_rendered_empty() {
        let cart = TempCart::new("blank");
        cart.write(
            "cartridge.toml",
            b"[cartridge]\ntitle=\"  Q  \"\nsubtitle=\"   \"\n\n[launch]\nsteam=\"1\"\n",
        );
        let view = inspect(&cart.volume(), &Settings::default()).unwrap();
        assert_eq!(view.title, "Q");
        assert_eq!(view.subtitle, None);
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn a_volume_without_a_manifest_is_not_a_cartridge() {
        let cart = TempCart::new("nomanifest");
        assert!(inspect(&cart.volume(), &Settings::default()).is_err());
    }
}

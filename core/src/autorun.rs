//! `autorun.inf`, for the drive's name and icon in Windows Explorer.
//!
//! This is the one legitimate remaining use of the file. Windows has ignored
//! `open=` and `shellexecute=` on non-optical media since Windows 7, so nothing
//! here is executable — but `label=` and `icon=` are still honoured, which is
//! what makes a cartridge show up in Explorer as "HOLLOW KNIGHT" with its cover
//! art instead of "Removable Disk (D:)".
//!
//! Explorer only accepts `.ico`, `.bmp`, `.exe` or `.dll` for `icon=`, not JPEG.
//! Rather than pull in an image decoder to convert Steam's JPEG covers, this
//! takes the one no-dependency route that exists: an `.ico` may *contain* a PNG
//! verbatim (Vista and later), so a small enough PNG can be wrapped in an icon
//! container by writing a 22-byte header in front of it.

use std::path::Path;

/// PNG-in-ICO entries record their size in one byte, where 0 means 256, so
/// anything larger cannot be described.
const MAX_ICON_EDGE: u32 = 256;

/// Render the file. `icon` is a filename on the cartridge, if there is one.
pub fn render_autorun(label: &str, icon: Option<&str>) -> String {
    let mut out = String::from("[autorun]\r\n");
    // CRLF throughout: this file is read by Windows.
    out.push_str(&format!("label={}\r\n", sanitize_inf_value(label)));
    if let Some(icon) = icon {
        out.push_str(&format!("icon={}\r\n", sanitize_inf_value(icon)));
    }
    out.push_str("\r\n; Written by the PC GamePak create wizard.\r\n");
    out.push_str("; label and icon only - this cartridge is launched by the\r\n");
    out.push_str("; launcher app, never by Windows autorun.\r\n");
    out
}

/// Strip anything that would break the INI or inject another key.
pub fn sanitize_inf_value(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '[' | ']' | '='))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Width and height from a PNG's IHDR chunk.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE {
        return None;
    }
    // IHDR is always first: length (4) + "IHDR" (4) + width (4) + height (4).
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width > 0 && height > 0).then_some((width, height))
}

/// Wrap a PNG in a single-image `.ico` container.
///
/// Returns `None` when the PNG is too large to describe in an icon directory
/// entry, or is not a PNG at all.
pub fn ico_from_png(png: &[u8]) -> Option<Vec<u8>> {
    let (width, height) = png_dimensions(png)?;
    if width > MAX_ICON_EDGE || height > MAX_ICON_EDGE {
        return None;
    }

    let mut out = Vec::with_capacity(png.len() + 22);

    // ICONDIR: reserved, type 1 (icon), one image.
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());

    // ICONDIRENTRY. 256 is encoded as 0.
    out.push(if width == MAX_ICON_EDGE {
        0
    } else {
        width as u8
    });
    out.push(if height == MAX_ICON_EDGE {
        0
    } else {
        height as u8
    });
    out.push(0); // palette count: 0 for non-palettised
    out.push(0); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // colour planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    out.extend_from_slice(&(png.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes()); // offset: straight after the header

    out.extend_from_slice(png);
    Some(out)
}

/// Write `autorun.inf` to the cartridge, and a `cover.ico` when one can be made.
///
/// `cover` is the art already copied onto the cartridge, if any. Returns the
/// icon filename that ended up in the file.
pub fn write_autorun(
    root: &Path,
    label: &str,
    cover: Option<&Path>,
) -> std::io::Result<Option<String>> {
    let icon = cover.and_then(|path| make_icon(root, path));
    let contents = render_autorun(label, icon.as_deref());
    std::fs::write(root.join("autorun.inf"), contents)?;
    Ok(icon)
}

/// Produce `cover.ico` on the cartridge if we can, and return its name.
fn make_icon(root: &Path, cover: &Path) -> Option<String> {
    // An .ico supplied by the user is used as-is.
    if cover
        .extension()
        .and_then(|e| e.to_str())?
        .eq_ignore_ascii_case("ico")
    {
        let destination = root.join("cover.ico");
        if cover != destination {
            std::fs::copy(cover, &destination).ok()?;
        }
        return Some("cover.ico".to_string());
    }

    // Otherwise the only conversion available without an image decoder is
    // wrapping a small PNG. Steam's covers are 600x900 JPEGs, so this usually
    // declines and the cartridge simply keeps Explorer's default icon.
    let bytes = std::fs::read(cover).ok()?;
    let ico = ico_from_png(&bytes)?;
    std::fs::write(root.join("cover.ico"), ico).ok()?;
    Some("cover.ico".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_label_and_icon_with_crlf() {
        let out = render_autorun("Hollow Knight", Some("cover.ico"));
        assert!(out.starts_with("[autorun]\r\n"));
        assert!(out.contains("label=Hollow Knight\r\n"));
        assert!(out.contains("icon=cover.ico\r\n"));
        // Nothing executable may ever appear in a file we write.
        assert!(!out.to_lowercase().contains("open="));
        assert!(!out.to_lowercase().contains("shellexecute="));
    }

    #[test]
    fn omits_the_icon_key_when_there_is_no_icon() {
        let out = render_autorun("Cinder & Salt", None);
        assert!(out.contains("label=Cinder & Salt"));
        assert!(!out.contains("icon="));
    }

    #[test]
    fn sanitises_values_that_would_break_the_ini() {
        // A newline plus a key would otherwise let a title add its own entry.
        assert_eq!(
            sanitize_inf_value("Doom\r\nopen=evil.exe"),
            "Doom openevil.exe"
        );
        assert_eq!(sanitize_inf_value("[autorun]"), "autorun");
        assert_eq!(sanitize_inf_value("  spaced   out  "), "spaced out");
    }

    #[test]
    fn a_sanitised_title_cannot_introduce_a_key() {
        let out = render_autorun("X\r\nicon=C:\\evil.dll", None);
        // Only one line may start with a key name.
        let keys: Vec<&str> = out
            .lines()
            .filter(|l| l.contains('=') && !l.trim_start().starts_with(';'))
            .collect();
        assert_eq!(keys.len(), 1, "{keys:?}");
        assert!(keys[0].starts_with("label="));
    }

    #[test]
    fn reads_png_dimensions() {
        let png = fake_png(64, 64);
        assert_eq!(png_dimensions(&png), Some((64, 64)));
        assert_eq!(png_dimensions(b"not a png at all"), None);
        assert_eq!(png_dimensions(&[]), None);
        // A JPEG must not be mistaken for one.
        assert_eq!(
            png_dimensions(&[0xff, 0xd8, 0xff, 0xe0, 0, 0, 0, 0, 0, 0]),
            None
        );
    }

    #[test]
    fn wraps_a_small_png_into_a_valid_ico() {
        let png = fake_png(128, 128);
        let ico = ico_from_png(&png).expect("128px wraps");

        // ICONDIR: reserved 0, type 1, count 1.
        assert_eq!(&ico[0..2], &[0, 0]);
        assert_eq!(&ico[2..4], &[1, 0]);
        assert_eq!(&ico[4..6], &[1, 0]);
        // Entry dimensions.
        assert_eq!(ico[6], 128);
        assert_eq!(ico[7], 128);
        // Size and offset.
        assert_eq!(
            u32::from_le_bytes([ico[14], ico[15], ico[16], ico[17]]),
            png.len() as u32
        );
        assert_eq!(u32::from_le_bytes([ico[18], ico[19], ico[20], ico[21]]), 22);
        // The PNG follows verbatim.
        assert_eq!(&ico[22..], &png[..]);
    }

    #[test]
    fn encodes_256_pixels_as_zero() {
        let ico = ico_from_png(&fake_png(256, 256)).expect("256px is the maximum");
        assert_eq!(ico[6], 0);
        assert_eq!(ico[7], 0);
    }

    #[test]
    fn declines_art_it_cannot_describe() {
        // Steam's covers are 600x900, so this is the common case.
        assert_eq!(ico_from_png(&fake_png(600, 900)), None);
        assert_eq!(ico_from_png(&fake_png(257, 100)), None);
        assert_eq!(ico_from_png(b"jpeg bytes"), None);
    }

    #[test]
    fn writes_autorun_and_uses_a_supplied_ico() {
        let scratch = crate::testutil::Scratch::new("autorun");

        // No cover: label only.
        assert_eq!(write_autorun(scratch.path(), "PLAIN", None).unwrap(), None);
        let text = std::fs::read_to_string(scratch.join("autorun.inf")).unwrap();
        assert!(text.contains("label=PLAIN"));
        assert!(!text.contains("icon="));

        // A PNG small enough to wrap.
        let png_path = scratch.join("art.png");
        std::fs::write(&png_path, fake_png(64, 64)).unwrap();
        assert_eq!(
            write_autorun(scratch.path(), "WRAPPED", Some(&png_path)).unwrap(),
            Some("cover.ico".to_string())
        );
        assert!(scratch.join("cover.ico").is_file());

        // A JPEG cover: autorun still written, no icon key.
        let jpg_path = scratch.join("art.jpg");
        std::fs::write(&jpg_path, b"\xff\xd8\xff\xe0 not really a jpeg").unwrap();
        assert_eq!(
            write_autorun(scratch.path(), "JPEG", Some(&jpg_path)).unwrap(),
            None
        );
        let text = std::fs::read_to_string(scratch.join("autorun.inf")).unwrap();
        assert!(text.contains("label=JPEG"));
    }

    /// A PNG header with real dimensions; the pixel data is irrelevant here.
    fn fake_png(width: u32, height: u32) -> Vec<u8> {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&width.to_be_bytes());
        png.extend_from_slice(&height.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]); // stand-in CRC
        png
    }
}

//! Did the bytes actually arrive?
//!
//! A cartridge is written over USB, often 60 GB of it, sometimes through a
//! bridge chip in a £12 enclosure that is also getting warm. Copies mostly work.
//! When they do not, the failure is silent: `std::fs::copy` reports success for
//! every byte the kernel accepted, and a truncated or flipped-bit game shows up
//! later as a crash on a level you have not played yet.
//!
//! So the wizard can check its own work. Each file is summed as it is copied —
//! CRC-32 costs nothing next to a USB write — and then the cartridge is read
//! back and the sums compared. That is one extra pass over the drive, which is
//! why it is opt-in.
//!
//! **This is an integrity check, not a signature.** CRC-32 is the right tool for
//! "did this survive the cable", the same job it does in zip and gzip, and the
//! wrong tool for "did somebody change this on purpose". Nothing here defends
//! against a person; it defends against a cable, a bridge and a hot drive.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where the record of a copy lives on the cartridge, so it can be checked
/// again later without the machine that wrote it.
pub const MANIFEST_PATH: &str = ".gamepak/manifest.json";

/// Read in chunks this size. Large enough that the syscall overhead disappears,
/// small enough to stay out of the way on a modest machine.
const CHUNK: usize = 1024 * 1024;

/// One file, as it was when it was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDigest {
    /// Relative to the cartridge root, with forward slashes so a cartridge
    /// written on Windows verifies on Linux and the other way round.
    pub path: String,
    pub bytes: u64,
    pub crc: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub files: Vec<FileDigest>,
}

impl Manifest {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// What was wrong with a file, in the words the user will see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    /// The file is not on the cartridge at all.
    Missing(String),
    /// It is there but the wrong length — the classic half-written file.
    Truncated {
        path: String,
        expected: u64,
        found: u64,
    },
    /// Right length, wrong contents.
    Corrupt(String),
    /// It could not be read back to check.
    Unreadable { path: String, why: String },
}

impl Problem {
    pub fn path(&self) -> &str {
        match self {
            Problem::Missing(path) | Problem::Corrupt(path) => path,
            Problem::Truncated { path, .. } | Problem::Unreadable { path, .. } => path,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Problem::Missing(path) => format!("{path} did not make it across"),
            Problem::Truncated {
                path,
                expected,
                found,
            } => format!("{path} is {found} bytes, not {expected}"),
            Problem::Corrupt(path) => format!("{path} arrived with different contents"),
            Problem::Unreadable { path, why } => format!("{path} could not be read back: {why}"),
        }
    }
}

/// Copy one file, summing it on the way past.
///
/// This is the reason the verified path does not use `std::fs::copy`: the bytes
/// have to pass through here to be summed, and reading the source a second time
/// afterwards would double the cost of the check.
pub fn copy_and_digest(from: &Path, to: &Path) -> std::io::Result<(u64, u32)> {
    let mut source = std::fs::File::open(from)?;
    let mut destination = std::fs::File::create(to)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut crc = Crc32::new();
    let mut bytes = 0u64;

    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        crc.update(&buffer[..read]);
        bytes += read as u64;
        std::io::Write::write_all(&mut destination, &buffer[..read])?;
    }

    // Without this the check could pass against a file the kernel has not
    // finished writing, which is exactly the failure it is meant to catch.
    destination.sync_all()?;
    Ok((bytes, crc.finish()))
}

/// Sum a file that is already written.
pub fn digest_file(path: &Path) -> std::io::Result<(u64, u32)> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; CHUNK];
    let mut crc = Crc32::new();
    let mut bytes = 0u64;

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        crc.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((bytes, crc.finish()))
}

/// Read the cartridge back and compare it with what was written.
///
/// `progress` is called with (bytes checked, bytes total) so a 60 GB read can
/// say something other than "working".
pub fn verify(
    root: &Path,
    manifest: &Manifest,
    progress: &mut dyn FnMut(u64, u64),
) -> Vec<Problem> {
    let total = manifest.total_bytes();
    let mut checked = 0u64;
    let mut problems = Vec::new();

    for file in &manifest.files {
        let path = root.join(file.path.replace('/', std::path::MAIN_SEPARATOR_STR));

        match std::fs::metadata(&path) {
            Err(_) => {
                problems.push(Problem::Missing(file.path.clone()));
                checked += file.bytes;
                progress(checked, total);
                continue;
            }
            Ok(meta) if meta.len() != file.bytes => {
                problems.push(Problem::Truncated {
                    path: file.path.clone(),
                    expected: file.bytes,
                    found: meta.len(),
                });
                checked += file.bytes;
                progress(checked, total);
                continue;
            }
            Ok(_) => {}
        }

        match digest_file(&path) {
            Ok((_, crc)) if crc == file.crc => {}
            Ok(_) => problems.push(Problem::Corrupt(file.path.clone())),
            Err(e) => problems.push(Problem::Unreadable {
                path: file.path.clone(),
                why: e.to_string(),
            }),
        }
        checked += file.bytes;
        progress(checked, total);
    }

    problems
}

pub fn write_manifest(root: &Path, manifest: &Manifest) -> Result<(), String> {
    let path = root.join(MANIFEST_PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("could not encode the manifest: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// The record left on a cartridge by whichever machine wrote it.
pub fn read_manifest(root: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(root.join(MANIFEST_PATH)).ok()?;
    serde_json::from_str(&text).ok()
}

/// A path relative to the cartridge root, in the manifest's own spelling.
pub fn relative_path(root: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?;
    let mut out = String::new();
    for part in relative.components() {
        if let std::path::Component::Normal(name) = part {
            if !out.is_empty() {
                out.push('/');
            }
            out.push_str(&name.to_string_lossy());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Somewhere to put a digest as a tree is copied.
#[derive(Debug, Default)]
pub struct Digests {
    root: PathBuf,
    files: Vec<FileDigest>,
}

impl Digests {
    /// `root` is the cartridge root, so recorded paths are relative to it
    /// rather than to the folder being copied.
    pub fn new(root: &Path) -> Self {
        Digests {
            root: root.to_path_buf(),
            files: Vec::new(),
        }
    }

    pub fn record(&mut self, destination: &Path, bytes: u64, crc: u32) {
        if let Some(path) = relative_path(&self.root, destination) {
            self.files.push(FileDigest { path, bytes, crc });
        }
    }

    pub fn into_manifest(self) -> Manifest {
        Manifest { files: self.files }
    }
}

// --------------------------------------------------------------------------
// CRC-32 (IEEE 802.3), the one zip and gzip use
// --------------------------------------------------------------------------

/// Built at compile time, so there is no lazy initialisation to synchronise.
const TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

#[derive(Debug, Clone)]
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub fn new() -> Self {
        Crc32(0xFFFF_FFFF)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        let mut crc = self.0;
        for byte in bytes {
            crc = TABLE[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        self.0 = crc;
    }

    pub fn finish(&self) -> u32 {
        self.0 ^ 0xFFFF_FFFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc(bytes: &[u8]) -> u32 {
        let mut c = Crc32::new();
        c.update(bytes);
        c.finish()
    }

    #[test]
    fn crc32_matches_the_reference_vectors() {
        // The check value every CRC-32/ISO-HDLC implementation is tested with.
        assert_eq!(crc(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc(b""), 0x0000_0000);
        assert_eq!(crc(b"a"), 0xE8B7_BE43);
        assert_eq!(
            crc(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn feeding_it_in_pieces_gives_the_same_answer() {
        // It is fed a megabyte at a time in use, so this is the property that
        // actually matters.
        let mut whole = Crc32::new();
        whole.update(b"123456789");

        let mut pieces = Crc32::new();
        pieces.update(b"1234");
        pieces.update(b"");
        pieces.update(b"56789");

        assert_eq!(whole.finish(), pieces.finish());
    }

    #[test]
    fn a_copy_is_summed_as_it_is_made() {
        let scratch = crate::testutil::Scratch::new("copy-digest");
        scratch.write("source.bin", b"123456789");
        let destination = scratch.join("copy.bin");

        let (bytes, sum) = copy_and_digest(&scratch.join("source.bin"), &destination).unwrap();
        assert_eq!(bytes, 9);
        assert_eq!(sum, 0xCBF4_3926);
        assert_eq!(std::fs::read(&destination).unwrap(), b"123456789");

        // And reading it back afterwards agrees with what the copy reported.
        assert_eq!(digest_file(&destination).unwrap(), (bytes, sum));
    }

    #[test]
    fn a_good_cartridge_reports_nothing() {
        let scratch = crate::testutil::Scratch::new("verify-good");
        scratch.write("Games/Tunic/TUNIC.exe", b"pretend a game");
        scratch.write("cover.jpg", b"pretend art");

        let manifest = manifest_of(&scratch, &["Games/Tunic/TUNIC.exe", "cover.jpg"]);
        let mut seen = Vec::new();
        let problems = verify(scratch.path(), &manifest, &mut |done, total| {
            seen.push((done, total))
        });

        assert!(problems.is_empty(), "{problems:?}");
        // Progress runs to the end, so a bar can reach 100%.
        assert_eq!(seen.last().unwrap().0, manifest.total_bytes());
    }

    #[test]
    fn every_way_a_file_can_be_wrong_is_named() {
        let scratch = crate::testutil::Scratch::new("verify-bad");
        scratch.write("intact.bin", b"123456789");
        scratch.write("short.bin", b"123456789");
        scratch.write("flipped.bin", b"123456789");

        let mut manifest = manifest_of(
            &scratch,
            &["intact.bin", "short.bin", "flipped.bin", "gone.bin"],
        );
        // "gone.bin" was never written; the other two are damaged after the
        // fact, the way a bad cable would.
        manifest.files.push(FileDigest {
            path: "gone.bin".to_string(),
            bytes: 9,
            crc: 0,
        });
        scratch.write("short.bin", b"1234");
        scratch.write("flipped.bin", b"12345678X");

        let problems = verify(scratch.path(), &manifest, &mut |_, _| {});

        let by_path: Vec<&str> = problems.iter().map(Problem::path).collect();
        assert!(!by_path.contains(&"intact.bin"), "{problems:?}");
        assert!(matches!(
            problems.iter().find(|p| p.path() == "short.bin"),
            Some(Problem::Truncated {
                expected: 9,
                found: 4,
                ..
            })
        ));
        assert!(matches!(
            problems.iter().find(|p| p.path() == "flipped.bin"),
            Some(Problem::Corrupt(_))
        ));
        assert!(matches!(
            problems.iter().find(|p| p.path() == "gone.bin"),
            Some(Problem::Missing(_))
        ));

        // A truncated file says both numbers, since "wrong size" alone does not
        // tell you whether it is half-written or a different file entirely.
        let said = problems
            .iter()
            .find(|p| p.path() == "short.bin")
            .unwrap()
            .describe();
        assert!(said.contains('4') && said.contains('9'), "{said}");
    }

    #[test]
    fn a_manifest_survives_the_round_trip() {
        let scratch = crate::testutil::Scratch::new("manifest");
        let manifest = Manifest {
            files: vec![FileDigest {
                path: "Games/Tunic/TUNIC.exe".to_string(),
                bytes: 42,
                crc: 0xDEAD_BEEF,
            }],
        };

        assert!(read_manifest(scratch.path()).is_none());
        write_manifest(scratch.path(), &manifest).unwrap();
        assert_eq!(read_manifest(scratch.path()).unwrap(), manifest);
    }

    #[test]
    fn recorded_paths_are_relative_and_use_forward_slashes() {
        let root = Path::new("/media/CART");
        assert_eq!(
            relative_path(root, &root.join("Games").join("Tunic").join("TUNIC.exe")).unwrap(),
            "Games/Tunic/TUNIC.exe"
        );
        // A file outside the cartridge has no place in its manifest.
        assert!(relative_path(root, Path::new("/etc/passwd")).is_none());
        assert!(relative_path(root, root).is_none());
    }

    /// Build a manifest from files that are already on disk.
    fn manifest_of(scratch: &crate::testutil::Scratch, paths: &[&str]) -> Manifest {
        let mut digests = Digests::new(scratch.path());
        for path in paths {
            let full = scratch.join(path);
            if let Ok((bytes, crc)) = digest_file(&full) {
                digests.record(&full, bytes, crc);
            }
        }
        digests.into_manifest()
    }
}

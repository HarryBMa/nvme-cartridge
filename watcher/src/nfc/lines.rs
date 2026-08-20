//! A reader that speaks in lines.
//!
//! `PC_GAMEPAK_NFC_SOURCE=/dev/ttyACM0` — or a FIFO, or an ordinary file that
//! something appends to. Two messages, both case-insensitive:
//!
//! ```text
//! UID 04A224B2      a tag is on the reader
//! GONE              it has been lifted off
//! ```
//!
//! Blank lines and anything starting with `#` are ignored, so a device that
//! prints a banner at reset does not confuse it.
//!
//! This exists for two reasons that turn out to be the same reason. An ESP32
//! with an RC522 costs about six pounds and can print those two lines in a few
//! lines of firmware, which is a real reader for people who would rather build
//! than buy — the same shape of hardware the NFC-Cartridge-Player project uses,
//! though nothing here is derived from it. And a FIFO is a reader that can be
//! driven from a shell, which is how the rest of this module is tested on a
//! machine with no NFC hardware at all.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use super::TagEvent;
use crate::log;

/// How long to wait before looking for the device again.
const REOPEN: Duration = Duration::from_secs(2);

pub fn run(path: &Path, deliver: &mut dyn FnMut(TagEvent)) {
    let reader = reader_name(path);
    let mut complained = false;

    loop {
        match std::fs::File::open(path) {
            Ok(file) => {
                if complained {
                    log::line(&format!("{reader}: back"));
                    complained = false;
                }
                pump(file, &reader, deliver);
            }
            Err(e) => {
                if !complained {
                    log::line(&format!(
                        "{}: cannot read it ({e}); waiting",
                        path.display()
                    ));
                    complained = true;
                }
            }
        }

        // A FIFO reports end-of-file every time a writer closes, which is once
        // per `echo` — normal, and not a reason to think the tag has gone. So
        // reopening is silent, and no departure is invented here.
        std::thread::sleep(REOPEN);
    }
}

fn pump(file: std::fs::File, reader: &str, deliver: &mut dyn FnMut(TagEvent)) {
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            return;
        };
        if let Some(event) = parse(&line, reader) {
            deliver(event);
        }
    }
}

/// Name this source after the device, so a log line says which reader spoke.
fn reader_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("line")
        .to_string()
}

/// One line into an event, or nothing.
fn parse(line: &str, reader: &str) -> Option<TagEvent> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let (word, rest) = match line.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (line, ""),
    };

    match word.to_ascii_uppercase().as_str() {
        "UID" | "TAG" => {
            let uid = crate::tags::normalise(rest);
            (!uid.is_empty()).then(|| TagEvent::Arrived {
                reader: reader.to_string(),
                uid,
            })
        }
        "GONE" | "REMOVED" | "OUT" => Some(TagEvent::Departed {
            reader: reader.to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arrived(uid: &str) -> Option<TagEvent> {
        Some(TagEvent::Arrived {
            reader: "r".to_string(),
            uid: uid.to_string(),
        })
    }

    #[test]
    fn a_tag_arriving_and_leaving() {
        assert_eq!(parse("UID 04A224B2", "r"), arrived("04A224B2"));
        // However the firmware chose to print it.
        assert_eq!(parse("uid 04:a2:24:b2", "r"), arrived("04A224B2"));
        assert_eq!(parse("TAG  04 a2 24 b2  ", "r"), arrived("04A224B2"));

        assert_eq!(
            parse("GONE", "r"),
            Some(TagEvent::Departed {
                reader: "r".to_string()
            })
        );
        assert_eq!(
            parse("removed", "r"),
            Some(TagEvent::Departed {
                reader: "r".to_string()
            })
        );
    }

    #[test]
    fn noise_from_a_board_that_talks_too_much_is_ignored() {
        assert_eq!(parse("", "r"), None);
        assert_eq!(parse("   ", "r"), None);
        assert_eq!(parse("# RC522 ready", "r"), None);
        assert_eq!(parse("rst:0x1 boot:0x13", "r"), None);
        // A UID line with no UID on it is not an arrival.
        assert_eq!(parse("UID", "r"), None);
        assert_eq!(parse("UID ????", "r"), None);
    }

    #[test]
    fn the_reader_is_named_after_the_device() {
        assert_eq!(reader_name(Path::new("/dev/ttyACM0")), "ttyACM0");
        assert_eq!(
            reader_name(Path::new("/run/user/1000/gamepak.fifo")),
            "gamepak.fifo"
        );
        assert_eq!(reader_name(Path::new("/")), "line");
    }
}

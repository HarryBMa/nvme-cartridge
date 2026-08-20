//! Tags: the other way a cartridge can arrive.
//!
//! A drive announces itself by being mounted. A tag announces itself by being
//! put on a reader — and since a tag carries an identifier rather than a game,
//! what it resolves to is a directory on this machine holding the same
//! `cartridge.conf` a drive would. See `tags.rs`.
//!
//! Everything downstream of that is unchanged: the same launcher, the same
//! window, the same manifest. Lifting the tag off closes it, the way unplugging
//! a cartridge does.
//!
//! Two sources, because two kinds of reader exist:
//!
//! * **PC/SC** — every CCID reader, the ACR122U among them. `WinSCard.dll` is
//!   part of Windows; `libpcsclite` is a package on Linux. Loaded at runtime so
//!   that neither is a build-time dependency and a machine without one simply
//!   has no tag support rather than a watcher that will not start.
//! * **A line source** — anything that writes `UID <hex>` and `GONE` to a file,
//!   a FIFO or a serial device. That is a DIY ESP32 and RC522 in about twenty
//!   lines of firmware, and it is also how this module is tested without
//!   hardware.

mod lines;
mod pcsc;

use std::collections::HashMap;
use std::process::Child;

use crate::{launcher, log, tags};

/// What a reader has to say. Departure carries no UID because PC/SC does not
/// report one — the card is gone by the time anybody asks — so the reader it
/// left is the identity that matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagEvent {
    Arrived { reader: String, uid: String },
    Departed { reader: String },
}

/// Start watching for tags, on a thread of its own.
///
/// Returns immediately, and returns nothing: a machine with no reader and no
/// tags directory is the normal case, and it is not an error worth handing back
/// to a caller that could do nothing about it.
pub fn spawn() {
    if !tags::nfc_wanted() {
        return;
    }

    let started = std::thread::Builder::new()
        .name("nfc".to_string())
        .spawn(watch);

    if let Err(e) = started {
        log::line(&format!("could not start the tag reader: {e}"));
    }
}

fn watch() {
    log::line(&format!(
        "watching for tags; virtual cartridges in {}",
        tags::tags_dir().display()
    ));

    // One launcher per reader, not per tag: a reader holds one card at a time,
    // and the departure event names the reader rather than the card. The tag it
    // is showing is kept alongside, because "the same tag again" and "a
    // different tag" are not the same thing.
    let mut open: HashMap<String, Showing> = HashMap::new();
    let mut deliver = |event: TagEvent| handle(event, &mut open);

    match std::env::var_os("PC_GAMEPAK_NFC_SOURCE") {
        Some(path) => lines::run(std::path::Path::new(&path), &mut deliver),
        None => pcsc::run(&mut deliver),
    }
}

/// A launcher this module started, and the tag it is showing.
struct Showing {
    uid: String,
    child: Child,
}

/// Act on one reader event.
///
/// Separated from the sources so both of them exercise the same decisions.
fn handle(event: TagEvent, open: &mut HashMap<String, Showing>) {
    // Collect any launcher the user closed themselves, so a long session does
    // not accumulate zombies.
    open.retain(|_, showing| !matches!(showing.child.try_wait(), Ok(Some(_))));

    match event {
        TagEvent::Arrived { reader, uid } => {
            // A tag left sitting on the reader should not open a second window
            // for itself. A *different* tag is a different matter: a reader
            // holds one card at a time, so this one has replaced whatever was
            // there, and the window it opened belongs to a tag that has gone.
            match open.get(&reader) {
                Some(showing) if showing.uid == uid => {
                    log::line(&format!("{reader}: tag {uid} is already showing"));
                    return;
                }
                Some(_) => {
                    if let Some(mut previous) = open.remove(&reader) {
                        log::line(&format!("{reader}: tag {} replaced", previous.uid));
                        launcher::close(&mut previous.child);
                    }
                }
                None => {}
            }

            let Some(cartridge) = tags::resolve(&uid) else {
                // The only way to learn a UID without a wizard is to tap the
                // tag, so an unknown one is an instruction, not a complaint.
                log::line(&format!(
                    "{reader}: tag {uid} is not set up. To use it, put a cartridge.conf in {}",
                    tags::tags_dir().join(tags::normalise(&uid)).display()
                ));
                return;
            };

            log::line(&format!("{reader}: tag {uid} -> {}", cartridge.display()));
            if let Some(child) = launcher::open(&cartridge) {
                open.insert(reader, Showing { uid, child });
            }
        }

        TagEvent::Departed { reader } => {
            if let Some(mut showing) = open.remove(&reader) {
                log::line(&format!("{reader}: tag {} lifted", showing.uid));
                launcher::close(&mut showing.child);
                // Reaped by the retain() on the next event.
            }
        }
    }
}

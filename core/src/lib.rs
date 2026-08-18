//! Cartridge logic, free of any UI.
//!
//! Everything the launcher and the wizard actually *do* lives here: reading a
//! cartridge, finding games, deciding which drives may be written to,
//! formatting, copying, and writing the files that make a cartridge.
//!
//! It is a separate crate from the Tauri binary for one reason: the binary
//! cannot be compiled without webkit2gtk and a display, so tests living inside
//! it could not run in CI or on a contributor's machine. Here, `cargo test`
//! works anywhere.

#[cfg(test)]
mod testutil;

pub mod autorun;
pub mod cartridge;
pub mod create;
pub mod drives;
pub mod format;
pub mod playnite;
pub mod portable;
pub mod sgdb;
pub mod steam;
pub mod steamlib;

pub use cartridge::base64_encode;

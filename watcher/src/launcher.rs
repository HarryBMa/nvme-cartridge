//! Starting and stopping the launcher.
//!
//! Shared by both things that can open one: a cartridge arriving on a USB port,
//! and a tag arriving on a reader. They differ in what wakes them and in what
//! path they end up with; from here on it is the same window.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use crate::log;

/// Open the launcher on a cartridge, keeping the handle so the window can be
/// closed again when the cartridge goes.
///
/// Not waited on: the launcher outlives the wake that started it.
pub fn open(path: &Path) -> Option<Child> {
    let Some(launcher) = installed_at() else {
        log::line("pc-gamepak is not installed anywhere I can find it");
        return None;
    };

    let mut command = Command::new(&launcher);
    command
        .arg("--drive")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    match command.spawn() {
        Ok(child) => {
            log::line(&format!("launcher started, pid {}", child.id()));
            Some(child)
        }
        Err(e) => {
            log::line(&format!("could not start the launcher: {e}"));
            None
        }
    }
}

/// Close a launcher this watcher started.
///
/// Asked to close rather than killed, so the window goes the way it would if
/// the user had dismissed it. The game, if one is running, is left alone.
pub fn close(child: &mut Child) {
    if !matches!(child.try_wait(), Ok(None)) {
        return; // Already gone.
    }
    log::line(&format!("closing launcher pid {}", child.id()));
    terminate(child);
}

#[cfg(unix)]
fn terminate(child: &Child) {
    // SAFETY: a pid this process started and has not reaped, and a constant
    // signal. SIGTERM rather than SIGKILL — see above.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
}

#[cfg(windows)]
fn terminate(child: &mut Child) {
    // Windows has no SIGTERM. A GUI process could be asked to close with
    // WM_CLOSE, but that means finding its window, and the launcher holds
    // nothing that needs flushing.
    if let Err(e) = child.kill() {
        log::line(&format!("could not close the launcher: {e}"));
    }
}

/// Where the launcher was installed.
#[cfg(not(windows))]
pub fn installed_at() -> Option<PathBuf> {
    // A rootless install puts it in ~/.local/bin; a system install in
    // /usr/local/bin or /usr/bin. `PC_GAMEPAK_LAUNCHER` overrides all of it,
    // which is what a Flatpak or a development build uses.
    if let Some(from_env) = std::env::var_os("PC_GAMEPAK_LAUNCHER") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Some(path);
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/bin/pc-gamepak"));
    }
    candidates.push(PathBuf::from("/usr/local/bin/pc-gamepak"));
    candidates.push(PathBuf::from("/usr/bin/pc-gamepak"));

    candidates.into_iter().find(|path| path.is_file())
}

/// Where the launcher was installed: next to this executable, since the
/// installer puts both in the same directory.
#[cfg(windows)]
pub fn installed_at() -> Option<PathBuf> {
    if let Some(from_env) = std::env::var_os("PC_GAMEPAK_LAUNCHER") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Some(path);
        }
    }

    let beside = std::env::current_exe()
        .ok()?
        .parent()?
        .join("pc-gamepak.exe");
    beside.is_file().then_some(beside)
}

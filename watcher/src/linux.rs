//! The Linux watcher: one process, no root, woken by the mount table.
//!
//! The system install does not need this at all — udev is already running and
//! starts the launcher through a systemd unit, so nothing is resident. This is
//! for the other shape of install: no root, no udev rule, a systemd *user*
//! service, and everything a sandboxed package like a Flatpak can actually do.
//!
//! It does not subscribe to udev. A sandbox has no `/run/udev`, and udev's
//! netlink group is not something a confined process should count on. Instead it
//! blocks in `poll()` on `/proc/self/mountinfo`, which the kernel wakes on any
//! mount activity — no timer, no polling loop, no CPU while it waits.
//!
//! That is also more accurate than the udev route. udev fires when the kernel
//! sees the partition, which is before the desktop has mounted it, so the helper
//! it starts spends up to sixty seconds waiting for a mount point to appear.
//! Waking on the mount table means the cartridge is readable the moment we hear
//! about it.

use std::collections::HashMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::log;
use crate::mounts::{self, Mount};

/// The same drive arriving twice in quick succession — a remount, or a desktop
/// that mounts and immediately re-mounts — should open one window, not two.
const DEBOUNCE: Duration = Duration::from_secs(4);

/// Where the kernel reports mount activity. `/proc/self/mounts` can be polled
/// too, but mountinfo is the one documented to signal POLLPRI, and it is
/// per-namespace, which is what makes this work inside a sandbox.
const MOUNTINFO: &str = "/proc/self/mountinfo";

/// The table itself, in the format `mounts.rs` parses.
const MOUNTS: &str = "/proc/self/mounts";

pub fn run() -> ! {
    log::line("watcher starting (mount table)");

    let Ok(watch) = std::fs::File::open(MOUNTINFO) else {
        log::line("could not open /proc/self/mountinfo; is this Linux?");
        std::process::exit(1);
    };

    // Whatever is already mounted at login is not an arrival. Starting a
    // session should not pop a window for a cartridge left plugged in.
    let mut previous = read_mounts();
    log::line(&format!("watching {} mounted filesystems", previous.len()));

    let mut recent: HashMap<PathBuf, Instant> = HashMap::new();
    // The launchers this watcher started, by the cartridge they are showing.
    let mut open: HashMap<PathBuf, std::process::Child> = HashMap::new();

    loop {
        wait_for_change(watch.as_raw_fd());

        // Collect any launcher that has closed itself, so a long session does
        // not accumulate zombies.
        open.retain(|_, child| !matches!(child.try_wait(), Ok(Some(_))));

        let current = read_mounts();

        for gone in mounts::departures(&previous, &current) {
            log::line(&format!("cartridge removed: {}", gone.display()));
            close_launcher_for(&gone, open.remove(&gone));
            recent.remove(&gone);
        }

        for arrived in mounts::arrivals(&previous, &current) {
            if !mounts::is_cartridge(&arrived) {
                continue;
            }
            let now = Instant::now();
            if let Some(last) = recent.get(&arrived) {
                if now.duration_since(*last) < DEBOUNCE {
                    continue;
                }
            }
            recent.insert(arrived.clone(), now);
            if let Some(child) = open_launcher_for(&arrived) {
                open.insert(arrived.clone(), child);
            }
        }

        previous = current;
    }
}

/// Block until the mount table changes.
///
/// `poll()` with no timeout: the process is off the run queue entirely until the
/// kernel has something to say, which is the whole reason this is affordable as
/// a resident service.
fn wait_for_change(fd: std::os::fd::RawFd) {
    // POLLPRI is what mountinfo signals; POLLERR is what /proc/mounts signals
    // and costs nothing to accept as well.
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLPRI | libc::POLLERR,
        revents: 0,
    };

    loop {
        // SAFETY: one initialised pollfd, count 1, no timeout. poll() writes
        // only revents.
        let ready = unsafe { libc::poll(&mut pollfd, 1, -1) };
        if ready >= 0 {
            return;
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue; // A signal, not a problem.
        }
        // Anything else would spin: back off rather than burn a core.
        log::line(&format!("poll failed: {error}; retrying"));
        std::thread::sleep(Duration::from_secs(1));
        return;
    }
}

fn read_mounts() -> Vec<Mount> {
    let mut text = String::new();
    match std::fs::File::open(MOUNTS).and_then(|mut f| f.read_to_string(&mut text)) {
        Ok(_) => mounts::parse(&text),
        Err(e) => {
            log::line(&format!("could not read {MOUNTS}: {e}"));
            Vec::new()
        }
    }
}

/// Start the launcher on a cartridge, keeping the handle so the window can be
/// closed again when the cartridge goes.
fn open_launcher_for(mount: &Path) -> Option<std::process::Child> {
    let Some(launcher) = launcher_path() else {
        log::line("pc-gamepak is not installed anywhere I can find it");
        return None;
    };

    log::line(&format!(
        "cartridge detected at {}; opening {}",
        mount.display(),
        launcher.display()
    ));

    // Not waited on: the launcher outlives each wake and closes itself.
    match Command::new(&launcher).arg("--drive").arg(mount).spawn() {
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

/// Where the launcher was installed.
///
/// A rootless install puts it in ~/.local/bin; a system install in
/// /usr/local/bin or /usr/bin. `PC_GAMEPAK_LAUNCHER` overrides all of it, which
/// is what a Flatpak or a development build uses.
fn launcher_path() -> Option<PathBuf> {
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

/// Close the launcher window for a cartridge that has gone.
///
/// The launcher this watcher started is closed by its own handle, which is
/// exact — no name matching, and it works whatever the launcher is wrapped in.
/// The scan afterwards is for windows this watcher did not start, from a
/// previous run of it or from udev.
///
/// The game itself is left alone: pulling a cartridge while it runs is the
/// user's business, and killing their session would be a worse surprise than a
/// stale window.
fn close_launcher_for(mount: &Path, ours: Option<std::process::Child>) {
    if let Some(mut child) = ours {
        if matches!(child.try_wait(), Ok(None)) {
            log::line(&format!("closing launcher pid {}", child.id()));
            terminate(child.id() as i32);
            // Reaped on the next wake by the retain() in the loop.
        }
        return;
    }

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if !shows_drive(&cmdline, mount) {
            continue;
        }

        log::line(&format!(
            "closing launcher pid {pid} (not started by this watcher)"
        ));
        terminate(pid);
    }
}

/// SIGTERM rather than SIGKILL, so the window closes the way it would if the
/// user had dismissed it.
fn terminate(pid: i32) {
    // SAFETY: a pid we started or read from /proc, and a constant signal.
    unsafe { libc::kill(pid, libc::SIGTERM) };
}

/// Was this process started with `--drive <mount>`?
///
/// argv arrives NUL-separated, and the check is exact: a launcher showing
/// /run/media/x/CINDER2 must not be closed when CINDER goes.
fn shows_drive(cmdline: &[u8], mount: &Path) -> bool {
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).collect();
    let Some(name) = args.first() else {
        return false;
    };
    let name = Path::new(std::str::from_utf8(name).unwrap_or(""));
    if name.file_name().and_then(|n| n.to_str()) != Some("pc-gamepak") {
        return false;
    }

    let wanted = mount.as_os_str().as_encoded_bytes();
    args.windows(2)
        .any(|pair| pair[0] == b"--drive" && pair[1] == wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmdline(parts: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for part in parts {
            out.extend_from_slice(part.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn only_the_launcher_showing_that_drive_is_closed() {
        let mount = Path::new("/run/media/harry/CINDER");

        assert!(shows_drive(
            &cmdline(&[
                "/usr/local/bin/pc-gamepak",
                "--drive",
                "/run/media/harry/CINDER"
            ]),
            mount
        ));

        // A different cartridge keeps its window.
        assert!(!shows_drive(
            &cmdline(&[
                "/usr/local/bin/pc-gamepak",
                "--drive",
                "/run/media/harry/HOLLOW"
            ]),
            mount
        ));

        // A prefix is not a match: CINDER2 is somebody else.
        assert!(!shows_drive(
            &cmdline(&[
                "/usr/local/bin/pc-gamepak",
                "--drive",
                "/run/media/harry/CINDER2"
            ]),
            mount
        ));

        // The wizard is not showing a cartridge at all.
        assert!(!shows_drive(
            &cmdline(&["/usr/local/bin/pc-gamepak", "--create"]),
            mount
        ));

        // And nothing else on the system is ours to kill, whatever it was
        // started with.
        assert!(!shows_drive(
            &cmdline(&["/usr/bin/vlc", "--drive", "/run/media/harry/CINDER"]),
            mount
        ));
        assert!(!shows_drive(&[], mount));
    }
}

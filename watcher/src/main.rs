//! Cartridge watcher (Windows).
//!
//! Replaces the resident PowerShell monitor. PowerShell holds the whole .NET
//! runtime and a WMI subscription open for the entire login session, which costs
//! tens of megabytes to do nothing. This does the same job by blocking on the
//! Windows message queue: no polling, no timer, no CPU while idle.
//!
//! On Linux none of this is needed — udev is already running as part of the OS
//! and starts the launcher through a systemd unit, so there is no resident
//! process at all. See `linux/99-pc-gamepak.rules`.
//!
//! Flow: volume arrives -> is there a cartridge.conf on it? -> start the
//! launcher with `--drive X:\` and go back to sleep.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Only the Windows arm logs; on other targets the module is compiled but unused.
#[cfg_attr(not(windows), allow(dead_code))]
mod log;

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "pc-gamepak-watcher is only needed on Windows.\n\
         On Linux, install the udev rule instead: sudo ./linux/install.sh"
    );
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    windows_watcher::run()
}

#[cfg(windows)]
mod windows_watcher {
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostQuitMessage,
        RegisterClassW, MSG, WM_DESTROY, WM_DEVICECHANGE, WNDCLASSW, WS_OVERLAPPED,
    };

    /// A volume has been inserted and is available.
    const DBT_DEVICEARRIVAL: u32 = 0x8000;
    /// `dbch_devicetype` for a logical volume.
    const DBT_DEVTYP_VOLUME: u32 = 0x0000_0002;

    /// Windows re-broadcasts arrival for the same volume; ignore repeats.
    const DEBOUNCE: Duration = Duration::from_secs(4);

    /// Files that mark a volume as a cartridge rather than an ordinary drive.
    const MARKERS: [&str; 2] = ["cartridge.conf", "autorun.inf"];

    /// Header shared by every `WM_DEVICECHANGE` payload.
    ///
    /// Declared here rather than imported: the layout is fixed ABI, and writing
    /// it out keeps this file compiling against any windows-sys minor version.
    #[repr(C)]
    struct DevBroadcastHdr {
        dbch_size: u32,
        dbch_devicetype: u32,
        dbch_reserved: u32,
    }

    /// Payload for `DBT_DEVTYP_VOLUME`.
    #[repr(C)]
    struct DevBroadcastVolume {
        dbcv_size: u32,
        dbcv_devicetype: u32,
        dbcv_reserved: u32,
        /// Bit 0 is A:, bit 1 is B:, and so on.
        dbcv_unitmask: u32,
        dbcv_flags: u16,
    }

    /// Last time each drive letter was acted on, for debouncing.
    static mut SEEN: Option<HashMap<char, Instant>> = None;

    pub fn run() {
        crate::log::line("watcher starting");

        // SAFETY: set up before the window exists, so before any message can be
        // dispatched. The message loop is single-threaded, so SEEN is only ever
        // touched from this thread.
        unsafe { SEEN = Some(HashMap::new()) };

        let class_name = wide("PcCartridgeWatcher");

        // A hidden *top-level* window, not a message-only (HWND_MESSAGE) one:
        // Windows does not deliver broadcast WM_DEVICECHANGE messages to
        // message-only windows, so a message-only window would never see a
        // volume arrive. The window is simply never shown.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(std::ptr::null());

            let class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(wnd_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: 0,
                hCursor: 0,
                hbrBackground: 0,
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
            };

            if RegisterClassW(&class) == 0 {
                crate::log::line("could not register the window class; giving up");
                return;
            }

            CreateWindowExW(
                0,
                class_name.as_ptr(),
                wide("PC GamePak Watcher").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                0,
                0,
                instance,
                std::ptr::null(),
            )
        };

        if hwnd == 0 {
            crate::log::line("could not create the listener window; giving up");
            return;
        }

        crate::log::line("listening for volume arrivals");

        // Blocks here for the rest of the session. GetMessageW sleeps in the
        // kernel until something arrives, so idle CPU is exactly zero.
        let mut msg = MSG {
            hwnd: 0,
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
        };
        unsafe {
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_DEVICECHANGE => {
                if wparam as u32 == DBT_DEVICEARRIVAL && lparam != 0 {
                    let header = lparam as *const DevBroadcastHdr;
                    if (*header).dbch_devicetype == DBT_DEVTYP_VOLUME {
                        let volume = lparam as *const DevBroadcastVolume;
                        for letter in letters_from_mask((*volume).dbcv_unitmask) {
                            on_volume_arrived(letter);
                        }
                    }
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    /// Expand a `dbcv_unitmask` bitfield into drive letters.
    fn letters_from_mask(mask: u32) -> Vec<char> {
        (0..26)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| (b'A' + bit as u8) as char)
            .collect()
    }

    /// SAFETY: called only from the window procedure, on the single thread that
    /// initialised SEEN.
    unsafe fn on_volume_arrived(letter: char) {
        let now = Instant::now();
        let seen = SEEN.as_mut().expect("initialised in run()");

        if let Some(last) = seen.get(&letter) {
            if now.duration_since(*last) < DEBOUNCE {
                crate::log::line(&format!("{letter}: ignoring repeat arrival"));
                return;
            }
        }

        let root = PathBuf::from(format!("{letter}:\\"));

        // Not every drive is a cartridge. Without this check the launcher would
        // pop up for every USB stick and phone the user plugs in.
        if !is_cartridge(&root) {
            crate::log::line(&format!(
                "{letter}: no cartridge.conf or autorun.inf at the root; ignoring"
            ));
            return;
        }

        seen.insert(letter, now);
        match start_launcher(&root) {
            Ok(()) => crate::log::line(&format!("{letter}: opened the launcher")),
            Err(e) => crate::log::line(&format!("{letter}: could not start the launcher: {e}")),
        }
    }

    /// A cartridge is a volume with a manifest at its root. Retried briefly:
    /// the volume is mounted by the time the message arrives, but the filesystem
    /// is not always readable on the very first attempt.
    fn is_cartridge(root: &Path) -> bool {
        for attempt in 0..6 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(250));
            }
            if MARKERS.iter().any(|name| root.join(name).is_file()) {
                return true;
            }
        }
        false
    }

    /// Start the launcher next to this executable and do not wait for it.
    fn start_launcher(root: &Path) -> std::io::Result<()> {
        let exe = std::env::current_exe()?
            .parent()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "no install directory")
            })?
            .join("pc-gamepak.exe");

        Command::new(exe)
            .arg("--drive")
            .arg(root)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn expands_a_unit_mask_into_drive_letters() {
            assert_eq!(letters_from_mask(0b1000), vec!['D']);
            assert_eq!(letters_from_mask(0b1), vec!['A']);
            assert_eq!(letters_from_mask(0b1100), vec!['C', 'D']);
            assert_eq!(letters_from_mask(0), Vec::<char>::new());
            assert_eq!(letters_from_mask(1 << 25), vec!['Z']);
        }
    }
}

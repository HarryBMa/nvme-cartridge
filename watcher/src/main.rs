//! Cartridge watcher.
//!
//! Replaces the resident PowerShell monitor. PowerShell holds the whole .NET
//! runtime and a WMI subscription open for the entire login session, which costs
//! tens of megabytes to do nothing. This does the same job by blocking on the
//! Windows message queue: no polling, no timer, no CPU while idle.
//!
//! On Linux the system install does not need this at all — udev is already
//! running as part of the OS and starts the launcher through a systemd unit, so
//! nothing is resident. See `linux/99-pc-gamepak.rules`.
//!
//! The Linux arm here is for the other shape of install: no root, no udev rule,
//! a systemd *user* service. It blocks in poll() on the mount table instead,
//! which is what a sandboxed package can do — and, as it happens, fires when the
//! cartridge is actually readable rather than when the kernel first sees the
//! partition. See `linux.rs`.
//!
//! Flow: volume arrives -> is there a cartridge.conf on it? -> start the
//! launcher with `--drive X:\` and go back to sleep.
//!
//! A tag on an NFC reader is the same flow with a different doorbell: the UID
//! names a directory holding a `cartridge.conf`, and the launcher is opened on
//! that instead. Off unless a tags directory exists — see `nfc.rs`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod launcher;
#[cfg(not(windows))]
mod linux;
mod log;
#[cfg(not(windows))]
mod mounts;
mod nfc;
mod tags;

#[cfg(not(windows))]
fn main() {
    linux::run()
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
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, EnumWindows, GetClassNameW,
        GetMessageW, GetWindowTextW, PostMessageW, PostQuitMessage, RegisterClassW, MSG,
        WM_CLOSE, WM_DESTROY, WM_DEVICECHANGE, WNDCLASSW, WS_OVERLAPPED,
    };

    /// A volume has been inserted and is available.
    const DBT_DEVICEARRIVAL: u32 = 0x8000;
    /// `dbch_devicetype` for a logical volume.
    const DBT_DEVTYP_VOLUME: u32 = 0x0000_0002;

    /// Windows re-broadcasts arrival for the same volume; ignore repeats.
    const DEBOUNCE: Duration = Duration::from_secs(4);

    /// Files that mark a volume as a cartridge rather than an ordinary drive.
    const MARKERS: [&str; 2] = ["cartridge.conf", "autorun.inf"];

    /// How long AutoPlay's own folder window takes to appear after arrival,
    /// before it is worth looking for.
    const AUTOPLAY_WINDOW_DELAY: Duration = Duration::from_millis(900);

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
    ///
    /// Behind a Mutex rather than a `static mut`: the message loop is
    /// single-threaded, so there is no contention to speak of, but a mutable
    /// reference to a static is undefined behaviour the moment that assumption
    /// stops holding, and the compiler is right to refuse it.
    static SEEN: Mutex<Option<HashMap<char, Instant>>> = Mutex::new(None);

    pub fn run() {
        crate::log::line("watcher starting");

        // Every logon starts this fresh, and a crash-restart from the
        // scheduled task can overlap the old instance for a moment — without
        // this, two watchers both hear the same WM_DEVICECHANGE broadcast and
        // each opens its own launcher window for the same cartridge. The
        // mutex is never released explicitly: it goes away when the process
        // does, which is the only time it should.
        if !acquired_single_instance() {
            crate::log::line("another watcher is already running; exiting");
            return;
        }

        // Its own thread: this one is about to block in the message queue for
        // the rest of the session, and PC/SC has its own blocking call.
        crate::nfc::spawn();

        *SEEN.lock().expect("no other thread to poison it") = Some(HashMap::new());

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

    fn on_volume_arrived(letter: char) {
        let now = Instant::now();

        {
            let mut guard = SEEN.lock().expect("no other thread to poison it");
            let seen = guard.get_or_insert_with(HashMap::new);
            if let Some(last) = seen.get(&letter) {
                if now.duration_since(*last) < DEBOUNCE {
                    crate::log::line(&format!("{letter}: ignoring repeat arrival"));
                    return;
                }
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

        // Recorded only once it is known to be a cartridge, so a plain USB
        // stick plugged in twice is not debounced into silence.
        if let Some(seen) = SEEN.lock().expect("no other thread to poison it").as_mut() {
            seen.insert(letter, now);
        }

        if crate::launcher::open(&root).is_some() {
            crate::log::line(&format!("{letter}: opened the launcher"));
        }

        // Every cartridge already opts out of the OS running anything on its
        // own — see cartridge.rs. AutoPlay's "open folder" is the same idea in
        // reverse: the launcher is the window a cartridge should show, so the
        // Explorer window AutoPlay opens for it is closed on its own thread,
        // rather than blocking WM_DEVICECHANGE while it waits for that window
        // to exist.
        std::thread::spawn(move || close_autoplay_window(letter));
    }

    /// Close the Explorer window AutoPlay opened for `letter`, if one exists.
    ///
    /// There is no event for "AutoPlay opened a folder", so this waits a beat
    /// and then looks: Explorer's title for a drive root always includes
    /// `(X:)`, which is enough to find the right window without COM.
    fn close_autoplay_window(letter: char) {
        std::thread::sleep(AUTOPLAY_WINDOW_DELAY);

        let needle = format!("({letter}:)").encode_utf16().collect::<Vec<u16>>();
        unsafe {
            EnumWindows(Some(enum_explorer_windows), &needle as *const Vec<u16> as LPARAM);
        }
    }

    unsafe extern "system" fn enum_explorer_windows(hwnd: HWND, lparam: LPARAM) -> i32 {
        let needle = &*(lparam as *const Vec<u16>);

        let mut class = [0u16; 64];
        let class_len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        // "CabinetWClass" is a real Explorer folder window, not the desktop or
        // a taskbar/tray host that also happens to own a top-level HWND.
        if class_len <= 0 || &class[..class_len as usize] != "CabinetWClass".encode_utf16().collect::<Vec<u16>>().as_slice() {
            return 1; // keep enumerating
        }

        let mut title = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if title_len > 0 && contains_utf16(&title[..title_len as usize], needle) {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
        }
        1
    }

    fn contains_utf16(haystack: &[u16], needle: &[u16]) -> bool {
        !needle.is_empty()
            && haystack
                .windows(needle.len())
                .any(|window| window == needle)
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

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// True when this process is the only watcher running.
    ///
    /// The handle returned by `CreateMutexW` is deliberately leaked: it must
    /// live for the process's whole lifetime, and the OS reclaims it on exit
    /// regardless.
    fn acquired_single_instance() -> bool {
        let name = wide("Global\\PcCartridgeWatcherSingleInstance");
        let handle =
            unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle == 0 {
            // Could not even ask; do not block a real launch on this.
            return true;
        }
        unsafe { windows_sys::Win32::Foundation::GetLastError() != ERROR_ALREADY_EXISTS }
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

        #[test]
        fn finds_the_drive_letter_wherever_it_sits_in_the_title() {
            let needle: Vec<u16> = "(D:)".encode_utf16().collect();
            assert!(contains_utf16(
                &"TEST (D:)".encode_utf16().collect::<Vec<u16>>(),
                &needle
            ));
            assert!(!contains_utf16(
                &"TEST (E:)".encode_utf16().collect::<Vec<u16>>(),
                &needle
            ));
            assert!(!contains_utf16(&[], &needle));
        }
    }
}

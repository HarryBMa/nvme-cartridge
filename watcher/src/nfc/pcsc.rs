//! PC/SC: the reader interface both platforms already have.
//!
//! `WinSCard.dll` is part of Windows. `libpcsclite` is a package on Linux, and
//! the daemon behind it is what every CCID reader — the ACR122U among them —
//! talks to. One interface covers both, which is why this is the tier that gets
//! written first.
//!
//! It is loaded at *runtime*, by name, rather than linked. Three reasons, in
//! order of how much they matter:
//!
//! 1. A reader is hardware almost nobody has. Linking `libpcsclite` would make
//!    a watcher that refuses to start on a machine without it, to support a
//!    feature that machine cannot use.
//! 2. It keeps the dependency list at zero, which is the standing rule for a
//!    process resident for a whole login session.
//! 3. The absence of the library becomes an ordinary answer — "no tag support
//!    here" — instead of a link error at install time.
//!
//! The card is never written to and never authenticated against. The only thing
//! sent is the PC/SC Get Data command, `FF CA 00 00 00`, which asks the *reader*
//! for the UID it already saw during anticollision.

use std::ffi::{c_void, CStr, CString};
use std::os::raw::c_char;
use std::time::Duration;

use super::TagEvent;
use crate::log;

// ---------------------------------------------------------------- ABI
//
// PC/SC is one API with two sets of integer widths. On Windows `DWORD` is 32
// bits and the handles are pointer-sized; on pcsc-lite `DWORD` is `unsigned
// long`, so 64 bits on the machines this runs on, and the handles are `long`.
// Getting this wrong does not fail to compile, it fails at the first call, so
// the two are spelled out rather than assumed.

#[cfg(windows)]
mod abi {
    pub type Dword = u32;
    pub type SLong = i32;
    /// `SCARDCONTEXT` is `ULONG_PTR` on Windows.
    pub type Context = usize;
    pub type CardHandle = usize;
    pub const MAX_ATR: usize = 36;
    pub const LIBRARY: &str = "WinSCard.dll";
    /// Windows exports the string-taking calls in ANSI and wide flavours.
    pub const LIST_READERS: &str = "SCardListReadersA";
    pub const GET_STATUS_CHANGE: &str = "SCardGetStatusChangeA";
    pub const CONNECT: &str = "SCardConnectA";
}

#[cfg(not(windows))]
mod abi {
    pub type Dword = std::os::raw::c_ulong;
    pub type SLong = std::os::raw::c_long;
    pub type Context = std::os::raw::c_long;
    pub type CardHandle = std::os::raw::c_long;
    pub const MAX_ATR: usize = 33;
    pub const LIBRARY: &str = "libpcsclite.so.1";
    pub const LIST_READERS: &str = "SCardListReaders";
    pub const GET_STATUS_CHANGE: &str = "SCardGetStatusChange";
    pub const CONNECT: &str = "SCardConnect";
}

use abi::{CardHandle, Context, Dword, SLong};

// Return codes, compared as u32 so the same literal works whether `LONG` is 32
// or 64 bits wide.
const SUCCESS: u32 = 0x0000_0000;
const E_TIMEOUT: u32 = 0x8010_000A;
const E_UNKNOWN_READER: u32 = 0x8010_0009;
const E_NO_READERS: u32 = 0x8010_002E;

const SCOPE_USER: Dword = 0;
const SHARE_SHARED: Dword = 2;
const PROTOCOL_T0_OR_T1: Dword = 1 | 2;
const LEAVE_CARD: Dword = 0;

const STATE_UNAWARE: Dword = 0x0000;
const STATE_CHANGED: Dword = 0x0002;
const STATE_UNKNOWN: Dword = 0x0004;
const STATE_PRESENT: Dword = 0x0020;

/// The pseudo-reader that reports readers themselves coming and going.
const PNP: &str = r"\\?PnP?\Notification";

/// How long to block in `SCardGetStatusChange` before looking around again.
///
/// Not infinite. The PnP pseudo-reader above makes a plugged-in reader wake us
/// immediately, but it is a convention rather than a guarantee, and a watcher
/// that needs restarting because a reader was plugged in second would be a poor
/// trade for one syscall every half minute.
const WAKE_EVERY_MS: Dword = 30_000;

/// After the service goes away — pcscd restarted, the last reader unplugged on
/// a system that stops the daemon with it — wait this long before trying again.
const RETRY: Duration = Duration::from_secs(5);

/// The card is not always ready the instant the reader says it is there.
const CONNECT_TRIES: u32 = 4;
const CONNECT_WAIT: Duration = Duration::from_millis(60);

/// Ask the reader for the UID it saw. PC/SC part 3, and the one command in this
/// file.
const GET_UID: [u8; 5] = [0xFF, 0xCA, 0x00, 0x00, 0x00];

#[repr(C)]
struct ReaderState {
    reader: *const c_char,
    user_data: *mut c_void,
    current_state: Dword,
    event_state: Dword,
    atr_len: Dword,
    atr: [u8; abi::MAX_ATR],
}

#[repr(C)]
struct IoRequest {
    protocol: Dword,
    pci_length: Dword,
}

impl IoRequest {
    /// What the exported `g_rgSCardT1Pci` globals contain. Built here rather
    /// than looked up, because a data symbol is a good deal more awkward to
    /// resolve by hand than a function one, and this is its whole content.
    fn for_protocol(protocol: Dword) -> IoRequest {
        IoRequest {
            protocol,
            pci_length: std::mem::size_of::<IoRequest>() as Dword,
        }
    }
}

type FnEstablish =
    unsafe extern "system" fn(Dword, *const c_void, *const c_void, *mut Context) -> SLong;
type FnRelease = unsafe extern "system" fn(Context) -> SLong;
type FnListReaders =
    unsafe extern "system" fn(Context, *const c_char, *mut c_char, *mut Dword) -> SLong;
type FnStatusChange = unsafe extern "system" fn(Context, Dword, *mut ReaderState, Dword) -> SLong;
type FnConnect = unsafe extern "system" fn(
    Context,
    *const c_char,
    Dword,
    Dword,
    *mut CardHandle,
    *mut Dword,
) -> SLong;
type FnTransmit = unsafe extern "system" fn(
    CardHandle,
    *const IoRequest,
    *const u8,
    Dword,
    *mut IoRequest,
    *mut u8,
    *mut Dword,
) -> SLong;
type FnDisconnect = unsafe extern "system" fn(CardHandle, Dword) -> SLong;

struct Api {
    establish: FnEstablish,
    release: FnRelease,
    list_readers: FnListReaders,
    status_change: FnStatusChange,
    connect: FnConnect,
    transmit: FnTransmit,
    disconnect: FnDisconnect,
}

impl Api {
    /// Load the library, or say why not.
    ///
    /// The handle is deliberately never closed: every function pointer below
    /// points into it, and the process only stops loading it by stopping.
    fn load() -> Option<Api> {
        let library = Library::open(abi::LIBRARY)?;
        // SAFETY: each symbol is transmuted to the signature PC/SC documents
        // for it, and the widths of every type in those signatures are pinned
        // per-platform in `abi` above.
        unsafe {
            Some(Api {
                establish: std::mem::transmute::<*mut c_void, FnEstablish>(
                    library.symbol("SCardEstablishContext")?,
                ),
                release: std::mem::transmute::<*mut c_void, FnRelease>(
                    library.symbol("SCardReleaseContext")?,
                ),
                list_readers: std::mem::transmute::<*mut c_void, FnListReaders>(
                    library.symbol(abi::LIST_READERS)?,
                ),
                status_change: std::mem::transmute::<*mut c_void, FnStatusChange>(
                    library.symbol(abi::GET_STATUS_CHANGE)?,
                ),
                connect: std::mem::transmute::<*mut c_void, FnConnect>(
                    library.symbol(abi::CONNECT)?,
                ),
                transmit: std::mem::transmute::<*mut c_void, FnTransmit>(
                    library.symbol("SCardTransmit")?,
                ),
                disconnect: std::mem::transmute::<*mut c_void, FnDisconnect>(
                    library.symbol("SCardDisconnect")?,
                ),
            })
        }
    }
}

// ---------------------------------------------------------------- loading

struct Library(*mut c_void);

#[cfg(not(windows))]
impl Library {
    fn open(name: &str) -> Option<Library> {
        let name = CString::new(name).ok()?;
        // SAFETY: a NUL-terminated name and documented flags.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        (!handle.is_null()).then_some(Library(handle))
    }

    fn symbol(&self, name: &str) -> Option<*mut c_void> {
        let name = CString::new(name).ok()?;
        // SAFETY: our own handle, and a NUL-terminated symbol name.
        let symbol = unsafe { libc::dlsym(self.0, name.as_ptr()) };
        (!symbol.is_null()).then_some(symbol)
    }
}

#[cfg(windows)]
impl Library {
    fn open(name: &str) -> Option<Library> {
        let name = CString::new(name).ok()?;
        // SAFETY: a NUL-terminated name.
        let handle = unsafe {
            windows_sys::Win32::System::LibraryLoader::LoadLibraryA(name.as_ptr() as *const u8)
        };
        (handle != 0).then_some(Library(handle as *mut c_void))
    }

    fn symbol(&self, name: &str) -> Option<*mut c_void> {
        let name = CString::new(name).ok()?;
        // SAFETY: our own handle, and a NUL-terminated symbol name.
        let symbol = unsafe {
            windows_sys::Win32::System::LibraryLoader::GetProcAddress(
                self.0 as isize,
                name.as_ptr() as *const u8,
            )
        };
        symbol.map(|address| address as *mut c_void)
    }
}

// ---------------------------------------------------------------- the loop

/// Watch every reader on the machine until the process ends.
pub fn run(deliver: &mut dyn FnMut(TagEvent)) {
    let Some(api) = Api::load() else {
        log::line(&format!(
            "{} is not available, so tags can only come from PC_GAMEPAK_NFC_SOURCE",
            abi::LIBRARY
        ));
        return;
    };

    let mut announced_failure = false;
    loop {
        if session(&api, deliver, &mut announced_failure).is_none() {
            // The service was not there. Say so once, then keep quiet about it:
            // on Linux the daemon is socket-activated and may simply not have
            // been started yet.
            if !announced_failure {
                log::line("no PC/SC service running; waiting for one");
                announced_failure = true;
            }
        }
        std::thread::sleep(RETRY);
    }
}

/// What we know about one reader between wakes.
struct Slot {
    name: CString,
    display: String,
    state: Dword,
    present: bool,
}

/// One context's worth of watching. Returns `None` if the service could not be
/// reached at all, so the caller knows whether to complain.
fn session(
    api: &Api,
    deliver: &mut dyn FnMut(TagEvent),
    announced_failure: &mut bool,
) -> Option<()> {
    let mut context: Context = Default::default();
    // SAFETY: reserved arguments are null as documented; `context` is written
    // only on success, which is what the return code is checked for.
    let rv =
        unsafe { (api.establish)(SCOPE_USER, std::ptr::null(), std::ptr::null(), &mut context) };
    if rv as u32 != SUCCESS {
        return None;
    }

    if *announced_failure {
        log::line("PC/SC is back");
        *announced_failure = false;
    }

    let mut slots: Vec<Slot> = Vec::new();
    let mut use_pnp = true;
    let mut announced_readers: Option<usize> = None;

    // Ends when the service stops answering, which is what `list_readers`
    // returning nothing means.
    while let Some(names) = list_readers(api, context) {
        // Carry over what each reader was doing, so a reader appearing next to
        // one that already holds a card does not re-announce that card.
        slots = names
            .into_iter()
            .map(
                |name| match slots.iter().position(|slot| slot.display == name) {
                    Some(index) => slots.swap_remove(index),
                    None => Slot {
                        name: CString::new(name.as_str()).unwrap_or_default(),
                        display: name,
                        state: STATE_UNAWARE,
                        present: false,
                    },
                },
            )
            .collect();

        if announced_readers != Some(slots.len()) {
            log::line(&match slots.len() {
                0 => "no card readers attached".to_string(),
                1 => format!("reader: {}", slots[0].display),
                n => format!(
                    "{n} readers: {}",
                    slots
                        .iter()
                        .map(|slot| slot.display.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
            announced_readers = Some(slots.len());
        }

        let mut states: Vec<ReaderState> = Vec::with_capacity(slots.len() + 1);
        let pnp = CString::new(PNP).unwrap_or_default();
        if use_pnp {
            states.push(state_for(&pnp, STATE_UNAWARE));
        }
        for slot in &slots {
            states.push(state_for(&slot.name, slot.state));
        }

        // SAFETY: `states` is a live array of exactly the length passed, and
        // every `reader` pointer in it borrows a CString that outlives the
        // call — `pnp` and `slots` are both still in scope here.
        let rv = unsafe {
            (api.status_change)(
                context,
                WAKE_EVERY_MS,
                states.as_mut_ptr(),
                states.len() as Dword,
            )
        };

        match rv as u32 {
            SUCCESS => {}
            // Waiting is the normal outcome. Round again, which re-lists the
            // readers — the fallback for a machine where PnP says nothing.
            E_TIMEOUT => continue,
            E_NO_READERS => {
                // No readers *and* no PnP support: there is nothing to block
                // on, so come back in a moment rather than spin.
                std::thread::sleep(RETRY);
                continue;
            }
            E_UNKNOWN_READER if use_pnp => {
                // This PC/SC does not know the pseudo-reader. Do without it.
                log::line("this PC/SC has no plug-and-play notifications; polling slowly instead");
                use_pnp = false;
                continue;
            }
            _ => break,
        }

        let offset = usize::from(use_pnp);
        for (slot, state) in slots.iter_mut().zip(states.iter().skip(offset)) {
            slot.state = state.event_state & !STATE_CHANGED;
            let now_present = present(state.event_state);
            match transition(slot.present, state.event_state) {
                Some(Change::Arrived) => match read_uid(api, context, &slot.name) {
                    Some(uid) => deliver(TagEvent::Arrived {
                        reader: slot.display.clone(),
                        uid,
                    }),
                    None => log::line(&format!(
                        "{}: something is on the reader, but it will not give a UID",
                        slot.display
                    )),
                },
                Some(Change::Departed) => deliver(TagEvent::Departed {
                    reader: slot.display.clone(),
                }),
                None => {}
            }
            slot.present = now_present;
        }
    }

    // SAFETY: a context this function established and has not released.
    unsafe { (api.release)(context) };
    Some(())
}

fn state_for(name: &CStr, current: Dword) -> ReaderState {
    ReaderState {
        reader: name.as_ptr(),
        user_data: std::ptr::null_mut(),
        current_state: current,
        event_state: 0,
        atr_len: 0,
        atr: [0; abi::MAX_ATR],
    }
}

/// The readers attached right now.
///
/// `None` means the service is gone; an empty list means it is there and has
/// nothing plugged into it, which are different situations.
fn list_readers(api: &Api, context: Context) -> Option<Vec<String>> {
    let mut length: Dword = 0;
    // SAFETY: a null buffer with a length out-parameter is the documented way
    // to ask how much space the answer needs.
    let rv =
        unsafe { (api.list_readers)(context, std::ptr::null(), std::ptr::null_mut(), &mut length) };
    match rv as u32 {
        SUCCESS => {}
        E_NO_READERS => return Some(Vec::new()),
        _ => return None,
    }

    let mut buffer = vec![0u8; length as usize];
    // SAFETY: the buffer is exactly the size the call above asked for, and
    // `length` is passed by pointer so it can be revised down.
    let rv = unsafe {
        (api.list_readers)(
            context,
            std::ptr::null(),
            buffer.as_mut_ptr() as *mut c_char,
            &mut length,
        )
    };
    match rv as u32 {
        SUCCESS => {}
        E_NO_READERS => return Some(Vec::new()),
        _ => return None,
    }

    buffer.truncate(length as usize);
    Some(split_multi_string(&buffer))
}

/// Ask the reader for the card's UID.
fn read_uid(api: &Api, context: Context, reader: &CStr) -> Option<String> {
    for attempt in 0..CONNECT_TRIES {
        if attempt > 0 {
            std::thread::sleep(CONNECT_WAIT);
        }

        let mut card: CardHandle = Default::default();
        let mut protocol: Dword = 0;
        // SAFETY: a NUL-terminated reader name that outlives the call, and two
        // out-parameters written only on success.
        let rv = unsafe {
            (api.connect)(
                context,
                reader.as_ptr(),
                SHARE_SHARED,
                PROTOCOL_T0_OR_T1,
                &mut card,
                &mut protocol,
            )
        };
        if rv as u32 != SUCCESS {
            continue;
        }

        let send = IoRequest::for_protocol(protocol);
        let mut response = [0u8; 64];
        let mut received: Dword = response.len() as Dword;
        // SAFETY: a connected card, a 5-byte command described by its own
        // length, and a receive buffer described by `received`, which the call
        // revises down to what it wrote.
        let rv = unsafe {
            (api.transmit)(
                card,
                &send,
                GET_UID.as_ptr(),
                GET_UID.len() as Dword,
                std::ptr::null_mut(),
                response.as_mut_ptr(),
                &mut received,
            )
        };

        // The card is left as it was found: not reset, not powered down. It may
        // well be somebody's door pass sitting on a desk reader.
        // SAFETY: a handle this loop connected and has not disconnected.
        unsafe { (api.disconnect)(card, LEAVE_CARD) };

        if rv as u32 != SUCCESS {
            continue;
        }

        let received = (received as usize).min(response.len());
        if let Some(uid) = uid_from_response(&response[..received]) {
            return Some(uid);
        }
    }

    None
}

// ---------------------------------------------------------------- pure parts
//
// Everything below is ordinary logic with no FFI in it, which is the half that
// can be wrong in interesting ways — so it is the half with tests.

#[derive(Debug, PartialEq, Eq)]
enum Change {
    Arrived,
    Departed,
}

fn present(event_state: Dword) -> bool {
    // A reader that has been unplugged reports UNKNOWN rather than "empty", and
    // whatever was on it is certainly not there any more.
    event_state & STATE_UNKNOWN == 0 && event_state & STATE_PRESENT != 0
}

/// What changed on one reader.
fn transition(was_present: bool, event_state: Dword) -> Option<Change> {
    match (was_present, present(event_state)) {
        (false, true) => Some(Change::Arrived),
        (true, false) => Some(Change::Departed),
        _ => None,
    }
}

/// PC/SC answers with a "multi-string": NUL-separated, and NUL-terminated
/// again at the end.
fn split_multi_string(buffer: &[u8]) -> Vec<String> {
    buffer
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect()
}

/// The UID out of a Get Data response: the payload, then two status bytes that
/// have to be 90 00.
fn uid_from_response(response: &[u8]) -> Option<String> {
    if response.len() < 3 {
        return None;
    }
    let (payload, status) = response.split_at(response.len() - 2);
    if status != [0x90, 0x00] {
        return None;
    }
    Some(crate::tags::format_uid(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_card_arriving_and_leaving_are_one_event_each() {
        // Empty reader, then a card on it.
        assert_eq!(
            transition(false, STATE_CHANGED | STATE_PRESENT),
            Some(Change::Arrived)
        );
        // Still there on the next wake: not an arrival again.
        assert_eq!(transition(true, STATE_PRESENT), None);
        // Lifted off.
        assert_eq!(transition(true, STATE_CHANGED), Some(Change::Departed));
        // And an empty reader that stays empty says nothing.
        assert_eq!(transition(false, STATE_CHANGED), None);
    }

    #[test]
    fn unplugging_the_reader_takes_the_card_with_it() {
        // A reader that has gone reports UNKNOWN, sometimes still with PRESENT
        // set from the state it was in. Whatever was on it has gone too.
        assert_eq!(
            transition(true, STATE_CHANGED | STATE_UNKNOWN | STATE_PRESENT),
            Some(Change::Departed)
        );
        assert_eq!(transition(false, STATE_UNKNOWN), None);
    }

    #[test]
    fn the_reader_list_is_a_multi_string() {
        let buffer = b"ACS ACR122U 00 00\0Yubico YubiKey 01 00\0\0";
        assert_eq!(
            split_multi_string(buffer),
            vec!["ACS ACR122U 00 00", "Yubico YubiKey 01 00"]
        );

        assert_eq!(split_multi_string(b"\0"), Vec::<String>::new());
        assert_eq!(split_multi_string(b""), Vec::<String>::new());
        assert_eq!(split_multi_string(b"One reader\0\0"), vec!["One reader"]);
    }

    #[test]
    fn a_uid_is_the_response_without_its_status_bytes() {
        // A 4-byte NTAG/MIFARE UID.
        assert_eq!(
            uid_from_response(&[0x04, 0xA2, 0x24, 0xB2, 0x90, 0x00]).as_deref(),
            Some("04A224B2")
        );
        // A 7-byte one, which is what most NTAGs actually have.
        assert_eq!(
            uid_from_response(&[0x04, 0xA2, 0x24, 0xB2, 0xC3, 0x1D, 0x80, 0x90, 0x00]).as_deref(),
            Some("04A224B2C31D80")
        );
    }

    #[test]
    fn a_card_that_will_not_answer_is_not_a_tag() {
        // "Instruction not supported" — a contact smartcard, or a phone
        // pretending to be one. Not something to open a launcher for.
        assert_eq!(uid_from_response(&[0x6D, 0x00]), None);
        // Success, but with no UID in front of it.
        assert_eq!(uid_from_response(&[0x90, 0x00]), None);
        assert_eq!(uid_from_response(&[]), None);
        assert_eq!(uid_from_response(&[0x04]), None);
    }

    #[test]
    fn the_pci_header_describes_itself() {
        let request = IoRequest::for_protocol(2);
        assert_eq!(request.protocol, 2);
        // Two DWORDs, whatever a DWORD is on this platform.
        assert_eq!(
            request.pci_length as usize,
            std::mem::size_of::<IoRequest>()
        );
    }
}

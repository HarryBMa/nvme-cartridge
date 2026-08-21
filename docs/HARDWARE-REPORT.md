# PC GamePak — first run on real Windows hardware

Branch: `claude/nvme-game-launcher-tauri-5g4p6g`
Date started: 2026-08-20
Machine: Windows 11 Pro 10.0.22621

Running log. Appended as work happens, not written up at the end.

---

## Test host

| Item | Value |
|---|---|
| OS | Windows 11 Pro 10.0.22621 |
| Shell session elevation | **Not elevated** (see Open issue 3) |
| rustc used for this run | **1.98.0 (88d9e12ae 2026-08-18)** via rustup |
| Rust also installed | standalone MSI `C:\Program Files\Rust stable MSVC 1.87` — shadows rustup on PATH (see Open issue 1) |
| node | v22.15.1 |
| npm | 11.16.0 |
| WebView2 runtime | 151.0.4129.93 — **present** |
| MSVC toolchain | present and linking (core and watcher link and run) |
| Steam | `c:/program files (x86)/steam`, running at session start |
| `config/libraryfolders.vdf` | present, 3 libraries: `C:\Program Files (x86)\Steam`, `B:\Steam`, `F:\Games\Steam` |

### Fixed disks — none of these is a cartridge candidate

| Disk | Model | Bus | Size | Letter | Label |
|---|---|---|---|---|---|
| 0 | CT1000MX500SSD1 | SATA | 931.5 GB | B: | Milo |
| 1 | Samsung SSD 850 EVO 500GB | SATA | 465.8 GB | E: | Harry |
| 2 | NVMe Samsung SSD 970 | NVMe | 931.5 GB | C: | Idris — **boot + system** |
| 3 | Force MP600 | NVMe | 1863.0 GB | F: | GAMES (108.3 GB free of 1863) |

All four are `DriveType = Fixed`.

### Cartridge candidate — the external NVMe

| Item | Value |
|---|---|
| Device | `\\.\PHYSICALDRIVE4` — disk 4 |
| Model | JMicron Tech SCSI Disk Device |
| Serial | DD56419883914 |
| Size | 256052966400 bytes (238.5 GiB / 256 GB) |
| Media type | External hard disk media |
| Partition style | **MBR** — one partition, offset 1048576, MBR type `FAT32 XINT13` |
| Filesystem | exFAT, label `External`, 238.40 GB free of 238.47 |
| Drive letter | **D:** |
| `GetDriveTypeW("D:\")` | **3 = `DRIVE_FIXED`** — not `DRIVE_REMOVABLE` (see Phase 1) |
| Bridge | USB `VID_152D&PID_A583` = **JMicron JMS583**, USB 3.1 Gen 2 NVMe-to-USB bridge |
| Driver bound | `USB Attached SCSI (UAS)` — **UASP, not BOT** |
| USB parent | `USB\ROOT_HUB30` — USB 3.x root hub |
| Port | `Port_#0005.Hub_#0004` |
| `SafeRemovalRequired` | True |
| `RemovalPolicy` | 3 (removable, surprise removal expected) |

**First real hardware measurement the project has ever had:** the enclosure
negotiates **UASP**, not BOT — Windows bound `uaspstor`, not `USBSTOR`. That is
the good outcome. Negotiated USB link speed not yet read; the wizard health
readout is the next place to look for it (Phase 2).

---

## Phase 1 — prepare the NVMe — **PASS**

The enclosure is **external, in a USB enclosure, not internal** — so
insert-detection has something to detect. JMicron JMS583 bridge, USB 3.1 Gen 2,
on a USB 3.x root hub, `Port_#0005.Hub_#0004`.

It was found in a state the storage stack would not enumerate at all (see the
former Open issue 2, now closed below). **A physical replug cleared it.** After
the replug it came up already initialised and already formatted:

| Item | State found in, after replug |
|---|---|
| Disk number | 4 |
| Partition style | MBR |
| Partitions | 1, spanning the disk |
| Filesystem | exFAT |
| Volume label | `External` |
| Drive letter | D: |
| Contents | empty — `$RECYCLE.BIN` and `System Volume Information` only |
| Free | 238.40 GB of 238.47 GB |

So no `Initialize-Disk` step was needed, and none was run. Nothing destructive
has been done to it.

**Docs gap, worth recording:** the brief expects an uninitialised drive to be set
up as **GPT**; this one arrived as **MBR**. The wizard formats the volume in
Phase 3 but does not repartition, so the cartridge will sit on an MBR disk. That
is fine for exFAT and for the launcher, but it means the partition style a
cartridge ends up with is whatever the drive already had — the wizard has no
opinion about it, and the docs do not say so.

### Finding: `GetDriveType` reports this enclosure as FIXED, and the drive list follows

`GetDriveTypeW("D:\")` returns **3, `DRIVE_FIXED`** — not `DRIVE_REMOVABLE`.
That is normal for a USB NVMe bridge, and `core/src/drives.rs:296` already
anticipates it, accepting both kinds with the comment *"A USB-C NVMe enclosure
usually reports FIXED, not REMOVABLE, so both are offered."* Without that, the
cartridge drive would not appear in the wizard at all. Correct call.

The cost is the other half of the filter. The only drive `list()` excludes is
`%SystemDrive%`, so on this host the wizard drive list will offer, alongside the
cartridge:

- B: Milo (931.5 GB, 159.2 GB free)
- E: Harry (465.8 GB, 438.3 GB free)
- F: GAMES (1863.0 GB, 108.2 GB free)

The brief expected the list to show the NVMe and **not** fixed disks. It cannot,
because on Windows the cartridge is itself a fixed disk by this API. What stands
between a mis-click and erasing B: Milo is the format confirmation gate
(`core/src/format.rs:192`), which refuses until the drive's current name is typed
back exactly. That gate is real and the UI prints the exact string to type
(`create.js:849`). Verified by reading, not yet exercised — Phase 3.

Flagged as a design question rather than fixed, since narrowing the filter would
hide the cartridge. See Open question 5.

### Wart: on Windows the "current label" is a display string

`format::current_label` reads `TargetDrive::label`. On Linux that is the bare
volume label (`CART`); on Windows `drives.rs:317` builds it for display as
`External (D:)`, label plus letter. So the Windows gate demands
`External (D:)` typed back, not `External`.

Harmless in practice — the UI shows the exact string in both the prompt and the
placeholder, so there is nothing to guess — and it makes the gate marginally
harder to satisfy by accident. But the two platforms ask for different things
from the same code path. Left alone; noted for Phase 3, where it can be
confirmed against the real dialog.

---

## Phase 0 — build and unit tests — **PASS**

Ran with the rustup stable toolchain, 1.98.0. Two host problems had to be
cleared first; both are described under Open issues, and neither was a defect in
this repo.

| Check | Result |
|---|---|
| `cargo test --manifest-path core/Cargo.toml` | **PASS** — 149 passed, 0 failed |
| `cargo test --manifest-path watcher/Cargo.toml` | **PASS** — 20 passed, 0 failed (after one fix, below) |
| `cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings` | **PASS** — clean |
| `cargo clippy --manifest-path watcher/Cargo.toml --all-targets -- -D warnings` | **PASS** — clean |
| `npm install` (tauri-ui) | **PASS** — 4 packages audited, 0 vulnerabilities |
| `npm run build` (tauri-ui, wraps `tauri build`) | **PASS** — release build in 1m 49s, **0 warnings** |
| `cargo build --release` (watcher) | **PASS** — 4.08s, 0 warnings |

### Bug found and fixed: a watcher test could never pass on Windows

`cargo test` on the watcher failed one of 20:

```
test tags::tests::a_directory_named_the_long_way_round_is_still_that_tag ... FAILED
panicked at src\tags.rs:179:39:
tag directory: Os { code: 123, kind: InvalidFilename,
  message: "Felaktig syntax för filnamn, katalognamn eller volymetikett." }
```

Cause: the test created a tag directory literally named `04:a2:24:b2`. A colon
cannot appear in a Windows filename, so `create_dir_all` returned
`ERROR_INVALID_NAME` (123). The test asserted correct behaviour — a tag
directory named with punctuation still resolves from the plain UID — but chose a
directory name that cannot exist on this platform. It had only ever run on
Linux, where the name is legal.

Not a production defect. `normalise` and `resolve_in` are correct; nothing that
ships was wrong.

Fix: name the directory `04-a2-24-b2`. Same behaviour under test, legal on both
platforms. The colon form is a *lookup* rather than a directory name, and is
already covered as one by `a_tag_resolves_to_its_directory` and
`a_uid_is_reduced_to_its_hex_digits`.

Commit `9cbbc69`. Re-verified: 20 passed, 0 failed.

### Build artifacts

| Artifact | Path | Size |
|---|---|---|
| Launcher and wizard | `tauri-ui/src-tauri/target/release/pc-gamepak.exe` | 7,148,544 bytes |
| Watcher | `watcher/target/release/pc-gamepak-watcher.exe` | 255,488 bytes |
| NSIS installer | `tauri-ui/src-tauri/target/release/bundle/nsis/PC GamePak_0.1.0_x64-setup.exe` | 2,284,089 bytes |
| MSI installer | `tauri-ui/src-tauri/target/release/bundle/msi/PC GamePak_0.1.0_x64_en-US.msi` | 3,026,944 bytes |

`npm run build` is `tauri build`, so it produces the binary *and* both
installers; there is no separate frontend build step. Note the binary lives
under `tauri-ui/src-tauri/target/release/`, not a top-level `target/release/`.
Tauri downloaded NSIS 3.11 and WiX 3.14 during bundling, so the first build on
a fresh machine needs network access.

Both binaries are unsigned, so SmartScreen prompts are expected and are not
defects.

---

## Phase 2 — the wizard, nothing destructive — **PARTIAL**

Run: `tauri-ui\src-tauri\target\release\pc-gamepak.exe --create`. SmartScreen
prompted, as expected for an unsigned binary; clicked through.

| Check | Result |
|---|---|
| Window opens | **PASS** |
| Steam games listed | **PASS** |
| Playnite games listed | **FAIL** — bug found and fixed, below |
| Drive list shows the cartridge | **PASS** — `External (D:)` present |
| Drive list excludes C: | **PASS** |
| Drive list excludes other fixed disks | **FAIL by design** — see Open question 5 |
| Health readout: link speed, UASP vs BOT | **FAIL — not present in the wizard at all** |
| Search / artwork | Partial — see below |
| Nothing written | **PASS** — Write not pressed |

### Bug found and fixed: Playnite games never appear, and nothing says why

Reported from the running wizard as *"only steam games are listed, no other"*.
Playnite is installed on this host (`%APPDATA%\Playnite`, `%LOCALAPPDATA%\Playnite`),
so this was not an absent-library case. Two faults compounding:

**1. Every extension's `config.json` counted as a library export.**
`playnite::find_exports` took every `.json` one level inside `ExtensionsData/*`.
Playnite keeps each installed extension's settings in `config.json`, so on this
host that scan returned seven files, all settings, none an export:

```
ExtensionsData/00000002-…/config.json
ExtensionsData/85dd7072-…/config.json
ExtensionsData/aebe8b7c-…/config.json
ExtensionsData/c2f038e5-…/config.json
ExtensionsData/cb91dfc9-…/config.json
ExtensionsData/cb91dfc9-…/tagnames-swedish.json
ExtensionsData/e3c26a3d-…/config.json
```

Because that list was not empty, the branch that says *"Playnite is installed at
… but has no JSON library export. Install a JSON library exporter extension and
run it."* could never run. Instead the code took the newest candidate and tried
to parse it as a library, which failed against an unrelated settings file.

Worth being clear that needing an exporter extension is **correct and
documented**: Playnite keeps its library in `games.db`, a LiteDB file with no
usable Rust reader, so a JSON export is the only way in. This host has no
exporter installed. The bug was never about that — it was that the wizard could
not say so.

**2. The reason was discarded even when it was correct.** `create::list_games`
collected a problem per library and then dropped the whole list whenever any
games were found. Steam almost always answers, so the Playnite failure never
reached the window under any circumstances.

Fixes, in commit `63c986c`:

- `config.json` is no longer a candidate export.
- New `playnite::import_newest_in` tries the remaining candidates newest-first
  until one parses, so a tag list or cache written after a real export cannot
  hide it. `NotFound` when none parse, which is what the user needs told.
- `list_games` returns `GameList { games, problems }`. The wizard shows the
  problems and opens the manual Playnite path field.

Three tests added; core is 152 passed, 0 failed. Clippy clean on core and on the
Tauri backend, and the release binary and both installers rebuilt.

**Still to confirm on this host:** with no exporter installed, the wizard should
now say so in as many words. Install any JSON library exporter extension in
Playnite and re-run to see real Playnite games listed — untested, because there
is no export on this machine to test against.

### Bug: there is no health readout in the wizard

`cartridge_health` is registered as a command and implemented, but it is invoked
only from `tauri-ui/app/src/main.js:215` — the **launcher**. The string
`cartridge_health` does not appear in `create.js` at all.

So the Phase 2 expectation of a health readout showing negotiated link speed and
UASP vs BOT while choosing a drive cannot be met: the wizard has never had one.
The information exists and the launcher shows it, but only once a cartridge has
been made and inserted.

Not fixed. Adding a health panel to the drive step is a feature, not a
correction, and it wants a design decision about when to run it — `health::inspect`
shells out to PowerShell on Windows, and doing that for every drive in the list
on every rescan is not free. See Open question 6.

This means the negotiated USB link speed is **still unmeasured**. It should
appear in the launcher in Phase 5.

### Artwork search needs a key, and says so

SteamGridDB search is off until switched on: `sgdb::api_key_from` refuses unless
`steamgriddb_enabled` is set and a key is present, and settings default to off.
So a fresh install has no artwork search, by design, and the dialog reports
`SteamGridDB unavailable. You can still paste a URL.`

Reported as *"search etc is a bit broken"*. The in-list game search
(`create.js:178`) is a plain case-insensitive substring match on name and source
and looks correct by inspection. Which of the two was meant is not yet pinned
down — **needs one more detail from the tester** before it can be called a bug.

---

## Open issues

### Open issue 1 — two Rust installations, and the old one wins on PATH

The machine has both:

- `C:\Program Files\Rust stable MSVC 1.87` — standalone MSI, **rustc 1.87.0**, first on PATH.
- rustup at `C:\Users\Harry\.cargo\bin\rustup.exe`, toolchain `stable-x86_64-pc-windows-msvc` = **rustc 1.98.0**.

A plain `cargo test` picks 1.87 and dies during resolution, because the
committed lockfiles pin `icu_* 2.3.0`, which declare `rust-version = 1.88`:

```
error: rustc 1.87.0 is not supported by the following packages:
  icu_collections@2.3.0 requires rustc 1.88
  ...
```

The `icu_*` tree is transitive: ureq, then url, then idna, then idna_adapter,
then icu_*. Nothing in this repo asks for it directly.

Everything in this report was run with `%USERPROFILE%\.cargo\bin` prepended to
PATH, which selects 1.98 and builds fine. **Nothing in the repo needs changing
for this.** It is a host PATH-ordering problem.

Worth deciding — your call, not done:

- Uninstall "Rust 1.87 (MSVC 64-bit)" from Add/Remove Programs, so the rustup
  shims are the only Rust on PATH. Recommended; two Rusts on one PATH will keep
  biting.
- And/or commit a `rust-toolchain.toml` pinning `stable`, so the real floor is
  stated in the repo instead of being discovered like this. The repo currently
  has no toolchain file and no stated MSRV.

### Open issue 2 (CLOSED — physical replug) — the cartridge drive was not enumerated by the storage stack

`\\.\PHYSICALDRIVE4` is present and healthy at the PnP and WMI layers, but the
Windows storage stack does not list it:

- `Get-CimInstance Win32_DiskDrive` — lists it, `Status = OK`.
- `Get-PnpDevice` — lists it and its UAS parent, `Status = OK`, `Present = True`, `Problem = CM_PROB_NONE`.
- `Get-Disk` / `MSFT_Disk` — **does not list it.** Only disks 0-3, before and after `Update-HostStorageCache`.

An uninitialised disk normally still appears in `Get-Disk` as
`PartitionStyle = RAW`. This one does not appear at all, so `Initialize-Disk`
has nothing to address.

Possible causes, untested: the NVMe inside the enclosure is not responding to
the bridge; the storage service will only resolve it with elevation; or a stale
enumeration that a physical replug would clear.

**Resolved by unplugging the enclosure and plugging it back in.** It then
enumerated as disk 4 with drive letter D:, already MBR and already exFAT. So it
was a stale enumeration, not a dead NVMe and not an elevation problem.

Worth keeping in the report because the failure mode is nasty: every layer that
reports health said the device was fine — `Win32_DiskDrive` `Status = OK`,
`Get-PnpDevice` `Present = True` `Problem = CM_PROB_NONE` — while the layer that
matters listed nothing, and `Update-HostStorageCache` did not shake it loose. If
a user hits this, the wizard will simply show no cartridge drive and give no
reason. **Replug first** is the answer, and it is not written down anywhere.

### Open issue 3 — the session is not elevated

`IsInRole(Administrator) = False`. Needed for: initialising the disk and
creating a partition, formatting, `FSCTL_LOCK_VOLUME` and
`FSCTL_DISMOUNT_VOLUME` on eject, Defender exclusions, and the watcher install
in `gamepak-windows.ps1`.

### Open issue 4 (host, now cleared) — corrupt cargo registry cache

Under rustc 1.98 the first builds still failed, differently:

```
error: invalid key
 --> ...\registry\src\index.crates.io-...\hashbrown-0.17.1\Cargo.toml:1:1
error: failed to download `hashbrown v0.17.1`
```

The cached `Cargo.toml` was 3847 bytes of **NUL**, with a bogus mtime of
`Jul 24 2006`. A scan of `~/.cargo/registry/src` found **167 of 254** extracted
crates with zeroed manifests — the signature of an unclean shutdown or power
loss while cargo was writing, and nothing to do with this project.

Cleared by deleting `~/.cargo/registry/src` (187.9 MB) and
`~/.cargo/registry/cache` (31.0 MB) and letting cargo re-fetch, plus
`cargo clean` on all three crates to drop 744.6 MB of artifacts built by the old
1.87 toolchain, which produced `error[E0786]: found invalid metadata files for
crate ...`.

Worth noting for the host: **C: has 23.3 GB free of 930.5 GB.** A read-only
`chkdsk C: /scan` would be a reasonable precaution given the corruption pattern.
Not run — needs elevation.

### Open question 5 — the drive list offers every non-system fixed disk

Raised by Phase 1. `drives::list()` on Windows takes both `DRIVE_REMOVABLE` and
`DRIVE_FIXED` — it has to, because a USB NVMe enclosure reports FIXED — and then
excludes only `%SystemDrive%`. On a machine with several data disks, all of them
appear in the wizard as things it will happily format.

Not fixed, because the obvious narrowing breaks the product: filter to
`DRIVE_REMOVABLE` and the cartridge disappears.

Options, none applied:

1. Leave it. The typed-label gate is the guard, and it is a good one.
2. Rank and mark: query `MSFT_Disk` for `BusType = USB` and sort those first,
   or badge the rest as "internal disk". Cosmetic, no behaviour change, keeps
   every drive reachable.
3. Refuse to *format* a non-USB disk while still listing it, so a fixed disk can
   receive a cartridge but never be erased by the wizard.

Your call. (2) is the cheap one; (3) is the one that would have prevented the
worst outcome on this host.

### Open question 6 — should the wizard show cartridge health?

Raised by Phase 2. The brief expects the drive step to show negotiated link
speed and UASP vs BOT. It does not, and never has: `cartridge_health` is wired
to the launcher only.

The cost is that the wizard cannot warn about the failure this project most
wants to catch — a cartridge on a BOT bridge, or negotiated at USB 2.0 — at the
one moment the user could still choose a different enclosure or port. They find
out after the copy instead.

The cost of adding it is that `health::inspect` shells out to PowerShell on
Windows. Running it per drive per rescan is too slow; running it once for the
selected drive, asynchronously, the way the launcher already does, is not.

Not implemented — it is a feature. Say if you want it and it is a small job.

---

## Phase status

| Phase | Status |
|---|---|
| 0 — build and unit tests | **PASS** — 172 tests, clippy clean, both binaries built (1 bug found and fixed) |
| 1 — prepare the NVMe | **PASS** — external USB enclosure, D:, UASP; needed a replug (former Open issue 2) |
| 2 — wizard, non-destructive | **PARTIAL** — window, Steam list and drive list pass; Playnite bug fixed; no health readout in the wizard |
| 3 — format and copy | Not started |
| 4 — cartridge contents | Not started |
| 5 — insert detection, Play, Eject | Not started |
| 6 — rewrite with Steam running | Not started |
| 7 — collections | Not started |
| 8 — tags without a reader | Not started |
| 9 — tuning, edit, controller, headroom | Not started |
| 10 — report | In progress (this file) |

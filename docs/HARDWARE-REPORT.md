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
| Device | `\\.\PHYSICALDRIVE4` |
| Model | JMicron Tech SCSI Disk Device |
| Serial | DD56419883914 |
| Size | 256052966400 bytes (238.5 GiB / 256 GB) |
| Media type | External hard disk media |
| Partitions | **none — no partition table** |
| Drive letter | **none** |
| Bridge | USB `VID_152D&PID_A583` = **JMicron JMS583**, USB 3.1 Gen 2 NVMe-to-USB bridge |
| Driver bound | `USB Attached SCSI (UAS)` — **UASP, not BOT** |
| USB parent | `USB\ROOT_HUB30` — USB 3.x root hub |
| Port | `Port_#0005.Hub_#0004` |
| `SafeRemovalRequired` | True |
| `RemovalPolicy` | 3 (removable, surprise removal expected) |

**First real hardware measurement the project has ever had:** the enclosure
negotiates **UASP**, not BOT — Windows bound `uaspstor`, not `USBSTOR`. That is
the good outcome. Negotiated USB link speed not yet read; it needs the drive to
be enumerated by the storage stack first (Open issue 2).

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

### Open issue 2 — the cartridge drive is not enumerated by the storage stack

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

Blocks Phase 1 onward. Nothing destructive has been attempted, and no drive
letter has been chosen.

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

---

## Phase status

| Phase | Status |
|---|---|
| 0 — build and unit tests | **PASS** — 169 tests, clippy clean, both binaries built (1 bug found and fixed) |
| 1 — prepare the NVMe | **Blocked** — Open issue 2 |
| 2 — wizard, non-destructive | Not started |
| 3 — format and copy | Not started |
| 4 — cartridge contents | Not started |
| 5 — insert detection, Play, Eject | Not started |
| 6 — rewrite with Steam running | Not started |
| 7 — collections | Not started |
| 8 — tags without a reader | Not started |
| 9 — tuning, edit, controller, headroom | Not started |
| 10 — report | In progress (this file) |

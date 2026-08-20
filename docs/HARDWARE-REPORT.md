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
| Shell session elevation | **Not elevated** (see Blocker 3) |
| rustc | 1.87.0 (17067e9ac 2025-05-09) |
| cargo | 1.87.0 (99624be96 2025-05-06) |
| Rust install | standalone MSI, `C:\Program Files\Rust stable MSVC 1.87` — **no rustup** |
| node | v22.15.1 |
| npm | 11.16.0 |
| WebView2 runtime | 151.0.4129.93 — **present** |
| Steam | `c:/program files (x86)/steam`, running at session start |
| `config/libraryfolders.vdf` | present |

### Fixed disks (not to be touched)

| Disk | Model | Bus | Size | Letter | Label |
|---|---|---|---|---|---|
| 0 | CT1000MX500SSD1 | SATA | 931.5 GB | B: | Milo |
| 1 | Samsung SSD 850 EVO 500GB | SATA | 465.8 GB | E: | Harry |
| 2 | NVMe Samsung SSD 970 | NVMe | 931.5 GB | C: | Idris — **boot + system** |
| 3 | Force MP600 | NVMe | 1863.0 GB | F: | GAMES (108.3 GB free of 1863) |

All four are `DriveType = Fixed`. None is a candidate cartridge.

### Cartridge candidate

| Item | Value |
|---|---|
| Device | `\.\PHYSICALDRIVE4` |
| Model | JMicron Tech SCSI Disk Device |
| Serial | DD56419883914 |
| Size | 256052966400 bytes (238.5 GiB / 256 GB) |
| Media type | External hard disk media |
| Partitions | **none — no partition table** |
| Drive letter | **none** |
| Bridge | USB `VID_152D&PID_A583` = **JMicron JMS583**, USB 3.1 Gen 2 NVMe-to-USB bridge |
| Driver attached | `USB Attached SCSI (UAS)` — **UASP, not BOT** |
| USB parent | `USB\ROOT_HUB30` — USB 3.x root hub |
| Port | `Port_#0005.Hub_#0004` |
| `SafeRemovalRequired` | True |
| `RemovalPolicy` | 3 (removable / surprise-removal expected) |

**First real measurement of the project:** the enclosure negotiates **UASP**, not
BOT. Windows bound `uaspstor` rather than `USBSTOR`. That is the good outcome and
had never been observed before.

---

## Phase 0 — build and unit tests

### `cargo test --manifest-path core/Cargo.toml` — **FAIL**

Never reached compilation. Dependency resolution rejected the toolchain:

```
error: rustc 1.87.0 is not supported by the following packages:
  icu_collections@2.3.0 requires rustc 1.88
  icu_locale_core@2.3.0 requires rustc 1.88
  icu_normalizer@2.3.0 requires rustc 1.88
  icu_normalizer_data@2.3.0 requires rustc 1.88
  icu_properties@2.3.0 requires rustc 1.88
  icu_properties_data@2.3.0 requires rustc 1.88
  icu_provider@2.3.0 requires rustc 1.88
```

Diagnosis: `core/Cargo.lock` is committed with `icu_* 2.3.0` pinned. Those crates
declare `rust-version = 1.88`. The installed toolchain is 1.87.0. The `icu_*` tree
is pulled in transitively: `ureq` → `rustls` / `webpki-roots` → `idna_adapter` →
`icu_*`. Nothing in the repo asks for it directly.

The repo has **no `rust-toolchain.toml`**, so nothing pinned the toolchain and
nothing warned that 1.88 is the real floor. CI on Windows evidently runs a newer
stable, which is why this never surfaced.

Status: **blocked**, awaiting a decision on the fix (see Blocker 1).
Not yet run, because they share the same dependency tree: watcher tests, both
clippy runs, the `tauri-ui` build.

---

## Blockers — need a decision

### Blocker 1 — toolchain too old for the committed lockfiles

Options:

1. **Install rustup and move to current stable.** Correct fix. rustc 1.87 is from
   May 2025. Tauri 2 and the rest of the tree will keep drifting past it. Changes
   the machine (installs rustup, and rustup and a standalone MSI Rust on the same
   PATH need the MSI one removed or ordered after).
2. **Pin the dependencies down** with `cargo update --precise` across all three
   lockfiles until the tree builds on 1.87. Keeps the machine untouched. It edits
   committed lockfiles, may cascade through `idna`/`rustls`, and CI will drift
   back the moment anything is re-resolved.

Recommendation: option 1, plus commit a `rust-toolchain.toml` so the floor is
stated in the repo rather than discovered like this.

### Blocker 2 — the cartridge drive is unconfirmed and invisible to the storage stack

`\.\PHYSICALDRIVE4` is present and healthy at the PnP and WMI layers, but the
Windows storage stack does not enumerate it at all:

- `Get-CimInstance Win32_DiskDrive` — lists it, `Status = OK`.
- `Get-PnpDevice` — lists it and its UAS parent, `Status = OK`, `Present = True`,
  `Problem = CM_PROB_NONE`.
- `Get-Disk` / `MSFT_Disk` — **does not list it.** Only disks 0–3 appear, before
  and after `Update-HostStorageCache`.

An uninitialised disk normally still appears in `Get-Disk` with
`PartitionStyle = RAW`. This one does not appear at all. Possible causes: the
enclosure is enumerating but the NVMe inside is not responding to the bridge; the
disk is in a state the storage service will only resolve with elevation; or a
stale enumeration that a physical replug would clear.

Cannot proceed to Phase 1 until this is identified and confirmed to be the empty
drive. Nothing destructive attempted.

### Blocker 3 — session is not elevated

`IsInRole(Administrator) = False`. Needed for: initialising the disk and creating
a partition, formatting, `FSCTL_LOCK_VOLUME` / `FSCTL_DISMOUNT_VOLUME` on eject,
Defender exclusions, and the watcher install in `gamepak-windows.ps1`.

---

## Phase status

| Phase | Status |
|---|---|
| 0 — build and unit tests | **FAIL** — blocked on toolchain |
| 1 — prepare the NVMe | **Blocked** — drive not enumerated by storage stack |
| 2 — wizard, non-destructive | Not started |
| 3 — format and copy | Not started |
| 4 — cartridge contents | Not started |
| 5 — insert detection, Play, Eject | Not started |
| 6 — rewrite with Steam running | Not started |
| 7 — collections | Not started |
| 8 — tags without a reader | Not started |
| 9 — tuning, edit, controller, headroom | Not started |
| 10 — report | In progress (this file) |

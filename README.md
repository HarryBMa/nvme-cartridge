# NVMe Game Cartridges

Turn NVMe drives into physical game cartridges. Plug one in, and a launcher pops
up with the game's cover art and two buttons: **Play** and **Eject**.

<img width="420" alt="The cartridge launcher showing cover art, the game title, and Play / Eject buttons" src="docs/launcher.png" />

Each cartridge is just a drive with a `cartridge.conf` at its root naming the
game. There are no scripts to write and nothing to allowlist.

## How it works

```
drive plugged in
      │
      ├─ Linux    udev rule ──▶ systemd unit ──▶ helper waits for the mount
      └─ Windows  watcher (resident, ~2 MB) sees the volume arrive
      │
      ▼
is there a cartridge.conf at the root?   ──no──▶  nothing happens
      │ yes
      ▼
launcher opens with the cover art
      │
      ├─ Play   ──▶ starts what cartridge.conf names, then closes
      └─ Eject  ──▶ flushes, unmounts, powers the drive down
```

Nothing on the cartridge is executed automatically. The launcher shows you what
it found and waits — pressing Play is the gate.

### Idle cost

The point is that this costs nothing while you are not using it.

| | Idle |
|---|---|
| **Linux** | **nothing resident.** udev is already part of the OS; the rule adds no process. The launcher exists only while its window is open. |
| **Windows** | **one small process, ~2 MB.** `pc-cartridge-watcher.exe` blocks on the Windows message queue, so it uses no CPU until a volume arrives. |

The launcher itself is a webview, so it is not small while it is on screen —
expect roughly 100 MB for the few seconds it is up, then it exits and gives all
of it back. There is no tray icon and no background service for the UI.

## Hardware

Built around **NVMe drives in USB-C enclosures**. Small capacities are ideal —
128 GB holds most single games, and the drive's whole job is to be one cartridge.

It works with any removable storage the OS will automount: SATA SSDs in docks,
SD cards, USB sticks, external HDDs. Nothing here is specific to NVMe beyond the
form factor being pleasant to handle.

**Filesystem:** exFAT if you want the cartridge to work on both Windows and
Linux. NTFS or ext4 are fine for one OS.

## Setup

### Prerequisites

Rust (stable) and Node 18+, plus a C toolchain for your platform.

```bash
# Linux
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev libssl-dev

# Windows: Visual Studio Build Tools, "Desktop development with C++"
```

### Build and install

```bash
git clone https://github.com/HarryBMa/nvme-cartridge.git
cd nvme-cartridge

# The launcher, on both platforms
cd tauri-ui && npm install && npm run build && cd ..
```

**Linux**

```bash
./cartridge-linux.sh     # → 1) Install
```

Installs the udev rule, the systemd template unit, the mount helper and the
launcher binary.

**Windows**

```powershell
cd watcher; cargo build --release; cd ..
# Right-click cartridge-windows.ps1 → Run with PowerShell → 1) Install
```

Installs the watcher and launcher to `%LOCALAPPDATA%\PC-Cartridge-System` and
registers a logon task to start the watcher.

## Making a cartridge

Copy `cartridge.conf.example` to the root of the drive as `cartridge.conf`:

```ini
executable=steam://rungameid/1091500
title=Cyberpunk 2077
cover=cover.jpg
```

Drop a `cover.jpg` next to it — portrait art at 3:4 fills the launcher window
exactly. That is the entire cartridge:

```
NVME-DRIVE/
├── cartridge.conf
├── cover.jpg
└── SteamLibrary/        (optional — the game itself can live here)
```

Then unplug it and plug it back in.

`executable=` takes any URI the OS can handle (`steam://`, `heroic://`, `gog://`,
`epic://`, `playnite://`, `lutris://`, `http://`, `https://`) or a path to a file
on the cartridge. See `cartridge.conf.example` for the full list of keys.

A classic `autorun.inf` is also read, for `label` and `icon` only. Its `open=`
and `shellexecute=` keys are deliberately ignored — Windows has ignored them on
non-optical media since Windows 7, and they are the oldest autorun malware vector
there is.

## Security

Nothing on a cartridge runs without a click. That is the whole model, and it is
why there is no trust list, no SHA-256 allowlist and no auto-launch toggle —
earlier versions auto-executed a `launch.sh` on insert, which needed an allowlist
to be safe at all.

What that leaves:

- **Play runs what `cartridge.conf` says.** If `executable=` points at a binary
  on the drive, Play runs that binary. On your own cartridges that is the
  feature. On a drive someone hands you, read the conf first — or keep to
  `steam://`-style URIs, where the argument goes to a program you already trust.
- **The launcher window cannot read your disk.** The webview has no filesystem
  access and no command that takes a path. The cover is read in Rust, from a path
  confined to the cartridge, and passed in as a `data:` URI.
- **Nothing is fetched.** Fonts are bundled, the cover is inlined, and the
  content-security policy is `default-src 'self'`.
- **A plugged-in cartridge shows a window.** Detection reads
  `cartridge.conf` — a text file — and draws the title. Titles are inserted as
  text, never as markup.

## Layout

```
cartridge-linux.sh          installer menu (Linux)
cartridge-windows.ps1       installer menu (Windows)
cartridge.conf.example      the one file a cartridge needs
linux/                      udev rule, systemd units, mount + eject helpers
windows/                    install / uninstall / eject scripts
watcher/                    resident volume watcher (Windows only, Rust)
tauri-ui/                   the launcher popup (Tauri 2 + Rust, no framework)
docs/                       screenshots
```

## Uninstall

Run the installer menu and choose Uninstall. It removes the udev rule and
systemd units (Linux), or the logon task and install folder (Windows).

## Disclaimer

A hobby project, not affiliated with Valve or Steam.

Auto-detection depends on your OS automounting removable drives. Some setups need
that configured before any of this works.

Use at your own risk.

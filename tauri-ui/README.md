# PC Cartridge Launcher — Tauri UI

A compact, dark popup window that appears when a cartridge is inserted.
Built with **Tauri 2.0** (Rust backend) and plain HTML/CSS/JS (no framework needed).

```
┌──────────────────────────────┐
│  PC CARTRIDGE            [×] │   ← draggable title bar
├──────────────────────────────┤
│                              │
│         [cover art]          │   ← 260 px cover image
│                              │
├──────────────────────────────┤
│  Cyberpunk 2077              │   ← title from cartridge.conf
│  steam://rungameid/1091500   │   ← executable field
│                              │
│  Ready                       │   ← status bar
├──────────────────────────────┤
│   [▶ Play]    [⏏ Eject]      │
└──────────────────────────────┘
```

## Prerequisites

### Windows
- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18+
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (C++ workload)

### Linux
```bash
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev \
                 librsvg2-dev libssl-dev
```

## Build & run

```bash
cd tauri-ui
npm install
npm run build     # production bundle → src-tauri/target/release/
# or
npm run dev       # dev server with hot-reload
```

## Launching from a cartridge script

The UI reads the cartridge drive path from the `?drive=` query parameter.
Your cartridge's `launch.ps1` (Windows) or `launch.sh` (Linux) should start the
launcher binary with the drive path:

**Windows `launch.ps1`:**
```powershell
$Drive = Split-Path -Qualifier $MyInvocation.MyCommand.Path
$LauncherExe = "C:\Path\To\pc-cartridge-launcher.exe"
Start-Process $LauncherExe -ArgumentList "--drive `"$Drive\`""
```

**Linux `launch.sh`:**
```bash
DRIVE="$(df --output=target "$(dirname "$0")" | tail -1)"
/usr/local/bin/pc-cartridge-launcher --drive "$DRIVE"
```

The Tauri app can also be started with a `?drive=` query string when the
dev server is running:

```
http://localhost:1420/?drive=D%3A%5C
```

## Cartridge format

Place a `cartridge.conf` at the root of the SSD:

```ini
executable=steam://rungameid/1091500
title=Cyberpunk 2077
cover=cover.png
```

Or use a classic `autorun.inf`:

```ini
[autorun]
label=Cyberpunk 2077
open=Game\bin\game.exe
icon=cover.png
```

Supported URI schemes: `steam://`, `heroic://`, `gog://`, `epic://`,
`playnite://`, `lutris://`, `http://`, `https://`.

## Project layout

```
tauri-ui/
├── index.html                  # Main HTML shell
├── src/
│   ├── main.js                 # Frontend logic (Tauri invoke calls)
│   └── style.css               # Dark popup styles
├── src-tauri/
│   ├── Cargo.toml
│   ├── build.rs
│   ├── tauri.conf.json
│   ├── capabilities/
│   │   └── default.json        # Tauri 2.0 capability file
│   ├── icons/
│   │   └── icon.png
│   └── src/
│       └── main.rs             # Rust backend (parse, launch, eject)
└── package.json
```

## Security note

The eject command uses `DeviceIoControl` (Windows) or `udisksctl` (Linux) to
safely flush write caches before the drive is powered off.  Always use the
**Eject** button rather than pulling the cartridge directly.

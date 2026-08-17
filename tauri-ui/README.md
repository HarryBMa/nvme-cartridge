# PC Cartridge Launcher — Tauri UI

A compact popup that appears when a cartridge is inserted.
Built with **Tauri 2.0** (Rust backend) and plain HTML/CSS/JS — no framework, no
bundler, no network access at runtime.

![The launcher, showing cover art, the game title, and Play / Eject](../docs/launcher.png)

The window is 420 × 560 — the 3:4 of a cover — and the cover art fills it. Only
three things sit on top: what the cartridge is, **Play**, and **Eject**.
Everything the launcher knows beyond that lives behind the gear:

![The details sheet](../docs/launcher-details.png)

### Design notes

- **The accent colour is sampled from the cover art** at load, so the Play
  button belongs to whatever game is in the dock. Each pixel is weighted by its
  own saturation squared and biased toward the lit areas, because a flat average
  of any cover is mud. The ink on the button is then chosen for contrast against
  that colour, so the label stays readable whatever the artwork is.
- **The scrim behind the title is deliberately short.** It has to carry the
  title and the buttons and nothing else; a tall, soft gradient would dim the
  half of the artwork people actually look at.
- **Nothing is fetched.** Fonts are bundled as woff2 and the cover is passed in
  as a `data:` URI, so the CSP can stay at `default-src 'self'` and the Tauri
  asset protocol stays switched off.

### Keyboard

| Key | Action |
|-----|--------|
| `Enter` | Play |
| `E` | Eject |
| `I` | Toggle details |
| `Esc` | Close details, or dismiss the window |

## Previewing without a cartridge

The page serves a sample cartridge when it is opened outside Tauri, so the
window can be worked on without a physical drive:

```bash
npx http-server tauri-ui     # → http://localhost:8080/?drive=/demo
```

Append `&state=noexec` to see the case where `cartridge.conf` sets no
`executable`.

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
│   ├── style.css               # Popup styles
│   ├── fonts/                  # Bundled woff2 (Archivo, Spline Sans Mono)
│   └── demo/                   # Sample cover art for browser preview
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

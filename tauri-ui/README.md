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
npm run build     # → src-tauri/target/release/pc-cartridge-launcher
npm run dev       # run it against a drive, for development
```

There is no bundler: `index.html` and `src/` are shipped as-is, which is why
`frontendDist` points at this directory.

## The create-cartridge wizard

The same binary, started with `--create`, opens the wizard that writes
cartridges:

```bash
pc-cartridge-launcher --create
```

![The create-cartridge wizard](../docs/wizard.png)

Only one window is ever built — `main()` looks at the arguments and constructs
either the popup or the wizard, never both, so neither mode costs memory while
the other is in use.

The game list comes from Steam's own `appmanifest_*.acf` files across every
library folder in `libraryfolders.vdf`, and the art from Steam's
`appcache/librarycache`. Nothing is fetched. Games that are mid-download are
skipped, since a cartridge pointing at a partial install cannot play.

Covers are loaded one at a time as you select a game: base64ing a whole library
into the webview at once would be tens of megabytes of IPC.

## How it gets started

The launcher takes the cartridge's mount point on the command line and shows one
cartridge per window:

```bash
pc-cartridge-launcher --drive /run/media/you/CARTRIDGE   # Linux
pc-cartridge-launcher.exe --drive "D:\"                  # Windows
```

Nothing on the cartridge invokes it. On Linux a udev rule starts
`linux/cartridge-launcher-helper.sh`, which waits for the automount and then runs
the launcher; on Windows `watcher/` sees the volume arrive and does the same. See
the [main README](../README.md) for the full path.

The window exits after Play, Eject or dismiss, so nothing stays resident.

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

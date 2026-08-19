<div align="center">

<img src="docs/icon.png" width="96" alt="" />


<a href="https://ko-fi.com/harrybma">Support my work!</a>


# PC GamePak

**Turn removable storage into physical game cartridges.**
Plug one in and a launcher appears with the game's cover art and two buttons.

<img width="420" alt="The cartridge launcher: cover art filling the window, the game title, and Play and Eject buttons" src="docs/launcher.png" />

</div>

---

A cartridge is a small drive in a pocketable enclosure with a game on it. Push it
into a USB-C port; the launcher opens showing what's on it. Press **Play** to
start the game, or **Eject** to power the drive down and pull it out.

Each cartridge is just a drive with a `cartridge.conf` text file at its root.
There are no scripts to write and nothing to allowlist, because **nothing on a
cartridge is ever executed automatically** — pressing Play is the gate.

```bash
git clone https://github.com/HarryBMa/pc-gamepak.git
cd pc-gamepak
cd tauri-ui && npm install && npm run build && cd ..

./gamepak-linux.sh          # Linux  → 1) Install
# Windows: right-click gamepak-windows.ps1 → Run with PowerShell → 1) Install
```

Then plug a drive in, or run `pc-gamepak --create` to make one.

## Contents

**Using it**
&nbsp;&nbsp;[The idea](#the-idea) ·
[The launcher](#the-launcher) ·
[Making a cartridge](#making-a-cartridge) ·
[Getting the most out of a cartridge](#performance)

**Building one**
&nbsp;&nbsp;[Hardware](#hardware) ·
[Cartridge format](#cartridge-format) ·
[Setup and install](#setup) ·
[Uninstall](#uninstall)

**Under it**
&nbsp;&nbsp;[How it works](#how-it-works) ·
[Security](#security) ·
[Working on it](#working-on-it) ·
[Packages](#packages)

**Everything else**
&nbsp;&nbsp;[Thanks](#thanks) ·
[Licence](#licence) ·
[Disclaimer](#disclaimer)

Click any heading below to open it.

---

<a id="the-idea"></a>
<details open>
<summary><b>The idea</b> — what a cartridge is, and why</summary>
<br />

It's the console-cartridge feeling, using hardware that already exists — and it
genuinely offloads a game off your internal disk, which is the practical half of
the appeal.

A shelf of cartridges is a library you can hold. Each one carries a game, its
cover art and nothing else, so what is on it is obvious from across the room and
from the drive's own name in Explorer.

The build documented here uses an **M.2 2230 NVMe** stick in a USB enclosure, but
nothing in the software requires that: any removable drive your OS automounts
will do — a SATA SSD in a dock, an SD card, a USB stick, an external HDD. The
form factor is a comfort choice, not a technical one.

</details>

<a id="the-launcher"></a>
<details>
<summary><b>The launcher</b> — one window, cover art, Play and Eject</summary>
<br />

The window is 420 × 560 — the 3:4 of a cover — and the artwork fills it. Only
three things sit on top: what the cartridge is, Play, and Eject.

<img width="420" alt="The launcher showing Cinder &amp; Salt with Play and Eject" src="docs/launcher.png" />
<img width="420" alt="The details sheet, showing mount point, launch target and cover path" src="docs/launcher-details.png" />

The accent colour is sampled from the cover art at load, so the Play button
belongs to whatever game is in the dock. Everything the launcher knows beyond
the title lives behind the gear, because you rarely need it.

### More than one game on a cartridge

A 256 GB drive holds a series, not a game. Put several on one cartridge and the
launcher shows the collection's artwork and title with **one Play button per
game** — no menu, no submenu, nothing to learn.

<img width="420" alt="The launcher showing a collection: the artwork behind a list of games, each with its own Play button, and Eject below" src="docs/launcher-bundle.png" />

Each row carries the game's own art, and the first nine answer to the number
keys. A cartridge with one game on it still gets the plain Play and Eject pair.

| Key | Action |
|-----|--------|
| `Enter` | Play, or the first game of a collection |
| `1`–`9` | Play the *n*th game of a collection |
| `E` | Eject |
| `I` | Details |
| `Esc` | Close details, or dismiss |

</details>

<a id="making-a-cartridge"></a>
<details>
<summary><b>Making a cartridge</b> — the wizard: pick a game, pick a drive, Write</summary>
<br />

Run the installer menu and choose **Create a cartridge**, or start it directly:

```bash
pc-gamepak --create
```

<img width="760" alt="The create-cartridge wizard: searchable game list on the left, cover preview, drive picker and options on the right" src="docs/wizard.png" />

The wizard lists everything installed. **Playnite** is read first when present —
one list covering Steam, GOG, Epic, Xbox, Ubisoft, itch and emulators — and
Steam's own manifests are read too, which is the only source on Linux. Cover art
comes from whichever cache the game came from, so **nothing is fetched**; the
wizard **works offline**, with the optional SteamGridDB integration switched
off — see [Artwork from SteamGridDB](#artwork-from-steamgriddb) to turn it on.

Playnite is detected automatically on Windows (the standard `%APPDATA%\Playnite`
location and portable installs in `Program Files`) and on Linux through every
Proton `compatdata` prefix that contains a Playnite install. If detection fails,
a **Playnite data folder** field appears at the bottom of the game list so you
can point the wizard at the right directory.

> **Note:** Playnite stores its library in a binary database. The wizard reads a
> JSON export instead — install a library-exporter extension in Playnite and run
> it once before using the wizard. Any extension that writes a `library.json` or
> `games.json` file will work.

Pick a game, pick the drive, choose what goes on it, press Write.

### Collections

Add a second game with the **+** beside it and the cartridge becomes a
collection. The wizard then asks for the two things it cannot work out on its
own — what to call it, and what it should look like:

<img width="760" alt="The wizard with two games added to a bundle: the collection preview, a collection name field, a Choose artwork button, and the copy option covering both games" src="docs/wizard-bundle.png" />

- **The name** is suggested from what the games share — *God of War* and *God of
  War Ragnarök* give *God of War Collection* — and can be typed over.
- **The artwork** is whatever picture you point at, through the desktop's own
  file dialog. Without one the first game's cover stands in.
- **The order** is the order you added them in, and it is the order of the Play
  buttons in the launcher.

Copying works the same for a collection as for a single game: tick the box and
every game goes across, each Play button pointing at its own copy.

### What it can put on the cartridge

**The launcher files** — `cartridge.conf` and the cover art. Always written.

**The drive's name and icon** — an `autorun.inf` with `label=`, so Explorer shows
*HOLLOW KNIGHT* rather than *Removable Disk (D:)*. The `icon=` key is written
when a usable `.ico` can be produced; Explorer will not take a JPEG, so a
Steam-sourced cover usually leaves the default icon in place.

**The game itself**, by whichever route suits where it came from:

<img width="760" alt="The wizard copying a GOG game, with a dropdown choosing which executable Play should start" src="docs/wizard-portable.png" />

- *Steam games* go to `steamapps/` and the drive is registered in Steam's
  `libraryfolders.vdf`, so Steam plays **from the cartridge** rather than your
  internal copy. Close Steam first — it rewrites that file when it exits.
- *Everything else* — GOG, itch, emulator builds, anything Playnite records an
  install folder for — is copied to `Games/<title>/` and Play is pointed at a
  file inside it. No launcher in the middle. The wizard ranks the executables it
  finds (Playnite's own play action first, then a binary named after the game;
  uninstallers and redistributables sink) and offers the best guess, which you
  can change.

**Games in no library at all** can be entered by hand with any supported URI or a
path on the cartridge.

<a id="artwork-from-steamgriddb"></a>

### Artwork from SteamGridDB

Some games have no cached art at all — anything added to Playnite by hand,
emulator entries, older GOG titles — and the launcher then shows a placeholder.
The wizard can look artwork up on [SteamGridDB](https://www.steamgriddb.com/)
to fill those gaps.

<img width="760" alt="The wizard's settings: a switch for SteamGridDB lookup, off by default, and a field for a personal API key" src="docs/wizard-settings.png" />

**It is off by default**, and it is the only part of this project that talks to
the network. Turn it on behind the gear in the wizard's title bar, where it also
asks for a personal API key — their API refuses unauthenticated requests, so the
lookup does nothing without one. The key is stored on this machine only, next to
the artwork cache, and the backend refuses every request while the setting is
off, so hiding the button is not the only thing keeping it quiet.

With it off you can still give a cartridge any picture you like: **Choose
artwork…** opens the desktop's own file dialog and copies whatever you point at.

### Formatting erases the drive

<img width="760" alt="The wizard with formatting enabled: a field asking you to type the drive's current name, with Write disabled until it matches" src="docs/wizard-format.png" />

Formatting is opt-in per cartridge and gated four ways: the target must be on
the removable-drive allowlist the wizard re-derives itself, it must not be the
system drive, you must type the drive's **current** name back exactly, and
Write stays disabled until you have. The backend re-checks all of it — it never
trusts the window's idea of where to write.

### Which filesystem

**exFAT is the default, and it is the right answer for a cartridge you hand to
someone.** Windows, Linux and macOS all read it with nothing to install, which
is the entire point of a thing you carry between machines.

**btrfs is there for enthusiasts**, and it is a real choice with real costs:

- It brings TRIM (`discard=async`) and transparent zstd compression
  (`compress=zstd`).
- Windows cannot read it at all without [WinBtrfs](https://github.com/maharmstone/btrfs),
  a third-party kernel driver — so a btrfs cartridge only opens on machines you
  have prepared.
- Neither benefit is as large here as it sounds. TRIM only reaches the drive if
  the USB bridge speaks UASP and passes UNMAP through, which many enclosures do
  not; and game data is already compressed, so zstd typically buys single-digit
  percentages in exchange for CPU on every read. A cartridge is also written
  once and read for years, which is the workload flash wear cares least about.

Pick btrfs if your cartridges live on Linux machines you control and you want
the filesystem's other properties. Otherwise exFAT.

The drive name follows the filesystem: exFAT allows 11 characters, btrfs has
room for the whole title. On Linux the relevant mount options are set by the
desktop environment or `/etc/fstab`.

</details>

<a id="performance"></a>
<details>
<summary><b>Getting the most out of a cartridge</b> — what actually causes stutter, in order</summary>
<br />

A game running from a cartridge is running over USB, and USB is the slowest
part of the machine. Most of what people blame on that is not actually the
drive, so this is in order: check the boring things first, then the storage.

### It is usually not the storage

**Shader compilation.** Modern DX12 and Vulkan games compile pipeline state
objects the first time each one is needed, and that stutter looks exactly like a
slow disk. The compiled cache lives on your internal drive, not the cartridge,
and it is keyed to GPU *and* driver version — so moving a cartridge to a second
PC, or updating your driver, throws it away and the first hour stutters again.

- NVIDIA Control Panel → Manage 3D settings → **Shader Cache Size → 10 GB** or
  Unlimited. The default is small enough that a big game evicts its own cache.
- On Steam, leave **Shader Pre-Caching** on. On Linux it is doing most of the
  work for you.

**VRAM.** A texture pool set above your VRAM budget spills over PCIe and hitches
in a way that reads as storage. Drop textures one notch and see whether the
stutter goes before touching anything else.

Neither of these gets better with a faster cartridge.

### The connection, which the launcher will tell you about

Press `I` on the launcher and it reports three things about the drive in front
of it:

<img width="420" alt="The launcher's details sheet showing link speed, transport and free space, with plain-language notes about what to do" src="docs/launcher-health.png" />

- **Link** — 10 Gbps is what a Gen 2 enclosure should negotiate. 5 Gbps means a
  front-panel port, a hub, or a cable that is not rated for it; 480 Mbps means
  USB 2.0, and games will stream badly.
- **Transport** — **UASP** queues commands; **BOT** sends one at a time. On the
  small random reads a game streams that is worth roughly two to three times as
  much, and which one you get depends on the enclosure's firmware and the port.
- **Space** — see below.

### Leave the drive some room

Almost every M.2 2230 drive is DRAM-less. On an internal slot that is fine: it
borrows host RAM over PCIe — the **Host Memory Buffer** — to hold its flash
translation table. **A USB bridge does not provide HMB**, so the translation
table is paged from the flash itself, and it gets worse the fuller the drive is.

Keep roughly **15% free** and the difference is measurable on random reads. The
wizard says so when a cartridge crosses 85%, and the launcher repeats it.

Free space only helps if the drive knows about it, which is what TRIM is for,
and nothing sends TRIM to removable media on a schedule. The wizard offers
**Release freed space back to the drive** when it is not formatting. Some
enclosures do not pass the command through at all — it will say so plainly if
yours is one of them, which is a good reason to keep the headroom anyway.

### Windows settings worth changing

The wizard's **Tune Windows for this cartridge** does the first two, per
cartridge, showing the exact commands first and offering to undo them:

- **Defender exclusion.** Real-time scanning walks a freshly copied 60 GB game
  the first time anything reads it, competing for the link you are trying to
  stream over.
- **Search indexing off** for the volume, for the same reason in the background.

The third is worth doing by hand, once per cartridge:

- **Device Manager → Disk drives → your cartridge → Policies → Better
  performance.** Windows sets removable drives to *Quick removal*, which turns
  write caching off entirely. *Better performance* is the right setting for a
  drive that is always ejected properly — which is what this launcher's Eject
  button is for. It is left as a manual step because the supported way to set it
  is that dialog; the registry keys behind it are per-device and undocumented,
  and this tool does not guess at those.

### Do not put a pagefile or swap on a cartridge

It comes up, and it is a bad idea three ways over. Windows will not page to a
disk it considers removable, and cannot page to exFAT at all. More importantly,
a failed read of a game asset is a retry, while a failed read of *swap* is a
bugcheck on Windows or a hard freeze on Linux — and a USB link that resets under
thermal load is a normal Tuesday. And it is backwards for stutter: swapping puts
*more* traffic on the slowest link in the machine.

If a game is short of RAM, add RAM. On Linux, `zram` gives you compressed swap
inside memory with no device involved.

### Install once, cleanly

exFAT's allocator is simple, so a game copied onto a freshly formatted cartridge
stays contiguous, and churning installs on and off it does not. Format, copy,
play. Sustained writes also heat the enclosure — that affects how long the copy
takes, not how the game runs.

</details>

<a id="hardware"></a>
<details>
<summary><b>Hardware</b> — 2230 NVMe, enclosures, and how fast that really is</summary>
<br />

Built around **M.2 2230 NVMe drives** — the short ones from Steam Decks and
Surface tablets — in compact aluminium USB enclosures.

| | |
|---|---|
| **Drives** | 128 GB M.2 2230 NVMe |
| **Enclosures** | ITGZ aluminium compact M.2 2230 case, USB 3.2 Gen 2 (10 Gbps), passive auto-cooling |
| **Filesystem** | exFAT by default, so a cartridge works in whatever machine it is plugged into. btrfs is offered for people who want TRIM and compression and do not mind [WinBtrfs](https://github.com/maharmstone/btrfs) on Windows. |

2230 is the right form factor for this: the drive plus enclosure is roughly the
size of a USB stick, so a shelf of ten cartridges takes almost no room. 128 GB
holds most single games, and the whole point of a cartridge is that it carries
one thing.

The enclosure is doing two jobs. It makes the cartridge pocketable, and it keeps
the wear away from the NVMe stick itself. A bare M.2 NVMe edge connector is
typically only rated for roughly **50–100 insertion cycles**; used as a raw
plug-in cartridge, the drive would become the sacrificial part. In a USB
enclosure, the NVMe drive is installed once and left alone, while the repeated
insertions happen on the cheaper, easier-to-replace USB side instead.

That trade-off does **not** mean giving up useful speed. 10 Gbps over USB 3.2
Gen 2 is around 1 GB/s in practice — already ahead of what a 2.5" SATA SSD can
deliver, and far beyond Switch-cartridge or SD-card territory. The aluminium
body doubles as the heatsink, which matters when a game is streaming assets off
it for hours.

| Medium | Practical read speed | What runs comfortably | Notes |
|---|---:|---|---|
| **USB 3.2 Gen 2 enclosure + 2230 NVMe** | **~800–1000 MB/s** | Indies, emulators, AA games, older AAA games, and many modern installs | The USB link is not the bottleneck here; drive quality and thermals usually matter more |
| **2.5" SATA SSD** | ~500–550 MB/s | Most PC games, including many large installs | Still slower than a 10 Gbps USB NVMe enclosure |
| **Nintendo Switch game card** | ~50–100 MB/s | Games built and optimised around console-style asset budgets | Much slower, but the software is designed for it |
| **UHS-I SD / microSD** | ~30–90 MB/s | Retro libraries, indies, lightweight PC games, emulators | Fine for small assets; weak for large modern PC installs |

So the practical answer is: the **adapter is the durability win**, and USB 3.2
is still fast enough that the cartridge remains a real play-from-media device
rather than just cold storage.

For this build, cheap refurbished bulk 2230 drives are a value play, not a
promise of flagship performance. They should be perfectly usable for indies,
retro, emulation, smaller AA releases and plenty of older AAA games, but the
newest asset-streaming-heavy PC blockbusters may still be happier on a strong
internal NVMe if a bargain cartridge drive cannot keep up.

Nothing here is specific to NVMe or to 2230. Any removable storage your OS will
automount works: 2.5" SATA SSDs in a dock, SD cards, USB sticks, external HDDs.
The form factor is a comfort choice, not a technical one.

> **Photos of the physical cartridges are not in the repository yet.** Drop them
> in `docs/` and link them here — the screenshots below are the software.

</details>

<a id="cartridge-format"></a>
<details>
<summary><b>Cartridge format</b> — the one file a cartridge needs, by hand</summary>
<br />

A cartridge is a text file and some art, so you can make one by hand. Copy
`cartridge.conf.example` to the root of the drive as `cartridge.conf`:

```ini
executable=steam://rungameid/1091500
title=Cyberpunk 2077
cover=cover.jpg
```

Portrait art at 3:4 fills the launcher window exactly. A finished cartridge:

```
CARTRIDGE/
├── cartridge.conf
├── cover.jpg
├── autorun.inf          drive name and icon in Explorer
├── Games/               a copied non-Steam game
│   └── Tunic/
│       └── TUNIC.exe
└── steamapps/           a copied Steam game
    ├── appmanifest_367520.acf
    └── common/Hollow Knight/
```

`executable=` takes any URI the OS can handle — `steam://`, `heroic://`, `gog://`,
`epic://`, `playnite://`, `lutris://`, `http://`, `https://` — or a path to a file
on the cartridge. See `cartridge.conf.example` for every key.

A classic `autorun.inf` is also read, for `label` and `icon` only. Its `open=`
and `shellexecute=` keys are deliberately ignored: Windows has ignored them on
non-optical media since Windows 7, and they are the oldest autorun malware vector
there is.

</details>

<a id="setup"></a>
<details>
<summary><b>Setup and install</b> — prerequisites, and the two shapes of Linux install</summary>
<br />

### Prerequisites

Rust (stable) and Node 18+, plus a C toolchain.

```bash
# Linux
sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev librsvg2-dev libssl-dev

# Windows: Visual Studio Build Tools, "Desktop development with C++"
```

### Build and install

```bash
git clone https://github.com/HarryBMa/pc-gamepak.git
cd pc-gamepak
cd tauri-ui && npm install && npm run build && cd ..
```

**Linux** — two shapes, and the menu asks which:

```bash
./gamepak-linux.sh          # → 1) Install            (recommended)
                            # → 2) Install without root
```

**1 — the system install.** A udev rule, two systemd template units, the helpers
and the launcher. udev is already running as part of the OS, so **nothing is
resident**: no process of ours exists until a cartridge is plugged in. Needs your
password once.

**2 — the rootless install.** Everything under `~/.local/bin`, plus a systemd
*user* service. No password, no udev rule, nothing written outside your home. In
exchange a small watcher stays running — about **2 MB, blocked in `poll()` on the
mount table**, no CPU while it waits.

Pick 1 unless you have a reason: zero is a better number than two megabytes. Pick
2 if you would rather not give a game launcher root, if you are on a machine
where you do not have it, or if you are installing from a sandboxed package,
which cannot write a udev rule at all.

They are not two codebases — the same launcher, the same detection rules, a
different trigger. The rootless one is arguably the more accurate of the two: it
wakes when the cartridge is *mounted and readable*, where udev fires when the
kernel first sees the partition and the helper then waits up to a minute for the
desktop to catch up.

```bash
# What the rootless install did, if you want to look:
systemctl --user status pc-gamepak-watcher.service
tail ~/.local/state/pc-gamepak/watcher.log
```

**Windows**

```powershell
cd watcher; cargo build --release; cd ..
# Right-click gamepak-windows.ps1 → Run with PowerShell → 1) Install
```

Installs the watcher and launcher to `%LOCALAPPDATA%\PC-GamePak` and
registers a logon task.

**Platforms:** Windows and Linux. macOS is not supported — there is no watcher,
no installer and no icon set for it, so rather than ship something half-working
the macOS branches were removed.

</details>

<a id="uninstall"></a>
<details>
<summary><b>Uninstall</b> — putting the machine back</summary>
<br />

Run the installer menu and choose Uninstall. It removes the udev rule and systemd
units on Linux, or the logon task and install folder on Windows.

</details>

<a id="how-it-works"></a>
<details>
<summary><b>How it works</b> — insertion to launcher, and what it costs while idle</summary>
<br />

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

**Nothing on a cartridge is executed automatically.** The launcher shows you what
it found and waits — pressing Play is the gate. That is the whole security model,
and it is why there is no trust list or allowlist to maintain.

### Idle cost

The point of a thing that waits all day for a drive is that it costs nothing
while it waits.

| | Idle |
|---|---|
| **Linux** | **Nothing resident** with the system install: udev is already part of the OS, and the rule adds no process. The rootless install trades that for one process of about 2 MB, blocked in `poll()` on the mount table. |
| **Windows** | **One process, ~2 MB, 0% CPU.** `pc-gamepak-watcher.exe` blocks on the Windows message queue — no polling, no timer. |

The launcher is a webview, so it is not small *while it is on screen* — expect
around 100 MB for the few seconds it is up, then it exits and gives all of it
back. There is no tray icon and no background service for the UI.

The watcher ignores a second arrival on the same drive letter within 4 seconds,
so a cartridge that is ejected and immediately re-inserted may require a brief
pause before the launcher reopens.

</details>

<a id="security"></a>
<details>
<summary><b>Security</b> — nothing runs without a click</summary>
<br />

Nothing on a cartridge runs without a click. That is the model. Earlier versions
of this idea auto-executed a `launch.sh` on insert, which needed a SHA-256
allowlist to be safe at all; removing the auto-execution removed the need for the
allowlist along with it.

- **Play runs what `cartridge.conf` says.** If `executable=` names a binary on the
  drive, Play runs that binary. On your own cartridges that is the feature. On a
  drive someone hands you, read the conf first — or keep to `steam://`-style URIs,
  where the argument goes to a program you already trust.
- **The launcher window cannot read your disk.** The webview has no filesystem
  access and no command that takes a path. The cover is read in Rust, from a path
  confined to the cartridge, and passed in as a `data:` URI.
- **Nothing is fetched**, with the optional SteamGridDB integration switched off.
  Fonts are bundled, the cover is inlined, and the content-security policy is
  `default-src 'self'`. The launcher never has a reason to reach the network at
  all; only the wizard does, and only once you have turned the lookup on and
  given it a key.
- **Titles are text, never markup.** They come off an untrusted volume and are
  inserted with `textContent`.
- **Eject asks twice** when the game lives on the cartridge, since pulling a drive
  a running game is reading from is a different mistake to pulling one that holds
  only a text file.

### Cartridges in Steam's library list

A cartridge you copied a Steam game onto is registered in `libraryfolders.vdf`,
labelled `PC GamePak`. Those entries are never removed automatically: a
cartridge is *meant* to spend most of its life unplugged, so a missing folder is
the normal state rather than stale cruft. When you reformat or repurpose one, the
wizard offers **Remove this drive from Steam's library list**. Steam must be
closed.

</details>

<a id="working-on-it"></a>
<details>
<summary><b>Working on it</b> — the crate split, the tests, the layout</summary>
<br />

The logic lives in `core/` (crate `gamepak-core`), deliberately free of any UI
dependency, so the tests run anywhere:

```bash
cargo test --manifest-path core/Cargo.toml
```

That split is the point: the Tauri binary cannot be compiled without webkit2gtk
and a display, so tests living inside it could not run in CI or on a
contributor's machine.

CI runs that suite plus clippy and rustfmt, compiles the watcher on Linux and
Windows, `cargo check`s the launcher on both, parses the frontend JavaScript, and
verifies every element the scripts reach for exists in the HTML — the UI ships
unbundled, so a missing id is a runtime crash rather than a build error.

```
gamepak-linux.sh            installer menu (Linux)
gamepak-windows.ps1         installer menu (Windows)
cartridge.conf.example      the one file a cartridge needs
core/                       cartridge logic, no UI — this is where the tests are
linux/                      udev rule, systemd units, the user service, helpers
watcher/                    volume watcher: WM_DEVICECHANGE on Windows, the
                            mount table on Linux (rootless install only)
tauri-ui/                   one binary, two windows (Tauri 2 + Rust, no framework)
  app/                      the HTML, CSS and JS, shipped unbundled
  src-tauri/                commands and window construction
packaging/                  AUR and Scoop manifests
tools/                      icon generation, DOM-id check
docs/                       screenshots, PUBLISHING.md, STATUS.md
```

[`docs/STATUS.md`](docs/STATUS.md) is the working inventory: what each module is
for, what is built, and what is missing.

When a cartridge does not open the launcher, the logs are the first place to
look: `%LOCALAPPDATA%\PC-GamePak\watcher.log` on Windows,
`~/.local/state/pc-gamepak/helper.log` on Linux.

</details>

<a id="packages"></a>
<details>
<summary><b>Packages</b> — where this will be published, and why not everywhere</summary>
<br />

Nothing is published yet. When it is, the shortlist is the AUR (which is where
the Steam Deck and Arch audience is), WinGet and Scoop on Windows — the channels
that can actually install the udev rule or the logon task this depends on.

[`docs/PUBLISHING.md`](docs/PUBLISHING.md) has the reasoning, including why
Flatpak, Snap and Homebrew are not on that list yet and what would change it.

</details>

<a id="thanks"></a>
<details>
<summary><b>Thanks</b> — the project this forked from, and a peer</summary>
<br />

This project began as a fork of
**[LewdM3at/PC-cartridge-system](https://github.com/LewdM3at/PC-cartridge-system)**,
which had the original idea and the first working implementation: the udev rule,
the systemd template unit and the Windows monitor that make insert-detection work
at all. The shape of the Linux side is still recognisably theirs.

That project is built around 2.5" SATA SSDs and has 3D-printable cartridge shells
on [MakerWorld](https://makerworld.com/en/models/3057977-2-5-ssd-dock-cartridge-system) — worth a look if you
want the full physical-cartridge build rather than a pocket enclosure.

This fork diverges in a few ways: 2230 NVMe rather than 2.5" SATA, a Tauri
launcher and a create-cartridge wizard instead of per-game shell scripts, and a
click-to-play model in place of the auto-execute-plus-allowlist one.

### Others working on the same idea

**[Uplinkpro/CartLaunchCompanion](https://github.com/Uplinkpro/CartLaunchCompanion)**
takes the opposite half of this problem, and takes it further than this project
does. It is a fullscreen, controller-first launcher — Avalonia and .NET, with
trailers and shelves — that lives **on the cartridge itself**, so a drive works
on a machine that has never been prepared. You lay the drive out yourself and its
configurator writes a `game.json` per game.

Where PC GamePak differs: the launcher is installed on the PC and the cartridge
carries only data, so a cartridge stays a text file and some art; and there is a
wizard that *makes* one — formatting, copying the game across, registering it as
a Steam library — which CartLaunchCompanion leaves to you. If you want a console
UI on a drive you assemble by hand, look there. Note its licence is PolyForm
Noncommercial, not MIT, so code cannot move between the two projects.


</details>

<a id="licence"></a>
<details>
<summary><b>Licence</b> — MIT</summary>
<br />

MIT, inherited from the upstream project. See [`LICENSE`](LICENSE) — the original
copyright notice is retained as the licence requires.

</details>

<a id="disclaimer"></a>
<details>
<summary><b>Disclaimer</b> — a hobby project</summary>
<br />

A hobby project, not affiliated with Valve, Steam, Playnite or ITGZ.

Auto-detection depends on your OS automounting removable drives. Some setups need
that configured before any of this works.

Use at your own risk.

</details>

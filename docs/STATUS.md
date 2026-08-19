# Where the project is

A working inventory: what exists, what it does, and what is missing. Kept in the
repository rather than in a chat log so it stays honest.

## What is built

### `core/` — `gamepak-core`, 137 tests

No Tauri, no UI, no display. That is the point: every decision the launcher and
the wizard make is testable on any machine, in CI, without a webview.

| Module | What it does |
|---|---|
| `cartridge` | Reads a cartridge: `cartridge.conf` (single game, or `[collection]` + `[game]` sections) and legacy `autorun.inf` for label and icon only. Inline INI parser, path confinement, cover inlined as a `data:` URI under an 8 MB cap. |
| `create` | The build pipeline: format → copy → check the launch target → cover art → `cartridge.conf` → `autorun.inf` → trim and report. Game lists from Playnite and Steam, collection naming, per-game covers. |
| `drives` | Which volumes may be written to — an allowlist of automount locations, never a denylist. Parses `/proc/mounts`; Win32 volume APIs on Windows. |
| `format` | exFAT and btrfs, behind four gates: removable allowlist re-derived here, not the system drive, the current label typed back exactly, and explicitly asked for. |
| `health` | Negotiated link speed, UASP vs BOT, how full the drive is. sysfs on Linux; the transport only, lazily, on Windows. |
| `playnite` | Reads a Playnite JSON library export: one list covering Steam, GOG, Epic, Xbox, itch, emulators. Finds Playnite on Windows and through Proton prefixes on Linux. |
| `portable` | Ranks the executables in a copied game folder so Play points at the game rather than its uninstaller. |
| `settings` | What the user has switched on, stored beside the artwork cache. Everything defaults to off. |
| `sgdb` | SteamGridDB artwork search, download and cache. Refuses every request until the user opts in and supplies a key. |
| `steam` | Steam's own manifests: `libraryfolders.vdf`, `appmanifest_*.acf`, the library cache for covers. Hand-written KeyValues parser. |
| `steamlib` | Copies a Steam game onto a cartridge and registers the drive as a Steam library, so Steam plays from the cartridge. |
| `trim` | Tells the drive which blocks it no longer has to keep. Treats "this enclosure will not" as a fact, not a failure. |
| `tuning` | The Windows settings worth changing per cartridge, the commands they run, and their exact opposites. |
| `autorun` | Writes `autorun.inf` so Explorer shows the game's name and icon; builds a PNG-in-ICO when the cover allows it. |

### `tauri-ui/` — one binary, two windows

`pc-gamepak --drive <path>` is the popup; `pc-gamepak --create` is the wizard.
Exactly one window is ever built, so neither mode costs anything for the other.
24 commands, no command that takes a path to read.

**Launcher** — the artwork fills a 420 × 560 window; title, Play, Eject. A
collection shows one Play per game with its own art, answering to `1`–`9`. The
accent colour is sampled from the cover. Details behind the gear, including the
connection health. Nothing on a cartridge runs without a click.

**Wizard** — search your library, pick one game or several, pick the drive,
choose what goes on it, Write. Formatting, copying, collections, artwork by file
picker or SteamGridDB, per-cartridge Windows tuning.

### `watcher/` — Windows only, 4 tests

A hidden top-level window blocking on `WM_DEVICECHANGE`. No polling, no timer,
about 2 MB resident. On Linux this binary is a stub that tells you to install the
udev rule instead.

### `linux/`, `windows/` — installers

udev rule plus two systemd template units on Linux; two binaries and a logon task
on Windows. Both installers uninstall cleanly, including names from before the
project was called PC GamePak.

### Everything else

CI on every push (core, watcher, launcher, frontend, shell), a release workflow
that builds both platforms from a tag, AUR and Scoop packaging, and
`docs/PUBLISHING.md` for what each channel can and cannot install.

## What is missing

Ranked by how much it matters.

1. **A tagged release.** Everything downstream — AUR, WinGet, Scoop — points at
   artefacts that do not exist yet. Nothing else on this list unblocks as much.
2. **Nobody has run this on real hardware.** Every path is unit-tested and the
   frontend is screenshotted, but no cartridge has been written by this code on a
   real drive. That is the next real milestone, not a feature.
3. **The rootless Linux install** (see below), which is also what makes Flatpak
   possible.
4. **Version numbers.** Three crates all saying `0.1.0`, moved by hand.
5. **A cartridge cannot be edited.** Changing a title or swapping the art means
   writing the whole cartridge again, or editing `cartridge.conf` by hand.
6. **No integrity check.** Nothing verifies that a copied game arrived intact.
   For 60 GB over USB that is worth having.
7. **Windows code signing.** Unsigned means SmartScreen on every download.
8. **macOS** is not supported at all — no watcher, no installer, no icons.

## The rootless Linux install

Today's Linux install needs root once, to place a udev rule and two systemd
units. In exchange nothing is resident: udev is already running, and the rule
adds no process.

The alternative is a small user-level watcher — the shape Flatpak, Snap and
Homebrew would all need. The obvious implementation, subscribing to udev, is the
wrong one: a sandbox has no `/run/udev`, and udev's netlink group is not
something a confined app should count on.

**Watching the mount table is better, and not just for the sandbox.** `poll()`
on `/proc/self/mountinfo` blocks until the mount table changes, costs nothing
while it waits, and fires exactly when a cartridge becomes *readable* — which is
the moment that matters. The current udev path fires when the kernel sees the
partition and then spends up to sixty seconds polling `findmnt`, waiting for the
automounter to catch up. The mount watcher has no such gap.

So the plan is two shapes of the same thing:

| Install | Trigger | Resident | Needs root |
|---|---|---|---|
| **System** (AUR, `.deb`, `install.sh`) | udev rule | nothing | yes, once |
| **Rootless** (Flatpak, or `--user`) | mount-table watcher, systemd user service | one process, ~2 MB | no |

Both run the same launcher and the same detection logic; only the trigger
differs. The system install stays the recommendation, because zero is a better
number than two megabytes, and the rootless one exists for people who would
rather not give a game launcher root — or who are installing from Flathub.

Unverified until it runs on real hardware: whether host mounts propagate into the
Flatpak sandbox promptly enough to be useful. bubblewrap makes the sandbox's
mounts slave to the host's, so they should — but "should" is not "does", and this
is exactly the kind of thing that is fine on one distribution and broken on
another.

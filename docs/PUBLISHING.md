# Publishing PC GamePak

## The constraint that decides everything

This is not a self-contained application. Installing it means installing
**system integration**:

| Platform | What goes where |
|---|---|
| Linux | a udev rule in `/usr/lib/udev/rules.d`, two systemd template units, three helpers in `/usr/local/bin` |
| Windows | two binaries under `%LOCALAPPDATA%`, plus a Task Scheduler logon task |

That is the whole feature. A cartridge opens its launcher **because** udev saw a
partition appear. Anything that cannot write those files, or cannot see the
device layer, ships a program that does nothing until the user installs the real
thing by hand.

So every channel below is judged on one question first: *can a package
installed this way actually watch for a drive?*

The wizard alone is a different matter — it is an ordinary desktop app and would
be fine in a sandbox. Splitting the two is possible, and worth considering later,
but shipping "PC GamePak" that cannot notice a cartridge would be a bait and
switch.

## Verdicts

### Yes — start here

| Channel | Why it fits | Effort |
|---|---|---|
| **GitHub Releases** | The source of truth every other channel points at. Tag `v0.1.0`, CI builds Linux and Windows artefacts with checksums. | Done — `.github/workflows/release.yml` |
| **AUR** (`pc-gamepak`, `pc-gamepak-git`) | pacman installs as root; udev rules and systemd units are ordinary here. Arch, CachyOS, Manjaro — and the Steam Deck crowd, who are the audience. | Low. `packaging/aur/PKGBUILD` is written |
| **WinGet** | Built into Windows 11. The installer script does the logon task; the manifest just delivers the files. | Low, once a release exists |
| **Scoop** | User-space, no admin, popular with the same people who own a drawer of NVMe drives. `packaging/scoop/pc-gamepak.json` is written. | Low |

### Later, and only with a reason

| Channel | Verdict |
|---|---|
| **`.deb` artefact** | Cheap and useful: `cargo-deb` produces one, attach it to the release, `sudo dpkg -i` installs the binary and the units. Do this before standing up an APT repository — a repo is weeks of maintenance for the same result. |
| **Chocolatey** | Fine technically; first submission goes through human moderation and the package needs a maintained `.nuspec`. Worth it only once Windows users actually ask. |
| **AppImage** | Tauri already builds one. It covers the *launcher*, not the watcher or the udev rule, so it is a convenience for people who want the wizard without installing anything — not a way to ship the product. |

### No, for now

| Channel | Why not |
|---|---|
| **Flatpak / Flathub** | The sandbox cannot install a udev rule or a system unit, and `flatpak-spawn --host` will not save you: writing to `/etc` is exactly what the sandbox exists to prevent. A Flatpak build would install a launcher that never launches. |
| **Snap** | Closer — snapd can ship daemons and has device interfaces — but it needs classic confinement for what this does, and classic confinement means a manual review queue for a project no reviewer has heard of. Reconsider if the rootless watcher below happens. |
| **Homebrew** | macOS is not a supported platform at all: no watcher, no installer, no icon set. On Linux, Homebrew installs into its own prefix and cannot place system units either. A tap would ship something that cannot work on the platform people would `brew install` it from. |
| **Native pacman repo** | Hosting a signed binary repository to serve what the AUR already serves from source. |

## The thing that would change these answers

Every "no" above traces back to the same root: the Linux side needs root to
install a udev rule.

It does not have to. A small daemon can subscribe to the kernel's uevent netlink
socket directly and see the same partition-arrival events udev sees, with no
rule file, no root, and no system unit — a **systemd user service** started at
login. That is roughly what the Windows watcher already does with
`WM_DEVICECHANGE`.

The cost is real and it is the thing this project has been careful about: Linux
would go from *nothing resident* to one process resident, on the order of the
2 MB the Windows watcher uses. In exchange, `flatpak install`, `snap install`
and `brew install` all become honest, and the Linux install stops needing a
password.

That is a product decision, not a packaging one, and it should be made
deliberately rather than because a package format asked for it.

## Order of work

1. **Tag `v0.1.0`.** Nothing below can start without artefacts to point at.
   Check `cargo build --release` on both platforms first — CI covers `check`,
   not `build`.
2. **AUR `pc-gamepak-git`** first: it builds from `main`, so it needs no
   checksums and no release cadence, and it puts the project in front of the
   Steam Deck audience immediately. Then the versioned `pc-gamepak`.
3. **Scoop**, in a personal bucket (`HarryBMa/scoop-bucket`). One JSON file, and
   `checkver`/`autoupdate` keep it current on their own.
4. **WinGet**, via `wingetcreate` for the first submission and the
   `winget-releaser` action thereafter.
5. **`.deb`** attached to releases via `cargo-deb`.
6. Revisit Flatpak and Snap **only** if the rootless watcher lands.

## Before the first tag

- **Version numbers.** All three crates say `0.1.0`. Decide whether they move
  together (simplest, and what the packaging assumes) and set them from the tag.
- **Code signing on Windows.** Unsigned binaries mean a SmartScreen warning on
  every download, and it does not go away until the certificate builds
  reputation. Azure Trusted Signing is the cheap path; self-signing achieves
  nothing here. Not a blocker, but decide before the first release rather than
  re-issuing artefacts later.
- **A `LICENSE` in every artefact.** The release workflow copies it; the AUR
  package installs it.
- **A changelog.** `--generate-notes` produces one from commits for the first
  release; a hand-written `CHANGELOG.md` earns its keep from the second.
- **The release is created as a draft.** Look at it, then publish — a tag is
  cheap to delete before anyone has downloaded it, and expensive afterwards.

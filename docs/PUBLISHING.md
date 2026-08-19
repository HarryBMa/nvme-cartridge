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
| **Flatpak / Flathub** | **Now plausible, and untested.** The blocker was the udev rule; the rootless watcher removes it. What is unverified is whether host mounts propagate into the sandbox promptly — bubblewrap makes the sandbox's mounts slave to the host's, so they should, but that needs checking on a real machine before a manifest is worth writing. The launcher also has to be able to start a game on the host, which means `--talk-name=org.freedesktop.Flatpak` or accepting that `steam://` goes through the portal. |
| **Snap** | Same unlock, same caveat, plus classic confinement's manual review queue. Behind Flatpak in the order. |
| **Homebrew** | macOS is not a supported platform at all: no watcher, no installer, no icon set. On Linux, Homebrew installs into its own prefix and cannot place system units either. A tap would ship something that cannot work on the platform people would `brew install` it from. |
| **Native pacman repo** | Hosting a signed binary repository to serve what the AUR already serves from source. |

## What changed these answers

Every "no" above used to trace back to one root: the Linux side needed root to
install a udev rule. It no longer does.

`linux/install-user.sh` installs everything under `$HOME` and runs the watcher as
a **systemd user service**. The watcher blocks in `poll()` on
`/proc/self/mountinfo` — not on udev, which a sandbox cannot reach — and wakes
when the mount table changes. About 2 MB resident, no CPU while it waits.

The system install stays the recommendation, because zero is a better number than
two megabytes. But the rootless one is what a package format can actually
install, which is what puts Flatpak back on the table.

Still unverified, and worth checking before writing a manifest: whether new host
mounts appear inside a Flatpak sandbox quickly enough to be useful, and how the
launcher hands a `steam://` URI back to the host.

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
6. **Flatpak**, once someone has confirmed on real hardware that mount events
   reach a sandboxed watcher and that `steam://` still launches from inside one.
   Snap after that, if ever.

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

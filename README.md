# Disclaimer

This project is a hobby experiment and is not an official Steam product.

Automatic launching depends on your operating system settings and security policies. Some systems may require additional configuration for automounting drives or allowing scripts to run automatically.

**Although there is a safe guard feature to auto-launch only trusted scripts, it still IS a security risk. Anyone with physical access to your PC *could* plug in a drive with a script on it, find the project folder and put their script to the trusted list. But at that point they could just execute their script already.<br>**

**Use this at your own risk**




# PC Cartridge System

<img width="970" height="546" alt="JTDUMcuDBav3BEspNBMw6A-970-80 jpg" src="https://github.com/user-attachments/assets/8c0a8d2b-ce5a-4aa5-9bac-5805016db31f" />

<br>
Physical game cartridges for your Steam library using 2.5" SATA SSDs.

Turn your digital Steam games into something that feels physical: insert a cartridge, and your PC automatically detects it and launches the configured game or action.

Each cartridge is a simple storage device containing a small launcher script. When inserted, the system detects the cartridge and executes the script file on the drive if it has been classified "trusted". 
Launching a Steam/GoG game, opening a game's details page, or running any custom commands.

## 3D-Print Files
STEP-Files are available over at MakerWorld: [MakerWorld](https://makerworld.com/en/models/3057977-2-5-ssd-dock-cartridge-system#profileId-3440827)

## Quickstart
### Linux

Clone the repository:

```bash
git clone https://github.com/LewdM3at/PC-Cartridge-System.git
```
Enter the project directory:
```bash
cd PC-Cartridge-System
```
Run the script:
```bash
./cartridge-linux.sh
```
<img width="811" height="425" alt="image" src="https://github.com/user-attachments/assets/9118395d-f977-48bb-bf3a-867d5a6143fd" />


**Installation**<br>
Select menu point 1) Install<br>
The installer will install the required udev rule, systemd service, and launcher helper.

**Trust Scripts / Check trust state**<br>
After you have created a Cartridge with the launch.sh script, add the script to trusted-scripts with menu point 2) Trust Scripts.
It will scan for any connected storage media for the launch.sh script and ask if you want to trust said script:
<img width="656" height="592" alt="image" src="https://github.com/user-attachments/assets/ec8772e4-8e48-40f4-878b-5741eac8cc05" />


You can also check the trust state of scripts here and have the option to stop trusting the scripts if they are already trusted.

Any script that hasn't been trusted through this process **will NOT be automatically executed**
! If you modify the script later on, you have to re-add it to trusted scripts again.<br>

**Auto-Launch Scripts**<br>
<img width="428" height="191" alt="image" src="https://github.com/user-attachments/assets/b233100c-f527-442b-87d6-c8746865450f" /><br>
You can toggle the automatic execution of scripts by choosing this menu point.<br>
For when you want to change something inside the script and don't want it to auto-launch when you insert the cartridge.<br>


**Uninstallation:**<br>
Select menu point 4) Uninstall <br>
This will remove everything including the config files and trust scripts list.

### Windows

Download the repo:
1. Click Code → Download ZIP OR download it from [Releases](https://github.com/LewdM3at/PC-Cartridge-System/releases)
2. Extract it
3. Right click on cartridge-windows.ps1 -> Run with Powershell

<img width="807" height="329" alt="image" src="https://github.com/user-attachments/assets/88e48354-0e2a-4d78-8e93-81e306e84617" />
<br>

**Installation**<br>
Select menu point 1) Install<br>
The installer will install the required udev rule, systemd service, and launcher helper.

**Trust Scripts**<br>
After you have created a Cartridge with the launch.sh script, add the script to trusted-scripts with menu point 2) Trust Scripts.
It will scan for any connected storage media for the launch.sh script and ask if you want to trust said script.
<img width="656" height="446" alt="image" src="https://github.com/user-attachments/assets/a1c71905-2e77-4ec0-99da-1a8e78f65035" />

You can also check the trust state of scripts here and have the option to stop trusting the scripts if they are already trusted.
Any script that hasn't been trusted through this process **will NOT be automatically executed**
! If you modify the script later on, you have to re-add it to trusted scripts again.<br>

**Auto-Launch Scripts**<br>
<img width="442" height="157" alt="image" src="https://github.com/user-attachments/assets/51109e77-a2e0-45da-be9a-3b621d3c7fd1" />

You can toggle the automatic execution of scripts by choosing this menu point.<br>
For when you want to change something inside the script and don't want it to auto-launch when you insert the cartridge.<br>

**Uninstallation:**<br>
Select menu point 4) Uninstall <br>
This will remove everything including the config files and trust scripts list.


## Supported Storage

The project is designed around **2.5" SATA SSDs**.

However, the same concept may work with other storage devices such as:

- SD cards
- USB flash drives
- External HDDs
- Other removable storage

Compatibility with other storage types is **not guaranteed** and depends on your operating system, filesystem, automount configuration, and hardware.

## How It Works

Each cartridge contains a launcher script (launch.sh/launch.ps1) that will be executed by the helpers (depending on OS).
Configure these scripts to whatever you need with Steam URL Protocol or any custom commands.

### Linux

The Linux version uses three components:

- **udev rule**<br>
The udev rule detects when a new storage partition is connected.<br>
Its only job is to notify systemd that a game cartridge may have been inserted.<br>
It does not execute the cartridge directly.<br>

- **systemd service**<br>
A systemd template service is used to handle cartridge launches.<br>
The template allows the same service to work with any inserted device.<br>
The service starts the launcher helper and passes the detected device name.<br>

- **cartridge-launcher-helper**<br>
The helper script waits for the desktop environment to mount the drive, then searches the cartridge at rool level for: `launch.sh` <br>
If found, it checks the SHA256 sums of said script against the stored trusted-scripts file. <br>
If the SHA256 matches, it executes the script.<br>
Example cartridge:<br>
SSD<br>
└── launch.sh<br>
└── SteamLibrary<br>
The content of `launch.sh` decide what happens next.

---

#### Windows

The Windows version uses two components:


- **Task Scheduler**<br>
The installer creates a scheduled task that starts the cartridge monitor when the user logs in.<br>
The task keeps the monitor running silently in the background.<br>
- **cartridge-monitoring.ps1**<br>
The PowerShell script monitors for newly inserted storage devices.<br>
When a new drive is detected, it checks the root of the cartridge for: `launch.ps1` <br>
If found, it checks the SHA256 sums of said script against the stored trusted-scripts file. <br>
If the SHA256 matches, it executes the script.<br>
Example cartridge:<br>
SSD<br>
└── launch.ps1<br>
└── SteamLibrary<br>
The content of `launch.ps1` decide what happens next.

---

## Advanced Features

### cartridge.conf — URI and executable launcher

Instead of hard-coding the game command in `launch.ps1` / `launch.sh`, you can place a
`cartridge.conf` file at the root of the cartridge and use the URI-aware template scripts
from `example-scripts/Windows/URI-Launch/` or `example-scripts/Linux/URI-Launch/`.

`cartridge.conf` format (plain text):

```
executable=steam://rungameid/1091500
title=My Game
cover=cover.png
```

The `executable` value can be:
- A **URI** for any protocol handler registered on the OS (`steam://`, `heroic://`, `gog://`, `epic://`, `lutris://`, etc.)
- A **relative path** to a file on the cartridge (e.g. `Game\bin\game.exe` on Windows or `Game/bin/start.sh` on Linux)

A full annotated example is at `example-scripts/cartridge.conf.example`.

#### How URI detection works

**Windows (`launch.ps1`):** if `executable` starts with a known scheme, `Start-Process` is
called with the URI string directly — Windows ShellExecute routes it to the registered
protocol handler automatically.

**Linux (`launch.sh`):** if `executable` starts with a known scheme, `xdg-open` is called —
it routes the URI to the default handler registered with the desktop environment.

---

### Safe Eject

#### Windows

Use menu option **4) Eject cartridge** in `cartridge-windows.ps1` to safely flush the write
cache and dismount a drive before pulling it out.

Alternatively run `windows\eject.ps1` directly:

```powershell
.\windows\eject.ps1 -DriveLetter D
```

The script first tries `Win32_Volume.Dismount()` and falls back to `mountvol /P` if that
fails.

#### Linux

After a cartridge's `launch.sh` script exits, the launcher helper automatically unmounts
and powers off the drive via `udisksctl`.

For manual ejection, use the installed helper:

```bash
pc-cartridge-eject sdb
```

Or run the script directly:

```bash
sudo linux/eject.sh sdb
```

---

### Auto-close on cartridge removal

#### Windows

When a drive is physically removed (or ejected), the cartridge monitor (`cartridge-monitoring.ps1`)
detects the `DeviceRemoved` event and terminates the process that was launched for that
drive. The process receives `CloseMainWindow()` first (allowing a graceful shutdown) and
`Stop-Process -Force` after a 2-second timeout if it has not yet exited.

#### Linux

The `BindsTo=dev-%i.device` directive in `game-cartridge@.service` means systemd
automatically stops the service when the device disappears. In addition, the udev
`ACTION=="remove"` rule fires `game-cartridge-remove@.service`, which sends `SIGTERM`
(and then `SIGKILL` if needed) to any running `launch.sh` processes.

---

### Tauri frontends — loading images from the cartridge drive

Tauri's default security policy blocks access to files outside the app bundle. Two
patterns for displaying cartridge images (e.g. cover art) are documented and demonstrated
in `example-scripts/Tauri-Frontend/tauri-asset-loading.md`:

- **Pattern A** — add the drive root to `assetScope` in `tauri.conf.json` and use
  `convertFileSrc()` in the frontend.
- **Pattern B** — read image bytes in a Rust `#[tauri::command]` and return a base64
  data URI (no `tauri.conf.json` change required; works with any dynamic drive letter).

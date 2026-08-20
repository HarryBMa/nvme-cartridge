# PC Cartridge System installer (Windows)
#
# Installs two native binaries and a logon task:
#
#   pc-cartridge-watcher.exe   resident, waits for a volume to arrive
#   pc-cartridge-launcher.exe  the popup, started by the watcher on insert
#
# The watcher replaces the old resident PowerShell monitor. PowerShell kept the
# .NET runtime and a WMI subscription alive for the whole session to do this job;
# the watcher blocks on the Windows message queue instead, so it uses a couple of
# megabytes and no CPU while it waits.

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "Installing PC Cartridge System..."
Write-Host ""

########################################
# Paths
########################################

$InstallFolder = Join-Path $env:LOCALAPPDATA "PC-Cartridge-System"
$RepoRoot      = Split-Path -Parent $PSScriptRoot

$WatcherSource  = Join-Path $RepoRoot "watcher\target\release\pc-cartridge-watcher.exe"
$LauncherSource = Join-Path $RepoRoot "tauri-ui\src-tauri\target\release\pc-cartridge-launcher.exe"

$WatcherTarget  = Join-Path $InstallFolder "pc-cartridge-watcher.exe"
$LauncherTarget = Join-Path $InstallFolder "pc-cartridge-launcher.exe"

$TaskName = "PC Cartridge System Watcher"

########################################
# Check the binaries have been built
########################################

$Missing = @()
if (-not (Test-Path $WatcherSource))  { $Missing += $WatcherSource }
if (-not (Test-Path $LauncherSource)) { $Missing += $LauncherSource }

if ($Missing.Count -gt 0) {
    Write-Host "Build the binaries first:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  cd watcher"
    Write-Host "  cargo build --release"
    Write-Host ""
    Write-Host "  cd ..\tauri-ui"
    Write-Host "  npm install"
    Write-Host "  npm run build"
    Write-Host ""
    Write-Host "Missing:" -ForegroundColor Yellow
    $Missing | ForEach-Object { Write-Host "  $_" }
    exit 1
}

########################################
# Install
########################################

Write-Host "Creating install directory..."
New-Item -ItemType Directory -Path $InstallFolder -Force | Out-Null

# Stop a running watcher before overwriting it.
Get-Process -Name "pc-cartridge-watcher" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 400

Write-Host "Installing watcher and launcher..."
Copy-Item -Path $WatcherSource  -Destination $WatcherTarget  -Force
Copy-Item -Path $LauncherSource -Destination $LauncherTarget -Force

########################################
# Logon task
########################################

# Remove tasks from previous versions, including the PowerShell monitor.
$OldTaskNames = @(
    "PC Cartridge System Watcher",
    "PC Cartridge System Monitor",
    "Steam Game Cartridge Monitor"
)

foreach ($Old in $OldTaskNames) {
    $Existing = Get-ScheduledTask -TaskName $Old -ErrorAction SilentlyContinue
    if ($null -ne $Existing) {
        Write-Host "Removing previous task: $Old"
        Stop-ScheduledTask -TaskName $Old -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 500
        Unregister-ScheduledTask -TaskName $Old -Confirm:$false
    }
}

$Action  = New-ScheduledTaskAction -Execute $WatcherTarget
$Trigger = New-ScheduledTaskTrigger -AtLogOn

# No execution time limit: this is meant to stay running for the session.
$Settings = New-ScheduledTaskSettingsSet `
    -ExecutionTimeLimit (New-TimeSpan -Seconds 0) `
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 1) `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries

Register-ScheduledTask `
    -TaskName $TaskName `
    -Action $Action `
    -Trigger $Trigger `
    -Settings $Settings `
    -Description "Opens the cartridge launcher when a cartridge is plugged in" | Out-Null

Write-Host "Starting watcher..."
Start-ScheduledTask -TaskName $TaskName

########################################
# Done
########################################

Write-Host ""
Write-Host "=========================================="
Write-Host " PC Cartridge System installed"
Write-Host "=========================================="
Write-Host ""
Write-Host " Put a cartridge.conf at the root of the drive:"
Write-Host ""
Write-Host "   executable=steam://rungameid/1091500"
Write-Host "   title=Cyberpunk 2077"
Write-Host "   cover=cover.jpg"
Write-Host ""
Write-Host " Then plug the cartridge in. Nothing runs on its own:"
Write-Host " the launcher opens and waits for you to press Play."
Write-Host ""
Write-Host " Installed to: $InstallFolder"
Write-Host ""
PAUSE

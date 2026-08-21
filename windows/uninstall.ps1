# PC GamePak Uninstaller

$ErrorActionPreference = "Stop"


Write-Host ""
Write-Host "Uninstalling PC GamePak..."
Write-Host ""


########################################
# Paths
########################################

$InstallFolders = @(
    (Join-Path $env:LOCALAPPDATA "SteamGameCartridge")
    (Join-Path $env:LOCALAPPDATA "PC-GamePak")
)

$TaskNames = @(
    "Steam Game Cartridge Monitor",
    "PC GamePak Monitor",
    "PC GamePak Watcher"
)

$AutoplayHandlerKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers\Handlers\PCGamePakCartridge"
$AutoplayEventKey   = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers\Events\StorageOnArrival\Handlers\PCGamePakCartridge"


########################################
# Remove scheduled tasks
########################################

Write-Host "Removing scheduled tasks..."

foreach ($TaskName in $TaskNames) {
    $Task = Get-ScheduledTask `
        -TaskName $TaskName `
        -ErrorAction SilentlyContinue

    if ($null -ne $Task) {
        Write-Host "Stopping monitor: $TaskName"

        Stop-ScheduledTask `
            -TaskName $TaskName `
            -ErrorAction SilentlyContinue

        Unregister-ScheduledTask `
            -TaskName $TaskName `
            -Confirm:$false
    }
    else {
        Write-Host "Scheduled task not found: $TaskName"
    }
}


########################################
# Remove AutoPlay experiment
########################################

Write-Host "Removing experimental AutoPlay handler..."

if (Test-Path $AutoplayEventKey) {
    Remove-Item -Path $AutoplayEventKey -Recurse -Force -ErrorAction SilentlyContinue
}

if (Test-Path $AutoplayHandlerKey) {
    Remove-Item -Path $AutoplayHandlerKey -Recurse -Force -ErrorAction SilentlyContinue
}


########################################
# Remove installed files
########################################

Write-Host "Stopping the watcher..."

Get-Process -Name "pc-gamepak-watcher" -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 400

Write-Host "Removing installed files..."

foreach ($InstallFolder in $InstallFolders) {
    if (Test-Path $InstallFolder) {
        Write-Host "Removing $InstallFolder"

        Remove-Item `
            -Path $InstallFolder `
            -Recurse `
            -Force
    }
    else {
        Write-Host "Install directory not found: $InstallFolder"
    }
}


########################################
# Done
########################################

Write-Host ""
Write-Host "=========================================="
Write-Host " PC GamePak removed"
Write-Host "=========================================="
Write-Host ""
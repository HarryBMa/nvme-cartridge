# PC Cartridge System Uninstaller

$ErrorActionPreference = "Stop"


Write-Host ""
Write-Host "Uninstalling PC Cartridge System..."
Write-Host ""


########################################
# Paths
########################################

$InstallFolders = @(
    (Join-Path $env:LOCALAPPDATA "SteamGameCartridge")
    (Join-Path $env:LOCALAPPDATA "PC-Cartridge-System")
)

$TaskNames = @(
    "Steam Game Cartridge Monitor",
    "PC Cartridge System Monitor",
    "PC Cartridge System Watcher"
)


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
# Remove installed files
########################################

Write-Host "Stopping the watcher..."

Get-Process -Name "pc-cartridge-watcher" -ErrorAction SilentlyContinue |
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
Write-Host " PC Cartridge System removed"
Write-Host "=========================================="
Write-Host ""
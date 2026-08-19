# Installer menu.
#
# There is no trust list and no auto-launch toggle any more: inserting a
# cartridge opens the launcher, and nothing runs until Play is pressed, so there
# is nothing to allowlist or switch off.

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

function Show-Menu {
    Clear-Host

    Write-Host ""
    Write-Host @"
    ██████╗ █████╗ ██████╗ ████████╗██████╗ ██╗██████╗  ██████╗ ███████╗███████╗
   ██╔════╝██╔══██╗██╔══██╗╚══██╔══╝██╔══██╗██║██╔══██╗██╔════╝ ██╔════╝██╔════╝
   ██║     ███████║██████╔╝   ██║   ██████╔╝██║██║  ██║██║  ███╗█████╗  ███████╗
   ██║     ██╔══██║██╔══██╗   ██║   ██╔══██╗██║██║  ██║██║   ██║██╔══╝  ╚════██║
   ╚██████╗██║  ██║██║  ██║   ██║   ██║  ██║██║██████╔╝╚██████╔╝███████╗███████║
    ╚═════╝╚═╝  ╚═╝╚═╝  ╚═╝   ╚═╝   ╚═╝  ╚═╝╚═╝╚═════╝  ╚═════╝ ╚══════╝╚══════╝
"@ -ForegroundColor Cyan

    Write-Host ""
    Write-Host "        ╭────────────────────────────────╮"
    Write-Host "        │   1) Install                   │"
    Write-Host "        │   2) Create a cartridge        │"
    Write-Host "        │   3) Eject cartridge           │"
    Write-Host "        │   4) Uninstall                 │"
    Write-Host "        │   5) Exit                      │"
    Write-Host "        ╰────────────────────────────────╯"
    Write-Host ""
}

while ($true) {
    Show-Menu
    $Option = Read-Host "     Select option"

    switch ($Option) {
        "1" {
            Clear-Host
            Write-Host "Starting installation..."
            Start-Process `
                powershell.exe `
                -Verb RunAs `
                -ArgumentList "-ExecutionPolicy Bypass -File `"$ScriptDir\windows\install.ps1`"" `
                -Wait
        }

        "2" {
            Clear-Host

            # Prefer a local build so the wizard works before installing.
            $Built = Join-Path $ScriptDir "tauri-ui\src-tauri\target\release\pc-gamepak.exe"
            $Installed = Join-Path $env:LOCALAPPDATA "PC-GamePak\pc-gamepak.exe"

            $Launcher = if (Test-Path $Built) { $Built } elseif (Test-Path $Installed) { $Installed } else { $null }

            if ($null -eq $Launcher) {
                Write-Host "The launcher has not been built yet:" -ForegroundColor Yellow
                Write-Host ""
                Write-Host "  cd tauri-ui"
                Write-Host "  npm install"
                Write-Host "  npm run build"
                Write-Host ""
                PAUSE
            }
            else {
                Write-Host "Opening the cartridge wizard..."
                Start-Process -FilePath $Launcher -ArgumentList "--create" -Wait
                Clear-Host
            }
        }

        "3" {
            Clear-Host
            Write-Host "Eject cartridge..."
            & "$ScriptDir\windows\eject.ps1"
            Clear-Host
        }

        "4" {
            Clear-Host
            Write-Host "Starting uninstall..."
            Start-Process `
                powershell.exe `
                -Verb RunAs `
                -ArgumentList "-ExecutionPolicy Bypass -File `"$ScriptDir\windows\uninstall.ps1`"" `
                -Wait
        }

        "5" {
            Clear-Host
            exit
        }

        default {
            Write-Host "Invalid option."
            Start-Sleep -Seconds 1
        }
    }
}

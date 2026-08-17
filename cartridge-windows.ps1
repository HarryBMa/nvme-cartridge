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
    Write-Host "        │   2) Eject cartridge           │"
    Write-Host "        │   3) Uninstall                 │"
    Write-Host "        │   4) Exit                      │"
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
            Write-Host "Eject cartridge..."
            & "$ScriptDir\windows\eject.ps1"
            Clear-Host
        }

        "3" {
            Clear-Host
            Write-Host "Starting uninstall..."
            Start-Process `
                powershell.exe `
                -Verb RunAs `
                -ArgumentList "-ExecutionPolicy Bypass -File `"$ScriptDir\windows\uninstall.ps1`"" `
                -Wait
        }

        "4" {
            Clear-Host
            exit
        }

        default {
            Write-Host "Invalid option."
            Start-Sleep -Seconds 1
        }
    }
}

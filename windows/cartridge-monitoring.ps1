# PC Cartridge System Monitor
# Watches for inserted drives and launches trusted launch.ps1 cartridges.
# Also handles safe auto-close when a cartridge is removed (EventType 3).

$InstallFolder = Join-Path $env:LOCALAPPDATA "PC-Cartridge-System"

$LogFile = Join-Path $InstallFolder "monitor.log"
$TrustFile = Join-Path $InstallFolder "trusted_scripts.sha256"

$ConfigFile = Join-Path $InstallFolder "settings.conf"

$DebounceSeconds = 5


if (-not (Test-Path $InstallFolder)) {
    New-Item -ItemType Directory -Path $InstallFolder -Force | Out-Null
}

if (-not (Test-Path $TrustFile)) {
    New-Item -ItemType File -Path $TrustFile -Force | Out-Null
}

if (-not (Test-Path $ConfigFile)) {
    "MODE=running" | Out-File $ConfigFile -Encoding UTF8
}


# Maps drive letter -> last event time (debounce)
$LastLaunches = @{}

# Maps drive letter -> PID of the launched cartridge process (for auto-close on removal)
$LaunchedPIDs = @{}


function Write-Log {
    param(
        [string]$Message
    )

    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"

    Add-Content `
        -Path $LogFile `
        -Value "[$timestamp] $Message"
}


function Get-FileHashSHA256 {
    param(
        [string]$Path
    )

    return (Get-FileHash `
        -Path $Path `
        -Algorithm SHA256).Hash.ToLower()
}


function Is-TrustedScript {
    param(
        [string]$Hash
    )

    if (-not (Test-Path $TrustFile)) {
        return $false
    }

    $TrustedHashes = Get-Content $TrustFile

    return $TrustedHashes -contains $Hash
}

function Get-Mode {

    if (-not (Test-Path $ConfigFile)) {
        return "stopped"
    }


    $ModeLine = Get-Content $ConfigFile |
        Where-Object { $_ -like "MODE=*" }


    if ($ModeLine) {

        $Mode = $ModeLine.Split("=")[1]

        return $Mode.ToLower()

    }


    return "stopped"
}


Write-Log "PC Cartridge System monitor started."


$null = Register-WmiEvent `
    -Class Win32_VolumeChangeEvent `
    -SourceIdentifier "PCCartridgeSystem" `
    -Action {

        # Inline log helper — uses MessageData.LogFile to avoid cross-runspace scope issues
        function Write-ActionLog {
            param([string]$Message)
            $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
            Add-Content -Path $Event.MessageData.LogFile -Value "[$ts] $Message"
        }

        $eventType = $Event.SourceEventArgs.NewEvent.EventType

        $drive = $Event.SourceEventArgs.NewEvent.DriveName

        if ([string]::IsNullOrWhiteSpace($drive)) {
            return
        }


        # ---- 3 = drive removed ----
        if ($eventType -eq 3) {

            Write-ActionLog "Cartridge removed: $drive"

            # Stop the process that was launched for this drive, if any
            $pidTable = $Event.MessageData.PIDs

            if ($pidTable.ContainsKey($drive)) {

                $cartPid = $pidTable[$drive]
                $pidTable.Remove($drive)

                try {

                    $proc = Get-Process -Id $cartPid -ErrorAction SilentlyContinue

                    if ($null -ne $proc) {
                        $proc.CloseMainWindow() | Out-Null
                        Start-Sleep -Seconds 2
                        if (-not $proc.HasExited) {
                            Stop-Process -Id $cartPid -Force -ErrorAction SilentlyContinue
                        }
                        Write-ActionLog "Closed cartridge process PID $cartPid for $drive"
                    }

                }
                catch {
                    Write-ActionLog "Could not stop process PID $cartPid : $($_.Exception.Message)"
                }

            }

            return
        }


        # ---- 2 = drive inserted ----
        if ($eventType -ne 2) {
            return
        }


        $launcher = Join-Path $drive "launch.ps1"


        if (-not (Test-Path $launcher)) {
            return
        }


        # Debounce
        $now = Get-Date
        $debounce = $Event.MessageData.Debounce
        $launches = $Event.MessageData.Launches


        if ($launches.ContainsKey($drive)) {

            $elapsed = ($now - $launches[$drive]).TotalSeconds

            if ($elapsed -lt $debounce) {

                Write-ActionLog "Ignoring duplicate event for $drive"

                return
            }
        }


        $launches[$drive] = $now


        Write-ActionLog "Cartridge detected: $drive"

        # Read mode from config file
        $configPath = $Event.MessageData.ConfigFile
        $modeLine = Get-Content $configPath -ErrorAction SilentlyContinue |
            Where-Object { $_ -like "MODE=*" }
        $mode = if ($modeLine) { $modeLine.Split("=")[1].ToLower() } else { "stopped" }


        if ($mode -ne "running") {

            Write-ActionLog "Cartridge execution disabled. Current mode: $mode"

            return
        }


        try {

            $hash = (Get-FileHash -Path $launcher -Algorithm SHA256).Hash.ToLower()

            Write-ActionLog "SHA256: $hash"

            $trustFile = $Event.MessageData.TrustFile
            $trusted = (Get-Content $trustFile -ErrorAction SilentlyContinue) -contains $hash

            if (-not $trusted) {

                Write-ActionLog "Blocked untrusted cartridge."

                return
            }


            Write-ActionLog "Trusted cartridge."


            $proc = Start-Process `
                -FilePath "powershell.exe" `
                -ArgumentList @(
                    "-NoProfile",
                    "-ExecutionPolicy", "Bypass",
                    "-WindowStyle", "Hidden",
                    "-File", "`"$launcher`""
                ) `
                -WindowStyle Hidden `
                -PassThru


            # Store PID so we can close the process on removal
            $Event.MessageData.PIDs[$drive] = $proc.Id

            Write-ActionLog "Launched: $launcher (PID $($proc.Id))"

        }

        catch {

            Write-ActionLog "Failed launching $launcher"
            Write-ActionLog $_.Exception.Message

        }

    } `
    -MessageData ([PSCustomObject]@{
        LogFile    = $LogFile
        PIDs       = $LaunchedPIDs
        Launches   = $LastLaunches
        Debounce   = $DebounceSeconds
        ConfigFile = $ConfigFile
        TrustFile  = $TrustFile
    })


Write-Log "Event watcher registered."


try {

    while ($true) {

        Wait-Event | Out-Null

    }

}

finally {

    Unregister-Event `
        -SourceIdentifier "PCCartridgeSystem" `
        -ErrorAction SilentlyContinue

    Remove-Job `
        -Name "PCCartridgeSystem" `
        -Force `
        -ErrorAction SilentlyContinue

    Write-Log "Monitor stopped."

}
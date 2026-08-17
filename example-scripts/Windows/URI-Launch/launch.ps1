# PC Cartridge System - URI-Aware Launcher (Windows)
# Put this script on your SSD at root level and name it "launch.ps1" (without quotes).
#
# Reads a cartridge.conf file from the same directory.
# cartridge.conf can contain either:
#   - A URI like steam://rungameid/1091500  (opened via the system protocol handler)
#   - A relative path like Game\bin\game.exe  (run directly from the cartridge)
#
# cartridge.conf format (plain text, one value per key):
#   executable=steam://rungameid/1091500
#   title=My Awesome Game

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfigPath = Join-Path $ScriptDir "cartridge.conf"
$LogPath = Join-Path $ScriptDir "launch.log"

function Write-Log {
    param([string]$Message)
    $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Add-Content -Path $LogPath -Value "[$ts] $Message"
}

Write-Log "Cartridge launch started."

if (-not (Test-Path $ConfigPath)) {
    Write-Log "cartridge.conf not found. Exiting."
    exit 1
}

# Parse cartridge.conf
$config = @{}
Get-Content $ConfigPath | ForEach-Object {
    if ($_ -match '^\s*([^=]+?)\s*=\s*(.+)$') {
        $config[$Matches[1].Trim().ToLower()] = $Matches[2].Trim()
    }
}

$executable = $config["executable"]

if ([string]::IsNullOrWhiteSpace($executable)) {
    Write-Log "No 'executable' key found in cartridge.conf. Exiting."
    exit 1
}

Write-Log "Executable: $executable"

# Detect whether the value is a URI (contains a scheme like steam://, heroic://, etc.)
# or a local file path on the cartridge.
$knownUriSchemes = @("steam://", "heroic://", "gog://", "epic://", "playnite://", "lutris://", "http://", "https://")
$isUri = $knownUriSchemes | Where-Object { $executable.ToLower().StartsWith($_) }

if ($isUri) {
    # Open through Windows shell — routes to the registered protocol handler.
    Write-Log "Detected URI. Opening via shell: $executable"
    Start-Process $executable
}
else {
    # Treat as a relative path to an executable on the cartridge.
    $fullPath = Join-Path $ScriptDir $executable

    if (-not (Test-Path $fullPath)) {
        Write-Log "Executable not found on cartridge: $fullPath"
        exit 1
    }

    Write-Log "Launching local executable: $fullPath"
    Start-Process -FilePath $fullPath -WorkingDirectory (Split-Path $fullPath -Parent)
}

Write-Log "Launch completed."

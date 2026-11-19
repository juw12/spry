# Installs Spry for the current user on Windows. No administrator rights needed.
#   irm https://raw.githubusercontent.com/juw12/spry/main/install.ps1 | iex
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$dir = Join-Path $env:LOCALAPPDATA "Programs\Spry"
$zip = Join-Path $env:TEMP "spry-windows-x64.zip"

$from = if ($env:SPRY_FROM) { $env:SPRY_FROM } else { "https://github.com/juw12/spry/releases/latest/download" }
Write-Host "Downloading Spry ..."
Invoke-WebRequest "$from/spry-windows-x64.zip" -OutFile $zip -UseBasicParsing
Invoke-WebRequest "$from/spry-windows-x64.zip.sha256" -OutFile "$zip.sha256" -UseBasicParsing

# The download must match the checksum published next to it, or nothing is installed.
$want = ((Get-Content "$zip.sha256" -Raw).Trim() -split "\s+")[0].ToLower()
$got = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
Remove-Item "$zip.sha256"
if (-not $want -or $want -ne $got) {
    Remove-Item $zip
    throw "The download does not match its published checksum. Nothing was installed."
}
Write-Host "Checksum matches."

# Stop a running copy first, so the file can be replaced and every app is put back.
$running = Get-Process spry -ErrorAction SilentlyContinue
if ($running) {
    $running | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Seconds 1
    Get-Process spry -ErrorAction SilentlyContinue | Stop-Process -Force
    & (Join-Path $dir "spry.exe") rescue | Out-Null
}

New-Item -ItemType Directory -Force $dir | Out-Null
Expand-Archive $zip -DestinationPath $dir -Force
Remove-Item $zip

$exe = Join-Path $dir "spry.exe"
Write-Host "Spry is in $exe"
Write-Host "Starting it now. To start it at every sign-in, tick the box on its page."
Start-Process $exe

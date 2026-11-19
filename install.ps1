# Installs Spry for the current user on Windows. No administrator rights needed.
#   irm https://raw.githubusercontent.com/juw12/spry/main/install.ps1 | iex
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$dir = Join-Path $env:LOCALAPPDATA "Programs\Spry"
$zip = Join-Path $env:TEMP "spry-windows-x64.zip"
New-Item -ItemType Directory -Force $dir | Out-Null

Write-Host "Downloading Spry ..."
Invoke-WebRequest "https://github.com/juw12/spry/releases/latest/download/spry-windows-x64.zip" -OutFile $zip

# Stop a running copy first, so the file can be replaced and every app is put back.
$running = Get-Process spry -ErrorAction SilentlyContinue
if ($running) {
    $running | ForEach-Object { $_.CloseMainWindow() | Out-Null }
    Start-Sleep -Seconds 1
    Get-Process spry -ErrorAction SilentlyContinue | Stop-Process -Force
    & (Join-Path $dir "spry.exe") rescue | Out-Null
}

Expand-Archive $zip -DestinationPath $dir -Force
Remove-Item $zip

$exe = Join-Path $dir "spry.exe"
Write-Host "Spry is in $exe"
Write-Host "Starting it now. To start it at every sign-in, tick the box on its page."
Start-Process $exe

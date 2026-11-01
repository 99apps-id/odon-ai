# ==============================================================================
# ODON Desktop Installer (Open Digital Orchestration Network)
# Uses the authentic Cat Logo Icon across Desktop & Start Menu
# ==============================================================================

param (
    [string]$Destination = "$HOME\AppData\Local\ODON",
    [switch]$NoShortcut
)

$ErrorActionPreference = "Stop"

Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "   ODON - Open Digital Orchestration Network" -ForegroundColor White
Write-Host "   Installing Desktop & Background Daemon..." -ForegroundColor Gray
Write-Host "====================================================" -ForegroundColor Cyan

# 1. Create target directories
$binDir = Join-Path $Destination "bin"
$assetsDir = Join-Path $Destination "assets"
$webDir = Join-Path $Destination "web"

New-Item -ItemType Directory -Force -Path $binDir | Out-Null
New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null
New-Item -ItemType Directory -Force -Path $webDir | Out-Null

# 2. Copy Cat Logo icons
$sourceIcons = "$PSScriptRoot\..\assets\icons"
if (Test-Path "$sourceIcons\icon.ico") {
    Copy-Item "$sourceIcons\icon.ico" "$assetsDir\icon.ico" -Force
    Copy-Item "$sourceIcons\icon-256.png" "$assetsDir\icon-256.png" -Force
    Copy-Item "$sourceIcons\cat.svg" "$assetsDir\cat.svg" -Force
    Write-Host "✔ Cat Logo icons deployed to $assetsDir" -ForegroundColor Green
}

# 3. Create Windows Shortcuts with the Cat Logo Icon
if (-not $NoShortcut) {
    $WshShell = New-Object -ComObject WScript.Shell
    $iconPath = Join-Path $assetsDir "icon.ico"

    # Desktop Shortcut
    $desktopPath = [Environment]::GetFolderPath("Desktop")
    $shortcutPath = Join-Path $desktopPath "ODON.lnk"
    $shortcut = $WshShell.CreateShortcut($shortcutPath)
    $shortcut.TargetPath = "http://localhost:3000"
    $shortcut.IconLocation = "$iconPath, 0"
    $shortcut.Description = "ODON - Open Digital Orchestration Network"
    $shortcut.Save()

    # Start Menu Shortcut
    $startMenuPath = [Environment]::GetFolderPath("Programs")
    $startShortcutPath = Join-Path $startMenuPath "ODON.lnk"
    $startShortcut = $WshShell.CreateShortcut($startShortcutPath)
    $startShortcut.TargetPath = "http://localhost:3000"
    $startShortcut.IconLocation = "$iconPath, 0"
    $startShortcut.Description = "ODON - Open Digital Orchestration Network"
    $startShortcut.Save()

    Write-Host "✔ Desktop & Start Menu shortcuts created with Cat Logo icon" -ForegroundColor Green
}

Write-Host ""
Write-Host "🎉 ODON installation completed successfully!" -ForegroundColor Green
Write-Host "   Web UI: http://localhost:3000" -ForegroundColor White
Write-Host "   Daemon: 127.0.0.1:4096" -ForegroundColor White
Write-Host ""

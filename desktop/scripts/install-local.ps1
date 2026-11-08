# Installs the current release build for everyday use and points the desktop
# shortcut at it. The app is copied out of the cargo target directory so later
# test builds never fight with the copy you launch.
#
#   powershell -ExecutionPolicy Bypass -File desktop/scripts/install-local.ps1
#
# -NoBuild      copy what is already built instead of running cargo first
# -TargetDir    cargo target dir name under src-tauri (default: target)
# -InstallDir   where the app is copied (default: %LOCALAPPDATA%\Programs\Jellyfin Desktop (Tauri))
param(
    [switch]$NoBuild,
    [string]$TargetDir = "target",
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Jellyfin Desktop (Tauri)")
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauri = Join-Path $root "src-tauri"

if (-not $NoBuild) {
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    $env:CARGO_TARGET_DIR = Join-Path $tauri $TargetDir
    $env:TAURI_CONFIG = '{"bundle":{"resources":[]}}'
    Push-Location $tauri
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally { Pop-Location }
}

$release = Join-Path $tauri "$TargetDir\release"
$src = Join-Path $release "jellyfin-desktop.exe"
if (-not (Test-Path $src)) { throw "no release build at $src" }

$exe = Join-Path $InstallDir "Jellyfin Desktop.exe"
# Stop the installed copy if it is running so the exe can be replaced.
Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe } | ForEach-Object {
    Write-Host "stopping running instance (pid $($_.Id))"
    Stop-Process -Id $_.Id -Force
    $_.WaitForExit(5000) | Out-Null
}

New-Item -ItemType Directory -Force $InstallDir | Out-Null
Copy-Item $src $exe -Force
$dll = Join-Path $InstallDir "libmpv-2.dll"
$dllSrc = Join-Path $tauri "mpv\libmpv-2.dll"
if (-not (Test-Path $dll) -or (Get-Item $dllSrc).Length -ne (Get-Item $dll).Length) {
    Copy-Item $dllSrc $dll -Force
}

# Desktop shortcut (the Desktop folder may be redirected to OneDrive).
$desktop = [Environment]::GetFolderPath("Desktop")
$lnk = Join-Path $desktop "Jellyfin Desktop.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $exe
$s.WorkingDirectory = $InstallDir
$s.IconLocation = "$exe,0"
$s.Description = "Jellyfin Desktop"
$s.Save()

$ver = (Get-Item $exe).VersionInfo.FileVersion
Write-Host "installed $exe ($ver, built $((Get-Item $src).LastWriteTime))"
Write-Host "shortcut  $lnk"

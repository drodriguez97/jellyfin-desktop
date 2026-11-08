# Installs the current release build for everyday use and points the desktop
# shortcut at it. The app is copied out of the cargo target directory so later
# test builds never fight with the copy you launch.
#
#   powershell -ExecutionPolicy Bypass -File desktop/scripts/install-local.ps1
#
# -NoBuild      copy what is already built instead of running cargo first
# -TargetDir    cargo target dir name under src-tauri (default: target)
# -InstallDir   where the app is copied (default: %LOCALAPPDATA%\Programs\Jellyfin Desktop (Tauri))
#
# A running instance is never killed: Windows lets a running exe be renamed, so
# the old file is moved aside, the new one takes its place and is picked up by
# the next launch. The moved-aside file is cleaned up on a later run.
param(
    [switch]$NoBuild,
    [string]$TargetDir = "target",
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA "Programs\Jellyfin Desktop (Tauri)")
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauri = Join-Path $root "src-tauri"
Write-Host "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] install-local: commit $(git -C $root rev-parse --short HEAD)"

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

New-Item -ItemType Directory -Force $InstallDir | Out-Null
$exe = Join-Path $InstallDir "Jellyfin Desktop.exe"
$old = Join-Path $InstallDir "Jellyfin Desktop.old.exe"

# Remove a previously moved-aside exe if nothing runs it any more.
if (Test-Path $old) {
    try { Remove-Item $old -Force } catch { Write-Host "previous build still running; will remove it next time" }
}

if ((Test-Path $exe) -and (Get-FileHash $exe).Hash -eq (Get-FileHash $src).Hash) {
    Write-Host "installed exe is already this build"
} else {
    $running = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
    if ($running) {
        Write-Host "app is running (pid $($running.Id -join ', ')); staging the new build for the next launch"
        Move-Item $exe $old -Force
    }
    Copy-Item $src $exe -Force
}

$dll = Join-Path $InstallDir "libmpv-2.dll"
$dllSrc = Join-Path $tauri "mpv\libmpv-2.dll"
if (-not (Test-Path $dll) -or (Get-Item $dllSrc).Length -ne (Get-Item $dll).Length) {
    try { Copy-Item $dllSrc $dll -Force } catch { Write-Host "libmpv-2.dll is in use; it is unchanged in this build anyway" }
}

# Desktop shortcut (the Desktop folder may be redirected to OneDrive).
$desktop = [Environment]::GetFolderPath("Desktop")
$lnk = Join-Path $desktop "Jellyfin Desktop.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
if ($s.TargetPath -ne $exe) {
    $s.TargetPath = $exe
    $s.WorkingDirectory = $InstallDir
    $s.IconLocation = "$exe,0"
    $s.Description = "Jellyfin Desktop"
    $s.Save()
}

Write-Host "installed $exe (built $((Get-Item $src).LastWriteTime))"
Write-Host "shortcut  $lnk"

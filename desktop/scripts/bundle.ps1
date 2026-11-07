# Builds the release binary, the NSIS installer and a portable zip.
# Usage: powershell -File desktop/scripts/bundle.ps1
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauri = Join-Path $root "src-tauri"
if (-not (Test-Path (Join-Path $tauri "mpv\mpv.lib"))) {
    powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "fetch-mpv.ps1")
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Push-Location $tauri
try {
    npx --yes @tauri-apps/cli@^2 build
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }
} finally {
    Pop-Location
}

$release = Join-Path $tauri "target\release"
$portable = Join-Path $root "dist\portable"
Remove-Item -Recurse -Force $portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $portable | Out-Null
Copy-Item (Join-Path $release "jellyfin-desktop.exe") $portable
Copy-Item (Join-Path $tauri "mpv\libmpv-2.dll") $portable
New-Item -ItemType File (Join-Path $portable "portable") | Out-Null
$zip = Join-Path $root "dist\JellyfinDesktop-windows-x64-portable.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$portable\*" -DestinationPath $zip

Write-Host "Installer: " (Get-ChildItem (Join-Path $release "bundle\nsis\*.exe") | Select-Object -ExpandProperty FullName)
Write-Host "Portable:  $zip"

# Downloads the libmpv Windows build used by Jellyfin Desktop and places
# libmpv-2.dll, the import library and headers under desktop/src-tauri/mpv/.
#
# Usage: powershell -File desktop/scripts/fetch-mpv.ps1 [-Arch x86_64|x86_64-v3]
param(
    [string]$Arch = "x86_64",
    [string]$MpvVersion = "20251214-git-f7be2ee"
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$outDir = Join-Path $root "src-tauri\mpv"
$tmpDir = Join-Path $env:TEMP ("jfd-mpv-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $tmpDir | Out-Null
New-Item -ItemType Directory -Force $outDir | Out-Null

$archive = "mpv-dev-$Arch-$MpvVersion.7z"
$url = "https://sourceforge.net/projects/mpv-player-windows/files/libmpv/$archive/download"
$archivePath = Join-Path $tmpDir $archive

Write-Host "Downloading $archive ..."
# curl.exe ships with Windows 10+ and follows SourceForge's mirror redirects reliably.
& curl.exe -L --fail --silent --show-error -o $archivePath $url
if ($LASTEXITCODE -ne 0) { throw "download failed ($LASTEXITCODE)" }

$sevenZip = "C:\Program Files\7-Zip\7z.exe"
if (-not (Test-Path $sevenZip)) {
    $sevenZip = (Get-Command 7z -ErrorAction Stop).Source
}
Write-Host "Extracting ..."
& $sevenZip x -y "-o$tmpDir\extract" $archivePath | Out-Null

Copy-Item (Join-Path $tmpDir "extract\libmpv-2.dll") $outDir -Force
Copy-Item (Join-Path $tmpDir "extract\libmpv.dll.a") $outDir -Force
if (Test-Path (Join-Path $tmpDir "extract\include")) {
    Copy-Item (Join-Path $tmpDir "extract\include") $outDir -Recurse -Force
}

# Produce an MSVC import library (libmpv-2.dll.lib / mpv.lib) from the DLL exports,
# so the Rust msvc toolchain can link against it.
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsPath) { throw "Visual Studio C++ build tools not found" }
$vcvars = Join-Path $vsPath "VC\Auxiliary\Build\vcvars64.bat"

$def = Join-Path $outDir "mpv.def"
$cmd = "call `"$vcvars`" >nul && dumpbin /exports `"$outDir\libmpv-2.dll`""
$exports = cmd /c $cmd
$lines = @("LIBRARY libmpv-2.dll", "EXPORTS")
$inTable = $false
foreach ($line in $exports) {
    if ($line -match "ordinal\s+hint\s+RVA\s+name") { $inTable = $true; continue }
    if ($inTable) {
        if ($line -match "^\s*$") { if ($lines.Count -gt 2) { break } else { continue } }
        if ($line -match "^\s*\d+\s+[0-9A-Fa-f]+\s+[0-9A-Fa-f]+\s+(\S+)") { $lines += "    " + $Matches[1] }
    }
}
Set-Content -Path $def -Value $lines -Encoding ascii
$cmd = "call `"$vcvars`" >nul && lib /nologo /def:`"$def`" /out:`"$outDir\mpv.lib`" /machine:x64"
cmd /c $cmd | Out-Null
Copy-Item (Join-Path $outDir "mpv.lib") (Join-Path $outDir "libmpv-2.dll.lib") -Force

Remove-Item -Recurse -Force $tmpDir
Write-Host "libmpv ready in $outDir"
Get-ChildItem $outDir | Select-Object Name, Length

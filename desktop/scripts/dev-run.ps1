# Starts the debug build with an isolated config dir and the devtools port
# open, logging to $Scratch. Prints the process id. Stop with:
#   Stop-Process -Id <pid>
param(
    [string]$Scratch = $env:JFD_SCRATCH,
    [int]$Port = 9229,
    [string]$Tag = "dev",
    [string]$TargetDir = "target",   # cargo target dir name under src-tauri
    [string[]]$ExtraArgs = @()
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
if (-not $Scratch) { $Scratch = Join-Path $env:TEMP "jfd-dev" }
New-Item -ItemType Directory -Force $Scratch | Out-Null
$config = Join-Path $Scratch "config"
$env:RUST_LOG = if ($env:RUST_LOG) { $env:RUST_LOG } else { "info" }
$exe = "$root\src-tauri\$TargetDir\debug\jellyfin-desktop.exe"
if (-not (Test-Path "$root\src-tauri\$TargetDir\debug\libmpv-2.dll")) {
    Copy-Item "$root\src-tauri\mpv\libmpv-2.dll" "$root\src-tauri\$TargetDir\debug\"
}
$args = @("--config-dir", $config, "--remote-debugging-port", "$Port") + $ExtraArgs
$p = Start-Process -FilePath $exe -ArgumentList $args -RedirectStandardOutput (Join-Path $Scratch "$Tag-out.log") -RedirectStandardError (Join-Path $Scratch "$Tag-err.log") -PassThru
Write-Host "started pid $($p.Id), logs in $Scratch\$Tag-*.log, config in $config"

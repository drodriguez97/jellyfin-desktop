# Runs the Rust unit tests under cargo-llvm-cov and fails when line coverage
# of the measured code drops below the threshold (default 90%).
#
#   powershell -File desktop/scripts/coverage.ps1 [-MinLines 90] [-Html]
#
# Needs: rustup component add llvm-tools-preview; cargo install cargo-llvm-cov;
# desktop/scripts/fetch-mpv.ps1 (the test binary links libmpv like the app).
#
# What is measured: the modules that hold the app's decisions and can run
# without a window — settings, profiles, cli, logging, shell, urls,
# connectivity, player/logic, player/state, display_match. The files that
# only exist to drive a live WebView2 window, libmpv or the Win32 display API
# (lib, window, video_host, tray, power, media_keys, input, artwork, system,
# display, player/mod, player/events, player/commands, settings/commands) are
# thin glue over those and are excluded; they are exercised by running the app.
param(
    [int]$MinLines = 90,
    [switch]$Html
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$tauri = Join-Path $root "src-tauri"
$env:Path = "$env:USERPROFILE\.cargo\bin;$(Join-Path $tauri 'mpv');$env:Path"
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $tauri "target-test" }
$env:TAURI_CONFIG = '{"bundle":{"resources":[]}}'

$ignore = '(^|[\\/])(main|lib|window|video_host|tray|power|media_keys|input|artwork|system|display)\.rs$' +
          '|player[\\/](mod|events|commands)\.rs$|settings[\\/]commands\.rs$' +
          '|_tests\.rs$|[\\/]tests\.rs$'

$report = Join-Path $root "dist\coverage"
New-Item -ItemType Directory -Force $report | Out-Null

Push-Location $tauri
try {
    cargo llvm-cov --lib --ignore-filename-regex $ignore --fail-under-lines $MinLines
    if ($LASTEXITCODE -ne 0) { throw "coverage below $MinLines% or tests failed" }
    cargo llvm-cov report --lcov --ignore-filename-regex $ignore --output-path (Join-Path $report "lcov.info")
    if ($Html) {
        cargo llvm-cov report --html --ignore-filename-regex $ignore --output-dir $report
        Write-Host "HTML report: $(Join-Path $report 'html\index.html')"
    }
} finally { Pop-Location }

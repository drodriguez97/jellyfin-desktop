# Runs the debug build unattended, screenshots the window and prints its log.
# Usage: powershell -File spike-run.ps1 -Tag name [-Env @{JFD_SPIKE_VIDEO_TOP="1"}] [-WaitSeconds 7]
param(
    [string]$Tag = "spike",
    [string]$EnvVars = "",   # "KEY=VALUE;KEY2=VALUE2"
    [int]$WaitSeconds = 7
)
$Env = @{}
foreach ($pair in $EnvVars.Split(';')) {
    if ($pair -match '^([^=]+)=(.*)$') { $Env[$Matches[1]] = $Matches[2] }
}
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$scratch = "C:\Users\drod1\AppData\Local\Temp\claude\b2bf914577e2e119\ef5945d0-23b0-4630-a9f3-ef04ef5\scratchpad"
if ($env:JFD_SCRATCH) { $scratch = $env:JFD_SCRATCH }
New-Item -ItemType Directory -Force $scratch | Out-Null
$env:JFD_SPIKE_AUTO = "1"
$env:RUST_LOG = "info"
foreach ($k in $Env.Keys) { Set-Item -Path "env:$k" -Value $Env[$k] }
$out = Join-Path $scratch "$Tag-out.log"
$err = Join-Path $scratch "$Tag-err.log"
$p = Start-Process -FilePath "$root\src-tauri\target\debug\jellyfin-desktop.exe" -RedirectStandardOutput $out -RedirectStandardError $err -PassThru
Start-Sleep -Seconds $WaitSeconds
powershell -NoProfile -File "$root\scripts\screenshot-window.ps1" -Out (Join-Path $scratch "$Tag.png")
Start-Sleep -Seconds 1
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Write-Host "---- stderr"; Get-Content $err
Write-Host "---- stdout"; Get-Content $out

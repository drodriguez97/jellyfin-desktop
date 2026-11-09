# Installs the repository's git hooks (run once per clone):
#
#   powershell -ExecutionPolicy Bypass -File desktop/scripts/install-hooks.ps1
#
# post-commit: after every commit that touches desktop/ (apart from Markdown),
# builds the release binary and installs it behind the desktop shortcut in the
# background via install-local.ps1. Output goes to desktop/dist/install-local.log.
$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$gitDir = git -C $root rev-parse --git-dir
if (-not [IO.Path]::IsPathRooted($gitDir)) { $gitDir = Join-Path $root $gitDir }
$hooks = Join-Path $gitDir "hooks"
New-Item -ItemType Directory -Force $hooks | Out-Null

$postCommit = @'
#!/bin/sh
# Installed by desktop/scripts/install-hooks.ps1. Rebuilds and installs the
# release exe in the background when the commit touched the app.
if git diff-tree --no-commit-id --name-only -r HEAD -- desktop/ ':(exclude)desktop/**/*.md' | grep -q .; then
    root=$(git rev-parse --show-toplevel)
    mkdir -p "$root/desktop/dist"
    log="$root/desktop/dist/install-local.log"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command \
        "Start-Process powershell.exe -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','$root/desktop/scripts/install-local.ps1','-TargetDir','target-test' -RedirectStandardOutput '$log' -RedirectStandardError '$log.err'" \
        >/dev/null 2>&1 &
    echo "post-commit: building and installing the release exe in the background (log: desktop/dist/install-local.log)"
fi
exit 0
'@
$dest = Join-Path $hooks "post-commit"
[IO.File]::WriteAllText($dest, $postCommit.Replace("`r`n", "`n"))
Write-Host "installed $dest"

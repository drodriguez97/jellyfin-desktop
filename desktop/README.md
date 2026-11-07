# Jellyfin Desktop (Tauri shell)

Windows-only rewrite of the Jellyfin Desktop shell: the system WebView2
(Edge Chromium) renders jellyfin-web, and libmpv renders video with D3D11
directly into the window underneath the transparent webview. No bundled
Chromium, no OpenGL interop.

```
desktop/
  src-tauri/      Rust app (Tauri 2): window, mpv player, settings, profiles, IPC commands
  web/            Connect screen + the scripts injected into every page (bridge/)
  scripts/        fetch-mpv.ps1 (libmpv), dev-run.ps1 (run with devtools), cdp.mjs (drive the webview)
```

## Build

Prerequisites: Rust (stable, MSVC), Visual Studio Build Tools with the C++
workload, 7-Zip, the WebView2 runtime (ships with Windows 10/11).

```powershell
powershell -File desktop/scripts/fetch-mpv.ps1      # downloads libmpv-2.dll and builds mpv.lib
cd desktop/src-tauri
cargo build --release
copy mpv\libmpv-2.dll target\release\
target\release\jellyfin-desktop.exe
```

The installer is produced with the Tauri CLI (`npx @tauri-apps/cli build`),
which bundles `libmpv-2.dll` and the WebView2 bootstrapper.

## How it fits together

- `src/lib.rs` parses the CLI, picks the profile, loads settings and builds
  the window. The window is created from Rust (not `tauri.conf.json`) so the
  initialization script and per-profile WebView2 data directory can be set.
- `src/video_host.rs` creates a child HWND at the bottom of the z-order and
  hands it to mpv as `wid`. It must not have `WS_CLIPSIBLINGS`, otherwise
  DWM clips the video to the area not covered by the webview.
- `src/player/` is the port of the Qt `PlayerComponent`: option mapping from
  settings, the mpv event thread, stream selection hooks, and the
  `player_*` Tauri commands.
- `src/settings/` and `src/profiles.rs` read and write the same
  `jellyfin-desktop.conf`, `storage.json` and `profiles/<id>/` layout as the
  Qt app, so existing installs keep their server and settings.
- `web/bridge/api.js` recreates the `window.api.{player,system,settings,
  input,window}` objects that `nativeshell.js` and the mpv plugins expect,
  on top of Tauri `invoke`/`listen`. Native-to-web signals are Tauri events
  named `<object>:<signal>` with an argument tuple as payload.
- `src/shell.rs` concatenates `jmpInfo` + the bridge scripts into one
  initialization script that runs on every page, including the remote
  jellyfin-web, before the page's own scripts.
- Remote origins get IPC access through `capabilities/remote.json`.

## Development

```powershell
cd desktop/src-tauri
cargo build
powershell -File ..\scripts\dev-run.ps1 -Tag dev       # isolated config dir, devtools on :9229
node --experimental-websocket ..\scripts\cdp.mjs 9229 logs 10
node --experimental-websocket ..\scripts\cdp.mjs 9229 eval "location.href"
```

Logs go to `profiles/<id>/logs/jellyfin-desktop.log` (auth tokens are
censored). `RUST_LOG=debug` raises the level; `JF_MPV_VERBOSE=1` makes mpv
verbose. Press F12 in the app for the WebView2 devtools.

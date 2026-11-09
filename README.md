# Jellyfin Desktop (Tauri)

A fast Windows desktop client for [Jellyfin](https://jellyfin.org): the
system WebView2 renders the Jellyfin web client, and
[libmpv](https://mpv.io) plays video with D3D11 hardware decoding straight
into the window. No bundled browser engine, no OpenGL copies per frame, and
an installer of about 35 MB.

This is an independent fork of
[jellyfin/jellyfin-desktop](https://github.com/jellyfin/jellyfin-desktop)
and is not affiliated with or endorsed by the Jellyfin project. The
original Qt shell has been removed; only the Windows Tauri shell under
[`desktop/`](desktop/) remains.

## What is different from upstream

- **Shell.** Rust + Tauri 2 with the WebView2 runtime that ships with
  Windows 10/11, instead of Qt 6 and a bundled Chromium.
- **Video.** libmpv renders through D3D11 into a child window underneath
  a transparent webview; hardware decoding is native D3D11VA by default.
- **Remote servers.** The mpv demuxer cache lives on disk, with a budget
  that follows the free space on the drive, so seeking and bitrate spikes
  over the internet do not stall.
- **Browsing.** Poster images load three screens ahead and stay loaded, so
  fast scrolling through a large library does not show empty tiles.
- **Polish.** Dark title bar, no flash at launch, native refresh-rate
  switching, media keys and the Windows media controls, per-profile
  settings that migrate from the Qt version.
- **No update check.** This build does not contact the upstream release
  feed.

## Requirements

- Windows 10 or 11, x64.
- The WebView2 runtime. It is part of Windows 11 and most Windows 10
  installs; the installer downloads it when it is missing.
- A Jellyfin server; developed against Jellyfin 12.

## Downloads

Every push to `master` builds an installer and a portable zip in the
[build-desktop-tauri](../../actions/workflows/build-desktop-tauri.yml)
workflow; the artifacts are attached to each run. The portable zip keeps
its data in a `data` folder next to the exe.

Builds are not code-signed, so Windows SmartScreen shows a warning on
first launch.

## Building

See [desktop/README.md](desktop/README.md). In short: install Rust (MSVC),
the Visual Studio C++ build tools and 7-Zip, then

```powershell
powershell -File desktop/scripts/fetch-mpv.ps1
cd desktop/src-tauri
cargo build --release
```

`desktop/scripts/install-local.ps1` builds a release and installs it behind
a desktop shortcut for day-to-day use; `desktop/scripts/install-hooks.ps1`
makes that happen automatically after every commit.

## Command line

```
--desktop | --tv              Start in desktop or TV layout
--windowed | --fullscreen     Start windowed or fullscreen
--ignore-certificate-errors   Ignore TLS certificate errors
--remote-debugging-port <n>   Expose the WebView2 devtools on this port
--config-dir <path>           Override the data directory
--log-level <level>           debug, info, warn, error (default: info)
--profile <name>              Use a specific profile for this session
--list-profiles               List all profiles and exit
--create-profile <name>       Create a new profile and exit
--delete-profile <name>       Delete a profile and exit
--set-default-profile <name>  Set the default profile and exit
```

## File locations

Data is stored per profile under `%LOCALAPPDATA%\Jellyfin Desktop\profiles\<profile-id>\`:

- `jellyfin-desktop.conf` settings; `storage.json` window state
- `mpv.conf` optional, applied on top of the shell's mpv options
- `logs\` application logs
- `mpv-cache\` the on-disk demuxer cache
- `WebView2\` the web client's own data (login, display preferences)

## License

GPL-2.0-or-later, as upstream. See [LICENSE](LICENSE). libmpv is
downloaded from the
[shinchiro mpv builds](https://github.com/shinchiro/mpv-winbuild-cmake)
and is distributed under its own GPL terms.

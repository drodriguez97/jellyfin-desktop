# Contributing

This fork is a Windows-only rewrite of the Jellyfin Desktop shell on
Rust, Tauri 2, WebView2 and libmpv. All of the code lives under
[`desktop/`](desktop/); its [README](desktop/README.md) explains the layout,
how to build, and how the pieces fit together.

## Ground rules

- Performance comes first. A change that makes browsing or playback slower,
  or adds work on the hot path (page load, scrolling, seeking), needs a
  measured justification.
- jellyfin-web is loaded from the server and is not patched here. Behaviour
  changes to the web client belong in the scripts under
  `desktop/web/bridge/`, which run before every page.
- Keep the shell small. Prefer a WebView2 or mpv option over new code.

## Workflow

1. Build and run from source: `desktop/scripts/dev-run.ps1` starts a debug
   build with the WebView2 devtools exposed, `desktop/scripts/cdp.mjs`
   drives it from the command line.
2. Open a pull request against `master`. The `build-desktop-tauri` workflow
   builds the installer and portable zip for every pull request that touches
   `desktop/`.
3. Write commit messages that say why, not just what.

## Reporting problems

Open an issue with the log from
`%LOCALAPPDATA%\Jellyfin Desktop\profiles\<profile-id>\logs\`. For
playback problems, run once with `--log-level debug` and include the mpv
lines around the time of the problem.

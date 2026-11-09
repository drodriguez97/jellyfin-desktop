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
2. Run the tests: `cargo test` in `desktop/src-tauri` (with
   `desktop/src-tauri/mpv` on `PATH`, since the test binary links libmpv like
   the app), or `desktop/scripts/coverage.ps1` for the coverage report.
3. Open a pull request against `master`. The `build-desktop-tauri` workflow
   runs the tests with coverage and builds the installer and portable zip for
   every pull request that touches `desktop/`.
4. Write commit messages that say why, not just what.

## Tests and coverage

The app's decisions live in modules that run without a window and are unit
tested: `settings`, `profiles`, `cli`, `logging`, `shell`, `urls`,
`connectivity`, `player/logic`, `player/state` and `display_match`. CI fails
when their line coverage drops below 90% (`desktop/scripts/coverage.ps1`,
cargo-llvm-cov). The remaining files only drive a live WebView2 window, libmpv
or the Win32 display API; they are thin glue over the tested modules, are
excluded from the measurement, and are exercised by running the app. New
logic goes into a tested module, not into the glue.

Tests sit next to their module in `<module>_tests.rs` (or `tests.rs` in a
module directory) so test code never counts toward coverage.

## Reporting problems

Open an issue with the log from
`%LOCALAPPDATA%\Jellyfin Desktop\profiles\<profile-id>\logs\`. For
playback problems, run once with `--log-level debug` and include the mpv
lines around the time of the problem.

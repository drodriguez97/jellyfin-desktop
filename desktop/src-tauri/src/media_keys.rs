//! Windows media integration: System Media Transport Controls (media keys,
//! the volume flyout's now-playing card) via `souvlaki`, driven from a
//! dedicated thread because the controls are bound to the window.
//! Port of the SMTC half of `TaskbarComponentWin`.

use crate::input;
use crate::settings;
use serde_json::Value;
use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tracing::{debug, info, warn};

#[derive(Debug)]
enum Cmd {
    Playback(MediaPlayback),
    Metadata {
        title: String,
        artist: String,
        album: String,
        duration_ms: i64,
        cover_url: Option<String>,
    },
    Position(i64),
}

pub struct MediaKeys {
    tx: Sender<Cmd>,
}

/// Starts the SMTC thread. `hwnd` is the main window handle.
pub fn start(app: &AppHandle, hwnd: isize) -> Option<MediaKeys> {
    let enabled = app
        .try_state::<settings::commands::SettingsState>()
        .map(|s| s.lock().bool(settings::SECTION_MAIN, "enableWindowsMediaIntegration"))
        .unwrap_or(true);
    if !enabled {
        info!("Windows media integration disabled by settings");
        return None;
    }

    let (tx, rx) = channel::<Cmd>();
    let handle = app.clone();
    let spawned = std::thread::Builder::new().name("smtc".into()).spawn(move || {
        let config = PlatformConfig {
            dbus_name: "jellyfin-desktop",
            display_name: "Jellyfin Desktop",
            hwnd: Some(hwnd as *mut std::ffi::c_void),
        };
        let mut controls = match MediaControls::new(config) {
            Ok(c) => c,
            Err(e) => {
                warn!("SystemMediaTransportControls unavailable: {e:?}");
                return;
            }
        };
        let events_app = handle.clone();
        if let Err(e) = controls.attach(move |event| on_event(&events_app, event)) {
            warn!("SMTC attach failed: {e:?}");
            return;
        }
        info!("SystemMediaTransportControls initialized");
        let _ = controls.set_playback(MediaPlayback::Stopped);

        let mut last_position = Duration::ZERO;
        let mut last_playing: Option<bool> = None;
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Cmd::Playback(p) => {
                    last_playing = match &p {
                        MediaPlayback::Playing { .. } => Some(true),
                        MediaPlayback::Paused { .. } => Some(false),
                        MediaPlayback::Stopped => None,
                    };
                    if let Err(e) = controls.set_playback(p) {
                        debug!("SMTC set_playback: {e:?}");
                    }
                }
                Cmd::Position(ms) => {
                    last_position = Duration::from_millis(ms.max(0) as u64);
                    let progress = Some(MediaPosition(last_position));
                    let playback = match last_playing {
                        Some(true) => MediaPlayback::Playing { progress },
                        Some(false) => MediaPlayback::Paused { progress },
                        None => continue,
                    };
                    let _ = controls.set_playback(playback);
                }
                Cmd::Metadata { title, artist, album, duration_ms, cover_url } => {
                    let metadata = MediaMetadata {
                        title: Some(title.as_str()),
                        artist: if artist.is_empty() { None } else { Some(artist.as_str()) },
                        album: if album.is_empty() { None } else { Some(album.as_str()) },
                        cover_url: cover_url.as_deref(),
                        duration: if duration_ms > 0 { Some(Duration::from_millis(duration_ms as u64)) } else { None },
                    };
                    if let Err(e) = controls.set_metadata(metadata) {
                        debug!("SMTC set_metadata: {e:?}");
                    }
                }
            }
        }
    });
    match spawned {
        Ok(_) => Some(MediaKeys { tx }),
        Err(e) => {
            warn!("could not start SMTC thread: {e}");
            None
        }
    }
}

fn on_event(app: &AppHandle, event: MediaControlEvent) {
    debug!("SMTC event: {event:?}");
    match event {
        MediaControlEvent::Play | MediaControlEvent::Pause | MediaControlEvent::Toggle => {
            input::send_action(app, "play_pause")
        }
        MediaControlEvent::Next => input::send_action(app, "next"),
        MediaControlEvent::Previous => input::send_action(app, "previous"),
        MediaControlEvent::Stop => input::send_action(app, "stop"),
        MediaControlEvent::Seek(souvlaki::SeekDirection::Forward) => input::send_action(app, "seek_forward"),
        MediaControlEvent::Seek(souvlaki::SeekDirection::Backward) => input::send_action(app, "seek_backward"),
        MediaControlEvent::SetPosition(MediaPosition(pos)) => {
            let _ = app.emit("input:positionSeek", (pos.as_millis() as i64,));
        }
        MediaControlEvent::SetVolume(v) => {
            let _ = app.emit("input:volumeChanged", ((v * 100.0).round() as i64,));
        }
        MediaControlEvent::Raise => crate::window::window_raise(app.clone()),
        MediaControlEvent::Quit => app.exit(0),
        _ => {}
    }
}

fn keys(app: &AppHandle) -> Option<Sender<Cmd>> {
    app.try_state::<MediaKeysState>()
        .and_then(|s| s.lock().as_ref().map(|k| k.tx.clone()))
}

pub type MediaKeysState = parking_lot::Mutex<Option<MediaKeys>>;

/// `state` is the web client's PlaybackState name: Playing, Paused, Stopped.
pub fn on_playback_state(app: &AppHandle, state: &str) {
    let Some(tx) = keys(app) else { return };
    let playback = match state {
        "Playing" => MediaPlayback::Playing { progress: None },
        "Paused" => MediaPlayback::Paused { progress: None },
        _ => MediaPlayback::Stopped,
    };
    let _ = tx.send(Cmd::Playback(playback));
}

pub fn on_position(app: &AppHandle, position_ms: i64) {
    if let Some(tx) = keys(app) {
        let _ = tx.send(Cmd::Position(position_ms));
    }
}

fn join_strings(v: Option<&Value>) -> String {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

/// `item` is a Jellyfin BaseItemDto (NowPlayingItem); `cover_url` an optional
/// local `file://` URL prepared by the player.
pub fn on_metadata(app: &AppHandle, item: &Value) {
    set_metadata(app, item, None);
}

pub fn set_metadata(app: &AppHandle, item: &Value, cover_url: Option<String>) {
    let Some(tx) = keys(app) else { return };
    let s = |k: &str| item.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut title = s("Name");
    let mut artist = join_strings(item.get("Artists"));
    if artist.is_empty() {
        artist = s("AlbumArtist");
    }
    let mut album = s("Album");
    if s("Type") == "Episode" {
        let series = s("SeriesName");
        if !series.is_empty() {
            let season = item.get("ParentIndexNumber").and_then(Value::as_i64);
            let episode = item.get("IndexNumber").and_then(Value::as_i64);
            let code = match (season, episode) {
                (Some(se), Some(ep)) => format!("S{se}:E{ep} - "),
                _ => String::new(),
            };
            title = format!("{code}{title}");
            if artist.is_empty() {
                artist = series.clone();
            }
            if album.is_empty() {
                album = series;
            }
        }
    }
    let duration_ms = item.get("RunTimeTicks").and_then(Value::as_i64).unwrap_or(0) / 10_000;
    let _ = tx.send(Cmd::Metadata { title, artist, album, duration_ms, cover_url });
}

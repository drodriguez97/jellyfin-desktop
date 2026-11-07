//! mpv event thread: observed properties, hooks and the canonical playback
//! state machine (port of `PlayerComponent::handleMpvEvent` /
//! `updatePlaybackState`). Everything the web client needs is emitted as
//! `player:<signal>` Tauri events.

use super::{MediaType, Player};
use anyhow::Result;
use libmpv2::events::{Event, PropertyData};
use libmpv2::Format;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::Emitter;
use tracing::{debug, info, warn};

const PROP_PAUSE: u64 = 1;
const PROP_CORE_IDLE: u64 = 2;
const PROP_BUFFERING: u64 = 3;
const PROP_PLAYBACK_TIME: u64 = 4;
const PROP_VO_CONFIGURED: u64 = 5;
const PROP_DURATION: u64 = 6;
const PROP_AUDIO_DEVICES: u64 = 7;
const PROP_VIDEO_DEC_PARAMS: u64 = 8;
const PROP_CACHE_STATE: u64 = 9;

const HOOK_ON_LOAD: u64 = 1;
const HOOK_ON_PRELOADED: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Finished,
    Canceled,
    Error,
    Paused,
    Playing,
    Buffering,
}

impl State {
    fn name(self) -> &'static str {
        match self {
            State::Finished => "finished",
            State::Canceled => "canceled",
            State::Error => "error",
            State::Paused => "paused",
            State::Playing => "playing",
            State::Buffering => "buffering",
        }
    }
}

struct Tracker {
    state: State,
    paused: bool,
    playback_active: bool,
    in_playback: bool,
    playback_canceled: bool,
    playback_error: String,
    buffering_percentage: i64,
    last_buffering_percentage: i64,
    window_visible: bool,
    video_playback_active: bool,
    last_position: f64,
    last_position_emit: std::time::Instant,
}

impl Tracker {
    fn new() -> Self {
        Self {
            state: State::Finished,
            paused: false,
            playback_active: false,
            in_playback: false,
            playback_canceled: false,
            playback_error: String::new(),
            buffering_percentage: 100,
            last_buffering_percentage: -1,
            window_visible: false,
            video_playback_active: false,
            last_position: 0.0,
            last_position_emit: std::time::Instant::now(),
        }
    }

    fn update_playback_state(&mut self, player: &Player) {
        let new_state = if self.in_playback {
            if self.paused {
                State::Paused
            } else if self.playback_active {
                State::Playing
            } else {
                if self.buffering_percentage == 100 {
                    self.buffering_percentage = 0;
                }
                State::Buffering
            }
        } else if !self.playback_error.is_empty() {
            State::Error
        } else if self.playback_canceled {
            State::Canceled
        } else {
            State::Finished
        };

        if new_state != self.state {
            info!("Entering state: {}", new_state.name());
            match new_state {
                State::Paused => emit(player, "paused", ()),
                State::Playing => emit(player, "playing", ()),
                State::Buffering => self.last_buffering_percentage = -1,
                State::Finished => {
                    emit(player, "finished", ());
                    emit(player, "stopped", ());
                }
                State::Canceled => {
                    emit(player, "canceled", ());
                    emit(player, "stopped", ());
                }
                State::Error => emit(player, "error", (self.playback_error.clone(),)),
            }
            emit(player, "stateChanged", (new_state.name(), self.state.name()));
            self.state = new_state;
        }

        if self.state == State::Buffering && self.last_buffering_percentage != self.buffering_percentage {
            emit(player, "buffering", (self.buffering_percentage as f64,));
        }
        self.last_buffering_percentage = self.buffering_percentage;

        let video_active = self.state == State::Playing && self.window_visible;
        if video_active != self.video_playback_active {
            self.video_playback_active = video_active;
            emit(player, "videoPlaybackActive", (video_active,));
            crate::power::set_display_required(video_active);
        }
    }
}

fn emit<S: serde::Serialize + Clone>(player: &Player, signal: &str, payload: S) {
    let _ = player.app.emit(&format!("player:{signal}"), payload);
}

/// Registers observers and hooks, then runs the event loop on its own thread.
pub fn start(player: Arc<Player>) -> Result<()> {
    let mpv = &player.mpv;
    mpv.observe_property("pause", Format::Flag, PROP_PAUSE)?;
    mpv.observe_property("core-idle", Format::Flag, PROP_CORE_IDLE)?;
    mpv.observe_property("cache-buffering-state", Format::Int64, PROP_BUFFERING)?;
    mpv.observe_property("playback-time", Format::Double, PROP_PLAYBACK_TIME)?;
    mpv.observe_property("vo-configured", Format::Flag, PROP_VO_CONFIGURED)?;
    mpv.observe_property("duration", Format::Double, PROP_DURATION)?;
    mpv.observe_property("audio-device-list", Format::String, PROP_AUDIO_DEVICES)?;
    mpv.observe_property("video-dec-params", Format::String, PROP_VIDEO_DEC_PARAMS)?;
    mpv.observe_property("demuxer-cache-state", Format::String, PROP_CACHE_STATE)?;

    unsafe {
        let ctx = mpv.ctx.as_ptr();
        let on_load = c"on_load";
        let on_preloaded = c"on_preloaded";
        libmpv2_sys::mpv_hook_add(ctx, HOOK_ON_LOAD, on_load.as_ptr(), 0);
        libmpv2_sys::mpv_hook_add(ctx, HOOK_ON_PRELOADED, on_preloaded.as_ptr(), 0);
    }

    std::thread::Builder::new()
        .name("mpv-events".into())
        .spawn(move || event_loop(player))?;
    Ok(())
}

fn event_loop(player: Arc<Player>) {
    let mut tracker = Tracker::new();
    loop {
        let Some(result) = player.mpv.wait_event(1.0) else { continue };
        let event = match result {
            Ok(ev) => ev,
            Err(e) => {
                debug!("mpv event error: {e}");
                continue;
            }
        };
        let mut state_dirty = true;
        match event {
            Event::Shutdown => {
                info!("mpv shutdown");
                break;
            }
            Event::StartFile => tracker.in_playback = true,
            Event::EndFile(reason) => {
                tracker.in_playback = false;
                tracker.playback_canceled = false;
                tracker.playback_error.clear();
                match reason {
                    libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_ERROR => {
                        // The error string is not exposed through this variant; use a generic message.
                        tracker.playback_error = "playback error".into();
                    }
                    libmpv2_sys::mpv_end_file_reason_MPV_END_FILE_REASON_STOP => {
                        tracker.playback_canceled = true;
                    }
                    _ => {}
                }
                let switching = player
                    .stream_switch_imminent
                    .swap(false, std::sync::atomic::Ordering::Relaxed);
                if !switching {
                    // Restore the display mode unless another file is about to play.
                    let app = player.app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        if let Some(p) = crate::player::commands::player_from(&app) {
                            if p.get_bool("idle-active") {
                                crate::display::restore_previous_mode();
                                p.update_video_configuration();
                            }
                        }
                    });
                }
            }
            Event::PropertyChange { change, reply_userdata, .. } => {
                state_dirty = handle_property(&player, &mut tracker, reply_userdata, change);
            }
            Event::LogMessage { prefix, text, log_level, .. } => {
                let line = format!("{prefix}: {}", text.trim_end());
                if log_level <= libmpv2_sys::mpv_log_level_MPV_LOG_LEVEL_ERROR {
                    warn!(target: "mpv", "{line}");
                } else if log_level <= libmpv2_sys::mpv_log_level_MPV_LOG_LEVEL_WARN {
                    warn!(target: "mpv", "{line}");
                } else {
                    debug!(target: "mpv", "{line}");
                }
                state_dirty = false;
            }
            Event::Deprecated(raw) => {
                if raw.event_id == libmpv2_sys::mpv_event_id_MPV_EVENT_HOOK {
                    let hook = unsafe { *(raw.data as *const libmpv2_sys::mpv_event_hook) };
                    handle_hook(&player, hook.id);
                }
                state_dirty = false;
            }
            _ => state_dirty = false,
        }

        if state_dirty {
            // Drain everything that is already queued before deciding the state.
            tracker.update_playback_state(&player);
        }
    }
}

fn handle_hook(player: &Player, id: u64) {
    match id {
        HOOK_ON_LOAD => {
            // Switch the display to the media's frame rate before loading, then
            // give the monitor time to settle (mode changes can break hwdec init).
            if player.switch_display_frame_rate() {
                let delay = player
                    .settings
                    .lock()
                    .i64(crate::settings::SECTION_VIDEO, "refreshrate.delay")
                    .max(0) as u64;
                info!("waiting {delay}s after rate switch before loading");
                std::thread::sleep(std::time::Duration::from_secs(delay));
                player.update_video_configuration();
            }
        }
        HOOK_ON_PRELOADED => {
            let (sub, audio) = {
                let pb = player.playback.lock();
                (pb.current_subtitle_stream.clone(), pb.current_audio_stream.clone())
            };
            player.reselect_stream(&sub, MediaType::Subtitle);
            player.reselect_stream(&audio, MediaType::Audio);
        }
        _ => {}
    }
    unsafe {
        libmpv2_sys::mpv_hook_continue(player.mpv.ctx.as_ptr(), id);
    }
}

/// Returns true when the playback state machine should be re-evaluated.
fn handle_property(player: &Player, t: &mut Tracker, id: u64, data: PropertyData<'_>) -> bool {
    match id {
        PROP_PAUSE => {
            if let PropertyData::Flag(v) = data {
                t.paused = v;
            }
            true
        }
        PROP_CORE_IDLE => {
            if let PropertyData::Flag(v) = data {
                t.playback_active = !v;
            }
            true
        }
        PROP_BUFFERING => {
            t.buffering_percentage = match data {
                PropertyData::Int64(v) => v,
                _ => 100,
            };
            true
        }
        PROP_PLAYBACK_TIME => {
            if let PropertyData::Double(pos) = data {
                // mpv reports every frame; the web client only needs the ~4 Hz an
                // HTML5 video element gives it. Seeks (big jumps) go through at once.
                let jumped = (pos - t.last_position).abs() > 1.0;
                let due = t.last_position_emit.elapsed() >= std::time::Duration::from_millis(200);
                if (pos - t.last_position).abs() > 0.015 && (jumped || due) {
                    let ms = (pos * 1000.0).max(0.0) as u64;
                    emit(player, "positionUpdate", (ms,));
                    t.last_position = pos;
                    t.last_position_emit = std::time::Instant::now();
                }
            }
            false
        }
        PROP_VO_CONFIGURED => {
            t.window_visible = matches!(data, PropertyData::Flag(true));
            emit(player, "windowVisible", (t.window_visible,));
            true
        }
        PROP_DURATION => {
            if let PropertyData::Double(d) = data {
                emit(player, "updateDuration", ((d * 1000.0) as i64,));
            }
            false
        }
        PROP_AUDIO_DEVICES => {
            player.update_audio_device_list();
            false
        }
        PROP_VIDEO_DEC_PARAMS => {
            player.update_video_aspect_settings();
            false
        }
        PROP_CACHE_STATE => {
            if let PropertyData::Str(s) = data {
                let ticks_per_second = 10_000_000.0; // Jellyfin's 100ns ticks
                let parsed: Value = serde_json::from_str(s).unwrap_or(Value::Null);
                let ranges: Vec<Value> = parsed
                    .get("seekable-ranges")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|r| {
                                let start = r.get("start").and_then(Value::as_f64).unwrap_or(0.0);
                                let end = r.get("end").and_then(Value::as_f64).unwrap_or(0.0);
                                json!({
                                    "start": (start * ticks_per_second) as i64,
                                    "end": (end * ticks_per_second) as i64
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                emit(player, "bufferedRangesUpdated", (ranges,));
            }
            false
        }
        _ => false,
    }
}

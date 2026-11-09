//! The playback state machine the web client sees (port of
//! `PlayerComponent::updatePlaybackState`), kept free of mpv and Tauri so the
//! transitions can be tested. `events.rs` feeds it mpv property changes and
//! turns the returned [`StateUpdate`] into `player:*` events.

use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Finished,
    Canceled,
    Error,
    Paused,
    Playing,
    Buffering,
}

impl State {
    pub fn name(self) -> &'static str {
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

/// Why mpv ended a file, as far as the state machine cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Eof,
    Stop,
    Error,
}

/// What changed after re-evaluating the state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StateUpdate {
    /// (new, old) when the state changed.
    pub transition: Option<(State, State)>,
    /// Buffering progress to report (only while buffering, only when it changed).
    pub buffering_percent: Option<i64>,
    /// Whether video is now actively showing on screen, when that changed.
    pub video_active: Option<bool>,
}

/// Minimum interval between position reports; mpv reports every frame but the
/// web client only needs the ~4 Hz an HTML5 video element gives it.
pub const POSITION_INTERVAL: Duration = Duration::from_millis(200);

pub struct Tracker {
    pub state: State,
    pub paused: bool,
    pub playback_active: bool,
    pub in_playback: bool,
    pub playback_canceled: bool,
    pub playback_error: String,
    pub buffering_percentage: i64,
    last_buffering_percentage: i64,
    pub window_visible: bool,
    video_playback_active: bool,
    last_position: f64,
    last_position_emit: Instant,
    pub buffering_since: Instant,
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

impl Tracker {
    pub fn new() -> Self {
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
            last_position_emit: Instant::now(),
            buffering_since: Instant::now(),
        }
    }

    pub fn start_file(&mut self) {
        self.in_playback = true;
    }

    pub fn end_file(&mut self, reason: EndReason) {
        self.in_playback = false;
        self.playback_canceled = false;
        self.playback_error.clear();
        match reason {
            // The error string is not exposed by libmpv2's event; use a generic message.
            EndReason::Error => self.playback_error = "playback error".into(),
            EndReason::Stop => self.playback_canceled = true,
            EndReason::Eof => {}
        }
    }

    /// Decides whether a position report is due: big jumps (seeks) go through
    /// at once, small advances at most every [`POSITION_INTERVAL`]. Returns the
    /// position in milliseconds to report.
    pub fn position_update(&mut self, pos: f64, now: Instant) -> Option<u64> {
        let delta = (pos - self.last_position).abs();
        let jumped = delta > 1.0;
        let due = now.duration_since(self.last_position_emit) >= POSITION_INTERVAL;
        if delta > 0.015 && (jumped || due) {
            self.last_position = pos;
            self.last_position_emit = now;
            Some((pos * 1000.0).max(0.0) as u64)
        } else {
            None
        }
    }

    fn next_state(&mut self) -> State {
        if self.in_playback {
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
        }
    }

    /// Re-evaluates the state from the flags and reports what changed.
    pub fn update(&mut self, now: Instant) -> StateUpdate {
        let mut out = StateUpdate::default();
        let new_state = self.next_state();
        if new_state != self.state {
            if new_state == State::Buffering {
                self.last_buffering_percentage = -1;
                self.buffering_since = now;
            }
            out.transition = Some((new_state, self.state));
            self.state = new_state;
        }

        if self.state == State::Buffering && self.last_buffering_percentage != self.buffering_percentage {
            out.buffering_percent = Some(self.buffering_percentage);
        }
        self.last_buffering_percentage = self.buffering_percentage;

        let video_active = self.state == State::Playing && self.window_visible;
        if video_active != self.video_playback_active {
            self.video_playback_active = video_active;
            out.video_active = Some(video_active);
        }
        out
    }
}

/// mpv's `demuxer-cache-state` seekable ranges as Jellyfin ticks (100 ns).
pub fn cache_ranges(json_text: &str) -> Vec<Value> {
    const TICKS_PER_SECOND: f64 = 10_000_000.0;
    let parsed: Value = serde_json::from_str(json_text).unwrap_or(Value::Null);
    parsed
        .get("seekable-ranges")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|r| {
                    let start = r.get("start").and_then(Value::as_f64).unwrap_or(0.0);
                    let end = r.get("end").and_then(Value::as_f64).unwrap_or(0.0);
                    json!({
                        "start": (start * TICKS_PER_SECOND) as i64,
                        "end": (end * TICKS_PER_SECOND) as i64
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;

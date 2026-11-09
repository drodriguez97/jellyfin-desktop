//! The decisions the player makes, separated from the mpv handle so they can
//! be tested: option strings for `loadfile`, the settings-to-mpv mappings,
//! cache budgets and stream selection.

use serde_json::{json, Value};
use std::collections::HashSet;

use super::{AUDIO_CODECS_ALL, AUDIO_CODECS_SPDIF};

const GIB: i64 = 1024 * 1024 * 1024;
/// Most the disk cache may use.
pub const DISK_CACHE_MAX: i64 = 8 * GIB;
/// Free space that is never used for the cache.
pub const DISK_RESERVE: i64 = 4 * GIB;
/// Below this the disk cache is not worth having; fall back to memory.
pub const DISK_CACHE_MIN: i64 = GIB;

/// The per-file options passed to `loadfile`: start position, disabled
/// streams (they are selected in the on_preloaded hook), no video for music,
/// pause state and the user agent the web client asked for.
pub fn loadfile_options(options: &Value, metadata: &Value) -> String {
    let mut extra = String::new();
    let start_ms = options.get("startMilliseconds").and_then(Value::as_i64).unwrap_or(0);
    if start_ms != 0 {
        extra.push_str(&format!("start=+{},", start_ms as f64 / 1000.0));
    }
    extra.push_str("aid=no,sid=no,");
    if metadata.get("type").and_then(Value::as_str) == Some("music") {
        extra.push_str("vid=no,");
    }
    let autoplay = options.get("autoplay").and_then(Value::as_bool).unwrap_or(false);
    extra.push_str(if autoplay { "pause=no," } else { "pause=yes," });
    if let Some(ua) = metadata
        .get("headers")
        .and_then(|h| h.get("User-Agent"))
        .and_then(Value::as_str)
    {
        if !ua.is_empty() {
            extra.push_str(&format!("user-agent={ua},"));
        }
    }
    extra.push_str("ad=,vd=");
    extra
}

/// `hwdec` and `hwdec-image-format` for the `video.hardwareDecoding` setting.
pub fn hwdec_options(setting: &str) -> (&'static str, &'static str) {
    match setting {
        "enabled" => ("auto-safe", "no"),
        "copy" => ("auto-copy-safe", "no"),
        _ => ("no", "no"),
    }
}

/// Codecs to pass through for the audio device type; `enabled` answers whether
/// `audio.passthrough.<codec>` is on. DTS-HD implies DTS, so plain DTS is dropped then.
pub fn passthrough_codecs(device_type: &str, enabled: impl Fn(&str) -> bool) -> Vec<String> {
    if device_type == "basic" {
        return Vec::new();
    }
    let codecs: &[&str] = match device_type {
        "spdif" => &AUDIO_CODECS_SPDIF,
        "hdmi" => &AUDIO_CODECS_ALL,
        _ => &[],
    };
    let mut out: Vec<String> = codecs.iter().filter(|c| enabled(c)).map(|c| c.to_string()).collect();
    if out.iter().any(|c| c == "dts-hd") {
        out.retain(|c| c != "dts");
    }
    out
}

/// S/PDIF carries stereo PCM only; everything else keeps the configured layout.
pub fn channel_layout(device_type: &str, configured: &str) -> String {
    if device_type == "spdif" {
        "2.0".to_string()
    } else {
        configured.to_string()
    }
}

/// AC3 transcoding is only needed for S/PDIF receivers that accept AC3.
pub fn ac3_transcoding(device_type: &str, ac3_passthrough: bool) -> bool {
    device_type == "spdif" && ac3_passthrough
}

/// Disk cache budget in bytes for `free_bytes` of free space, or None when
/// there is not enough room for a useful cache.
pub fn disk_cache_budget(free_bytes: i64) -> Option<i64> {
    let budget = (free_bytes - DISK_RESERVE).min(DISK_CACHE_MAX);
    (budget >= DISK_CACHE_MIN).then_some(budget)
}

/// (`demuxer-max-bytes`, `demuxer-max-back-bytes`) for the memory cache of `cache_mb` MB.
pub fn memory_cache_bytes(cache_mb: i64) -> (i64, i64) {
    let mb = cache_mb.max(10);
    (mb * 1024 * 1024, (mb / 2) * 1024 * 1024)
}

/// (`demuxer-max-bytes`, `demuxer-max-back-bytes`) for a disk cache budget.
pub fn disk_cache_bytes(budget: i64) -> (i64, i64) {
    (budget, budget / 4)
}

#[derive(Debug, Clone, PartialEq)]
pub struct AspectSettings {
    pub unscaled: bool,
    pub force_aspect: String,
    pub keep_aspect: bool,
    pub panscan: f64,
}

/// mpv aspect options for the `video.aspect` setting; None means "custom",
/// where the user's own mpv.conf is left alone. `video_aspect` is the
/// decoded aspect (for the "force 16:9 if 4:3" mode).
pub fn aspect_settings(mode: &str, video_aspect: f64) -> Option<AspectSettings> {
    let mut a = AspectSettings { unscaled: false, force_aspect: "no".into(), keep_aspect: true, panscan: 0.0 };
    match mode {
        "custom" => return None,
        "zoom" => a.panscan = 1.0,
        "force_4_3" => a.force_aspect = "4:3".into(),
        "force_16_9" => a.force_aspect = "16:9".into(),
        "force_16_9_if_4_3" => {
            if (video_aspect - 4.0 / 3.0).abs() < 0.1 {
                a.force_aspect = "16:9".into();
            }
        }
        "stretch" => a.keep_aspect = false,
        "noscaling" => a.unscaled = true,
        _ => {}
    }
    Some(a)
}

/// `key=value` lines of the free-form mpv options setting, quotes stripped,
/// blank and malformed lines skipped.
pub fn parse_other_conf(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let (k, v) = line.split_once('=')?;
            let key = k.trim().trim_matches('"');
            let value = v.trim().trim_matches('"');
            (!key.is_empty() && !value.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// Which `video.audio_delay.*` setting applies at a display refresh rate.
pub fn audio_delay_key(display_fps: f64) -> &'static str {
    if (display_fps - 24.0).abs() < 0.5 {
        "audio_delay.24hz"
    } else if (display_fps - 25.0).abs() < 0.5 {
        "audio_delay.25hz"
    } else if (display_fps - 50.0).abs() < 0.5 {
        "audio_delay.50hz"
    } else {
        "audio_delay.normal"
    }
}

/// `#RRGGBB` + transparency `AA` -> `#AARRGGBB` for `sub-back-color`.
pub fn subtitle_back_color(background: &str, transparency: &str) -> Option<String> {
    if background.is_empty() || transparency.is_empty() {
        return None;
    }
    let mut color = background.to_string();
    color.insert_str(1, transparency);
    Some(color)
}

/// `sub-align-x` and `sub-pos` for a `"<x>,<top|bottom>"` placement.
pub fn subtitle_placement(placement: &str) -> Option<(String, i64)> {
    let parts: Vec<&str> = placement.split(',').collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parts[0].to_string(), if parts[1] == "bottom" { 100 } else { 10 }))
}

/// `sub-scale` for the subtitle size setting (-1 = leave mpv's default).
pub fn subtitle_scale(size: i64) -> Option<f64> {
    (size != -1).then(|| size as f64 / 32.0)
}

/// What the web client asked for when selecting a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamSelection {
    /// Negative index, empty string or null: no track.
    Disable,
    /// A Jellyfin relative index, already 1-based for mpv.
    Index(i64),
    /// The legacy `#<id>` / `#<id>,<url>` / `#,<url>` form.
    External { id: String, name: String },
}

pub fn parse_stream_selection(selection: &Value) -> StreamSelection {
    if let Some(index) = selection.as_i64() {
        return if index < 0 { StreamSelection::Disable } else { StreamSelection::Index(index) };
    }
    let sel = match selection {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if sel.is_empty() {
        return StreamSelection::Disable;
    }
    let (id, name) = match sel.strip_prefix('#') {
        Some(rest) => match rest.find(',') {
            None => (rest.to_string(), String::new()),
            Some(pos) => (rest[..pos].to_string(), rest[pos + 1..].to_string()),
        },
        None => (String::new(), String::new()),
    };
    StreamSelection::External { id, name }
}

/// mpv's track-list entries that belong to `url` (external tracks) or to the
/// file itself (empty `url`).
pub fn tracks_for_url<'a>(tracks: &'a [Value], url: &str) -> Vec<&'a Value> {
    let is_external = !url.is_empty();
    tracks
        .iter()
        .filter(|t| t.get("external").and_then(Value::as_bool).unwrap_or(false) == is_external)
        .filter(|t| !is_external || t.get("external-filename").and_then(Value::as_str) == Some(url))
        .collect()
}

/// The mpv track id to select for an external selection: the explicit id when
/// given, else the id of the matching external track of `type_name`. Audio
/// (and anything with an explicit id) falls back to track 1 instead of "no",
/// because the web client sometimes sends a stale id.
pub fn choose_track(tracks: &[Value], type_name: &str, id: &str, name: &str, is_audio: bool) -> String {
    let mut chosen = String::from("no");
    if !id.is_empty() {
        chosen = id.to_string();
    } else {
        for s in tracks_for_url(tracks, name) {
            if s.get("type").and_then(Value::as_str) != Some(type_name) {
                continue;
            }
            if s.get("external-filename").and_then(Value::as_str) == Some(name) {
                chosen = s.get("id").map(json_to_string).unwrap_or_else(|| "no".into());
                break;
            }
        }
    }
    if (is_audio || !id.is_empty()) && chosen == "no" {
        chosen = "1".into();
    }
    chosen
}

pub fn json_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The audio device choices offered in the settings for mpv's
/// `audio-device-list`, plus the set of device names and whether the user's
/// configured device is among them. A missing configured device stays in the
/// list, marked as disconnected, so the choice is not silently lost.
pub fn audio_device_options(list: &Value, user_device: &str) -> (Vec<Value>, HashSet<String>, bool) {
    let mut found = false;
    let mut devices = HashSet::new();
    let mut options = Vec::new();
    if let Some(list) = list.as_array() {
        for d in list {
            let name = d.get("name").and_then(Value::as_str).unwrap_or("").to_string();
            let desc = d.get("description").and_then(Value::as_str).unwrap_or("").to_string();
            if name == user_device {
                found = true;
            }
            devices.insert(name.clone());
            options.push(json!({ "value": name, "title": desc }));
        }
    }
    if !found {
        options.push(json!({ "value": user_device, "title": format!("[Disconnected device: {user_device}]") }));
    }
    (options, devices, found)
}

/// The device to hand to mpv: the configured one when present, else "auto".
pub fn effective_audio_device(configured: &str, present: &HashSet<String>) -> String {
    if configured.is_empty() || configured == "auto" {
        return "auto".into();
    }
    if present.contains(configured) {
        configured.to_string()
    } else {
        "auto".into()
    }
}

#[cfg(test)]
#[path = "logic_tests.rs"]
mod tests;

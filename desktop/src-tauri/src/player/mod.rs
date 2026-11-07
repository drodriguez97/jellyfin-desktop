//! libmpv player: initialization, settings mapping and stream selection.
//! Port of the Qt `PlayerComponent`.

pub mod commands;
pub mod events;

use crate::settings::{self, Settings};
use anyhow::{anyhow, Result};
use libmpv2::Mpv;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tracing::{debug, info, warn};
use windows_sys::Win32::Foundation::HWND;

pub const AUDIO_CODECS_ALL: [&str; 5] = ["ac3", "dts", "eac3", "dts-hd", "truehd"];
pub const AUDIO_CODECS_SPDIF: [&str; 2] = ["ac3", "dts"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Audio,
    Subtitle,
}

/// Mutable playback bookkeeping mirrored from the Qt component.
#[derive(Debug, Default)]
pub struct Playback {
    pub current_audio_stream: Value,
    pub current_subtitle_stream: Value,
    pub media_frame_rate: f64,
    pub web_playlist: Value,
    pub current_web_playlist_item_id: String,
    pub passthrough_codecs: Vec<String>,
    pub do_ac3_transcoding: bool,
    pub audio_devices: HashSet<String>,
}

pub struct Player {
    pub mpv: Arc<Mpv>,
    pub app: AppHandle,
    pub settings: Arc<Mutex<Settings>>,
    pub playback: Mutex<Playback>,
    pub stream_switch_imminent: AtomicBool,
    pub audio_delay_ms: AtomicI64,
    pub audio_reload_pending: AtomicBool,
}

impl Player {
    /// Creates the mpv instance rendering into `wid` and applies the user's settings.
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>, wid: HWND) -> Result<Arc<Self>> {
        let verbose = std::env::var_os("JF_MPV_VERBOSE").is_some();
        let mpv_conf = {
            let s = settings.lock();
            s.conf_path().parent().map(|p| p.join("mpv.conf"))
        };
        let mpv = Mpv::with_initializer(|init| {
            // These two are required for embedding; everything else is best effort
            // (options come and go between mpv releases).
            init.set_option("wid", wid as i64)?;
            init.set_option("vo", "gpu-next,gpu")?;
            let opt = |name: &str, value: &str| {
                if let Err(e) = init.set_option(name, value) {
                    warn!("mpv option {name}={value} rejected: {e}");
                }
            };
            opt("gpu-context", "d3d11");
            opt("force-window", "yes");
            opt("idle", "yes");
            opt("keep-open", "no");
            opt("input-default-bindings", "no");
            opt("input-vo-keyboard", "no");
            opt("input-cursor", "no");
            opt("osc", "no");
            opt("osd-level", "0");
            opt("msg-level", if verbose { "all=v" } else { "all=warn" });
            opt("audio-client-name", "Jellyfin Desktop");
            opt("title", "Jellyfin Desktop");
            opt("target-colorspace-hint", "auto");
            // Jellyfin needs unrebased timestamps for mkv transcodes.
            opt("demuxer-mkv-probe-start-time", "no");
            opt("demuxer-lavf-probe-info", "yes");
            opt("audio-fallback-to-null", "yes");
            opt("ad-lavc-downmix", "no");
            opt("ytdl", "no");
            // Windows: no system CA bundle for mpv's TLS; use the built-in defaults.
            opt("tls-ca-file", "");
            if let Some(conf) = &mpv_conf {
                if conf.exists() {
                    if let Some(path) = conf.to_str() {
                        info!("loading user mpv.conf from {path}");
                        let _ = init.load_config(path);
                    }
                }
            }
            Ok(())
        })
        .map_err(|e| anyhow!("mpv init failed: {e}"))?;

        if settings.lock().bool(settings::SECTION_MAIN, "ignoreSSLErrors") {
            let _ = mpv.set_property("tls-verify", "no");
        }

        let player = Arc::new(Self {
            mpv: Arc::new(mpv),
            app,
            settings,
            playback: Mutex::new(Playback::default()),
            stream_switch_imminent: AtomicBool::new(false),
            audio_delay_ms: AtomicI64::new(0),
            audio_reload_pending: AtomicBool::new(false),
        });

        events::start(player.clone())?;

        player.update_audio_device_list();
        player.set_audio_configuration();
        player.set_video_configuration();
        player.set_subtitle_configuration();
        player.set_other_configuration();
        Ok(player)
    }

    fn set<T: libmpv2::SetData + std::fmt::Debug + Clone>(&self, name: &str, value: T) {
        if let Err(e) = self.mpv.set_property(name, value.clone()) {
            debug!("mpv set {name}={value:?} failed: {e}");
        }
    }

    pub fn command(&self, name: &str, args: &[&str]) {
        if let Err(e) = self.mpv.command(name, args) {
            warn!("mpv command {name} {args:?} failed: {e}");
        }
    }

    pub fn get_string(&self, name: &str) -> Option<String> {
        self.mpv.get_property::<String>(name).ok()
    }
    pub fn get_json(&self, name: &str) -> Value {
        self.get_string(name)
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null)
    }
    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.mpv.get_property::<f64>(name).ok()
    }
    pub fn get_bool(&self, name: &str) -> bool {
        self.mpv.get_property::<bool>(name).unwrap_or(false)
    }

    fn emit<S: serde::Serialize + Clone>(&self, signal: &str, payload: S) {
        let _ = self.app.emit(&format!("player:{signal}"), payload);
    }

    // ---- playback -----------------------------------------------------------

    pub fn load(&self, url: &str, options: &Value, metadata: &Value, audio: Value, subtitle: Value) {
        self.stop();
        self.queue_media(url, options, metadata, audio, subtitle);
    }

    pub fn queue_media(&self, url: &str, options: &Value, metadata: &Value, audio: Value, subtitle: Value) {
        {
            let mut pb = self.playback.lock();
            pb.media_frame_rate = metadata.get("frameRate").and_then(Value::as_f64).unwrap_or(0.0);
            pb.current_audio_stream = audio;
            pb.current_subtitle_stream = subtitle;
        }
        self.update_video_configuration();

        let mut extra = String::new();
        let start_ms = options.get("startMilliseconds").and_then(Value::as_i64).unwrap_or(0);
        if start_ms != 0 {
            extra.push_str(&format!("start=+{},", start_ms as f64 / 1000.0));
        }
        // Streams are selected in the on_preloaded hook.
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

        self.command("loadfile", &[url, "append-play", "-1", &extra]);

        let jf_meta = metadata.get("metadata").cloned().unwrap_or(Value::Null);
        let base_url = base_url_of(url);
        self.emit("onMetaData", (jf_meta.clone(), base_url.clone()));
        if jf_meta.is_object() {
            crate::artwork::request(self.app.clone(), jf_meta, url.to_string(), base_url);
        }
    }

    pub fn play(&self) {
        self.set("pause", false);
    }
    pub fn pause(&self) {
        self.set("pause", true);
    }
    pub fn stop(&self) {
        self.command("stop", &[]);
    }
    pub fn clear_queue(&self) {
        self.command("playlist-clear", &[]);
    }
    pub fn seek_to(&self, ms: i64) {
        let secs = ms as f64 / 1000.0;
        self.command("seek", &[&secs.to_string(), "absolute+exact"]);
    }
    pub fn set_volume(&self, volume: i64) {
        self.set("volume", volume as f64);
    }
    pub fn volume(&self) -> i64 {
        self.get_f64("volume").unwrap_or(0.0) as i64
    }
    pub fn set_muted(&self, muted: bool) {
        self.set("mute", muted);
    }
    pub fn muted(&self) -> bool {
        self.get_bool("mute")
    }
    pub fn set_playback_rate(&self, rate_thousandths: i64) {
        let speed = rate_thousandths as f64 / 1000.0;
        self.set("speed", speed);
        self.emit("playbackRateChanged", (speed,));
    }
    pub fn position_ms(&self) -> i64 {
        (self.get_f64("playback-time").unwrap_or(0.0) * 1000.0) as i64
    }
    pub fn duration_ms(&self) -> i64 {
        (self.get_f64("duration").unwrap_or(0.0) * 1000.0) as i64
    }
    pub fn set_subtitle_delay(&self, ms: i64) {
        self.set("sub-delay", ms as f64 / 1000.0);
    }
    pub fn user_command(&self, command: &str) {
        let ptr = self.mpv.ctx.as_ptr();
        if let Ok(c) = std::ffi::CString::new(command) {
            unsafe {
                libmpv2_sys::mpv_command_string(ptr, c.as_ptr());
            }
        }
    }

    // ---- stream selection --------------------------------------------------

    pub fn set_audio_stream(&self, stream: Value) {
        self.playback.lock().current_audio_stream = stream.clone();
        self.reselect_stream(&stream, MediaType::Audio);
    }
    pub fn set_subtitle_stream(&self, stream: Value) {
        self.playback.lock().current_subtitle_stream = stream.clone();
        self.reselect_stream(&stream, MediaType::Subtitle);
    }

    fn find_streams_for_url(&self, url: &str) -> Vec<Value> {
        let is_external = !url.is_empty();
        self.get_json("track-list")
            .as_array()
            .map(|tracks| {
                tracks
                    .iter()
                    .filter(|t| t.get("external").and_then(Value::as_bool).unwrap_or(false) == is_external)
                    .filter(|t| {
                        !is_external || t.get("external-filename").and_then(Value::as_str) == Some(url)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Selects a track from either a Jellyfin relative index (integer, already
    /// 1-based for mpv; negative disables) or the legacy "#id" / "#,url" string.
    pub fn reselect_stream(&self, selection: &Value, target: MediaType) {
        let (type_name, id_prop, add_cmd) = match target {
            MediaType::Subtitle => ("sub", "sid", "sub-add"),
            MediaType::Audio => ("audio", "aid", "audio-add"),
        };

        if let Some(index) = selection.as_i64() {
            if index < 0 {
                self.set(id_prop, "no");
            } else {
                self.set(id_prop, index);
            }
            return;
        }

        let sel = match selection {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        if sel.is_empty() {
            self.set(id_prop, "no");
            return;
        }

        let mut stream_id = String::new();
        let mut stream_name = String::new();
        if let Some(rest) = sel.strip_prefix('#') {
            match rest.find(',') {
                None => stream_id = rest.to_string(),
                Some(pos) => {
                    stream_id = rest[..pos].to_string();
                    stream_name = rest[pos + 1..].to_string();
                }
            }
        }

        if !stream_name.is_empty() && self.find_streams_for_url(&stream_name).is_empty() {
            self.command(add_cmd, &[&stream_name]);
        }

        let mut chosen = String::from("no");
        if !stream_id.is_empty() {
            chosen = stream_id.clone();
        } else {
            for s in self.find_streams_for_url(&stream_name) {
                if s.get("type").and_then(Value::as_str) != Some(type_name) {
                    continue;
                }
                if s.get("external-filename").and_then(Value::as_str) == Some(stream_name.as_str()) {
                    chosen = s.get("id").map(json_to_string).unwrap_or_else(|| "no".into());
                    break;
                }
            }
        }
        // Fall back to the first stream when the web client used a wrong id.
        if (target == MediaType::Audio || !stream_id.is_empty()) && chosen == "no" {
            chosen = "1".into();
        }
        self.set(id_prop, chosen.as_str());
    }

    // ---- configuration ---------------------------------------------------

    pub fn update_audio_configuration(&self) {
        self.set_audio_configuration();
        self.set_other_configuration();
    }
    pub fn update_video_configuration(&self) {
        self.set_video_configuration();
        self.set_other_configuration();
    }
    pub fn update_subtitle_configuration(&self) {
        self.set_subtitle_configuration();
        self.set_other_configuration();
    }
    pub fn update_configuration(&self) {
        self.set_audio_configuration();
        self.set_video_configuration();
        self.set_subtitle_configuration();
        self.set_other_configuration();
    }

    pub fn set_audio_configuration(&self) {
        let s = self.settings.lock();
        let device_type = s.string(settings::SECTION_AUDIO, "devicetype");
        self.set("audio-exclusive", s.bool(settings::SECTION_AUDIO, "exclusive"));
        drop(s);
        self.update_audio_device();
        let s = self.settings.lock();

        let normalize = s.bool(settings::SECTION_AUDIO, "normalize");
        self.set("audio-normalize-downmix", if normalize { "yes" } else { "no" });
        self.set("audio-swresample-o", "surround_mix_level=1");

        let mut passthrough: Vec<String> = Vec::new();
        if device_type != "basic" {
            let codecs: &[&str] = match device_type.as_str() {
                "spdif" => &AUDIO_CODECS_SPDIF,
                "hdmi" => &AUDIO_CODECS_ALL,
                _ => &[],
            };
            for codec in codecs {
                if s.bool(settings::SECTION_AUDIO, &format!("passthrough.{codec}")) {
                    passthrough.push(codec.to_string());
                }
            }
            if passthrough.iter().any(|c| c == "dts-hd") {
                passthrough.retain(|c| c != "dts");
            }
        }
        let passthrough_str = passthrough.join(",");
        self.set("audio-spdif", passthrough_str.as_str());

        let mut layout = s.string(settings::SECTION_AUDIO, "channels");
        if device_type == "spdif" {
            layout = "2.0".into();
        }
        self.set("audio-channels", layout.as_str());

        let do_ac3 = device_type == "spdif" && s.bool(settings::SECTION_AUDIO, "passthrough.ac3");
        let device = s.string(settings::SECTION_AUDIO, "device");
        drop(s);

        let was_ac3 = {
            let mut pb = self.playback.lock();
            let was = pb.do_ac3_transcoding;
            pb.do_ac3_transcoding = do_ac3;
            pb.passthrough_codecs = passthrough.clone();
            was
        };
        if do_ac3 && !was_ac3 {
            self.command("af", &["add", "@ac3:lavcac3enc"]);
        } else if !do_ac3 && was_ac3 {
            self.command("af", &["remove", "@ac3"]);
        }

        info!(
            "Audio config - device: {device}, channel layout: {layout}, passthrough codecs: {}, ac3 transcoding: {}",
            if passthrough_str.is_empty() { "none" } else { &passthrough_str },
            if do_ac3 { "yes" } else { "no" }
        );
    }

    pub fn set_video_configuration(&self) {
        let s = self.settings.lock();
        let sync_mode = s.string(settings::SECTION_VIDEO, "sync_mode");
        self.set("video-sync", sync_mode.as_str());

        let (hwdec, image_format) = match s.string(settings::SECTION_VIDEO, "hardwareDecoding").as_str() {
            "enabled" => ("auto-safe", "no"),
            "copy" => ("auto-copy-safe", "no"),
            _ => ("no", "no"),
        };
        self.set("hwdec", hwdec);
        self.set("hwdec-image-format", image_format);

        let deinterlace = s.bool(settings::SECTION_VIDEO, "deinterlace");
        self.set("deinterlace", if deinterlace { "yes" } else { "no" });

        let cache_mb = s.i64(settings::SECTION_VIDEO, "cache");
        self.set("demuxer-max-bytes", cache_mb * 1024 * 1024);
        drop(s);

        self.set_audio_delay(self.audio_delay_ms.load(Ordering::Relaxed));
        self.update_video_aspect_settings();
    }

    pub fn update_video_aspect_settings(&self) {
        let mode = self.settings.lock().string(settings::SECTION_VIDEO, "aspect");
        let mut unscaled = false;
        let mut keep_aspect = true;
        let mut force_aspect = "no".to_string();
        let mut panscan = 0.0;
        match mode.as_str() {
            "custom" => return,
            "zoom" => panscan = 1.0,
            "force_4_3" => force_aspect = "4:3".into(),
            "force_16_9" => force_aspect = "16:9".into(),
            "force_16_9_if_4_3" => {
                let aspect = self
                    .get_json("video-dec-params")
                    .get("aspect")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                if (aspect - 4.0 / 3.0).abs() < 0.1 {
                    force_aspect = "16:9".into();
                }
            }
            "stretch" => keep_aspect = false,
            "noscaling" => unscaled = true,
            _ => {}
        }
        self.set("video-unscaled", unscaled);
        self.set("video-aspect-override", force_aspect.as_str());
        self.set("keepaspect", keep_aspect);
        self.set("panscan", panscan);
    }

    pub fn set_subtitle_configuration(&self) {
        let s = self.settings.lock();
        let sec = settings::SECTION_SUBTITLES;
        let scale_border = s.bool(sec, "ass_scale_border_and_shadow");
        self.set(
            "sub-ass-style-overrides",
            if scale_border { "ScaledBorderAndShadow=yes" } else { "ScaledBorderAndShadow=no" },
        );
        let override_mode = s.string(sec, "ass_style_override");
        if !override_mode.is_empty() {
            self.set("sub-ass-override", override_mode.as_str());
        }
        let size = s.i64(sec, "size");
        if size != -1 {
            self.set("sub-scale", size as f64 / 32.0);
        }
        let font = s.string(sec, "font");
        if !font.is_empty() {
            self.set("sub-font", font.as_str());
        }
        let color = s.string(sec, "color");
        if !color.is_empty() {
            self.set("sub-color", color.as_str());
        }
        let border_color = s.string(sec, "border_color");
        if !border_color.is_empty() {
            self.set("sub-border-color", border_color.as_str());
        }
        let border_size = s.i64(sec, "border_size");
        if border_size != -1 {
            self.set("sub-border-size", border_size);
        }
        let bg = s.string(sec, "background_color");
        let bg_alpha = s.string(sec, "background_transparency");
        if !bg.is_empty() && !bg_alpha.is_empty() {
            let mut color = bg.clone();
            color.insert_str(1, &bg_alpha);
            self.set("sub-back-color", color.as_str());
        }
        let placement = s.string(sec, "placement");
        let parts: Vec<&str> = placement.split(',').collect();
        if parts.len() == 2 {
            self.set("sub-align-x", parts[0]);
            self.set("sub-pos", if parts[1] == "bottom" { 100_i64 } else { 10_i64 });
        }
    }

    pub fn set_other_configuration(&self) {
        let other = self.settings.lock().string(settings::SECTION_OTHER, "other_conf");
        for line in other.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim().trim_matches('"');
                let value = v.trim().trim_matches('"');
                if !key.is_empty() && !value.is_empty() {
                    self.set(key, value);
                }
            }
        }
    }

    pub fn set_audio_delay(&self, ms: i64) {
        self.audio_delay_ms.store(ms, Ordering::Relaxed);
        let fixed = self.settings.lock().f64(settings::SECTION_VIDEO, "audio_delay.normal");
        self.set("audio-delay", (fixed + ms as f64) / 1000.0);
    }

    // ---- audio devices -----------------------------------------------------

    pub fn audio_device_list(&self) -> Value {
        self.get_json("audio-device-list")
    }

    pub fn update_audio_device_list(&self) {
        let user_device = self.settings.lock().string(settings::SECTION_AUDIO, "device");
        let mut found = false;
        let mut devices = HashSet::new();
        let mut options = Vec::new();
        if let Some(list) = self.audio_device_list().as_array() {
            for d in list {
                let name = d.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let desc = d.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                if name == user_device {
                    found = true;
                }
                devices.insert(name.clone());
                options.push(serde_json::json!({ "value": name, "title": desc }));
            }
        }
        if !found {
            options.push(serde_json::json!({
                "value": user_device,
                "title": format!("[Disconnected device: {user_device}]")
            }));
        }
        let updates = self
            .settings
            .lock()
            .update_possible_values(settings::SECTION_AUDIO, "device", options);
        settings::commands::emit_updates(&self.app, &updates);

        let old = std::mem::replace(&mut self.playback.lock().audio_devices, devices.clone());
        if !user_device.is_empty() {
            let changed = old.contains(&user_device) != devices.contains(&user_device);
            if changed && !self.audio_reload_pending.swap(true, Ordering::SeqCst) {
                // Debounce: the device may need a moment before it can be opened.
                let app = self.app.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(p) = commands::player_from(&app) {
                        p.audio_reload_pending.store(false, Ordering::SeqCst);
                        p.update_audio_device();
                    }
                });
            }
        }
    }

    pub fn update_audio_device(&self) {
        let mut device = self.settings.lock().string(settings::SECTION_AUDIO, "device");
        if device.is_empty() {
            device = "auto".into();
        }
        if device != "auto" && !self.playback.lock().audio_devices.contains(&device) {
            warn!("Not using audio device {device} because it's not present.");
            device = "auto".into();
        }
        self.set("audio-device", device.as_str());
    }

    pub fn set_audio_device(&self, name: &str) {
        self.set("audio-device", name);
    }

    // ---- debug info --------------------------------------------------------

    pub fn video_information(&self) -> String {
        if self.get_bool("idle-active") {
            return String::new();
        }
        let p = |name: &str| self.get_string(name).unwrap_or_else(|| "-".into());
        format!(
            "File:\nURL: {}\nContainer: {}\n\nVideo:\nCodec: {}\nSize: {}x{}\nFPS: {}\nHardware Decoding: {} ({})\n\nAudio:\nCodec: {}\nOutput driver: {}\n\nPerformance:\nA/V: {}\nDropped frames: {}\n\nCache:\nSeconds: {}\nBuffering: {}\nSpeed: {}\n",
            p("path"),
            p("file-format"),
            p("video-codec"),
            p("video-params/dw"),
            p("video-params/dh"),
            p("container-fps"),
            p("hwdec-current"),
            p("hwdec-interop"),
            p("audio-codec"),
            p("current-ao"),
            p("avsync"),
            p("vo-drop-frame-count"),
            p("demuxer-cache-duration"),
            p("cache-buffering-state"),
            p("cache-speed"),
        )
    }
}

fn json_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// scheme://host[:port] of a URL, without path or query.
fn base_url_of(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some(x) => x,
        None => return String::new(),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    format!("{scheme}://{host}")
}

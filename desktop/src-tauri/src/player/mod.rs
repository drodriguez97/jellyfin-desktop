//! libmpv player: initialization, settings mapping and stream selection.
//! Port of the Qt `PlayerComponent`.

pub mod commands;
pub mod events;
pub mod logic;
pub mod state;

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
    /// Top-level window handle (for display lookups).
    pub window_hwnd: std::sync::atomic::AtomicIsize,
}

impl Player {
    /// Creates the mpv instance rendering into `wid` and applies the user's settings.
    pub fn new(app: AppHandle, settings: Arc<Mutex<Settings>>, wid: HWND, window: HWND) -> Result<Arc<Self>> {
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
            window_hwnd: std::sync::atomic::AtomicIsize::new(window as isize),
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

        let extra = logic::loadfile_options(options, metadata);

        self.command("loadfile", &[url, "append-play", "-1", &extra]);

        let jf_meta = metadata.get("metadata").cloned().unwrap_or(Value::Null);
        let base_url = crate::urls::base_url_of(url);
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
        let tracks = self.get_json("track-list").as_array().cloned().unwrap_or_default();
        logic::tracks_for_url(&tracks, url).into_iter().cloned().collect()
    }

    /// Selects a track from either a Jellyfin relative index (integer, already
    /// 1-based for mpv; negative disables) or the legacy "#id" / "#,url" string.
    pub fn reselect_stream(&self, selection: &Value, target: MediaType) {
        let (type_name, id_prop, add_cmd) = match target {
            MediaType::Subtitle => ("sub", "sid", "sub-add"),
            MediaType::Audio => ("audio", "aid", "audio-add"),
        };
        match logic::parse_stream_selection(selection) {
            logic::StreamSelection::Disable => self.set(id_prop, "no"),
            logic::StreamSelection::Index(index) => self.set(id_prop, index),
            logic::StreamSelection::External { id, name } => {
                if !name.is_empty() && self.find_streams_for_url(&name).is_empty() {
                    self.command(add_cmd, &[&name]);
                }
                let tracks = self.get_json("track-list").as_array().cloned().unwrap_or_default();
                let chosen = logic::choose_track(&tracks, type_name, &id, &name, target == MediaType::Audio);
                self.set(id_prop, chosen.as_str());
            }
        }
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

        let passthrough =
            logic::passthrough_codecs(&device_type, |codec| s.bool(settings::SECTION_AUDIO, &format!("passthrough.{codec}")));
        let passthrough_str = passthrough.join(",");
        self.set("audio-spdif", passthrough_str.as_str());

        let layout = logic::channel_layout(&device_type, &s.string(settings::SECTION_AUDIO, "channels"));
        self.set("audio-channels", layout.as_str());

        let do_ac3 = logic::ac3_transcoding(&device_type, s.bool(settings::SECTION_AUDIO, "passthrough.ac3"));
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

        let (hwdec, image_format) = logic::hwdec_options(&s.string(settings::SECTION_VIDEO, "hardwareDecoding"));
        self.set("hwdec", hwdec);
        self.set("hwdec-image-format", image_format);

        let deinterlace = s.bool(settings::SECTION_VIDEO, "deinterlace");
        self.set("deinterlace", if deinterlace { "yes" } else { "no" });

        let cache_mb = s.i64(settings::SECTION_VIDEO, "cache");
        let cache_on_disk = s.bool(settings::SECTION_VIDEO, "cache_on_disk");
        let cache_dir = s.conf_path().parent().map(|p| p.join("mpv-cache"));
        drop(s);
        // Disk-backed cache: keep (almost) the whole file ahead and a long tail
        // behind so skips in both directions stay local. The budget is capped by
        // the free space on the drive, with a reserve left untouched; when there
        // is not enough room for a useful cache, fall back to the memory cache.
        let disk_budget = if cache_on_disk {
            cache_dir.as_ref().and_then(|dir| {
                std::fs::create_dir_all(dir).ok()?;
                let free = free_disk_space(dir)?;
                match logic::disk_cache_budget(free) {
                    Some(budget) => Some((dir.clone(), budget)),
                    None => {
                        warn!("disk cache disabled: only {} MB free on {}", free / (1024 * 1024), dir.display());
                        None
                    }
                }
            })
        } else {
            None
        };
        match disk_budget {
            Some((dir, budget)) => {
                if let Some(d) = dir.to_str() {
                    self.set("cache-dir", d);
                }
                self.set("cache-on-disk", "yes");
                let (max, back) = logic::disk_cache_bytes(budget);
                self.set("demuxer-max-bytes", max);
                self.set("demuxer-max-back-bytes", back);
                info!("disk cache enabled, budget {} MB", budget / (1024 * 1024));
            }
            None => {
                self.set("cache-on-disk", "no");
                let (max, back) = logic::memory_cache_bytes(cache_mb);
                self.set("demuxer-max-bytes", max);
                self.set("demuxer-max-back-bytes", back);
            }
        }

        let display_fps = crate::display::current_refresh_rate(self.window_hwnd());
        if display_fps > 0.0 {
            self.set("display-fps-override", display_fps);
        }

        self.set_audio_delay(self.audio_delay_ms.load(Ordering::Relaxed));
        self.update_video_aspect_settings();
    }

    pub fn update_video_aspect_settings(&self) {
        let mode = self.settings.lock().string(settings::SECTION_VIDEO, "aspect");
        let video_aspect = self
            .get_json("video-dec-params")
            .get("aspect")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let Some(a) = logic::aspect_settings(&mode, video_aspect) else { return };
        self.set("video-unscaled", a.unscaled);
        self.set("video-aspect-override", a.force_aspect.as_str());
        self.set("keepaspect", a.keep_aspect);
        self.set("panscan", a.panscan);
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
        if let Some(scale) = logic::subtitle_scale(s.i64(sec, "size")) {
            self.set("sub-scale", scale);
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
        if let Some(color) =
            logic::subtitle_back_color(&s.string(sec, "background_color"), &s.string(sec, "background_transparency"))
        {
            self.set("sub-back-color", color.as_str());
        }
        if let Some((align, pos)) = logic::subtitle_placement(&s.string(sec, "placement")) {
            self.set("sub-align-x", align.as_str());
            self.set("sub-pos", pos);
        }
    }

    pub fn set_other_configuration(&self) {
        let other = self.settings.lock().string(settings::SECTION_OTHER, "other_conf");
        for (key, value) in logic::parse_other_conf(&other) {
            self.set(key.as_str(), value.as_str());
        }
    }

    pub fn set_audio_delay(&self, ms: i64) {
        self.audio_delay_ms.store(ms, Ordering::Relaxed);
        let display_fps = crate::display::current_refresh_rate(self.window_hwnd());
        let key = logic::audio_delay_key(display_fps);
        let fixed = self.settings.lock().f64(settings::SECTION_VIDEO, key);
        self.set("audio-delay", (fixed + ms as f64) / 1000.0);
    }

    fn window_hwnd(&self) -> HWND {
        self.window_hwnd.load(Ordering::Relaxed) as HWND
    }

    /// Port of `PlayerComponent::switchDisplayFrameRate`.
    pub fn switch_display_frame_rate(&self) -> bool {
        let (auto_switch, avoid, fullscreen) = {
            let s = self.settings.lock();
            (
                s.bool(settings::SECTION_VIDEO, "refreshrate.auto_switch"),
                s.bool(settings::SECTION_VIDEO, "refreshrate.avoid_25hz_30hz"),
                s.bool(settings::SECTION_MAIN, "fullscreen"),
            )
        };
        let fps = self.playback.lock().media_frame_rate;
        debug!("Video framerate: {fps} fps");
        if !auto_switch {
            return false;
        }
        if !fullscreen && !crate::window::is_fullscreen(&self.app) {
            debug!("Not switching refresh-rate (not in fullscreen mode).");
            return false;
        }
        if fps < 1.0 {
            debug!("Not switching refresh-rate (no known video framerate).");
            return false;
        }
        crate::display::switch_to_best_mode(self.window_hwnd(), fps as f32, avoid)
    }

    // ---- audio devices -----------------------------------------------------

    pub fn audio_device_list(&self) -> Value {
        self.get_json("audio-device-list")
    }

    pub fn update_audio_device_list(&self) {
        let user_device = self.settings.lock().string(settings::SECTION_AUDIO, "device");
        let (options, devices, _found) = logic::audio_device_options(&self.audio_device_list(), &user_device);
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
        let configured = self.settings.lock().string(settings::SECTION_AUDIO, "device");
        let device = logic::effective_audio_device(&configured, &self.playback.lock().audio_devices);
        if device == "auto" && configured != "auto" && !configured.is_empty() {
            warn!("Not using audio device {configured} because it's not present.");
        }
        self.set("audio-device", device.as_str());
    }

    pub fn set_audio_device(&self, name: &str) {
        self.set("audio-device", name);
    }

    // ---- debug info --------------------------------------------------------

    /// One-line snapshot of why playback may be stalling.
    pub fn stall_diagnostics(&self) -> String {
        let p = |name: &str| self.get_string(name).unwrap_or_else(|| "-".into());
        format!(
            "seeking={} paused-for-cache={} cache-speed={}B/s cache-duration={}s buffering={}% hwdec={} vo-dropped={} decoder-dropped={} fps={} pos={}",
            p("seeking"),
            p("paused-for-cache"),
            p("cache-speed"),
            p("demuxer-cache-duration"),
            p("cache-buffering-state"),
            p("hwdec-current"),
            p("frame-drop-count"),
            p("decoder-frame-drop-count"),
            p("estimated-vf-fps"),
            p("playback-time"),
        )
    }

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

/// Free bytes available to the current user on the volume holding `path`.
fn free_disk_space(path: &std::path::Path) -> Option<i64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free_to_caller: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        warn!("GetDiskFreeSpaceExW failed for {}", path.display());
        return None;
    }
    Some(free_to_caller.min(i64::MAX as u64) as i64)
}

//! Builds the JavaScript injected into every page: `jmpInfo` (client info and
//! settings snapshot), the `window.api` shim, the jellyfin-web plugins and
//! `nativeshell.js`. Port of `SystemComponent::getNativeShellScript`.

use crate::settings::{self, Settings};
use base64::Engine;
use serde_json::json;

const API_JS: &str = include_str!("../../web/bridge/api.js");
const HOST_KEYS_JS: &str = include_str!("../../web/bridge/hostKeys.js");
const THEME_JS: &str = include_str!("../../web/bridge/theme.js");
const LAZY_IMAGES_JS: &str = include_str!("../../web/bridge/lazyImages.js");
const MPV_VIDEO_PLAYER_JS: &str = include_str!("../../web/bridge/mpvVideoPlayer.js");
const MPV_AUDIO_PLAYER_JS: &str = include_str!("../../web/bridge/mpvAudioPlayer.js");
const INPUT_PLUGIN_JS: &str = include_str!("../../web/bridge/inputPlugin.js");
const UPDATE_PLUGIN_JS: &str = include_str!("../../web/bridge/updatePlugin.js");
const CONNECTIVITY_HELPER_JS: &str = include_str!("../../web/bridge/connectivityHelper.js");
const NATIVESHELL_JS: &str = include_str!("../../web/bridge/nativeshell.js");

/// `app_origin` is where the bundled pages live (e.g. `http://tauri.localhost`).
pub fn build_init_script(settings: &Settings, app_origin: &str, user_agent: &str) -> String {
    let client_data = json!({
        "deviceName": settings.client_name(),
        "version": env!("CARGO_PKG_VERSION"),
        "userAgent": user_agent,
        "scriptPath": app_origin,
        "mode": settings.string(settings::SECTION_MAIN, "layout"),
        "sections": settings.ordered_sections(),
        "settingsDescriptions": settings.setting_descriptions_map(),
        "settings": settings.all_values(""),
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(client_data.to_string());

    let mut script = String::with_capacity(256 * 1024);
    script.push_str("const jmpInfo = JSON.parse(window.atob(\"");
    script.push_str(&encoded);
    script.push_str("\"));\nwindow.jmpInfo = jmpInfo;\n");
    for part in [
        API_JS,
        HOST_KEYS_JS,
        THEME_JS,
        LAZY_IMAGES_JS,
        MPV_VIDEO_PLAYER_JS,
        MPV_AUDIO_PLAYER_JS,
        INPUT_PLUGIN_JS,
        UPDATE_PLUGIN_JS,
        CONNECTIVITY_HELPER_JS,
        NATIVESHELL_JS,
    ] {
        script.push_str(part);
        script.push('\n');
    }
    script
}

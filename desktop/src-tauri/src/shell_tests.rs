use super::*;
use base64::Engine;
use serde_json::Value;

const DESCRIPTION: &str = include_str!("../settings_description.json");

#[test]
fn init_script_embeds_the_client_info_and_every_bridge_script() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::load(DESCRIPTION, dir.path()).unwrap();
    settings.set_value_no_save(settings::SECTION_MAIN, "layout", serde_json::json!("tv"));
    settings.set_value_no_save(settings::SECTION_SYSTEM, "systemname", serde_json::json!("Den"));

    let script = build_init_script(&settings, "http://tauri.localhost", "JellyfinDesktop/2.0.0 Test");
    let prefix = "const jmpInfo = JSON.parse(window.atob(\"";
    assert!(script.starts_with(prefix));
    let encoded_end = script[prefix.len()..].find('"').unwrap();
    let encoded = &script[prefix.len()..prefix.len() + encoded_end];
    let decoded = base64::engine::general_purpose::STANDARD.decode(encoded).unwrap();
    let info: Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(info["deviceName"], "Den");
    assert_eq!(info["mode"], "tv");
    assert_eq!(info["userAgent"], "JellyfinDesktop/2.0.0 Test");
    assert_eq!(info["scriptPath"], "http://tauri.localhost");
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(info["settings"]["main"]["layout"], "tv");
    assert!(info["settingsDescriptions"]["video"].is_array());
    assert!(info["sections"].as_array().unwrap().iter().any(|s| s["key"] == "main"));

    assert!(script.contains("window.jmpInfo = jmpInfo;"));
    for part in [API_JS, HOST_KEYS_JS, THEME_JS, LAZY_IMAGES_JS, MPV_VIDEO_PLAYER_JS, MPV_AUDIO_PLAYER_JS, INPUT_PLUGIN_JS, CONNECTIVITY_HELPER_JS, NATIVESHELL_JS] {
        assert!(script.contains(part));
    }
    // Scripts are separated so a missing trailing newline cannot merge two files.
    assert!(script.ends_with('\n'));
}

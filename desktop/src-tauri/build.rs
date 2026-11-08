use std::path::PathBuf;

/// Every `#[tauri::command]` the app exposes. Tauri generates
/// `allow-<name>` / `deny-<name>` permissions from this list, which the
/// capability files under `capabilities/` reference.
pub const COMMANDS: &[&str] = &[
    // player
    "player_load",
    "player_queue_media",
    "player_clear_queue",
    "player_play",
    "player_pause",
    "player_stop",
    "player_seek_to",
    "player_stream_switch",
    "player_get_position",
    "player_get_duration",
    "player_set_volume",
    "player_volume",
    "player_set_muted",
    "player_muted",
    "player_set_playback_rate",
    "player_set_audio_stream",
    "player_set_subtitle_stream",
    "player_set_audio_delay",
    "player_set_subtitle_delay",
    "player_set_video_rectangle",
    "player_set_video_only_mode",
    "player_get_audio_device_list",
    "player_set_audio_device",
    "player_user_command",
    "player_get_property",
    "player_set_web_playlist",
    "player_get_web_playlist",
    "player_get_current_web_playlist_item_id",
    "player_notify_shuffle_change",
    "player_notify_repeat_change",
    "player_notify_fullscreen_change",
    "player_notify_rate_change",
    "player_notify_queue_change",
    "player_notify_playback_stop",
    "player_notify_duration_change",
    "player_notify_playback_state",
    "player_notify_position",
    "player_notify_seek",
    "player_notify_metadata",
    "player_notify_volume_change",
    // settings
    "settings_set_value",
    "settings_set_values",
    "settings_value",
    "settings_all_values",
    "settings_remove_value",
    "settings_reset_to_default",
    "settings_descriptions",
    "settings_ordered_sections",
    // system
    "system_exit",
    "system_restart",
    "system_hello",
    "system_open_external_url",
    "system_check_server_connectivity",
    "system_cancel_server_connectivity",
    "system_get_capabilities_string",
    "system_network_addresses",
    "system_run_user_script",
    "system_fetch_page_for_csp_workaround",
    // input
    "input_execute_actions",
    "input_set_volume",
    "input_seek_to",
    "input_set_rate",
    // window
    "window_set_fullscreen",
    "window_is_fullscreen",
    "window_toggle_fullscreen",
    "window_set_always_on_top",
    "window_is_always_on_top",
    "window_toggle_always_on_top",
    "window_set_cursor_visibility",
    "window_raise",
];

fn main() {
    // libmpv2-sys emits `cargo:rustc-link-lib=mpv`; point the linker at the
    // import library produced by desktop/scripts/fetch-mpv.ps1.
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mpv_dir = std::env::var("MPV_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| manifest_dir.join("mpv"));
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    println!("cargo:rerun-if-changed={}", mpv_dir.join("mpv.lib").display());
    if !mpv_dir.join("mpv.lib").exists() {
        println!(
            "cargo:warning=mpv.lib not found in {}; run desktop/scripts/fetch-mpv.ps1",
            mpv_dir.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", mpv_dir.display());

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build");
}

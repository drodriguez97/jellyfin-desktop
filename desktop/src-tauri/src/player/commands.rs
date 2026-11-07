//! Tauri commands backing `window.api.player.*`.

use super::Player;
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// The player is created after the window exists, so the state holds an Option.
pub type PlayerState = RwLock<Option<Arc<Player>>>;

pub fn player_from(app: &AppHandle) -> Option<Arc<Player>> {
    app.try_state::<PlayerState>().and_then(|s| s.read().clone())
}

fn get(state: &State<'_, PlayerState>) -> Result<Arc<Player>, String> {
    state.read().clone().ok_or_else(|| "player not initialized".to_string())
}

#[tauri::command]
pub fn player_load(
    state: State<'_, PlayerState>,
    url: String,
    options: Option<Value>,
    metadata: Option<Value>,
    audio_stream: Option<Value>,
    subtitle_stream: Option<Value>,
) -> Result<bool, String> {
    get(&state)?.load(
        &url,
        &options.unwrap_or(Value::Null),
        &metadata.unwrap_or(Value::Null),
        audio_stream.unwrap_or(Value::Null),
        subtitle_stream.unwrap_or(Value::Null),
    );
    Ok(true)
}

#[tauri::command]
pub fn player_queue_media(
    state: State<'_, PlayerState>,
    url: String,
    options: Option<Value>,
    metadata: Option<Value>,
    audio_stream: Option<Value>,
    subtitle_stream: Option<Value>,
) -> Result<(), String> {
    get(&state)?.queue_media(
        &url,
        &options.unwrap_or(Value::Null),
        &metadata.unwrap_or(Value::Null),
        audio_stream.unwrap_or(Value::Null),
        subtitle_stream.unwrap_or(Value::Null),
    );
    Ok(())
}

#[tauri::command]
pub fn player_clear_queue(state: State<'_, PlayerState>) -> Result<(), String> {
    get(&state)?.clear_queue();
    Ok(())
}

#[tauri::command]
pub fn player_play(state: State<'_, PlayerState>) -> Result<(), String> {
    get(&state)?.play();
    Ok(())
}

#[tauri::command]
pub fn player_pause(state: State<'_, PlayerState>) -> Result<(), String> {
    get(&state)?.pause();
    Ok(())
}

#[tauri::command]
pub fn player_stop(state: State<'_, PlayerState>) -> Result<(), String> {
    get(&state)?.stop();
    Ok(())
}

#[tauri::command]
pub fn player_seek_to(state: State<'_, PlayerState>, ms: i64) -> Result<(), String> {
    get(&state)?.seek_to(ms);
    Ok(())
}

#[tauri::command]
pub fn player_stream_switch(state: State<'_, PlayerState>) -> Result<(), String> {
    get(&state)?
        .stream_switch_imminent
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn player_get_position(state: State<'_, PlayerState>) -> Result<i64, String> {
    Ok(get(&state)?.position_ms())
}

#[tauri::command]
pub fn player_get_duration(state: State<'_, PlayerState>) -> Result<i64, String> {
    Ok(get(&state)?.duration_ms())
}

#[tauri::command]
pub fn player_set_volume(state: State<'_, PlayerState>, volume: f64) -> Result<(), String> {
    get(&state)?.set_volume(volume.round() as i64);
    Ok(())
}

#[tauri::command]
pub fn player_volume(state: State<'_, PlayerState>) -> Result<i64, String> {
    Ok(get(&state)?.volume())
}

#[tauri::command]
pub fn player_set_muted(state: State<'_, PlayerState>, muted: bool) -> Result<(), String> {
    get(&state)?.set_muted(muted);
    Ok(())
}

#[tauri::command]
pub fn player_muted(state: State<'_, PlayerState>) -> Result<bool, String> {
    Ok(get(&state)?.muted())
}

#[tauri::command]
pub fn player_set_playback_rate(state: State<'_, PlayerState>, rate: f64) -> Result<(), String> {
    get(&state)?.set_playback_rate(rate.round() as i64);
    Ok(())
}

#[tauri::command]
pub fn player_set_audio_stream(state: State<'_, PlayerState>, audio_stream: Value) -> Result<(), String> {
    get(&state)?.set_audio_stream(audio_stream);
    Ok(())
}

#[tauri::command]
pub fn player_set_subtitle_stream(state: State<'_, PlayerState>, subtitle_stream: Value) -> Result<(), String> {
    get(&state)?.set_subtitle_stream(subtitle_stream);
    Ok(())
}

#[tauri::command]
pub fn player_set_audio_delay(state: State<'_, PlayerState>, milliseconds: i64) -> Result<(), String> {
    get(&state)?.set_audio_delay(milliseconds);
    Ok(())
}

#[tauri::command]
pub fn player_set_subtitle_delay(state: State<'_, PlayerState>, milliseconds: i64) -> Result<(), String> {
    get(&state)?.set_subtitle_delay(milliseconds);
    Ok(())
}

#[tauri::command]
pub fn player_set_video_rectangle(app: AppHandle, x: i32, y: i32, w: i32, h: i32) {
    // The video always fills the window; the web client toggles its own
    // transparency. Kept so the plugin's calls keep working.
    let _ = app.emit("player:onVideoRecangleChanged", (x, y, w, h));
}

#[tauri::command]
pub fn player_set_video_only_mode(_enable: bool) {}

#[tauri::command]
pub fn player_get_audio_device_list(state: State<'_, PlayerState>) -> Result<Value, String> {
    Ok(get(&state)?.audio_device_list())
}

#[tauri::command]
pub fn player_set_audio_device(state: State<'_, PlayerState>, name: String) -> Result<(), String> {
    get(&state)?.set_audio_device(&name);
    Ok(())
}

#[tauri::command]
pub fn player_user_command(state: State<'_, PlayerState>, command: String) -> Result<(), String> {
    get(&state)?.user_command(&command);
    Ok(())
}

#[tauri::command]
pub fn player_set_web_playlist(
    app: AppHandle,
    state: State<'_, PlayerState>,
    playlist: Value,
    current_item_id: String,
) -> Result<(), String> {
    let player = get(&state)?;
    {
        let mut pb = player.playback.lock();
        pb.web_playlist = playlist.clone();
        pb.current_web_playlist_item_id = current_item_id.clone();
    }
    let _ = app.emit("player:webPlaylistChanged", (playlist, current_item_id));
    Ok(())
}

#[tauri::command]
pub fn player_get_web_playlist(state: State<'_, PlayerState>) -> Result<Value, String> {
    Ok(get(&state)?.playback.lock().web_playlist.clone())
}

#[tauri::command]
pub fn player_get_current_web_playlist_item_id(state: State<'_, PlayerState>) -> Result<String, String> {
    Ok(get(&state)?.playback.lock().current_web_playlist_item_id.clone())
}

// ---- notify*: the web client reports its own state for OS media integration.

#[tauri::command]
pub fn player_notify_shuffle_change(app: AppHandle, enabled: bool) {
    let _ = app.emit("player:shuffleChanged", (enabled,));
}
#[tauri::command]
pub fn player_notify_repeat_change(app: AppHandle, mode: String) {
    let _ = app.emit("player:repeatChanged", (mode,));
}
#[tauri::command]
pub fn player_notify_fullscreen_change(app: AppHandle, is_fullscreen: bool) {
    let _ = app.emit("player:fullscreenChanged", (is_fullscreen,));
}
#[tauri::command]
pub fn player_notify_rate_change(app: AppHandle, rate: f64) {
    let _ = app.emit("player:rateChanged", (rate,));
}
#[tauri::command]
pub fn player_notify_queue_change(app: AppHandle, can_next: bool, can_previous: bool) {
    let _ = app.emit("player:queueChanged", (can_next, can_previous));
}
#[tauri::command]
pub fn player_notify_playback_stop(app: AppHandle, is_navigating: bool) {
    let _ = app.emit("player:playbackStopped", (is_navigating,));
}
#[tauri::command]
pub fn player_notify_duration_change(app: AppHandle, duration_ms: i64) {
    let _ = app.emit("player:durationChanged", (duration_ms,));
}
#[tauri::command]
pub fn player_notify_playback_state(app: AppHandle, state: String) {
    crate::media_keys::on_playback_state(&app, &state);
    let _ = app.emit("player:playbackStateChanged", (state,));
}
#[tauri::command]
pub fn player_notify_position(app: AppHandle, position_ms: i64) {
    crate::media_keys::on_position(&app, position_ms);
    let _ = app.emit("player:positionChanged", (position_ms,));
}
#[tauri::command]
pub fn player_notify_seek(app: AppHandle, position_ms: i64) {
    let _ = app.emit("player:seekPerformed", (position_ms,));
}
#[tauri::command]
pub fn player_notify_metadata(app: AppHandle, metadata: Value) {
    // Artwork and metadata are set from queue_media (which knows the server
    // URL); this notification only carries the item, so re-send text only
    // when no load went through the player (e.g. the HTML5 player path).
    if player_from(&app).is_none() {
        crate::media_keys::on_metadata(&app, &metadata);
    }
    let _ = app.emit("player:metadataChanged", (metadata,));
}
#[tauri::command]
pub fn player_notify_volume_change(app: AppHandle, volume: f64) {
    let _ = app.emit("player:volumeChanged", (volume,));
}

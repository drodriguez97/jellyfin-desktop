//! Host input actions (port of the relevant parts of `InputComponent`).
//! Actions prefixed with `host:` are handled natively; everything else is
//! forwarded to the web client's input plugin through `input:hostInput`.

use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};

pub fn handle_action(app: &AppHandle, action: &str) {
    if let Some(rest) = action.strip_prefix("host:") {
        let mut parts = rest.splitn(2, ' ');
        let command = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("");
        match command {
            "fullscreen" => crate::window::set_fullscreen(app, !crate::window::is_fullscreen(app)),
            "player" => {
                if let Some(p) = crate::player::commands::player_from(app) {
                    p.user_command(args);
                }
            }
            "script" => crate::system::run_user_script(app, args),
            other => warn!("unknown host command: {other}"),
        }
    } else {
        debug!("Emit input action: {action}");
        let _ = app.emit("input:hostInput", (vec![action.to_string()],));
    }
}

/// Sends an action to the web client (used by media keys / taskbar buttons).
pub fn send_action(app: &AppHandle, action: &str) {
    let _ = app.emit("input:hostInput", (vec![action.to_string()],));
}

#[tauri::command]
pub fn input_execute_actions(app: AppHandle, actions: Vec<String>) {
    for action in actions {
        handle_action(&app, &action);
    }
}

#[tauri::command]
pub fn input_set_volume(app: AppHandle, volume: i64) {
    let _ = app.emit("input:volumeChanged", (volume,));
}

#[tauri::command]
pub fn input_seek_to(app: AppHandle, position: i64) {
    let _ = app.emit("input:positionSeek", (position,));
}

#[tauri::command]
pub fn input_set_rate(app: AppHandle, rate: f64) {
    let _ = app.emit("input:rateChanged", (rate,));
}

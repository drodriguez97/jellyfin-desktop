//! Tauri commands backing `window.api.settings.*` and the events
//! `settings:sectionValueUpdate` / `settings:groupUpdate`.

use super::{Settings, Updates};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

pub type SettingsState = Arc<Mutex<Settings>>;

/// Forwards pending updates to the web client.
pub fn emit_updates(app: &AppHandle, updates: &Updates) {
    for (section, values) in &updates.values {
        let _ = app.emit("settings:sectionValueUpdate", (section, values));
    }
    for (section, descriptions) in &updates.groups {
        let _ = app.emit("settings:groupUpdate", (section, descriptions));
    }
}

#[tauri::command]
pub fn settings_set_value(
    app: AppHandle,
    state: State<'_, SettingsState>,
    section: String,
    key: String,
    value: Value,
) {
    let updates = state.lock().set_value(&section, &key, value);
    emit_updates(&app, &updates);
}

#[tauri::command]
pub fn settings_set_values(app: AppHandle, state: State<'_, SettingsState>, options: Value) {
    let key = options.get("key").and_then(Value::as_str).unwrap_or("").to_string();
    let value = options.get("value").cloned().unwrap_or(Value::Null);
    if key.is_empty() {
        return;
    }
    let updates = state.lock().set_values(&key, value);
    emit_updates(&app, &updates);
}

#[tauri::command]
pub fn settings_value(state: State<'_, SettingsState>, section: String, key: String) -> Value {
    state.lock().value(&section, &key)
}

#[tauri::command]
pub fn settings_all_values(state: State<'_, SettingsState>, section: Option<String>) -> Value {
    state.lock().all_values(section.as_deref().unwrap_or(""))
}

#[tauri::command]
pub fn settings_remove_value(app: AppHandle, state: State<'_, SettingsState>, section_or_key: String) {
    let updates = state.lock().remove_value(&section_or_key);
    emit_updates(&app, &updates);
}

#[tauri::command]
pub fn settings_reset_to_default(app: AppHandle, state: State<'_, SettingsState>, section: Option<String>) {
    let updates = match section {
        Some(s) => state.lock().reset_to_default(&s),
        None => state.lock().reset_to_default_all(),
    };
    emit_updates(&app, &updates);
}

#[tauri::command]
pub fn settings_descriptions(state: State<'_, SettingsState>) -> Value {
    state.lock().setting_descriptions()
}

#[tauri::command]
pub fn settings_ordered_sections(state: State<'_, SettingsState>) -> Value {
    state.lock().ordered_sections()
}

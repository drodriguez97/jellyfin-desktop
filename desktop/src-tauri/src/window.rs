//! Window management: fullscreen, always-on-top, cursor, and geometry
//! persistence compatible with the Qt `WindowManager` keys in `storage.json`.

use crate::settings::{self, Settings};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow, WindowEvent};
use tracing::{debug, info, warn};

pub const MAIN_LABEL: &str = "main";
const DEFAULT_SIZE: (u32, u32) = (1280, 720);
const MIN_SIZE: (u32, u32) = (213, 120);
const GEOMETRY_SYNC_DELAY: Duration = Duration::from_secs(30);

pub struct WindowState {
    pub settings: Arc<Mutex<Settings>>,
    geometry_dirty: Mutex<Option<Instant>>,
    initial_size: Mutex<Option<PhysicalSize<u32>>>,
}

impl WindowState {
    pub fn new(settings: Arc<Mutex<Settings>>) -> Self {
        Self {
            settings,
            geometry_dirty: Mutex::new(None),
            initial_size: Mutex::new(None),
        }
    }
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN_LABEL)
}

/// Per-screen-configuration key prefix, identical to the Qt implementation.
fn key_prefix(window: &WebviewWindow) -> String {
    let monitors = window.available_monitors().unwrap_or_default();
    if monitors.len() == 1 {
        if let Ok(Some(primary)) = window.primary_monitor() {
            let size = primary.size();
            return format!("{}x{} screen: ", size.width, size.height);
        }
    }
    format!("{} screens: ", monitors.len())
}

fn get_i64(settings: &Settings, key: &str) -> i64 {
    match settings.value_opt(settings::SECTION_STATE, key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn get_bool(settings: &Settings, key: &str) -> bool {
    settings
        .value_opt(settings::SECTION_STATE, key)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn get_string(settings: &Settings, key: &str) -> String {
    match settings.value_opt(settings::SECTION_STATE, key) {
        Some(Value::String(s)) => s,
        _ => String::new(),
    }
}

/// Applies the saved size/position/maximized state before the window shows.
pub fn restore_geometry(window: &WebviewWindow, state: &WindowState) {
    let prefix = key_prefix(window);
    let (mut width, mut height, x, y, maximized, legacy) = {
        let s = state.settings.lock();
        let legacy = get_string(&s, "geometry");
        (
            get_i64(&s, &format!("{prefix}Width")),
            get_i64(&s, &format!("{prefix}Height")),
            get_i64(&s, &format!("{prefix}XPosition")),
            get_i64(&s, &format!("{prefix}YPosition")),
            get_bool(&s, &format!("{prefix}Window-Maximized")) || get_bool(&s, "maximized"),
            legacy,
        )
    };
    let mut pos = (x, y);
    let mut have_rect = width > 0 && height > 0;
    if !have_rect && !legacy.is_empty() {
        let parts: Vec<i64> = legacy.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 4 {
            pos = (parts[0], parts[1]);
            width = parts[2];
            height = parts[3];
            have_rect = width > 0 && height > 0;
        }
    }

    let scale = window.scale_factor().unwrap_or(1.0);
    if have_rect {
        let width = (width.max(MIN_SIZE.0 as i64)) as u32;
        let height = (height.max(MIN_SIZE.1 as i64)) as u32;
        // Saved values are physical pixels (Qt saved logical, but both start as
        // defaults on first run of this shell, so just treat them as physical).
        let size = PhysicalSize::new(width, height);
        let _ = window.set_size(size);
        *state.initial_size.lock() = Some(size);

        // Only restore the position if it intersects a monitor.
        let fits = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .any(|m| {
                let p = m.position();
                let s = m.size();
                let (mx, my, mw, mh) = (p.x as i64, p.y as i64, s.width as i64, s.height as i64);
                pos.0 < mx + mw && pos.0 + width as i64 > mx && pos.1 < my + mh && pos.1 + height as i64 > my
            });
        if fits {
            let _ = window.set_position(PhysicalPosition::new(pos.0 as i32, pos.1 as i32));
        } else {
            let _ = window.center();
        }
    } else {
        let size = PhysicalSize::new((DEFAULT_SIZE.0 as f64 * scale) as u32, (DEFAULT_SIZE.1 as f64 * scale) as u32);
        let _ = window.set_size(size);
        let _ = window.center();
        *state.initial_size.lock() = Some(size);
    }
    if maximized {
        let _ = window.maximize();
    }
    debug!("restored geometry: rect={have_rect} maximized={maximized} prefix={prefix:?}");
}

fn save_geometry(window: &WebviewWindow, state: &WindowState) {
    let is_maximized = window.is_maximized().unwrap_or(false);
    let is_fullscreen = window.is_fullscreen().unwrap_or(false);
    let prefix = key_prefix(window);
    let mut s = state.settings.lock();

    if !is_maximized && !is_fullscreen {
        if let Ok(size) = window.inner_size() {
            let unchanged = state.initial_size.lock().map(|i| i == size).unwrap_or(false);
            if unchanged {
                s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}Width"), Value::Null);
                s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}Height"), Value::Null);
            } else {
                s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}Width"), json!(size.width));
                s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}Height"), json!(size.height));
            }
        }
        if let Ok(pos) = window.outer_position() {
            s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}XPosition"), json!(pos.x));
            s.set_value_no_save(settings::SECTION_STATE, &format!("{prefix}YPosition"), json!(pos.y));
        }
    }
    if !is_fullscreen {
        s.set_value_no_save(
            settings::SECTION_STATE,
            &format!("{prefix}Window-Maximized"),
            json!(is_maximized),
        );
    }
}

pub fn flush_geometry(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    let state = app.state::<WindowState>();
    save_geometry(&window, &state);
    *state.geometry_dirty.lock() = None;
    let result = state.settings.lock().save_storage();
    if let Err(e) = result {
        warn!("saving window geometry failed: {e:#}");
    }
}

/// Hooks window events: geometry tracking (debounced to disk), fullscreen
/// notifications and the kiosk "always fullscreen" setting.
pub fn install_handlers(app: &AppHandle, window: &WebviewWindow) {
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
            let state = handle.state::<WindowState>();
            if let Some(window) = main_window(&handle) {
                save_geometry(&window, &state);
                let mut dirty = state.geometry_dirty.lock();
                if dirty.is_none() {
                    *dirty = Some(Instant::now());
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(GEOMETRY_SYNC_DELAY).await;
                        flush_geometry(&h);
                    });
                }
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            flush_geometry(&handle);
            if crate::tray::enabled(&handle) {
                // Minimize to tray instead of quitting.
                api.prevent_close();
                if let Some(w) = main_window(&handle) {
                    let _ = w.hide();
                }
            }
        }
        _ => {}
    });
}

/// Applies the main-section settings that affect the window.
pub fn apply_settings(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    let state = app.state::<WindowState>();
    let (fullscreen, on_top, always_fs) = {
        let s = state.settings.lock();
        (
            s.bool(settings::SECTION_MAIN, "fullscreen"),
            s.bool(settings::SECTION_MAIN, "alwaysOnTop"),
            s.bool(settings::SECTION_MAIN, "forceAlwaysFS"),
        )
    };
    if fullscreen || always_fs {
        let _ = window.set_fullscreen(true);
    }
    if on_top {
        let _ = window.set_always_on_top(true);
    }
}

pub fn set_fullscreen(app: &AppHandle, enable: bool) {
    if let Some(window) = main_window(app) {
        info!("fullscreen: {enable}");
        let _ = window.set_fullscreen(enable);
        if let Some(state) = app.try_state::<WindowState>() {
            let updates = state
                .settings
                .lock()
                .set_value_no_save(settings::SECTION_MAIN, "fullscreen", json!(enable));
            settings::commands::emit_updates(app, &updates);
        }
        let _ = app.emit("window:fullScreenSwitched", ());
    }
}

pub fn is_fullscreen(app: &AppHandle) -> bool {
    main_window(app).and_then(|w| w.is_fullscreen().ok()).unwrap_or(false)
}

#[tauri::command]
pub fn window_set_fullscreen(app: AppHandle, enable: bool) {
    set_fullscreen(&app, enable);
}

#[tauri::command]
pub fn window_is_fullscreen(app: AppHandle) -> bool {
    is_fullscreen(&app)
}

#[tauri::command]
pub fn window_toggle_fullscreen(app: AppHandle) {
    let now = is_fullscreen(&app);
    set_fullscreen(&app, !now);
}

#[tauri::command]
pub fn window_set_always_on_top(app: AppHandle, enable: bool) {
    if let Some(w) = main_window(&app) {
        let _ = w.set_always_on_top(enable);
    }
}

#[tauri::command]
pub fn window_is_always_on_top(app: AppHandle) -> bool {
    main_window(&app).and_then(|w| w.is_always_on_top().ok()).unwrap_or(false)
}

#[tauri::command]
pub fn window_toggle_always_on_top(app: AppHandle) {
    let now = window_is_always_on_top(app.clone());
    window_set_always_on_top(app, !now);
}

#[tauri::command]
pub fn window_set_cursor_visibility(app: AppHandle, visible: bool) {
    if let Some(w) = main_window(&app) {
        let _ = w.set_cursor_visible(visible);
    }
}

#[tauri::command]
pub fn window_raise(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[allow(dead_code)]
pub fn settings_of(state: &State<'_, WindowState>) -> Arc<Mutex<Settings>> {
    state.settings.clone()
}

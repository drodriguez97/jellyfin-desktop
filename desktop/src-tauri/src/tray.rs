//! System tray icon with minimize-to-tray on close (setting
//! `main.enableWindowsTrayIcon`, off by default), as in the Qt shell.

use crate::settings;
use anyhow::Result;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tracing::info;

pub fn enabled(app: &AppHandle) -> bool {
    app.try_state::<settings::commands::SettingsState>()
        .map(|s| s.lock().bool(settings::SECTION_MAIN, "enableWindowsTrayIcon"))
        .unwrap_or(false)
}

pub fn install(app: &AppHandle) -> Result<()> {
    if !enabled(app) {
        return Ok(());
    }
    let restore = MenuItem::with_id(app, "restore", "Restore", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&restore, &PredefinedMenuItem::separator(app)?, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Jellyfin Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "restore" => crate::window::window_raise(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::window::window_raise(tray.app_handle().clone());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    info!("tray icon installed");
    Ok(())
}

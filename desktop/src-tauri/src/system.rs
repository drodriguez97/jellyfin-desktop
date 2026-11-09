//! System-level commands: server connectivity probing, update check,
//! external URLs, user scripts, exit/restart (port of `SystemComponent`).

use crate::profiles::Profile;
use crate::settings::{self, Settings};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tracing::{info, warn};

pub use crate::connectivity::{user_agent, CONNECTIVITY_RETRY_INTERVAL_MS, CONNECTIVITY_TIMEOUT_MS};

pub struct SystemState {
    pub settings: Arc<Mutex<Settings>>,
    pub profile: Profile,
    pub web_client_version: Mutex<String>,
    /// Incremented on every new check / cancel; stale attempts stop themselves.
    pub connectivity_generation: AtomicU64,
    pub user_agent: String,
}

impl SystemState {
    pub fn is_web_client_connected(&self) -> bool {
        !self.web_client_version.lock().is_empty()
    }
}

/// Probes `<base>/System/Info/Public`; retries every few seconds until it
/// succeeds or is cancelled. Results are reported through
/// `system:serverConnectivityResult` / `system:serverConnectivityAttemptFailed`.
async fn connectivity_loop(app: AppHandle, state: Arc<ConnectivityCtx>, url: String, generation: u64) {
    loop {
        if state.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let ignore_ssl = state.settings.lock().bool(settings::SECTION_MAIN, "ignoreSSLErrors");
        let client = crate::connectivity::http_client(ignore_ssl, CONNECTIVITY_TIMEOUT_MS);
        let outcome = crate::connectivity::probe(&client, &url).await;

        if state.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        match outcome {
            Ok(resolved) => {
                let _ = app.emit("system:serverConnectivityResult", (url.clone(), true, resolved));
                return;
            }
            Err(reason) => {
                warn!("connectivity check failed: {reason}");
                let _ = app.emit("system:serverConnectivityAttemptFailed", (url.clone(), reason));
                tokio::time::sleep(Duration::from_millis(CONNECTIVITY_RETRY_INTERVAL_MS)).await;
            }
        }
    }
}

pub struct ConnectivityCtx {
    pub settings: Arc<Mutex<Settings>>,
    pub generation: AtomicU64,
}

#[tauri::command]
pub async fn system_check_server_connectivity(
    app: AppHandle,
    ctx: State<'_, Arc<ConnectivityCtx>>,
    url: String,
) -> Result<(), String> {
    let generation = ctx.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let ctx = ctx.inner().clone();
    tauri::async_runtime::spawn(connectivity_loop(app, ctx, url, generation));
    Ok(())
}

#[tauri::command]
pub fn system_cancel_server_connectivity(ctx: State<'_, Arc<ConnectivityCtx>>) {
    ctx.generation.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
pub fn system_exit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn system_restart(app: AppHandle) {
    app.restart();
}

#[tauri::command]
pub fn system_hello(state: State<'_, SystemState>, version: String) {
    info!("Web client ({version}) fully inited.");
    *state.web_client_version.lock() = version;
}

#[tauri::command]
pub fn system_open_external_url(url: String) {
    if let Err(e) = tauri_plugin_opener::open_url(&url, None::<&str>) {
        warn!("openExternalUrl {url}: {e}");
    }
}

#[tauri::command]
pub fn system_get_capabilities_string(state: State<'_, SystemState>) -> String {
    let s = state.settings.lock();
    crate::connectivity::capabilities_string(
        &s.string(settings::SECTION_AUDIO, "channels"),
        s.bool(settings::SECTION_AUDIO, "passthrough.dts"),
        s.bool(settings::SECTION_AUDIO, "passthrough.ac3"),
    )
}

#[tauri::command]
pub fn system_network_addresses() -> Vec<String> {
    Vec::new()
}

#[tauri::command]
pub fn system_run_user_script(app: AppHandle, script: String) {
    run_user_script(&app, &script);
}

pub fn run_user_script(app: &AppHandle, script: &str) {
    use tauri::Manager;
    let state = app.state::<SystemState>();
    // Only the file name is honoured so keymaps can't escape the scripts dir.
    let file_name = std::path::Path::new(script)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let path = state.profile.data_dir().join("scripts").join(&file_name);
    if !path.exists() {
        warn!("Could not find script: {}", path.display());
        return;
    }
    info!("Running script: {}", path.display());
    if let Err(e) = std::process::Command::new(&path).spawn() {
        warn!("Error running script {}: {e}", path.display());
    }
}

#[tauri::command]
pub fn system_fetch_page_for_csp_workaround(app: AppHandle, url: String) {
    // Initialization scripts are not subject to page CSP, so there is nothing
    // to work around; report "no CSP" so the caller continues normally.
    let _ = app.emit("system:pageContentReady", (String::new(), url, false));
}


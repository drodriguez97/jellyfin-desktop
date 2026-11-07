//! System-level commands: server connectivity probing, update check,
//! external URLs, user scripts, exit/restart (port of `SystemComponent`).

use crate::profiles::Profile;
use crate::settings::{self, Settings};
use parking_lot::Mutex;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tracing::{info, warn};

pub const CONNECTIVITY_RETRY_INTERVAL_MS: u64 = 5000;
pub const CONNECTIVITY_TIMEOUT_MS: u64 = 10000;

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

/// "JellyfinDesktop/<version> (Windows; x86_64) Chrome/<major>.0.0.0"
pub fn user_agent() -> String {
    let chrome_major = tauri::webview_version()
        .ok()
        .and_then(|v| v.split('.').next().map(str::to_string))
        .unwrap_or_else(|| "120".into());
    format!(
        "JellyfinDesktop/{} (Windows; x86_64) Chrome/{}.0.0.0",
        env!("CARGO_PKG_VERSION"),
        chrome_major
    )
}

fn http_client(settings: &Settings, timeout_ms: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_millis(timeout_ms))
        .danger_accept_invalid_certs(settings.bool(settings::SECTION_MAIN, "ignoreSSLErrors"))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("reqwest client")
}

/// scheme://host[:port][/path-before-/web], mirroring `SystemComponent::extractBaseUrl`.
pub fn extract_base_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else { return url.to_string() };
    let mut origin = format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or(""));
    if let Some(port) = parsed.port() {
        origin.push_str(&format!(":{port}"));
    }
    let path = parsed.path();
    let lower = path.to_lowercase();
    match lower.rfind("/web") {
        Some(idx) => format!("{origin}{}", &path[..idx]),
        None => origin,
    }
}

async fn resolve_url(client: &reqwest::Client, url: &str) -> String {
    match client.head(url).send().await {
        Ok(resp) => resp.url().to_string(),
        Err(e) => {
            warn!("resolveUrl: {e}");
            url.to_string()
        }
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
        let client = http_client(&state.settings.lock(), CONNECTIVITY_TIMEOUT_MS);
        let resolved = resolve_url(&client, &url).await;
        let base = extract_base_url(&resolved);
        let check_url = format!("{base}/System/Info/Public");
        info!("checking connectivity: {check_url}");

        let outcome = match client.get(&check_url).header("Cache-Control", "no-cache").send().await {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(json)
                    if json
                        .get("Id")
                        .and_then(Value::as_str)
                        .map(|s| !s.is_empty())
                        .unwrap_or(false) =>
                {
                    Ok(())
                }
                Ok(_) => Err("Something answered at this address, but it isn't a Jellyfin server.".to_string()),
                Err(e) => Err(format!("invalid response: {e}")),
            },
            Err(e) if e.is_timeout() => Err("no response from the server".to_string()),
            Err(e) => Err(e.to_string()),
        };

        if state.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        match outcome {
            Ok(()) => {
                let _ = app.emit("system:serverConnectivityResult", (url.clone(), true, resolved.clone()));
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
    let channels = s.string(settings::SECTION_AUDIO, "channels");
    let dts = s.bool(settings::SECTION_AUDIO, "passthrough.dts");
    let ac3 = s.bool(settings::SECTION_AUDIO, "passthrough.ac3");
    let (mut dts_ch, mut ac3_ch) = (2, 2);
    if channels != "2.0" {
        dts_ch = 8;
        ac3_ch = 8;
    } else if dts {
        dts_ch = 8;
    } else if ac3 {
        ac3_ch = 8;
    }
    format!(
        "protocols=shoutcast,http-video;videoDecoders=h264{{profile:high&resolution:2160&level:52}};audioDecoders=mp3,aac,dts{{bitrate:800000&channels:{dts_ch}}},ac3{{bitrate:800000&channels:{ac3_ch}}}"
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

#[tauri::command]
pub async fn system_check_for_updates(app: AppHandle, state: State<'_, SystemState>) -> Result<(), String> {
    let enabled = state.settings.lock().bool(settings::SECTION_MAIN, "checkForUpdates");
    if !enabled {
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let url = "https://github.com/jellyfin/jellyfin-desktop/releases/latest";
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_redirection() => {
            if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                let _ = app.emit("system:updateInfoEmitted", (loc.to_string(),));
            }
        }
        Ok(resp) => warn!("update check: unexpected status {}", resp.status()),
        Err(e) => warn!("update check failed: {e}"),
    }
    Ok(())
}

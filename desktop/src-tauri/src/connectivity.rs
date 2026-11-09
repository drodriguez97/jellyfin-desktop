//! Server connectivity probing (the HTTP half of `SystemComponent`): resolve
//! redirects, find the server root and check `/System/Info/Public`. The retry
//! loop and the Tauri events around it live in `system.rs`.

use serde_json::Value;
use std::time::Duration;
use tracing::warn;

pub const CONNECTIVITY_RETRY_INTERVAL_MS: u64 = 5000;
pub const CONNECTIVITY_TIMEOUT_MS: u64 = 10000;

/// "JellyfinDesktop/<version> (Windows; x86_64) Chrome/<major>.0.0.0"
pub fn user_agent() -> String {
    user_agent_for(tauri::webview_version().ok().as_deref())
}

pub fn user_agent_for(webview_version: Option<&str>) -> String {
    let chrome_major = webview_version
        .and_then(|v| v.split('.').next().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "120".into());
    format!("JellyfinDesktop/{} (Windows; x86_64) Chrome/{}.0.0.0", env!("CARGO_PKG_VERSION"), chrome_major)
}

pub fn http_client(ignore_ssl_errors: bool, timeout_ms: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(user_agent())
        .timeout(Duration::from_millis(timeout_ms))
        .danger_accept_invalid_certs(ignore_ssl_errors)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("reqwest client")
}

/// Follows redirects with a HEAD request and returns the final URL (the
/// original one when the request fails).
pub async fn resolve_url(client: &reqwest::Client, url: &str) -> String {
    match client.head(url).send().await {
        Ok(resp) => resp.url().to_string(),
        Err(e) => {
            warn!("resolveUrl: {e}");
            url.to_string()
        }
    }
}

/// Interprets the body of `/System/Info/Public`.
pub fn classify_public_info(body: Result<Value, String>) -> Result<(), String> {
    match body {
        Ok(json) if json.get("Id").and_then(Value::as_str).map(|s| !s.is_empty()).unwrap_or(false) => Ok(()),
        Ok(_) => Err("Something answered at this address, but it isn't a Jellyfin server.".to_string()),
        Err(e) => Err(format!("invalid response: {e}")),
    }
}

/// One connectivity attempt. Ok carries the resolved URL the web client
/// should use; Err carries the reason shown to the user.
pub async fn probe(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resolved = resolve_url(client, url).await;
    let base = crate::urls::extract_base_url(&resolved);
    let check_url = format!("{base}/System/Info/Public");
    tracing::info!("checking connectivity: {check_url}");
    match client.get(&check_url).header("Cache-Control", "no-cache").send().await {
        Ok(resp) => classify_public_info(resp.json::<Value>().await.map_err(|e| e.to_string())).map(|_| resolved),
        Err(e) if e.is_timeout() => Err("no response from the server".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

/// The `X-Emby-Capabilities`-style string the web client reports for this device.
pub fn capabilities_string(channels: &str, dts_passthrough: bool, ac3_passthrough: bool) -> String {
    let (mut dts_ch, mut ac3_ch) = (2, 2);
    if channels != "2.0" {
        dts_ch = 8;
        ac3_ch = 8;
    } else if dts_passthrough {
        dts_ch = 8;
    } else if ac3_passthrough {
        ac3_ch = 8;
    }
    format!(
        "protocols=shoutcast,http-video;videoDecoders=h264{{profile:high&resolution:2160&level:52}};audioDecoders=mp3,aac,dts{{bitrate:800000&channels:{dts_ch}}},ac3{{bitrate:800000&channels:{ac3_ch}}}"
    )
}

#[cfg(test)]
#[path = "connectivity_tests.rs"]
mod tests;

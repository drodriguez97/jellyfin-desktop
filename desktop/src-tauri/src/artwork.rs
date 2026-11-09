//! Downloads an item's primary image to a temp file so the Windows media
//! controls can show it (SMTC wants a `file://` URL). Port of `AlbumArtProvider`.

use serde_json::Value;
use std::path::PathBuf;
use tauri::AppHandle;
use tracing::debug;

fn api_key_from(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k.eq_ignore_ascii_case("ApiKey") || k.eq_ignore_ascii_case("api_key") {
            Some(v.to_string())
        } else {
            None
        }
    })
}

fn cache_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("jellyfin-desktop").join("artwork");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Fetches the artwork in the background, then updates the media controls.
pub fn request(app: AppHandle, item: Value, stream_url: String, base_url: String) {
    let item_id = item.get("Id").and_then(Value::as_str).unwrap_or("").to_string();
    // Prefer the album's image for tracks, the item's own otherwise.
    let (image_item, tag) = match (item.get("AlbumId").and_then(Value::as_str), item.get("AlbumPrimaryImageTag").and_then(Value::as_str)) {
        (Some(album), Some(tag)) => (album.to_string(), tag.to_string()),
        _ => (
            item_id.clone(),
            item.get("ImageTags")
                .and_then(|t| t.get("Primary"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        ),
    };
    if image_item.is_empty() || tag.is_empty() || base_url.is_empty() {
        crate::media_keys::set_metadata(&app, &item, None);
        return;
    }
    let key = api_key_from(&stream_url);
    let mut url = format!("{base_url}/Items/{image_item}/Images/Primary?maxWidth=500&tag={tag}");
    if let Some(k) = &key {
        url.push_str(&format!("&ApiKey={k}"));
    }
    let path = cache_dir().join(format!("{image_item}-{tag}.jpg"));

    tauri::async_runtime::spawn(async move {
        if !path.exists() {
            let client = reqwest::Client::builder()
                .user_agent(crate::system::user_agent())
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(15))
                .build();
            let Ok(client) = client else { return };
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(bytes) = resp.bytes().await {
                        let _ = std::fs::write(&path, &bytes);
                    }
                }
                Ok(resp) => debug!("artwork fetch {url}: status {}", resp.status()),
                Err(e) => debug!("artwork fetch failed: {e}"),
            }
        }
        let cover = if path.exists() {
            Some(format!("file:///{}", path.to_string_lossy().replace('\\', "/")))
        } else {
            None
        };
        crate::media_keys::set_metadata(&app, &item, cover);
    });
}

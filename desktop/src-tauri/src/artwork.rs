//! Downloads an item's primary image to a temp file so the Windows media
//! controls can show it (SMTC wants a `file://` URL). Port of `AlbumArtProvider`.

use serde_json::Value;
use std::path::PathBuf;
use tauri::AppHandle;
use tracing::debug;

fn cache_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("jellyfin-desktop").join("artwork");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Fetches the artwork in the background, then updates the media controls.
pub fn request(app: AppHandle, item: Value, stream_url: String, base_url: String) {
    let Some((url, file_name)) = crate::urls::artwork_image(&item, &stream_url, &base_url) else {
        crate::media_keys::set_metadata(&app, &item, None);
        return;
    };
    let path = cache_dir().join(file_name);

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
        let cover = if path.exists() { Some(crate::urls::file_url(&path)) } else { None };
        crate::media_keys::set_metadata(&app, &item, cover);
    });
}

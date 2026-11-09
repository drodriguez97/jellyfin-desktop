//! Small URL helpers shared by the player, the connectivity check and the
//! artwork fetcher. Pure functions, no I/O.

use serde_json::Value;

/// scheme://host[:port] of a URL, without path or query ("" when there is no scheme).
pub fn base_url_of(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some(x) => x,
        None => return String::new(),
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    format!("{scheme}://{host}")
}

/// scheme://host[:port][/path-before-/web], mirroring `SystemComponent::extractBaseUrl`.
/// Anything that does not parse as an absolute URL is returned unchanged.
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

/// The `ApiKey` / `api_key` query parameter of a stream URL.
pub fn api_key_from(url: &str) -> Option<String> {
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

/// Picks the image to show for an item in the Windows media controls: the
/// album's primary image for tracks, the item's own otherwise. Returns the
/// image URL and a cache file name, or None when there is nothing to fetch.
pub fn artwork_image(item: &Value, stream_url: &str, base_url: &str) -> Option<(String, String)> {
    let item_id = item.get("Id").and_then(Value::as_str).unwrap_or("");
    let (image_item, tag) = match (
        item.get("AlbumId").and_then(Value::as_str),
        item.get("AlbumPrimaryImageTag").and_then(Value::as_str),
    ) {
        (Some(album), Some(tag)) => (album, tag),
        _ => (
            item_id,
            item.get("ImageTags")
                .and_then(|t| t.get("Primary"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
    };
    if image_item.is_empty() || tag.is_empty() || base_url.is_empty() {
        return None;
    }
    let mut url = format!("{base_url}/Items/{image_item}/Images/Primary?maxWidth=500&tag={tag}");
    if let Some(k) = api_key_from(stream_url) {
        url.push_str(&format!("&ApiKey={k}"));
    }
    Some((url, format!("{image_item}-{tag}.jpg")))
}

/// `file:///C:/...` form of a local path, as SMTC wants it.
pub fn file_url(path: &std::path::Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
#[path = "urls_tests.rs"]
mod tests;

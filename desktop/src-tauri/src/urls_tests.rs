use super::*;
use serde_json::json;

#[test]
fn base_url() {
    assert_eq!(base_url_of("https://jf.example.com:8920/Videos/1/stream?x=1"), "https://jf.example.com:8920");
    assert_eq!(base_url_of("http://10.0.0.5/web/index.html#/home"), "http://10.0.0.5");
    assert_eq!(base_url_of("https://h?query"), "https://h");
    assert_eq!(base_url_of("not a url"), "");
}

#[test]
fn extract_base_url_strips_web_and_keeps_prefix() {
    assert_eq!(extract_base_url("https://jf.example.com/web/index.html"), "https://jf.example.com");
    assert_eq!(extract_base_url("http://host:8096/jellyfin/web/"), "http://host:8096/jellyfin");
    assert_eq!(extract_base_url("http://host:8096/WEB/index.html#!/login"), "http://host:8096");
    assert_eq!(extract_base_url("https://host/System/Info/Public"), "https://host");
    assert_eq!(extract_base_url("garbage"), "garbage");
}

#[test]
fn api_key() {
    assert_eq!(api_key_from("https://h/Videos/1/stream?ApiKey=abc&x=1"), Some("abc".into()));
    assert_eq!(api_key_from("https://h/Audio/1/stream?x=1&api_key=def"), Some("def".into()));
    assert_eq!(api_key_from("https://h/Videos/1/stream?x=1"), None);
    assert_eq!(api_key_from("https://h/Videos/1/stream"), None);
    assert_eq!(api_key_from("https://h/s?novalue"), None);
}

#[test]
fn artwork_prefers_album_image() {
    let track = json!({
        "Id": "t1", "AlbumId": "a1", "AlbumPrimaryImageTag": "tagA", "ImageTags": {"Primary": "tagT"}
    });
    let (url, file) = artwork_image(&track, "https://h/s?ApiKey=k", "https://h").unwrap();
    assert_eq!(url, "https://h/Items/a1/Images/Primary?maxWidth=500&tag=tagA&ApiKey=k");
    assert_eq!(file, "a1-tagA.jpg");
}

#[test]
fn artwork_falls_back_to_item_image_and_needs_everything() {
    let movie = json!({"Id": "m1", "ImageTags": {"Primary": "p"}});
    let (url, file) = artwork_image(&movie, "https://h/s", "https://h").unwrap();
    assert_eq!(url, "https://h/Items/m1/Images/Primary?maxWidth=500&tag=p");
    assert_eq!(file, "m1-p.jpg");
    assert_eq!(artwork_image(&movie, "https://h/s", ""), None);
    assert_eq!(artwork_image(&json!({"Id": "m1"}), "https://h/s", "https://h"), None);
    assert_eq!(artwork_image(&json!({"ImageTags": {"Primary": "p"}}), "https://h/s", "https://h"), None);
}

#[test]
fn file_urls_use_forward_slashes() {
    assert_eq!(file_url(std::path::Path::new("C:\\Temp\\a b.jpg")), "file:///C:/Temp/a b.jpg");
}

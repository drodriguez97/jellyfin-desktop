use super::*;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;

#[test]
fn user_agent_strings() {
    let ua = user_agent_for(Some("157.0.3456.1"));
    assert_eq!(ua, format!("JellyfinDesktop/{} (Windows; x86_64) Chrome/157.0.0.0", env!("CARGO_PKG_VERSION")));
    assert!(user_agent_for(None).ends_with("Chrome/120.0.0.0"));
    assert!(user_agent_for(Some("")).ends_with("Chrome/120.0.0.0"));
    assert!(user_agent().starts_with("JellyfinDesktop/"));
}

#[test]
fn capabilities_follow_audio_settings() {
    assert!(capabilities_string("2.0", false, false).contains("dts{bitrate:800000&channels:2},ac3{bitrate:800000&channels:2}"));
    assert!(capabilities_string("2.0", true, false).contains("dts{bitrate:800000&channels:8},ac3{bitrate:800000&channels:2}"));
    assert!(capabilities_string("2.0", false, true).contains("dts{bitrate:800000&channels:2},ac3{bitrate:800000&channels:8}"));
    assert!(capabilities_string("5.1", false, false).contains("dts{bitrate:800000&channels:8},ac3{bitrate:800000&channels:8}"));
}

#[test]
fn public_info_classification() {
    assert_eq!(classify_public_info(Ok(json!({"Id": "abc"}))), Ok(()));
    assert!(classify_public_info(Ok(json!({"Id": ""}))).unwrap_err().contains("isn't a Jellyfin server"));
    assert!(classify_public_info(Ok(json!({"hello": 1}))).unwrap_err().contains("isn't a Jellyfin server"));
    assert!(classify_public_info(Err("boom".into())).unwrap_err().contains("invalid response: boom"));
}

/// A one-request-at-a-time HTTP server answering by path.
fn serve(responses: Vec<(&'static str, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for (_, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let is_head = request.starts_with("HEAD");
            let payload = if is_head { "" } else { body };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                payload
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://{addr}")
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
}

#[test]
fn probe_accepts_a_jellyfin_server() {
    let base = serve(vec![("HEAD", r#"{"Id":"srv"}"#), ("GET", r#"{"Id":"srv","ServerName":"B"}"#)]);
    let client = http_client(false, 2000);
    let url = format!("{base}/web/index.html");
    let resolved = block_on(probe(&client, &url)).unwrap();
    assert_eq!(resolved, url);
}

#[test]
fn probe_rejects_a_non_jellyfin_answer() {
    let base = serve(vec![("HEAD", "{}"), ("GET", r#"{"hello":"world"}"#)]);
    let client = http_client(false, 2000);
    let err = block_on(probe(&client, &base)).unwrap_err();
    assert!(err.contains("isn't a Jellyfin server"), "{err}");
}

#[test]
fn probe_reports_unreachable_servers() {
    // Nothing listens here; connection refused.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let client = http_client(true, 1500);
    let err = block_on(probe(&client, &format!("http://{addr}"))).unwrap_err();
    assert!(!err.is_empty());
    // resolve_url falls back to the input when the HEAD fails
    assert_eq!(block_on(resolve_url(&client, "http://127.0.0.1:1/x")), "http://127.0.0.1:1/x");
}

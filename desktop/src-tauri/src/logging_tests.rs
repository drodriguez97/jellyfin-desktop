use super::*;

#[test]
fn censor_hides_tokens_after_every_marker() {
    let token = "0123456789abcdef0123456789abcdef";
    let xs = "x".repeat(32);
    for marker in ["api_key=", "X-MediaBrowser-Token%3D", "X-MediaBrowser-Token=", "ApiKey=", "AccessToken=", "AccessToken\":\""] {
        let msg = format!("GET /x?{marker}{token}&y=1");
        let out = censor(&msg);
        assert_eq!(out, format!("GET /x?{marker}{xs}&y=1"), "{marker}");
    }
}

#[test]
fn censor_handles_short_tails_and_multibyte_text() {
    assert_eq!(censor("api_key=abc"), "api_key=xxx");
    assert_eq!(censor("api_key="), "api_key=");
    assert_eq!(censor("nothing to see"), "nothing to see");
    // two tokens in one line, and a token followed by multi-byte characters
    let two = censor("ApiKey=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ApiKey=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert!(!two.contains('a') && !two.contains('b'), "{two}");
    let multibyte = censor("api_key=héllo wörld ünïcode téxt here!!");
    assert!(multibyte.starts_with("api_key=x"));
    assert!(std::str::from_utf8(multibyte.as_bytes()).is_ok());
}

#[test]
fn rotate_shifts_older_logs_and_drops_the_tenth() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("jellyfin-desktop.log");
    fs::write(&main, "current").unwrap();
    for i in 1..=MAX_LOGS {
        fs::write(dir.path().join(format!("jellyfin-desktop.log.{i}")), format!("old{i}")).unwrap();
    }
    let path = rotate(dir.path());
    assert_eq!(path, main);
    assert!(!main.exists(), "the main log was moved to .1");
    assert_eq!(fs::read_to_string(dir.path().join("jellyfin-desktop.log.1")).unwrap(), "current");
    assert_eq!(fs::read_to_string(dir.path().join("jellyfin-desktop.log.2")).unwrap(), "old1");
    assert_eq!(
        fs::read_to_string(dir.path().join(format!("jellyfin-desktop.log.{MAX_LOGS}"))).unwrap(),
        format!("old{}", MAX_LOGS - 1)
    );
    // rotating an empty dir just returns the main path
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(rotate(empty.path()), empty.path().join("jellyfin-desktop.log"));
}

#[test]
fn init_writes_censored_lines_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    let path = init(&logs, "debug");
    assert_eq!(path, logs.join("jellyfin-desktop.log"));
    tracing::info!("hello api_key=0123456789abcdef0123456789abcdef end");
    tracing::debug!("debug line");
    // The writer flushes per line; give the subscriber a moment anyway.
    std::thread::sleep(std::time::Duration::from_millis(50));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("hello api_key=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx end"), "{text}");
    assert!(!text.contains("0123456789abcdef"));
    assert!(text.contains("debug line"));
    // A second init in the same process is a no-op for the global subscriber
    // (tracing only takes one); it must not panic.
    let other = dir.path().join("logs2");
    let _ = std::panic::catch_unwind(|| init(&other, "info"));
}

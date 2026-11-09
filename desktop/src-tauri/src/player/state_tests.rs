use super::*;

fn t0() -> Instant {
    Instant::now()
}

#[test]
fn idle_tracker_reports_nothing() {
    let mut t = Tracker::new();
    assert_eq!(t.update(t0()), StateUpdate::default());
    assert_eq!(t.state, State::Finished);
    assert_eq!(State::Finished.name(), "finished");
    assert_eq!(State::Buffering.name(), "buffering");
    assert_eq!(State::Error.name(), "error");
    assert_eq!(State::Canceled.name(), "canceled");
    let _ = Tracker::default();
}

#[test]
fn start_buffers_then_plays_then_pauses() {
    let mut t = Tracker::new();
    t.start_file();
    let u = t.update(t0());
    assert_eq!(u.transition, Some((State::Buffering, State::Finished)));
    // entering buffering resets the 100% marker to 0 and reports it
    assert_eq!(u.buffering_percent, Some(0));
    assert_eq!(u.video_active, None);

    t.buffering_percentage = 40;
    let u = t.update(t0());
    assert_eq!(u.transition, None);
    assert_eq!(u.buffering_percent, Some(40));
    // same percentage again: nothing new
    assert_eq!(t.update(t0()).buffering_percent, None);

    t.playback_active = true;
    t.window_visible = true;
    let u = t.update(t0());
    assert_eq!(u.transition, Some((State::Playing, State::Buffering)));
    assert_eq!(u.video_active, Some(true));
    assert_eq!(u.buffering_percent, None);

    t.paused = true;
    let u = t.update(t0());
    assert_eq!(u.transition, Some((State::Paused, State::Playing)));
    assert_eq!(u.video_active, Some(false));

    t.paused = false;
    assert_eq!(t.update(t0()).transition, Some((State::Playing, State::Paused)));
}

#[test]
fn end_reasons_map_to_states() {
    let mut t = Tracker::new();
    t.start_file();
    t.playback_active = true;
    t.update(t0());

    t.end_file(EndReason::Stop);
    assert_eq!(t.update(t0()).transition, Some((State::Canceled, State::Playing)));

    // mpv reports core-idle again before the next file starts
    t.playback_active = false;
    t.start_file();
    t.update(t0());
    t.end_file(EndReason::Error);
    assert_eq!(t.update(t0()).transition, Some((State::Error, State::Buffering)));
    assert_eq!(t.playback_error, "playback error");

    t.start_file();
    t.update(t0());
    t.end_file(EndReason::Eof);
    assert_eq!(t.update(t0()).transition, Some((State::Finished, State::Buffering)));
    assert!(t.playback_error.is_empty() && !t.playback_canceled);
}

#[test]
fn video_active_needs_a_visible_window() {
    let mut t = Tracker::new();
    t.start_file();
    t.playback_active = true;
    let u = t.update(t0());
    assert_eq!(u.transition, Some((State::Playing, State::Finished)));
    assert_eq!(u.video_active, None);
    t.window_visible = true;
    assert_eq!(t.update(t0()).video_active, Some(true));
    t.window_visible = false;
    assert_eq!(t.update(t0()).video_active, Some(false));
}

#[test]
fn position_reports_are_rate_limited_but_seeks_pass() {
    let mut t = Tracker::new();
    let start = Instant::now();
    // first report after the interval
    assert_eq!(t.position_update(0.5, start + POSITION_INTERVAL), Some(500));
    // tiny advance right after: suppressed
    assert_eq!(t.position_update(0.52, start + POSITION_INTERVAL + Duration::from_millis(10)), None);
    // below the 15 ms jitter threshold even when due
    assert_eq!(t.position_update(0.51, start + 2 * POSITION_INTERVAL), None);
    // a seek goes through immediately
    assert_eq!(t.position_update(42.0, start + POSITION_INTERVAL + Duration::from_millis(20)), Some(42000));
    // due again after the interval
    assert_eq!(t.position_update(42.3, start + 3 * POSITION_INTERVAL), Some(42300));
    // negative positions clamp to 0
    assert_eq!(t.position_update(-5.0, start + 10 * POSITION_INTERVAL), Some(0));
}

#[test]
fn cache_ranges_convert_to_ticks() {
    let ranges = cache_ranges(r#"{"seekable-ranges":[{"start":0.5,"end":12.25},{"start":100,"end":101}]}"#);
    assert_eq!(ranges, vec![json!({"start": 5_000_000, "end": 122_500_000}), json!({"start": 1_000_000_000, "end": 1_010_000_000})]);
    assert!(cache_ranges("{}").is_empty());
    assert!(cache_ranges("not json").is_empty());
    assert_eq!(cache_ranges(r#"{"seekable-ranges":[{}]}"#), vec![json!({"start": 0, "end": 0})]);
}

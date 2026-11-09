use super::*;

#[test]
fn loadfile_options_cover_start_music_autoplay_and_ua() {
    let plain = loadfile_options(&json!({}), &json!({}));
    assert_eq!(plain, "aid=no,sid=no,pause=yes,ad=,vd=");
    let full = loadfile_options(
        &json!({"startMilliseconds": 90500, "autoplay": true}),
        &json!({"type": "music", "headers": {"User-Agent": "JF/1"}}),
    );
    assert_eq!(full, "start=+90.5,aid=no,sid=no,vid=no,pause=no,user-agent=JF/1,ad=,vd=");
    let empty_ua = loadfile_options(&json!({"autoplay": false}), &json!({"headers": {"User-Agent": ""}}));
    assert!(!empty_ua.contains("user-agent"));
}

#[test]
fn hwdec_mapping() {
    assert_eq!(hwdec_options("enabled"), ("auto-safe", "no"));
    assert_eq!(hwdec_options("copy"), ("auto-copy-safe", "no"));
    assert_eq!(hwdec_options("disabled"), ("no", "no"));
    assert_eq!(hwdec_options(""), ("no", "no"));
}

#[test]
fn passthrough_depends_on_device_type() {
    let all = |_: &str| true;
    assert!(passthrough_codecs("basic", all).is_empty());
    assert_eq!(passthrough_codecs("spdif", all), vec!["ac3", "dts"]);
    // dts-hd drops plain dts
    assert_eq!(passthrough_codecs("hdmi", all), vec!["ac3", "eac3", "dts-hd", "truehd"]);
    let only_dts = |c: &str| c == "dts";
    assert_eq!(passthrough_codecs("hdmi", only_dts), vec!["dts"]);
    assert!(passthrough_codecs("weird", all).is_empty());
    assert_eq!(channel_layout("spdif", "7.1"), "2.0");
    assert_eq!(channel_layout("hdmi", "7.1"), "7.1");
    assert!(ac3_transcoding("spdif", true));
    assert!(!ac3_transcoding("spdif", false));
    assert!(!ac3_transcoding("hdmi", true));
}

#[test]
fn cache_budgets() {
    assert_eq!(disk_cache_budget(DISK_RESERVE + DISK_CACHE_MIN - 1), None);
    assert_eq!(disk_cache_budget(DISK_RESERVE + DISK_CACHE_MIN), Some(DISK_CACHE_MIN));
    assert_eq!(disk_cache_budget(100 * GIB), Some(DISK_CACHE_MAX));
    assert_eq!(disk_cache_budget(0), None);
    assert_eq!(disk_cache_bytes(8 * GIB), (8 * GIB, 2 * GIB));
    assert_eq!(memory_cache_bytes(150), (150 * 1024 * 1024, 75 * 1024 * 1024));
    // never below 10 MB
    assert_eq!(memory_cache_bytes(0), (10 * 1024 * 1024, 5 * 1024 * 1024));
}

#[test]
fn aspect_modes() {
    let normal = aspect_settings("normal", 1.78).unwrap();
    assert_eq!(normal, AspectSettings { unscaled: false, force_aspect: "no".into(), keep_aspect: true, panscan: 0.0 });
    assert_eq!(aspect_settings("custom", 1.78), None);
    assert_eq!(aspect_settings("zoom", 1.78).unwrap().panscan, 1.0);
    assert_eq!(aspect_settings("force_4_3", 1.78).unwrap().force_aspect, "4:3");
    assert_eq!(aspect_settings("force_16_9", 1.78).unwrap().force_aspect, "16:9");
    assert_eq!(aspect_settings("force_16_9_if_4_3", 1.333).unwrap().force_aspect, "16:9");
    assert_eq!(aspect_settings("force_16_9_if_4_3", 1.78).unwrap().force_aspect, "no");
    assert!(!aspect_settings("stretch", 1.78).unwrap().keep_aspect);
    assert!(aspect_settings("noscaling", 1.78).unwrap().unscaled);
}

#[test]
fn other_conf_lines() {
    let parsed = parse_other_conf("  hwdec = \"d3d11va\"  \n\nno-equals\n=novalue\nkey=\n\"deband\"=yes\n");
    assert_eq!(parsed, vec![("hwdec".to_string(), "d3d11va".to_string()), ("deband".to_string(), "yes".to_string())]);
    assert!(parse_other_conf("").is_empty());
}

#[test]
fn audio_delay_keys_follow_refresh_rate() {
    assert_eq!(audio_delay_key(23.976), "audio_delay.24hz");
    assert_eq!(audio_delay_key(25.0), "audio_delay.25hz");
    assert_eq!(audio_delay_key(50.0), "audio_delay.50hz");
    assert_eq!(audio_delay_key(59.94), "audio_delay.normal");
    assert_eq!(audio_delay_key(0.0), "audio_delay.normal");
}

#[test]
fn subtitle_helpers() {
    assert_eq!(subtitle_back_color("#000000", "80"), Some("#80000000".into()));
    assert_eq!(subtitle_back_color("", "80"), None);
    assert_eq!(subtitle_back_color("#000000", ""), None);
    assert_eq!(subtitle_placement("center,bottom"), Some(("center".into(), 100)));
    assert_eq!(subtitle_placement("left,top"), Some(("left".into(), 10)));
    assert_eq!(subtitle_placement(""), None);
    assert_eq!(subtitle_placement("a,b,c"), None);
    assert_eq!(subtitle_scale(-1), None);
    assert_eq!(subtitle_scale(32), Some(1.0));
    assert_eq!(subtitle_scale(48), Some(1.5));
}

#[test]
fn stream_selection_forms() {
    assert_eq!(parse_stream_selection(&json!(-1)), StreamSelection::Disable);
    assert_eq!(parse_stream_selection(&json!(2)), StreamSelection::Index(2));
    assert_eq!(parse_stream_selection(&Value::Null), StreamSelection::Disable);
    assert_eq!(parse_stream_selection(&json!("")), StreamSelection::Disable);
    assert_eq!(
        parse_stream_selection(&json!("#3")),
        StreamSelection::External { id: "3".into(), name: String::new() }
    );
    assert_eq!(
        parse_stream_selection(&json!("#,https://h/sub.srt")),
        StreamSelection::External { id: String::new(), name: "https://h/sub.srt".into() }
    );
    assert_eq!(
        parse_stream_selection(&json!("#2,https://h/a.srt")),
        StreamSelection::External { id: "2".into(), name: "https://h/a.srt".into() }
    );
    // Anything else is an "external" selection without id or url.
    assert_eq!(
        parse_stream_selection(&json!(true)),
        StreamSelection::External { id: String::new(), name: String::new() }
    );
}

fn tracks() -> Vec<Value> {
    vec![
        json!({"id": 1, "type": "audio", "external": false}),
        json!({"id": 2, "type": "sub", "external": false}),
        json!({"id": 3, "type": "sub", "external": true, "external-filename": "https://h/a.srt"}),
        json!({"id": "4", "type": "audio", "external": true, "external-filename": "https://h/b.mka"}),
    ]
}

#[test]
fn track_lookup_by_url() {
    let t = tracks();
    assert_eq!(tracks_for_url(&t, "").len(), 2);
    assert_eq!(tracks_for_url(&t, "https://h/a.srt").len(), 1);
    assert!(tracks_for_url(&t, "https://h/none").is_empty());
}

#[test]
fn choosing_tracks() {
    let t = tracks();
    assert_eq!(choose_track(&t, "sub", "7", "", false), "7");
    assert_eq!(choose_track(&t, "sub", "", "https://h/a.srt", false), "3");
    assert_eq!(choose_track(&t, "audio", "", "https://h/b.mka", true), "4");
    // wrong type for that url -> nothing for subs, track 1 for audio
    assert_eq!(choose_track(&t, "sub", "", "https://h/b.mka", false), "no");
    assert_eq!(choose_track(&t, "audio", "", "https://h/a.srt", true), "1");
    assert_eq!(choose_track(&t, "sub", "", "https://h/none", false), "no");
    assert_eq!(json_to_string(&json!(5)), "5");
    assert_eq!(json_to_string(&json!("x")), "x");
}

#[test]
fn audio_devices() {
    let list = json!([
        {"name": "auto", "description": "Autoselect device"},
        {"name": "wasapi/abc", "description": "Speakers"}
    ]);
    let (options, set, found) = audio_device_options(&list, "wasapi/abc");
    assert!(found);
    assert_eq!(options.len(), 2);
    assert!(set.contains("wasapi/abc") && set.contains("auto"));
    let (options, _, found) = audio_device_options(&list, "wasapi/gone");
    assert!(!found);
    assert_eq!(options.len(), 3);
    assert_eq!(options[2]["title"], "[Disconnected device: wasapi/gone]");
    let (options, set, found) = audio_device_options(&Value::Null, "x");
    assert!(!found && set.is_empty() && options.len() == 1);

    assert_eq!(effective_audio_device("", &set), "auto");
    assert_eq!(effective_audio_device("auto", &set), "auto");
    let present: HashSet<String> = ["wasapi/abc".to_string()].into_iter().collect();
    assert_eq!(effective_audio_device("wasapi/abc", &present), "wasapi/abc");
    assert_eq!(effective_audio_device("wasapi/zzz", &present), "auto");
}

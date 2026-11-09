use super::*;
use serde_json::json;

const DESCRIPTION: &str = include_str!("../../settings_description.json");

fn fresh() -> (tempfile::TempDir, Settings) {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings::load(DESCRIPTION, dir.path()).unwrap();
    (dir, s)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn fresh_profile_gets_defaults_and_files() {
    let (dir, s) = fresh();
    assert!(dir.path().join("jellyfin-desktop.conf").exists());
    assert!(dir.path().join("storage.json").exists());
    assert_eq!(s.string(SECTION_MAIN, "layout"), "desktop");
    assert!(!s.bool(SECTION_MAIN, "fullscreen"));
    assert_eq!(s.string(SECTION_VIDEO, "hardwareDecoding"), "enabled");
    assert!(s.bool(SECTION_VIDEO, "cache_on_disk"));
    assert_eq!(s.i64(SECTION_VIDEO, "cache"), 150);
    assert_eq!(s.string(SECTION_MAIN, "logLevel"), "info");
    // the migration marker is written on first load
    assert_eq!(s.i64(SECTION_MAIN, "settingsRevision"), 1);
    let conf = read_json(&dir.path().join("jellyfin-desktop.conf"));
    assert_eq!(conf["version"], 7);
    assert_eq!(conf["sections"]["main"]["settingsRevision"], 1);
    assert!(conf["sections"].get("state").is_none(), "storage sections stay out of the conf");
    s.log_summary();
    assert_eq!(s.conf_path(), dir.path().join("jellyfin-desktop.conf"));
}

#[test]
fn set_value_persists_and_reports_changes() {
    let (dir, mut s) = fresh();
    let up = s.set_value(SECTION_MAIN, "fullscreen", json!(true));
    assert_eq!(up.values, vec![(SECTION_MAIN.to_string(), {
        let mut m = Map::new();
        m.insert("fullscreen".into(), json!(true));
        m
    })]);
    assert!(!up.is_empty());
    // same value again: nothing to report
    assert!(s.set_value(SECTION_MAIN, "fullscreen", json!(true)).is_empty());
    // unknown section: ignored
    assert!(s.set_value("nope", "x", json!(1)).is_empty());
    // "index" is reserved by the web client and ignored
    assert!(s.set_value(SECTION_MAIN, "index", json!(3)).is_empty());

    let reloaded = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert!(reloaded.bool(SECTION_MAIN, "fullscreen"));
    assert_eq!(reloaded.user_web_client(), "");
}

#[test]
fn typed_getters_coerce() {
    let (_d, mut s) = fresh();
    s.set_value_no_save(SECTION_VIDEO, "cache", json!("250"));
    assert_eq!(s.i64(SECTION_VIDEO, "cache"), 250);
    assert_eq!(s.f64(SECTION_VIDEO, "cache"), 250.0);
    s.set_value_no_save(SECTION_VIDEO, "cache", json!(12.7));
    assert_eq!(s.i64(SECTION_VIDEO, "cache"), 12);
    assert_eq!(s.f64(SECTION_VIDEO, "cache"), 12.7);
    s.set_value_no_save(SECTION_VIDEO, "cache", json!(true));
    assert_eq!(s.i64(SECTION_VIDEO, "cache"), 0);
    assert_eq!(s.f64(SECTION_VIDEO, "cache"), 0.0);
    assert_eq!(s.string(SECTION_VIDEO, "cache"), "true");
    assert_eq!(s.string(SECTION_MAIN, "missing"), "");
    assert_eq!(s.value("missing-section", "x"), Value::Null);
    assert_eq!(s.value_opt(SECTION_MAIN, "missing"), None);
    assert_eq!(s.value_opt(SECTION_MAIN, "layout"), Some(json!("desktop")));
    assert!(!s.bool(SECTION_MAIN, "missing"));
}

#[test]
fn set_value_no_save_null_resets_and_adhoc_keys_vanish() {
    let (_d, mut s) = fresh();
    s.set_value_no_save(SECTION_MAIN, "fullscreen", json!(true));
    let up = s.set_value_no_save(SECTION_MAIN, "fullscreen", Value::Null);
    assert_eq!(up.values[0].1["fullscreen"], json!(false));
    // resetting a default value changes nothing
    assert!(s.set_value_no_save(SECTION_MAIN, "fullscreen", Value::Null).is_empty());
    // ad-hoc key in a storage section
    s.set_value_no_save(SECTION_STATE, "Width", json!(1920));
    assert_eq!(s.value_opt(SECTION_STATE, "Width"), Some(json!(1920)));
    assert!(s.set_value_no_save(SECTION_STATE, "Width", json!(1920)).is_empty());
    s.set_value_no_save(SECTION_STATE, "Width", Value::Null);
    assert_eq!(s.value_opt(SECTION_STATE, "Width"), None);
    assert!(s.set_value_no_save(SECTION_STATE, "index", json!(1)).is_empty());
    assert!(s.set_value_no_save("nope", "x", json!(1)).is_empty());
}

#[test]
fn storage_api_handles_maps_null_and_strings() {
    let (dir, mut s) = fresh();
    let mut map = Map::new();
    map.insert("a".into(), json!(1));
    map.insert("b".into(), json!("two"));
    map.insert("".into(), json!("ignored"));
    let up = s.set_values("webstate", Value::Object(map.clone()));
    assert_eq!(up.values[0].0, "webstate");
    assert_eq!(s.all_values("webstate"), json!({"a": 1, "b": "two"}));
    // whole-section replace drops keys that are not in the new map
    let mut map2 = Map::new();
    map2.insert("a".into(), json!(1));
    let up = s.set_values("webstate", Value::Object(map2));
    assert!(up.values.is_empty(), "unchanged key, removed key: nothing reported for described-less removals");
    assert_eq!(s.all_values("webstate"), json!({"a": 1}));
    // null resets the section
    s.set_values("webstate", Value::Null);
    assert_eq!(s.all_values("webstate"), json!({}));
    // strings go to the webclient section, readable via all_values(key)
    s.set_values("serverId", json!("abc"));
    assert_eq!(s.all_values("serverId"), json!("abc"));
    assert_eq!(s.all_values("unknown-key"), Value::Null);
    // unsupported type is ignored
    assert!(s.set_values("x", json!(5)).is_empty());

    let storage = read_json(&dir.path().join("storage.json"));
    assert_eq!(storage["sections"]["webclient"]["serverId"], "abc");
    let reloaded = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert_eq!(reloaded.all_values("serverId"), json!("abc"));
}

#[test]
fn remove_and_reset() {
    let (_d, mut s) = fresh();
    s.set_values("serverId", json!("abc"));
    s.set_values("token", json!("x"));
    // Ad-hoc web client keys are dropped silently (there is no default to announce).
    let up = s.remove_value("serverId");
    assert!(up.is_empty());
    assert_eq!(s.all_values("serverId"), Value::Null);
    assert_eq!(s.all_values("token"), json!("x"));
    // a key that happens to be a section name returns that section
    assert!(s.all_values(SECTION_OTHER).is_object());
    // removing a whole section resets it
    s.set_value(SECTION_MAIN, "fullscreen", json!(true));
    s.remove_value(SECTION_MAIN);
    assert!(!s.bool(SECTION_MAIN, "fullscreen"));
    // reset_to_default on one section / all
    s.set_value(SECTION_VIDEO, "cache", json!(999));
    s.set_value(SECTION_MAIN, "layout", json!("tv"));
    let up = s.reset_to_default(SECTION_VIDEO);
    assert_eq!(up.values[0].1["cache"], 150);
    assert_eq!(s.string(SECTION_MAIN, "layout"), "tv");
    assert!(s.reset_to_default("nope").is_empty());
    let up = s.reset_to_default_all();
    assert!(up.values.iter().any(|(sec, m)| sec == SECTION_MAIN && m["layout"] == "desktop"));
    assert_eq!(s.string(SECTION_MAIN, "layout"), "desktop");
}

#[test]
fn descriptions_and_ordering() {
    let (_d, mut s) = fresh();
    let sections = s.ordered_sections();
    let names: Vec<&str> = sections.as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert!(names.contains(&"main") && names.contains(&"video") && names.contains(&"audio"));
    assert!(!names.contains(&"state"), "storage/hidden sections are not listed");
    let descs = s.setting_descriptions();
    let main = descs.as_array().unwrap().iter().find(|d| d["key"] == "main").unwrap();
    let keys: Vec<&str> = main["settings"].as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"fullscreen"));
    assert!(!keys.contains(&"settingsRevision"), "hidden values are not described");
    let map = s.setting_descriptions_map();
    assert!(map.get("main").is_some() && map.get("audio").is_some());
    let hw = map["video"].as_array().unwrap().iter().find(|v| v["key"] == "hardwareDecoding").unwrap();
    assert!(hw["options"].as_array().unwrap().len() >= 2);

    let up = s.update_possible_values(SECTION_AUDIO, "device", vec![json!({"value": "x", "title": "X"})]);
    assert_eq!(up.groups.len(), 1);
    assert_eq!(up.groups[0].0, SECTION_AUDIO);
    assert_eq!(s.section(SECTION_AUDIO).unwrap().possible_values("device"), vec![json!({"value": "x", "title": "X"})]);
    assert!(s.update_possible_values("nope", "x", vec![]).is_empty());
    let sec = s.section(SECTION_AUDIO).unwrap();
    assert_eq!(sec.default_value("channels"), json!("2.0"));
    assert_eq!(sec.default_value("missing"), Value::Null);
    assert!(sec.possible_values("missing").is_empty());
    assert_eq!(sec.value("missing"), Value::Null);
    assert_eq!(sec.section_order()["key"], SECTION_AUDIO);
    let everything = s.all_values("");
    assert!(everything.get("main").is_some() && everything.get("state").is_some());
}

#[test]
fn migration_resets_old_defaults_only() {
    let dir = tempfile::tempdir().unwrap();
    // A profile written by the previous build: old defaults pinned, one deliberate choice.
    let conf = json!({
        "version": 7,
        "sections": {
            "video": {"hardwareDecoding": "copy", "cache": 75},
            "main": {"logLevel": "debug", "layout": "tv"}
        }
    });
    fs::write(dir.path().join("jellyfin-desktop.conf"), conf.to_string()).unwrap();
    let s = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert_eq!(s.string(SECTION_VIDEO, "hardwareDecoding"), "enabled");
    assert_eq!(s.i64(SECTION_VIDEO, "cache"), 150);
    assert_eq!(s.string(SECTION_MAIN, "logLevel"), "info");
    assert_eq!(s.string(SECTION_MAIN, "layout"), "tv");
    assert_eq!(s.i64(SECTION_MAIN, "settingsRevision"), 1);

    // A deliberate choice equal to a *new* value is untouched, and the migration runs once.
    let conf2 = json!({
        "version": 7,
        "sections": {"video": {"hardwareDecoding": "disabled"}, "main": {"settingsRevision": 1, "logLevel": "debug"}}
    });
    fs::write(dir.path().join("jellyfin-desktop.conf"), conf2.to_string()).unwrap();
    let s2 = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert_eq!(s2.string(SECTION_VIDEO, "hardwareDecoding"), "disabled");
    assert_eq!(s2.string(SECTION_MAIN, "logLevel"), "debug", "already migrated profiles keep debug");
}

#[test]
fn version_mismatch_moves_the_file_aside() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("jellyfin-desktop.conf"),
        json!({"version": 3, "sections": {"main": {"layout": "tv"}}}).to_string(),
    )
    .unwrap();
    fs::write(dir.path().join("storage.json"), json!({"version": 3, "sections": {}}).to_string()).unwrap();
    let s = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert_eq!(s.string(SECTION_MAIN, "layout"), "desktop");
    assert!(dir.path().join("jellyfin-desktop.conf.broken").exists());
    assert!(dir.path().join("storage.json.broken").exists());
    assert_eq!(read_json(&dir.path().join("jellyfin-desktop.conf"))["version"], 7);
}

#[test]
fn unparsable_conf_and_unknown_sections() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("jellyfin-desktop.conf"), "{ not json").unwrap();
    let s = Settings::load(DESCRIPTION, dir.path()).unwrap();
    assert_eq!(s.string(SECTION_MAIN, "layout"), "desktop");

    let dir2 = tempfile::tempdir().unwrap();
    fs::write(
        dir2.path().join("jellyfin-desktop.conf"),
        json!({"version": 7, "sections": {"bogus": {"a": 1}, "main": {"layout": "tv"}, "video": 5}}).to_string(),
    )
    .unwrap();
    fs::write(
        dir2.path().join("storage.json"),
        json!({"version": 7, "sections": {"webstate": {"k": "v"}}}).to_string(),
    )
    .unwrap();
    let s = Settings::load(DESCRIPTION, dir2.path()).unwrap();
    assert!(s.section("bogus").is_none(), "unknown sections in the conf are ignored");
    assert_eq!(s.all_values("webstate"), json!({"k": "v"}), "storage sections are created on demand");
    assert_eq!(s.string(SECTION_MAIN, "layout"), "tv");
}

#[test]
fn description_parsing_handles_platforms_and_defaults() {
    let desc = json!([
        {"section": "__meta__", "version": 7},
        {"section": "nope"},
        {"section": "linuxonly", "order": 5, "platforms": "linux", "values": [{"value": "x", "default": 1}]},
        {"section": "anywhere", "order": 2, "platforms": ["any"], "values": [
            {"value": "perplatform", "default": [
                {"value": "L", "platforms": ["linux"]},
                {"value": "W", "platforms_excluded": ["osx"]}
            ]},
            {"value": "nodefault"},
            {"value": "perplatform", "default": 2},
            {"value": "winhidden", "default": true, "platforms_excluded": "windows"},
            {"value": "choice", "default": "a", "input_type": "text", "possible_values": [
                ["a", "A"], ["b", "B", {"platforms": "linux"}], ["c", "C", {"platforms_excluded": "linux"}], ["short"]
            ]},
            {"value": "hidden", "default": 0, "hidden": true, "help": "tab\there"}
        ]},
        {"section": "store", "storage": true, "hidden": true, "values": []}
    ])
    .to_string()
    .replace("tab\\there", "tab\there");
    let dir = tempfile::tempdir().unwrap();
    let s = Settings::load(&desc, dir.path()).unwrap();
    assert!(s.section("nope").is_none());
    let anywhere = s.section("anywhere").unwrap();
    assert_eq!(anywhere.value("perplatform"), json!("W"), "first default matching the platform wins");
    assert!(anywhere.value("nodefault").is_null());
    assert!(s.section("linuxonly").unwrap().is_hidden());
    let ordered = s.ordered_sections();
    let listed: Vec<&str> = ordered.as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert_eq!(listed, vec!["anywhere"]);
    let descs = s.setting_descriptions_map();
    let keys: Vec<&str> = descs["anywhere"].as_array().unwrap().iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["perplatform", "choice"], "hidden and other-platform values are not described");
    let choice = descs["anywhere"].as_array().unwrap().iter().find(|v| v["key"] == "choice").unwrap();
    assert_eq!(choice["inputType"], "text");
    assert_eq!(choice["options"], json!([{"value": "a", "title": "A"}, {"value": "c", "title": "C"}]));
    assert!(s.section("store").unwrap().storage);

    // bad descriptions
    assert!(Settings::load("{}", dir.path()).is_err());
    assert!(Settings::load("[1]", dir.path()).is_err());
    assert!(Settings::load(r#"[{"nosection": 1}]"#, dir.path()).is_err());
}

#[test]
fn client_name_falls_back_to_the_host_name() {
    let (_d, mut s) = fresh();
    let default_name = s.client_name();
    assert!(!default_name.is_empty());
    assert_ne!(default_name, "JellyfinDesktop");
    s.set_value_no_save(SECTION_SYSTEM, "systemname", json!("Living Room"));
    assert_eq!(s.client_name(), "Living Room");
    assert_eq!(hostname(), std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Jellyfin Desktop".into()));
}

#[test]
fn platform_filters() {
    let obj = |v: Value| v.as_object().unwrap().clone();
    assert!(platform_ok_from(&obj(json!({}))));
    assert!(platform_ok_from(&obj(json!({"platforms": "windows"}))));
    assert!(platform_ok_from(&obj(json!({"platforms": ["linux", "any"]}))));
    assert!(!platform_ok_from(&obj(json!({"platforms": ["linux"]}))));
    assert!(!platform_ok_from(&obj(json!({"platforms": 5}))));
    assert!(!platform_ok_from(&obj(json!({"platforms_excluded": "windows"}))));
    assert!(!platform_ok_from(&obj(json!({"platforms_excluded": ["osx", "windows"]}))));
    assert!(platform_ok_from(&obj(json!({"platforms_excluded": ["osx"]}))));
    assert!(platform_ok_from(&obj(json!({"platforms_excluded": 5}))));
}

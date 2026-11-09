use super::*;

fn temp_paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        data_root: dir.path().join("data"),
        cache_root: dir.path().join("cache"),
        portable: false,
    };
    fs::create_dir_all(&paths.data_root).unwrap();
    (dir, paths)
}

#[test]
fn resolve_paths_defaults_to_local_app_data() {
    let p = resolve_paths(None, None, Some(PathBuf::from("C:\\lad"))).unwrap();
    assert_eq!(p.data_root, PathBuf::from("C:\\lad").join(APP_DATA_NAME));
    assert_eq!(p.cache_root, PathBuf::from("C:\\lad").join("cache").join(APP_DATA_NAME));
    assert!(!p.portable);
}

#[test]
fn resolve_paths_needs_local_app_data() {
    assert!(resolve_paths(None, None, None).is_err());
}

#[test]
fn resolve_paths_config_dir_overrides_data_root_only() {
    let p = resolve_paths(None, Some(PathBuf::from("D:\\cfg")), Some(PathBuf::from("C:\\lad"))).unwrap();
    assert_eq!(p.data_root, PathBuf::from("D:\\cfg"));
    assert_eq!(p.cache_root, PathBuf::from("C:\\lad").join("cache").join(APP_DATA_NAME));
}

#[test]
fn resolve_paths_portable_marker_moves_both_roots() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("portable"), "").unwrap();
    let p = resolve_paths(Some(dir.path()), None, Some(PathBuf::from("C:\\lad"))).unwrap();
    assert!(p.portable);
    assert_eq!(p.data_root, dir.path().join("data"));
    assert_eq!(p.cache_root, dir.path().join("cache"));
    // portable.txt works too, and --config-dir still wins for data.
    let dir2 = tempfile::tempdir().unwrap();
    fs::write(dir2.path().join("portable.txt"), "").unwrap();
    let p2 = resolve_paths(Some(dir2.path()), Some(PathBuf::from("E:\\x")), Some(PathBuf::from("C:\\lad")))
        .unwrap();
    assert!(p2.portable);
    assert_eq!(p2.data_root, PathBuf::from("E:\\x"));
    assert_eq!(p2.cache_root, dir2.path().join("cache"));
}

#[test]
fn init_paths_creates_the_roots_once() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    let p = init_paths(Some(cfg.clone())).unwrap();
    assert!(p.data_root.is_dir());
    // The global is set once; later calls return the first value.
    let again = init_paths(Some(dir.path().join("other"))).unwrap();
    assert_eq!(p.data_root, again.data_root);
    assert_eq!(paths().data_root, p.data_root);
}

#[test]
fn profile_ids() {
    assert!(is_profile_id("602995625a8249d5b407b9baa953b923"));
    assert!(!is_profile_id("602995625A8249D5B407B9BAA953B923"));
    assert!(!is_profile_id("short"));
    assert!(!is_profile_id("g02995625a8249d5b407b9baa953b923"));
    let id = uuid_simple();
    assert!(is_profile_id(&id), "{id}");
    assert_ne!(uuid_simple(), uuid_simple());
}

#[test]
fn fresh_install_creates_a_default_profile() {
    let (_d, paths) = temp_paths();
    let (p, how) = resolve_active_profile(&paths, None).unwrap();
    assert_eq!(p.name(), "Default");
    assert!(how.contains("no profiles, created one"), "{how}");
    assert!(p.data_dir().is_dir() && p.log_dir().is_dir() && p.cache_dir().is_dir());
    assert_eq!(default_profile(&paths), Some(p.clone()));
    assert_eq!(profiles(&paths), vec![p.clone()]);
    assert_eq!(p.data_file("x.json"), p.data_dir().join("x.json"));
    // Second start: the default is honoured.
    let (p2, how2) = resolve_active_profile(&paths, None).unwrap();
    assert_eq!(p2, p);
    assert!(how2.starts_with("default from"), "{how2}");
}

#[test]
fn requested_profile_by_name() {
    let (_d, paths) = temp_paths();
    let a = create_profile(&paths, "A").unwrap();
    let b = create_profile(&paths, "B").unwrap();
    assert_eq!(profile_by_name(&paths, "B"), Some(b.clone()));
    assert_eq!(profile_by_name(&paths, "C"), None);
    let (p, how) = resolve_active_profile(&paths, Some("A")).unwrap();
    assert_eq!(p, a);
    assert_eq!(how, "--profile A");
    // --profile does not touch profiles.json
    assert!(!profiles_file(&paths).exists());
    let err = resolve_active_profile(&paths, Some("Nope")).unwrap_err();
    assert!(err.to_string().contains("Profile not found"));
}

#[test]
fn default_pointing_at_missing_directory_falls_back_to_first() {
    let (_d, paths) = temp_paths();
    let _a = create_profile(&paths, "A").unwrap();
    let _b = create_profile(&paths, "B").unwrap();
    fs::write(profiles_file(&paths), r#"{"defaultProfile":"00000000000000000000000000000000"}"#).unwrap();
    assert_eq!(default_profile(&paths), None);
    let (p, how) = resolve_active_profile(&paths, None).unwrap();
    let first = profiles(&paths).into_iter().next().unwrap();
    assert_eq!(p, first);
    assert!(how.contains("has no directory") && how.contains("first existing"), "{how}");
    assert_eq!(default_profile(&paths), Some(first));
}

#[test]
fn unparsable_or_empty_profiles_file() {
    let (_d, paths) = temp_paths();
    let a = create_profile(&paths, "A").unwrap();
    fs::write(profiles_file(&paths), "not json").unwrap();
    let (p, how) = resolve_active_profile(&paths, None).unwrap();
    assert_eq!(p, a);
    assert!(how.contains("unparsable"), "{how}");
    fs::write(profiles_file(&paths), r#"{"defaultProfile":""}"#).unwrap();
    let (_, how) = resolve_active_profile(&paths, None).unwrap();
    assert!(how.contains("no defaultProfile"), "{how}");
    fs::remove_file(profiles_file(&paths)).unwrap();
    let (_, how) = resolve_active_profile(&paths, None).unwrap();
    assert!(how.contains("unreadable"), "{how}");
}

#[test]
fn deleting_the_default_promotes_another_profile() {
    let (_d, paths) = temp_paths();
    let a = create_profile(&paths, "A").unwrap();
    let b = create_profile(&paths, "B").unwrap();
    set_default_profile(&paths, &a).unwrap();
    delete_profile(&paths, &a).unwrap();
    assert!(!a.data_dir().exists());
    assert_eq!(profiles(&paths), vec![b.clone()]);
    assert_eq!(default_profile(&paths), Some(b.clone()));
    delete_profile(&paths, &b).unwrap();
    assert!(profiles(&paths).is_empty());
    assert_eq!(default_profile(&paths), None);
}

#[test]
fn stray_directories_are_not_profiles() {
    let (_d, paths) = temp_paths();
    fs::create_dir_all(profiles_dir(&paths).join("not-a-profile")).unwrap();
    fs::write(profiles_dir(&paths).join("602995625a8249d5b407b9baa953b923"), "file").unwrap();
    assert!(profiles(&paths).is_empty());
    let p = Profile::new(&paths, "602995625a8249d5b407b9baa953b923".into());
    assert_eq!(p.name(), "");
}

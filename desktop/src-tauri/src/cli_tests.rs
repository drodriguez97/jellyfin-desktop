use super::*;
use crate::profiles::{create_profile, default_profile, profiles, set_default_profile, Paths};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn temp_paths() -> (tempfile::TempDir, Paths) {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        data_root: dir.path().join("data"),
        cache_root: dir.path().join("cache"),
        portable: false,
    };
    std::fs::create_dir_all(&paths.data_root).unwrap();
    (dir, paths)
}

#[test]
fn no_arguments_gives_defaults() {
    assert_eq!(parse_args(vec![]), Cli::default());
    // The test binary's own arguments are not ours; they must not crash the parser.
    let _ = parse_cli();
}

#[test]
fn values_with_space_and_with_equals() {
    let a = parse_args(args(&["--profile", "Living Room", "--config-dir=D:\\cfg", "--log-level", "debug"]));
    assert_eq!(a.profile.as_deref(), Some("Living Room"));
    assert_eq!(a.config_dir, Some(PathBuf::from("D:\\cfg")));
    assert_eq!(a.log_level.as_deref(), Some("debug"));
    let b = parse_args(args(&["--remote-debugging-port=9229", "--create-profile", "X"]));
    assert_eq!(b.remote_debugging_port.as_deref(), Some("9229"));
    assert_eq!(b.create_profile.as_deref(), Some("X"));
}

#[test]
fn flags_and_layouts() {
    let a = parse_args(args(&["--fullscreen", "--tv", "--ignore-certificate-errors", "--list-profiles"]));
    assert_eq!(a.fullscreen, Some(true));
    assert_eq!(a.layout.as_deref(), Some("tv"));
    assert!(a.ignore_certificate_errors && a.list_profiles);
    let b = parse_args(args(&["--windowed", "--desktop", "-h"]));
    assert_eq!(b.fullscreen, Some(false));
    assert_eq!(b.layout.as_deref(), Some("desktop"));
    assert!(b.help);
    assert!(parse_args(args(&["--help"])).help);
    let c = parse_args(args(&["--delete-profile", "A", "--set-default-profile", "B"]));
    assert_eq!(c.delete_profile.as_deref(), Some("A"));
    assert_eq!(c.set_default_profile.as_deref(), Some("B"));
}

#[test]
fn unknown_and_dangling_arguments_are_ignored() {
    let a = parse_args(args(&["--bogus", "value", "--profile"]));
    assert_eq!(a, Cli::default());
}

#[test]
fn help_and_list_exit_early() {
    let (_d, paths) = temp_paths();
    assert!(handle_profile_commands(&Cli { help: true, ..Cli::default() }, &paths).unwrap());
    assert!(HELP.contains("--profile <name>"));
    // Listing with no profiles, then with two (one default).
    assert!(handle_profile_commands(&Cli { list_profiles: true, ..Cli::default() }, &paths).unwrap());
    let a = create_profile(&paths, "A").unwrap();
    create_profile(&paths, "B").unwrap();
    set_default_profile(&paths, &a).unwrap();
    assert!(handle_profile_commands(&Cli { list_profiles: true, ..Cli::default() }, &paths).unwrap());
    assert!(!handle_profile_commands(&Cli::default(), &paths).unwrap());
}

#[test]
fn create_delete_and_set_default() {
    let (_d, paths) = temp_paths();
    let create = |n: &str| Cli { create_profile: Some(n.into()), ..Cli::default() };
    assert!(handle_profile_commands(&create("A"), &paths).unwrap());
    assert!(handle_profile_commands(&create("B"), &paths).unwrap());
    assert_eq!(profiles(&paths).len(), 2);
    let dup = handle_profile_commands(&create("A"), &paths).unwrap_err();
    assert!(dup.to_string().contains("already exists"));

    let set = Cli { set_default_profile: Some("B".into()), ..Cli::default() };
    assert!(handle_profile_commands(&set, &paths).unwrap());
    assert_eq!(default_profile(&paths).unwrap().name(), "B");
    let missing = Cli { set_default_profile: Some("Z".into()), ..Cli::default() };
    assert!(handle_profile_commands(&missing, &paths).unwrap_err().to_string().contains("not found"));

    let del = Cli { delete_profile: Some("B".into()), ..Cli::default() };
    assert!(handle_profile_commands(&del, &paths).unwrap());
    assert_eq!(profiles(&paths).len(), 1);
    assert_eq!(default_profile(&paths).unwrap().name(), "A");
    let del_missing = Cli { delete_profile: Some("B".into()), ..Cli::default() };
    assert!(handle_profile_commands(&del_missing, &paths).unwrap_err().to_string().contains("not found"));
}

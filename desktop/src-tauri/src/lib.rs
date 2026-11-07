//! Jellyfin Desktop shell: WebView2 (jellyfin-web) over libmpv (D3D11).

mod artwork;
mod display;
mod input;
mod logging;
mod media_keys;
mod player;
mod power;
mod profiles;
mod settings;
mod shell;
mod system;
mod tray;
mod video_host;
mod window;

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tauri::webview::{Color, WebviewWindowBuilder};
use tauri::{Manager, RunEvent, WebviewUrl, WindowEvent};
use tracing::{error, info};
use windows_sys::Win32::Foundation::HWND;

const SETTINGS_DESCRIPTION: &str = include_str!("../settings_description.json");
const APP_ORIGIN: &str = "http://tauri.localhost";

#[derive(Default, Debug)]
struct Cli {
    profile: Option<String>,
    config_dir: Option<PathBuf>,
    log_level: Option<String>,
    fullscreen: Option<bool>,
    layout: Option<String>,
    ignore_certificate_errors: bool,
    remote_debugging_port: Option<String>,
    list_profiles: bool,
    create_profile: Option<String>,
    delete_profile: Option<String>,
    set_default_profile: Option<String>,
    help: bool,
}

fn parse_cli() -> Cli {
    let mut cli = Cli::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let value = |i: &mut usize, args: &[String]| -> Option<String> {
        let a = &args[*i];
        if let Some((_, v)) = a.split_once('=') {
            return Some(v.to_string());
        }
        *i += 1;
        args.get(*i).cloned()
    };
    while i < args.len() {
        let a = args[i].clone();
        let name = a.split('=').next().unwrap_or("").to_string();
        match name.as_str() {
            "--profile" => cli.profile = value(&mut i, &args),
            "--config-dir" => cli.config_dir = value(&mut i, &args).map(PathBuf::from),
            "--log-level" => cli.log_level = value(&mut i, &args),
            "--remote-debugging-port" => cli.remote_debugging_port = value(&mut i, &args),
            "--create-profile" => cli.create_profile = value(&mut i, &args),
            "--delete-profile" => cli.delete_profile = value(&mut i, &args),
            "--set-default-profile" => cli.set_default_profile = value(&mut i, &args),
            "--list-profiles" => cli.list_profiles = true,
            "--fullscreen" => cli.fullscreen = Some(true),
            "--windowed" => cli.fullscreen = Some(false),
            "--desktop" => cli.layout = Some("desktop".into()),
            "--tv" => cli.layout = Some("tv".into()),
            "--ignore-certificate-errors" => cli.ignore_certificate_errors = true,
            "-h" | "--help" => cli.help = true,
            _ => {}
        }
        i += 1;
    }
    cli
}

const HELP: &str = "Jellyfin Desktop

Options:
  --desktop | --tv              Start in desktop or TV layout
  --windowed | --fullscreen     Start windowed or fullscreen
  --ignore-certificate-errors   Ignore TLS certificate errors
  --remote-debugging-port <n>   Expose WebView2 devtools on this port
  --config-dir <path>           Override the config directory
  --log-level <level>           debug, info, warn, error (default: info)

Profile Options:
  --profile <name>              Use a specific profile for this session
  --list-profiles               List all profiles and exit
  --create-profile <name>       Create a new profile and exit
  --delete-profile <name>       Delete a profile and exit
  --set-default-profile <name>  Set the default profile and exit
";

/// Handles the one-shot profile commands; returns true when the app should exit.
fn handle_profile_commands(cli: &Cli) -> Result<bool> {
    if cli.help {
        print!("{HELP}");
        return Ok(true);
    }
    if cli.list_profiles {
        let default = profiles::default_profile();
        let all = profiles::profiles();
        if all.is_empty() {
            println!("No profiles found.");
        }
        for p in all {
            let marker = if default.as_ref() == Some(&p) { " *" } else { "" };
            println!("{}{marker}", p.name());
        }
        return Ok(true);
    }
    if let Some(name) = &cli.create_profile {
        if profiles::profile_by_name(name).is_some() {
            return Err(anyhow!("Profile already exists: {name}"));
        }
        profiles::create_profile(name)?;
        println!("Created profile: {name}");
        return Ok(true);
    }
    if let Some(name) = &cli.delete_profile {
        let p = profiles::profile_by_name(name).ok_or_else(|| anyhow!("Profile not found: {name}"))?;
        profiles::delete_profile(&p)?;
        println!("Deleted profile: {name}");
        return Ok(true);
    }
    if let Some(name) = &cli.set_default_profile {
        let p = profiles::profile_by_name(name).ok_or_else(|| anyhow!("Profile not found: {name}"))?;
        profiles::set_default_profile(&p)?;
        println!("Default profile set to: {name}");
        return Ok(true);
    }
    Ok(false)
}

fn hwnd_of(window: &tauri::WebviewWindow) -> Result<HWND> {
    let handle = window.window_handle().context("window handle")?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as HWND),
        _ => Err(anyhow!("not a Win32 window")),
    }
}

struct Boot {
    cli: Cli,
    profile: profiles::Profile,
    settings: Arc<Mutex<settings::Settings>>,
}

fn setup(app: &mut tauri::App, boot: &Boot) -> Result<()> {
    let t0 = std::time::Instant::now();
    let user_agent = app.state::<system::SystemState>().user_agent.clone();
    let (init_script, allow_zoom) = {
        let s = boot.settings.lock();
        (
            shell::build_init_script(&s, APP_ORIGIN, &user_agent),
            s.bool(settings::SECTION_MAIN, "allowBrowserZoom"),
        )
    };

    let mut browser_args = String::from(
        "--autoplay-policy=no-user-gesture-required --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
    );
    if !allow_zoom {
        browser_args.push_str(" --disable-pinch");
    }
    if boot.cli.ignore_certificate_errors {
        browser_args.push_str(" --ignore-certificate-errors");
    }
    if let Some(port) = &boot.cli.remote_debugging_port {
        browser_args.push_str(&format!(" --remote-debugging-port={port}"));
    }

    let window = WebviewWindowBuilder::new(app, window::MAIN_LABEL, WebviewUrl::App("index.html".into()))
        .title("Jellyfin Desktop")
        .inner_size(1280.0, 720.0)
        .min_inner_size(213.0, 120.0)
        .visible(false)
        .background_color(Color(0, 0, 0, 0))
        .zoom_hotkeys_enabled(allow_zoom)
        .disable_drag_drop_handler()
        .additional_browser_args(&browser_args)
        .user_agent(&user_agent)
        .data_directory(boot.profile.data_dir().join("WebView2"))
        .initialization_script(init_script)
        .build()
        .context("create main window")?;
    info!("window + webview created in {:?}", t0.elapsed());

    let handle = app.handle().clone();
    window::restore_geometry(&window, &app.state::<window::WindowState>());
    window::install_handlers(&handle, &window);

    // Video host child + mpv.
    let parent = hwnd_of(&window)?;
    let video = video_host::create(parent)?;
    let player = player::Player::new(handle.clone(), boot.settings.clone(), video, parent)?;
    *app.state::<player::commands::PlayerState>().write() = Some(player);
    info!("mpv initialized (video host {:?}) at {:?}", video, t0.elapsed());

    let child = video as isize;
    let parent_i = parent as isize;
    window.on_window_event(move |event| {
        if let WindowEvent::Resized(_) = event {
            video_host::resize_to_parent(child as HWND, parent_i as HWND);
        }
    });

    window.show().context("show window")?;
    window::apply_settings(&handle);
    info!("window shown at {:?}", t0.elapsed());

    *app.state::<media_keys::MediaKeysState>().lock() = media_keys::start(&handle, parent as isize);
    if let Err(e) = tray::install(&handle) {
        error!("tray icon failed: {e:#}");
    }
    Ok(())
}

pub fn run() {
    let cli = parse_cli();

    if let Err(e) = profiles::init_paths(cli.config_dir.clone()) {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
    match handle_profile_commands(&cli) {
        Ok(true) => return,
        Ok(false) => {}
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    }

    let profile = match profiles::resolve_active_profile(cli.profile.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    };

    let mut settings = match settings::Settings::load(SETTINGS_DESCRIPTION, &profile.data_dir()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to load settings: {e:#}");
            std::process::exit(1);
        }
    };

    let level = cli
        .log_level
        .clone()
        .unwrap_or_else(|| settings.string(settings::SECTION_MAIN, "logLevel"));
    let level = match level.as_str() {
        "debug" | "info" | "warn" | "error" => level,
        "fatal" | "disable" => "error".to_string(),
        _ => "info".to_string(),
    };
    let log_path = logging::init(&profile.log_dir(), &level);
    info!(
        "Starting Jellyfin Desktop {} (profile {:?}, log {})",
        env!("CARGO_PKG_VERSION"),
        profile.name(),
        log_path.display()
    );
    info!("Config directory: {}", profile.data_dir().display());
    settings.log_summary();

    if let Some(fs) = cli.fullscreen {
        settings.set_value_no_save(settings::SECTION_MAIN, "fullscreen", json!(fs));
    }
    if let Some(layout) = &cli.layout {
        settings.set_value_no_save(settings::SECTION_MAIN, "layout", json!(layout));
    }
    if cli.ignore_certificate_errors {
        settings.set_value_no_save(settings::SECTION_MAIN, "ignoreSSLErrors", json!(true));
    }

    let settings = Arc::new(Mutex::new(settings));
    let boot = Boot { cli, profile: profile.clone(), settings: settings.clone() };
    let t_ua = std::time::Instant::now();
    let user_agent = system::user_agent();
    info!("user agent {user_agent:?} resolved in {:?}", t_ua.elapsed());
    let t_build = std::time::Instant::now();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage::<settings::commands::SettingsState>(settings.clone())
        .manage::<player::commands::PlayerState>(parking_lot::RwLock::new(None))
        .manage(system::SystemState {
            settings: settings.clone(),
            profile: profile.clone(),
            web_client_version: Mutex::new(String::new()),
            connectivity_generation: AtomicU64::new(0),
            user_agent: user_agent.clone(),
        })
        .manage(Arc::new(system::ConnectivityCtx { settings: settings.clone(), generation: AtomicU64::new(0) }))
        .manage(window::WindowState::new(settings.clone()))
        .manage::<media_keys::MediaKeysState>(parking_lot::Mutex::new(None))
        .invoke_handler(tauri::generate_handler![
            player::commands::player_load,
            player::commands::player_queue_media,
            player::commands::player_clear_queue,
            player::commands::player_play,
            player::commands::player_pause,
            player::commands::player_stop,
            player::commands::player_seek_to,
            player::commands::player_stream_switch,
            player::commands::player_get_position,
            player::commands::player_get_duration,
            player::commands::player_set_volume,
            player::commands::player_volume,
            player::commands::player_set_muted,
            player::commands::player_muted,
            player::commands::player_set_playback_rate,
            player::commands::player_set_audio_stream,
            player::commands::player_set_subtitle_stream,
            player::commands::player_set_audio_delay,
            player::commands::player_set_subtitle_delay,
            player::commands::player_set_video_rectangle,
            player::commands::player_set_video_only_mode,
            player::commands::player_get_audio_device_list,
            player::commands::player_set_audio_device,
            player::commands::player_user_command,
            player::commands::player_set_web_playlist,
            player::commands::player_get_web_playlist,
            player::commands::player_get_current_web_playlist_item_id,
            player::commands::player_notify_shuffle_change,
            player::commands::player_notify_repeat_change,
            player::commands::player_notify_fullscreen_change,
            player::commands::player_notify_rate_change,
            player::commands::player_notify_queue_change,
            player::commands::player_notify_playback_stop,
            player::commands::player_notify_duration_change,
            player::commands::player_notify_playback_state,
            player::commands::player_notify_position,
            player::commands::player_notify_seek,
            player::commands::player_notify_metadata,
            player::commands::player_notify_volume_change,
            settings::commands::settings_set_value,
            settings::commands::settings_set_values,
            settings::commands::settings_value,
            settings::commands::settings_all_values,
            settings::commands::settings_remove_value,
            settings::commands::settings_reset_to_default,
            settings::commands::settings_descriptions,
            settings::commands::settings_ordered_sections,
            system::system_exit,
            system::system_restart,
            system::system_hello,
            system::system_open_external_url,
            system::system_check_server_connectivity,
            system::system_cancel_server_connectivity,
            system::system_check_for_updates,
            system::system_get_capabilities_string,
            system::system_network_addresses,
            system::system_run_user_script,
            system::system_fetch_page_for_csp_workaround,
            input::input_execute_actions,
            input::input_set_volume,
            input::input_seek_to,
            input::input_set_rate,
            window::window_set_fullscreen,
            window::window_is_fullscreen,
            window::window_toggle_fullscreen,
            window::window_set_always_on_top,
            window::window_is_always_on_top,
            window::window_toggle_always_on_top,
            window::window_set_cursor_visibility,
            window::window_raise,
        ])
        .setup(move |app| {
            info!("tauri builder ready in {:?}", t_build.elapsed());
            if let Err(e) = setup(app, &boot) {
                error!("setup failed: {e:#}");
                return Err(Box::new(std::io::Error::other(e.to_string())));
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { .. } = event {
            window::flush_geometry(app);
            display::restore_previous_mode();
            if let Some(p) = player::commands::player_from(app) {
                p.stop();
            }
        }
    });
}

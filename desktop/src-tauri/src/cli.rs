//! Command line parsing and the one-shot profile commands.

use crate::profiles;
use anyhow::{anyhow, Result};
use std::path::PathBuf;

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Cli {
    pub profile: Option<String>,
    pub config_dir: Option<PathBuf>,
    pub log_level: Option<String>,
    pub fullscreen: Option<bool>,
    pub layout: Option<String>,
    pub ignore_certificate_errors: bool,
    pub remote_debugging_port: Option<String>,
    pub list_profiles: bool,
    pub create_profile: Option<String>,
    pub delete_profile: Option<String>,
    pub set_default_profile: Option<String>,
    pub help: bool,
}

pub fn parse_cli() -> Cli {
    parse_args(std::env::args().skip(1).collect())
}

/// Parses the arguments after the program name. Unknown arguments are ignored.
pub fn parse_args(args: Vec<String>) -> Cli {
    let mut cli = Cli::default();
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

pub const HELP: &str = "Jellyfin Desktop

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
pub fn handle_profile_commands(cli: &Cli, root: &profiles::Paths) -> Result<bool> {
    if cli.help {
        print!("{HELP}");
        return Ok(true);
    }
    if cli.list_profiles {
        let default = profiles::default_profile(root);
        let all = profiles::profiles(root);
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
        if profiles::profile_by_name(root, name).is_some() {
            return Err(anyhow!("Profile already exists: {name}"));
        }
        profiles::create_profile(root, name)?;
        println!("Created profile: {name}");
        return Ok(true);
    }
    if let Some(name) = &cli.delete_profile {
        let p = profiles::profile_by_name(root, name).ok_or_else(|| anyhow!("Profile not found: {name}"))?;
        profiles::delete_profile(root, &p)?;
        println!("Deleted profile: {name}");
        return Ok(true);
    }
    if let Some(name) = &cli.set_default_profile {
        let p = profiles::profile_by_name(root, name).ok_or_else(|| anyhow!("Profile not found: {name}"))?;
        profiles::set_default_profile(root, &p)?;
        println!("Default profile set to: {name}");
        return Ok(true);
    }
    Ok(false)
}

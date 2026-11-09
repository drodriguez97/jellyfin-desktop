//! Profile and path handling, compatible with the Qt app's layout:
//!   %LOCALAPPDATA%\Jellyfin Desktop\profiles.json           -> { "defaultProfile": "<id>" }
//!   %LOCALAPPDATA%\Jellyfin Desktop\profiles\<id>\profile.json -> { "name": "..." }
//!   %LOCALAPPDATA%\Jellyfin Desktop\profiles\<id>\jellyfin-desktop.conf, storage.json, logs\, mpv.conf
//! Portable mode: a `portable` or `portable.txt` file next to the exe moves data to <exe>\data
//! and cache to <exe>\cache.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const APP_DATA_NAME: &str = "Jellyfin Desktop";
const PROFILES_DIR: &str = "profiles";

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_root: PathBuf,
    pub cache_root: PathBuf,
    pub portable: bool,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

/// Resolves the global data/cache roots. `config_dir` overrides the data root
/// (the `--config-dir` CLI option); portable mode is detected from the exe dir.
pub fn init_paths(config_dir: Option<PathBuf>) -> Result<&'static Paths> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));

    let mut portable = false;
    let mut data_root = None;
    let mut cache_root = None;

    if let Some(dir) = &exe_dir {
        if dir.join("portable").exists() || dir.join("portable.txt").exists() {
            portable = true;
            data_root = Some(dir.join("data"));
            cache_root = Some(dir.join("cache"));
        }
    }
    if let Some(cfg) = config_dir {
        data_root = Some(cfg);
    }

    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("LOCALAPPDATA not set"))?;
    let data_root = data_root.unwrap_or_else(|| local.join(APP_DATA_NAME));
    let cache_root = cache_root.unwrap_or_else(|| local.join("cache").join(APP_DATA_NAME));
    fs::create_dir_all(&data_root).with_context(|| format!("create {}", data_root.display()))?;
    fs::create_dir_all(&cache_root).ok();

    let paths = Paths { data_root, cache_root, portable };
    let _ = PATHS.set(paths);
    Ok(PATHS.get().unwrap())
}

pub fn paths() -> &'static Paths {
    PATHS.get().expect("init_paths not called")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: String,
}

#[derive(Serialize, Deserialize, Default)]
struct ProfileMeta {
    name: String,
}

#[derive(Serialize, Deserialize, Default)]
struct ProfilesFile {
    #[serde(rename = "defaultProfile", default)]
    default_profile: String,
}

impl Profile {
    pub fn data_dir(&self) -> PathBuf {
        paths().data_root.join(PROFILES_DIR).join(&self.id)
    }
    pub fn data_file(&self, name: &str) -> PathBuf {
        self.data_dir().join(name)
    }
    pub fn cache_dir(&self) -> PathBuf {
        paths().cache_root.join(PROFILES_DIR).join(&self.id)
    }
    pub fn log_dir(&self) -> PathBuf {
        self.data_dir().join("logs")
    }
    pub fn name(&self) -> String {
        fs::read_to_string(self.data_file("profile.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<ProfileMeta>(&s).ok())
            .map(|m| m.name)
            .unwrap_or_default()
    }
    pub fn ensure_directories(&self) -> Result<()> {
        fs::create_dir_all(self.data_dir())?;
        fs::create_dir_all(self.log_dir())?;
        fs::create_dir_all(self.cache_dir())?;
        Ok(())
    }
}

fn profiles_dir() -> PathBuf {
    paths().data_root.join(PROFILES_DIR)
}

fn is_profile_id(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

pub fn profiles() -> Vec<Profile> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(profiles_dir()) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.path().is_dir() && is_profile_id(&name) {
                out.push(Profile { id: name });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn profile_by_name(name: &str) -> Option<Profile> {
    profiles().into_iter().find(|p| p.name() == name)
}

pub fn create_profile(name: &str) -> Result<Profile> {
    let id = uuid_simple();
    let profile = Profile { id };
    profile.ensure_directories()?;
    let meta = ProfileMeta { name: name.to_string() };
    fs::write(profile.data_file("profile.json"), serde_json::to_string_pretty(&meta)?)?;
    Ok(profile)
}

pub fn delete_profile(profile: &Profile) -> Result<()> {
    let _ = fs::remove_dir_all(profile.data_dir());
    let _ = fs::remove_dir_all(profile.cache_dir());
    if let Some(def) = default_profile() {
        if def == *profile {
            if let Some(first) = profiles().into_iter().next() {
                set_default_profile(&first)?;
            }
        }
    }
    Ok(())
}

pub fn default_profile() -> Option<Profile> {
    let file = paths().data_root.join("profiles.json");
    let content = fs::read_to_string(file).ok()?;
    let parsed: ProfilesFile = serde_json::from_str(&content).ok()?;
    if parsed.default_profile.is_empty() {
        return None;
    }
    let p = Profile { id: parsed.default_profile };
    if p.data_dir().is_dir() {
        Some(p)
    } else {
        None
    }
}

pub fn set_default_profile(profile: &Profile) -> Result<()> {
    let file = paths().data_root.join("profiles.json");
    let data = ProfilesFile { default_profile: profile.id.clone() };
    fs::write(file, serde_json::to_string_pretty(&data)?)?;
    Ok(())
}

/// Picks the profile for this session: `--profile <name>`, else the default,
/// else the first existing one, else a freshly created "Default".
/// Also returns a short description of how the profile was chosen, for the log
/// (logging is not up yet at this point, so the caller logs it).
pub fn resolve_active_profile(requested: Option<&str>) -> Result<(Profile, String)> {
    if let Some(name) = requested {
        let p = profile_by_name(name).ok_or_else(|| anyhow!("Profile not found: {name}"))?;
        return Ok((p, format!("--profile {name}")));
    }
    let file = paths().data_root.join("profiles.json");
    let (profile, how) = match default_profile() {
        Some(p) => (p, format!("default from {}", file.display())),
        None => {
            let why = match fs::read_to_string(&file) {
                Err(e) => format!("{} unreadable: {e}", file.display()),
                Ok(c) => match serde_json::from_str::<ProfilesFile>(&c) {
                    Err(e) => format!("{} unparsable: {e}", file.display()),
                    Ok(f) if f.default_profile.is_empty() => format!("{} has no defaultProfile", file.display()),
                    Ok(f) => format!("default {} has no directory", f.default_profile),
                },
            };
            match profiles().into_iter().next() {
                Some(p) => (p, format!("{why}; using the first existing profile")),
                None => (create_profile("Default")?, format!("{why}; no profiles, created one")),
            }
        }
    };
    set_default_profile(&profile)?;
    profile.ensure_directories()?;
    Ok((profile, how))
}

/// 32 lowercase hex chars, matching QUuid::Id128 formatting.
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let addr = &nanos as *const u128 as usize as u128;
    let mut x = nanos ^ (pid << 64) ^ addr.rotate_left(17);
    // xorshift to spread the bits, then format as 128-bit hex.
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{:032x}", x)
}

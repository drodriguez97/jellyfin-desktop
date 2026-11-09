//! Port of the Qt SettingsComponent/SettingsSection/SettingsValue trio.
//!
//! Settings are described by `settings_description.json` (sections, values,
//! defaults, platform filters, possible values) and stored per profile in
//! `jellyfin-desktop.conf` (regular sections) and `storage.json` (storage
//! sections, e.g. window geometry and web client state).

pub mod commands;

#[cfg(test)]
mod tests;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const SECTION_MAIN: &str = "main";
pub const SECTION_AUDIO: &str = "audio";
pub const SECTION_VIDEO: &str = "video";
pub const SECTION_SUBTITLES: &str = "subtitles";
pub const SECTION_OTHER: &str = "other";
pub const SECTION_PATH: &str = "path";
pub const SECTION_STATE: &str = "state";
pub const SECTION_SYSTEM: &str = "system";
pub const SECTION_WEBCLIENT: &str = "webclient";

const CURRENT_PLATFORM: &str = "windows";

#[derive(Debug, Clone)]
pub struct SettingValue {
    pub key: String,
    value: Option<Value>,
    pub default: Value,
    pub display_name: String,
    pub help: String,
    pub input_type: String,
    pub possible_values: Vec<Value>,
    pub hidden: bool,
    pub platform_ok: bool,
    pub index_order: usize,
    pub has_description: bool,
}

impl SettingValue {
    fn new(key: &str, default: Value, platform_ok: bool) -> Self {
        Self {
            key: key.to_string(),
            value: None,
            default,
            display_name: String::new(),
            help: String::new(),
            input_type: String::new(),
            possible_values: Vec::new(),
            hidden: true,
            platform_ok,
            index_order: 0,
            has_description: false,
        }
    }
    pub fn value(&self) -> &Value {
        self.value.as_ref().unwrap_or(&self.default)
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden || !self.platform_ok
    }
    fn description(&self) -> Value {
        let mut m = Map::new();
        m.insert("key".into(), json!(self.key));
        m.insert("displayName".into(), json!(self.display_name));
        m.insert("help".into(), json!(self.help));
        if !self.possible_values.is_empty() {
            m.insert("options".into(), Value::Array(self.possible_values.clone()));
        }
        if !self.input_type.is_empty() {
            m.insert("inputType".into(), json!(self.input_type));
        }
        Value::Object(m)
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub order: i64,
    pub hidden: bool,
    pub storage: bool,
    pub platform_ok: bool,
    values: Vec<SettingValue>,
}

impl Section {
    fn new(name: &str, order: i64, platform_ok: bool) -> Self {
        Self {
            name: name.to_string(),
            order,
            hidden: false,
            storage: false,
            platform_ok,
            values: Vec::new(),
        }
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden || !self.platform_ok
    }
    fn get(&self, key: &str) -> Option<&SettingValue> {
        self.values.iter().find(|v| v.key == key)
    }
    fn get_mut(&mut self, key: &str) -> Option<&mut SettingValue> {
        self.values.iter_mut().find(|v| v.key == key)
    }
    pub fn value(&self, key: &str) -> Value {
        match self.get(key) {
            Some(v) => v.value().clone(),
            None => {
                warn!("setting {}.{} not found", self.name, key);
                Value::Null
            }
        }
    }
    pub fn default_value(&self, key: &str) -> Value {
        self.get(key).map(|v| v.default.clone()).unwrap_or(Value::Null)
    }
    pub fn possible_values(&self, key: &str) -> Vec<Value> {
        self.get(key).map(|v| v.possible_values.clone()).unwrap_or_default()
    }
    pub fn all_values(&self) -> Value {
        let mut m = Map::new();
        for v in &self.values {
            m.insert(v.key.clone(), v.value().clone());
        }
        Value::Object(m)
    }
    pub fn descriptions(&self) -> Value {
        let mut sorted: Vec<&SettingValue> = self.values.iter().collect();
        sorted.sort_by_key(|v| v.index_order);
        let settings: Vec<Value> = sorted
            .into_iter()
            .filter(|v| !v.is_hidden())
            .map(|v| v.description())
            .collect();
        json!({ "key": self.name, "settings": settings })
    }
    pub fn section_order(&self) -> Value {
        json!({ "key": self.name, "order": self.order })
    }

    /// Replaces the section's values with `map`; keys missing from the map are
    /// reset (described values go back to default, ad-hoc values are removed).
    /// Returns the changed keys with their new values.
    fn set_values(&mut self, map: &Map<String, Value>) -> Map<String, Value> {
        let mut updated = Map::new();
        let existing: Vec<String> = self.values.iter().map(|v| v.key.clone()).collect();
        for key in existing {
            if !map.contains_key(&key) {
                self.reset_value_no_notify(&key, &mut updated);
            }
        }
        for (key, new_value) in map {
            if key.is_empty() {
                continue;
            }
            match self.get_mut(key) {
                Some(v) => {
                    if v.value() == new_value {
                        continue;
                    }
                    v.value = Some(new_value.clone());
                }
                None => {
                    let mut v = SettingValue::new(key, Value::Null, true);
                    v.value = Some(new_value.clone());
                    self.values.push(v);
                }
            }
            updated.insert(key.clone(), new_value.clone());
        }
        updated
    }

    fn set_value(&mut self, key: &str, value: Value) -> Map<String, Value> {
        if key == "index" {
            return Map::new();
        }
        let mut map = match self.all_values() {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        map.insert(key.to_string(), value);
        self.set_values(&map)
    }

    /// Sets a value in memory without the caller persisting it (used for state
    /// that is written on a debounce timer, e.g. window geometry).
    pub fn set_value_no_save(&mut self, key: &str, value: Value) -> Map<String, Value> {
        let mut updated = Map::new();
        if key == "index" {
            return updated;
        }
        if value.is_null() {
            // Null means "back to default": described values reset, ad-hoc keys go away.
            self.reset_value_no_notify(key, &mut updated);
            return updated;
        }
        match self.get_mut(key) {
            Some(v) => {
                if v.value() != &value {
                    v.value = Some(value.clone());
                    updated.insert(key.to_string(), value);
                }
            }
            None => {
                let mut v = SettingValue::new(key, Value::Null, true);
                v.value = Some(value.clone());
                self.values.push(v);
                updated.insert(key.to_string(), value);
            }
        }
        updated
    }

    fn reset_value_no_notify(&mut self, key: &str, updated: &mut Map<String, Value>) {
        let Some(idx) = self.values.iter().position(|v| v.key == key) else { return };
        let v = &mut self.values[idx];
        if v.has_description {
            if v.value() == &v.default {
                return;
            }
            v.value = Some(v.default.clone());
            updated.insert(key.to_string(), v.default.clone());
        } else {
            self.values.remove(idx);
        }
    }

    fn reset_value(&mut self, key: &str) -> Map<String, Value> {
        let mut updated = Map::new();
        self.reset_value_no_notify(key, &mut updated);
        updated
    }

    fn reset_values(&mut self) -> Map<String, Value> {
        let mut updated = Map::new();
        let keys: Vec<String> = self.values.iter().map(|v| v.key.clone()).collect();
        for key in keys {
            self.reset_value_no_notify(&key, &mut updated);
        }
        updated
    }
}

/// Changes produced by a mutation, to be forwarded to the web client.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Updates {
    /// (section, {key: value}) pairs for `settings.sectionValueUpdate`.
    pub values: Vec<(String, Map<String, Value>)>,
    /// (section, descriptions) pairs for `settings.groupUpdate`.
    pub groups: Vec<(String, Value)>,
}

impl Updates {
    fn push_values(&mut self, section: &str, map: Map<String, Value>) {
        if !map.is_empty() {
            self.values.push((section.to_string(), map));
        }
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.groups.is_empty()
    }
}

pub struct Settings {
    version: i64,
    sections: BTreeMap<String, Section>,
    conf_path: PathBuf,
    storage_path: PathBuf,
}

fn platform_ok_from(obj: &Map<String, Value>) -> bool {
    let matches = |v: &Value| -> bool {
        match v {
            Value::String(s) => s == CURRENT_PLATFORM || s == "any",
            Value::Array(a) => a
                .iter()
                .any(|x| x.as_str().map(|s| s == CURRENT_PLATFORM || s == "any").unwrap_or(false)),
            _ => false,
        }
    };
    if let Some(p) = obj.get("platforms") {
        return matches(p);
    }
    if let Some(p) = obj.get("platforms_excluded") {
        let excluded = match p {
            Value::String(s) => s == CURRENT_PLATFORM,
            Value::Array(a) => a.iter().any(|x| x.as_str() == Some(CURRENT_PLATFORM)),
            _ => false,
        };
        return !excluded;
    }
    true
}

impl Settings {
    /// Parses the description JSON and loads the profile's config files.
    pub fn load(description_json: &str, profile_dir: &Path) -> Result<Self> {
        let mut s = Settings {
            version: -1,
            sections: BTreeMap::new(),
            conf_path: profile_dir.join("jellyfin-desktop.conf"),
            storage_path: profile_dir.join("storage.json"),
        };
        s.load_description(description_json)?;
        s.load_conf(false)?;
        s.migrate_defaults();
        s.load_conf(true)?;
        Ok(s)
    }

    /// Applies defaults that changed since older builds. The config file stores
    /// every value, defaults included, so a profile written by an older build
    /// pins the old default forever. For each rule a stored value equal to the
    /// old default counts as "never chosen" and goes back to the current
    /// default; anything else is a deliberate choice and is kept. Runs once per
    /// profile, tracked by `main.settingsRevision`.
    fn migrate_defaults(&mut self) {
        const REVISION: i64 = 1;
        if self.i64(SECTION_MAIN, "settingsRevision") >= REVISION {
            return;
        }
        let rules: [(&str, &str, Value); 3] = [
            (SECTION_VIDEO, "hardwareDecoding", json!("copy")),
            (SECTION_MAIN, "logLevel", json!("debug")),
            (SECTION_VIDEO, "cache", json!(75)),
        ];
        for (section, key, old_default) in rules {
            let Some(v) = self.sections.get_mut(section).and_then(|s| s.get_mut(key)) else { continue };
            if v.value.as_ref() == Some(&old_default) && v.default != old_default {
                info!("settings: {section}.{key} reset from old default {old_default} to new default {}", v.default);
                v.value = None;
            }
        }
        if let Some(v) = self.sections.get_mut(SECTION_MAIN).and_then(|s| s.get_mut("settingsRevision")) {
            v.value = Some(json!(REVISION));
        }
        if let Err(e) = self.save_settings() {
            warn!("could not save migrated settings: {e}");
        }
    }

    fn load_description(&mut self, text: &str) -> Result<()> {
        // The description has literal tabs inside help strings (Qt's parser
        // tolerated them); serde_json does not.
        let text = text.replace('\t', " ");
        let doc: Value = serde_json::from_str(&text).context("settings_description.json")?;
        let arr = doc.as_array().ok_or_else(|| anyhow!("settings description must be an array"))?;
        for item in arr {
            let obj = item.as_object().ok_or_else(|| anyhow!("section is not an object"))?;
            let name = obj
                .get("section")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("section without name"))?;
            if name == "__meta__" {
                self.version = obj.get("version").and_then(Value::as_i64).unwrap_or(0);
            } else {
                self.parse_section(name, obj);
            }
        }
        Ok(())
    }

    fn parse_section(&mut self, name: &str, obj: &Map<String, Value>) {
        let Some(values) = obj.get("values").and_then(Value::as_array) else {
            warn!("section {name} has no values array");
            return;
        };
        let mut section = Section::new(
            name,
            obj.get("order").and_then(Value::as_i64).unwrap_or(-1),
            platform_ok_from(obj),
        );
        section.hidden = obj.get("hidden").and_then(Value::as_bool).unwrap_or(false);
        section.storage = obj.get("storage").and_then(Value::as_bool).unwrap_or(false);

        for (order, val) in values.iter().enumerate() {
            let Some(vobj) = val.as_object() else { continue };
            let Some(key) = vobj.get("value").and_then(Value::as_str) else { continue };
            let Some(defaults) = vobj.get("default") else { continue };

            let default = match defaults {
                Value::Array(list) => list
                    .iter()
                    .filter_map(Value::as_object)
                    .find(|d| platform_ok_from(d))
                    .and_then(|d| d.get("value").cloned())
                    .unwrap_or(Value::Null),
                other => other.clone(),
            };

            let mut sv = SettingValue::new(key, default, platform_ok_from(vobj));
            sv.display_name = vobj.get("display_name").and_then(Value::as_str).unwrap_or("").to_string();
            sv.help = vobj.get("help").and_then(Value::as_str).unwrap_or("").to_string();
            sv.has_description = true;
            sv.hidden = vobj.get("hidden").and_then(Value::as_bool).unwrap_or(false);
            sv.index_order = order;
            sv.input_type = vobj.get("input_type").and_then(Value::as_str).unwrap_or("").to_string();

            if let Some(list) = vobj.get("possible_values").and_then(Value::as_array) {
                for pv in list.iter().filter_map(Value::as_array) {
                    if pv.len() < 2 {
                        continue;
                    }
                    let platform_ok = match pv.get(2).and_then(Value::as_object) {
                        Some(opts) => platform_ok_from(opts),
                        None => true,
                    };
                    if platform_ok {
                        sv.possible_values.push(json!({ "value": pv[0], "title": pv[1] }));
                    }
                }
            }
            if section.get(key).is_some() {
                warn!("duplicate setting {name}.{key}");
                continue;
            }
            section.values.push(sv);
        }
        self.sections.insert(name.to_string(), section);
    }

    fn load_conf(&mut self, storage: bool) -> Result<()> {
        let path = if storage { self.storage_path.clone() } else { self.conf_path.clone() };
        let json: Value = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                warn!("could not parse {}: {e}", path.display());
                Value::Null
            }),
            Err(_) => Value::Null,
        };
        let version = json.get("version").and_then(Value::as_i64).unwrap_or(0);
        if version != self.version {
            if version != 0 {
                let backup = path.with_extension(
                    format!("{}.broken", path.extension().and_then(|e| e.to_str()).unwrap_or("")),
                );
                let _ = fs::remove_file(&backup);
                let _ = fs::rename(&path, &backup);
                warn!(
                    "config version {version} != expected {}; moved {} to {}",
                    self.version,
                    path.display(),
                    backup.display()
                );
            }
            return if storage { self.save_storage() } else { self.save_settings() };
        }

        if let Some(sections) = json.get("sections").and_then(Value::as_object) {
            for (name, values) in sections {
                let Some(values) = values.as_object() else { continue };
                if !self.sections.contains_key(name) {
                    if storage {
                        let mut sec = Section::new(name, -1, true);
                        sec.hidden = true;
                        sec.storage = true;
                        self.sections.insert(name.clone(), sec);
                    } else {
                        warn!("ignoring unknown section {name} in config");
                        continue;
                    }
                }
                let sec = self.sections.get_mut(name).unwrap();
                for (key, value) in values {
                    sec.set_value(key, value.clone());
                }
            }
        }
        Ok(())
    }

    fn write_json(path: &Path, value: &Value, pretty: bool) -> Result<()> {
        let text = if pretty { serde_json::to_string_pretty(value)? } else { serde_json::to_string(value)? };
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
        Ok(())
    }

    pub fn save_settings(&self) -> Result<()> {
        let mut sections = Map::new();
        for s in self.sections.values().filter(|s| !s.storage) {
            sections.insert(s.name.clone(), s.all_values());
        }
        Self::write_json(&self.conf_path, &json!({ "sections": sections, "version": self.version }), true)
    }

    pub fn save_storage(&self) -> Result<()> {
        let mut sections = Map::new();
        for s in self.sections.values().filter(|s| s.storage) {
            sections.insert(s.name.clone(), s.all_values());
        }
        Self::write_json(&self.storage_path, &json!({ "sections": sections, "version": self.version }), false)
    }

    fn save_section(&self, name: &str) {
        let storage = self.sections.get(name).map(|s| s.storage).unwrap_or(false);
        let r = if storage { self.save_storage() } else { self.save_settings() };
        if let Err(e) = r {
            warn!("saving settings failed: {e:#}");
        }
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.get(name)
    }

    pub fn value(&self, section: &str, key: &str) -> Value {
        match self.sections.get(section) {
            Some(s) => s.value(key),
            None => {
                warn!("unknown settings section {section}");
                Value::Null
            }
        }
    }

    /// Like `value` but silent when the key is absent (ad-hoc storage keys).
    pub fn value_opt(&self, section: &str, key: &str) -> Option<Value> {
        self.sections.get(section)?.get(key).map(|v| v.value().clone())
    }
    pub fn bool(&self, section: &str, key: &str) -> bool {
        self.value(section, key).as_bool().unwrap_or(false)
    }
    pub fn string(&self, section: &str, key: &str) -> String {
        match self.value(section, key) {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
    pub fn f64(&self, section: &str, key: &str) -> f64 {
        match self.value(section, key) {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::String(s) => s.parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }
    pub fn i64(&self, section: &str, key: &str) -> i64 {
        match self.value(section, key) {
            Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
            Value::String(s) => s.parse().unwrap_or(0),
            _ => 0,
        }
    }

    pub fn set_value(&mut self, section: &str, key: &str, value: Value) -> Updates {
        let mut up = Updates::default();
        match self.sections.get_mut(section) {
            Some(s) => {
                let changed = s.set_value(key, value);
                up.push_values(section, changed);
                self.save_section(section);
            }
            None => warn!("unknown settings section {section}"),
        }
        up
    }

    pub fn set_value_no_save(&mut self, section: &str, key: &str, value: Value) -> Updates {
        let mut up = Updates::default();
        if let Some(s) = self.sections.get_mut(section) {
            let changed = s.set_value_no_save(key, value);
            up.push_values(section, changed);
        }
        up
    }

    /// Web client storage API: `{key, value}` where value is a map (whole
    /// section), null (reset section) or a string (stored under `webclient`).
    pub fn set_values(&mut self, key: &str, value: Value) -> Updates {
        let mut up = Updates::default();
        match value {
            Value::Object(map) => {
                self.ensure_storage_section(key);
                let changed = self.sections.get_mut(key).unwrap().set_values(&map);
                up.push_values(key, changed);
                self.save_section(key);
            }
            Value::Null => {
                self.ensure_storage_section(key);
                let changed = self.sections.get_mut(key).unwrap().reset_values();
                up.push_values(key, changed);
                self.save_section(key);
            }
            Value::String(s) => {
                self.ensure_storage_section(SECTION_WEBCLIENT);
                return self.set_value(SECTION_WEBCLIENT, key, Value::String(s));
            }
            _ => warn!("set_values: unsupported value type for {key}"),
        }
        up
    }

    fn ensure_storage_section(&mut self, name: &str) {
        if !self.sections.contains_key(name) {
            let mut sec = Section::new(name, -1, true);
            sec.hidden = true;
            sec.storage = true;
            self.sections.insert(name.to_string(), sec);
        }
    }

    pub fn remove_value(&mut self, section_or_key: &str) -> Updates {
        let mut up = Updates::default();
        if self.sections.contains_key(section_or_key) {
            let changed = self.sections.get_mut(section_or_key).unwrap().reset_values();
            up.push_values(section_or_key, changed);
            self.save_section(section_or_key);
        } else {
            self.ensure_storage_section(SECTION_WEBCLIENT);
            let changed = self.sections.get_mut(SECTION_WEBCLIENT).unwrap().reset_value(section_or_key);
            up.push_values(SECTION_WEBCLIENT, changed);
            self.save_section(SECTION_WEBCLIENT);
        }
        up
    }

    pub fn reset_to_default(&mut self, section: &str) -> Updates {
        let mut up = Updates::default();
        if let Some(s) = self.sections.get_mut(section) {
            let changed = s.reset_values();
            up.push_values(section, changed);
            self.save_section(section);
        }
        up
    }

    pub fn reset_to_default_all(&mut self) -> Updates {
        let mut up = Updates::default();
        let names: Vec<String> = self.sections.keys().cloned().collect();
        for name in names {
            let changed = self.sections.get_mut(&name).unwrap().reset_values();
            up.push_values(&name, changed);
        }
        let _ = self.save_settings();
        let _ = self.save_storage();
        up
    }

    pub fn update_possible_values(&mut self, section: &str, key: &str, values: Vec<Value>) -> Updates {
        let mut up = Updates::default();
        if let Some(s) = self.sections.get_mut(section) {
            if let Some(v) = s.get_mut(key) {
                v.possible_values = values;
            }
            up.groups.push((section.to_string(), s.descriptions()));
        }
        up
    }

    pub fn all_values(&self, section: &str) -> Value {
        if section.is_empty() {
            let mut all = Map::new();
            for (name, s) in &self.sections {
                all.insert(name.clone(), s.all_values());
            }
            Value::Object(all)
        } else if let Some(s) = self.sections.get(section) {
            s.all_values()
        } else {
            self.sections
                .get(SECTION_WEBCLIENT)
                .map(|s| s.value(section))
                .unwrap_or(Value::Null)
        }
    }

    pub fn ordered_sections(&self) -> Value {
        Value::Array(
            self.sections
                .values()
                .filter(|s| !s.is_hidden())
                .map(Section::section_order)
                .collect(),
        )
    }

    pub fn setting_descriptions(&self) -> Value {
        let mut list: Vec<&Section> = self.sections.values().collect();
        list.sort_by_key(|s| s.order);
        Value::Array(list.into_iter().filter(|s| !s.is_hidden()).map(Section::descriptions).collect())
    }

    /// `settingsDescriptions` as the web client expects: `{section: [settings]}`.
    pub fn setting_descriptions_map(&self) -> Value {
        let mut m = Map::new();
        if let Value::Array(list) = self.setting_descriptions() {
            for d in list {
                if let (Some(k), Some(s)) = (d.get("key").and_then(Value::as_str), d.get("settings")) {
                    m.insert(k.to_string(), s.clone());
                }
            }
        }
        Value::Object(m)
    }

    pub fn conf_path(&self) -> &Path {
        &self.conf_path
    }

    /// The server URL the user connected to last, or empty.
    pub fn user_web_client(&self) -> String {
        self.string(SECTION_MAIN, "userWebClient")
    }

    /// The device name shown to the server.
    pub fn client_name(&self) -> String {
        let name = self.string(SECTION_SYSTEM, "systemname");
        if name.is_empty() || name == "JellyfinDesktop" {
            hostname()
        } else {
            name
        }
    }

    pub fn log_summary(&self) {
        info!("settings loaded from {} (version {})", self.conf_path.display(), self.version);
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Jellyfin Desktop".to_string())
}

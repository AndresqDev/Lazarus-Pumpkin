//! YAML configuration layer.
//!
//! Mirrors `me.qiooip.lazarus.config.ConfigFile` from the original: every config
//! file is loaded verbatim from the plugin data folder (created from the embedded
//! original if missing) and read through typed getters.
//!
//! The original ran `Color.translate` on every string (turning `&x` into `§x`).
//! We keep the raw `&` codes because Pumpkin's `TextComponent` parses them
//! directly, which produces the exact same rendered output.

use serde_yaml::Value;

/// One loaded YAML config file (`config.yml`, `language.yml`, ...).
pub struct ConfigFile {
    pub name: String,
    value: Value,
}

impl ConfigFile {
    /// Loads `<data_folder>/<name>`, creating it from `default` when missing.
    pub fn load_or_create(data_folder: &str, name: &str, default: &str) -> Self {
        let path = format!("{data_folder}/{name}");
        let contents = match std::fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(_) => {
                let _ = std::fs::write(&path, default);
                default.to_string()
            }
        };

        Self::from_str(name, &contents)
    }

    /// Parses an in-memory YAML document.
    pub fn from_str(name: &str, contents: &str) -> Self {
        let value = serde_yaml::from_str(contents).unwrap_or_else(|error| {
            tracing::warn!("failed to parse {name}: {error}");
            Value::Mapping(Default::default())
        });
        Self {
            name: name.to_string(),
            value,
        }
    }

    /// Navigates a dotted path (`FACTION_DTR.MAX_DTR`). Numeric path segments
    /// also match numeric YAML keys (e.g. `DEATHBAN.BAN_TIMES.30`).
    fn lookup(&self, path: &str) -> Option<&Value> {
        let mut current = &self.value;
        for part in path.split('.') {
            current = match current {
                Value::Mapping(map) => map
                    .get(Value::String(part.to_string()))
                    .or_else(|| part.parse::<i64>().ok().and_then(|n| map.get(Value::Number(n.into()))))
                    .or_else(|| {
                        part.parse::<bool>()
                            .ok()
                            .and_then(|b| map.get(Value::Bool(b)))
                    })?,
                _ => return None,
            };
        }
        Some(current)
    }

    /// `getString(path)` — colour codes are kept raw (see module docs).
    pub fn string(&self, path: &str) -> String {
        match self.lookup(path) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Null) | None => String::new(),
            Some(other) => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
        }
    }

    /// `getStringList(path)`.
    pub fn string_list(&self, path: &str) -> Vec<String> {
        match self.lookup(path) {
            Some(Value::Sequence(seq)) => seq
                .iter()
                .map(|item| match item {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => String::new(),
                })
                .collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// `getInt(path)`.
    pub fn int(&self, path: &str) -> i64 {
        match self.lookup(path) {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
            Some(Value::Bool(b)) => i64::from(*b),
            _ => 0,
        }
    }

    /// `getDouble(path)`.
    pub fn double(&self, path: &str) -> f64 {
        match self.lookup(path) {
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// `getBoolean(path)`.
    pub fn bool(&self, path: &str) -> bool {
        match self.lookup(path) {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
            _ => false,
        }
    }

    /// `getSection(path)`.
    pub fn section(&self, path: &str) -> Option<&Value> {
        self.lookup(path)
    }

    /// Whether a key exists (distinguishes "missing" from "empty string").
    pub fn has(&self, path: &str) -> bool {
        self.lookup(path).is_some()
    }

    /// Keys of a mapping section (used for dynamic menus like kits). Numeric
    /// keys are stringified (`DEATHBAN.BAN_TIMES` uses minute keys).
    pub fn keys(&self, path: &str) -> Vec<String> {
        match self.lookup(path) {
            Some(Value::Mapping(map)) => map
                .keys()
                .filter_map(|k| match k {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Raw value at a path (for iterating nested maps/lists).
    pub fn value(&self, path: &str) -> Option<&Value> {
        self.lookup(path)
    }

    /// Keys of the top-level mapping (`items.yml` name -> id lookups).
    pub fn root_keys(&self) -> Vec<String> {
        match &self.value {
            Value::Mapping(map) => map
                .keys()
                .filter_map(|k| match k {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

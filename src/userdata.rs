//! Port of `me.qiooip.lazarus.userdata.Userdata` / `UserdataManager`.
//!
//! Data is persisted as JSON in the plugin's private data folder, replacing the
//! original JSON/Mongo datastore (see `DATABASE.DATASTORE`). Mongo is not ported
//! because Pumpkin's WASM sandbox only exposes the plugin's own data directory.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub found_ore: bool,
    pub sounds: bool,
    pub messages: bool,
    pub scoreboard: bool,
    pub death_messages: bool,
    pub public_chat: bool,
    pub cobble: bool,
    pub lightning: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            found_ore: true,
            sounds: true,
            messages: true,
            scoreboard: true,
            death_messages: true,
            public_chat: true,
            cobble: true,
            lightning: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Userdata {
    pub uuid: String,
    pub name: String,

    pub kills: i32,
    pub deaths: i32,
    pub killstreak: i32,
    pub highest_killstreak: i32,

    pub balance: i64,
    pub lives: i32,
    /// Diamonds mined (used by the Miner class rewards).
    #[serde(default)]
    pub diamonds_mined: i32,
    pub spawn_credits: i32,

    pub reclaim_used: bool,
    pub settings: Settings,

    /// Unix millis until which the player is deathbanned (None = not banned).
    #[serde(default)]
    pub deathban_until: Option<i64>,

    pub ignoring: Vec<String>,
    pub notes: Vec<String>,
    pub last_kills: Vec<String>,
    pub last_deaths: Vec<String>,
    pub kit_delays: BTreeMap<String, i64>,
}

impl Userdata {
    pub fn new(uuid: String, name: String, config: &Config) -> Self {
        Self {
            uuid,
            name,
            kills: 0,
            deaths: 0,
            killstreak: 0,
            highest_killstreak: 0,
            balance: config.economy.default_player_balance,
            lives: config.timers.default_lives,
            diamonds_mined: 0,
            spawn_credits: 0,
            reclaim_used: false,
            settings: Settings::default(),
            deathban_until: None,
            ignoring: Vec::new(),
            notes: Vec::new(),
            last_kills: Vec::new(),
            last_deaths: Vec::new(),
            kit_delays: BTreeMap::new(),
        }
    }

    pub fn add_kill(&mut self, message: String) {
        self.kills += 1;
        push_capped(&mut self.last_kills, message);
    }

    pub fn add_death(&mut self, message: String) {
        self.deaths += 1;
        push_capped(&mut self.last_deaths, message);
    }

    pub fn add_killstreak(&mut self) {
        self.killstreak += 1;
        if self.killstreak > self.highest_killstreak {
            self.highest_killstreak = self.killstreak;
        }
    }

    pub fn reset_killstreak(&mut self) -> i32 {
        let previous = self.killstreak;
        self.killstreak = 0;
        previous
    }

    pub fn change_balance(&mut self, amount: i64, max: i64) {
        self.balance = (self.balance + amount).clamp(0, max);
    }

    pub fn is_ignoring(&self, uuid: &str) -> bool {
        self.ignoring.iter().any(|id| id == uuid)
    }

    /// True when the player's deathban is still active.
    pub fn is_deathbanned(&self) -> bool {
        self.deathban_until
            .is_some_and(|until| until > crate::util::now_millis())
    }

    pub fn deathban_remaining(&self) -> i64 {
        self.deathban_until
            .map(|until| (until - crate::util::now_millis()).max(0))
            .unwrap_or(0)
    }

    pub fn deathban(&mut self, minutes: u64) {
        self.deathban_until = Some(crate::util::now_millis() + (minutes as i64 * 60 * 1000));
    }

    pub fn revive(&mut self) {
        self.deathban_until = None;
    }

    pub fn toggle_ignoring(&mut self, uuid: &str) {
        if let Some(pos) = self.ignoring.iter().position(|id| id == uuid) {
            self.ignoring.remove(pos);
        } else {
            self.ignoring.push(uuid.to_string());
        }
    }
}

fn push_capped(list: &mut Vec<String>, message: String) {
    list.insert(0, message);
    if list.len() > 5 {
        list.truncate(5);
    }
}

/// Port of `UserdataManager`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UserdataManager {
    pub users: BTreeMap<String, Userdata>,
}

impl UserdataManager {
    pub fn get(&self, uuid: &str) -> Option<&Userdata> {
        self.users.get(uuid)
    }

    pub fn get_mut(&mut self, uuid: &str) -> Option<&mut Userdata> {
        self.users.get_mut(uuid)
    }

    /// Returns the user's data, creating it if this is their first join.
    pub fn get_or_create(&mut self, uuid: String, name: String, config: &Config) -> &mut Userdata {
        self.users
            .entry(uuid.clone())
            .or_insert_with(|| Userdata::new(uuid, name, config))
    }

    pub fn update_name(&mut self, uuid: &str, name: String) {
        if let Some(user) = self.users.get_mut(uuid) {
            user.name = name;
        }
    }

    /// Top players by a numeric metric, descending.
    pub fn top_by<F: Fn(&Userdata) -> i64>(&self, key: F, limit: usize) -> Vec<&Userdata> {
        let mut all: Vec<&Userdata> = self.users.values().collect();
        all.sort_by(|a, b| key(b).cmp(&key(a)));
        all.into_iter().take(limit).collect()
    }
}

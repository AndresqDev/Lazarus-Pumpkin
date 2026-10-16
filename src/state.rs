//! Global plugin state.
//!
//! Pumpkin plugins implement `Plugin` with `&self` methods and event handlers with
//! `&self`, so all mutable state lives behind a global `RwLock`. This mirrors how
//! the original used singletons (`FactionsManager.getInstance()`, ...).
//!
//! Reentrancy note: never hold the lock across a host API call that can synchronously
//! re-enter the plugin (e.g. sending a message that triggers another event). Handlers
//! below collect messages first and emit them after releasing the guard.

use std::sync::{OnceLock, RwLock};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::config::{Config, SidebarConfig, TabConfig};
use crate::glass::GlassManager;
use crate::hologram::Hologram;
use crate::kits::KitsManager;
use crate::selection::SelectionManager;
use crate::staffmode::StaffManager;
use pumpkin_plugin_api::Entity;
use crate::factions::{ClaimManager, FactionsManager};
use crate::lang::Lang;
use crate::timers::TimerManager;
use crate::userdata::UserdataManager;
use crate::yaml::ConfigFile;

/// Raw YAML configs — the original files, read 1:1 (Fase 0).
pub struct RawConfigs {
    pub config: ConfigFile,
    pub language: ConfigFile,
    pub scoreboard: ConfigFile,
    pub tab: ConfigFile,
    pub utilities: ConfigFile,
    pub classes: ConfigFile,
    pub limiters: ConfigFile,
    pub abilities: ConfigFile,
    pub items: ConfigFile,
}

impl RawConfigs {
    /// Loads every original YAML file, creating it from the embedded default.
    pub fn load(folder: &str) -> Self {
        Self {
            config: ConfigFile::load_or_create(
                folder,
                "config.yml",
                include_str!("../original-configs/config.yml"),
            ),
            language: ConfigFile::load_or_create(
                folder,
                "language.yml",
                include_str!("../original-configs/language.yml"),
            ),
            scoreboard: ConfigFile::load_or_create(
                folder,
                "scoreboard.yml",
                include_str!("../original-configs/scoreboard.yml"),
            ),
            tab: ConfigFile::load_or_create(
                folder,
                "tab.yml",
                include_str!("../original-configs/tab.yml"),
            ),
            utilities: ConfigFile::load_or_create(
                folder,
                "utilities.yml",
                include_str!("../original-configs/utilities.yml"),
            ),
            classes: ConfigFile::load_or_create(
                folder,
                "classes.yml",
                include_str!("../original-configs/classes.yml"),
            ),
            limiters: ConfigFile::load_or_create(
                folder,
                "limiters.yml",
                include_str!("../original-configs/limiters.yml"),
            ),
            abilities: ConfigFile::load_or_create(
                folder,
                "abilities.yml",
                include_str!("../original-configs/abilities.yml"),
            ),
            items: ConfigFile::load_or_create(
                folder,
                "items.yml",
                include_str!("../original-configs/items.yml"),
            ),
        }
    }
}

pub struct AppState {
    pub data_folder: String,
    pub raw: RawConfigs,
    pub config: Config,
    pub lang: Lang,
    pub tab: TabConfig,
    pub sidebar: SidebarConfig,
    pub factions: FactionsManager,
    pub claims: ClaimManager,
    pub userdata: UserdataManager,
    pub timers: TimerManager,
    pub glass: GlassManager,
    pub selection: SelectionManager,
    pub staff: StaffManager,
    pub holograms: std::collections::BTreeMap<String, Hologram>,
    /// Runtime display entities per hologram (recreated on startup).
    pub hologram_entities: std::collections::BTreeMap<String, Vec<Entity>>,
    pub kits: KitsManager,
    /// Transient: last chunk each player occupied, for territory-entry messages.
    pub chunk_cache: std::collections::BTreeMap<String, ChunkState>,
    /// Transient: last tab lines sent per viewer (for the packet-based tab list).
    pub tab_cache: std::collections::BTreeMap<String, Vec<String>>,
    /// Transient: world name -> (spawn x, spawn z), used to centre the warzone.
    pub spawn_centers: std::collections::BTreeMap<String, (i32, i32)>,
}

/// Tracks a player's last known chunk + displayed territory.
#[derive(Debug, Clone, Default)]
pub struct ChunkState {
    pub world: String,
    pub cx: i32,
    pub cz: i32,
    pub territory: String,
}

static STATE: OnceLock<RwLock<AppState>> = OnceLock::new();

/// Initialises global state from disk (creating default files on first run).
pub fn init(data_folder: String) -> Result<(), String> {
    let raw = RawConfigs::load(&data_folder);
    let mut config: Config = load_toml(&data_folder, "config.toml");
    sync_faction_yaml(&mut config, &raw);
    let lang: Lang = load_toml(&data_folder, "lang.toml");
    let tab: TabConfig = load_toml(&data_folder, "tab.toml");
    let sidebar: SidebarConfig = load_toml(&data_folder, "scoreboard.toml");

    let mut factions: FactionsManager = load_json(&data_folder, "factions.json");
    factions.init_system_factions();

    let mut claims: ClaimManager = load_json(&data_folder, "claims.json");
    claims.rebuild_index();

    let userdata: UserdataManager = load_json(&data_folder, "userdata.json");
    let timers: TimerManager = load_json(&data_folder, "timers.json");
    let holograms: std::collections::BTreeMap<String, Hologram> =
        load_json(&data_folder, "holograms.json");
    let kits: KitsManager = load_json(&data_folder, "kits.json");

    STATE
        .set(RwLock::new(AppState {
            data_folder,
            raw,
            config,
            lang,
            tab,
            sidebar,
            factions,
            claims,
            userdata,
            timers,
            glass: GlassManager::default(),
            selection: SelectionManager::default(),
            staff: StaffManager::default(),
            holograms,
            hologram_entities: std::collections::BTreeMap::new(),
            kits,
            chunk_cache: std::collections::BTreeMap::new(),
            tab_cache: std::collections::BTreeMap::new(),
            spawn_centers: std::collections::BTreeMap::new(),
        }))
        .map_err(|_| "state already initialised".to_string())
}

/// Access the global state. Panics if [`init`] has not run yet.
pub fn state() -> &'static RwLock<AppState> {
    STATE.get().expect("state not initialised")
}

/// Persists every store to the data folder.
pub fn save_all() {
    let guard = state().read().unwrap();
    let folder = guard.data_folder.clone();

    write_json(&folder, "factions.json", &guard.factions);
    write_json(&folder, "claims.json", &guard.claims);
    write_json(&folder, "userdata.json", &guard.userdata);
    write_json(&folder, "timers.json", &guard.timers);
    write_json(&folder, "holograms.json", &guard.holograms);
    write_json(&folder, "kits.json", &guard.kits);
}

/// Reloads `config.toml` and `lang.toml` from disk.
pub fn reload_config() {
    let folder = state().read().unwrap().data_folder.clone();
    let raw = RawConfigs::load(&folder);
    let mut config: Config = load_toml(&folder, "config.toml");
    sync_faction_yaml(&mut config, &raw);
    let lang: Lang = load_toml(&folder, "lang.toml");
    let tab: TabConfig = load_toml(&folder, "tab.toml");
    let sidebar: SidebarConfig = load_toml(&folder, "scoreboard.toml");

    let mut guard = state().write().unwrap();
    guard.raw = raw;
    guard.config = config;
    guard.lang = lang;
    guard.tab = tab;
    guard.sidebar = sidebar;
}

// ---------------------------------------------------------------------------
// File helpers (WASI sandbox: only the plugin data folder is accessible).
// ---------------------------------------------------------------------------

/// Makes the original `config.yml` authoritative for the faction/DTR values the
/// gameplay logic uses, so the tab/commands (which read the YAML) and the typed
/// config never disagree. (Porting note: the typed `config.toml` is legacy.)
pub fn sync_faction_yaml(config: &mut Config, raw: &RawConfigs) {
    let y = &raw.config;
    if y.has("FACTION_DTR") {
        config.factions.dtr_per_player = y.double("FACTION_DTR.DTR_PER_PLAYER");
        config.factions.max_dtr = y.double("FACTION_DTR.MAX_DTR");
        config.factions.min_dtr = y.double("FACTION_DTR.MIN_DTR");
        config.factions.solo_faction_dtr = y.double("FACTION_DTR.SOLO_FACTION_DTR");
        config.factions.regen_per_minute = y.double("FACTION_DTR.REGEN_PER_MINUTE");
    }
    if y.has("FACTION_CLAIM") {
        config.factions.must_be_connected = y.bool("FACTION_CLAIM.MUST_BE_CONNECTED");
        config.factions.max_claims = y.int("FACTION_CLAIM.MAX_CLAIMS") as usize;
        config.factions.claims_per_player = y.int("FACTION_CLAIM.CLAIMS_PER_PLAYER") as usize;
        config.factions.min_claim_size = y.int("FACTION_CLAIM.MIN_CLAIM_SIZE") as i32;
        config.factions.max_claim_size = y.int("FACTION_CLAIM.MAX_CLAIM_SIZE") as i32;
        config.factions.claim_buffer = y.int("FACTION_CLAIM.CLAIM_BUFFER") as i32;
        config.factions.claim_buffer_ignore_roads = y.bool("FACTION_CLAIM.CLAIM_BUFFER_IGNORE_ROADS");
        config.factions.claim_price_per_block = y.double("FACTION_CLAIM.CLAIM_PRICE_PER_BLOCK");
        config.factions.unclaim_price_multiplier = y.double("FACTION_CLAIM.UNCLAIM_PRICE_MULTIPLIER");
    }
    if y.has("FACTION_PLAYER") {
        config.factions.faction_limit = y.int("FACTION_PLAYER.FACTION_LIMIT") as usize;
        config.factions.join_while_frozen = y.bool("FACTION_PLAYER.JOIN_WHILE_FROZEN");
        config.factions.leave_while_frozen = y.bool("FACTION_PLAYER.LEAVE_WHILE_FROZEN");
        config.factions.disband_while_frozen = y.bool("FACTION_PLAYER.DISBAND_WHILE_FROZEN");
        config.factions.unclaim_while_frozen = y.bool("FACTION_PLAYER.UNCLAIM_WHILE_FROZEN");
        config.factions.leave_while_in_own_claim = y.bool("FACTION_PLAYER.LEAVE_WHILE_IN_OWN_CLAIM");
        config.factions.players_take_own_damage = y.bool("FACTION_PLAYER.PLAYERS_TAKE_OWN_DAMAGE");
        config.factions.ally_friendly_fire = y.bool("FACTION_PLAYER.ALLY_FRIENDLY_FIRE");
        config.factions.warzone_break_after.overworld =
            y.int("FACTION_PLAYER.WARZONE_BREAK_AFTER.OVERWORLD") as i32;
        config.factions.warzone_break_after.nether =
            y.int("FACTION_PLAYER.WARZONE_BREAK_AFTER.NETHER") as i32;
    }
    if y.has("WARZONE_RADIUS") {
        config.warzone_radius.overworld = y.int("WARZONE_RADIUS.OVERWORLD") as i32;
        config.warzone_radius.nether = y.int("WARZONE_RADIUS.NETHER") as i32;
        config.warzone_radius.end = y.int("WARZONE_RADIUS.END") as i32;
    }
    if y.has("BORDER_SIZE") {
        config.border_size.overworld = y.int("BORDER_SIZE.OVERWORLD") as i32;
        config.border_size.nether = y.int("BORDER_SIZE.NETHER") as i32;
        config.border_size.end = y.int("BORDER_SIZE.END") as i32;
    }
    if y.has("FACTION_MAX_ALLIES") {
        config.factions.max_allies = y.int("FACTION_MAX_ALLIES") as usize;
    }
    if y.has("RELATION_COLORS") {
        config.relation_colors.teammate = y.string("RELATION_COLORS.TEAMMATE_COLOR");
        config.relation_colors.ally = y.string("RELATION_COLORS.ALLY_COLOR");
        config.relation_colors.enemy = y.string("RELATION_COLORS.ENEMY_COLOR");
    }
}

fn load_toml<T>(folder: &str, name: &str) -> T
where
    T: DeserializeOwned + Default + Serialize,
{
    let path = format!("{folder}/{name}");
    match std::fs::read_to_string(&path) {
        Ok(contents) => match toml::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("failed to parse {name}: {error}; using defaults");
                T::default()
            }
        },
        Err(_) => {
            let default = T::default();
            write_toml(folder, name, &default);
            default
        }
    }
}

fn write_toml<T: Serialize>(folder: &str, name: &str, value: &T) {
    match toml::to_string_pretty(value) {
        Ok(contents) => {
            let _ = std::fs::write(format!("{folder}/{name}"), contents);
        }
        Err(error) => tracing::warn!("failed to serialise {name}: {error}"),
    }
}

fn load_json<T>(folder: &str, name: &str) -> T
where
    T: DeserializeOwned + Default,
{
    let path = format!("{folder}/{name}");
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => T::default(),
    }
}

fn write_json<T: Serialize>(folder: &str, name: &str, value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(contents) => {
            if let Err(error) = std::fs::write(format!("{folder}/{name}"), contents) {
                tracing::warn!("failed to write {name}: {error}");
            }
        }
        Err(error) => tracing::warn!("failed to serialise {name}: {error}"),
    }
}

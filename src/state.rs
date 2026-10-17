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
    pub sotw: crate::sotw::SotwState,
    pub glass: GlassManager,
    pub selection: SelectionManager,
    pub staff: StaffManager,
    pub holograms: std::collections::BTreeMap<String, Hologram>,
    /// Runtime display entities per hologram (recreated on startup).
    pub hologram_entities: std::collections::BTreeMap<String, Vec<Entity>>,
    pub kits: KitsManager,
    pub classes: crate::classes::PvpClassManager,
    /// Transient: last chunk each player occupied, for territory-entry messages.
    pub chunk_cache: std::collections::BTreeMap<String, ChunkState>,
    /// Transient: last tab lines sent per viewer (for the packet-based tab list).
    pub tab_cache: std::collections::BTreeMap<String, Vec<String>>,
    /// Transient: world name -> (spawn x, spawn z), used to centre the warzone.
    pub spawn_centers: std::collections::BTreeMap<String, (i32, i32)>,
    /// Transient: protected players to eject on the next tick (they respawned
    /// inside a claim they cannot enter).
    pub protection_ejects: std::collections::BTreeSet<String>,
    /// Transient: players whose respawn landed in the wrong world (legacy
    /// respawn point in another dimension) to teleport to the spawn next tick.
    pub respawn_fixes: std::collections::BTreeSet<String>,
    /// Transient: placed cobwebs awaiting the remover (`handlers.block`,
    /// key -> expiry millis).
    pub cobwebs: std::collections::BTreeMap<(String, i32, i32, i32), i64>,
    /// Transient: diamond ores marked as placed or already counted
    /// (`FoundOreHandler`).
    pub found_ore_marks: std::collections::BTreeSet<(String, i32, i32, i32)>,
    /// Transient: autosmel drop replacements awaiting the host drop event
    /// (key -> (ingot key, expiry millis)).
    pub autosmel_pending: std::collections::BTreeMap<(String, i32, i32, i32), (String, i64)>,
    /// `ChatControlHandler#isShowFoundOre` (default on; the chat-control command
    /// lands with the chat sector).
    pub chat_control_show_found_ore: bool,
    /// `EotwHandler#isActive` (the EOTW handler itself is not ported yet).
    pub eotw_active: bool,
    /// Emulates the subclaim signs whose second line the original fills with the
    /// creator name (sign key -> creator name).
    pub subclaim_creators: std::collections::BTreeMap<(String, i32, i32, i32), String>,
    /// Transient: players kicked with the deathban message at 1s after death
    /// (port of `Tasks.syncLater(() -> kickPlayer(...), 20L)`).
    pub pending_kicks: Vec<PendingKick>,
    /// Transient: players that already saw the "join again to use life" deathban
    /// kick (port of `DeathbanManager#confirmations`).
    pub deathban_confirm: std::collections::BTreeSet<String>,
    /// Transient: last player that damaged each victim (uuid -> damager uuid +
    /// timestamp), used as the port of Bukkit's `Player#getKiller`.
    pub last_damagers: std::collections::BTreeMap<String, (String, i64)>,
    /// Transient: last damage taken by each player (port of Bukkit's
    /// `Player#getLastDamageCause`), used to build the death messages.
    pub last_damage: std::collections::BTreeMap<String, LastDamage>,
}

/// The last damage dealt to a player, recorded by the combat listeners.
#[derive(Debug, Clone)]
pub struct LastDamage {
    /// Canonical `damage_type` registry id (see `death::normalize_cause`).
    pub cause: String,
    /// True when it came from `EntityDamageByEntityEvent`.
    pub entity: bool,
    /// Resolved player damager/shooter.
    pub damager: Option<String>,
    /// Registry name of the non-player damager/shooter (mob, TNT, …).
    pub entity_key: Option<String>,
    /// True when the damage was dealt by a projectile.
    pub projectile: bool,
}

/// A kick scheduled from an event handler (deathban), fired by the 1s tick.
pub struct PendingKick {
    pub uuid: String,
    pub message: String,
    pub at: i64,
}

/// Tracks a player's last known chunk + the territory there (identity, so the
/// claim announcements compare factions like the original).
#[derive(Debug, Clone, Default)]
pub struct ChunkState {
    pub world: String,
    pub cx: i32,
    pub cz: i32,
    pub territory: crate::core::Territory,
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
    factions.normalize_system_factions();

    let mut claims: ClaimManager = load_json(&data_folder, "claims.json");
    claims.rebuild_index();

    let userdata: UserdataManager = load_json(&data_folder, "userdata.json");
    let mut timers: TimerManager = load_json(&data_folder, "timers.json");
    // Resume a persisted SOTW timer from its saved remaining milliseconds (the
    // original `SystemTimer#loadTimer` reactivates from `timers.yml`). Old files
    // stored whole seconds; both are accepted.
    if timers.sotw_remaining_millis > 0 {
        timers.sotw_until = Some(crate::util::now_millis() + timers.sotw_remaining_millis);
    } else if let Some(seconds) = timers.sotw_remaining_seconds_legacy.filter(|seconds| *seconds > 0) {
        timers.sotw_until = Some(crate::util::now_millis() + seconds as i64 * 1000);
    }
    let holograms: std::collections::BTreeMap<String, Hologram> =
        load_json(&data_folder, "holograms.json");
    // Ship the 9 default kits (converted from the original `kits.yml`) on first run.
    let kits_path = format!("{data_folder}/kits.json");
    if std::fs::metadata(&kits_path).is_err() {
        let _ = std::fs::write(&kits_path, include_str!("../default-kits.json"));
    }
    let kits: KitsManager = load_json(&data_folder, "kits.json");
    tracing::info!("lazarus-kits loaded {} kits", kits.kits.len());
    let mut classes = crate::classes::PvpClassManager::new();
    classes.load(&raw.classes);

    let sotw = crate::sotw::SotwState::new(config.sotw.hide_players);

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
            sotw,
            glass: GlassManager::default(),
            selection: SelectionManager::default(),
            staff: StaffManager::default(),
            holograms,
            hologram_entities: std::collections::BTreeMap::new(),
            kits,
            classes,
            chunk_cache: std::collections::BTreeMap::new(),
            tab_cache: std::collections::BTreeMap::new(),
            spawn_centers: std::collections::BTreeMap::new(),
            protection_ejects: std::collections::BTreeSet::new(),
            respawn_fixes: std::collections::BTreeSet::new(),
            cobwebs: std::collections::BTreeMap::new(),
            found_ore_marks: std::collections::BTreeSet::new(),
            autosmel_pending: std::collections::BTreeMap::new(),
            chat_control_show_found_ore: true,
            eotw_active: false,
            subclaim_creators: std::collections::BTreeMap::new(),
            pending_kicks: Vec::new(),
            deathban_confirm: std::collections::BTreeSet::new(),
            last_damagers: std::collections::BTreeMap::new(),
            last_damage: std::collections::BTreeMap::new(),
        }))
        .map_err(|_| "state already initialised".to_string())
}

/// Access the global state. Panics if [`init`] has not run yet.
pub fn state() -> &'static RwLock<AppState> {
    STATE.get().expect("state not initialised")
}

/// Persists every store to the data folder.
///
/// Stores are serialised under a short read lock and the files are written
/// afterwards, outside the lock, through [`atomic_write`] — so gameplay is
/// never blocked on file I/O and a crash mid-write can never leave a truncated
/// store behind. `timers.json` (with millisecond precision), `factions.json`,
/// `claims.json` and `userdata.json` are additionally flushed every 30 seconds
/// by [`save_hot`], so a hard shutdown loses at most half a minute of state.
pub fn save_all() {
    persist(true);
}

/// Frequent flush of the gameplay-volatile stores: timers (per-player, faction
/// freeze and the global SOTW timer, all stored as absolute epoch millis),
/// factions, claims and userdata. The rarely-changed, admin-edited stores
/// (`kits.json`, `holograms.json`) are only rewritten by [`save_all`].
pub fn save_hot() {
    persist(false);
}

fn persist(full: bool) {
    // Refresh the persisted SOTW remainder from the live expiry first (small
    // write-lock scope; the lock is never held across serialisation or I/O).
    {
        let mut guard = state().write().unwrap();
        guard.timers.sotw_capture_remaining();
    }

    let (folder, payloads) = {
        let guard = state().read().unwrap();
        let folder = guard.data_folder.clone();

        fn push<T: Serialize>(
            payloads: &mut Vec<(&'static str, String)>,
            name: &'static str,
            value: &T,
            pretty: bool,
        ) {
            let serialised = if pretty {
                serde_json::to_string_pretty(value)
            } else {
                serde_json::to_string(value)
            };
            match serialised {
                Ok(contents) => payloads.push((name, contents)),
                Err(error) => tracing::warn!("failed to serialise {name}: {error}"),
            }
        }

        let mut payloads: Vec<(&'static str, String)> = Vec::new();
        push(&mut payloads, "timers.json", &guard.timers, false);
        push(&mut payloads, "factions.json", &guard.factions, false);
        push(&mut payloads, "claims.json", &guard.claims, false);
        push(&mut payloads, "userdata.json", &guard.userdata, false);
        if full {
            // Compact JSON for the machine stores above; the kits/holograms
            // files stay pretty-printed because admins hand-edit them.
            push(&mut payloads, "kits.json", &guard.kits, true);
            push(&mut payloads, "holograms.json", &guard.holograms, true);
        }
        (folder, payloads)
    };

    for (name, contents) in payloads {
        atomic_write(&folder, name, &contents);
    }
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
        config.factions.freeze_duration_minutes = y.int("FACTION_DTR.FREEZE_DURATION").max(0) as u64;
        config.factions.dtr_full_character = y.string("FACTION_DTR.DTR_CHARACTERS.FULL_DTR");
        config.factions.dtr_regen_character = y.string("FACTION_DTR.DTR_CHARACTERS.REGENERATING");
        config.factions.dtr_frozen_character = y.string("FACTION_DTR.DTR_CHARACTERS.FROZEN");
        config.factions.dtr_death_loss.overworld = y.double("FACTION_DTR.DTR_DEATH_LOSS.OVERWORLD");
        config.factions.dtr_death_loss.nether = y.double("FACTION_DTR.DTR_DEATH_LOSS.NETHER");
        config.factions.dtr_death_loss.end = y.double("FACTION_DTR.DTR_DEATH_LOSS.END");
    }
    if y.has("FACTION_TOP") {
        config.factions.allow_negative_points = y.bool("FACTION_TOP.ALLOW_NEGATIVE_POINTS");
        config.factions.points_per_kill = y.int("FACTION_TOP.KILL") as i32;
        config.factions.points_per_death = y.int("FACTION_TOP.DEATH") as i32;
        config.factions.koth_cap_points = y.int("FACTION_TOP.KOTH_CAP") as i32;
        config.factions.conquest_cap_points = y.int("FACTION_TOP.CONQUEST_CAP") as i32;
        config.factions.dtc_destroy_points = y.int("FACTION_TOP.DTC_DESTROY") as i32;
    }
    if y.has("FACTION_STUCK") {
        config.timers.stuck_movement_radius =
            y.int("FACTION_STUCK.ALLOWED_MOVEMENT_RADIUS") as f64;
    }
    if y.has("SYSTEM_FACTION_NAMES") {
        config.system_faction_names.wilderness = y.string("SYSTEM_FACTION_NAMES.WILDERNESS");
        config.system_faction_names.warzone = y.string("SYSTEM_FACTION_NAMES.WARZONE");
    }
    if y.has("FACTIONS_AUTO_SAVE") {
        config.factions.auto_save_minutes = y.int("FACTIONS_AUTO_SAVE").max(1) as u64;
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
    // Lives / deathban (the original gives `DEFAULT_LIVES` on first join).
    if y.has("DEFAULT_LIVES") {
        config.timers.default_lives = y.int("DEFAULT_LIVES") as i32;
    }
    if y.has("DEATHBAN") {
        config.deathban.enabled = y.bool("DEATHBAN.ENABLED");
        config.deathban.default_ban_time = y.int("DEATHBAN.DEFAULT_BAN_TIME") as u64;
        // `BAN_TIMES` keeps the config order: `getBanTime` takes the first
        // matching permission (`DeathbanManager#getBanTime`).
        config.deathban.ban_times = y
            .keys("DEATHBAN.BAN_TIMES")
            .into_iter()
            .filter_map(|key| {
                let minutes = key.parse::<u64>().ok()?;
                let permission = y.string(&format!("DEATHBAN.BAN_TIMES.{key}"));
                (!permission.is_empty()).then_some(crate::config::BanTime { minutes, permission })
            })
            .collect();
    }
    if y.has("RELATION_COLORS") {
        config.relation_colors.teammate = y.string("RELATION_COLORS.TEAMMATE_COLOR");
        config.relation_colors.ally = y.string("RELATION_COLORS.ALLY_COLOR");
        config.relation_colors.enemy = y.string("RELATION_COLORS.ENEMY_COLOR");
        config.relation_colors.sotw = y.string("RELATION_COLORS.SOTW_COLOR");
    }
    // PvP protection duration (the original `PvpProtTimer` default delay).
    if y.has("PVP_PROTECTION") {
        config.timers.pvp_protection_seconds = y.int("PVP_PROTECTION.DURATION").max(0) as u64;
    }
    // SOTW timer (port of the `SOTW_TIMER` section).
    if y.has("SOTW_TIMER") {
        config.sotw.default_time_minutes = y.int("SOTW_TIMER.DEFAULT_TIME").max(0) as u64;
        config.sotw.spawn_mobs_from_spawners_only =
            y.bool("SOTW_TIMER.SPAWN_MOBS_FROM_SPAWNERS_ONLY");
        config.sotw.hide_players = y.bool("SOTW_TIMER.HIDE_PLAYERS");
    }
    // Kitmap mode (`KITMAP_MODE` / `KITMAP_CLEAR_ITEMS` / `KITMAP_KILL_REWARD` /
    // `KITMAP_KILLSTREAK`).
    if y.has("KITMAP_MODE") {
        config.kitmap.enabled = y.bool("KITMAP_MODE.ENABLED");
        config.kitmap.disable_deathban = y.bool("KITMAP_MODE.DISABLE_DEATHBAN");
        config.kitmap.disable_dtr_loss = y.bool("KITMAP_MODE.DISABLE_DTR_LOSS");
        config.kitmap.disable_item_drop_in_safezone =
            y.bool("KITMAP_MODE.DISABLE_ITEM_DROP_IN_SAFEZONE");
        config.kitmap.pvp_timer_enabled = y.bool("KITMAP_MODE.PVP_TIMER_ENABLED");
        config.kitmap.spawn_teleport_delay = y.int("KITMAP_MODE.SPAWN_TELEPORT_DELAY").max(0) as u64;
    }
    if y.has("KITMAP_CLEAR_ITEMS") {
        config.kitmap.clear_items_enabled = y.bool("KITMAP_CLEAR_ITEMS.ENABLED");
        config.kitmap.clear_items_interval = y.int("KITMAP_CLEAR_ITEMS.INTERVAL").max(1) as u64;
    }
    if y.has("KITMAP_KILL_REWARD") {
        config.kitmap.kill_reward_enabled = y.bool("KITMAP_KILL_REWARD.ENABLED");
        config.kitmap.kill_reward_commands = y.string_list("KITMAP_KILL_REWARD.COMMANDS");
    }
    if y.has("KITMAP_KILLSTREAK") {
        config.kitmap.killstreak_enabled = y.bool("KITMAP_KILLSTREAK.ENABLED");
        // `KillstreakHandler#loadKillstreakRewards`: only integer keys count.
        config.kitmap.killstreak_rewards = y
            .keys("KITMAP_KILLSTREAK.KILLS")
            .into_iter()
            .filter_map(|key| {
                key.parse::<i32>().ok().map(|count| {
                    (count, y.string_list(&format!("KITMAP_KILLSTREAK.KILLS.{key}")))
                })
            })
            .collect();
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
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!("failed to parse {name}: {error}");
                preserve_corrupt(folder, name);
                T::default()
            }
        },
        Err(_) => T::default(),
    }
}

/// Renames a store that failed to parse out of the way (`<name>.corrupt-<ts>`)
/// so the next save can never silently overwrite (and destroy) the bad file.
fn preserve_corrupt(folder: &str, name: &str) {
    let backup = format!("{folder}/{name}.corrupt-{}", crate::util::now_millis());
    match std::fs::rename(format!("{folder}/{name}"), &backup) {
        Ok(()) => tracing::warn!("corrupt {name} preserved as {backup}"),
        Err(error) => tracing::warn!("failed to preserve corrupt {name}: {error}"),
    }
}

/// Writes `contents` to `<folder>/<name>` atomically: a sibling `.tmp` file is
/// written (and synced) first and then renamed over the target, which POSIX
/// guarantees is atomic. A crash mid-write can therefore never truncate or
/// corrupt an existing store.
pub fn atomic_write(folder: &str, name: &str, contents: &str) {
    use std::io::Write;

    let path = format!("{folder}/{name}");
    let tmp = format!("{path}.tmp");

    let result = std::fs::File::create(&tmp).and_then(|mut file| {
        file.write_all(contents.as_bytes())?;
        file.sync_all()
    });

    match result {
        Ok(()) => {
            if let Err(error) = std::fs::rename(&tmp, &path) {
                // e.g. a filesystem without rename support: fall back to a
                // plain write so saving still works.
                tracing::warn!("atomic rename failed for {name}: {error}; writing directly");
                let _ = std::fs::write(&path, contents);
                let _ = std::fs::remove_file(&tmp);
            }
        }
        Err(error) => {
            tracing::warn!("failed to write {name}: {error}");
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

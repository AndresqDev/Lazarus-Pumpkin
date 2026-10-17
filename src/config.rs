//! Port of Lazarus `config.yml`.
//!
//! The original file was YAML with 1.8 numeric `MATERIAL_ID` values (`"294:0"`).
//! Pumpkin is a modern (1.21+) server, so item references are registry keys
//! (`minecraft:diamond`) — see `PORTING.md`.
//!
//! Knockback and LunarClient configuration from the original is intentionally
//! omitted, as requested.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub timezone: String,
    pub date_format: String,

    pub join_welcome_message_enabled: bool,
    pub commands_cooldown: u64,
    pub spawn_mobs_in_warzone: bool,

    pub factions: FactionsConfig,
    pub relation_colors: RelationColors,
    pub warzone_radius: WorldRadii,
    pub border_size: WorldRadii,
    pub home_warmup: HomeWarmup,
    pub economy: EconomyConfig,
    pub timers: TimersConfig,
    pub sotw: SotwConfig,
    pub system_faction_names: SystemFactionNames,
    pub disabled_lazarus_commands: Vec<String>,
    pub blocked_commands: Vec<String>,
    pub tab: TabConfig,
    pub sidebar: SidebarConfig,
    pub deathban: DeathbanConfig,
    pub kitmap: KitmapConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            timezone: "Europe/Zagreb".into(),
            date_format: "dd.MM HH:mm:ss".into(),
            join_welcome_message_enabled: true,
            commands_cooldown: 2,
            spawn_mobs_in_warzone: false,
            factions: FactionsConfig::default(),
            relation_colors: RelationColors::default(),
            warzone_radius: WorldRadii { overworld: 500, nether: 300, end: 300 },
            border_size: WorldRadii { overworld: 3000, nether: 1000, end: 300 },
            home_warmup: HomeWarmup::default(),
            economy: EconomyConfig::default(),
            timers: TimersConfig::default(),
            sotw: SotwConfig::default(),
            system_faction_names: SystemFactionNames::default(),
            disabled_lazarus_commands: vec!["cmd1".into(), "cmd2".into()],
            blocked_commands: vec![
                "/op".into(),
                "/reload".into(),
                "/rl".into(),
                "/restart".into(),
                "/plugins".into(),
                "/pl".into(),
                "/icanhasbukkit".into(),
            ],
            tab: TabConfig::default(),
            sidebar: SidebarConfig::default(),
            deathban: DeathbanConfig::default(),
            kitmap: KitmapConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FactionsConfig {
    pub dtr_per_player: f64,
    pub max_dtr: f64,
    pub min_dtr: f64,
    pub solo_faction_dtr: f64,
    pub regen_per_minute: f64,
    pub freeze_duration_minutes: u64,
    /// `FACTION_DTR.DTR_CHARACTERS` — appended to the DTR display
    /// (full `&a◀` / regenerating `&e▲` / frozen `&c■`).
    pub dtr_full_character: String,
    pub dtr_regen_character: String,
    pub dtr_frozen_character: String,
    pub dtr_death_loss: WorldRadiiF64,

    pub faction_limit: usize,
    pub max_allies: usize,

    pub join_while_frozen: bool,
    pub leave_while_frozen: bool,
    pub disband_while_frozen: bool,
    pub unclaim_while_frozen: bool,
    pub leave_while_in_own_claim: bool,
    pub players_take_own_damage: bool,
    pub ally_friendly_fire: bool,

    pub must_be_connected: bool,
    pub max_claims: usize,
    pub claims_per_player: usize,
    pub min_claim_size: i32,
    pub max_claim_size: i32,
    pub claim_buffer: i32,
    pub claim_buffer_ignore_roads: bool,
    pub claim_price_per_block: f64,
    pub unclaim_price_multiplier: f64,

    pub name_min_length: usize,
    pub name_max_length: usize,
    pub disallowed_names: Vec<String>,

    pub map_radius: i32,
    pub auto_save_minutes: u64,
    pub show_info_on_join: bool,
    pub create_cooldown: u64,
    pub rename_cooldown: u64,
    pub open_change_cooldown: u64,
    pub warzone_break_after: WarzoneBreakAfter,

    // ---- `FACTION_TOP` (faction leaderboard points) ----
    pub allow_negative_points: bool,
    pub points_per_kill: i32,
    pub points_per_death: i32,
    pub koth_cap_points: i32,
    pub conquest_cap_points: i32,
    pub dtc_destroy_points: i32,
}

impl Default for FactionsConfig {
    fn default() -> Self {
        Self {
            dtr_per_player: 0.9,
            max_dtr: 6.5,
            min_dtr: -6.5,
            solo_faction_dtr: 1.1,
            regen_per_minute: 0.1,
            freeze_duration_minutes: 45,
            dtr_full_character: "&a◀".into(),
            dtr_regen_character: "&e▲".into(),
            dtr_frozen_character: "&c■".into(),
            dtr_death_loss: WorldRadiiF64 { overworld: 1.0, nether: 0.5, end: 1.0 },
            faction_limit: 15,
            max_allies: 0,
            join_while_frozen: false,
            leave_while_frozen: true,
            disband_while_frozen: false,
            unclaim_while_frozen: false,
            leave_while_in_own_claim: false,
            players_take_own_damage: true,
            ally_friendly_fire: false,
            must_be_connected: true,
            max_claims: 6,
            claims_per_player: 1,
            min_claim_size: 5,
            max_claim_size: 40,
            claim_buffer: 2,
            claim_buffer_ignore_roads: true,
            claim_price_per_block: 0.5,
            unclaim_price_multiplier: 0.5,
            name_min_length: 3,
            name_max_length: 15,
            disallowed_names: vec!["Staff".into(), "Admin".into(), "Dick".into(), "Test".into()],
            map_radius: 32,
            auto_save_minutes: 30,
            show_info_on_join: true,
            create_cooldown: 30,
            rename_cooldown: 30,
            open_change_cooldown: 30,
            warzone_break_after: WarzoneBreakAfter::default(),
            allow_negative_points: true,
            points_per_kill: 5,
            points_per_death: -5,
            koth_cap_points: 20,
            conquest_cap_points: 100,
            dtc_destroy_points: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RelationColors {
    pub teammate: String,
    pub ally: String,
    pub enemy: String,
    pub archer_tagged: String,
    pub focused: String,
    pub sotw: String,
    pub staff_mode: String,
}

impl Default for RelationColors {
    fn default() -> Self {
        Self {
            teammate: "&a".into(),
            ally: "&d".into(),
            enemy: "&c".into(),
            archer_tagged: "&4".into(),
            focused: "&b&l".into(),
            sotw: "&6".into(),
            staff_mode: "&8[&bS&8] &b".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldRadii {
    pub overworld: i32,
    pub nether: i32,
    pub end: i32,
}

impl Default for WorldRadii {
    fn default() -> Self {
        Self { overworld: 1000, nether: 500, end: 500 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldRadiiF64 {
    pub overworld: f64,
    pub nether: f64,
    pub end: f64,
}

impl Default for WorldRadiiF64 {
    fn default() -> Self {
        Self { overworld: 1.0, nether: 0.5, end: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeWarmup {
    pub overworld: u64,
    pub nether: u64,
    pub end: u64,
    pub enemy_territory: u64,
    pub deny_from_enemy_territory: bool,
}

impl Default for HomeWarmup {
    fn default() -> Self {
        Self {
            overworld: 10,
            nether: 30,
            end: 30,
            enemy_territory: 20,
            deny_from_enemy_territory: true,
        }
    }
}

/// Kitmap mode (`KITMAP_MODE` / `KITMAP_CLEAR_ITEMS` / `KITMAP_KILL_REWARD` /
/// `KITMAP_KILLSTREAK` sections of the original `config.yml`).
///
/// The typed `config.toml` is legacy: [`crate::state::sync_faction_yaml`] makes
/// the raw `config.yml` authoritative for these values on every load.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KitmapConfig {
    pub enabled: bool,
    pub disable_deathban: bool,
    pub disable_dtr_loss: bool,
    pub disable_item_drop_in_safezone: bool,
    pub pvp_timer_enabled: bool,
    /// Spawn teleport delay in seconds (`/spawn`; command not ported yet).
    pub spawn_teleport_delay: u64,

    pub clear_items_enabled: bool,
    pub clear_items_interval: u64,

    pub kill_reward_enabled: bool,
    pub kill_reward_commands: Vec<String>,

    pub killstreak_enabled: bool,
    /// Kill count -> console commands (`KITMAP_KILLSTREAK.KILLS`).
    pub killstreak_rewards: BTreeMap<i32, Vec<String>>,
}

impl Default for KitmapConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            disable_deathban: true,
            disable_dtr_loss: false,
            disable_item_drop_in_safezone: true,
            pvp_timer_enabled: false,
            spawn_teleport_delay: 10,
            clear_items_enabled: true,
            clear_items_interval: 30,
            kill_reward_enabled: true,
            kill_reward_commands: vec!["econ give <player> 150".to_string()],
            killstreak_enabled: true,
            killstreak_rewards: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct EconomyConfig {
    pub max_balance: i64,
    pub default_player_balance: i64,
    pub default_faction_balance: i64,
}

impl Default for EconomyConfig {
    fn default() -> Self {
        Self { max_balance: 1_000_000, default_player_balance: 200, default_faction_balance: 0 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct TimersConfig {
    pub combat_tag_seconds: u64,
    pub enderpearl_seconds: u64,
    pub gapple_seconds: u64,
    pub gapple_enchanted_seconds: u64,
    pub pvp_protection_seconds: u64,
    pub logout_delay_seconds: u64,
    pub stuck_warmup_seconds: u64,
    pub stuck_movement_radius: f64,
    pub default_lives: i32,
}

impl Default for TimersConfig {
    fn default() -> Self {
        Self {
            combat_tag_seconds: 30,
            enderpearl_seconds: 15,
            gapple_seconds: 120,
            gapple_enchanted_seconds: 7200,
            pvp_protection_seconds: 3600,
            logout_delay_seconds: 30,
            stuck_warmup_seconds: 60,
            stuck_movement_radius: 5.0,
            default_lives: 0,
        }
    }
}

/// Port of the `SOTW_TIMER` section of `config.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SotwConfig {
    /// Default `/sotw start` duration, in minutes.
    pub default_time_minutes: u64,
    /// Cancel every creature spawn that is not spawner-driven while SOTW runs.
    pub spawn_mobs_from_spawners_only: bool,
    /// Hide players that are inside the safezone while SOTW runs.
    pub hide_players: bool,
}

impl Default for SotwConfig {
    fn default() -> Self {
        Self { default_time_minutes: 120, spawn_mobs_from_spawners_only: true, hide_players: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemFactionNames {
    pub wilderness: String,
    pub warzone: String,
    pub safezone: String,
}

impl Default for SystemFactionNames {
    fn default() -> Self {
        Self {
            wilderness: "&2Wilderness".into(),
            warzone: "&4Warzone".into(),
            // `SpawnFaction` (name "Spawn", colour `&a`).
            safezone: "&aSpawn".into(),
        }
    }
}

/// Port of `tab.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TabConfig {
    pub enabled: bool,
    pub header: Vec<String>,
    pub footer: Vec<String>,
    /// The four 20-line columns from tab.yml (LEFT, CENTER, RIGHT, FAR_RIGHT).
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    pub far_right: Vec<String>,
}

impl Default for TabConfig {
    fn default() -> Self {
        let blank = || (0..20).map(|_| String::new()).collect::<Vec<_>>();

        let mut left = blank();
        left[2] = "&5&lFaction Info".into();
        left[6] = "&5&lStatistics".into();
        left[7] = "&7Kills: <kills>".into();
        left[8] = "&7Deaths: <deaths>".into();
        left[9] = "&7Lives: <lives>".into();
        left[11] = "&5&lLocation".into();
        left[12] = "<faction>".into();
        left[13] = "&7<location>".into();

        let mut center = blank();
        center[1] = "&d&lHCF &8(&cMap I&8)".into();
        center[2] = "&7Online: <online>".into();

        let mut right = blank();
        right[2] = "&5&lMapKit".into();
        right[3] = "&7Sharp I, Prot I".into();
        right[5] = "&5&lBorder".into();
        right[6] = "&7Size: 3000".into();
        right[8] = "&5&lEnd exit".into();
        right[9] = "&7South (0, 50)".into();

        let mut far_right = blank();
        far_right[6] = "&c&lWarning!".into();
        far_right[8] = "&7Please use 1.7 for".into();
        far_right[9] = "&7the optimal gaming".into();
        far_right[10] = "&7experience!".into();

        Self {
            enabled: true,
            header: vec![String::new(), "&5&lLazarus HCF core".into(), String::new()],
            footer: vec![String::new(), "&7mc.pumpkinmc.org".into(), String::new()],
            left,
            center,
            right,
            far_right,
        }
    }
}

/// Port of `scoreboard.yml` (template form).
///
/// Each entry in `lines` may contain the placeholders listed in the README;
/// lines that resolve to an empty string are hidden, so timer placeholders can
/// disappear naturally.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarConfig {
    pub enabled: bool,
    pub title: String,
    pub line_color: String,
    pub line_invisible: bool,
    pub line_after_every_section: bool,
    pub footer_enabled: bool,
    pub footer: String,

    // Section placeholders (port of scoreboard.yml).
    pub claim: String,
    pub combat_tag: String,
    pub enderpearl: String,
    pub pvp_protection: String,
    pub home: String,
    pub stuck: String,
    pub teleport: String,
    pub logout: String,
    pub apple: String,
    pub gapple: String,

    // SOTW section (port of scoreboard.yml `SOTW_PLACEHOLDER` /
    // `SOTW_ENABLE_PLACEHOLDER`; the second one is used when the viewing player
    // opted out of the damage protection).
    pub sotw: String,
    pub sotw_enabled: String,

    // Staff / vanish section.
    pub staffmode: String,
    pub vanish: String,
    pub visibility: String,
    pub chatmode: String,
    pub gamemode: String,
    pub online: String,
    pub staff_visibility_vanished: String,
    pub staff_visibility_visible: String,
    pub staff_chat_staffchat: String,
    pub staff_chat_global: String,
    pub staff_gamemode_survival: String,
    pub staff_gamemode_creative: String,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            title: "&d&lHCF &8(&cMap I&8)".into(),
            line_color: "&7".into(),
            line_invisible: false,
            line_after_every_section: true,
            footer_enabled: true,
            footer: "&7lazarus.com".into(),
            claim: "&c&lClaim&7: ".into(),
            combat_tag: "&c&lSpawn Tag&7: &c".into(),
            enderpearl: "&e&lEnderpearl&7: &c".into(),
            pvp_protection: "&a&lPvP Timer&7: &c".into(),
            home: "&b&lHome teleport&7: &c".into(),
            stuck: "&6&lStuck teleport&7: &c".into(),
            teleport: "&a&lTeleporting&7: &c".into(),
            logout: "&b&lLogout&7: &c".into(),
            apple: "&e&lApple&7: &c".into(),
            gapple: "&6&lGApple&7: &c".into(),
            sotw: "&a&lSOTW&7: &c".into(),
            sotw_enabled: "&c&l&mSOTW&7: &c".into(),
            staffmode: "&e&lStaff Mode".into(),
            vanish: "&e&lVanish Mode".into(),
            visibility: " &6* &9Visibility&7: ".into(),
            chatmode: " &6* &9ChatMode&7: ".into(),
            gamemode: " &6* &9GameMode&7: ".into(),
            online: " &6* &9Online&7: &c".into(),
            staff_visibility_vanished: "&aVanished".into(),
            staff_visibility_visible: "&cVisible".into(),
            staff_chat_staffchat: "&aStaffChat".into(),
            staff_chat_global: "&cGlobal".into(),
            staff_gamemode_survival: "&cSurvival".into(),
            staff_gamemode_creative: "&aCreative".into(),
        }
    }
}

/// Port of the `DEATHBAN` section of `config.yml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DeathbanConfig {
    pub enabled: bool,
    /// Default ban time in minutes.
    pub default_ban_time: u64,
    /// `DEATHBAN.BAN_TIMES` — first matching permission wins, in config order
    /// (port of `DeathbanManager#getBanTime`).
    pub ban_times: Vec<BanTime>,
}

/// One entry of `DEATHBAN.BAN_TIMES`: time in minutes + required permission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanTime {
    pub minutes: u64,
    pub permission: String,
}

impl Default for DeathbanConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_ban_time: 180,
            ban_times: vec![
                BanTime { minutes: 30, permission: "lazarus.deathban.30".into() },
                BanTime { minutes: 45, permission: "lazarus.deathban.45".into() },
                BanTime { minutes: 60, permission: "lazarus.deathban.60".into() },
                BanTime { minutes: 75, permission: "lazarus.deathban.75".into() },
                BanTime { minutes: 90, permission: "lazarus.deathban.90".into() },
            ],
        }
    }
}

/// Port of `FACTION_PLAYER.WARZONE_BREAK_AFTER`: blocks closer than this to spawn
/// are protected inside the warzone. Beyond it, building is allowed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct WarzoneBreakAfter {
    pub overworld: i32,
    pub nether: i32,
    /// The End is never breakable in the warzone (matches the original).
    pub end: i32,
}

impl Default for WarzoneBreakAfter {
    fn default() -> Self {
        // Same as the warzone radius: the whole warzone is protected by default.
        Self { overworld: 500, nether: 300, end: 0 }
    }
}

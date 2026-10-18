//! Leaderboards — port of `me.qiooip.lazarus.handlers.leaderboard`.
//!
//! The original keeps a `ConcurrentSkipListSet<UuidCacheEntry<Integer>>` per
//! board (four player boards filled from `Userdata`, four faction boards filled
//! from `PlayerFaction`). The sets sort by value descending, then by insertion
//! timestamp ascending, then by key; they hold at most ten entries. Every
//! mutation fires a `LeaderboardUpdateEvent` (only when the key was already
//! cached or the set overflowed) which refreshes the leaderboard holograms.
//!
//! This port stores the same entries as sorted `Vec<Entry>` (serde writes them
//! as JSON arrays in the same order the Java `ConcurrentSkipListSet` iterates in)
//! and calls the hologram refresh directly from [`refresh_entry`] — there is no
//! Bukkit event bus to route it through. The original's three type enums
//! (`PlayerLeaderboardType`, `FactionLeaderboardType`, `LeaderboardHologramType`)
//! collapse into the single [`Board`] enum, which maps 1:1 to the eight rows.
//!
//! Like the original, every hook is a mutator: the caller's stat change and the
//! cache refresh happen under the same state guard.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::utils;

/// Maximum number of cached entries per board (`leaderboard.pollLast()` above
/// this size).
pub const MAX_ENTRIES: usize = 10;

/// One cache entry (`UuidCacheEntry<Integer>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Player UUID or faction id.
    pub key: String,
    pub name: String,
    pub value: i64,
    pub timestamp: i64,
}

impl Entry {
    pub fn new(key: &str, name: String, value: i64) -> Self {
        Self {
            key: key.to_string(),
            name,
            value,
            timestamp: utils::now_millis(),
        }
    }
}

/// `CacheEntry#compareTo`: value descending, timestamp ascending, key ascending.
fn compare(left: &Entry, right: &Entry) -> Ordering {
    right
        .value
        .cmp(&left.value)
        .then_with(|| left.timestamp.cmp(&right.timestamp))
        .then_with(|| left.key.cmp(&right.key))
}

/// Re-inserts `entry` keeping the list sorted and capped, exactly like
/// `LeaderboardHandler#updateCacheValue`. Returns whether the cache "updated"
/// (the key was already cached, or the list overflowed and trimmed its tail) —
/// the original fires the hologram refresh only in that case.
pub fn insert_entry(list: &mut Vec<Entry>, entry: Entry) -> bool {
    let mut updated = match list.iter().position(|cached| cached.key == entry.key) {
        Some(index) => {
            list.remove(index);
            true
        }
        None => false,
    };

    let index = list
        .iter()
        .position(|cached| compare(cached, &entry) == Ordering::Greater)
        .unwrap_or(list.len());
    list.insert(index, entry);

    if list.len() > MAX_ENTRIES {
        list.pop();
        updated = true;
    }

    updated
}

/// `PlayerCacheHolder` — persisted at `<dataFolder>/leaderboards/players.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerBoards {
    #[serde(rename = "topKills")]
    pub top_kills: Vec<Entry>,
    #[serde(rename = "topDeaths")]
    pub top_deaths: Vec<Entry>,
    #[serde(rename = "topBalance")]
    pub top_balance: Vec<Entry>,
    #[serde(rename = "topHighestKillstreak")]
    pub top_highest_killstreak: Vec<Entry>,
}

/// `FactionCacheHolder` — persisted at `<dataFolder>/leaderboards/factions.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FactionBoards {
    #[serde(rename = "topKills")]
    pub top_kills: Vec<Entry>,
    #[serde(rename = "topPoints")]
    pub top_points: Vec<Entry>,
    #[serde(rename = "topBalance")]
    pub top_balance: Vec<Entry>,
    #[serde(rename = "topKothsCapped")]
    pub top_koths_capped: Vec<Entry>,
}

/// One of the eight leaderboards. The serde names are the hologram
/// `paramName`s the original's `LeaderboardHologramType#getByName` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Board {
    #[serde(rename = "playerKills")]
    PlayerKills,
    #[serde(rename = "playerDeaths")]
    PlayerDeaths,
    #[serde(rename = "playerBalance")]
    PlayerBalance,
    #[serde(rename = "playerKillstreak")]
    PlayerKillstreak,
    #[serde(rename = "factionKills")]
    FactionKills,
    #[serde(rename = "factionPoints")]
    FactionPoints,
    #[serde(rename = "factionBalance")]
    FactionBalance,
    #[serde(rename = "factionKoths")]
    FactionKoths,
}

impl Board {
    /// Every player board in `PlayerLeaderboardType.values()` order.
    pub const PLAYER_BOARDS: [Board; 4] = [
        Board::PlayerKills,
        Board::PlayerDeaths,
        Board::PlayerBalance,
        Board::PlayerKillstreak,
    ];

    /// Every faction board in `FactionLeaderboardType.values()` order.
    pub const FACTION_BOARDS: [Board; 4] = [
        Board::FactionKills,
        Board::FactionPoints,
        Board::FactionBalance,
        Board::FactionKoths,
    ];

    pub fn is_faction(self) -> bool {
        matches!(
            self,
            Board::FactionKills | Board::FactionPoints | Board::FactionBalance | Board::FactionKoths
        )
    }

    /// `LeaderboardHologramType#getByName` (`/hologram create <type>`).
    pub fn from_hologram_param(name: &str) -> Option<Board> {
        [
            Board::PlayerKills,
            Board::PlayerDeaths,
            Board::PlayerBalance,
            Board::PlayerKillstreak,
            Board::FactionKills,
            Board::FactionPoints,
            Board::FactionBalance,
            Board::FactionKoths,
        ]
        .into_iter()
        .find(|board| board.hologram_param().eq_ignore_ascii_case(name))
    }

    /// The persisted hologram parameter name.
    pub fn hologram_param(self) -> &'static str {
        match self {
            Board::PlayerKills => "playerKills",
            Board::PlayerDeaths => "playerDeaths",
            Board::PlayerBalance => "playerBalance",
            Board::PlayerKillstreak => "playerKillstreak",
            Board::FactionKills => "factionKills",
            Board::FactionPoints => "factionPoints",
            Board::FactionBalance => "factionBalance",
            Board::FactionKoths => "factionKoths",
        }
    }

    /// `PlayerLeaderboardType#getByName` (`/leaderboard`, `/topkills`, ...).
    pub fn from_player_param(name: &str) -> Option<Board> {
        Board::PLAYER_BOARDS
            .into_iter()
            .find(|board| board.player_param().eq_ignore_ascii_case(name))
    }

    /// `FactionLeaderboardType#getByName` (`/f top`).
    pub fn from_faction_param(name: &str) -> Option<Board> {
        Board::FACTION_BOARDS
            .into_iter()
            .find(|board| board.faction_param().eq_ignore_ascii_case(name))
    }

    /// The `getParamName()` of the player type (`kills|deaths|balance|killstreak`).
    fn player_param(self) -> &'static str {
        match self {
            Board::PlayerKills => "kills",
            Board::PlayerDeaths => "deaths",
            Board::PlayerBalance => "balance",
            Board::PlayerKillstreak => "killstreak",
            _ => unreachable!("not a player board"),
        }
    }

    /// The `getParamName()` of the faction type (`kills|points|balance|koths`).
    fn faction_param(self) -> &'static str {
        match self {
            Board::FactionKills => "kills",
            Board::FactionPoints => "points",
            Board::FactionBalance => "balance",
            Board::FactionKoths => "koths",
            _ => unreachable!("not a faction board"),
        }
    }

    /// `LEADERBOARDS.*.TITLE` (commands).
    fn title_key(self) -> &'static str {
        match self {
            Board::PlayerKills => "LEADERBOARDS.TOP_KILLS.TITLE",
            Board::PlayerDeaths => "LEADERBOARDS.TOP_DEATHS.TITLE",
            Board::PlayerBalance => "LEADERBOARDS.TOP_BALANCE.TITLE",
            Board::PlayerKillstreak => "LEADERBOARDS.TOP_KILLSTREAK.TITLE",
            Board::FactionKills => "LEADERBOARDS.FACTION_TOP_KILLS.TITLE",
            Board::FactionPoints => "LEADERBOARDS.FACTION_TOP_POINTS.TITLE",
            Board::FactionBalance => "LEADERBOARDS.FACTION_TOP_BALANCE.TITLE",
            Board::FactionKoths => "LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.TITLE",
        }
    }

    /// `LEADERBOARDS.*.LINE_FORMAT` (commands).
    fn line_format_key(self) -> &'static str {
        match self {
            Board::PlayerKills => "LEADERBOARDS.TOP_KILLS.LINE_FORMAT",
            Board::PlayerDeaths => "LEADERBOARDS.TOP_DEATHS.LINE_FORMAT",
            Board::PlayerBalance => "LEADERBOARDS.TOP_BALANCE.LINE_FORMAT",
            Board::PlayerKillstreak => "LEADERBOARDS.TOP_KILLSTREAK.LINE_FORMAT",
            Board::FactionKills => "LEADERBOARDS.FACTION_TOP_KILLS.LINE_FORMAT",
            Board::FactionPoints => "LEADERBOARDS.FACTION_TOP_POINTS.LINE_FORMAT",
            Board::FactionBalance => "LEADERBOARDS.FACTION_TOP_BALANCE.LINE_FORMAT",
            Board::FactionKoths => "LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.LINE_FORMAT",
        }
    }

    /// `HOLOGRAM_LEADERBOARDS.*.HEADER`.
    fn hologram_header_key(self) -> &'static str {
        match self {
            Board::PlayerKills => "HOLOGRAM_LEADERBOARDS.TOP_KILLS.HEADER",
            Board::PlayerDeaths => "HOLOGRAM_LEADERBOARDS.TOP_DEATHS.HEADER",
            Board::PlayerBalance => "HOLOGRAM_LEADERBOARDS.TOP_BALANCE.HEADER",
            Board::PlayerKillstreak => "HOLOGRAM_LEADERBOARDS.TOP_KILLSTREAK.HEADER",
            Board::FactionKills => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KILLS.HEADER",
            Board::FactionPoints => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_POINTS.HEADER",
            Board::FactionBalance => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_BALANCE.HEADER",
            Board::FactionKoths => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.HEADER",
        }
    }

    /// `HOLOGRAM_LEADERBOARDS.*.FOOTER`.
    fn hologram_footer_key(self) -> &'static str {
        match self {
            Board::PlayerKills => "HOLOGRAM_LEADERBOARDS.TOP_KILLS.FOOTER",
            Board::PlayerDeaths => "HOLOGRAM_LEADERBOARDS.TOP_DEATHS.FOOTER",
            Board::PlayerBalance => "HOLOGRAM_LEADERBOARDS.TOP_BALANCE.FOOTER",
            Board::PlayerKillstreak => "HOLOGRAM_LEADERBOARDS.TOP_KILLSTREAK.FOOTER",
            Board::FactionKills => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KILLS.FOOTER",
            Board::FactionPoints => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_POINTS.FOOTER",
            Board::FactionBalance => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_BALANCE.FOOTER",
            Board::FactionKoths => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.FOOTER",
        }
    }

    /// `HOLOGRAM_LEADERBOARDS.*.LINE_FORMAT` (placeholders `<player>`/`<value>`).
    fn hologram_line_format_key(self) -> &'static str {
        match self {
            Board::PlayerKills => "HOLOGRAM_LEADERBOARDS.TOP_KILLS.LINE_FORMAT",
            Board::PlayerDeaths => "HOLOGRAM_LEADERBOARDS.TOP_DEATHS.LINE_FORMAT",
            Board::PlayerBalance => "HOLOGRAM_LEADERBOARDS.TOP_BALANCE.LINE_FORMAT",
            Board::PlayerKillstreak => "HOLOGRAM_LEADERBOARDS.TOP_KILLSTREAK.LINE_FORMAT",
            Board::FactionKills => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KILLS.LINE_FORMAT",
            Board::FactionPoints => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_POINTS.LINE_FORMAT",
            Board::FactionBalance => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_BALANCE.LINE_FORMAT",
            Board::FactionKoths => "HOLOGRAM_LEADERBOARDS.FACTION_TOP_KOTHS_CAPPED.LINE_FORMAT",
        }
    }

    /// The cached entries for this board.
    pub fn entries(self, state: &AppState) -> &[Entry] {
        match self {
            Board::PlayerKills => &state.player_leaderboards.top_kills,
            Board::PlayerDeaths => &state.player_leaderboards.top_deaths,
            Board::PlayerBalance => &state.player_leaderboards.top_balance,
            Board::PlayerKillstreak => &state.player_leaderboards.top_highest_killstreak,
            Board::FactionKills => &state.faction_leaderboards.top_kills,
            Board::FactionPoints => &state.faction_leaderboards.top_points,
            Board::FactionBalance => &state.faction_leaderboards.top_balance,
            Board::FactionKoths => &state.faction_leaderboards.top_koths_capped,
        }
    }

    fn entries_mut(self, state: &mut AppState) -> &mut Vec<Entry> {
        match self {
            Board::PlayerKills => &mut state.player_leaderboards.top_kills,
            Board::PlayerDeaths => &mut state.player_leaderboards.top_deaths,
            Board::PlayerBalance => &mut state.player_leaderboards.top_balance,
            Board::PlayerKillstreak => &mut state.player_leaderboards.top_highest_killstreak,
            Board::FactionKills => &mut state.faction_leaderboards.top_kills,
            Board::FactionPoints => &mut state.faction_leaderboards.top_points,
            Board::FactionBalance => &mut state.faction_leaderboards.top_balance,
            Board::FactionKoths => &mut state.faction_leaderboards.top_koths_capped,
        }
    }

    /// `UserdataValueType#getNewValue`.
    fn player_value(self, user: &crate::userdata::Userdata) -> i64 {
        match self {
            Board::PlayerKills => user.kills as i64,
            Board::PlayerDeaths => user.deaths as i64,
            Board::PlayerBalance => user.balance,
            Board::PlayerKillstreak => user.highest_killstreak as i64,
            _ => unreachable!("not a player board"),
        }
    }

    /// `FactionDataType#getNewValue`.
    fn faction_value(self, faction: &crate::factions::Faction) -> i64 {
        match self {
            Board::FactionKills => faction.kills as i64,
            Board::FactionPoints => faction.points as i64,
            Board::FactionBalance => faction.balance,
            Board::FactionKoths => faction.koths_capped as i64,
            _ => unreachable!("not a faction board"),
        }
    }

    /// Title used by the chat commands.
    pub fn title(self, state: &AppState) -> String {
        state.raw.language.string(self.title_key())
    }

    /// Line format used by the chat commands.
    pub fn line_format(self, state: &AppState) -> String {
        state.raw.language.string(self.line_format_key())
    }
}

/// `LeaderboardHandler#updateCacheValue` for one cached key: re-reads the
/// current value, re-inserts the entry (fresh timestamp) and, when the original
/// would have fired `LeaderboardUpdateEvent`, refreshes the holograms.
fn refresh_entry(state: &mut AppState, board: Board, key: &str) -> bool {
    let current = if board.is_faction() {
        state
            .factions
            .get(key)
            .map(|faction| (faction.name.clone(), board.faction_value(faction)))
    } else {
        state
            .userdata
            .get(key)
            .map(|user| (user.name.clone(), board.player_value(user)))
    };

    let Some((name, value)) = current else {
        return false;
    };

    let updated = insert_entry(board.entries_mut(state), Entry::new(key, name, value));

    if updated {
        crate::hologram::refresh_board_holograms(state, board);
    }

    updated
}

// ---------------------------------------------------------------------------
// Hook points (the original's event listeners, inlined into the mutators).
// ---------------------------------------------------------------------------

/// `Userdata#updateKillStats` (kills board).
pub fn add_kill(state: &mut AppState, uuid: &str, message: String) {
    if let Some(user) = state.userdata.get_mut(uuid) {
        user.add_kill(message);
    }
    refresh_entry(state, Board::PlayerKills, uuid);
}

/// `Userdata#updateDeathStats` (deaths board).
pub fn add_death(state: &mut AppState, uuid: &str, message: String) {
    if let Some(user) = state.userdata.get_mut(uuid) {
        user.add_death(message);
    }
    refresh_entry(state, Board::PlayerDeaths, uuid);
}

/// `Userdata#addKillstreak` — the original only fires
/// `changeHighestKillstreak` when the record grows, so the board is only
/// refreshed then.
pub fn add_killstreak(state: &mut AppState, uuid: &str) {
    let improved = match state.userdata.get_mut(uuid) {
        Some(user) => {
            let previous = user.highest_killstreak;
            user.add_killstreak();
            user.highest_killstreak != previous
        }
        None => false,
    };

    if improved {
        refresh_entry(state, Board::PlayerKillstreak, uuid);
    }
}

/// `Userdata#changeBalance`.
pub fn set_player_balance(state: &mut AppState, uuid: &str, balance: i64) {
    if let Some(user) = state.userdata.get_mut(uuid) {
        user.balance = balance;
    }
    refresh_entry(state, Board::PlayerBalance, uuid);
}

/// `PlayerFaction#addKill`.
pub fn faction_add_kill(state: &mut AppState, faction_id: &str) {
    if let Some(faction) = state.factions.get_mut(faction_id) {
        faction.add_kill();
    }
    refresh_entry(state, Board::FactionKills, faction_id);
}

/// `PlayerFaction#addBalance` / `#removeBalance`.
pub fn faction_set_balance(state: &mut AppState, faction_id: &str, balance: i64) {
    if let Some(faction) = state.factions.get_mut(faction_id) {
        faction.balance = balance;
    }
    refresh_entry(state, Board::FactionBalance, faction_id);
}

/// Re-caches one faction row without changing the faction — for call sites that
/// already applied the value through a field borrow.
pub fn refresh_faction(state: &mut AppState, board: Board, faction_id: &str) {
    refresh_entry(state, board, faction_id);
}

/// `PlayerFaction#changeKills` (the `/f admin setkills` command).
pub fn faction_set_kills(state: &mut AppState, faction_id: &str, kills: i32) {
    if let Some(faction) = state.factions.get_mut(faction_id) {
        faction.kills = kills;
    }
    refresh_entry(state, Board::FactionKills, faction_id);
}

/// `PlayerFaction#incrementPoints`.
pub fn faction_increment_points(state: &mut AppState, faction_id: &str, value: i32) {
    let allow_negative = state.config.factions.allow_negative_points;
    if let Some(faction) = state.factions.get_mut(faction_id) {
        faction.increment_points(value, allow_negative);
    }
    refresh_entry(state, Board::FactionPoints, faction_id);
}

/// `PlayerFaction#changePoints`.
pub fn faction_change_points(state: &mut AppState, faction_id: &str, value: i32) {
    let allow_negative = state.config.factions.allow_negative_points;
    if let Some(faction) = state.factions.get_mut(faction_id) {
        faction.change_points(value, allow_negative);
    }
    refresh_entry(state, Board::FactionPoints, faction_id);
}

/// `Userdata` constructor: the original fires `changeKills(0)`,
/// `changeDeaths(0)`, `changeHighestKillstreak(0)` and
/// `changeBalance(DEFAULT_BALANCE_PLAYER)` for every new player, which caches a
/// zero (or default-balance) row on all four player boards.
pub fn on_userdata_created(state: &mut AppState, uuid: &str) {
    for board in Board::PLAYER_BOARDS {
        refresh_entry(state, board, uuid);
    }
}

/// `UserdataManager#checkUsernameChange` + `PlayerUsernameChangeEvent`: the new
/// name is written back and every player board re-caches the entry.
pub fn on_userdata_renamed(state: &mut AppState, uuid: &str, new_name: String) {
    state.userdata.update_name(uuid, new_name);
    for board in Board::PLAYER_BOARDS {
        refresh_entry(state, board, uuid);
    }
}

/// `PlayerFaction` constructor (see [`on_userdata_created`]).
pub fn on_faction_created(state: &mut AppState, faction_id: &str) {
    for board in Board::FACTION_BOARDS {
        refresh_entry(state, board, faction_id);
    }
}

/// `FactionRenameEvent` handling: `LeaderboardHandler#handleFactionNameChange`.
pub fn on_faction_renamed(state: &mut AppState, faction_id: &str) {
    for board in Board::FACTION_BOARDS {
        refresh_entry(state, board, faction_id);
    }
}

/// `LeaderboardHandler#removeFactionOnDisband`: drops the faction from every
/// board that contained it and, in that case, re-caches ALL player factions
/// with their current values (the original's `reCacheFactionsOnDisband`).
pub fn on_faction_disbanded(state: &mut AppState, faction_id: &str) {
    let mut containing: Vec<Board> = Vec::new();

    for board in Board::FACTION_BOARDS {
        let list = board.entries_mut(state);
        if let Some(index) = list.iter().position(|entry| entry.key == faction_id) {
            list.remove(index);
            containing.push(board);
        }
    }

    if containing.is_empty() {
        return;
    }

    let ids: Vec<String> = state
        .factions
        .player_factions()
        .map(|faction| faction.id.clone())
        .collect();

    for board in containing {
        let timestamp = utils::now_millis();
        let mut fresh: Vec<Entry> = ids
            .iter()
            .filter_map(|id| {
                let faction = state.factions.get(id)?;
                Some(Entry {
                    key: id.clone(),
                    name: faction.name.clone(),
                    value: board.faction_value(faction),
                    timestamp,
                })
            })
            .collect();

        fresh.sort_by(compare);
        fresh.truncate(MAX_ENTRIES);
        *board.entries_mut(state) = fresh;

        // The original fires `LeaderboardUpdateEvent` unconditionally after a
        // re-cache.
        crate::hologram::refresh_board_holograms(state, board);
    }
}

/// The number of lines [`hologram_lines`] renders (the leaderboard layout is
/// fixed: header list + ten slots + footer list), computed without building the
/// text.
pub fn hologram_line_count(state: &AppState, board: Board) -> usize {
    let language = &state.raw.language;
    language.string_list(board.hologram_header_key()).len()
        + MAX_ENTRIES
        + language.string_list(board.hologram_footer_key()).len()
}

/// The hologram layout of a board (`LeaderboardHologram#createHologramLines`):
/// header, ten fixed slots (cached entries padded with
/// `HOLOGRAM_LEADERBOARDS.EMPTY_LINE_FORMAT`) and footer.
pub fn hologram_lines(state: &AppState, board: Board) -> Vec<String> {
    let language = &state.raw.language;
    let mut lines = language.string_list(board.hologram_header_key());

    let line_format = language.string(board.hologram_line_format_key());
    let empty_format = language.string("HOLOGRAM_LEADERBOARDS.EMPTY_LINE_FORMAT");

    for index in 0..MAX_ENTRIES {
        let number = index + 1;

        match board.entries(state).get(index) {
            Some(entry) => lines.push(utils::replace_all(
                &line_format,
                &[
                    ("<number>", &number.to_string()),
                    ("<player>", &entry.name),
                    ("<value>", &entry.value.to_string()),
                ],
            )),
            None => lines.push(utils::replace(&empty_format, "<number>", &number.to_string())),
        }
    }

    lines.extend(language.string_list(board.hologram_footer_key()));
    lines
}

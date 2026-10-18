//! Tab list driver for the WASM plugin — 1:1 port of `tab/`.
//!
//! The artificial tab is rendered by the native `native_tab` companion (the WASM
//! host cannot send `CPlayerInfoUpdate`). We build the desired state from
//! `tab.yml` here and write it to `tab_state.json`; the native side diffs it and
//! only updates changed slots.
//!
//! Mirrors the original layout:
//! * `TabUpdaterImpl.loadInitialSet/loadUpdates` — the four `LEFT/CENTER/RIGHT/FAR_RIGHT`
//!   columns, with `<kills>`, `<deaths>`, `<lives>`, `<balance>`, `<faction>`,
//!   `<online>` and `<location>` placeholders.
//! * `FactionInfoModule`, `PlayerListModule`, `NextKothModule` — `tab.yml` modules.
//! * Every entry uses the custom head from `TabManager.VALUE/SIGNATURE`.

pub mod api;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use pumpkin_plugin_api::{Player, Server};

use crate::factions::{Faction, FactionMember};
use crate::state::{state, AppState};
use crate::tab::api::{Skin, SlotSpec, TabState};

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;

const FACES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];

/// `TabManager.VALUE` — the custom head applied to every artificial entry.
const TAB_SKIN_VALUE: &str = "eyJ0aW1lc3RhbXAiOjE0MTEyNjg3OTI3NjUsInByb2ZpbGVJZCI6IjNmYmVjN2RkMGE1ZjQwYmY5ZDExODg1YTU0NTA3MTEyIiwicHJvZmlsZU5hbWUiOiJsYXN0X3VzZXJuYW1lIiwidGV4dHVyZXMiOnsiU0tJTiI6eyJ1cmwiOiJodHRwOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlLzg0N2I1Mjc5OTg0NjUxNTRhZDZjMjM4YTFlM2MyZGQzZTMyOTY1MzUyZTNhNjRmMzZlMTZhOTQwNWFiOCJ9fX0=";
const TAB_SKIN_SIGNATURE: &str = "u8sG8tlbmiekrfAdQjy4nXIcCfNdnUZzXSx9BE1X5K27NiUvE1dDNIeBBSPdZzQG1kHGijuokuHPdNi/KXHZkQM7OJ4aCu5JiUoOY28uz3wZhW4D+KG3dH4ei5ww2KwvjcqVL7LFKfr/ONU5Hvi7MIIty1eKpoGDYpWj3WjnbN4ye5Zo88I2ZEkP1wBw2eDDN4P3YEDYTumQndcbXFPuRRTntoGdZq3N5EBKfDZxlw4L3pgkcSLU5rWkd5UH4ZUOHAP/VaJ04mpFLsFXzzdU4xNZ5fthCwxwVBNLtHRWO26k/qcVBzvEXtKGFJmxfLGCzXScET/OjUBak/JEkkRG2m+kpmBMgFRNtjyZgQ1w08U6HHnLTiAiio3JswPlW5v56pGWRHQT5XWSkfnrXDalxtSmPnB5LmacpIImKgL8V9wLnWvBzI7SHjlyQbbgd+kUOkLlu7+717ySDEJwsFJekfuR6N/rpcYgNZYrxDwe4w57uDPlwNL6cJPfNUHV7WEbIU1pMgxsxaXe8WSvV87qLsR7H06xocl2C0JFfe2jZR4Zh3k9xzEnfCeFKBgGb4lrOWBu1eDWYgtKV67M2Y+B3W5pjuAjwAxn0waODtEn/3jKPbc/sxbPvljUCw65X+ok0UUN1eOwXV5l2EGzn05t3Yhwq19/GxARg63ISGE8CKw=";

static STATE: Mutex<Option<TabState>> = Mutex::new(None);
static DIRTY: AtomicBool = AtomicBool::new(false);

fn default_skin() -> Skin {
    Skin {
        value: TAB_SKIN_VALUE.to_string(),
        signature: Some(TAB_SKIN_SIGNATURE.to_string()),
    }
}

/// Full rebuild for one player (called on join and every tick).
pub fn setup(player: &Player, server: &Server) {
    // Header/footer (static, from `tab.yml`).
    {
        let guard = state().read().unwrap();
        let header = guard.raw.tab.string_list("TAB_HEADER").join("\n");
        let footer = guard.raw.tab.string_list("TAB_FOOTER").join("\n");
        if !header.is_empty() || !footer.is_empty() {
            player.set_tab_list_header_footer(crate::utils::text(header), crate::utils::text(footer));
        }
    }

    rebuild(player, server);
}

/// Per-player dynamic updates (called every tick).
pub fn update(player: &Player, server: &Server) {
    rebuild(player, server);
}

fn rebuild(player: &Player, server: &Server) {
    let uuid = crate::utils::core::uuid_of(player);

    let lines = {
        let guard = state().read().unwrap();
        if !guard.raw.tab.bool("TAB_ENABLED") {
            return;
        }

        let mut lines = compute_lines(&guard, player, server);
        apply_modules(&guard, player, server, &mut lines);
        lines
    };

    let mut state = STATE.lock().unwrap();
    let tab = state.get_or_insert_with(TabState::default);
    let viewer = tab.viewers.entry(uuid).or_default();
    viewer.clear();

    let skin = default_skin();
    for (slot, line) in lines.iter().enumerate() {
        viewer.insert(
            slot.to_string(),
            SlotSpec {
                text: line.clone(),
                name: None,
                order: Some((SLOTS as i32 - 1) - slot as i32),
                skin: Some(skin.clone()),
            },
        );
    }

    DIRTY.store(true, Ordering::Relaxed);
}

/// Writes the state file once per tick if it changed.
pub fn flush() {
    if !DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }

    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else {
        return;
    };
    let folder = state_folder();
    crate::tab::api::write(&folder, state);
}

/// Removes a player's personal tab state (on quit).
pub fn forget(uuid: &str) {
    let mut state = STATE.lock().unwrap();
    if let Some(tab) = state.as_mut() {
        tab.viewers.remove(uuid);
        DIRTY.store(true, Ordering::Relaxed);
    }
}

fn state_folder() -> String {
    crate::state::state().read().unwrap().data_folder.clone()
}

// ---------------------------------------------------------------------------
// Column lines (TabUpdaterImpl.loadInitialSet / loadUpdates)
// ---------------------------------------------------------------------------

/// Builds the 80 configured lines (4 columns of 20), resolving placeholders.
fn compute_lines(guard: &AppState, player: &Player, server: &Server) -> Vec<String> {
    let tab = &guard.raw.tab;

    let mut flat: Vec<String> = Vec::with_capacity(SLOTS);
    for column in ["LEFT", "CENTER", "RIGHT", "FAR_RIGHT"] {
        let mut lines = tab.string_list(column);
        lines.resize(COLUMN_HEIGHT, String::new());
        flat.extend(lines);
    }

    let mut out: Vec<String> = flat
        .iter()
        .map(|line| resolve(line, guard, player, server))
        .collect();
    out.resize(SLOTS, String::new());
    out
}

/// Mirrors `TabUpdaterImpl.getFunction` / the `<location>` special case.
fn resolve(line: &str, guard: &AppState, player: &Player, server: &Server) -> String {
    let uuid = crate::utils::core::uuid_of(player);

    if line.contains("<location>") {
        let location = crate::utils::core::location(player);
        let direction = FACES[((location.yaw / 45.0).round() as i32 & 0x7) as usize];
        return format!(
            "{}({}, {}) [{}]",
            line.replace("<location>", ""),
            location.block_x(),
            location.block_z(),
            direction
        );
    }

    let (kills, deaths, lives, balance) = guard
        .userdata
        .get(&uuid)
        .map(|u| {
            (
                u.kills.to_string(),
                u.deaths.to_string(),
                u.lives.to_string(),
                u.balance.to_string(),
            )
        })
        .unwrap_or_else(|| ("0".into(), "0".into(), "0".into(), "0".into()));

    let territory = {
        let location = crate::utils::core::location(player);
        let territory = crate::utils::core::territory_at_guard(
            guard,
            &location.world,
            location.block_x(),
            location.block_z(),
        );
        let viewer = guard.factions.by_member(&uuid).map(|f| f.id.clone());
        crate::utils::core::territory_display_guard(guard, &territory, viewer.as_deref())
    };

    let online = if guard.raw.config.bool("LUNAR_CLIENT_API_ENABLED") {
        server.get_all_players().len()
    } else {
        server.get_all_players().len()
    };

    let mut out = line
        .replace("<kills>", &kills)
        .replace("<deaths>", &deaths)
        .replace("<lives>", &lives)
        .replace("<balance>", &balance)
        .replace("<faction>", &territory)
        .replace(
            "<online>",
            &format!("{online}/{}", server.get_max_players()),
        );
    out = out.replace("<player>", &crate::utils::core::name_of(player));
    out
}

// ---------------------------------------------------------------------------
// tab.yml modules
// ---------------------------------------------------------------------------

fn apply_modules(guard: &AppState, player: &Player, server: &Server, lines: &mut [String]) {
    let tab = &guard.raw.tab;
    let uuid = crate::utils::core::uuid_of(player);
    let faction = guard.factions.by_member(&uuid);

    apply_faction_info(guard, tab, faction, lines);
    apply_player_list(guard, tab, faction, server, lines);
    apply_next_koth(guard, tab, lines);
}

fn apply_faction_info(
    guard: &AppState,
    tab: &crate::config::yaml::ConfigFile,
    faction: Option<&Faction>,
    lines: &mut [String],
) {
    if !tab.bool("FACTION_INFO_MODULE.ENABLED") {
        return;
    }

    let start = slot_index(tab.int("FACTION_INFO_MODULE.START_SLOT"));

    match faction {
        None => {
            let no_faction = tab.string_list("FACTION_INFO_MODULE.NO_FACTION");
            set(lines, start, no_faction.first().cloned().unwrap_or_default());
            set(lines, start + 1, no_faction.get(1).cloned().unwrap_or_default());
        }
        Some(faction) => {
            let in_faction = tab.string_list("FACTION_INFO_MODULE.IN_FACTION");
            let first = in_faction
                .first()
                .cloned()
                .unwrap_or_default()
                .replace("<dtr>", "")
                .replace("<home>", "");
            let second = in_faction
                .get(1)
                .cloned()
                .unwrap_or_default()
                .replace("<dtr>", "")
                .replace("<home>", "");

            let frozen = guard.timers.is_frozen(&faction.id);
            set(
                lines,
                start,
                format!(
                    "{}{}&7/{}",
                    first,
                    faction.dtr_string(&guard.config, frozen),
                    faction.max_dtr_string(&guard.config),
                ),
            );
            set(lines, start + 1, format!("{second}{}", faction.home_string()));
        }
    }
}

fn apply_player_list(
    _guard: &AppState,
    tab: &crate::config::yaml::ConfigFile,
    faction: Option<&Faction>,
    server: &Server,
    lines: &mut [String],
) {
    if !tab.bool("PLAYER_LIST_MODULE.ENABLED") {
        return;
    }

    let start = slot_index(tab.int("PLAYER_LIST_MODULE.START_SLOT"));
    let count = tab.int("PLAYER_LIST_MODULE.NUMBER_OF_PLAYERS").max(0) as usize;
    let faction_color = tab.string("PLAYER_LIST_MODULE.FACTION_NAME_COLOR");
    let player_color = tab.string("PLAYER_LIST_MODULE.PLAYER_COLOR");

    // clearFactionPlayerList
    for i in start..=(start + count + 1) {
        set(lines, i, String::new());
    }

    let Some(faction) = faction else {
        return;
    };

    set(lines, start, format!("{faction_color}{}", faction.name));

    let mut online: Vec<&FactionMember> = faction
        .members
        .values()
        .filter(|member| is_online(server, &member.uuid))
        .collect();
    // Same comparator as PlayerListModule: `m2.role.ordinal() - m1.role.ordinal()`.
    online.sort_by(|a, b| b.role.cmp(&a.role));

    for (i, member) in online.iter().take(count).enumerate() {
        set(
            lines,
            start + 1 + i,
            format!("{player_color}{}{}", member.role.prefix(), member.name),
        );
    }
}

fn apply_next_koth(guard: &AppState, tab: &crate::config::yaml::ConfigFile, lines: &mut [String]) {
    let _ = guard;
    if !tab.bool("NEXT_KOTH_MODULE.ENABLED") {
        return;
    }

    // No KOTH/schedule manager yet: always the "none scheduled" state.
    let start = slot_index(tab.int("NEXT_KOTH_MODULE.START_SLOT"));
    let none = tab.string_list("NEXT_KOTH_MODULE.NONE_SCHEDULED");
    for (i, line) in none.iter().enumerate() {
        set(lines, start + i, line.clone());
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `tab.yml` slots are 1-based; our slots are 0-based.
fn slot_index(start_slot: i64) -> usize {
    (start_slot - 1).max(0) as usize
}

fn set(lines: &mut [String], index: usize, value: String) {
    if let Some(slot) = lines.get_mut(index) {
        *slot = value;
    }
}

fn is_online(server: &Server, uuid: &str) -> bool {
    pumpkin_plugin_api::uuid::parse(uuid)
        .and_then(|id| server.get_player_by_uuid(id))
        .is_some()
}

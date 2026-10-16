//! Tab list driver for the WASM plugin.
//!
//! The artificial tab is rendered by the native `native_tab` companion (the WASM
//! host cannot send `CPlayerInfoUpdate`). We build the desired state here and write
//! it to `tab_state.json`; the native side diffs it and only updates changed slots.
//!
//! * `setup`  — fills the 80 configured slots (from `tab.toml`).
//! * `update` — sets a per-player dynamic line (live coordinates).
//! * `flush`  — writes the file once per tick if anything changed.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use pumpkin_plugin_api::{Player, Server};

use crate::state::{state, AppState};
use crate::tab_api::{SlotSpec, TabState};
use crate::util;

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;

/// Slot used for the per-player dynamic line (empty cell of the CENTER column).
const DYNAMIC_SLOT: u8 = 23;

const FACES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];

static STATE: Mutex<Option<TabState>> = Mutex::new(None);
static DIRTY: AtomicBool = AtomicBool::new(false);

/// Pushes the whole artificial layout (called on join / reload).
pub fn setup(player: &Player, server: &Server) {
    let lines = {
        let guard = state().read().unwrap();
        if !guard.config.tab.enabled {
            return;
        }
        compute_lines(&guard, player, server)
    };

    let mut state = STATE.lock().unwrap();
    let tab = state.get_or_insert_with(TabState::default);

    let mut global = BTreeMap::new();
    for (slot, line) in lines.iter().enumerate() {
        global.insert(
            slot.to_string(),
            SlotSpec {
                text: line.clone(),
                name: None,
                order: Some(slot as i32),
                skin: None,
            },
        );
    }
    tab.global = global;

    DIRTY.store(true, Ordering::Relaxed);
}

/// Per-player dynamic updates (called every tick).
pub fn update(player: &Player, _server: &Server) {
    let uuid = crate::core::uuid_of(player);
    let location = crate::core::location(player);
    let direction = FACES[((location.yaw / 45.0).round() as i32 & 0x7) as usize];
    let text = format!(
        "&7Coords: &f{}, {} &7[{direction}]",
        location.block_x(),
        location.block_z()
    );

    let mut state = STATE.lock().unwrap();
    let tab = state.get_or_insert_with(TabState::default);
    tab.viewers
        .entry(uuid)
        .or_default()
        .insert(DYNAMIC_SLOT.to_string(), SlotSpec::text(text));

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
    crate::tab_api::write(&folder, state);
}

/// No per-player state to clean up beyond the file.
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

/// Builds the 80 configured lines (4 columns of 20), resolving placeholders.
fn compute_lines(guard: &AppState, player: &Player, server: &Server) -> Vec<String> {
    let tab = &guard.config.tab;

    let mut flat: Vec<String> = Vec::with_capacity(SLOTS);
    for column in [&tab.left, &tab.center, &tab.right, &tab.far_right] {
        let mut padded = column.clone();
        padded.resize(COLUMN_HEIGHT, String::new());
        flat.extend(padded);
    }

    flat.iter().map(|line| resolve(line, guard, player, server)).collect()
}

fn resolve(line: &str, guard: &AppState, player: &Player, server: &Server) -> String {
    let uuid = crate::core::uuid_of(player);

    let (kills, deaths, lives) = guard
        .userdata
        .get(&uuid)
        .map(|u| (u.kills.to_string(), u.deaths.to_string(), u.lives.to_string()))
        .unwrap_or_else(|| ("0".into(), "0".into(), "0".into()));

    let location = crate::core::location(player);
    let territory = {
        let territory = crate::core::territory_at(&location.world, location.block_x(), location.block_z());
        let viewer = crate::core::faction_id_of(&uuid);
        crate::core::territory_display(&territory, viewer.as_deref())
    };
    let direction = FACES[((location.yaw / 45.0).round() as i32 & 0x7) as usize];
    let location_str = format!("({}, {}) [{direction}]", location.block_x(), location.block_z());

    util::replace_all(
        line,
        &[
            ("<online>", &server.get_all_players().len().to_string()),
            ("<max>", &server.get_max_players().to_string()),
            ("<player>", &crate::core::name_of(player)),
            ("<kills>", &kills),
            ("<deaths>", &deaths),
            ("<lives>", &lives),
            ("<faction>", &territory),
            ("<territory>", &territory),
            ("<location>", &location_str),
            ("<direction>", direction),
        ],
    )
}

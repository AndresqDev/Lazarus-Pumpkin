//! Tab list driver for the WASM plugin.
//!
//! The artificial tab list is rendered by the native `native_tab` companion (the
//! WASM host cannot send `CPlayerInfoUpdate`). Here we push the 80 slots from
//! `tab.toml` and update per-player dynamic lines (e.g. live coordinates).
//!
//! The helpers live in [`crate::tab_api`]. If the native plugin is not installed
//! the IPC calls simply fail and nothing happens.

use pumpkin_plugin_api::{Player, Server};

use crate::state::{state, AppState};
use crate::tab_api;
use crate::util;

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;

/// Slot used to show a per-player dynamic line (live coordinates). It lives in an
/// empty cell of the CENTER column so it doesn't overwrite a configured line.
const DYNAMIC_SLOT: u8 = 23;

const FACES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];

/// Pushes the whole artificial layout (called on join / reload).
pub fn setup(player: &Player, server: &Server) {
    let lines = {
        let guard = state().read().unwrap();
        if !guard.config.tab.enabled {
            return;
        }
        compute_lines(&guard, player, server)
    };

    for (slot, line) in lines.iter().enumerate() {
        tab_api::set_slot(slot as u8, line, None, Some(slot as i32));
    }
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
    tab_api::set_viewer_text(&uuid, DYNAMIC_SLOT, &text);
}

/// No per-player state to clean up (the native plugin owns the slots).
pub fn forget(_uuid: &str) {}

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

//! Event-driven invalidation for the per-player UI surfaces.
//!
//! Claim and timer mutations happen inside gameplay handlers, often while the
//! global [`crate::state`] lock is held. They only mark work here; a 1-tick
//! task drains the queues and calls the tab/scoreboard renderers outside that
//! lock. This makes a claim edge or a newly-started timer visible on the next
//! tick instead of waiting for the 1 Hz tab rebuild or the 5 Hz sidebar pass,
//! without running either renderer for every player on every tick.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use pumpkin_plugin_api::Server;

use crate::scoreboard;
use crate::tab;
use crate::utils::core;

#[derive(Default)]
struct Dirty {
    tab_all: bool,
    scoreboard_all: bool,
    tab_players: BTreeSet<String>,
    scoreboard_players: BTreeSet<String>,
}

static DIRTY: OnceLock<Mutex<Dirty>> = OnceLock::new();

fn dirty() -> &'static Mutex<Dirty> {
    DIRTY.get_or_init(|| Mutex::new(Dirty::default()))
}

/// Requests a tab refresh for one player.
pub fn tab_player(uuid: &str) {
    dirty().lock().unwrap().tab_players.insert(uuid.to_string());
}

/// Requests a tab refresh for every online player.
pub fn tab_all() {
    dirty().lock().unwrap().tab_all = true;
}

/// Requests a sidebar refresh for one player.
pub fn scoreboard_player(uuid: &str) {
    dirty()
        .lock()
        .unwrap()
        .scoreboard_players
        .insert(uuid.to_string());
}

/// Requests a sidebar refresh for every online player.
pub fn scoreboard_all() {
    dirty().lock().unwrap().scoreboard_all = true;
}

/// A claim was added or removed: both surfaces show territory/faction data.
pub fn claim_changed() {
    let mut dirty = dirty().lock().unwrap();
    dirty.tab_all = true;
    dirty.scoreboard_all = true;
}

/// Drains the queues and renders the affected players. Scheduled once per tick
/// from the existing 1-tick task, so no renderer runs while a gameplay handler
/// still holds the global state lock.
pub fn tick(server: &Server) {
    let (tab_all, scoreboard_all, tab_players, scoreboard_players) = {
        let mut dirty = dirty().lock().unwrap();
        let snapshot = (
            dirty.tab_all,
            dirty.scoreboard_all,
            std::mem::take(&mut dirty.tab_players),
            std::mem::take(&mut dirty.scoreboard_players),
        );
        // Reset in the same critical section as the snapshot: a mutation that
        // races this call either is in the snapshot or sets the flag again for
        // the next tick. Both renderers are idempotent.
        dirty.tab_all = false;
        dirty.scoreboard_all = false;
        snapshot
    };

    let needs_tab = tab_all || !tab_players.is_empty();
    let needs_scoreboard = scoreboard_all || !scoreboard_players.is_empty();
    if !needs_tab && !needs_scoreboard {
        return;
    }

    let players = server.get_all_players();

    if needs_scoreboard {
        if scoreboard_all {
            for player in &players {
                scoreboard::update(player, server);
            }
        } else {
            for player in &players {
                if scoreboard_players.contains(&core::uuid_of(player)) {
                    scoreboard::update(player, server);
                }
            }
        }
    }

    if needs_tab {
        if tab_all {
            for player in &players {
                tab::update(player, server);
            }
        } else {
            for player in &players {
                if tab_players.contains(&core::uuid_of(player)) {
                    tab::update(player, server);
                }
            }
        }
        // The native companion polls the file; flush now instead of waiting
        // for the 1 Hz tick so the immediate update is actually visible.
        tab::flush();
    }
}

//! Holograms — port of `me.qiooip.lazarus.hologram`.
//!
//! The original built floating armor stands through NMS packets and tracked a
//! viewer set per hologram with a render task. Modern Minecraft has dedicated
//! `TextDisplay` entities, which Pumpkin can spawn and customise, so the port
//! stacks one real `TextDisplay` per line at `y - 0.25 * index` (the original's
//! `Hologram#getLineLocation`) and lets the host's entity tracker handle the
//! view distance — no per-player packet bookkeeping.
//!
//! Holograms are identified by a numeric id (`1..=len`), like the original: a
//! new hologram takes `holograms.size() + 1` and deleting one shifts every
//! higher id down by one (`Hologram#decrementId`). Layouts:
//!
//! * `normal`: the stored `lines` (`EMPTY` from the command means an empty
//!   line).
//! * `leaderboard`: header + ten slots + footer rendered by
//!   [`crate::handlers::leaderboard::hologram_lines`]; the slot count is fixed,
//!   so board updates rewrite the existing entities' text in place.
//!
//! The display entities are spawned directly: `World#spawn_entity` is a plain
//! blocking pump as long as the plugin registers no `EntitySpawnEvent`
//! handler. A registered handler makes the host re-enter the guest in the
//! middle of that pump, and the host's `sender_for`/`register` race then
//! deadlocks the call (measured on ~5–15% of the pumps — see PORTING.md), so
//! the port bans that handler globally (see [`crate::listeners`]) and can
//! build a hologram line by line inside one root call.
//!
//! [`ensure_respawned`] runs from the scheduled tick and rebuilds holograms
//! whose entities are missing (they are never persisted, so every server start
//! rebuilds them) or incomplete (the hologram's dimension was not live yet at
//! spawn time).
//!
//! Persistence: `<dataFolder>/holograms/holograms.json` (the original
//! `HOLOGRAMS_DIR`), written by the periodic full save.

use serde::{Deserialize, Serialize};

use pumpkin_plugin_api::display::BillboardMode;
use pumpkin_plugin_api::{Entity, EntityDisplayExt, EntityType, Server, World};

use crate::handlers::leaderboard::Board;
use crate::state::AppState;
use crate::utils;

/// A persisted hologram (`me.qiooip.lazarus.hologram.Hologram`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hologram {
    pub id: i32,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    #[serde(flatten)]
    pub kind: HologramKind,
}

/// `StaticHologram` and `LeaderboardHologram` in one enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum HologramKind {
    /// `StaticHologram` — the lines the staff edited.
    Normal { lines: Vec<String> },
    /// `LeaderboardHologram` — rendered from the shared leaderboard cache.
    Leaderboard { board: Board },
}

impl Hologram {
    /// `StaticHologram` default text: `Hologram <id> (edit me)`.
    pub fn new_normal(id: i32, world: String, x: f64, y: f64, z: f64) -> Self {
        Self {
            id,
            world,
            x,
            y,
            z,
            kind: HologramKind::Normal {
                lines: vec![format!("Hologram {id} (edit me)")],
            },
        }
    }

    pub fn new_leaderboard(id: i32, world: String, x: f64, y: f64, z: f64, board: Board) -> Self {
        Self {
            id,
            world,
            x,
            y,
            z,
            kind: HologramKind::Leaderboard { board },
        }
    }

    /// `Hologram#getLineLocation`: the first line sits 0.25 blocks below the
    /// stored location, each next one another 0.25 below.
    pub fn line_y(&self, index: usize) -> f64 {
        self.y - (index as f64 * 0.25)
    }

    /// Number of rendered lines (`Hologram#createHologramLines().size()`)
    /// without materialising the text: the known layout lets
    /// [`ensure_respawned`] check the spawned entities cheaply.
    pub fn line_count(&self, state: &AppState) -> usize {
        match &self.kind {
            HologramKind::Normal { lines } => lines.len(),
            HologramKind::Leaderboard { board } => {
                crate::handlers::leaderboard::hologram_line_count(state, *board)
            }
        }
    }
}

/// The rendered lines of a hologram (`Hologram#createHologramLines`).
pub fn lines_of(state: &AppState, hologram: &Hologram) -> Vec<String> {
    match &hologram.kind {
        HologramKind::Normal { lines } => lines.clone(),
        HologramKind::Leaderboard { board } => {
            crate::handlers::leaderboard::hologram_lines(state, *board)
        }
    }
}

/// Spawns the display entity for one line.
fn spawn_line(world: &World, hologram: &Hologram, index: usize, line: &str) -> Entity {
    let entity = world.spawn_entity(
        EntityType::TextDisplay,
        (hologram.x, hologram.line_y(index), hologram.z),
    );

    if let Some(display) = entity.as_text_display() {
        display.set_text(utils::text(line));
        display.get_display().set_billboard(BillboardMode::Center);
        display.set_see_through(false);
    }

    entity
}

/// Removes the display entities of a hologram (`Hologram#removeHologram`).
pub fn despawn(entities: Vec<Entity>) {
    for entity in entities {
        entity.remove();
    }
}

/// Recreates the entities of one hologram in place (used after line edits,
/// teleports and creation; the original resends the whole hologram to viewers
/// on each of those operations).
///
/// The whole hologram is rebuilt inside this call: spawning is a plain
/// blocking pump while no `EntitySpawnEvent` handler exists (see the module
/// docs).
pub fn respawn(server: &Server, id: i32) {
    let (hologram, world, old) = {
        let mut guard = crate::state::state().write().unwrap();
        let Some(hologram) = guard
            .holograms
            .iter()
            .find(|hologram| hologram.id == id)
            .cloned()
        else {
            return;
        };
        let Some(world) = server.get_world_by_name(&hologram.world) else {
            // Dimensions are not all live during early startup: leave the
            // hologram untouched, `ensure_respawned` retries it.
            return;
        };
        let old = guard.hologram_entities.insert(id, Vec::new());
        (hologram, world, old)
    };

    despawn(old.unwrap_or_default());

    let lines = {
        let guard = crate::state::state().read().unwrap();
        lines_of(&guard, &hologram)
    };

    let spawned: Vec<Entity> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| spawn_line(&world, &hologram, index, line))
        .collect();

    // Spawning cannot be interrupted (no handler re-enters the guest), so the
    // handles are registered directly.
    crate::state::state()
        .write()
        .unwrap()
        .hologram_entities
        .insert(id, spawned);
}

/// Rebuilds every hologram whose display entities are missing or incomplete
/// (called from the scheduled tick).
///
/// The entity handles are never persisted, so after a server start every
/// hologram is rebuilt here; the same path retries a hologram whose dimension
/// was not live yet when [`respawn`] ran.
pub fn ensure_respawned(server: &Server) {
    let pending: Vec<i32> = {
        let guard = crate::state::state().read().unwrap();
        guard
            .holograms
            .iter()
            .filter(|hologram| {
                guard
                    .hologram_entities
                    .get(&hologram.id)
                    .map_or(0, Vec::len)
                    < hologram.line_count(&guard)
            })
            .map(|hologram| hologram.id)
            .collect()
    };

    for id in pending {
        respawn(server, id);
    }
}

/// `HologramManager#removeHologramById` + the `decrementId` loop: drops the
/// hologram, its entities and shifts every higher id (and its entity list) down.
pub fn delete(state: &mut AppState, id: i32) {
    if let Some(entities) = state.hologram_entities.remove(&id) {
        despawn(entities);
    }
    state.holograms.retain(|hologram| hologram.id != id);

    let mut shifted: Vec<i32> = state
        .holograms
        .iter()
        .map(|hologram| hologram.id)
        .filter(|hologram_id| *hologram_id > id)
        .collect();
    shifted.sort_unstable();

    for old_id in shifted {
        for hologram in state.holograms.iter_mut() {
            if hologram.id == old_id {
                hologram.id -= 1;
                break;
            }
        }
        if let Some(entities) = state.hologram_entities.remove(&old_id) {
            state.hologram_entities.insert(old_id - 1, entities);
        }
    }
}

/// `HologramManager#updateLeaderboardHologram`: rewrites the text of every
/// leaderboard hologram showing `board`. The line count never changes, so the
/// entities (and their positions) are reused.
pub fn refresh_board_holograms(state: &mut AppState, board: Board) {
    let lines = crate::handlers::leaderboard::hologram_lines(state, board);

    let AppState {
        holograms,
        hologram_entities,
        ..
    } = state;

    for hologram in holograms.iter() {
        let HologramKind::Leaderboard {
            board: hologram_board,
        } = &hologram.kind
        else {
            continue;
        };
        if *hologram_board != board {
            continue;
        }

        let Some(entities) = hologram_entities.get(&hologram.id) else {
            continue;
        };

        for (index, entity) in entities.iter().enumerate() {
            if let Some(line) = lines.get(index)
                && let Some(display) = entity.as_text_display()
            {
                display.set_text(utils::text(line));
            }
        }
    }
}

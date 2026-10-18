//! `EndPortalHandler` port — the `/endportal` selection check.
//!
//! [`check`] runs for every wand click on a `SELECTION`-kind selection
//! (mirroring `SelectionManager#onPlayerInteract`): the clicked block must be
//! an end portal frame, both positions must share the same Y and be 2–4 blocks
//! apart on X and Z — and then the selected rectangle is filled with end portal
//! blocks (frames are kept). The selection process ends with the successful
//! check, exactly like `removeSelectionProcess` in the original.

use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::{Player, Server, World};

use crate::selection::{SelectedPos, Selection};
use crate::state::state;
use crate::utils::core;

const END_PORTAL_FRAME: &str = "minecraft:ender_portal_frame";
const END_PORTAL: &str = "minecraft:end_portal";

/// Port of `EndPortalHandler#createSelectionCheck`. Returns `true` when the
/// position was accepted — the caller then sends `SELECTION.LOCATION_*_SET`.
pub fn check(server: &Server, player: &Player, uuid: &str, pos: BlockPos, left_click: bool) -> bool {
    let world_name = core::world_name(player);
    let Some(world) = server.get_world_by_name(&world_name) else {
        return false;
    };

    // `block.getType() != Material.ENDER_PORTAL_FRAME`.
    if world.get_block_state(pos).block_name != END_PORTAL_FRAME {
        message(player, "ENDPORTAL.NOT_END_PORTAL_FRAME");
        return false;
    }

    // `SelectionManager#onPlayerInteract` clones the selection *before* applying
    // the position and puts the clone back when the check fails.
    let before = {
        let guard = state().read().unwrap();
        guard.selection.get(uuid).cloned().unwrap_or_default()
    };

    let new_pos: SelectedPos = (world_name, pos.x, pos.y, pos.z);
    let selection = {
        let mut guard = state().write().unwrap();
        guard.selection.set_position(uuid, new_pos, left_click);
        guard.selection.get(uuid).cloned().unwrap_or_default()
    };

    // `!selection.areBothPositionsSet()` — position recorded, check later.
    if !selection.both_set() {
        return true;
    }

    let (_, _, y1, _) = selection.pos_one.clone().unwrap();
    let (_, _, y2, _) = selection.pos_two.clone().unwrap();

    if y1 != y2 {
        message(player, "ENDPORTAL.WRONG_ELEVATION");
        restore(uuid, before);
        return false;
    }

    if !(2..=4).contains(&selection.size_x()) || !(2..=4).contains(&selection.size_z()) {
        message(player, "ENDPORTAL.SELECTION_TOO_BIG_OR_SMALL");
        restore(uuid, before);
        return false;
    }

    fill(&world, &selection);

    // `removeSelectionProcess`: drops the selection + wand. The `SELECTION` type
    // has no claim visuals to clear.
    state().write().unwrap().selection.cancel_local(player);

    true
}

/// Fills the selected rectangle at the Y of position one, keeping the frames.
fn fill(world: &World, selection: &Selection) {
    let (_, x1, y1, z1) = selection.pos_one.clone().unwrap();
    let (_, x2, _, z2) = selection.pos_two.clone().unwrap();

    for x in x1.min(x2)..=x1.max(x2) {
        for z in z1.min(z2)..=z1.max(z2) {
            let pos = BlockPos { x, y: y1, z };

            if world.get_block_state(pos).block_name != END_PORTAL_FRAME {
                let _ = world.set_block_by_name(pos, END_PORTAL, crate::handlers::block::air_flags());
            }
        }
    }
}

/// Puts the pre-click selection back (`selections.put(uuid, copy)`).
fn restore(uuid: &str, before: Selection) {
    state().write().unwrap().selection.restore(uuid, before);
}

/// Prefixed message from the raw `language.yml`.
fn message(player: &Player, key: &str) {
    let (prefix, message) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard.raw.language.string(key),
        )
    };

    core::send(player, format!("{prefix}{message}"));
}

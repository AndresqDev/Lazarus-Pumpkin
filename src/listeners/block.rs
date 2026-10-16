//! Block protection listeners — port of `factions.listeners.BlockEventListener`.
//!
//! Rules (`checkPlayerBuild`):
//! * wilderness and your own faction's claim are free to build in;
//! * a player faction's claim is free to build in when it is raidable (DTR <= 0);
//! * the warzone is buildable past `WARZONE_BREAK_AFTER` (never in the End);
//! * safezones deny everyone.

use pumpkin_plugin_api::events::{BlockBreakEvent, BlockPlaceEvent, EventData, EventHandler};
use pumpkin_plugin_api::{Player, Server};

use crate::core::{self, Territory};
use crate::state::state;
use crate::util;

fn can_build(player_uuid: &str, world: &str, x: i32, z: i32) -> bool {
    let territory = core::territory_at(world, x, z);
    match territory {
        Territory::Safezone => false,
        Territory::Warzone => {
            // Port of `playerCanBreakInWarzone`: protected within `break_after`,
            // never breakable in the End.
            let dimension = core::dimension_of(world);
            if dimension == core::Dimension::End {
                return false;
            }
            let guard = state().read().unwrap();
            let break_after = match dimension {
                core::Dimension::Overworld => guard.config.factions.warzone_break_after.overworld,
                core::Dimension::Nether => guard.config.factions.warzone_break_after.nether,
                core::Dimension::End => 0,
            };
            x.abs().max(z.abs()) > break_after
        }
        Territory::Faction(owner_id) => {
            let guard = state().read().unwrap();
            if guard.factions.by_member(player_uuid).map(|f| f.id.as_str()) == Some(owner_id.as_str()) {
                return true;
            }
            match guard.factions.get(&owner_id) {
                Some(faction) => faction.is_raidable(),
                None => true,
            }
        }
        _ => true,
    }
}

/// Port of the `message.replace("<faction>", factionAt.getDisplayName(player))`
/// line, using the original `language.yml` text.
fn deny_message(player: &Player, world: &str, x: i32, z: i32, interact: bool) -> String {
    let territory = core::territory_at(world, x, z);
    let viewer = core::faction_id_of(&core::uuid_of(player));
    let name = core::territory_display(&territory, viewer.as_deref());

    let key = if interact {
        "FACTIONS.PROTECTION_MESSAGES.DENY_INTERACT"
    } else {
        "FACTIONS.PROTECTION_MESSAGES.DENY_BUILD"
    };
    let template = state().read().unwrap().raw.language.string(key);
    util::replace(&template, "<faction>", &name)
}

pub struct BlockBreakListener;

impl EventHandler<BlockBreakEvent> for BlockBreakListener {
    fn handle(&self, server: Server, mut event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        let Some(player) = event.player.as_ref() else {
            return event;
        };

        let uuid = core::uuid_of(player);
        let world = core::world_name(player);

        // Always re-assert any client-side render at this position: the client
        // predicts the break/place and would otherwise erase our fake blocks.
        state().read().unwrap().glass.regenerate_at(
            &server,
            &world,
            event.block_pos.x,
            event.block_pos.y,
            event.block_pos.z,
        );

        // Holding the selection wand must never break the block (port of
        // `SelectionManager#onBlockBreak`).
        {
            let guard = state().read().unwrap();
            if guard.selection.is_active(&uuid) && guard.selection.holds_wand(player) {
                event.cancelled = true;
                return event;
            }
        }

        if !can_build(&uuid, &world, event.block_pos.x, event.block_pos.z) {
            event.cancelled = true;
            let message = deny_message(player, &world, event.block_pos.x, event.block_pos.z, false);
            core::send(player, &message);
        }

        event
    }
}

pub struct BlockPlaceListener;

impl EventHandler<BlockPlaceEvent> for BlockPlaceListener {
    fn handle(&self, server: Server, mut event: EventData<BlockPlaceEvent>) -> EventData<BlockPlaceEvent> {
        let player = &event.player;
        let uuid = core::uuid_of(player);
        let world = core::world_name(player);

        // Re-assert any client-side render at this position (see BlockBreakListener).
        state().read().unwrap().glass.regenerate_at(
            &server,
            &world,
            event.block_pos.x,
            event.block_pos.y,
            event.block_pos.z,
        );

        if !can_build(&uuid, &world, event.block_pos.x, event.block_pos.z) {
            event.cancelled = true;
            let message = deny_message(player, &world, event.block_pos.x, event.block_pos.z, false);
            core::send(player, &message);
        }

        event
    }
}

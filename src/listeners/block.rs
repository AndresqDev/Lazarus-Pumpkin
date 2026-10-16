//! Block protection listeners — port of `factions.listeners.BlockEventListener`.
//!
//! Rules:
//! * safezones are protected from everyone;
//! * a faction's own claim is protected for members;
//! * enemies may only build in a faction's claim when it is raidable (DTR <= 0);
//! * wilderness and warzone are free to build in.

use pumpkin_plugin_api::events::{BlockBreakEvent, BlockPlaceEvent, EventData, EventHandler};
use pumpkin_plugin_api::Server;

use crate::core::{self, Territory};
use crate::state::state;

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

pub struct BlockBreakListener;

impl EventHandler<BlockBreakEvent> for BlockBreakListener {
    fn handle(&self, server: Server, mut event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        let Some(player) = event.player.as_ref() else {
            return event;
        };

        let uuid = core::uuid_of(player);
        let world = core::world_name(player);
        let territory = core::territory_at(&world, event.block_pos.x, event.block_pos.z);
        let allowed = can_build(&uuid, &world, event.block_pos.x, event.block_pos.z);
        tracing::info!(
            "lazarus-break world={} x={} z={} dist={} territory={:?} can_build={}",
            world,
            event.block_pos.x,
            event.block_pos.z,
            event.block_pos.x.abs().max(event.block_pos.z.abs()),
            territory,
            allowed
        );
        if !allowed {
            event.cancelled = true;
            core::send(player, "&cYou cannot break blocks here.");
        } else {
            // Keep any active client-side view (claim pillars/walls) alive at this
            // position: real block changed, so re-send the fake block.
            state()
                .read()
                .unwrap()
                .glass
                .regenerate_at(&server, &world, event.block_pos.x, event.block_pos.y, event.block_pos.z);
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
        if !can_build(&uuid, &world, event.block_pos.x, event.block_pos.z) {
            event.cancelled = true;
            core::send(player, "&cYou cannot place blocks here.");
        } else {
            // Re-assert any client-side view at this position.
            state()
                .read()
                .unwrap()
                .glass
                .regenerate_at(&server, &world, event.block_pos.x, event.block_pos.y, event.block_pos.z);
        }

        event
    }
}

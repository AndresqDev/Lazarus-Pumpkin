//! Block protection listeners — port of `factions.listeners.BlockEventListener`.
//!
//! Rules (`checkPlayerBuild`):
//! * wilderness and your own faction's claim are free to build in;
//! * a player faction's claim is free to build in when it is raidable (DTR <= 0);
//! * the warzone is buildable past `WARZONE_BREAK_AFTER` (never in the End);
//! * safezones deny everyone.

use pumpkin_plugin_api::events::{BlockBreakEvent, BlockPlaceEvent, EventData, EventHandler};
use pumpkin_plugin_api::common::BlockPos;
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
        state().write().unwrap().glass.regenerate_at(
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
        state().write().unwrap().glass.regenerate_at(
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

// ---------------------------------------------------------------------------
// Grief protection (blocks/liquids). Port of the rest of `BlockEventListener`.
//
// NOTE: Pumpkin's `block-from-to` (fluid flow) / `block-explode` / `entity-explode`
// events carry no world, so per-claim checks are impossible there. We still stop
// the two main player-driven grief vectors:
//   * buckets (have a player -> we know the world),
//   * explosions (cancelled when `DISABLE_EXPLOSIONS_BLOCK_DAMAGE` is on).
// ---------------------------------------------------------------------------

pub struct BucketEmptyListener;

impl EventHandler<pumpkin_plugin_api::events::PlayerBucketEmptyEvent> for BucketEmptyListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::PlayerBucketEmptyEvent>,
    ) -> EventData<pumpkin_plugin_api::events::PlayerBucketEmptyEvent> {
        let player = &event.player;
        let uuid = core::uuid_of(player);
        let world = core::world_name(player);

        if !can_build(&uuid, &world, event.block_pos.x, event.block_pos.z) {
            event.cancelled = true;
            let message = deny_message(player, &world, event.block_pos.x, event.block_pos.z, true);
            core::send(player, &message);
        }

        event
    }
}

pub struct BucketFillListener;

impl EventHandler<pumpkin_plugin_api::events::PlayerBucketFillEvent> for BucketFillListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::PlayerBucketFillEvent>,
    ) -> EventData<pumpkin_plugin_api::events::PlayerBucketFillEvent> {
        let player = &event.player;
        let uuid = core::uuid_of(player);
        let world = core::world_name(player);

        if !can_build(&uuid, &world, event.block_pos.x, event.block_pos.z) {
            event.cancelled = true;
            let message = deny_message(player, &world, event.block_pos.x, event.block_pos.z, true);
            core::send(player, &message);
        }

        event
    }
}

/// Cancels explosion block damage when `DISABLE_EXPLOSIONS_BLOCK_DAMAGE` is on
/// (the original default). The event has no world, so this is the only possible
/// protection there.
pub struct EntityExplodeListener;

impl EventHandler<pumpkin_plugin_api::events::EntityExplodeEvent> for EntityExplodeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::EntityExplodeEvent>,
    ) -> EventData<pumpkin_plugin_api::events::EntityExplodeEvent> {
        if state().read().unwrap().raw.config.bool("DISABLE_EXPLOSIONS_BLOCK_DAMAGE") {
            event.cancelled = true;
        }
        event
    }
}

pub struct BlockExplodeListener;

impl EventHandler<pumpkin_plugin_api::events::BlockExplodeEvent> for BlockExplodeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::BlockExplodeEvent>,
    ) -> EventData<pumpkin_plugin_api::events::BlockExplodeEvent> {
        if state().read().unwrap().raw.config.bool("DISABLE_EXPLOSIONS_BLOCK_DAMAGE") {
            event.cancelled = true;
        }
        event
    }
}

/// Fluid (water/lava) flowing between territories — port of `onBlockFromTo`.
///
/// The event carries no world, so we infer it: the world whose block at `from_pos`
/// is the liquid that is flowing. If the source is wilderness/warzone and the
/// destination is a claim/system faction, the flow is cancelled (unless the config
/// explicitly allows flowing from wilderness into claims).
pub struct BlockFromToListner;

impl EventHandler<pumpkin_plugin_api::events::BlockFromToEvent> for BlockFromToListner {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<pumpkin_plugin_api::events::BlockFromToEvent>,
    ) -> EventData<pumpkin_plugin_api::events::BlockFromToEvent> {
        // Pumpkin fires this event with `from_pos == to_pos == destination` and no
        // world, so we use the destination and infer the source from a liquid
        // neighbour (and the world from that neighbour).
        let pos = event.to_pos;
        let neighbours = [
            (pos.x + 1, pos.y, pos.z),
            (pos.x - 1, pos.y, pos.z),
            (pos.x, pos.y, pos.z + 1),
            (pos.x, pos.y, pos.z - 1),
            (pos.x, pos.y + 1, pos.z),
            (pos.x, pos.y - 1, pos.z),
        ];

        let Some(world) = server.get_all_worlds().into_iter().find(|world| {
            neighbours.iter().any(|(x, y, z)| {
                world
                    .get_block_state(BlockPos { x: *x, y: *y, z: *z })
                    .is_liquid
            })
        }) else {
            return event;
        };

        let world_name = world.get_name();
        let blocked = {
            let guard = state().read().unwrap();
            let allow = guard
                .raw
                .config
                .bool("FACTION_CLAIM.LIQUID_FLOW_FROM_WILDERNESS_TO_CLAIMS");
            let is_claim = |t: &core::Territory| {
                matches!(t, core::Territory::Faction(_) | core::Territory::Safezone)
            };

            let destination = core::territory_at_guard(&guard, &world_name, pos.x, pos.z);

            let mut source = None;
            for (x, y, z) in neighbours {
                if world.get_block_state(BlockPos { x, y, z }).is_liquid {
                    source = Some(core::territory_at_guard(&guard, &world_name, x, z));
                    break;
                }
            }

            match source {
                Some(source) if source != destination => {
                    let enters = is_claim(&destination) && !is_claim(&source);
                    let leaves = is_claim(&source) && !is_claim(&destination);
                    let between = is_claim(&source) && is_claim(&destination);
                    (enters && !allow) || leaves || between
                }
                _ => false,
            }
        };

        if blocked {
            event.cancelled = true;
        }

        event
    }
}

/// Ignition (fire spread, lightning, …). The event has no cause field, so — as
/// requested — every ignition is cancelled.
pub struct BlockIgniteListener;

impl EventHandler<pumpkin_plugin_api::events::BlockIgniteEvent> for BlockIgniteListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::BlockIgniteEvent>,
    ) -> EventData<pumpkin_plugin_api::events::BlockIgniteEvent> {
        event.cancelled = true;
        event
    }
}

/// Pistons are disabled entirely (no lag machines): both extend and retract are
/// cancelled.
pub struct PistonExtendListener;

impl EventHandler<pumpkin_plugin_api::events::BlockPistonExtendEvent> for PistonExtendListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::BlockPistonExtendEvent>,
    ) -> EventData<pumpkin_plugin_api::events::BlockPistonExtendEvent> {
        event.cancelled = true;
        event
    }
}

pub struct PistonRetractListener;

impl EventHandler<pumpkin_plugin_api::events::BlockPistonRetractEvent> for PistonRetractListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<pumpkin_plugin_api::events::BlockPistonRetractEvent>,
    ) -> EventData<pumpkin_plugin_api::events::BlockPistonRetractEvent> {
        event.cancelled = true;
        event
    }
}

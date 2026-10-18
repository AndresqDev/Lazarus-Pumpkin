//! `PortalHandler` port — portal-travel destination rules.
//!
//! The original hooked `PlayerPortalEvent` twice: at LOWEST it rewrote the
//! destination of nether portals when `NETHER_PORTAL_POSITION_TRANSLATION` is
//! enabled, and at HIGHEST it sent end portals and safezone nether portals to
//! the configured spawn/exit entries of `utilities.yml`.
//!
//! Pumpkin's host never fires `PlayerPortalEvent`, but every portal trip goes
//! through `Player::teleport_world`, which fires [`PlayerChangeWorldEvent`]
//! *before* the move with `position`/`yaw`/`pitch` (and the world) read back
//! afterwards — so both rules live in this listener. The host does not report
//! the Bukkit `TeleportCause`: the portal kind is inferred from the block the
//! player is standing in, which is where the original's `event.getFrom()`
//! location sat. The host's own travel agent still runs first, so its vanilla
//! 1:8 destination search (and the nether-side portal / end platform it may
//! place there) happens before this listener moves the player elsewhere; that
//! is a documented deviation (the original cancelled the travel agent without
//! touching the destination blocks).

use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::events::{EventData, EventHandler, PlayerChangeWorldEvent};
use pumpkin_plugin_api::{Server, World};

use crate::state::state;
use crate::utils::core::{self, Dimension, Territory};

const NETHER_PORTAL: &str = "minecraft:nether_portal";
const END_PORTAL: &str = "minecraft:end_portal";

/// Bukkit's standing eye height (`Player#getEyeLocation`).
const EYE_HEIGHT: f64 = 1.62;

pub struct PortalChangeWorldListener;

impl EventHandler<PlayerChangeWorldEvent> for PortalChangeWorldListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<PlayerChangeWorldEvent>,
    ) -> EventData<PlayerChangeWorldEvent> {
        if event.cancelled {
            return event;
        }

        let player = &event.player;
        let from = core::dimension_of(&event.previous_world.get_dimension());
        let to = core::dimension_of(&event.new_world.get_dimension());

        // The player is still in the source world at this point.
        let (x, y, z) = player.get_position();
        let (block_x, block_y, block_z) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
        let world = player.get_world();
        let block_at = |y: i32| {
            world
                .get_block_state(BlockPos {
                    x: block_x,
                    y,
                    z: block_z,
                })
                .block_name
        };
        let feet = block_at(block_y);
        let eye = block_at((y + EYE_HEIGHT).floor() as i32);

        // End portals (`cause == END_PORTAL`): entering the end sends the player
        // to `END_SPAWN`, leaving it to `END_EXIT` (no safezone condition).
        if feet == END_PORTAL || eye == END_PORTAL {
            let key = match (from, to) {
                (Dimension::Overworld, Dimension::End) => "END_SPAWN",
                (Dimension::End, Dimension::Overworld) => "END_EXIT",
                _ => return event,
            };

            let (position, yaw, pitch) = destination(&server, key, &event.new_world);
            event.position = position;
            event.yaw = yaw;
            event.pitch = pitch;
            return event;
        }

        // Nether portals (`cause == NETHER_PORTAL`) in both directions.
        if feet != NETHER_PORTAL && eye != NETHER_PORTAL {
            return event;
        }

        if !matches!(
            (from, to),
            (Dimension::Overworld, Dimension::Nether) | (Dimension::Nether, Dimension::Overworld)
        ) {
            return event;
        }

        // HIGHEST rule: a portal inside the spawn safezone ignores the travel
        // agent and jumps to `NETHER_SPAWN` / `NETHER_EXIT`.
        if core::territory_at(&core::world_name(&player), block_x, block_z) == Territory::Safezone {
            let key = if from == Dimension::Overworld {
                "NETHER_SPAWN"
            } else {
                "NETHER_EXIT"
            };

            let (position, yaw, pitch) = destination(&server, key, &event.new_world);
            event.position = position;
            event.yaw = yaw;
            event.pitch = pitch;
            return event;
        }

        // LOWEST rule (`NETHER_PORTAL_POSITION_TRANSLATION`): the source X/Z are
        // scaled by the configured value, Y and the player's rotation are kept.
        let (enabled, value) = {
            let guard = state().read().unwrap();
            (
                guard
                    .raw
                    .config
                    .bool("NETHER_PORTAL_POSITION_TRANSLATION.ENABLED"),
                guard.raw.config.int("NETHER_PORTAL_POSITION_TRANSLATION.VALUE"),
            )
        };

        if !enabled || value <= 0 {
            return event;
        }

        let factor = value as f64;
        let (new_x, new_z) = if from == Dimension::Overworld {
            (x / factor, z / factor)
        } else {
            (x * factor, z * factor)
        };

        event.position = (new_x, y, new_z);
        event.yaw = player.get_yaw();
        event.pitch = player.get_pitch();
        event
    }
}

/// Resolves a `utilities.yml` spawn/exit entry with the original's fallback
/// (`toWorld.getSpawnLocation()`). The entry's world token is ignored: the host
/// already chose the transition's destination world and only the
/// position/rotation are editable through the event.
fn destination(server: &Server, key: &str, world: &World) -> ((f64, f64, f64), f32, f32) {
    if let Some((_world, position, yaw, pitch)) = core::configured_location(server, key) {
        return (position, yaw, pitch);
    }

    let spawn = world.get_spawn_location();
    (
        (spawn.pos.x as f64, spawn.pos.y as f64, spawn.pos.z as f64),
        spawn.yaw,
        spawn.pitch,
    )
}

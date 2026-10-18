//! `NetherPortalTrapHandler` port — a client-side fix for players trapped
//! inside a nether portal.
//!
//! Right-clicking a portal block while the feet *and* eye blocks are portal
//! blocks sends fake air for every portal block in the clicked block's 3×3×3
//! neighbourhood (`Player#sendBlockChange` → [`Player::send_block_change`]).
//! Nothing changes on the server; the client can simply walk/command out.

use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::events::{EventData, EventHandler, InteractAction, PlayerInteractEvent};
use pumpkin_plugin_api::world::Block;
use pumpkin_plugin_api::Server;

use crate::state::state;
use crate::utils::core;

const NETHER_PORTAL: &str = "minecraft:nether_portal";

/// Bukkit's standing eye height (`Player#getEyeLocation`).
const EYE_HEIGHT: f64 = 1.62;

pub struct NetherPortalTrapListener;

impl EventHandler<PlayerInteractEvent> for NetherPortalTrapListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerInteractEvent>,
    ) -> EventData<PlayerInteractEvent> {
        if event.cancelled || !matches!(event.action, InteractAction::RightClickBlock) {
            return event;
        }

        let Some(clicked) = event.clicked_pos.clone() else {
            return event;
        };

        let player = &event.player;
        let (x, y, z) = player.get_position();
        let world = player.get_world();
        let block_at = |y: i32| {
            world
                .get_block_state(BlockPos {
                    x: x.floor() as i32,
                    y,
                    z: z.floor() as i32,
                })
                .block_name
        };

        // `isInPortal`: both the feet and the eye blocks must be portal blocks.
        if block_at(y.floor() as i32) != NETHER_PORTAL
            || block_at((y + EYE_HEIGHT).floor() as i32) != NETHER_PORTAL
        {
            return event;
        }

        if world.get_block_state(clicked.clone()).block_name != NETHER_PORTAL {
            return event;
        }

        let air = Block::from_name("minecraft:air")
            .map(|block| block.get_default_state().id)
            .unwrap_or(0);
        let mut sent = false;

        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let pos = BlockPos {
                        x: clicked.x + dx,
                        y: clicked.y + dy,
                        z: clicked.z + dz,
                    };

                    if world.get_block_state(pos.clone()).block_name != NETHER_PORTAL {
                        continue;
                    }

                    sent = true;
                    player.send_block_change(pos, air);
                }
            }
        }

        if sent {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.raw.language.string("NETHER_PORTAL_TRAP_FIX"),
                )
            };

            core::send(player, format!("{prefix}{message}"));
        }

        event
    }
}

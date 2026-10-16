//! Entity listeners — port of `factions.listeners.EntityEventListener`.
//!
//! * mobs must not target players standing inside the safezone
//!   (`onEntityTarget`, random/closest-player targeting);
//! * arrows that land inside the safezone are removed (`onProjectileHit`);
//! * monsters must not spawn in the warzone / system factions unless
//!   `SPAWN_MOBS_IN_WARZONE` is enabled (`onCreatureSpawn`).

use pumpkin_plugin_api::events::{
    CreatureSpawnEvent, EntityTargetEvent, EventData, EventHandler, PotionSplashEvent,
};
use pumpkin_plugin_api::Server;

use crate::core;
use crate::state::state;

pub struct PotionSplashListener;

/// Port of the first half of `PlayerEventListener#onPotionSplash`: a potion that
/// lands inside the safezone is cancelled. The event carries no world, so it is
/// resolved from an affected player (potions only affect nearby entities).
///
/// The second half (zeroing the intensity for safezone players) cannot be ported:
/// the host WIT has no `set-intensity`.
impl EventHandler<PotionSplashEvent> for PotionSplashListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<PotionSplashEvent>,
    ) -> EventData<PotionSplashEvent> {
        let world = event
            .affected_entities
            .iter()
            .find_map(|id| core::player_by_entity(&server, *id))
            .map(|player| core::world_name(&player));

        let Some(world) = world else {
            return event;
        };

        let x = event.location.x;
        let z = event.location.z;
        if matches!(
            core::territory_at(&world, x, z),
            core::Territory::Safezone
        ) {
            event.cancelled = true;
        }

        event
    }
}

pub struct EntityTargetListener;

impl EventHandler<EntityTargetEvent> for EntityTargetListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<EntityTargetEvent>,
    ) -> EventData<EntityTargetEvent> {
        let Some(target) = event
            .target_id
            .and_then(|id| core::player_by_entity(&server, id))
        else {
            return event;
        };

        let location = core::location(&target);
        if matches!(
            core::territory_at(&location.world, location.block_x(), location.block_z()),
            core::Territory::Safezone
        ) {
            event.cancelled = true;
        }

        event
    }
}

/// Hostile mobs that must not spawn inside protected territories.
///
/// The host only exposes the entity type as a registry key string, so we keep an
/// explicit list of the 1.8-era monsters (the original tested `instanceof Monster`).
const MONSTERS: &[&str] = &[
    "zombie", "husk", "drowned", "skeleton", "stray", "bogged", "wither_skeleton",
    "creeper", "spider", "cave_spider", "enderman", "witch", "slime", "magma_cube",
    "blaze", "ghast", "piglin", "piglin_brute", "hoglin", "zoglin", "zombified_piglin",
    "guardian", "elder_guardian", "shulker", "silverfish", "endermite", "phantom",
    "vex", "vindicator", "pillager", "evoker", "ravager", "illusioner", "warden",
    "wither", "ender_dragon", "zombie_villager",
];

pub struct CreatureSpawnListener;

impl EventHandler<CreatureSpawnEvent> for CreatureSpawnListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<CreatureSpawnEvent>,
    ) -> EventData<CreatureSpawnEvent> {
        if !MONSTERS.contains(&event.entity_type.as_str()) {
            return event;
        }

        let world = event.target_world.get_name();
        let spawn_mobs_in_warzone = state().read().unwrap().config.spawn_mobs_in_warzone;

        if spawn_mobs_in_warzone {
            return event;
        }

        // Never in the End.
        if core::dimension_of(&world) == core::Dimension::End {
            return event;
        }

        // Spawners and custom reasons bypass the check.
        let reason = event.spawn_reason.to_ascii_lowercase();
        if reason.contains("spawner") || reason.contains("custom") {
            return event;
        }

        // Only warzone / system factions are protected from natural spawns.
        let x = event.position.0.floor() as i32;
        let z = event.position.2.floor() as i32;
        if matches!(
            core::territory_at(&world, x, z),
            core::Territory::Warzone | core::Territory::Safezone
        ) {
            event.cancelled = true;
        }

        event
    }
}

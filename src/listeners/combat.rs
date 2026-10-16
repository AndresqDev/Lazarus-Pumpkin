//! Combat listeners: friendly-fire prevention, combat tagging and PvP protection.
//!
//! Port of `handlers.timer.CombatTagHandler` / `PvpProtHandler` and the friendly
//! fire checks from the faction entity listener.

use pumpkin_plugin_api::events::{
    EntityDamageByEntityEvent, EntityDamageEvent, EventData, EventHandler, FoodLevelChangeEvent,
    ProjectileHitEvent, ProjectileLaunchEvent,
};
use pumpkin_plugin_api::{Player, Server};

use crate::core;
use crate::factions::Relation;
use crate::state::state;
use crate::timers::TimerType;
use crate::util;

pub struct EntityDamageByEntityListener;

impl EventHandler<EntityDamageByEntityEvent> for EntityDamageByEntityListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<EntityDamageByEntityEvent>,
    ) -> EventData<EntityDamageByEntityEvent> {
        let victim = core::player_by_entity(&server, event.entity_id);
        // Projectiles (arrows, snowballs, …) have their own entity id, so resolve
        // the shooter we tracked on launch.
        let damager = core::player_by_entity(&server, event.damager_id).or_else(|| {
            take_projectile_shooter(event.damager_id).and_then(|uuid| {
                pumpkin_plugin_api::uuid::parse(&uuid)
                    .and_then(|id| server.get_player_by_uuid(id))
            })
        });

        let (Some(victim), Some(damager)) = (victim, damager) else {
            return event;
        };

        let victim_uuid = core::uuid_of(&victim);
        let damager_uuid = core::uuid_of(&damager);

        let (relation, victim_protected, victim_combat, damager_combat, tag_seconds, allow_own) = {
            let guard = state().read().unwrap();
            let relation = match (
                guard.factions.by_member(&victim_uuid),
                guard.factions.by_member(&damager_uuid),
            ) {
                (Some(victim_faction), Some(damager_faction)) => victim_faction.relation_to(damager_faction),
                _ => Relation::Enemy,
            };

            (
                relation,
                guard.timers.is_active(&victim_uuid, TimerType::PvpProtection),
                guard.timers.is_active(&victim_uuid, TimerType::CombatTag),
                guard.timers.is_active(&damager_uuid, TimerType::CombatTag),
                guard.config.timers.combat_tag_seconds,
                guard.config.factions.players_take_own_damage,
            )
        };

        if victim_uuid == damager_uuid && !allow_own {
            event.cancelled = true;
            return event;
        }

        // Safezone checks (port of `PlayerEventListener#onEntityDamageByEntity`).
        let (damager_safe, victim_safe) = {
            let dl = core::location(&damager);
            let vl = core::location(&victim);
            (
                matches!(
                    core::territory_at(&dl.world, dl.block_x(), dl.block_z()),
                    core::Territory::Safezone
                ),
                matches!(
                    core::territory_at(&vl.world, vl.block_x(), vl.block_z()),
                    core::Territory::Safezone
                ),
            )
        };

        if damager_safe {
            event.cancelled = true;
            let message = state()
                .read()
                .unwrap()
                .raw
                .language
                .string("FACTIONS.SAFEZONE.DENY_DAMAGE_ATTACKER");
            core::send(&damager, &message);
            return event;
        }

        if victim_safe {
            event.cancelled = true;
            let message = state()
                .read()
                .unwrap()
                .raw
                .language
                .string("FACTIONS.SAFEZONE.DENY_DAMAGE_OTHERS");
            core::send(&damager, &message);
            return event;
        }

        // Same faction (always) or ally (unless ally friendly fire is enabled) -> cancel.
        let (ally_ff, max_allies, ally_color) = {
            let guard = state().read().unwrap();
            (
                guard.config.factions.ally_friendly_fire,
                guard.config.factions.max_allies,
                guard.config.relation_colors.ally.clone(),
            )
        };

        if relation == Relation::Member {
            event.cancelled = true;
            return event;
        }

        if relation == Relation::Ally && !ally_ff {
            event.cancelled = true;
            if max_allies > 0 {
                let template = state()
                    .read()
                    .unwrap()
                    .raw
                    .language
                    .string("FACTIONS.DENY_DAMAGE_ALLIES");
                let message = util::replace(
                    &template,
                    "<player>",
                    &format!("{}{}", ally_color, core::name_of(&victim)),
                );
                core::send(&damager, &message);
            }
            return event;
        }

        if victim_protected {
            event.cancelled = true;
            core::send(&damager, "&cThat player has PvP protection.");
            return event;
        }

        // Activate combat tags for both players.
        let tag_message = util::replace(
            "&cYou are now combat tagged for &e<time>&c.",
            "<time>",
            &crate::util::format_duration(tag_seconds as i64 * 1000),
        );

        {
            let mut guard = state().write().unwrap();
            guard.timers.activate(&victim_uuid, TimerType::CombatTag, tag_seconds);
            guard.timers.activate(&damager_uuid, TimerType::CombatTag, tag_seconds);
        }

        if !victim_combat {
            core::send(&victim, &tag_message);
        }
        if !damager_combat {
            core::send(&damager, &tag_message);
        }

        event
    }
}

/// Blocks all damage (e.g. fall damage) while a player has PvP protection, and
/// cancels warmup teleports when damaged (port of `cancelHomeAndStuckTimersOnDamage`).
pub struct EntityDamageListener;

impl EventHandler<EntityDamageEvent> for EntityDamageListener {
    fn handle(&self, server: Server, mut event: EventData<EntityDamageEvent>) -> EventData<EntityDamageEvent> {
        let Some(player) = core::player_by_entity(&server, event.entity_id) else {
            return event;
        };

        let uuid = core::uuid_of(&player);

        // All damage is cancelled inside the safezone (port of `onEntityDamage`).
        {
            let location = core::location(&player);
            if matches!(
                core::territory_at(&location.world, location.block_x(), location.block_z()),
                core::Territory::Safezone
            ) {
                event.cancelled = true;
                return event;
            }
        }

        let message = {
            let mut guard = state().write().unwrap();
            match guard.timers.pending(&uuid).map(|p| p.timer_type) {
                Some(TimerType::Home) => {
                    guard.timers.clear_pending(&uuid);
                    Some(
                        guard
                            .raw
                            .language
                            .string("FACTIONS.HOME_COMMAND.TELEPORT_CANCELLED.DAMAGED"),
                    )
                }
                Some(TimerType::Stuck) => {
                    guard.timers.clear_pending(&uuid);
                    Some(
                        guard
                            .raw
                            .language
                            .string("FACTIONS.STUCK_COMMAND.TELEPORT_CANCELLED.DAMAGED"),
                    )
                }
                _ => None,
            }
        };
        if let Some(message) = message {
            core::send(&player, &message);
        }

        if protected(&player) {
            event.cancelled = true;
        }

        event
    }
}

/// Freezes hunger inside the safezone (port of `PlayerEventListener#onFoodLevelChange`).
pub struct FoodLevelChangeListener;

impl EventHandler<FoodLevelChangeEvent> for FoodLevelChangeListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<FoodLevelChangeEvent>,
    ) -> EventData<FoodLevelChangeEvent> {
        let Some(player) = core::player_by_entity(&server, event.entity_id) else {
            return event;
        };

        // Regeneration (food going up) is always allowed.
        if player.get_food_level() < event.food_level {
            return event;
        }

        let location = core::location(&player);
        if matches!(
            core::territory_at(&location.world, location.block_x(), location.block_z()),
            core::Territory::Safezone
        ) {
            event.cancelled = true;
        }

        event
    }
}

fn protected(player: &Player) -> bool {
    let uuid = core::uuid_of(player);
    state()
        .read()
        .unwrap()
        .timers
        .is_active(&uuid, TimerType::PvpProtection)
}

// ---------------------------------------------------------------------------
// Projectile ownership (so arrows respect safezone / friendly fire).
// ---------------------------------------------------------------------------

static PROJECTILE_SHOOTERS: std::sync::Mutex<Option<std::collections::HashMap<i32, String>>> =
    std::sync::Mutex::new(None);

fn set_projectile_shooter(entity_id: i32, shooter: String) {
    let mut guard = PROJECTILE_SHOOTERS.lock().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(entity_id, shooter);
}

fn take_projectile_shooter(entity_id: i32) -> Option<String> {
    let mut guard = PROJECTILE_SHOOTERS.lock().unwrap();
    guard.as_mut().and_then(|map| map.remove(&entity_id))
}

/// Records which player shot each projectile (arrows have their own entity id,
/// which is all the damage event gives us).
pub struct ProjectileLaunchListener;

impl EventHandler<ProjectileLaunchEvent> for ProjectileLaunchListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<ProjectileLaunchEvent>,
    ) -> EventData<ProjectileLaunchEvent> {
        if let Some(shooter_id) = event.shooter_id
            && let Some(shooter) = core::player_by_entity(&server, shooter_id)
        {
            set_projectile_shooter(event.entity_id, core::uuid_of(&shooter));
        }
        event
    }
}

pub struct ProjectileHitListener;

impl EventHandler<ProjectileHitEvent> for ProjectileHitListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<ProjectileHitEvent>,
    ) -> EventData<ProjectileHitEvent> {
        take_projectile_shooter(event.entity_id);
        event
    }
}

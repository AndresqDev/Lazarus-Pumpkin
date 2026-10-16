//! Combat listeners: friendly-fire prevention, combat tagging and PvP protection.
//!
//! Port of `handlers.timer.CombatTagHandler` / `PvpProtHandler` and the friendly
//! fire checks from the faction entity listener.

use pumpkin_plugin_api::events::{EntityDamageByEntityEvent, EntityDamageEvent, EventData, EventHandler};
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
        let damager = core::player_by_entity(&server, event.damager_id);

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

        // Same faction (always) or ally (unless ally friendly fire is enabled) -> cancel.
        let ally_ff = state().read().unwrap().config.factions.ally_friendly_fire;
        if relation == Relation::Member || (relation == Relation::Ally && !ally_ff) {
            event.cancelled = true;
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

/// Blocks all damage (e.g. fall damage) while a player has PvP protection.
pub struct EntityDamageListener;

impl EventHandler<EntityDamageEvent> for EntityDamageListener {
    fn handle(&self, server: Server, mut event: EventData<EntityDamageEvent>) -> EventData<EntityDamageEvent> {
        let Some(player) = core::player_by_entity(&server, event.entity_id) else {
            return event;
        };

        if protected(&player) {
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

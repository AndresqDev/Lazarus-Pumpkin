//! Combat listeners: friendly-fire prevention, combat tagging and PvP protection.
//!
//! Port of `handlers.timer.CombatTagHandler` / `PvpProtHandler` and the friendly
//! fire checks from the faction entity listener.

use pumpkin_plugin_api::events::{
    EntityDamageByEntityEvent, EntityDamageEvent, EntityShootBowEvent, EventData, EventHandler,
    FoodLevelChangeEvent, PlayerItemConsumeEvent, ProjectileHitEvent, ProjectileLaunchEvent,
};
use pumpkin_plugin_api::{DamageType, Player, Server};

use crate::utils::core;
use crate::factions::Relation;
use crate::state::state;
use crate::timer::TimerType;
use crate::utils;

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
        // Pumpkin attributes projectile damage directly to the shooter player, so
        // detect arrows via the damage type (the damager entity IS the player).
        let projectile_damager = event.cause.to_ascii_lowercase().contains("arrow");
        let cause = crate::handlers::death::cause_from_damage_debug(&event.cause).unwrap_or_default();

        let direct = core::player_by_entity(&server, event.damager_id);
        let shooter = if direct.is_none() {
            take_projectile_shooter(event.damager_id)
        } else {
            None
        };
        let shooter_player = match &shooter {
            Some(Shooter::Player(uuid)) => {
                pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
            }
            _ => None,
        };

        // Death message bookkeeping (port of `Player#getLastDamageCause`). The
        // generic damage event fires first and is overwritten here with the
        // damager details.
        if let Some(victim) = &victim {
            let entity_key = match &shooter {
                Some(Shooter::Entity(key)) => Some(key.clone()),
                None if direct.is_none() => {
                    crate::handlers::death::entity_key_by_id(&server, event.damager_id)
                }
                _ => None,
            };
            let damager_uuid = direct.as_ref().map(core::uuid_of).or_else(|| match &shooter {
                Some(Shooter::Player(uuid)) => Some(uuid.clone()),
                _ => None,
            });
            let projectile = crate::handlers::death::is_projectile_cause(&cause) || shooter.is_some();
            let record = crate::state::LastDamage {
                cause,
                entity: true,
                damager: damager_uuid,
                entity_key,
                projectile,
            };
            state().write().unwrap().last_damage.insert(core::uuid_of(victim), record);
        }

        let damager = direct.or(shooter_player);

        // Archer tag on a *non-player* living entity (e.g. a mannequin) — for
        // testing: apply the damage multiplier and notify the shooter.
        if projectile_damager && victim.is_none() {
            if let Some(shooter) = damager.as_ref() {
                let shooter_uuid = core::uuid_of(shooter);
                let (is_archer, multiplier, duration) = {
                    let guard = state().read().unwrap();
                    (
                        guard
                            .classes
                            .active
                            .get(&shooter_uuid)
                            == Some(&crate::classes::ClassKind::Archer),
                        guard
                            .raw.classes.double("ARCHER_CLASS.ARCHER_TAG.DAMAGE_MULTIPLIER") as f32,
                        guard
                            .raw.classes.int("ARCHER_CLASS.ARCHER_TAG.DURATION")
                            .max(0) as u64,
                    )
                };
                if is_archer {
                    event.damage *= multiplier;
                    let (prefix, template) = {
                        let guard = state().read().unwrap();
                        (
                            guard.raw.language.string("PREFIX"),
                            guard.raw.language.string("ARCHER_TAG.TAGGED_TAGGER"),
                        )
                    };
                    core::send(
                        shooter,
                        &format!(
                            "{prefix}{}",
                            utils::replace_all(
                                &template,
                                &[("<victim>", "mannequin"), ("<seconds>", &duration.to_string())],
                            )
                        ),
                    );
                }
            }
        }

        let (Some(victim), Some(damager)) = (victim, damager) else {
            return event;
        };

        let victim_uuid = core::uuid_of(&victim);
        let damager_uuid = core::uuid_of(&damager);

        let (
            relation,
            victim_protected,
            damager_protected,
            victim_combat,
            damager_combat,
            tag_seconds,
            allow_own,
            sotw_active,
            sotw_victim_enabled,
            sotw_damager_enabled,
        ) = {
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
                guard.timers.is_active(&damager_uuid, TimerType::PvpProtection),
                guard.timers.is_active(&victim_uuid, TimerType::CombatTag),
                guard.timers.is_active(&damager_uuid, TimerType::CombatTag),
                guard.config.timers.combat_tag_seconds,
                guard.config.factions.players_take_own_damage,
                guard.timers.sotw_active(),
                guard.sotw.enabled_players.contains(&victim_uuid),
                guard.sotw.enabled_players.contains(&damager_uuid),
            )
        };

        // Self damage is always denied while protected (`PvpProtHandler` cancels it
        // silently), plus the `PLAYERS_TAKE_OWN_DAMAGE` rule.
        if victim_uuid == damager_uuid && (damager_protected || !allow_own) {
            event.cancelled = true;
            return event;
        }

        // Safezone checks (port of `PlayerEventListener#onEntityDamageByEntity`).
        let dl = core::location(&damager);
        let vl = core::location(&victim);
        let (damager_safe, victim_safe) = {
            let guard = state().read().unwrap();
            (
                core::is_safezone_guard(
                    &guard,
                    &core::territory_at_guard(&guard, &dl.world, dl.block_x(), dl.block_z()),
                ),
                core::is_safezone_guard(
                    &guard,
                    &core::territory_at_guard(&guard, &vl.world, vl.block_x(), vl.block_z()),
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

        // SOTW: player-vs-player damage is only allowed between two players that
        // both opted out with `/sotw enable` (port of
        // `SotwHandler#onEntityDamageByEntity`, cancelled silently).
        if sotw_active && !(sotw_victim_enabled && sotw_damager_enabled) {
            event.cancelled = true;
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
                let message = utils::replace(
                    &template,
                    "<player>",
                    &format!("{}{}", ally_color, core::name_of(&victim)),
                );
                core::send(&damager, &message);
            }
            return event;
        }

        // PvP protection (port of `PvpProtHandler#onEntityDamageByEntity`, which
        // checks the attacker first and then the victim, with the original texts).
        if damager_protected {
            event.cancelled = true;
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.pvp_protection_deny_victim.clone(),
                )
            };
            core::send(&damager, &format!("{prefix}{message}"));
            return event;
        }

        if victim_protected {
            event.cancelled = true;
            let (prefix, template) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.pvp_protection_deny_attacker.clone(),
                )
            };
            let message = utils::replace(&template, "<name>", &core::name_of(&victim));
            core::send(&damager, &format!("{prefix}{message}"));
            return event;
        }

        // Archer tag: arrow damage from an active Archer.
        if projectile_damager {
            let (is_archer, victim_is_archer, can_tag_archers, require_force, duration) = {
                let guard = state().read().unwrap();
                (
                    guard.classes.active.get(&damager_uuid) == Some(&crate::classes::ClassKind::Archer),
                    guard.classes.active.get(&victim_uuid) == Some(&crate::classes::ClassKind::Archer),
                    guard.raw.classes.bool("ARCHER_CLASS.ARCHER_TAG.CAN_TAG_OTHER_ARCHERS"),
                    guard.raw.classes.bool("ARCHER_CLASS.ARCHER_TAG.REQUIRE_FULL_FORCE"),
                    guard.raw.classes.int("ARCHER_CLASS.ARCHER_TAG.DURATION").max(0) as u64,
                )
            };
            let force_ok = !require_force || bow_force(&damager_uuid) >= 0.5;
            if is_archer && duration > 0 && (can_tag_archers || !victim_is_archer) && force_ok {
                state().write().unwrap().classes.tag_archer(&victim_uuid, duration);
                let (victim_tpl, tagger_tpl, prefix) = {
                    let guard = state().read().unwrap();
                    (
                        guard.raw.language.string("ARCHER_TAG.TAGGED_VICTIM"),
                        guard.raw.language.string("ARCHER_TAG.TAGGED_TAGGER"),
                        guard.raw.language.string("PREFIX"),
                    )
                };
                let tagger_name = core::name_of(&damager);
                let victim_name = core::name_of(&victim);
                core::send(
                    &victim,
                    &format!(
                        "{prefix}{}",
                        utils::replace_all(
                            &victim_tpl,
                            &[("<player>", &tagger_name), ("<seconds>", &duration.to_string())],
                        )
                    ),
                );
                core::send(
                    &damager,
                    &format!(
                        "{prefix}{}",
                        utils::replace_all(
                            &tagger_tpl,
                            &[("<victim>", &victim_name), ("<seconds>", &duration.to_string())],
                        )
                    ),
                );
            }
        }

        // Rogue backstab: melee from behind with a gold sword.
        if !projectile_damager {
            let is_rogue = {
                let guard = state().read().unwrap();
                guard.classes.active.get(&damager_uuid) == Some(&crate::classes::ClassKind::Rogue)
            };
            let backstab_item = {
                let raw = state().read().unwrap();
                raw.raw
                    .classes
                    .string("ROGUE_CLASS.BACKSTAB.ITEM")
                    .to_lowercase()
            };
            let backstab_item = if backstab_item == "gold_sword" {
                "golden_sword".to_string()
            } else {
                backstab_item
            };
            let holding_sword = damager
                .get_item_in_hand(pumpkin_plugin_api::common::Hand::Right)
                .is_some_and(|item| {
                    item.get_registry_key()
                        .trim_start_matches("minecraft:")
                        .contains(&backstab_item)
                });

            if is_rogue && holding_sword {
                let victim_yaw = core::location(&victim).yaw;
                let damager_yaw = core::location(&damager).yaw;
                let diff = ((victim_yaw - damager_yaw + 540.0) % 360.0) - 180.0;
                let angle = diff.abs().to_radians();

                if angle < 1.4 {
                    let (damage, cooldown_ms, effects_enabled, effects) = {
                        let guard = state().read().unwrap();
                        (
                            guard.raw.classes.double("ROGUE_CLASS.BACKSTAB.DAMAGE") as f32,
                            guard.raw.classes.int("ROGUE_CLASS.BACKSTAB.COOLDOWN") * 1000,
                            guard.raw.classes.bool("ROGUE_CLASS.BACKSTAB.EFFECTS_ENABLED"),
                            guard.raw.classes.keys("ROGUE_CLASS.BACKSTAB.EFFECTS"),
                        )
                    };
                    let now = utils::now_millis();
                    let on_cooldown = {
                        let guard = state().read().unwrap();
                        guard
                            .classes
                            .rogue_backstabs
                            .get(&damager_uuid)
                            .is_some_and(|expiry| *expiry > now)
                    };

                    if on_cooldown {
                        let (prefix, template) = {
                            let guard = state().read().unwrap();
                            (
                                guard.raw.language.string("PREFIX"),
                                guard.raw.language.string("ROGUE_CLASS.BACKSTAB_COOLDOWN"),
                            )
                        };
                        let seconds = {
                            let guard = state().read().unwrap();
                            guard
                                .classes
                                .rogue_backstabs
                                .get(&damager_uuid)
                                .map(|expiry| (*expiry - now) / 1000)
                                .unwrap_or(0)
                        };
                        core::send(
                            &damager,
                            format!(
                                "{prefix}{}",
                                utils::replace(&template, "<seconds>", &seconds.to_string())
                            ),
                        );
                    } else {
                        event.damage = damage * 2.0;
                        state()
                            .write()
                            .unwrap()
                            .classes
                            .rogue_backstabs
                            .insert(damager_uuid.clone(), now + cooldown_ms);

                        if effects_enabled {
                            let (language, parsed) = {
                                let guard = state().read().unwrap();
                                let mut parsed = Vec::new();
                                for name in &effects {
                                    if let Some(effect) =
                                        crate::classes::effect_by_name(name)
                                    {
                                        let base =
                                            format!("ROGUE_CLASS.BACKSTAB.EFFECTS.{name}");
                                        let amplifier = (guard
                                            .raw
                                            .config
                                            .int(&format!("{base}.LEVEL"))
                                            .max(1)
                                            - 1) as u8;
                                        let duration = (guard
                                            .raw
                                            .config
                                            .int(&format!("{base}.DURATION"))
                                            .max(1)
                                            as u32)
                                            * 20;
                                        parsed.push((effect, amplifier, duration));
                                    }
                                }
                                (guard.raw.language.string("ROGUE_BACKSTAB_COOLDOWN_EXPIRED"), parsed)
                            };
                            for (effect, amplifier, duration) in parsed {
                                damager.add_effect(
                                    pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::StatusEffectInstance {
                                        effect_type: effect,
                                        duration,
                                        amplifier,
                                        ambient: false,
                                        show_particles: true,
                                        show_icon: true,
                                    },
                                );
                            }
                            let _ = language;
                        }
                    }
                }
            }
        }

        // Activate combat tags for both players.
        let tag_message = utils::replace(
            "&cYou are now combat tagged for &e<time>&c.",
            "<time>",
            &crate::utils::format_duration(tag_seconds as i64 * 1000),
        );

        {
            let mut guard = state().write().unwrap();
            guard.timers.activate(&victim_uuid, TimerType::CombatTag, tag_seconds);
            guard.timers.activate(&damager_uuid, TimerType::CombatTag, tag_seconds);
            // Runtime replacement for Bukkit's `Player#getKiller` (used by the
            // death flow to credit the kill): the last player to damage the
            // victim, valid for a few seconds.
            guard
                .last_damagers
                .insert(victim_uuid.clone(), (damager_uuid.clone(), utils::now_millis()));
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

/// Blocks the damage causes the original PvP protection denied and cancels
/// warmup teleports when damaged (port of `PvpProtHandler#onEntityDamage` and
/// `cancelHomeAndStuckTimersOnDamage`). SOTW immunity is handled here too.
pub struct EntityDamageListener;

impl EventHandler<EntityDamageEvent> for EntityDamageListener {
    fn handle(&self, server: Server, mut event: EventData<EntityDamageEvent>) -> EventData<EntityDamageEvent> {
        let Some(player) = core::player_by_entity(&server, event.entity_id) else {
            return event;
        };

        let uuid = core::uuid_of(&player);

        // Death message bookkeeping (`Player#getLastDamageCause`): recorded from
        // the generic event first; `EntityDamageByEntity` overwrites it with the
        // damager details afterwards.
        {
            let cause = crate::handlers::death::normalize_cause(&format!("{:?}", event.damage_type));
            let record = crate::state::LastDamage {
                cause,
                entity: false,
                damager: None,
                entity_key: None,
                projectile: false,
            };
            state().write().unwrap().last_damage.insert(uuid.clone(), record);
        }

        // Archer tag: increased damage taken.
        let archer_multiplier = {
            let guard = state().read().unwrap();
            if guard.classes.is_archer_tagged(&uuid) {
                guard.raw.classes.double("ARCHER_CLASS.ARCHER_TAG.DAMAGE_MULTIPLIER") as f32
            } else {
                1.0
            }
        };
        if archer_multiplier != 1.0 {
            event.damage *= archer_multiplier;
        }

        // SOTW: players are damage-immune unless they opted out; falling into the
        // void teleports back to the world spawn first (port of
        // `SotwHandler#onPlayerDamage`, which cancels every cause).
        if crate::handlers::timer::sotw::is_under_protection(&uuid) {
            if event.damage_type == DamageType::OutOfWorld {
                let world_name = core::world_name(&player);
                let (x, z) = state()
                    .read()
                    .unwrap()
                    .spawn_centers
                    .get(&world_name)
                    .copied()
                    .unwrap_or((0, 0));
                if let Some(world) = server.get_world_by_name(&world_name) {
                    player.teleport((x as f64 + 0.5, 100.0, z as f64 + 0.5), None, None, world);
                }
                let (prefix, message) = {
                    let guard = state().read().unwrap();
                    (
                        guard.raw.language.string("PREFIX"),
                        guard.lang.sotw_void_fix.clone(),
                    )
                };
                core::send(&player, &format!("{prefix}{message}"));
            }
            event.cancelled = true;
            return event;
        }

        // All damage is cancelled inside the safezone (port of `onEntityDamage`).
        {
            let location = core::location(&player);
            let in_safezone = {
                let guard = state().read().unwrap();
                core::is_safezone_guard(
                    &guard,
                    &core::territory_at_guard(
                        &guard,
                        &location.world,
                        location.block_x(),
                        location.block_z(),
                    ),
                )
            };
            if in_safezone {
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
                // `LogoutHandler#onPlayerDamage`.
                Some(TimerType::Logout) => {
                    guard.timers.clear_pending(&uuid);
                    Some(format!(
                        "{}{}",
                        guard.raw.language.string("PREFIX"),
                        guard
                            .raw
                            .language
                            .string("LOGOUT.TELEPORT_CANCELLED_DAMAGED")
                    ))
                }
                _ => None,
            }
        };
        if let Some(message) = message {
            core::send(&player, &message);
        }

        // PvP protection (port of the `disabledDamageCauses` EnumSet in
        // `PvpProtHandler#onEntityDamage`): only drowning, suffocation, poison
        // and fire damage are blocked — fall damage, mobs and explosions still
        // hurt protected players, exactly like the original.
        if protected(&player)
            && matches!(
                event.damage_type,
                DamageType::Drown
                    | DamageType::InWall
                    | DamageType::Magic
                    | DamageType::InFire
                    | DamageType::OnFire
            )
        {
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

        // PvP protection keeps hunger and saturation full (port of
        // `PvpProtHandler#onFoodLevelChange`).
        if protected(&player) {
            event.cancelled = true;
            player.set_food_level(20);
            player.set_saturation(20.0);
            return event;
        }

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

static PROJECTILE_SHOOTERS: std::sync::Mutex<Option<std::collections::HashMap<i32, Shooter>>> =
    std::sync::Mutex::new(None);

/// Who launched a projectile: a player, or a non-player entity recorded by its
/// registry name (resolved through the `EntitySpawnEvent` registry so death
/// messages can name it).
#[derive(Clone)]
pub(crate) enum Shooter {
    Player(String),
    Entity(String),
}

fn set_projectile_shooter(entity_id: i32, shooter: Shooter) {
    let mut guard = PROJECTILE_SHOOTERS.lock().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(entity_id, shooter);
}

pub(crate) fn take_projectile_shooter(entity_id: i32) -> Option<Shooter> {
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
        if let Some(shooter_id) = event.shooter_id {
            if let Some(shooter) = core::player_by_entity(&server, shooter_id) {
                set_projectile_shooter(event.entity_id, Shooter::Player(core::uuid_of(&shooter)));
            } else if let Some(key) = crate::handlers::death::entity_key_by_id(&server, shooter_id) {
                set_projectile_shooter(event.entity_id, Shooter::Entity(key));
            }
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
        // Do NOT remove the shooter here: the `EntityDamageByEntity` for the same
        // arrow may fire afterwards, and it needs the shooter to resolve.
        let _ = event;
        event
    }
}


// ---------------------------------------------------------------------------
// Archer class: bow force + tag
// ---------------------------------------------------------------------------

static LAST_BOW_FORCE: std::sync::Mutex<Option<std::collections::HashMap<String, f32>>> =
    std::sync::Mutex::new(None);

fn set_bow_force(uuid: &str, force: f32) {
    LAST_BOW_FORCE
        .lock()
        .unwrap()
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(uuid.to_string(), force);
}

fn bow_force(uuid: &str) -> f32 {
    LAST_BOW_FORCE
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|map| map.get(uuid).copied())
        .unwrap_or(1.0)
}

/// Records the bow force of the shooter (used by `REQUIRE_FULL_FORCE`).
pub struct ShootBowListener;

impl EventHandler<EntityShootBowEvent> for ShootBowListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<EntityShootBowEvent>,
    ) -> EventData<EntityShootBowEvent> {
        if let Some(shooter) = core::player_by_entity(&server, event.entity_id) {
            set_bow_force(&core::uuid_of(&shooter), event.force);
        }
        event
    }
}

/// Re-applies the active class effects after drinking milk.
pub struct MilkListener;

impl EventHandler<PlayerItemConsumeEvent> for MilkListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerItemConsumeEvent>,
    ) -> EventData<PlayerItemConsumeEvent> {
        if event.item_name.contains("milk") {
            crate::classes::reapply_class_effects(&event.player);
        }
        event
    }
}

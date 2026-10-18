//! Combat logger — port of `me.qiooip.lazarus.handlers.logger.CombatLoggerHandler`
//! and the NMS logger entities (`nms.VillagerCombatLogger_1_8` /
//! `nms.SkeletonCombatLogger_1_8`).
//!
//! A player who quits (unless protected/banned from logging) leaves a named mob
//! carrying their gear on the logout spot; whoever kills it receives the
//! captured items, while the offline victim is deathbanned, loses the inventory
//! and gets the copied potion effects restored on their next join.
//!
//! Host-API limits (the original used direct NMS access):
//! * there is no item-drop endpoint, so the captured inventory is handed to the
//!   killer's inventory (the same convention as the head drops); items that do
//!   not fit — and every item when no *player* dealt the killing blow — are
//!   lost instead of staying on the ground;
//! * there is no potion-effect endpoint for non-player entities: the effects
//!   are only *stored* on the record, which is exactly what
//!   `handleEffectChanges` needs on the victim's rejoin;
//! * offline player data cannot be read or written: the `die()` inventory clear
//!   is deferred to the victim's next join (`Userdata#pending_logger_clear`)
//!   and the offline `setPosition`/`setHealth(0)` are skipped — the player
//!   already logs out (and back in) on the logger spot, so only the death
//!   screen part of that update is missing;
//! * the equipment displayed on the entity is cleared right before the host
//!   computes its death drops (the death event fires first), so the vanilla
//!   random equipment drops cannot duplicate the hand-out — the original
//!   emptied `dropDeathLoot` for the same reason; the remaining vanilla loot
//!   (bones/arrows) and the experience orbs of that death flow are cancelled
//!   through `ItemSpawnEvent`/`EntitySpawnEvent` markers, replicating the
//!   empty `dropDeathLoot` override and `expToDrop = 0`.

use std::sync::Mutex;

use pumpkin_plugin_api::common::{BlockPos, Hand};
use pumpkin_plugin_api::events::{
    EntityDamageByEntityEvent, EntityDamageEvent, EntityDeathEvent, EntitySpawnEvent, EventData,
    EventHandler, ItemSpawnEvent, PlayerLeaveEvent,
};
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::{
    StatusEffectInstance, StatusEffectType,
};
use pumpkin_plugin_api::world::EquipmentSlot;
use pumpkin_plugin_api::{
    Context, DamageType, Entity, EntityCastExt, EntityType, ItemStack, Player, Server, Villager,
    VillagerProfession,
};

use crate::utils::core;
use crate::factions::Relation;
use crate::listeners::combat::Shooter;
use crate::state::{state, AppState, PendingLoggerRemoval};
use crate::timer::{PendingAction, TimerType};
use crate::utils;

/// Exact copies of the `COMBAT_LOGGER` defaults from the original `config.yml`,
/// used when the deployed file is missing a key.
const DEFAULT_NAME_FORMAT: &str = "&7(CombatLogger) &c<player>";
const DEFAULT_TIME_SECONDS: i64 = 15;
const DEFAULT_VILLAGER_PROFESSION: i32 = 2;

/// One copied potion effect (`this.addEffect` in the NMS constructor).
#[derive(Debug, Clone)]
pub struct CapturedEffect {
    pub effect_type: StatusEffectType,
    /// Ticks left when the effect was copied; frozen at removal time to match
    /// `nmsEffect.getDuration()` in `handleEffectChanges`.
    pub duration_ticks: u32,
    pub amplifier: u8,
    pub ambient: bool,
}

/// One live combat logger (`CombatLoggerHandler#combatLoggers` value).
pub struct CombatLogger {
    /// Victim uuid (also the map key).
    pub uuid: String,
    /// Victim name, for the death message.
    pub name: String,
    /// Host entity id of the spawned logger (the death event only carries ids).
    pub entity_id: i32,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    /// `getCombatLoggerHealth` (the player's health, clamped to the entity max).
    pub health: f32,
    /// `player.getInventory().getContents()` (36 main slots).
    pub contents: Vec<Option<ItemStack>>,
    /// `player.getInventory().getArmorContents()`: boots, leggings, chest, helmet.
    pub armor: Vec<Option<ItemStack>>,
    /// Effects copied at logout (only those with `duration <= 12000` ticks).
    pub effects: Vec<CapturedEffect>,
    /// When the logout happened (used to freeze the copied durations).
    pub captured_at: i64,
    /// `DeathbanManager#getBanTime` resolved while the permissions were valid.
    pub deathban_millis: i64,
    /// Damager of the last damage taken (`damageSource.getEntity()`, players
    /// only) — resolves the killer of the death event, which carries none.
    pub last_damager: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `Register the bypass node` (same rules as `death::has_headdrop_permission`:
/// Bukkit granted undefined nodes to operators, the host does not).
pub fn init(context: &Context) {
    if let Err(error) = crate::commands::register_op_permission(
        context,
        "lazarus:combatlogger.bypass",
        "Bypass combat loggers on quit",
    ) {
        tracing::warn!("failed to register lazarus:combatlogger.bypass: {error}");
    }
}

fn has_bypass_permission(player: &Player) -> bool {
    player.has_permission("lazarus.combatlogger.bypass")
        || player.has_permission("lazarus:combatlogger.bypass")
}

/// The `logger` entity of a record, resolved through the world's entity list
/// (host entity ids are stable while the entity is loaded).
fn resolve_entity(server: &Server, world_name: &str, entity_id: i32) -> Option<Entity> {
    let world = server.get_world_by_name(world_name)?;
    world
        .get_entities()
        .into_iter()
        .find(|entity| entity.get_id() as i32 == entity_id)
}

/// TTL of a deferred logger removal; a chunk that stays unloaded beyond this
/// keeps the ghost entity, exactly like a plugin restart would.
const PENDING_REMOVAL_TTL_MILLIS: i64 = 10 * 60 * 1000;

fn remove_entity(server: &Server, world_name: &str, entity_id: i32) {
    if let Some(entity) = resolve_entity(server, world_name, entity_id) {
        entity.remove();
        return;
    }

    // The host serialises an entity away as soon as its chunk loses its last
    // watcher (`remove_entities_in_chunks`, without the usual unload delay), so
    // a logger that expires while its owner walked away is not live anymore.
    // Queue the removal for the tick, which retries until the chunk loads.
    let mut guard = state().write().unwrap();
    guard.pending_logger_removals.push(PendingLoggerRemoval {
        world: world_name.to_string(),
        entity_id,
        expires: utils::now_millis() + PENDING_REMOVAL_TTL_MILLIS,
    });
}

/// Retries the deferred removals (1s tick). Runs only while entries exist.
pub fn process_pending_removals(server: &Server) {
    let now = utils::now_millis();
    let pending = {
        let mut guard = state().write().unwrap();
        std::mem::take(&mut guard.pending_logger_removals)
    };

    if pending.is_empty() {
        return;
    }

    let mut keep = Vec::new();
    for entry in pending {
        if entry.expires <= now {
            continue;
        }

        if let Some(entity) = resolve_entity(server, &entry.world, entry.entity_id) {
            // Safety net: a ghost that turns live again must not carry the
            // display equipment into the world's drop table.
            if let Some(living) = entity.as_living() {
                living.clear_equipment();
            }
            entity.remove();
        } else {
            keep.push(entry);
        }
    }

    if !keep.is_empty() {
        state().write().unwrap().pending_logger_removals.extend(keep);
    }
}

/// `nmsEffect.getDuration()` when the logger is removed: the copied durations
/// tick down while the entity lives, so they are frozen at removal.
fn freeze_effects(record: &mut CombatLogger) {
    let elapsed = ((utils::now_millis() - record.captured_at).max(0) / 50) as u32;
    for effect in &mut record.effects {
        effect.duration_ticks = effect.duration_ticks.saturating_sub(elapsed);
    }
}

/// `NmsUtils#spawnCombatLogger`: only `VILLAGER` selects the villager (the
/// original `CombatLoggerType#valueOf` would throw on an unknown value).
fn logger_type(raw: &str) -> EntityType {
    if raw.eq_ignore_ascii_case("VILLAGER") {
        EntityType::Villager
    } else {
        EntityType::Skeleton
    }
}

/// 1.7/1.8 profession ids (`config.yml`: FARMER 0 … BUTCHER 4) mapped to the
/// modern professions.
fn profession_for(id: i64) -> VillagerProfession {
    match id {
        0 => VillagerProfession::Farmer,
        1 => VillagerProfession::Librarian,
        2 => VillagerProfession::Cleric,
        3 => VillagerProfession::Armorer,
        4 => VillagerProfession::Butcher,
        _ => VillagerProfession::None,
    }
}

/// `DeathMessageHandler#getPlayerName` with the offline victim's userdata.
fn player_display(guard: &AppState, uuid: &str, name: &str) -> String {
    let kills = guard.userdata.get(uuid).map(|user| user.kills).unwrap_or(0);

    utils::replace_all(
        &guard.raw.language.string("DEATHMESSAGE.PLAYER_NAME_FORMAT"),
        &[("<player>", name), ("<kills>", &kills.to_string())],
    )
}

/// `DeathMessageHandler#getKillerName` (displayed count already includes the
/// kill that is about to be credited).
fn killer_display(guard: &AppState, uuid: &str, name: &str) -> String {
    let kills = guard
        .userdata
        .get(uuid)
        .map(|user| (user.kills + 1).to_string())
        .unwrap_or_else(|| "?".to_string());

    utils::replace_all(
        &guard.raw.language.string("DEATHMESSAGE.KILLER_NAME_FORMAT"),
        &[("<killer>", name), ("<kills>", &kills)],
    )
}

/// `handleEffectChanges`: every effect on the rejoining player is replaced by
/// the logger's copy (fresh duration, the entity's own amplifier/ambient) or
/// removed when the logger did not carry it. The original went through
/// `PotionEffectRestorer#removePlayerEffect` for the removal branch; the class
/// effect bank it restores from is part of the abilities sector, so the direct
/// removal is used until that lands.
fn handle_effect_changes(player: &Player, captured: &[CapturedEffect]) {
    for effect in player.get_active_effects() {
        let Some(copy) = captured
            .iter()
            .find(|copy| copy.effect_type == effect.effect_type)
        else {
            player.remove_effect(effect.effect_type);
            continue;
        };

        if copy.duration_ticks == 0 {
            player.remove_effect(effect.effect_type);
            continue;
        }

        player.add_effect(StatusEffectInstance {
            effect_type: copy.effect_type,
            duration: copy.duration_ticks,
            amplifier: copy.amplifier,
            ambient: copy.ambient,
            show_particles: true,
            show_icon: true,
        });
    }
}

/// `CombatLoggerHandler#onPlayerJoin`: cancels the safe-logout flag, applies
/// the deferred inventory wipe of a dead logger and restores a live one.
/// Called from the join listener right after the deathban gate.
pub fn on_join(server: &Server, player: &Player) {
    let uuid = core::uuid_of(player);

    let (record, clear_inventory) = {
        let mut guard = state().write().unwrap();

        // `player.removeMetadata("Logout", ...)`.
        guard.safe_logouts.remove(&uuid);

        let clear = guard
            .userdata
            .get_mut(&uuid)
            .is_some_and(|user| std::mem::take(&mut user.pending_logger_clear));

        (guard.combat_loggers.remove(&uuid), clear)
    };

    // Deferred part of `die()`: the offline inventory was cleared (main + armor,
    // the off-hand is untouched exactly like the original's 1.8 world).
    if clear_inventory {
        let inventory = player.get_inventory();
        inventory.clear_main();
        inventory.clear_armor();
    }

    let Some(mut record) = record else {
        return;
    };

    player.set_health(record.health);

    if let Some(world) = server.get_world_by_name(&record.world) {
        player.teleport(
            (record.x, record.y, record.z),
            Some(record.yaw),
            Some(record.pitch),
            world,
        );
    }

    freeze_effects(&mut record);
    handle_effect_changes(player, &record.effects);
    remove_entity(server, &record.world, record.entity_id);
}

// ---------------------------------------------------------------------------
// Quit: spawn the logger
// ---------------------------------------------------------------------------

pub struct CombatLoggerQuitListener;

impl EventHandler<PlayerLeaveEvent> for CombatLoggerQuitListener {
    fn handle(&self, server: Server, event: EventData<PlayerLeaveEvent>) -> EventData<PlayerLeaveEvent> {
        let player = &event.player;
        let uuid = core::uuid_of(player);

        // `LogoutHandler#onPlayerQuit`: a warmup interrupted by the quit never
        // completes, so this logout does not count as safe.
        {
            let mut guard = state().write().unwrap();
            if guard.timers.pending(&uuid).map(|action| action.timer_type)
                == Some(TimerType::Logout)
            {
                guard.timers.clear_pending(&uuid);
            }
        }

        // `CombatLoggerHandler#onPlayerQuit` conditions. Host calls first.
        let health = player.get_health();

        if state().read().unwrap().combat_logger_kick_all {
            return event;
        }
        if health == 0.0 {
            return event;
        }
        if has_bypass_permission(player) {
            return event;
        }

        {
            let mut guard = state().write().unwrap();

            // `player.hasMetadata("logout")`.
            if guard.safe_logouts.remove(&uuid) {
                return event;
            }
            if guard.timers.is_active(&uuid, TimerType::PvpProtection) {
                return event;
            }
            // `StaffModeManager#isInStaffModeOrVanished`.
            if guard.staff.is_staff(&uuid) || guard.staff.is_vanished(&uuid) {
                return event;
            }
        }

        if crate::handlers::timer::sotw::is_under_protection(&uuid) {
            return event;
        }

        let location = core::location(player);
        {
            let guard = state().read().unwrap();
            let territory = core::territory_at_guard(
                &guard,
                &location.world,
                location.block_x(),
                location.block_z(),
            );
            if core::is_safezone_guard(&guard, &territory) {
                return event;
            }
        }

        // Config snapshot (`COMBAT_LOGGER` section).
        let (raw_type, name_format, seconds, profession) = {
            let guard = state().read().unwrap();
            (
                guard.raw.config.string("COMBAT_LOGGER.TYPE"),
                guard.raw.config.string("COMBAT_LOGGER.NAME_FORMAT"),
                guard.raw.config.int("COMBAT_LOGGER.TIME"),
                guard.raw.config.int("COMBAT_LOGGER.VILLAGER_PROFESSION"),
            )
        };
        let name_format = if name_format.is_empty() {
            DEFAULT_NAME_FORMAT.to_string()
        } else {
            name_format
        };
        let seconds = if seconds <= 0 {
            DEFAULT_TIME_SECONDS
        } else {
            seconds
        };
        let profession = if profession < 0 {
            DEFAULT_VILLAGER_PROFESSION as i64
        } else {
            profession
        };

        // The `VillagerCombatLogger_1_8` constructor reads everything at once.
        let name = core::name_of(player);
        let contents: Vec<Option<ItemStack>> = (0..=35u8)
            .map(|slot| player.get_inventory_item(slot))
            .collect();
        let armor = {
            let inventory = player.get_inventory();
            vec![
                inventory.get_boots(),
                inventory.get_leggings(),
                inventory.get_chestplate(),
                inventory.get_helmet(),
            ]
        };
        let now = utils::now_millis();
        let effects: Vec<CapturedEffect> = player
            .get_active_effects()
            .into_iter()
            .filter(|effect| effect.duration <= 12000)
            .map(|effect| CapturedEffect {
                effect_type: effect.effect_type,
                duration_ticks: effect.duration,
                amplifier: effect.amplifier,
                ambient: effect.ambient,
            })
            .collect();
        let deathban_millis = crate::deathban::ban_time_millis(player);

        // Spawn (host calls): the logger lands on the feet block, or one block
        // above when a solid block occupies that spot.
        let Some(world) = server.get_world_by_name(&location.world) else {
            return event;
        };
        let feet = world.get_block_state(BlockPos {
            x: location.block_x(),
            y: location.block_y(),
            z: location.block_z(),
        });
        let spawn_y = if feet.is_air {
            location.y
        } else {
            location.y.floor() + 1.0
        };
        let entity_type = logger_type(&raw_type);
        let entity = world.spawn_entity(entity_type, (location.x, spawn_y, location.z));

        entity.set_rotation(location.yaw, location.pitch);
        let display = utils::replace(&name_format, "<player>", &name);
        entity.set_custom_name(utils::text(&display));
        entity.set_custom_name_visible(true);

        if let Some(living) = entity.as_living() {
            // `this.setHealth(player.getHealth())` with the 1.8 clamp to the
            // entity's max health.
            let max_health = living.get_max_health();
            living.set_health(health.min(max_health).max(0.0));

            // `setEquipment(0, itemInHand)` + `setEquipment(1..4, armor)`. The
            // items are re-read so the recorded wrappers stay alive.
            living.set_equipment(EquipmentSlot::MainHand, player.get_item_in_hand(Hand::Right));
            let inventory = player.get_inventory();
            living.set_equipment(EquipmentSlot::Feet, inventory.get_boots());
            living.set_equipment(EquipmentSlot::Legs, inventory.get_leggings());
            living.set_equipment(EquipmentSlot::Chest, inventory.get_chestplate());
            living.set_equipment(EquipmentSlot::Head, inventory.get_helmet());
        }

        if let Some(mob) = entity.as_mob() {
            // The original overrode `move`/`getGoalTarget`/`collide` and never
            // ran AI on the logger.
            mob.set_ai_disabled(true);
            mob.set_target(None);
        }

        if entity_type == EntityType::Villager {
            if let Some(villager) = entity.cast::<Villager>() {
                villager.set_profession(profession_for(profession));
            }
        }

        let entity_id = entity.get_id() as i32;
        {
            let mut guard = state().write().unwrap();
            guard.combat_loggers.insert(
                uuid.clone(),
                CombatLogger {
                    uuid: uuid.clone(),
                    name,
                    entity_id,
                    world: location.world,
                    x: location.x,
                    y: spawn_y,
                    z: location.z,
                    yaw: location.yaw,
                    pitch: location.pitch,
                    health,
                    contents,
                    armor,
                    effects,
                    captured_at: now,
                    deathban_millis,
                    last_damager: None,
                },
            );
        }

        // `Tasks.syncLater(() -> { removeCombatLogger; bukkitEntity.remove(); },
        // TIME * 20)`. Host tasks cannot be cancelled, so the entity id guards
        // against removing a *later* logger of the same player.
        server.schedule_delayed_task(seconds.max(1) as u64 * 20, move |server| {
            let removed = {
                let mut guard = state().write().unwrap();
                match guard.combat_loggers.get(&uuid) {
                    Some(record) if record.entity_id == entity_id => {
                        guard.combat_loggers.remove(&uuid)
                    }
                    _ => None,
                }
            };

            if let Some(mut record) = removed {
                freeze_effects(&mut record);
                remove_entity(&server, &record.world, record.entity_id);
            }
        });

        event
    }
}

// ---------------------------------------------------------------------------
// Damage: fire immunity + killing-blow bookkeeping + player rules
// ---------------------------------------------------------------------------

/// Generic damage: `fireProof = true` (`Entity#isInvulnerable` returns true for
/// fire sources) plus the killing-blow reset (the by-entity event, fired right
/// after, records the actual source).
pub struct CombatLoggerDamageListener;

impl EventHandler<EntityDamageEvent> for CombatLoggerDamageListener {
    fn handle(&self, _server: Server, mut event: EventData<EntityDamageEvent>) -> EventData<EntityDamageEvent> {
        {
            let mut guard = state().write().unwrap();
            match guard
                .combat_loggers
                .values_mut()
                .find(|record| record.entity_id == event.entity_id)
            {
                Some(record) => record.last_damager = None,
                None => return event,
            }
        }

        // Fire damage never touches the logger (1.8 `DamageSource#isFire`:
        // FIRE, FIRE_TICK, LAVA and HOT_FLOOR; campfires are the modern
        // equivalent, lightning bolts were *not* fire sources in 1.8).
        if matches!(
            event.damage_type,
            DamageType::InFire
                | DamageType::OnFire
                | DamageType::Lava
                | DamageType::HotFloor
                | DamageType::Campfire
        ) {
            event.cancelled = true;
            return event;
        }

        // `damageSource instanceof EntityDamageSourceIndirect &&
        // damageSource.isMagic()`: splash potions never damage the logger. The
        // by-entity handler covers the same check through the cause string.
        if event.damage_type == DamageType::IndirectMagic {
            event.cancelled = true;
        }

        event
    }
}

/// By-entity damage: the `damageEntity` player rules of the NMS logger.
pub struct CombatLoggerDamageByEntityListener;

impl EventHandler<EntityDamageByEntityEvent> for CombatLoggerDamageByEntityListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<EntityDamageByEntityEvent>,
    ) -> EventData<EntityDamageByEntityEvent> {
        let owner = {
            let guard = state().read().unwrap();
            match guard
                .combat_loggers
                .values()
                .find(|record| record.entity_id == event.entity_id)
            {
                Some(record) => record.uuid.clone(),
                None => return event,
            }
        };

        // Indirect magic through a projectile source (splash potion).
        if crate::handlers::death::cause_from_damage_debug(&event.cause).as_deref() == Some("indirect_magic") {
            event.cancelled = true;
            return event;
        }

        // `damageSource.getEntity()`: the direct player damager, or the shooter
        // tracked on launch (Pumpkin already attributes bow damage to players).
        let damager_uuid = match core::player_by_entity(&server, event.damager_id) {
            Some(player) => Some(core::uuid_of(&player)),
            None => match crate::listeners::combat::take_projectile_shooter(event.damager_id) {
                Some(Shooter::Player(uuid)) => Some(uuid),
                _ => None,
            },
        };

        // Killing-blow bookkeeping (the death event carries no source).
        {
            let mut guard = state().write().unwrap();
            if let Some(record) = guard.combat_loggers.get_mut(&owner) {
                record.last_damager = damager_uuid.clone();
            }
        }

        // `damageSourceEntity == this.player`: the owner cannot hit their own
        // logger.
        if damager_uuid.as_deref() == Some(owner.as_str()) {
            event.cancelled = true;
            return event;
        }

        let Some(damager_uuid) = damager_uuid else {
            // Non-player damagers only pass the checks above.
            return event;
        };

        // `SotwHandler#isUnderSotwProtection` (silent).
        if crate::handlers::timer::sotw::is_under_protection(&damager_uuid) {
            event.cancelled = true;
            return event;
        }

        enum Outcome {
            Allow,
            Silent,
            Message(String),
        }

        let outcome = {
            let mut guard = state().write().unwrap();
            let prefix = guard.raw.language.string("PREFIX");

            if guard.timers.is_active(&damager_uuid, TimerType::PvpProtection) {
                Outcome::Message(format!("{prefix}{}", guard.lang.pvp_protection_deny_victim))
            } else if guard.staff.is_staff(&damager_uuid) {
                Outcome::Message(format!(
                    "{prefix}{}",
                    guard.raw.language.string("STAFF_MODE.DAMAGE_DENY")
                ))
            } else if guard.staff.is_vanished(&damager_uuid) {
                Outcome::Message(format!(
                    "{prefix}{}",
                    guard.raw.language.string("VANISH.DAMAGE_DENY")
                ))
            } else {
                // `FactionsManager#getPlayerFaction(this.player)` vs the
                // damager's faction.
                let relation = match (
                    guard.factions.by_member(&owner),
                    guard.factions.by_member(&damager_uuid),
                ) {
                    (Some(victim_faction), Some(damager_faction)) => {
                        victim_faction.relation_to(damager_faction)
                    }
                    _ => Relation::Enemy,
                };

                if relation == Relation::Member {
                    Outcome::Silent
                } else if relation == Relation::Ally && !guard.config.factions.ally_friendly_fire {
                    let victim_name = guard
                        .combat_loggers
                        .get(&owner)
                        .map(|record| record.name.clone())
                        .unwrap_or_default();
                    let ally_color = guard.config.relation_colors.ally.clone();
                    Outcome::Message(utils::replace(
                        &guard.raw.language.string("FACTIONS.DENY_DAMAGE_ALLIES"),
                        "<player>",
                        &format!("{ally_color}{victim_name}"),
                    ))
                } else {
                    // `TimerManager#getCombatTagTimer().activate(damager)`.
                    let seconds = guard.config.timers.combat_tag_seconds;
                    guard
                        .timers
                        .activate(&damager_uuid, TimerType::CombatTag, seconds);
                    Outcome::Allow
                }
            }
        };

        match outcome {
            Outcome::Allow => {}
            Outcome::Silent => event.cancelled = true,
            Outcome::Message(message) => {
                event.cancelled = true;
                if let Some(damager) =
                    pumpkin_plugin_api::uuid::parse(&damager_uuid).and_then(|id| server.get_player_by_uuid(id))
                {
                    core::send(&damager, message);
                }
            }
        }

        event
    }
}

// ---------------------------------------------------------------------------
// Death: `VillagerCombatLogger_1_8#die`
// ---------------------------------------------------------------------------

pub struct CombatLoggerDeathListener;

impl EventHandler<EntityDeathEvent> for CombatLoggerDeathListener {
    fn handle(&self, server: Server, event: EventData<EntityDeathEvent>) -> EventData<EntityDeathEvent> {
        // The death event carries neither a type nor a killer: the logger is
        // matched through the tracked entity id.
        let found = {
            let mut guard = state().write().unwrap();
            let uuid = guard
                .combat_loggers
                .iter()
                .find(|(_, record)| record.entity_id == event.entity_id)
                .map(|(uuid, _)| uuid.clone());
            uuid.and_then(|uuid| {
                guard
                    .combat_loggers
                    .remove(&uuid)
                    .map(|record| (uuid, record))
            })
        };
        let Some((uuid, mut record)) = found else {
            return event;
        };

        // The host fires this event *before* computing the death drops:
        // clearing the equipment keeps the vanilla random equipment drop from
        // handing the same items out twice, and the position marker swallows
        // the vanilla loot table and experience orbs (`dropDeathLoot` was an
        // empty override and `expToDrop` stayed 0 in the original). Both use
        // the entity's live position, like `this.bukkitEntity.getLocation()`.
        let mut death_position = (record.x, record.y, record.z);
        if let Some(entity) = resolve_entity(&server, &record.world, record.entity_id) {
            death_position = entity.get_position();
            if let Some(living) = entity.as_living() {
                living.clear_equipment();
            }
        }
        mark_vanilla_drop(&record.world, death_position);

        freeze_effects(&mut record);

        // `damageSource.getEntity() instanceof EntityPlayer`.
        let killer = record
            .last_damager
            .as_deref()
            .and_then(pumpkin_plugin_api::uuid::parse)
            .and_then(|id| server.get_player_by_uuid(id));
        let killer_uuid = killer.as_ref().map(core::uuid_of);
        let killer_name = killer.as_ref().map(core::name_of);

        // `VillagerCombatLogger_1_8#die` → `deathbanPlayer` →
        // `saveInventoryUponDeath`: the original read the (still intact)
        // offline player inventory; the port snapshots the logger's capture
        // right before it is drained into the killer.
        crate::handlers::restore::save_captured(&uuid, &record.contents, &record.armor);

        // `dropPlayerItems(this.contents, this.armor, location)`: the WASM API
        // cannot spawn item entities, so both arrays go to the killer's
        // inventory (contents first, like the two drop streams).
        let items: Vec<ItemStack> = record
            .contents
            .drain(..)
            .flatten()
            .chain(record.armor.drain(..).flatten())
            .collect();
        if let Some(killer) = &killer {
            for stack in items {
                crate::handlers::block::give_stack_to_inventory(killer, stack);
            }
        }

        let date = crate::utils::datetime::now_from_config();
        let reason: String;
        let entry: String;
        let mut faction_messages: Vec<(String, String)> = Vec::new();
        let mut killer_streak_reward: Option<(String, i32, Vec<String>)> = None;

        {
            let mut guard = state().write().unwrap();
            let config = guard.config.clone();

            // `DeathMessageHandler#getPlayerName` / `getKillerName`, built
            // before the kill is credited.
            if let Some(killer_uuid) = &killer_uuid {
                let victim_display = player_display(&guard, &uuid, &record.name);
                let killer_display = killer_display(
                    &guard,
                    killer_uuid,
                    killer_name.as_deref().unwrap_or_default(),
                );
                reason = utils::replace_all(
                    &guard
                        .raw
                        .language
                        .string("DEATHMESSAGE.REASON_COMBATLOGGER_KILLER"),
                    &[("<player>", &victim_display), ("<killer>", &killer_display)],
                );
            } else {
                let victim_display = player_display(&guard, &uuid, &record.name);
                reason = utils::replace(
                    &guard.raw.language.string("DEATHMESSAGE.REASON_COMBATLOGGER"),
                    "<player>",
                    &victim_display,
                );
            }

            // `Tasks.async`: the victim's offline userdata. `updateDeathStats`
            // (= addDeath + addLastDeath) plus the silent killstreak reset.
            entry = format!("{date} - {}", reason.replace('§', "&"));

            if let Some(user) = guard.userdata.get_mut(&uuid) {
                user.add_death(entry.clone());
                user.reset_killstreak();
                // `die()` clears the offline inventory; the wipe is applied on
                // the next join (offline data is not writable).
                user.pending_logger_clear = true;
            }

            if let Some(killer_uuid) = &killer_uuid {
                // `updateKillStats(reason)`.
                if let Some(user) = guard.userdata.get_mut(killer_uuid) {
                    user.add_kill(entry.clone());
                }

                // `KillstreakHandler#checkKillerKillstreak`.
                if config.kitmap.enabled && config.kitmap.killstreak_enabled {
                    if let Some(user) = guard.userdata.get_mut(killer_uuid) {
                        user.add_killstreak();
                        let streak = user.killstreak;
                        if let Some(commands) = config.kitmap.killstreak_rewards.get(&streak) {
                            killer_streak_reward = Some((
                                killer_name.clone().unwrap_or_default(),
                                streak,
                                commands.clone(),
                            ));
                        }
                    }
                }

                // `killerFaction.incrementPoints(Config.FACTION_TOP_KILL)`: a
                // logger kill does not count towards the faction kill counter.
                if let Some(faction_id) = guard
                    .factions
                    .by_member(killer_uuid)
                    .map(|faction| faction.id.clone())
                {
                    if let Some(faction) = guard.factions.get_mut(&faction_id) {
                        faction.increment_points(
                            config.factions.points_per_kill as i32,
                            config.factions.allow_negative_points,
                        );
                    }
                }
            }

            // `faction.onDeath(this.player)`.
            if let Some((faction_id, message)) =
                crate::handlers::death::faction_on_death(&mut guard, &uuid, &record.name, &record.world, &config)
            {
                faction_messages.push((faction_id, message));
            }
        }

        for (faction_id, message) in faction_messages {
            let faction = state().read().unwrap().factions.get(&faction_id).cloned();
            if let Some(faction) = faction {
                core::faction_message(&server, &faction, &message);
            }
        }

        // Kitmap killstreak rewards (host calls outside the state lock).
        if let Some((killer, streak, commands)) = killer_streak_reward {
            for command in &commands {
                server.execute_command(
                    &utils::replace(command, "<player>", &killer),
                    pumpkin_plugin_api::server::CommandSender::Console,
                );
            }

            let template = state()
                .read()
                .unwrap()
                .raw
                .language
                .string("KITMAP_KILLSTREAK.MESSAGE");
            let message =
                utils::replace_all(&template, &[("<player>", &killer), ("<amount>", &streak.to_string())]);
            crate::handlers::timer::sotw::broadcast(&server, &message);
        }

        // Delivery (port of `Bukkit.getOnlinePlayers().forEach`): with a killer
        // only players with death messages enabled receive it — the killer
        // always does; without one the message reaches everyone.
        if let Some(killer_uuid) = &killer_uuid {
            crate::handlers::death::send_death_message(&server, &uuid, Some(killer_uuid), &reason);
        } else {
            for player in server.get_all_players() {
                core::send(&player, &reason);
            }
        }

        // `DeathbanManager#deathbanPlayer(this.player, loggerLocation,
        // this.deathban, reason)`.
        crate::deathban::apply_logger_death(
            &uuid,
            &record.world,
            death_position.0.floor() as i32,
            death_position.1.floor() as i32,
            death_position.2.floor() as i32,
            record.deathban_millis,
            &reason,
        );

        event
    }
}

// ---------------------------------------------------------------------------
// Vanilla drop suppression
// ---------------------------------------------------------------------------

/// Live markers for the vanilla drops of a dead logger: `(world, x, y, z,
/// expires_at_millis)`. The host computes `drop_loot`/`drop_equipment` and the
/// experience orbs in the same call stack as `EntityDeathEvent`, right after
/// the handlers return, so a marker only has to outlive the event call.
static VANILLA_DROP_MARKERS: Mutex<Vec<(String, f64, f64, f64, i64)>> = Mutex::new(Vec::new());

/// Grace period of a marker; the spawns it must catch happen immediately after
/// the death event, this is pure scheduling margin.
const VANILLA_DROP_MARKER_MILLIS: i64 = 500;

fn mark_vanilla_drop(world: &str, position: (f64, f64, f64)) {
    let now = utils::now_millis();
    let mut markers = VANILLA_DROP_MARKERS.lock().unwrap();
    markers.retain(|(_, _, _, _, expires)| *expires > now);
    markers.push((
        world.to_string(),
        position.0,
        position.1,
        position.2,
        now + VANILLA_DROP_MARKER_MILLIS,
    ));
}

/// Whether a spawn at `position` belongs to a dead logger's vanilla drops.
/// `ItemSpawnEvent` carries no world (the item entity does not exist yet), so
/// the item check passes `None`; the float coordinates make a cross-world
/// collision of a live marker impossible in practice.
fn is_vanilla_logger_drop(world: Option<&str>, position: (f64, f64, f64)) -> bool {
    let now = utils::now_millis();
    let mut markers = VANILLA_DROP_MARKERS.lock().unwrap();
    markers.retain(|(_, _, _, _, expires)| *expires > now);

    markers.iter().any(|(marker_world, x, y, z, _)| {
        if let Some(world) = world
            && world != marker_world
        {
            return false;
        }

        let dx = position.0 - x;
        let dy = position.1 - y;
        let dz = position.2 - z;
        // 1.5-block radius: `dropItemNaturally` jitters a stack around its
        // block while the experience orbs spawn on the entity itself.
        dx * dx + dy * dy + dz * dz <= 2.25
    })
}

/// `EntityLiving#dropDeathLoot` was an empty override in the NMS logger, so
/// the loot table of the underlying mob is swallowed (skeletons drop bones and
/// arrows, villagers drop nothing).
pub struct CombatLoggerLootListener;

impl EventHandler<ItemSpawnEvent> for CombatLoggerLootListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<ItemSpawnEvent>,
    ) -> EventData<ItemSpawnEvent> {
        if is_skeleton_loot(&event.item_name) && is_vanilla_logger_drop(None, event.position) {
            event.cancelled = true;
        }
        event
    }
}

fn is_skeleton_loot(item_name: &str) -> bool {
    let name = item_name.strip_prefix("minecraft:").unwrap_or(item_name);
    matches!(name, "bone" | "arrow")
}

/// The original entity's `expToDrop` was 0, so the `get_experience_reward`
/// hand-out of the host's death flow is swallowed too.
pub struct CombatLoggerExpListener;

impl EventHandler<EntitySpawnEvent> for CombatLoggerExpListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<EntitySpawnEvent>,
    ) -> EventData<EntitySpawnEvent> {
        if core::entity_type_name(&event.entity_type) == Some("experience_orb")
            && is_vanilla_logger_drop(Some(&event.target_world.get_name()), event.position)
        {
            event.cancelled = true;
        }
        event
    }
}

// ---------------------------------------------------------------------------
// Command support
// ---------------------------------------------------------------------------

/// Starts the `/logout` warmup (`LogoutTimer#activate`).
pub fn start_logout(player: &Player, seconds: i64) {
    let uuid = core::uuid_of(player);
    let location = core::location(player);

    let mut guard = state().write().unwrap();
    guard.timers.clear_pending(&uuid);
    guard.timers.set_pending(
        &uuid,
        PendingAction {
            timer_type: TimerType::Logout,
            expiry: utils::now_millis() + seconds.max(1) * 1000,
            origin_world: location.world,
            origin_x: location.x,
            origin_y: location.y,
            origin_z: location.z,
            target: None,
        },
    );
}

/// Completion of the logout warmup (`LogoutTimer#activate`'s `Tasks.sync`):
/// flags the quit as safe and kicks the player.
pub fn finish_logout(player: &Player) {
    let uuid = core::uuid_of(player);

    let message = {
        let mut guard = state().write().unwrap();
        guard.safe_logouts.insert(uuid);
        guard.raw.language.string("LOGOUT.KICK_MESSAGE")
    };

    if let Some(java) = player.as_java() {
        java.kick(pumpkin_plugin_api::player::JavaKickOptions::new(utils::text(
            &message,
        )));
    }
}

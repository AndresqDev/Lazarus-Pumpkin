//! Event listener registration.

pub mod block;
pub mod chat;
pub mod combat;
pub mod command;
pub mod entity;
pub mod interact;
pub mod items;
pub mod kitmap;
pub mod player;

use pumpkin_plugin_api::events::EventPriority;
use pumpkin_plugin_api::Context;

use crate::blockhandlers;
use crate::combatlogger;
use crate::death;
use crate::elevator;
use crate::limiters;

/// Registers every ported event handler.
pub fn register(context: &Context) -> Result<(), String> {
    context.register_event_handler(player::JoinListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::LeaveListener, EventPriority::Normal, true)?;
    // Combat logger quit handling (`CombatLoggerHandler#onPlayerQuit` ran at
    // LOWEST in the original): after `LeaveListener` so the banked state is
    // already stored when the logger is built.
    context.register_event_handler(
        combatlogger::CombatLoggerQuitListener,
        EventPriority::Lowest,
        true,
    )?;
    context.register_event_handler(player::MoveListener, EventPriority::Normal, true)?;

    // Death handlers (`handlers.death.*`) that react to the death itself: they
    // run *before* `DeathListener` so the killer is still resolvable from
    // `last_damagers` (the DeathListener clears it while crediting the stats).
    context.register_event_handler(death::EntityNameListener, EventPriority::Normal, true)?;
    context.register_event_handler(death::StattrakListener, EventPriority::Normal, true)?;
    context.register_event_handler(death::HeadDropDeathListener, EventPriority::Normal, true)?;
    context.register_event_handler(death::DeathSignDeathListener, EventPriority::Normal, true)?;

    context.register_event_handler(player::DeathListener, EventPriority::Normal, true)?;
    // Combat logger entity deaths (`VillagerCombatLogger_1_8#die`).
    context.register_event_handler(combatlogger::CombatLoggerDeathListener, EventPriority::Normal, true)?;
    // Vanilla drops of a dead logger (`dropDeathLoot` was empty and
    // `expToDrop` stayed 0 on the original entity).
    context.register_event_handler(combatlogger::CombatLoggerLootListener, EventPriority::Normal, true)?;
    context.register_event_handler(combatlogger::CombatLoggerExpListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::RespawnListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::SpawnLocationListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::SpawnChangeListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::TeleportListener, EventPriority::Normal, true)?;

    context.register_event_handler(chat::ChatListener, EventPriority::Normal, true)?;
    context.register_event_handler(kitmap::DropItemListener, EventPriority::Normal, true)?;

    // Combat logger damage rules: registered before the shared combat
    // listeners so the by-entity handler consumes the tracked projectile
    // shooter first (the combat listener would otherwise take it).
    context.register_event_handler(
        combatlogger::CombatLoggerDamageByEntityListener,
        EventPriority::High,
        true,
    )?;
    context.register_event_handler(combatlogger::CombatLoggerDamageListener, EventPriority::High, true)?;

    context.register_event_handler(combat::EntityDamageByEntityListener, EventPriority::High, true)?;
    context.register_event_handler(combat::EntityDamageListener, EventPriority::High, true)?;
    context.register_event_handler(combat::FoodLevelChangeListener, EventPriority::High, true)?;
    context.register_event_handler(combat::ProjectileLaunchListener, EventPriority::Normal, true)?;
    context.register_event_handler(combat::ProjectileHitListener, EventPriority::Normal, true)?;
    context.register_event_handler(combat::ShootBowListener, EventPriority::Normal, true)?;
    context.register_event_handler(combat::MilkListener, EventPriority::Normal, true)?;

    context.register_event_handler(entity::EntityTargetListener, EventPriority::High, true)?;
    context.register_event_handler(entity::CreatureSpawnListener, EventPriority::High, true)?;
    context.register_event_handler(entity::PotionSplashListener, EventPriority::High, true)?;

    context.register_event_handler(block::BlockBreakListener, EventPriority::High, true)?;
    // Disabled block placement used LOWEST priority in the original: it runs
    // before the territory listener so the build denial is skipped
    // (`ignoreCancelled`) and the player only sees this message.
    context.register_event_handler(blockhandlers::DisabledPlacementListener, EventPriority::Normal, true)?;
    context.register_event_handler(block::BlockPlaceListener, EventPriority::High, true)?;
    // Crowbar spawner restores: the extracted spawner's `block_entity_data`
    // must be queued before the block lands (see `CrowbarSpawnerPlaceListener`)
    // and only for placements the territory listener allows.
    context.register_event_handler(blockhandlers::CrowbarSpawnerPlaceListener, EventPriority::High, true)?;
    // Block gameplay handlers (`handlers.block.*`), all `ignoreCancelled` in the
    // original: they run after the territory listeners and skip denied actions.
    context.register_event_handler(blockhandlers::AutoSmeltBreakListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::AutoSmeltDropListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::CobwebBreakListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::CobwebPlaceListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::FoundOrePlaceListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::FoundOreBreakListener, EventPriority::Normal, true)?;
    // Subclaims (`SubclaimHandler`): creation signs, chest opens, chest/sign
    // breaks and hopper pulls out of subclaimed chests.
    context.register_event_handler(blockhandlers::SubclaimSignChangeListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::SubclaimInteractListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::SubclaimBreakListener, EventPriority::Normal, true)?;
    context.register_event_handler(blockhandlers::SubclaimHopperListener, EventPriority::Normal, true)?;
    // Use protection must run before the wand listener (see `BlockInteractListener`).
    context.register_event_handler(block::BlockInteractListener, EventPriority::High, true)?;
    // Crowbar (`CrowbarHandler#onPlayerInteract`, `ignoreCancelled`): after the
    // territory listener, denied uses must not reach it.
    context.register_event_handler(blockhandlers::CrowbarInteractListener, EventPriority::High, true)?;
    context.register_event_handler(block::BucketEmptyListener, EventPriority::High, true)?;
    context.register_event_handler(block::BucketFillListener, EventPriority::High, true)?;
    context.register_event_handler(block::EntityExplodeListener, EventPriority::High, true)?;
    context.register_event_handler(block::BlockExplodeListener, EventPriority::High, true)?;
    context.register_event_handler(block::BlockFromToListner, EventPriority::High, true)?;
    context.register_event_handler(block::BlockIgniteListener, EventPriority::High, true)?;
    context.register_event_handler(block::PistonExtendListener, EventPriority::High, true)?;
    context.register_event_handler(block::PistonRetractListener, EventPriority::High, true)?;

    context.register_event_handler(command::CommandPreprocessListener, EventPriority::Lowest, true)?;

    context.register_event_handler(interact::InteractListener, EventPriority::High, true)?;
    context.register_event_handler(interact::SneakListener, EventPriority::Normal, true)?;

    // Limiters (`handlers.limiter.*`): enchantment table + anvil result,
    // potion drinking/throwing/brewing and disabled entity spawns. They run
    // after the interact group and before the elevators, like the original's
    // `HandlerManager` (EnchantLimiter < EntityLimiter < MinecartElevator).
    context.register_event_handler(limiters::EnchantTableListener, EventPriority::Normal, true)?;
    context.register_event_handler(limiters::AnvilClickListener, EventPriority::Normal, true)?;
    context.register_event_handler(limiters::PotionConsumeListener, EventPriority::Low, true)?;
    context.register_event_handler(limiters::BrewListener, EventPriority::Normal, true)?;
    context.register_event_handler(limiters::EntitySpawnLimiterListener, EventPriority::Lowest, true)?;

    // Elevators (`handlers.elevator.*`, `ignoreCancelled` in the original):
    // they run after the territory/use listeners above, so denied actions are
    // skipped.
    context.register_event_handler(elevator::SignElevatorBreakListener, EventPriority::High, true)?;
    context.register_event_handler(elevator::SignElevatorInteractListener, EventPriority::High, true)?;
    context.register_event_handler(elevator::SignElevatorSignChangeListener, EventPriority::Normal, true)?;
    context.register_event_handler(elevator::MinecartElevatorEnterListener, EventPriority::Normal, true)?;
    context.register_event_handler(elevator::MinecartElevatorExitListener, EventPriority::Normal, true)?;

    // Death handlers reacting to blocks (`ignoreCancelled` in the original):
    // they run after the territory listeners, and the head/sign placements
    // after `BlockPlaceListener` so denied placements are not tracked.
    context.register_event_handler(death::HeadTrackPlaceListener, EventPriority::High, true)?;
    context.register_event_handler(death::HeadDropBreakListener, EventPriority::High, true)?;
    context.register_event_handler(death::DeathSignPlaceListener, EventPriority::High, true)?;
    context.register_event_handler(death::DeathSignBreakListener, EventPriority::High, true)?;
    context.register_event_handler(death::HeadDropInteractListener, EventPriority::High, true)?;
    context.register_event_handler(death::DeathSignSignChangeListener, EventPriority::Normal, true)?;

    context.register_event_handler(items::EnderPearlListener, EventPriority::Normal, true)?;
    context.register_event_handler(items::GoldenAppleListener, EventPriority::Normal, true)?;

    Ok(())
}

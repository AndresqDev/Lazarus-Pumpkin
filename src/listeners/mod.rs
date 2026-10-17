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

/// Registers every ported event handler.
pub fn register(context: &Context) -> Result<(), String> {
    context.register_event_handler(player::JoinListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::LeaveListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::MoveListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::DeathListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::RespawnListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::SpawnLocationListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::SpawnChangeListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::TeleportListener, EventPriority::Normal, true)?;

    context.register_event_handler(chat::ChatListener, EventPriority::Normal, true)?;
    context.register_event_handler(kitmap::DropItemListener, EventPriority::Normal, true)?;

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

    context.register_event_handler(items::EnderPearlListener, EventPriority::Normal, true)?;
    context.register_event_handler(items::GoldenAppleListener, EventPriority::Normal, true)?;

    Ok(())
}

//! Event listener registration.

pub mod block;
pub mod chat;
pub mod combat;
pub mod command;
pub mod entity;
pub mod interact;
pub mod items;
pub mod player;

use pumpkin_plugin_api::events::EventPriority;
use pumpkin_plugin_api::Context;

/// Registers every ported event handler.
pub fn register(context: &Context) -> Result<(), String> {
    context.register_event_handler(player::JoinListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::LeaveListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::MoveListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::DeathListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::RespawnListener, EventPriority::Normal, true)?;
    context.register_event_handler(player::TeleportListener, EventPriority::Normal, true)?;

    context.register_event_handler(chat::ChatListener, EventPriority::Normal, true)?;

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
    context.register_event_handler(block::BlockPlaceListener, EventPriority::High, true)?;
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

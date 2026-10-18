//! Consumable/usage cooldowns — port of `handlers.timer.EnderPearlHandler` and
//! `handlers.timer.GoldenAppleHandler`.
//!
//! Cooldowns are stored in the shared [`crate::timer::TimerManager`] and shown on
//! the sidebar. Enderpearls are detected on right-click (the API has no dedicated
//! enderpearl event), golden apples on item consume.

use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::events::{
    EventData, EventHandler, InteractAction, PlayerInteractEvent, PlayerItemConsumeEvent,
};
use pumpkin_plugin_api::Server;

use crate::utils::core;
use crate::state::state;
use crate::timer::TimerType;
use crate::utils;

const ENDER_PEARL: &str = "minecraft:ender_pearl";

pub struct EnderPearlListener;

impl EventHandler<PlayerInteractEvent> for EnderPearlListener {
    fn handle(&self, _server: Server, mut event: EventData<PlayerInteractEvent>) -> EventData<PlayerInteractEvent> {
        // `useInteractedBlock() == DENY && useItemInHand() == DENY`: an interact
        // already denied (block protection, wand) must not arm the pearl timer.
        if event.cancelled {
            return event;
        }

        if !matches!(event.action, InteractAction::RightClickBlock | InteractAction::RightClickAir) {
            return event;
        }

        let Some(item) = event.player.get_item_in_hand(Hand::Right) else {
            return event;
        };
        if item.get_registry_key() != ENDER_PEARL {
            return event;
        }

        let uuid = core::uuid_of(&event.player);
        let (remaining, seconds, message_template) = {
            let guard = state().read().unwrap();
            (
                guard.timers.remaining(&uuid, TimerType::Enderpearl),
                guard.config.timers.enderpearl_seconds,
                guard.lang.enderpearl_cooldown.clone(),
            )
        };

        if remaining > 0 {
            event.cancelled = true;
            let message = utils::replace(&message_template, "<time>", &utils::format_duration(remaining));
            core::send(&event.player, message);
        } else {
            state().write().unwrap().timers.activate(&uuid, TimerType::Enderpearl, seconds);
        }

        event
    }
}

pub struct GoldenAppleListener;

impl EventHandler<PlayerItemConsumeEvent> for GoldenAppleListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerItemConsumeEvent>,
    ) -> EventData<PlayerItemConsumeEvent> {
        let name = event.item_name.to_lowercase();
        let uuid = core::uuid_of(&event.player);

        let (timer_type, seconds, message_template) = {
            let guard = state().read().unwrap();
            if name.contains("enchanted_golden_apple") {
                (
                    TimerType::Gapple,
                    guard.config.timers.gapple_enchanted_seconds,
                    guard.lang.gapple_cooldown.clone(),
                )
            } else if name.contains("golden_apple") {
                (
                    TimerType::Apple,
                    guard.config.timers.gapple_seconds,
                    guard.lang.gapple_cooldown.clone(),
                )
            } else {
                return event;
            }
        };

        let remaining = state().read().unwrap().timers.remaining(&uuid, timer_type);
        if remaining > 0 {
            event.cancelled = true;
            let message = utils::replace(&message_template, "<time>", &utils::format_duration(remaining));
            core::send(&event.player, message);
        } else {
            state().write().unwrap().timers.activate(&uuid, timer_type, seconds);
        }

        event
    }
}

//! Kitmap listeners — port of `me.qiooip.lazarus.handlers.kitmap.KitmapHandler`.
//!
//! The original only registers this handler when kitmap mode is on; the port
//! registers it always and checks the config flags (the `KillstreakHandler`
//! style, which is registered unconditionally in the original too).

use pumpkin_plugin_api::events::{EventData, EventHandler, PlayerDropItemEvent};
use pumpkin_plugin_api::Server;

use crate::core;
use crate::state::state;

/// `KitmapHandler#onPlayerDropItem`: drops are denied inside the safezone while
/// kitmap mode is on (the `ignoreCancelled = true` of the original is honoured
/// by skipping events another handler already cancelled).
pub struct DropItemListener;

impl EventHandler<PlayerDropItemEvent> for DropItemListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerDropItemEvent>,
    ) -> EventData<PlayerDropItemEvent> {
        if event.cancelled {
            return event;
        }

        let message = {
            let guard = state().read().unwrap();
            if !(guard.config.kitmap.enabled && guard.config.kitmap.disable_item_drop_in_safezone) {
                return event;
            }

            let location = core::location(&event.player);
            let territory = core::territory_at_guard(
                &guard,
                &location.world,
                location.block_x(),
                location.block_z(),
            );
            if !matches!(territory, core::Territory::Safezone) {
                return event;
            }

            // `Language.FACTION_PREFIX + Language.KITMAP_DENY_ITEM_DROP`.
            format!(
                "{}{}",
                guard.raw.language.string("FACTION_PREFIX"),
                guard.raw.language.string("KITMAP_DENY_ITEM_DROP"),
            )
        };

        event.cancelled = true;
        core::send(&event.player, &message);

        event
    }
}

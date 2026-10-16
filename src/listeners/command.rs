//! Command pre-processing — port of `handlers.chat.BlockedCommandsHandler` and the
//! combat-tag / pvp-protection command restrictions.

use pumpkin_plugin_api::events::{EventData, EventHandler, PlayerCommandPreprocessEvent};
use pumpkin_plugin_api::Server;

use crate::core;
use crate::state::state;
use crate::timers::TimerType;

pub struct CommandPreprocessListener;

impl EventHandler<PlayerCommandPreprocessEvent> for CommandPreprocessListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerCommandPreprocessEvent>,
    ) -> EventData<PlayerCommandPreprocessEvent> {
        let uuid = core::uuid_of(&event.player);

        let first = event
            .command
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_lowercase();

        let (blocked, combat) = {
            let guard = state().read().unwrap();
            (
                guard
                    .config
                    .blocked_commands
                    .iter()
                    .any(|entry| entry.trim_start_matches('/').eq_ignore_ascii_case(&first)),
                guard.timers.is_active(&uuid, TimerType::CombatTag),
            )
        };

        if blocked {
            event.cancelled = true;
            core::send(&event.player, "&cThat command is disabled.");
            return event;
        }

        if combat && matches!(first.as_str(), "spawn" | "f" | "faction") {
            // Only block teleport-ish subcommands during combat tag.
            let lower = event.command.to_lowercase();
            if lower.contains(" home") || lower.contains(" stuck") || first == "spawn" {
                event.cancelled = true;
                core::send(&event.player, "&cYou cannot use that command while combat tagged.");
            }
        }

        event
    }
}

//! `/endportal` command — port of `commands/staff/EndPortalCommand`.
//!
//! Hands out the `SELECTION` wand (the original's `SelectionType.SELECTION`,
//! which shares its wand with `SYSTEM_CLAIM`) and always restarts the selection
//! process: `toggleSelectionProcess` removes the previous one first and the
//! confirmation message is unconditional.

use pumpkin_plugin_api::command::{CommandError, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_op_permission, reply};
use crate::state::state;

/// `EndPortalCommand` had no permission node declared in the original's
/// `plugin.yml`, which makes Bukkit treat it as operator-only.
pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.endportal", "Get the end portal selector")?;

    let root = command(&["endportal"], "End portal selector").execute(EndPortalExecutor);

    context.register_command(root, "lazarus:command.endportal");
    Ok(())
}

struct EndPortalExecutor;

impl CommandHandler for EndPortalExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        // `toggleSelectionProcess(player, SELECTION, createSelectionCheck())`.
        state().write().unwrap().selection.start_selection(&player);

        let (prefix, message) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard
                    .raw
                    .language
                    .string("ENDPORTAL.SELECTOR_ADDED_TO_INVENTORY"),
            )
        };

        reply(&sender, &format!("{prefix}{message}"));
        Ok(1)
    }
}

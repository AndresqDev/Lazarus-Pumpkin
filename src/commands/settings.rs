//! `/settings` — port of `commands/SettingsCommand`.
//!
//! The original opened the inventory the `SettingsHandler` had cached for the
//! player on join; the port builds it here (the GUI module keeps the deviation
//! documented) so the lore always reflects the stored settings.

use pumpkin_plugin_api::command::{CommandError, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply};
use crate::state::state;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.settings", "Open the settings GUI")?;

    let root = command(&["settings"], "Open the settings GUI").execute(SettingsExecutor);
    context.register_command(root, "lazarus:command.settings");
    Ok(())
}

struct SettingsExecutor;

impl CommandHandler for SettingsExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        crate::handlers::settings::open(&player);
        Ok(1)
    }
}

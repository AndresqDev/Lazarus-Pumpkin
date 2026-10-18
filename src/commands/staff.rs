//! `/vanish` and `/staffmode` — port of `staffmode.VanishManager` / `StaffModeManager`.

use pumpkin_plugin_api::command::{CommandError, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_op_permission, reply};
use crate::utils::core;
use crate::state::state;

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.vanish", "Toggle vanish")?;
    register_op_permission(context, "lazarus:command.staffmode", "Toggle staff mode")?;

    context.register_command(
        command(&["vanish", "v"], "Toggle vanish").execute(VanishExecutor),
        "lazarus:command.vanish",
    );
    context.register_command(
        command(&["staffmode", "staff"], "Toggle staff mode").execute(StaffExecutor),
        "lazarus:command.staffmode",
    );

    Ok(())
}

struct VanishExecutor;

impl CommandHandler for VanishExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            reply(&sender, "&cPlayers only.");
            return Ok(0);
        };

        let now_vanished = {
            let mut guard = state().write().unwrap();
            guard.staff.toggle_vanish(&server, &player)
        };

        let message = {
            let guard = state().read().unwrap();
            if now_vanished {
                guard.lang.vanish_enabled.clone()
            } else {
                guard.lang.vanish_disabled.clone()
            }
        };

        reply(&sender, message);
        Ok(1)
    }
}

struct StaffExecutor;

impl CommandHandler for StaffExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            reply(&sender, "&cPlayers only.");
            return Ok(0);
        };
        let uuid = core::uuid_of(&player);

        let enabled = {
            let mut guard = state().write().unwrap();
            if guard.staff.staff.remove(&uuid) {
                false
            } else {
                guard.staff.staff.insert(uuid.clone());
                true
            }
        };

        let message = {
            let guard = state().read().unwrap();
            if enabled {
                guard.lang.staffmode_enabled.clone()
            } else {
                guard.lang.staffmode_disabled.clone()
            }
        };

        reply(&sender, message);
        Ok(1)
    }
}

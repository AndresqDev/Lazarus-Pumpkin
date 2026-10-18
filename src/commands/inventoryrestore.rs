//! `/inventoryrestore [invrestore, lastinv, invrollback] <player>` — port of
//! `commands/staff/InventoryRestoreCommand`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, present_string_arg, register_op_permission, reply};
use crate::state::state;
use crate::utils;

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(
        context,
        "lazarus:command.inventoryrestore",
        "Preview and roll back a death inventory",
    )?;

    let root = command(
        &["inventoryrestore", "invrestore", "lastinv", "invrollback"],
        "Preview and roll back a death inventory",
    )
    .execute(UsageExecutor)
    .then(
        CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
            .execute(RestoreExecutor),
    );

    context.register_command(root, "lazarus:command.inventoryrestore");
    Ok(())
}

/// `Language.PREFIX + <message>`.
fn prefixed(key: &str) -> String {
    let guard = state().read().unwrap();

    format!(
        "{}{}",
        guard.raw.language.string("PREFIX"),
        guard.raw.language.string(key)
    )
}

struct UsageExecutor;

impl CommandHandler for UsageExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if sender.as_player().is_none() {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        }

        reply(&sender, prefixed("INVENTORY_RESTORE_COMMAND.USAGE"));
        Ok(1)
    }
}

struct RestoreExecutor;

impl CommandHandler for RestoreExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        let Some(name) = present_string_arg(&args, "player") else {
            reply(&sender, prefixed("INVENTORY_RESTORE_COMMAND.USAGE"));
            return Ok(1);
        };

        // `Bukkit.getPlayer(args[0])` + `checkPlayer`: online only.
        let Some(target) = server.get_player_by_name(&name) else {
            let message = utils::replace(
                &prefixed("COMMANDS.PLAYER_NOT_ONLINE"),
                "<player>",
                &name,
            );
            reply(&sender, &message);
            return Ok(1);
        };

        crate::handlers::restore::open_restore_inventory(&player, &target);
        Ok(1)
    }
}

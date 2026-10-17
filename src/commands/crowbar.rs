//! `/crowbar` command — port of `commands/staff/CrowbarCommand`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_op_permission, reply, string_arg};
use crate::blockhandlers;
use crate::core;
use crate::state::state;
use crate::util;

enum Action {
    Usage,
    Give,
}

struct CrowbarExecutor(Action);

impl CommandHandler for CrowbarExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        match self.0 {
            Action::Usage => usage(&sender),
            Action::Give => give(&sender, &server, &args),
        }

        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.crowbar", "Give a crowbar")?;

    let root = command(&["crowbar"], "Crowbar command")
        .execute(CrowbarExecutor(Action::Usage))
        .then(
            CommandNode::literal("give").then(
                CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                    .execute(CrowbarExecutor(Action::Give)),
            ),
        );

    context.register_command(root, "lazarus:command.crowbar");
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

fn usage(sender: &CommandSender) {
    reply(sender, prefixed("CROWBAR.COMMAND_USAGE"));
}

fn give(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "player") else {
        return;
    };

    let Some(target) = server.get_player_by_name(&name) else {
        reply(
            sender,
            util::replace(
                &state().read().unwrap().lang.player_not_online,
                "<player>",
                &name,
            ),
        );
        return;
    };

    // `target.getInventory().addItem(...)`; leftovers are lost when the
    // inventory is full (the original never inspected them either).
    let _ = blockhandlers::give_stack_to_inventory(&target, blockhandlers::new_crowbar());

    reply(
        sender,
        util::replace(&prefixed("CROWBAR.GAVE"), "<player>", &core::name_of(&target)),
    );
    core::send(&target, prefixed("CROWBAR.RECEIVED"));
}

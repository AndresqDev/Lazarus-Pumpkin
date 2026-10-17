//! `/lastdeaths` and `/lastkills` — port of `commands/staff/LastDeathsCommand`
//! and `commands/staff/LastKillsCommand`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, present_string_arg, register_op_permission, reply};
use crate::core;
use crate::state::state;
use crate::util;

#[derive(Clone, Copy)]
enum Kind {
    Deaths,
    Kills,
}

struct LastStatsExecutor(Kind);

impl CommandHandler for LastStatsExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        last_stats(&sender, &server, &args, self.0);
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.lastdeaths", "Show a player's last deaths")?;
    register_op_permission(context, "lazarus:command.lastkills", "Show a player's last kills")?;

    context.register_command(
        command(&["lastdeaths"], "Show a player's last deaths").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(LastStatsExecutor(Kind::Deaths)),
        ),
        "lazarus:command.lastdeaths",
    );
    context.register_command(
        command(&["lastkills"], "Show a player's last kills").then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(LastStatsExecutor(Kind::Kills)),
        ),
        "lazarus:command.lastkills",
    );

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

fn raw(key: &str) -> String {
    state().read().unwrap().raw.language.string(key)
}

fn last_stats(sender: &CommandSender, server: &Server, args: &ConsumedArgs, kind: Kind) {
    let (command, list, header, line, empty, placeholder) = match kind {
        Kind::Deaths => (
            "LAST_DEATHS_COMMAND.USAGE",
            true,
            "LAST_DEATHS_COMMAND.HEADER_FORMAT",
            "LAST_DEATHS_COMMAND.DEATH_MESSAGE_FORMAT",
            "LAST_DEATHS_COMMAND.NO_DEATHS_YET",
            "<deathMessage>",
        ),
        Kind::Kills => (
            "LAST_KILLS_COMMAND.USAGE",
            false,
            "LAST_KILLS_COMMAND.HEADER_FORMAT",
            "LAST_KILLS_COMMAND.KILL_MESSAGE_FORMAT",
            "LAST_KILLS_COMMAND.NO_KILLS_YET",
            "<killMessage>",
        ),
    };

    let Some(name) = present_string_arg(args, "player") else {
        reply(sender, &prefixed(command));
        return;
    };

    let Some(target) = server.get_player_by_name(&name) else {
        let message = state().read().unwrap().lang.player_not_online.clone();
        reply(sender, &util::replace(&message, "<player>", &name));
        return;
    };

    let uuid = core::uuid_of(&target);
    let (target_name, entries) = {
        let guard = state().read().unwrap();
        let user = guard.userdata.get(&uuid);
        let name = user
            .map(|user| user.name.clone())
            .unwrap_or_else(|| core::name_of(&target));
        let entries = user
            .map(|user| {
                if list {
                    user.last_deaths.clone()
                } else {
                    user.last_kills.clone()
                }
            })
            .unwrap_or_default();
        (name, entries)
    };

    if entries.is_empty() {
        let message = prefixed(empty);
        reply(sender, &util::replace(&message, "<player>", &target_name));
        return;
    }

    reply(sender, "");
    reply(sender, &util::replace(&raw(header), "<player>", &target_name));
    for entry in entries {
        reply(sender, &util::replace(&raw(line), placeholder, &entry));
    }
    reply(sender, "");
}

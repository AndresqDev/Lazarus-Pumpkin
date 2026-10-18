//! `/rankrevive <player>` — port of `commands/RankReviveCommand`.
//!
//! The command itself has no permission in the original: the handler grants it
//! per rank (`RANK_REVIVE_COMMAND.<n>.PERMISSION`).

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, present_string_arg, register_public_permission, reply};
use crate::state::state;
use crate::utils::{self, core};

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.rankrevive", "Use a rank revive")?;

    let root = command(&["rankrevive"], "Use a rank revive")
        .execute(UsageExecutor)
        .then(
            CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord))
                .execute(ReviveExecutor),
        );

    context.register_command(root, "lazarus:command.rankrevive");
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

/// `BaseCommand#checkOfflinePlayer`: online players first, then players known
/// from userdata (`hasPlayedBefore`).
fn resolve_target(server: &Server, name: &str) -> Option<(String, String)> {
    if let Some(player) = server.get_player_by_name(name) {
        return Some((core::uuid_of(&player), core::name_of(&player)));
    }

    state()
        .read()
        .unwrap()
        .userdata
        .users
        .values()
        .find(|user| user.name.eq_ignore_ascii_case(name))
        .map(|user| (user.uuid.clone(), user.name.clone()))
}

struct UsageExecutor;

impl CommandHandler for UsageExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if sender.as_player().is_none() {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        }

        reply(&sender, prefixed("RANK_REVIVE_COMMAND.USAGE"));
        Ok(1)
    }
}

struct ReviveExecutor;

impl CommandHandler for ReviveExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        let Some(name) = present_string_arg(&args, "player") else {
            reply(&sender, prefixed("RANK_REVIVE_COMMAND.USAGE"));
            return Ok(1);
        };

        let Some((uuid, target_name)) = resolve_target(&server, &name) else {
            let message = state().read().unwrap().lang.player_not_found.clone();
            reply(&sender, utils::replace(&message, "<player>", &name));
            return Ok(1);
        };

        crate::handlers::rank::perform_rank_revive(&server, &player, &uuid, &target_name);
        Ok(1)
    }
}

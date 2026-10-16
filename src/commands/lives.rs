//! `/lives` command — port of `LivesCommand` / `LivesReviveCommand` / `LivesAddCommand`.

use pumpkin_plugin_api::command::{CommandError, CommandNode, ArgumentType, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, int_arg, register_op_permission, register_public_permission, reply, string_arg};
use crate::core;
use crate::state::state;
use crate::util;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.lives", "View your lives")?;
    register_op_permission(context, "lazarus:command.lives.admin", "Manage players' lives")?;

    let root = command(&["lives"], "Manage lives")
        .execute(LivesExecutor)
        .then(
            CommandNode::literal("add")
                .then(
                    CommandNode::argument("player", &ArgumentType::String(pumpkin_plugin_api::command::StringType::SingleWord))
                        .then(CommandNode::argument("amount", &ArgumentType::Integer((None, None))).execute(LivesAddExecutor)),
                ),
        )
        .then(
            CommandNode::literal("set")
                .then(
                    CommandNode::argument("player", &ArgumentType::String(pumpkin_plugin_api::command::StringType::SingleWord))
                        .then(CommandNode::argument("amount", &ArgumentType::Integer((None, None))).execute(LivesSetExecutor)),
                ),
        )
        .then(
            CommandNode::literal("revive")
                .then(CommandNode::argument("player", &ArgumentType::String(pumpkin_plugin_api::command::StringType::SingleWord)).execute(LivesReviveExecutor)),
        );

    context.register_command(root, "lazarus:command.lives");
    Ok(())
}

struct LivesExecutor;

impl CommandHandler for LivesExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            reply(&sender, "&cPlayers only.");
            return Ok(0);
        };
        let uuid = core::uuid_of(&player);

        let message = {
            let guard = state().read().unwrap();
            let lives = guard.userdata.get(&uuid).map(|u| u.lives).unwrap_or(0);
            util::replace(&guard.lang.lives_message, "<lives>", &lives.to_string())
        };

        reply(&sender, message);
        Ok(1)
    }
}

struct LivesAddExecutor;

impl CommandHandler for LivesAddExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        change_lives(&sender, &server, &args, false)
    }
}

struct LivesSetExecutor;

impl CommandHandler for LivesSetExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        change_lives(&sender, &server, &args, true)
    }
}

fn change_lives(sender: &CommandSender, server: &Server, args: &ConsumedArgs, absolute: bool) -> Result<i32, CommandError> {
    if !allowed(sender, server, "lazarus:command.lives.admin") {
        return Ok(0);
    }

    let Some(name) = string_arg(args, "player") else {
        return Ok(0);
    };
    let Some(amount) = int_arg(args, "amount") else {
        reply(sender, "&cInvalid amount.");
        return Ok(0);
    };

    let Some(target) = server.get_player_by_name(&name) else {
        let guard = state().read().unwrap();
        reply(sender, util::replace(&guard.lang.player_not_online, "<player>", &name));
        return Ok(0);
    };
    let uuid = core::uuid_of(&target);

    let message = {
        let mut guard = state().write().unwrap();
        let Some(user) = guard.userdata.get_mut(&uuid) else {
            return Ok(0);
        };
        if absolute {
            user.lives = amount.max(0) as i32;
        } else {
            user.lives = (user.lives + amount as i32).max(0);
        }
        let new_lives = user.lives;

        if absolute {
            util::replace(
                &util::replace(&guard.lang.lives_set, "<amount>", &new_lives.to_string()),
                "<player>",
                &name,
            )
        } else {
            util::replace(
                &util::replace(&guard.lang.lives_added, "<amount>", &amount.to_string()),
                "<player>",
                &name,
            )
        }
    };

    reply(sender, message);
    Ok(1)
}

struct LivesReviveExecutor;

impl CommandHandler for LivesReviveExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if !allowed(&sender, &server, "lazarus:command.lives.admin") {
            return Ok(0);
        }

        let Some(name) = string_arg(&args, "player") else {
            return Ok(0);
        };
        let Some(target) = server.get_player_by_name(&name) else {
            reply(&sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
            return Ok(0);
        };
        let uuid = core::uuid_of(&target);

        let message = {
            let mut guard = state().write().unwrap();
            let was_banned = guard.userdata.get(&uuid).is_some_and(|u| u.is_deathbanned());
            if !was_banned {
                reply(&sender, &guard.lang.deathban_not_banned);
                return Ok(0);
            }
            if let Some(user) = guard.userdata.get_mut(&uuid) {
                user.revive();
            }
            util::replace(&guard.lang.deathban_revive_sender, "<player>", &name)
        };

        reply(&sender, message);

        let revived = {
            let guard = state().read().unwrap();
            util::replace(&guard.lang.deathban_revived, "<sender>", &sender.get_name())
        };
        core::send(&target, revived);
        Ok(1)
    }
}

/// Op check that also allows the console.
fn allowed(sender: &CommandSender, server: &Server, node: &str) -> bool {
    if sender.is_console() {
        return true;
    }
    if sender.has_permission(server, node) {
        return true;
    }
    reply(sender, "&cYou do not have permission.");
    false
}

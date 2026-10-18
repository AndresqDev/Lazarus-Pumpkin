//! `/deathban check|revive` — port of `commands/staff/DeathbanCommand`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, present_string_arg, register_op_permission, reply, sender_player};
use crate::utils::core;
use crate::state::state;
use crate::utils;

#[derive(Clone, Copy)]
enum Action {
    Usage,
    Check,
    Revive,
}

struct DeathbanExecutor(Action);

impl CommandHandler for DeathbanExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        match self.0 {
            Action::Usage => usage(&sender),
            Action::Check => check(&sender, &server, &args),
            Action::Revive => revive(&sender, &server, &args),
        }

        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.deathban", "Deathban management")?;
    // Node used by the revive broadcast (`Messages.sendMessage(…, "lazarus.staff")`).
    // The original relied on Bukkit's `isOp()` fallback for the undefined dot
    // node; the host only accepts namespaced registrations, so the op default
    // lives on the colon variant and both are checked when broadcasting.
    register_op_permission(context, "lazarus:staff", "Receive staff announcements")?;

    let target = || CommandNode::argument("player", &ArgumentType::String(StringType::SingleWord));

    let root = command(&["deathban"], "Deathban management")
        .execute(DeathbanExecutor(Action::Usage))
        .then(CommandNode::literal("check").then(target().execute(DeathbanExecutor(Action::Check))))
        .then(CommandNode::literal("revive").then(target().execute(DeathbanExecutor(Action::Revive))));

    context.register_command(root, "lazarus:command.deathban");
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
    let lines = state()
        .read()
        .unwrap()
        .raw
        .language
        .string_list("DEATHBAN.COMMAND_USAGE");

    for line in lines {
        reply(sender, &line);
    }
}

/// `Bukkit.getOfflinePlayer` + `BaseCommand#checkOfflinePlayer`: online players
/// first, then anyone stored in the userdata.
fn lookup(server: &Server, name: &str) -> Option<(String, String)> {
    if let Some(player) = server.get_player_by_name(name) {
        return Some((core::uuid_of(&player), core::name_of(&player)));
    }

    let guard = state().read().unwrap();
    guard
        .userdata
        .find_by_name(name)
        .map(|user| (user.uuid.clone(), user.name.clone()))
}

fn not_found(sender: &CommandSender, name: &str) {
    let message = state().read().unwrap().lang.player_not_found.clone();
    reply(sender, &utils::replace(&message, "<player>", name));
}

fn not_deathbanned(sender: &CommandSender, name: &str) {
    let message = prefixed("DEATHBAN.PLAYER_NOT_DEATHBANNED");
    reply(sender, &utils::replace(&message, "<player>", name));
}

/// `DeathbanManager#checkStatus`.
fn check(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(name) = present_string_arg(args, "player") else {
        usage(sender);
        return;
    };
    let Some((uuid, target_name)) = lookup(server, &name) else {
        not_found(sender, &name);
        return;
    };

    let info = {
        let guard = state().read().unwrap();
        guard
            .userdata
            .get(&uuid)
            .filter(|user| user.is_deathbanned())
            .map(|user| {
                (
                    user.deathban_remaining(),
                    user.deathban_reason(),
                    user.deathban_location(),
                )
            })
    };

    let Some((remaining, reason, location)) = info else {
        not_deathbanned(sender, &target_name);
        return;
    };

    let duration = if remaining <= 0 {
        "Unbanned".to_string()
    } else {
        utils::format_duration_words(remaining)
    };

    let lines = state()
        .read()
        .unwrap()
        .raw
        .language
        .string_list("DEATHBAN.COMMAND_CHECK");

    for line in lines {
        reply(
            sender,
            &utils::replace_all(
                &line,
                &[
                    ("<player>", &target_name),
                    ("<duration>", &duration),
                    ("<reason>", &reason),
                    ("<location>", &location),
                ],
            ),
        );
    }
}

/// `DeathbanManager#revivePlayer(CommandSender, OfflinePlayer)`.
fn revive(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(name) = present_string_arg(args, "player") else {
        usage(sender);
        return;
    };
    let Some((uuid, target_name)) = lookup(server, &name) else {
        not_found(sender, &name);
        return;
    };

    let revived = {
        let mut guard = state().write().unwrap();
        match guard.userdata.get_mut(&uuid) {
            Some(user) if user.is_deathbanned() => {
                user.revive();
                true
            }
            _ => false,
        }
    };

    if !revived {
        not_deathbanned(sender, &target_name);
        return;
    }

    // The original saves the deathban store asynchronously right away.
    crate::state::save_all();

    let sender_name = sender_player(sender)
        .map(|player| core::name_of(&player))
        .unwrap_or_else(|| "CONSOLE".to_string());

    let message = utils::replace(
        &utils::replace(
            &prefixed("DEATHBAN.REVIVED_PLAYER"),
            "<player>",
            &sender_name,
        ),
        "<target>",
        &target_name,
    );

    let staff: Vec<_> = server
        .get_all_players()
        .into_iter()
        .filter(|player| player.has_permission("lazarus.staff") || player.has_permission("lazarus:staff"))
        .collect();
    for player in staff {
        core::send(&player, &message);
    }
}

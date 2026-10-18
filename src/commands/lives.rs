//! `/lives` command — port of `me.qiooip.lazarus.commands.staff.LivesCommand`
//! and the `DeathbanManager` lives helpers.
//!
//! - `/lives check [player]` shows a lives count.
//! - `/lives revive <player>` consumes one of the **sender's** own lives
//!   (players only, exactly like the original).
//! - `/lives send <player> <amount>` transfers lives between players.
//! - `/lives add|set <player> <amount>` are staff commands guarded by
//!   `lazarus.lives.add` / `lazarus.lives.set`.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply, string_arg};
use crate::utils::core;
use crate::state::state;
use crate::utils;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.lives", "Manage lives")?;

    let word = |name: &str| {
        CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord))
    };

    let root = command(&["lives"], "Manage lives")
        .execute(LivesUsageExecutor)
        .then(
            CommandNode::literal("check")
                .execute(LivesCheckSelfExecutor)
                .then(word("player").execute(LivesCheckOtherExecutor)),
        )
        .then(
            CommandNode::literal("revive")
                .execute(LivesUsageExecutor)
                .then(word("player").execute(LivesReviveExecutor)),
        )
        .then(
            CommandNode::literal("send")
                .execute(LivesUsageExecutor)
                .then(word("player").then(word("amount").execute(LivesSendExecutor))),
        )
        .then(
            CommandNode::literal("add")
                .execute(LivesAddUsageExecutor)
                .then(
                    word("player")
                        .execute(LivesAddUsageExecutor)
                        .then(word("amount").execute(LivesAddExecutor)),
                ),
        )
        .then(
            CommandNode::literal("set")
                .execute(LivesSetUsageExecutor)
                .then(
                    word("player")
                        .execute(LivesSetUsageExecutor)
                        .then(word("amount").execute(LivesSetExecutor)),
                ),
        );

    context.register_command(root, "lazarus:command.lives");
    Ok(())
}

/// `LIVES_COMMAND.USAGE` (printed for `/lives` and incomplete subcommands).
struct LivesUsageExecutor;

impl CommandHandler for LivesUsageExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let lines = state()
            .read()
            .unwrap()
            .raw
            .language
            .string_list("LIVES_COMMAND.USAGE");
        for line in lines {
            reply(&sender, line);
        }
        Ok(1)
    }
}

struct LivesCheckSelfExecutor;

impl CommandHandler for LivesCheckSelfExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            reply(&sender, for_players_only());
            return Ok(0);
        };

        let uuid = core::uuid_of(&player);
        let name = core::name_of(&player);
        send_lives_count(&sender, &uuid, &name);
        Ok(1)
    }
}

struct LivesCheckOtherExecutor;

impl CommandHandler for LivesCheckOtherExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(name) = string_arg(&args, "player") else {
            return Ok(0);
        };
        let Some((target_uuid, target_name)) = resolve_target(&server, &name) else {
            player_not_found(&sender, &name);
            return Ok(0);
        };

        send_lives_count(&sender, &target_uuid, &target_name);
        Ok(1)
    }
}

struct LivesReviveExecutor;

impl CommandHandler for LivesReviveExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let Some(name) = string_arg(&args, "player") else {
            return Ok(0);
        };
        let Some((target_uuid, target_name)) = resolve_target(&server, &name) else {
            player_not_found(&sender, &name);
            return Ok(0);
        };

        // `DeathbanManager#reviveUsingLives`: players only (they pay the life).
        let Some(player) = sender.as_player() else {
            reply(&sender, for_players_only());
            return Ok(0);
        };
        let sender_uuid = core::uuid_of(&player);

        let message = {
            let mut guard = state().write().unwrap();
            let prefix = guard.raw.language.string("PREFIX");

            if !guard
                .userdata
                .get(&target_uuid)
                .is_some_and(|user| user.is_deathbanned())
            {
                let template = guard
                    .raw
                    .language
                    .string("LIVES_COMMAND.PLAYER_NOT_DEATHBANNED");
                format!(
                    "{prefix}{}",
                    utils::replace(&template, "<player>", &target_name)
                )
            } else {
                let sender_lives = guard
                    .userdata
                    .get(&sender_uuid)
                    .map(|user| user.lives)
                    .unwrap_or(0);

                if sender_lives <= 0 {
                    format!(
                        "{prefix}{}",
                        guard.raw.language.string("LIVES_COMMAND.ZERO_LIVES")
                    )
                } else {
                    if let Some(user) = guard.userdata.get_mut(&sender_uuid) {
                        user.lives -= 1;
                    }
                    if let Some(user) = guard.userdata.get_mut(&target_uuid) {
                        user.revive();
                    }

                    let template = guard
                        .raw
                        .language
                        .string("LIVES_COMMAND.SUCCESSFULLY_REVIVED_PLAYER");
                    format!(
                        "{prefix}{}",
                        utils::replace(&template, "<player>", &target_name)
                    )
                }
            }
        };

        reply(&sender, message);
        Ok(1)
    }
}

struct LivesSendExecutor;

impl CommandHandler for LivesSendExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        // `LivesCommand` resolves the target before calling `sendLives`.
        let Some(name) = string_arg(&args, "player") else {
            return Ok(0);
        };
        let Some((target_uuid, target_name)) = resolve_target(&server, &name) else {
            player_not_found(&sender, &name);
            return Ok(0);
        };

        // `DeathbanManager#sendLives`.
        let Some(player) = sender.as_player() else {
            reply(&sender, for_players_only());
            return Ok(0);
        };
        let sender_uuid = core::uuid_of(&player);

        let Some(amount_raw) = string_arg(&args, "amount") else {
            return Ok(0);
        };
        let Ok(amount) = amount_raw.parse::<i32>() else {
            invalid_number(&sender);
            return Ok(0);
        };
        let lives = amount.abs();

        let (message, transferred) = {
            let mut guard = state().write().unwrap();
            let prefix = guard.raw.language.string("PREFIX");

            if target_uuid == sender_uuid {
                (
                    format!(
                        "{prefix}{}",
                        guard
                            .raw
                            .language
                            .string("LIVES_COMMAND.CAN_NOT_SEND_LIVES_TO_YOURSELF")
                    ),
                    false,
                )
            } else {
                let sender_lives = guard
                    .userdata
                    .get(&sender_uuid)
                    .map(|user| user.lives)
                    .unwrap_or(0);

                if sender_lives <= 0 {
                    (
                        format!(
                            "{prefix}{}",
                            guard.raw.language.string("LIVES_COMMAND.ZERO_LIVES")
                        ),
                        false,
                    )
                } else if sender_lives < lives {
                    let template = guard
                        .raw
                        .language
                        .string("LIVES_COMMAND.NOT_ENOUGH_LIVES");
                    (
                        format!(
                            "{prefix}{}",
                            utils::replace(&template, "<amount>", &lives.to_string())
                        ),
                        false,
                    )
                } else {
                    if let Some(user) = guard.userdata.get_mut(&sender_uuid) {
                        user.lives -= lives;
                    }
                    if let Some(user) = guard.userdata.get_mut(&target_uuid) {
                        user.lives += lives;
                    }

                    let template = guard
                        .raw
                        .language
                        .string("LIVES_COMMAND.SUCCESSFULLY_SENT_LIVES");
                    (
                        format!(
                            "{prefix}{}",
                            utils::replace_all(
                                &template,
                                &[
                                    ("<amount>", &lives.to_string()),
                                    ("<player>", &target_name),
                                ],
                            )
                        ),
                        true,
                    )
                }
            }
        };

        reply(&sender, message);

        // Notify the target when they are online.
        if transferred
            && let Some(target_player) = pumpkin_plugin_api::uuid::parse(&target_uuid)
                .and_then(|id| server.get_player_by_uuid(id))
        {
            let (prefix, template) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard
                        .raw
                        .language
                        .string("LIVES_COMMAND.SUCCESSFULLY_RECEIVED_LIVES"),
                )
            };
            let message = utils::replace_all(
                &template,
                &[
                    ("<amount>", &lives.to_string()),
                    ("<player>", &sender.get_name()),
                ],
            );
            core::send(&target_player, format!("{prefix}{message}"));
        }

        Ok(1)
    }
}

struct LivesAddUsageExecutor;

impl CommandHandler for LivesAddUsageExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !require_lives_permission(&sender, &server, "lazarus.lives.add") {
            return Ok(0);
        }
        let template = state()
            .read()
            .unwrap()
            .raw
            .language
            .string("LIVES_COMMAND.LIVES_ADD_USAGE");
        reply(&sender, prefixed(&template));
        Ok(0)
    }
}

struct LivesSetUsageExecutor;

impl CommandHandler for LivesSetUsageExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        if !require_lives_permission(&sender, &server, "lazarus.lives.set") {
            return Ok(0);
        }
        let template = state()
            .read()
            .unwrap()
            .raw
            .language
            .string("LIVES_COMMAND.LIVES_SET_USAGE");
        reply(&sender, prefixed(&template));
        Ok(0)
    }
}

struct LivesAddExecutor;

impl CommandHandler for LivesAddExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        change_lives(&sender, &server, &args, true)
    }
}

struct LivesSetExecutor;

impl CommandHandler for LivesSetExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        change_lives(&sender, &server, &args, false)
    }
}

/// `DeathbanManager#changeLives` (`/lives add|set`).
fn change_lives(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
    add: bool,
) -> Result<i32, CommandError> {
    let permission = if add { "lazarus.lives.add" } else { "lazarus.lives.set" };
    if !require_lives_permission(sender, server, permission) {
        return Ok(0);
    }

    let Some(name) = string_arg(args, "player") else {
        return Ok(0);
    };
    let Some((target_uuid, target_name)) = resolve_target(server, &name) else {
        player_not_found(sender, &name);
        return Ok(0);
    };

    // The original takes the amount as a string and validates it here, so the
    // invalid-number message matches.
    let Some(amount_raw) = string_arg(args, "amount") else {
        return Ok(0);
    };
    let Ok(amount) = amount_raw.parse::<i32>() else {
        invalid_number(sender);
        return Ok(0);
    };

    let message = {
        let mut guard = state().write().unwrap();
        let prefix = guard.raw.language.string("PREFIX");
        let template = guard.raw.language.string(if add {
            "LIVES_COMMAND.LIVES_ADDED"
        } else {
            "LIVES_COMMAND.LIVES_SET"
        });

        guard.userdata.get_mut(&target_uuid).map(|user| {
            if add {
                user.lives += amount.abs();
            } else {
                user.lives = amount.abs();
            }

            format!(
                "{prefix}{}",
                utils::replace_all(
                    &template,
                    &[("<amount>", &amount_raw), ("<player>", &target_name)],
                )
            )
        })
    };

    let Some(message) = message else {
        player_not_found(sender, &name);
        return Ok(0);
    };
    reply(sender, message);

    // Notify the target when they are online.
    if let Some(target_player) = pumpkin_plugin_api::uuid::parse(&target_uuid)
        .and_then(|id| server.get_player_by_uuid(id))
    {
        let (prefix, template) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.raw.language.string(if add {
                    "LIVES_COMMAND.LIVES_ADD_RECEIVED"
                } else {
                    "LIVES_COMMAND.LIVES_SET_RECEIVED"
                }),
            )
        };
        let message = utils::replace_all(
            &template,
            &[
                ("<amount>", &amount_raw),
                ("<player>", &sender.get_name()),
            ],
        );
        core::send(&target_player, format!("{prefix}{message}"));
    }

    Ok(1)
}

/// `DeathbanManager#sendLivesCount`.
fn send_lives_count(sender: &CommandSender, target_uuid: &str, target_name: &str) {
    let is_self = sender
        .as_player()
        .map(|player| core::uuid_of(&player) == target_uuid)
        .unwrap_or(false);

    let (prefix, template, lives) = {
        let guard = state().read().unwrap();
        let lives = guard
            .userdata
            .get(target_uuid)
            .map(|user| user.lives)
            .unwrap_or(0);
        let key = if is_self {
            "LIVES_COMMAND.CHECK_SELF"
        } else {
            "LIVES_COMMAND.CHECK_OTHERS"
        };
        (
            guard.raw.language.string("PREFIX"),
            guard.raw.language.string(key),
            lives,
        )
    };

    let message = if is_self {
        utils::replace(&template, "<lives>", &lives.to_string())
    } else {
        utils::replace(
            &utils::replace(&template, "<player>", target_name),
            "<lives>",
            &lives.to_string(),
        )
    };

    reply(sender, format!("{prefix}{message}"));
}

/// `BaseCommand#checkOfflinePlayer` equivalent: online players first, then the
/// userdata of players seen before (persisted in `userdata.json`).
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

fn require_lives_permission(sender: &CommandSender, server: &Server, node: &str) -> bool {
    if sender.is_console() || sender.has_permission(server, node) {
        return true;
    }

    let (prefix, template) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard.raw.language.string("COMMANDS.NO_PERMISSION"),
        )
    };
    reply(sender, format!("{prefix}{template}"));
    false
}

fn prefixed(template: &str) -> String {
    format!("{}{}", state().read().unwrap().raw.language.string("PREFIX"), template)
}

fn for_players_only() -> String {
    // Guard dropped between the statements, `prefixed` takes its own lock.
    let template = state()
        .read()
        .unwrap()
        .raw
        .language
        .string("COMMANDS.FOR_PLAYERS_ONLY");
    prefixed(&template)
}

fn player_not_found(sender: &CommandSender, name: &str) {
    let template = state()
        .read()
        .unwrap()
        .raw
        .language
        .string("COMMANDS.PLAYER_NOT_FOUND");
    let message = prefixed(&utils::replace(&template, "<player>", name));
    reply(sender, message);
}

fn invalid_number(sender: &CommandSender) {
    let template = state()
        .read()
        .unwrap()
        .raw
        .language
        .string("COMMANDS.INVALID_NUMBER");
    let message = prefixed(&template);
    reply(sender, message);
}

//! `/top`, `/topkills`, `/topdeaths`, `/topkillstreak`, `/topbalance` and
//! `/leaderboard` — port of the original `commands` package classes
//! (`TopCommand`, `TopKillsCommand`, `TopDeathsCommand`,
//! `TopKillstreaksCommand`, `TopBalanceCommand`, `LeaderboardCommand`).
//!
//! Every command prints the rows held by the shared leaderboard cache (see
//! [`crate::handlers::leaderboard`]) — never live-sorted values — exactly like
//! the original. `/topkillstreak` and the `killstreak` argument of
//! `/leaderboard` are kitmap-mode only.

use pumpkin_plugin_api::command::{ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, present_string_arg, register_public_permission, reply};
use crate::handlers::leaderboard::Board;
use crate::state::{AppState, state};
use crate::utils;

/// `LeaderboardHandler#sendLeaderboardMessage`.
fn send_leaderboard_message(sender: &CommandSender, state: &AppState, board: Board) {
    let language = &state.raw.language;

    if board.entries(state).is_empty() {
        reply(
            sender,
            format!(
                "{}{}",
                language.string("PREFIX"),
                language.string("LEADERBOARDS.NO_LEADERBOARDS")
            ),
        );
        return;
    }

    reply(sender, language.string("LEADERBOARDS.COMMAND_HEADER"));
    reply(sender, board.title(state));

    let format = board.line_format(state);
    let lines: Vec<String> = board
        .entries(state)
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            utils::replace_all(
                &format,
                &[
                    ("<number>", &(index + 1).to_string()),
                    ("<player>", &entry.name),
                    ("<value>", &entry.value.to_string()),
                ],
            )
        })
        .collect();

    for line in lines {
        reply(sender, line);
    }

    reply(sender, language.string("LEADERBOARDS.COMMAND_FOOTER"));
}

/// `PREFIX + message` from the raw language file.
fn prefixed(state: &AppState, key: &str) -> String {
    format!(
        "{}{}",
        state.raw.language.string("PREFIX"),
        state.raw.language.string(key)
    )
}

/// `TopCommand`: teleports the player to the highest block at their position.
fn top(sender: &CommandSender) {
    let Some(player) = sender.as_player() else {
        let guard = state().read().unwrap();
        reply(
            sender,
            format!(
                "{}{}",
                guard.raw.language.string("PREFIX"),
                guard.raw.language.string("COMMANDS.FOR_PLAYERS_ONLY")
            ),
        );
        return;
    };

    let location = crate::utils::core::location(&player);
    let (prefix, message) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard.raw.language.string("TOP_COMMAND_MESSAGE"),
        )
    };

    let world = player.get_world();

    // `player.getWorld().getHighestBlockAt(player.getLocation())` uses the
    // motion-blocking heightmap; the port stands the player on top of that
    // block (+1) instead of inside it, which is what the original's
    // `Location#add(0.5, 0, 0.5)` teleport resolves to in practice.
    let y = world.get_motion_blocking_height(location.block_x(), location.block_z()) + 1;
    let position = (
        location.x.floor() + 0.5,
        y as f64,
        location.z.floor() + 0.5,
    );

    player.teleport(position, Some(location.yaw), Some(location.pitch), world);
    reply(sender, format!("{prefix}{message}"));
}

struct TopExecutor;

impl CommandHandler for TopExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        top(&sender);
        Ok(1)
    }
}

/// The argument-less `/topkills` family.
struct LeaderboardExecutor(Board);

impl CommandHandler for LeaderboardExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let guard = state().read().unwrap();

        if self.0 == Board::PlayerKillstreak && !guard.config.kitmap.enabled {
            reply(&sender, prefixed(&guard, "LEADERBOARDS.KITMAP_MODE_ONLY"));
            return Ok(1);
        }

        send_leaderboard_message(&sender, &guard, self.0);
        Ok(1)
    }
}

/// `/leaderboard` without arguments — usage.
struct LeaderboardListExecutor;

impl CommandHandler for LeaderboardListExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        _args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let guard = state().read().unwrap();
        reply(&sender, prefixed(&guard, "LEADERBOARDS.COMMAND_USAGE"));
        Ok(1)
    }
}

/// `/leaderboard <type>`.
struct LeaderboardArgExecutor;

impl CommandHandler for LeaderboardArgExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        _server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        let guard = state().read().unwrap();

        let Some(argument) = present_string_arg(&args, "type") else {
            reply(&sender, prefixed(&guard, "LEADERBOARDS.COMMAND_USAGE"));
            return Ok(1);
        };

        let Some(board) = Board::from_player_param(&argument) else {
            let template = guard.raw.language.string("LEADERBOARDS.TYPE_DOESNT_EXIST");
            let message = utils::replace(&template, "<type>", &argument);
            reply(&sender, format!("{}{}", guard.raw.language.string("PREFIX"), message));
            return Ok(1);
        };

        if board == Board::PlayerKillstreak && !guard.config.kitmap.enabled {
            reply(&sender, prefixed(&guard, "LEADERBOARDS.KITMAP_MODE_ONLY"));
            return Ok(1);
        }

        send_leaderboard_message(&sender, &guard, board);
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    let word = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));

    register_public_permission(context, "lazarus:command.top", "Teleport to the highest block")?;
    context.register_command(
        command(&["top"], "Teleport to the highest block").execute(TopExecutor),
        "lazarus:command.top",
    );

    register_public_permission(context, "lazarus:command.topkills", "Top player kills")?;
    context.register_command(
        command(&["topkills"], "Top player kills").execute(LeaderboardExecutor(Board::PlayerKills)),
        "lazarus:command.topkills",
    );

    register_public_permission(context, "lazarus:command.topdeaths", "Top player deaths")?;
    context.register_command(
        command(&["topdeaths"], "Top player deaths")
            .execute(LeaderboardExecutor(Board::PlayerDeaths)),
        "lazarus:command.topdeaths",
    );

    register_public_permission(
        context,
        "lazarus:command.topkillstreak",
        "Top player killstreak (kitmap mode)",
    )?;
    context.register_command(
        command(&["topkillstreak", "topkillstreaks"], "Top player killstreak")
            .execute(LeaderboardExecutor(Board::PlayerKillstreak)),
        "lazarus:command.topkillstreak",
    );

    register_public_permission(context, "lazarus:command.topbalance", "Top player balance")?;
    context.register_command(
        command(&["topbalance"], "Top player balance")
            .execute(LeaderboardExecutor(Board::PlayerBalance)),
        "lazarus:command.topbalance",
    );

    register_public_permission(context, "lazarus:command.leaderboard", "Show a leaderboard")?;
    context.register_command(
        command(&["leaderboard", "lb"], "Show a leaderboard")
            .execute(LeaderboardListExecutor)
            .then(word("type").execute(LeaderboardArgExecutor)),
        "lazarus:command.leaderboard",
    );

    Ok(())
}

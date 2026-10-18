//! `/pvp` command — port of `commands.PvpCommand`.
//!
//! `/pvp enable` disables the player's PvP protection, `/pvp time` shows the
//! remaining time — including the banked remainder while paused inside the
//! safezone (`PvpProtTimer#getTimeLeft`).

use pumpkin_plugin_api::command::{CommandError, CommandNode, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply};
use crate::utils::core;
use crate::state::state;
use crate::timer::TimerType;
use crate::utils;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.pvp", "Use /pvp")?;

    let root = command(&["pvp"], "Manage your PvP protection")
        .execute(PvpExecutor)
        .then(CommandNode::literal("enable").execute(PvpEnableExecutor))
        .then(CommandNode::literal("time").execute(PvpTimeExecutor));

    context.register_command(root, "lazarus:command.pvp");
    Ok(())
}

/// `Language.PREFIX` prepended to the message, like the original
/// (`Language.PREFIX + ...`; the default prefix is empty).
fn prefixed(prefix: &str, message: &str) -> String {
    format!("{prefix}{message}")
}

/// Player-only guard (`BaseCommand#checkConsoleSender`).
fn require_player(sender: &CommandSender) -> Option<pumpkin_plugin_api::Player> {
    let player = sender.as_player();
    if player.is_none() {
        let message = state().read().unwrap().lang.for_players_only.clone();
        reply(sender, &message);
    }
    player
}

/// Prints `PVP_COMMAND.USAGE` (header, enable, time, footer).
fn send_usage(sender: &CommandSender) {
    let lines = {
        let guard = state().read().unwrap();
        [
            guard.lang.pvp_command_usage_header.clone(),
            guard.lang.pvp_command_usage_enable.clone(),
            guard.lang.pvp_command_usage_time.clone(),
            guard.lang.pvp_command_usage_footer.clone(),
        ]
    };
    for line in lines {
        reply(sender, &line);
    }
}

struct PvpExecutor;

impl CommandHandler for PvpExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if require_player(&sender).is_none() {
            return Ok(0);
        }
        send_usage(&sender);
        Ok(1)
    }
}

struct PvpEnableExecutor;

impl CommandHandler for PvpEnableExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = require_player(&sender) else {
            return Ok(0);
        };
        let uuid = core::uuid_of(&player);

        let (prefix, not_active, disabled) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.lang.pvp_command_not_active.clone(),
                guard.lang.pvp_command_disabled.clone(),
            )
        };

        if !state().read().unwrap().timers.is_active(&uuid, TimerType::PvpProtection) {
            reply(&sender, prefixed(&prefix, &not_active));
            return Ok(0);
        }

        // `PvpProtTimer#cancel`: removes running and banked time (the claim-wall
        // visuals disappear on the next glass tick).
        state().write().unwrap().timers.cancel_pvp(&uuid);
        reply(&sender, prefixed(&prefix, &disabled));
        Ok(1)
    }
}

struct PvpTimeExecutor;

impl CommandHandler for PvpTimeExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = require_player(&sender) else {
            return Ok(0);
        };
        let uuid = core::uuid_of(&player);

        let (prefix, not_active, template, remaining) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.lang.pvp_command_not_active.clone(),
                guard.lang.pvp_command_time.clone(),
                guard.timers.remaining(&uuid, TimerType::PvpProtection),
            )
        };

        if remaining <= 0 {
            reply(&sender, prefixed(&prefix, &not_active));
            return Ok(0);
        }

        let time = utils::format_clock(remaining);
        reply(&sender, prefixed(&prefix, &utils::replace(&template, "<time>", &time)));
        Ok(1)
    }
}

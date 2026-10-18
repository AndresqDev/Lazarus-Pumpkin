//! `/logout` command — port of `commands.LogoutCommand` and the
//! `handlers.timer.LogoutHandler` cancellations.
//!
//! Starts the logout warmup (`LogoutTimer`): if it runs to completion the quit
//! counts as safe and spawns no combat logger. Moving horizontally (or
//! teleporting), taking damage or quitting cancels the warmup.

use pumpkin_plugin_api::command::{CommandError, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply};
use crate::state::state;
use crate::timer::TimerType;

/// `LogoutCommand` had no permission node defined in the original (`super(...,
/// "lazarus.logout", true)`); like `/pvp`, the port registers a public node.
pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.logout", "Use /logout")?;

    let root = command(&["logout"], "Log out safely").execute(LogoutExecutor);

    context.register_command(root, "lazarus:command.logout");
    Ok(())
}

struct LogoutExecutor;

impl CommandHandler for LogoutExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };
        let uuid = crate::utils::core::uuid_of(&player);

        // `LogoutTimer#isActive(player)`.
        let already_running = {
            let guard = state().read().unwrap();
            guard.timers.pending(&uuid).map(|action| action.timer_type)
                == Some(TimerType::Logout)
        };

        if already_running {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.raw.language.string("LOGOUT.ALREADY_RUNNING"),
                )
            };
            reply(&sender, &format!("{prefix}{message}"));
            return Ok(1);
        }

        // `Config.LOGOUT_DELAY` drives both the timer and the message.
        let (prefix, template, seconds) = {
            let guard = state().read().unwrap();
            let delay = guard.raw.config.int("LOGOUT_DELAY");
            (
                guard.raw.language.string("PREFIX"),
                guard.raw.language.string("LOGOUT.START_MESSAGE"),
                if delay <= 0 { 30 } else { delay as i64 },
            )
        };

        crate::handlers::logger::start_logout(&player, seconds);
        reply(
            &sender,
            &format!("{prefix}{}", template.replace("<seconds>", &seconds.to_string())),
        );
        Ok(1)
    }
}

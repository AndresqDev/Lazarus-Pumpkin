//! `/sotw` command — port of `commands.SotwCommand`.
//!
//! * `/sotw start [time]`, `/sotw stop`, `/sotw hideplayers` — admin only
//!   (`lazarus:sotw`);
//! * `/sotw enable` — player only (`lazarus:sotw.enable`), opts out of the
//!   damage protection;
//! * `/sotw time` — anyone, shows the remaining time;
//! * no arguments (or an unknown one) prints the admin usage for staff and the
//!   player usage for everyone else.

use pumpkin_plugin_api::command::{ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_op_permission, register_public_permission, reply};
use crate::state::state;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.sotw", "Use /sotw")?;
    register_op_permission(context, "lazarus:sotw", "Manage the SOTW timer")?;
    register_public_permission(context, "lazarus:sotw.enable", "Opt out of SOTW protection")?;

    let root = command(&["sotw"], "Start of the world")
        .execute(SotwExecutor)
        .then(
            CommandNode::literal("start")
                .execute(SotwStartExecutor)
                .then(
                    CommandNode::argument("time", &ArgumentType::String(StringType::SingleWord))
                        .execute(SotwStartTimeExecutor),
                ),
        )
        .then(CommandNode::literal("stop").execute(SotwStopExecutor))
        .then(CommandNode::literal("hideplayers").execute(SotwHidePlayersExecutor))
        .then(CommandNode::literal("enable").execute(SotwEnableExecutor))
        .then(CommandNode::literal("time").execute(SotwTimeExecutor));

    context.register_command(root, "lazarus:command.sotw");
    Ok(())
}

/// True when the sender may manage the timer (`BaseCommand#checkPermission`).
fn require_admin(sender: &CommandSender, server: &Server) -> bool {
    if sender.is_console() || sender.has_permission(server, "lazarus:sotw") {
        return true;
    }
    let (prefix, message) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard.lang.no_permission.clone(),
        )
    };
    reply(sender, format!("{prefix}{message}"));
    false
}

struct SotwExecutor;

impl CommandHandler for SotwExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        // The original prints `SOTW_ADMIN_COMMAND_USAGE` for staff and
        // `SOTW_PLAYER_COMMAND_USAGE` for everyone else — never a permission
        // error (no `checkPermission` call here).
        let admin = sender.is_console() || sender.has_permission(&server, "lazarus:sotw");

        let lines: Vec<String> = {
            let guard = state().read().unwrap();
            if admin {
                vec![
                    guard.lang.sotw_admin_usage_header.clone(),
                    guard.lang.sotw_admin_usage_title.clone(),
                    guard.lang.sotw_admin_usage_start.clone(),
                    guard.lang.sotw_admin_usage_stop.clone(),
                    guard.lang.sotw_admin_usage_hideplayers.clone(),
                    guard.lang.sotw_admin_usage_footer.clone(),
                ]
            } else {
                vec![
                    guard.lang.sotw_player_usage_header.clone(),
                    guard.lang.sotw_player_usage_enable.clone(),
                    guard.lang.sotw_player_usage_time.clone(),
                    guard.lang.sotw_player_usage_footer.clone(),
                ]
            }
        };

        // Usage lines are sent raw, without the prefix.
        for line in lines {
            reply(&sender, &line);
        }
        Ok(1)
    }
}

struct SotwStartExecutor;

impl CommandHandler for SotwStartExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_admin(&sender, &server) {
            return Ok(0);
        }

        // `Config.SOTW_DEFAULT_TIME * 60`: the config stores minutes.
        let seconds = {
            let guard = state().read().unwrap();
            guard.config.sotw.default_time_minutes * 60
        };

        if !crate::sotw::start(&server, seconds) {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.sotw_already_running.clone(),
                )
            };
            reply(&sender, format!("{prefix}{message}"));
            return Ok(0);
        }

        Ok(1)
    }
}

struct SotwStartTimeExecutor;

impl CommandHandler for SotwStartTimeExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_admin(&sender, &server) {
            return Ok(0);
        }

        let Some(raw) = super::string_arg(&args, "time") else {
            return Ok(0);
        };

        // Port of `StringUtils#parseSeconds`; `KIT_PREFIX` is empty in the
        // original, so the invalid-duration message goes out unprefixed.
        let Some(seconds) = crate::util::parse_seconds(&raw) else {
            let message = state().read().unwrap().lang.invalid_duration.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        // `activate(0)` quirk kept: a zero duration still starts (and
        // broadcasts) the timer.
        if !crate::sotw::start(&server, seconds.max(0) as u64) {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.sotw_already_running.clone(),
                )
            };
            reply(&sender, format!("{prefix}{message}"));
            return Ok(0);
        }

        Ok(1)
    }
}

struct SotwStopExecutor;

impl CommandHandler for SotwStopExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_admin(&sender, &server) {
            return Ok(0);
        }

        if !crate::sotw::stop(&server) {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.sotw_not_running.clone(),
                )
            };
            reply(&sender, format!("{prefix}{message}"));
            return Ok(0);
        }

        Ok(1)
    }
}

struct SotwHidePlayersExecutor;

impl CommandHandler for SotwHidePlayersExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_admin(&sender, &server) {
            return Ok(0);
        }

        // The original runs the toggle async (with a null sender, so only the
        // visibility update happens there); synchronous is fine here.
        let hidden = crate::sotw::toggle_invisibility(&server, None);

        let (prefix, message) = {
            let guard = state().read().unwrap();
            let message = if hidden {
                guard.lang.sotw_hide_players_enabled.clone()
            } else {
                guard.lang.sotw_hide_players_disabled.clone()
            };
            (guard.raw.language.string("PREFIX"), message)
        };
        reply(&sender, format!("{prefix}{message}"));
        Ok(1)
    }
}

struct SotwEnableExecutor;

impl CommandHandler for SotwEnableExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        // `checkConsoleSender`: the console never reaches the body.
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        // `checkPermission(sender, "lazarus.sotw.enable")`.
        if !sender.has_permission(&server, "lazarus:sotw.enable") {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.no_permission.clone(),
                )
            };
            reply(&sender, format!("{prefix}{message}"));
            return Ok(0);
        }

        // `enableSotwForPlayer` sends its own not-running/already/enabled
        // messages.
        crate::sotw::enable_for_player(&player);
        Ok(1)
    }
}

struct SotwTimeExecutor;

impl CommandHandler for SotwTimeExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        // `SotwTimer#isActive` — `/sotw time` needs no permission and works
        // from the console too.
        if !crate::sotw::is_active() {
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.sotw_not_running.clone(),
                )
            };
            reply(&sender, format!("{prefix}{message}"));
            return Ok(0);
        }

        let (prefix, template) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.lang.sotw_time_status.clone(),
            )
        };
        let time = crate::util::format_clock(crate::sotw::remaining());
        let message = crate::util::replace(&template, "<time>", &time);
        reply(&sender, format!("{prefix}{message}"));
        Ok(1)
    }
}

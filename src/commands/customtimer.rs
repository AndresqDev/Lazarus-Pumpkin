//! `/customtimer` — port of `commands.staff.CustomTimerCommand` +
//! `timer.custom.CustomTimer`.
//!
//! * `/customtimer start <name> <placeholder> <time>` — starts a global
//!   sidebar timer (e.g. `Keyall`), permission `lazarus:customtimer`;
//! * `/customtimer cancel|stop <name>` — stops it;
//! * `/customtimer list` — lists the running timers.
//!
//! The original split `args[2 .. length-1]` into the placeholder and parsed the
//! trailing argument as the duration; the greedy `text` argument reproduces
//! that split (last whitespace-separated word = duration).

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_op_permission, reply};
use crate::state::state;
use crate::utils;

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.customtimer", "Manage custom timers")?;
    register_op_permission(context, "lazarus:customtimer", "Manage custom timers")?;

    let name = || CommandNode::argument("name", &ArgumentType::String(StringType::SingleWord));
    let text = || CommandNode::argument("text", &ArgumentType::String(StringType::Greedy));

    let root = command(&["customtimer"], "Manage custom timers")
        .execute(CustomTimerUsageExecutor)
        .then(CommandNode::literal("list").execute(CustomTimerListExecutor))
        .then(
            CommandNode::literal("cancel")
                .execute(CustomTimerUsageExecutor)
                .then(name().execute(CustomTimerCancelExecutor)),
        )
        .then(
            CommandNode::literal("stop")
                .execute(CustomTimerUsageExecutor)
                .then(name().execute(CustomTimerCancelExecutor)),
        )
        .then(
            CommandNode::literal("start")
                .execute(CustomTimerUsageExecutor)
                .then(
                    name()
                        .execute(CustomTimerUsageExecutor)
                        .then(text().execute(CustomTimerStartExecutor)),
                ),
        );

    context.register_command(root, "lazarus:command.customtimer");
    Ok(())
}

/// `BaseCommand#checkPermission` for `lazarus.customtimer` (the console passes,
/// like Bukkit's `ConsoleCommandSender`).
fn require_permission(sender: &CommandSender, server: &Server) -> bool {
    if sender.is_console() || sender.has_permission(server, "lazarus:customtimer") {
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

/// `CUSTOM_TIMER_COMMAND.USAGE`, sent raw (without the prefix) like the
/// original.
fn send_usage(sender: &CommandSender) {
    let lines = {
        let guard = state().read().unwrap();
        [
            guard.lang.custom_timer_usage_header.clone(),
            guard.lang.custom_timer_usage_title.clone(),
            guard.lang.custom_timer_usage_start.clone(),
            guard.lang.custom_timer_usage_cancel.clone(),
            guard.lang.custom_timer_usage_list.clone(),
            guard.lang.custom_timer_usage_footer.clone(),
        ]
    };

    for line in lines {
        reply(sender, &line);
    }
}

/// The original falls back to the usage block for `/customtimer`,
/// `/customtimer start`, `/customtimer start <name>` and
/// `/customtimer cancel` without a name.
struct CustomTimerUsageExecutor;

impl CommandHandler for CustomTimerUsageExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_permission(&sender, &server) {
            return Ok(0);
        }

        send_usage(&sender);
        Ok(1)
    }
}

struct CustomTimerListExecutor;

impl CommandHandler for CustomTimerListExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_permission(&sender, &server) {
            return Ok(0);
        }

        let (header, title, footer, format, timers) = {
            let mut guard = state().write().unwrap();
            // Expired timers (`CustomTimer#scheduleExpiry`) no longer list.
            guard.timers.purge_expired_custom();

            (
                guard.lang.custom_timer_list_header.clone(),
                guard.lang.custom_timer_list_title.clone(),
                guard.lang.custom_timer_list_footer.clone(),
                guard.lang.custom_timer_list_format.clone(),
                guard.timers.custom.clone(),
            )
        };

        reply(&sender, &header);
        reply(&sender, &title);

        for (index, timer) in timers.iter().enumerate() {
            // `<placeholder>` keeps the stored trailing space, like the
            // original's `Color.translate(timer.getPlaceholder())`.
            let line = utils::replace_all(
                &format,
                &[
                    ("<id>", &(index + 1).to_string()),
                    ("<timerName>", timer.name.as_str()),
                    ("<placeholder>", timer.placeholder.as_str()),
                ],
            );
            reply(&sender, &line);
        }

        reply(&sender, &footer);
        Ok(1)
    }
}

struct CustomTimerCancelExecutor;

impl CommandHandler for CustomTimerCancelExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_permission(&sender, &server) {
            return Ok(0);
        }

        let Some(name) = super::string_arg(&args, "name") else {
            return Ok(0);
        };
        let sender_name = sender.get_name();

        let removed = {
            let mut guard = state().write().unwrap();
            guard.timers.purge_expired_custom();
            guard.timers.cancel_custom(&name)
        };

        let (prefix, message) = {
            let guard = state().read().unwrap();
            match &removed {
                // `Color.translate(placeholder).replace(": ", "")`.
                Some(timer) => {
                    let timer_name = utils::replace(&timer.placeholder, ": ", "");
                    let message = utils::replace_all(
                        &guard.lang.custom_timer_stopped,
                        &[("<timerName>", &timer_name), ("<player>", &sender_name)],
                    );
                    (guard.raw.language.string("PREFIX"), message)
                }
                None => {
                    let message = utils::replace(&guard.lang.custom_timer_not_running, "<name>", &name);
                    (guard.raw.language.string("PREFIX"), message)
                }
            }
        };

        if removed.is_some() {
            // `Messages.sendMessage` when the timer was stopped...
            crate::handlers::timer::sotw::broadcast(&server, &format!("{prefix}{message}"));
        } else {
            // ...but the not-running error only reaches the sender.
            reply(&sender, &format!("{prefix}{message}"));
        }
        Ok(1)
    }
}

struct CustomTimerStartExecutor;

impl CommandHandler for CustomTimerStartExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        if !require_permission(&sender, &server) {
            return Ok(0);
        }

        let Some(name) = super::string_arg(&args, "name") else {
            return Ok(0);
        };
        let Some(text) = super::string_arg(&args, "text") else {
            return Ok(0);
        };

        // Original: `args[2 .. length-1]` is the placeholder and the trailing
        // argument the duration. Fewer than two words means no placeholder was
        // given (`args.length < 4`) → usage.
        let mut words: Vec<&str> = text.split_whitespace().collect();
        if words.len() < 2 {
            send_usage(&sender);
            return Ok(0);
        }

        let duration_word = words.pop().unwrap_or_default();
        let placeholder = words.join(" ");

        let Some(seconds) = utils::parse_seconds(duration_word) else {
            // `COMMANDS_INVALID_DURATION`; the original `PREFIX` is empty, so
            // the message goes out unprefixed.
            let message = state().read().unwrap().lang.invalid_duration.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        let sender_name = sender.get_name();

        let (prefix, message) = {
            let mut guard = state().write().unwrap();
            guard.timers.activate_custom(&name, &placeholder, seconds.max(0) as u64);

            // `placeholder.replace(":", "")` on the translated text (the
            // renderer parses the colour codes anyway).
            let timer_name = utils::replace(&placeholder, ":", "");
            let message = utils::replace_all(
                &guard.lang.custom_timer_started,
                &[("<timerName>", &timer_name), ("<player>", &sender_name)],
            );
            (guard.raw.language.string("PREFIX"), message)
        };

        crate::handlers::timer::sotw::broadcast(&server, &format!("{prefix}{message}"));
        Ok(1)
    }
}

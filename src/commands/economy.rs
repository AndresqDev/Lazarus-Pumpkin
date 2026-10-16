//! `/balance` and `/economy` — port of `economy.commands.BalanceCommand` /
//! `EconomyCommand`.

use pumpkin_plugin_api::command::{ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, int_arg, register_op_permission, register_public_permission, reply, string_arg};
use crate::core;
use crate::state::state;
use crate::util;

#[derive(Clone, Copy)]
enum Action {
    BalanceSelf,
    BalanceOther,
    Give,
    Take,
    Set,
    Reset,
}

struct BalanceExecutor(Action);

impl CommandHandler for BalanceExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        match self.0 {
            Action::BalanceSelf => balance_self(&sender),
            Action::BalanceOther => balance_other(&sender, &server, &args),
            Action::Give => change_balance(&sender, &server, &args, Change::Give),
            Action::Take => change_balance(&sender, &server, &args, Change::Take),
            Action::Set => change_balance(&sender, &server, &args, Change::Set),
            Action::Reset => change_balance(&sender, &server, &args, Change::Reset),
        }
        Ok(1)
    }
}

enum Change {
    Give,
    Take,
    Set,
    Reset,
}

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.balance", "View your balance")?;
    register_op_permission(context, "lazarus:command.economy", "Manage the economy")?;

    let word = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let int = |name: &str| CommandNode::argument(name, &ArgumentType::Integer((None, None)));

    let balance = command(&["balance", "bal", "money"], "View a balance")
        .execute(BalanceExecutor(Action::BalanceSelf))
        .then(word("player").execute(BalanceExecutor(Action::BalanceOther)));
    context.register_command(balance, "lazarus:command.balance");

    let economy = command(&["economy", "eco"], "Manage the economy")
        .then(CommandNode::literal("give").then(word("player").then(int("amount").execute(BalanceExecutor(Action::Give)))))
        .then(CommandNode::literal("take").then(word("player").then(int("amount").execute(BalanceExecutor(Action::Take)))))
        .then(CommandNode::literal("set").then(word("player").then(int("amount").execute(BalanceExecutor(Action::Set)))))
        .then(CommandNode::literal("reset").then(word("player").execute(BalanceExecutor(Action::Reset))));
    context.register_command(economy, "lazarus:command.economy");

    Ok(())
}

fn balance_self(sender: &CommandSender) {
    let Some(player) = sender.as_player() else {
        reply(sender, "&cPlayers only.");
        return;
    };
    let uuid = core::uuid_of(&player);
    let guard = state().read().unwrap();
    let balance = guard.userdata.get(&uuid).map(|u| u.balance).unwrap_or(0);
    reply(sender, format!("&eYour balance: &a${balance}"));
}

fn balance_other(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    if !allowed(sender, server) {
        return;
    }
    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);
    let guard = state().read().unwrap();
    let balance = guard.userdata.get(&uuid).map(|u| u.balance).unwrap_or(0);
    reply(sender, format!("&e{name}'s balance: &a${balance}"));
}

fn change_balance(sender: &CommandSender, server: &Server, args: &ConsumedArgs, change: Change) {
    if !allowed(sender, server) {
        return;
    }

    let Some(name) = string_arg(args, "player") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&name) else {
        reply(sender, util::replace(&state().read().unwrap().lang.player_not_online, "<player>", &name));
        return;
    };
    let uuid = core::uuid_of(&target);

    let amount = if matches!(change, Change::Reset) { 0 } else { int_arg(args, "amount").unwrap_or(0) };

    let new_balance = {
        let mut guard = state().write().unwrap();
        let config = guard.config.clone();
        let Some(user) = guard.userdata.get_mut(&uuid) else {
            return;
        };

        user.balance = match change {
            Change::Give => (user.balance + amount).min(config.economy.max_balance),
            Change::Take => (user.balance - amount).max(0),
            Change::Set => amount.min(config.economy.max_balance),
            Change::Reset => config.economy.default_player_balance,
        };
        user.balance
    };

    reply(sender, format!("&e{name}'s balance is now &a${new_balance}&e."));
}

fn allowed(sender: &CommandSender, server: &Server) -> bool {
    if sender.is_console() || sender.has_permission(server, "lazarus:command.economy") {
        return true;
    }
    reply(sender, "&cYou do not have permission.");
    false
}

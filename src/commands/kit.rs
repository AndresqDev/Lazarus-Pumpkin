//! `/kit` command — port of `kits.commands.KitCommandExecutor` and friends.

use pumpkin_plugin_api::command::{ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, int_arg, register_op_permission, register_public_permission, reply, string_arg};
use crate::utils::core;
use crate::kits;
use crate::state::state;
use crate::utils;

#[derive(Clone, Copy)]
enum Action {
    List,
    GiveSelf,
    Create,
    Remove,
    Delay,
    GiveOther,
}

struct KitExecutor(Action);

impl CommandHandler for KitExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        match self.0 {
            Action::List => list(&sender),
            Action::GiveSelf => give_self(&sender, &args),
            Action::Create => create(&sender, &args),
            Action::Remove => remove(&sender, &args),
            Action::Delay => set_delay(&sender, &args),
            Action::GiveOther => give_other(&sender, &server, &args),
        }
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.kit", "Use kits")?;
    register_op_permission(context, "lazarus:command.kit.admin", "Manage kits")?;

    let word = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));

    let root = command(&["kit", "kits"], "Kit command")
        .execute(KitExecutor(Action::List))
        .then(CommandNode::literal("create").then(word("name").execute(KitExecutor(Action::Create))))
        .then(CommandNode::literal("remove").then(word("name").execute(KitExecutor(Action::Remove))))
        .then(
            CommandNode::literal("delay")
                .then(word("name").then(CommandNode::argument("seconds", &ArgumentType::Integer((None, None))).execute(KitExecutor(Action::Delay)))),
        )
        .then(
            CommandNode::literal("give")
                .then(word("player").then(word("name").execute(KitExecutor(Action::GiveOther)))),
        )
        .then(word("name").execute(KitExecutor(Action::GiveSelf)));

    context.register_command(root, "lazarus:command.kit");
    Ok(())
}

fn list(sender: &CommandSender) {
    let guard = state().read().unwrap();
    if guard.kits.kits.is_empty() {
        reply(sender, "&cThere are no kits defined.");
        return;
    }
    reply(sender, "&3=== &bKits&3 ===");
    for kit in guard.kits.kits.values() {
        reply(sender, format!("&e{} &7(delay: {}s)", kit.name, kit.delay_seconds));
    }
}

fn give_self(sender: &CommandSender, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        reply(sender, "&cPlayers only.");
        return;
    };
    let Some(name) = string_arg(args, "name") else {
        return;
    };
    let uuid = core::uuid_of(&player);

    let (kit, remaining) = {
        let guard = state().read().unwrap();
        let kit = guard.kits.get(&name).cloned();
        let remaining = guard
            .userdata
            .get(&uuid)
            .and_then(|u| u.kit_delays.get(&name.to_lowercase()).copied())
            .map(|expiry| (expiry - utils::now_millis()).max(0))
            .unwrap_or(0);
        (kit, remaining)
    };

    let Some(kit) = kit else {
        reply(sender, format!("&cKit &e{name} &cdoes not exist."));
        return;
    };

    // `KitsManager#giveKit`: KITMAP kits are only obtainable inside a safezone.
    if kit.kind == "KITMAP" {
        let (in_safezone, prefix, message) = {
            let guard = state().read().unwrap();
            let location = core::location(&player);
            let in_safezone = matches!(
                core::territory_at_guard(
                    &guard,
                    &location.world,
                    location.block_x(),
                    location.block_z()
                ),
                core::Territory::Safezone
            );
            (
                in_safezone,
                guard.raw.language.string("KIT_PREFIX"),
                guard.raw.language.string("KITS.EXCEPTION.KITMAP_ONLY_IN_SAFEZONE"),
            )
        };

        if !in_safezone {
            reply(sender, format!("{prefix}{message}"));
            return;
        }
    }

    if remaining > 0 {
        reply(sender, format!("&cYou must wait &e{} &cbefore using that kit.", utils::format_duration(remaining)));
        return;
    }

    kits::give(&player, &kit);

    if kit.delay_seconds > 0 {
        let mut guard = state().write().unwrap();
        if let Some(user) = guard.userdata.get_mut(&uuid) {
            user.kit_delays
                .insert(name.to_lowercase(), utils::now_millis() + kit.delay_seconds as i64 * 1000);
        }
    }

    reply(sender, format!("&aKit &e{name} &areceived."));
}

fn create(sender: &CommandSender, args: &ConsumedArgs) {
    let Some(player) = sender.as_player() else {
        reply(sender, "&cPlayers only.");
        return;
    };
    let Some(name) = string_arg(args, "name") else {
        return;
    };

    let kit = kits::capture(&player, &name, 0);
    let count = kit.items.len();
    state().write().unwrap().kits.insert(kit);
    reply(sender, format!("&aKit &e{name} &acreated with &e{count} &aitems."));
}

fn remove(sender: &CommandSender, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "name") else {
        return;
    };
    if state().write().unwrap().kits.remove(&name).is_some() {
        reply(sender, format!("&cKit &e{name} &cremoved."));
    } else {
        reply(sender, format!("&cKit &e{name} &cdoes not exist."));
    }
}

fn set_delay(sender: &CommandSender, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "name") else {
        return;
    };
    let Some(seconds) = int_arg(args, "seconds") else {
        reply(sender, "&cInvalid number.");
        return;
    };

    let mut guard = state().write().unwrap();
    let Some(kit) = guard.kits.kits.get_mut(&name.to_lowercase()) else {
        reply(sender, format!("&cKit &e{name} &cdoes not exist."));
        return;
    };
    kit.delay_seconds = seconds.max(0) as i64;
    reply(sender, format!("&aKit &e{name} &adelay set to &e{seconds}s&a."));
}

fn give_other(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player_name) = string_arg(args, "player") else {
        return;
    };
    let Some(name) = string_arg(args, "name") else {
        return;
    };
    let Some(target) = server.get_player_by_name(&player_name) else {
        reply(sender, utils::replace(&state().read().unwrap().lang.player_not_online, "<player>", &player_name));
        return;
    };

    let kit = state().read().unwrap().kits.get(&name).cloned();
    let Some(kit) = kit else {
        reply(sender, format!("&cKit &e{name} &cdoes not exist."));
        return;
    };

    kits::give(&target, &kit);
    reply(sender, format!("&aGave kit &e{name} &ato &e{player_name}&a."));
}

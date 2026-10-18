//! `/hologram` command — port of `hologram.command.HologramCommandExecutor`.

use pumpkin_plugin_api::command::{ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, int_arg, register_op_permission, reply, string_arg};
use crate::utils::core;
use crate::hologram::Hologram;
use crate::state::state;
use crate::utils;

#[derive(Clone, Copy)]
enum Action {
    Create,
    Delete,
    AddLine,
    RemoveLine,
    TpHere,
    List,
}

struct HologramExecutor(Action);

impl CommandHandler for HologramExecutor {
    fn handle(&self, sender: CommandSender, server: Server, args: ConsumedArgs) -> Result<i32, CommandError> {
        match self.0 {
            Action::Create => create(&sender, &server, &args),
            Action::Delete => delete(&sender, &args),
            Action::AddLine => add_line(&sender, &server, &args),
            Action::RemoveLine => remove_line(&sender, &server, &args),
            Action::TpHere => tp_here(&sender, &server, &args),
            Action::List => list(&sender),
        }
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.hologram", "Manage holograms")?;

    let word = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let greedy = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::Greedy));
    let int = |name: &str| CommandNode::argument(name, &ArgumentType::Integer((None, None)));

    let root = command(&["hologram", "holo"], "Manage holograms")
        .execute(HologramExecutor(Action::List))
        .then(CommandNode::literal("create").then(word("name").execute(HologramExecutor(Action::Create))))
        .then(CommandNode::literal("delete").then(word("name").execute(HologramExecutor(Action::Delete))))
        .then(
            CommandNode::literal("addline")
                .then(word("name").then(greedy("text").execute(HologramExecutor(Action::AddLine)))),
        )
        .then(
            CommandNode::literal("removeline")
                .then(word("name").then(int("line").execute(HologramExecutor(Action::RemoveLine)))),
        )
        .then(CommandNode::literal("tphere").then(word("name").execute(HologramExecutor(Action::TpHere))))
        .then(CommandNode::literal("list").execute(HologramExecutor(Action::List)));

    context.register_command(root, "lazarus:command.hologram");
    Ok(())
}

fn player_only(sender: &CommandSender) -> Option<Player> {
    let player = sender.as_player();
    if player.is_none() {
        reply(sender, "&cPlayers only.");
    }
    player
}

fn create(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player) = player_only(sender) else { return };
    let Some(name) = string_arg(args, "name") else { return };
    let location = core::location(&player);

    let holo = Hologram {
        world: location.world,
        x: location.x,
        y: location.y + 0.5,
        z: location.z,
        lines: vec![format!("&e{name}")],
    };

    let mut guard = state().write().unwrap();
    if let Some(old) = guard.hologram_entities.remove(&name) {
        crate::hologram::despawn(old);
    }
    let entities = crate::hologram::spawn(server, &holo);
    guard.hologram_entities.insert(name.clone(), entities);
    guard.holograms.insert(name.clone(), holo);

    let message = utils::replace(&guard.lang.hologram_created, "<name>", &name);
    drop(guard);
    reply(sender, message);
}

fn delete(sender: &CommandSender, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "name") else { return };

    let message;
    {
        let mut guard = state().write().unwrap();
        if guard.holograms.remove(&name).is_none() {
            message = utils::replace(&guard.lang.hologram_not_found, "<name>", &name);
        } else {
            if let Some(entities) = guard.hologram_entities.remove(&name) {
                crate::hologram::despawn(entities);
            }
            message = utils::replace(&guard.lang.hologram_deleted, "<name>", &name);
        }
    }
    reply(sender, message);
}

fn add_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "name") else { return };
    let Some(text) = string_arg(args, "text") else { return };

    let message;
    {
        let mut guard = state().write().unwrap();
        let Some(holo) = guard.holograms.get_mut(&name).cloned() else {
            reply(sender, utils::replace(&guard.lang.hologram_not_found, "<name>", &name));
            return;
        };

        let mut updated = holo;
        updated.lines.push(text);
        respawn(server, &mut guard, &name, updated);

        message = utils::replace(&guard.lang.hologram_line_added, "<name>", &name);
    }
    reply(sender, message);
}

fn remove_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(name) = string_arg(args, "name") else { return };
    let Some(index) = int_arg(args, "line") else { return };

    let message;
    {
        let mut guard = state().write().unwrap();
        let Some(holo) = guard.holograms.get(&name).cloned() else {
            reply(sender, utils::replace(&guard.lang.hologram_not_found, "<name>", &name));
            return;
        };
        if index < 0 || index as usize >= holo.lines.len() {
            reply(sender, "&cInvalid line index.");
            return;
        }

        let mut updated = holo;
        updated.lines.remove(index as usize);
        respawn(server, &mut guard, &name, updated);

        message = utils::replace(&guard.lang.hologram_line_removed, "<name>", &name);
    }
    reply(sender, message);
}

fn tp_here(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let Some(player) = player_only(sender) else { return };
    let Some(name) = string_arg(args, "name") else { return };
    let location = core::location(&player);

    let mut guard = state().write().unwrap();
    let Some(holo) = guard.holograms.get(&name).cloned() else {
        reply(sender, utils::replace(&guard.lang.hologram_not_found, "<name>", &name));
        return;
    };

    let mut updated = holo;
    updated.world = location.world;
    updated.x = location.x;
    updated.y = location.y + 0.5;
    updated.z = location.z;
    respawn(server, &mut guard, &name, updated);
    reply(sender, format!("&aHologram &e{name} &amoved here."));
}

fn list(sender: &CommandSender) {
    let guard = state().read().unwrap();
    reply(sender, "&3=== &bHolograms&3 ===");
    for name in guard.holograms.keys() {
        reply(sender, format!("&e{name}"));
    }
}

fn respawn(server: &Server, guard: &mut crate::state::AppState, name: &str, holo: Hologram) {
    if let Some(old) = guard.hologram_entities.remove(name) {
        crate::hologram::despawn(old);
    }
    let entities = crate::hologram::spawn(server, &holo);
    guard.hologram_entities.insert(name.to_string(), entities);
    guard.holograms.insert(name.to_string(), holo);
}

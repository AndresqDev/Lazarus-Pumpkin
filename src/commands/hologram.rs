//! `/hologram` command — port of
//! `me.qiooip.lazarus.hologram.command.HologramCommandExecutor` and its nine
//! subcommands (`create`, `delete|remove`, `addline`, `insertline`,
//! `updateline`, `removeline`, `teleport|tp`, `teleporthere|tphere`, `list`).
//!
//! Holograms are addressed by their numeric id; `addline`/`insertline`/
//! `updateline`/`removeline` take `Math.abs` of the id and line arguments while
//! `delete`/`teleport`/`teleporthere` use the raw number, exactly like the
//! original. `EMPTY` (case-insensitive) inserts an empty line, which the
//! original also honours for every line text.

use pumpkin_plugin_api::command::{
    ArgumentType, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, present_string_arg, register_op_permission, reply};
use crate::handlers::leaderboard::Board;
use crate::hologram::{Hologram, HologramKind};
use crate::state::{AppState, state};
use crate::utils;

#[derive(Clone, Copy)]
enum Action {
    Usage,
    Create,
    Delete,
    AddLine,
    InsertLine,
    UpdateLine,
    RemoveLine,
    Teleport,
    TeleportHere,
    List,
}

struct HologramExecutor(Action);

impl CommandHandler for HologramExecutor {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> Result<i32, CommandError> {
        match self.0 {
            Action::Usage => usage(&sender),
            Action::Create => create(&sender, &server, &args),
            Action::Delete => delete(&sender, &args),
            Action::AddLine => add_line(&sender, &server, &args),
            Action::InsertLine => insert_line(&sender, &server, &args),
            Action::UpdateLine => update_line(&sender, &server, &args),
            Action::RemoveLine => remove_line(&sender, &server, &args),
            Action::Teleport => teleport(&sender, &server, &args),
            Action::TeleportHere => teleport_here(&sender, &server, &args),
            Action::List => list(&sender),
        }
        Ok(1)
    }
}

pub fn register(context: &Context) -> Result<(), String> {
    register_op_permission(context, "lazarus:command.hologram", "Manage holograms")?;

    let word = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::SingleWord));
    let greedy = |name: &str| CommandNode::argument(name, &ArgumentType::String(StringType::Greedy));

    let root = command(&["hologram", "holo"], "Manage holograms")
        .execute(HologramExecutor(Action::Usage))
        .then(
            CommandNode::literal("create")
                .execute(HologramExecutor(Action::Create))
                .then(word("type").execute(HologramExecutor(Action::Create))),
        )
        .then(
            CommandNode::literal("delete")
                .execute(HologramExecutor(Action::Delete))
                .then(word("id").execute(HologramExecutor(Action::Delete))),
        )
        .then(
            CommandNode::literal("remove")
                .execute(HologramExecutor(Action::Delete))
                .then(word("id").execute(HologramExecutor(Action::Delete))),
        )
        .then(
            CommandNode::literal("addline")
                .execute(HologramExecutor(Action::AddLine))
                .then(
                    word("id").then(greedy("text").execute(HologramExecutor(Action::AddLine))),
                ),
        )
        .then(
            CommandNode::literal("insertline")
                .execute(HologramExecutor(Action::InsertLine))
                .then(word("id").then(
                    word("line").then(greedy("text").execute(HologramExecutor(Action::InsertLine))),
                )),
        )
        .then(
            CommandNode::literal("updateline")
                .execute(HologramExecutor(Action::UpdateLine))
                .then(word("id").then(
                    word("line").then(greedy("text").execute(HologramExecutor(Action::UpdateLine))),
                )),
        )
        .then(
            CommandNode::literal("removeline")
                .execute(HologramExecutor(Action::RemoveLine))
                .then(
                    word("id").then(word("line").execute(HologramExecutor(Action::RemoveLine))),
                ),
        )
        .then(
            CommandNode::literal("teleport")
                .execute(HologramExecutor(Action::Teleport))
                .then(word("id").execute(HologramExecutor(Action::Teleport))),
        )
        .then(
            CommandNode::literal("tp")
                .execute(HologramExecutor(Action::Teleport))
                .then(word("id").execute(HologramExecutor(Action::Teleport))),
        )
        .then(
            CommandNode::literal("teleporthere")
                .execute(HologramExecutor(Action::TeleportHere))
                .then(word("id").execute(HologramExecutor(Action::TeleportHere))),
        )
        .then(
            CommandNode::literal("tphere")
                .execute(HologramExecutor(Action::TeleportHere))
                .then(word("id").execute(HologramExecutor(Action::TeleportHere))),
        )
        .then(CommandNode::literal("list").execute(HologramExecutor(Action::List)));

    context.register_command(root, "lazarus:command.hologram");
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers — the original's `HOLOGRAMS_PREFIX` messages.
// ---------------------------------------------------------------------------

fn key(state: &AppState, path: &str) -> String {
    state.raw.language.string(path)
}

/// `sender.sendMessage(Language.HOLOGRAMS_PREFIX + message)`.
fn fail(sender: &CommandSender, state: &AppState, message: String) {
    reply(sender, format!("{}{}", key(state, "HOLOGRAMS_PREFIX"), message));
}

/// Same as [`fail`], but for call sites that just released the state lock to
/// rebuild hologram entities (see [`crate::hologram::respawn`]) and read it
/// back briefly.
fn fail_later(sender: &CommandSender, message: String) {
    let guard = state().read().unwrap();
    fail(sender, &guard, message);
}

/// `SubCommandExecutor`'s usage list (`/hologram` without arguments).
fn usage(sender: &CommandSender) {
    let lines = state()
        .read()
        .unwrap()
        .raw
        .language
        .string_list("HOLOGRAMS.COMMAND_USAGE");

    for line in lines {
        reply(sender, line);
    }
}

/// `Player` cast check (`forPlayersOnly` subcommands).
fn player_only(sender: &CommandSender, state: &AppState) -> Option<Player> {
    let player = sender.as_player();
    if player.is_none() {
        fail(sender, state, key(state, "COMMANDS.FOR_PLAYERS_ONLY"));
    }
    player
}

/// `SubCommand#checkNumber`: parses an integer, reporting `INVALID_NUMBER`.
fn check_number(sender: &CommandSender, state: &AppState, raw: &str) -> Option<i32> {
    match raw.parse::<i32>() {
        Ok(value) => Some(value),
        Err(_) => {
            fail(sender, state, key(state, "COMMANDS.INVALID_NUMBER"));
            None
        }
    }
}

/// `HologramManager#getByCommandParam`.
fn hologram_or_fail(sender: &CommandSender, state: &AppState, id: i32) -> Option<Hologram> {
    let hologram = state.holograms.iter().find(|hologram| hologram.id == id);
    match hologram {
        Some(hologram) => Some(hologram.clone()),
        None => {
            let template = key(state, "HOLOGRAMS.EXCEPTIONS.DOESNT_EXIST");
            fail(sender, state, utils::replace(&template, "<id>", &id.to_string()));
            None
        }
    }
}

/// `HologramManager#getStaticHologramByCommandParam`.
fn static_hologram_or_fail(
    sender: &CommandSender,
    state: &AppState,
    id: i32,
) -> Option<(usize, Hologram)> {
    let hologram = hologram_or_fail(sender, state, id)?;

    if matches!(hologram.kind, HologramKind::Leaderboard { .. }) {
        fail(sender, state, key(state, "HOLOGRAMS.EXCEPTIONS.TYPE_MUST_BE_NORMAL"));
        return None;
    }

    let index = state
        .holograms
        .iter()
        .position(|candidate| candidate.id == id)?;
    Some((index, hologram))
}

/// `HologramManager#addHologramLine`'s `EMPTY` keyword.
fn line_text(raw: String) -> String {
    if raw.eq_ignore_ascii_case("EMPTY") {
        String::new()
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// Subcommands.
// ---------------------------------------------------------------------------

fn create(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();
    let Some(player) = player_only(sender, &guard) else {
        return;
    };

    let Some(param) = present_string_arg(args, "type") else {
        let usage = key(&guard, "HOLOGRAMS.CREATE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };

    // `HologramManager#createHologramFromParameter`: id = size + 1, at the
    // player's eye location.
    let id = guard.holograms.len() as i32 + 1;
    let world = crate::utils::core::world_dimension(&player);
    let (x, y, z) = player.as_entity().get_eye_position();

    let hologram = if param.eq_ignore_ascii_case("normal") {
        Hologram::new_normal(id, world, x, y, z)
    } else if let Some(board) = Board::from_hologram_param(&param) {
        Hologram::new_leaderboard(id, world, x, y, z, board)
    } else {
        let template = key(&guard, "HOLOGRAMS.EXCEPTIONS.TYPE_NOT_FOUND");
        fail(sender, &guard, utils::replace(&template, "<type>", &param));
        return;
    };

    guard.holograms.push(hologram);
    let created = key(&guard, "HOLOGRAMS.CREATE_COMMAND.CREATED");
    drop(guard);

    // Rebuild the display entities right away; spawning is a plain blocking
    // pump while no `EntitySpawnEvent` handler is registered (see
    // [`crate::hologram::respawn`]).
    crate::hologram::respawn(server, id);
    fail_later(sender, created);
}

fn delete(sender: &CommandSender, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();

    let Some(raw) = present_string_arg(args, "id") else {
        let usage = key(&guard, "HOLOGRAMS.DELETE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let Some(id) = check_number(sender, &guard, &raw) else {
        return;
    };

    // `delete`/`teleport`/`teleporthere` do not take the absolute value.
    if hologram_or_fail(sender, &guard, id).is_none() {
        return;
    }

    crate::hologram::delete(&mut guard, id);

    let template = key(&guard, "HOLOGRAMS.DELETE_COMMAND.DELETED");
    let message = utils::replace(&template, "<id>", &id.to_string());
    fail(sender, &guard, message);
}

fn add_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();

    let (Some(raw_id), Some(text)) = (present_string_arg(args, "id"), present_string_arg(args, "text"))
    else {
        let usage = key(&guard, "HOLOGRAMS.ADD_LINE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let Some(id) = check_number(sender, &guard, &raw_id) else {
        return;
    };

    let id = id.wrapping_abs();
    let Some((index, _)) = static_hologram_or_fail(sender, &guard, id) else {
        return;
    };

    if let HologramKind::Normal { lines } = &mut guard.holograms[index].kind {
        lines.push(line_text(text));
    }
    let added = utils::replace(
        &key(&guard, "HOLOGRAMS.ADD_LINE_COMMAND.LINE_ADDED"),
        "<id>",
        &id.to_string(),
    );
    drop(guard);

    crate::hologram::respawn(server, id);
    fail_later(sender, added);
}

fn insert_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();

    let (Some(raw_id), Some(raw_line), Some(text)) = (
        present_string_arg(args, "id"),
        present_string_arg(args, "line"),
        present_string_arg(args, "text"),
    ) else {
        let usage = key(&guard, "HOLOGRAMS.INSERT_LINE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let (Some(id), Some(line_number)) = (
        check_number(sender, &guard, &raw_id),
        check_number(sender, &guard, &raw_line),
    ) else {
        return;
    };

    // `insertline`: the original's `HologramManager#insertHologramLine` (the
    // `getStaticHologramByCommandParam` lookup happens before the bounds check).
    let id = id.wrapping_abs();
    let line_number = line_number.wrapping_abs();
    let Some((index, _)) = static_hologram_or_fail(sender, &guard, id) else {
        return;
    };

    let line_index = line_number.saturating_sub(1);
    let inserted = if let HologramKind::Normal { lines } = &mut guard.holograms[index].kind {
        if line_index < 0 || lines.is_empty() || line_index as usize > lines.len() - 1 {
            false
        } else {
            lines.insert(line_index as usize, line_text(text));
            true
        }
    } else {
        false
    };

    if !inserted {
        let template = key(&guard, "HOLOGRAMS.EXCEPTIONS.CANT_INSERT_LINE");
        let message = utils::replace(&template, "<lineIndex>", &line_number.to_string());
        fail(sender, &guard, message);
        return;
    }

    let inserted_message = utils::replace(
        &key(&guard, "HOLOGRAMS.INSERT_LINE_COMMAND.LINE_INSERTED"),
        "<id>",
        &id.to_string(),
    );
    drop(guard);

    crate::hologram::respawn(server, id);
    fail_later(sender, inserted_message);
}

fn update_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();

    let (Some(raw_id), Some(raw_line), Some(text)) = (
        present_string_arg(args, "id"),
        present_string_arg(args, "line"),
        present_string_arg(args, "text"),
    ) else {
        let usage = key(&guard, "HOLOGRAMS.UPDATE_LINE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let (Some(id), Some(line_number)) = (
        check_number(sender, &guard, &raw_id),
        check_number(sender, &guard, &raw_line),
    ) else {
        return;
    };

    let id = id.wrapping_abs();
    let line_number = line_number.wrapping_abs();
    let Some((index, _)) = static_hologram_or_fail(sender, &guard, id) else {
        return;
    };

    let line_index = line_number.saturating_sub(1);
    let updated = if let HologramKind::Normal { lines } = &mut guard.holograms[index].kind {
        if line_index < 0 || lines.is_empty() || line_index as usize > lines.len() - 1 {
            false
        } else {
            lines[line_index as usize] = line_text(text);
            true
        }
    } else {
        false
    };

    if !updated {
        let template = key(&guard, "HOLOGRAMS.EXCEPTIONS.LINE_DOESNT_EXIST");
        let message = utils::replace(&template, "<lineIndex>", &line_number.to_string());
        fail(sender, &guard, message);
        return;
    }

    let updated_message = utils::replace(
        &key(&guard, "HOLOGRAMS.UPDATE_LINE_COMMAND.LINE_UPDATED"),
        "<id>",
        &id.to_string(),
    );
    drop(guard);

    crate::hologram::respawn(server, id);
    fail_later(sender, updated_message);
}

fn remove_line(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();

    let (Some(raw_id), Some(raw_line)) = (
        present_string_arg(args, "id"),
        present_string_arg(args, "line"),
    ) else {
        let usage = key(&guard, "HOLOGRAMS.REMOVE_LINE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let (Some(id), Some(line_number)) = (
        check_number(sender, &guard, &raw_id),
        check_number(sender, &guard, &raw_line),
    ) else {
        return;
    };

    let id = id.wrapping_abs();
    let line_number = line_number.wrapping_abs();
    let Some((index, _)) = static_hologram_or_fail(sender, &guard, id) else {
        return;
    };

    let line_index = line_number.saturating_sub(1);
    let removed = if let HologramKind::Normal { lines } = &mut guard.holograms[index].kind {
        if line_index < 0 || lines.is_empty() || line_index as usize > lines.len() - 1 {
            false
        } else {
            lines.remove(line_index as usize);
            true
        }
    } else {
        false
    };

    if !removed {
        let template = key(&guard, "HOLOGRAMS.EXCEPTIONS.LINE_DOESNT_EXIST");
        let message = utils::replace(&template, "<lineIndex>", &line_number.to_string());
        fail(sender, &guard, message);
        return;
    }

    // The original rebuilds the entries (`createHologramLines`) and resends the
    // whole hologram after a removal.
    let removed_message = utils::replace(
        &key(&guard, "HOLOGRAMS.REMOVE_LINE_COMMAND.LINE_REMOVED"),
        "<id>",
        &id.to_string(),
    );
    drop(guard);

    crate::hologram::respawn(server, id);
    fail_later(sender, removed_message);
}

fn teleport(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let guard = state().read().unwrap();
    let Some(player) = player_only(sender, &guard) else {
        return;
    };

    let Some(raw) = present_string_arg(args, "id") else {
        // Faithful to the original, which prints the teleporthere usage here.
        let usage = key(&guard, "HOLOGRAMS.TELEPORT_HERE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let Some(id) = check_number(sender, &guard, &raw) else {
        return;
    };
    let Some(hologram) = hologram_or_fail(sender, &guard, id) else {
        return;
    };

    // `HologramManager#teleportToTheHologram`: teleport to the hologram's own
    // location, keeping the player's rotation (`teleport(Location)`). The
    // teleport fires `PlayerTeleportEvent` back into the plugin, so the state
    // lock must be released first.
    let (world_name, x, y, z) = (hologram.world, hologram.x, hologram.y, hologram.z);
    let teleported = utils::replace(
        &key(&guard, "HOLOGRAMS.TELEPORT_COMMAND.TELEPORTED"),
        "<id>",
        &id.to_string(),
    );
    drop(guard);

    let Some(world) = server.get_world_by_name(&world_name) else {
        return;
    };
    player.teleport((x, y, z), None, None, world);

    fail_later(sender, teleported);
}

fn teleport_here(sender: &CommandSender, server: &Server, args: &ConsumedArgs) {
    let mut guard = state().write().unwrap();
    let Some(player) = player_only(sender, &guard) else {
        return;
    };

    let Some(raw) = present_string_arg(args, "id") else {
        let usage = key(&guard, "HOLOGRAMS.TELEPORT_HERE_COMMAND.USAGE");
        fail(sender, &guard, usage);
        return;
    };
    let Some(id) = check_number(sender, &guard, &raw) else {
        return;
    };
    if hologram_or_fail(sender, &guard, id).is_none() {
        return;
    }

    let world = crate::utils::core::world_dimension(&player);
    let (x, y, z) = player.as_entity().get_eye_position();

    if let Some(hologram) = guard
        .holograms
        .iter_mut()
        .find(|hologram| hologram.id == id)
    {
        hologram.world = world;
        hologram.x = x;
        hologram.y = y;
        hologram.z = z;
    }
    let teleported = key(&guard, "HOLOGRAMS.TELEPORT_HERE_COMMAND.TELEPORTED");
    drop(guard);

    crate::hologram::respawn(server, id);
    fail_later(sender, teleported);
}

fn list(sender: &CommandSender) {
    let guard = state().read().unwrap();

    if guard.holograms.is_empty() {
        let message = key(&guard, "HOLOGRAMS.LIST_COMMAND.NO_HOLOGRAMS");
        fail(sender, &guard, message);
        return;
    }

    reply(sender, key(&guard, "HOLOGRAMS.COMMAND_HEADER"));
    reply(sender, key(&guard, "HOLOGRAMS.LIST_COMMAND.TITLE"));
    reply(sender, "");

    let mut holograms = guard.holograms.clone();
    holograms.sort_by_key(|hologram| hologram.id);

    for hologram in holograms {
        let format = key(&guard, "HOLOGRAMS.LIST_COMMAND.FORMAT");
        let location = location_name(&guard, &hologram);
        let message = utils::replace_all(
            &format,
            &[
                ("<id>", &hologram.id.to_string()),
                ("<location>", &location),
            ],
        );
        reply(sender, message);
    }

    reply(sender, key(&guard, "HOLOGRAMS.COMMAND_FOOTER"));
}

/// `StringUtils#getLocationNameWithWorld`: `<x>, <y>, <z> (<world>)` with the
/// `WORLD_NAMES` label per dimension.
fn location_name(state: &AppState, hologram: &Hologram) -> String {
    let world = match crate::utils::core::dimension_of(&hologram.world) {
        crate::utils::core::Dimension::Overworld => state.raw.language.string("WORLD_NAMES.OVERWORLD"),
        crate::utils::core::Dimension::Nether => state.raw.language.string("WORLD_NAMES.NETHER"),
        crate::utils::core::Dimension::End => state.raw.language.string("WORLD_NAMES.THE_END"),
    };

    format!(
        "{}, {}, {} ({})",
        hologram.x.floor() as i32,
        hologram.y.floor() as i32,
        hologram.z.floor() as i32,
        world
    )
}

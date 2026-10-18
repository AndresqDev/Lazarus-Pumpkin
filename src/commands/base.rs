//! Base commands: `/lazarus` and the per-player setting toggles.

use pumpkin_plugin_api::command::{CommandError, CommandNode, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply};
use crate::core;
use crate::scoreboard;
use crate::state::{self, state};
use crate::util;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.lazarus", "Use /lazarus")?;

    let root = command(&["lazarus", "lz"], "Lazarus HCF core")
        .execute(InfoExecutor)
        .then(CommandNode::literal("reload").execute(ReloadExecutor))
        .then(CommandNode::literal("save").execute(SaveExecutor));

    context.register_command(root, "lazarus:command.lazarus");

    register_toggles(context)?;
    Ok(())
}

struct InfoExecutor;

impl CommandHandler for InfoExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        reply(&sender, "&3=== &bLazarus &7(Pumpkin port) &3===");
        reply(&sender, "&7Commands: &f/f &7(factions), &f/lazarus reload&7, &f/lazarus save");
        reply(&sender, "&7Toggles: &f/togglescoreboard&7, &f/togglechat&7, &f/togglefoundore&7, &f/togglemessages&7, &f/togglesounds&7, &f/toggledeathmessages&7, &f/togglecobble&7, &f/togglelightning");
        Ok(1)
    }
}

struct ReloadExecutor;

impl CommandHandler for ReloadExecutor {
    fn handle(&self, sender: CommandSender, server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        state::reload_config();
        crate::limiters::reload();
        crate::core::refresh_spawn_centers(&server);
        crate::core::apply_world_borders_from_config(&server);

        // Re-apply tab + sidebar layouts so config changes take effect immediately.
        for player in server.get_all_players() {
            scoreboard::setup(&player);
            crate::tab::setup(&player, &server);
        }

        reply(&sender, "&aLazarus configuration reloaded.");
        Ok(1)
    }
}

struct SaveExecutor;

impl CommandHandler for SaveExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        state::save_all();
        reply(&sender, "&aData saved.");
        Ok(1)
    }
}

#[derive(Clone, Copy)]
enum Toggle {
    Scoreboard,
    Chat,
    FoundOre,
    Messages,
    Sounds,
    DeathMessages,
    Cobble,
    Lightning,
}

struct ToggleExecutor(Toggle);

impl CommandHandler for ToggleExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            reply(&sender, "&cPlayers only.");
            return Ok(0);
        };
        let uuid = core::uuid_of(&player);

        let (label, value) = {
            let mut guard = state().write().unwrap();
            let Some(user) = guard.userdata.get_mut(&uuid) else {
                return Ok(0);
            };
            let settings = &mut user.settings;

            let (label, value) = match self.0 {
                Toggle::Scoreboard => {
                    settings.scoreboard = !settings.scoreboard;
                    ("scoreboard", settings.scoreboard)
                }
                Toggle::Chat => {
                    settings.public_chat = !settings.public_chat;
                    ("public chat", settings.public_chat)
                }
                Toggle::FoundOre => {
                    settings.found_ore = !settings.found_ore;
                    ("found ore", settings.found_ore)
                }
                Toggle::Messages => {
                    settings.messages = !settings.messages;
                    ("private messages", settings.messages)
                }
                Toggle::Sounds => {
                    settings.sounds = !settings.sounds;
                    ("message sounds", settings.sounds)
                }
                Toggle::DeathMessages => {
                    settings.death_messages = !settings.death_messages;
                    ("death messages", settings.death_messages)
                }
                Toggle::Cobble => {
                    settings.cobble = !settings.cobble;
                    ("cobble pickup", settings.cobble)
                }
                Toggle::Lightning => {
                    settings.lightning = !settings.lightning;
                    ("death lightning", settings.lightning)
                }
            };
            (label, value)
        };

        if matches!(self.0, Toggle::Scoreboard) {
            if value {
                scoreboard::setup(&player);
            } else {
                scoreboard::teardown(&player);
            }
        }

        let status = if value { "&aenabled" } else { "&cdisabled" };
        reply(&sender, util::replace("&e<label> has been <status>&e.", "<label>", label)
            .replace("<status>", status));
        Ok(1)
    }
}

fn register_toggles(context: &Context) -> Result<(), String> {
    let toggles: [(&str, Toggle); 8] = [
        ("togglescoreboard", Toggle::Scoreboard),
        ("togglechat", Toggle::Chat),
        ("togglefoundore", Toggle::FoundOre),
        ("togglemessages", Toggle::Messages),
        ("togglesounds", Toggle::Sounds),
        ("toggledeathmessages", Toggle::DeathMessages),
        ("togglecobble", Toggle::Cobble),
        ("togglelightning", Toggle::Lightning),
    ];

    for (name, toggle) in toggles {
        let node = format!("lazarus:command.{name}");
        register_public_permission(context, &node, "Toggle a player setting")?;
        let tree = command(&[name], "Toggle a player setting").execute(ToggleExecutor(toggle));
        context.register_command(tree, &node);
    }

    Ok(())
}

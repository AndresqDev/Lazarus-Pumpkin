//! Base commands: `/lazarus` and the per-player setting toggles.

use pumpkin_plugin_api::command::{CommandError, CommandNode, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::{Context, Player, Server};

use super::{command, register_public_permission, reply};
use crate::utils::core;
use crate::scoreboard;
use crate::state::{self, state};

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
        crate::handlers::limiter::reload();
        crate::utils::core::refresh_spawn_centers(&server);
        crate::utils::core::apply_world_borders_from_config(&server);

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

/// A per-player setting toggle (`/toggle*` commands and the `/settings` GUI).
#[derive(Clone, Copy)]
pub(crate) enum Toggle {
    Scoreboard,
    Chat,
    FoundOre,
    Messages,
    Sounds,
    DeathMessages,
    Cobble,
    Lightning,
}

impl Toggle {
    /// Maps a `PLAYER_SETTINGS.SETTINGS.*.SETTING` token (`<messages>`, ...) to
    /// its toggle — `SettingsHandler#getSettingStatus` in the original.
    pub(crate) fn from_setting(setting: &str) -> Option<Toggle> {
        Some(match setting.to_lowercase().as_str() {
            "<messages>" => Toggle::Messages,
            "<sounds>" => Toggle::Sounds,
            "<publicchat>" => Toggle::Chat,
            "<foundore>" => Toggle::FoundOre,
            "<deathmessages>" => Toggle::DeathMessages,
            "<cobble>" => Toggle::Cobble,
            "<lightning>" => Toggle::Lightning,
            "<scoreboard>" => Toggle::Scoreboard,
            _ => return None,
        })
    }

    /// Maps a `PLAYER_SETTINGS.SETTINGS.*.COMMAND` (`/togglemessages`, ...) to
    /// its toggle: `/settings` ran the command through `player.chat(command)`.
    pub(crate) fn from_command(command: &str) -> Option<Toggle> {
        Some(match command.trim().trim_start_matches('/').to_lowercase().as_str() {
            "togglescoreboard" => Toggle::Scoreboard,
            "togglechat" => Toggle::Chat,
            "togglefoundore" => Toggle::FoundOre,
            "togglemessages" => Toggle::Messages,
            "togglesounds" => Toggle::Sounds,
            "toggledeathmessages" => Toggle::DeathMessages,
            "togglecobble" => Toggle::Cobble,
            "togglelightning" => Toggle::Lightning,
            _ => return None,
        })
    }

    /// Current value of the setting (`SettingsHandler#getSettingStatus`).
    pub(crate) fn status_of(self, settings: &crate::userdata::Settings) -> bool {
        match self {
            Toggle::Scoreboard => settings.scoreboard,
            Toggle::Chat => settings.public_chat,
            Toggle::FoundOre => settings.found_ore,
            Toggle::Messages => settings.messages,
            Toggle::Sounds => settings.sounds,
            Toggle::DeathMessages => settings.death_messages,
            Toggle::Cobble => settings.cobble,
            Toggle::Lightning => settings.lightning,
        }
    }

    /// The `TOGGLE_<X>_COMMAND` language section holding `TOGGLED_ON` /
    /// `TOGGLED_OFF` (`Toggle*.java` sent one of the two after flipping).
    fn language_key(self) -> &'static str {
        match self {
            Toggle::Scoreboard => "TOGGLE_SCOREBOARD_COMMAND",
            Toggle::Chat => "TOGGLE_PUBLIC_CHAT_COMMAND",
            Toggle::FoundOre => "TOGGLE_FOUND_ORE_COMMAND",
            Toggle::Messages => "TOGGLE_MESSAGES_COMMAND",
            Toggle::Sounds => "TOGGLE_SOUNDS_COMMAND",
            Toggle::DeathMessages => "TOGGLE_DEATHMESSAGES_COMMAND",
            Toggle::Cobble => "TOGGLE_COBBLE_COMMAND",
            Toggle::Lightning => "TOGGLE_LIGHTNING_COMMAND",
        }
    }
}

/// Flips a setting, applying the scoreboard side effect and sending the
/// original `TOGGLE_<X>_COMMAND.TOGGLED_ON`/`TOGGLED_OFF` message. Shared by the
/// toggle commands and the `/settings` GUI (which ran `player.chat(command)`).
pub(crate) fn toggle(player: &Player, toggle: Toggle) {
    let uuid = core::uuid_of(player);

    let value = {
        let mut guard = state().write().unwrap();
        let Some(user) = guard.userdata.get_mut(&uuid) else {
            return;
        };
        let settings = &mut user.settings;

        match toggle {
            Toggle::Scoreboard => settings.scoreboard = !settings.scoreboard,
            Toggle::Chat => settings.public_chat = !settings.public_chat,
            Toggle::FoundOre => settings.found_ore = !settings.found_ore,
            Toggle::Messages => settings.messages = !settings.messages,
            Toggle::Sounds => settings.sounds = !settings.sounds,
            Toggle::DeathMessages => settings.death_messages = !settings.death_messages,
            Toggle::Cobble => settings.cobble = !settings.cobble,
            Toggle::Lightning => settings.lightning = !settings.lightning,
        }

        toggle.status_of(settings)
    };

    // The original's updater cleared the board every cycle and re-rendered only
    // when the setting was on; the port renders once per second, so the change
    // is applied to the live board right away.
    if matches!(toggle, Toggle::Scoreboard) {
        if value {
            scoreboard::setup(player);
        } else {
            scoreboard::teardown(player);
        }
    }

    let message = {
        let guard = state().read().unwrap();

        format!(
            "{}{}",
            guard.raw.language.string("PREFIX"),
            guard.raw.language.string(&format!(
                "{}.{}",
                toggle.language_key(),
                if value { "TOGGLED_ON" } else { "TOGGLED_OFF" }
            ))
        )
    };
    core::send(player, message);
}

struct ToggleExecutor(Toggle);

impl CommandHandler for ToggleExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        // The original toggle commands extend `BaseCommand("...", true)`, so
        // console gets `COMMANDS.FOR_PLAYERS_ONLY` (`BaseCommand#checkPlayer`).
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        toggle(&player, self.0);
        Ok(1)
    }
}

fn register_toggles(context: &Context) -> Result<(), String> {
    // Names + aliases exactly as registered in the original
    // (`Toggle*.java#super(...)`).
    let toggles: [(&[&str], Toggle); 8] = [
        (
            &["togglescoreboard", "togglesb", "tscoreboard"],
            Toggle::Scoreboard,
        ),
        (&["togglechat", "tchat"], Toggle::Chat),
        (&["togglefoundore", "tfoundore"], Toggle::FoundOre),
        (
            &["togglemessages", "togglemsg", "tmsg", "togglepm", "tpm"],
            Toggle::Messages,
        ),
        (&["togglesounds", "tsounds"], Toggle::Sounds),
        (&["toggledeathmessages", "tdeathmessages"], Toggle::DeathMessages),
        (&["togglecobble", "tcobble", "cobble"], Toggle::Cobble),
        (&["togglelightning", "tlightning"], Toggle::Lightning),
    ];

    for (names, toggle) in toggles {
        let node = format!("lazarus:command.{}", names[0]);
        register_public_permission(context, &node, "Toggle a player setting")?;
        let tree = command(names, "Toggle a player setting").execute(ToggleExecutor(toggle));
        context.register_command(tree, &node);
    }

    Ok(())
}

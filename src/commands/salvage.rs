//! `/salvage` — port of `commands/SalvageCommand`.

use pumpkin_plugin_api::command::{CommandError, CommandSender, ConsumedArgs};
use pumpkin_plugin_api::commands::CommandHandler;
use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::{Context, Server};

use super::{command, register_public_permission, reply};
use crate::handlers::salvage;
use crate::state::state;

pub fn register(context: &Context) -> Result<(), String> {
    register_public_permission(context, "lazarus:command.salvage", "Salvage the held item")?;

    let root = command(&["salvage"], "Salvage the held item").execute(SalvageExecutor);
    context.register_command(root, "lazarus:command.salvage");
    Ok(())
}

/// `Language.PREFIX + <message>`.
fn prefixed(key: &str) -> String {
    let guard = state().read().unwrap();

    format!(
        "{}{}",
        guard.raw.language.string("PREFIX"),
        guard.raw.language.string(key)
    )
}

struct SalvageExecutor;

impl CommandHandler for SalvageExecutor {
    fn handle(&self, sender: CommandSender, _server: Server, _args: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            let message = state().read().unwrap().lang.for_players_only.clone();
            reply(&sender, &message);
            return Ok(0);
        };

        // `ItemUtils.isSalvageable(player.getItemInHand().getType())` — air is
        // not salvageable either (the 1.8 `getItemInHand` never returned null).
        let salvageable = player
            .get_item_in_hand(Hand::Right)
            .is_some_and(|item| salvage::is_salvageable(&item.get_registry_key()));

        if !salvageable {
            reply(&sender, prefixed("SALVAGE_COMMAND.NOT_SALVAGEABLE"));
            return Ok(1);
        }

        // `SalvageHandler#handleSalvage` re-reads the held item itself.
        salvage::handle_salvage(&player);
        Ok(1)
    }
}

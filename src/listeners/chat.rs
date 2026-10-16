//! Chat formatting + faction/ally chat.
//!
//! Port of `handlers.chat.ChatHandler` and the faction chat command. Pumpkin
//! delivers a `PlayerChatEvent` variant into which the plugin can supply its own
//! recipients/format; we cancel the vanilla event and re-emit the formatted message
//! so ignore lists and faction chat are respected.

use pumpkin_plugin_api::events::{EventData, EventHandler, PlayerChatEvent};
use pumpkin_plugin_api::Server;

use crate::core;
use crate::factions::ChatType;
use crate::state::state;
use crate::util;

pub struct ChatListener;

impl EventHandler<PlayerChatEvent> for ChatListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerChatEvent>) -> EventData<PlayerChatEvent> {
        let uuid = core::uuid_of(&event.player);
        let name = core::name_of(&event.player);

        let (chat_type, faction_id, faction_name) = {
            let guard = state().read().unwrap();
            match guard.factions.by_member(&uuid) {
                Some(faction) => {
                    let member = faction.member(&uuid);
                    let chat_type = member.map(|m| m.chat_type).unwrap_or(ChatType::Public);
                    (chat_type, Some(faction.id.clone()), Some(faction.name.clone()))
                }
                None => (ChatType::Public, None, None),
            }
        };

        // Leave the vanilla broadcast in place for public chat.
        if chat_type == ChatType::Public {
            return event;
        }

        let message = event.message.clone();
        let format = {
            let guard = state().read().unwrap();
            match chat_type {
                ChatType::Ally => guard.lang.ally_chat_format.clone(),
                _ => guard.lang.faction_chat_format.clone(),
            }
        };

        let formatted = util::replace_all(
            &format,
            &[("<player>", &name), ("<message>", &message)],
        );

        // Suppress the vanilla message and deliver our own.
        event.cancelled = true;

        match chat_type {
            ChatType::Faction => {
                if let Some(faction_id) = &faction_id {
                    let faction = state().read().unwrap().factions.get(faction_id).cloned();
                    if let Some(faction) = faction {
                        core::faction_message(&server, &faction, &formatted);
                    }
                }
            }
            ChatType::Ally => {
                let _ = faction_name;
                let recipients = {
                    let guard = state().read().unwrap();
                    guard
                        .factions
                        .get(faction_id.as_deref().unwrap_or(""))
                        .map(|f| f.allies.clone())
                        .unwrap_or_default()
                };
                for ally_id in recipients {
                    let ally = state().read().unwrap().factions.get(&ally_id).cloned();
                    if let Some(ally) = ally {
                        core::faction_message(&server, &ally, &formatted);
                    }
                }
                if let Some(faction_id) = faction_id
                    && let Some(faction) = state().read().unwrap().factions.get(&faction_id).cloned()
                {
                    core::faction_message(&server, &faction, &formatted);
                }
            }
            ChatType::Public => {}
        }

        event
    }
}

//! Vanish + staff mode — port of `staffmode.VanishManager` / `StaffModeManager`
//! (the parts that use only server-side visibility).
//!
//! Pumpkin exposes `Player::hide_player` / `show_player`, the modern equivalent of
//! the packet-based vanish in the original.

use std::collections::BTreeSet;

use pumpkin_plugin_api::{Player, Server};

#[derive(Debug, Default)]
pub struct StaffManager {
    pub vanished: BTreeSet<String>,
    pub staff: BTreeSet<String>,
}

impl StaffManager {
    pub fn is_vanished(&self, uuid: &str) -> bool {
        self.vanished.contains(uuid)
    }

    pub fn is_staff(&self, uuid: &str) -> bool {
        self.staff.contains(uuid)
    }

    /// Toggles vanish and updates visibility for everyone online. Returns the new state.
    pub fn toggle_vanish(&mut self, server: &Server, player: &Player) -> bool {
        let uuid = crate::utils::core::uuid_of(player);
        let now_vanished = if self.vanished.remove(&uuid) {
            false
        } else {
            self.vanished.insert(uuid.clone());
            true
        };

        for other in server.get_all_players() {
            if crate::utils::core::uuid_of(&other) == uuid {
                continue;
            }

            let Some(target) = lookup(server, &uuid) else {
                continue;
            };

            if now_vanished {
                other.hide_player(target);
            } else {
                other.show_player(target);
            }
        }

        now_vanished
    }

    /// Hides every currently vanished player from a newly joined player.
    pub fn apply_vanish_to(&self, server: &Server, joined: &Player) {
        let joined_uuid = crate::utils::core::uuid_of(joined);
        for vanished_uuid in &self.vanished {
            if *vanished_uuid == joined_uuid {
                continue;
            }
            if let Some(target) = lookup(server, vanished_uuid) {
                // Re-fetch a fresh handle for the specific viewer.
                if let Some(viewer) = lookup(server, &joined_uuid) {
                    viewer.hide_player(target);
                }
            }
        }
    }
}

fn lookup(server: &Server, uuid: &str) -> Option<Player> {
    pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
}

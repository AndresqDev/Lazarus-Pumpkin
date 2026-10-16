//! Deathban handling — port of `me.qiooip.lazarus.deathban.DeathbanManager`.
//!
//! Deathbans are stored on the player's [`Userdata`](crate::userdata::Userdata)
//! (`deathban_until`) so they survive restarts without touching Pumpkin's own
//! ban list. On join, an active deathban kicks the player; on death, either a life
//! is consumed or a new deathban is applied.

use pumpkin_plugin_api::Player;

use crate::state::state;
use crate::util;

/// Kicks the player if their deathban is still active. Returns `true` if kicked.
pub fn enforce_on_join(player: &Player) -> bool {
    let uuid = crate::core::uuid_of(player);

    let message = {
        let guard = state().read().unwrap();
        let Some(user) = guard.userdata.get(&uuid) else {
            return false;
        };
        if !user.is_deathbanned() {
            return false;
        }

        let time = util::format_duration(user.deathban_remaining());
        util::replace(&guard.lang.deathban_message, "<time>", &time)
    };

    if let Some(java) = player.as_java() {
        java.kick(pumpkin_plugin_api::player::JavaKickOptions::new(util::text(message)));
    }

    true
}

/// Applies the deathban (or consumes a life). Returns a message to send, if any.
pub fn apply_on_death(player: &Player) -> Option<String> {
    let uuid = crate::core::uuid_of(player);
    let world = crate::core::world_name(player);

    let mut guard = state().write().unwrap();
    let config = guard.config.deathban.clone();
    if !config.enabled {
        return None;
    }
    if !config.deathban_worlds.is_empty() && !config.deathban_worlds.iter().any(|w| w == &world) {
        return None;
    }

    let lang = guard.lang.clone();

    // Faction auto-revive: consume a faction life instead of deathbanning.
    let auto_revive = guard
        .factions
        .by_member(&uuid)
        .map(|f| f.id.clone())
        .filter(|id| {
            guard.factions.get(id).map(|f| f.auto_revive).unwrap_or(false)
                && guard.factions.get(id).map(|f| f.lives).unwrap_or(0) > 0
        });
    if let Some(id) = auto_revive {
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.lives -= 1;
        }
        return None;
    }

    let Some(user) = guard.userdata.get_mut(&uuid) else {
        return None;
    };

    if config.use_lives && user.lives > 0 {
        user.lives -= 1;
        let lives = user.lives;
        return Some(util::replace(&lang.deathban_lives_used, "<lives>", &lives.to_string()));
    }

    user.deathban(config.default_ban_time);
    let time = util::format_duration(config.default_ban_time as i64 * 60 * 1000);
    Some(util::replace(&lang.deathban_message, "<time>", &time))
}

/// Whether the player's deathban is still active.
pub fn is_death_banned(uuid: &str) -> bool {
    state()
        .read()
        .unwrap()
        .userdata
        .get(uuid)
        .map(|user| user.is_deathbanned())
        .unwrap_or(false)
}

/// Clears a player's deathban (`/f livesrevive`).
pub fn revive_player(uuid: &str) {
    if let Some(user) = state().write().unwrap().userdata.get_mut(uuid) {
        user.revive();
    }
}

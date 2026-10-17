//! Deathban handling — port of `me.qiooip.lazarus.deathban.DeathbanManager`.
//!
//! Faithful flow:
//! - On death, when the death location's faction is deathban (and the player
//!   lacks `lazarus.deathban.bypass`), the player is banned and the kick is
//!   scheduled ~20 ticks later (`Tasks.syncLater(() -> kickPlayer(...), 20L)`).
//! - On the first join attempt with an active ban, a player with lives is told
//!   to join again (`JOIN_AGAIN_FOR_REVIVE`); the second attempt consumes one
//!   life and lets them in. Without lives the ban message is shown.
//!
//! Deathbans are stored on the player's [`Userdata`](crate::userdata::Userdata)
//! (`deathban_until` + `deathban_reason`) so they survive restarts without
//! touching Pumpkin's own ban list.

use pumpkin_plugin_api::{Player, Server};

use crate::state::{state, AppState, PendingKick};
use crate::util;

/// Exact copy of the original `DEATHBAN.BAN_MESSAGE` default, used as a fallback
/// when the language file is missing the key.
const BAN_MESSAGE_FALLBACK: &str = "&eYou are &cdeathbanned &efor: &c<time>&7. \n\n &eReason: <reason>&e";
const JOIN_AGAIN_FALLBACK: &str = "&eYou have &b<amount> &elive(s). Please join again to use life.";

fn ban_message(guard: &AppState) -> String {
    let message = guard.raw.language.string("DEATHBAN.BAN_MESSAGE");
    if message.is_empty() { BAN_MESSAGE_FALLBACK.to_string() } else { message }
}

fn join_again_message(guard: &AppState) -> String {
    let message = guard.raw.language.string("DEATHBAN.JOIN_AGAIN_FOR_REVIVE");
    if message.is_empty() { JOIN_AGAIN_FALLBACK.to_string() } else { message }
}

fn kick(player: &Player, message: &str) {
    if let Some(java) = player.as_java() {
        java.kick(pumpkin_plugin_api::player::JavaKickOptions::new(util::text(message)));
    }
}

/// Kicks the player if their deathban is still active, or consumes a life on the
/// second join attempt (port of the original prelogin flow). Returns `true` when
/// the player must stay disconnected.
pub fn enforce_on_join(player: &Player) -> bool {
    let uuid = crate::core::uuid_of(player);

    enum Outcome {
        Allow,
        Kick(String),
    }

    let outcome = {
        let mut guard = state().write().unwrap();

        let banned = guard
            .userdata
            .get(&uuid)
            .filter(|user| user.is_deathbanned())
            .map(|user| (user.deathban_remaining(), user.deathban_reason(), user.lives));

        match banned {
            None => {
                // A clean join clears any stale confirmation. (The original only
                // removed it on a successful revive; clearing here avoids
                // consuming a life silently for a previous, expired ban.)
                guard.deathban_confirm.remove(&uuid);
                Outcome::Allow
            }
            Some((remaining, reason, lives)) if lives <= 0 => Outcome::Kick(util::replace(
                &util::replace(
                    &ban_message(&guard),
                    "<time>",
                    &util::format_duration_words(remaining),
                ),
                "<reason>",
                &reason,
            )),
            Some(_) if guard.deathban_confirm.contains(&uuid) => {
                if let Some(user) = guard.userdata.get_mut(&uuid) {
                    user.lives -= 1;
                    user.revive();
                }
                guard.deathban_confirm.remove(&uuid);
                Outcome::Allow
            }
            Some((remaining, reason, lives)) => {
                guard.deathban_confirm.insert(uuid.clone());
                let message = format!(
                    "{}\n\n{}",
                    util::replace(
                        &util::replace(
                            &ban_message(&guard),
                            "<time>",
                            &util::format_duration_words(remaining),
                        ),
                        "<reason>",
                        &reason,
                    ),
                    util::replace(&join_again_message(&guard), "<amount>", &lives.to_string()),
                );
                Outcome::Kick(message)
            }
        }
    };

    match outcome {
        Outcome::Allow => false,
        Outcome::Kick(message) => {
            kick(player, &message);
            true
        }
    }
}

/// Applies the deathban on death (port of `DeathbanManager#deathbanPlayer`), or
/// auto-revives using the faction's lives. When a ban is applied, the kick is
/// queued ~1s later via [`process_pending_kicks`].
pub fn apply_on_death(player: &Player, reason: &str) {
    let uuid = crate::core::uuid_of(player);
    let location = crate::core::location(player);

    // Host calls happen before taking the state lock (port convention).
    if player.has_permission("lazarus.deathban.bypass") {
        return;
    }

    let (enabled, ban_times, default_ban_time, kitmap_disables_deathban) = {
        let guard = state().read().unwrap();
        (
            guard.config.deathban.enabled,
            guard.config.deathban.ban_times.clone(),
            guard.config.deathban.default_ban_time,
            guard.config.kitmap.enabled && guard.config.kitmap.disable_deathban,
        )
    };
    if !enabled {
        return;
    }
    // Kitmap can disable deathbans entirely (port of the check in
    // `DeathbanManager#deathbanPlayer`).
    if kitmap_disables_deathban {
        return;
    }

    // `DeathbanManager#getBanTime`: first matching permission wins.
    let mut millis = default_ban_time as i64 * 60_000;
    for entry in &ban_times {
        if player.has_permission(&entry.permission) {
            millis = entry.minutes as i64 * 60_000;
            break;
        }
    }

    let now = util::now_millis();
    let mut guard = state().write().unwrap();

    // `ClaimManager.getInstance().getFactionAt(player).isDeathban()`.
    let territory = crate::core::territory_at_guard(
        &guard,
        &location.world,
        location.block_x(),
        location.block_z(),
    );
    if !crate::core::territory_deathbans_guard(&guard, &territory) {
        return;
    }

    // Faction auto-revive: consume a faction life instead of deathbanning
    // (port of the `PlayerDeathbanEvent` auto-revive handler).
    let auto_revive = guard
        .factions
        .by_member(&uuid)
        .map(|faction| faction.id.clone())
        .filter(|id| {
            guard
                .factions
                .get(id)
                .map(|faction| faction.auto_revive && faction.lives > 0)
                .unwrap_or(false)
        });
    if let Some(id) = auto_revive {
        if let Some(faction) = guard.factions.get_mut(&id) {
            faction.lives -= 1;
        }
        return;
    }

    let message = util::replace(
        &util::replace(
            &ban_message(&guard),
            "<time>",
            &util::format_duration_words(millis),
        ),
        "<reason>",
        reason,
    );

    if let Some(user) = guard.userdata.get_mut(&uuid) {
        user.deathban(millis, reason.to_string());
    }

    guard.pending_kicks.push(PendingKick { uuid, message, at: now + 1000 });
}

/// Fires the kicks scheduled by [`apply_on_death`].
pub fn process_pending_kicks(server: &Server) {
    let now = util::now_millis();

    let ready: Vec<PendingKick> = {
        let mut guard = state().write().unwrap();
        let mut ready = Vec::new();

        let mut index = 0;
        while index < guard.pending_kicks.len() {
            if guard.pending_kicks[index].at <= now {
                ready.push(guard.pending_kicks.remove(index));
            } else {
                index += 1;
            }
        }

        ready
    };

    for pending in ready {
        let player = pumpkin_plugin_api::uuid::parse(&pending.uuid)
            .and_then(|id| server.get_player_by_uuid(id));
        if let Some(player) = player {
            kick(&player, &pending.message);
        }
    }
}

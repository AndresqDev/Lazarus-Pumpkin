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
use crate::utils;

/// Exact copy of the original `DEATHBAN.BAN_MESSAGE` default, used as a fallback
/// when the language file is missing the key.
const BAN_MESSAGE_FALLBACK: &str = "&eYou are &cdeathbanned &efor: &c<time>&7. \n\n &eReason: <reason>&e";
const JOIN_AGAIN_FALLBACK: &str = "&eYou have &b<amount> &elive(s). Please join again to use life.";
const EOTW_MESSAGE_FALLBACK: &str =
    "&eYou died while &4EOTW &ewas running! Therefore you cannot join until next map!";

fn ban_message(guard: &AppState) -> String {
    let message = guard.raw.language.string("DEATHBAN.BAN_MESSAGE");
    if message.is_empty() { BAN_MESSAGE_FALLBACK.to_string() } else { message }
}

fn join_again_message(guard: &AppState) -> String {
    let message = guard.raw.language.string("DEATHBAN.JOIN_AGAIN_FOR_REVIVE");
    if message.is_empty() { JOIN_AGAIN_FALLBACK.to_string() } else { message }
}

fn eotw_message(guard: &AppState) -> String {
    let message = guard.raw.language.string("DEATHBAN.EOTW_MESSAGE");
    if message.is_empty() { EOTW_MESSAGE_FALLBACK.to_string() } else { message }
}

fn kick(player: &Player, message: &str) {
    if let Some(java) = player.as_java() {
        java.kick(pumpkin_plugin_api::player::JavaKickOptions::new(utils::text(message)));
    }
}

/// Kicks the player if their deathban is still active, or consumes a life on the
/// second join attempt (port of the original prelogin flow). Returns `true` when
/// the player must stay disconnected.
pub fn enforce_on_join(player: &Player) -> bool {
    let uuid = crate::utils::core::uuid_of(player);

    enum Outcome {
        Allow,
        Kick(String),
    }

    let outcome = {
        let mut guard = state().write().unwrap();

        // `onAsyncPlayerPreLoginEvent`: during EOTW every stored deathban record
        // (active or already expired) keeps the player out until the next map.
        let eotw_lockout = guard.eotw_active
            && guard
                .userdata
                .get(&uuid)
                .is_some_and(|user| user.deathban_until.is_some());

        if eotw_lockout {
            Outcome::Kick(eotw_message(&guard))
        } else {
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
                Some((remaining, reason, lives)) if lives <= 0 => Outcome::Kick(utils::replace(
                    &utils::replace(
                        &ban_message(&guard),
                        "<time>",
                        &utils::format_duration_words(remaining),
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
                        utils::replace(
                            &utils::replace(
                                &ban_message(&guard),
                                "<time>",
                                &utils::format_duration_words(remaining),
                            ),
                            "<reason>",
                            &reason,
                        ),
                        utils::replace(&join_again_message(&guard), "<amount>", &lives.to_string()),
                    );
                    Outcome::Kick(message)
                }
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
    // `DeathbanManager#deathbanPlayer` saved the inventory first thing, before
    // every early return (bypass permission included).
    tracing::info!("LOADTEST_DEATHBAN save_inventory start");
    crate::handlers::restore::save_inventory_upon_death(player);
    tracing::info!("LOADTEST_DEATHBAN save_inventory done");

    let uuid = crate::utils::core::uuid_of(player);
    let location = crate::utils::core::location(player);
    tracing::info!("LOADTEST_DEATHBAN config start uuid={uuid}");

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
    tracing::info!("LOADTEST_DEATHBAN config done uuid={uuid} millis={millis}");
    // The original only stores a ban when the resolved time is not zero.
    if millis == 0 {
        return;
    }

    let mut guard = state().write().unwrap();
    store_deathban(
        &mut guard,
        &uuid,
        &location.world,
        location.block_x(),
        location.block_y(),
        location.block_z(),
        millis,
        reason,
    );
}

/// `DeathbanManager#getBanTime` resolved for a still-online player (the combat
/// logger captures it at logout, while the permissions are valid).
pub fn ban_time_millis(player: &Player) -> i64 {
    let (ban_times, default_ban_time) = {
        let guard = state().read().unwrap();
        (
            guard.config.deathban.ban_times.clone(),
            guard.config.deathban.default_ban_time,
        )
    };

    let mut millis = default_ban_time as i64 * 60_000;
    for entry in &ban_times {
        if player.has_permission(&entry.permission) {
            millis = entry.minutes as i64 * 60_000;
            break;
        }
    }
    millis
}

/// `DeathbanManager#deathbanPlayer` for the offline combat-logger victim: the
/// original's `hasPermission` on a logged-out player always resolved to false,
/// so the bypass check is skipped and the ban time captured at logout is used.
pub fn apply_logger_death(
    uuid: &str,
    world: &str,
    block_x: i32,
    block_y: i32,
    block_z: i32,
    millis: i64,
    reason: &str,
) {
    let (enabled, kitmap_disables_deathban) = {
        let guard = state().read().unwrap();
        (
            guard.config.deathban.enabled,
            guard.config.kitmap.enabled && guard.config.kitmap.disable_deathban,
        )
    };
    if !enabled || kitmap_disables_deathban {
        return;
    }
    if millis == 0 {
        return;
    }

    let mut guard = state().write().unwrap();
    store_deathban(&mut guard, uuid, world, block_x, block_y, block_z, millis, reason);
}

/// Shared tail of `DeathbanManager#deathbanPlayer`: territory check, faction
/// auto-revive, stored ban and the ~1s kick.
fn store_deathban(
    guard: &mut AppState,
    uuid: &str,
    world: &str,
    block_x: i32,
    block_y: i32,
    block_z: i32,
    millis: i64,
    reason: &str,
) {
    let now = utils::now_millis();

    // `ClaimManager.getInstance().getFactionAt(player).isDeathban()`.
    let territory = crate::utils::core::territory_at_guard(guard, world, block_x, block_z);
    if !crate::utils::core::territory_deathbans_guard(guard, &territory) {
        return;
    }

    // Faction auto-revive: consume a faction life instead of deathbanning
    // (port of the `PlayerDeathbanEvent` auto-revive handler).
    let auto_revive = guard
        .factions
        .by_member(uuid)
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

    // `DeathbanManager#kickPlayer`: during EOTW the kick shows the EOTW message
    // instead of the ban message.
    let message = if guard.eotw_active {
        eotw_message(guard)
    } else {
        utils::replace(
            &utils::replace(
                &ban_message(guard),
                "<time>",
                &utils::format_duration_words(millis),
            ),
            "<reason>",
            reason,
        )
    };

    if let Some(user) = guard.userdata.get_mut(uuid) {
        user.deathban(
            millis,
            reason.to_string(),
            format!("{block_x}, {block_y}, {block_z} ({world})"),
        );
    }

    guard
        .pending_kicks
        .push(PendingKick { uuid: uuid.to_string(), message, at: now + 1000 });
}

/// Fires the kicks scheduled by [`apply_on_death`].
pub fn process_pending_kicks(server: &Server) {
    let now = utils::now_millis();

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

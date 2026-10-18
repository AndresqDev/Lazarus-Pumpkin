//! Rank revive and the online donators announcer — port of
//! `me.qiooip.lazarus.handlers.rank.*`.
//!
//! Fidelity notes:
//! * `RankReviveTimer` was a `PlayerTimer` with `persistable = true` and
//!   `FormatType.MILLIS_TO_MINUTES`; its cooldown is ported to
//!   [`TimerType::RankRevive`], the `<seconds>` placeholder receives the same
//!   dynamic clock (`MM:SS`, `HH:MM:SS` past an hour) and the expiry message is
//!   announced by the 1-second tick ([`crate::tasks`] `announce_expired`).
//! * The announcer's `Tasks.asyncTimer(..., 0L, interval * 20L)` ran on an
//!   async pool; the port uses the tick scheduler (delay 10, like the other
//!   ported repeating tasks).

use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::{Player, Server};

use crate::config::yaml::ConfigFile;
use crate::state::state;
use crate::timer::TimerType;
use crate::utils::{self, core};

/// One `RANK_REVIVE_COMMAND.<n>` entry (`RankReviveHandler.RankReviveData`).
#[derive(Clone)]
pub struct RankReviveEntry {
    pub rank_name: String,
    pub permission: String,
    pub cooldown_minutes: i64,
}

/// One `ONLINE_RANK_ANNOUNCER.<n>` entry
/// (`RankAnnouncerHandler.RankAnnouncementData`).
#[derive(Clone)]
pub struct RankAnnouncementEntry {
    pub permission: String,
    pub rank_prefix: String,
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

/// `RankReviveHandler#loadRankReviveData` (run once at startup, like the
/// original constructor).
pub fn load_rank_revive_entries(config: &ConfigFile) -> Vec<RankReviveEntry> {
    config
        .keys("RANK_REVIVE_COMMAND")
        .into_iter()
        .filter_map(|key| {
            let prefix = format!("RANK_REVIVE_COMMAND.{key}");
            let permission = config.string(&format!("{prefix}.PERMISSION"));
            // A missing PERMISSION matched nobody in the original
            // (`hasPermission(null)`); skipping it keeps the host call safe.
            if permission.is_empty() {
                return None;
            }

            Some(RankReviveEntry {
                rank_name: config.string(&format!("{prefix}.RANK_NAME")),
                permission,
                cooldown_minutes: config.int(&format!("{prefix}.COOLDOWN")),
            })
        })
        .collect()
}

/// `RankAnnouncerHandler#loadRankAnnouncementData`.
pub fn load_announcement_entries(config: &ConfigFile) -> Vec<RankAnnouncementEntry> {
    config
        .keys("ONLINE_RANK_ANNOUNCER")
        .into_iter()
        .map(|key| {
            let prefix = format!("ONLINE_RANK_ANNOUNCER.{key}");
            RankAnnouncementEntry {
                permission: config.string(&format!("{prefix}.PERMISSION")),
                rank_prefix: config.string(&format!("{prefix}.RANK_PREFIX")),
            }
        })
        .collect()
}

/// `RankReviveHandler#performCommand`.
pub fn perform_rank_revive(server: &Server, player: &Player, target_uuid: &str, target_name: &str) {
    let entries = state().read().unwrap().rank_revives.clone();
    let Some(data) = entries
        .into_iter()
        .find(|entry| player.has_permission(&entry.permission))
    else {
        core::send(player, prefixed("RANK_REVIVE_COMMAND.NO_PERMISSION"));
        return;
    };

    let uuid = core::uuid_of(player);

    // `RankReviveTimer#isActive` + `getDynamicTimeLeft`.
    let remaining = state()
        .read()
        .unwrap()
        .timers
        .remaining(&uuid, TimerType::RankRevive);
    if remaining > 0 {
        let message = utils::replace(
            &prefixed("RANK_REVIVE_COMMAND.COOLDOWN_ACTIVE"),
            "<seconds>",
            &utils::format_clock(remaining),
        );
        core::send(player, &message);
        return;
    }

    // `DeathbanManager#isDeathBanned`.
    let deathbanned = state()
        .read()
        .unwrap()
        .userdata
        .get(target_uuid)
        .is_some_and(|user| user.is_deathbanned());
    if !deathbanned {
        let message = utils::replace(
            &prefixed("RANK_REVIVE_COMMAND.NOT_DEATHBANNED"),
            "<player>",
            target_name,
        );
        core::send(player, &message);
        return;
    }

    {
        let mut guard = state().write().unwrap();
        if let Some(user) = guard.userdata.get_mut(target_uuid) {
            user.revive();
        }

        // `PlayerTimer#activate(uuid, seconds)` is a no-op for a non-positive
        // delay; the original cooldown came from the config in minutes.
        let seconds = data.cooldown_minutes.max(0) as u64 * 60;
        if seconds > 0 {
            guard.timers.activate(&uuid, TimerType::RankRevive, seconds);
        }
    }

    crate::state::save_all();

    // `Messages.sendMessage(String)`: the console plus every online player.
    let broadcast = utils::replace_all(
        &state()
            .read()
            .unwrap()
            .raw
            .language
            .string("RANK_REVIVE_COMMAND.BROADCAST_MESSAGE"),
        &[
            ("<rankName>", &data.rank_name),
            ("<player>", &core::name_of(player)),
            ("<target>", target_name),
        ],
    );
    crate::handlers::timer::sotw::broadcast(server, &broadcast);
}

/// `RankAnnouncerHandler#sendRankAnnouncementMessage`.
pub fn announce(server: &Server) {
    let (entries, delimiter, lines) = {
        let guard = state().read().unwrap();
        (
            guard.rank_announcements.clone(),
            guard.raw.language.string("ONLINE_RANK_ANNOUNCER.DELIMITER"),
            guard.raw.language.string_list("ONLINE_RANK_ANNOUNCER.MESSAGE"),
        )
    };
    if entries.is_empty() || lines.is_empty() {
        return;
    }

    // `handlePlayerAnnouncementMessage`: a player matching several ranks is
    // listed once per rank, in config order.
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); entries.len()];
    for player in server.get_all_players() {
        if player.get_permission_level() != PermissionLevel::Zero {
            continue;
        }

        for (index, entry) in entries.iter().enumerate() {
            if player.has_permission(&entry.permission) {
                groups[index].push(format!("{}{}", entry.rank_prefix, core::name_of(&player)));
            }
        }
    }

    let joined = groups
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(&delimiter);
    if joined.is_empty() {
        return;
    }

    // `sendGlobalMessage`: every online player, no console.
    for line in lines {
        let message = utils::replace(&line, "<donators>", &joined);
        for player in server.get_all_players() {
            core::send(&player, &message);
        }
    }
}

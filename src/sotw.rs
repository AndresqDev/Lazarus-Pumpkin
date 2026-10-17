//! SOTW ("Start Of The World") system — port of `handlers.timer.SotwHandler`
//! and `timer.scoreboard.SotwTimer`.
//!
//! While the SOTW timer runs every player is damage-immune (`/sotw enable` lets
//! a player opt out) and players inside the safezone can be hidden from
//! everyone (`hide-players`). The timer itself lives in
//! [`crate::timers::TimerManager`] (`sotw_until`), which is persisted in
//! `timers.json` — the equivalent of the original `timers.yml` `SotwTimer`
//! entry.
//!
//! Deviations from the original, forced by the host API, are documented in the
//! individual helpers: no nametag team colour (`SOTW_COLOR`) and no
//! auto-inventory drop routing.

use std::collections::BTreeSet;

use pumpkin_plugin_api::{Player, Server};

use crate::core;
use crate::state::state;
use crate::util;

/// Runtime SOTW state that the original kept in memory only.
#[derive(Debug, Default)]
pub struct SotwState {
    /// Players that used `/sotw enable` (opted out of the damage protection).
    pub enabled_players: BTreeSet<String>,
    /// Whether safezone players are hidden; initialised from
    /// `SOTW_TIMER.HIDE_PLAYERS` and toggled by `/sotw hideplayers`.
    pub hide_players: bool,
}

impl SotwState {
    pub fn new(hide_players: bool) -> Self {
        Self { enabled_players: BTreeSet::new(), hide_players }
    }
}

/// `SotwHandler#isActive`.
pub fn is_active() -> bool {
    state().read().unwrap().timers.sotw_active()
}

/// Remaining time in millis (0 when not running).
pub fn remaining() -> i64 {
    state().read().unwrap().timers.sotw_remaining()
}

/// `SotwHandler#isPlayerSotwEnabled`.
pub fn is_player_enabled(uuid: &str) -> bool {
    state().read().unwrap().sotw.enabled_players.contains(uuid)
}

/// `SotwHandler#isUnderSotwProtection`.
pub fn is_under_protection(uuid: &str) -> bool {
    let guard = state().read().unwrap();
    guard.timers.sotw_active() && !guard.sotw.enabled_players.contains(uuid)
}

/// True when the safezone hiding feature is currently effective.
pub fn hiding_active() -> bool {
    let guard = state().read().unwrap();
    guard.timers.sotw_active() && guard.sotw.hide_players
}

/// `SotwHandler#startSotwTimer`: starts the timer (broadcasting `SOTW.STARTED`)
/// and force-hides safezone players when the feature is enabled. Returns
/// `false` when a timer is already running.
pub fn start(server: &Server, seconds: u64) -> bool {
    if is_active() {
        return false;
    }

    if state().read().unwrap().sotw.hide_players {
        toggle_invisibility(server, Some(true));
    }

    state().write().unwrap().timers.activate_sotw(seconds);

    let message = state()
        .read()
        .unwrap()
        .lang
        .sotw_started
        .replace("<time>", &util::format_hms(seconds));
    broadcast(server, &message);
    true
}

/// `SotwHandler#stopSotwTimer`: clears the opt-outs, shows everyone, cancels
/// the timer and broadcasts `SOTW.ENDED`. Returns `false` when not running.
pub fn stop(server: &Server) -> bool {
    if !is_active() {
        return false;
    }

    state().write().unwrap().sotw.enabled_players.clear();
    show_all(server);
    state().write().unwrap().timers.cancel_sotw();

    let message = state().read().unwrap().lang.sotw_ended.clone();
    broadcast(server, &message);
    true
}

/// Called from the 1-second task; ends the timer naturally (port of the
/// `SystemTimer` expiry callable in `SotwTimer#activate`).
pub fn check_expiry(server: &Server) {
    let expired = {
        let guard = state().read().unwrap();
        guard
            .timers
            .sotw_until
            .map(|until| until <= util::now_millis())
            .unwrap_or(false)
    };
    if !expired {
        return;
    }

    show_all(server);
    state().write().unwrap().timers.cancel_sotw();

    let message = state().read().unwrap().lang.sotw_ended.clone();
    broadcast(server, &message);
}

/// `SotwHandler#enableSotwForPlayer`: opts a player out of the protection.
pub fn enable_for_player(player: &Player) {
    let uuid = core::uuid_of(player);

    if !is_active() {
        let (prefix, message) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.lang.sotw_not_running.clone(),
            )
        };
        core::send(player, &format!("{prefix}{message}"));
        return;
    }

    let already = {
        let mut guard = state().write().unwrap();
        !guard.sotw.enabled_players.insert(uuid)
    };
    if already {
        let (prefix, message) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.lang.sotw_already_enabled.clone(),
            )
        };
        core::send(player, &format!("{prefix}{message}"));
        return;
    }

    let (prefix, message) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("PREFIX"),
            guard.lang.sotw_enabled.clone(),
        )
    };
    core::send(player, &format!("{prefix}{message}"));
}

/// `SotwHandler#toggleSotwInvisibility`: updates the flag (or forces `hide`)
/// and applies visibility. Returns the new state.
pub fn toggle_invisibility(server: &Server, hide: Option<bool>) -> bool {
    let hide_players = {
        let mut guard = state().write().unwrap();
        guard.sotw.hide_players = hide.unwrap_or(!guard.sotw.hide_players);
        guard.sotw.hide_players
    };

    if hide_players {
        let targets = safezone_uuids(server);
        for viewer in server.get_all_players() {
            let viewer_uuid = core::uuid_of(&viewer);
            for target_uuid in &targets {
                if *target_uuid == viewer_uuid {
                    continue;
                }
                // `Player` handles are single-use: fetch a fresh handle per call.
                if let Some(target) = lookup(server, target_uuid) {
                    viewer.hide_player(target);
                }
            }
        }
    } else {
        show_all(server);
    }

    hide_players
}

/// `SotwHandler#showSotwInvisiblePlayers`: shows everyone again, skipping
/// vanished staff.
pub fn show_all(server: &Server) {
    let visible = visible_uuids(server);
    for viewer in server.get_all_players() {
        let viewer_uuid = core::uuid_of(&viewer);
        for target_uuid in &visible {
            if *target_uuid == viewer_uuid {
                continue;
            }
            if let Some(target) = lookup(server, target_uuid) {
                viewer.show_player(target);
            }
        }
    }
}

/// Per-player visibility update when their faction changes (port of the
/// private `toggleSotwInvisibility(Player, Faction)` used by the move /
/// teleport / world-change handlers).
pub fn update_player_visibility(server: &Server, player: &Player, in_safezone: bool) {
    if !hiding_active() {
        return;
    }

    let uuid = core::uuid_of(player);
    if state().read().unwrap().staff.is_vanished(&uuid) {
        return;
    }

    for viewer in server.get_all_players() {
        if core::uuid_of(&viewer) == uuid {
            continue;
        }
        let Some(target) = lookup(server, &uuid) else {
            continue;
        };
        if in_safezone {
            viewer.hide_player(target);
        } else {
            viewer.show_player(target);
        }
    }
}

/// `SotwHandler#onPlayerJoin`: hides safezone players from the joiner and
/// applies the joiner's own visibility based on their faction.
pub fn on_player_join(server: &Server, player: &Player, in_safezone: bool) {
    if !hiding_active() {
        return;
    }

    let joiner_uuid = core::uuid_of(player);
    for target_uuid in safezone_uuids(server) {
        if target_uuid == joiner_uuid {
            continue;
        }
        if let Some(target) = lookup(server, target_uuid.as_str()) {
            player.hide_player(target);
        }
    }

    update_player_visibility(server, player, in_safezone);
}

/// UUIDs of online players standing inside the safezone, skipping vanished
/// staff.
fn safezone_uuids(server: &Server) -> Vec<String> {
    snapshot(server)
        .into_iter()
        .filter(|(_, safezone, vanished)| *safezone && !*vanished)
        .map(|(player, _, _)| core::uuid_of(&player))
        .collect()
}

/// UUIDs of online players that are visible (not vanished staff).
fn visible_uuids(server: &Server) -> Vec<String> {
    snapshot(server)
        .into_iter()
        .filter(|(_, _, vanished)| !*vanished)
        .map(|(player, _, _)| core::uuid_of(&player))
        .collect()
}

fn lookup(server: &Server, uuid: &str) -> Option<Player> {
    pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
}

/// `Messages.sendMessage`: sends to the console and every online player.
pub fn broadcast(server: &Server, message: &str) {
    tracing::info!("{message}");
    for player in server.get_all_players() {
        core::send(&player, message);
    }
}

/// Online players with the flags the visibility logic needs:
/// `(player, in_safezone, vanished)`. Taken under a single state read so no
/// lock is held while host calls run.
fn snapshot(server: &Server) -> Vec<(Player, bool, bool)> {
    let players = server.get_all_players();
    let guard = state().read().unwrap();

    players
        .into_iter()
        .map(|player| {
            let uuid = core::uuid_of(&player);
            let location = core::location(&player);
            let safezone = matches!(
                core::territory_at_guard(
                    &guard,
                    &location.world,
                    location.block_x(),
                    location.block_z()
                ),
                core::Territory::Safezone
            );
            let vanished = guard.staff.is_vanished(&uuid);
            (player, safezone, vanished)
        })
        .collect()
}

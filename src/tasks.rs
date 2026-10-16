//! Repeating server task: timers, DTR regen, warmup teleports, glass visuals,
//! sidebar and tab list.
//!
//! Consolidates the various Lazarus `Tasks` / `Timer` / `GlassUpdater` /
//! `TabUpdater` scheduler loops into a single 1-second tick (glass walls are
//! refreshed twice per second via a second task) scheduled through Pumpkin.

use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{Context, Player, Server};

use crate::core;
use crate::glass;
use crate::scoreboard;
use crate::state::{state, AppState};
use crate::tab;
use crate::timers::{PendingAction, TimerType};
use crate::util;

/// Starts the repeating tasks (called from `on_load`).
pub fn start(context: &Context) {
    context.schedule_repeating_task(20, 20, |server| tick(&server));
    // Re-send fake blocks that were broken/placed over (next tick, after the
    // server's own block update has been applied).
    context.schedule_repeating_task(1, 1, |server| {
        state().write().unwrap().glass.process_pending_regen(&server);
    });
    // Client-side glass walls refresh a bit faster, like the original 100ms loop.
    context.schedule_repeating_task(10, 10, |server| glass_tick(&server));

    // Periodic autosave (config: factions.auto_save_minutes).
    let minutes = state().read().unwrap().config.factions.auto_save_minutes.max(1);
    let period_ticks = minutes * 60 * 20;
    context.schedule_repeating_task(period_ticks, period_ticks, |_server| crate::state::save_all());

    // Recreate hologram display entities once worlds are available.
    context.schedule_delayed_task(20, |server| {
        let mut guard = state().write().unwrap();
        let AppState { holograms, hologram_entities, .. } = &mut *guard;
        crate::hologram::respawn_all(&server, holograms, hologram_entities);
    });

    // Apply the configured world borders (square, infinite on Y), centred on spawn.
    context.schedule_delayed_task(20, |server| {
        crate::core::refresh_spawn_centers(&server);
        crate::core::setup_spawn_claim(&server);
        crate::core::apply_world_borders_from_config(&server);
    });
}

fn tick(server: &Server) {
    let expired: Vec<(String, TimerType)> = {
        let mut guard = state().write().unwrap();
        let expired = guard.timers.purge_expired();

        // DTR regeneration (port of DtrRegenTimer).
        let config = guard.config.clone();
        let per_second = config.factions.regen_per_minute / 60.0;
        let frozen_ids: Vec<String> = guard.timers.faction_freeze.keys().cloned().collect();
        for faction in guard.factions.player_factions_mut() {
            if frozen_ids.contains(&faction.id) {
                continue;
            }
            let max = faction.max_dtr(&config);
            if faction.dtr < max {
                faction.dtr = (faction.dtr + per_second).min(max);
            }
        }

        expired
    };

    announce_expired(server, &expired);
    process_pending(server);

    for player in server.get_all_players() {
        scoreboard::update(&player, server);
        tab::update(&player, server);
    }

    // Persist the tab state file once per tick (the native plugin diffs it).
    tab::flush();
}

/// Faster loop that only refreshes client-side block visuals.
fn glass_tick(server: &Server) {
    for player in server.get_all_players() {
        let mut guard = state().write().unwrap();
        let AppState { glass, factions, claims, config, timers, .. } = &mut *guard;
        glass::update_walls(glass, &player, claims, factions, timers);
        glass::update_map(glass, &player, claims, config);
        // Safety net: re-send every active fake block so a broken/placed real block
        // (or any desync) is corrected. Only positions that pass the conditions are
        // in `entries`, so red walls stay correct.
        glass.refresh_all(server);
    }
}

fn announce_expired(server: &Server, expired: &[(String, TimerType)]) {
    for (uuid, timer_type) in expired {
        let message = match timer_type {
            TimerType::CombatTag => Some("&aYou are no longer combat tagged."),
            TimerType::PvpProtection => Some("&cYour PvP protection has expired."),
            _ => None,
        };
        let Some(message) = message else {
            continue;
        };
        if let Some(player) = lookup(server, uuid) {
            core::send(&player, message);
        }
    }
}

/// Executes warmup teleports whose timers elapsed.
fn process_pending(server: &Server) {
    let ready: Vec<(String, PendingAction)> = {
        let mut guard = state().write().unwrap();
        let now = util::now_millis();

        let mut ready: Vec<(String, PendingAction)> = Vec::new();
        for (uuid, action) in guard.timers.pending.clone() {
            if action.expiry <= now {
                guard.timers.clear_pending(&uuid);
                ready.push((uuid, action));
            }
        }
        ready
    };

    for (uuid, action) in ready {
        let Some(player) = lookup(server, &uuid) else {
            continue;
        };

        match action.timer_type {
            TimerType::Home => {
                if let Some(home) = action.target {
                    teleport(server, &player, &home);
                    core::send(&player, "&aYou have been teleported to your faction home.");
                }
            }
            TimerType::Stuck => {
                let world_name = core::world_name(&player);
                if let Some(world) = server.get_world_by_name(&world_name) {
                    let y = world.get_top_block_y(0, 0) as f64 + 1.0;
                    player.teleport((0.5, y, 0.5), None, None, world);
                    core::send(&player, "&aYou have been teleported to a safe location.");
                }
            }
            _ => {}
        }
    }
}

fn teleport(server: &Server, player: &Player, target: &crate::factions::Location) {
    if let Some(world) = server.get_world_by_name(&target.world) {
        player.teleport(
            (target.x, target.y, target.z),
            Some(target.yaw),
            Some(target.pitch),
            world,
        );
    }
}

fn lookup(server: &Server, uuid: &str) -> Option<Player> {
    pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
}

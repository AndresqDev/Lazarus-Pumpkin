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
    // Re-send all active fake blocks frequently (fast resync of the render).
    context.schedule_repeating_task(2, 2, |server| {
        state().write().unwrap().glass.refresh_all(&server);
    });
    // Disable mob AI globally (sweep so already-spawned mobs are covered too).
    context.schedule_repeating_task(100, 100, |server| {
        for world in server.get_all_worlds() {
            for entity in world.get_entities() {
                if let Some(mob) = entity.as_mob() {
                    mob.set_ai_disabled(true);
                }
            }
        }
    });

    // Periodic autosave (config: factions.auto_save_minutes).
    let minutes = state().read().unwrap().config.factions.auto_save_minutes.max(1);
    let period_ticks = minutes * 60 * 20;
    context.schedule_repeating_task(period_ticks, period_ticks, |_server| crate::state::save_all());

    // Clear stale "infinite" class effects once the server is up.
    context.schedule_delayed_task(20, |server| {
        for player in server.get_all_players() {
            crate::classes::remove_infinite_effects(&player);
        }
    });

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

    // PvP classes: per-tick armour check + Bard holdables/clickable energy.
    {
        let mut guard = state().write().unwrap();
        let players = server.get_all_players();

        // Precompute the per-player flags (avoids borrowing fields twice).
        let flags: Vec<(bool, bool)> = players
            .iter()
            .map(|player| {
                let uuid = crate::core::uuid_of(player);
                let location = crate::core::location(player);
                (
                    guard
                        .timers
                        .is_active(&uuid, crate::timers::TimerType::PvpProtection),
                    matches!(
                        crate::core::territory_at_guard(
                            &guard,
                            &location.world,
                            location.block_x(),
                            location.block_z()
                        ),
                        crate::core::Territory::Safezone
                    ),
                )
            })
            .collect();

        let crate::state::AppState { classes, raw, factions, .. } = &mut *guard;
        for (player, (pvp_protected, in_safezone)) in players.iter().zip(flags) {
            let uuid = crate::core::uuid_of(player);
            let faction_id = factions.by_member(&uuid).map(|f| f.id.clone());
            classes.tick_player(player, &server, &raw.language, &raw.config, faction_id.as_deref());
            classes.bard_tick(
                player,
                &server,
                factions,
                &raw.config,
                pvp_protected,
                in_safezone,
            );
            // Mage energy regen.
            if classes.active.get(&uuid) == Some(&crate::classes::ClassKind::Mage) {
                let max = classes
                    .classes
                    .get(&crate::classes::ClassKind::Mage)
                    .map(|c| c.max_energy)
                    .unwrap_or(120.0);
                let energy = classes.mage_energy.entry(uuid.clone()).or_insert(max);
                *energy = (*energy + crate::classes::POWER_GAIN).min(max);
            }
        }
        classes.tick_warmups(&server, &raw.language, &raw.config, factions);
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
                if let Some(target) = action.target {
                    teleport(server, &player, &target);
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

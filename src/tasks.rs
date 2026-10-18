//! Repeating server task: timers, DTR regen, warmup teleports, glass visuals,
//! sidebar and tab list.
//!
//! Consolidates the various Lazarus `Tasks` / `Timer` / `GlassUpdater` /
//! `TabUpdater` scheduler loops into a single 1-second tick (glass walls are
//! refreshed twice per second via a second task) scheduled through Pumpkin.

use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{Context, Player, Server};

use crate::utils::core;
use crate::glass;
use crate::scoreboard;
use crate::state::{state, AppState};
use crate::tab;
use crate::timer::{PendingAction, TimerType};
use crate::utils;

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
    // Block gameplay handlers (`handlers.block`): one second passes of the
    // cobweb remover and the autosmel replacement queue.
    context.schedule_repeating_task(20, 20, |server| crate::handlers::block::tick_second(&server));
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

    // Frequent flush of the gameplay-volatile stores (timers with millisecond
    // precision, factions, claims, userdata): bounds data loss on a hard crash
    // to half a minute while staying far too small to affect TPS.
    context.schedule_repeating_task(600, 600, |_server| crate::state::save_hot());

    // DTR regeneration (port of `DtrRegenTimer`): every non-frozen player
    // faction below its maximum gains `REGEN_PER_MINUTE` once per minute.
    context.schedule_repeating_task(1200, 1200, |_server| dtr_regen_tick());

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
        crate::utils::core::refresh_spawn_centers(&server);
        crate::utils::core::apply_world_borders_from_config(&server);
    });

    // Kitmap item clear task (port of `KitmapHandler.ItemClearTask`): every
    // `KITMAP_CLEAR_ITEMS.INTERVAL` seconds, deleting every dropped item in
    // every world. (The original also ran once at tick 0, when no items can
    // exist yet.)
    {
        let (enabled, interval) = {
            let guard = state().read().unwrap();
            (
                guard.config.kitmap.enabled && guard.config.kitmap.clear_items_enabled,
                guard.config.kitmap.clear_items_interval.max(1),
            )
        };
        if enabled {
            let period = interval * 20;
            context.schedule_repeating_task(period, period, |server| clear_items(&server));
        }
    }

    // Online donators announcer (`RankAnnouncerHandler#setupAnnouncementTask`):
    // the original scheduled an async timer with delay 0 and period
    // `INTERVAL * 20`; the port waits half a second so the first pass lands
    // with the server up.
    {
        let (enabled, interval) = {
            let guard = state().read().unwrap();
            (
                guard.raw.config.bool("ONLINE_RANK_ANNOUNCER_ENABLED"),
                guard.raw.config.int("ONLINE_RANK_ANNOUNCER_INTERVAL"),
            )
        };
        if enabled {
            let period = (interval.max(1) * 20) as u64;
            context.schedule_repeating_task(10, period, |server| {
                crate::handlers::rank::announce(&server);
            });
        }
    }
}

/// Deletes every item entity in every world (port of
/// `KitmapHandler.ItemClearTask#run`).
fn clear_items(server: &Server) {
    for world in server.get_all_worlds() {
        for entity in world.get_entities() {
            if entity.get_type() == pumpkin_plugin_api::EntityType::Item {
                entity.remove();
            }
        }
    }
}

fn tick(server: &Server) {
    let expired: Vec<(String, TimerType)> = {
        let mut guard = state().write().unwrap();
        // Expired custom timers are removed by their scheduled task in the
        // original (`CustomTimer#scheduleExpiry`).
        guard.timers.purge_expired_custom();
        // Stale killer candidates (the window is 5s).
        let now = crate::utils::now_millis();
        guard
            .last_damagers
            .retain(|_, (_, at)| now - *at <= 10_000);
        guard.timers.purge_expired()
    };

    announce_expired(server, &expired);
    crate::handlers::timer::sotw::check_expiry(server);
    process_pending(server);
    process_protection_ejects(server);
    process_respawn_fixes(server);
    crate::deathban::process_pending_kicks(server);
    crate::handlers::logger::process_pending_removals(server);

    for player in server.get_all_players() {
        scoreboard::update(&player, server);
        tab::update(&player, server);
    }

    // PvP classes: per-tick armour check + Bard holdables/clickable energy.
    {
        let mut guard = state().write().unwrap();
        let players = server.get_all_players();

        // Precompute the per-player flags (avoids borrowing fields twice).
        let flags: Vec<(bool, bool, bool)> = players
            .iter()
            .map(|player| {
                let uuid = crate::utils::core::uuid_of(player);
                let location = crate::utils::core::location(player);
                (
                    guard
                        .timers
                        .is_active(&uuid, crate::timer::TimerType::PvpProtection),
                    guard.timers.sotw_active() && !guard.sotw.enabled_players.contains(&uuid),
                    matches!(
                        crate::utils::core::territory_at_guard(
                            &guard,
                            &location.world,
                            location.block_x(),
                            location.block_z()
                        ),
                        crate::utils::core::Territory::Safezone
                    ),
                )
            })
            .collect();

        let crate::state::AppState { classes, raw, factions, .. } = &mut *guard;
        for (player, (pvp_protected, sotw_protected, in_safezone)) in players.iter().zip(flags) {
            let uuid = crate::utils::core::uuid_of(player);
            let faction_id = factions.by_member(&uuid).map(|f| f.id.clone());
            classes.tick_player(player, &server, &raw.language, &raw.config, faction_id.as_deref());
            classes.bard_tick(
                player,
                &server,
                factions,
                &raw.config,
                pvp_protected,
                sotw_protected,
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

/// Port of `DtrRegenTimer#handleDtrUpdate`: each minute, every non-frozen
/// player faction below its maximum DTR regenerates `REGEN_PER_MINUTE` in one
/// step (the original schedules the task at a one-minute fixed rate). The
/// resulting value is rounded through [`crate::factions::Faction::set_dtr`] so
/// the tab list never shows floating-point noise.
fn dtr_regen_tick() {
    let mut guard = state().write().unwrap();
    let config = guard.config.clone();
    let frozen_ids: Vec<String> = guard.timers.faction_freeze.keys().cloned().collect();

    for faction in guard.factions.player_factions_mut() {
        if frozen_ids.contains(&faction.id) {
            continue;
        }
        let max = faction.max_dtr(&config);
        if faction.dtr < max {
            let amount = faction.dtr + config.factions.regen_per_minute;
            faction.set_dtr(&config, amount);
        }
    }
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
            TimerType::CombatTag => Some("&aYou are no longer combat tagged.".to_string()),
            TimerType::PvpProtection => {
                Some(state().read().unwrap().lang.pvp_protection_expired.clone())
            }
            TimerType::RankRevive => {
                let guard = state().read().unwrap();
                Some(format!(
                    "{}{}",
                    guard.raw.language.string("PREFIX"),
                    guard.raw.language.string("RANK_REVIVE_COMMAND.COOLDOWN_EXPIRED")
                ))
            }
            _ => None,
        };
        let Some(message) = message else {
            continue;
        };
        if let Some(player) = lookup(server, uuid) {
            core::send(&player, &message);
        }
    }
}

/// Executes warmup teleports whose timers elapsed.
fn process_pending(server: &Server) {
    let ready: Vec<(String, PendingAction)> = {
        let mut guard = state().write().unwrap();
        let now = utils::now_millis();

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
            TimerType::Logout => {
                // `LogoutTimer#activate` completion: the quit counts as safe
                // (`player.setMetadata("logout", ...)`) and the player is kicked
                // (`LogoutHandler` reads the flag in the quit handler).
                crate::handlers::logger::finish_logout(&player);
            }
            _ => {}
        }
    }
}

/// Teleports protected players out of claims they cannot enter (delayed respawn
/// ejections; port of the safe-location teleport in
/// `PvpProtHandler#onPlayerRespawn`).
fn process_protection_ejects(server: &Server) {
    let pending: Vec<String> = {
        let mut guard = state().write().unwrap();
        std::mem::take(&mut guard.protection_ejects).into_iter().collect()
    };

    let mut retry: Vec<String> = Vec::new();

    for uuid in pending {
        let Some(player) = lookup(server, &uuid) else {
            // Offline: keep the flag so the ejection runs after they log back in.
            retry.push(uuid);
            continue;
        };

        let safe = {
            let guard = state().read().unwrap();
            if !guard.timers.is_active(&uuid, TimerType::PvpProtection) {
                continue;
            }
            let location = core::location(&player);
            let Some(owner) =
                guard.claims.faction_at(&location.world, location.block_x(), location.block_z())
            else {
                continue;
            };
            let can_enter = guard.raw.config.bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");
            let deny = guard
                .factions
                .get(owner)
                .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                .unwrap_or(false);
            if !deny {
                continue;
            }
            guard
                .claims
                .get_safe_location(&location.world, location.block_x(), location.block_z())
        };

        let (world_name, safe_x, safe_z) = safe;
        if let Some(world) = server.get_world_by_name(&world_name) {
            let (_, y, _) = player.get_position();
            player.teleport((safe_x as f64 + 0.5, y, safe_z as f64 + 0.5), None, None, world);
        }

        let (prefix, message) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("FACTION_PREFIX"),
                guard.lang.pvp_protection_teleport_due_to_claim.clone(),
            )
        };
        core::send(&player, &format!("{prefix}{message}"));
    }

    if !retry.is_empty() {
        state().write().unwrap().protection_ejects.extend(retry);
    }
}

/// Teleports players whose respawn landed in a different world back to the
/// configured spawn. Only reachable with a legacy respawn point in another
/// dimension (kept in the player NBT); new ones cannot be created
/// (`SpawnChangeListener`) and the position override covers all other cases
/// (`SpawnLocationListener`). Port of the unconditional respawn location set
/// in `DynamicEventHandler#onPlayerRespawn`.
fn process_respawn_fixes(server: &Server) {
    let pending: Vec<String> = {
        let mut guard = state().write().unwrap();
        std::mem::take(&mut guard.respawn_fixes).into_iter().collect()
    };
    if pending.is_empty() {
        return;
    }

    let mut retry: Vec<String> = Vec::new();

    for uuid in pending {
        let Some(player) = lookup(server, &uuid) else {
            // Offline (e.g. kicked by the deathban): keep the flag for later.
            retry.push(uuid);
            continue;
        };

        let Some((world, (x, y, z), yaw, pitch)) = core::configured_spawn(server) else {
            continue;
        };
        if world.get_name() == core::world_name(&player) {
            continue;
        }
        player.teleport((x, y, z), Some(yaw), Some(pitch), world);
    }

    if !retry.is_empty() {
        state().write().unwrap().respawn_fixes.extend(retry);
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

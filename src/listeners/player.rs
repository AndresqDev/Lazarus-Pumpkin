//! Player lifecycle listeners (port of `factions.listeners.PlayerEventListener`
//! and the join/quit/timer handlers).

use pumpkin_plugin_api::events::{
    EventData, EventHandler, PlayerDeathEvent, PlayerJoinEvent, PlayerLeaveEvent, PlayerMoveEvent,
    PlayerRespawnEvent, PlayerTeleportEvent,
};
use pumpkin_plugin_api::{Player, Server};

use crate::core;
use crate::factions::Location;
use crate::scoreboard;
use crate::state::{state, ChunkState};
use crate::timers::TimerType;
use crate::util;

pub struct JoinListener;

impl EventHandler<PlayerJoinEvent> for JoinListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerJoinEvent>) -> EventData<PlayerJoinEvent> {
        let uuid = core::uuid_of(&event.player);
        crate::classes::remove_infinite_effects(&event.player);
        let name = core::name_of(&event.player);

        let (welcome_enabled, welcome_message, protect_seconds, show_info, first_join, in_safezone) = {
            let mut guard = state().write().unwrap();
            let config = guard.config.clone();
            let first_join = guard.userdata.get(&uuid).is_none();
            guard.userdata.get_or_create(uuid.clone(), name.clone(), &config);

            let location = core::location(&event.player);
            let in_safezone = matches!(
                core::territory_at_guard(
                    &guard,
                    &location.world,
                    location.block_x(),
                    location.block_z()
                ),
                core::Territory::Safezone
            );

            (
                config.join_welcome_message_enabled,
                guard.lang.join_welcome.clone(),
                config.timers.pvp_protection_seconds,
                config.factions.show_info_on_join,
                first_join,
                in_safezone,
            )
        };

        if welcome_enabled {
            let message = util::replace(&welcome_message, "<player>", &name);
            event.join_message = util::text(message);
        }

        // PvP protection (port of `PvpProtHandler#onPlayerJoin`): first joins are
        // awarded the full timer — banked while inside the safezone — while
        // returning players only resume whatever they had banked.
        {
            let mut guard = state().write().unwrap();
            if first_join {
                guard.timers.activate_pvp(&uuid, protect_seconds, in_safezone);
            } else if !in_safezone {
                guard.timers.resume_pvp(&uuid);
            }
        }

        // Deathban enforcement (port of DeathbanManager join handling).
        if crate::deathban::enforce_on_join(&event.player) {
            return event;
        }

        // Hide vanished staff from the joiner.
        state().read().unwrap().staff.apply_vanish_to(&server, &event.player);

        // SOTW: hide safezone players from the joiner (port of
        // `SotwHandler#onPlayerJoin`).
        crate::sotw::on_player_join(&server, &event.player, in_safezone);

        scoreboard::setup(&event.player);
        crate::tab::setup(&event.player, &server);

        if show_info {
            send_faction_info(&event.player, &uuid);
        }

        event
    }
}

pub struct LeaveListener;

impl EventHandler<PlayerLeaveEvent> for LeaveListener {
    fn handle(&self, _server: Server, event: EventData<PlayerLeaveEvent>) -> EventData<PlayerLeaveEvent> {
        let uuid = core::uuid_of(&event.player);

        scoreboard::teardown(&event.player);
        crate::tab::forget(&uuid);

        let mut guard = state().write().unwrap();
        guard.chunk_cache.remove(&uuid);
        guard.selection.forget(&uuid);
        guard.glass.forget(&uuid);
        // PvP protection banks while offline (port of `PvpProtHandler#onPlayerQuit`).
        guard.timers.bank_pvp(&uuid);

        event
    }
}

pub struct MoveListener;

impl EventHandler<PlayerMoveEvent> for MoveListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerMoveEvent>) -> EventData<PlayerMoveEvent> {
        let uuid = core::uuid_of(&event.player);

        let (x, _y, z) = event.to_position;
        let block_x = x.floor() as i32;
        let block_z = z.floor() as i32;
        let world = core::world_name(&event.player);
        let (cx, cz) = util::chunk_key(block_x, block_z);

        let territory = core::territory_at(&world, block_x, block_z);
        let viewer_faction = core::faction_id_of(&uuid);
        let display = core::territory_display(&territory, viewer_faction.as_deref());

        // Combat-tagged players may not enter the safezone (port of the original
        // `PlayerEventListener#onPlayerMove`), and the entering message matches
        // `CLAIM_MESSAGES.ENTERING`.
        let (combat, was_safezone, is_safezone, deny_message, entering_message, leaving_message) = {
            let guard = state().read().unwrap();

            let previous = guard.chunk_cache.get(&uuid).cloned();

            let (was_safezone, prev_display, prev_deathban) = match &previous {
                Some(p) => {
                    let territory = core::territory_at_guard(&guard, &p.world, p.cx * 16, p.cz * 16);
                    let deathban = match &territory {
                        core::Territory::Faction(id) => guard
                            .factions
                            .get(id)
                            .map(|f| if f.deathban { "&cDeathban" } else { "&aNon-Deathban" })
                            .unwrap_or("&aNon-Deathban"),
                        _ => "&aNon-Deathban",
                    };
                    (
                        matches!(territory, core::Territory::Safezone),
                        core::territory_display_guard(&guard, &territory, viewer_faction.as_deref()),
                        deathban.to_string(),
                    )
                }
                None => (false, String::new(), "&aNon-Deathban".to_string()),
            };

            let deathban = match &territory {
                core::Territory::Faction(id) => guard
                    .factions
                    .get(id)
                    .map(|f| if f.deathban { "&cDeathban" } else { "&aNon-Deathban" })
                    .unwrap_or("&aNon-Deathban"),
                _ => "&aNon-Deathban",
            };

            let entering = util::replace(
                &util::replace(&guard.lang.entering_territory, "<faction>", &display),
                "<deathban>",
                deathban,
            );
            let leaving = util::replace(
                &util::replace(&guard.lang.leaving_territory, "<faction>", &prev_display),
                "<deathban>",
                &prev_deathban,
            );

            (
                guard.timers.is_active(&uuid, TimerType::CombatTag),
                was_safezone,
                matches!(territory, core::Territory::Safezone),
                guard.lang.safezone_deny_enter.clone(),
                entering,
                leaving,
            )
        };

        if combat && is_safezone && !was_safezone {
            event.cancelled = true;
            core::send(&event.player, deny_message);
            return event;
        }

        // PvP protection: entering another player's claim (or a non-safezone
        // system claim) is denied; wilderness, warzone, safezone and roads are
        // walkable. (Combat tag is handled separately above: it only blocks the
        // safezone.)
        let block_enter = {
            let guard = state().read().unwrap();
            if !guard.timers.is_active(&uuid, TimerType::PvpProtection) {
                false
            } else {
                let can_enter = guard
                    .raw
                    .config
                    .bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");
                match &territory {
                    core::Territory::Faction(id) => guard
                        .factions
                        .get(id)
                        .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                        .unwrap_or(false),
                    _ => false,
                }
            }
        };

        if block_enter {
            event.cancelled = true;
            return event;
        }

        // PvP protection banking: time only ticks outside the safezone (port of
        // the `PvpProtTimer#pause` calls on faction/world changes).
        {
            let mut guard = state().write().unwrap();
            if is_safezone {
                guard.timers.bank_pvp(&uuid);
            } else {
                guard.timers.resume_pvp(&uuid);
            }
        }

        // SOTW visibility follows the safezone boundary (port of
        // `SotwHandler#toggleSotwInvisibility(Player, Faction)`).
        if is_safezone != was_safezone {
            crate::sotw::update_player_visibility(&server, &event.player, is_safezone);
        }

        let announce = {
            let mut guard = state().write().unwrap();
            let previous = guard.chunk_cache.get(&uuid).cloned();
            let territory_changed = previous.as_ref().is_some_and(|p| p.territory != display);
            guard.chunk_cache.insert(
                uuid.clone(),
                ChunkState { world: world.clone(), cx, cz, territory: display.clone() },
            );
            territory_changed
        };

        if announce {
            if !leaving_message.is_empty() {
                core::send(&event.player, leaving_message);
            }
            core::send(&event.player, entering_message);
        }

        // Cancel warmup teleports if the player moved too far (Home/Stuck warmups).
        let cancelled = {
            let mut guard = state().write().unwrap();
            let pending = guard.timers.pending(&uuid).cloned();
            match pending {
                Some(action) => {
                    let dx = x - action.origin_x;
                    let dz = z - action.origin_z;
                    let moved = (dx * dx + dz * dz).sqrt();
                    let radius = match action.timer_type {
                        TimerType::Home => 3.0,
                        TimerType::Stuck => guard.config.timers.stuck_movement_radius,
                        _ => 3.0,
                    };
                    if moved > radius {
                        guard.timers.clear_pending(&uuid);
                        true
                    } else {
                        false
                    }
                }
                None => false,
            }
        };

        if cancelled {
            core::send(&event.player, "&cTeleport cancelled, you moved!");
        }

        event
    }
}

pub struct DeathListener;

impl EventHandler<PlayerDeathEvent> for DeathListener {
    fn handle(&self, server: Server, event: EventData<PlayerDeathEvent>) -> EventData<PlayerDeathEvent> {
        let uuid = core::uuid_of(&event.player);
        let name = core::name_of(&event.player);
        let world = core::world_name(&event.player);

        let mut messages: Vec<(String, String)> = Vec::new();

        {
            let mut guard = state().write().unwrap();
            let config = guard.config.clone();

            // Update stats (port of Userdata.updateDeathStats).
            if let Some(user) = guard.userdata.get_mut(&uuid) {
                user.add_death(format!("{name} died in {world}"));
            }

            guard.timers.cancel(&uuid, TimerType::CombatTag);
            // Death removes PvP protection entirely (port of `PvpProtHandler#onPlayerDeath`).
            guard.timers.cancel_pvp(&uuid);

            // DTR loss for the victim's faction (port of the faction death handlers).
            let faction_id = guard.factions.by_member(&uuid).map(|f| f.id.clone());
            if let Some(faction_id) = faction_id
                && let Some(faction) = guard.factions.get_mut(&faction_id)
            {
                let loss = match core::dimension_of(&world) {
                    core::Dimension::Overworld => config.factions.dtr_death_loss.overworld,
                    core::Dimension::Nether => config.factions.dtr_death_loss.nether,
                    core::Dimension::End => config.factions.dtr_death_loss.end,
                };
                faction.dtr = (faction.dtr - loss).max(config.factions.min_dtr);

                let dtr = faction.dtr;
                messages.push((
                    faction_id.clone(),
                    format!("&cYour faction lost &e{loss} &cDTR. Now: &e{dtr:.2}"),
                ));
            }
        }

        for (faction_id, message) in messages {
            let faction = state().read().unwrap().factions.get(&faction_id).cloned();
            if let Some(faction) = faction {
                core::faction_message(&server, &faction, &message);
            }
        }

        // Deathban / life consumption (port of DeathbanManager death handling).
        if let Some((message, banned)) = crate::deathban::apply_on_death(&event.player) {
            core::send(&event.player, &message);
            if banned {
                if let Some(java) = event.player.as_java() {
                    java.kick(pumpkin_plugin_api::player::JavaKickOptions::new(util::text(&message)));
                }
            }
        }

        // Drop the claim wand (port of the death handling in SelectionManager).
        {
            let mut guard = state().write().unwrap();
            guard.selection.cancel_local(&event.player);
            guard.glass.clear_type(&event.player, crate::glass::GlassType::ClaimSelection);
        }

        event
    }
}

/// Re-awards PvP protection on respawn and flags protected players that
/// respawned inside a claim they may not enter (port of
/// `PvpProtHandler#onPlayerRespawn`).
pub struct RespawnListener;

impl EventHandler<PlayerRespawnEvent> for RespawnListener {
    fn handle(&self, _server: Server, event: EventData<PlayerRespawnEvent>) -> EventData<PlayerRespawnEvent> {
        let uuid = core::uuid_of(&event.player);
        let (x, _y, z) = event.position;
        let world = event.respawned_world.get_name();
        let block_x = x.floor() as i32;
        let block_z = z.floor() as i32;

        let protect_seconds = state().read().unwrap().config.timers.pvp_protection_seconds;
        let (in_safezone, deny) = {
            let guard = state().read().unwrap();
            let in_safezone = matches!(
                core::territory_at_guard(&guard, &world, block_x, block_z),
                core::Territory::Safezone
            );
            let deny = if in_safezone {
                false
            } else {
                let can_enter = guard.raw.config.bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");
                guard
                    .claims
                    .faction_at(&world, block_x, block_z)
                    .and_then(|owner| guard.factions.get(owner))
                    .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                    .unwrap_or(false)
            };
            (in_safezone, deny)
        };

        {
            let mut guard = state().write().unwrap();
            guard.timers.cancel_pvp(&uuid);
            guard.timers.activate_pvp(&uuid, protect_seconds, in_safezone);
            if deny {
                // Ejected on the next tick, once the respawn teleport has been applied.
                guard.protection_ejects.insert(uuid.clone());
            }
            // Keep the territory cache in sync with the new position.
            guard.chunk_cache.remove(&uuid);
        }

        event
    }
}

/// Denies teleports into claims while PvP protection is active (port of
/// `PvpProtHandler#onPlayerTeleport`; the enderpearl refund is not ported),
/// banks/resumes the timer depending on the destination and applies SOTW
/// visibility (port of `SotwHandler#onPlayerTeleport`).
pub struct TeleportListener;

impl EventHandler<PlayerTeleportEvent> for TeleportListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerTeleportEvent>) -> EventData<PlayerTeleportEvent> {
        let uuid = core::uuid_of(&event.player);
        let (x, _y, z) = event.to_position;
        let block_x = x.floor() as i32;
        let block_z = z.floor() as i32;
        let world = core::world_name(&event.player);

        let (in_safezone, deny, protected) = {
            let guard = state().read().unwrap();
            let in_safezone = matches!(
                core::territory_at_guard(&guard, &world, block_x, block_z),
                core::Territory::Safezone
            );
            let protected = guard.timers.is_active(&uuid, TimerType::PvpProtection);
            let deny = if protected && !in_safezone {
                let can_enter = guard.raw.config.bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");
                guard
                    .claims
                    .faction_at(&world, block_x, block_z)
                    .and_then(|owner| guard.factions.get(owner))
                    .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                    .unwrap_or(false)
            } else {
                false
            };
            (in_safezone, deny, protected)
        };

        if deny {
            event.cancelled = true;
            let (prefix, message) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.lang.pvp_protection_deny_teleport.clone(),
                )
            };
            core::send(&event.player, &format!("{prefix}{message}"));
            return event;
        }

        if protected {
            let mut guard = state().write().unwrap();
            if in_safezone {
                guard.timers.bank_pvp(&uuid);
            } else {
                guard.timers.resume_pvp(&uuid);
            }
        }

        // SOTW: update visibility for the destination (no-op when the timer is
        // not running or hiding is disabled).
        crate::sotw::update_player_visibility(&server, &event.player, in_safezone);

        event
    }
}

fn send_faction_info(player: &Player, uuid: &str) {
    let info = {
        let guard = state().read().unwrap();
        guard.factions.by_member(uuid).map(|faction| {
            format!(
                "&eFaction: &c{} &7| &eDTR: &c{:.2} &7| &eMembers: &c{}",
                faction.name,
                faction.dtr,
                faction.members.len()
            )
        })
    };

    if let Some(message) = info {
        core::send(player, message);
    }
}

/// Convenience used by the `f home` command and `faction_home` teleports.
pub fn faction_home(uuid: &str) -> Option<Location> {
    let guard = state().read().unwrap();
    guard.factions.by_member(uuid).and_then(|faction| faction.home.clone())
}

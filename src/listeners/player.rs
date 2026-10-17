//! Player lifecycle listeners (port of `factions.listeners.PlayerEventListener`
//! and the join/quit/timer handlers).

use pumpkin_plugin_api::events::{
    EventData, EventHandler, PlayerDeathEvent, PlayerJoinEvent, PlayerLeaveEvent, PlayerMoveEvent,
    PlayerRespawnEvent, PlayerSpawnChangeEvent, PlayerSpawnLocationEvent, PlayerTeleportEvent,
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

        let (
            welcome_enabled,
            welcome_message,
            protect_seconds,
            show_info,
            first_join,
            kitmap_skips_pvp,
        ) = {
            let mut guard = state().write().unwrap();
            let config = guard.config.clone();
            let first_join = guard.userdata.get(&uuid).is_none();
            guard.userdata.get_or_create(uuid.clone(), name.clone(), &config);

            (
                config.join_welcome_message_enabled,
                guard.lang.join_welcome.clone(),
                config.timers.pvp_protection_seconds,
                config.factions.show_info_on_join,
                first_join,
                config.kitmap.enabled && !config.kitmap.pvp_timer_enabled,
            )
        };

        if welcome_enabled {
            let message = util::replace(&welcome_message, "<player>", &name);
            event.join_message = util::text(message);
        }

        // First-join spawn teleport (port of `PvpProtHandler#onPlayerJoin` and,
        // when kitmap mode skips that handler, `KitmapHandler#onPlayerJoin`).
        if first_join {
            teleport_to_join_spawn(&server, &event.player);
        }

        let in_safezone = {
            let guard = state().read().unwrap();
            let location = core::location(&event.player);
            matches!(
                core::territory_at_guard(
                    &guard,
                    &location.world,
                    location.block_x(),
                    location.block_z()
                ),
                core::Territory::Safezone
            )
        };

        // PvP protection (port of `PvpProtHandler#onPlayerJoin`): first joins are
        // awarded the full timer — banked while inside the safezone — while
        // returning players only resume whatever they had banked. Kitmap without
        // the PvP timer skips the whole block.
        if !kitmap_skips_pvp {
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

/// First-join teleport (port of `PvpProtHandler#onPlayerJoin` /
/// `KitmapHandler#onPlayerJoin`): new players are sent to `WORLD_SPAWN` from
/// `utilities.yml`.
///
/// The original falls back to `player.getWorld().getSpawnLocation()` when the
/// entry is unset; on Pumpkin the host has *already* placed the player on the
/// world spawn (surface `top + 1`, i.e. the same spot without the stale Y kept
/// in `level.dat`), so the fallback branch only has to leave them there.
fn teleport_to_join_spawn(server: &Server, player: &Player) {
    if let Some((world, (x, y, z), yaw, pitch)) = core::configured_spawn(server) {
        player.teleport((x, y, z), Some(yaw), Some(pitch), world);
    }
}

/// Forces every respawn onto the configured `WORLD_SPAWN` (port of
/// `DynamicEventHandler#onPlayerRespawn`'s `event.setRespawnLocation(spawn)`).
///
/// The host applies `spawn-pos` to the respawn teleport right before sending
/// the respawn packet, so this covers players with no respawn point as well as
/// the host's own world-spawn resolution. The rotation of the configured spawn
/// is not settable here (the event only carries the position); the first-join
/// teleport applies it exactly.
pub struct SpawnLocationListener;

impl EventHandler<PlayerSpawnLocationEvent> for SpawnLocationListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<PlayerSpawnLocationEvent>,
    ) -> EventData<PlayerSpawnLocationEvent> {
        if event.cancelled {
            return event;
        }

        if let Some((_world, (x, y, z), _yaw, _pitch)) = core::configured_spawn(&server) {
            event.spawn_pos = (x, y, z);
        }

        event
    }
}

/// Beds and respawn anchors must not store spawn points: the original always
/// respawns players at the spawn, and a stored point can additionally drag the
/// host into another dimension (a charged anchor resolves *before* the respawn
/// position override). `forced` entries come from the admin `/spawnpoint`
/// command and are left alone — they cannot change the respawn destination
/// either (see [`SpawnLocationListener`]).
pub struct SpawnChangeListener;

impl EventHandler<PlayerSpawnChangeEvent> for SpawnChangeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerSpawnChangeEvent>,
    ) -> EventData<PlayerSpawnChangeEvent> {
        if !event.forced {
            event.cancelled = true;
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

/// Port of `PlayerEventListener#checkMovement`: only runs when the block
/// coordinates (X/Y/Z) change. Cancels home/stuck warmups and, when the faction
/// at the destination differs from the one at the origin, pauses/resumes the PvP
/// protection timer and sends the claim leave/enter messages.
fn check_movement(
    player: &Player,
    uuid: &str,
    from: (f64, f64, f64),
    to: (f64, f64, f64),
    world: &str,
) {
    let (fx, fy, fz) = from;
    let (tx, ty, tz) = to;

    if fx.floor() == tx.floor() && fy.floor() == ty.floor() && fz.floor() == tz.floor() {
        return;
    }

    let (home_message, stuck_message, announce) = {
        let mut guard = state().write().unwrap();

        // `checkFactionHomeAndStuckOnMove(player)`: any block move cancels the
        // home warmup; the stuck warmup only cancels when the player moved
        // farther than `FACTION_STUCK.ALLOWED_MOVEMENT_RADIUS` (squared value,
        // compared against the 3D distance just like `hasExceededStuckLimit`).
        let mut home_message = None;
        let mut stuck_message = None;
        let prefix = guard.raw.language.string("FACTION_PREFIX");

        match guard.timers.pending(uuid).cloned() {
            Some(action) if action.timer_type == TimerType::Home => {
                guard.timers.clear_pending(uuid);
                home_message = Some(format!(
                    "{prefix}{}",
                    guard
                        .raw
                        .language
                        .string("FACTIONS.HOME_COMMAND.TELEPORT_CANCELLED.MOVED")
                ));
            }
            Some(action) if action.timer_type == TimerType::Stuck => {
                let dx = tx - action.origin_x;
                let dy = ty - action.origin_y;
                let dz = tz - action.origin_z;
                let radius = guard.config.timers.stuck_movement_radius;
                if dx * dx + dy * dy + dz * dz > radius * radius {
                    guard.timers.clear_pending(uuid);
                    stuck_message = Some(format!(
                        "{prefix}{}",
                        guard
                            .raw
                            .language
                            .string("FACTIONS.STUCK_COMMAND.TELEPORT_CANCELLED.MOVED")
                    ));
                }
            }
            _ => {}
        }

        // `ClaimManager#getFactionAt(from/to)`. The cached chunk (kept in sync by
        // moves and teleports) is the origin so teleports announce correctly; the
        // event origin is the fallback for the first move after a join.
        let from_territory = match guard.chunk_cache.get(uuid) {
            Some(cached) => {
                core::territory_at_guard(&guard, &cached.world, cached.cx * 16, cached.cz * 16)
            }
            None => core::territory_at_guard(&guard, world, fx.floor() as i32, fz.floor() as i32),
        };
        let to_territory =
            core::territory_at_guard(&guard, world, tx.floor() as i32, tz.floor() as i32);

        let mut announce = None;

        if from_territory != to_territory {
            // `PvpProtTimer#pause(player, toFaction.isSafezone(), to)`.
            if core::is_safezone_guard(&guard, &to_territory) {
                guard.timers.bank_pvp(uuid);
            } else {
                guard.timers.resume_pvp(uuid);
            }

            let viewer_faction = guard.factions.by_member(uuid).map(|f| f.id.clone());

            let leaving = util::replace(
                &util::replace(
                    &guard.raw.language.string("FACTIONS.CLAIM_MESSAGES.LEAVING"),
                    "<faction>",
                    &core::territory_display_guard(
                        &guard,
                        &from_territory,
                        viewer_faction.as_deref(),
                    ),
                ),
                "<deathban>",
                &core::deathban_string_guard(&guard, &from_territory),
            );
            let entering = util::replace(
                &util::replace(
                    &guard.raw.language.string("FACTIONS.CLAIM_MESSAGES.ENTERING"),
                    "<faction>",
                    &core::territory_display_guard(&guard, &to_territory, viewer_faction.as_deref()),
                ),
                "<deathban>",
                &core::deathban_string_guard(&guard, &to_territory),
            );

            announce = Some((leaving, entering));
        }

        // Remember the latest position for the next check (and teleports).
        let (cx, cz) = util::chunk_key(tx.floor() as i32, tz.floor() as i32);
        guard.chunk_cache.insert(
            uuid.to_string(),
            ChunkState { world: world.to_string(), cx, cz, territory: to_territory },
        );

        (home_message, stuck_message, announce)
    };

    if let Some(message) = home_message {
        core::send(player, message);
    }
    if let Some(message) = stuck_message {
        core::send(player, message);
    }
    if let Some((leaving, entering)) = announce {
        core::send(player, leaving);
        core::send(player, entering);
    }
}

pub struct MoveListener;

impl EventHandler<PlayerMoveEvent> for MoveListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerMoveEvent>) -> EventData<PlayerMoveEvent> {
        let uuid = core::uuid_of(&event.player);
        let world = core::world_name(&event.player);
        let from = event.from_position;
        let to = event.to_position;

        // Fresh from/to territories, exactly like the original computes
        // `ClaimManager#getFactionAt(event.getFrom()/getTo())`.
        let (combat, was_safezone, is_safezone, block_enter) = {
            let guard = state().read().unwrap();
            let (fx, _fy, fz) = from;
            let (tx, _ty, tz) = to;
            let from_territory =
                core::territory_at_guard(&guard, &world, fx.floor() as i32, fz.floor() as i32);
            let to_territory =
                core::territory_at_guard(&guard, &world, tx.floor() as i32, tz.floor() as i32);
            let was_safezone = core::is_safezone_guard(&guard, &from_territory);
            let is_safezone = core::is_safezone_guard(&guard, &to_territory);

            let block_enter = guard.timers.is_active(&uuid, TimerType::PvpProtection) && {
                let can_enter = guard.raw.config.bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");
                match &to_territory {
                    core::Territory::Faction(id) => guard
                        .factions
                        .get(id)
                        .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                        .unwrap_or(false),
                    _ => false,
                }
            };

            (
                guard.timers.is_active(&uuid, TimerType::CombatTag),
                was_safezone,
                is_safezone,
                block_enter,
            )
        };

        // Combat-tagged players may not enter the safezone (silent, exactly like
        // the original `event.setTo(event.getFrom())`).
        if combat && is_safezone && !was_safezone {
            event.cancelled = true;
            return event;
        }

        // PvP protection: entering another player's claim (or a non-safezone
        // system claim) is denied; wilderness, warzone, safezone and roads are
        // walkable. (Port of `PvpProtHandler#onPlayerMove`, silent.)
        if block_enter {
            event.cancelled = true;
            return event;
        }

        // PvP protection banking: time only ticks outside safezones (port of the
        // `PvpProtTimer#pause` calls; running them on every move is equivalent
        // to the original's faction-change checks because bank/resume are
        // idempotent).
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

        // Claim leave/enter messages + warmup cancels (`checkMovement`).
        check_movement(&event.player, &uuid, from, to, &world);

        event
    }
}

/// Window in which the last damaging player still counts as the killer
/// (approximates Bukkit's `Player#getKiller` combat-tracker window; the port has
/// no NMS combat loggers).
pub(crate) const KILLER_WINDOW_MILLIS: i64 = 5000;

pub struct DeathListener;

impl EventHandler<PlayerDeathEvent> for DeathListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerDeathEvent>) -> EventData<PlayerDeathEvent> {
        let uuid = core::uuid_of(&event.player);
        let name = core::name_of(&event.player);
        let world = core::world_name(&event.player);
        let environment = core::dimension_of(&world);

        // `Player#getKiller` replacement: the last player to damage the victim
        // within the combat window.
        let (killer_uuid, killer_player) = match crate::death::resolve_killer(&server, &uuid) {
            Some((killer_uuid, killer_player)) => (Some(killer_uuid), Some(killer_player)),
            None => (None, None),
        };

        let killer_name = killer_player.as_ref().map(core::name_of).or_else(|| {
            killer_uuid.as_ref().map(|killer| {
                state()
                    .read()
                    .unwrap()
                    .userdata
                    .get(killer)
                    .map(|user| user.name.clone())
                    .unwrap_or_else(|| killer.clone())
            })
        });

        // `DeathMessageHandler#getDeathMessage`: the message is also the
        // deathban reason and the `/lastdeaths` entry. Built before the kill is
        // credited so the killer's displayed count already includes it.
        let reason = crate::death::build_message(&server, &event.player, killer_player.as_ref());

        // Date prefix and `§`-to-`&` conversion of
        // `Userdata#addLastKillOrDeathEntry`.
        let date = crate::datetime::now_from_config();
        let entry = format!("{date} - {}", reason.replace('§', "&"));

        let mut messages: Vec<(String, String)> = Vec::new();
        let mut victim_killstreak_lost = 0i32;
        let mut killer_killstreak_reward: Option<(String, i32, Vec<String>)> = None;
        let mut kill_reward_commands: Option<Vec<String>> = None;

        {
            let mut guard = state().write().unwrap();
            let config = guard.config.clone();

            guard.last_damagers.remove(&uuid);
            guard.last_damage.remove(&uuid);

            // Stats (port of `DynamicEventHandler#onPlayerDeath`): every death is
            // recorded; kills only when the killer is online.
            if let Some(user) = guard.userdata.get_mut(&uuid) {
                user.add_death(entry.clone());
            }
            if killer_player.is_some() {
                if let Some(killer) = &killer_uuid {
                    if let Some(user) = guard.userdata.get_mut(killer) {
                        user.add_kill(entry.clone());
                    }
                }
            }

            // Kitmap killstreaks (port of `KillstreakHandler#onPlayerDeath`):
            // the victim loses theirs (announced when it was non-zero) and the
            // killer extends theirs, receiving the threshold rewards.
            if config.kitmap.enabled && config.kitmap.killstreak_enabled {
                if let Some(user) = guard.userdata.get_mut(&uuid) {
                    victim_killstreak_lost = user.reset_killstreak();
                }

                if killer_player.is_some()
                    && let Some(killer) = &killer_uuid
                    && let Some(user) = guard.userdata.get_mut(killer)
                {
                    user.add_killstreak();
                    let streak = user.killstreak;

                    if let Some(commands) = config.kitmap.killstreak_rewards.get(&streak) {
                        killer_killstreak_reward = Some((
                            killer_name.clone().unwrap_or_default(),
                            streak,
                            commands.clone(),
                        ));
                    }
                }
            }

            // Kitmap kill reward commands (port of `KitmapHandler#onPlayerDeath`).
            if config.kitmap.enabled
                && config.kitmap.kill_reward_enabled
                && killer_player.is_some()
                && !config.kitmap.kill_reward_commands.is_empty()
            {
                kill_reward_commands = Some(config.kitmap.kill_reward_commands.clone());
            }

            guard.timers.cancel(&uuid, TimerType::CombatTag);
            // Death removes PvP protection entirely (port of
            // `PvpProtHandler#onPlayerDeath`; skipped in kitmap mode when the
            // PvP timer is disabled).
            if !(config.kitmap.enabled && !config.kitmap.pvp_timer_enabled) {
                guard.timers.cancel_pvp(&uuid);
            }

            let victim_faction = guard.factions.by_member(&uuid).map(|f| f.id.clone());

            // `PlayerFaction#onDeath` + the silent warmup cancels, both guarded by
            // the faction check in `PlayerEventListener#onPlayerDeath`.
            if let Some(faction_id) = &victim_faction {
                let frozen = guard.timers.is_frozen(faction_id);
                // Kitmap can disable DTR loss (and the freeze) entirely, while
                // still awarding the death points and sending the message with a
                // zero loss (`PlayerFaction#onDeath`).
                let skip_dtr_loss = config.kitmap.enabled && config.kitmap.disable_dtr_loss;
                let loss = if skip_dtr_loss {
                    0.0
                } else {
                    match environment {
                        core::Dimension::Overworld => config.factions.dtr_death_loss.overworld,
                        core::Dimension::Nether => config.factions.dtr_death_loss.nether,
                        core::Dimension::End => config.factions.dtr_death_loss.end,
                    }
                };
                let template = guard.raw.language.string("FACTIONS.MEMBER_DEATH");

                if let Some(faction) = guard.factions.get_mut(faction_id) {
                    if !skip_dtr_loss {
                        let amount = faction.dtr - loss;
                        faction.set_dtr(&config, amount);
                    }
                    faction.change_points(
                        faction.points + config.factions.points_per_death,
                        config.factions.allow_negative_points,
                    );

                    let loss_string = format!("-{}", crate::util::java_double(loss));
                    let dtr_string = faction.dtr_string(&config, frozen);
                    let max_dtr_string = faction.max_dtr_string(&config);
                    messages.push((
                        faction_id.clone(),
                        crate::util::replace_all(
                            &template,
                            &[
                                ("<player>", &name),
                                ("<dtrLoss>", &loss_string),
                                ("<dtr>", &dtr_string),
                                ("<maxDtr>", &max_dtr_string),
                            ],
                        ),
                    ));
                }

                // `FactionFreezeTimer#activate(this)`.
                if !skip_dtr_loss {
                    guard.timers.freeze_faction(
                        faction_id,
                        config.factions.freeze_duration_minutes * 60,
                    );
                }

                guard.timers.clear_pending(&uuid);
            }

            // Killer faction (port of the `PlayerEventListener#onPlayerDeath`
            // kill credit).
            if let Some(killer) = &killer_uuid {
                if let Some(killer_faction_id) =
                    guard.factions.by_member(killer).map(|f| f.id.clone())
                {
                    if let Some(faction) = guard.factions.get_mut(&killer_faction_id) {
                        faction.add_kill();
                        faction.increment_points(
                            config.factions.points_per_kill as i32,
                            config.factions.allow_negative_points,
                        );
                    }
                }
            }
        }

        for (faction_id, message) in messages {
            let faction = state().read().unwrap().factions.get(&faction_id).cloned();
            if let Some(faction) = faction {
                core::faction_message(&server, &faction, &message);
            }
        }

        // Kitmap killstreak messages and console reward commands (port of
        // `KillstreakHandler` and `KitmapHandler#onPlayerDeath`); host calls
        // happen outside the state lock.
        if victim_killstreak_lost > 0 {
            let (prefix, template) = {
                let guard = state().read().unwrap();
                (
                    guard.raw.language.string("PREFIX"),
                    guard.raw.language.string("KITMAP_KILLSTREAK.ON_DEATH"),
                )
            };
            let message =
                util::replace(&template, "<amount>", &victim_killstreak_lost.to_string());
            core::send(&event.player, &format!("{prefix}{message}"));
        }

        if let Some((killer, streak, commands)) = killer_killstreak_reward {
            for command in &commands {
                server.execute_command(
                    &util::replace(command, "<player>", &killer),
                    pumpkin_plugin_api::server::CommandSender::Console,
                );
            }

            let template = state()
                .read()
                .unwrap()
                .raw
                .language
                .string("KITMAP_KILLSTREAK.MESSAGE");
            let message = util::replace_all(
                &template,
                &[("<player>", &killer), ("<amount>", &streak.to_string())],
            );
            crate::sotw::broadcast(&server, &message);
        }

        if let Some(commands) = kill_reward_commands {
            if let Some(killer) = &killer_name {
                for command in &commands {
                    server.execute_command(
                        &util::replace(command, "<player>", killer),
                        pumpkin_plugin_api::server::CommandSender::Console,
                    );
                }
            }
        }

        // `DeathMessageHandler#onPlayerDeath` delivery: the victim, the killer
        // and every player with death messages enabled (the vanilla broadcast is
        // disabled at load, see `death::init`).
        crate::death::send_death_message(&server, &uuid, killer_uuid.as_deref(), &reason);
        event.death_message = util::text(&reason);

        // Deathban (`DeathbanManager#onPlayerDeath`; the kick fires ~1s later,
        // like the original's `Tasks.syncLater`).
        crate::deathban::apply_on_death(&event.player, &reason);

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
    fn handle(&self, server: Server, event: EventData<PlayerRespawnEvent>) -> EventData<PlayerRespawnEvent> {
        let uuid = core::uuid_of(&event.player);
        let (x, _y, z) = event.position;
        let world = event.respawned_world.get_name();
        let block_x = x.floor() as i32;
        let block_z = z.floor() as i32;

        // `DynamicEventHandler#onPlayerRespawn` extinguishes the player. The
        // destination itself is forced to `WORLD_SPAWN` by
        // `SpawnLocationListener` (the host applies the position override
        // before the respawn packet).
        event.player.as_entity().set_fire_ticks(0);

        // A stored respawn point in another dimension (legacy bed/anchor NBT)
        // makes the host resolve that dimension *before* the spawn override is
        // applied, so the player can land in the wrong world: queue a teleport
        // for the next tick, once the respawn has been applied, restoring the
        // original's "always respawn at the spawn" behaviour.
        if let Some((configured_world, ..)) = core::configured_spawn(&server)
            && configured_world.get_name() != world
        {
            state().write().unwrap().respawn_fixes.insert(uuid.clone());
        }

        let (protect_seconds, kitmap_skips_pvp) = {
            let guard = state().read().unwrap();
            (
                guard.config.timers.pvp_protection_seconds,
                guard.config.kitmap.enabled && !guard.config.kitmap.pvp_timer_enabled,
            )
        };

        // `PvpProtHandler#onPlayerRespawn` returns early in kitmap mode without
        // the PvP timer: neither cancels nor (re)activates protection.
        if kitmap_skips_pvp {
            state().write().unwrap().chunk_cache.remove(&uuid);
            return event;
        }

        let (in_safezone, deny) = {
            let guard = state().read().unwrap();
            let in_safezone = core::is_safezone_guard(
                &guard,
                &core::territory_at_guard(&guard, &world, block_x, block_z),
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
            let in_safezone = core::is_safezone_guard(
                &guard,
                &core::territory_at_guard(&guard, &world, block_x, block_z),
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

        // `PlayerEventListener#onPlayerTeleport` also runs `checkMovement`, so
        // teleports announce claim changes immediately (and cancel warmups).
        check_movement(&event.player, &uuid, event.from_position, event.to_position, &world);

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

//! Port of `handlers.elevator.*` — the sign elevator (`SignElevatorHandler`)
//! and the fence-gate minecart elevator (`MinecartElevatorHandler`).
//!
//! Deviations from the original:
//! * `SignChangeEvent#setLine` is ignored by the host (it applies the packet's
//!   lines verbatim after the listeners run), so the configured format is
//!   written straight into the sign block entity (`set_front_text`) and the
//!   event is cancelled to keep the typed lines from overwriting it. The host
//!   sends no block-entity update, so the client keeps showing what was typed
//!   until the chunk reloads; the checks therefore also accept the typed
//!   `[elevator]`/`[e]` + `up`/`down` shorthand (same class of deviation as the
//!   subclaim signs).
//! * `Material.AIR` maps to the block-state `is_air` flag (modern worlds split
//!   air into cave/void variants); `Material.WALL_SIGN`/`SIGN_POST` map to any
//!   `*_sign` block name.
//! * `Material.isSolid()` maps to the block-state `is_solid` flag (minus the
//!   signs for the sign elevator, exactly as the original excluded them).
//! * `PlayerInteractEvent#setUseInteractedBlock(DENY)` maps to cancelling the
//!   interact event (the host offers no finer-grained result).
//! * The column search uses the world's own bounds (`get_min_y` plus the
//!   tallest vanilla column) instead of the fixed 1.8 `getMaxHeight()` 256.
//! * `MinecartElevatorHandler#onVehicleExit` teleports on the next tick in the
//!   original (`Tasks.sync`); the host re-positions the dismounted passenger
//!   right after the event, so the teleport is scheduled one tick later
//!   through the host scheduler.

use pumpkin_plugin_api::block_entity::BlockEntityType;
use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::events::{
    BlockBreakEvent, EventData, EventHandler, InteractAction, PlayerInteractEvent, SignChangeEvent,
    VehicleEnterEvent, VehicleExitEvent,
};
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::{BlockState, EntityType, Player, Server, World};

use crate::blockhandlers;
use crate::core::{self, Territory};
use crate::state::state;
use crate::timers::TimerType;
use crate::util;

/// Tallest vanilla column (the overworld since 1.18). The original used the
/// fixed 1.8 build height (256) as its ceiling; positions past the build limit
/// read as air, so scanning the extra blocks in shorter dimensions matches
/// nothing and is harmless.
const WORLD_COLUMN_HEIGHT: i32 = 384;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

struct SignElevatorConfig {
    enabled: bool,
    own_claim_only: bool,
    disabled_in_combat: bool,
    up: Vec<String>,
    down: Vec<String>,
}

struct MinecartElevatorConfig {
    enabled: bool,
    own_claim_only: bool,
    disabled_in_combat: bool,
}

fn sign_elevator_config() -> SignElevatorConfig {
    let guard = state().read().unwrap();
    let config = &guard.raw.config;

    SignElevatorConfig {
        enabled: config.bool("SIGN_ELEVATOR.ENABLED"),
        own_claim_only: config.bool("SIGN_ELEVATOR.OWN_CLAIM_ONLY"),
        disabled_in_combat: config.bool("SIGN_ELEVATOR.DISABLED_IN_COMBAT"),
        up: config.string_list("SIGN_ELEVATOR.ELEVATOR_UP"),
        down: config.string_list("SIGN_ELEVATOR.ELEVATOR_DOWN"),
    }
}

fn minecart_elevator_config() -> MinecartElevatorConfig {
    let guard = state().read().unwrap();
    let config = &guard.raw.config;

    MinecartElevatorConfig {
        enabled: config.bool("MINECART_ELEVATOR.ENABLED"),
        own_claim_only: config.bool("MINECART_ELEVATOR.OWN_CLAIM_ONLY"),
        disabled_in_combat: config.bool("MINECART_ELEVATOR.DISABLED_IN_COMBAT"),
    }
}

fn send_message(player: &Player, key: &str) {
    let guard = state().read().unwrap();
    let prefix = guard.raw.language.string("PREFIX");
    let message = guard.raw.language.string(key);
    drop(guard);

    core::send(player, &format!("{prefix}{message}"));
}

// ---------------------------------------------------------------------------
// Sign matching
// ---------------------------------------------------------------------------

/// Plain text of a configured format line (the original compared the
/// §-translated strings; here both sides lose their styling the same way).
fn plain_line(line: &str) -> String {
    blockhandlers::component_text(&util::text(line).to_json())
}

/// `SignElevatorHandler#isUpSign`/`#isDownSign`: all four lines match the
/// configured format (case-insensitively in the original).
fn format_matches(lines: &[String], format: &[String]) -> bool {
    format.len() >= 4
        && lines.len() >= 4
        && (0..4).all(|i| lines[i].eq_ignore_ascii_case(&plain_line(&format[i])))
}

/// The `[elevator]`/`[e]` + `up`/`down` shorthand the player actually typed.
/// See the module note: the rewrite reaches the block entity but not the
/// client, so both forms have to count.
fn shorthand_direction(lines: &[String]) -> Option<bool> {
    let title = lines.first()?;
    if !title.eq_ignore_ascii_case("[elevator]") && !title.eq_ignore_ascii_case("[e]") {
        return None;
    }

    let direction = lines.get(1)?;
    if direction.eq_ignore_ascii_case("up") {
        Some(true)
    } else if direction.eq_ignore_ascii_case("down") {
        Some(false)
    } else {
        None
    }
}

fn sign_elevator_direction(lines: &[String], config: &SignElevatorConfig) -> Option<bool> {
    if format_matches(lines, &config.up) {
        Some(true)
    } else if format_matches(lines, &config.down) {
        Some(false)
    } else {
        shorthand_direction(lines)
    }
}

// ---------------------------------------------------------------------------
// Teleport search
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElevatorKind {
    Sign,
    Minecart,
}

/// The original's `materials` set: `AIR`, `WALL_SIGN` and `SIGN_POST`.
fn is_pass_through(kind: ElevatorKind, state: &BlockState) -> bool {
    match kind {
        ElevatorKind::Sign => state.is_air || state.block_name.ends_with("_sign"),
        ElevatorKind::Minecart => state.is_air,
    }
}

/// The original's `isSolidGround`: `Material.isSolid()`, with the sign
/// materials excluded for the sign elevator.
fn is_solid_ground(kind: ElevatorKind, state: &BlockState) -> bool {
    match kind {
        ElevatorKind::Sign => state.is_solid && !state.block_name.ends_with("_sign"),
        ElevatorKind::Minecart => state.is_solid,
    }
}

/// Upward half of `findTeleportLocation`/`MinecartElevatorHandler#teleport`:
/// walk the column from `y` (already offset past the origin block) and return
/// the first floor with two passable blocks above it, keeping the sign block's
/// centre / the vehicle's exact column.
fn find_teleport_up(
    world: &World,
    kind: ElevatorKind,
    x: f64,
    z: f64,
    mut y: i32,
) -> Option<(f64, f64, f64)> {
    let bx = x.floor() as i32;
    let bz = z.floor() as i32;
    let ceiling = world.get_min_y() + WORLD_COLUMN_HEIGHT;

    while y < ceiling {
        let ground = world.get_block_state(BlockPos { x: bx, y, z: bz });
        let plus_one = world.get_block_state(BlockPos { x: bx, y: y + 1, z: bz });
        let plus_two = world.get_block_state(BlockPos { x: bx, y: y + 2, z: bz });

        if is_pass_through(kind, &plus_one)
            && is_pass_through(kind, &plus_two)
            && is_solid_ground(kind, &ground)
        {
            let (target_x, target_z) = match kind {
                ElevatorKind::Sign => (f64::from(bx) + 0.5, f64::from(bz) + 0.5),
                ElevatorKind::Minecart => (x, z),
            };

            return Some((target_x, f64::from(y) + 1.0, target_z));
        }

        y += 1;
    }

    None
}

/// Downward half of `findTeleportLocation` (sign elevator only): find a floor
/// whose two blocks above are passable and stand the player on it.
fn find_teleport_down(world: &World, x: f64, z: f64, mut y: i32) -> Option<(f64, f64, f64)> {
    let bx = x.floor() as i32;
    let bz = z.floor() as i32;
    let bottom = world.get_min_y();

    while y > bottom {
        let minus_one = world.get_block_state(BlockPos { x: bx, y: y - 1, z: bz });
        let minus_two = world.get_block_state(BlockPos { x: bx, y: y - 2, z: bz });
        let ground = world.get_block_state(BlockPos { x: bx, y: y - 3, z: bz });

        if is_pass_through(ElevatorKind::Sign, &minus_one)
            && is_pass_through(ElevatorKind::Sign, &minus_two)
            && is_solid_ground(ElevatorKind::Sign, &ground)
        {
            return Some((
                f64::from(bx) + 0.5,
                f64::from(y - 2),
                f64::from(bz) + 0.5,
            ));
        }

        y -= 1;
    }

    None
}

// ---------------------------------------------------------------------------
// Shared gate
// ---------------------------------------------------------------------------

/// Shared body of both handlers' `teleport` methods: the `OWN_CLAIM_ONLY` and
/// `DISABLED_IN_COMBAT` checks (the original sends the minecart message keys
/// from the sign elevator as well).
fn elevator_allowed(
    player: &Player,
    world_name: &str,
    x: i32,
    z: i32,
    own_claim_only: bool,
    disabled_in_combat: bool,
) -> bool {
    let uuid = core::uuid_of(player);

    if own_claim_only {
        let own_claim = {
            let guard = state().read().unwrap();

            guard.factions.by_member(&uuid).is_some_and(|faction| {
                matches!(
                    core::territory_at_guard(&guard, world_name, x, z),
                    Territory::Faction(owner_id) if owner_id == faction.id
                )
            })
        };

        if !own_claim {
            send_message(player, "MINECART_ELEVATOR.OWN_CLAIM_ONLY");
            return false;
        }
    }

    if disabled_in_combat && state().read().unwrap().timers.is_active(&uuid, TimerType::CombatTag)
    {
        send_message(player, "MINECART_ELEVATOR.DENY_COMBAT");
        return false;
    }

    true
}

fn sign_elevator_teleport(
    player: &Player,
    world: World,
    pos: BlockPos,
    up: bool,
    config: &SignElevatorConfig,
) {
    if !elevator_allowed(
        player,
        &world.get_name(),
        pos.x,
        pos.z,
        config.own_claim_only,
        config.disabled_in_combat,
    ) {
        return;
    }

    let target = if up {
        find_teleport_up(
            &world,
            ElevatorKind::Sign,
            f64::from(pos.x),
            f64::from(pos.z),
            pos.y + 1,
        )
    } else {
        find_teleport_down(&world, f64::from(pos.x), f64::from(pos.z), pos.y)
    };

    match target {
        Some((x, y, z)) => player.teleport((x, y, z), None, None, world),
        None => send_message(player, "SIGN_ELEVATOR.NO_VALID_LOCATION"),
    }
}

/// The rider part both minecart listeners share: gate checks plus the upward
/// column search from the vehicle's position (`location.setY(getBlockY())`
/// followed by the initial `add(0, 1, 0)`).
fn minecart_elevator_teleport(
    player: &Player,
    world: World,
    x: f64,
    z: f64,
    block_y: i32,
    config: &MinecartElevatorConfig,
) {
    if !elevator_allowed(
        player,
        &world.get_name(),
        x.floor() as i32,
        z.floor() as i32,
        config.own_claim_only,
        config.disabled_in_combat,
    ) {
        return;
    }

    match find_teleport_up(&world, ElevatorKind::Minecart, x, z, block_y + 1) {
        Some((x, y, z)) => player.teleport((x, y, z), None, None, world),
        None => send_message(player, "MINECART_ELEVATOR.NO_VALID_LOCATION"),
    }
}

// ---------------------------------------------------------------------------
// Sign elevator
// ---------------------------------------------------------------------------

/// `SignElevatorHandler#onSignChange`: `[elevator]`/`[e]` plus `up`/`down`
/// re-writes the sign into the configured format.
pub struct SignElevatorSignChangeListener;

impl EventHandler<SignChangeEvent> for SignElevatorSignChangeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<SignChangeEvent>,
    ) -> EventData<SignChangeEvent> {
        if event.cancelled {
            return event;
        }

        let config = sign_elevator_config();
        if !config.enabled {
            return event;
        }

        let lines: Vec<String> = event
            .lines
            .iter()
            .map(|line| blockhandlers::component_text(line))
            .collect();

        let Some(up) = shorthand_direction(&lines) else {
            return event;
        };

        let format = if up { &config.up } else { &config.down };
        let world = event.player.get_world();

        if let Some(BlockEntityType::SignBlockEntity(sign)) = world.get_block_entity(event.block_pos)
        {
            if !format.is_empty() {
                let mut text = sign.get_front_text();
                text.messages = format
                    .iter()
                    .take(4)
                    .map(|line| util::text(line).to_json())
                    .collect();
                sign.set_front_text(&text);
            }

            event.cancelled = true;
        }

        send_message(&event.player, "SIGN_ELEVATOR.CREATED");
        event
    }
}

/// `SignElevatorHandler#onBlockBreak`: destroying an elevator sign tells the
/// breaker (the original's `ignoreCancelled` runs it after the protections).
pub struct SignElevatorBreakListener;

impl EventHandler<BlockBreakEvent> for SignElevatorBreakListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<BlockBreakEvent>,
    ) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        let config = sign_elevator_config();
        if !config.enabled {
            return event;
        }

        let Some(player) = event.player.as_ref() else {
            return event;
        };
        let world = player.get_world();

        let Some(lines) = blockhandlers::sign_lines(&world, event.block_pos) else {
            return event;
        };

        if sign_elevator_direction(&lines, &config).is_some() {
            send_message(player, "SIGN_ELEVATOR.DESTROYED");
        }

        event
    }
}

/// `SignElevatorHandler#onPlayerInteract`: right-clicking an elevator sign
/// teleports and the sign use is denied.
pub struct SignElevatorInteractListener;

impl EventHandler<PlayerInteractEvent> for SignElevatorInteractListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerInteractEvent>,
    ) -> EventData<PlayerInteractEvent> {
        if event.cancelled || event.action != InteractAction::RightClickBlock {
            return event;
        }

        let config = sign_elevator_config();
        if !config.enabled {
            return event;
        }

        let Some(pos) = event.clicked_pos else {
            return event;
        };

        let player = &event.player;
        let world = player.get_world();

        let Some(lines) = blockhandlers::sign_lines(&world, pos) else {
            return event;
        };
        let Some(up) = sign_elevator_direction(&lines, &config) else {
            return event;
        };

        // `event.setUseInteractedBlock(Result.DENY)`, run after `teleport()` in
        // the original — unconditionally, even when the gate denies the ride.
        event.cancelled = true;

        sign_elevator_teleport(player, world, pos, up, &config);
        event
    }
}

// ---------------------------------------------------------------------------
// Minecart elevator
// ---------------------------------------------------------------------------

/// `instanceof Minecart` covers every minecart variant.
fn is_minecart(kind: EntityType) -> bool {
    matches!(
        kind,
        EntityType::Minecart
            | EntityType::ChestMinecart
            | EntityType::CommandBlockMinecart
            | EntityType::FurnaceMinecart
            | EntityType::HopperMinecart
            | EntityType::SpawnerMinecart
            | EntityType::TntMinecart
    )
}

/// The WIT exposes no entity lookup by id; the passengers the original handled
/// were online players anyway (`instanceof Player`).
fn player_by_entity_id(server: &Server, id: i32) -> Option<Player> {
    server
        .get_all_players()
        .into_iter()
        .find(|player| player.as_entity().get_id() as i32 == id)
}

/// The vehicle's own block must be a fence gate (`Material.FENCE_GATE`; modern
/// worlds name the wood variants `*_fence_gate`).
fn on_fence_gate(world: &World, x: f64, y: f64, z: f64) -> bool {
    world
        .get_block_state(BlockPos {
            x: x.floor() as i32,
            y: y.floor() as i32,
            z: z.floor() as i32,
        })
        .block_name
        .ends_with("_fence_gate")
}

/// `MinecartElevatorHandler#onVehicleEnter`: a sneaking player entering a
/// minecart that sits on a fence gate is pulled up instead (the mount is
/// cancelled, so nothing happens on the claim/combat denial either).
pub struct MinecartElevatorEnterListener;

impl EventHandler<VehicleEnterEvent> for MinecartElevatorEnterListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<VehicleEnterEvent>,
    ) -> EventData<VehicleEnterEvent> {
        if event.cancelled {
            return event;
        }

        let config = minecart_elevator_config();
        if !config.enabled {
            return event;
        }

        let Some(player) = player_by_entity_id(&server, event.entered_id) else {
            return event;
        };
        if !player.as_entity().is_sneaking() {
            return event;
        }

        // The mount has not happened yet, so the vehicle is resolved from the
        // entities around the entering player.
        let Some(vehicle) = player
            .as_entity()
            .get_nearby_entities(3.0, 3.0, 3.0)
            .into_iter()
            .find(|candidate| candidate.get_id() as i32 == event.vehicle_id)
        else {
            return event;
        };

        if !is_minecart(vehicle.get_type()) {
            return event;
        }

        let (x, y, z) = vehicle.get_position();
        let world = vehicle.get_world();
        if !on_fence_gate(&world, x, y, z) {
            return event;
        }

        event.cancelled = true;

        let block_y = y.floor() as i32;
        minecart_elevator_teleport(&player, world, x, z, block_y, &config);
        event
    }
}

/// `MinecartElevatorHandler#onVehicleExit`: leaving a minecart parked on a
/// fence gate rides it up one tick later (`Tasks.sync`; the host re-positions
/// the dismounted passenger right after the event).
pub struct MinecartElevatorExitListener;

impl EventHandler<VehicleExitEvent> for MinecartElevatorExitListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<VehicleExitEvent>,
    ) -> EventData<VehicleExitEvent> {
        if event.cancelled {
            return event;
        }

        let config = minecart_elevator_config();
        if !config.enabled {
            return event;
        }

        let Some(player) = player_by_entity_id(&server, event.exited_id) else {
            return event;
        };

        // The passenger is still attached while the event runs.
        let Some(vehicle) = player.as_entity().get_vehicle() else {
            return event;
        };
        if vehicle.get_id() as i32 != event.vehicle_id || !is_minecart(vehicle.get_type()) {
            return event;
        }

        let (x, y, z) = vehicle.get_position();
        let world = vehicle.get_world();
        if !on_fence_gate(&world, x, y, z) {
            return event;
        }

        let uuid = core::uuid_of(&player);
        let world_name = world.get_name();
        let block_y = y.floor() as i32;

        server.schedule_delayed_task(1, move |server| {
            let (Some(player), Some(world)) = (
                pumpkin_plugin_api::uuid::parse(&uuid).and_then(|id| server.get_player_by_uuid(id)),
                server.get_world_by_name(&world_name),
            ) else {
                return;
            };

            minecart_elevator_teleport(&player, world, x, z, block_y, &config);
        });

        event
    }
}

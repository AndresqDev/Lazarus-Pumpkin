//! High-level helpers that bridge the domain model with the Pumpkin host API.

use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Player, Server};

use crate::factions::{Faction, FactionKind, Location, Relation};
use crate::state::state;
use crate::util;

/// Returns a player's UUID as a string.
pub fn uuid_of(player: &Player) -> String {
    pumpkin_plugin_api::uuid::to_string(player.get_id())
}

pub fn name_of(player: &Player) -> String {
    player.get_name()
}

pub fn world_name(player: &Player) -> String {
    player.get_world().get_name()
}

/// Snapshot of a player's position as a serialisable [`Location`].
pub fn location(player: &Player) -> Location {
    let (x, y, z) = player.get_position();
    Location {
        world: world_name(player),
        x,
        y,
        z,
        yaw: player.get_yaw(),
        pitch: player.get_pitch(),
    }
}

pub fn send(player: &Player, message: impl AsRef<str>) {
    player.send_system_message(util::text(message), false);
}

pub fn send_component(player: &Player, component: TextComponent) {
    player.send_system_message(component, false);
}

/// Broadcasts a message to every online member of a faction.
pub fn faction_message(server: &Server, faction: &Faction, message: &str) {
    for member in faction.members.values() {
        if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member.uuid)
            && let Some(player) = server.get_player_by_uuid(uuid)
        {
            send(&player, message);
        }
    }
}

/// Broadcasts a component-styled message to every online member of a faction (unused helper kept for parity).
#[allow(dead_code)]
pub fn faction_message_text(server: &Server, faction: &Faction, message: &str) {
    faction_message(server, faction, message);
}

/// Returns online [`Player`] handles for every member of a faction.
pub fn online_members(server: &Server, faction: &Faction) -> Vec<Player> {
    faction
        .members
        .values()
        .filter_map(|member| {
            pumpkin_plugin_api::uuid::parse(&member.uuid).and_then(|uuid| server.get_player_by_uuid(uuid))
        })
        .collect()
}

/// Finds the [`Player`] handle for an entity id, if it is a player.
pub fn player_by_entity(server: &Server, entity_id: i32) -> Option<Player> {
    server
        .get_all_players()
        .into_iter()
        .find(|player| player.as_entity().get_id() as i32 == entity_id)
}

pub fn find_online(server: &Server, name: &str) -> Option<Player> {
    server.get_player_by_name(name)
}

/// The faction id a player currently belongs to.
pub fn faction_id_of(uuid: &str) -> Option<String> {
    let guard = state().read().unwrap();
    guard.factions.by_member(uuid).map(|f| f.id.clone())
}

/// The dimension a world name belongs to, used to look up per-world radii.
pub fn dimension_of(world: &str) -> Dimension {
    let lower = world.to_lowercase();
    if lower.contains("nether") {
        Dimension::Nether
    } else if lower.contains("end") {
        Dimension::End
    } else {
        Dimension::Overworld
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

/// The territory type at a location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Territory {
    Wilderness,
    Warzone,
    Safezone,
    /// Owned by a player faction with the given id.
    Faction(String),
}

impl Default for Territory {
    fn default() -> Self {
        Self::Wilderness
    }
}

impl Dimension {
    pub fn radius(radii: &crate::config::WorldRadii) -> i32 {
        radii.overworld
    }
}

/// Applies the configured world borders (port of `handlers.BorderHandler`).
///
/// `border_size` is a radius: `|x| <= size && |z| <= size`, infinite on Y.
/// Refreshes the cached world spawn centres (used to centre the warzone/safezone
/// and the world border). Pumpkin worlds may not spawn at 0,0.
pub fn refresh_spawn_centers(server: &Server) {
    let mut guard = state().write().unwrap();
    for world in server.get_all_worlds() {
        let spawn = world.get_spawn_location();
        guard
            .spawn_centers
            .insert(world.get_name(), (spawn.pos.x, spawn.pos.z));
    }
}

/// Resolves the `WORLD_SPAWN` entry of `utilities.yml` against the loaded
/// worlds (same format as the original `LocationUtils#stringToLocation`).
///
/// Returns the world plus position and rotation. `None` when the entry is
/// empty, malformed, or points at a world that is not loaded.
pub fn configured_spawn(
    server: &Server,
) -> Option<(pumpkin_plugin_api::World, (f64, f64, f64), f32, f32)> {
    let value = {
        let guard = state().read().unwrap();
        guard.raw.utilities.string("WORLD_SPAWN")
    };
    let (world_name, x, y, z, yaw, pitch) = parse_utilities_location(&value)?;
    let world = resolve_world(server, &world_name)?;
    Some((world, (x, y, z), yaw, pitch))
}

/// Resolves a world token from `utilities.yml` against the loaded worlds.
///
/// Pumpkin names the nether/end worlds after their *root* folder as well
/// ("world"), so the Bukkit folder names of the original configs are mapped to
/// their dimension first; anything else falls back to the host lookup (which
/// also accepts dimension names like `minecraft:overworld`).
fn resolve_world(server: &Server, token: &str) -> Option<pumpkin_plugin_api::World> {
    let dimension = match token {
        "world" => Some("minecraft:overworld"),
        "world_nether" | "DIM-1" | "nether" => Some("minecraft:the_nether"),
        "world_the_end" | "DIM1" | "end" => Some("minecraft:the_end"),
        _ => None,
    };

    if let Some(dimension) = dimension
        && let Some(world) = server
            .get_all_worlds()
            .into_iter()
            .find(|world| world.get_dimension() == dimension)
    {
        return Some(world);
    }

    server.get_world_by_name(token)
}

/// Parses `LocationUtils#stringToLocation`: `world|x|y|z|yaw|pitch`.
pub fn parse_utilities_location(value: &str) -> Option<(String, f64, f64, f64, f32, f32)> {
    if value.is_empty() {
        return None;
    }

    let parts: Vec<&str> = value.split('|').collect();
    if parts.len() < 6 {
        return None;
    }

    Some((
        parts[0].to_string(),
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
        parts[3].parse().ok()?,
        parts[4].parse().ok()?,
        parts[5].parse().ok()?,
    ))
}

/// Applies the configured world borders using the live server config, centred on
/// each world's spawn.
///
/// `border_size` is a radius: `|x| <= size && |z| <= size`, infinite on Y.
pub fn apply_world_borders_from_config(server: &Server) {
    let radii = state().read().unwrap().config.border_size;

    for world in server.get_all_worlds() {
        let name = world.get_name();
        let size = match dimension_of(&name) {
            Dimension::Overworld => radii.overworld,
            Dimension::Nether => radii.nether,
            Dimension::End => radii.end,
        };
        let border = world.get_world_border();
        border.set_center(0.0, 0.0);
        border.set_size((size * 2) as f64);
    }
}

/// Resolves the territory at `(world, x, z)`.
pub fn territory_at(world: &str, x: i32, z: i32) -> Territory {
    let guard = state().read().unwrap();
    territory_at_guard(&guard, world, x, z)
}

/// Lock-free variant for callers that already hold the state lock (avoids
/// re-entering `RwLock`, which deadlocks).
pub fn territory_at_guard(guard: &crate::state::AppState, world: &str, x: i32, z: i32) -> Territory {
    if let Some(owner_id) = guard.claims.faction_at(world, x, z)
        && let Some(faction) = guard.factions.get(owner_id)
    {
        return match faction.kind {
            FactionKind::Wilderness => Territory::Wilderness,
            FactionKind::Warzone => Territory::Warzone,
            FactionKind::Safezone => Territory::Safezone,
            _ => Territory::Faction(owner_id.clone()),
        };
    }

    let dimension = dimension_of(world);
    let war_radius = match dimension {
        Dimension::Overworld => guard.config.warzone_radius.overworld,
        Dimension::Nether => guard.config.warzone_radius.nether,
        Dimension::End => guard.config.warzone_radius.end,
    };

    let distance = x.abs().max(z.abs());
    if war_radius > 0 && distance <= war_radius {
        Territory::Warzone
    } else {
        Territory::Wilderness
    }
}

/// The relation of `viewer_faction` towards the faction owning a territory.
pub fn relation_in(territory: &Territory, viewer_faction: Option<&str>) -> Relation {
    match territory {
        Territory::Faction(owner) => match viewer_faction {
            Some(viewer) if viewer == owner => Relation::Member,
            Some(viewer) => {
                let guard = state().read().unwrap();
                match guard.factions.get(viewer) {
                    Some(faction) if faction.is_ally(owner) => Relation::Ally,
                    _ => Relation::Enemy,
                }
            }
            None => Relation::Enemy,
        },
        _ => Relation::Enemy,
    }
}

/// Builds the coloured territory string shown when entering a chunk.
pub fn territory_display(territory: &Territory, viewer_faction: Option<&str>) -> String {
    let guard = state().read().unwrap();
    territory_display_guard(&guard, territory, viewer_faction)
}

/// Lock-free variant of [`territory_display`].
pub fn territory_display_guard(
    guard: &crate::state::AppState,
    territory: &Territory,
    viewer_faction: Option<&str>,
) -> String {
    let colors = &guard.config.relation_colors;

    match territory {
        // `WildernessFaction` / `WarzoneFaction` take their colour-coded names
        // from `SYSTEM_FACTION_NAMES` (`FactionsManager#createDefaultFaction`).
        Territory::Wilderness => guard.config.system_faction_names.wilderness.clone(),
        Territory::Warzone => guard.config.system_faction_names.warzone.clone(),
        // `SpawnFaction`: name "Spawn" with the `&a` colour.
        Territory::Safezone => guard.config.system_faction_names.safezone.clone(),
        Territory::Faction(id) => {
            let Some(faction) = guard.factions.get(id) else {
                return guard.config.system_faction_names.wilderness.clone();
            };
            faction_display_name_guard(guard, faction, viewer_faction, colors)
        }
    }
}

/// `Faction#getName(sender)` / `SystemFaction#getDisplayName(sender)`: player
/// factions use the viewer's relation colour, system factions their own colour.
pub fn faction_display_name_guard(
    guard: &crate::state::AppState,
    faction: &crate::factions::Faction,
    viewer_faction: Option<&str>,
    colors: &crate::config::RelationColors,
) -> String {
    if faction.is_player() {
        let relation = match viewer_faction {
            Some(viewer) if viewer == faction.id => Relation::Member,
            Some(viewer) => match guard.factions.get(viewer) {
                Some(viewer_faction) if viewer_faction.is_ally(&faction.id) => Relation::Ally,
                _ => Relation::Enemy,
            },
            None => Relation::Enemy,
        };
        return format!("{}{}", relation.color(colors), faction.name);
    }

    match faction.kind {
        // `RoadFaction#getDisplayName`: "North Road", "East Road", ...
        crate::factions::FactionKind::Road => {
            let spaced = match faction.name.as_str() {
                "NorthRoad" => "North Road".to_string(),
                "EastRoad" => "East Road".to_string(),
                "WestRoad" => "West Road".to_string(),
                "SouthRoad" => "South Road".to_string(),
                other => other.to_string(),
            };
            format!("{}{}", faction.color, spaced)
        }
        // `SystemFaction#getName`: own colour + raw name.
        _ => format!("{}{}", faction.color, faction.name),
    }
}

/// `Faction#getDeathbanString` for a territory: `&cDeathban` / `&aNon-Deathban`.
/// Only the spawn safezone is non-deathban by default; wilderness, warzone,
/// roads and (player) factions use the `deathban` flag.
pub fn deathban_string_guard(guard: &crate::state::AppState, territory: &Territory) -> String {
    let deathban = match territory {
        Territory::Wilderness | Territory::Warzone => true,
        Territory::Safezone => false,
        Territory::Faction(id) => guard
            .factions
            .get(id)
            .map(|faction| faction.deathban)
            .unwrap_or(false),
    };

    if deathban {
        "&cDeathban".to_string()
    } else {
        "&aNon-Deathban".to_string()
    }
}

/// `Faction#isSafezone` for a territory (only system factions can be safezones).
pub fn is_safezone_guard(guard: &crate::state::AppState, territory: &Territory) -> bool {
    match territory {
        Territory::Safezone => true,
        Territory::Faction(id) => guard
            .factions
            .get(id)
            .map(|faction| faction.safezone)
            .unwrap_or(false),
        _ => false,
    }
}

/// Deathban gate for the location a player died at: `Faction#isDeathban()`
/// (`DeathbanManager#deathbanPlayer`). Handles system territories directly.
pub fn territory_deathbans(territory: &Territory) -> bool {
    let guard = state().read().unwrap();
    territory_deathbans_guard(&guard, territory)
}

/// Lock-free variant of [`territory_deathbans`].
pub fn territory_deathbans_guard(
    guard: &crate::state::AppState,
    territory: &Territory,
) -> bool {
    match territory {
        Territory::Wilderness | Territory::Warzone => true,
        Territory::Safezone => false,
        Territory::Faction(id) => guard
            .factions
            .get(id)
            .map(|faction| faction.deathban)
            .unwrap_or(false),
    }
}

/// Teleports players who are not allowed to be inside `claim` to just outside it.
/// Port of the PvP-protection ejection on claim change (`PvpProtHandler`).
/// Must be called WITHOUT holding the state lock.
pub fn eject_intruders(server: &Server, claim: &crate::factions::Claim, owner_id: &str) {
    let can_enter = state()
        .read()
        .unwrap()
        .raw
        .config
        .bool("PVP_PROTECTION.CAN_ENTER_OWN_CLAIM");

    let center_x = (claim.min_x + claim.max_x) / 2;

    for player in server.get_all_players() {
        let location = location(&player);
        if location.world != claim.world {
            continue;
        }
        if !claim.contains(&claim.world, location.block_x(), location.block_z()) {
            continue;
        }

        let uuid = uuid_of(&player);
        let eject = {
            let guard = state().read().unwrap();
            guard.timers.is_active(&uuid, crate::timers::TimerType::PvpProtection)
                && guard
                    .factions
                    .get(owner_id)
                    .map(|faction| faction.should_cancel_pvp_timer_entrance(&uuid, can_enter))
                    .unwrap_or(false)
        };
        if !eject {
            continue;
        }

        let x = if location.block_x() <= center_x {
            claim.min_x - 2
        } else {
            claim.max_x + 2
        };
        let target = (x as f64 + 0.5, location.y, location.z as f64 + 0.5);

        if let Some(world) = server
            .get_all_worlds()
            .into_iter()
            .find(|world| world.get_name() == claim.world)
        {
            player.teleport(target, None, None, world);
        }

        let message = state()
            .read()
            .unwrap()
            .raw
            .language
            .string("PVP_PROTECTION.TELEPORT_DUE_TO_CLAIM");
        if !message.is_empty() {
            send(&player, &message);
        }
    }
}

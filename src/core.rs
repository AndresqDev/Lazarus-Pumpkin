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

/// Creates the spawn/safezone faction claim from config if it does not exist yet.
///
/// The safezone is *exclusively* the spawn claim, so this materialises a square
/// claim (`spawn.safezone_radius`) owned by the safezone faction.
pub fn setup_spawn_claim(server: &Server) {
    let mut guard = state().write().unwrap();
    let radius = guard.config.spawn.safezone_radius;
    if radius <= 0 {
        return;
    }

    let safezone_id = guard.factions.safezone_id.clone();
    if !guard.claims.claims_of(&safezone_id).is_empty() {
        return;
    }

    let world = server
        .get_all_worlds()
        .into_iter()
        .find(|w| dimension_of(&w.get_name()) == Dimension::Overworld)
        .map(|w| w.get_name())
        .unwrap_or_else(|| "world".to_string());

    let claim = crate::factions::Claim::new(world.clone(), -radius, radius, -radius, radius);
    guard.claims.add_claim(&safezone_id, claim);
    tracing::info!("lazarus-spawn created safezone claim world={world} radius={radius}");
}

/// Resolves the territory at `(world, x, z)`.
pub fn territory_at(world: &str, x: i32, z: i32) -> Territory {
    let guard = state().read().unwrap();

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

    // Safezone is *only* the spawn faction's claims (handled above via the claim
    // index); the radius below is purely the warzone.
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
    let colors = &guard.config.relation_colors;

    match territory {
        Territory::Wilderness => guard.lang.territory_wilderness.clone(),
        Territory::Warzone => guard.lang.territory_warzone.clone(),
        Territory::Safezone => guard.lang.territory_safezone.clone(),
        Territory::Faction(id) => {
            let Some(faction) = guard.factions.get(id) else {
                return guard.lang.territory_wilderness.clone();
            };
            let relation = match viewer_faction {
                Some(viewer) if viewer == id => Relation::Member,
                Some(viewer) => match guard.factions.get(viewer) {
                    Some(viewer_faction) if viewer_faction.is_ally(id) => Relation::Ally,
                    _ => Relation::Enemy,
                },
                None => Relation::Enemy,
            };
            format!("{}{}", relation.color(colors), faction.name)
        }
    }
}

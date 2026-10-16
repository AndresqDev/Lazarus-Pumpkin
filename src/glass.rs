//! Client-side block rendering engine — port of `me.qiooip.lazarus.glass.GlassManager`.
//!
//! Bukkit used `Player#sendBlockChange`. Pumpkin exposes the exact equivalent:
//! `Player::send_block_change(pos, state_id)` and `Player::reset_block_change(pos)`.
//! We track the fake blocks each player currently sees so we can revert them,
//! grouped by [`GlassType`] (spawn walls, claim walls, claim map, claim selection).

use std::collections::BTreeMap;

use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::world::Block;
use pumpkin_plugin_api::{Player, Server};

use crate::config::Config;
use crate::factions::{Claim, ClaimManager, FactionKind, FactionsManager};
use crate::timers::{TimerManager, TimerType};

/// The kind of visual, mirroring `GlassManager.GlassType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassType {
    /// Wall shown while combat tagged (marks spawn/safezone borders).
    SpawnWall,
    /// Wall shown during PvP protection (marks enemy claim borders).
    ClaimWall,
    /// Pillars that map nearby claims.
    ClaimMap,
    /// Pillars that show the current claim selection.
    ClaimSelection,
}

type PosKey = (String, i32, i32, i32);

/// Per-player fake-block bookkeeping.
///
/// Each entry stores the [`GlassType`] (so it can be cleared per view) and the
/// block-state id that was sent (so the view can be regenerated if the player
/// breaks/places a real block at that position).
#[derive(Debug, Default)]
pub struct GlassManager {
    /// uuid -> (world, x, y, z) -> (type, block-state id)
    pub entries: BTreeMap<String, BTreeMap<PosKey, (GlassType, u16)>>,
    /// uuid -> last chunk the claim map was re-generated for ("world", cx, cz)
    pub map_users: BTreeMap<String, (String, i32, i32)>,
    /// Positions whose fake block must be re-sent for a few ticks (the client
    /// predicts breaks/places and would otherwise erase our render). Value is the
    /// number of remaining re-sends.
    pub pending_regen: std::collections::BTreeMap<PosKey, u8>,
}

impl GlassManager {
    fn bucket(&mut self, uuid: &str) -> &mut BTreeMap<PosKey, (GlassType, u16)> {
        self.entries.entry(uuid.to_string()).or_default()
    }

    /// Sends a single fake block to `player` if not already faked.
    fn place(&mut self, player: &Player, kind: GlassType, world: &str, x: i32, y: i32, z: i32, state_id: u16) {
        let uuid = crate::core::uuid_of(player);
        let key = (world.to_string(), x, y, z);

        if self.bucket(&uuid).contains_key(&key) {
            return;
        }

        player.send_block_change(BlockPos { x, y, z }, state_id);
        self.bucket(&uuid).insert(key, (kind, state_id));
    }

    /// Reverts every fake block of a given type for a player.
    pub fn clear_type(&mut self, player: &Player, kind: GlassType) {
        let uuid = crate::core::uuid_of(player);
        let current_world = crate::core::world_name(player);

        let Some(map) = self.entries.get_mut(&uuid) else {
            return;
        };

        let keys: Vec<PosKey> = map
            .iter()
            .filter(|(_, (value, _))| *value == kind)
            .map(|(key, _)| key.clone())
            .collect();

        for key in keys {
            let (world, x, y, z) = key.clone();
            if world == current_world {
                player.reset_block_change(BlockPos { x, y, z });
            }
            map.remove(&key);
        }
    }

    /// Reverts fake blocks of `kind` that are now farther than `width`/`height` away.
    pub fn clear_far(
        &mut self,
        player: &Player,
        kind: GlassType,
        to_world: &str,
        to_x: i32,
        to_y: i32,
        to_z: i32,
        width: i32,
        height: i32,
    ) {
        let uuid = crate::core::uuid_of(player);

        let Some(map) = self.entries.get_mut(&uuid) else {
            return;
        };

        let keys: Vec<PosKey> = map
            .iter()
            .filter(|(_, (value, _))| *value == kind)
            .filter(|((world, x, y, z), _)| {
                world != to_world
                    || (x - to_x).abs() > width
                    || (y - to_y).abs() > height
                    || (z - to_z).abs() > width
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in keys {
            let (world, x, y, z) = key.clone();
            if world == to_world {
                player.reset_block_change(BlockPos { x, y, z });
            }
            map.remove(&key);
        }
    }

    /// Reverts every fake block a player currently sees (used on quit).
    pub fn clear_all(&mut self, player: &Player) {
        self.clear_type(player, GlassType::SpawnWall);
        self.clear_type(player, GlassType::ClaimWall);
        self.clear_type(player, GlassType::ClaimMap);
        self.clear_type(player, GlassType::ClaimSelection);

        let uuid = crate::core::uuid_of(player);
        self.entries.remove(&uuid);
        self.map_users.remove(&uuid);
    }

    pub fn is_map_user(&self, uuid: &str) -> bool {
        self.map_users.contains_key(uuid)
    }

    pub fn set_map_user(&mut self, uuid: &str, world: &str, cx: i32, cz: i32) {
        self.map_users.insert(uuid.to_string(), (world.to_string(), cx, cz));
    }

    pub fn remove_map_user(&mut self, uuid: &str) {
        self.map_users.remove(uuid);
    }

    /// Drops all bookkeeping for a player without sending any resets (on quit).
    pub fn forget(&mut self, uuid: &str) {
        self.entries.remove(uuid);
        self.map_users.remove(uuid);
    }

    /// Regenerates the fake block at `(world, x, y, z)` for every player that sees
    /// one, so an active client-side view survives a real block break/place there.
    ///
    /// The immediate send is overridden by the server's own break/place update, so
    /// the position is also queued and re-sent on the next tick.
    pub fn regenerate_at(&mut self, server: &Server, world: &str, x: i32, y: i32, z: i32) {
        let key = (world.to_string(), x, y, z);

        let mut seen = false;
        for map in self.entries.values() {
            if map.get(&key).is_some() {
                seen = true;
                break;
            }
        }
        if !seen {
            return;
        }

        self.send_at(server, &key);
        // Keep re-sending for a few ticks: the server's own block update may land
        // one or two ticks later and would otherwise override this send.
        self.pending_regen.insert(key, 6);
    }

    fn send_at(&self, server: &Server, key: &PosKey) {
        let (world, x, y, z) = key.clone();
        for (uuid, map) in &self.entries {
            let Some((_, state_id)) = map.get(key) else {
                continue;
            };
            if let Some(player) =
                pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
            {
                player.send_block_change(BlockPos { x, y, z }, *state_id);
            }
        }
    }

    /// Re-sends every queued fake block and decrements its countdown.
    pub fn process_pending_regen(&mut self, server: &Server) {
        if self.pending_regen.is_empty() {
            return;
        }
        let keys: Vec<PosKey> = self.pending_regen.keys().cloned().collect();
        for key in keys {
            self.send_at(server, &key);
        }
        self.pending_regen.retain(|_, remaining| {
            *remaining = remaining.saturating_sub(1);
            *remaining > 0
        });
    }

    /// Re-sends every active fake block to the player that owns it. This is the
    /// safety net that keeps the client-side render in sync (a broken/placed real
    /// block, a plugin desync, …), and mirrors the original's periodic regeneration.
    pub fn refresh_all(&self, server: &Server) {
        for (uuid, map) in &self.entries {
            let Some(player) =
                pumpkin_plugin_api::uuid::parse(uuid).and_then(|id| server.get_player_by_uuid(id))
            else {
                continue;
            };
            for ((_, x, y, z), (_, state_id)) in map {
                player.send_block_change(BlockPos { x: *x, y: *y, z: *z }, *state_id);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Visual generation (driven by the tick task)
// ---------------------------------------------------------------------------

const WALL_BORDER_HEIGHT: i32 = 4;
const WALL_BORDER_WIDTH: i32 = 5;
const GLASS_RED: &str = "minecraft:red_stained_glass";
const GLASS_LIME: &str = "minecraft:lime_stained_glass";
const GLASS_LIGHT_BLUE: &str = "minecraft:light_blue_stained_glass";
const DIAMOND_BLOCK: &str = "minecraft:diamond_block";

/// Resolves a block registry name to its default block-state id.
fn state_id(name: &str) -> u16 {
    Block::from_name(name)
        .or_else(|| Block::from_name(name.strip_prefix("minecraft:").unwrap_or(name)))
        .map(|block| block.get_default_state().id)
        .unwrap_or(0)
}

/// Regenerates the claim/spawn walls for a player based on their active timers.
pub fn update_walls(
    glass: &mut GlassManager,
    player: &Player,
    claims: &ClaimManager,
    factions: &FactionsManager,
    timers: &TimerManager,
) {
    let uuid = crate::core::uuid_of(player);
    let world = crate::core::world_name(player);
    let (x, y, z) = player.get_position();
    let (bx, by, bz) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);

    let kind = if timers.is_active(&uuid, TimerType::CombatTag) {
        GlassType::SpawnWall
    } else if timers.is_active(&uuid, TimerType::PvpProtection) {
        GlassType::ClaimWall
    } else {
        glass.clear_type(player, GlassType::SpawnWall);
        glass.clear_type(player, GlassType::ClaimWall);
        return;
    };

    glass.clear_far(player, kind, &world, bx, by, bz, WALL_BORDER_WIDTH, WALL_BORDER_HEIGHT);

    for (owner_id, claim) in nearby_claims(claims, &world, bx, bz, WALL_BORDER_WIDTH) {
        let Some(faction) = factions.get(owner_id) else {
            continue;
        };

        let is_safezone = faction.kind == FactionKind::Safezone;
        if (kind == GlassType::SpawnWall && !is_safezone) || (kind == GlassType::ClaimWall && is_safezone) {
            continue;
        }

        for (sx, sz) in closest_sides(claim, bx, bz, WALL_BORDER_WIDTH) {
            for dy in -WALL_BORDER_HEIGHT + 1..=WALL_BORDER_HEIGHT {
                glass.place(player, kind, &world, sx, by + dy, sz, state_id(GLASS_RED));
            }
        }
    }
}

/// Toggles/regenerates the claim-map pillars for a player.
pub fn update_map(glass: &mut GlassManager, player: &Player, claims: &ClaimManager, config: &Config) {
    let uuid = crate::core::uuid_of(player);
    if !glass.is_map_user(&uuid) {
        return;
    }

    let world = crate::core::world_name(player);
    let (x, _, z) = player.get_position();
    let (bx, bz) = (x.floor() as i32, z.floor() as i32);
    let (cx, cz) = crate::util::chunk_key(bx, bz);

    if glass.map_users.get(&uuid) == Some(&(world.clone(), cx, cz)) {
        return;
    }

    glass.clear_type(player, GlassType::ClaimMap);

    let radius = config.factions.map_radius;
    let nearby = nearby_claims(claims, &world, bx, bz, radius);
    if nearby.is_empty() {
        glass.remove_map_user(&uuid);
        crate::core::send(player, "&cThere are no claims nearby.");
        return;
    }

    let materials = map_materials();
    let mut owners: Vec<&String> = Vec::new();
    let mut assigned: Vec<&'static str> = Vec::new();

    for (owner, claim) in nearby {
        let index = match owners.iter().position(|existing| *existing == owner) {
            Some(index) => index,
            None => {
                owners.push(owner);
                let index = owners.len() - 1;
                assigned.push(materials[index % materials.len()]);
                index
            }
        };

        place_claim_pillars(glass, player, claim, assigned[index]);
    }

    glass.set_map_user(&uuid, &world, cx, cz);
}

/// Regenerates the claim-selection pillars for a player from pos1/pos2.
pub fn update_selection(
    glass: &mut GlassManager,
    player: &Player,
    pos_one: Option<BlockPos>,
    pos_two: Option<BlockPos>,
) {
    glass.clear_type(player, GlassType::ClaimSelection);

    let (_, py, _) = player.get_position();
    let top = (py.floor() as i32 + 60).max(1);
    let world = crate::core::world_name(player);

    for pos in [pos_one, pos_two].into_iter().flatten() {
        for y in 0..top {
            let block = if y % 4 == 0 { DIAMOND_BLOCK } else { GLASS_LIGHT_BLUE };
            glass.place(player, GlassType::ClaimSelection, &world, pos.x, y, pos.z, state_id(block));
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn nearby_claims<'a>(
    claims: &'a ClaimManager,
    world: &str,
    bx: i32,
    bz: i32,
    radius: i32,
) -> Vec<(&'a String, &'a Claim)> {
    let mut result = Vec::new();
    for (owner, list) in &claims.claims {
        for claim in list {
            if claim.world != world {
                continue;
            }
            if claim.max_x < bx - radius || claim.min_x > bx + radius {
                continue;
            }
            if claim.max_z < bz - radius || claim.min_z > bz + radius {
                continue;
            }
            result.push((owner, claim));
        }
    }
    result
}

/// The wall positions along a claim's closest edges (within `radius` blocks).
fn closest_sides(claim: &Claim, bx: i32, bz: i32, radius: i32) -> Vec<(i32, i32)> {
    let mut sides = Vec::new();

    if (bx - claim.min_x).abs() <= radius && bz >= claim.min_z - radius && bz <= claim.max_z + radius {
        for z in (bz - radius).max(claim.min_z)..=(bz + radius).min(claim.max_z) {
            sides.push((claim.min_x, z));
        }
    }
    if (claim.max_x - bx).abs() <= radius && bz >= claim.min_z - radius && bz <= claim.max_z + radius {
        for z in (bz - radius).max(claim.min_z)..=(bz + radius).min(claim.max_z) {
            sides.push((claim.max_x, z));
        }
    }
    if (bz - claim.min_z).abs() <= radius && bx >= claim.min_x - radius && bx <= claim.max_x + radius {
        for x in (bx - radius).max(claim.min_x)..=(bx + radius).min(claim.max_x) {
            sides.push((x, claim.min_z));
        }
    }
    if (claim.max_z - bz).abs() <= radius && bx >= claim.min_x - radius && bx <= claim.max_x + radius {
        for x in (bx - radius).max(claim.min_x)..=(bx + radius).min(claim.max_x) {
            sides.push((x, claim.max_z));
        }
    }

    sides
}

fn place_claim_pillars(glass: &mut GlassManager, player: &Player, claim: &Claim, material: &str) {
    let (_, py, _) = player.get_position();
    let top = (py.floor() as i32 + 60).max(1);
    let world = claim.world.as_str();

    let corners = [
        (claim.min_x, claim.min_z),
        (claim.min_x, claim.max_z),
        (claim.max_x, claim.min_z),
        (claim.max_x, claim.max_z),
    ];

    for (x, z) in corners {
        for y in 0..top {
            let block = if y % 4 == 0 { state_id(material) } else { state_id(GLASS_LIME) };
            glass.place(player, GlassType::ClaimMap, world, x, y, z, block);
        }
    }
}

fn map_materials() -> [&'static str; 20] {
    [
        "minecraft:iron_ore",
        "minecraft:gold_ore",
        "minecraft:coal_ore",
        "minecraft:diamond_ore",
        "minecraft:emerald_ore",
        "minecraft:lapis_ore",
        "minecraft:nether_quartz_ore",
        "minecraft:redstone_ore",
        "minecraft:netherrack",
        "minecraft:end_stone",
        "minecraft:nether_bricks",
        "minecraft:stone",
        "minecraft:terracotta",
        "minecraft:stone_bricks",
        "minecraft:iron_block",
        "minecraft:gold_block",
        "minecraft:emerald_block",
        "minecraft:diamond_block",
        "minecraft:cobblestone",
        "minecraft:bricks",
    ]
}

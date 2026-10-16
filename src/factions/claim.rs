//! Claim store — faithful port of `me.qiooip.lazarus.factions.claim.Claim` /
//! `ClaimManager`.
//!
//! The original stored claims as rectangles and checked **exact block
//! containment** (`getClaimAt`) and **rectangle intersections**. A previous version
//! of this port indexed whole chunks, which made partial-chunk claims protect the
//! entire chunk. We now keep a chunk index only as an accelerator and always verify
//! with exact rectangle math.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ChunkKey = (String, i32, i32);

/// A rectangular claim, ported 1:1 from `factions.claim.Claim`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub world: String,
    pub min_x: i32,
    pub max_x: i32,
    pub min_z: i32,
    pub max_z: i32,
}

impl Claim {
    pub fn new(world: impl Into<String>, x1: i32, x2: i32, z1: i32, z2: i32) -> Self {
        // Port of the constructor: min/max of both coordinates.
        Self {
            world: world.into(),
            min_x: x1.min(x2),
            max_x: x1.max(x2),
            min_z: z1.min(z2),
            max_z: z1.max(z2),
        }
    }

    /// Port of `Claim#contains`: the rectangle is inclusive on all sides.
    pub fn contains(&self, world: &str, x: i32, z: i32) -> bool {
        self.world == world && self.max_x >= x && self.min_x <= x && self.max_z >= z && self.min_z <= z
    }

    /// `sizeX`/`sizeZ` are `max - min` (NOT `+ 1`), exactly like the original.
    pub fn size_x(&self) -> i32 {
        (self.max_x - self.min_x).abs()
    }

    pub fn size_z(&self) -> i32 {
        (self.max_z - self.min_z).abs()
    }

    /// Port of `Claim#expand`.
    pub fn expand(mut self, amount: i32) -> Self {
        self.min_x -= amount;
        self.max_x += amount;
        self.min_z -= amount;
        self.max_z += amount;
        self
    }

    /// Port of `Claim#getCorners`.
    pub fn corners(&self) -> [(i32, i32); 4] {
        [
            (self.min_x, self.min_z),
            (self.min_x, self.max_z),
            (self.max_x, self.min_z),
            (self.max_x, self.max_z),
        ]
    }

    /// Exact rectangle intersection (same world).
    pub fn intersects(&self, other: &Claim) -> bool {
        self.world == other.world
            && self.min_x <= other.max_x
            && self.max_x >= other.min_x
            && self.min_z <= other.max_z
            && self.max_z >= other.min_z
    }

    /// Every chunk covered by this claim, used to (re)build the fast index.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        let (min_cx, min_cz) = crate::util::chunk_key(self.min_x, self.min_z);
        let (max_cx, max_cz) = crate::util::chunk_key(self.max_x, self.max_z);
        let world = self.world.clone();
        (min_cx..=max_cx).flat_map(move |cx| {
            let world = world.clone();
            (min_cz..=max_cz).map(move |cz| (world.clone(), cx, cz))
        })
    }
}

/// Serializable claim storage plus a transient chunk accelerator.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ClaimManager {
    /// faction id -> claims (same layout as the original)
    pub claims: HashMap<String, Vec<Claim>>,
    /// chunk -> (faction id, index into that faction's claim list)
    #[serde(skip)]
    pub index: HashMap<ChunkKey, Vec<(String, usize)>>,
}

impl ClaimManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuilds the chunk index. Call after loading data or any mutation.
    pub fn rebuild_index(&mut self) {
        self.index.clear();
        for (faction_id, claims) in &self.claims {
            for (index, claim) in claims.iter().enumerate() {
                for chunk in claim.chunks() {
                    self.index
                        .entry(chunk)
                        .or_default()
                        .push((faction_id.clone(), index));
                }
            }
        }
    }

    /// Exact block lookup (port of `ClaimManager#getClaimAt`).
    pub fn claim_at(&self, world: &str, x: i32, z: i32) -> Option<(&String, &Claim)> {
        let (cx, cz) = crate::util::chunk_key(x, z);
        let candidates = self.index.get(&(world.to_string(), cx, cz))?;
        for (owner, index) in candidates {
            if let Some(claim) = self.claims.get(owner).and_then(|list| list.get(*index))
                && claim.contains(world, x, z)
            {
                return Some((owner, claim));
            }
        }
        None
    }

    /// Returns the faction id owning the block at `(x, z)`, if any.
    pub fn faction_at(&self, world: &str, x: i32, z: i32) -> Option<&String> {
        self.claim_at(world, x, z).map(|(owner, _)| owner)
    }

    pub fn is_claimed(&self, world: &str, x: i32, z: i32) -> bool {
        self.claim_at(world, x, z).is_some()
    }

    /// Port of `ClaimManager#getSafeLocation`: steps out of every claim at
    /// `(world, x, z)` and returns the first position outside all claims.
    pub fn get_safe_location(&self, world: &str, x: i32, z: i32) -> (String, i32, i32) {
        let mut cx = x;
        let mut cz = z;

        for _ in 0..128 {
            let Some((_, claim)) = self.claim_at(world, cx, cz) else {
                break;
            };
            cx = if claim.max_x < 0 { claim.min_x - 2 } else { claim.max_x + 2 };
            cz = if claim.max_z < 0 { claim.min_z - 2 } else { claim.max_z + 2 };
        }

        if cx == x && cz == z {
            cx += if cx > 0 { 1 } else { -1 };
            cz += if cz > 0 { 1 } else { -1 };
        }

        (world.to_string(), cx, cz)
    }

    pub fn claims_of(&self, faction_id: &str) -> &[Claim] {
        self.claims.get(faction_id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn claim_count(&self, faction_id: &str) -> usize {
        self.claims.get(faction_id).map_or(0, Vec::len)
    }

    /// Adds a claim and rebuilds the index.
    pub fn add_claim(&mut self, faction_id: &str, claim: Claim) {
        self.claims.entry(faction_id.to_string()).or_default().push(claim);
        self.rebuild_index();
    }

    pub fn remove_all(&mut self, faction_id: &str) {
        self.claims.remove(faction_id);
        self.rebuild_index();
    }

    /// Removes the claim owned by `faction_id` that contains `(x, z)`.
    pub fn remove_claim_at(&mut self, faction_id: &str, world: &str, x: i32, z: i32) -> bool {
        let mut removed = false;
        if let Some(list) = self.claims.get_mut(faction_id) {
            if let Some(pos) = list.iter().position(|claim| claim.contains(world, x, z)) {
                list.remove(pos);
                removed = true;
            }
            if list.is_empty() {
                self.claims.remove(faction_id);
            }
        }
        self.rebuild_index();
        removed
    }

    /// Overlap check — port of `ClaimManager#canClaimSelection`'s
    /// `getClaimsInSelection(selection)` test. The original returns the claims of
    /// **every** faction (including the claimant's own), so claiming over your own
    /// claim is also denied.
    pub fn overlaps(&self, candidate: &Claim) -> bool {
        self.claims
            .values()
            .flat_map(|list| list.iter())
            .any(|claim| claim.intersects(candidate))
    }

    /// Buffer check (port of the expanded `getClaimsInSelection` + `canClaimWithBuffer`).
    pub fn within_buffer(
        &self,
        candidate: &Claim,
        buffer: i32,
        ignore_faction: &str,
        ignore_factions: &[String],
    ) -> bool {
        let expanded = candidate.clone().expand(buffer);
        self.claims
            .iter()
            .filter(|(owner, _)| owner.as_str() != ignore_faction && !ignore_factions.contains(owner))
            .flat_map(|(_, list)| list.iter())
            .any(|claim| claim.intersects(&expanded))
    }
}

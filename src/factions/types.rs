//! Faction domain model — direct port of `me.qiooip.lazarus.factions.*`.
//!
//! Bukkit's `PlayerFaction` / `SystemFaction` hierarchy is flattened into a single
//! [`Faction`] struct with a [`FactionKind`] discriminator. This keeps (de)serialisation
//! trivial (no trait objects) while preserving every field of the original model.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::config::Config;

/// A serialisable location. Bukkit's `Location` is replaced by this plain struct
/// because Pumpkin has no persistent location type — world is stored by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
}

impl Location {
    pub fn new(world: impl Into<String>, x: f64, y: f64, z: f64) -> Self {
        Self { world: world.into(), x, y, z, yaw: 0.0, pitch: 0.0 }
    }

    pub fn block_x(&self) -> i32 {
        self.x.floor() as i32
    }

    pub fn block_y(&self) -> i32 {
        self.y.floor() as i32
    }

    pub fn block_z(&self) -> i32 {
        self.z.floor() as i32
    }

    pub fn is_in_world(&self, world: &str) -> bool {
        self.world == world
    }
}

/// Faction member roles, ported from `factions.enums.Role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum Role {
    #[default]
    Member,
    Captain,
    CoLeader,
    Leader,
}

impl Role {
    pub fn prefix(self) -> &'static str {
        match self {
            Role::Member => "",
            Role::Captain => "*",
            Role::CoLeader => "**",
            Role::Leader => "**",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Member => "Member",
            Role::Captain => "Captain",
            Role::CoLeader => "CoLeader",
            Role::Leader => "Leader",
        }
    }

    pub fn is_at_least(self, other: Role) -> bool {
        self >= other
    }

    pub fn promote(self) -> Option<Role> {
        match self {
            Role::Member => Some(Role::Captain),
            Role::Captain => Some(Role::CoLeader),
            _ => None,
        }
    }

    pub fn demote(self) -> Option<Role> {
        match self {
            Role::CoLeader => Some(Role::Captain),
            Role::Captain => Some(Role::Member),
            _ => None,
        }
    }
}

/// Ported from `factions.enums.Relation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Member,
    Ally,
    Enemy,
}

impl Relation {
    pub fn color(self, colors: &crate::config::RelationColors) -> &str {
        match self {
            Relation::Member => &colors.teammate,
            Relation::Ally => &colors.ally,
            Relation::Enemy => &colors.enemy,
        }
    }
}

/// Ported from `factions.enums.ChatType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ChatType {
    #[default]
    Public,
    Faction,
    Ally,
}

/// Distinguishes player factions from the built-in system factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FactionKind {
    #[default]
    Player,
    Wilderness,
    Warzone,
    Safezone,
    Road,
    /// Generic admin-created system faction (`/f systemcreate`).
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionMember {
    pub uuid: String,
    pub name: String,
    pub role: Role,
    pub chat_type: ChatType,
}

/// A faction. Mirrors `Faction` + `PlayerFaction` + `SystemFaction` from Lazarus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Faction {
    pub id: String,
    pub name: String,
    pub kind: FactionKind,
    pub deathban: bool,
    /// Legacy system-faction color (e.g. `&4`), only used by system factions.
    pub color: String,

    // ---- SystemFaction toggles (`/f safezone`, `/f enderpearls`, `/f abilities`) ----
    /// Marks this faction's territory as a safezone.
    #[serde(default)]
    pub safezone: bool,
    #[serde(default = "default_true")]
    pub enderpearls: bool,
    #[serde(default = "default_true")]
    pub abilities: bool,
    /// `/f autorevive` toggle (faction-wide auto revive on deathban).
    #[serde(default)]
    pub auto_revive: bool,

    // ---- PlayerFaction fields ----
    pub dtr: f64,
    pub balance: i64,
    pub lives: i32,
    pub points: i32,
    pub kills: i32,
    pub koths_capped: i32,
    pub home: Option<Location>,
    pub open: bool,
    pub friendly_fire: bool,
    pub allies: Vec<String>,
    /// UUIDs invited to join this faction.
    #[serde(default)]
    pub invitations: Vec<String>,
    /// Faction ids this faction has pending ally requests with.
    #[serde(default)]
    pub ally_invitations: Vec<String>,
    pub members: BTreeMap<String, FactionMember>,
    pub announcement: Option<String>,
    pub created_at: i64,
}

impl Faction {
    pub fn new_player(id: String, name: String, config: &Config) -> Self {
        Self {
            id,
            name,
            kind: FactionKind::Player,
            deathban: true,
            color: String::new(),
            safezone: false,
            enderpearls: true,
            abilities: true,
            auto_revive: false,
            dtr: config.factions.solo_faction_dtr,
            balance: config.economy.default_faction_balance,
            lives: 0,
            points: 0,
            kills: 0,
            koths_capped: 0,
            home: None,
            open: false,
            friendly_fire: false,
            allies: Vec::new(),
            invitations: Vec::new(),
            ally_invitations: Vec::new(),
            members: BTreeMap::new(),
            announcement: None,
            created_at: crate::util::now_millis(),
        }
    }

    pub fn new_system(id: String, name: String, kind: FactionKind, color: String) -> Self {
        Self {
            id,
            name,
            kind,
            deathban: false,
            color,
            safezone: kind == FactionKind::Safezone,
            enderpearls: true,
            abilities: true,
            auto_revive: false,
            dtr: 0.0,
            balance: 0,
            lives: 0,
            points: 0,
            kills: 0,
            koths_capped: 0,
            home: None,
            open: false,
            friendly_fire: false,
            allies: Vec::new(),
            invitations: Vec::new(),
            ally_invitations: Vec::new(),
            members: BTreeMap::new(),
            announcement: None,
            created_at: crate::util::now_millis(),
        }
    }

    pub fn is_player(&self) -> bool {
        self.kind == FactionKind::Player
    }

    pub fn is_system(&self) -> bool {
        self.kind != FactionKind::Player
    }

    pub fn is_safezone(&self) -> bool {
        self.safezone || self.kind == FactionKind::Safezone
    }

    /// Port of `Faction#areEnderpearlEnabled`.
    pub fn are_enderpearls_enabled(&self) -> bool {
        self.enderpearls
    }

    /// Port of `SystemFaction#isAbilities`.
    pub fn abilities_enabled(&self) -> bool {
        self.abilities
    }

    pub fn is_warzone(&self) -> bool {
        self.kind == FactionKind::Warzone
    }

    pub fn is_wilderness(&self) -> bool {
        self.kind == FactionKind::Wilderness
    }

    pub fn is_raidable(&self) -> bool {
        self.is_player() && self.dtr <= 0.0
    }

    pub fn leader(&self) -> Option<&FactionMember> {
        self.members.values().find(|m| m.role == Role::Leader)
    }

    pub fn member(&self, uuid: &str) -> Option<&FactionMember> {
        self.members.get(uuid)
    }

    pub fn member_mut(&mut self, uuid: &str) -> Option<&mut FactionMember> {
        self.members.get_mut(uuid)
    }

    pub fn member_names(&self) -> Vec<&str> {
        self.members.values().map(|m| m.name.as_str()).collect()
    }

    pub fn is_ally(&self, faction_id: &str) -> bool {
        self.allies.iter().any(|id| id == faction_id)
    }

    pub fn max_dtr(&self, config: &Config) -> f64 {
        // Port of `PlayerFaction#getMaxDtr`: solo/empty factions use the fixed
        // solo DTR, otherwise `min(MAX_DTR, members * DTR_PER_PLAYER)`.
        if self.members.len() <= 1 {
            config.factions.solo_faction_dtr
        } else {
            (config.factions.dtr_per_player * self.members.len() as f64)
                .min(config.factions.max_dtr)
        }
    }

    pub fn home_string(&self) -> String {
        match &self.home {
            Some(loc) => format!("{}, {}", loc.block_x(), loc.block_z()),
            None => "None".to_string(),
        }
    }

    /// Port of `Faction#shouldCancelPvpTimerEntrance`: entering such a territory
    /// while PvP protection is active is denied (movement blocked / teleport
    /// cancelled). Wilderness, warzone, safezone and roads allow entry.
    pub fn should_cancel_pvp_timer_entrance(
        &self,
        player_uuid: &str,
        can_enter_own_claim: bool,
    ) -> bool {
        match self.kind {
            FactionKind::Player => {
                !can_enter_own_claim || !self.members.contains_key(player_uuid)
            }
            FactionKind::System => !self.safezone,
            _ => false,
        }
    }

    /// Computes the relation between `self` and another faction.
    pub fn relation_to(&self, other: &Faction) -> Relation {
        if self.id == other.id {
            return Relation::Member;
        }
        if !self.is_player() || !other.is_player() {
            return Relation::Enemy;
        }
        if self.is_ally(&other.id) {
            Relation::Ally
        } else {
            Relation::Enemy
        }
    }
}

fn default_true() -> bool {
    true
}

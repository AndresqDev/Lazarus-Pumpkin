//! Faction registry — port of `me.qiooip.lazarus.factions.FactionsManager`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::types::{Faction, FactionKind, FactionMember, Role};
use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactionsManager {
    pub factions: BTreeMap<String, Faction>,
    pub wilderness_id: String,
    pub warzone_id: String,
    pub safezone_id: String,
    /// Set to true once the built-in wilderness/warzone/safezone factions exist.
    pub initialized: bool,
}

impl Default for FactionsManager {
    fn default() -> Self {
        Self {
            factions: BTreeMap::new(),
            wilderness_id: String::new(),
            warzone_id: String::new(),
            safezone_id: String::new(),
            initialized: false,
        }
    }
}

impl FactionsManager {
    /// Ensures the special system factions exist. Idempotent.
    pub fn init_system_factions(&mut self) {
        if self.initialized {
            return;
        }

        let wilderness = Faction::new_system(
            new_id(),
            "Wilderness".to_string(),
            FactionKind::Wilderness,
            "&2".into(),
        );
        let warzone = Faction::new_system(
            new_id(),
            "Warzone".to_string(),
            FactionKind::Warzone,
            "&4".into(),
        );
        let safezone = Faction::new_system(
            new_id(),
            "Safezone".to_string(),
            FactionKind::Safezone,
            "&a".into(),
        );

        self.wilderness_id = wilderness.id.clone();
        self.warzone_id = warzone.id.clone();
        self.safezone_id = safezone.id.clone();
        self.factions.insert(wilderness.id.clone(), wilderness);
        self.factions.insert(warzone.id.clone(), warzone);
        self.factions.insert(safezone.id.clone(), safezone);
        self.initialized = true;
    }

    pub fn wilderness(&self) -> &Faction {
        &self.factions[&self.wilderness_id]
    }

    pub fn warzone(&self) -> &Faction {
        &self.factions[&self.warzone_id]
    }

    pub fn safezone(&self) -> &Faction {
        &self.factions[&self.safezone_id]
    }

    pub fn get(&self, id: &str) -> Option<&Faction> {
        self.factions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Faction> {
        self.factions.get_mut(id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Faction> {
        self.factions.values().find(|f| f.name.eq_ignore_ascii_case(name))
    }

    /// Returns the player faction containing the given member UUID, if any.
    pub fn by_member(&self, uuid: &str) -> Option<&Faction> {
        self.factions
            .values()
            .find(|f| f.is_player() && f.members.contains_key(uuid))
    }

    pub fn player_factions(&self) -> impl Iterator<Item = &Faction> {
        self.factions.values().filter(|f| f.is_player())
    }

    /// Ids of road system factions (used to ignore them in the claim buffer).
    pub fn road_ids(&self) -> Vec<String> {
        self.factions
            .values()
            .filter(|f| f.kind == FactionKind::Road)
            .map(|f| f.id.clone())
            .collect()
    }

    pub fn player_factions_mut(&mut self) -> impl Iterator<Item = &mut Faction> {
        self.factions.values_mut().filter(|f| f.is_player())
    }

    pub fn name_taken(&self, name: &str) -> bool {
        self.by_name(name).is_some()
    }

    /// Creates a player faction and returns its id.
    pub fn create(&mut self, name: String, config: &Config) -> String {
        let id = new_id();
        let mut faction = Faction::new_player(id.clone(), name, config);
        faction.dtr = config.factions.solo_faction_dtr;
        self.factions.insert(id.clone(), faction);
        id
    }

    pub fn disband(&mut self, id: &str) {
        self.factions.remove(id);
        // Allies elsewhere may still reference the disbanded faction; prune them.
        for faction in self.factions.values_mut() {
            faction.allies.retain(|ally| ally != id);
        }
    }

    pub fn add_member(&mut self, faction_id: &str, member: FactionMember) {
        if let Some(faction) = self.factions.get_mut(faction_id) {
            faction.members.insert(member.uuid.clone(), member);
        }
    }

    pub fn remove_member(&mut self, faction_id: &str, uuid: &str) -> Option<FactionMember> {
        self.factions.get_mut(faction_id).and_then(|f| f.members.remove(uuid))
    }
}

/// Generates a fresh random UUID string.
pub fn new_id() -> String {
    let id = pumpkin_plugin_api::uuid::generate();
    pumpkin_plugin_api::uuid::to_string(id)
}

/// Builds a faction member entry with the default role/chat type.
pub fn member(uuid: String, name: String, role: Role) -> FactionMember {
    FactionMember {
        uuid,
        name,
        role,
        chat_type: Default::default(),
    }
}

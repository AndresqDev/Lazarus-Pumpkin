//! PvP classes — port of `me.qiooip.lazarus.classes`.
//!
//! Activation is armour-based: wearing the full set of a class equips it (after a
//! warmup). Pumpkin has no reliable armour-change event, so the equipment is
//! checked every tick from the main task (see `tasks::tick`).

use std::collections::{BTreeMap, BTreeSet};

use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::{StatusEffectInstance, StatusEffectType};
use pumpkin_plugin_api::{Player, Server};

use crate::state::state;
use crate::core;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClassKind {
    Archer,
    Bard,
    Miner,
    Rogue,
    Mage,
    Bomber,
}

impl ClassKind {
    pub const ALL: [ClassKind; 6] = [
        ClassKind::Archer,
        ClassKind::Bard,
        ClassKind::Miner,
        ClassKind::Rogue,
        ClassKind::Mage,
        ClassKind::Bomber,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ClassKind::Archer => "Archer",
            ClassKind::Bard => "Bard",
            ClassKind::Miner => "Miner",
            ClassKind::Rogue => "Rogue",
            ClassKind::Mage => "Mage",
            ClassKind::Bomber => "Bomber",
        }
    }

    fn config_section(self) -> &'static str {
        match self {
            ClassKind::Archer => "ARCHER_CLASS",
            ClassKind::Bard => "BARD_CLASS",
            ClassKind::Miner => "MINER_CLASS",
            ClassKind::Rogue => "ROGUE_CLASS",
            ClassKind::Mage => "MAGE_CLASS",
            ClassKind::Bomber => "BOMBER_CLASS",
        }
    }

    /// `(helmet, chestplate, leggings, boots)` registry names.
    fn armor(self) -> [&'static str; 4] {
        match self {
            ClassKind::Archer => [
                "minecraft:leather_helmet",
                "minecraft:leather_chestplate",
                "minecraft:leather_leggings",
                "minecraft:leather_boots",
            ],
            ClassKind::Bard => [
                "minecraft:golden_helmet",
                "minecraft:golden_chestplate",
                "minecraft:golden_leggings",
                "minecraft:golden_boots",
            ],
            ClassKind::Miner => [
                "minecraft:iron_helmet",
                "minecraft:iron_chestplate",
                "minecraft:iron_leggings",
                "minecraft:iron_boots",
            ],
            ClassKind::Rogue => [
                "minecraft:chainmail_helmet",
                "minecraft:chainmail_chestplate",
                "minecraft:chainmail_leggings",
                "minecraft:chainmail_boots",
            ],
            ClassKind::Mage => [
                "minecraft:golden_helmet",
                "minecraft:chainmail_chestplate",
                "minecraft:chainmail_leggings",
                "minecraft:golden_boots",
            ],
            ClassKind::Bomber => [
                "minecraft:golden_helmet",
                "minecraft:diamond_chestplate",
                "minecraft:diamond_leggings",
                "minecraft:golden_boots",
            ],
        }
    }
}

/// Legacy 1.8 `MATERIAL_ID` (e.g. `"353:0"`) to a modern registry name.
fn legacy_material(id: &str) -> Option<&'static str> {
    let base = id.split(':').next().unwrap_or(id);
    Some(match base {
        "263" => "minecraft:coal",
        "265" => "minecraft:iron_ingot",
        "280" => "minecraft:stick",
        "288" => "minecraft:feather",
        "295" => "minecraft:wheat_seeds",
        "351" => "minecraft:green_dye", // :2 cactus green
        "353" => "minecraft:sugar",
        "367" => "minecraft:rotten_flesh",
        "370" => "minecraft:ghast_tear",
        "371" => "minecraft:gold_nugget",
        "375" => "minecraft:spider_eye",
        "377" => "minecraft:blaze_powder",
        "378" => "minecraft:magma_cream",
        _ => return None,
    })
}

/// Bukkit potion-effect name (from `classes.yml`) to the Pumpkin status effect.
pub fn effect_by_name(name: &str) -> Option<StatusEffectType> {
    effect_from_name(name)
}

fn effect_from_name(name: &str) -> Option<StatusEffectType> {
    use StatusEffectType as E;
    Some(match name.to_ascii_uppercase().as_str() {
        "SPEED" => E::Speed,
        "SLOW" => E::Slowness,
        "FAST_DIGGING" => E::Haste,
        "SLOW_DIGGING" => E::MiningFatigue,
        "INCREASE_DAMAGE" => E::Strength,
        "HEAL" => E::InstantHealth,
        "HARM" => E::InstantDamage,
        "JUMP" => E::JumpBoost,
        "CONFUSION" => E::Nausea,
        "REGENERATION" => E::Regeneration,
        "DAMAGE_RESISTANCE" => E::Resistance,
        "FIRE_RESISTANCE" => E::FireResistance,
        "WATER_BREATHING" => E::WaterBreathing,
        "INVISIBILITY" => E::Invisibility,
        "BLINDNESS" => E::Blindness,
        "NIGHT_VISION" => E::NightVision,
        "HUNGER" => E::Hunger,
        "WEAKNESS" => E::Weakness,
        "POISON" => E::Poison,
        "WITHER" => E::Wither,
        "HEALTH_BOOST" => E::HealthBoost,
        "ABSORPTION" => E::Absorption,
        "SATURATION" => E::Saturation,
        "GLOWING" => E::Glowing,
        "LEVITATION" => E::Levitation,
        "LUCK" => E::Luck,
        "UNLUCK" => E::Unluck,
        "SLOW_FALLING" => E::SlowFalling,
        _ => return None,
    })
}

pub const INFINITE: u32 = u32::MAX;

/// Bard holdable item: holding `material` buffs nearby faction members.
#[derive(Debug, Clone)]
pub struct BardHoldable {
    pub material: String,
    pub distance_sq: f64,
    pub can_bard_himself: bool,
    pub effect: PassiveEffect,
    pub duration: u32,
}

/// Bard clickable item: right-click applies `effect` (to allies or enemies).
#[derive(Debug, Clone)]
pub struct BardClickable {
    pub material: String,
    pub energy_needed: f64,
    pub distance_sq: f64,
    pub cooldown_ms: i64,
    pub can_bard_himself: bool,
    pub apply_to_enemy: bool,
    pub chat_color: String,
    pub effect: PassiveEffect,
    pub duration: u32,
}

#[derive(Debug, Clone)]
pub struct PassiveEffect {
    pub effect: StatusEffectType,
    pub amplifier: u8,
}

#[derive(Debug, Clone)]
pub struct PvpClass {
    pub kind: ClassKind,
    pub warmup: u64,
    pub effects: Vec<PassiveEffect>,
}

#[derive(Debug, Default)]
pub struct PvpClassManager {
    pub classes: BTreeMap<ClassKind, PvpClass>,
    /// player uuid -> active class
    pub active: BTreeMap<String, ClassKind>,
    /// player uuid -> (class, warmup expiry millis)
    pub warmups: BTreeMap<String, (ClassKind, i64)>,
    /// (class, faction id) -> count
    pub faction_limits: BTreeMap<(ClassKind, String), i32>,
    /// Miner diamond thresholds: (diamonds, name, effects)
    pub miner_rewards: Vec<(i32, String, Vec<PassiveEffect>)>,
    /// Archer-tagged players: uuid -> expiry millis.
    pub archer_tags: BTreeMap<String, i64>,
    /// Bard holdable items.
    pub bard_holdables: Vec<BardHoldable>,
    /// Bard energy: uuid -> current (0..MAX_ENERGY).
    pub bard_energy: BTreeMap<String, f64>,
    /// Bard clickable items.
    pub bard_clickables: Vec<BardClickable>,
    /// (uuid, material) -> cooldown expiry millis.
    pub bard_cooldowns: BTreeMap<(String, String), i64>,
    /// Rogue backstab cooldown: uuid -> expiry millis.
    pub rogue_backstabs: BTreeMap<String, i64>,
    /// Rogue clickable items.
    pub rogue_clickables: Vec<BardClickable>,
    /// (uuid, material) -> cooldown expiry millis.
    pub rogue_click_cooldowns: BTreeMap<(String, String), i64>,
    /// Mage energy and clickables.
    pub mage_energy: BTreeMap<String, f64>,
    pub mage_clickables: Vec<BardClickable>,
    pub mage_cooldowns: BTreeMap<(String, String), i64>,
    /// Bomber TNT gun: (material, cooldown ms).
    pub bomber_gun: Option<(String, i64)>,
    /// Bomber cooldown: uuid -> expiry millis.
    pub bomber_cooldowns: BTreeMap<String, i64>,
    /// Effects snapshot taken when a class activates, restored on deactivation:
    /// uuid -> (effect, duration ticks, amplifier).
    pub effect_cache: BTreeMap<String, Vec<(StatusEffectType, u32, u8)>>,
}

impl PvpClassManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the enabled classes from `classes.yml`.
    pub fn load(&mut self, raw: &crate::yaml::ConfigFile) {
        self.classes.clear();
        for kind in ClassKind::ALL {
            let section = kind.config_section();
            if !raw.bool(&format!("{section}.ENABLED")) {
                continue;
            }

            let mut effects = Vec::new();
            for name in raw.keys(&format!("{section}.PASSIVE_EFFECTS")) {
                if let Some(effect) = effect_from_name(&name) {
                    let level = raw.int(&format!("{section}.PASSIVE_EFFECTS.{name}")).max(1) as u8;
                    effects.push(PassiveEffect {
                        effect,
                        amplifier: level - 1,
                    });
                }
            }

            self.classes.insert(
                kind,
                PvpClass {
                    kind,
                    warmup: raw.int(&format!("{section}.WARMUP")).max(0) as u64,
                    effects,
                },
            );

            if kind == ClassKind::Bard {
                self.bard_holdables.clear();
                for potion in raw.keys("BARD_CLASS.CLICKABLE_ITEMS") {
                    let base = format!("BARD_CLASS.CLICKABLE_ITEMS.{potion}");
                    let Some(effect) = effect_from_name(&potion) else { continue };
                    let material = legacy_material(&raw.string(&format!("{base}.MATERIAL_ID")))
                        .unwrap_or_default()
                        .to_string();
                    let distance = raw.int(&format!("{base}.DISTANCE")).max(0) as f64;
                    self.bard_clickables.push(BardClickable {
                        material,
                        energy_needed: raw.int(&format!("{base}.ENERGY_NEEDED")).max(0) as f64,
                        distance_sq: distance * distance,
                        cooldown_ms: raw.int(&format!("{base}.COOLDOWN")).max(0) * 1000,
                        can_bard_himself: raw.bool(&format!("{base}.CAN_BARD_HIMSELF")),
                        apply_to_enemy: raw.bool(&format!("{base}.APPLY_TO_ENEMY")),
                        chat_color: raw.string(&format!("{base}.CHAT_COLOR")),
                        effect: PassiveEffect {
                            effect,
                            amplifier: (raw.int(&format!("{base}.LEVEL")).max(1) - 1) as u8,
                        },
                        duration: (raw.int(&format!("{base}.DURATION")).max(1) as u32) * 20,
                    });
                }

                for potion in raw.keys("BARD_CLASS.HOLDABLE_ITEMS") {
                    let base = format!("BARD_CLASS.HOLDABLE_ITEMS.{potion}");
                    let Some(effect) = effect_from_name(&potion) else { continue };
                    let material = legacy_material(
                        &raw.string(&format!("{base}.MATERIAL_ID")),
                    )
                    .unwrap_or_default()
                    .to_string();
                    let distance = raw.int(&format!("{base}.DISTANCE")).max(0) as f64;
                    self.bard_holdables.push(BardHoldable {
                        material,
                        distance_sq: distance * distance,
                        can_bard_himself: raw.bool(&format!("{base}.CAN_BARD_HIMSELF")),
                        effect: PassiveEffect {
                            effect,
                            amplifier: (raw.int(&format!("{base}.LEVEL")).max(1) - 1) as u8,
                        },
                        duration: (raw.int(&format!("{base}.DURATION")).max(1) as u32) * 20,
                    });
                }
            }

            if kind == ClassKind::Bomber {
                let material = legacy_material(&raw.string("BOMBER_CLASS.TNT_GUN.MATERIAL_ID"))
                    .unwrap_or_default()
                    .to_string();
                self.bomber_gun = Some((
                    material,
                    raw.int("BOMBER_CLASS.TNT_GUN.COOLDOWN").max(0) * 1000,
                ));
            }

            if kind == ClassKind::Mage {
                self.mage_clickables.clear();
                for potion in raw.keys("MAGE_CLASS.CLICKABLE_ITEMS") {
                    let base = format!("MAGE_CLASS.CLICKABLE_ITEMS.{potion}");
                    let Some(effect) = effect_from_name(&potion) else { continue };
                    let material = legacy_material(&raw.string(&format!("{base}.MATERIAL_ID")))
                        .unwrap_or_default()
                        .to_string();
                    let distance = raw.int(&format!("{base}.DISTANCE")).max(0) as f64;
                    self.mage_clickables.push(BardClickable {
                        material,
                        energy_needed: raw.int(&format!("{base}.ENERGY_NEEDED")).max(0) as f64,
                        distance_sq: distance * distance,
                        cooldown_ms: raw.int(&format!("{base}.COOLDOWN")).max(0) * 1000,
                        can_bard_himself: raw.bool(&format!("{base}.APPLY_TO_HIMSELF")),
                        apply_to_enemy: true,
                        chat_color: raw.string(&format!("{base}.CHAT_COLOR")),
                        effect: PassiveEffect {
                            effect,
                            amplifier: (raw.int(&format!("{base}.LEVEL")).max(1) - 1) as u8,
                        },
                        duration: (raw.int(&format!("{base}.DURATION")).max(1) as u32) * 20,
                    });
                }
            }

            if kind == ClassKind::Rogue {
                self.rogue_clickables.clear();
                for potion in raw.keys("ROGUE_CLASS.CLICKABLE_POTION_EFFECTS") {
                    let base = format!("ROGUE_CLASS.CLICKABLE_POTION_EFFECTS.{potion}");
                    let Some(effect) = effect_from_name(&potion) else { continue };
                    let material = legacy_material(&raw.string(&format!("{base}.MATERIAL_ID")))
                        .unwrap_or_default()
                        .to_string();
                    self.rogue_clickables.push(BardClickable {
                        material,
                        energy_needed: 0.0,
                        distance_sq: 0.0,
                        cooldown_ms: raw.int(&format!("{base}.COOLDOWN")).max(0) * 1000,
                        can_bard_himself: true,
                        apply_to_enemy: false,
                        chat_color: String::new(),
                        effect: PassiveEffect {
                            effect,
                            amplifier: (raw.int(&format!("{base}.LEVEL")).max(1) - 1) as u8,
                        },
                        duration: (raw.int(&format!("{base}.DURATION")).max(1) as u32) * 20,
                    });
                }
            }

            if kind == ClassKind::Miner {
                self.miner_rewards.clear();
                let mut thresholds: Vec<i32> = raw
                    .keys("MINER_CLASS.POTION_EFFECT_REWARDS")
                    .iter()
                    .filter_map(|k| k.parse::<i32>().ok())
                    .collect();
                thresholds.sort_unstable();
                for threshold in thresholds {
                    let base = format!("MINER_CLASS.POTION_EFFECT_REWARDS.{threshold}");
                    let name = raw.string(&format!("{base}.NAME"));
                    let mut effects = Vec::new();
                    for effect_name in raw.keys(&format!("{base}.EFFECTS")) {
                        if let Some(effect) = effect_from_name(&effect_name) {
                            let level = raw.int(&format!("{base}.EFFECTS.{effect_name}")).max(1) as u8;
                            effects.push(PassiveEffect { effect, amplifier: level - 1 });
                        }
                    }
                    self.miner_rewards.push((threshold, name, effects));
                }
            }
        }
    }

    pub fn active_class(&self, uuid: &str) -> Option<ClassKind> {
        self.active.get(uuid).copied()
    }

    /// Applies the Bard holdable buff when the active Bard holds the item.
    pub fn bard_tick(
        &mut self,
        player: &Player,
        server: &Server,
        factions: &crate::factions::FactionsManager,
        raw_config: &crate::yaml::ConfigFile,
        pvp_protected: bool,
        in_safezone: bool,
    ) {
        let uuid = crate::core::uuid_of(player);
        if self.active.get(&uuid) != Some(&ClassKind::Bard) {
            return;
        }

        // `canBard` for holdables: not under pvp protection, not in the safezone.
        // NOTE: combat tag is intentionally allowed.
        if pvp_protected || in_safezone {
            return;
        }

        // Regenerate energy (1 per tick, capped at MAX_ENERGY).
        let max = raw_config.int("BARD_CLASS.MAX_ENERGY").max(1) as f64;
        let energy = self.bard_energy.entry(uuid.clone()).or_insert(max);
        *energy = (*energy + 1.0).min(max);

        let Some(item) = player.get_item_in_hand(pumpkin_plugin_api::common::Hand::Right) else {
            return;
        };
        let key = item.get_registry_key();
        let key = key.trim_start_matches("minecraft:").to_string();

        let Some(holdable) = self
            .bard_holdables
            .iter()
            .find(|h| h.material.trim_start_matches("minecraft:") == key)
            .cloned()
        else {
            return;
        };

        let faction_id = factions.by_member(&uuid).map(|f| f.id.clone());
        let origin = crate::core::location(player);

        for online in server.get_all_players() {
            let member_uuid = crate::core::uuid_of(&online);
            if member_uuid != uuid {
                let same_faction = match &faction_id {
                    Some(id) => factions.by_member(&member_uuid).map(|f| &f.id) == Some(id),
                    None => false,
                };
                if !same_faction {
                    continue;
                }
            } else if !holdable.can_bard_himself {
                continue;
            }

            let target = crate::core::location(&online);
            if target.world != origin.world {
                continue;
            }
            let dx = target.x - origin.x;
            let dy = target.y - origin.y;
            let dz = target.z - origin.z;
            if dx * dx + dy * dy + dz * dz > holdable.distance_sq {
                continue;
            }

            online.add_effect(StatusEffectInstance {
                effect_type: holdable.effect.effect,
                duration: holdable.duration,
                amplifier: holdable.effect.amplifier,
                ambient: false,
                show_particles: true,
                show_icon: true,
            });
        }
    }

    /// Pending warmup for a player: (class, remaining seconds).
    pub fn warmup_info(&self, uuid: &str) -> Option<(ClassKind, i64)> {
        let (kind, expiry) = self.warmups.get(uuid)?;
        let remaining = (*expiry - util::now_millis()).max(0) / 1000;
        Some((*kind, remaining))
    }

    /// Remaining cooldown (seconds) of the active class's clickable, if any.
    pub fn active_cooldown_seconds(&self, uuid: &str) -> Option<i64> {
        let kind = self.active.get(uuid)?;
        let now = util::now_millis();
        let mut remaining = 0i64;

        match kind {
            ClassKind::Bard => {
                for ((owner, _), expiry) in &self.bard_cooldowns {
                    if owner == uuid {
                        remaining = remaining.max(*expiry - now);
                    }
                }
            }
            ClassKind::Mage => {
                for ((owner, _), expiry) in &self.mage_cooldowns {
                    if owner == uuid {
                        remaining = remaining.max(*expiry - now);
                    }
                }
            }
            ClassKind::Rogue => {
                for ((owner, _), expiry) in &self.rogue_click_cooldowns {
                    if owner == uuid {
                        remaining = remaining.max(*expiry - now);
                    }
                }
            }
            ClassKind::Bomber => {
                if let Some(expiry) = self.bomber_cooldowns.get(uuid) {
                    remaining = remaining.max(*expiry - now);
                }
            }
            _ => {}
        }

        if remaining > 0 {
            Some(remaining / 1000)
        } else {
            None
        }
    }

    /// Passive effects of the player's currently active class.
    pub fn active_passive_effects(&self, uuid: &str) -> Vec<PassiveEffect> {
        self.active
            .get(uuid)
            .and_then(|kind| self.classes.get(kind))
            .map(|class| class.effects.clone())
            .unwrap_or_default()
    }

    pub fn is_archer_tagged(&self, uuid: &str) -> bool {
        self.archer_tags
            .get(uuid)
            .is_some_and(|expiry| *expiry > util::now_millis())
    }

    /// Tags `victim` with the Archer effect for `seconds`.
    pub fn tag_archer(&mut self, victim_uuid: &str, seconds: u64) {
        self.archer_tags
            .insert(victim_uuid.to_string(), util::now_millis() + seconds as i64 * 1000);
    }

    /// Wearing the full armour set of a class?
    fn is_wearing_full(player: &Player, kind: ClassKind) -> bool {
        let inventory = player.get_inventory();
        let armor = kind.armor();
        let pieces = [
            inventory.get_helmet(),
            inventory.get_chestplate(),
            inventory.get_leggings(),
            inventory.get_boots(),
        ];
        pieces.iter().zip(armor).all(|(piece, expected)| {
            piece.as_ref().is_some_and(|item| {
                let key = item.get_registry_key();
                let key = key.trim_start_matches("minecraft:");
                key == expected.trim_start_matches("minecraft:")
            })
        })
    }

    /// Per-tick armour check for one player (port of `checkEquipmentChange`).
    /// `language`/`config`/`faction_id` are passed in to avoid re-entering the
    /// state lock (the caller already holds it).
    pub fn tick_player(
        &mut self,
        player: &Player,
        server: &Server,
        language: &crate::yaml::ConfigFile,
        raw_config: &crate::yaml::ConfigFile,
        faction_id: Option<&str>,
    ) {
        let uuid = crate::core::uuid_of(player);
        let now = util::now_millis();

        for kind in ClassKind::ALL {
            let Some(pvp_class) = self.classes.get(&kind).cloned() else {
                continue;
            };

            if Self::is_wearing_full(player, kind) {
                if self.active.get(&uuid) == Some(&kind) {
                    continue;
                }
                if let Some((warming, _)) = self.warmups.get(&uuid) {
                    if *warming == kind {
                        continue;
                    }
                }

                if pvp_class.warmup == 0 {
                    self.activate(player, server, kind, language, raw_config, faction_id);
                } else {
                    self.warmups
                        .insert(uuid.clone(), (kind, now + pvp_class.warmup as i64 * 1000));
                    core::send(
                        player,
                        &util::replace(
                            &language.string("PVP_CLASSES.WARMING_UP"),
                            "<name>",
                            kind.name(),
                        ),
                    );
                }
                return;
            }

            // Not wearing this class's armour: cancel warmup / deactivate.
            if let Some((warming, _)) = self.warmups.get(&uuid).copied() {
                if warming == kind {
                    self.warmups.remove(&uuid);
                    core::send(
                        player,
                        &util::replace(
                            &language.string("PVP_CLASSES.WARMUP_CANCELLED"),
                            "<name>",
                            kind.name(),
                        ),
                    );
                    return;
                }
            }

            if self.active.get(&uuid) == Some(&kind) {
                self.deactivate(player, kind, faction_id);
                core::send(
                    player,
                    &util::replace(
                        &language.string("PVP_CLASSES.DEACTIVATED"),
                        "<name>",
                        kind.name(),
                    ),
                );
                return;
            }
        }
    }

    /// Finishes warmups whose timer expired.
    pub fn tick_warmups(
        &mut self,
        server: &Server,
        language: &crate::yaml::ConfigFile,
        raw_config: &crate::yaml::ConfigFile,
        factions: &crate::factions::FactionsManager,
    ) {
        let now = util::now_millis();
        let ready: Vec<String> = self
            .warmups
            .iter()
            .filter(|(_, (_, expiry))| *expiry <= now)
            .map(|(uuid, _)| uuid.clone())
            .collect();

        for uuid in ready {
            let Some((kind, _)) = self.warmups.remove(&uuid) else {
                continue;
            };
            if let Some(player) = pumpkin_plugin_api::uuid::parse(&uuid)
                .and_then(|id| server.get_player_by_uuid(id))
            {
                let faction_id = factions.by_member(&uuid).map(|f| f.id.clone());
                self.activate(
                    &player,
                    server,
                    kind,
                    language,
                    raw_config,
                    faction_id.as_deref(),
                );
            }
        }
    }

    fn activate(
        &mut self,
        player: &Player,
        _server: &Server,
        kind: ClassKind,
        language: &crate::yaml::ConfigFile,
        raw_config: &crate::yaml::ConfigFile,
        faction_id: Option<&str>,
    ) {
        let uuid = crate::core::uuid_of(player);
        let Some(pvp_class) = self.classes.get(&kind).cloned() else {
            return;
        };

        // Faction limit check.
        if let Some(faction_id) = faction_id {
            let limit = raw_config.int(&format!(
                "FACTION_PVP_CLASS_LIMIT.{}",
                kind.name().to_uppercase()
            ));
            let current = self
                .faction_limits
                .get(&(kind, faction_id.to_string()))
                .copied()
                .unwrap_or(0);
            if limit != -1 && current >= limit as i32 {
                core::send(
                    player,
                    &util::replace(
                        &language.string("FACTIONS.PVP_CLASS_LIMIT.DENY_EQUIP"),
                        "<pvpClass>",
                        kind.name(),
                    ),
                );
                return;
            }
            *self
                .faction_limits
                .entry((kind, faction_id.to_string()))
                .or_insert(0) += 1;
        }

        // Snapshot the player's current effects so we can restore them when the
        // class is removed (the class effects would otherwise overwrite them).
        let snapshot: Vec<(StatusEffectType, u32, u8)> = player
            .get_active_effects()
            .into_iter()
            .filter(|effect| effect.duration < 12000)
            .map(|effect| (effect.effect_type, effect.duration, effect.amplifier))
            .collect();
        self.effect_cache.insert(uuid.clone(), snapshot);

        for effect in &pvp_class.effects {
            player.add_effect(StatusEffectInstance {
                effect_type: effect.effect,
                duration: INFINITE,
                amplifier: effect.amplifier,
                ambient: false,
                show_particles: false,
                show_icon: true,
            });
        }

        if kind == ClassKind::Bard {
            let max = raw_config.int("BARD_CLASS.MAX_ENERGY").max(1) as f64;
            self.bard_energy.insert(uuid.clone(), max);
        }
        if kind == ClassKind::Mage {
            let max = raw_config.int("MAGE_CLASS.MAX_ENERGY").max(1) as f64;
            self.mage_energy.insert(uuid.clone(), max);
        }
        self.active.insert(uuid, kind);
        core::send(
            player,
            &util::replace(&language.string("PVP_CLASSES.ACTIVATED"), "<name>", kind.name()),
        );
    }

    fn deactivate(&mut self, player: &Player, kind: ClassKind, faction_id: Option<&str>) {
        let uuid = crate::core::uuid_of(player);
        let Some(pvp_class) = self.classes.get(&kind).cloned() else {
            return;
        };

        for effect in &pvp_class.effects {
            player.remove_effect(effect.effect);
        }

        if let Some(faction_id) = faction_id {
            let entry = self
                .faction_limits
                .entry((kind, faction_id.to_string()))
                .or_insert(0);
            *entry = (*entry - 1).max(0);
        }

        if kind == ClassKind::Bard {
            self.bard_energy.remove(&uuid);
        }
        if kind == ClassKind::Mage {
            self.mage_energy.remove(&uuid);
        }
        self.active.remove(&uuid);

        // Restore the effects the player had before the class activated.
        if let Some(snapshot) = self.effect_cache.remove(&uuid) {
            for (effect, duration, amplifier) in snapshot {
                player.add_effect(StatusEffectInstance {
                    effect_type: effect,
                    duration,
                    amplifier,
                    ambient: false,
                    show_particles: true,
                    show_icon: true,
                });
            }
        }
    }
}

/// Removes every "infinite" status effect (duration >= 12000 ticks). Port of
/// `PvpClassManager#removeInfiniteEffects`; called on join and on plugin start so
/// stale class effects never survive a restart.
pub fn remove_infinite_effects(player: &Player) {
    for effect in player.get_active_effects() {
        if effect.duration >= 12000 {
            player.remove_effect(effect.effect_type);
        }
    }
}

/// Handles a diamond-ore break for an active Miner (port of `Miner#onBlockBreak`).
pub fn miner_diamond(player: &Player, server: &Server) {
    let uuid = crate::core::uuid_of(player);
    let mut guard = state().write().unwrap();
    if guard.classes.active.get(&uuid) != Some(&ClassKind::Miner) {
        return;
    }

    let count = {
        let Some(user) = guard.userdata.get_mut(&uuid) else {
            return;
        };
        user.diamonds_mined += 1;
        user.diamonds_mined
    };

    let reward = guard
        .classes
        .miner_rewards
        .iter()
        .find(|(threshold, _, _)| *threshold == count)
        .cloned();

    // Re-apply every reward reached so far (the original applies them cumulatively).
    let reached: Vec<PassiveEffect> = guard
        .classes
        .miner_rewards
        .iter()
        .filter(|(threshold, _, _)| *threshold <= count)
        .flat_map(|(_, _, effects)| effects.iter().cloned())
        .collect();
    drop(guard);

    for effect in reached {
        player.add_effect(StatusEffectInstance {
            effect_type: effect.effect,
            duration: INFINITE,
            amplifier: effect.amplifier,
            ambient: false,
            show_particles: false,
            show_icon: true,
        });
    }

    if let Some((_, name, _)) = reward {
        let message = {
            let guard = state().read().unwrap();
            util::replace_all(
                &guard.raw.language.string("MINER_EFFECT_REWARD"),
                &[("<player>", &crate::core::name_of(player)), ("<name>", &name)],
            )
        };
        for online in server.get_all_players() {
            crate::core::send(&online, &message);
        }
    }
}

/// Handles a Bard clickable (right-click with the configured item). Acquires the
/// state lock itself (called from the interact listener, which holds no lock).
pub fn bard_click(server: &Server, player: &Player) {
    let uuid = crate::core::uuid_of(player);
    let location = crate::core::location(player);

    // Active Bard, and `canBard` (staff/pvp-prot/safezone) — combat tag IS allowed.
    {
        let guard = state().read().unwrap();
        if guard.classes.active.get(&uuid) != Some(&ClassKind::Bard) {
            return;
        }
        if guard
            .timers
            .is_active(&uuid, crate::timers::TimerType::PvpProtection)
        {
            return;
        }
        if matches!(
            crate::core::territory_at_guard(&guard, &location.world, location.block_x(), location.block_z()),
            crate::core::Territory::Safezone
        ) {
            return;
        }
    }

    let Some(item) = player.get_item_in_hand(pumpkin_plugin_api::common::Hand::Right) else {
        return;
    };
    let key = item
        .get_registry_key()
        .trim_start_matches("minecraft:")
        .to_string();

    let (clickable, faction_members, energy, now) = {
        let guard = state().read().unwrap();
        let Some(clickable) = guard
            .classes
            .bard_clickables
            .iter()
            .find(|c| c.material.trim_start_matches("minecraft:") == key)
            .cloned()
        else {
            return;
        };
        let faction_members: std::collections::BTreeSet<String> = guard
            .factions
            .by_member(&uuid)
            .map(|f| f.members.keys().cloned().collect())
            .unwrap_or_default();
        let energy = guard.classes.bard_energy.get(&uuid).copied().unwrap_or(0.0);
        (clickable, faction_members, energy, util::now_millis())
    };

    if energy < clickable.energy_needed {
        return;
    }
    {
        let guard = state().read().unwrap();
        if guard
            .classes
            .bard_cooldowns
            .get(&(uuid.clone(), clickable.material.clone()))
            .is_some_and(|expiry| *expiry > now)
        {
            return;
        }
    }

    for online in server.get_all_players() {
        let member_uuid = crate::core::uuid_of(&online);
        let is_self = member_uuid == uuid;
        let same_faction = faction_members.contains(&member_uuid);

        let enabled = if clickable.apply_to_enemy {
            !is_self && !same_faction
        } else {
            same_faction || (is_self && clickable.can_bard_himself)
        };
        if !enabled {
            continue;
        }

        let target = crate::core::location(&online);
        if target.world != location.world {
            continue;
        }
        let dx = target.x - location.x;
        let dy = target.y - location.y;
        let dz = target.z - location.z;
        if dx * dx + dy * dy + dz * dz > clickable.distance_sq {
            continue;
        }

        online.add_effect(StatusEffectInstance {
            effect_type: clickable.effect.effect,
            duration: clickable.duration,
            amplifier: clickable.effect.amplifier,
            ambient: false,
            show_particles: true,
            show_icon: true,
        });

        if clickable.apply_to_enemy && !is_self {
            core::send(
                &online,
                &format!(
                    "{}{}",
                    language_prefix(),
                    util::replace(
                        &state().read().unwrap().raw.language.string("BARD_CLICKABLE_MESSAGE_OTHERS"),
                        "<effect>",
                        &format!("{}{}", clickable.chat_color, clickable.material),
                    )
                ),
            );
        }
    }

    // Pay the cost and start the cooldown.
    let mut guard = state().write().unwrap();
    if let Some(current) = guard.classes.bard_energy.get_mut(&uuid) {
        *current = (*current - clickable.energy_needed).max(0.0);
    }
    guard
        .classes
        .bard_cooldowns
        .insert((uuid, clickable.material.clone()), now + clickable.cooldown_ms);
}

fn language_prefix() -> String {
    state().read().unwrap().raw.language.string("PREFIX")
}

/// Handles a Rogue clickable (right-click, self-only effect with cooldown).
pub fn rogue_click(_server: &Server, player: &Player) {
    let uuid = crate::core::uuid_of(player);
    {
        let guard = state().read().unwrap();
        if guard.classes.active.get(&uuid) != Some(&ClassKind::Rogue) {
            return;
        }
    }

    let Some(item) = player.get_item_in_hand(pumpkin_plugin_api::common::Hand::Right) else {
        return;
    };
    let key = item
        .get_registry_key()
        .trim_start_matches("minecraft:")
        .to_string();

    let (clickable, now) = {
        let guard = state().read().unwrap();
        let Some(clickable) = guard
            .classes
            .rogue_clickables
            .iter()
            .find(|c| c.material.trim_start_matches("minecraft:") == key)
            .cloned()
        else {
            return;
        };
        (clickable, util::now_millis())
    };

    {
        let guard = state().read().unwrap();
        if guard
            .classes
            .rogue_click_cooldowns
            .get(&(uuid.clone(), clickable.material.clone()))
            .is_some_and(|expiry| *expiry > now)
        {
            return;
        }
    }

    player.add_effect(StatusEffectInstance {
        effect_type: clickable.effect.effect,
        duration: clickable.duration,
        amplifier: clickable.effect.amplifier,
        ambient: false,
        show_particles: true,
        show_icon: true,
    });
    state()
        .write()
        .unwrap()
        .classes
        .rogue_click_cooldowns
        .insert((uuid, clickable.material.clone()), now + clickable.cooldown_ms);
}

/// Handles a Mage clickable (right-click): buffs self and debuffs nearby enemies.
pub fn mage_click(server: &Server, player: &Player) {
    let uuid = crate::core::uuid_of(player);

    let (pvp_protected, in_safezone) = {
        let guard = state().read().unwrap();
        if guard.classes.active.get(&uuid) != Some(&ClassKind::Mage) {
            return;
        }
        let location = crate::core::location(player);
        (
            guard
                .timers
                .is_active(&uuid, crate::timers::TimerType::PvpProtection),
            matches!(
                crate::core::territory_at_guard(&guard, &location.world, location.block_x(), location.block_z()),
                crate::core::Territory::Safezone
            ),
        )
    };
    if pvp_protected || in_safezone {
        return;
    }

    let Some(item) = player.get_item_in_hand(pumpkin_plugin_api::common::Hand::Right) else {
        return;
    };
    let key = item
        .get_registry_key()
        .trim_start_matches("minecraft:")
        .to_string();

    let (clickable, faction_members, energy, now) = {
        let guard = state().read().unwrap();
        let Some(clickable) = guard
            .classes
            .mage_clickables
            .iter()
            .find(|c| c.material.trim_start_matches("minecraft:") == key)
            .cloned()
        else {
            return;
        };
        let faction_members: std::collections::BTreeSet<String> = guard
            .factions
            .by_member(&uuid)
            .map(|f| f.members.keys().cloned().collect())
            .unwrap_or_default();
        let energy = guard.classes.mage_energy.get(&uuid).copied().unwrap_or(0.0);
        (clickable, faction_members, energy, util::now_millis())
    };

    if energy < clickable.energy_needed {
        return;
    }
    {
        let guard = state().read().unwrap();
        if guard
            .classes
            .mage_cooldowns
            .get(&(uuid.clone(), clickable.material.clone()))
            .is_some_and(|expiry| *expiry > now)
        {
            return;
        }
    }

    let location = crate::core::location(player);
    let apply = |target: &Player| {
        target.add_effect(StatusEffectInstance {
            effect_type: clickable.effect.effect,
            duration: clickable.duration,
            amplifier: clickable.effect.amplifier,
            ambient: false,
            show_particles: true,
            show_icon: true,
        });
    };

    if clickable.can_bard_himself {
        apply(player);
    }

    let combat_tag = state()
        .read()
        .unwrap()
        .raw
        .config
        .bool("MAGE_CLASS.COMBAT_TAG_ON_CLICKABLE_ITEM");
    let tag_seconds = state().read().unwrap().config.timers.combat_tag_seconds;

    for online in server.get_all_players() {
        let member_uuid = crate::core::uuid_of(&online);
        if member_uuid == uuid || faction_members.contains(&member_uuid) {
            continue;
        }

        let target = crate::core::location(&online);
        if target.world != location.world {
            continue;
        }
        let dx = target.x - location.x;
        let dy = target.y - location.y;
        let dz = target.z - location.z;
        if dx * dx + dy * dy + dz * dz > clickable.distance_sq {
            continue;
        }

        apply(&online);

        if combat_tag {
            state().write().unwrap().timers.activate(
                &member_uuid,
                crate::timers::TimerType::CombatTag,
                tag_seconds,
            );
        }
    }

    let mut guard = state().write().unwrap();
    if let Some(current) = guard.classes.mage_energy.get_mut(&uuid) {
        *current = (*current - clickable.energy_needed).max(0.0);
    }
    guard
        .classes
        .mage_cooldowns
        .insert((uuid, clickable.material.clone()), now + clickable.cooldown_ms);
}

/// Handles the Bomber TNT gun (right-click): spawns a primed TNT.
pub fn bomber_click(server: &Server, player: &Player) {
    let uuid = crate::core::uuid_of(player);

    let (pvp_protected, in_safezone, gun) = {
        let guard = state().read().unwrap();
        if guard.classes.active.get(&uuid) != Some(&ClassKind::Bomber) {
            return;
        }
        let location = crate::core::location(player);
        (
            guard
                .timers
                .is_active(&uuid, crate::timers::TimerType::PvpProtection),
            matches!(
                crate::core::territory_at_guard(&guard, &location.world, location.block_x(), location.block_z()),
                crate::core::Territory::Safezone
            ),
            guard.classes.bomber_gun.clone(),
        )
    };
    if pvp_protected || in_safezone {
        return;
    }
    let Some((material, cooldown_ms)) = gun else {
        return;
    };

    let Some(item) = player.get_item_in_hand(pumpkin_plugin_api::common::Hand::Right) else {
        return;
    };
    if item.get_registry_key().trim_start_matches("minecraft:") != material.trim_start_matches("minecraft:") {
        return;
    }

    let now = util::now_millis();
    {
        let guard = state().read().unwrap();
        if guard
            .classes
            .bomber_cooldowns
            .get(&uuid)
            .is_some_and(|expiry| *expiry > now)
        {
            return;
        }
    }

    let location = crate::core::location(player);
    if let Some(world) = server
        .get_all_worlds()
        .into_iter()
        .find(|w| w.get_name() == location.world)
    {
        let _ = world.spawn_entity(
            pumpkin_plugin_api::EntityType::Tnt,
            (location.x, location.y + 1.0, location.z),
        );
    }

    let combat_tag = state()
        .read()
        .unwrap()
        .raw
        .config
        .bool("BOMBER_CLASS.COMBAT_TAG_ON_TNT_USE");
    let tag_seconds = state().read().unwrap().config.timers.combat_tag_seconds;

    let mut guard = state().write().unwrap();
    guard
        .classes
        .bomber_cooldowns
        .insert(uuid.clone(), now + cooldown_ms);
    if combat_tag {
        guard
            .timers
            .activate(&uuid, crate::timers::TimerType::CombatTag, tag_seconds);
    }
}

/// Re-applies the active class passive effects (used after drinking milk).
pub fn reapply_class_effects(player: &Player) {
    let uuid = crate::core::uuid_of(player);
    let effects = state().read().unwrap().classes.active_passive_effects(&uuid);
    for effect in effects {
        player.add_effect(StatusEffectInstance {
            effect_type: effect.effect,
            duration: INFINITE,
            amplifier: effect.amplifier,
            ambient: false,
            show_particles: false,
            show_icon: true,
        });
    }
}

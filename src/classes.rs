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

/// Bukkit potion-effect name (from `classes.yml`) to the Pumpkin status effect.
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
        }
    }

    pub fn active_class(&self, uuid: &str) -> Option<ClassKind> {
        self.active.get(uuid).copied()
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
                self.deactivate(player, kind);
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

        self.active.insert(uuid, kind);
        core::send(
            player,
            &util::replace(&language.string("PVP_CLASSES.ACTIVATED"), "<name>", kind.name()),
        );
    }

    fn deactivate(&mut self, player: &Player, kind: ClassKind) {
        let uuid = crate::core::uuid_of(player);
        let Some(pvp_class) = self.classes.get(&kind).cloned() else {
            return;
        };

        for effect in &pvp_class.effects {
            player.remove_effect(effect.effect);
        }

        if let Some(faction_id) = crate::core::faction_id_of(&uuid) {
            let entry = self.faction_limits.entry((kind, faction_id)).or_insert(0);
            *entry = (*entry - 1).max(0);
        }

        self.active.remove(&uuid);
    }
}

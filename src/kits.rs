//! Kits — port of `me.qiooip.lazarus.kits.KitsManager` / `KitData` / `kit.KitType`.
//!
//! A kit stores the contents of a player's inventory (main, armor and off-hand)
//! together with a delay. Because the WASM sandbox has no Bukkit serialisation, we
//! store item registry keys, counts, custom names/lore (as JSON text components)
//! and vanilla enchantments.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use pumpkin_plugin_api::enchantments_wit::Enchantment;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{ItemStack, Player};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KitItem {
    pub slot: u8,
    pub key: String,
    pub count: u8,
    pub name: Option<String>,
    pub lore: Vec<String>,
    #[serde(default)]
    pub enchants: Vec<(String, u32)>,
    /// Modern potion id (e.g. `strong_healing`), from the legacy damage value.
    #[serde(default)]
    pub potion: Option<String>,
    /// Original 1.8 potion damage value (kept for reference).
    #[serde(default)]
    pub legacy_damage: Option<i16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Kit {
    pub name: String,
    #[serde(default)]
    pub kind: String,
    /// Cooldown in seconds (`-1` = no cooldown, as in the original).
    pub delay_seconds: i64,
    pub items: Vec<KitItem>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KitsManager {
    pub kits: BTreeMap<String, Kit>,
}

impl KitsManager {
    pub fn get(&self, name: &str) -> Option<&Kit> {
        self.kits.get(&name.to_lowercase())
    }

    pub fn insert(&mut self, kit: Kit) {
        self.kits.insert(kit.name.to_lowercase(), kit);
    }

    pub fn remove(&mut self, name: &str) -> Option<Kit> {
        self.kits.remove(&name.to_lowercase())
    }
}

/// Captures a player's current inventory (slots 0..=40) as a kit.
pub fn capture(player: &Player, name: &str, delay_seconds: i64) -> Kit {
    let mut items = Vec::new();

    for slot in 0..=40u8 {
        let Some(item) = player.get_inventory_item(slot) else {
            continue;
        };
        if item.get_count() == 0 {
            continue;
        }

        let enchants = item
            .get_enchantments()
            .into_iter()
            .map(|value| (enchantment_key(value.enchantment), value.level))
            .collect();

        items.push(KitItem {
            slot,
            key: item.get_registry_key(),
            count: item.get_count(),
            name: item.get_custom_name().map(|component| component.to_json()),
            lore: item.get_lore().into_iter().map(|line| line.to_json()).collect(),
            enchants,
            potion: None,
            legacy_damage: None,
        });
    }

    Kit {
        name: name.to_string(),
        kind: "NORMAL".to_string(),
        delay_seconds,
        items,
    }
}

/// Gives a kit to a player by filling its stored slots.
pub fn give(player: &Player, kit: &Kit) {
    let uuid = crate::core::uuid_of(player);
    let mut potion_slots: Vec<(u8, String)> = Vec::new();

    for entry in &kit.items {
        let item = ItemStack::new(&entry.key, entry.count);

        if let Some(name) = &entry.name
            && let Ok(component) = TextComponent::from_json(name)
        {
            item.set_custom_name(Some(component));
        }

        let lore: Vec<TextComponent> = entry
            .lore
            .iter()
            .filter_map(|line| TextComponent::from_json(line).ok())
            .collect();
        if !lore.is_empty() {
            item.set_lore(lore);
        }

        for (enchantment, level) in &entry.enchants {
            if let Some(enchantment) = enchantment_from_key(enchantment) {
                item.add_enchantment(enchantment, *level);
            }
        }

        player.set_inventory_item(entry.slot, Some(item));

        // Potions need the native bridge to set the `potion-contents` component.
        if let Some(potion) = &entry.potion {
            potion_slots.push((entry.slot, potion.clone()));
        }
    }

    if !potion_slots.is_empty() {
        let folder = crate::state::state().read().unwrap().data_folder.clone();
        for (slot, potion) in potion_slots {
            crate::potion_api::request(&folder, &uuid, slot, &potion);
        }
    }
}

/// A stable string key for a vanilla enchantment (used for persistence).
pub fn enchantment_key(enchantment: Enchantment) -> String {
    format!("{enchantment:?}")
}

/// Parses a key produced by [`enchantment_key`] OR a modern registry key
/// (`sharpness`, `bane_of_arthropods`, …).
pub fn enchantment_from_key(key: &str) -> Option<Enchantment> {
    use Enchantment::*;

    // `sharpness` -> `Sharpness`, `bane_of_arthropods` -> `BaneOfArthropods`.
    let normalized: String = key
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();

    Some(match normalized.as_str() {
        "AquaAffinity" => AquaAffinity,
        "BaneOfArthropods" => BaneOfArthropods,
        "BindingCurse" => BindingCurse,
        "BlastProtection" => BlastProtection,
        "Breach" => Breach,
        "Channeling" => Channeling,
        "Density" => Density,
        "DepthStrider" => DepthStrider,
        "Efficiency" => Efficiency,
        "FeatherFalling" => FeatherFalling,
        "FireAspect" => FireAspect,
        "FireProtection" => FireProtection,
        "Flame" => Flame,
        "Fortune" => Fortune,
        "FrostWalker" => FrostWalker,
        "Impaling" => Impaling,
        "Infinity" => Infinity,
        "Knockback" => Knockback,
        "Looting" => Looting,
        "Loyalty" => Loyalty,
        "LuckOfTheSea" => LuckOfTheSea,
        "Lunge" => Lunge,
        "Lure" => Lure,
        "Mending" => Mending,
        "Multishot" => Multishot,
        "Piercing" => Piercing,
        "Power" => Power,
        "ProjectileProtection" => ProjectileProtection,
        "Protection" => Protection,
        "Punch" => Punch,
        "QuickCharge" => QuickCharge,
        "Respiration" => Respiration,
        "Riptide" => Riptide,
        "Sharpness" => Sharpness,
        "SilkTouch" => SilkTouch,
        "Smite" => Smite,
        "SoulSpeed" => SoulSpeed,
        "SweepingEdge" => SweepingEdge,
        "SwiftSneak" => SwiftSneak,
        "Thorns" => Thorns,
        "Unbreaking" => Unbreaking,
        "VanishingCurse" => VanishingCurse,
        "WindBurst" => WindBurst,
        _ => return None,
    })
}

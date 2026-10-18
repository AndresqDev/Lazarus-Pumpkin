//! Legacy 1.8 item ids <-> modern registry keys, shared across the port.
//!
//! The original had the whole Bukkit `Material` enum available; Pumpkin items
//! are keyed by registry key (`minecraft:diamond_sword`). This table covers the
//! entries the shipped configs reference: the crowbar (`CROWBAR.MATERIAL_ID`),
//! the settings GUI (`PLAYER_SETTINGS.SETTINGS.*.MATERIAL_ID`), the salvage
//! material names and `items.yml` reverse lookups.
//!
//! `parse` keeps the original `ItemUtils#parseItem` semantics for `id:data`
//! strings (data variants that modern Minecraft split into separate items —
//! planks, dyes — are mapped explicitly; the rest resolve by id alone).

use crate::state::state;

/// `(1.8 id, data, registry key)`.
const ITEMS: &[(u32, u16, &str)] = &[
    (4, 0, "minecraft:cobblestone"),
    (5, 0, "minecraft:oak_planks"),
    (25, 0, "minecraft:note_block"),
    (56, 0, "minecraft:diamond_ore"),
    (261, 0, "minecraft:bow"),
    (264, 0, "minecraft:diamond"),
    (265, 0, "minecraft:iron_ingot"),
    (266, 0, "minecraft:gold_ingot"),
    (267, 0, "minecraft:iron_sword"),
    (268, 0, "minecraft:wooden_sword"),
    (269, 0, "minecraft:wooden_shovel"),
    (270, 0, "minecraft:wooden_pickaxe"),
    (271, 0, "minecraft:wooden_axe"),
    (272, 0, "minecraft:stone_sword"),
    (273, 0, "minecraft:stone_shovel"),
    (274, 0, "minecraft:stone_pickaxe"),
    (275, 0, "minecraft:stone_axe"),
    (276, 0, "minecraft:diamond_sword"),
    (277, 0, "minecraft:diamond_shovel"),
    (278, 0, "minecraft:diamond_pickaxe"),
    (279, 0, "minecraft:diamond_axe"),
    (280, 0, "minecraft:stick"),
    (283, 0, "minecraft:golden_sword"),
    (284, 0, "minecraft:golden_shovel"),
    (285, 0, "minecraft:golden_pickaxe"),
    (286, 0, "minecraft:golden_axe"),
    (287, 0, "minecraft:string"),
    (290, 0, "minecraft:wooden_hoe"),
    (291, 0, "minecraft:stone_hoe"),
    (292, 0, "minecraft:iron_hoe"),
    (293, 0, "minecraft:diamond_hoe"),
    (294, 0, "minecraft:golden_hoe"),
    (298, 0, "minecraft:leather_helmet"),
    (299, 0, "minecraft:leather_chestplate"),
    (300, 0, "minecraft:leather_leggings"),
    (301, 0, "minecraft:leather_boots"),
    (302, 0, "minecraft:chainmail_helmet"),
    (303, 0, "minecraft:chainmail_chestplate"),
    (304, 0, "minecraft:chainmail_leggings"),
    (305, 0, "minecraft:chainmail_boots"),
    (306, 0, "minecraft:iron_helmet"),
    (307, 0, "minecraft:iron_chestplate"),
    (308, 0, "minecraft:iron_leggings"),
    (309, 0, "minecraft:iron_boots"),
    (310, 0, "minecraft:diamond_helmet"),
    (311, 0, "minecraft:diamond_chestplate"),
    (312, 0, "minecraft:diamond_leggings"),
    (313, 0, "minecraft:diamond_boots"),
    (314, 0, "minecraft:golden_helmet"),
    (315, 0, "minecraft:golden_chestplate"),
    (316, 0, "minecraft:golden_leggings"),
    (317, 0, "minecraft:golden_boots"),
    (334, 0, "minecraft:leather"),
    (339, 0, "minecraft:paper"),
    (346, 0, "minecraft:fishing_rod"),
    (351, 1, "minecraft:red_dye"),
    (352, 0, "minecraft:bone"),
    (369, 0, "minecraft:blaze_rod"),
    (386, 0, "minecraft:writable_book"),
    (399, 0, "minecraft:nether_star"),
    (403, 0, "minecraft:enchanted_book"),
];

/// 1.8 `id:data` to a registry key. `data` falls back to `0` and then to any
/// entry of that id (1.7 configs wrote bare ids, e.g. `294`, for `294:0`).
pub fn key_of(id: u32, data: u16) -> Option<&'static str> {
    ITEMS
        .iter()
        .find(|(i, d, _)| *i == id && *d == data)
        .or_else(|| ITEMS.iter().find(|(i, d, _)| *i == id && *d == 0))
        .or_else(|| ITEMS.iter().find(|(i, _, _)| *i == id))
        .map(|(_, _, key)| *key)
}

/// Registry key back to its `(1.8 id, data)` (`items.yml` reverse lookups).
pub fn id_of(key: &str) -> Option<(u32, u16)> {
    ITEMS
        .iter()
        .find(|(_, _, k)| *k == key)
        .map(|(id, data, _)| (*id, *data))
}

/// `ItemUtils#parseItem`: parses a config `MATERIAL_ID` (`"386:0"`) into a
/// registry key, passing through anything that is already namespaced.
pub fn parse(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }

    if let Some((id, data)) = entry.split_once(':')
        && let Ok(id) = id.parse::<u32>()
    {
        let data = data.parse::<u16>().unwrap_or(0);
        return key_of(id, data).map(str::to_string);
    }

    // Bare numeric ids (`"294"`) resolve by id alone.
    if let Ok(id) = entry.parse::<u32>() {
        return key_of(id, 0).map(str::to_string);
    }

    if entry.contains(':') {
        Some(entry.to_string())
    } else {
        Some(format!("minecraft:{entry}"))
    }
}

/// `ItemUtils#getMaterialName`: the `items.yml` key for an `id:data` entry
/// (e.g. `"276:0"` -> `"DiamondSword"`); `None` when the file has no entry.
pub fn items_name_opt(entry: &str) -> Option<String> {
    let guard = state().read().unwrap();

    guard
        .raw
        .items
        .root_keys()
        .into_iter()
        .find(|key| guard.raw.items.string(key) == entry)
}

/// [`items_name_opt`] with the original's `entry`-string fallback.
pub fn items_name(entry: &str) -> String {
    items_name_opt(entry).unwrap_or_else(|| entry.to_string())
}

/// `ItemUtils#getItemName(itemStack)`: the display name of a registry key,
/// trying its data variant first and then `id:0` (`"276"` -> `"276:0"`).
pub fn name_of(key: &str) -> String {
    let Some((id, data)) = id_of(key) else {
        return key.to_string();
    };

    let entry = format!("{id}:{data}");
    if let Some(name) = items_name_opt(&entry) {
        return name;
    }

    items_name(&format!("{id}:0"))
}

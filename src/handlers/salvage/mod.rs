//! `/salvage` — port of `me.qiooip.lazarus.handlers.salvage.SalvageHandler`
//! and `commands/SalvageCommand`.
//!
//! The original asked Bukkit for the shaped recipe of the held item
//! (`Bukkit.getRecipesFor`, matching material + durability`0`) and read its
//! ingredient map. Pumpkin has no recipe lookup, so the same information is
//! inlined from the vanilla 1.8 recipe files (`RecipesTools`, `RecipesWeapons`,
//! `RecipesArmor`): one entry per salvageable output with the amounts being the
//! **number of pattern cells** carrying each ingredient (CraftBukkit's
//! `ShapedRecipes#toBukkitRecipe` gave every cell its own character, so
//! `getIngredientMap()` counted cells — helmet 5, chestplate 8, ...).
//!
//! Fidelity notes:
//! * Chainmail armor passes `ItemUtils#isSalvageable` but has no 1.8 recipe, so
//!   the original returned silently after finding none; same here.
//! * `PlayerUtils#addToInventoryOrDropToFloor` drops leftovers on the ground;
//!   the WASM API cannot spawn item entities, so a full inventory loses the
//!   ingredient (documented port deviation).

use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::data_components::DataComponent;
use pumpkin_plugin_api::{ItemStack, Player};

use crate::state::state;
use crate::utils::{self, legacy_items};

/// One crafting recipe as salvage sees it: the output registry key and the
/// `(ingredient key, amount)` pairs in the original `EnumMap<Material, ...>`
/// iteration order (1.8 `Material` ordinals: block materials first, then
/// `DIAMOND < IRON_INGOT < GOLD_INGOT < STICK < STRING`).
struct SalvageRecipe {
    output: &'static str,
    ingredients: &'static [(&'static str, u32)],
}

const PLANK: &str = "minecraft:oak_planks";
const COBBLE: &str = "minecraft:cobblestone";
const IRON: &str = "minecraft:iron_ingot";
const DIAMOND: &str = "minecraft:diamond";
const GOLD: &str = "minecraft:gold_ingot";
const STICK: &str = "minecraft:stick";
const STRING: &str = "minecraft:string";
const LEATHER: &str = "minecraft:leather";

/// `{"XXX", " # ", " # "}` cell counts per tool/armor kind; see [`RECIPES`].
const RECIPES: &[SalvageRecipe] = &[
    // Tools (`RecipesTools`): pickaxe {"XXX"," # "," # "}, shovel {"X","#","#"},
    // axe {"XX","X#"," #"}, hoe {"XX"," #"," #"}.
    SalvageRecipe { output: "minecraft:wooden_pickaxe", ingredients: &[(PLANK, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:stone_pickaxe", ingredients: &[(COBBLE, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:iron_pickaxe", ingredients: &[(IRON, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:diamond_pickaxe", ingredients: &[(DIAMOND, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:golden_pickaxe", ingredients: &[(GOLD, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:wooden_shovel", ingredients: &[(PLANK, 1), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:stone_shovel", ingredients: &[(COBBLE, 1), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:iron_shovel", ingredients: &[(IRON, 1), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:diamond_shovel", ingredients: &[(DIAMOND, 1), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:golden_shovel", ingredients: &[(GOLD, 1), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:wooden_axe", ingredients: &[(PLANK, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:stone_axe", ingredients: &[(COBBLE, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:iron_axe", ingredients: &[(IRON, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:diamond_axe", ingredients: &[(DIAMOND, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:golden_axe", ingredients: &[(GOLD, 3), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:wooden_hoe", ingredients: &[(PLANK, 2), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:stone_hoe", ingredients: &[(COBBLE, 2), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:iron_hoe", ingredients: &[(IRON, 2), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:diamond_hoe", ingredients: &[(DIAMOND, 2), (STICK, 2)] },
    SalvageRecipe { output: "minecraft:golden_hoe", ingredients: &[(GOLD, 2), (STICK, 2)] },
    // Weapons (`RecipesWeapons`): sword {"X","X","#"} and bow {" #X","# X"," #X"}.
    SalvageRecipe { output: "minecraft:wooden_sword", ingredients: &[(PLANK, 2), (STICK, 1)] },
    SalvageRecipe { output: "minecraft:stone_sword", ingredients: &[(COBBLE, 2), (STICK, 1)] },
    SalvageRecipe { output: "minecraft:iron_sword", ingredients: &[(IRON, 2), (STICK, 1)] },
    SalvageRecipe { output: "minecraft:diamond_sword", ingredients: &[(DIAMOND, 2), (STICK, 1)] },
    SalvageRecipe { output: "minecraft:golden_sword", ingredients: &[(GOLD, 2), (STICK, 1)] },
    SalvageRecipe { output: "minecraft:bow", ingredients: &[(STICK, 3), (STRING, 3)] },
    // Armor (`RecipesArmor`): helmet {"XXX","X X"}, chestplate {"X X","XXX","XXX"},
    // leggings {"XXX","X X","X X"}, boots {"X X","X X"}. No chainmail recipes.
    SalvageRecipe { output: "minecraft:leather_helmet", ingredients: &[(LEATHER, 5)] },
    SalvageRecipe { output: "minecraft:iron_helmet", ingredients: &[(IRON, 5)] },
    SalvageRecipe { output: "minecraft:diamond_helmet", ingredients: &[(DIAMOND, 5)] },
    SalvageRecipe { output: "minecraft:golden_helmet", ingredients: &[(GOLD, 5)] },
    SalvageRecipe { output: "minecraft:leather_chestplate", ingredients: &[(LEATHER, 8)] },
    SalvageRecipe { output: "minecraft:iron_chestplate", ingredients: &[(IRON, 8)] },
    SalvageRecipe { output: "minecraft:diamond_chestplate", ingredients: &[(DIAMOND, 8)] },
    SalvageRecipe { output: "minecraft:golden_chestplate", ingredients: &[(GOLD, 8)] },
    SalvageRecipe { output: "minecraft:leather_leggings", ingredients: &[(LEATHER, 7)] },
    SalvageRecipe { output: "minecraft:iron_leggings", ingredients: &[(IRON, 7)] },
    SalvageRecipe { output: "minecraft:diamond_leggings", ingredients: &[(DIAMOND, 7)] },
    SalvageRecipe { output: "minecraft:golden_leggings", ingredients: &[(GOLD, 7)] },
    SalvageRecipe { output: "minecraft:leather_boots", ingredients: &[(LEATHER, 4)] },
    SalvageRecipe { output: "minecraft:iron_boots", ingredients: &[(IRON, 4)] },
    SalvageRecipe { output: "minecraft:diamond_boots", ingredients: &[(DIAMOND, 4)] },
    SalvageRecipe { output: "minecraft:golden_boots", ingredients: &[(GOLD, 4)] },
];

/// `ItemUtils#isSalvageable` hardcoded material list: every armor piece
/// (chainmail included — its missing recipe makes the handler return silently),
/// every tool/sword tier and the bow.
pub fn is_salvageable(key: &str) -> bool {
    if key == "minecraft:bow" {
        return true;
    }

    let Some((tier, kind)) = key.strip_prefix("minecraft:").and_then(|rest| rest.split_once('_')) else {
        return false;
    };

    const TIERS: [&str; 7] = ["wooden", "stone", "iron", "golden", "diamond", "leather", "chainmail"];
    const KINDS: [&str; 9] = ["sword", "pickaxe", "shovel", "axe", "hoe", "helmet", "chestplate", "leggings", "boots"];

    TIERS.contains(&tier) && KINDS.contains(&kind)
}

fn find_recipe(key: &str) -> Option<&'static SalvageRecipe> {
    RECIPES.iter().find(|recipe| recipe.output == key)
}

/// 1.8 `Item#getType().getMaxDurability()` (the values are unchanged in modern
/// Minecraft; the item patch exposes no `max_damage` component on the guest).
fn max_damage(key: &str) -> i32 {
    match key {
        "minecraft:wooden_sword"
        | "minecraft:wooden_shovel"
        | "minecraft:wooden_pickaxe"
        | "minecraft:wooden_axe"
        | "minecraft:wooden_hoe" => 59,
        "minecraft:stone_sword"
        | "minecraft:stone_shovel"
        | "minecraft:stone_pickaxe"
        | "minecraft:stone_axe"
        | "minecraft:stone_hoe" => 131,
        "minecraft:iron_sword"
        | "minecraft:iron_shovel"
        | "minecraft:iron_pickaxe"
        | "minecraft:iron_axe"
        | "minecraft:iron_hoe" => 250,
        "minecraft:diamond_sword"
        | "minecraft:diamond_shovel"
        | "minecraft:diamond_pickaxe"
        | "minecraft:diamond_axe"
        | "minecraft:diamond_hoe" => 1561,
        "minecraft:golden_sword"
        | "minecraft:golden_shovel"
        | "minecraft:golden_pickaxe"
        | "minecraft:golden_axe"
        | "minecraft:golden_hoe" => 32,
        "minecraft:leather_helmet"
        | "minecraft:leather_chestplate"
        | "minecraft:leather_leggings"
        | "minecraft:leather_boots" => 55,
        "minecraft:chainmail_helmet"
        | "minecraft:chainmail_chestplate"
        | "minecraft:chainmail_leggings"
        | "minecraft:chainmail_boots" => 165,
        "minecraft:iron_helmet"
        | "minecraft:iron_chestplate"
        | "minecraft:iron_leggings"
        | "minecraft:iron_boots" => 240,
        "minecraft:diamond_helmet"
        | "minecraft:diamond_chestplate"
        | "minecraft:diamond_leggings"
        | "minecraft:diamond_boots" => 528,
        "minecraft:golden_helmet"
        | "minecraft:golden_chestplate"
        | "minecraft:golden_leggings"
        | "minecraft:golden_boots" => 112,
        "minecraft:bow" => 384,
        _ => 0,
    }
}

/// `ItemStack#getDurability()`: the `minecraft:damage` component, a single
/// varint.
fn item_damage(item: &ItemStack) -> i32 {
    let Some(value) = item
        .get_components()
        .into_iter()
        .find(|value| matches!(value.component, DataComponent::Damage))
    else {
        return 0;
    };

    let mut index = 0usize;
    read_varint(&value.value, &mut index).unwrap_or(0)
}

fn read_varint(bytes: &[u8], index: &mut usize) -> Option<i32> {
    let mut value = 0i32;
    let mut shift = 0;

    loop {
        let byte = *bytes.get(*index)?;
        *index += 1;
        value |= i32::from(byte & 0x7f) << shift;

        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
        if shift >= 32 {
            return Some(value);
        }
    }
}

/// `PlayerUtils#addToInventoryOrDropToFloor`: the host cannot spawn item
/// entities, so a full inventory loses the stack (see the module header).
fn add_to_inventory(player: &Player, stack: ItemStack) {
    crate::handlers::block::give_stack_to_inventory(player, stack);
}

/// `ItemUtils.removeOneItem(player)`.
fn remove_one_item(player: &Player, item: ItemStack) {
    let count = item.get_count();
    if count > 1 {
        item.set_count(count - 1);
        player.set_item_in_hand(Hand::Right, Some(item));
    } else {
        player.set_item_in_hand(Hand::Right, None);
    }
}

/// `SalvageHandler#handleSalvage`.
pub fn handle_salvage(player: &Player) {
    let Some(hand_item) = player.get_item_in_hand(Hand::Right) else {
        return;
    };
    let key = hand_item.get_registry_key();

    let Some(recipe) = find_recipe(&key) else {
        return;
    };

    // Durability and enchantments are read off the Java object *after*
    // `removeOneItem` in the original; the WASM handle is consumed by the
    // removal, so both are captured first (same values).
    let max = max_damage(&key);
    let damage = item_damage(&hand_item);
    let enchantments = hand_item.get_enchantments();

    // `(maxDurability - durability) / maxDurability`.
    let percentage = if max <= 0 {
        0.0
    } else {
        f64::from(max - damage) / f64::from(max)
    };

    remove_one_item(player, hand_item);

    let (salvaged, ingredient_format) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("SALVAGE_COMMAND.SALVAGED"),
            guard.raw.language.string("SALVAGE_COMMAND.INGREDIENT_FORMAT"),
        )
    };

    let mut message = String::new();
    for (ingredient, amount) in recipe.ingredients {
        // `(int) Math.round(Math.max(1, amount * percentage))`.
        let amount = ((f64::from(*amount) * percentage).max(1.0)).round() as u8;
        let stack = ItemStack::new(ingredient, amount);
        let material = legacy_items::name_of(ingredient);

        add_to_inventory(player, stack);

        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&utils::replace_all(
            &ingredient_format,
            &[("<amount>", &amount.to_string()), ("<material>", &material)],
        ));
    }

    // `dropRandomEnchantedBook`.
    if !enchantments.is_empty() {
        let entry = &enchantments[random_index(enchantments.len())];
        let book = ItemStack::new("minecraft:enchanted_book", 1);
        book.add_enchantment(entry.enchantment, entry.level);

        // `EnchantmentStorageMeta`: move the `Enchantments` patch over to
        // `StoredEnchantments` (same codec, see `handlers::limiter`).
        let bytes = book.get_components().into_iter().find_map(|value| {
            matches!(value.component, DataComponent::Enchantments).then_some(value.value)
        });
        if let Some(bytes) = bytes {
            book.set_component(DataComponent::StoredEnchantments, &bytes);
            book.remove_component(DataComponent::Enchantments);
        }

        add_to_inventory(player, book);

        message.push('\n');
        message.push_str(&utils::replace_all(
            &ingredient_format,
            &[
                ("<amount>", "1"),
                ("<material>", &legacy_items::name_of("minecraft:enchanted_book")),
            ],
        ));
    }

    let durability = round_two(percentage * 100.0);
    crate::utils::core::send(
        player,
        utils::replace_all(
            &salvaged,
            &[("<item>", &legacy_items::name_of(&key)), ("<durability>", &durability)],
        ),
    );
    crate::utils::core::send(player, &message);
}

fn random_index(bound: usize) -> usize {
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    // splitmix64 seeded with the clock + a counter (the port has no RNG crate).
    let mut z = (crate::utils::now_millis() as u64)
        ^ COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;

    (z % bound as u64) as usize
}

/// `(double) Math.round(pct * 100 * 100d) / 100d` rendered like Java's
/// `Double.toString` (two decimals, integral values keep one decimal: `100.0`).
fn round_two(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.1}")
    } else {
        format!("{rounded}")
    }
}

//! Limiters — port of `handlers.limiter.*`.
//!
//! Three independent pieces, mirroring the original classes:
//!
//! * [`EnchantTableListener`] / [`AnvilClickListener`] — `EnchantLimiterHandler`.
//! * [`PotionConsumeListener`] / [`BrewListener`] — `PotionLimiterHandler`.
//! * [`enforce_disabled_entities`] — the disabled-entity half of
//!   `EntityLimiterHandler`.
//!
//! `limiters.yml` values are parsed once and cached; [`reload`] drops the cache
//! and is called by `/lazarus reload` (the original kept the startup values
//! until a restart — applying the new file is the port's improvement).
//!
//! Fidelity notes / deviations (see `PORTING.md` for the full list):
//!
//! * `EnchantItemEvent` round-trips `enchantments-to-add`, so enchanting
//!   tables are clamped in place exactly like the original.
//! * Anvil results: the host hands the guest *clones* of `clicked-item` /
//!   `cursor` and ignores item mutations in the returned event, so the result
//!   slot itself cannot be rewritten. In place of the original's
//!   `ItemUtils.updateInventory(player)` the port refreshes the player's whole
//!   inventory (main, armor, off-hand) one tick later — when a taken result
//!   has landed there — and clamps every enchant found. Unrepairable items are
//!   still denied in place (click cancelled + message) like the original.
//! * Enchanted books are clamped by rewriting the `stored-enchantments`
//!   component (`varint count` + `(varint id, varint level)` pairs). The
//!   guest cannot read the server registry, so the id→enchantment mapping is
//!   probed at the first book using throw-away stacks.
//! * `EntityDeathEvent` carries no drops and the host never fires
//!   `PlayerFishEvent`, so the original's drop/fishing clamp sites are not
//!   reachable (its fishing branch is dead code on 1.8 anyway).
//! * Potions are classified through the native component table
//!   ([`crate::utils::potion_components::potion_name_for_item`]): the registry id the
//!   item carries plus the vanilla effects of that potion type — the same data
//!   `Potion.fromItemStack` produced on 1.8.
//! * Brewing: `BrewEvent` carries no contents; the stand is read through the
//!   block-entity container API and the pre-brew potions are restored one tick
//!   later (the event fires before the slots are replaced).
//! * Splash potions are denied in `PlayerItemConsumeEvent`, before the throw
//!   exists (the host has no splash event with an item): identical outcome for
//!   players, but potions thrown by dispensers/witches are not limited.
//! * Disabling an entity is enforced by a one-second sweep that removes the
//!   disabled entities from the world (the original cancelled their
//!   `EntitySpawnEvent`, and its `VehicleCreateEvent` companion removed the
//!   vehicle the same way). The event cannot be used here: registering
//!   `EntitySpawnEvent` deadlocks the host when the plugin spawns entities
//!   (see `crate::hologram`), and natural mob spawns do not reach the plugin
//!   anyway. The one-second flash is the deviation this buys.
//! * `MOBS_SPAWN_ONLY_FROM_SPAWNERS` and `MOB_LIMIT_PER_CHUNK` are not
//!   enforceable through the plugin API: natural mob spawns fire no events
//!   (no `CreatureSpawnEvent`, no spawn reason, no chunk entity query).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pumpkin_plugin_api::block_entity::BlockEntityType;
use pumpkin_plugin_api::common::{BlockPos, Hand};
use pumpkin_plugin_api::data_components::DataComponent;
use pumpkin_plugin_api::enchantments_wit::Enchantment;
use pumpkin_plugin_api::events::{
    BrewEvent, EnchantItemEvent, EventData, EventHandler, InventoryClickEvent,
    PlayerItemConsumeEvent,
};
use pumpkin_plugin_api::scheduler::SchedulerExt;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::wit::pumpkin::plugin::status_effect::StatusEffectType;
use pumpkin_plugin_api::{EntityType, ItemStack, Player, Screen, Server, World};

use crate::utils::potion_components;
use crate::state::state;
use crate::{kits, utils, utils::core};

// ---------------------------------------------------------------------------
// Configuration (`limiters.yml` + the unrepairable lore from `config.yml`)
// ---------------------------------------------------------------------------

struct Limits {
    /// `ENCHANTMENT_LIMITER`: enchantment key (`kits::enchantment_key`) → level.
    enchantments: Vec<(String, i32)>,
    /// `POTION_LIMITER`: effect → (max level, extended potions allowed).
    potions: Vec<(StatusEffectType, i32, bool)>,
    /// `ENTITY_LIMITER`: disabled entities as modern registry names.
    entities: Vec<String>,
    /// `config.yml#UNREPAIRABLE_ITEM_LORE` in plain text.
    unrepairable_lore: Option<String>,
}

static LIMITS: Mutex<Option<Limits>> = Mutex::new(None);

/// Invalidates the parsed limits; called from `/lazarus reload`.
pub fn reload() {
    *LIMITS.lock().expect("limiter mutex poisoned") = None;
}

fn with_limits<R>(f: impl FnOnce(&Limits) -> R) -> R {
    let mut guard = LIMITS.lock().expect("limiter mutex poisoned");
    if guard.is_none() {
        *guard = Some(Limits::load());
    }
    f(guard.as_ref().expect("limits were just loaded"))
}

impl Limits {
    fn load() -> Self {
        let guard = state().read().unwrap();
        let raw = &guard.raw;

        let mut enchantments = Vec::new();
        for key in raw.limiters.keys("ENCHANTMENT_LIMITER") {
            let level = raw.limiters.int(&format!("ENCHANTMENT_LIMITER.{key}")) as i32;
            if level < 0 {
                continue;
            }
            if let Some(enchantment) = legacy_enchantment(&key) {
                enchantments.push((kits::enchantment_key(enchantment), level));
            }
        }

        let mut potions = Vec::new();
        for key in raw.limiters.keys("POTION_LIMITER") {
            let level = raw.limiters.int(&format!("POTION_LIMITER.{key}.LEVEL")) as i32;
            if level < 0 {
                continue;
            }
            if let Some(effect) = crate::classes::effect_by_name(&key) {
                let extended = raw.limiters.bool(&format!("POTION_LIMITER.{key}.EXTENDED"));
                potions.push((effect, level, extended));
            }
        }

        let mut entities = Vec::new();
        for key in raw.limiters.keys("ENTITY_LIMITER") {
            if raw.limiters.bool(&format!("ENTITY_LIMITER.{key}")) {
                continue;
            }
            entities.push(legacy_entity_name(&key));
        }

        let configured = raw.config.string("UNREPAIRABLE_ITEM_LORE");
        let unrepairable_lore = if configured.is_empty() {
            None
        } else {
            Some(plain(&utils::text(&configured)))
        };

        Limits {
            enchantments,
            potions,
            entities,
            unrepairable_lore,
        }
    }
}

/// `Enchantment.getByName` for the 1.8 names used by `limiters.yml`
/// (`Enchantment` was renamed in 1.13); modern keys pass through.
fn legacy_enchantment(name: &str) -> Option<Enchantment> {
    let upper = name.to_ascii_uppercase();
    let modern: &str = match upper.as_str() {
        "ARROW_DAMAGE" => "power",
        "ARROW_FIRE" => "flame",
        "ARROW_INFINITE" => "infinity",
        "ARROW_KNOCKBACK" => "punch",
        "DAMAGE_ALL" => "sharpness",
        "DAMAGE_ARTHROPODS" => "bane_of_arthropods",
        "DAMAGE_UNDEAD" => "smite",
        "DIG_SPEED" => "efficiency",
        "DURABILITY" => "unbreaking",
        "FIRE_ASPECT" => "fire_aspect",
        "KNOCKBACK" => "knockback",
        "LOOT_BONUS_BLOCKS" => "fortune",
        "LOOT_BONUS_MOBS" => "looting",
        "LUCK" => "luck_of_the_sea",
        "LURE" => "lure",
        "OXYGEN" => "respiration",
        "PROTECTION_ENVIRONMENTAL" => "protection",
        "PROTECTION_EXPLOSIONS" => "blast_protection",
        "PROTECTION_FALL" => "feather_falling",
        "PROTECTION_FIRE" => "fire_protection",
        "PROTECTION_PROJECTILE" => "projectile_protection",
        "SILK_TOUCH" => "silk_touch",
        "THORNS" => "thorns",
        "WATER_WORKER" => "aqua_affinity",
        other => other,
    };

    kits::enchantment_from_key(&modern.to_ascii_lowercase())
}

/// `EntityType.valueOf` for the 1.8 names used by `limiters.yml`; modern keys
/// pass through. Unknown names are kept as-is (they simply never match).
fn legacy_entity_name(name: &str) -> String {
    match name.to_ascii_uppercase().as_str() {
        "DROPPED_ITEM" => "item".to_string(),
        "ENDER_CRYSTAL" => "end_crystal".to_string(),
        "ENDER_SIGNAL" => "eye_of_ender".to_string(),
        "FIREWORK" => "firework_rocket".to_string(),
        "FISHING_HOOK" => "fishing_bobber".to_string(),
        "LEASH_HITCH" => "leash_knot".to_string(),
        "MINECART" => "minecart".to_string(),
        "MINECART_CHEST" => "chest_minecart".to_string(),
        "MINECART_COMMAND" => "command_block_minecart".to_string(),
        "MINECART_FURNACE" => "furnace_minecart".to_string(),
        "MINECART_HOPPER" => "hopper_minecart".to_string(),
        "MINECART_MOB_SPAWNER" => "spawner_minecart".to_string(),
        "MINECART_TNT" => "tnt_minecart".to_string(),
        "MUSHROOM_COW" => "mooshroom".to_string(),
        "PIG_ZOMBIE" => "zombified_piglin".to_string(),
        "PRIMED_TNT" => "tnt".to_string(),
        "SNOWMAN" => "snow_golem".to_string(),
        "THROWN_EXP_BOTTLE" => "experience_bottle".to_string(),
        "BOAT" => "boat".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

/// Plain text of a text component (styling stripped), used for the lore
/// comparison like everywhere else in the port.
fn plain(component: &TextComponent) -> String {
    crate::handlers::block::component_text(&component.to_json())
}

fn send_message(player: &Player, key: &str) {
    let guard = state().read().unwrap();
    let prefix = guard.raw.language.string("PREFIX");
    let message = guard.raw.language.string(key);
    drop(guard);

    core::send(player, &format!("{prefix}{message}"));
}

// ---------------------------------------------------------------------------
// Enchantment limiter
// ---------------------------------------------------------------------------

/// `EnchantLimiterHandler#onEnchantItem`: the host applies the mutations on the
/// returned `enchantments-to-add` list.
pub struct EnchantTableListener;

impl EventHandler<EnchantItemEvent> for EnchantTableListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<EnchantItemEvent>,
    ) -> EventData<EnchantItemEvent> {
        if event.cancelled {
            return event;
        }

        for value in event.enchantments_to_add.iter_mut() {
            let Some(limit) = enchantment_limit(value.enchantment) else {
                continue;
            };
            if i64::from(value.level) <= i64::from(limit) {
                continue;
            }

            value.level = if limit < 1 { 0 } else { limit as u32 };
        }
        event.enchantments_to_add.retain(|value| value.level > 0);

        event
    }
}

/// `EnchantLimiterHandler#onInventoryClick` for anvil result clicks.
pub struct AnvilClickListener;

impl EventHandler<InventoryClickEvent> for AnvilClickListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<InventoryClickEvent>,
    ) -> EventData<InventoryClickEvent> {
        if event.cancelled || event.window_type != Some(Screen::Anvil) {
            return event;
        }

        // Anvil window: 0/1 inputs, 2 result, 3+ the player part.
        if event.slot == 2 {
            let Some(item) = event.clicked_item.as_ref() else {
                return event;
            };

            // Books are clamped whatever lore they carry (the original checked
            // `EnchantmentStorageMeta` before the unrepairable-lore branch).
            if has_stored_enchantments(item) {
                if clamp_item(item, None) {
                    schedule_sweep(server, &event.player);
                }
                return event;
            }

            if has_unrepairable_lore(item) {
                event.cancelled = true;
                send_message(&event.player, "UNREPAIRABLE_ITEM_MESSAGE");
                return event;
            }

            if clamp_item(item, None) {
                schedule_sweep(server, &event.player);
            }
            return event;
        }

        // The original rewrote the result slot in place; a result taken to the
        // cursor first (leave-click) is only seen by the next sweep.
        if event.slot >= 3
            && let Some(cursor) = event.cursor.as_ref()
            && clamp_item(cursor, None)
        {
            schedule_sweep(server, &event.player);
        }

        event
    }
}

/// `EnchantLimiterHandler#updateEnchantments` — clamps the item in place and
/// reports whether anything changed.
fn clamp_item(item: &ItemStack, unrepairable: Option<&str>) -> bool {
    if has_stored_enchantments(item) {
        return clamp_stored_enchantments(item);
    }

    if let Some(unrepairable) = unrepairable
        && item.get_lore().iter().any(|line| plain(line) == unrepairable)
    {
        return false;
    }

    clamp_enchantments(item)
}

fn clamp_enchantments(item: &ItemStack) -> bool {
    let mut changed = false;

    for value in item.get_enchantments() {
        let Some(limit) = enchantment_limit(value.enchantment) else {
            continue;
        };
        if i64::from(value.level) <= i64::from(limit) {
            continue;
        }

        item.remove_enchantment(value.enchantment);
        changed = true;

        if limit > 0 {
            item.add_enchantment(value.enchantment, limit as u32);
        }
    }

    changed
}

/// `EnchantLimiterHandler#updateEnchantmentBook`: rewrites the
/// `stored-enchantments` component. Its bytes are the component's network
/// codec: `varint count` followed by `(varint id, varint level)` pairs.
fn clamp_stored_enchantments(item: &ItemStack) -> bool {
    let Some(bytes) = stored_enchantment_bytes(item) else {
        return false;
    };
    let Some(entries) = parse_enchantment_list(&bytes) else {
        tracing::warn!(
            "lazarus: could not parse `stored-enchantments` ({bytes:?}); \
             leaving the book untouched"
        );
        return false;
    };

    let ids = enchantment_ids();
    let mut kept = Vec::with_capacity(entries.len());
    let mut changed = false;

    for (id, level) in entries {
        let limit = ids.get(&id).copied().and_then(enchantment_limit);
        let Some(limit) = limit else {
            kept.push((id, level));
            continue;
        };
        if i64::from(level) <= i64::from(limit) {
            kept.push((id, level));
            continue;
        }

        changed = true;
        if limit > 0 {
            kept.push((id, limit));
        }
    }

    if changed {
        let bytes = serialize_enchantment_list(&kept);
        item.set_component(DataComponent::StoredEnchantments, &bytes);
    }

    changed
}

fn enchantment_limit(enchantment: Enchantment) -> Option<i32> {
    let key = kits::enchantment_key(enchantment);
    with_limits(|limits| {
        limits
            .enchantments
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, level)| *level)
    })
}

fn has_unrepairable_lore(item: &ItemStack) -> bool {
    let Some(configured) = with_limits(|limits| limits.unrepairable_lore.clone()) else {
        return false;
    };
    item.get_lore().iter().any(|line| plain(line) == configured)
}

fn has_stored_enchantments(item: &ItemStack) -> bool {
    item.get_components()
        .into_iter()
        .any(|value| matches!(value.component, DataComponent::StoredEnchantments))
}

fn stored_enchantment_bytes(item: &ItemStack) -> Option<Vec<u8>> {
    component_bytes(item, ComponentKind::StoredEnchantments)
}

fn enchantment_component_bytes(item: &ItemStack) -> Option<Vec<u8>> {
    component_bytes(item, ComponentKind::Enchantments)
}

enum ComponentKind {
    Enchantments,
    StoredEnchantments,
}

fn component_bytes(item: &ItemStack, wanted: ComponentKind) -> Option<Vec<u8>> {
    item.get_components().into_iter().find_map(|value| {
        let matches = match wanted {
            ComponentKind::Enchantments => matches!(value.component, DataComponent::Enchantments),
            ComponentKind::StoredEnchantments => {
                matches!(value.component, DataComponent::StoredEnchantments)
            }
        };

        matches.then_some(value.value)
    })
}

/// Runs [`sweep_player_inventory`] on the next tick. Only the UUID crosses the
/// tick boundary: the host releases the event's player resource when the
/// handler returns.
fn schedule_sweep(server: Server, player: &Player) {
    let uuid = core::uuid_of(player);
    server.schedule_delayed_task(1, move |server| {
        let Some(player) =
            pumpkin_plugin_api::uuid::parse(&uuid).and_then(|id| server.get_player_by_uuid(id))
        else {
            return;
        };
        sweep_player_inventory(&player);
    });
}

/// `ItemUtils.updateInventory(player)` equivalent: clamps every enchant the
/// player carries (main inventory, armor, off-hand) and writes the changed
/// slots back so the client resyncs.
fn sweep_player_inventory(player: &Player) -> bool {
    let unrepairable = with_limits(|limits| limits.unrepairable_lore.clone());
    let unrepairable = unrepairable.as_deref();

    let inventory = player.get_inventory();
    let main = inventory.as_inventory();

    let mut changed = false;
    for slot in 0..main.get_size() {
        let Some(item) = main.get_item(slot) else {
            continue;
        };
        if clamp_item(&item, unrepairable) {
            main.set_item(slot, Some(item));
            changed = true;
        }
    }

    macro_rules! clamp_worn {
        ($get:ident, $set:ident) => {
            if let Some(item) = inventory.$get()
                && clamp_item(&item, unrepairable)
            {
                inventory.$set(Some(item));
                changed = true;
            }
        };
    }

    clamp_worn!(get_helmet, set_helmet);
    clamp_worn!(get_chestplate, set_chestplate);
    clamp_worn!(get_leggings, set_leggings);
    clamp_worn!(get_boots, set_boots);
    clamp_worn!(get_off_hand, set_off_hand);

    changed
}

// ---------------------------------------------------------------------------
// Enchantment / component codecs
// ---------------------------------------------------------------------------

/// Every enchantment of the WIT enum, used to probe the server's registry ids.
const ALL_ENCHANTMENTS: [Enchantment; 43] = [
    Enchantment::AquaAffinity,
    Enchantment::BaneOfArthropods,
    Enchantment::BindingCurse,
    Enchantment::BlastProtection,
    Enchantment::Breach,
    Enchantment::Channeling,
    Enchantment::Density,
    Enchantment::DepthStrider,
    Enchantment::Efficiency,
    Enchantment::FeatherFalling,
    Enchantment::FireAspect,
    Enchantment::FireProtection,
    Enchantment::Flame,
    Enchantment::Fortune,
    Enchantment::FrostWalker,
    Enchantment::Impaling,
    Enchantment::Infinity,
    Enchantment::Knockback,
    Enchantment::Looting,
    Enchantment::Loyalty,
    Enchantment::LuckOfTheSea,
    Enchantment::Lunge,
    Enchantment::Lure,
    Enchantment::Mending,
    Enchantment::Multishot,
    Enchantment::Piercing,
    Enchantment::Power,
    Enchantment::ProjectileProtection,
    Enchantment::Protection,
    Enchantment::Punch,
    Enchantment::QuickCharge,
    Enchantment::Respiration,
    Enchantment::Riptide,
    Enchantment::Sharpness,
    Enchantment::SilkTouch,
    Enchantment::Smite,
    Enchantment::SoulSpeed,
    Enchantment::SweepingEdge,
    Enchantment::SwiftSneak,
    Enchantment::Thorns,
    Enchantment::Unbreaking,
    Enchantment::VanishingCurse,
    Enchantment::WindBurst,
];

static ENCHANTMENT_IDS: OnceLock<HashMap<u8, Enchantment>> = OnceLock::new();

/// The guest cannot read the server's enchantment registry, so the ids used by
/// the `enchantments`/`stored-enchantments` codec are probed: every candidate
/// is added to a throw-away stack (the host does not validate levels) and the
/// id is read back from the patched component.
///
/// An empty map (host changed) logs a warning and makes books fail open.
fn enchantment_ids() -> &'static HashMap<u8, Enchantment> {
    ENCHANTMENT_IDS.get_or_init(|| {
        let mut ids = HashMap::new();

        for enchantment in ALL_ENCHANTMENTS {
            let item = ItemStack::new("minecraft:stick", 1);
            item.add_enchantment(enchantment, 1);

            let Some(bytes) = enchantment_component_bytes(&item) else {
                continue;
            };
            let Some(entries) = parse_enchantment_list(&bytes) else {
                continue;
            };
            if let Some((id, _)) = entries.first() {
                ids.insert(*id, enchantment);
            }
        }

        if ids.is_empty() {
            tracing::warn!(
                "lazarus: could not probe the server's enchantment ids; \
                 enchanted books will not be clamped"
            );
        } else {
            tracing::debug!("lazarus: probed {} enchantment ids", ids.len());
        }

        ids
    })
}

/// Decodes the shared enchantment list codec:
/// `varint count` + `(varint id, varint level)` pairs.
fn parse_enchantment_list(bytes: &[u8]) -> Option<Vec<(u8, i32)>> {
    let mut cursor = 0usize;
    let count = read_varint(bytes, &mut cursor)?;
    // Same bound the server's codec enforces when reading the component.
    if count > 256 {
        return None;
    }

    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let id = u8::try_from(read_varint(bytes, &mut cursor)?).ok()?;
        let level = read_varint(bytes, &mut cursor)?;
        entries.push((id, level));
    }

    Some(entries)
}

fn serialize_enchantment_list(entries: &[(u8, i32)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 2 + 1);
    write_varint(&mut out, entries.len() as i32);
    for (id, level) in entries {
        write_varint(&mut out, i32::from(*id));
        write_varint(&mut out, *level);
    }
    out
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Option<i32> {
    let mut value = 0u32;

    for shift in 0..5 {
        let byte = *bytes.get(*cursor)?;
        *cursor += 1;
        value |= u32::from(byte & 0x7F) << (shift * 7);

        if byte & 0x80 == 0 {
            return i32::try_from(value).ok();
        }
    }

    None
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if value & !0x7F == 0 {
            out.push(value as u8);
            return;
        }
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
}

// ---------------------------------------------------------------------------
// Potion limiter
// ---------------------------------------------------------------------------

/// `PotionLimiterHandler#onPlayerItemConsume` / `#onPotionSplash`.
///
/// The consume event fires before the use is applied, so both drinking and
/// throwing are denied here: the potion is cleared, the action aborted and the
/// player is told — the original's outcome, minus the short-lived splash
/// entity (which aborted actions never create).
pub struct PotionConsumeListener;

impl EventHandler<PlayerItemConsumeEvent> for PotionConsumeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerItemConsumeEvent>,
    ) -> EventData<PlayerItemConsumeEvent> {
        if event.cancelled {
            return event;
        }

        let name = event
            .item_name
            .strip_prefix("minecraft:")
            .unwrap_or(&event.item_name);
        if !matches!(name, "potion" | "splash_potion") {
            return event;
        }

        let Some((hand, item)) = held_potion(&event.player, name) else {
            return event;
        };
        if potion_allowed(&item) {
            return event;
        }

        event.cancelled = true;
        event.player.set_item_in_hand(hand, None);
        send_message(&event.player, "POTION_LIMITER_DENY_MESSAGE");
        event
    }
}

/// The potion the player is using: the hand whose registry key matches the
/// event's item name (the original read `event.getItem()`), falling back to any
/// potion in hand.
fn held_potion(player: &Player, expected: &str) -> Option<(Hand, ItemStack)> {
    let mut fallback = None;

    for hand in [Hand::Right, Hand::Left] {
        let Some(item) = player.get_item_in_hand(hand) else {
            continue;
        };

        if registry_matches(&item.get_registry_key(), expected) {
            return Some((hand, item));
        }
        if fallback.is_none() && potion_components::potion_name_for_item(&item).is_some() {
            fallback = Some((hand, item));
        }
    }

    fallback
}

/// Compares registry keys tolerantly (`potion` vs `minecraft:potion`).
fn registry_matches(key: &str, expected: &str) -> bool {
    key == expected
        || key.strip_prefix("minecraft:") == Some(expected)
        || expected.strip_prefix("minecraft:") == Some(key)
}

/// `PotionLimiterHandler#shouldNotCancelEffect(Potion)`. Effects without a
/// limit allow the whole potion (the original returned immediately there).
fn potion_allowed(item: &ItemStack) -> bool {
    let Some(name) = potion_components::potion_name_for_item(item) else {
        return true;
    };
    let Some((extended, effects)) = potion_definition(&name) else {
        return true;
    };

    for (effect, amplifier) in effects {
        let Some(effect) = modern_effect(effect) else {
            return true;
        };
        let Some((level, extended_allowed)) = potion_limit(effect) else {
            return true;
        };

        if level == 0 || i32::from(*amplifier) + 1 > level {
            return false;
        }
        if extended && !extended_allowed {
            return false;
        }
    }

    true
}

fn potion_limit(effect: StatusEffectType) -> Option<(i32, bool)> {
    with_limits(|limits| {
        limits
            .potions
            .iter()
            .find(|(kind, _, _)| *kind == effect)
            .map(|(_, level, extended)| (*level, *extended))
    })
}

/// `PotionLimiterHandler#onPotionBrew`'s deferred pass: restores the potions a
/// brew replaced when the result violates the limits.
pub struct BrewListener;

impl EventHandler<BrewEvent> for BrewListener {
    fn handle(&self, server: Server, event: EventData<BrewEvent>) -> EventData<BrewEvent> {
        if event.cancelled {
            return event;
        }

        // The event has no contents; try every world that holds a stand at the
        // position (the original read `event.getContents()` directly).
        let (x, y, z) = (event.block_pos.x, event.block_pos.y, event.block_pos.z);
        let mut captures = Vec::new();
        for world in server.get_all_worlds() {
            let pos = BlockPos { x, y, z };
            let Some(BlockEntityType::BrewingStandBlockEntity(stand)) =
                world.get_block_entity(pos)
            else {
                continue;
            };

            let container = stand.get_container();
            let previous = [
                container.get_stack(0),
                container.get_stack(1),
                container.get_stack(2),
            ];
            captures.push((world, previous));
        }

        if captures.is_empty() {
            return event;
        }

        // The event fires before the slots are replaced, so the previous
        // potions are restored one tick later (the original used `Tasks.sync`
        // and held the live inventory). The captured handles stay alive inside
        // the closure, so they remain valid at task time.
        let captures = Mutex::new(captures);
        server.schedule_delayed_task(1, move |_server| {
            let captures = std::mem::take(
                &mut *captures.lock().expect("brew capture mutex poisoned"),
            );
            for (world, previous) in captures {
                restore_brew(&world, BlockPos { x, y, z }, previous);
            }
        });

        event
    }
}

fn restore_brew(world: &World, pos: BlockPos, previous: [Option<ItemStack>; 3]) {
    let Some(BlockEntityType::BrewingStandBlockEntity(stand)) = world.get_block_entity(pos) else {
        return;
    };
    let container = stand.get_container();

    for (slot, restore) in previous.into_iter().enumerate() {
        // The original skipped water bottles (`getDurability() == 0`); water
        // carries no effects here either, so `potion_allowed` covers it.
        let Some(current) = container.get_stack(slot as u32) else {
            continue;
        };
        if potion_allowed(&current) {
            continue;
        }

        container.set_stack(slot as u32, restore);
    }
}

/// Vanilla potion → `(extended, effects)`. On 1.8 `Potion.fromItemStack`
/// derived the effects from the potion type only, so the same table is used
/// here (names/amplifiers mirror `pumpkin-data/generated/potion.rs`).
/// `None` for unknown (custom) potions, like `Potion.fromItemStack == null`.
fn potion_definition(name: &str) -> Option<(bool, &'static [(&'static str, u8)])> {
    Some(match name {
        "fire_resistance" => (false, &[("FIRE_RESISTANCE", 0)]),
        "harming" => (false, &[("INSTANT_DAMAGE", 0)]),
        "healing" => (false, &[("INSTANT_HEALTH", 0)]),
        "infested" => (false, &[("INFESTED", 0)]),
        "invisibility" => (false, &[("INVISIBILITY", 0)]),
        "leaping" => (false, &[("JUMP_BOOST", 0)]),
        "long_fire_resistance" => (true, &[("FIRE_RESISTANCE", 0)]),
        "long_invisibility" => (true, &[("INVISIBILITY", 0)]),
        "long_leaping" => (true, &[("JUMP_BOOST", 0)]),
        "long_night_vision" => (true, &[("NIGHT_VISION", 0)]),
        "long_poison" => (true, &[("POISON", 0)]),
        "long_regeneration" => (true, &[("REGENERATION", 0)]),
        "long_slow_falling" => (true, &[("SLOW_FALLING", 0)]),
        "long_slowness" => (true, &[("SLOWNESS", 0)]),
        "long_strength" => (true, &[("STRENGTH", 0)]),
        "long_swiftness" => (true, &[("SPEED", 0)]),
        "long_turtle_master" => (true, &[("SLOWNESS", 3), ("RESISTANCE", 2)]),
        "long_water_breathing" => (true, &[("WATER_BREATHING", 0)]),
        "long_weakness" => (true, &[("WEAKNESS", 0)]),
        "luck" => (false, &[("LUCK", 0)]),
        "night_vision" => (false, &[("NIGHT_VISION", 0)]),
        "oozing" => (false, &[("OOZING", 0)]),
        "poison" => (false, &[("POISON", 0)]),
        "regeneration" => (false, &[("REGENERATION", 0)]),
        "slow_falling" => (false, &[("SLOW_FALLING", 0)]),
        "slowness" => (false, &[("SLOWNESS", 0)]),
        "strength" => (false, &[("STRENGTH", 0)]),
        "strong_harming" => (false, &[("INSTANT_DAMAGE", 1)]),
        "strong_healing" => (false, &[("INSTANT_HEALTH", 1)]),
        "strong_leaping" => (false, &[("JUMP_BOOST", 1)]),
        "strong_poison" => (false, &[("POISON", 1)]),
        "strong_regeneration" => (false, &[("REGENERATION", 1)]),
        "strong_slowness" => (false, &[("SLOWNESS", 3)]),
        "strong_strength" => (false, &[("STRENGTH", 1)]),
        "strong_swiftness" => (false, &[("SPEED", 1)]),
        "strong_turtle_master" => (false, &[("SLOWNESS", 5), ("RESISTANCE", 3)]),
        "swiftness" => (false, &[("SPEED", 0)]),
        "turtle_master" => (false, &[("SLOWNESS", 3), ("RESISTANCE", 2)]),
        "water_breathing" => (false, &[("WATER_BREATHING", 0)]),
        "weakness" => (false, &[("WEAKNESS", 0)]),
        "weaving" => (false, &[("WEAVING", 0)]),
        "wind_charged" => (false, &[("WIND_CHARGED", 0)]),
        // No-effect potions (`Potion.fromItemStack` returned null for the
        // water bottle on 1.8; these are all allowed).
        "water" | "mundane" | "thick" | "awkward" => (false, &[]),
        _ => return None,
    })
}

/// Effect names of [`potion_definition`] → Pumpkin status effects.
fn modern_effect(name: &str) -> Option<StatusEffectType> {
    use StatusEffectType as E;

    Some(match name {
        "SPEED" => E::Speed,
        "SLOWNESS" => E::Slowness,
        "INSTANT_DAMAGE" => E::InstantDamage,
        "INSTANT_HEALTH" => E::InstantHealth,
        "JUMP_BOOST" => E::JumpBoost,
        "NIGHT_VISION" => E::NightVision,
        "WATER_BREATHING" => E::WaterBreathing,
        "FIRE_RESISTANCE" => E::FireResistance,
        "INVISIBILITY" => E::Invisibility,
        "POISON" => E::Poison,
        "REGENERATION" => E::Regeneration,
        "STRENGTH" => E::Strength,
        "WEAKNESS" => E::Weakness,
        "RESISTANCE" => E::Resistance,
        "LUCK" => E::Luck,
        "SLOW_FALLING" => E::SlowFalling,
        "WIND_CHARGED" => E::WindCharged,
        "WEAVING" => E::Weaving,
        "OOZING" => E::Oozing,
        "INFESTED" => E::Infested,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Entity limiter
// ---------------------------------------------------------------------------

/// `EntityLimiterHandler#onEntitySpawn` + `#onVehicleCreate`: removes the
/// entities listed as disabled in `limiters.yml` from every world. Called by
/// the one-second tick.
///
/// The host sends the numeric registry id through `EntitySpawnEvent`; the
/// world entity list carries the typed [`EntityType`] instead, resolved
/// through [`core::entity_type_key`]. Players are never touched (the original
/// could list `player` harmlessly; the sweep must not). `CreatureSpawnEvent`
/// is never fired by the host, so the spawn-reason/chunk-limit checks of the
/// original are not reachable (see the module docs).
pub fn enforce_disabled_entities(server: &Server) {
    let disabled = with_limits(|limits| limits.entities.clone());
    if disabled.is_empty() {
        return;
    }

    for world in server.get_all_worlds() {
        for entity in world.get_entities() {
            let entity_type = entity.get_type();
            if entity_type == EntityType::Player {
                continue;
            }
            let name = core::entity_type_key(&entity_type);
            if entity_disabled(&disabled, &name) {
                entity.remove();
            }
        }
    }
}

fn entity_disabled(disabled: &[String], name: &str) -> bool {
    disabled.iter().any(|candidate| {
        // The 1.8 `EntityType.BOAT` had no per-wood subtypes.
        name == candidate.as_str() || (candidate == "boat" && name.ends_with("_boat"))
    })
}

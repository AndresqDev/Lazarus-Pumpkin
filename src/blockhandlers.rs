//! Block gameplay handlers — port of `me.qiooip.lazarus.handlers.block`.
//!
//! Covered here: auto smelt (`AutoSmeltHandler`), the cobweb remover
//! (`CobwebRemoveHandler`), disabled block placement (`DisabledBlocksHandler`)
//! and the found-ore announcements (`FoundOreHandler`). Subclaims
//! (`SubclaimHandler`) and the crowbar (`CrowbarHandler`) belong to the same
//! original package and follow in this module.

use std::collections::BTreeSet;

use pumpkin_plugin_api::common::{BlockPos, GameMode, Hand};
use pumpkin_plugin_api::enchantments_wit::Enchantment;
use pumpkin_plugin_api::events::{
    BlockBreakEvent, BlockDropItemEvent, BlockPlaceEvent, EventData, EventHandler,
};
use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::world::BlockFlags;
use pumpkin_plugin_api::{ItemStack, Player, Server};

use crate::core;
use crate::state::state;
use crate::util;

/// `(world, x, y, z)` position key for the transient block maps.
pub type BlockKey = (String, i32, i32, i32);

fn key_of(world: &str, pos: &BlockPos) -> BlockKey {
    (world.to_string(), pos.x, pos.y, pos.z)
}

/// Autosmel replacements stay valid for a few seconds: the host fires the
/// block-drop event synchronously right after the break event, so anything
/// left over never matches a real drop.
const AUTOSMEL_TTL_MILLIS: i64 = 5_000;

/// Air writes from the handler tasks: update neighbours and clients, no drops
/// (Bukkit's `Block#setType(AIR)` had the same effect).
fn air_flags() -> BlockFlags {
    BlockFlags::NOTIFY_NEIGHBORS | BlockFlags::NOTIFY_LISTENERS | BlockFlags::SKIP_DROPS
}

// ---------------------------------------------------------------------------
// Auto smelt (`AutoSmeltHandler`).
// ---------------------------------------------------------------------------

/// The original only knew `IRON_ORE`/`GOLD_ORE`; the modern split adds deepslate
/// variants, which map to the same ingots.
fn autosmel_ingot(block: &str) -> Option<&'static str> {
    Some(match block {
        "minecraft:iron_ore" | "minecraft:deepslate_iron_ore" => "minecraft:iron_ingot",
        "minecraft:gold_ore" | "minecraft:deepslate_gold_ore" => "minecraft:gold_ingot",
        _ => return None,
    })
}

fn has_silk_touch(tool: &ItemStack) -> bool {
    tool.get_enchantments()
        .into_iter()
        .any(|value| value.enchantment == Enchantment::SilkTouch && value.level > 0)
}

/// True when the held tool can produce the vanilla ore drop (stone pickaxe or
/// better). The original rewarded the break with an ingot through any
/// non-silk-touch item, so when the host would not drop anything at all the
/// ingot is handed to the inventory instead of the drop replacement.
fn tool_harvests_ores(tool: &ItemStack) -> bool {
    matches!(
        tool.get_registry_key().as_str(),
        "minecraft:stone_pickaxe"
            | "minecraft:iron_pickaxe"
            | "minecraft:golden_pickaxe"
            | "minecraft:diamond_pickaxe"
            | "minecraft:netherite_pickaxe"
    )
}

/// Vanilla `Inventory#addItem` order for a single item: merge into an existing
/// stack first, then the first empty main-inventory slot (0..=35). Returns
/// false when there is no room (the caller keeps the vanilla drop instead).
fn give_to_inventory(player: &Player, key: &str) -> bool {
    for slot in 0..=35u8 {
        if let Some(item) = player.get_inventory_item(slot)
            && item.get_registry_key() == key
            && item.get_count() < item.get_max_count()
        {
            item.set_count(item.get_count() + 1);
            player.set_inventory_item(slot, Some(item));
            return true;
        }
    }

    for slot in 0..=35u8 {
        let empty = player
            .get_inventory_item(slot)
            .map_or(true, |item| item.get_count() == 0);
        if empty {
            player.set_inventory_item(slot, Some(ItemStack::new(key, 1)));
            return true;
        }
    }

    false
}

/// Port of `AutoSmeltHandler#onBlockBreak`.
///
/// The break event is the only place that knows the player and their tool; the
/// host fires its own drop event without a player, so the replacement (and the
/// occasional direct hand-out) is decided here.
pub struct AutoSmeltBreakListener;

impl EventHandler<BlockBreakEvent> for AutoSmeltBreakListener {
    fn handle(&self, _server: Server, mut event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        let Some(player) = event.player.as_ref() else {
            return event;
        };

        if player.get_gamemode() == GameMode::Creative
            || !player.has_permission("lazarus.autosmelt")
        {
            return event;
        }

        let Some(ingot) = autosmel_ingot(&event.block) else {
            return event;
        };

        let Some(tool) = player.get_item_in_hand(Hand::Right) else {
            return event;
        };

        if has_silk_touch(&tool) {
            return event;
        }

        // `player.giveExp(ThreadLocalRandom.nextInt(3, 6))`.
        player.add_experience_points(3 + (util::now_millis().unsigned_abs() % 3) as i32);

        let world = core::world_name(player);
        let key = key_of(&world, &event.block_pos);

        // SOTW routes drops into the inventory, and a tool that cannot harvest
        // the ore never triggers the host's drop event: both hand the ingot
        // over directly when there is room.
        if crate::sotw::is_active() || !tool_harvests_ores(&tool) {
            if give_to_inventory(player, ingot) {
                event.should_drop = false;
                return event;
            }
        }

        // Otherwise the vanilla ore drop is replaced by the ingot in the drop
        // event, keeping the original's floor drop at the broken block.
        state().write().unwrap().autosmel_pending.insert(
            key,
            (ingot.to_string(), util::now_millis() + AUTOSMEL_TTL_MILLIS),
        );

        event
    }
}

/// Port of the drop half of `AutoSmeltHandler`: swaps the raw ore for the
/// ingot this break queued. The host drops whatever `items` holds.
pub struct AutoSmeltDropListener;

impl EventHandler<BlockDropItemEvent> for AutoSmeltDropListener {
    fn handle(&self, _server: Server, mut event: EventData<BlockDropItemEvent>) -> EventData<BlockDropItemEvent> {
        let key = key_of(&event.target_world.get_name(), &event.block_pos);
        let pending = state().write().unwrap().autosmel_pending.remove(&key);

        if let Some((ingot, expires)) = pending
            && expires >= util::now_millis()
        {
            event.items = vec![ItemStack::new(&ingot, 1)];
        }

        event
    }
}

// ---------------------------------------------------------------------------
// Cobweb remover (`CobwebRemoveHandler`).
// ---------------------------------------------------------------------------

/// Port of `CobwebRemoveHandler#onBlockPlace`: remembers placed cobwebs.
pub struct CobwebPlaceListener;

impl EventHandler<BlockPlaceEvent> for CobwebPlaceListener {
    fn handle(&self, _server: Server, event: EventData<BlockPlaceEvent>) -> EventData<BlockPlaceEvent> {
        if event.cancelled || event.block_placed != "minecraft:cobweb" {
            return event;
        }

        let (enabled, seconds) = {
            let guard = state().read().unwrap();
            (
                guard.raw.config.bool("COBWEB_REMOVER.ENABLED"),
                guard.raw.config.int("COBWEB_REMOVER.REMOVE_AFTER"),
            )
        };

        if !enabled {
            return event;
        }

        let world = core::world_name(&event.player);
        state().write().unwrap().cobwebs.insert(
            key_of(&world, &event.block_pos),
            util::now_millis() + seconds.max(0) * 1000,
        );

        event
    }
}

/// Port of `CobwebRemoveHandler#onBlockBreak`: a cobweb broken early leaves the
/// task queue.
pub struct CobwebBreakListener;

impl EventHandler<BlockBreakEvent> for CobwebBreakListener {
    fn handle(&self, _server: Server, event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        if event.cancelled || event.block != "minecraft:cobweb" {
            return event;
        }

        if let Some(player) = event.player.as_ref() {
            let world = core::world_name(player);
            state().write().unwrap().cobwebs.remove(&key_of(&world, &event.block_pos));
        }

        event
    }
}

/// One-second pass of the block handlers: the cobweb removal task plus the
/// pruning of expired autosmel replacements.
pub fn tick_second(server: &Server) {
    process_cobwebs(server);

    let now = util::now_millis();
    state()
        .write()
        .unwrap()
        .autosmel_pending
        .retain(|_, (_, expires)| *expires >= now);
}

/// Port of `CobwebRemoveHandler.CobwebRemovalTask#run`: remove up to
/// [`COBWEB_REMOVAL_LIMIT`] cobwebs whose timer expired.
const COBWEB_REMOVAL_LIMIT: usize = 50;

fn process_cobwebs(server: &Server) {
    let now = util::now_millis();

    let due: Vec<BlockKey> = {
        let mut guard = state().write().unwrap();
        let due: Vec<BlockKey> = guard
            .cobwebs
            .iter()
            .filter(|(_, expires)| **expires <= now)
            .map(|(key, _)| key.clone())
            .take(COBWEB_REMOVAL_LIMIT)
            .collect();

        for key in &due {
            guard.cobwebs.remove(key);
        }

        due
    };

    for key in due {
        set_air(server, &key);
    }
}

/// `CobwebRemoveHandler#disable`: puts every tracked cobweb back to air.
pub fn clear_cobwebs(server: &Server) {
    let tracked: Vec<BlockKey> = {
        let mut guard = state().write().unwrap();
        std::mem::take(&mut guard.cobwebs).into_keys().collect()
    };

    for key in tracked {
        set_air(server, &key);
    }
}

fn set_air(server: &Server, key: &BlockKey) {
    if let Some(world) = server.get_world_by_name(&key.0) {
        let _ = world.set_block_by_name(
            BlockPos { x: key.1, y: key.2, z: key.3 },
            "minecraft:air",
            air_flags(),
        );
    }
}

// ---------------------------------------------------------------------------
// Disabled block placement (`DisabledBlocksHandler`).
// ---------------------------------------------------------------------------

/// Port of `DisabledBlocksHandler#onBlockPlace` (`LOWEST` priority: it runs
/// before the territory listeners so their `ignoreCancelled` semantics hold).
pub struct DisabledPlacementListener;

impl EventHandler<BlockPlaceEvent> for DisabledPlacementListener {
    fn handle(&self, _server: Server, mut event: EventData<BlockPlaceEvent>) -> EventData<BlockPlaceEvent> {
        if event.cancelled {
            return event;
        }

        // `player.isOp()` — the op permission level.
        if event.player.get_permission_level() == PermissionLevel::Four {
            return event;
        }

        let entry = {
            let guard = state().read().unwrap();
            guard
                .raw
                .config
                .string_list("DISABLED_BLOCK_PLACEMENT")
                .into_iter()
                .find(|entry| {
                    resolve_block_key(entry).as_deref() == Some(event.block_placed.as_str())
                })
        };

        let Some(entry) = entry else {
            return event;
        };

        event.cancelled = true;

        let (prefix, message) = {
            let guard = state().read().unwrap();
            (
                guard.raw.language.string("PREFIX"),
                guard.raw.language.string("BLOCKS.PLACEMENT_DISABLED"),
            )
        };

        let name = items_name(&entry);
        core::send(
            &event.player,
            &format!("{prefix}{}", util::replace(&message, "<block>", &name)),
        );

        event
    }
}

/// Legacy 1.8 `id:meta` (still found in the original configs, e.g. `"137:0"`)
/// to a modern registry key. Only the block ids that plausibly appear in block
/// lists are mapped; registry keys pass through unchanged.
fn resolve_block_key(entry: &str) -> Option<String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return None;
    }

    if let Some((id, _)) = entry.split_once(':')
        && let Ok(id) = id.parse::<u32>()
    {
        return legacy_block_key(id).map(str::to_string);
    }

    if entry.contains(':') {
        Some(entry.to_string())
    } else {
        Some(format!("minecraft:{entry}"))
    }
}

/// Minimal 1.8 block-id table for `DISABLED_BLOCK_PLACEMENT`-style configs.
fn legacy_block_key(id: u32) -> Option<&'static str> {
    Some(match id {
        7 => "minecraft:bedrock",
        23 => "minecraft:dispenser",
        46 => "minecraft:tnt",
        49 => "minecraft:obsidian",
        51 => "minecraft:fire",
        52 => "minecraft:spawner",
        116 => "minecraft:enchanting_table",
        130 => "minecraft:ender_chest",
        137 => "minecraft:command_block",
        145 => "minecraft:anvil",
        154 => "minecraft:hopper",
        166 => "minecraft:barrier",
        _ => return None,
    })
}

/// `ItemUtils#getMaterialName`: the `items.yml` key for a config entry
/// (e.g. `"137:0"` -> `"CommandBlock"`).
fn items_name(entry: &str) -> String {
    let guard = state().read().unwrap();

    for key in guard.raw.items.root_keys() {
        if guard.raw.items.string(&key) == entry {
            return key;
        }
    }

    entry.to_string()
}

// ---------------------------------------------------------------------------
// Found ore (`FoundOreHandler`).
// ---------------------------------------------------------------------------

fn is_diamond_ore(block: &str) -> bool {
    block == "minecraft:diamond_ore" || block == "minecraft:deepslate_diamond_ore"
}

/// Port of `FoundOreHandler#onBlockPlace`: placed diamonds never announce.
pub struct FoundOrePlaceListener;

impl EventHandler<BlockPlaceEvent> for FoundOrePlaceListener {
    fn handle(&self, _server: Server, event: EventData<BlockPlaceEvent>) -> EventData<BlockPlaceEvent> {
        if event.cancelled || !is_diamond_ore(&event.block_placed) {
            return event;
        }

        if !state().read().unwrap().raw.config.bool("FOUND_ORE.ENABLED") {
            return event;
        }

        let world = core::world_name(&event.player);
        state()
            .write()
            .unwrap()
            .found_ore_marks
            .insert(key_of(&world, &event.block_pos));

        event
    }
}

/// Port of `FoundOreHandler#onBlockBreak`: announces a newly mined diamond vein
/// to every eligible player and marks it so it only ever announces once.
pub struct FoundOreBreakListener;

impl EventHandler<BlockBreakEvent> for FoundOreBreakListener {
    fn handle(&self, server: Server, event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        {
            let guard = state().read().unwrap();
            if !guard.raw.config.bool("FOUND_ORE.ENABLED") || !guard.chat_control_show_found_ore {
                return event;
            }
        }

        let Some(player) = event.player.as_ref() else {
            return event;
        };

        if player.get_gamemode() == GameMode::Creative || !is_diamond_ore(&event.block) {
            return event;
        }

        let world = core::world_name(player);
        let key = key_of(&world, &event.block_pos);

        if state().read().unwrap().found_ore_marks.contains(&key) {
            return event;
        }

        let count = {
            let mut guard = state().write().unwrap();
            let mut marks = std::mem::take(&mut guard.found_ore_marks);
            let count = count_diamonds(&server, &world, &event.block_pos, &mut marks);
            guard.found_ore_marks = marks;
            count
        };

        // `Bukkit.getOnlinePlayers().forEach(...)`.
        let breaker = core::uuid_of(player);
        let name = core::name_of(player);
        let template = state().read().unwrap().raw.language.string("FOUND_ORE.MESSAGE");

        for online in server.get_all_players() {
            if !online.has_permission("lazarus.foundore.receive") {
                continue;
            }

            if core::uuid_of(&online) != breaker {
                let show = state()
                    .read()
                    .unwrap()
                    .userdata
                    .get(&core::uuid_of(&online))
                    .map(|user| user.settings.found_ore)
                    .unwrap_or(true);

                if !show {
                    continue;
                }
            }

            core::send(
                &online,
                &util::replace(
                    &util::replace(&template, "<player>", &name),
                    "<amount>",
                    &count.to_string(),
                ),
            );
        }

        event
    }
}

/// `FoundOreHandler#countDiamonds`: flood-fills the vein around the broken ore,
/// marking every counted block so a vein only announces once. Capped at 50 like
/// the original.
fn count_diamonds(
    server: &Server,
    world_name: &str,
    from: &BlockPos,
    marks: &mut BTreeSet<BlockKey>,
) -> u32 {
    let Some(world) = server.get_world_by_name(world_name) else {
        return 0;
    };

    let mut count = 0u32;
    let mut stack: Vec<BlockPos> = vec![*from];

    while let Some(current) = stack.pop() {
        for dx in -1..=1i32 {
            for dy in -1..=1i32 {
                for dz in -1..=1i32 {
                    if count >= 50 {
                        return count;
                    }

                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }

                    let next = BlockPos {
                        x: current.x + dx,
                        y: current.y + dy,
                        z: current.z + dz,
                    };
                    let key = key_of(world_name, &next);

                    if marks.contains(&key) || !is_diamond_ore(&world.get_block_state(next).block_name) {
                        continue;
                    }

                    count += 1;
                    marks.insert(key);
                    stack.push(next);
                }
            }
        }
    }

    count
}

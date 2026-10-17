//! Block gameplay handlers — port of `me.qiooip.lazarus.handlers.block`.
//!
//! Covered here: auto smelt (`AutoSmeltHandler`), the cobweb remover
//! (`CobwebRemoveHandler`), disabled block placement (`DisabledBlocksHandler`),
//! the found-ore announcements (`FoundOreHandler`) and subclaims
//! (`SubclaimHandler`). The crowbar (`CrowbarHandler`) belongs to the same
//! original package and follows in this module.

use std::collections::BTreeSet;

use pumpkin_plugin_api::block_entity::BlockEntityType;
use pumpkin_plugin_api::common::{BlockPos, GameMode, Hand};
use pumpkin_plugin_api::enchantments_wit::Enchantment;
use pumpkin_plugin_api::events::{
    BlockBreakEvent, BlockDropItemEvent, BlockPlaceEvent, EventData, EventHandler,
    HopperInventorySearchEvent, InteractAction, PlayerInteractEvent, SignChangeEvent,
};
use pumpkin_plugin_api::permission::PermissionLevel;
use pumpkin_plugin_api::world::BlockFlags;
use pumpkin_plugin_api::{ItemStack, Player, Server, World};

use crate::core::{self, Territory};
use crate::factions::types::Role;
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

// ---------------------------------------------------------------------------
// Subclaims (`SubclaimHandler`).
// ---------------------------------------------------------------------------
//
// Deviation from the original: the host ignores `SignChangeEvent#line` edits
// and has no way to broadcast a server-side sign rewrite back to clients
// (`set_front_text` changes the block entity but sends no update packet, and
// the edit packet's lines are applied verbatim after the event). The port
// therefore keeps the text the player typed: the title/role checks accept the
// typed `[subclaim]`/`[s]` titles and the plain `leader`/`coleader`/`captain`/
// `officer` keywords next to the configured values, and an empty second line is
// emulated with a creator registry instead of being filled with the name.

/// Subclaim signs carry a role keyword (or player names) on their second line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubclaimRole {
    Leader,
    CoLeader,
    Captain,
    Named,
}

fn is_subclaimable(name: &str) -> bool {
    matches!(name, "minecraft:chest" | "minecraft:trapped_chest")
}

fn is_standing_sign(name: &str) -> bool {
    name.starts_with("minecraft:")
        && name.ends_with("_sign")
        && !name.ends_with("_wall_sign")
        && !name.contains("hanging")
}

fn is_wall_sign(name: &str) -> bool {
    name.starts_with("minecraft:") && name.ends_with("_wall_sign") && !name.contains("hanging")
}

/// Plain text of a JSON text component (the client sends sign lines as
/// components, e.g. `{"text":"[subclaim]"}`).
fn component_text(raw: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.to_string();
    };

    let mut out = String::new();
    collect_component_text(&value, &mut out);
    out
}

fn collect_component_text(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::String(text) => out.push_str(text),
        serde_json::Value::Array(parts) => {
            for part in parts {
                collect_component_text(part, out);
            }
        }
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(text)) = map.get("text") {
                out.push_str(text);
            }
            if let Some(extra) = map.get("extra") {
                collect_component_text(extra, out);
            }
        }
        _ => {}
    }
}

/// Strips legacy formatting codes and lowercases: the original compared the
/// `§`-translated stored lines with the translated config values.
fn normalized(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();

    while let Some(c) = chars.next() {
        if c == '§' || c == '&' {
            chars.next();
            continue;
        }
        out.extend(c.to_lowercase());
    }

    out
}

/// `ItemUtils#shortenName`: sign lines only fit 15 characters.
fn shorten_name(name: &str) -> &str {
    if name.chars().count() > 15 {
        let end = name.char_indices().nth(15).map(|(index, _)| index).unwrap_or(name.len());
        &name[..end]
    } else {
        name
    }
}

/// `SubclaimHandler#getSign`: the *first* sign attached to the chest (top, then
/// north/east/south/west). The caller checks the title, so a decoration sign
/// hides the side faces exactly like in the original.
fn attached_sign(world: &World, chest: BlockPos) -> Option<(BlockPos, Vec<String>)> {
    let up = BlockPos { x: chest.x, y: chest.y + 1, z: chest.z };
    if is_standing_sign(&world.get_block_state(up).block_name)
        && let Some(lines) = sign_lines(world, up)
    {
        return Some((up, lines));
    }

    // A wall sign's `facing` points at the block it is mounted on.
    for (dx, dz, facing) in [
        (0i32, -1i32, "north"),
        (1, 0, "east"),
        (0, 1, "south"),
        (-1, 0, "west"),
    ] {
        let pos = BlockPos { x: chest.x + dx, y: chest.y, z: chest.z + dz };
        if wall_sign_faces(world, pos, facing)
            && let Some(lines) = sign_lines(world, pos)
        {
            return Some((pos, lines));
        }
    }

    None
}

fn wall_sign_faces(world: &World, pos: BlockPos, facing: &str) -> bool {
    let state = world.get_block_state(pos);

    is_wall_sign(&state.block_name)
        && state
            .properties
            .iter()
            .any(|(key, value)| key == "facing" && value == facing)
}

/// Front text of the sign at `pos` (only signs carry one here).
fn sign_lines(world: &World, pos: BlockPos) -> Option<Vec<String>> {
    match world.get_block_entity(pos)? {
        BlockEntityType::SignBlockEntity(sign) => Some(
            sign.get_front_text()
                .messages
                .iter()
                .map(|message| component_text(message))
                .collect(),
        ),
        _ => None,
    }
}

fn is_subclaim_title(line: &str) -> bool {
    let value = normalized(line);
    if value == "[subclaim]" || value == "[s]" {
        return true;
    }

    let title = state().read().unwrap().raw.config.string("SUBCLAIMS.SIGN_TITLE");
    value == normalized(&title)
}

/// `SubclaimHandler#getSubclaim`: the lines of the first attached sign when it
/// carries the title. Double chests also look at the neighbouring half.
fn get_subclaim(world: &World, chest_pos: BlockPos) -> Option<(BlockPos, Vec<String>)> {
    if !is_subclaimable(&world.get_block_state(chest_pos).block_name) {
        return None;
    }

    let mut halves = vec![chest_pos];
    let name = world.get_block_state(chest_pos).block_name;
    for (dx, dz) in [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)] {
        let pos = BlockPos { x: chest_pos.x + dx, y: chest_pos.y, z: chest_pos.z + dz };
        if world.get_block_state(pos).block_name == name {
            halves.push(pos);
            break;
        }
    }

    for half in halves {
        let Some((sign_pos, lines)) = attached_sign(world, half) else {
            continue;
        };

        if lines.first().is_some_and(|line| is_subclaim_title(line)) {
            return Some((sign_pos, lines));
        }
    }

    None
}

/// `SubclaimHandler#getBlockAttachedTo`: the block a sign hangs on (below for
/// standing signs, `facing` for wall signs).
fn attached_block(world: &World, sign_pos: BlockPos) -> BlockPos {
    let state = world.get_block_state(sign_pos);

    if is_standing_sign(&state.block_name) {
        return BlockPos { x: sign_pos.x, y: sign_pos.y - 1, z: sign_pos.z };
    }

    if is_wall_sign(&state.block_name)
        && let Some(facing) = state
            .properties
            .iter()
            .find(|(key, _)| key == "facing")
            .map(|(_, value)| value.as_str())
    {
        let (dx, dz) = match facing {
            "north" => (0, -1),
            "south" => (0, 1),
            "east" => (1, 0),
            "west" => (-1, 0),
            _ => (0, 0),
        };

        return BlockPos { x: sign_pos.x + dx, y: sign_pos.y, z: sign_pos.z + dz };
    }

    // The original's fallback for non-`Attachable` sign data.
    BlockPos { x: sign_pos.x, y: sign_pos.y - 1, z: sign_pos.z }
}

fn subclaim_config() -> (String, String, String) {
    let guard = state().read().unwrap();
    (
        guard.raw.config.string("SUBCLAIMS.LEADER_ONLY"),
        guard.raw.config.string("SUBCLAIMS.CO_LEADERS_ONLY"),
        guard.raw.config.string("SUBCLAIMS.CAPTAINS_ONLY"),
    )
}

/// Which restriction the sign asks for (second line).
fn subclaim_role(line1: &str, config: &(String, String, String)) -> SubclaimRole {
    let value = normalized(line1);

    if value == normalized(&config.0) {
        SubclaimRole::Leader
    } else if value == normalized(&config.1) || value == "coleader" {
        SubclaimRole::CoLeader
    } else if value == normalized(&config.2) || value == "captain" || value == "officer" {
        SubclaimRole::Captain
    } else {
        SubclaimRole::Named
    }
}

/// The role/name check (`containsName` + the per-role comparisons). `creator`
/// emulates the original's auto-filled second line (an empty line stores the
/// creator through the sign registry).
fn subclaim_allows(
    lines: &[String],
    viewer_role: Role,
    viewer_name: &str,
    creator: Option<&str>,
) -> bool {
    let line1 = lines.get(1).map(String::as_str).unwrap_or("");
    let config = subclaim_config();

    match subclaim_role(line1, &config) {
        SubclaimRole::Leader => viewer_role == Role::Leader,
        SubclaimRole::CoLeader => viewer_role.is_at_least(Role::CoLeader),
        SubclaimRole::Captain => viewer_role.is_at_least(Role::Captain),
        SubclaimRole::Named => {
            let short = normalized(shorten_name(viewer_name));

            for index in 1..=3 {
                if let Some(line) = lines.get(index)
                    && !line.is_empty()
                    && normalized(line) == short
                {
                    return true;
                }
            }

            if line1.is_empty()
                && let Some(creator) = creator
                && normalized(shorten_name(creator)) == short
            {
                return true;
            }

            viewer_role.is_at_least(Role::Captain)
        }
    }
}

/// Access check shared by the open and break handlers: `true` when the viewer
/// is denied. Factionless players, foreign or raidable claims and an active
/// EOTW event leave the chest unprotected, like the original.
fn subclaim_denied(
    world_name: &str,
    pos: BlockPos,
    viewer: &Player,
    lines: &[String],
    sign_pos: BlockPos,
) -> bool {
    let uuid = core::uuid_of(viewer);

    let (role, creator) = {
        let guard = state().read().unwrap();

        if guard.eotw_active {
            return false;
        }

        let Some(player_faction) = guard.factions.by_member(&uuid) else {
            return false;
        };
        let faction_id = player_faction.id.clone();
        let role = player_faction.members.get(&uuid).map(|member| member.role).unwrap_or_default();

        match core::territory_at_guard(&guard, world_name, pos.x, pos.z) {
            Territory::Faction(owner_id) if owner_id == faction_id => {
                let raidable = guard
                    .factions
                    .get(&owner_id)
                    .map(|faction| faction.is_raidable())
                    .unwrap_or(false);

                if raidable {
                    return false;
                }
            }
            _ => return false,
        }

        let creator = guard
            .subclaim_creators
            .get(&(world_name.to_string(), sign_pos.x, sign_pos.y, sign_pos.z))
            .cloned();

        (role, creator)
    };

    !subclaim_allows(lines, role, &core::name_of(viewer), creator.as_deref())
}

fn send_subclaim_message(player: &Player, key: &str) {
    let guard = state().read().unwrap();
    let prefix = guard.raw.language.string("PREFIX");
    let message = guard.raw.language.string(key);
    drop(guard);

    core::send(player, &format!("{prefix}{message}"));
}

/// Port of `SubclaimHandler#onSignChange`. The success paths leave the event
/// untouched (the host applies the typed lines; see the deviation note above).
pub struct SubclaimSignChangeListener;

impl EventHandler<SignChangeEvent> for SubclaimSignChangeListener {
    fn handle(&self, server: Server, mut event: EventData<SignChangeEvent>) -> EventData<SignChangeEvent> {
        if event.cancelled {
            return event;
        }

        let player = &event.player;
        let world = player.get_world();
        let sign_pos = event.block_pos;

        let chest_pos = attached_block(&world, sign_pos);
        if !is_subclaimable(&world.get_block_state(chest_pos).block_name) {
            return event;
        }

        let lines: Vec<String> = event.lines.iter().map(|line| component_text(line)).collect();
        let first = normalized(lines.first().map(String::as_str).unwrap_or(""));
        if first != "[subclaim]" && first != "[s]" {
            return event;
        }

        let uuid = core::uuid_of(player);
        let world_name = world.get_name();

        let (faction, role) = {
            let guard = state().read().unwrap();

            let Some(player_faction) = guard.factions.by_member(&uuid) else {
                drop(guard);
                send_subclaim_message(player, "FACTIONS.NOT_IN_FACTION_SELF");
                return event;
            };

            let role = player_faction
                .members
                .get(&uuid)
                .map(|member| member.role)
                .unwrap_or_default();

            let own_claim = matches!(
                core::territory_at_guard(&guard, &world_name, chest_pos.x, chest_pos.z),
                Territory::Faction(owner_id) if owner_id == player_faction.id
            );

            if !own_claim {
                drop(guard);
                event.cancelled = true;
                send_subclaim_message(player, "SUBCLAIMS.NOT_IN_OWN_CLAIM");
                return event;
            }

            (player_faction.clone(), role)
        };

        if get_subclaim(&world, chest_pos).is_some() {
            event.cancelled = true;
            send_subclaim_message(player, "SUBCLAIMS.ALREADY_EXISTS");
            return event;
        }

        let second = lines.get(1).cloned().unwrap_or_default();

        match second.to_lowercase().as_str() {
            "leader" if role != Role::Leader => {
                event.cancelled = true;
                send_subclaim_message(player, "SUBCLAIMS.MUST_BE_LEADER");
                return event;
            }
            "coleader" if !role.is_at_least(Role::CoLeader) => {
                event.cancelled = true;
                send_subclaim_message(player, "SUBCLAIMS.MUST_BE_CO_LEADER");
                return event;
            }
            "officer" | "captain" if !role.is_at_least(Role::Captain) => {
                event.cancelled = true;
                send_subclaim_message(player, "SUBCLAIMS.MUST_BE_CAPTAIN");
                return event;
            }
            "leader" | "coleader" | "officer" | "captain" => {}
            _ => {
                if !role.is_at_least(Role::Captain) {
                    return event;
                }

                // The original fills an empty second line with the creator's
                // name; the port remembers them instead (see the top note).
                if second.is_empty() {
                    state().write().unwrap().subclaim_creators.insert(
                        (world_name, sign_pos.x, sign_pos.y, sign_pos.z),
                        core::name_of(player),
                    );
                }
            }
        }

        let guard = state().read().unwrap();
        let prefix = guard.raw.language.string("PREFIX");
        let message = guard.raw.language.string("SUBCLAIMS.SUCCESSFULLY_CREATED");
        drop(guard);

        let loc = event.block_pos;
        let message = util::replace(
            &util::replace(
                &util::replace(
                    &util::replace(&message, "<player>", &core::name_of(player)),
                    "<x>",
                    &loc.x.to_string(),
                ),
                "<y>",
                &loc.y.to_string(),
            ),
            "<z>",
            &loc.z.to_string(),
        );
        core::faction_message(&server, &faction, &format!("{prefix}{message}"));

        event
    }
}

/// Port of `SubclaimHandler#onPlayerInteract` (`RIGHT_CLICK_BLOCK` chests).
pub struct SubclaimInteractListener;

impl EventHandler<PlayerInteractEvent> for SubclaimInteractListener {
    fn handle(&self, _server: Server, mut event: EventData<PlayerInteractEvent>) -> EventData<PlayerInteractEvent> {
        if event.cancelled || event.action != InteractAction::RightClickBlock {
            return event;
        }

        let Some(pos) = event.clicked_pos else {
            return event;
        };

        let player = &event.player;
        let world = player.get_world();

        let Some((sign_pos, lines)) = get_subclaim(&world, pos) else {
            return event;
        };

        if player.has_permission("lazarus.subclaim.bypass") {
            return event;
        }

        let uuid = core::uuid_of(player);
        if subclaim_denied(&world.get_name(), pos, player, &lines, sign_pos) {
            event.cancelled = true;

            if !crate::listeners::interact::is_duplicate(&format!("subclaim:{uuid}:{pos:?}")) {
                send_subclaim_message(player, "SUBCLAIMS.CAN_NOT_OPEN");
            }
        }

        event
    }
}

/// Port of `SubclaimHandler#onBlockBreak` + `#onSignBreakEvent`.
pub struct SubclaimBreakListener;

impl EventHandler<BlockBreakEvent> for SubclaimBreakListener {
    fn handle(&self, _server: Server, mut event: EventData<BlockBreakEvent>) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        let Some(player) = event.player.as_ref() else {
            return event;
        };

        if player.has_permission("lazarus.subclaim.bypass") {
            return event;
        }

        let world = player.get_world();
        let world_name = world.get_name();
        let pos = event.block_pos;

        if is_subclaimable(&event.block) {
            let Some((sign_pos, lines)) = get_subclaim(&world, pos) else {
                return event;
            };

            if subclaim_denied(&world_name, pos, player, &lines, sign_pos) {
                let uuid = core::uuid_of(player);
                event.cancelled = true;

                if !crate::listeners::interact::is_duplicate(&format!("subclaim-break:{uuid}:{pos:?}")) {
                    send_subclaim_message(player, "SUBCLAIMS.CAN_NOT_DESTROY_SUBCLAIM");
                }
            }

            return event;
        }

        if !is_standing_sign(&event.block) && !is_wall_sign(&event.block) {
            return event;
        }

        let Some(lines) = sign_lines(&world, pos) else {
            return event;
        };
        if !lines.first().is_some_and(|line| is_subclaim_title(line)) {
            return event;
        }

        if subclaim_denied(&world_name, pos, player, &lines, pos) {
            let uuid = core::uuid_of(player);
            event.cancelled = true;

            if !crate::listeners::interact::is_duplicate(&format!("subclaim-sign:{uuid}:{pos:?}")) {
                send_subclaim_message(player, "SUBCLAIMS.CAN_NOT_DESTROY_SIGN");
            }
            return event;
        }

        // The sign is gone: drop the emulated creator of an empty second line.
        state()
            .write()
            .unwrap()
            .subclaim_creators
            .remove(&(world_name, pos.x, pos.y, pos.z));

        event
    }
}

/// Hopper protection. The original cancelled `InventoryMoveItemEvent` when a
/// hopper pulled out of a subclaimed chest; the host fires that event only for
/// hopper *pushes* and signals pulls with `HopperInventorySearchEvent` instead
/// (the hopper searching the container above it), which is the equivalent hook.
/// The event carries no world, so every loaded world is probed.
pub struct SubclaimHopperListener;

impl EventHandler<HopperInventorySearchEvent> for SubclaimHopperListener {
    fn handle(&self, server: Server, mut event: EventData<HopperInventorySearchEvent>) -> EventData<HopperInventorySearchEvent> {
        if event.cancelled {
            return event;
        }

        let search_pos = event.search_pos;

        for world in server.get_all_worlds() {
            if get_subclaim(&world, search_pos).is_some() {
                event.cancelled = true;
                break;
            }
        }

        event
    }
}


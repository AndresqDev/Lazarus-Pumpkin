//! Inventory restore — port of `me.qiooip.lazarus.handlers.restore.*`.
//!
//! Fidelity and deviations (all documented in `PORTING.md`):
//! * The original kept the snapshots in memory and only wrote
//!   `<uuid>.json` files on disable. The port writes the file on every save so
//!   a crash cannot lose a death, and keeps the file as the persistence layer.
//! * The original matched clicks by the preview title (`endsWith("'s
//!   Inventory")`), which the WASM click event does not carry: the port keeps a
//!   per-viewer session holding the `Inventory` handle and the target.
//! * There is no API to close a window: `Close Preview` cannot close the
//!   preview (the player presses Escape) and a rollback clears the preview
//!   instead of closing it.
//! * Items are stored as registry key + count + data components
//!   (`utils::components` maps the WIT names), not as NMS NBT.

use std::collections::BTreeMap;

use pumpkin_plugin_api::events::{EventData, EventHandler, InventoryClickEvent, InventoryCloseEvent};
use pumpkin_plugin_api::gui::Gui;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Inventory, ItemStack, Player, Screen, Server};
use serde::{Deserialize, Serialize};

use crate::state::state;
use crate::utils::{self, components, core};

/// One saved stack: registry key + count + the item's data components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedStack {
    pub key: String,
    pub count: u8,
    #[serde(default)]
    pub components: Vec<SavedComponent>,
}

/// One data component of a saved stack (`utils::components` name table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedComponent {
    pub name: String,
    #[serde(default)]
    pub value: Vec<u8>,
}

/// `InventoryData`: the 36 main slots (index 0-35) and the armor
/// (boots, leggings, chestplate, helmet — `getArmorContents` order).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SavedInventory {
    #[serde(default)]
    pub contents: Vec<Option<SavedStack>>,
    #[serde(default)]
    pub armor: Vec<Option<SavedStack>>,
}

/// The in-memory store (`InventoryRestoreManager#inventories`).
#[derive(Default)]
pub struct RestoreStore {
    pub inventories: BTreeMap<String, SavedInventory>,
}

/// Per-viewer preview window. The original held no state: it matched clicks by
/// the inventory title and split the target name back out of it.
pub struct RestoreSession {
    pub screen: Screen,
    pub inventory: Inventory,
    pub target: String,
    pub target_name: String,
    /// See `SettingsSession::reopened`.
    pub reopened: bool,
}

/// `Language.PREFIX + <message>`.
fn prefixed(key: &str) -> String {
    let guard = state().read().unwrap();

    format!(
        "{}{}",
        guard.raw.language.string("PREFIX"),
        guard.raw.language.string(key)
    )
}

/// `InventoryRestoreManager#loadInventories`: reads `<dataFolder>/restore` and
/// deletes files older than `INVENTORY_RESTORE.FILE_CACHE` hours.
pub fn load_inventories(data_folder: &str) {
    let dir = format!("{data_folder}/restore");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };

    // `ChronoUnit.HOURS.between(creationDate, Instant.now()) > FILE_CACHE`:
    // the comparison truncates to whole hours, so the division mirrors it.
    let cache_hours = state()
        .read()
        .unwrap()
        .raw
        .config
        .int("INVENTORY_RESTORE.FILE_CACHE")
        .max(0);
    let now = utils::now_millis();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }

        // `FileUtils#creationDate`; WASI does not always expose the birth time,
        // so the modification time is the fallback (a save only makes it newer).
        let created = std::fs::metadata(&path)
            .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()))
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis() as i64);

        if created.is_some_and(|created| (now - created) / 3_600_000 > cache_hours) {
            let _ = std::fs::remove_file(&path);
            continue;
        }

        let Some(uuid) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<SavedInventory>(&content) else {
            continue;
        };

        state()
            .write()
            .unwrap()
            .restore
            .inventories
            .insert(uuid.to_string(), data);
    }
}

/// `InventoryRestoreManager#saveInventoryUponDeath`.
pub fn save_inventory_upon_death(player: &Player) {
    if !enabled() {
        return;
    }
    let contents: Vec<Option<ItemStack>> = (0..=35u8)
        .map(|slot| player.get_inventory_item(slot))
        .collect();
    let armor = {
        let inventory = player.get_inventory();
        vec![
            inventory.get_boots(),
            inventory.get_leggings(),
            inventory.get_chestplate(),
            inventory.get_helmet(),
        ]
    };

    save_captured(&core::uuid_of(player), &contents, &armor);
}

/// [`save_inventory_upon_death`] for the combat-logger death (`record.contents`
/// / `record.armor`, captured at logout in `getArmorContents` order).
pub fn save_captured(uuid: &str, contents: &[Option<ItemStack>], armor: &[Option<ItemStack>]) {
    if !enabled() {
        return;
    }

    let data = SavedInventory {
        contents: contents
            .iter()
            .map(|stack| stack.as_ref().map(capture))
            .collect(),
        armor: armor
            .iter()
            .map(|stack| stack.as_ref().map(capture))
            .collect(),
    };

    save(uuid, data);
}

fn enabled() -> bool {
    state()
        .read()
        .unwrap()
        .raw
        .config
        .bool("INVENTORY_RESTORE.ENABLED")
}

/// `inventories.put` plus the disk write the original deferred to disable:
/// every death is stored immediately (`<uuid>.json`), so a crash cannot lose
/// it. The read/purge logic is unchanged.
fn save(uuid: &str, data: SavedInventory) {
    let folder = state().read().unwrap().data_folder.clone();
    let dir = format!("{folder}/restore");
    let _ = std::fs::create_dir_all(&dir);

    if let Ok(json) = serde_json::to_string(&data) {
        let _ = std::fs::write(format!("{dir}/{uuid}.json"), json);
    }

    state()
        .write()
        .unwrap()
        .restore
        .inventories
        .insert(uuid.to_string(), data);
}

/// `ItemStack` -> `SavedStack`.
fn capture(stack: &ItemStack) -> SavedStack {
    let components = stack
        .get_components()
        .into_iter()
        .map(|value| SavedComponent {
            name: components::name(value.component).to_string(),
            value: value.value,
        })
        .collect();

    SavedStack {
        key: stack.get_registry_key(),
        count: stack.get_count(),
        components,
    }
}

/// `SavedStack` -> `ItemStack`. Unknown component names (only possible after a
/// WIT rename) are dropped instead of failing the restore.
fn rebuild(saved: &SavedStack) -> Option<ItemStack> {
    if saved.count == 0 {
        return None;
    }

    let item = ItemStack::new(&saved.key, saved.count);
    for component in &saved.components {
        match components::from_name(&component.name) {
            Some(kind) => item.set_component(kind, &component.value),
            None => tracing::warn!(
                "inventory restore: dropping unknown component {} of {}",
                component.name,
                saved.key
            ),
        }
    }

    Some(item)
}

fn plain(component: &TextComponent) -> String {
    crate::handlers::block::component_text(&component.to_json())
}

/// `new ItemBuilder(material, 1, data).setName(name).build()`.
fn placeholder(name: &str, key: &str) -> ItemStack {
    let item = ItemStack::new(key, 1);
    item.set_custom_name(Some(utils::text(name)));
    item
}

/// `InventoryRestoreManager#openRestoreInventory`.
pub fn open_restore_inventory(viewer: &Player, target: &Player) {
    let target_uuid = core::uuid_of(target);
    let target_name = core::name_of(target);

    let data = state()
        .read()
        .unwrap()
        .restore
        .inventories
        .get(&target_uuid)
        .cloned();
    let Some(data) = data else {
        let message = utils::replace(
            &prefixed("INVENTORY_RESTORE_COMMAND.NOTHING_TO_RESTORE"),
            "<target>",
            &target_name,
        );
        core::send(viewer, &message);
        return;
    };

    let gui = Gui::new(
        Screen::Generic9x6,
        utils::text(format!("{target_name}'s Inventory")),
    );
    gui.set_allow_grab_items(false);
    gui.set_allow_put_items(false);

    // `inventory.setContents(data.getContents())` then the armor at 45-48.
    for (slot, stack) in data.contents.iter().enumerate() {
        if let Some(stack) = stack.as_ref().and_then(rebuild) {
            gui.set_item(slot as u32, stack);
        }
    }
    for (index, stack) in data.armor.iter().enumerate() {
        if let Some(stack) = stack.as_ref().and_then(rebuild) {
            gui.set_item(45 + index as u32, stack);
        }
    }

    for slot in (36..=44).chain(49..=51) {
        gui.set_item(slot, placeholder("&cInventory Preview", "minecraft:gray_stained_glass_pane"));
    }
    gui.set_item(52, placeholder("&cClose Preview", "minecraft:red_terracotta"));
    gui.set_item(
        53,
        placeholder(
            &format!("&aRollback Inventory ({target_name})"),
            "minecraft:lime_terracotta",
        ),
    );

    let inventory = gui.get_inventory();

    {
        let mut guard = state().write().unwrap();
        let uuid = core::uuid_of(viewer);
        let reopened = guard.restore_sessions.contains_key(&uuid);
        guard.restore_sessions.insert(
            uuid,
            RestoreSession {
                screen: Screen::Generic9x6,
                inventory,
                target: target_uuid,
                target_name,
                // See `SettingsSession::reopened`.
                reopened,
            },
        );
    }

    viewer.open_gui(gui);
}

/// `InventoryRestoreManager#restorePlayerInventory`.
fn restore(server: &Server, viewer: &Player, target: &Player) {
    let target_uuid = core::uuid_of(target);

    let data = state()
        .write()
        .unwrap()
        .restore
        .inventories
        .remove(&target_uuid);
    let Some(data) = data else {
        return;
    };

    for (slot, stack) in data.contents.iter().enumerate() {
        target.set_inventory_item(slot as u8, stack.as_ref().and_then(rebuild));
    }

    let armor = |index: usize| data.armor.get(index).and_then(|stack| stack.as_ref()).and_then(rebuild);
    let inventory = target.get_inventory();
    inventory.set_boots(armor(0));
    inventory.set_leggings(armor(1));
    inventory.set_chestplate(armor(2));
    inventory.set_helmet(armor(3));

    let viewer_name = core::name_of(viewer);
    let target_name = core::name_of(target);

    // `Messages.sendMessage(message, "lazarus.staff")`: console plus every
    // staff player (same filter as the deathban revive broadcast).
    let message = utils::replace_all(
        &prefixed("INVENTORY_RESTORE_COMMAND.RESTORED_SENDER"),
        &[("<player>", &viewer_name), ("<target>", &target_name)],
    );
    let staff: Vec<_> = server
        .get_all_players()
        .into_iter()
        .filter(|player| {
            player.has_permission("lazarus.staff") || player.has_permission("lazarus:staff")
        })
        .collect();
    for player in staff {
        core::send(&player, &message);
    }

    let target_message = utils::replace(
        &prefixed("INVENTORY_RESTORE_COMMAND.RESTORED_PLAYER"),
        "<player>",
        &viewer_name,
    );
    core::send(target, &target_message);

    // `new File(Config.RESTORE_DIR, uuid + ".json").delete()`.
    let folder = state().read().unwrap().data_folder.clone();
    let _ = std::fs::remove_file(format!("{folder}/restore/{target_uuid}.json"));
}

/// `InventoryRestoreManager#onInventoryClick` (original default priority).
pub struct RestoreClickListener;

impl EventHandler<InventoryClickEvent> for RestoreClickListener {
    fn handle(&self, server: Server, mut event: EventData<InventoryClickEvent>) -> EventData<InventoryClickEvent> {
        let uuid = core::uuid_of(&event.player);

        {
            let guard = state().read().unwrap();
            let Some(session) = guard.restore_sessions.get(&uuid) else {
                return event;
            };
            if event.window_type != Some(session.screen) {
                return event;
            }
        }

        // `event.setCancelled(true)` happens after the `SlotType.OUTSIDE` check
        // (slot -1/-999) in the original, so outside clicks stay vanilla.
        if event.slot < 0 {
            return event;
        }
        event.cancelled = true;

        let Some(item) = event.clicked_item.as_ref() else {
            return event;
        };
        let Some(name) = item.get_custom_name() else {
            return event;
        };
        let name = plain(&name);

        if name.contains("Close Preview") {
            // The original scheduled `player.closeInventory()`; the WASM API
            // cannot close a window, so the cancelled click changes nothing and
            // the player closes the preview (documented deviation).
            return event;
        }

        if !name.contains("Rollback Inventory") {
            return event;
        }

        let (target_uuid, target_name) = {
            let guard = state().read().unwrap();
            let Some(session) = guard.restore_sessions.get(&uuid) else {
                return event;
            };
            (session.target.clone(), session.target_name.clone())
        };

        let target = pumpkin_plugin_api::uuid::parse(&target_uuid)
            .and_then(|id| server.get_player_by_uuid(id));

        if let Some(target) = target {
            restore(&server, &event.player, &target);
        } else {
            let message = utils::replace(
                &prefixed("COMMANDS.PLAYER_NOT_ONLINE"),
                "<player>",
                &target_name,
            );
            core::send(&event.player, &message);
        }

        // The original closed the window (`Tasks.sync(player::closeInventory)`);
        // the port clears the preview and drops the session instead.
        let inventory = {
            let mut guard = state().write().unwrap();
            guard.restore_sessions.remove(&uuid).map(|session| session.inventory)
        };
        if let Some(inventory) = inventory {
            inventory.clear();
        }

        event
    }
}

/// Port-only listener: drops the preview session when the window closes.
pub struct RestoreCloseListener;

impl EventHandler<InventoryCloseEvent> for RestoreCloseListener {
    fn handle(&self, _server: Server, event: EventData<InventoryCloseEvent>) -> EventData<InventoryCloseEvent> {
        let uuid = core::uuid_of(&event.player);
        let mut guard = state().write().unwrap();

        let matches = guard
            .restore_sessions
            .get(&uuid)
            .is_some_and(|session| event.window_type == Some(session.screen));
        if !matches {
            return event;
        }

        if guard.restore_sessions.get(&uuid).is_some_and(|session| session.reopened) {
            if let Some(session) = guard.restore_sessions.get_mut(&uuid) {
                session.reopened = false;
            }
        } else {
            guard.restore_sessions.remove(&uuid);
        }

        event
    }
}

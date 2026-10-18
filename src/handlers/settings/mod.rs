//! `/settings` GUI — port of `me.qiooip.lazarus.handlers.settings.SettingsHandler`.
//!
//! Fidelity notes:
//! * The original built one inventory per player on join and matched clicks by
//!   the inventory title (`getInventory().getName().equals(...)`), which the
//!   WASM click event does not carry. The port builds the inventory when the
//!   command runs, keeps its `Inventory` handle in a per-player session and
//!   matches the clicked item's display name — the same `getSetting(itemName)`
//!   lookup the original used.
//! * `player.chat(setting.getCommand())` becomes a direct call into the toggle
//!   shared with the `/toggle*` commands (which still sends the original
//!   message); a command the port does not know only skips the toggle.
//! * Clicks are cancelled exactly like the original (every click of the
//!   matching window, the player's own slots included).

use pumpkin_plugin_api::events::{EventData, EventHandler, InventoryClickEvent, InventoryCloseEvent};
use pumpkin_plugin_api::gui::Gui;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Inventory, ItemStack, Player, Screen, Server};

use crate::commands::base::{toggle, Toggle};
use crate::config::yaml::ConfigFile;
use crate::state::state;
use crate::utils::{self, core, legacy_items};

/// One `PLAYER_SETTINGS.SETTINGS.*` entry (`SettingsHandler.PlayerSetting`).
#[derive(Clone)]
pub struct SettingsEntry {
    pub name: String,
    /// `NAME` with formatting stripped — what click events expose.
    plain_name: String,
    pub key: String,
    pub slot: u32,
    pub setting: String,
    pub command: String,
    pub on_lore: Vec<String>,
    pub off_lore: Vec<String>,
}

/// Per-player open settings window (replaces the original's join-time cache).
pub struct SettingsSession {
    pub screen: Screen,
    pub inventory: Inventory,
    /// Set while `open_gui` replaces the previous window: the close event for
    /// the old window arrives after the new session is stored.
    pub reopened: bool,
}

/// `SettingsHandler#loadSettings` (run once at startup, like the original
/// constructor: a `/lazarus reload` does not rebuild the entries).
pub fn load_entries(config: &ConfigFile) -> Vec<SettingsEntry> {
    config
        .keys("PLAYER_SETTINGS.SETTINGS")
        .into_iter()
        .filter_map(|item| {
            let prefix = format!("PLAYER_SETTINGS.SETTINGS.{item}");
            // `ItemUtils.parseItem` failing skipped the entry in the original.
            let key = legacy_items::parse(&config.string(&format!("{prefix}.MATERIAL_ID")))?;

            let name = config.string(&format!("{prefix}.NAME"));
            Some(SettingsEntry {
                plain_name: plain(&utils::text(&name)),
                name,
                key,
                // `getInt(...SLOT) - 1`.
                slot: (config.int(&format!("{prefix}.SLOT")) - 1).max(0) as u32,
                setting: config.string(&format!("{prefix}.SETTING")),
                command: config.string(&format!("{prefix}.COMMAND")),
                on_lore: config.string_list(&format!("{prefix}.TOGGLED_ON_LORE")),
                off_lore: config.string_list(&format!("{prefix}.TOGGLED_OFF_LORE")),
            })
        })
        .collect()
}

fn plain(component: &TextComponent) -> String {
    crate::handlers::block::component_text(&component.to_json())
}

/// `SettingsHandler#getSettingStatus`.
pub fn status(settings: &crate::userdata::Settings, setting: &str) -> bool {
    Toggle::from_setting(setting).is_some_and(|toggle| toggle.status_of(settings))
}

/// `new ItemBuilder(item).setLore(getSettingLore().apply(player)).build()`.
fn build_item(entry: &SettingsEntry, on: bool) -> ItemStack {
    let item = ItemStack::new(&entry.key, 1);
    item.set_custom_name(Some(utils::text(&entry.name)));

    let lore = if on { &entry.on_lore } else { &entry.off_lore };
    item.set_lore(lore.iter().map(utils::text).collect());
    item
}

/// `PLAYER_SETTINGS.INVENTORY_SIZE` to a container screen. The original handed
/// the size to `Bukkit.createInventory`, which required a multiple of 9; the
/// port rounds up and clamps (a broken value must not take the plugin down).
/// `Generic9x1` is avoided on purpose: the host sizes it as 5 slots (hopper).
fn screen_for(size: usize) -> Screen {
    match size.div_ceil(9).clamp(1, 6) {
        1 | 2 => Screen::Generic9x2,
        3 => Screen::Generic9x3,
        4 => Screen::Generic9x4,
        5 => Screen::Generic9x5,
        _ => Screen::Generic9x6,
    }
}

/// `SettingsHandler#getSettingsInventory` + the `/settings` command's
/// `openInventory`.
pub fn open(player: &Player) {
    let uuid = core::uuid_of(player);

    let (name, size, entries, settings) = {
        let guard = state().read().unwrap();
        let name = guard.raw.config.string("PLAYER_SETTINGS.INVENTORY_NAME");
        let size = guard.raw.config.int("PLAYER_SETTINGS.INVENTORY_SIZE").clamp(9, 54) as usize;
        let settings = guard.userdata.get(&uuid).map(|user| user.settings.clone());
        (name, size, guard.settings.clone(), settings)
    };

    let screen = screen_for(size);
    let gui = Gui::new(screen, utils::text(&name));
    // The original cancelled every click; the flags keep the host from moving
    // items even if a click ever slipped through.
    gui.set_allow_grab_items(false);
    gui.set_allow_put_items(false);

    let slots = gui.get_size();
    for entry in &entries {
        if entry.slot >= slots {
            continue;
        }
        let on = settings
            .as_ref()
            .is_some_and(|settings| status(settings, &entry.setting));
        gui.set_item(entry.slot, build_item(entry, on));
    }

    let inventory = gui.get_inventory();

    {
        let mut guard = state().write().unwrap();
        // `open_gui` closes the previous window and fires its close event
        // synchronously; the flag tells the listener the close belongs to the
        // window being replaced, not to the session stored here.
        let reopened = guard.settings_sessions.contains_key(&uuid);
        guard
            .settings_sessions
            .insert(uuid, SettingsSession { screen, inventory, reopened });
    }

    player.open_gui(gui);
}

/// `SettingsHandler#onInventoryClick` (original priority `LOW`).
pub struct SettingsClickListener;

impl EventHandler<InventoryClickEvent> for SettingsClickListener {
    fn handle(&self, _server: Server, mut event: EventData<InventoryClickEvent>) -> EventData<InventoryClickEvent> {
        let uuid = core::uuid_of(&event.player);

        {
            let guard = state().read().unwrap();
            let Some(session) = guard.settings_sessions.get(&uuid) else {
                return event;
            };
            if event.window_type != Some(session.screen) {
                return event;
            }
        }

        // `event.setCancelled(true)`: the original cancelled every click of the
        // window, including the player's own slots (`getInventory()` is the top
        // inventory) — but returned *before* cancelling for
        // `SlotType.OUTSIDE` clicks (slot -1/-999), which must stay vanilla.
        if event.slot < 0 {
            return event;
        }
        event.cancelled = true;

        let Some(clicked) = event.clicked_item.as_ref() else {
            return event;
        };
        let Some(name) = clicked.get_custom_name() else {
            return event;
        };
        let plain_name = plain(&name);

        let entry = {
            let guard = state().read().unwrap();
            guard
                .settings
                .iter()
                .find(|entry| entry.plain_name == plain_name)
                .cloned()
        };
        let Some(entry) = entry else {
            return event;
        };

        // `player.chat(setting.getCommand())`.
        if let Some(value) = Toggle::from_command(&entry.command) {
            toggle(&event.player, value);
        }

        // Refresh the item with the new status (`getSettingStatus`).
        let on = {
            let guard = state().read().unwrap();
            guard
                .userdata
                .get(&uuid)
                .is_some_and(|user| status(&user.settings, &entry.setting))
        };
        let item = build_item(&entry, on);

        // `set_item` on a plugin inventory is a pure in-memory slot update (the
        // host syncs the open window per tick), so it is safe under the lock.
        let guard = state().read().unwrap();
        if let Some(session) = guard.settings_sessions.get(&uuid) {
            session.inventory.set_item(entry.slot, Some(item));
        }

        event
    }
}

/// Port-only listener: the original cached the inventory per player and never
/// had to learn about closes (the map was cleared on quit). Drops the session
/// when the window really closes (window switch, death, manual close).
pub struct SettingsCloseListener;

impl EventHandler<InventoryCloseEvent> for SettingsCloseListener {
    fn handle(&self, _server: Server, event: EventData<InventoryCloseEvent>) -> EventData<InventoryCloseEvent> {
        let uuid = core::uuid_of(&event.player);
        let mut guard = state().write().unwrap();

        let matches = guard
            .settings_sessions
            .get(&uuid)
            .is_some_and(|session| event.window_type == Some(session.screen));
        if !matches {
            return event;
        }

        if guard.settings_sessions.get(&uuid).is_some_and(|session| session.reopened) {
            // Close of the window `/settings` just replaced (`open_gui` fires
            // the previous window's close synchronously): keep the new session.
            if let Some(session) = guard.settings_sessions.get_mut(&uuid) {
                session.reopened = false;
            }
        } else {
            guard.settings_sessions.remove(&uuid);
        }

        event
    }
}

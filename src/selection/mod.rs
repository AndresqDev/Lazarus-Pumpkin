//! Claim selection wand — port of `me.qiooip.lazarus.selection.SelectionManager`.
//!
//! `/f claim` hands the player a claim wand. Left-click a block to set position 1,
//! right-click to set position 2, sneak + left-click to purchase, and right-click
//! the air to clear the selection. Selection pillars are rendered client-side via
//! the [`crate::glass`] engine.
//!
//! The `SELECTION` type (`/endportal`) shares the second wand: its clicks are
//! validated by [`crate::handlers::portal::end_portal`]. Both wands are built
//! once at startup from `CLAIM_WAND` / `SELECTION_WAND` (`config.yml`), like the
//! `SelectionManager` constructor in the original.

use std::collections::{BTreeMap, BTreeSet};

use pumpkin_plugin_api::common::{BlockPos, Hand};
use pumpkin_plugin_api::events::{
    EntityDamageByEntityEvent, EventData, EventHandler, PlayerChangedWorldEvent,
    PlayerDropItemEvent,
};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{ItemStack, Player, Server};

use crate::config::yaml::ConfigFile;
use crate::utils;

/// The block position a player clicked (world + coordinates).
pub type SelectedPos = (String, i32, i32, i32);

pub const WAND_ITEM: &str = "minecraft:golden_hoe";

/// The host strips the `minecraft:` prefix from registry keys, so compare on the
/// bare name (this was why the wand was never detected).
pub fn is_wand_key(key: &str) -> bool {
    key.trim_start_matches("minecraft:") == "golden_hoe"
}

/// What a player's active selection is for. The original uses three selection
/// types (`CLAIM`, `SYSTEM_CLAIM` and `SELECTION`); we keep the same separation
/// so a system-claim wand never buys a claim for the player's own faction and
/// the `/endportal` wand never buys one at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SelectionKind {
    #[default]
    Claim,
    /// `/f claimfor <faction>` — claims for the given system faction id.
    System(String),
    /// `/endportal` — clicks are checked by `EndPortalHandler` (frames, equal
    /// elevation, 2–4 block span) and fill the selection with end portals.
    Selection,
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub kind: SelectionKind,
    pub pos_one: Option<SelectedPos>,
    pub pos_two: Option<SelectedPos>,
}

impl Selection {
    pub fn both_set(&self) -> bool {
        self.pos_one.is_some() && self.pos_two.is_some()
    }

    /// Inclusive min/max X/Z for the selection, if complete.
    pub fn bounds(&self) -> Option<(String, i32, i32, i32, i32)> {
        let (world, x1, _, z1) = self.pos_one.clone()?;
        let (_, x2, _, z2) = self.pos_two.clone()?;
        Some((world, x1.min(x2), x1.max(x2), z1.min(z2), z1.max(z2)))
    }

    /// Port of `Selection#toClaim`.
    pub fn to_claim(&self) -> Option<crate::factions::Claim> {
        let (world, x1, _, z1) = self.pos_one.clone()?;
        let (_, x2, _, z2) = self.pos_two.clone()?;
        Some(crate::factions::Claim::new(world, x1, x2, z1, z2))
    }

    /// Port of `Selection#getSize`: returns `(min, max)` side lengths relative to
    /// the *other* position, or `(-1, -1)` when the other position is not set.
    pub fn get_size(&self, left_click: bool, x: i32, z: i32) -> (i32, i32) {
        let other = if left_click { &self.pos_two } else { &self.pos_one };
        match other {
            Some(pos) => {
                let size_x = (pos.1 - x).abs();
                let size_z = (pos.3 - z).abs();
                (size_x.min(size_z), size_x.max(size_z))
            }
            None => (-1, -1),
        }
    }

    /// Port of `Selection#sizeX`: the X distance between both positions.
    pub fn size_x(&self) -> i32 {
        match (&self.pos_one, &self.pos_two) {
            (Some(one), Some(two)) => (one.1 - two.1).abs(),
            _ => 0,
        }
    }

    /// Port of `Selection#sizeZ`: the Z distance between both positions.
    pub fn size_z(&self) -> i32 {
        match (&self.pos_one, &self.pos_two) {
            (Some(one), Some(two)) => (one.3 - two.3).abs(),
            _ => 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct SelectionManager {
    pub selections: BTreeMap<String, Selection>,
    /// Players currently sneaking (used for "sneak + left-click to buy").
    pub sneaking: BTreeSet<String>,
    /// `CLAIM_WAND` (`config.yml`) name + lore, parsed once at startup
    /// (`ItemStack` is not cloneable, so the text is cached and the stack is
    /// rebuilt on every handout).
    pub claim_wand: Option<(String, Vec<String>)>,
    /// `SELECTION_WAND` (`config.yml`) name + lore. The original aliases
    /// `SYSTEM_CLAIM` to `SELECTION`, so `/f claimfor` uses it too.
    pub selection_wand: Option<(String, Vec<String>)>,
}

impl SelectionManager {
    pub fn is_active(&self, uuid: &str) -> bool {
        self.selections.contains_key(uuid)
    }

    /// Drops a player's selection without touching their inventory (on quit).
    pub fn forget(&mut self, uuid: &str) {
        self.selections.remove(uuid);
        self.sneaking.remove(uuid);
    }

    pub fn get(&self, uuid: &str) -> Option<&Selection> {
        self.selections.get(uuid)
    }

    pub fn set_sneaking(&mut self, uuid: &str, sneaking: bool) {
        if sneaking {
            self.sneaking.insert(uuid.to_string());
        } else {
            self.sneaking.remove(uuid);
        }
    }

    pub fn is_sneaking(&self, uuid: &str) -> bool {
        self.sneaking.contains(uuid)
    }

    /// Gives the claim wand and starts (or restarts) the selection process.
    ///
    /// Does not acquire the state lock, so it is safe to call while holding it.
    pub fn start(&mut self, player: &Player) {
        self.cancel_local(player);
        self.give_wand(player, self.claim_wand_or_default());
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.insert(uuid, Selection::default());
    }

    /// Starts (or restarts) a selection bound to a system faction. The wand is
    /// the `SELECTION_WAND` (`SYSTEM_CLAIM` shares `SELECTION` in the original).
    pub fn start_system(&mut self, player: &Player, faction_id: String) {
        self.cancel_local(player);
        self.give_wand(player, self.selection_wand_or_default());
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.insert(
            uuid,
            Selection {
                kind: SelectionKind::System(faction_id),
                ..Selection::default()
            },
        );
    }

    /// `/endportal`: `toggleSelectionProcess` — always removes the previous
    /// selection and starts a fresh `SELECTION` one.
    pub fn start_selection(&mut self, player: &Player) {
        self.cancel_local(player);
        self.give_wand(player, self.selection_wand_or_default());
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.insert(
            uuid,
            Selection {
                kind: SelectionKind::Selection,
                ..Selection::default()
            },
        );
    }

    /// Puts a selection snapshot back (`selections.put(uuid, copy)` when a
    /// selection check fails).
    pub fn restore(&mut self, uuid: &str, selection: Selection) {
        self.selections.insert(uuid.to_string(), selection);
    }

    fn claim_wand_or_default(&self) -> ItemStack {
        let (name, lore) = self
            .claim_wand
            .clone()
            .unwrap_or_else(|| claim_wand_text(None));
        build_wand_from(name, lore)
    }

    fn selection_wand_or_default(&self) -> ItemStack {
        let (name, lore) = self
            .selection_wand
            .clone()
            .unwrap_or_else(|| selection_wand_text(None));
        build_wand_from(name, lore)
    }

    /// Ends the selection process, removing the wand and selection visuals.
    pub fn stop(&mut self, player: &Player) {
        self.cancel_local(player);
        crate::state::state().write().unwrap().glass.clear_type(player, crate::glass::GlassType::ClaimSelection);
    }

    /// Removes the selection + wand without touching any lock-guarded visuals.
    /// Callers that already hold the state lock must use this instead of [`stop`].
    pub fn cancel_local(&mut self, player: &Player) {
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.remove(&uuid);
        self.remove_wand(player);
    }

    /// Port of `removeSelectionProcess`: drops the selection + wand and clears
    /// the claim pillars for `CLAIM` selections. Callers hold the state lock and
    /// pass the sibling glass manager of the same guard.
    pub fn remove_selection_process(
        &mut self,
        glass: &mut crate::glass::GlassManager,
        player: &Player,
    ) {
        let uuid = crate::utils::core::uuid_of(player);
        let was_claim = matches!(
            self.get(&uuid).map(|selection| selection.kind.clone()),
            Some(SelectionKind::Claim)
        );

        self.cancel_local(player);

        if was_claim {
            glass.clear_type(player, crate::glass::GlassType::ClaimSelection);
        }
    }

    pub fn toggle(&mut self, player: &Player) {
        let uuid = crate::utils::core::uuid_of(player);
        if self.is_active(&uuid) {
            self.stop(player);
        } else {
            self.start(player);
        }
    }

    /// True when the player is holding the claim wand in either hand.
    pub fn holds_wand(&self, player: &Player) -> bool {
        [Hand::Right, Hand::Left].iter().any(|hand| {
            player
                .get_item_in_hand(*hand)
                .is_some_and(|item| is_wand_key(&item.get_registry_key()))
        })
    }

    /// Main-hand wand check (`Player#getItemInHand` in the original's attack and
    /// drop handlers).
    pub fn holds_wand_main_hand(&self, player: &Player) -> bool {
        player
            .get_item_in_hand(Hand::Right)
            .is_some_and(|item| is_wand_key(&item.get_registry_key()))
    }

    pub fn set_position(&mut self, uuid: &str, pos: SelectedPos, left_click: bool) {
        if let Some(selection) = self.selections.get_mut(uuid) {
            if left_click {
                selection.pos_one = Some(pos);
            } else {
                selection.pos_two = Some(pos);
            }
        }
    }

    pub fn clear_positions(&mut self, uuid: &str) {
        if let Some(selection) = self.selections.get_mut(uuid) {
            selection.pos_one = None;
            selection.pos_two = None;
        }
    }

    pub fn give_wand(&self, player: &Player, item: ItemStack) {
        for slot in 0..36u8 {
            if player.get_inventory_item(slot).is_none() {
                player.set_inventory_item(slot, Some(item));
                return;
            }
        }
    }

    pub fn remove_wand(&self, player: &Player) {
        for slot in 0..41u8 {
            if let Some(item) = player.get_inventory_item(slot)
                && is_wand_key(&item.get_registry_key())
            {
                player.set_inventory_item(slot, None);
            }
        }
    }
}

/// Parses `CLAIM_WAND` (`Config.CLAIM_WAND`) into a name + lore pair, falling
/// back to the original's default text when the entry is missing.
pub fn claim_wand_text(config: Option<&ConfigFile>) -> (String, Vec<String>) {
    let name = config
        .map(|config| config.string("CLAIM_WAND.NAME"))
        .filter(|name| !name.is_empty());
    let lore = config
        .map(|config| config.string_list("CLAIM_WAND.LORE"))
        .filter(|lore| !lore.is_empty());

    (
        name.unwrap_or_else(|| "&cClaim Wand".to_string()),
        lore.unwrap_or_else(default_claim_lore),
    )
}

/// Parses `SELECTION_WAND` (`Config.SELECTION_WAND`) into a name + lore pair.
pub fn selection_wand_text(config: Option<&ConfigFile>) -> (String, Vec<String>) {
    let name = config
        .map(|config| config.string("SELECTION_WAND.NAME"))
        .filter(|name| !name.is_empty());
    let lore = config
        .map(|config| config.string_list("SELECTION_WAND.LORE"))
        .filter(|lore| !lore.is_empty());

    (
        name.unwrap_or_else(|| "&cSelection Wand".to_string()),
        lore.unwrap_or_else(default_selection_lore),
    )
}

fn default_claim_lore() -> Vec<String> {
    vec![
        "&7Left/right click a block to set".to_string(),
        "&7the first and second position.".to_string(),
        String::new(),
        "&7Sneak and left click to &apurchase".to_string(),
        "&7your claim selection.".to_string(),
        String::new(),
        "&7Right click the air to &aclear&7.".to_string(),
    ]
}

fn default_selection_lore() -> Vec<String> {
    vec![
        "&7Left click to set &afirst &7position".to_string(),
        "&7Right click to set &asecond &7position".to_string(),
    ]
}

fn build_wand_from(name: String, lore: Vec<String>) -> ItemStack {
    let item = ItemStack::new(WAND_ITEM, 1);
    item.set_custom_name(Some(utils::text(name)));
    item.set_lore(lore.into_iter().map(utils::text).collect());
    item
}

/// Convenience: convert a selection position tuple to a [`BlockPos`].
pub fn to_block_pos(pos: SelectedPos) -> BlockPos {
    BlockPos { x: pos.1, y: pos.2, z: pos.3 }
}

/// Human readable `x, z` for selection messages.
pub fn format_pos(pos: SelectedPos) -> String {
    format!("{}, {}", pos.1, pos.3)
}

/// Used to satisfy the `TextComponent` import in downstream return types.
pub fn wand_name() -> TextComponent {
    utils::text("&cClaim Wand")
}

/// Port of `SelectionManager#onPlayerChangedWorld`: any world change drops the
/// selection process (the wand goes with it; claim pillars are cleared).
pub struct WorldChangeListener;

impl EventHandler<PlayerChangedWorldEvent> for WorldChangeListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerChangedWorldEvent>,
    ) -> EventData<PlayerChangedWorldEvent> {
        let mut guard = crate::state::state().write().unwrap();
        let app = &mut *guard;
        app.selection.remove_selection_process(&mut app.glass, &event.player);

        drop(guard);
        event
    }
}

/// Port of `SelectionManager#onEntityDamageByEntity`: attacking while holding
/// the wand of an active selection cancels the hit and ends the selection
/// process (`ignoreCancelled`, so hits denied by an earlier listener are
/// skipped). Only direct melee hits (`getDamager() instanceof Player`):
/// projectiles resolve to the shooter in the host and keep their damage.
pub struct WandAttackListener;

impl EventHandler<EntityDamageByEntityEvent> for WandAttackListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<EntityDamageByEntityEvent>,
    ) -> EventData<EntityDamageByEntityEvent> {
        if event.cancelled {
            return event;
        }

        let cause = crate::handlers::death::cause_from_damage_debug(&event.cause).unwrap_or_default();
        if crate::handlers::death::is_projectile_cause(&cause) {
            return event;
        }

        let Some(player) = crate::utils::core::player_by_entity(&server, event.damager_id) else {
            return event;
        };

        let uuid = crate::utils::core::uuid_of(&player);

        let mut guard = crate::state::state().write().unwrap();
        if !(guard.selection.is_active(&uuid) && guard.selection.holds_wand_main_hand(&player)) {
            return event;
        }

        event.cancelled = true;

        let app = &mut *guard;
        app.selection.remove_selection_process(&mut app.glass, &player);

        drop(guard);
        event
    }
}

/// Port of `SelectionManager#onPlayerDropItem` (LOWEST priority in the
/// original): dropping the wand is cancelled and the selection process ends.
/// The host reports only the dropped stack's registry key, so any wand-kind
/// item counts while a selection is active (the original compared the whole
/// `ItemStack`).
pub struct WandDropListener;

impl EventHandler<PlayerDropItemEvent> for WandDropListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<PlayerDropItemEvent>,
    ) -> EventData<PlayerDropItemEvent> {
        if event.cancelled {
            return event;
        }

        let uuid = crate::utils::core::uuid_of(&event.player);

        let mut guard = crate::state::state().write().unwrap();
        if !(guard.selection.is_active(&uuid) && is_wand_key(&event.item_name)) {
            return event;
        }

        event.cancelled = true;

        let app = &mut *guard;
        app.selection.remove_selection_process(&mut app.glass, &event.player);

        drop(guard);
        event
    }
}

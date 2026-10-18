//! Claim selection wand — port of `me.qiooip.lazarus.selection.SelectionManager`.
//!
//! `/f claim` hands the player a claim wand. Left-click a block to set position 1,
//! right-click to set position 2, sneak + left-click to purchase, and right-click
//! the air to clear the selection. Selection pillars are rendered client-side via
//! the [`crate::glass`] engine.

use std::collections::{BTreeMap, BTreeSet};

use pumpkin_plugin_api::common::{BlockPos, Hand};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{ItemStack, Player};

use crate::utils;

/// The block position a player clicked (world + coordinates).
pub type SelectedPos = (String, i32, i32, i32);

pub const WAND_ITEM: &str = "minecraft:golden_hoe";

/// The host strips the `minecraft:` prefix from registry keys, so compare on the
/// bare name (this was why the wand was never detected).
pub fn is_wand_key(key: &str) -> bool {
    key.trim_start_matches("minecraft:") == "golden_hoe"
}

/// What a player's active selection is for. The original uses two selection types
/// (`CLAIM` and `SYSTEM_CLAIM`); we keep the same separation so a system-claim wand
/// never buys a claim for the player's own faction.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SelectionKind {
    #[default]
    Claim,
    /// `/f claimfor <faction>` — claims for the given system faction id.
    System(String),
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
}

#[derive(Debug, Default)]
pub struct SelectionManager {
    pub selections: BTreeMap<String, Selection>,
    /// Players currently sneaking (used for "sneak + left-click to buy").
    pub sneaking: BTreeSet<String>,
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

    /// Gives the wand and starts (or restarts) the selection process.
    ///
    /// Does not acquire the state lock, so it is safe to call while holding it.
    pub fn start(&mut self, player: &Player) {
        self.cancel_local(player);
        self.give_wand(player);
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.insert(uuid, Selection::default());
    }

    /// Starts (or restarts) a selection bound to a system faction.
    pub fn start_system(&mut self, player: &Player, faction_id: String) {
        self.cancel_local(player);
        self.give_wand(player);
        let uuid = crate::utils::core::uuid_of(player);
        self.selections.insert(
            uuid,
            Selection {
                kind: SelectionKind::System(faction_id),
                ..Selection::default()
            },
        );
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

    pub fn give_wand(&self, player: &Player) {
        for slot in 0..36u8 {
            if player.get_inventory_item(slot).is_none() {
                player.set_inventory_item(slot, Some(build_wand()));
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

/// Builds the claim wand item stack.
pub fn build_wand() -> ItemStack {
    let item = ItemStack::new(WAND_ITEM, 1);
    item.set_custom_name(Some(utils::text("&cClaim Wand")));
    item.set_lore(vec![
        utils::text("&7Left/right click a block to set"),
        utils::text("&7the first and second position."),
        utils::text(""),
        utils::text("&7Sneak and left click to &apurchase"),
        utils::text("&7your claim selection."),
        utils::text(""),
        utils::text("&7Right click the air to &aclear&7."),
    ]);
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

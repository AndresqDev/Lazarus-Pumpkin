//! Claim wand interaction + sneak tracking.
//!
//! Faithful port of `ClaimManager#checkSelectionUpdate` and
//! `ClaimManager#onPlayerInteract` (the CLAIM selection branch), including:
//! * overworld-only claims that must land on wilderness;
//! * min/max size checks using the original `Selection#getSize` math;
//! * the price message when both positions are set;
//! * connectivity via `expand(1)` + corners/midpoints;
//! * the warzone test on the two selection corners.

use pumpkin_plugin_api::common::BlockPos;
use pumpkin_plugin_api::events::{
    EventData, EventHandler, InteractAction, PlayerInteractEvent, PlayerToggleSneakEvent,
};
use pumpkin_plugin_api::{Player, Server};

use crate::core::{self, Dimension, Territory};
use crate::factions::{Claim, ClaimManager, Faction};
use crate::glass;
use crate::selection::SelectedPos;
use crate::state::{state, AppState};
use crate::util;

/// Dedupes interaction events: the host fires them once per hand (main + off),
/// and the event carries no hand field, so identical interactions within a short
/// window must be collapsed (otherwise wand messages/positions duplicate).
static LAST_INTERACT: std::sync::Mutex<Option<std::collections::HashMap<String, i64>>> =
    std::sync::Mutex::new(None);

pub(crate) fn is_duplicate(key: &str) -> bool {
    let now = crate::util::now_millis();
    let mut guard = LAST_INTERACT.lock().unwrap();
    let map = guard.get_or_insert_with(std::collections::HashMap::new);
    if let Some(last) = map.get(key)
        && now - last < 120
    {
        return true;
    }
    map.insert(key.to_string(), now);
    false
}

pub struct InteractListener;

impl EventHandler<PlayerInteractEvent> for InteractListener {
    fn handle(&self, server: Server, mut event: EventData<PlayerInteractEvent>) -> EventData<PlayerInteractEvent> {
        let uuid = core::uuid_of(&event.player);
        let action = event.action;
        let clicked = event.clicked_pos.clone();

        // Bard clickables (right click), before the selection early-return.
        if matches!(
            action,
            InteractAction::RightClickBlock | InteractAction::RightClickAir
        ) && !is_duplicate(&format!("bard:{uuid}:{clicked:?}"))
        {
            crate::classes::bard_click(&server, &event.player);
            crate::classes::rogue_click(&server, &event.player);
            crate::classes::archer_click(&server, &event.player);
            crate::classes::mage_click(&server, &event.player);
            crate::classes::bomber_click(&server, &event.player);
        }

        // The class clicks above mirror the original's HIGHEST-priority handlers
        // (they run even on a cancelled event); the selection part mirrors
        // `SelectionManager#onPlayerInteract`'s `ignoreCancelled = true`.
        if event.cancelled {
            return event;
        }

        let (has_selection, holds_wand) = {
            let guard = state().read().unwrap();
            (
                guard.selection.is_active(&uuid),
                guard.selection.holds_wand(&event.player),
            )
        };
        if !(has_selection && holds_wand) {
            return event;
        }

        if is_duplicate(&format!("{uuid}:{action:?}:{clicked:?}")) {
            return event;
        }

        event.cancelled = true;

        let sneaking = state().read().unwrap().selection.is_sneaking(&uuid);
        // System selections (`/f claimfor`) never buy through sneaking: the claim is
        // created by re-running the command (original `SelectionManager`).
        let is_system = matches!(
            state().read().unwrap().selection.get(&uuid).map(|s| s.kind.clone()),
            Some(crate::selection::SelectionKind::System(_))
        );

        match action {
            InteractAction::LeftClickBlock => match clicked {
                Some(_) if sneaking && !is_system => purchase(&server, &event.player, &uuid),
                Some(pos) => set_position(&event.player, &uuid, pos, true),
                None => {}
            },
            InteractAction::LeftClickAir => {
                if sneaking && !is_system {
                    purchase(&server, &event.player, &uuid);
                }
            }
            InteractAction::RightClickBlock => {
                if let Some(pos) = clicked {
                    set_position(&event.player, &uuid, pos, false);
                }
            }
            InteractAction::RightClickAir => clear_selection(&event.player, &uuid),
        }

        event
    }
}

pub struct SneakListener;

impl EventHandler<PlayerToggleSneakEvent> for SneakListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerToggleSneakEvent>,
    ) -> EventData<PlayerToggleSneakEvent> {
        let uuid = core::uuid_of(&event.player);
        state()
            .write()
            .unwrap()
            .selection
            .set_sneaking(&uuid, event.is_sneaking);
        event
    }
}

fn selected_to_block_pos(pos: &SelectedPos) -> BlockPos {
    BlockPos { x: pos.1, y: pos.2, z: pos.3 }
}

fn location_string(x: i32, z: i32) -> String {
    format!("{x}, {z}")
}

/// Port of `ClaimManager#checkSelectionUpdate`.
fn set_position(player: &Player, uuid: &str, pos: BlockPos, left: bool) {
    let (world, x, y, z) = (core::world_name(player), pos.x, pos.y, pos.z);
    let position: SelectedPos = (world.clone(), x, y, z);

    let mut messages: Vec<String> = Vec::new();

    // Resolve the world/territory BEFORE taking the state lock: `core::territory_at`
    // locks `state()` internally and a write guard + read guard on the same RwLock
    // deadlocks (which trapped the WASM store).
    let is_overworld = core::dimension_of(&world) == Dimension::Overworld;
    let is_wilderness = matches!(core::territory_at(&world, x, z), Territory::Wilderness);
    // `/f claimfor` (system) selections are placed by `SelectionManager` in the
    // original: no wilderness / min-max / price checks, so they can cover the
    // warzone or the safezone.
    let is_system = matches!(
        state().read().unwrap().selection.get(uuid).map(|s| s.kind.clone()),
        Some(crate::selection::SelectionKind::System(_))
    );

    {
        let mut guard = state().write().unwrap();
        let AppState { selection, glass, factions, config, lang, .. } = &mut *guard;

        // Staff using a `/f claimfor` (system) selection are not required to be in
        // a faction, so only the player-claim price message needs one.
        let faction_id = factions.by_member(uuid).map(|f| f.id.clone());
        let faction_balance = faction_id
            .as_deref()
            .and_then(|id| factions.get(id))
            .map(|f| f.balance)
            .unwrap_or(0);

        // Can only claim in the overworld (player claims only).
        if !is_system && !is_overworld {
            messages.push(lang.faction_claim_only_overworld.clone());
        } else if !is_system && !is_wilderness {
            // The clicked location must be wilderness (player claims only).
            messages.push(lang.faction_claim_location_claimed.clone());
        } else {
            // Min/max size restriction (original `Selection#getSize`).
            let (min_size, max_size) = if is_system {
                (-1, -1)
            } else {
                selection
                    .get(uuid)
                    .map(|s| s.get_size(left, x, z))
                    .unwrap_or((-1, -1))
            };

            if min_size != -1 && min_size < config.factions.min_claim_size {
                messages.push(util::replace(
                    &lang.faction_claim_too_small,
                    "<size>",
                    &format!(
                        "{}x{}",
                        config.factions.min_claim_size, config.factions.min_claim_size
                    ),
                ));
            } else if max_size != -1 && max_size > config.factions.max_claim_size {
                messages.push(util::replace(
                    &lang.faction_claim_too_large,
                    "<size>",
                    &format!(
                        "{}x{}",
                        config.factions.max_claim_size, config.factions.max_claim_size
                    ),
                ));
            } else {
                selection.set_position(uuid, position.clone(), left);
                let current = selection.get(uuid).cloned().unwrap_or_default();

                glass::update_selection(
                    glass,
                    player,
                    current.pos_one.as_ref().map(selected_to_block_pos),
                    current.pos_two.as_ref().map(selected_to_block_pos),
                );

                let template = if left {
                    &lang.faction_claim_position_one
                } else {
                    &lang.faction_claim_position_two
                };
                messages.push(util::replace(template, "<location>", &location_string(x, z)));

                // Price message when both positions are set (player claims only).
                if !is_system
                    && let Some(claim) = current.to_claim()
                {
                    let size = claim.size_x() * claim.size_z();
                    let price = (size as f64 * config.factions.claim_price_per_block) as i64;
                    let price_string = if faction_balance < price {
                        format!("&c${price}")
                    } else {
                        format!("&a${price}")
                    };
                    messages.push(
                        util::replace_all(
                            &lang.faction_claim_price_message,
                            &[
                                ("<price>", &price_string),
                                ("<sizeX>", &claim.size_x().to_string()),
                                ("<sizeZ>", &claim.size_z().to_string()),
                                ("<size>", &size.to_string()),
                            ],
                        ),
                    );
                }
            }
        }
    }

    for message in messages {
        core::send(player, message);
    }
}

fn clear_selection(player: &Player, uuid: &str) {
    let mut guard = state().write().unwrap();
    let AppState { selection, glass, lang, .. } = &mut *guard;
    selection.clear_positions(uuid);
    glass::update_selection(glass, player, None, None);
    let message = lang.faction_claim_selection_cleared.clone();
    drop(guard);

    core::send(player, message);
}

/// Port of the shift-left-click purchase branch of `ClaimManager#onPlayerInteract`.
fn purchase(server: &Server, player: &Player, uuid: &str) {
    let mut messages: Vec<String> = Vec::new();
    let mut created: Option<(crate::factions::Claim, String)> = None;

    let (system_missing, system_claimed) = {
        let guard = state().read().unwrap();
        (
            guard.raw.language.string("FACTIONS.FACTION_DOESNT_EXIST"),
            guard
                .raw
                .language
                .string("FACTIONS.CLAIM_FOR_COMMAND.CLAIM_CLAIMED"),
        )
    };

    {
        let mut guard = state().write().unwrap();
        let AppState { factions, claims, config, lang, selection, glass, .. } = &mut *guard;

        let Some(claim) = selection.get(uuid).and_then(|s| s.to_claim()) else {
            messages.push(lang.faction_claim_selection_not_set.clone());
            return apply(server, player, &messages);
        };

        // ---- System claim (`/f claimfor`): never touches the player's own faction ----
        if let Some(crate::selection::SelectionKind::System(target_id)) =
            selection.get(uuid).map(|s| s.kind.clone())
        {
            if claims.overlaps(&claim) {
                messages.push(lang.faction_claim_overlap.clone());
                return apply(server, player, &messages);
            }

            match factions.get(&target_id).cloned() {
                Some(target) => {
                    claims.add_claim(&target_id, claim.clone());
                    messages.push(util::replace_all(
                        &system_claimed,
                        &[
                            ("<player>", &core::name_of(player)),
                            ("<faction>", &format!("{}{}", target.color, target.name)),
                        ],
                    ));
                }
                None => {
                    // The faction disappeared: drop the wand instead of erroring.
                    messages.push(util::replace(&system_missing, "<argument>", &target_id));
                }
            }

            selection.cancel_local(player);
            glass::update_selection(glass, player, None, None);
            return apply(server, player, &messages);
        }

        let Some(faction_id) = factions.by_member(uuid).map(|f| f.id.clone()) else {
            messages.push(lang.faction_not_in.clone());
            return apply(server, player, &messages);
        };
        let Some(faction) = factions.get(&faction_id).cloned() else {
            // Faction gone: cancel the selection and remove the wand.
            selection.cancel_local(player);
            glass::update_selection(glass, player, None, None);
            return apply(server, player, &messages);
        };

        // isSelectionConnected
        if !is_selection_connected(
            claims,
            &faction_id,
            &claim,
            config.factions.must_be_connected,
        ) {
            messages.push(lang.faction_claim_not_connected.clone());
            return apply(server, player, &messages);
        }

        // canClaimSelection -> warzone (two corners)
        let warzone = match core::dimension_of(&claim.world) {
            Dimension::Overworld => config.warzone_radius.overworld,
            Dimension::Nether => config.warzone_radius.nether,
            Dimension::End => config.warzone_radius.end,
        };
        if is_warzone(&claim, warzone) {
            messages.push(lang.faction_claim_warzone.clone());
            return apply(server, player, &messages);
        }

        // canClaimSelection -> overlap (every faction, including your own)
        if claims.overlaps(&claim) {
            messages.push(lang.faction_claim_overlap.clone());
            return apply(server, player, &messages);
        }

        // canClaimSelection -> buffer (roads may be ignored)
        let ignore_factions = if config.factions.claim_buffer_ignore_roads {
            factions.road_ids()
        } else {
            Vec::new()
        };
        if claims.within_buffer(
            &claim,
            config.factions.claim_buffer,
            &faction_id,
            &ignore_factions,
        ) {
            messages.push(util::replace(
                &lang.faction_claim_buffer,
                "<buffer>",
                &config.factions.claim_buffer.to_string(),
            ));
            return apply(server, player, &messages);
        }

        // Price: sizeX * sizeZ * price-per-block (original math, no +1).
        let size = claim.size_x() * claim.size_z();
        let price = (size as f64 * config.factions.claim_price_per_block) as i64;
        if faction.balance < price {
            messages.push(lang.faction_claim_not_enough_money.clone());
            return apply(server, player, &messages);
        }

        claims.add_claim(&faction_id, claim.clone());
        if let Some(faction) = factions.get_mut(&faction_id) {
            faction.balance -= price;
        }
        created = Some((claim.clone(), faction_id.clone()));

        selection.cancel_local(player);
        glass::update_selection(glass, player, None, None);

        let player_name = core::name_of(player);
        messages.push(util::replace_all(
            &lang.faction_claim_claimed,
            &[
                ("<player>", &player_name),
                ("<location1>", &location_string(claim.min_x, claim.min_z)),
                ("<location2>", &location_string(claim.max_x, claim.max_z)),
            ],
        ));
        faction_broadcast(server, &faction, messages.pop().unwrap());
    }

    for message in messages {
        core::send(player, message);
    }

    // Eject PvP-protected players that ended up inside the new claim.
    if let Some((claim, owner_id)) = created {
        crate::core::eject_intruders(server, &claim, &owner_id);
    }
}

/// Small helper so the early-returns above still emit their messages.
fn apply(server: &Server, player: &Player, messages: &[String]) {
    let _ = server;
    for message in messages {
        core::send(player, message);
    }
}

/// Port of `ClaimManager#isSelectionConnected`.
pub(crate) fn is_selection_connected(
    claims: &ClaimManager,
    faction_id: &str,
    claim: &Claim,
    must_be_connected: bool,
) -> bool {
    if !must_be_connected {
        return true;
    }

    let existing = claims.claims_of(faction_id);
    if existing.is_empty() {
        return true;
    }

    let half_x = (claim.min_x + claim.max_x) / 2;
    let half_z = (claim.min_z + claim.max_z) / 2;

    for other in existing {
        let expanded = other.clone().expand(1);

        for (cx, cz) in claim.corners() {
            if expanded.contains(&claim.world, cx, cz) {
                return true;
            }
        }

        if expanded.contains(&claim.world, half_x, claim.min_z)
            || expanded.contains(&claim.world, half_x, claim.max_z)
            || expanded.contains(&claim.world, claim.min_x, half_z)
            || expanded.contains(&claim.world, claim.max_x, half_z)
        {
            return true;
        }
    }

    false
}

/// Port of `ClaimManager#isWarzone`: tests the two selection corners.
fn is_warzone(claim: &Claim, warzone: i32) -> bool {
    (claim.min_x.abs() <= warzone && claim.min_z.abs() <= warzone)
        || (claim.max_x.abs() <= warzone && claim.max_z.abs() <= warzone)
}

fn faction_broadcast(server: &Server, faction: &Faction, message: String) {
    for member in faction.members.values() {
        if let Some(uuid) = pumpkin_plugin_api::uuid::parse(&member.uuid)
            && let Some(online) = server.get_player_by_uuid(uuid)
        {
            core::send(&online, &message);
        }
    }
}

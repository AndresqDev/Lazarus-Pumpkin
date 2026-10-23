//! Native artificial tab-list **API** for the Lazarus Pumpkin port.
//!
//! The WASM host cannot serialise `CPlayerInfoUpdate`, so this native companion
//! owns the tab list. The WASM plugin drives it by writing a JSON state file to
//! its data folder:
//!
//! `<data_folder>/tab_state.json` (i.e. `plugins/data/lazarus/tab_state.json`)
//!
//! ```jsonc
//! {
//!   // slots shown to everyone
//!   "global": {
//!     "0": { "text": "&5&lFaction Info", "name": "lz00", "order": 0,
//!            "skin": { "value": "<base64>", "signature": "<base64|omitted>" } }
//!   },
//!   // per-player overrides (dynamic content, e.g. live coordinates)
//!   "viewers": {
//!     "<player-uuid>": { "23": { "text": "&7Coords: &f12, 34" } }
//!   }
//! }
//! ```
//!
//! The native plugin diffs the file against what it already sent and only sends
//! the slots that changed (no full reload), broadcasting global slots and sending
//! per-player overrides only to that player. Changed slots are grouped by action
//! set, so one viewer normally receives one packet for the updates and a second
//! one only when newly-added entries need the skin/`ADD_PLAYER` action. New
//! joiners get the full current state in the same grouped form.
//!
//! A file-based API is used because Pumpkin's `#[plugin_method]` macro cannot
//! implement `on_ipc_message` (borrowed `&str`/`&[u8]` lifetimes don't match the
//! trait's returned `PluginFuture<'_, _>`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pumpkin::entity::player::Player as ServerPlayer;
use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin::server::Server;
use pumpkin_api_macros::{plugin_impl, plugin_method};
use pumpkin_protocol::java::client::play::{
    CPlayerInfoUpdate, CRemovePlayerInfo, CSetPlayerTeam, Player, PlayerAction, PlayerInfoFlags,
    TeamMethod, TeamParameters,
};
use pumpkin_protocol::{Property, VarInt};
use pumpkin_util::text::TextComponent;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;
const STATE_FILE: &str = "plugins/data/lazarus/tab_state.json";
/// Team that holds every real player on the client side.
///
/// The 1.8 tab sorts by team name before profile name and only renders the
/// first 80 entries (see `GuiPlayerTabOverlay.PlayerComparator` and its
/// `subList(0, min(size, 80))`). The 80 artificial slots belong to no team, so
/// real players (sorted into this named team) always fall behind them and are
/// cut off by the 80-entry cap. ViaVersion drops the modern `listed` flag for
/// 1.8 clients, so this is the only way to keep them out of the tab there.
const REAL_TEAM: &str = "lzreal";

/// `TabManager.VALUE` — the shared head used by every artificial slot.
/// Kept here (not in the WASM state) so `tab_state.json` does not repeat the
/// ~1 KB skin blob 80 times per player.
const DEFAULT_SKIN_VALUE: &str = "eyJ0aW1lc3RhbXAiOjE0MTEyNjg3OTI3NjUsInByb2ZpbGVJZCI6IjNmYmVjN2RkMGE1ZjQwYmY5ZDExODg1YTU0NTA3MTEyIiwicHJvZmlsZU5hbWUiOiJsYXN0X3VzZXJuYW1lIiwidGV4dHVyZXMiOnsiU0tJTiI6eyJ1cmwiOiJodHRwOi8vdGV4dHVyZXMubWluZWNyYWZ0Lm5ldC90ZXh0dXJlLzg0N2I1Mjc5OTg0NjUxNTRhZDZjMjM4YTFlM2MyZGQzZTMyOTY1MzUyZTNhNjRmMzZlMTZhOTQwNWFiOCJ9fX0=";
const DEFAULT_SKIN_SIGNATURE: &str = "u8sG8tlbmiekrfAdQjy4nXIcCfNdnUZzXSx9BE1X5K27NiUvE1dDNIeBBSPdZzQG1kHGijuokuHPdNi/KXHZkQM7OJ4aCu5JiUoOY28uz3wZhW4D+KG3dH4ei5ww2KwvjcqVL7LFKfr/ONU5Hvi7MIIty1eKpoGDYpWj3WjnbN4ye5Zo88I2ZEkP1wBw2eDDN4P3YEDYTumQndcbXFPuRRTntoGdZq3N5EBKfDZxlw4L3pgkcSLU5rWkd5UH4ZUOHAP/VaJ04mpFLsFXzzdU4xNZ5fthCwxwVBNLtHRWO26k/qcVBzvEXtKGFJmxfLGCzXScET/OjUBak/JEkkRG2m+kpmBMgFRNtjyZgQ1w08U6HHnLTiAiio3JswPlW5v56pGWRHQT5XWSkfnrXDalxtSmPnB5LmacpIImKgL8V9wLnWvBzI7SHjlyQbbgd+kUOkLlu7+717ySDEJwsFJekfuR6N/rpcYgNZYrxDwe4w57uDPlwNL6cJPfNUHV7WEbIU1pMgxsxaXe8WSvV87qLsR7H06xocl2C0JFfe2jZR4Zh3k9xzEnfCeFKBgGb4lrOWBu1eDWYgtKV67M2Y+B3W5pjuAjwAxn0waODtEn/3jKPbc/sxbPvljUCw65X+ok0UUN1eOwXV5l2EGzn05t3Yhwq19/GxARg63ISGE8CKw=";

#[derive(Clone, Deserialize, PartialEq)]
struct Skin {
    value: String,
    signature: Option<String>,
}

#[derive(Clone, Deserialize, PartialEq)]
#[serde(default)]
struct SlotSpec {
    text: String,
    name: Option<String>,
    order: Option<i32>,
    skin: Option<Skin>,
}

impl Default for SlotSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            name: None,
            order: None,
            skin: None,
        }
    }
}

impl SlotSpec {
    fn profile_name(&self, slot: u8) -> String {
        self.name.clone().unwrap_or_else(|| entry_name(slot as usize))
    }

    fn order(&self, slot: u8) -> i32 {
        self.order.unwrap_or(slot as i32)
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct FileState {
    /// slot (as string) -> spec
    global: HashMap<String, SlotSpec>,
    /// player uuid -> (slot -> spec)
    viewers: HashMap<String, HashMap<String, SlotSpec>>,
}

#[derive(Default)]
struct Rendered {
    global: HashMap<u8, SlotSpec>,
    viewers: HashMap<Uuid, HashMap<u8, SlotSpec>>,
}

impl NativeTab {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(Rendered::default())),
        }
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        context.log(format!("NativeTab API loaded (Pumpkin API v{PLUGIN_API_VERSION})"));

        let server = context.server.clone();
        let state = Arc::clone(&self.state);
        let log_ctx = context.clone();
        std::thread::spawn(move || run(server, state, log_ctx));

        Ok(())
    }

    #[plugin_method]
    async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
        info!("NativeTab API unloaded");
        Ok(())
    }
}

#[plugin_impl]
pub struct NativeTab {
    state: Arc<Mutex<Rendered>>,
}

// ---------------------------------------------------------------------------
// Loop
// ---------------------------------------------------------------------------

fn run(server: Arc<Server>, state: Arc<Mutex<Rendered>>, log_ctx: Arc<Context>) {
    let mut known: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    let mut last_signature: Option<(u128, u64)> = None;
    // Players that joined recently: re-send their full state once so the
    // packets survive the client's join/readiness window.
    let mut pending_full: std::collections::HashMap<Uuid, u8> = std::collections::HashMap::new();
    // Last set of real player names that was pushed to the clients' scoreboard
    // teams (see `REAL_TEAM`).
    let mut team_signature: Option<Vec<String>> = None;

    loop {
        let players = server.get_all_players();
        let current: std::collections::HashSet<Uuid> =
            players.iter().map(|p| p.gameprofile.id).collect();

        for uuid in current.difference(&known) {
            pending_full.insert(*uuid, 1);
            log_ctx.log(format!("NativeTab: queued full resend for {uuid}"));
        }
        pending_full.retain(|uuid, _| current.contains(uuid));

        hide_real_players(&server, &mut team_signature);

        // Apply the state file when it changes.
        if let Some(signature) = file_signature()
            && last_signature != Some(signature)
        {
            last_signature = Some(signature);
            apply_file(&server, &state);
        }

        // Retry the full per-viewer send for recent joiners (the client may
        // drop the first packets while still finishing the join sequence).
        // Only the *first* add carries the skin property, so this cannot
        // restart the 1.8 skin download (see `send_changes`).
        let retry: Vec<Uuid> = pending_full
            .iter()
            .filter(|(_, remaining)| **remaining > 0)
            .map(|(uuid, _)| *uuid)
            .collect();
        for uuid in retry {
            if send_full_viewer(&server, &state, uuid) {
                if let Some(remaining) = pending_full.get_mut(&uuid) {
                    *remaining -= 1;
                }
            }
        }

        known = current;
        // The WASM side flushes event-driven tab changes immediately (claim
        // changes, for example); polling at 100 ms keeps that latency below
        // the next game tick instead of adding up to 500 ms.
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn file_signature() -> Option<(u128, u64)> {
    let meta = std::fs::metadata(STATE_FILE).ok()?;
    // Use the full modified timestamp, not only whole seconds: a claim/timer
    // update can rewrite the file with the same byte length twice within one
    // second, and an `(as_secs(), len)` signature would miss the second write.
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((duration.as_nanos(), meta.len()))
}

fn read_state() -> Option<FileState> {
    let contents = std::fs::read_to_string(STATE_FILE).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Applies the file: diffs against what was rendered and only sends changes.
fn apply_file(server: &Arc<Server>, state: &Arc<Mutex<Rendered>>) {
    let Some(file) = read_state() else {
        return;
    };

    let mut global_changes: Vec<(u8, SlotSpec, bool)> = Vec::new();
    let mut global_removals: Vec<u8> = Vec::new();
    let mut viewer_changes: HashMap<Uuid, Vec<(u8, SlotSpec, bool)>> = HashMap::new();
    let mut viewer_removals: HashMap<Uuid, Vec<u8>> = HashMap::new();

    {
        let mut rendered = state.lock().unwrap();

        // Global diff.
        let mut desired_global: HashMap<u8, SlotSpec> = HashMap::new();
        for (slot_key, spec) in &file.global {
            if let Ok(slot) = slot_key.parse::<u8>()
                && slot < SLOTS as u8
            {
                desired_global.insert(slot, spec.clone());
            }
        }

        for (slot, spec) in &desired_global {
            if rendered.global.get(slot) != Some(spec) {
                // Only the first add for a slot carries the skin property; later
                // updates must not re-add the profile or 1.8 clients restart the
                // skin download on every text change.
                let first = !rendered.global.contains_key(slot);
                global_changes.push((*slot, spec.clone(), first));
            }
        }
        for slot in rendered.global.keys() {
            if !desired_global.contains_key(slot) {
                global_removals.push(*slot);
            }
        }
        rendered.global = desired_global;

        // Per-viewer diff.
        let mut desired_viewers: HashMap<Uuid, HashMap<u8, SlotSpec>> = HashMap::new();
        for (uuid_key, slots) in &file.viewers {
            let Ok(uuid) = Uuid::parse_str(uuid_key) else {
                continue;
            };
            let mut map = HashMap::new();
            for (slot_key, spec) in slots {
                if let Ok(slot) = slot_key.parse::<u8>()
                    && slot < SLOTS as u8
                {
                    map.insert(slot, spec.clone());
                }
            }
            desired_viewers.insert(uuid, map);
        }

        // Removed viewer overrides.
        for (uuid, slots) in &rendered.viewers {
            let desired = desired_viewers.get(uuid);
            for slot in slots.keys() {
                if desired.map(|d| !d.contains_key(slot)).unwrap_or(true) {
                    viewer_removals.entry(*uuid).or_default().push(*slot);
                }
            }
        }

        for (uuid, slots) in &desired_viewers {
            let previous = rendered.viewers.get(uuid);
            for (slot, spec) in slots {
                if previous.and_then(|p| p.get(slot)) != Some(spec) {
                    let first = previous.map(|p| !p.contains_key(slot)).unwrap_or(true);
                    viewer_changes
                        .entry(*uuid)
                        .or_default()
                        .push((*slot, spec.clone(), first));
                }
            }
        }

        rendered.viewers = desired_viewers;
    }

    // Send outside the native render lock: a grouped packet can serialise
    // several entries (and one skin), and another state update should not have
    // to wait for the network writes.
    if !global_removals.is_empty() {
        send_remove_broadcast(server, &global_removals);
    }
    send_changes(server, None, &global_changes);

    for (uuid, slots) in viewer_removals {
        send_remove_to(server, uuid, &slots);
    }
    for (uuid, changes) in viewer_changes {
        send_changes(server, Some(uuid), &changes);
    }
}

/// Sends every slot of a single viewer's state to that player.
/// Returns `false` when the WASM side has not published this viewer yet.
fn send_full_viewer(server: &Arc<Server>, state: &Arc<Mutex<Rendered>>, viewer: Uuid) -> bool {
    let changes: Vec<(u8, SlotSpec, bool)> = {
        let rendered = state.lock().unwrap();
        match rendered.viewers.get(&viewer) {
            Some(map) => map
                .iter()
                .map(|(slot, spec)| (*slot, spec.clone(), true))
                .collect(),
            None => return false,
        }
    };

    send_changes(server, Some(viewer), &changes);
    true
}

// ---------------------------------------------------------------------------
// Packets
// ---------------------------------------------------------------------------

/// Sends all changed slots in at most two packets: one for entries that need
/// `ADD_PLAYER` (skin included) and one for display-only updates.
///
/// `viewer = None` broadcasts the global slots; `Some(uuid)` sends only to that
/// player. The entries in a packet are independent profiles, so one viewer can
/// receive the 80 changed slots without 80 separate `CPlayerInfoUpdate`s.
fn send_changes(server: &Arc<Server>, viewer: Option<Uuid>, changes: &[(u8, SlotSpec, bool)]) {
    if changes.is_empty() {
        return;
    }

    let player = if let Some(uuid) = viewer {
        let Some(player) = server.get_player_by_uuid(uuid) else {
            return;
        };
        Some(player)
    } else {
        None
    };

    // Group by action set: ADD_PLAYER cannot be mixed with display-only
    // entries because every profile in a packet must carry the same fields in
    // the same order.
    for full in [true, false] {
        let selected: Vec<(u8, SlotSpec, bool)> = changes
            .iter()
            .filter(|(_, _, is_full)| *is_full == full)
            .cloned()
            .collect();
        if selected.is_empty() {
            continue;
        }

        let names: Vec<String> = selected
            .iter()
            .map(|(slot, spec, _)| spec.profile_name(*slot))
            .collect();
        let texts: Vec<TextComponent> = selected
            .iter()
            .map(|(_, spec, _)| TextComponent::from_legacy_string_with_code(&spec.text, '&'))
            .collect();
        let properties: Vec<Vec<Property>> = selected
            .iter()
            .map(|(_, spec, _)| properties(spec))
            .collect();
        let orders: Vec<i32> = selected
            .iter()
            .map(|(slot, spec, _)| spec.order(*slot))
            .collect();
        let uuids: Vec<Uuid> = selected
            .iter()
            .map(|(slot, _, _)| entry_uuid(*slot as usize))
            .collect();

        let actions: Vec<Vec<PlayerAction>> = (0..selected.len())
            .map(|i| {
                if full {
                    vec![
                        PlayerAction::AddPlayer {
                            name: &names[i],
                            properties: &properties[i],
                        },
                        PlayerAction::UpdateListed(true),
                        PlayerAction::UpdateDisplayName(Some(&texts[i])),
                        PlayerAction::UpdateListOrder(VarInt(orders[i])),
                    ]
                } else {
                    // The tab order of an artificial slot never changes after
                    // the first ADD_PLAYER, and 1.8 has no list-order field, so
                    // omit `UpdateListOrder` from display-only updates. A race
                    // in an earlier smoke made this look like ViaVersion was
                    // dropping the whole packet; the corrected control with the
                    // previous native also applied tab changes.
                    vec![PlayerAction::UpdateDisplayName(Some(&texts[i]))]
                }
            })
            .collect();

        let players: Vec<Player> = (0..selected.len())
            .map(|i| Player {
                uuid: uuids[i],
                actions: &actions[i],
            })
            .collect();
        let flags = if full {
            PlayerInfoFlags::ADD_PLAYER
                | PlayerInfoFlags::UPDATE_LISTED
                | PlayerInfoFlags::UPDATE_DISPLAY_NAME
                | PlayerInfoFlags::UPDATE_LIST_PRIORITY
        } else {
            PlayerInfoFlags::UPDATE_DISPLAY_NAME
        };
        let packet = CPlayerInfoUpdate::new(flags.bits(), &players);

        if let Some(player) = &player {
            let player = Arc::clone(player);
            let _ = crate::GLOBAL_RUNTIME.block_on(async move {
                player.send_client_packet(&packet).await;
            });
        } else {
            server.broadcast_packet_all(&packet);
        }
    }
}

fn send_remove_broadcast(server: &Arc<Server>, slots: &[u8]) {
    let uuids: Vec<Uuid> = slots.iter().map(|slot| entry_uuid(*slot as usize)).collect();
    server.broadcast_packet_all(&CRemovePlayerInfo { players: &uuids });
}

fn send_remove_to(server: &Arc<Server>, viewer: Uuid, slots: &[u8]) {
    let Some(player) = server.get_player_by_uuid(viewer) else {
        return;
    };
    let uuids: Vec<Uuid> = slots.iter().map(|slot| entry_uuid(*slot as usize)).collect();
    let packet = CRemovePlayerInfo { players: &uuids };
    let _ = crate::GLOBAL_RUNTIME.block_on(async move {
        player.send_client_packet(&packet).await;
    });
}

fn properties(spec: &SlotSpec) -> Vec<Property> {
    let fallback = Skin {
        value: DEFAULT_SKIN_VALUE.to_string(),
        signature: Some(DEFAULT_SKIN_SIGNATURE.to_string()),
    };
    let skin = spec.skin.as_ref().unwrap_or(&fallback);
    vec![Property {
        name: "textures".into(),
        value: skin.value.clone().into(),
        signature: skin.signature.clone().map(Into::into),
    }]
}

fn hide_real_players(server: &Arc<Server>, team_signature: &mut Option<Vec<String>>) {
    let real = server.get_all_players();
    if real.is_empty() {
        *team_signature = None;
        return;
    }

    // 1.8 tab order: team name → profile name, capped at 80 entries. The
    // artificial slots have no team, so a named team always sorts after them
    // and pushes every real player past the cap (see `REAL_TEAM`).
    let mut names: Vec<String> = real.iter().map(|p| p.gameprofile.name.clone()).collect();
    names.sort();
    if team_signature.as_ref() != Some(&names) {
        *team_signature = Some(names.clone());
        send_real_team(server, &names);

        // Modern clients (1.19.3+) honour the listed flag; ViaVersion drops it
        // for older protocols, where the team ordering above hides the entries.
        // Only send this when the player set changes, not on every 500 ms pass.
        let actions: Vec<Vec<PlayerAction>> = real
            .iter()
            .map(|_| vec![PlayerAction::UpdateListed(false)])
            .collect();
        let players: Vec<Player> = real
            .iter()
            .enumerate()
            .map(|(i, player)| Player {
                uuid: player.gameprofile.id,
                actions: &actions[i],
            })
            .collect();
        server.broadcast_packet_all(&CPlayerInfoUpdate::new(
            PlayerInfoFlags::UPDATE_LISTED.bits(),
            &players,
        ));
    }
}

/// (Re)creates the client-side team that holds every real player.
fn send_real_team(server: &Arc<Server>, names: &[String]) {
    // Recreate instead of updating so a stale copy from an earlier session
    // cannot keep removed players.
    server.broadcast_packet_all(&CSetPlayerTeam {
        team_name: REAL_TEAM.to_string(),
        method: TeamMethod::Remove,
        parameters: None,
        players: Box::new([]),
    });

    let display = TextComponent::text(REAL_TEAM);
    let empty = TextComponent::text("");
    let parameters = TeamParameters {
        display_name: &display,
        // 0x01 = friendly fire allowed, 0x02 = see friendly invisibles.
        // Keep both vanilla defaults: no PvP or invisibility change.
        options: 0x01,
        nametag_visibility: "always",
        collision_rule: "always",
        // White is the default player name colour.
        color: 15,
        player_prefix: &empty,
        player_suffix: &empty,
    };

    server.broadcast_packet_all(&CSetPlayerTeam {
        team_name: REAL_TEAM.to_string(),
        method: TeamMethod::Create,
        parameters: Some(parameters),
        players: names.to_vec().into_boxed_slice(),
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn entry_uuid(index: usize) -> Uuid {
    Uuid::from_u64_pair(
        0x1A2B_0000_0000_0000u64 | (index as u64 & 0xFFFF),
        0xDEAD_BEEF_0000_0000u64 | index as u64,
    )
}

fn entry_name(index: usize) -> String {
    format!("lz{index:02}")
}

/// Seeds the global slots from `tab.toml` when no state file exists yet.
#[allow(dead_code)]
fn column_lines(config: &TabConfig) -> Vec<String> {
    let mut flat: Vec<String> = Vec::with_capacity(SLOTS);
    for column in [&config.left, &config.center, &config.right, &config.far_right] {
        let mut padded = column.clone();
        padded.resize(COLUMN_HEIGHT, String::new());
        flat.extend(padded);
    }
    flat
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
#[allow(dead_code)]
struct TabConfig {
    enabled: bool,
    left: Vec<String>,
    center: Vec<String>,
    right: Vec<String>,
    far_right: Vec<String>,
}

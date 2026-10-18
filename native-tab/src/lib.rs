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
//! the slots that changed (per-slot update, no full reload), broadcasting global
//! slots and sending per-player overrides only to that player. New joiners get the
//! full current state.
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
    let mut last_signature: Option<(u64, u64)> = None;
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
        // restart the 1.8 skin download (see `send_slot_to`).
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
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn file_signature() -> Option<(u64, u64)> {
    let meta = std::fs::metadata(STATE_FILE).ok()?;
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((duration.as_secs(), meta.len()))
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
            send_slot_broadcast(server, *slot, spec, first);
        }
    }
    for slot in rendered.global.keys() {
        if !desired_global.contains_key(slot) {
            send_remove_broadcast(server, &[*slot]);
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
                send_remove_to(server, *uuid, &[*slot]);
            }
        }
    }

    for (uuid, slots) in &desired_viewers {
        let previous = rendered.viewers.get(uuid);
        for (slot, spec) in slots {
            if previous.and_then(|p| p.get(slot)) != Some(spec) {
                let first = previous.map(|p| !p.contains_key(slot)).unwrap_or(true);
                send_slot_to(server, Some(*uuid), *slot, spec, first);
            }
        }
    }

    rendered.viewers = desired_viewers;
}

/// Sends every slot of a single viewer's state to that player.
/// Returns `false` when the WASM side has not published this viewer yet.
fn send_full_viewer(server: &Arc<Server>, state: &Arc<Mutex<Rendered>>, viewer: Uuid) -> bool {
    let slots: Vec<(u8, SlotSpec)> = {
        let rendered = state.lock().unwrap();
        match rendered.viewers.get(&viewer) {
            Some(map) => map.iter().map(|(slot, spec)| (*slot, spec.clone())).collect(),
            None => return false,
        }
    };

    for (slot, spec) in slots {
        send_slot_to(server, Some(viewer), slot, &spec, true);
    }
    true
}

// ---------------------------------------------------------------------------
// Packets
// ---------------------------------------------------------------------------

fn send_slot_broadcast(server: &Arc<Server>, slot: u8, spec: &SlotSpec, full: bool) {
    let name = spec.profile_name(slot);
    let text = TextComponent::from_legacy_string_with_code(&spec.text, '&');
    let properties = properties(spec);
    let actions = if full {
        vec![
            PlayerAction::AddPlayer {
                name: &name,
                properties: &properties,
            },
            PlayerAction::UpdateListed(true),
            PlayerAction::UpdateDisplayName(Some(&text)),
            PlayerAction::UpdateListOrder(VarInt(spec.order(slot))),
        ]
    } else {
        vec![
            PlayerAction::UpdateDisplayName(Some(&text)),
            PlayerAction::UpdateListOrder(VarInt(spec.order(slot))),
        ]
    };
    let players = [Player {
        uuid: entry_uuid(slot as usize),
        actions: &actions,
    }];
    let flags = if full {
        PlayerInfoFlags::ADD_PLAYER
            | PlayerInfoFlags::UPDATE_LISTED
            | PlayerInfoFlags::UPDATE_DISPLAY_NAME
            | PlayerInfoFlags::UPDATE_LIST_PRIORITY
    } else {
        PlayerInfoFlags::UPDATE_DISPLAY_NAME | PlayerInfoFlags::UPDATE_LIST_PRIORITY
    };
    server.broadcast_packet_all(&CPlayerInfoUpdate::new(flags.bits(), &players));
}

fn send_slot_to(server: &Arc<Server>, viewer: Option<Uuid>, slot: u8, spec: &SlotSpec, full: bool) {
    let Some(uuid) = viewer else {
        send_slot_broadcast(server, slot, spec, full);
        return;
    };
    let Some(player) = server.get_player_by_uuid(uuid) else {
        return;
    };

    let name = spec.profile_name(slot);
    let text = TextComponent::from_legacy_string_with_code(&spec.text, '&');
    let properties = properties(spec);
    let actions = if full {
        vec![
            PlayerAction::AddPlayer {
                name: &name,
                properties: &properties,
            },
            PlayerAction::UpdateListed(true),
            PlayerAction::UpdateDisplayName(Some(&text)),
            PlayerAction::UpdateListOrder(VarInt(spec.order(slot))),
        ]
    } else {
        vec![
            PlayerAction::UpdateDisplayName(Some(&text)),
            PlayerAction::UpdateListOrder(VarInt(spec.order(slot))),
        ]
    };
    let players = [Player {
        uuid: entry_uuid(slot as usize),
        actions: &actions,
    }];
    let flags = if full {
        PlayerInfoFlags::ADD_PLAYER
            | PlayerInfoFlags::UPDATE_LISTED
            | PlayerInfoFlags::UPDATE_DISPLAY_NAME
            | PlayerInfoFlags::UPDATE_LIST_PRIORITY
    } else {
        PlayerInfoFlags::UPDATE_DISPLAY_NAME | PlayerInfoFlags::UPDATE_LIST_PRIORITY
    };
    let packet = CPlayerInfoUpdate::new(flags.bits(), &players);

    let _ = crate::GLOBAL_RUNTIME.block_on(async move {
        player.send_client_packet(&packet).await;
    });
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
    spec.skin
        .iter()
        .map(|skin| Property {
            name: "textures".into(),
            value: skin.value.clone().into(),
            signature: skin.signature.clone().map(Into::into),
        })
        .collect()
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
    }

    // Modern clients (1.19.3+) honour the listed flag; ViaVersion drops it for
    // older protocols, where the team ordering above hides the entries.
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

//! Native artificial tab-list **API** for the Lazarus Pumpkin port.
//!
//! The WASM host cannot serialise `CPlayerInfoUpdate`, so a native companion owns
//! the tab list and exposes an IPC API that the WASM plugin drives.
//!
//! The API is a JSON message sent via `ipc::send_ipc_message("native_tab", bytes)`:
//!
//! ```jsonc
//! // set / create one slot (with optional skin + order)
//! { "op":"set", "viewer": null, "slot": 0, "name": "lz00",
//!   "text": "&aMy line", "order": 0,
//!   "skin": { "value": "<base64>", "signature": "<base64|omitted>" } }
//! // update only the text of a slot (cheap)
//! { "op":"text", "viewer": "uuid-or-null", "slot": 0, "text": "&7x, z" }
//! // remove one slot / all slots
//! { "op":"remove", "viewer": null, "slot": 0 }
//! { "op":"clear",  "viewer": null }
//! ```
//!
//! `viewer: null` applies to everyone (broadcast); a UUID targets a single player
//! (per-player dynamic content, e.g. live coordinates). Slots are 0..80 and each
//! owns a deterministic fake UUID, so updates are per-slot without reloading the
//! whole tab.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pumpkin::entity::player::Player as ServerPlayer;
use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin::server::Server;
use pumpkin_api_macros::{plugin_impl, plugin_method};
use pumpkin_protocol::java::client::play::{
    CPlayerInfoUpdate, CRemovePlayerInfo, Player, PlayerAction, PlayerInfoFlags,
};
use pumpkin_protocol::{Property, VarInt};
use pumpkin_util::text::TextComponent;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;

#[derive(Clone, Deserialize)]
struct Skin {
    value: String,
    signature: Option<String>,
}

#[derive(Clone)]
struct SlotSpec {
    name: String,
    text: String,
    skin: Option<Skin>,
    order: i32,
}

#[derive(Default)]
struct TabState {
    /// Slots shown to everyone (unless a viewer override exists).
    global: HashMap<u8, SlotSpec>,
    /// Per-viewer overrides.
    per_viewer: HashMap<Uuid, HashMap<u8, SlotSpec>>,
}

#[derive(Deserialize)]
#[serde(tag = "op")]
enum Request {
    #[serde(rename = "set")]
    Set {
        viewer: Option<Uuid>,
        slot: u8,
        #[serde(default)]
        name: Option<String>,
        text: String,
        #[serde(default)]
        skin: Option<Skin>,
        #[serde(default)]
        order: Option<i32>,
    },
    #[serde(rename = "text")]
    Text {
        viewer: Option<Uuid>,
        slot: u8,
        text: String,
    },
    #[serde(rename = "remove")]
    Remove { viewer: Option<Uuid>, slot: u8 },
    #[serde(rename = "clear")]
    Clear { viewer: Option<Uuid> },
}

pub struct NativeTab {
    state: Arc<Mutex<TabState>>,
}

impl NativeTab {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TabState::default())),
        }
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        context.log(format!("NativeTab API loaded (Pumpkin API v{PLUGIN_API_VERSION})"));
        let _ = SERVER.set(context.server.clone());

        // Seed the global slots from tab.toml (so it works even without the WASM API).
        if let Some(config) = load_config() {
            let mut state = self.state.lock().unwrap();
            for (slot, text) in column_lines(&config).into_iter().enumerate() {
                state.global.insert(
                    slot as u8,
                    SlotSpec {
                        name: entry_name(slot),
                        text,
                        skin: None,
                        order: slot as i32,
                    },
                );
            }
        }

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

    /// IPC entry point. Receives JSON requests from the WASM plugin.
    #[plugin_method]
    async fn on_ipc_message(&self, _sender: &str, message: &[u8]) -> Result<Vec<u8>, String> {
        let request: Request = serde_json::from_slice(message).map_err(|e| e.to_string())?;
        let server = server_from_context().ok_or("no server")?;
        apply(&server, &self.state, request).await?;
        Ok(Vec::new())
    }
}

#[plugin_impl]
pub struct NativeTab;

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Periodic loop: hides real players and re-sends global slots when the player
/// set changes (new joiners must receive the artificial entries).
fn run(server: Arc<Server>, state: Arc<Mutex<TabState>>, log_ctx: Arc<Context>) {
    let mut known: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

    loop {
        let players = server.get_all_players();
        let current: std::collections::HashSet<Uuid> =
            players.iter().map(|p| p.gameprofile.id).collect();

        hide_real_players(&server);

        if current != known {
            // Re-send every global slot (broadcast) so new joiners get them.
            let globals: Vec<(u8, SlotSpec)> = {
                let state = state.lock().unwrap();
                state.global.iter().map(|(k, v)| (*k, v.clone())).collect()
            };
            for (slot, spec) in globals {
                send_full_broadcast(&server, slot, &spec);
            }
            known = current;
            log_ctx.log(format!(
                "NativeTab: sent {} global slots to {} player(s)",
                state.lock().unwrap().global.len(),
                players.len()
            ));
        }

        std::thread::sleep(Duration::from_secs(1));
    }
}

async fn apply(server: &Arc<Server>, state: &Arc<Mutex<TabState>>, request: Request) -> Result<(), String> {
    match request {
        Request::Set { viewer, slot, name, text, skin, order } => {
            let spec = SlotSpec {
                name: name.unwrap_or_else(|| entry_name(slot as usize)),
                text,
                skin,
                order: order.unwrap_or(slot as i32),
            };
            store(state, viewer, slot, Some(spec.clone()));
            send_full(server, viewer, slot, &spec).await;
        }
        Request::Text { viewer, slot, text } => {
            let spec = {
                let mut guard = state.lock().unwrap();
                if let Some(spec) = effective_mut(&mut guard, viewer, slot) {
                    spec.text = text;
                    Some(spec.clone())
                } else {
                    None
                }
            };
            if let Some(spec) = spec {
                send_text(server, viewer, slot, &spec).await;
            }
        }
        Request::Remove { viewer, slot } => {
            store(state, viewer, slot, None);
            send_remove(server, viewer, &[slot]).await;
        }
        Request::Clear { viewer } => {
            {
                let mut guard = state.lock().unwrap();
                match viewer {
                    Some(uuid) => {
                        guard.per_viewer.remove(&uuid);
                    }
                    None => guard.global.clear(),
                }
            }
            let slots: Vec<u8> = (0..SLOTS as u8).collect();
            send_remove(server, viewer, &slots).await;
        }
    }
    Ok(())
}

fn store(state: &Arc<Mutex<TabState>>, viewer: Option<Uuid>, slot: u8, spec: Option<SlotSpec>) {
    let mut guard = state.lock().unwrap();
    match viewer {
        Some(uuid) => {
            let map = guard.per_viewer.entry(uuid).or_default();
            match spec {
                Some(spec) => {
                    map.insert(slot, spec);
                }
                None => {
                    map.remove(&slot);
                }
            }
        }
        None => match spec {
            Some(spec) => {
                guard.global.insert(slot, spec);
            }
            None => {
                guard.global.remove(&slot);
            }
        },
    }
}

fn effective_mut<'a>(state: &'a mut TabState, viewer: Option<Uuid>, slot: u8) -> Option<&'a mut SlotSpec> {
    if let Some(uuid) = viewer
        && let Some(map) = state.per_viewer.get_mut(&uuid)
        && let Some(spec) = map.get_mut(&slot)
    {
        return Some(spec);
    }
    state.global.get_mut(&slot)
}

/// Recipients for a request: one player (per-viewer) or everyone (global).
fn viewers(server: &Server, viewer: Option<Uuid>) -> Vec<Arc<ServerPlayer>> {
    match viewer {
        Some(uuid) => server
            .get_player_by_uuid(uuid)
            .map(|player| vec![player])
            .unwrap_or_default(),
        None => server.get_all_players(),
    }
}

async fn send_full(server: &Arc<Server>, viewer: Option<Uuid>, slot: u8, spec: &SlotSpec) {
    let flags = PlayerInfoFlags::ADD_PLAYER
        | PlayerInfoFlags::UPDATE_LISTED
        | PlayerInfoFlags::UPDATE_DISPLAY_NAME
        | PlayerInfoFlags::UPDATE_LIST_PRIORITY;

    let text = TextComponent::from_legacy_string_with_code(&spec.text, '&');
    let properties: Vec<Property> = spec
        .skin
        .iter()
        .map(|skin| Property {
            name: "textures".to_string(),
            value: skin.value.clone(),
            signature: skin.signature.clone(),
        })
        .collect();

    let actions = vec![
        PlayerAction::AddPlayer {
            name: &spec.name,
            properties: &properties,
        },
        PlayerAction::UpdateListed(true),
        PlayerAction::UpdateDisplayName(Some(&text)),
        PlayerAction::UpdateListOrder(VarInt(spec.order)),
    ];
    let players = [Player {
        uuid: entry_uuid(slot as usize),
        actions: &actions,
    }];
    let packet = CPlayerInfoUpdate::new(flags.bits(), &players);

    send_packet(server, viewer, &packet).await;
}

async fn send_text(server: &Arc<Server>, viewer: Option<Uuid>, slot: u8, spec: &SlotSpec) {
    let text = TextComponent::from_legacy_string_with_code(&spec.text, '&');
    let actions = vec![PlayerAction::UpdateDisplayName(Some(&text))];
    let players = [Player {
        uuid: entry_uuid(slot as usize),
        actions: &actions,
    }];
    let packet = CPlayerInfoUpdate::new(PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(), &players);

    send_packet(server, viewer, &packet).await;
}

async fn send_remove(server: &Arc<Server>, viewer: Option<Uuid>, slots: &[u8]) {
    let uuids: Vec<Uuid> = slots.iter().map(|slot| entry_uuid(*slot as usize)).collect();
    let packet = CRemovePlayerInfo { players: &uuids };
    send_packet(server, viewer, &packet).await;
}

async fn send_packet<P: pumpkin_protocol::ClientPacket + Sync>(
    server: &Arc<Server>,
    viewer: Option<Uuid>,
    packet: &P,
) {
    match viewer {
        Some(_) => {
            for player in viewers(server, viewer) {
                player.send_client_packet(packet).await;
            }
        }
        None => server.broadcast_packet_all(packet),
    }
}

fn send_full_broadcast(server: &Arc<Server>, slot: u8, spec: &SlotSpec) {
    let flags = PlayerInfoFlags::ADD_PLAYER
        | PlayerInfoFlags::UPDATE_LISTED
        | PlayerInfoFlags::UPDATE_DISPLAY_NAME
        | PlayerInfoFlags::UPDATE_LIST_PRIORITY;

    let text = TextComponent::from_legacy_string_with_code(&spec.text, '&');
    let properties: Vec<Property> = spec
        .skin
        .iter()
        .map(|skin| Property {
            name: "textures".to_string(),
            value: skin.value.clone(),
            signature: skin.signature.clone(),
        })
        .collect();

    let actions = vec![
        PlayerAction::AddPlayer {
            name: &spec.name,
            properties: &properties,
        },
        PlayerAction::UpdateListed(true),
        PlayerAction::UpdateDisplayName(Some(&text)),
        PlayerAction::UpdateListOrder(VarInt(spec.order)),
    ];
    let players = [Player {
        uuid: entry_uuid(slot as usize),
        actions: &actions,
    }];
    server.broadcast_packet_all(&CPlayerInfoUpdate::new(flags.bits(), &players));
}

/// Hides every real player from the tab list (`UpdateListed(false)`).
fn hide_real_players(server: &Arc<Server>) {
    let real = server.get_all_players();
    if real.is_empty() {
        return;
    }
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

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TabConfig {
    enabled: bool,
    left: Vec<String>,
    center: Vec<String>,
    right: Vec<String>,
    far_right: Vec<String>,
}

fn load_config() -> Option<TabConfig> {
    let path = std::path::Path::new("plugins")
        .join("data")
        .join("lazarus")
        .join("tab.toml");
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn column_lines(config: &TabConfig) -> Vec<String> {
    let mut flat: Vec<String> = Vec::with_capacity(SLOTS);
    for column in [&config.left, &config.center, &config.right, &config.far_right] {
        let mut padded = column.clone();
        padded.resize(COLUMN_HEIGHT, String::new());
        flat.extend(padded);
    }
    flat
}

/// The IPC context is not available inside `on_ipc_message`, so we stash the
/// `Server` handle in a global when `on_load` runs.
static SERVER: std::sync::OnceLock<Arc<Server>> = std::sync::OnceLock::new();

fn server_from_context() -> Option<Arc<Server>> {
    SERVER.get().cloned()
}

pub fn set_server(server: Arc<Server>) {
    let _ = SERVER.set(server);
}

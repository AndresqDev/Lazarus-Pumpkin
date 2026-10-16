//! Native artificial tab-list for the Lazarus Pumpkin port.
//!
//! Pumpkin's WebAssembly host does **not** serialise `CPlayerInfoUpdate`, so the
//! classic artificial tab (80 fake slots laid out in 4 columns) cannot be built
//! from a `.wasm` plugin. A native plugin can send the packet directly.
//!
//! Behaviour (matches the original `tab.yml` / `PlayerTab_1_8`):
//! * every real player is hidden from the tab list (`UpdateListed(false)`);
//! * 80 fake, skin-less player entries are added;
//! * their display names are the four 20-line columns from `tab.toml`
//!   (LEFT / CENTER / RIGHT / FAR_RIGHT), i.e. purely artificial text.
//!
//! No faction/userdata is used. Only the generic `<online>` placeholder is
//! supported (server-wide, so the packet can be broadcast to everyone).
//!
//! Build via the repo's GitHub Actions workflow (both aarch64 and x86_64).

use std::sync::Arc;
use std::time::Duration;

use pumpkin::entity::player::Player as ServerPlayer;
use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin::server::Server;
use pumpkin_api_macros::{plugin_impl, plugin_method};
use pumpkin_protocol::java::client::play::{
    CPlayerInfoUpdate, Player, PlayerAction, PlayerInfoFlags,
};
use pumpkin_protocol::{Property, VarInt};
use pumpkin_util::text::TextComponent;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

const COLUMN_HEIGHT: usize = 20;
const SLOTS: usize = 80;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TabConfig {
    enabled: bool,
    left: Vec<String>,
    center: Vec<String>,
    right: Vec<String>,
    far_right: Vec<String>,
}

#[plugin_impl]
pub struct NativeTab;

impl NativeTab {
    pub fn new() -> Self {
        Self
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        info!("NativeTab loaded (Pumpkin API v{PLUGIN_API_VERSION})");

        let config = load_config(&context.get_data_folder());
        if !config.enabled {
            info!("NativeTab: [tab] disabled in config");
            return Ok(());
        }

        let server = context.server.clone();
        tokio::spawn(async move {
            run(server, config).await;
        });

        Ok(())
    }

    #[plugin_method]
    async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
        info!("NativeTab unloaded");
        Ok(())
    }
}

async fn run(server: Arc<Server>, config: TabConfig) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut added = false;
    let mut last: Vec<String> = vec![String::new(); SLOTS];

    loop {
        interval.tick().await;

        let lines = build_lines(&config, &server);

        if !added {
            add_entries(&server, &lines);
            added = true;
        } else {
            update_names(&server, &lines, &last);
        }
        last = lines;

        // Real players must not show up on the artificial tab.
        hide_real_players(&server);
    }
}

fn load_config(folder: &std::path::Path) -> TabConfig {
    let path = folder.join("tab.toml");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|contents| toml::from_str(&contents).ok())
        .unwrap_or_default()
}

/// Builds the 80 artificial lines (4 columns of 20).
fn build_lines(config: &TabConfig, server: &Server) -> Vec<String> {
    let mut flat: Vec<String> = Vec::with_capacity(SLOTS);
    for column in [&config.left, &config.center, &config.right, &config.far_right] {
        let mut padded = column.clone();
        padded.resize(COLUMN_HEIGHT, String::new());
        flat.extend(padded);
    }

    let online = server.get_all_players().len();
    for line in flat.iter_mut() {
        *line = line.replace("<online>", &online.to_string());
    }
    flat
}

fn entry_uuid(index: usize) -> Uuid {
    Uuid::from_u64_pair(
        0x1A2B_0000_0000_0000u64 | (index as u64 & 0xFFFF),
        0xDEAD_BEEF_0000_0000u64 | index as u64,
    )
}

fn entry_name(index: usize) -> String {
    format!("lz{index:02}")
}

/// Sends the initial add packet with all 80 artificial entries.
fn add_entries(server: &Server, lines: &[String]) {
    let flags = PlayerInfoFlags::ADD_PLAYER
        | PlayerInfoFlags::UPDATE_LISTED
        | PlayerInfoFlags::UPDATE_DISPLAY_NAME
        | PlayerInfoFlags::UPDATE_LIST_PRIORITY;

    let names: Vec<String> = (0..SLOTS).map(entry_name).collect();
    let texts: Vec<TextComponent> = lines
        .iter()
        .map(|line| TextComponent::text(line.clone()))
        .collect();
    let properties: Vec<Property> = Vec::new();

    // Actions must be in canonical flag order.
    let actions: Vec<Vec<PlayerAction>> = (0..SLOTS)
        .map(|i| {
            vec![
                PlayerAction::AddPlayer {
                    name: &names[i],
                    properties: &properties,
                },
                PlayerAction::UpdateListed(true),
                PlayerAction::UpdateDisplayName(Some(&texts[i])),
                PlayerAction::UpdateListOrder(VarInt(i as i32)),
            ]
        })
        .collect();

    let players: Vec<Player> = (0..SLOTS)
        .map(|i| Player {
            uuid: entry_uuid(i),
            actions: &actions[i],
        })
        .collect();

    server.broadcast_packet_all(&CPlayerInfoUpdate::new(flags.bits(), &players));
}

/// Sends display-name updates only for the lines that changed.
fn update_names(server: &Server, lines: &[String], last: &[String]) {
    let changed: Vec<usize> = (0..SLOTS)
        .filter(|&i| lines.get(i) != last.get(i))
        .collect();
    if changed.is_empty() {
        return;
    }

    let texts: Vec<TextComponent> = changed
        .iter()
        .map(|&i| TextComponent::text(lines[i].clone()))
        .collect();
    let actions: Vec<Vec<PlayerAction>> = (0..changed.len())
        .map(|k| vec![PlayerAction::UpdateDisplayName(Some(&texts[k]))])
        .collect();
    let players: Vec<Player> = changed
        .iter()
        .enumerate()
        .map(|(k, &i)| Player {
            uuid: entry_uuid(i),
            actions: &actions[k],
        })
        .collect();

    server.broadcast_packet_all(&CPlayerInfoUpdate::new(
        PlayerInfoFlags::UPDATE_DISPLAY_NAME.bits(),
        &players,
    ));
}

/// Hides every real player from the tab list (`UpdateListed(false)`).
fn hide_real_players(server: &Server) {
    let real: Vec<Arc<ServerPlayer>> = server.get_all_players();
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

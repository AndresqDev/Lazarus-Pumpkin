//! Native **potion bridge** for the Lazarus Pumpkin port.
//!
//! Pumpkin's WASM API has no typed way to set an item's `potion-contents` data
//! component (the only setter, `set-component`, expects the network codec's
//! binary form, which the guest cannot produce). This native companion fills
//! exactly that gap: it takes the item already placed in a player's inventory and
//! patches its `PotionContents` component.
//!
//! The WASM plugin drives it by writing a JSON request file:
//!
//! `<server>/plugins/data/lazarus/potion_requests.json`
//! ```jsonc
//! {
//!   "requests": [
//!     { "player": "<uuid>", "slot": 36, "potion": "strong_healing" }
//!   ]
//! }
//! ```
//!
//! After applying every request the native clears the file (`requests: []`).

use std::sync::Arc;
use std::time::Duration;

use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin::server::Server;
use pumpkin_api_macros::{plugin_impl, plugin_method};
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::PotionContentsImpl;
use pumpkin_data::potion::Potion;
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

const REQUEST_FILE: &str = "plugins/data/lazarus/potion_requests.json";

#[derive(Deserialize)]
struct Request {
    player: String,
    slot: usize,
    potion: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct FileRequests {
    requests: Vec<Request>,
}

impl NativePotions {
    pub fn new() -> Self {
        NativePotions
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        context.log(format!("NativePotions bridge loaded (Pumpkin API v{PLUGIN_API_VERSION})"));

        let server = context.server.clone();
        let log_ctx = context.clone();
        std::thread::spawn(move || run(server, log_ctx));

        Ok(())
    }

    #[plugin_method]
    async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
        info!("NativePotions bridge unloaded");
        Ok(())
    }
}

#[plugin_impl]
pub struct NativePotions;

// ---------------------------------------------------------------------------
// Loop
// ---------------------------------------------------------------------------

fn run(server: Arc<Server>, log_ctx: Arc<Context>) {
    let mut last_signature: Option<(u64, u64)> = None;

    loop {
        if let Some(signature) = file_signature()
            && last_signature != Some(signature)
        {
            last_signature = Some(signature);
            if let Some(count) = apply(&server) {
                log_ctx.log(format!("NativePotions: applied {count} potion request(s)"));
            }
        }

        std::thread::sleep(Duration::from_millis(300));
    }
}

fn file_signature() -> Option<(u64, u64)> {
    let meta = std::fs::metadata(REQUEST_FILE).ok()?;
    let modified = meta.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((duration.as_secs(), meta.len()))
}

fn read_requests() -> Option<FileRequests> {
    let contents = std::fs::read_to_string(REQUEST_FILE).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Applies every request; returns how many were applied (and clears the file).
fn apply(server: &Arc<Server>) -> Option<usize> {
    let file = read_requests()?;
    if file.requests.is_empty() {
        return None;
    }

    let mut applied = 0usize;
    for request in &file.requests {
        let Ok(uuid) = Uuid::parse_str(&request.player) else {
            continue;
        };
        let Some(player) = server.get_player_by_uuid(uuid) else {
            continue;
        };

        let name = request
            .potion
            .strip_prefix("minecraft:")
            .unwrap_or(&request.potion);
        let Some(potion) = Potion::from_name(name) else {
            continue;
        };

        let mut stack = player.inventory().get_slot(request.slot);
        if stack.item_count == 0 {
            continue;
        }

        // Replace any existing potion component and add ours.
        stack
            .patch
            .retain(|(component, _)| *component != DataComponent::PotionContents);
        stack.patch.push((
            DataComponent::PotionContents,
            Some(Box::new(PotionContentsImpl {
                potion_id: Some(i32::from(potion.id)),
                custom_color: None,
                custom_effects: Vec::new(),
                custom_name: None,
            })),
        ));

        player.inventory().set_slot(request.slot, stack);
        applied += 1;
    }

    // Clear the processed requests.
    let _ = std::fs::write(REQUEST_FILE, "{\"requests\":[]}");

    Some(applied)
}

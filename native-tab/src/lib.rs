//! Native tab-list companion for the Lazarus Pumpkin port.
//!
//! Pumpkin's WebAssembly host does **not** serialise `CPlayerInfoUpdate`
//! (only `CTabList`, i.e. header/footer), so the classic 80-slot / 4-column tab
//! list cannot be reproduced from a `.wasm` plugin. A **native** plugin, however,
//! gets `Arc<Server>` and can send arbitrary clientbound packets.
//!
//! This is the scaffold: it loads, logs the online players and unloads. The tab
//! rendering (80 fake `PlayerInfoUpdate` entries per viewer, fed from the WASM
//! plugin's JSON files) is implemented on top of `context.server`.
//!
//! Build (Linux x86_64) -> `target/release/libnative_tab.so`, copy it into the
//! server's `plugins/` folder as `native_tab.so`.
//!
//! Keep `[tab] enabled = false` in the WASM plugin so both don't fight over the
//! tab list.

use std::sync::Arc;

use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin_api_macros::{plugin_impl, plugin_method};
use tracing::info;

#[plugin_impl]
pub struct NativeTab;

impl NativeTab {
    pub fn new() -> Self {
        Self
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        info!("NativeTab loaded (Pumpkin API v{PLUGIN_API_VERSION})");

        let players = context.server.get_all_players();
        info!("NativeTab: {} player(s) online", players.len());

        // TODO: render the 80-slot / 4-column tab list.
        //   1. Read `plugins/data/lazarus/factions.json` and `userdata.json`.
        //   2. Build 80 lines from the tab.yml columns (LEFT/CENTER/RIGHT/FAR_RIGHT)
        //      plus the FactionInfo / PlayerList module overrides.
        //   3. For each viewer send `CPlayerInfoUpdate` (ADD_PLAYER |
        //      UPDATE_LISTED | UPDATE_DISPLAY_NAME) and update changed names on a
        //      1s task. Use `context.server` -> player client to send the packets.
        Ok(())
    }

    #[plugin_method]
    async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
        info!("NativeTab unloaded");
        Ok(())
    }
}

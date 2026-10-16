//! Native **potion component factory** for the Lazarus Pumpkin port.
//!
//! Pumpkin's WASM plugin API cannot build typed server data components: the only
//! setter, `item.set-component`, takes the network codec's binary form
//! (`list<u8>`), which a guest cannot produce — and any `Box<dyn DataComponentImpl>`
//! created *outside* the server binary makes the server panic when it serializes
//! the stack, because the `TypeId` of the component does not match the server's
//! own `pumpkin-data` copy (two independently compiled crate instances).
//!
//! This native companion does the heavy lifting instead. It runs in-process as a
//! regular Pumpkin native plugin (no server patches, no server rebuild): at start
//! it builds the `potion-contents` component for **every** potion in the registry,
//! serializes it with the exact network codec
//! (`pumpkin_protocol::codec::data_component`) and writes a lookup table next to
//! the Lazarus WASM plugin:
//!
//! `plugins/data/lazarus/potion_components.json`
//! ```jsonc
//! {
//!   "version": 1,
//!   "lzp": "eW9zb3llbHB1Z2lu",          // base64 of the binary code `yosoyelplugin`
//!   "potion_components": {
//!     "strong_healing": "<base64 of the component's network bytes>",
//!     ...
//!   }
//! }
//! ```
//!
//! The WASM plugin then reads that table and calls
//! `item.set-component(potion-contents, <bytes>)`; the host deserializes the bytes
//! with **server** types, so the resulting item is a real potion everywhere
//! (inventory, sync packets, drinking). Only Pumpkin's built-in `set-component`
//! path is used — the server binary stays untouched.
//!
//! The table is rewritten on every start, so it always matches the Pumpkin
//! revision that produced it. Build this `.so` from the same Pumpkin tag as the
//! server binary that loads it (the byte layout is stable inside a tag).
//!
//! Keep the constants below in sync with:
//! - `src/kits.rs` (`POTION_MARKER_B64`, marker writer),
//! - `src/potion_components.rs` (marker reader + table consumer).

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use pumpkin::plugin::{Context, PLUGIN_API_VERSION};
use pumpkin_api_macros::{plugin_impl, plugin_method};
use pumpkin_data::data_component::DataComponent;
use pumpkin_data::data_component_impl::{PotionContentsImpl, get as get_component};
use pumpkin_data::potion::Potion;
use pumpkin_protocol::codec::data_component::{deserialize, serialize};
use serde::Serialize;

/// base64 of the binary code `yosoyelplugin` that identifies Lazarus items.
const MARKER_CODE_B64: &str = "eW9zb3llbHB1Z2lu";

/// Table format version understood by `src/potion_components.rs`.
const FORMAT_VERSION: u32 = 1;

/// Output path, relative to the server working directory. Must match the WASM
/// plugin's data folder (`<server>/plugins/data/lazarus`).
const OUTPUT_FILE: &str = "plugins/data/lazarus/potion_components.json";

/// The JSON table written for the WASM plugin.
#[derive(Serialize)]
struct ComponentTable {
    version: u32,
    lzp: &'static str,
    potion_components: BTreeMap<String, String>,
}

#[plugin_impl]
pub struct NativePotions;

impl NativePotions {
    pub fn new() -> Self {
        NativePotions
    }

    #[plugin_method]
    async fn on_load(&self, context: Arc<Context>) -> Result<(), String> {
        context.log(format!(
            "native-potions: component factory loaded (Pumpkin API v{PLUGIN_API_VERSION})"
        ));

        let table = build_table()?;
        let potions = table.potion_components.len();
        let json = serde_json::to_string_pretty(&table)
            .map_err(|error| format!("failed to serialize the component table: {error}"))?;

        if let Some(parent) = Path::new(OUTPUT_FILE).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        std::fs::write(OUTPUT_FILE, json)
            .map_err(|error| format!("{OUTPUT_FILE}: {error}"))?;

        context.log(format!(
            "native-potions: wrote {potions} potion components to {OUTPUT_FILE}"
        ));
        Ok(())
    }

    #[plugin_method]
    async fn on_unload(&self, _context: Arc<Context>) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Table generation
// ---------------------------------------------------------------------------

/// Builds the `potion name -> base64(network bytes)` table for every registered
/// potion. Each entry is round-tripped through the codec as a self-check.
fn build_table() -> Result<ComponentTable, String> {
    let mut potion_components = BTreeMap::new();

    // `from_id` is dense over the registry ids (0..=45 in 26.3). Scanning the
    // whole `u8` range picks up new potions automatically after a tag bump.
    for id in 0..=u8::MAX {
        let Some(potion) = Potion::from_id(id) else {
            continue;
        };

        let contents = PotionContentsImpl {
            potion_id: Some(i32::from(potion.id)),
            custom_color: None,
            custom_effects: Vec::new(),
            custom_name: None,
        };

        let bytes = encode_contents(&contents)
            .map_err(|error| format!("{}: {error}", potion.name))?;

        // Self-check: the same codec the server uses must decode what we encoded.
        let mut cursor = Cursor::new(bytes.clone());
        let decoded = deserialize(DataComponent::PotionContents, &mut cursor)
            .map_err(|error| format!("{}: {error}", potion.name))?;
        if *get_component::<PotionContentsImpl>(decoded.as_ref()) != contents {
            return Err(format!("{}: codec round-trip mismatch", potion.name));
        }

        potion_components.insert(potion.name.to_string(), BASE64_STANDARD.encode(&bytes));
    }

    if potion_components.is_empty() {
        return Err("the potion registry reported no potions".to_string());
    }

    Ok(ComponentTable {
        version: FORMAT_VERSION,
        lzp: MARKER_CODE_B64,
        potion_components,
    })
}

/// Serializes one `potion-contents` component exactly like the host does when it
/// serializes items for the wire (`pumpkin_protocol::codec::data_component`).
fn encode_contents(contents: &PotionContentsImpl) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    serialize(DataComponent::PotionContents, contents, &mut bytes)
        .map_err(|error| format!("serialize: {error}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_covers_the_registry_and_round_trips() {
        let table = build_table().expect("the table must build");

        assert_eq!(table.version, FORMAT_VERSION);
        assert_eq!(table.lzp, MARKER_CODE_B64);
        assert!(
            table.potion_components.len() >= 46,
            "expected the full registry, got {} entries",
            table.potion_components.len()
        );

        // Spot-check ids the kits use, plus decode + round-trip each of them.
        for name in ["strong_healing", "strong_swiftness", "water", "strong_strength"] {
            let encoded = table
                .potion_components
                .get(name)
                .unwrap_or_else(|| panic!("missing {name}"));

            let bytes = BASE64_STANDARD.decode(encoded).expect("valid base64");
            let mut cursor = Cursor::new(bytes);
            let decoded = deserialize(DataComponent::PotionContents, &mut cursor)
                .expect("the host codec must accept our bytes");
            let decoded = get_component::<PotionContentsImpl>(decoded.as_ref());

            assert_eq!(
                decoded.potion_id,
                Potion::from_name(name).map(|potion| i32::from(potion.id))
            );
            assert!(decoded.custom_color.is_none());
            assert!(decoded.custom_effects.is_empty());
            assert!(decoded.custom_name.is_none());
        }
    }

    #[test]
    fn table_serializes_to_the_documented_shape() {
        let table = build_table().expect("the table must build");
        let value = serde_json::to_value(&table).expect("serializable");

        assert_eq!(value["version"], FORMAT_VERSION);
        assert_eq!(value["lzp"], MARKER_CODE_B64);
        assert!(value["potion_components"]["strong_healing"].is_string());
    }
}

//! Applies the potion-contents component produced by the native `native_potions`
//! companion whenever an item carries the Lazarus delivery marker.
//!
//! Flow (all *before* the player ever receives the item):
//!
//! 1. A give site (e.g. [`crate::kits`]) stamps the item's lore with the marker
//!    line `{"text":"{\"lzp\":\"eW9zb3llbHBsdWdpbg==\",\"potion\":\"<id>\"}"}`.
//! 2. [`apply_marker`] detects the marker in the item's own lore, looks the
//!    potion up in the native table and calls `item.set-component`, which the
//!    host deserializes with **server** types — so the item is a real potion
//!    anywhere (inventory, sync packets, drinking). The marker line is removed.
//! 3. The caller gives the (already converted) item to the player.
//!
//! The table lives at `<data_folder>/potion_components.json` and is written on
//! every server start by the `native-potions` `.so` (see `<repo>/native-potions`).
//! It contains the network-codec bytes for every registered potion — data a WASM
//! guest cannot produce, and the components themselves cannot be built by the
//! guest without crashing server-side serialization (`TypeId` mismatch).
//!
//! Degraded mode: if the `.so` (or its table) is missing, the item is given
//! untouched, with the marker line still visible, and a warning is logged.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use pumpkin_plugin_api::data_components::DataComponent;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::ItemStack;

/// Binary marker code embedded (base64) in the marker's `lzp` field. This is the
/// single source of truth: the writer ([`crate::kits`]) embeds
/// [`marker_code_b64`] and the reader validates that `lzp` decodes to exactly
/// these bytes. Keep in sync with `native-potions/src/lib.rs`
/// (`MARKER_CODE_B64`, informational field of the table).
pub const MARKER_CODE: &[u8] = b"yosoyelplugin";

/// Table format version understood by this module.
const FORMAT_VERSION: u32 = 1;

static DATA_FOLDER: OnceLock<String> = OnceLock::new();
static TABLE: Mutex<Option<Table>> = Mutex::new(None);

struct Table {
    components: HashMap<String, Vec<u8>>,
}

/// Stores the plugin's data folder so the native table can be found later.
/// Called once from `on_load`.
pub fn init(data_folder: &str) {
    let _ = DATA_FOLDER.set(data_folder.to_string());
}

/// Detects the Lazarus marker in `item`'s lore and, when present, replaces it
/// with the real potion component from the native table. Returns `true` when
/// the item was converted. Give sites must call this right before handing the
/// item to a player.
pub fn apply_marker(item: &ItemStack) -> bool {
    let lore = item.get_lore();
    let Some((index, potion)) = find_marker(&lore) else {
        return false;
    };

    let Some(bytes) = component_bytes(&potion) else {
        tracing::warn!(
            "lazarus: no native potion table entry for `{potion}`; \
             the item is given untouched (is native_potions installed?)"
        );
        return false;
    };

    item.set_component(DataComponent::PotionContents, &bytes);

    // Strip the marker so the player never sees the protocol line.
    let mut kept = lore;
    kept.remove(index);
    if kept.is_empty() {
        item.remove_component(DataComponent::Lore);
    } else {
        item.set_lore(kept);
    }

    tracing::debug!("lazarus: marker converted the item into `{potion}`");
    true
}

/// Finds the first lore line that carries the marker and returns its index plus
/// the potion id it points at.
fn find_marker(lines: &[TextComponent]) -> Option<(usize, String)> {
    for (index, line) in lines.iter().enumerate() {
        // The marker line is a plain text component; accept it regardless of
        // how the text component was normalised on the round-trip.
        let json = line.to_json();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else {
            continue;
        };
        let Some(marker_text) = value.get("text").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Ok(marker) = serde_json::from_str::<serde_json::Value>(marker_text) else {
            continue;
        };

        // The binary code must decode to exactly `yosoyelplugin`.
        let Some(code) = marker.get("lzp").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if decode_base64(code).as_deref() != Some(MARKER_CODE) {
            continue;
        }

        let Some(potion) = marker.get("potion").and_then(serde_json::Value::as_str) else {
            continue;
        };
        return Some((index, potion.to_owned()));
    }
    None
}

/// Looks a potion up in the native table, (re)loading it on demand. The native
/// plugin writes the file at server start, but plugins may load in any order and
/// the `.so` may be added later, so a missing file is retried on the next call.
fn component_bytes(potion: &str) -> Option<Vec<u8>> {
    {
        let guard = TABLE.lock().expect("potion table mutex poisoned");
        if let Some(table) = guard.as_ref() {
            return table.components.get(potion).cloned();
        }
    }

    let table = match load_table() {
        Ok(table) => table,
        Err(error) => {
            tracing::warn!("lazarus: {error}");
            return None;
        }
    };
    let bytes = table.components.get(potion).cloned();
    *TABLE.lock().expect("potion table mutex poisoned") = Some(table);
    bytes
}

fn load_table() -> Result<Table, String> {
    let folder = DATA_FOLDER
        .get()
        .ok_or("potion_components::init() was never called")?;
    let path = format!("{folder}/potion_components.json");

    let raw = std::fs::read_to_string(&path).map_err(|error| {
        format!("cannot read the native potion table ({path}): {error}")
    })?;
    let document: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("invalid native potion table ({path}): {error}"))?;

    let version = document
        .get("version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("native potion table ({path}) has no version"))?;
    if version != u64::from(FORMAT_VERSION) {
        return Err(format!(
            "native potion table ({path}) version {version} is not supported \
             (expected {FORMAT_VERSION}); rebuild native-potions for this server"
        ));
    }

    let entries = document
        .get("potion_components")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("native potion table ({path}) has no potion_components"))?;

    let mut components = HashMap::with_capacity(entries.len());
    for (name, value) in entries {
        let Some(encoded) = value.as_str() else {
            continue;
        };
        let Some(bytes) = decode_base64(encoded) else {
            tracing::warn!("lazarus: skipping malformed entry `{name}` in {path}");
            continue;
        };
        components.insert(name.clone(), bytes);
    }

    if components.is_empty() {
        return Err(format!("native potion table ({path}) is empty"));
    }
    tracing::info!(
        "lazarus: loaded {} potion components from the native table",
        components.len()
    );
    Ok(Table { components })
}

/// base64 of [`MARKER_CODE`], as embedded in the marker line by the writer.
pub fn marker_code_b64() -> String {
    encode_base64(MARKER_CODE)
}

/// Minimal standard-alphabet base64 encoder with `=` padding.
fn encode_base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();

    for chunk in input.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = chunk.get(1).copied().map_or(0, u32::from);
        let third = chunk.get(2).copied().map_or(0, u32::from);
        let triple = (first << 16) | (second << 8) | third;

        output.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        output.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            output.push('=');
        }
    }

    output
}

/// Minimal standard-alphabet base64 decoder (accepts and ignores `=` padding —
/// kept dependency-free).
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    let mut output = Vec::new();

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_base64_round_trips_to_the_binary_code() {
        let encoded = marker_code_b64();
        // Pinned value: base64 of b"yosoyelplugin".
        assert_eq!(encoded, "eW9zb3llbHBsdWdpbg==");
        assert_eq!(decode_base64(&encoded).as_deref(), Some(MARKER_CODE));
    }

    #[test]
    fn rejects_foreign_codes() {
        assert_ne!(
            decode_base64("AAAAAAAAAAAAAAAA").as_deref(),
            Some(MARKER_CODE)
        );
        assert_eq!(decode_base64("not base64!!"), None);
        // The historical typo (`yosoyelpugin`) must not match.
        assert_ne!(
            decode_base64("eW9zb3llbHB1Z2lu").as_deref(),
            Some(MARKER_CODE)
        );
    }
}

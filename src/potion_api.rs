//! Potion bridge API for the native `native_potions` companion.
//!
//! The WASM host cannot set an item's `potion-contents` data component (the only
//! setter expects the network codec's binary form). We therefore hand the item off
//! to the native companion by writing a request file; the native patches the potion
//! component on the item already in that inventory slot.
//!
//! Request file: `<data_folder>/potion_requests.json`
//! ```jsonc
//! { "requests": [ { "player": "<uuid>", "slot": 36, "potion": "strong_healing" } ] }
//! ```

use serde_json::{Value, json};

/// Appends a potion request (read-modify-write; the native clears it after applying).
pub fn request(data_folder: &str, player: &str, slot: u8, potion: &str) {
    let path = format!("{data_folder}/potion_requests.json");

    let mut requests: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
        .and_then(|value| value.get("requests").and_then(Value::as_array).cloned())
        .unwrap_or_default();

    requests.push(json!({ "player": player, "slot": slot, "potion": potion }));

    let document = json!({ "requests": requests });
    let _ = std::fs::write(path, document.to_string());
}

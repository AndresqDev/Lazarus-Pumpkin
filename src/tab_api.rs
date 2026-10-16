//! Client helpers for the native `native_tab` IPC API.
//!
//! The native companion plugin owns the tab list (the WASM host can't serialise
//! `CPlayerInfoUpdate`) and exposes these JSON operations. `viewer = null` affects
//! everyone; a UUID targets one player (per-player dynamic content).

use serde_json::json;

/// Name of the native companion plugin.
pub const NATIVE_TAB: &str = "native_tab";

fn send(request: serde_json::Value) {
    let payload = request.to_string().into_bytes();
    let _ = pumpkin_plugin_api::ipc::send_ipc_message(NATIVE_TAB, &payload);
}

/// Create/replace a slot for everyone, optionally with a profile name and order.
pub fn set_slot(slot: u8, text: &str, name: Option<&str>, order: Option<i32>) {
    send(json!({
        "op": "set",
        "viewer": null,
        "slot": slot,
        "text": text,
        "name": name,
        "order": order,
    }));
}

/// Replace a slot's text for everyone (cheap; no skin/re-add).
pub fn set_text(slot: u8, text: &str) {
    send(json!({ "op": "text", "viewer": null, "slot": slot, "text": text }));
}

/// Replace a slot's text for a single player.
pub fn set_viewer_text(viewer: &str, slot: u8, text: &str) {
    send(json!({ "op": "text", "viewer": viewer, "slot": slot, "text": text }));
}

/// Create/replace a slot for a single player, optionally with a head/skin.
///
/// `skin_value`/`skin_signature` are the base64 `textures` GameProfile property
/// (the same blob used by player heads). Passing `skin_value = None` keeps no skin.
pub fn set_viewer_slot(
    viewer: &str,
    slot: u8,
    text: &str,
    name: Option<&str>,
    skin_value: Option<&str>,
    skin_signature: Option<&str>,
) {
    let skin = skin_value.map(|value| json!({ "value": value, "signature": skin_signature }));
    send(json!({
        "op": "set",
        "viewer": viewer,
        "slot": slot,
        "text": text,
        "name": name,
        "skin": skin,
    }));
}

/// Remove a slot for everyone.
pub fn remove_slot(slot: u8) {
    send(json!({ "op": "remove", "viewer": null, "slot": slot }));
}

/// Remove all artificial slots for everyone.
pub fn clear() {
    send(json!({ "op": "clear", "viewer": null }));
}

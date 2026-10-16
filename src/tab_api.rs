//! Shared tab-state model + writer for the native `native_tab` API.
//!
//! The WASM plugin writes `plugins/data/lazarus/tab_state.json`, which the native
//! companion diffs against what it already sent and applies per-slot.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct Skin {
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Serialize, Clone, Default, PartialEq)]
pub struct SlotSpec {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin: Option<Skin>,
}

impl SlotSpec {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Default::default()
        }
    }
}

#[derive(Serialize, Default)]
pub struct TabState {
    /// slot (as string) -> spec (shown to everyone)
    pub global: BTreeMap<String, SlotSpec>,
    /// player uuid -> (slot -> spec) per-player overrides
    pub viewers: BTreeMap<String, BTreeMap<String, SlotSpec>>,
}

/// Writes the state to `<data_folder>/tab_state.json`.
pub fn write(data_folder: &str, state: &TabState) {
    if let Ok(json) = serde_json::to_string(state) {
        let _ = std::fs::write(format!("{data_folder}/tab_state.json"), json);
    }
}

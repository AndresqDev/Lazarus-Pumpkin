//! Holograms — port of `me.qiooip.lazarus.hologram.HologramManager`.
//!
//! The original built armor-stand packets through NMS. Modern Minecraft has
//! dedicated `TextDisplay` entities, which Pumpkin can spawn and customise, so we
//! stack one `TextDisplay` per line.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use pumpkin_plugin_api::display::BillboardMode;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Entity, EntityDisplayExt, EntityType, Server};

use crate::util;

/// A persisted hologram.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hologram {
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub lines: Vec<String>,
}

/// Spawns the display entities for a hologram and returns their handles.
pub fn spawn(server: &Server, holo: &Hologram) -> Vec<Entity> {
    let Some(world) = server.get_world_by_name(&holo.world) else {
        return Vec::new();
    };

    let mut entities = Vec::with_capacity(holo.lines.len());

    for (index, line) in holo.lines.iter().enumerate() {
        let y = holo.y - (index as f64 * 0.25);
        let entity = world.spawn_entity(EntityType::TextDisplay, (holo.x, y, holo.z));

        if let Some(display) = entity.as_text_display() {
            display.set_text(util::text(line));
            display.get_display().set_billboard(BillboardMode::Center);
            display.set_see_through(false);
        }

        entities.push(entity);
    }

    entities
}

/// Removes the display entities of a hologram.
pub fn despawn(entities: Vec<Entity>) {
    for entity in entities {
        entity.remove();
    }
}

/// Removes and respawns every hologram (used after a restart, once worlds load).
pub fn respawn_all(
    server: &Server,
    holograms: &BTreeMap<String, Hologram>,
    entities: &mut BTreeMap<String, Vec<Entity>>,
) {
    for (name, holo) in holograms {
        if let Some(old) = entities.remove(name) {
            despawn(old);
        }
        let spawned = spawn(server, holo);
        entities.insert(name.clone(), spawned);
    }
}

/// A display line helper used by leaderboard holograms.
pub fn line(text: impl AsRef<str>) -> TextComponent {
    util::text(text)
}

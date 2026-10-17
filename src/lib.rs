//! Lazarus HCF core — Pumpkin (WASM) port.
//!
//! The original plugin targeted Bukkit/Spigot 1.7–1.8 with NMS packet access.
//! Pumpkin plugins are WebAssembly components written in Rust, so this is a
//! *rewrite* of the same feature set against Pumpkin's plugin API rather than a
//! binary-compatible port. See `PORTING.md` for the module-by-module mapping,
//! what was omitted (knockback + LunarClient, as requested) and the roadmap.
//!
//! Build with `cargo build --release` and drop the produced
//! `target/wasm32-wasip2/release/lazarus_pumpkin.wasm` into the server's
//! `plugins/` folder.

// A cdylib exposes no Rust API, so rustc cannot see that the `pub` items below
// are used across modules. Silence the resulting false-positive lints.
#![allow(dead_code, unused_imports)]

mod classes;
mod commands;
mod config;
mod core;
mod deathban;
mod factions;
mod glass;
mod hologram;
mod kits;
mod lang;
mod listeners;
mod potion_components;
mod scoreboard;
mod selection;
mod sotw;
mod staffmode;
mod state;
mod tab;
mod tab_api;
mod tasks;
mod timers;
mod userdata;
mod util;
mod yaml;

use pumpkin_plugin_api::permissions;
use pumpkin_plugin_api::{Context, Plugin, PluginMetadata};

pub struct LazarusPlugin;

impl Plugin for LazarusPlugin {
    fn new() -> Self {
        LazarusPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "lazarus".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["qIooIp (original)".into(), "Pumpkin port".into()],
            description: "Advanced HCF core (factions, claims, timers, DTR).".into(),
            dependencies: Vec::new(),
            permissions: vec![
                permissions::FS_READ_DATA.into(),
                permissions::FS_WRITE_DATA.into(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> pumpkin_plugin_api::Result<()> {
        // The plugin's private data folder is our persistence + config location.
        let data_folder = context.get_data_folder();
        state::init(data_folder.clone())?;
        potion_components::init(&data_folder);

        // Cache world spawns before anything else so the warzone is centred correctly.
        let server = context.get_server();
        core::refresh_spawn_centers(&server);
        log_spawn_diagnostics(&server);

        commands::register(&context)?;
        listeners::register(&context)?;
        tasks::start(&context);

        // KitmapHandler constructor: clear every PvP protection on startup so
        // nobody keeps a timer when kitmap mode is on.
        {
            let mut guard = state::state().write().unwrap();
            if guard.config.kitmap.enabled {
                guard.timers.clear_pvp_protections();
                tracing::info!("kitmap mode is enabled: PvP protections cleared");
            }
        }

        tracing::info!("Lazarus HCF core enabled (Pumpkin/WASM port).");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        state::save_all();
        tracing::info!("Lazarus HCF core disabled; data saved.");
        Ok(())
    }
}

/// Logs how the join/respawn spawn resolves, so admins can copy the exact
/// coordinates into `utilities.yml` (`WORLD_SPAWN`, `world|x|y|z|yaw|pitch`).
///
/// With an empty/unresolvable `WORLD_SPAWN` the port falls back to each world's
/// own spawn point (like the original's `player.getWorld().getSpawnLocation()`);
/// this prints those coordinates plus the top block Y as a ready-to-use value.
fn log_spawn_diagnostics(server: &pumpkin_plugin_api::Server) {
    let raw = {
        let guard = state::state().read().unwrap();
        guard.raw.utilities.string("WORLD_SPAWN")
    };

    if let Some((world, (x, y, z), yaw, pitch)) = core::configured_spawn(server) {
        tracing::info!(
            "spawn: WORLD_SPAWN=\"{raw}\" resolved to world '{}' ({x}, {y}, {z}, {yaw}, {pitch})",
            world.get_name()
        );
        return;
    }

    for world in server.get_all_worlds() {
        let spawn = world.get_spawn_location();
        // The lowest buildable layer of the dimension: a `top block` on it
        // means the chunk was not loaded and no Y can be suggested.
        let min_y = if world.get_dimension() == "minecraft:overworld" {
            -64
        } else {
            0
        };
        let top = world.get_top_block_y(spawn.pos.x, spawn.pos.z);
        if top > min_y {
            tracing::info!(
                "spawn: WORLD_SPAWN=\"{raw}\" is unusable; world '{}' ({}) spawns at ({}, {}, {}) (top block y={top}) -> set WORLD_SPAWN: \"{}|{}.5|{}|{}.5|0|0\"",
                world.get_name(),
                world.get_dimension(),
                spawn.pos.x,
                spawn.pos.y,
                spawn.pos.z,
                world.get_name(),
                spawn.pos.x,
                top + 1,
                spawn.pos.z
            );
        } else {
            tracing::info!(
                "spawn: WORLD_SPAWN=\"{raw}\" is unusable; world '{}' ({}) spawns at ({}, {}, {}) [chunks not loaded yet, top block unknown]",
                world.get_name(),
                world.get_dimension(),
                spawn.pos.x,
                spawn.pos.y,
                spawn.pos.z
            );
        }
    }
}

pumpkin_plugin_api::register_plugin!(LazarusPlugin);

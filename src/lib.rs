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
mod scoreboard;
mod selection;
mod staffmode;
mod state;
mod tab;
mod tasks;
mod timers;
mod userdata;
mod util;

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
        state::init(data_folder)?;

        // Cache world spawns before anything else so the warzone is centred correctly.
        core::refresh_spawn_centers(&context.get_server());

        commands::register(&context)?;
        listeners::register(&context)?;
        tasks::start(&context);

        tracing::info!("Lazarus HCF core enabled (Pumpkin/WASM port).");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        state::save_all();
        tracing::info!("Lazarus HCF core disabled; data saved.");
        Ok(())
    }
}

pumpkin_plugin_api::register_plugin!(LazarusPlugin);

//! Command registration.

pub mod base;
pub mod crowbar;
pub mod customtimer;
pub mod deathban;
pub mod economy;
pub mod endportal;
pub mod faction;
pub mod hologram;
pub mod inventoryrestore;
pub mod kit;
pub mod lastdeaths;
pub mod lives;
pub mod logout;
pub mod pvp;
pub mod rankrevive;
pub mod salvage;
pub mod settings;
pub mod sotw;
pub mod staff;

use pumpkin_plugin_api::command::{Arg, Command, ConsumedArgs};
use pumpkin_plugin_api::permission::{Permission, PermissionDefault, PermissionLevel};
use pumpkin_plugin_api::wit::pumpkin::plugin::command::Number;
use pumpkin_plugin_api::Context;

use crate::utils::core;

/// Registers a permission node that defaults to allowing everyone.
pub fn register_public_permission(context: &Context, node: &str, description: &str) -> Result<(), String> {
    context.register_permission(&Permission {
        node: node.to_string(),
        description: description.to_string(),
        default: PermissionDefault::Allow,
        children: Vec::new(),
    })
}

/// Registers a permission node that defaults to operators only.
pub fn register_op_permission(context: &Context, node: &str, description: &str) -> Result<(), String> {
    context.register_permission(&Permission {
        node: node.to_string(),
        description: description.to_string(),
        default: PermissionDefault::Op(PermissionLevel::Four),
        children: Vec::new(),
    })
}

/// Registers every command tree.
pub fn register(context: &Context) -> Result<(), String> {
    base::register(context)?;
    crowbar::register(context)?;
    customtimer::register(context)?;
    deathban::register(context)?;
    economy::register(context)?;
    endportal::register(context)?;
    faction::register(context)?;
    lastdeaths::register(context)?;
    lives::register(context)?;
    logout::register(context)?;
    staff::register(context)?;
    hologram::register(context)?;
    inventoryrestore::register(context)?;
    kit::register(context)?;
    pvp::register(context)?;
    rankrevive::register(context)?;
    salvage::register(context)?;
    settings::register(context)?;
    sotw::register(context)?;
    Ok(())
}

/// Extracts a string argument, tolerating `simple`/`msg` argument kinds.
pub(crate) fn string_arg(args: &ConsumedArgs, key: &str) -> Option<String> {
    match args.get_value(key) {
        Arg::Simple(value) | Arg::Msg(value) => Some(value),
        _ => None,
    }
}

/// Like [`string_arg`], but treats the host placeholder for a missing argument
/// as absent: Pumpkin's `consumed-args.get-value` returns `simple("")` when the
/// key was not parsed (`wasm_host` `get_value` fallback), and single-word
/// arguments can never legitimately be empty.
pub(crate) fn present_string_arg(args: &ConsumedArgs, key: &str) -> Option<String> {
    string_arg(args, key).filter(|value| !value.is_empty())
}

/// Extracts an integer argument.
pub(crate) fn int_arg(args: &ConsumedArgs, key: &str) -> Option<i64> {
    match args.get_value(key) {
        Arg::Num(Ok(Number::Int32(value))) => Some(value as i64),
        Arg::Num(Ok(Number::Int64(value))) => Some(value),
        Arg::Num(Ok(Number::Float32(value))) => Some(value as i64),
        Arg::Num(Ok(Number::Float64(value))) => Some(value as i64),
        _ => None,
    }
}

/// Extracts a floating point argument.
pub(crate) fn double_arg(args: &ConsumedArgs, key: &str) -> Option<f64> {
    match args.get_value(key) {
        Arg::Num(Ok(Number::Int32(value))) => Some(value as f64),
        Arg::Num(Ok(Number::Int64(value))) => Some(value as f64),
        Arg::Num(Ok(Number::Float32(value))) => Some(value as f64),
        Arg::Num(Ok(Number::Float64(value))) => Some(value),
        _ => None,
    }
}

/// Builds a `Command` from a list of `&str` names.
pub(crate) fn command(names: &[&str], description: &str) -> Command {
    let names: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
    Command::new(&names, description)
}

/// Returns the sender's player handle or `None` (console).
pub(crate) fn sender_player(sender: &pumpkin_plugin_api::command::CommandSender) -> Option<pumpkin_plugin_api::Player> {
    sender.as_player()
}

/// Sends a message to the command sender.
pub(crate) fn reply(sender: &pumpkin_plugin_api::command::CommandSender, message: impl AsRef<str>) {
    sender.send_message(crate::utils::text(message));
}

/// Sends an error to the command sender.
pub(crate) fn reply_error(sender: &pumpkin_plugin_api::command::CommandSender, message: impl AsRef<str>) {
    sender.send_error(crate::utils::text(message));
}

/// The invoking player's faction id, if any.
pub(crate) fn sender_faction_id(sender: &pumpkin_plugin_api::command::CommandSender) -> Option<String> {
    sender_player(sender).map(|player| core::uuid_of(&player)).and_then(|uuid| core::faction_id_of(&uuid))
}

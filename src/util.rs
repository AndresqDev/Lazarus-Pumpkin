//! Small helpers shared across the port.
//!
//! Bukkit used `ChatColor.translateAlternateColorCodes('&', ...)`. Pumpkin exposes
//! `TextComponent::from_legacy_string_with_code`, which does the same job for
//! legacy `&`-prefixed color/format codes.

use pumpkin_plugin_api::text::TextComponent;

/// Parses a legacy `&`-formatted string into a Pumpkin text component.
pub fn text(input: impl AsRef<str>) -> TextComponent {
    TextComponent::from_legacy_string_with_code(input.as_ref(), '&')
}

/// Parses a legacy `§`-formatted string into a Pumpkin text component.
pub fn text_section(input: impl AsRef<str>) -> TextComponent {
    TextComponent::from_legacy_string(input.as_ref())
}

/// Plain, unformatted text component.
pub fn plain(input: impl AsRef<str>) -> TextComponent {
    TextComponent::text(input.as_ref())
}

/// Replaces every occurrence of `from` with `to`.
///
/// Equivalent to the `<placeholder>` replacement used all over Lazarus.
pub fn replace(input: &str, from: &str, to: &str) -> String {
    input.replace(from, to)
}

/// Applies a list of `(placeholder, value)` replacements in order.
pub fn replace_all(input: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = input.to_string();
    for (from, to) in pairs {
        out = out.replace(from, to);
    }
    out
}

/// Chunk coordinates for a block position.
pub fn chunk_key(x: i32, z: i32) -> (i32, i32) {
    (x >> 4, z >> 4)
}

/// Formats a duration (in milliseconds) the way players expect on the scoreboard,
/// e.g. `1h 05m`, `2m 30s`, `12s`.
pub fn format_duration(millis: i64) -> String {
    if millis <= 0 {
        return "0s".to_string();
    }

    let total_seconds = millis / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes:02}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}

/// Capitalizes the first character of a string (used by role names).
pub fn capitalize(input: &str) -> String {
    let mut chars = input.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}

/// Current unix time in milliseconds.
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

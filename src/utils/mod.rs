//! Small helpers shared across the port.
//!
//! Bukkit used `ChatColor.translateAlternateColorCodes('&', ...)`. Pumpkin exposes
//! `TextComponent::from_legacy_string_with_code`, which does the same job for
//! legacy `&`-prefixed color/format codes.

pub mod core;
pub mod datetime;
pub mod nbt;
pub mod potion_components;

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

/// Formats a duration (in milliseconds) as a `MM:SS` clock (or `HH:MM:SS` past
/// one hour), the format `PvpProtTimer#getTimeLeft` / `SystemTimer#getDynamicTimeLeft`
/// used for `/pvp time` and `/sotw time`.
pub fn format_clock(millis: i64) -> String {
    let total_seconds = millis.max(0) / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Formats an amount of seconds as `HH:MM:SS` (original
/// `StringUtils.formatTime(..., SECONDS_TO_HOURS)`, used by the SOTW broadcast).
pub fn format_hms(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

/// Formats a double the way Java's `String.valueOf(double)` does (`1.0`, `6.5`).
/// Used by the DTR display, which prints the raw value (`1.1`) instead of a
/// fixed number of decimals.
pub fn java_double(value: f64) -> String {
    if value == value.trunc() {
        format!("{value:.1}")
    } else {
        format!("{value}")
    }
}

/// Port of `StringUtils#formatDurationWords` =
/// `DurationFormatUtils.formatDurationWords(millis, true, true)`: a pluralised
/// "1 day 2 hours 3 minutes" string with leading/trailing zero elements
/// suppressed ("45 minutes", "1 hour", "0 seconds").
pub fn format_duration_words(millis: i64) -> String {
    let millis = millis.max(0);
    let days = millis / 86_400_000;
    let rem = millis % 86_400_000;
    let hours = rem / 3_600_000;
    let rem = rem % 3_600_000;
    let minutes = rem / 60_000;
    let rem = rem % 60_000;
    let seconds = rem / 1000;

    let mut duration = format!("{days} days {hours} hours {minutes} minutes {seconds} seconds");

    // `suppressLeadingZeroElements` (mirrors the nested chain in commons-lang3;
    // note the original never strips " 0 seconds" here — its check is dead code
    // because `duration` is reassigned right before it).
    duration.insert(0, ' ');
    let tmp = duration.replacen(" 0 days", "", 1);
    if tmp.len() != duration.len() {
        duration = tmp;
        let tmp = duration.replacen(" 0 hours", "", 1);
        if tmp.len() != duration.len() {
            duration = tmp;
            duration = duration.replacen(" 0 minutes", "", 1);
        }
    }
    if !duration.is_empty() {
        duration.remove(0);
    }

    // `suppressTrailingZeroElements`.
    let tmp = duration.replacen(" 0 seconds", "", 1);
    if tmp.len() != duration.len() {
        duration = tmp;
        let tmp = duration.replacen(" 0 minutes", "", 1);
        if tmp.len() != duration.len() {
            duration = tmp;
            let tmp = duration.replacen(" 0 hours", "", 1);
            if tmp.len() != duration.len() {
                duration = tmp.replacen(" 0 days", "", 1);
            }
        }
    }

    // Pluralisation pass.
    duration.insert(0, ' ');
    duration = duration.replacen(" 1 seconds", " 1 second", 1);
    duration = duration.replacen(" 1 minutes", " 1 minute", 1);
    duration = duration.replacen(" 1 hours", " 1 hour", 1);
    duration = duration.replacen(" 1 days", " 1 day", 1);
    duration.trim().to_string()
}

/// Port of `StringUtils#parseSeconds`: accepts a plain integer (seconds) or a
/// combination of `d`/`h`/`m`/`s` suffixes (`10`, `10s`, `5m`, `2h`, `1d2h30m`).
/// Returns `None` for invalid input (the original returns `-1`).
pub fn parse_seconds(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if let Ok(seconds) = trimmed.parse::<i64>() {
        return Some(seconds.abs());
    }

    let mut rest = trimmed.to_lowercase();
    if rest == "0s" {
        return Some(0);
    }

    let mut seconds: i64 = 0;
    for (ch, multiplier) in [('d', 86400_i64), ('h', 3600), ('m', 60), ('s', 1)] {
        if !rest.contains(ch) {
            continue;
        }
        let mut split = rest.splitn(2, ch);
        let first = split.next().unwrap_or("");
        let after = split.next().unwrap_or("");
        if let Ok(part) = first.parse::<i64>() {
            seconds += part.abs() * multiplier;
            if !after.is_empty() {
                rest = after.to_string();
            }
        }
    }

    if seconds == 0 {
        None
    } else {
        Some(seconds)
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

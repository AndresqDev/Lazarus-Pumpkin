//! Date formatting for the `DATE_FORMAT`/`TIMEZONE` settings.
//!
//! The original formats every timestamp (death sign lore, stattrak kill
//! strings, `/lastdeaths` and `/lastkills` entries) with Apache Commons
//! `FastDateFormat.getInstance(Config.DATE_FORMAT, Config.TIMEZONE,
//! Locale.ENGLISH)`. This module ports that call: the FastDateFormat pattern
//! is translated to a chrono pattern and the timezone name is resolved through
//! the tz database embedded in `chrono-tz` (`Europe/Zagreb`, `UTC`,
//! `GMT+2`-style offsets). Unknown names fall back to UTC, where the original
//! would have had FastDateFormat throw at first use.

use chrono::{DateTime, FixedOffset, Utc};
use chrono_tz::Tz;

/// Formats the current time with a `FastDateFormat`-style pattern and timezone.
pub fn format_now(pattern: &str, timezone: &str) -> String {
    let pattern = translate_pattern(pattern);
    let millis = crate::utils::now_millis();
    let Some(now) = DateTime::<Utc>::from_timestamp_millis(millis) else {
        return String::new();
    };

    match resolve_zone(timezone) {
        Zone::Tz(tz) => now.with_timezone(&tz).format(&pattern).to_string(),
        Zone::Fixed(offset) => now.with_timezone(&offset).format(&pattern).to_string(),
    }
}

/// Reads `DATE_FORMAT` and `TIMEZONE` from the raw `config.yml` and formats the
/// current time. Empty values fall back to the original's defaults.
///
/// Must be called without holding the state lock.
pub fn now_from_config() -> String {
    let (pattern, timezone) = {
        let guard = crate::state::state().read().unwrap();
        (
            guard.raw.config.string("DATE_FORMAT"),
            guard.raw.config.string("TIMEZONE"),
        )
    };

    let pattern = if pattern.is_empty() { "dd.MM HH:mm:ss".to_string() } else { pattern };
    let timezone = if timezone.is_empty() { "UTC".to_string() } else { timezone };
    format_now(&pattern, &timezone)
}

enum Zone {
    Tz(Tz),
    Fixed(FixedOffset),
}

fn resolve_zone(name: &str) -> Zone {
    let name = name.trim();
    if let Ok(tz) = name.parse::<Tz>() {
        return Zone::Tz(tz);
    }

    // `GMT+2` / `UTC-0230` style fixed offsets.
    let upper = name.to_ascii_uppercase();
    for prefix in ["GMT", "UTC"] {
        if let Some(rest) = upper.strip_prefix(prefix)
            && !rest.is_empty()
            && let Some(offset) = parse_offset(rest)
        {
            return Zone::Fixed(offset);
        }
    }

    Zone::Fixed(FixedOffset::east_opt(0).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap()))
}

fn parse_offset(input: &str) -> Option<FixedOffset> {
    let (sign, rest) = match input.as_bytes().first()? {
        b'+' => (1, &input[1..]),
        b'-' => (-1, &input[1..]),
        _ => return None,
    };

    let (hours, minutes) = if let Some((hours, minutes)) = rest.split_once(':') {
        (hours.parse::<i32>().ok()?, minutes.parse::<i32>().ok()?)
    } else if rest.len() > 2 {
        let split = rest.len() - 2;
        (
            rest[..split].parse::<i32>().ok()?,
            rest[split..].parse::<i32>().ok()?,
        )
    } else {
        (rest.parse::<i32>().ok()?, 0)
    };

    FixedOffset::east_opt(sign * (hours * 3600 + minutes * 60))
}

/// Translates a `FastDateFormat` pattern into a chrono pattern, covering the
/// tokens the original's configs use: `y`, `M`, `d`, `E`, `H`, `h`, `m`, `s`,
/// `S`, `a`, `z`, `Z`. Everything else is copied literally (`'quoted'` text is
/// copied as-is, per FastDateFormat).
fn translate_pattern(pattern: &str) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '\'' {
                quoted = false;
            } else {
                push_literal(&mut out, ch);
            }
            continue;
        }

        if ch == '\'' {
            quoted = true;
            continue;
        }

        let mut count = 1usize;
        while chars.peek() == Some(&ch) {
            chars.next();
            count += 1;
        }

        let token: &str = match ch {
            'y' => if count == 2 { "%y" } else { "%Y" },
            'M' => match count {
                1 => "%-m",
                2 => "%m",
                3 => "%b",
                _ => "%B",
            },
            'd' => if count == 1 { "%-d" } else { "%d" },
            'E' => if count >= 4 { "%A" } else { "%a" },
            'H' | 'k' => if count == 1 { "%-H" } else { "%H" },
            'h' | 'K' => if count == 1 { "%-I" } else { "%I" },
            'm' => if count == 1 { "%-M" } else { "%M" },
            's' => if count == 1 { "%-S" } else { "%S" },
            'S' => "%3f",
            'a' => "%p",
            'z' => "%Z",
            'Z' | 'X' => "%z",
            'G' => "%G",
            'D' => "%j",
            'u' => "%u",
            'w' => "%w",
            _ => {
                for _ in 0..count {
                    push_literal(&mut out, ch);
                }
                continue;
            }
        };

        out.push_str(token);
    }

    out
}

/// Appends a literal character, escaping chrono's `%` introducer.
fn push_literal(out: &mut String, ch: char) {
    if ch == '%' {
        out.push('%');
    }
    out.push(ch);
}

#[cfg(test)]
mod tests {
    use super::translate_pattern;

    #[test]
    fn translates_the_default_pattern() {
        assert_eq!(translate_pattern("dd.MM HH:mm:ss"), "%d.%m %H:%M:%S");
    }

    #[test]
    fn translates_quoted_literals() {
        assert_eq!(translate_pattern("'at' HH"), "at %H");
        assert_eq!(translate_pattern("100% d"), "100%% %-d");
    }
}

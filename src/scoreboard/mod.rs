//! Sidebar scoreboard — port of `me.qiooip.lazarus.scoreboard.ScoreboardManager` /
//! `ScoreboardUpdaterImpl` / `PlayerScoreboard_1_8`.
//!
//! The original built the sidebar by *adding sections in a fixed order* (staff →
//! territory → timers → faction focus/rally → class → abilities) and wrapping them
//! with coloured separator lines and a footer. We reproduce that order and the
//! `scoreboard.yml` placeholders, rendering each line with scoreboard teams.

use pumpkin_plugin_api::common::{GameMode, NamedColor};
use pumpkin_plugin_api::scoreboard::{CollisionRule, DisplaySlot, NametagVisibility, RenderType, TeamSettings};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Player, Server};

use crate::config::SidebarConfig;
use crate::state::{state, AppState};
use crate::timer::TimerType;
use crate::utils;

const OBJECTIVE: &str = "lazarus";
const SLOTS: usize = 15;

/// Creates the sidebar objective and its line teams for a player.
pub fn setup(player: &Player) {
    // Invalidate the render diff: the teams below are re-created with empty
    // prefixes, so the next update has to send every row again.
    state()
        .write()
        .unwrap()
        .scoreboard_cache
        .remove(&utils::core::uuid_of(player));

    let title = {
        let guard = state().read().unwrap();
        guard.sidebar.title.clone()
    };

    let Some(java) = player.as_java() else {
        return;
    };
    let board = java.get_scoreboard();

    // Idempotent: clear any previous layout (also used by /lazarus reload).
    for slot in 0..SLOTS {
        board.remove_team(&team_name(slot));
    }
    board.remove_objective(OBJECTIVE);

    board.add_objective(OBJECTIVE, utils::text(title), RenderType::Integer, None);
    board.set_display_slot(DisplaySlot::Sidebar, OBJECTIVE);

    for slot in 0..SLOTS {
        board.create_team(&team_name(slot), team_settings("", ""));
    }
}

/// Rebuilds and renders the sidebar for a player. Runs every 2 ticks — the
/// original `ScoreboardUpdaterImpl` loops at a 100 ms fixed rate — and sends
/// only the rows that changed, like `PlayerScoreboard_1_8#update`'s entry
/// cache.
pub fn update(player: &Player, server: &Server) {
    // The sidebar is a Java-only surface.
    if player.as_java().is_none() {
        return;
    }

    let uuid = crate::utils::core::uuid_of(player);

    let lines = {
        let guard = state().read().unwrap();
        if !guard.sidebar.enabled {
            return;
        }

        let Some(user) = guard.userdata.get(&uuid) else {
            return;
        };
        if !user.settings.scoreboard {
            return;
        }

        build_lines(&guard, player, server, &uuid)
    };

    let previous = {
        let guard = state().read().unwrap();
        guard.scoreboard_cache.get(&uuid).cloned()
    };
    if previous.as_deref() == Some(lines.as_slice()) {
        return;
    }

    render(player, previous.as_deref(), &lines);

    state()
        .write()
        .unwrap()
        .scoreboard_cache
        .insert(uuid, lines);
}

fn render(player: &Player, previous: Option<&[String]>, lines: &[String]) {
    let Some(java) = player.as_java() else {
        return;
    };
    let board = java.get_scoreboard();

    for slot in 0..SLOTS {
        let old = previous.and_then(|p| p.get(slot));
        let new = lines.get(slot);
        // Untouched rows cost no packets (`PlayerScoreboard_1_8#update`
        // compares against its `entryCache` the same way).
        if old == new {
            continue;
        }

        let team = team_name(slot);
        let entry = entry_name(slot);

        match new {
            Some(line) => {
                let (prefix, suffix) = split_line(line);
                board.update_team(&team, team_settings(&prefix, &suffix));
                board.add_player_to_team(&team, &entry);
                board.update_score(&entry, OBJECTIVE, (SLOTS - slot) as i32, None);
            }
            None => {
                // Remove the row entirely so no blank line is reserved.
                board.remove_score(&entry, OBJECTIVE);
                board.remove_player_from_team(&team, &entry);
                board.update_team(&team, team_settings("", ""));
            }
        }
    }
}

/// Removes the sidebar from a player (called on quit / when disabled).
pub fn teardown(player: &Player) {
    state()
        .write()
        .unwrap()
        .scoreboard_cache
        .remove(&utils::core::uuid_of(player));

    let Some(java) = player.as_java() else {
        return;
    };
    let board = java.get_scoreboard();
    for slot in 0..SLOTS {
        board.remove_team(&team_name(slot));
    }
    board.remove_objective(OBJECTIVE);
}

/// Mirrors `PlayerScoreboard_1_8`'s ordered section builder.
struct SidebarBuilder<'a> {
    s: &'a SidebarConfig,
    lines: Vec<String>,
    last_line: bool,
    max: usize,
}

impl<'a> SidebarBuilder<'a> {
    fn new(s: &'a SidebarConfig) -> Self {
        Self {
            s,
            lines: Vec::new(),
            last_line: false,
            max: if s.footer_enabled { 11 } else { 13 },
        }
    }

    /// `add(value, time)` in the original; the rendered line is `value + time`.
    fn add(&mut self, prefix: &str, value: &str) {
        if prefix.is_empty() || self.lines.len() >= self.max {
            return;
        }
        self.lines.push(format!("{prefix}{value}"));
        self.last_line = false;
    }

    fn add_line(&mut self, color: &str) {
        if !self.s.line_after_every_section
            || self.lines.is_empty()
            || self.last_line
            || self.lines.len() >= self.max
        {
            return;
        }
        self.lines.push(self.sb_line(color));
        self.last_line = true;
    }

    /// `PlayerScoreboard#addEmptyLine(color)`: a blank spacer row. The original
    /// stores an empty entry with a coloured strikethrough prefix (renders as
    /// nothing) and still sets `lastLine`; it has no size/line guards.
    fn add_empty_line(&mut self, _color: &str) {
        self.lines.push(String::new());
        self.last_line = true;
    }

    fn sb_line(&self, color: &str) -> String {
        if self.s.line_invisible {
            format!("{color}&r")
        } else {
            // Port of `getScoreboardLineInput`: the line is duplicated three times
            // (prefix + name + suffix) to fill the row, which is why the original
            // looks longer.
            let sb = format!("{}&m------", self.s.line_color);
            format!("{sb}{color}{sb}---------")
        }
    }

    /// Port of `addLinesAndFooter`.
    fn finish(mut self) -> Vec<String> {
        self.lines.insert(0, self.sb_line("&1"));

        if self.s.footer_enabled && self.last_line {
            self.lines.pop();
            self.last_line = false;
        }

        if !self.last_line {
            if self.s.footer_enabled {
                // `addFooter` pushes an invisible `AQUA + RESET` spacer row
                // before the footer text.
                self.lines.push(String::new());
                self.lines.push(self.s.footer.clone());
            }
            if !self.s.line_invisible {
                self.lines.push(self.sb_line("&2"));
            }
        }

        self.lines.truncate(SLOTS);
        self.lines
    }
}

fn build_lines(guard: &AppState, player: &Player, server: &Server, uuid: &str) -> Vec<String> {
    let s = &guard.sidebar;
    let mut b = SidebarBuilder::new(s);

    let in_staff_mode = guard.staff.is_staff(uuid);
    let vanished = guard.staff.is_vanished(uuid);

    // ---- Staff / vanish section (applyStaffPlaceholders) ----
    if in_staff_mode {
        b.add(&s.staffmode, "");
        b.add(
            &s.visibility,
            if vanished { &s.staff_visibility_vanished } else { &s.staff_visibility_visible },
        );
        b.add(&s.chatmode, &s.staff_chat_global);
        let creative = player.get_gamemode() == GameMode::Creative;
        b.add(
            &s.gamemode,
            if creative { &s.staff_gamemode_creative } else { &s.staff_gamemode_survival },
        );
        b.add(&s.online, &server.get_all_players().len().to_string());
        b.add_line("&3");
    } else if vanished {
        b.add(&s.vanish, "");
        b.add(&s.visibility, &s.staff_visibility_vanished);
        b.add_line("&3");
    }

    // ---- Territory + kitmap stats (applyMiscellaneousPlaceholders) ----
    {
        let location = crate::utils::core::location(player);
        // Guard-based helpers: never re-enter the state lock (the caller already
        // holds it) or the scheduler task deadlocks.
        let territory = crate::utils::core::territory_at_guard(
            guard,
            &location.world,
            location.block_x(),
            location.block_z(),
        );
        let viewer = guard.factions.by_member(uuid).map(|f| f.id.clone());
        let display = crate::utils::core::territory_display_guard(guard, &territory, viewer.as_deref());

        // Kitmap mode prepends the `KITMAP_STATS` block (title, kills, deaths,
        // balance and — only while streaking — the killstreak) and an extra
        // spacer line before the claim. `KITMAP_STATS.ONLY_IN_SPAWN` (port
        // addition, default true) hides the whole block outside safezones.
        if guard.config.kitmap.enabled {
            let only_in_spawn = !guard.raw.scoreboard.has("KITMAP_STATS.ONLY_IN_SPAWN")
                || guard.raw.scoreboard.bool("KITMAP_STATS.ONLY_IN_SPAWN");
            let show_stats =
                !only_in_spawn || matches!(territory, crate::utils::core::Territory::Safezone);

            if show_stats {
                if let Some(user) = guard.userdata.get(uuid) {
                    let sb = &guard.raw.scoreboard;
                    b.add(&sb.string("KITMAP_STATS.TITLE_PLACEHOLDER"), "");
                    b.add(
                        &sb.string("KITMAP_STATS.KILLS_PLACEHOLDER"),
                        &user.kills.to_string(),
                    );
                    b.add(
                        &sb.string("KITMAP_STATS.DEATHS_PLACEHOLDER"),
                        &user.deaths.to_string(),
                    );
                    b.add(
                        &sb.string("KITMAP_STATS.BALANCE_PLACEHOLDER"),
                        &user.balance.to_string(),
                    );
                    if user.killstreak > 0 {
                        b.add(
                            &sb.string("KITMAP_STATS.KILLSTREAK_PLACEHOLDER"),
                            &user.killstreak.to_string(),
                        );
                    }
                }

                if !s.claim.is_empty() {
                    b.add_empty_line("&4");
                }
            }

            if !s.claim.is_empty() {
                b.add(&s.claim, &display);
            }
            b.add_line("&5");
        } else {
            b.add(&s.claim, &display);
        }
    }

    // ---- Custom timers (port of `CustomTimer#handleScoreboardUpdate`; the
    // original applies them right after the claim line) ----
    {
        let now = crate::utils::now_millis();
        for timer in &guard.timers.custom {
            let remaining = timer.expiry - now;
            if remaining > 0 {
                // The stored placeholder already carries its trailing space
                // (`placeholder + " "` in the original).
                b.add(&timer.placeholder, &utils::format_clock(remaining));
            }
        }
    }

    // ---- Scoreboard timers (applyScoreboardTimerPlaceholders), in
    // TimerManager order: sale, keySale, `sotw`, eotw, purge, `pvpProt`,
    // `combatTag`, `enderPearl`, archerTag, home, stuck, teleport, logout,
    // gApple, apple. Only the ported timers render here, and the SOTW line
    // comes before the rest.
    let sotw_remaining = guard.timers.sotw_remaining();
    if sotw_remaining > 0 {
        // Port of `SotwTimer`: while the global timer runs the line shows the
        // dynamic time left, using the struck-through placeholder for players
        // that opted out with `/sotw enable`.
        let placeholder = if guard.sotw.enabled_players.contains(uuid) {
            &s.sotw_enabled
        } else {
            &s.sotw
        };
        b.add(placeholder, &utils::format_clock(sotw_remaining));
    }
    for (placeholder, timer_type) in [
        (&s.pvp_protection, TimerType::PvpProtection),
        (&s.combat_tag, TimerType::CombatTag),
        (&s.enderpearl, TimerType::Enderpearl),
        (&s.home, TimerType::Home),
        (&s.stuck, TimerType::Stuck),
        (&s.logout, TimerType::Logout),
        (&s.gapple, TimerType::Gapple),
        (&s.apple, TimerType::Apple),
    ] {
        let remaining = guard.timers.remaining(uuid, timer_type);
        if remaining > 0 {
            b.add(placeholder, &utils::format_duration(remaining));
        }
    }

    // ---- PvP class (applyPvpClassPlaceholders): the original applies it after
    // the scoreboard timers (and the faction focus/rally section), just before
    // abilities — the BLUE separator goes *before* its lines.
    if let Some(kind) = guard.classes.active_class(uuid) {
        use crate::classes::ClassKind;
        b.add_line("&9");
        b.add(
            &guard.raw.scoreboard.string("PVPCLASS_ACTIVE_PLACEHOLDER"),
            kind.name(),
        );
        match kind {
            ClassKind::Bard => {
                let value = guard.classes.bard_energy.get(uuid).copied().unwrap_or(0.0);
                b.add(
                    &guard.raw.scoreboard.string("BARD_ENERGY_PLACEHOLDER"),
                    &round1(value),
                );
            }
            ClassKind::Mage => {
                let value = guard.classes.mage_energy.get(uuid).copied().unwrap_or(0.0);
                b.add(
                    &guard.raw.scoreboard.string("MAGE_ENERGY_PLACEHOLDER"),
                    &round1(value),
                );
            }
            ClassKind::Miner => {
                // `Miner#applyActiveScoreboardLines` only adds the diamonds line
                // outside kitmap mode.
                if !guard.config.kitmap.enabled {
                    let diamonds = guard.userdata.get(uuid).map(|u| u.diamonds_mined).unwrap_or(0);
                    b.add(
                        &guard.raw.scoreboard.string("MINER_DIAMOND_COUNT_PLACEHOLDER"),
                        &diamonds.to_string(),
                    );
                }
            }
            _ => {}
        }
        if let Some(seconds) = guard.classes.active_cooldown_seconds(uuid) {
            b.add(
                &guard.raw.scoreboard.string("COOLDOWN_PLACEHOLDER"),
                &format!("{seconds}s"),
            );
        }
    } else if let Some((kind, seconds)) = guard.classes.warmup_info(uuid) {
        b.add_line("&9");
        b.add(
            &guard.raw.scoreboard.string("PVPCLASS_WARMUP_PLACEHOLDER"),
            &format!("{} &7({seconds}s)", kind.name()),
        );
    }

    b.finish()
}

fn team_name(slot: usize) -> String {
    format!("lz{slot:02}")
}

/// An invisible, unique sidebar entry per slot.
fn entry_name(slot: usize) -> String {
    let colors = b"0123456789abcdef";
    format!("\u{00a7}{}\u{00a7}r", colors[slot % colors.len()] as char)
}

fn team_settings(prefix: &str, suffix: &str) -> TeamSettings {
    TeamSettings {
        display_name: TextComponent::text(""),
        friendly_fire: true,
        see_friendly_invisibles: true,
        nametag_visibility: NametagVisibility::Always,
        collision_rule: CollisionRule::Always,
        color: NamedColor::White,
        prefix: utils::text(prefix),
        suffix: utils::text(suffix),
    }
}

/// Splits a rendered line into the two 1.8 team fields.
///
/// The original `PlayerScoreboard_1_8` never sent more than 16 characters in a
/// team prefix/suffix: long values were split into `prefix + entry + suffix`
/// (`ScoreboardInput`). The port renders the visible text through the team
/// prefix, which modern clients accept at any length but 1.8 (and the whole
/// 1.9–1.12 range) caps at 16 — ViaVersion simply truncates, which is what cut
/// the sidebar lines for 1.8 clients. We mirror the original limits with the
/// same colour continuation the original's 1.7 tab used
/// (`ChatColor.getLastColors(prefix)`), since the invisible entry between the
/// two fields resets the formatting.
fn split_line(line: &str) -> (String, String) {
    let chars: Vec<char> = line.chars().collect();
    if chars.len() <= 16 {
        return (line.to_string(), String::new());
    }

    // Never cut a `&x` sequence in half (same guard as the original tab).
    let split = if chars[15] == '&' || chars[15] == '\u{00a7}' {
        15
    } else {
        16
    };

    let prefix: String = chars[..split].iter().collect();
    let rest: String = chars[split..].iter().collect();

    let mut suffix: Vec<char> = last_colors(&prefix).chars().chain(rest.chars()).collect();
    if suffix.len() > 16 {
        suffix.truncate(16);
        if matches!(suffix.last(), Some('&' | '\u{00a7}')) {
            suffix.pop();
        }
    }

    (prefix, suffix.into_iter().collect())
}

/// Bukkit's `ChatColor.getLastColors`: the last colour code plus the
/// formatting codes that follow it (`&r` clears both).
fn last_colors(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut color: Option<char> = None;
    let mut formats: Vec<char> = Vec::new();

    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '&' || chars[i] == '\u{00a7}' {
            let code = chars[i + 1].to_ascii_lowercase();
            if "0123456789abcdef".contains(code) {
                color = Some(code);
                formats.clear();
            } else if "klmno".contains(code) {
                formats.push(code);
            } else if code == 'r' {
                color = None;
                formats.clear();
            }
            i += 2;
        } else {
            i += 1;
        }
    }

    let mut out = String::new();
    if let Some(code) = color {
        out.push('&');
        out.push(code);
    }
    for code in formats {
        out.push('&');
        out.push(code);
    }
    out
}

/// Rounds to one decimal (the way the original shows Bard/Mage energy).
fn round1(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    format!("{rounded}")
}

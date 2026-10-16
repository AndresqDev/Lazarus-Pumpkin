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
use crate::timers::TimerType;
use crate::util;

const OBJECTIVE: &str = "lazarus";
const SLOTS: usize = 15;

/// Creates the sidebar objective and its line teams for a player.
pub fn setup(player: &Player) {
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

    board.add_objective(OBJECTIVE, util::text(title), RenderType::Integer, None);
    board.set_display_slot(DisplaySlot::Sidebar, OBJECTIVE);

    for slot in 0..SLOTS {
        board.create_team(&team_name(slot), team_settings(""));
    }
}

/// Rebuilds and renders the sidebar for a player (called once per second).
pub fn update(player: &Player, server: &Server) {
    let lines = {
        let guard = state().read().unwrap();
        if !guard.sidebar.enabled {
            return;
        }

        let uuid = crate::core::uuid_of(player);
        let Some(user) = guard.userdata.get(&uuid) else {
            return;
        };
        if !user.settings.scoreboard {
            return;
        }

        let mut lines = build_lines(&guard, player, server, &uuid);

        // Active PvP class line.
        if let Some(kind) = guard.classes.active_class(&uuid) {
            let template = guard.raw.scoreboard.string("PVPCLASS_ACTIVE_PLACEHOLDER");
            if !template.is_empty() {
                lines.push(format!("{template}{}", kind.name()));
            }
        }

        lines
    };

    render(player, &lines);
}

fn render(player: &Player, lines: &[String]) {
    let Some(java) = player.as_java() else {
        return;
    };
    let board = java.get_scoreboard();

    for slot in 0..SLOTS {
        let team = team_name(slot);
        let entry = entry_name(slot);

        match lines.get(slot) {
            Some(line) => {
                board.update_team(&team, team_settings(line));
                board.add_player_to_team(&team, &entry);
                board.update_score(&entry, OBJECTIVE, (SLOTS - slot) as i32, None);
            }
            None => {
                // Remove the row entirely so no blank line is reserved.
                board.remove_score(&entry, OBJECTIVE);
                board.remove_player_from_team(&team, &entry);
                board.update_team(&team, team_settings(""));
            }
        }
    }
}

/// Removes the sidebar from a player (called on quit / when disabled).
pub fn teardown(player: &Player) {
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

    // ---- Territory (applyMiscellaneousPlaceholders) ----
    {
        let location = crate::core::location(player);
        let territory = crate::core::territory_at(&location.world, location.block_x(), location.block_z());
        let viewer = crate::core::faction_id_of(uuid);
        let display = crate::core::territory_display(&territory, viewer.as_deref());
        b.add(&s.claim, &display);
    }

    // ---- Timers (applyScoreboardTimerPlaceholders), in TimerManager order ----
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
            b.add(placeholder, &util::format_duration(remaining));
        }
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

fn team_settings(line: &str) -> TeamSettings {
    TeamSettings {
        display_name: TextComponent::text(""),
        friendly_fire: true,
        see_friendly_invisibles: true,
        nametag_visibility: NametagVisibility::Always,
        collision_rule: CollisionRule::Always,
        color: NamedColor::White,
        prefix: util::text(line),
        suffix: TextComponent::text(""),
    }
}

//! Tab list — port of `me.qiooip.lazarus.tab.TabManager`.
//!
//! Important: the original wrote 80 text slots with `PacketPlayOutPlayerInfo`.
//! Pumpkin's WIT declares `CPlayerInfoUpdate`, **but the current host build does
//! not serialise it** (`serialize_java_packet` has no arm for it), so the packet is
//! silently dropped. The same applies to `CSetPlayerTeam`. The only tab packet the
//! host implements is `CTabList` (header/footer), so we render the LEFT/CENTER/
//! RIGHT/FAR_RIGHT content there (the modern equivalent).

use pumpkin_plugin_api::{Player, Server};

use crate::state::{state, AppState};
use crate::util;

const COLUMN_HEIGHT: usize = 20;

const FACES: [&str; 8] = ["S", "SW", "W", "NW", "N", "NE", "E", "SE"];

/// Sends the initial header/footer (called on join/reload).
pub fn setup(player: &Player, server: &Server) {
    apply(player, server);
}

/// Refreshes the header/footer (called every tick).
pub fn update(player: &Player, server: &Server) {
    apply(player, server);
}

/// No per-player packet state to clean up with the header/footer approach.
pub fn forget(_uuid: &str) {}

fn apply(player: &Player, server: &Server) {
    let (header, footer) = {
        let guard = state().read().unwrap();
        if !guard.config.tab.enabled {
            return;
        }

        let columns = compute_columns(&guard, player, server);
        let header = columns.into_iter().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>().join("\n");
        let footer = resolve_lines(&guard.config.tab.footer, &guard, player, server)
            .into_iter()
            .filter(|line| !line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        (header, footer)
    };

    player.set_tab_list_header_footer(util::text(header), util::text(footer));
}

/// Builds the 80 tab lines (4 columns), applying the original FactionInfo and
/// PlayerList modules, then resolves placeholders.
fn compute_columns(guard: &AppState, player: &Player, server: &Server) -> Vec<String> {
    let tab = &guard.config.tab;

    let mut flat: Vec<String> = Vec::with_capacity(COLUMN_HEIGHT * 4);
    for column in [&tab.left, &tab.center, &tab.right, &tab.far_right] {
        let mut padded = column.clone();
        padded.resize(COLUMN_HEIGHT, String::new());
        flat.extend(padded);
    }

    let uuid = crate::core::uuid_of(player);

    // FactionInfoModule: START_SLOT 4 -> indices 3,4.
    match guard.factions.by_member(&uuid) {
        Some(faction) => {
            let max_dtr = faction.max_dtr(&guard.config);
            flat[3] = format!("&7DTR: &c{:.2}&7/&a{max_dtr:.2}", faction.dtr);
            flat[4] = format!("&7Home: &7{}", faction.home_string());
        }
        None => {
            flat[3] = "&7Not in faction.".to_string();
            flat[4] = "&7/f create <name>".to_string();
        }
    }

    // PlayerListModule: START_SLOT 25 -> index 24.
    if let Some(faction) = guard.factions.by_member(&uuid).cloned() {
        let color = &guard.config.relation_colors.teammate;
        flat[24] = format!("{color}{}", faction.name);

        let mut members: Vec<&crate::factions::FactionMember> = faction.members.values().collect();
        members.sort_by(|a, b| b.role.cmp(&a.role));

        for (i, member) in members.iter().take(10).enumerate() {
            let index = 25 + i;
            if index < flat.len() {
                flat[index] = format!(
                    "{}{}{}",
                    guard.config.relation_colors.teammate,
                    member.role.prefix(),
                    member.name
                );
            }
        }
    }

    flat.iter().map(|line| resolve(line, guard, player, server)).collect()
}

fn resolve_lines(lines: &[String], guard: &AppState, player: &Player, server: &Server) -> Vec<String> {
    lines.iter().map(|line| resolve(line, guard, player, server)).collect()
}

fn resolve(line: &str, guard: &AppState, player: &Player, server: &Server) -> String {
    let uuid = crate::core::uuid_of(player);

    let (kills, deaths, lives) = guard
        .userdata
        .get(&uuid)
        .map(|u| (u.kills.to_string(), u.deaths.to_string(), u.lives.to_string()))
        .unwrap_or_else(|| ("0".into(), "0".into(), "0".into()));

    let location = crate::core::location(player);
    let territory = {
        let territory = crate::core::territory_at(&location.world, location.block_x(), location.block_z());
        let viewer = crate::core::faction_id_of(&uuid);
        crate::core::territory_display(&territory, viewer.as_deref())
    };
    let direction = FACES[((location.yaw / 45.0).round() as i32 & 0x7) as usize];
    let location_str = format!("({}, {}) [{direction}]", location.block_x(), location.block_z());

    util::replace_all(
        line,
        &[
            ("<online>", &server.get_all_players().len().to_string()),
            ("<max>", &server.get_max_players().to_string()),
            ("<player>", &crate::core::name_of(player)),
            ("<kills>", &kills),
            ("<deaths>", &deaths),
            ("<lives>", &lives),
            ("<faction>", &territory),
            ("<territory>", &territory),
            ("<location>", &location_str),
            ("<direction>", direction),
        ],
    )
}

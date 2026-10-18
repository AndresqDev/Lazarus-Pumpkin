//! Death handlers — port of `me.qiooip.lazarus.handlers.death.*`.
//!
//! Covered here:
//! * `DeathMessageHandler` — the custom death messages, their placeholder
//!   resolution and targeted delivery (the same message doubles as the
//!   deathban reason and the `/lastdeaths` entry).
//! * `StattrakHandler` — the kill counter lore on tracked weapons.
//! * `HeadDropHandler` — the player-head trophy for the killer plus the
//!   placed-head protections.
//! * `DeathSignHandler` — the death sign item, the fill on placement and the
//!   edit/break protection.
//!
//! Host deviations (also listed in `PORTING.md`):
//! * `PlayerDeathEvent` carries no drops list and the WIT cannot spawn item
//!   entities, so the head and the death sign are handed to the killer's
//!   inventory instead of being dropped at the death location.
//! * The WIT cannot read a skull's owner back from the block entity, so placed
//!   heads are tracked in memory from the placing item.
//! * The host cannot close the sign editor or broadcast `set_front_text`
//!   writes, so a placed death sign is filled server-side at placement (like
//!   the original's `sign.update()`) while the client keeps showing its editor
//!   until the chunk reloads (the same applies to edits, which are discarded
//!   like the original cancels them).
//! * Entity names in death messages are resolved by scanning the world entity
//!   lists for the id (the WIT has no entity lookup by id) and cached for the
//!   session. An `EntitySpawnEvent` registry would be cheaper but registering
//!   that event deadlocks the host when the plugin spawns entities (see
//!   `crate::hologram`).
//! * The vanilla death-message broadcast is disabled at load
//!   (`showDeathMessages`): the host has no per-death silent flag, so the
//!   plugin replicates the original's targeted delivery itself.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use pumpkin_plugin_api::block_entity::BlockEntityType;
use pumpkin_plugin_api::common::{BlockPos, Hand};
use pumpkin_plugin_api::data_components::DataComponent;
use pumpkin_plugin_api::events::{
    BlockBreakEvent, BlockPlaceEvent, EventData, EventHandler, InteractAction, PlayerDeathEvent,
    PlayerInteractEvent, SignChangeEvent,
};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Context, GameRule, GameRuleValue, ItemStack, Player, Server};

use crate::utils::core;
use crate::state::state;
use crate::utils;

/// Transient maps (heavily tracked) are dropped wholesale past this size; the
/// entries only matter while a block/item is in active use.
const TRACKED_LIMIT: usize = 4096;

/// Resolved entity ids (`entity_id` -> registry name). The WIT has no entity
/// lookup by id, so the first hit costs one scan over the world entity lists;
/// the host allocates entity ids from a monotonic counter, so it can then be
/// cached for the rest of the session.
static ENTITY_TYPES: Mutex<Option<HashMap<i32, String>>> = Mutex::new(None);

/// Placed player heads (`(world, x, y, z)` -> owner name). `SkullBlockEntity`
/// exposes no owner through the WIT, so the name is captured from the placing
/// item's `profile` component.
static HEADS: Mutex<Option<BTreeMap<(String, i32, i32, i32), String>>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Damage cause normalisation
// ---------------------------------------------------------------------------

/// The damaging entity family dealt with as a projectile (the original's
/// `instanceof Projectile` checks).
const PROJECTILE_FAMILY: &[&str] = &[
    "arrow",
    "trident",
    "thrown",
    "spit",
    "mob_projectile",
    "fireball",
    "unattributed_fireball",
    "wind_charge",
    "fireworks",
    "wither_skull",
    "ender_pearl",
    "spear",
];

/// The melee/ranged attack family (handled as `ENTITY_ATTACK`/"slain by").
const ATTACK_FAMILY: &[&str] = &[
    "player_attack",
    "mob_attack",
    "mob_attack_no_aggro",
    "mace_smash",
    "sonic_boom",
    "sting",
    "spear",
];

/// True when the canonical cause belongs to the projectile family.
pub fn is_projectile_cause(cause: &str) -> bool {
    PROJECTILE_FAMILY.contains(&cause)
}

/// Extracts and normalises the `message_id` out of a host `DamageType` debug
/// string (`format!("{damage_type:?}")` in the damage events).
pub fn cause_from_damage_debug(debug: &str) -> Option<String> {
    let rest = debug.split("message_id: \"").nth(1)?;
    let raw = rest.split('"').next()?;
    Some(normalize_cause(raw))
}

/// Normalises a translation key (`inFire`, `explosion.player`), a WIT enum
/// variant (`InFire`) or a kebab-case registry id (`in-fire`) to the canonical
/// `damage_type` registry id (`in_fire`). The translation keys shared by
/// several registry ids are folded onto their primary id.
pub fn normalize_cause(raw: &str) -> String {
    let mut snake = String::new();
    for (index, ch) in raw.chars().enumerate() {
        if ch == '-' {
            snake.push('_');
            continue;
        }
        if ch.is_ascii_uppercase() {
            if index != 0 && !snake.ends_with('_') {
                snake.push('_');
            }
            snake.push(ch.to_ascii_lowercase());
        } else {
            snake.push(ch);
        }
    }

    match snake.trim_start_matches('_') {
        "player" => "player_attack".to_string(),
        "mob" => "mob_attack".to_string(),
        "explosion.player" => "player_explosion".to_string(),
        other => other.to_string(),
    }
}

/// Registry name of a live entity, resolved from the world entity lists (used
/// by the combat listeners for mob damage and for projectiles whose shooter is
/// not a player). Misses (already despawned entities) are not cached, so a
/// later reuse of the sphere of life resolves it again.
pub(crate) fn entity_key_by_id(server: &Server, entity_id: i32) -> Option<String> {
    {
        let guard = ENTITY_TYPES.lock().unwrap();
        if let Some(name) = guard.as_ref().and_then(|map| map.get(&entity_id)) {
            return Some(name.clone());
        }
    }

    let entity = core::entity_by_id(server, entity_id)?;

    // The world list carries the host's `EntityType`; resolve it to the
    // vanilla resource name used by the death messages.
    let name = core::entity_type_key(&entity.get_type());

    let mut guard = ENTITY_TYPES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if map.len() >= TRACKED_LIMIT * 2 {
        map.clear();
    }
    map.insert(entity_id, name.clone());
    Some(name)
}

/// `StringUtils#getEntityName`: lower-cased, `_` turned into spaces, otherwise
/// just capitalized.
fn entity_name(key: &str) -> String {
    let bare = key
        .strip_prefix("minecraft:")
        .unwrap_or(key)
        .to_ascii_lowercase();

    if bare.contains('_') {
        bare.replace('_', " ")
    } else {
        utils::capitalize(&bare)
    }
}

/// Approximation of the NMS localized item name (`NmsUtils#getItemName`'s
/// fallback): every `_`-separated word capitalized.
fn item_key_name(key: &str) -> String {
    let bare = key.strip_prefix("minecraft:").unwrap_or(key);
    bare.split('_')
        .map(utils::capitalize)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `NmsUtils#getItemName`: the custom display name when set, the material name
/// otherwise.
fn item_display_name(item: &ItemStack) -> String {
    match item.get_custom_name() {
        Some(name) => plain(&name),
        None => item_key_name(&item.get_registry_key()),
    }
}

/// Plain text of a text component (formatting stripped).
fn plain(component: &TextComponent) -> String {
    crate::handlers::block::component_text(&component.to_json())
}

/// `utils::replace_all` as a small alias for the many templates below.
fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    utils::replace_all(template, pairs)
}

// ---------------------------------------------------------------------------
// Death message
// ---------------------------------------------------------------------------

/// `Player#getKiller` replacement: the last player to damage the victim inside
/// the combat window (see `listeners::player::KILLER_WINDOW_MILLIS`).
pub fn resolve_killer(server: &Server, victim_uuid: &str) -> Option<(String, Player)> {
    let killer_uuid = {
        let guard = state().read().unwrap();
        guard.last_damagers.get(victim_uuid).and_then(|(killer, at)| {
            (killer != victim_uuid
                && utils::now_millis() - at <= crate::listeners::player::KILLER_WINDOW_MILLIS)
                .then(|| killer.clone())
        })
    }?;

    let player = pumpkin_plugin_api::uuid::parse(&killer_uuid)
        .and_then(|id| server.get_player_by_uuid(id))?;
    Some((killer_uuid, player))
}

/// Port of `DeathMessageHandler#getDeathMessage`: resolves every placeholder
/// from the recorded last damage, the userdata kill counts and the language
/// file. Must be called before the kill is credited (the killer's displayed
/// count is `kills + 1`, like the original).
pub fn build_message(server: &Server, victim: &Player, killer: Option<&Player>) -> String {
    let victim_uuid = core::uuid_of(victim);
    let victim_name = core::name_of(victim);
    let killer_uuid = killer.map(core::uuid_of);

    // State snapshot (no host calls under the lock).
    let (victim_kills, damage, damager_uuid, damager_kills, killer_kills) = {
        let guard = state().read().unwrap();
        let damage = guard.last_damage.get(&victim_uuid).cloned();
        let damager_uuid = damage.as_ref().and_then(|damage| damage.damager.clone());
        let damager_kills = damager_uuid
            .as_ref()
            .and_then(|uuid| guard.userdata.get(uuid))
            .map(|user| user.kills + 1);
        let killer_kills = killer_uuid
            .as_ref()
            .and_then(|uuid| guard.userdata.get(uuid))
            .map(|user| user.kills + 1);
        (
            guard
                .userdata
                .get(&victim_uuid)
                .map(|user| user.kills)
                .unwrap_or(0),
            damage,
            damager_uuid,
            damager_kills,
            killer_kills,
        )
    };

    // Host calls: the damaging player's online handle and held item (the hand
    // item is taken at death time, like the original).
    let damager = damager_uuid.map(|uuid| {
        let online = pumpkin_plugin_api::uuid::parse(&uuid).and_then(|id| server.get_player_by_uuid(id));
        let name = online.as_ref().map(core::name_of).unwrap_or_else(|| {
            let guard = state().read().unwrap();
            guard
                .userdata
                .get(&uuid)
                .map(|user| user.name.clone())
                .unwrap_or_else(|| uuid.clone())
        });
        (name, online)
    });
    let damager_item = damager
        .as_ref()
        .and_then(|(_, online)| online.as_ref())
        .and_then(|player| player.get_item_in_hand(Hand::Right))
        .map(|item| item_display_name(&item));

    let (killer_name, killer_parts) = match killer {
        Some(killer) => (Some(core::name_of(killer)), Some(killer_kills)),
        None => (None, None),
    };
    let entity_display = damage
        .as_ref()
        .and_then(|damage| damage.entity_key.as_deref())
        .map(entity_name);

    // Template resolution (read lock held, no host calls).
    let guard = state().read().unwrap();
    let language = &guard.raw.language;
    let reason = |key: &str| format!("DEATHMESSAGE.REASON_{key}");

    let player_format = language.string("DEATHMESSAGE.PLAYER_NAME_FORMAT");
    let killer_format = language.string("DEATHMESSAGE.KILLER_NAME_FORMAT");
    let victim_display = player_format
        .replace("<player>", &victim_name)
        .replace("<kills>", &victim_kills.to_string());
    let killer_display = |name: &str, kills: Option<i32>| {
        killer_format
            .replace("<killer>", name)
            .replace("<kills>", &kills.map(|kills| kills.to_string()).unwrap_or_else(|| "?".to_string()))
    };
    let custom = fill(&language.string(&reason("CUSTOM")), &[("<player>", &victim_display)]);

    let Some(damage) = &damage else {
        return custom;
    };
    let cause = damage.cause.as_str();

    if !damage.entity {
        return match cause {
            "explosion" | "player_explosion" | "bad_respawn_point" => {
                fill(&language.string(&reason("BLOCK_EXPLOSION")), &[("<player>", &victim_display)])
            }
            "cactus" | "sweet_berry_bush" => {
                fill(&language.string(&reason("CONTACT")), &[("<player>", &victim_display)])
            }
            "drown" => fill(&language.string(&reason("DROWNING")), &[("<player>", &victim_display)]),
            "in_fire" | "campfire" => {
                fill(&language.string(&reason("FIRE")), &[("<player>", &victim_display)])
            }
            "on_fire" => {
                fill(&language.string(&reason("FIRE_TICK")), &[("<player>", &victim_display)])
            }
            "lava" => fill(&language.string(&reason("LAVA")), &[("<player>", &victim_display)]),
            "magic" | "indirect_magic" | "dragon_breath" => {
                fill(&language.string(&reason("MAGIC")), &[("<player>", &victim_display)])
            }
            "hot_floor" => {
                fill(&language.string(&reason("MELTING")), &[("<player>", &victim_display)])
            }
            "poison" => fill(&language.string(&reason("POISON")), &[("<player>", &victim_display)]),
            "starve" => {
                fill(&language.string(&reason("STARVATION")), &[("<player>", &victim_display)])
            }
            "in_wall" => {
                fill(&language.string(&reason("SUFFOCATION")), &[("<player>", &victim_display)])
            }
            "wither" => fill(&language.string(&reason("WITHER")), &[("<player>", &victim_display)]),
            "thorns" => fill(&language.string(&reason("THORNS")), &[("<player>", &victim_display)]),
            "fall" | "fly_into_wall" => match (&killer_name, killer_parts) {
                (Some(name), Some(kills)) if name != &victim_name => fill(
                    &language.string(&reason("FALL_KILLER")),
                    &[("<player>", &victim_display), ("<killer>", &killer_display(name, kills))],
                ),
                _ => fill(&language.string(&reason("FALL")), &[("<player>", &victim_display)]),
            },
            "out_of_world" => match (&killer_name, killer_parts) {
                (Some(name), Some(kills)) if name != &victim_name => fill(
                    &language.string(&reason("VOID_KILLER")),
                    &[("<player>", &victim_display), ("<killer>", &killer_display(name, kills))],
                ),
                _ => fill(&language.string(&reason("VOID")), &[("<player>", &victim_display)]),
            },
            "generic_kill" => {
                fill(&language.string(&reason("SUICIDE")), &[("<player>", &victim_display)])
            }
            _ => custom,
        };
    }

    // Entity branch: the damager details decide between the player, entity and
    // projectile variants.
    let damager_display = damager.as_ref().map(|(name, _)| name.clone());
    if is_projectile_cause(cause) {
        return match (damager_display.as_deref(), damager_item.as_deref()) {
            (Some(name), Some(item)) => fill(
                &language.string(&reason("PROJECTILE_PLAYER_ITEM")),
                &[
                    ("<player>", &victim_display),
                    ("<killer>", &killer_display(name, damager_kills)),
                    ("<item>", item),
                ],
            ),
            (Some(name), None) => fill(
                &language.string(&reason("PROJECTILE_PLAYER_NO_ITEM")),
                &[
                    ("<player>", &victim_display),
                    ("<killer>", &killer_display(name, damager_kills)),
                ],
            ),
            (None, _) => match &entity_display {
                Some(entity) => fill(
                    &language.string(&reason("PROJECTILE_ENTITY")),
                    &[("<player>", &victim_display), ("<entity>", entity)],
                ),
                None => custom,
            },
        };
    }

    if ATTACK_FAMILY.contains(&cause) {
        return match (damager_display.as_deref(), damager_item.as_deref()) {
            (Some(name), Some(item)) => fill(
                &language.string(&reason("ENTITY_ATTACK_PLAYER_ITEM")),
                &[
                    ("<player>", &victim_display),
                    ("<killer>", &killer_display(name, damager_kills)),
                    ("<item>", item),
                ],
            ),
            (Some(name), None) => fill(
                &language.string(&reason("ENTITY_ATTACK_PLAYER_NO_ITEM")),
                &[
                    ("<player>", &victim_display),
                    ("<killer>", &killer_display(name, damager_kills)),
                ],
            ),
            (None, _) => match &entity_display {
                Some(entity) => fill(
                    &language.string(&reason("ENTITY_ATTACK_ENTITY")),
                    &[("<player>", &victim_display), ("<entity>", entity)],
                ),
                None => custom,
            },
        };
    }

    match cause {
        "explosion" | "player_explosion" | "bad_respawn_point" => {
            if damage.entity_key.as_deref() == Some("tnt") {
                fill(&language.string(&reason("BLOCK_EXPLOSION")), &[("<player>", &victim_display)])
            } else {
                fill(&language.string(&reason("ENTITY_EXPLOSION")), &[("<player>", &victim_display)])
            }
        }
        "falling_anvil" | "falling_block" | "falling_stalactite" => {
            fill(&language.string(&reason("FALLING_BLOCK")), &[("<player>", &victim_display)])
        }
        "lightning_bolt" => {
            fill(&language.string(&reason("LIGHTNING")), &[("<player>", &victim_display)])
        }
        "fall" => fill(&language.string(&reason("FALL")), &[("<player>", &victim_display)]),
        _ => custom,
    }
}

/// `DeathMessageHandler#onPlayerDeath` delivery: the victim, the killer and
/// every online player with the death-messages setting enabled.
pub fn send_death_message(
    server: &Server,
    victim_uuid: &str,
    killer_uuid: Option<&str>,
    message: &str,
) {
    let online: Vec<(String, Player)> = server
        .get_all_players()
        .into_iter()
        .map(|player| (core::uuid_of(&player), player))
        .collect();

    let targets: Vec<Player> = {
        let guard = state().read().unwrap();
        online
            .into_iter()
            .filter(|(uuid, _)| {
                uuid.as_str() == victim_uuid
                    || Some(uuid.as_str()) == killer_uuid
                    || guard
                        .userdata
                        .get(uuid)
                        .map(|user| user.settings.death_messages)
                        .unwrap_or(true)
            })
            .map(|(_, player)| player)
            .collect()
    };

    for player in &targets {
        core::send(player, message);
    }
}

/// `PlayerFaction#onDeath` plus the silent warmup cancels, both guarded by the
/// faction check in `PlayerEventListener#onPlayerDeath`. Shared by the player
/// death listener and the combat logger's offline victim; returns the faction
/// id and the `FACTIONS.MEMBER_DEATH` message for the caller to deliver.
pub(crate) fn faction_on_death(
    guard: &mut crate::state::AppState,
    uuid: &str,
    name: &str,
    world: &str,
    config: &crate::config::Config,
) -> Option<(String, String)> {
    let faction_id = guard.factions.by_member(uuid).map(|f| f.id.clone())?;
    let frozen = guard.timers.is_frozen(&faction_id);

    // Kitmap can disable DTR loss (and the freeze) entirely, while still
    // awarding the death points and sending the message with a zero loss.
    let skip_dtr_loss = config.kitmap.enabled && config.kitmap.disable_dtr_loss;
    let loss = if skip_dtr_loss {
        0.0
    } else {
        match core::dimension_of(world) {
            core::Dimension::Overworld => config.factions.dtr_death_loss.overworld,
            core::Dimension::Nether => config.factions.dtr_death_loss.nether,
            core::Dimension::End => config.factions.dtr_death_loss.end,
        }
    };
    let template = guard.raw.language.string("FACTIONS.MEMBER_DEATH");

    if !skip_dtr_loss {
        let faction = guard.factions.get_mut(&faction_id)?;
        let amount = faction.dtr - loss;
        faction.set_dtr(config, amount);
    }

    // `PlayerFaction#changePoints` (fires the faction points leaderboard).
    let points = guard.factions.get(&faction_id)?.points + config.factions.points_per_death;
    crate::handlers::leaderboard::faction_change_points(guard, &faction_id, points);

    let faction = guard.factions.get(&faction_id)?;
    let loss_string = format!("-{}", crate::utils::java_double(loss));
    let dtr_string = faction.dtr_string(config, frozen);
    let max_dtr_string = faction.max_dtr_string(config);
    let message = crate::utils::replace_all(
        &template,
        &[
            ("<player>", name),
            ("<dtrLoss>", &loss_string),
            ("<dtr>", &dtr_string),
            ("<maxDtr>", &max_dtr_string),
        ],
    );

    // `FactionFreezeTimer#activate(this)`.
    if !skip_dtr_loss {
        guard
            .timers
            .freeze_faction(&faction_id, config.factions.freeze_duration_minutes * 60);
    }

    guard.timers.clear_pending(uuid);

    Some((faction_id, message))
}

// ---------------------------------------------------------------------------
// Stattrak
// ---------------------------------------------------------------------------

/// Port of `StattrakHandler#updateStattrakLore`.
fn update_stattrak(victim: &Player, killer: &Player, item: &ItemStack) {
    let (enabled, tracking, kills_format, kill_string) = {
        let guard = state().read().unwrap();
        (
            guard.raw.config.bool("STAT_TRAK.ENABLED"),
            guard.raw.config.string_list("STAT_TRAK.TRACKING_ITEMS"),
            guard.raw.config.string("STAT_TRAK.KILLS_FORMAT"),
            guard.raw.config.string("STAT_TRAK.KILL_STRING"),
        )
    };

    let key = item.get_registry_key();
    let bare = key
        .strip_prefix("minecraft:")
        .unwrap_or(&key)
        .to_ascii_lowercase();
    if !enabled || !tracking.iter().any(|entry| tracking_matches(entry, &bare)) {
        return;
    }

    let kill_line = |victim: &Player, killer: &Player| {
        utils::text(utils::replace_all(
            &kill_string,
            &[
                ("<player>", &core::name_of(victim)),
                ("<killer>", &core::name_of(killer)),
                ("<date>", &crate::utils::datetime::now_from_config()),
            ],
        ))
    };
    let kills_line = |kills: i64| utils::text(kills_format.replace("<kills>", &kills.to_string()));

    let mut lore = item.get_lore();
    let count = stattrak_kill_count(&lore, &kills_format);

    if lore.is_empty() || count == 0 {
        item.set_lore(vec![
            utils::text(""),
            kills_line(1),
            utils::text(""),
            kill_line(victim, killer),
        ]);
        return;
    }

    let formatted = plain(&kills_line(count));
    let Some(index) = lore.iter().position(|line| plain(line) == formatted) else {
        return;
    };

    let size = lore.len();
    let line = kill_line(victim, killer);
    match size - (index + 1) {
        2 => {
            lore[index] = kills_line(count + 1);
            lore.insert(size - 1, line);
        }
        3 => {
            lore[index] = kills_line(count + 1);
            lore.insert(size - 2, line);
        }
        4 => {
            lore[index] = kills_line(count + 1);
            lore.insert(size - 3, line);
            lore.remove(lore.len() - 1);
        }
        _ => return,
    }

    item.set_lore(lore);
}

/// The original compares the Bukkit `Material` enum name (`GOLD_SWORD`,
/// `WOOD_SWORD`, …). The config may carry either the legacy material name or
/// the modern registry name, so both are accepted for the 1.13 renames.
fn tracking_matches(entry: &str, bare_registry_key: &str) -> bool {
    let entry = entry.trim();
    if entry.eq_ignore_ascii_case(bare_registry_key) {
        return true;
    }

    let legacy = if let Some(rest) = bare_registry_key.strip_prefix("golden_") {
        format!("GOLD_{rest}")
    } else if let Some(rest) = bare_registry_key.strip_prefix("wooden_") {
        format!("WOOD_{rest}")
    } else {
        return false;
    };

    entry.eq_ignore_ascii_case(&legacy)
}

/// `StattrakHandler#getKillCount`: the first digit run of the first lore line
/// that starts with the kills format (prefix before `<kills>`).
fn stattrak_kill_count(lore: &[TextComponent], kills_format: &str) -> i64 {
    let prefix = plain(&utils::text(kills_format.replace("<kills>", "")));
    lore.iter()
        .map(plain)
        .find(|line| line.starts_with(&prefix))
        .and_then(|line| first_number(line.as_str()))
        .unwrap_or(0)
}

fn first_number(input: &str) -> Option<i64> {
    let mut digits = String::new();
    for ch in input.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

/// `StattrakHandler#onPlayerDeath`.
pub struct StattrakListener;

impl EventHandler<PlayerDeathEvent> for StattrakListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerDeathEvent>,
    ) -> EventData<PlayerDeathEvent> {
        let victim_uuid = core::uuid_of(&event.player);
        let Some((killer_uuid, killer)) = resolve_killer(&server, &victim_uuid) else {
            return event;
        };
        if killer_uuid == victim_uuid {
            return event;
        }

        let Some(item) = killer.get_item_in_hand(Hand::Right) else {
            return event;
        };
        update_stattrak(&event.player, &killer, &item);
        event
    }
}

// ---------------------------------------------------------------------------
// Head drops
// ---------------------------------------------------------------------------

fn head_drop_config() -> (bool, String) {
    let guard = state().read().unwrap();
    (
        guard.raw.config.bool("HEAD_DROP.ENABLED"),
        guard.raw.config.string("HEAD_DROP.SKULL_NAME_FORMAT"),
    )
}

fn track_head(key: (String, i32, i32, i32), owner: String) {
    let mut guard = HEADS.lock().unwrap();
    let map = guard.get_or_insert_with(BTreeMap::new);
    if map.len() >= TRACKED_LIMIT {
        map.clear();
    }
    map.insert(key, owner);
}

fn tracked_head(key: &(String, i32, i32, i32)) -> Option<String> {
    HEADS.lock().unwrap().as_ref()?.get(key).cloned()
}

fn untrack_head(key: &(String, i32, i32, i32)) {
    if let Some(map) = HEADS.lock().unwrap().as_mut() {
        map.remove(key);
    }
}

/// Builds the killer's trophy head (name + name-only `profile`).
fn head_item(name_format: &str, owner: &str) -> ItemStack {
    let item = ItemStack::new("minecraft:player_head", 1);
    item.set_custom_name(Some(utils::text(name_format.replace("<player>", owner))));
    item.set_component(DataComponent::Profile, &profile_component(owner));
    item
}

/// Encodes a name-only `ResolvableProfile` (`ProfileImpl` wire format: varint
/// discriminator, optional name/uuid, properties, texture/cape/elytra/model).
fn profile_component(name: &str) -> Vec<u8> {
    let mut out = vec![1u8, 1u8]; // varint(1) + "has name"
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
    // No uuid, no properties, no texture/cape/elytra/model.
    out.extend_from_slice(&[0u8; 6]);
    out
}

/// Reads the owner name back out of a `profile` component patch written by
/// [`profile_component`].
fn profile_name(value: &[u8]) -> Option<String> {
    let mut index = 0usize;
    // Varint discriminator (skips continuation bytes).
    while value.get(index).is_some_and(|byte| *byte & 0x80 != 0) {
        index += 1;
    }
    index += 1;
    if value.get(index) != Some(&1) {
        return None;
    }
    index += 1;

    let length = u16::from_be_bytes([*value.get(index)?, *value.get(index + 1)?]) as usize;
    index += 2;
    let bytes = value.get(index..index + length)?;
    String::from_utf8(bytes.to_vec()).ok()
}

/// `killer.hasPermission("lazarus.headdrop")` with the host's node rules: the
/// original node stays grantable through permission attachments, while the
/// registered colon node carries the op default (Bukkit granted undefined
/// nodes to operators, the host does not).
fn has_headdrop_permission(player: &Player) -> bool {
    player.has_permission("lazarus.headdrop") || player.has_permission("lazarus:headdrop")
}

/// `HeadDropHandler#onPlayerDeath`: the killer receives the victim's head when
/// holding `lazarus.headdrop`.
pub struct HeadDropDeathListener;

impl EventHandler<PlayerDeathEvent> for HeadDropDeathListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerDeathEvent>,
    ) -> EventData<PlayerDeathEvent> {
        let (enabled, name_format) = head_drop_config();
        if !enabled {
            return event;
        }

        let victim_uuid = core::uuid_of(&event.player);
        let Some((killer_uuid, killer)) = resolve_killer(&server, &victim_uuid) else {
            return event;
        };
        if killer_uuid == victim_uuid || !has_headdrop_permission(&killer) {
            return event;
        }

        let victim_name = core::name_of(&event.player);
        crate::handlers::block::give_stack_to_inventory(&killer, head_item(&name_format, &victim_name));
        event
    }
}

/// Port mechanism: the placing item's `profile` component is the only owner
/// information available, so it is captured here.
pub struct HeadTrackPlaceListener;

impl EventHandler<BlockPlaceEvent> for HeadTrackPlaceListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<BlockPlaceEvent>,
    ) -> EventData<BlockPlaceEvent> {
        if event.cancelled || !event.block_placed.ends_with("head") {
            return event;
        }

        let Some(owner) = [Hand::Right, Hand::Left].into_iter().find_map(|hand| {
            event
                .player
                .get_item_in_hand(hand)
                .filter(|item| item.get_registry_key() == "minecraft:player_head")
                .and_then(|item| {
                    item.get_components()
                        .into_iter()
                        .find(|component| component.component == DataComponent::Profile)
                        .and_then(|component| profile_name(&component.value))
                })
        }) else {
            return event;
        };

        let world_name = core::world_name(&event.player);
        track_head(
            (world_name, event.block_pos.x, event.block_pos.y, event.block_pos.z),
            owner,
        );
        event
    }
}

/// `HeadDropHandler#onBlockBreak`: a tracked head is given back to the breaker
/// (the original drops it at the block).
pub struct HeadDropBreakListener;

impl EventHandler<BlockBreakEvent> for HeadDropBreakListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<BlockBreakEvent>,
    ) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        let (enabled, name_format) = head_drop_config();
        if !enabled || !event.block.ends_with("head") {
            return event;
        }

        let Some(player) = event.player.as_ref() else {
            return event;
        };
        let world_name = core::world_name(player);
        let key = (
            world_name.clone(),
            event.block_pos.x,
            event.block_pos.y,
            event.block_pos.z,
        );
        let Some(owner) = tracked_head(&key) else {
            return event;
        };

        event.cancelled = true;
        crate::handlers::block::give_stack_to_inventory(player, head_item(&name_format, &owner));

        if let Some(world) = server.get_world_by_name(&world_name) {
            let pos = BlockPos { x: key.1, y: key.2, z: key.3 };
            let _ = world.set_block_by_name(pos, "minecraft:air", crate::handlers::block::air_flags());
        }
        untrack_head(&key);
        event
    }
}

/// `HeadDropHandler#onPlayerInteract`: right-clicking a tracked head shows its
/// owner.
pub struct HeadDropInteractListener;

impl EventHandler<PlayerInteractEvent> for HeadDropInteractListener {
    fn handle(
        &self,
        _server: Server,
        event: EventData<PlayerInteractEvent>,
    ) -> EventData<PlayerInteractEvent> {
        let (enabled, _) = head_drop_config();
        if !enabled || event.cancelled || event.action != InteractAction::RightClickBlock {
            return event;
        }
        if !event.block.ends_with("head") {
            return event;
        }
        let Some(pos) = event.clicked_pos else {
            return event;
        };

        let world_name = core::world_name(&event.player);
        let Some(owner) = tracked_head(&(world_name, pos.x, pos.y, pos.z)) else {
            return event;
        };

        let message = state().read().unwrap().raw.language.string("HEAD_DROP.CLICK_MESSAGE");
        core::send(&event.player, utils::replace(&message, "<player>", &owner));
        event
    }
}

// ---------------------------------------------------------------------------
// Death signs
// ---------------------------------------------------------------------------

fn death_sign_config() -> (bool, String, Vec<String>) {
    let guard = state().read().unwrap();
    (
        guard.raw.config.bool("DEATH_SIGN.ENABLED"),
        guard.raw.config.string("DEATH_SIGN.NAME"),
        guard.raw.config.string_list("DEATH_SIGN.LORE"),
    )
}

/// `DeathSignHandler#isDeathSign`: the second line carries the configured
/// marker line (line indexes are 0-based here).
fn is_death_sign(lines: &[String], lore: &[String]) -> bool {
    let Some(wanted) = lore.get(1) else {
        return false;
    };
    let wanted = plain(&utils::text(wanted));
    lines.len() > 1 && lines[1] == wanted
}

/// `DeathSignHandler#onPlayerDeath`: the death sign goes to the killer (the
/// original puts it into the victim's drops).
pub struct DeathSignDeathListener;

impl EventHandler<PlayerDeathEvent> for DeathSignDeathListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<PlayerDeathEvent>,
    ) -> EventData<PlayerDeathEvent> {
        let (enabled, name, lore) = death_sign_config();
        if !enabled {
            return event;
        }

        let victim_uuid = core::uuid_of(&event.player);
        let Some((killer_uuid, killer)) = resolve_killer(&server, &victim_uuid) else {
            return event;
        };
        if killer_uuid == victim_uuid {
            return event;
        }

        let killer_name = core::name_of(&killer);
        let victim_name = core::name_of(&event.player);
        let date = crate::utils::datetime::now_from_config();

        let item = ItemStack::new("minecraft:oak_sign", 1);
        item.set_custom_name(Some(utils::text(&name)));
        item.set_lore(
            lore.iter()
                .map(|line| {
                    utils::text(utils::replace_all(
                        line,
                        &[
                            ("<killer>", &killer_name),
                            ("<victim>", &victim_name),
                            ("<date>", &date),
                        ],
                    ))
                })
                .collect(),
        );
        crate::handlers::block::give_stack_to_inventory(&killer, item);
        event
    }
}

/// `DeathSignHandler#onBlockBreak`: death signs break into their named item and
/// leave air behind.
pub struct DeathSignBreakListener;

impl EventHandler<BlockBreakEvent> for DeathSignBreakListener {
    fn handle(
        &self,
        server: Server,
        mut event: EventData<BlockBreakEvent>,
    ) -> EventData<BlockBreakEvent> {
        if event.cancelled {
            return event;
        }

        let (_, name, lore) = death_sign_config();
        let Some(player) = event.player.as_ref() else {
            return event;
        };
        let world_name = core::world_name(player);
        let Some(world) = server.get_world_by_name(&world_name) else {
            return event;
        };
        let pos = BlockPos {
            x: event.block_pos.x,
            y: event.block_pos.y,
            z: event.block_pos.z,
        };
        let Some(BlockEntityType::SignBlockEntity(sign)) = world.get_block_entity(pos) else {
            return event;
        };

        let messages = sign.get_front_text().messages;
        let lines: Vec<String> = messages
            .iter()
            .map(|message| crate::handlers::block::component_text(message))
            .collect();
        if !is_death_sign(&lines, &lore) {
            return event;
        }

        event.cancelled = true;
        let item = ItemStack::new("minecraft:oak_sign", 1);
        item.set_custom_name(Some(utils::text(&name)));
        // The original reuses `Sign#getLines()` verbatim (keeping the sign's own
        // formatting); the WIT exposes the raw components, so they are parsed
        // back and reused as the item lore.
        item.set_lore(
            messages
                .iter()
                .map(|message| {
                    TextComponent::from_json(message).unwrap_or_else(|_| {
                        utils::text(crate::handlers::block::component_text(message))
                    })
                })
                .collect(),
        );
        crate::handlers::block::give_stack_to_inventory(player, item);
        let _ = world.set_block_by_name(pos, "minecraft:air", crate::handlers::block::air_flags());
        event
    }
}

/// `DeathSignHandler#onBlockPlace`: a sign placed from the death sign item is
/// filled with the item's lore right away (the original's `sign.setLine` loop;
/// the original throws when the lore has fewer than four lines, the port fills
/// the lines the lore provides).
pub struct DeathSignPlaceListener;

impl EventHandler<BlockPlaceEvent> for DeathSignPlaceListener {
    fn handle(
        &self,
        server: Server,
        event: EventData<BlockPlaceEvent>,
    ) -> EventData<BlockPlaceEvent> {
        if event.cancelled || !event.block_placed.ends_with("_sign") {
            return event;
        }

        // `item.getItemMeta().getDisplayName().equals(Config.DEATH_SIGN_NAME)`
        // (both sides go through the config's/setter's color translation).
        let (_, name, _) = death_sign_config();
        let wanted = utils::text(&name).to_json();
        let Some(item) = [Hand::Right, Hand::Left].into_iter().find_map(|hand| {
            event
                .player
                .get_item_in_hand(hand)
                .filter(|item| item.get_custom_name().map(|name| name.to_json()) == Some(wanted.clone()))
        }) else {
            return event;
        };

        let messages: Vec<String> = item.get_lore().iter().take(4).map(|line| line.to_json()).collect();
        if messages.is_empty() {
            return event;
        }

        let world_name = core::world_name(&event.player);
        let Some(world) = server.get_world_by_name(&world_name) else {
            return event;
        };
        if let Some(BlockEntityType::SignBlockEntity(sign)) = world.get_block_entity(event.block_pos) {
            let mut text = sign.get_front_text();
            text.messages = messages;
            sign.set_front_text(&text);
        }

        event
    }
}

/// `DeathSignHandler#onSignChange`: edits of an existing death sign are
/// discarded (the block keeps the plugin's lines).
pub struct DeathSignSignChangeListener;

impl EventHandler<SignChangeEvent> for DeathSignSignChangeListener {
    fn handle(
        &self,
        _server: Server,
        mut event: EventData<SignChangeEvent>,
    ) -> EventData<SignChangeEvent> {
        if event.cancelled {
            return event;
        }

        let world = event.player.get_world();
        let pos = event.block_pos;

        let (_, _, lore) = death_sign_config();
        if let Some(lines) = crate::handlers::block::sign_lines(&world, pos)
            && is_death_sign(&lines, &lore)
        {
            event.cancelled = true;
        }
        event
    }
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

/// Registers the `lazarus.headdrop` node and turns off the vanilla
/// death-message broadcast (the host has no per-death silent flag; the plugin
/// replicates the original's targeted delivery itself).
pub fn init(context: &Context, server: &Server) {
    // The original checked `lazarus.headdrop`, an undefined Bukkit node (which
    // falls back to `isOp()`); the host neither allows dot nodes in the
    // registry nor grants unknown nodes, so the op default lives on the colon
    // variant and both are checked (`has_headdrop_permission`).
    if let Err(error) = crate::commands::register_op_permission(
        context,
        "lazarus:headdrop",
        "Receive player head drops",
    ) {
        tracing::warn!("failed to register lazarus:headdrop: {error}");
    }

    for world in server.get_all_worlds() {
        if world.get_game_rule_bool(GameRule::ShowDeathMessages) != Some(false) {
            world.set_game_rule(GameRule::ShowDeathMessages, GameRuleValue::Bool(false));
            tracing::info!(
                "disabled the vanilla death messages of '{}' (handled by the plugin)",
                world.get_name()
            );
        }
    }
}

# Porting Lazarus → Pumpkin

This document explains what it means to port **Lazarus** (an advanced HCF core for
Bukkit/Spigot 1.7–1.8) to **Pumpkin**, what was ported, what was intentionally
omitted, and the roadmap for the rest.

## 1. The fundamental difference

| | Original (`Lazarus-master`) | This port |
| :--- | :--- | :--- |
| Server | Spigot/Bukkit 1.7–1.8 (Java) | Pumpkin (Rust) |
| Plugin format | `.jar` (JVM classes) | `.wasm` (WebAssembly Component) |
| Language | Java 8 | Rust |
| Server version target | 1.7 / 1.8 | modern (1.21+ protocol) |
| Data access | NMS packets, reflection | Pumpkin plugin API (WIT) |
| Datastore | JSON / MongoDB | JSON (plugin data folder) |
| Config | `config.yml` (YAML) | `config.toml` / `lang.toml` (TOML) |

Pumpkin **cannot run Java plugins**. Its plugin engine loads Wasmtime components
compiled from Rust, Python, C#, Go, C, Kotlin or Zig. This port is therefore a
**rewrite of the same feature set in Rust against Pumpkin's plugin API**, not a
binary-compatible port. NMS/reflection code from the original
(`utils/nms/NmsUtils_1_7|1_8.java`, `scoreboard/nms/*`, `tab/nms/*`, etc.) has no
equivalent and is replaced by high-level Pumpkin APIs (scoreboard, teams, boss
bars, item components, world/block API, `TextDisplay` entities, …).

### Requested omissions

* **Knockback configuration** — omitted (`utils/.../KnockbackHandler` in some
  forks, knockback multipliers, `EntityKnockbackByEntityEvent` tuning).
* **LunarClient integration** — omitted entirely (`lunarclient/` package,
  waypoints, team view, nametags, cooldown packets, server rules, forced mods).

## 2. Build & install

```bash
rustup target add wasm32-wasip2
cargo build --release
# output: target/wasm32-wasip2/release/lazarus_pumpkin.wasm
```

Copy the `.wasm` into the server's `plugins/` folder. On first run the plugin
creates its data folder with default `config.toml`, `lang.toml`,
`factions.json`, `userdata.json`, `claims.json` and `timers.json`.

Because the plugin persists data it requests the `fs.read.data` and
`fs.write.data` permissions. In `pumpkin.toml`:

```toml
[plugins]
enabled = true
allow_unsigned = true
allowed_permissions = ["fs.read.data", "fs.write.data"]
```

## 3. Module mapping

Legend: ✅ ported · 🟡 partial/simplified · ⬜ roadmap (not ported yet)

| Lazarus package / feature | Status | Notes |
| :--- | :---: | :--- |
| `config.Config`, `config.Language` | ✅ | now TOML; legacy `MATERIAL_ID` (`"294:0"`) replaced by registry keys |
| `factions.*` (model, manager, claims, roles, relations) | ✅ | flattened `Faction` + `FactionKind`; fast chunk claim index |
| `factions.commands.player.*` | ✅ | core + admin subcommands; selection-wand claiming (see below) |
| `factions.commands.admin.*` | 🟡 | setdtr/setpoints/setbalance/setlives/forcekick/forceleader/freeze/thaw/save |
| `factions.listeners.*` | 🟡 | join/leave/move/death/chat/block/damage/command/interact ported |
| `factions.event.*` (Bukkit events) | ⬜ | replaced by direct calls; no custom Bukkit event bus in Pumpkin |
| `userdata.Userdata`, `UserdataManager` | ✅ | JSON persistence (Mongo dropped; sandbox only exposes the data folder) |
| `timer.*` (combat tag, apple, enderpearl, pvp prot, home, stuck, logout) | ✅ | timer storage, scoreboard + consumption hooks (pearl/apple) |
| `timer.cooldown.DtrRegenTimer` | ✅ | DTR regen in the 1s task |
| `scoreboard.*` | ✅ | config template (`scoreboard.toml`) + placeholders + teams |
| `tab.*` | ✅ | header/footer + per-player name/order/latency (`tab.toml`) |
| `hologram.*` | ✅ | `TextDisplay` entities + `/hologram` commands |
| `handlers.chat.*` | 🟡 | faction/ally chat + format; blocked commands handled |
| `handlers.timer.*` | ✅ | combat tag, pvp protection (banked/paused in safezones), SOTW timer + hide-players |
| `handlers.block.*` (auto-smelt, cobweb, crowbar, filter, subclaim, found ore) | ⬜ | |
| `handlers.death.*` (death messages, death signs, head drop, stattrak) | ⬜ | |
| `handlers.limiter.*` (enchant/potion/entity limits) | ⬜ | |
| `handlers.logger.*` (combat logger) | ⬜ | |
| `handlers.leaderboard.*` | 🟡 | `/f top` + `/f list`; leaderboard holograms TODO |
| `abilities.*` | ⬜ | large subsystem; ported last |
| `classes.*` (Archer, Bard, Miner, Rogue, Mage, Bomber) | ⬜ | |
| `kits.*` | ✅ | inventory capture/give incl. lore + vanilla enchantments (`kits.json`) |
| `games.*` (Koth, Conquest, DTC, Dragon, King, Mountain, Loot, Schedule) | ⬜ | |
| `economy.*` + `integration.vault` | 🟡 | internal balance implemented; Vault has no Pumpkin equivalent |
| `deathban.*` | ✅ | `deathban_until` on userdata + `/lives` |
| `staffmode.*`, `VanishManager` | 🟡 | vanish via `hide_player/show_player`; staff-mode flag |
| `glass.*`, claim pillars, `/f map` visuals | ✅ | client-side `send_block_change` engine |
| `selection.*` (WorldEdit-style wand) | ✅ | `/f claim` wand with sneak-purchase |
| `integration.Chat_*` (permission plugins) | ⬜ | Pumpkin has its own permission system |
| `lunarclient.*` | 🚫 | omitted as requested |
| Knockback tuning | 🚫 | omitted as requested |
| NMS (`utils/nms`, `scoreboard/nms`, `tab/nms`, `logger/nms`, `dragon/nms`, abilities reflection) | 🚫 | not portable (Java bytecode) |

## 4. Design decisions

* **One `Faction` struct** instead of the `Faction` → `PlayerFaction` / `SystemFaction`
  hierarchy. A `FactionKind` enum (`Player`, `Wilderness`, `Warzone`, `Safezone`,
  `Road`) replaces the class hierarchy so serialisation stays trivial.
* **UUIDs are stored as strings** (`uuid` in WIT is a record; strings are the
  stable on-disk form).
* **Claims are still rectangles**, but a `chunk -> faction` index is maintained so
  `territory_at(world, x, z)` is O(1) instead of scanning every claim.
* **Single-chunk claiming** is replaced by the original **selection wand**: `/f claim`
  gives a golden-hoe wand; left/right click set positions, sneak + left-click buys.
  Selection pillars and claim maps are rendered entirely client-side (`glass.rs`).
* **A single global `RwLock<AppState>`** mirrors the original singletons. Handlers
  avoid holding the lock across host calls that can re-enter the plugin.
* **Economy** is internal (`Userdata.balance` + `Faction.balance`); Vault does not
  exist on Pumpkin.
* **Time** is measured in unix milliseconds for timers, using Pumpkin's scheduler
  (20 ticks/second).
* **PvP protection is banked, not ticking, inside safezones**: the full
  `PvpProtTimer` model is ported — `pvp_banked` holds the paused remainder,
  `is_active`/`remaining` report paused protection as active and every consumer
  (scoreboard, class gates, glass walls, claim entry) sees it.
* **SOTW** keeps the original's runtime state in memory (`enabled_players`,
  `hide_players`) and persists only the timer (`timers.json` → `sotw_until`),
  matching `timers.yml` `SotwTimer`.

### Incomplete ports caused by host API limits

Final where the API allows; the following pieces of the originals could not be
ported and are documented instead of silently diverging (see the sources for
context):

* `PVP_PROTECTION.DISABLE_END_ENTRY` — the host teleport event carries no cause,
  so end-portal teleports cannot be told apart from any other teleport.
* `PVP_PROTECTION.DISABLED_ITEMS` / `PICKUP_DENY_ITEMS` — dead code in the
  original snapshot (no consumption path); not ported, like the original.
* SOTW auto-inventory drops (`onBlockBreak`) — the host block-drop event fires
  without the owning player, so drops cannot be routed into the miner's
  inventory; they stay on the ground.
* SOTW drops-to-killer (`onEntityDeath`) — the entity-drop event has no killer
  id in the host API.
* SOTW nametag colour via `updateTabRelations` — the port has no nametag team
  system.
* Enderpearl refund on a denied protected teleport — the cancellation works,
  the item refund is not implemented.
* `CreatureSpawnEvent` — the gate is ported but the host does not fire it for
  every spawn path, so the SOTW spawner-only rule is best-effort.

## 5. Roadmap

Recently completed: client-side block render (`glass.rs`), selection wand,
scoreboard placeholders, tab list, enderpearl/gapple cooldowns, deathbans + lives,
admin faction subcommands, vanish/staff mode, holograms, kits, PvP protection
phase timers (safe-time banking, `/pvp`, attacker-side denial, `/f home` gate)
and the SOTW system (`/sotw`, damage immunity, safezone hiding, scoreboard line).

Still to port (in rough priority order):

1. Combat logger (spawn a mob holding the player's drops on disconnect).
2. Abilities + PvP classes (Archer/Bard/Miner/Rogue/Mage/Bomber).
3. Events/games (Koth, Conquest, DTC, Ender Dragon, King of the Hill, Mountain,
   Loot, Schedule) using boss bars + the world API.
4. Block handlers (auto-smelt, cobweb remover, crowbar, filter, subclaim signs,
   found-ore, elevators, mob stacking).
5. Death handlers (death messages, death signs, head drops, stattrak).
6. Limiters (enchant/potion/entity limits), custom recipes, sign shops.
7. World border, portals/end-exit, safezone food/potion rules.
8. Leaderboard holograms.

## 6. Files

```
src/
├── lib.rs              plugin entry (metadata, permissions, lifecycle)
├── state.rs            global state + JSON/TOML persistence
├── config.rs           Config (config.toml, tab.toml, scoreboard.toml, deathban)
├── lang.rs             Lang (lang.toml)
├── util.rs             colour/string/time helpers
├── core.rs             host-API bridge + territory resolution
├── scoreboard.rs       sidebar renderer (placeholders + teams)
├── tab.rs              tab list header/footer + names
├── glass.rs            client-side block render engine (send_block_change)
├── selection.rs        claim wand + selection state
├── timers.rs           TimerManager (player/faction/custom timers, PvP-prot banking, SOTW)
├── staffmode.rs        vanish + staff mode
├── sotw.rs             Start-Of-The-World timer, immunity + safezone hiding
├── hologram.rs         TextDisplay holograms
├── kits.rs             inventory kits (lore + enchantments)
├── deathban.rs         deathban / lives handling
├── tasks.rs            repeating tick (timers, DTR, teleports, glass, sidebar, tab)
├── userdata.rs         Userdata + UserdataManager
├── factions/
│   ├── types.rs        Faction, Role, Relation, ChatType, Location
│   ├── claim.rs        Claim + ClaimManager (chunk index)
│   └── manager.rs      FactionsManager
├── commands/
│   ├── mod.rs          registration helpers
│   ├── base.rs         /lazarus + setting toggles
│   ├── faction.rs      /f command tree (+ admin subcommands)
│   ├── lives.rs        /lives
│   ├── staff.rs        /vanish, /staffmode
│   ├── hologram.rs     /hologram
│   ├── pvp.rs          /pvp (enable / time)
│   ├── sotw.rs         /sotw (start/stop/hideplayers/enable/time)
│   └── kit.rs          /kit
└── listeners/
    ├── player.rs       join/leave/move/death/respawn/teleport
    ├── chat.rs         faction/ally chat
    ├── combat.rs       friendly fire, combat tag, pvp protection
    ├── block.rs        claim block protection
    ├── command.rs      blocked commands
    ├── interact.rs     claim wand interactions + sneak tracking
    └── items.rs        enderpearl / golden apple cooldowns
```

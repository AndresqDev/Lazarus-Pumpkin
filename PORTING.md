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
| `factions.commands.player.*` | ✅ | core + admin subcommands; `/f show`/`i`/`info`/`who [faction\|player]` with the original's dual lookup (`getFactionByName` + `getPlayerFaction`) and the `SystemFaction`/`RoadFaction` information panels (claims block included; roads reproduce the original's raw `true`/`false` toggles and literal `<abilities>` placeholder); selection-wand claiming (see below) |
| `factions.commands.admin.*` | ✅ | setdtr/adddtr/removedtr/setpoints/setbalance/setlives/setkills/setfreeze(`setregen`)/thaw/forcekick/forceleader/forcerename/system*/color/deathban/save/… — messages + usage from the raw `language.yml` |
| `factions.listeners.*` | 🟡 | join/leave/move/death/chat/block (break/place/bucket/grief) + block-use protection/damage/command/wand ported; hanging (paintings/item frames) + `PHYSICAL` interacts pending |
| `factions.event.*` (Bukkit events) | ⬜ | replaced by direct calls; no custom Bukkit event bus in Pumpkin |
| `userdata.Userdata`, `UserdataManager` | ✅ | JSON persistence (Mongo dropped; sandbox only exposes the data folder) |
| `timer.*` (combat tag, apple, enderpearl, pvp prot, home, stuck, logout) | ✅ | timer storage, scoreboard + consumption hooks (pearl/apple); `/logout` warmup with the `LogoutHandler` move/damage/quit cancels |
| `timer.cooldown.DtrRegenTimer` | ✅ | one `REGEN_PER_MINUTE` step per minute; `setDtr` clamp + 2-decimal rounding |
| `timer.cooldown.RankReviveTimer` | ✅ | persisted player timer (`TimerType::RankRevive`), `MM:SS` dynamic clock in the cooldown message, expiry message through the tick |
| `timer.custom.CustomTimer` | ✅ | `/customtimer start/cancel/list` + sidebar lines; stored as expiry millis (the original was memory-only) |
| `scoreboard.*` | ✅ | config template (`scoreboard.toml`) + placeholders + teams |
| `tab.*` | ✅ | header/footer + per-player name/order/latency (`tab.toml`) |
| `hologram.*` | ✅ | `TextDisplay` entities (normal + leaderboard layouts, the original's numeric-id list with the `decrementId` shift and `BASE - 0.25 * index` line spacing) + `/hologram` commands; `<dataFolder>/holograms/holograms.json` |
| `handlers.chat.*` | 🟡 | faction/ally chat + format; blocked commands handled |
| `handlers.timer.*` | ✅ | combat tag, pvp protection (banked/paused in safezones), SOTW timer + hide-players |
| `handlers.block.*` (auto-smelt, cobweb, counter, crowbar, filter, subclaim, found ore) | 🟡 | auto-smelt (ingot replacement in the host drop event), cobweb remover (1s task, 50/cycle), disabled block placement, found ore (vein count capped at 50, per-player setting + `lazarus.foundore.receive`), subclaim (creation sign, open/break denial, hopper pull block), crowbar (`/crowbar give`, lore-counted spawner/portal-frame extraction with spawner-type restore) ported; filter needs a player pickup event the host does not fire |
| ↳ subclaim deviations | | host applies the packet's sign lines verbatim and ignores `SignChangeEvent#line` edits (`set_front_text` updates the block entity but sends no client update), so signs keep what the player typed: the checks accept the typed `[subclaim]`/`[s]` titles and the plain `leader`/`coleader`/`captain`/`officer` keywords next to the configured values, and an empty second line is emulated with an in-memory creator registry instead of being filled with the creator's name; hopper pulls are blocked through the host's `HopperInventorySearchEvent` (the host fires `InventoryMoveItemEvent` only for hopper pushes, where the original never cancelled) |
| ↳ crowbar deviations | | extracted spawners/portal frames go straight to the inventory (no item-spawn API): the extracted spawner's `block_entity_data` is queued as pending block-entity NBT on `BlockPlaceEvent` (the host never applies the item component to the placed spawner), so the mob type survives while the client shows its predicted empty spawner until the chunk reloads; `Effect.STEP_SOUND` is the block break sound only (no block-state particle payload in the host API); numeric `CROWBAR.MATERIAL_ID` values use the small 1.8 id table, registry keys pass through |
| `handlers.death.*` (death messages, death signs, head drop, stattrak) | ✅ | full `DeathMessageHandler#getDeathMessage` resolver (player/killer name formats with `kills`/`kills + 1`, item/projectile/entity attack variants, explosion-TNT branch, fall/void killer branches, `StringUtils#getEntityName`), targeted delivery (victim + killer + `death_messages` setting), stattrak counter (insert/rotate lore logic over the configured item list), head drop for `lazarus.headdrop` holders + placed-head protection, death sign item + fill + edit/break protection; `datetime.rs` ports the `FastDateFormat` subset + timezone resolution of `DATE_FORMAT`/`TIMEZONE` |
| ↳ death deviations | | the vanilla broadcast is turned off (`showDeathMessages=false`, one shared level data for all dimensions) because the host cannot suppress a per-death message; the plugin replicates the original's targeted delivery and sets `death_message` for the death screen; head/death signs go straight to the killer's inventory (no item-entity spawns); placed heads are tracked from the placing item's `profile` component (no owner on `SkullBlockEntity`); death signs are filled at placement (`sign.update()` equivalent) but the client only sees it after a chunk reload and the editor cannot be closed (a `DEATH_SIGN.LORE` shorter than four lines fills what it has, where the original throws); item names are title-cased registry keys without NMS localization and custom names lose their colors; unknown entity-branch causes fall back to `REASON_CUSTOM` instead of the original's empty message; `lazarus.headdrop`/`lazarus.staff` also accept the registered op-default `lazarus:headdrop`/`lazarus:staff` (the host does not grant undefined nodes to operators) |
| `handlers.elevator.*` (sign + minecart elevators) | ✅ | sign elevator (`[elevator]`/`[e]` + `up`/`down` created, up/down column search with the 0.5 block-centre landing, create/destroy/right-click messages), minecart elevator (sneaking mount on a cart parked in an open fence gate pulled up with the mount cancelled, dismount pulled up one tick later), both gated by `OWN_CLAIM_ONLY` (player faction vs faction at the sign/vehicle) and `DISABLED_IN_COMBAT` using the original's shared minecart message keys |
| ↳ elevator deviations | | the sign rewrite lands in the block entity (`set_front_text`) while the event is cancelled so the typed lines are not applied on top: the client keeps showing what was typed until the chunk reloads, so the checks also accept the typed `[elevator]`/`[e]` + `up`/`down` shorthand; `setUseInteractedBlock(DENY)` maps to cancelling the interact event; `Material.AIR`/`WALL_SIGN`/`SIGN_POST`/`isSolid()` map to the block-state `is_air`/`is_solid` flags and `*_sign` names (cave/void air counts as air); the column search runs from `get_min_y` to `+384` (tallest vanilla column) instead of the fixed 1.8 `getMaxHeight()` 256; the dismount teleport is scheduled one tick later through the host scheduler because the host re-positions the dismounted passenger right after the event |
| `handlers.limiter.*` (enchant/potion/entity limits) | 🟡 | enchantment table (clamped `enchantments_to_add`), anvil results (enchanted-book `stored-enchantments` rewrite, unrepairable-lore denial, over-cap clamp through a next-tick inventory sweep), potion drinking/throwing (`PlayerItemConsumeEvent` denial + hand clear + message), brewing (stand read through the block entity; violating slots restored one tick later), disabled entities removed by a one-second world sweep; `limiters.yml` is parsed lazily with the 1.8 legacy name tables and re-read by `/lazarus reload` |
| ↳ limiter deviations | | anvil results are clamped one tick after the take (the host hands the guest throw-away clones and drops item mutations), so the sweep covers the player's whole inventory — the `ItemUtils.updateInventory` equivalent; enchanted-book enchantment ids are probed from the server because the guest cannot read the registry (fail-open if the probe finds nothing); splash potions are denied before the throw is created and potions thrown by dispensers/witches cannot be identified (no item on the projectile); `MOBS_SPAWN_ONLY_FROM_SPAWNERS` + `MOB_LIMIT_PER_CHUNK` cannot be enforced; the mob-drop/fishing clamp sites are unreachable |
| `handlers.logger.*` (combat logger) | 🟡 | quit spawn (skeleton/villager per `COMBAT_LOGGER.TYPE` + `VILLAGER_PROFESSION`, name format, copied health/36-slot + armor inventory/effects ≤ 12000 ticks, feet-block spawn rule, no AI), full damage rules (fire immunity, indirect magic, own-damager, SOTW / PvP-prot / staff / vanish / same-faction / ally denies with the original messages, combat-tag activation, killing-blow bookkeeping for the killer), death flow (captured items to the killer, `REASON_COMBATLOGGER` / `_KILLER` messages + targeted delivery, kill/death stats with dated entries, killstreak + kitmap rewards + faction points, `faction.onDeath` DTR/points/freeze shared with the player death flow, offline deathban from the ban time resolved at logout, deferred inventory wipe), 15 s expiry removal, rejoin restore (safe-logout flag, health/position/effects, entity removal); `/logout` warmup with the `LogoutHandler` move/damage/quit cancels and `LOGOUT.KICK_MESSAGE` kick |
| ↳ logger deviations | | no item-entity spawn API: the captured inventory goes straight to the killer's inventory (items that do not fit, and every item when no *player* dealt the killing blow, are lost instead of dropping on the ground); the equipment displayed on the entity is cleared right before the host computes the death drops so the vanilla random equipment drops cannot duplicate the hand-out, and the remaining vanilla loot (bones/arrows) is cancelled through `ItemSpawnEvent` position markers while the orbs of that death flow are swept one and three ticks after the death (replicating the empty `dropDeathLoot` override and `expToDrop = 0`); the host serialises entities away *immediately* when the last chunk watcher leaves (no unload delay), so an expiry/rejoin removal that misses the live entity is queued and retried by the 1 s tick until the chunk loads again (host entity ids are never reused; entries expire after 10 minutes), with the display equipment cleared on the retry — no ghost logger remains; offline player data is unreachable, so `die()`'s inventory clear is deferred to the victim's next join (`Userdata#pending_logger_clear`) and the offline position/health-0 update is skipped (they log back in on the logger spot anyway); potion effects cannot be applied to non-player entities, so the logger only stores them for `handleEffectChanges`; an expired logger (and a rejoin) removes the entity without the death flow, exactly like the original, but items never exist on the ground; `CombatLoggerSpawnEvent` has no host equivalent (nothing cancels it); the logout warmup lives in the shared pending-warmup slot and `/logout` registers a public node (the original's `lazarus.logout` node was undefined); `lazarus.combatlogger.bypass` also accepts the registered `lazarus:combatlogger.bypass` spelling; the rejoin teleport is deferred a few ticks like the first-join spawn teleport (`tasks::queue_join_teleport`), so it cannot race the host's own join teleport |
| `handlers.portal.*` (PortalHandler, EndPortalHandler, NetherPortalTrapHandler) | ✅ | nether travel translation (`NETHER_PORTAL_POSITION_TRANSLATION`: ÷/×`VALUE` on X/Z, Y and rotation kept), portals inside the spawn safezone redirected to `NETHER_SPAWN`/`NETHER_EXIT`, end entry/exit to `END_SPAWN`/`END_EXIT` (world-spawn fallbacks); `/endportal` hands out the `SELECTION` wand (`toggleSelectionProcess` always restarts) and its check validates end portal frames, equal elevation and a 2–4 block X/Z span before filling the rectangle with end portals (frames kept, selection + wand removed, position message still sent); the trap fix sends fake air for the portal blocks around a right-clicked portal while both feet and eye blocks are portals |
| ↳ portal deviations | | the host never fires `PlayerPortalEvent`: the travel rules hook `PlayerChangeWorldEvent` (`Player::teleport_world`, which reads `position`/`yaw`/`pitch` back) and infer the portal kind from the block the player stands in (feet/eye, where the original's `from` location sat); the host's own travel agent runs *before* the event, so its vanilla 1:8 portal search/creation (and the end-side obsidian platform rebuild) still happens at the vanilla destination before the player is moved to the configured one; the destination world cannot be swapped to the entry's world token (only position/rotation are applied) |
| `handlers.kitmap.*` (KitmapHandler, KillstreakHandler) | 🟡 | kitmap mode fully wired: config sync from raw `config.yml`, deathban/DTR-loss skips, safezone drop denial, item-clear task, kill reward + killstreaks (console commands + broadcast), PvP-timer clearing/gating, first-join spawn teleport, `KITMAP_STATS` sidebar block, KITMAP-only kits restricted to safezones; `/spawn` teleport and the Miner kitmap renames pending (below) |
| `handlers.salvage.*` (SalvageHandler) + `/salvage` | ✅ | 1.8 `Bukkit.getRecipesFor` data inlined per output (material + stick, bow stick→string, 1.8 pattern cell counts, planks→`oak_planks`), `ItemUtils#isSalvageable` tier/kind composite, durability from the `Damage` component + the 1.8 max damage table (`<durability>` percentage like `Double.toString`), enchanted books keep their `stored-enchantments`, ingredient names from the raw `items.yml` reverse lookup |
| `handlers.settings.*` (SettingsHandler) + `/settings` | ✅ | `PLAYER_SETTINGS` parsed once at startup, GUI built at open (session per player, click by plain display name), toggle runs the shared `/toggle*` path and refreshes the lore, every click in the window cancelled like the original (outside clicks stay vanilla) |
| `handlers.restore.*` (InventoryRestoreManager) + `/inventoryrestore` | ✅ | `saveInventoryUponDeath` hooked into both death flows (player death + combat-logger death, from the captured record before the wipe), JSON snapshot per death (`<dataFolder>/restore/<uuid>.json`, 36 slots + armor), `FILE_CACHE` purge at load, 54-slot preview (armor 45-48, placeholders, `Rollback` in 53), rollback into the (online) target + `RESTORED_SENDER`/`RESTORED_PLAYER` messages, file deleted on restore |
| `handlers.rank.*` (RankReviveHandler, RankAnnouncerHandler) + `/rankrevive` | ✅ | per-rank `RANK_REVIVE_COMMAND` entries (first matching permission wins), cooldown through the persisted `RankReviveTimer`, deathban + revive + activation + broadcast (`<rankName>`/`<player>`/`<target>` to console + all players); donators announcer with `ONLINE_RANK_ANNOUNCER` entries, op skip, config-order groups joined with `DELIMITER`, `MESSAGE` lines with `<donators>` |
| `handlers.leaderboard.*` | ✅ | eight boards (player kills/deaths/balance/killstreak + faction kills/points/balance/KOTHs) cached as sorted 10-entry lists updated only on change/overflow, `/topkills`, `/topdeaths`, `/topkillstreak(s)`, `/topbalance`, `/leaderboard`, `/f top` + `/f list` (hover/click, relation colours, pagination), leaderboard holograms (header + ten rows + footer, text rewritten in place on updates, kitmap gating through `LEADERBOARDS.KITMAP_MODE_ONLY`); `<dataFolder>/leaderboards/{players,factions}.json` |
| `abilities.*` | ⬜ | large subsystem; ported last |
| `classes.*` (Archer, Bard, Miner, Rogue, Mage, Bomber) | ⬜ | |
| `kits.*` | ✅ | inventory capture/give incl. lore + vanilla enchantments (`kits.json`) |
| `games.*` (Koth, Conquest, DTC, Dragon, King, Mountain, Loot, Schedule) | ⬜ | |
| `economy.*` + `integration.vault` | 🟡 | internal balance implemented; Vault has no Pumpkin equivalent |
| `deathban.*` | ✅ | faithful prelogin flow (first join with lives = join-again prompt, second join consumes a life, EOTW lockout for stored bans), per-permission `BAN_TIMES`, faction auto-revive, `deathban_until` + `deathban_reason` + `deathban_location` on userdata, `/lives` (+ `/f livesrevive`, deposit/withdraw), `/deathban check|revive` (offline lookups through `userdata.json`) |
| `staffmode.*`, `VanishManager` | 🟡 | vanish via `hide_player/show_player`; staff-mode flag |
| `glass.*`, claim pillars, `/f map` visuals | ✅ | client-side `send_block_change` engine |
| `selection.*` (WorldEdit-style wand) | ✅ | `/f claim` wand with sneak-purchase; `/endportal` `SELECTION` wand whose clicks run the end-portal check (clone-revert on failure; `SYSTEM_CLAIM` shares the selection wand, like the original); attacking while holding the wand cancels the hit and ends the selection, dropping the wand is denied and any world change drops the selection process (claim pillars cleared); both wands parsed once at startup from `CLAIM_WAND` / `SELECTION_WAND` |
| ↳ selection deviations | | the host reports only the dropped stack's registry key `PlayerDropItemEvent`, so the drop denial matches any wand-kind item while a selection is active (the original compared the whole `ItemStack`); the attack denial skips projectiles, which the host attributes directly to the shooter player |
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
* **Absent command arguments read back as `""`** — the host
  `consumed-args.get-value` returns an empty string for an argument the parser
  never reached, so `commands::present_string_arg` maps `Some("")` back to
  "missing" (numeric args already failed to parse and were unaffected).
* **Claims are still rectangles**, but a `chunk -> faction` index is maintained so
  `territory_at(world, x, z)` is O(1) instead of scanning every claim.
* **Single-chunk claiming** is replaced by the original **selection wand**: `/f claim`
  gives a golden-hoe wand; left/right click set positions, sneak + left-click buys.
  Selection pillars and claim maps are rendered entirely client-side (`glass.rs`).
* **A single global `RwLock<AppState>`** mirrors the original singletons. Handlers
  avoid holding the lock across host calls that can re-enter the plugin.
* **`EntitySpawnEvent` is never handled.** The host re-enters the guest in the
  middle of a plugin `world.spawn_entity` call when that event has a handler,
  and its re-entry bookkeeping loses a race often enough (~5–15% of such pumps,
  measured) to park the call forever: the plugin's store queue is never serviced
  again and only a restart recovers. With no handler the host's `fire` returns
  before it can re-enter, so spawning stays a plain blocking pump and a
  hologram/logger can create its entities inside one root call. The three
  features the original based on spawn cancellation are ported as
  scans/sweeps instead (limiter, logger orbs, death-message entity names);
  future sectors must follow the same rule (`src/listeners/mod.rs` documents it).
* **Economy** is internal (`Userdata.balance` + `Faction.balance`); Vault does not
  exist on Pumpkin.
* **Time** is measured in unix milliseconds for timers, using Pumpkin's scheduler
  (20 ticks/second).
* **Persistence is crash-safe.** Every store is written through an
  atomic temp-file + rename (`state::atomic_write`), so a crash mid-write can
  never truncate or corrupt a file. The gameplay-volatile stores
  (`timers.json`, `factions.json`, `claims.json`, `userdata.json`) are flushed
  every 30 seconds (`state::save_hot`) — bounding crash loss to half a minute —
  while `kits.json`/`holograms.json` and the configured full pass
  (`FACTIONS_AUTO_SAVE`, default 30 min) are written by `state::save_all`.
  Timers persist as absolute epoch **milliseconds** (per-player, faction-freeze,
  custom and the global SOTW remainder — downtime does not consume SOTW, so its
  stored form is the remaining millis), and a store that fails to parse is moved
  aside to `<name>.corrupt-<ts>` instead of being overwritten by the next save.
* **PvP protection is banked, not ticking, inside safezones**: the full
  `PvpProtTimer` model is ported — `pvp_banked` holds the paused remainder,
  `is_active`/`remaining` report paused protection as active and every consumer
  (scoreboard, class gates, glass walls, claim entry) sees it.
* **SOTW** keeps the original's runtime state in memory (`enabled_players`,
  `hide_players`) and persists only the timer (`timers.json` →
  `sotw_remaining_millis`), matching `timers.yml` `SotwTimer`.
* **The spawn safezone is never claimed automatically.** Like the original, a fresh
  install only creates the system factions (Wilderness, Warzone, the
  `SpawnFaction` — kind `Safezone` — and the four road factions from
  `setupInitialFactions`: NorthRoad, EastRoad, SouthRoad, WestRoad); the spawn
  claim itself is made manually (`/f claimfor Spawn`, admin wand, …).
  `additionalFactionSetup` is ported as
  `normalize_system_factions`: it forces `safezone = true` / `deathban = false` on
  the spawn faction on every load but **does not rename it**, so `/f rename` on
  the spawn faction persists like in the original.
* **Kitmap mode** mirrors the original's two classes: `KitmapHandler` is emulated
  by checking `KITMAP_MODE.ENABLED` inside the relevant handlers (the original only
  registered the handler when kitmap was on) and `KillstreakHandler` by the death
  handler's kitmap block. Config values are synced from the raw `config.yml` on
  every load (`KITMAP_MODE`, `KITMAP_CLEAR_ITEMS`, `KITMAP_KILL_REWARD`,
  `KITMAP_KILLSTREAK.KILLS`), and the sidebar adds the `KITMAP_STATS` block from the
  raw `scoreboard.yml`. A port **addition** — `KITMAP_STATS.ONLY_IN_SPAWN`
  (default `true`) — keeps that block visible only inside a safezone territory;
  `false` restores the original always-on behaviour.
* **The spawn is `utilities.yml` `WORLD_SPAWN`, and it owns every spawn path.**
  Joins (first join only, like `PvpProtHandler#onPlayerJoin`), respawns (port of
  `DynamicEventHandler#onPlayerRespawn`'s `setRespawnLocation`, applied through
  `PlayerSpawnLocationEvent`) and `respawn_fixes` all resolve the same entry
  (`core::configured_spawn`). Beds and respawn anchors are cancelled on
  `PlayerSpawnChangeEvent`, so no spawn point is ever stored — the original
  respawned everyone at the spawn regardless, and a stored point can additionally
  drag Pumpkin into the respawn point's dimension *before* the position override
  applies (legacy NBT points are caught by the next-tick `respawn_fixes`
  teleport). With `WORLD_SPAWN` unset the host's own placement is kept: it already
  spawns players on the world-spawn surface (`top + 1`), which is the portable
  equivalent of `player.getWorld().getSpawnLocation()` without the stale
  `level.dat` Y. The plugin logs the resolution at startup (and a ready-to-paste
  `WORLD_SPAWN` suggestion when the entry is unusable and the chunks are loaded).
  Join-time teleports (the first-join spawn and the combat logger's rejoin
  restore) are **deferred** a few ticks (`tasks::queue_join_teleport`): the host
  teleports the client itself right before firing the join event and keeps a
  single pending teleport, so a plugin teleport in the same tick makes the
  client's confirmation for the host's id arrive after the awaited id was
  overwritten, which Pumpkin answers with a "Wrong teleport id" kick. The
  deferred teleport is skipped when the target has already been reached (within
  two blocks), so the host's own placement doubles as the teleport whenever both
  resolve to the same spot.
  Bukkit folder names (`world`, `world_nether`, `world_the_end`) are mapped to
  their dimensions because Pumpkin names the nether/end worlds after the root
  folder as well.

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
* Natural spawns and the limiter spawn rules — the host's natural spawner
  registers mobs without firing `EntitySpawnEvent`, there is no spawn reason
  and no chunk entity query, so `MOBS_SPAWN_ONLY_FROM_SPAWNERS` and
  `MOB_LIMIT_PER_CHUNK` cannot be enforced; disabled entities are removed by a
  one-second world sweep instead of cancelling their spawn (the event is off
  limits — see the design notes — and the original's `CreatureSpawnEvent`
  companion is never fired either), so a denied mob/vehicle/projectile/drop can
  exist for up to a second before it disappears.
* Mob-drop enchant clamping — `EntityDeathEvent` carries no drops list, so the
  enchant limiter cannot rewrite what a mob drops (its fishing branch is dead
  code on 1.8).
* Anvil results — the host gives the guest clones of `clicked-item`/`cursor`
  and ignores item mutations in the returned event, so the result slot cannot
  be rewritten in place: the port clamps every item the player carries one tick
  after a result click that needs it (`ItemUtils.updateInventory` equivalent).
  Unrepairable-lore clicks are still denied exactly like the original, and a
  result taken to the cursor is only clamped once it lands in the inventory.
* Enchantment ids — the guest cannot read the server's enchantment registry,
  so the ids used by the `enchantments`/`stored-enchantments` component codec
  are probed at the first book using throw-away stacks (books fail open if the
  probe finds nothing).
* Splash potions — the host has no splash event carrying the item, so the
  potion limiter denies throwing in `PlayerItemConsumeEvent` (before the throw
  entity exists; the potion is cleared and the player told, like the original's
  splash denial) and potions thrown by dispensers/witches cannot be limited.
* `Player#getKiller` — the host death event carries no damage source, so the
  killer is the last player recorded by the combat-tag handler (`last_damagers`,
  5-second window, the same window Bukkit's combat tracker uses). A death by
  environment right after PvP therefore still credits the last attacker.
* Death-message broadcast — the host has no per-death "silent" flag (an empty
  `death_message` would still be sent), so the vanilla broadcast is turned off
  (`showDeathMessages=false` on the shared level data of all dimensions) and the
  plugin delivers the message itself to the same targeted list as the original
  (victim + killer + everyone with the death-messages setting); the message is
  also stored on the event, so the victim's death screen shows it (a nicety the
  original's `setDeathMessage(null)` suppressed).
* Head-drop / death-sign drops — `PlayerDeathEvent` has no drops list and the
  WIT cannot spawn item entities, so the killer's head and the death sign go
  straight into the killer's inventory instead of dropping at the death
  location.
* Placed-head ownership — `SkullBlockEntity` exposes no owner through the WIT,
  so heads placed from the plugin's item are tracked in memory from the placing
  item's `profile` component; heads without a tracked owner (creative menu, mob
  skulls) keep vanilla break/interact behaviour.
* Death-sign editor — the host cannot close the sign editor nor broadcast a
  `set_front_text` write: the fill happens at placement like the original's
  `sign.setLine` + `sign.update()`, but the client only sees the lines after a
  chunk reload and the editor stays open (later edits are discarded, like the
  original cancels them). With fewer than four `DEATH_SIGN.LORE` lines the
  original throws and aborts the fill; the port fills the lines it has.
* Item names in death messages (`NmsUtils#getItemName`) — no NMS localized
  names: the port title-cases the registry key (matches the common items) and
  strips the colors of custom names (the original kept the `§` codes).
* Entity names in death messages (`StringUtils#getEntityName` callers) — the WIT
  has no entity lookup by id, so the type behind the damager id is resolved by
  scanning the loaded world entity lists (`get_all_worlds` → `get_entities`) and
  cached for the session (host entity ids are monotonic). An attacker that
  despawned before the lookup yields no name, like a missed spawn event would.
* Unknown damage causes in the entity branch — the original leaves the message
  empty (nothing is sent, the host cannot replicate that); the port falls back
  to `REASON_CUSTOM`.
* `lazarus.headdrop` / `lazarus.staff` — the host rejects dot-namespaced
  registrations and grants undefined nodes to nobody (no Bukkit `isOp()`
  fallback), so the op default lives on the registered `lazarus:headdrop` /
  `lazarus:staff` variants, checked together with the original nodes.
* Offline player lookup (`/lives check|revive|send|add|set`) is limited to the
  players present in `userdata.json` (online + everyone seen before), since the
  port has no Bukkit profile cache.
* `/f forcekick` / `/f forceleader` targets — the original accepts offline
  players (`Bukkit.getOfflinePlayer`); the port still requires the target to be
  online and reports `PLAYER_NOT_ONLINE` otherwise.
* `/f top` from console — the original casts the sender to `Player` to build the
  clickable lines; the port prints the plain header/footer/entries instead.
* Faction leaderboards (`/f top`) — the original fills a 10-entry cache only when
  a faction's value *changes*; the port sorts the live values of factions whose
  metric is non-zero, which shows the same top-10 once factions have activity.
* Leaderboard holograms — the original rebuilt the whole line set on every
  update; the port rewrites the text of the existing `TextDisplay` entities in
  place (the layout is fixed), and it refreshes all ten rows where the original
  only touched one (same output; the original skipped unchanged rows as an
  optimisation).
* Hologram persistence — the stored `world` is the dimension id
  (`minecraft:overworld`), since every Pumpkin level is named `world`; the
  original's configured world tokens are accepted when reading the file.
* Leaderboard storage — one JSON object per holder at
  `<dataFolder>/leaderboards/players.json` / `factions.json` with the original's
  `topXxx` keys, each board a sorted array of `key`/`name`/`value`/`timestamp`
  entries (the original kept a `ConcurrentSkipListSet`); sorting reproduces the
  collection order, and unlike the original a disbanded faction is also removed
  from every board it appeared on (the original left the stale entry until the
  next rebuild).
* `/top` — the host exposes the motion-blocking heightmap, so the port stands
  the player on top of that block (+1) instead of inside it, replicating the
  original's `getHighestBlockAt` teleport plus the `add(0.5, 0, 0.5)` offset.
* Command pre-processing (`BlockedCommandsHandler`, the combat-tag /
  PvP-protection command restrictions) — the host never fires
  `PlayerCommandPreprocessEvent`, so the registered listener is unreachable and
  the blocked / combat-denied commands stay usable.
* Combat logger host limits: there is no item-entity spawn API, so a dead
  logger's captured inventory is handed to the killer's inventory — items that
  do not fit, and every item when no *player* dealt the killing blow, are lost
  instead of staying on the ground — and the equipment shown on the entity is
  cleared right before the drops so the vanilla random equipment drop cannot
  duplicate the hand-out; the rest of the vanilla death drops (the loot table)
  is cancelled through `ItemSpawnEvent` position markers and the orbs — spawned
  after the death event returns, with `EntitySpawnEvent` off limits — are swept
  one and three ticks later within the same radius. No potion
  effects can be applied to entities (the logger only stores them for the
  rejoin restore) and no offline player data can be written (`die()`'s
  inventory clear runs on the victim's next join, the offline
  position/health-0 update is skipped).
* Kitmap kill/killstreak credit follows the same online-killer convention as the
  kill stats (see `Player#getKiller` above): the victim's killstreak is always
  reset, the killer's only when the killer is online.
* Kitmap reward commands (`KITMAP_KILL_REWARD`, `KITMAP_KILLSTREAK.KILLS`) run
  through Pumpkin's console command dispatcher, so they must reference commands
  the server actually provides. The original's `econ`/legacy item-id examples
  come from an economy plugin + 1.8 syntax; on the port the internal economy is
  `/economy give <player> <amount>` (`/eco`), and `give`/`effect` take modern
  registry names.
* Kitmap `/spawn` (`SpawnTeleportHandler`, `KITMAP_MODE.SPAWN_TELEPORT_DELAY`,
  `SPAWN_COMMAND.TELEPORTED_KITMAP`, the quit teleport-timer cancel) — the config
  value is loaded, the command itself is not ported yet.
* Kitmap killstreak leaderboards (`/topkillstreaks`, the `killstreak` argument
  of `/leaderboard`, `LEADERBOARDS.KITMAP_MODE_ONLY`) — ported with the
  leaderboard sector; the faction KOTHs board/hologram is wired but stays empty
  until the games sector credits captures (`FactionDataChangeEvent` equivalent).
* Kitmap `Miner`-class renames and ability changes (`SpawnTeleportHandler`,
  `KitmapHandler`-driven class tweaks) — pending with the abilities/classes port.
* Block *use* protection is ported (`BlockInteractListener`: clickable materials,
  safezone allow-list, sneak-with-a-non-block-item bypass, kitmap warzone
  clickables). Two pieces of the same original listener remain:
  `PlayerInteractEvent` with the `PHYSICAL` action (the host never fires it, so
  pressure plates keep their vanilla behaviour instead of being cancelled in
  protected areas) and the hanging listeners (`HangingPlaceEvent`,
  `HangingBreakByEntityEvent`, `HangingDamageByEntityEvent` for paintings/item
  frames — the host events exist and this is the next candidate).
* `PvpProtHandler#onPlayerInteract` (item-use denial while the PvP timer runs,
  behind the 10-second logout rule) — pending with the next PvP-protection pass.
* Respawn rotation — `PlayerSpawnLocationEvent` carries only the position, so the
  `WORLD_SPAWN` yaw/pitch are applied on the first-join teleport but respawns keep
  the host's world-spawn rotation.
* Salvage full-inventory leftovers — `PlayerUtils#addToInventoryOrDropToFloor`
  dropped them on the ground; the WASM API cannot spawn item entities, so a full
  inventory loses the ingredient (durability/enchantments are read off the hand
  stack before it is consumed, like Java reading the still-referenced object).
* `/settings` — the original cached one inventory per player on join and matched
  clicks by the window title; the port builds the window when the command runs and
  matches clicks by the item's plain display name through a per-player session
  (the same `getSetting(displayName)` lookup). Sizes 9/18 map to `Generic9x2`
  because the host sizes `Generic9x1` as a 5-slot hopper.
* Inventory restore — snapshots are written to `<dataFolder>/restore/<uuid>.json`
  on every death (the original kept them in memory and only wrote files on
  disable), stored as registry key + count + data components (an unknown component
  name is dropped); there is no API to close a window, so `Close Preview` cannot
  close the preview (Escape does) and a rollback clears the preview instead of
  closing it.
* Permission-targeted broadcasts (`Messages.sendMessage(message, permission)`: the
  deathban revive and the inventory-restore notices) reach the staff players only —
  the port does not mirror server-console message lines.

## 5. Roadmap

Recently completed: client-side block render (`glass.rs`), selection wand,
scoreboard placeholders, tab list, enderpearl/gapple cooldowns, deathbans + lives,
admin faction subcommands, vanish/staff mode, holograms, kits, PvP protection
phase timers (safe-time banking, `/pvp`, attacker-side denial, `/f home` gate),
the SOTW system (`/sotw`, damage immunity, safezone hiding, scoreboard line),
custom timers (`/customtimer`, sidebar lines), and the fidelity pass over the
faction systems: `/f list` + `/f top` (pagination, relation colours, hover/click),
death/kill/death counting, DTR death loss + freeze, claim enter/leave messages
(`<faction>`/`<deathban>` placeholders resolved from the raw `language.yml`),
the full `/lives` family (check/revive/send/add/set), and the faction message
pass: `/f help` pages from `FACTIONS.HELP_COMMAND.PAGES.<n>`, usage +
`searchForFaction` + raw-language message flows for the `set*` admin commands
(absolute-vs-points values, `getDtrString()` broadcasts, `setfreeze`/`thaw`
with `parseSeconds`), `deposit`/`withdraw` broadcasts, `forcekick`/`forceleader`
messages, and usage messages on every admin subcommand reached without
arguments (the literals now execute). Kitmap mode is now ported end to end
(config sync, every toggle + handler above), the spawn safezone is no longer
auto-claimed (claims are manual, `additionalFactionSetup` no longer renames the
spawn faction), and block *use* protection landed: using a clickable block
outside your territory is denied with the original's `DENY_INTERACT` message
(chests, doors, buttons, ... — with the safezone allow-list, the sneak bypass
and the kitmap warzone clickables), and the claim wand skips denied interacts
like the original `SelectionManager`. The death sector is now complete: the full
`DeathMessageHandler` resolver + targeted delivery, stattrak, head drops, death
signs, `/lastdeaths` + `/lastkills`, `/deathban check|revive` with the stored
ban location, and `datetime.rs` for `DATE_FORMAT`/`TIMEZONE`. The elevator
sector is done as well: the sign elevator (create/destroy/ride with the
`OWN_CLAIM_ONLY` + combat gates) and the fence-gate minecart elevator (sneak
mount and dismount pulled up, the dismount one tick later like the original's
`Tasks.sync`). The limiter sector landed next: the enchantment table clamps the
rolled `enchantments_to_add` in place, anvil results clamp enchanted books
(through the `stored-enchantments` codec, with the enchantment ids probed from
the server), deny unrepairable-lore items and sweep over-cap pieces out of the
player's inventory one tick after the take, potions are denied when drunk or
thrown (`PlayerItemConsumeEvent` + the original's message) and brews are rolled
back one tick later when the result violates the limits, and disabling an entity
cancels its `EntitySpawnEvent` (1.8 legacy names included). The combat logger
sector is done as well: quitting builds a named skeleton/villager carrying the
captured gear (`COMBAT_LOGGER` config), the damage rules (fire immunity,
indirect magic, SOTW / PvP-prot / staff / vanish / faction denies + combat-tag
activation) guard it, killing it hands the items to the killer and credits the
stats, kitmap killstreaks and faction points, the offline victim is deathbanned
with the ban time resolved while the permissions were still valid (the
inventory wipe runs on the next join), the logger expires after `TIME` seconds
and the next join restores health/position/effects and removes the entity — and
`/logout` landed with it (warmup + move/damage/quit cancels + the safe-logout
flag the quit gate reads). The portal sector followed: the nether travel
translation (`NETHER_PORTAL_POSITION_TRANSLATION`), the spawn/exit redirections
(`NETHER_SPAWN`/`NETHER_EXIT`/`END_SPAWN`/`END_EXIT`) driven by
`PlayerChangedWorldEvent`, the `/endportal` `SELECTION` wand with the
frame/elevation/size check that fills the rectangle with end portals, the
nether-portal trap fix (fake air around the clicked portal), and
`SelectionManager#onPlayerChangedWorld` (any world change drops the selection).
The rank / restore / salvage / settings sector landed next: `/salvage` rebuilds
the 1.8 `Bukkit.getRecipesFor` data (inlined recipe table, 1.8 pattern cell
counts, durability + enchantment preservation, `items.yml` ingredient names),
`/settings` opens the `PLAYER_SETTINGS` GUI (startup-parsed entries,
click-by-display-name, lore refresh through the shared `/toggle*` toggles),
inventory restore snapshots every death into `<data>/restore/<uuid>.json`
(`INVENTORY_RESTORE.*`, `FILE_CACHE` purge at load, combat-logger deaths included
through the captured record), the 54-slot preview shows contents/armor with the
original placeholders and rolls back into an online target with the staff
broadcast, and the rank sector adds `/rankrevive` (per-rank permission +
persisted `RankReviveTimer` cooldown, broadcast with `<rankName>`) plus the
online donators announcer (`ONLINE_RANK_ANNOUNCER`, op skip, delimiter join).
The leaderboard / hologram sector closed next: the eight boards (player
kills/deaths/balance/killstreak, faction kills/points/balance/KOTHs) are cached
as sorted 10-entry lists that only change on a real update (disband cleanup
included), `/topkills`, `/topdeaths`, `/topkillstreak(s)` and `/topbalance`
print them with the original's entries, `/top` teleports to the
motion-blocking height (+1), `/leaderboard` renders a board in chat with the
kitmap gating, and the `/hologram` subsystem was reworked: one `TextDisplay`
per line on the original's `BASE - 0.25 * index` layout, numeric ids with the
`decrementId` shift, normal + leaderboard layouts persisted to
`<data>/holograms/holograms.json`, in-place text refresh on board updates and a
per-second rebuild for holograms whose entities vanished (they are never
persisted). The same pass removed the last `EntitySpawnEvent` handlers: the
host deadlocks ~5–15% of the plugin's entity-spawn pumps when that event has a
handler (its re-entry bookkeeping races and parks the call), so disabled-entity
limiting is a one-second sweep, the logger's experience orbs are swept one and
three ticks after the death, and death-message entity names resolve by scanning
the world entity lists — spawning is now a plain blocking pump (see §4).

Still to port (in rough priority order):

1. Events/games (Koth, Conquest, DTC, Ender Dragon, King of the Hill, Mountain,
   Loot, Schedule) using boss bars + the world API.
2. Abilities + PvP classes (Archer/Bard/Miner/Rogue/Mage/Bomber).
3. Mob stacking (the remaining block handler; the filter stays blocked by the
   missing player pickup event).
4. Custom recipes, sign shops.
5. World border, safezone food/potion rules.

## 6. Files

The tree mirrors the original `me.qiooip.lazarus` packages: each feature lives
in its package folder (`handlers/logger`, `factions/listeners`, `timer`, ...).
Files that only exist as port glue (global state, the repeating tick) stay at
the crate root, and the event adapters that span several original packages stay
in `listeners/` (the original distributed them across packages; here they keep
one file per event family, and each file's header names its original class).

```
src/
├── lib.rs              plugin entry (metadata, permissions, lifecycle)
├── state.rs            global state + JSON/TOML persistence
├── tasks.rs            repeating tick (timers, DTR, teleports, glass, sidebar, tab)
├── config/
│   ├── mod.rs          Config (config.toml, tab.toml, scoreboard.toml, deathban)
│   ├── lang.rs         Lang (lang.toml)
│   └── yaml.rs         YAML subset parser (original-style config/language files)
├── utils/
│   ├── mod.rs          colour/string/time helpers
│   ├── core.rs         host-API bridge + territory resolution
│   ├── components.rs   WIT `DataComponent` <-> kebab-name table (restore snapshots)
│   ├── datetime.rs     `DATE_FORMAT`/`TIMEZONE` formatter (FastDateFormat subset)
│   ├── legacy_items.rs 1.8 item ids <-> registry keys + `items.yml` names
│   ├── nbt.rs          NBT tree helpers for the host bridge
│   └── potion_components.rs  native potion-component table bridge
├── classes/            PvP classes (Archer/Bard/Miner/Rogue/Mage/Bomber)
├── deathban/           deathban / lives handling
├── glass/              client-side block render engine (send_block_change)
├── hologram/           TextDisplay holograms
├── kits/               inventory kits (lore + enchantments)
├── scoreboard/         sidebar renderer (placeholders + teams)
├── selection/          claim + selection wands and selection state
├── staffmode/          vanish + staff mode
├── tab/
│   ├── mod.rs          tab list driver (native companion state)
│   └── api.rs          shared tab-state model + writer
├── timer/              TimerManager (player/faction/custom timers, PvP-prot banking, SOTW)
├── userdata/           Userdata + UserdataManager
├── factions/
│   ├── types.rs        Faction, Role, Relation, ChatType, Location
│   ├── claim.rs        Claim + ClaimManager (chunk index)
│   ├── manager.rs      FactionsManager
│   └── listeners/      block + entity protection (`factions.listeners`)
├── handlers/
│   ├── block/          auto-smelt, cobweb, crowbar, subclaims, found ore, ...
│   ├── chat/           chat formatting + faction chat (`handlers.chat`)
│   ├── death/          death messages, stattrak, head drops, death signs
│   ├── elevator/       sign + minecart elevators
│   ├── kitmap/         kitmap mode (safezone item-drop denial)
│   ├── limiter/        enchant/potion/entity limits (limiters.yml)
│   ├── logger/         combat loggers + /logout warmup
│   ├── portal/         portal travel rules, /endportal check, nether trap fix
│   ├── rank/           rank revive + online donators announcer
│   ├── restore/        death-inventory snapshots + preview/rollback
│   ├── salvage/        1.8 recipe salvage
│   ├── settings/       /settings GUI
│   └── timer/          enderpearl / golden apple cooldowns + SOTW
├── commands/
│   ├── mod.rs          registration helpers
│   ├── base.rs         /lazarus + setting toggles
│   ├── crowbar.rs      /crowbar
│   ├── customtimer.rs  /customtimer (start/cancel/list)
│   ├── deathban.rs     /deathban check|revive
│   ├── economy.rs      /balance + economy admin commands
│   ├── endportal.rs    /endportal (SELECTION wand)
│   ├── lastdeaths.rs   /lastdeaths + /lastkills
│   ├── faction.rs      /f command tree (+ admin subcommands)
│   ├── lives.rs        /lives
│   ├── logout.rs       /logout (safe logout warmup)
│   ├── staff.rs        /vanish, /staffmode
│   ├── hologram.rs     /hologram
│   ├── inventoryrestore.rs  /inventoryrestore (aliases invrestore/lastinv/invrollback)
│   ├── pvp.rs          /pvp (enable / time)
│   ├── rankrevive.rs   /rankrevive
│   ├── salvage.rs      /salvage
│   ├── settings.rs     /settings
│   ├── sotw.rs         /sotw (start/stop/hideplayers/enable/time)
│   └── kit.rs          /kit
└── listeners/
    ├── mod.rs          registration order (the original `HandlerManager`)
    ├── player.rs       join/leave/move/death/respawn/teleport
    ├── combat.rs       friendly fire, combat tag, pvp protection
    ├── command.rs      blocked commands
    └── interact.rs     claim wand interactions + sneak tracking
```

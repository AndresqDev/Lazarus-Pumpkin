# Lazarus-Pumpkin — Plan de port 1:1

Port fiel del core Java **Lazarus** (Bukkit/Spigot 1.8, ~55k LOC, 655 clases)
a **Pumpkin** (Rust/WASM + plugin nativo companion).

Fuente original: `/home/andresq/IdeaProjects/Lazarus-master`
Destino: `/home/andresq/IdeaProjects/Lazarus-Pumpkin` (WASM) + `native-tab/` (nativo)

---

## 0. Decisiones fijas

| Tema | Decisión |
|---|---|
| Knockback | **Omitido** |
| LunarClient | **Omitido** (waypoints, cooldowns, teampack…) |
| Datastore | JSON en la data folder (Mongo/Vault fuera) |
| Safezone | **Solo el claim de spawn** (no radio) |
| Warzone | Cuadrado centrado en 0,0 (±radius), Y infinito; `WARZONE_BREAK_AFTER` = radio completo |
| Glass | Bloques fake client-side, **regeneran** al romper/poner bloque real |
| Scoreboard | **Per-player** (packets `CScoreboard*`/`CTeam` desde el WASM) |
| Tablist | **Per-player** artificial vía `.so` nativo (`tab_state.json`, diff por slot) |
| Configs | **Leer los YAML originales** (copia en `original-configs/`) tal cual |

---

## 1. Estrategia de configuración

El original tiene 9 ficheros de config. Se copian verbatim a `original-configs/`
y el plugin debe **parsearlos directamente** (no reescribir a TOML) para no perder
fidelidad de claves/valores.

| Fichero | Tamaño | Contenido |
|---|---|---|
| `config.yml` | 35 KB | 128 claves top-level: facciones, claims, timers, kits, clases, etc. |
| `language.yml` | 126 KB | Todos los mensajes |
| `scoreboard.yml` | 3 KB | Placeholders y títulos de scoreboard |
| `tab.yml` | 2 KB | Columnas + módulos de tab |
| `classes.yml` | 9 KB | Clases PvP |
| `limiters.yml` | 3 KB | Límites de pociones |
| `abilities.yml` | 17 KB | Habilidades |
| `items.yml` | 12 KB | Ítems custom |
| `utilities.yml` | 0.1 KB | Utilidades |

**Plan de config (Fase 0):** capa `SerdeYaml` (`serde_yaml::Value`) con getters
tipados (`string`, `int`, `bool`, `list`, `section`) que imitan `config/Config.java`.
Cargar los 9 ficheros desde la data folder. Migrar cada sistema a leer de ahí.

---

## 2. Inventario de sistemas (original → estado Rust)

Leyenda: ✅ hecho · 🟡 parcial · ❌ pendiente · 🚫 fuera de alcance

| Sistema (paquete Java) | Nº clases | Estado | Notas |
|---|---|---|---|
| `factions` (Faction, PlayerFaction, SystemFaction, types) | 113 | 🟡 | Tipos básicos; faltan eventos, NMS, tipos especiales |
| `factions/claim` (Claim, ClaimManager, FactionMap) | 3 | 🟡 | Rectángulos exactos hechos; falta mapa/auto-claim/buffer |
| `factions/commands/player` | 41 | 🟡 | Muchos hechos; revisar 1:1 |
| `factions/commands/admin` | 30 | ❌ | Casi ninguno |
| `factions/event` | 18 | ❌ | Event bus inexistente en Pumpkin |
| `factions/listeners` | 3 | 🟡 | |
| `commands` (base) | 122 | 🟡 | `/f`, `/lazarus`, `/lives`, `/kit`, `/hologram`, `/balance`… |
| `commands/staff` | 34 | 🟡 | staffmode básico |
| `timer` (Timer, cooldowns, types, scoreboard) | 32 | 🟡 | combat/pvp/pearl/gapple/home/stuck |
| `scoreboard` | 11 | 🟡 | Per-player OK; faltan muchos placeholders |
| `tab` (PlayerTab, módulos) | 13 | 🟡 | Columnas + FactionInfo/PlayerList; falta NextKoth |
| `userdata` | 6 | 🟡 | JSON; revisar campos/settings |
| `deathban` | 3 | 🟡 | |
| `economy` (EconomyManager, SignShop) | 5 | 🟡 | Balance sí; sign shops no |
| `kits` | 10 | 🟡 | |
| `glass` | 2 | ✅ | Regen hecha |
| `hologram` | 19 | 🟡 | TextDisplay básico |
| `staffmode` (StaffMode, Vanish) | 3 | 🟡 | |
| `selection` | 4 | ✅ | Wand |
| `classes` (Mage, Archer, Miner, Bomber, Rogue, Bard) | 18 | ❌ | Sin clases PvP |
| `abilities` | 37 | ❌ | |
| `handlers` (82 clases: block, chat, death, kitmap, leaderboard, limiter, logger, portal, rank, staff, timer…) | 82 | ❌/🟡 | Muchísimos handlers |
| `games` (conquest, dragon, dtc, king, koth, loot, mountain, schedule) | 108 | ❌ | Todos los eventos |
| `integration` (chat, vault, lunar…) | 21 | 🚫/❌ | Chat integrations; Vault fuera |
| `lunarclient` | 8 | 🚫 | Fuera |
| `database` (Mongo) | 5 | 🚫 | JSON ya |
| `utils` | 29 | 🟡 | Parcial |
| `config` | 3 | 🟡 | Reescribir a YAML |

---

## 3. Plan por fases

### Fase 0 — Cimientos de config
- [ ] Capa YAML (`serde_yaml`) + getters tipados.
- [ ] Cargar los 9 ficheros originales desde la data folder.
- [ ] Migrar `config.rs`, `lang.rs`, `scoreboard.rs`, `tab.rs` a leer del YAML original.

### Fase 1 — Facciones + Claims (CRÍTICO)
- [ ] `Faction` 1:1 (campos, roles, relaciones, DTR, home, rally, points, kills…).
- [ ] `PlayerFaction`, `SystemFaction`, `WarzoneFaction`, `SpawnFaction`, `WildernessFaction`, `RoadFaction`, `KothFaction`, `ConquestFaction`, `DtcFaction`, `MountainFaction`.
- [ ] `ClaimManager` 1:1: `getClaimAt`, overlap, buffer, `canBuild`, `FactionMap` (ASCII map), auto-claim.
- [ ] `Claim`: `contains`, `getCuboid`, `getWorld`, `isInside`, `isNear`.
- [ ] Comandos player (41) + admin (30) 1:1.
- [ ] Listeners de facción (block, entity, player) 1:1 con **todas** las comprobaciones parcheadas.

### Fase 2 — Timers + Scoreboard + Tab + Userdata
- [ ] `TimerManager` 1:1 (todos los tipos y placeholders).
- [ ] Scoreboard: todos los placeholders de `scoreboard.yml`.
- [ ] Tab: módulo `NEXT_KOTH`, START_SLOT configurables.
- [ ] `Userdata`/`Settings` 1:1.

### Fase 3 — Handlers (82)
- [ ] block, chat, death, kitmap, limiter, logger, portal, rank, staff, timer, leaderboard.

### Fase 4 — Games (108)
- [ ] Koth, Conquest, DTC, KillTheKing, EnderDragon, Mountain, Loot, Schedule.

### Fase 5 — Classes + Abilities
- [ ] PvpClassManager: Mage, Archer, Miner, Bomber, Rogue, Bard.
- [ ] AbilitiesManager (37).

### Fase 6 — Resto
- [ ] Kits 1:1, Hologramas, Economy/SignShops, Deathban, StaffMode/Vanish.
- [ ] Integraciones de chat (adaptar a Pumpkin).

---

## 4. Notas críticas de claims (bugs parcheados a replicar)

Revisar en `FactionPlayer`/`ClaimManager`/`PlayerEventListener`/`BlockEventListener`:
- Overlap entre claims del mismo mundo y mundos distintos.
- Buffer/borde entre claims (no reclamar pegado a enemigo).
- `WARZONE_BREAK_AFTER` y protección warzone/safezone.
- Denegación de home teleport desde territorio enemigo.
- Auto-claim chunk a chunk y `FACTION_CLAIM` limits.
- Comprobaciones de `canBuild` para aliados/enemigos según relación.
- Subclaims (`SUBCLAIMS` config).
- Regeneración de glass al romper/poner.

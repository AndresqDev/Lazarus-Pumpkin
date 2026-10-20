# Plan de cambios en el core de Pumpkin para HCF

- **Estado:** propuesto
- **Core base:** Pumpkin `0.2.0+26.3-26.51` (`204a94e`)
- **Plugin base:** Lazarus-Pumpkin `7ee9e40`
- **Perfil:** `hcf-1.8`, desactivado por defecto
- **Última actualización:** 2026-10-20

## 0. Resumen ejecutivo

El objetivo es que el **backend** de Pumpkin sea más rápido que Paper 1.8 en el
caso de uso HCF, sin convertir el proyecto en un fork imposible de actualizar.

**Prioridad actual: MSPT.** El cuello medido está en el tick del backend, no en
el proxy. `HCF-CORE-060` (gateway/backend 1.8 nativo) queda **explícitamente
diferido** hasta que las fases 0–2 estén medidas; no se diseña ni se implementa
ahora. Este plan se centra en bajar milisegundos por tick y CPU por jugador.

La estrategia es:

1. **Medir** antes de tocar.
2. **Eliminar trabajo** que HCF no usa.
3. **Crear homólogos** (`*_hcf`) que compongan helpers existentes, sin reescribir
   la ruta vanilla.
4. **Deshabilitar con early-return** en helpers extraídos.
5. **Añadir APIs** (`*_v2`, `*_batch`, `*_raw`) sin romper plugins existentes.
6. **Documentar cada parche** con ID, ficha, tests, métricas y rollback.
7. **Actualizar el binario** desde el plugin con firma, backup, health check y
   rollback.

### Baseline y metas

| Métrica | Baseline B21 (100 bots) | Fase 1 | Fase 2 | Fase 3 |
|---|---:|---:|---:|---:|
| CPU backend | 0.82 core | 0.55–0.65 | 0.35–0.45 | 0.25–0.35 |
| MSPT p99 | ~19–24 ms | 14–16 ms | 10–12 ms | <10 ms |
| CPU backend 200 | ~1.6 core (proyección) | 1.2–1.3 | 0.9–1.1 | 0.7–0.9 |
| MSPT p99 200 | ~38 ms (proyección) | 28–30 ms | 20–24 ms | <20 ms |

Son **gates**, no promesas. Un cambio solo se queda si el A/B demuestra la
mejora sin regresión vanilla.

### Lo que este plan no arregla

El coste de Velocity + ViaVersion (~3.0 cores a 100 bots, ~6.0 a 200) no baja
con ningún parche del core. Si el objetivo es “coste total por jugador 1.8”,
el proyecto real es un gateway/backend 1.8 nativo (Fase 5), no un `.so` de tab.
Este plan optimiza el backend y deja esa decisión para después de medir.

## 1. Alcance y no-objetivos

### En alcance

- Perfil HCF runtime y build HCF.
- Fast paths de tick, chunks, jugadores, entidades y paquetes.
- APIs aditivas para plugins.
- Instrumentación y CI de rendimiento.
- Actualización de binario desde el plugin, con seguridad y rollback.
- Documentación y gobernanza de parches.

### Fuera de alcance

- Reescribir Pumpkin desde cero.
- Portar todo el core a nativo.
- Cambiar la lógica de negocio de HCF.
- Optimizar RAM/locks a ciegas.
- Sustituir el proxy 1.8 en esta fase.
- Romper plugins existentes.

## 2. Arquitectura de la modificación

### 2.1 Ruta vanilla + ruta HCF

```text
Server::tick_worlds
  └── World::tick
        ├── ruta vanilla (sin cambios)
        └── si hcf_profile.fast_path:
              └── tick_hcf / tick_chunks_hcf / tick_players_hcf
```

La ruta HCF **se va antes**; la vanilla sigue abajo. No se cambia el
comportamiento por defecto.

### 2.2 Patrón de dispatch

```rust
pub fn tick_chunks(self: &Arc<Self>, server: &Arc<Server>) {
    // HCF-CORE-020: ruta HCF; la vanilla sigue intacta debajo.
    if self.hcf_profile().fast_chunks() {
        self.tick_chunks_hcf(server);
        return;
    }

    // ... ruta vanilla original (o helpers extraídos, mismo orden) ...
}
```

### 2.3 Patrón de deshabilitado

```rust
fn tick_weather(&self) {
    // HCF-CORE-011: si HCF no usa clima, no se ejecuta nada.
    if self.hcf_profile().disabled(HcfFeature::Weather) {
        return;
    }

    // ... lógica original ...
}
```

Si el bloque está inline, **primero se extrae a un helper** y luego se le pone
el early-return. No se envuelve un bloque grande en un `if` que dificulte el
rebase.

### 2.4 Homólogo por composición

No se duplican 200 líneas. Se extraen fases mecánicamente y se componen:

```rust
fn tick_chunks_hcf(self: &Arc<Self>, server: &Arc<Server>) {
    self.tick_scheduled_block_ticks(server);
    self.tick_fluid_ticks(server);
    if !self.hcf_profile().disabled(HcfFeature::RandomTicks) {
        self.tick_random_ticks(server);
    }
    if !self.hcf_profile().disabled(HcfFeature::NaturalSpawning) {
        self.tick_natural_spawning(server);
    }
    if !self.hcf_profile().disabled(HcfFeature::InhabitedTime) {
        self.tick_inhabited_time_hcf(server);
    }
}
```

Las reglas completas están en [PATCH_STANDARD.md](PATCH_STANDARD.md).

### 2.5 API aditiva

- Las funciones públicas existentes no cambian.
- Se añaden `*_v2`, `*_batch`, `*_raw`.
- WIT v0.1 se congela; WIT v0.2 añade interfaces nuevas.
- Plugins nativos usan ABI C versionada, no el ABI Rust inestable.
- `PUMPKIN_BUILD_ID`, `PUMPKIN_VERSION`, `PUMPKIN_HCF_ABI` y
  `PUMPKIN_CAPABILITIES` permiten que el plugin falle cerrado si no está en el
  binario correcto.

## 3. Perfil HCF

### 3.1 Configuración

Nueva sección en `pumpkin.toml`:

```toml
[hcf]
enabled = false
profile = "hcf-1.8"

# Dimensiones
only_overworld = true

# Trabajo que HCF no usa
disable_mob_ai = true
disable_natural_spawning = true
disable_raids = true
disable_villages = true
disable_weather = true
disable_random_ticks = true
disable_advancements = true
disable_statistics = true
disable_recipe_book = true
disable_chat_signing = true
disable_warden = true
disable_maps = true

# Fast paths
chunk_tick_fast_path = true
player_tick_fast_path = true
entity_tracker_fast_path = true
packet_batch = true
scheduler_budget_ms = 5
inhabited_time_hz = 1

# Colisiones
# Se desactivan las colisiones de jugador entre sí y contra mobs.
# Bloques, mobs contra bloques y proyectiles se mantienen.
player_player_collisions = false
player_mob_collisions = false

# Actualización
[hcf.updater]
enabled = false
allow_restart_with_players = false
allowed_plugins = ["lazarus"]
trusted_public_key = ""
```

Precedencia:

1. `hcf.enabled = false` ⇒ todo vanilla.
2. `profile` aplica defaults.
3. Las claves explícitas de `[hcf]` ganan sobre el perfil.
4. Si `only_overworld = true` y `allow_nether/end = true`, se ignora el segundo
   con un warning claro.
5. Los gamerules siguen siendo la fuente de verdad del comportamiento visible;
   el perfil solo decide qué rutas de código se ejecutan. Si hay conflicto, se
   loguea y se recomienda corregir la config.

### 3.2 `HcfFeature`

Bitflags/`EnumSet` con estos grupos iniciales:

- `Weather`
- `MobAi`
- `NaturalSpawning`
- `Raids`
- `Villages`
- `Dimensions`
- `RandomTicks`
- `InhabitedTime`
- `Advancements`
- `Statistics`
- `RecipeBook`
- `ChatSigning`
- `Warden`
- `Maps`
- `ChunkTickFastPath`
- `PlayerTickFastPath`
- `EntityTrackerFastPath`
- `PacketBatch`
- `SchedulerBudget`
- `PlayerPlayerCollisions`
- `PlayerMobCollisions`

`HcfProfile` se carga una vez y se guarda como `Arc<HcfProfile>` inmutable. Los
accesos son `#[inline]` y sin locks:

```rust
#[inline]
pub fn disabled(&self, feature: HcfFeature) -> bool {
    !self.enabled || self.disabled_features.contains(feature)
}
```

### 3.3 Matriz HCF

| Sistema | ¿HCF lo usa? | Default HCF | Nota |
|---|---|---|---|
| IA de mobs / spawning natural | No | Off | Los mobs de eventos se pueden whitelistear |
| Raids / aldeas / patrols | No | Off | Sin aldeas no hay raids |
| Nether / End | Normalmente no | Off | Activar si hay eventos ahí |
| Clima | No | Off | Evita tick de weather y relámpagos |
| Random ticks | No, salvo farms | Off | No desactivar si hay cultivos/fuego |
| Advancements | Opcional | Off | Configurable por si un plugin los usa |
| Statistics vanilla | Opcional | Off | Lazarus tiene sus propios contadores |
| Recipe book | No | Off | No enviar `CUpdateRecipes` |
| Chat signing | No con 1.8 | Off | `allow_chat_reports=false` |
| Warden / sculk | No | Off | Evita tracker y spawning |
| Maps | Opcional | Off | Si no hay mapas custom |
| Redstone / pistones / TNT | Quizá traps/cannon | Keep | No desactivar a ciegas |
| Fluidos | Sí | Keep | Optimizar, no quitar |
| Projectiles / pearls / potions | Sí | Keep | Fast path, no off |
| Colisiones de jugador | No | Off | Jugador-jugador y jugador-mob; bloques y proyectiles se mantienen |
| Block entities | Sí | Keep | Cofres, signs, etc. |
| View distance | 8–10 | Config | Medir antes de bajar más |
| Simulation distance | 4–6 | Config | Gran impacto en chunks activos |
| Autosave | 5 min | Async | Sin `fsync` en tick |

La desactivación de colisiones de jugador es una decisión de gameplay del perfil
HCF: los jugadores pueden ocupar el mismo bloque y atravesar mobs, pero siguen
colisionando con bloques y projectiles, y los ataques/raycasts no cambian. La
ruta vanilla conserva todas las colisiones. `HCF-CORE-029` va **antes** que
`HCF-CORE-023`; si elimina la mayoría de pares, el spatial hash de 023 se reduce
o se descarta.

## 4. Fase 0 — Fundamentos y medición

**Objetivo:** saber dónde se va el tiempo y tener una base reproducible.

| ID | Cambio | Archivos | Criterio |
|---|---|---|---|
| HCF-CORE-001 | Perfil HCF + build ID + capabilities | `pumpkin-config/src/hcf.rs`, `lib.rs`, `server/mod.rs`, `main.rs` | `/version` muestra build/perfil; default off |
| HCF-CORE-002 | Métricas por sección y p99 | `server/metrics.rs`, `ticker.rs`, `world/mod.rs`, `entity/player.rs`, `scheduler.rs` | `/tps` con p50/p95/p99 y desglose |
| HCF-CORE-003 | Estándar y documentación | `docs/core/*` | Este documento y las fichas |
| HCF-CORE-004 | Build PGO + CPU + allocator | `Cargo.toml`, CI | A/B de build actual vs PGO |
| HCF-CORE-005 | Helpers de perfil | `server/hcf.rs` | Accesos inline, sin locks |

### 4.1 Métricas mínimas

`HcfMetrics` con contadores atómicos e histograms ligeros:

- `tick_worlds`
- `tick_chunks` (block ticks, fluid ticks, random ticks, spawning, inhabited)
- `player_tick`
- `entity_tick`
- `entity_tracker`
- `block_entities`
- `packet_encode`
- `plugin_tasks`
- `scheduler_overruns`
- `players`, `entities`, `active_chunks`, `packets/s`, `bytes/s`

Exponer en `/tps` (texto y JSON) y en el comando `/debug`. No usar `tracing` por
tick si genera allocations; usar `AtomicU64` y snapshots.

### 4.2 Build

- `lto = true`, `codegen-units = 1` ya están.
- Añadir PGO:
  1. Build con `-Cprofile-generate`.
  2. Correr 100 bots 10 minutos (idle/move/fight).
  3. `llvm-profdata merge`.
  4. Rebuild con `-Cprofile-use`.
- `target-cpu=x86-64-v3` o `native` en el VPS.
- Probar `mimalloc`/`jemalloc`; no cambiar el default sin A/B.
- `PUMPKIN_BUILD_ID` generado desde git SHA + perfil + target + PGO.
- CI publica build vanilla y build HCF; ambos con hash.

**Gate:** top 10 símbolos >60 % del CPU identificados; `/tps` con p99; PGO
medido.

## 5. Fase 1 — Eliminar trabajo que HCF no usa

**Objetivo:** bajar MSPT sin tocar la lógica de juego que HCF sí necesita.

### HCF-CORE-010 — Solo dimensiones necesarias

- En `Server::new`, la lista `dimensions` se construye con `allow_nether` y
  `allow_end`.
- Si `hcf.only_overworld`, cargar solo `OVERWORLD`; no borrar datos, solo no
  cargar.
- Warning si el admin tenía Nether/End activos.
- Test: con perfil off, mismas dimensiones que antes; con perfil on, solo
  overworld.

### HCF-CORE-011 — Clima desactivable

- Extraer `tick_weather` de `World::tick_environment`.
- Early-return si `HcfFeature::Weather` está off.
- Saltar también `weather.reset_weather_cycle` y `should_reset_weather`.
- Test: el tiempo del mundo sigue avanzando; el clima no cambia.

### HCF-CORE-012 — Raids desactivables

- Extraer `tick_raids` del bloque inline de `World::tick`.
- Early-return si `HcfFeature::Raids` está off.
- Si hubiera raids existentes, loguear y no tickear; no borrar datos.
- Test: mapa sin raids no cambia; mapa con raid no crashea.

### HCF-CORE-013 — Spawning natural fuera

- Extraer `tick_natural_spawning` de `tick_chunks`.
- Early-return si `NaturalSpawning` off; no llamar a
  `get_filtered_spawning_categories`, no alocar `spawn_list`, no leer chunks
  para spawn.
- Test: con perfil off, aparece un mob en condiciones controladas; con perfil
  on, no.

### HCF-CORE-014 — Random ticks e inhabited time

- En `Level::get_tick_data`, si `random_tick_speed <= 0`, no recorrer secciones
  para random ticks; seguir recolectando block/fluid ticks.
- Extraer `tick_random_ticks`; early-return si `RandomTicks` off.
- Extraer `tick_inhabited_time_hcf`: actualizar a `inhabited_time_hz` (1 Hz) o
  time-slice por chunks; no recorrer todos los active chunks a 20 Hz.
- Test: random tick de un bloque de prueba; inhabited time avanza a la
  frecuencia configurada.

### HCF-CORE-015 — Política de IA de mobs

- Definir `HcfEntityPolicy` con:
  - `Ai` (mob normal: no tickear si `MobAi` off)
  - `Physics` (item, projectile, pearl: tickear)
  - `Player`
  - `Whitelisted` (mob de evento)
- Clasificar los tipos de entidad existentes sin cambiar su lógica.
- La aplicación de la política en `World::tick` se hace en HCF-CORE-027.
- No tocar la lógica de `Mob`; solo se evita entrar.
- Test: clasificación correcta de mob, item, projectile y pearl.

### HCF-CORE-016 — Advancements, statistics, recipe book, chat signing

> Partición de agente: `016a` (stats/advancements) en `entity/player.rs`;
> `016b` (recipe book/chat signing) en `server/mod.rs` y config. Misma ficha,
> dos commits.

- Extraer `tick_statistics`; si `Statistics` off, no incrementar por tick. Si
  está on, batch cada 20 ticks.
- Extraer `tick_advancements`; early-return si `Advancements` off.
- Saltar `CUpdateRecipes` si `send_recipes=false` (ya existe config; el perfil
  la pone en false).
- Chat signing: con `allow_chat_reports=false` no se firma; documentar y no
  añadir trabajo.
- Test: stats no cambian con off; con on, cambian cada 20 ticks como máximo.

### HCF-CORE-017 — View/sim distance, autosave, logging

> Partición de agente: `017a` (config/autosave/logging) en Foundation;
> `017b` (view/sim distance y nivel) en World. Misma ficha, dos commits.

- El perfil fija defaults de `view_distance`/`simulation_distance`; el admin
  puede override.
- `update_active_chunks` ya usa simulation distance; validar que el perfil no
  lo fuerce a 10.
- Autosave: guardar async, sin `sync_all` en el tick; revisar
  `State::atomic_write` del plugin (esto es plugin, no core).
- Logging: `level=warn` en producción; evitar logs por paquete.
- Test: con perfil on, `active_chunks` baja al bajar sim distance.

**Gate Fase 1:** backend ≤0.65 core/100 y MSPT ≤16 ms; sin regresión de
gameplay en la matriz HCF.

## 6. Fase 2 — Homólogos y hot paths

**Objetivo:** atacar los bucles que quedan después de eliminar trabajo.

### HCF-CORE-020 — Composición de `tick_chunks_hcf`

Extraer de `World::tick_chunks`:

- `tick_scheduled_block_ticks`
- `tick_fluid_ticks`
- `tick_random_ticks`
- `tick_natural_spawning`
- `tick_inhabited_time`

La ruta vanilla llama a todos en el mismo orden. La ruta HCF:

- block/fluid siempre;
- random solo si está on;
- spawning solo si está on;
- inhabited a 1 Hz/time-slice;
- reutiliza `active_chunks_vec` y `TickData` con capacidad;
- evita `sort` si el vector está vacío;
- evita `read_chunk_sync` para spawn si spawning off.

No cambiar el orden de block/fluid/random en vanilla.

### HCF-CORE-021 — `Player::tick` HCF

`Player::tick` es largo. No duplicarlo. Extraer:

- `tick_statistics` (batch/off)
- `tick_warden_tracker` (off)
- `tick_chunk_send` (solo si hay dirty/listener; evitar locks si no)
- `tick_advancements` (off)
- `tick_maps` (off si `Maps` off)
- `tick_screen_handler` (solo si hay handler)
- `tick_camera` (solo si hay target)

La ruta HCF usa un `tick_hcf` que llama a los helpers necesarios y evita los
que no. Si se prefiere no duplicar el orden, se usan early-returns en los
helpers; el efecto es el mismo.

Ideas concretas:

- Cachear `view_distance`/`version` una vez por tick.
- No lockear `chunk_sender` si `chunk_listener` está vacío y no hay epoch
  change.
- No llamar `prepare_batch` si no hay chunks pendientes.
- No lockear `stats` 5 veces por tick; un batch cada 20.
- No lockear `warden_spawn_tracker` si `Warden` off.
- Fast path para jugador normal: sin cámara, sin pantalla, sin vehículo, sin
  espectador.

### HCF-CORE-022 — Entity tracker HCF

Problemas actuales:

- `get_effective_range()` crea un `Vec` y recorre pasajeros por cada par
  entidad-jugador.
- `update_player()` lockea `player.chunk_sender` por cada par.
- `update_all()` recorre todas las entidades y, si un jugador se movió, vuelve
  a recorrer todas para ese jugador; es O(n²) en ráfagas.
- Con solo jugadores, se paga el coste genérico de mobs.

Cambios:

- Cachear `effective_range` en `TrackedEntity` (se recalcula solo si cambian
  pasajeros).
- Cachear `chunk_ready` en un `AtomicBool`/epoch por jugador; evitar lock por
  par.
- Añadir `update_all_hcf` con índice espacial por sección; actualizar solo
  entidades cercanas a jugadores que cambiaron de sección.
- Fast path `player-only`: si no hay mobs, el tracker solo gestiona jugadores,
  items y proyectiles.
- Mantener `update_all` vanilla intacto; dispatch arriba.

### HCF-CORE-029 — Colisiones de jugador desactivadas

Decisión de gameplay HCF: en 1.8 los jugadores no colisionan entre sí ni contra
mobs. Se implementa como feature de perfil, no como cambio vanilla.

- `HcfFeature::PlayerPlayerCollisions` y `HcfFeature::PlayerMobCollisions`, off
  en `hcf-1.8`.
- En el resolver de colisiones de `World::tick`, saltar los pares:
  - `Player` ↔ `Player`
  - `Player` ↔ `Mob`
- Saltar en ambos sentidos y no llamar a `on_player_collision` ni aplicar
  push/impulso para esos pares.
- No tocar:
  - colisión jugador ↔ bloque;
  - colisión mob ↔ bloque;
  - proyectiles, pearls, items y vehículos;
  - raycast/alcance de ataque;
  - hitboxes.
- No disparar eventos de colisión para los pares desactivados; documentar que
  un plugin HCF no debe depender de ellos.
- La IA de mobs sigue pudiendo targetear/atacar jugadores; solo se quita la
  colisión física/push.
- Test: dos jugadores ocupan el mismo bloque; jugador y mob se atraviesan; el
  ataque sigue registrando; jugador sigue colisionando con bloque; projectile
  sigue impactando.
- Métrica: contar pares saltados y medir la sección de colisiones. Ganancia
  **alta** con 100+ jugadores, **media/baja** con pocos mobs.
- Aceptación: es una decisión de gameplay aprobada; se acepta por correctitud
  (colisiones off, ataque/bloques on), no por superar el 3 % de mejora. Se
  revierte solo si causa un bug o una regresión medible.

### HCF-CORE-023 — Colisiones restantes con spatial hash

> Se implementa **después** de `HCF-CORE-029`. Si 029 elimina la mayoría de
> pares, este parche se reduce o se descarta.

El bucle actual dentro de `World::tick` compara cada entidad con todos los
jugadores (`players_cache`) y llama `on_player_collision`. Con 100 jugadores son
~10k comprobaciones/tick; con mobs, más.

- Extraer `tick_entity_collisions`.
- Ruta HCF: spatial hash solo para los pares que sigan activos (mob-mob,
  proyectiles/items contra jugador si aplica).
- Añadir `EntityBase::needs_player_collision()`; mobs con IA off no lo
  necesitan.
- Test: colisión de item/pearl con jugador sigue funcionando.

### HCF-CORE-024 — Batch de paquetes y serializer

Hallazgo: `java-packets.wit` ya declara `c-player-info-update` y
`c-multi-block-update`, pero `generated_packets.rs` no los serializa;
`serialize_java_packet()` devuelve `None`. Por eso existe `native_tab` y el
glass manda un block change por bloque.

Cambios:

- Implementar serialización de `CPlayerInfoUpdate` y `CMultiBlockUpdate`.
- Añadir API v0.2:
  - `send_packet_batch(list<clientbound-packet>)`
  - `send_block_changes_raw(list<tuple<s32,s32,s32,u16>>)`
  - `send_player_info_batch(...)`
- Añadir `JavaClient::send_packets_now_data(Vec<Bytes>)` que encola todos y
  espera **una** confirmación, no una por paquete.
- Para tab/glass, usar batch; para paquetes críticos, mantener
  `send_packet_now_data`.
- No cambiar `send_packet` existente.
- Test: bytes de batch == bytes de paquetes individuales; tab y glass se ven
  igual.

### HCF-CORE-025 — Scheduler con guarda y presupuesto

`TaskScheduler::tick` reprograma la tarea antes de que termine. Si tab tarda
>50 ms o glass >500 ms, se apilan.

- Añadir `running: AtomicBool` por tarea; si ya corre, saltar y contar
  `overrun`.
- Añadir `max_task_ms` por tarea/perfil; loguear overrun y saltar la siguiente
  ejecución si se repite. No cancelar una tarea a mitad.
- Medir duración por tarea/plugin.
- No cambiar la API pública.
- Test: tarea que duerme más que su intervalo no se solapa; se registra
  overrun.

### HCF-CORE-026 — Filtros de eventos

- Añadir `EventFilter`/`HandlerOptions` aditivo:
  - `PlayerMoveEvent` solo si cambió el bloque.
  - `BlockEvent` solo si el bloque cambió.
  - `PacketEvent` por tipo.
- `register_event_handler_filtered` nuevo; el registro viejo sigue igual.
- `fire_blocking` consulta el filtro antes de `block_in_place`.
- El filtro es opt-in por handler: no se aplica a warmups/combat tag que
  dependan de movimiento sub-bloque.
- Test: handler filtrado no se llama en movimientos dentro del mismo bloque;
  handler viejo sí.

### HCF-CORE-027 — Política de tick por tipo de entidad

- Complementa HCF-CORE-015.
- `EntityBase::hcf_tick_policy()` default `Physics` (compatibilidad).
- `Mob` devuelve `Ai`; `Item`/`Projectile`/`Pearl` devuelven `Physics`.
- `World::tick` filtra `tickable` según perfil.
- Whitelist de mobs de evento por tipo/ID.
- Test: mob normal no tickea; mob whitelisted sí.

### HCF-CORE-028 — Menos allocations por tick

> Partición de agente: `028a` (buffers del mundo) en World; `028b` (buffers del
> tracker) en Tracker. Misma ficha, dos commits.

Solo después de `perf`:

- `players_cache`, `tickable`, `block_entities`, `active_chunks_vec` con
  capacidad reutilizada.
- `TickData` con `Vec::with_capacity` y `clear`.
- Evitar `collect()` intermedio si se puede iterar.
- `SmallVec` para listas pequeñas (pasajeros, block entities).
- No usar `unsafe`; no cambiar el allocator global sin A/B.

**Gate Fase 2:** 0.35–0.45 core/100, MSPT p99 ≤12 ms; 200 ≤1.1 core,
p99 ≤24 ms; sin deadlocks nuevos en soak de 24 h.

## 7. Fase 3 — APIs y compatibilidad

### HCF-CORE-030 — WIT v0.2

- Congelar `v0.1`.
- Crear `v0.2` con interfaces nuevas:
  - `java-packets-v2` (batch, raw block changes)
  - `player-v2` (player info batch)
  - `core-update` (ver [UPDATER.md](UPDATER.md))
  - `capabilities`
- El host implementa v0.1 y v0.2; el plugin declara versión.
- No añadir variantes a enums v0.1.
- Test: plugin v0.1 compila y carga en host v0.2.

### HCF-CORE-031 — ABI C para nativos

- `PUMPKIN_NATIVE_ABI_VERSION`.
- `PumpkinHostApiV1` con `size` y function pointers; append-only.
- El loader valida `PUMPKIN_API_VERSION`, `PUMPKIN_NATIVE_ABI_VERSION` y
  `PUMPKIN_BUILD_ID`.
- Los plugins nativos actuales se recompilan; no se promete ABI Rust estable.
- Test: plugin nativo viejo falla con mensaje claro, no crash.

### HCF-CORE-032 — Test de compatibilidad

- CI compila un plugin v0.1 de ejemplo y lo carga en el host v0.2.
- CI compila un plugin v0.2 y verifica batch/raw.
- Test de contrato: `send_packet` viejo produce los mismos bytes.

**Gate Fase 3:** ningún plugin existente deja de cargar; APIs nuevas
documentadas y testeadas.

## 8. Fase 4 — Actualización del binario desde el plugin

La decisión está aceptada. El diseño completo está en
[UPDATER.md](UPDATER.md).

Resumen:

- `[hcf.updater] enabled = false` por defecto.
- El plugin comprueba `PUMPKIN_BUILD_ID`/`HCF_ABI`; si no coincide, avisa o
  solicita actualización.
- `core-update` es una capacidad del host con permiso `core:update`; no es
  filesystem libre.
- Manifiesto firmado Ed25519 + SHA-256 + expiración + `min_current_build`.
- Backup, rename atómico, `pending.json`, exit code 75, launcher/health check,
  rollback.
- Sin jugadores online por defecto; `allow_restart_with_players` opt-in.
- `.so` solo si el sandbox impide file/process; el `.so` no lleva lógica HCF.

**Gate Fase 4:** actualización válida, firma inválida, hash inválido, rollback y
crash loop probados en contenedor desechable.

## 9. Fase 5 — Estructural (diferida, solo con datos)

**No se empieza ahora.** El gateway/backend 1.8 (`HCF-CORE-060`) y el resto de
esta fase quedan aparcados hasta que las fases 0–2 estén implementadas y
medidas. Se documentan solo para no perder la decisión estratégica.

No se empieza sin que las fases anteriores estén medidas.

| ID | Proyecto | Cuándo | Riesgo |
|---|---|---|---|
| HCF-CORE-050 | Executor de plugins en un solo hilo | Si locks/eventos dominan p99 | Alto |
| HCF-CORE-051 | Tick regionizado | Si 300+ es objetivo y el backend escala mal | Muy alto |
| HCF-CORE-060 | Gateway/backend 1.8 nativo | Si el coste total del proxy es el objetivo | Muy alto |
| HCF-CORE-061 | Data plane nativo del plugin | Si tab/glass siguen >25 % CPU tras batch | Alto |

`HCF-CORE-060` no es un parche: es un proyecto separado de 2–4 meses. Sin él,
el stack completo con clientes 1.8 seguirá dominado por Via.

## 10. Cambios del plugin que acompañan al core

El core solo no arregla el coste del plugin. En paralelo:

| ID | Cambio | Archivo | Ganancia |
|---|---|---|---|
| LAZ-HCF-001 | Tab a 1 Hz, `online` una vez, early-out | `src/tasks.rs`, `src/tab/mod.rs` | Alta |
| LAZ-HCF-002 | Quitar `sync_all` del tick; escritura async | `src/state.rs` | Media |
| LAZ-HCF-003 | Glass sin `refresh_all`; batch por sección | `src/glass/mod.rs` | Alta |
| LAZ-HCF-004 | Scoreboard 1–2 Hz, diff por filas | `src/scoreboard/mod.rs` | Media |
| LAZ-HCF-005 | `by_member` O(1) con índice UUID | `src/factions/manager.rs` | Alta |
| LAZ-HCF-006 | `claim_at` con `ChunkKey` sin `String` | `src/factions/claim.rs` | Media |
| LAZ-HCF-007 | Move event solo al cambiar de chunk/bloque | `src/listeners/player.rs` | Alta |
| LAZ-HCF-008 | Locks fuera de llamadas al host | `src/**` | p99/deadlocks |
| LAZ-HCF-009 | Scheduler guard en tareas del plugin | `src/tasks.rs` | p99 |
| LAZ-HCF-010 | Retirar `native_tab` si el batch WASM alcanza | `native-tab/` | Menos mantenimiento |

Orden: primero tab/glass/claims, porque son el mayor coste conocido; después
el resto.

## 11. Medición, tests y gates

### 11.1 Matriz A/B

| Eje | Valores |
|---|---|
| Perfil | off / on |
| Plugin | off / on |
| Build | release / PGO |
| Carga | 100 / 200 / 300 bots |
| Fase | idle / move / fight |
| Repeticiones | 3 × 10 min |

Métricas: MSPT p50/p95/p99, CPU backend, CPU proxy, RSS, TPS, timeouts,
paquetes/s, bytes/s, overruns de scheduler.

### 11.2 Tests

La suite no se corre por parche. Se ejecuta en la fase **T1** (test-and-fix),
después de la compilación única de la fase C1. En code-first solo se revisa
estáticamente y se define qué test cubrirá cada patch.

- Unitarios: perfil off == vanilla; flags; parser de config.
- Integración/gametest: random tick, spawning, weather, raids, stats,
  advancements, packet batch, colisiones jugador-jugador/jugador-mob off con
  ataque/bloques on.
- Compatibilidad: plugin v0.1 en host v0.2; plugin nativo viejo falla limpio.
- Soak: 24 h con 100 bots; sin deadlocks, sin leaks, MSPT estable.
- Updater: firma/hash/rollback/crash loop/read-only.
- CI: `fmt`, `clippy`, `test`, build vanilla, build HCF, loadtest smoke.

### 11.3 Gates

- **G0:** métricas y PGO listos; top 10 símbolos identificados.
- **G1:** Fase 1 sin regresión; ≤0.65 core/100; MSPT ≤16 ms.
- **G2:** Fase 2 ≤0.45 core/100; p99 ≤12 ms; 200 ≤1.1 core.
- **G3:** APIs y compatibilidad; ningún plugin roto.
- **G4:** updater con rollback probado.
- **G5:** decisión sobre proxy/1.8 con números.

### 11.4 Presupuesto MSPT por sección

Presupuesto **orientativo** para Fase 2 a 100 bots (p99 ≤12 ms). No es un gate
por sí solo: `HCF-CORE-002` debe medir cada sección y el presupuesto se ajusta
con datos. Sirve para que los agentes sepan qué atacar primero y para evitar
microoptimizar una sección que no es el cuello.

| Sección | Presupuesto p99 | Dueño típico |
|---|---:|---|
| World tick: chunks, bloques, fluidos, random ticks | 4.0–4.5 ms | World |
| Player tick: stats, warden, chunks, packets | 2.0–2.5 ms | Player |
| Entity tracker: movimiento, rangos, spawn/despawn | 1.5 ms | Tracker |
| Entity tick + colisiones restantes (029) | 1.0–1.5 ms | World |
| Scheduler, eventos y host de plugins | 1.0 ms | Scheduler |
| Reserva para picos/GC del allocator | 1.0–1.5 ms | Orquestador |
| **Total** | **≤12 ms** | — |

Si una sección consume más de su presupuesto, se prioriza esa sección en la
siguiente oleada. Si todas están dentro y el p99 sigue alto, el problema es de
picos/allocator/locks, no de suma de medias.

## 12. Roadmap y esfuerzo

| Semana | Entregable |
|---|---|
| 1 | Fase 0: perfil, métricas, PGO, baseline A/B |
| 2–3 | Fase 1: dimensiones, weather, raids, spawning, random ticks, mob AI, stats |
| 4–5 | Fase 2a: chunk tick, player tick, entity tracker |
| 6 | Fase 2b: colisiones, packet batch, scheduler, filtros |
| 7 | Fase 3: WIT v0.2, compatibilidad, tests |
| 8 | Fase 4: updater, manifiesto, launcher, rollback |
| 9–10 | Soak, ajustes, documentación, decisión Fase 5 |

Esfuerzo orientativo: **8–10 semanas** para el backend en modo serial, con
20–30 % de mantenimiento continuo por rebase. El gateway 1.8 es aparte y queda
diferido.

### 12.1 Ejecución acelerada con agentes

El plan anterior es la estimación serial. La implementación se acelera con
agentes en paralelo, pero sin paralelizar lo que no se puede paralelizar:
benchmarks, merges y decisiones de gameplay. El detalle operativo está en
[`AGENT_PLAYBOOK.md`](AGENT_PLAYBOOK.md).

Regla:

> **Paralelizar código y análisis. Serializar compilación final, tests,
> benchmarks, merges y decisiones.**

| Oleada | Duración | Qué se paraleliza | Gate |
|---|---:|---|---|
| 0. Recon | 60–90 min | 3 scouts + profiler + baseline | Top 10 símbolos y baseline p99 |
| 1. Fundamentos | 90–120 min | Perfil, métricas, build ID, docs | Contratos mergeados; `cargo check` único |
| 2. Implementación code-first | 3–4 h | World, Player, Tracker, Packet, Scheduler | Cada branch con commit; **sin build/test** |
| 3. Merge + compilación | 60–120 min | Merge queue; fixes por archivo | Workspace compila (C1) |
| 4. Test-and-fix | 2–4 h | Tests por módulo; fixes | Suite y paridad verdes (T1) |
| 5. Benchmark | 2–3 h | Nada: un solo runner | p99 ≥3 % mejor; sin regresión (B1) |
| 6. Cierre | 45–60 min | Docs + siguiente plan | Fichas y tracker actualizados |

Gates de ejecución: **C1** = workspace compila; **T1** = tests/paridad verdes;
**B1** = benchmark aceptado. `HCF-CORE-029` es una decisión de gameplay: B1 mide
su impacto, pero se acepta por correctitud y solo se revierte por bug/regresión.

El orden es deliberado: **primero se escribe todo el código, después se compila
y se testea**. La implementación inicial de 7–15 h baja a **2–4 h**; el total
con validación queda en **7–12 h**, no en 2 h. La aceleración realista es
**2–3×**, no 10×: compilar Rust, correr tests y medir 100/200 bots tienen un
suelo que no baja por lanzar más agentes.

Condiciones para que funcione:

- Los contratos (`HcfProfile`, `HcfMetrics`, firmas de helpers, API de paquetes)
  se mergean **antes** de la oleada paralela.
- En la Oleada 2 los agentes **no** compilan, testean ni benchmarkean; solo
  escriben y commitean. La única compilación temprana es el `cargo check` de
  contratos.
- Un solo dueño por archivo; `world/mod.rs` es el long pole y no se edita en
  paralelo.
- Cada agente trabaja en un `git worktree` con `CARGO_TARGET_DIR` propio y
  `sccache` compartido.
- Cada agente tiene timebox y devuelve branch, commit, archivos y blockers.
- La primera compilación completa y la suite se hacen en serie; los fixes se
  reparten por archivo.
- El benchmark corre con lock exclusivo y sin builds grandes en paralelo.

## 13. Riesgos y mitigaciones

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Rebase roto por upstream | Alto | Parches pequeños, helpers, rebase semanal |
| Regresión de gameplay | Alto | Perfil off = vanilla; tests; config |
| Colisiones off rompen expectativas | Medio | Perfil opt-in; documentar; tests de ataque/bloque |
| Deadlock por locks | Alto | No locks nuevos en tick; soak; `perf` |
| Pérdida de rendimiento por duplicar lógica | Medio | Homólogo por composición, no copia |
| Plugin roto por API | Alto | WIT v0.1 congelado; v0.2 aditivo; tests |
| Updater comprometido | Muy alto | Firma, pin, opt-in, backup, rollback |
| Reinicio con jugadores | Alto | Default false; countdown/kick; aviso |
| Proxy sigue dominando | Muy alto | Fase 5; no prometer total sin 1.8 |
| `.so` ABI roto | Medio | ABI C, build ID, recompilar |

## 14. Checklist de entrega

- [ ] `HCF-CORE-001` perfil/build ID/capabilities.
- [ ] `HCF-CORE-002` métricas y `/tps` p99.
- [ ] `HCF-CORE-004` PGO y build HCF.
- [ ] `HCF-CORE-029` colisiones de jugador off y tests de ataque/bloque.
- [ ] Fase 1 completa y medida.
- [ ] Fase 2 completa y medida.
- [ ] WIT v0.2 y compatibilidad v0.1.
- [ ] Updater con firma/rollback probado.
- [ ] Documentación de cada parche.
- [ ] Soak 24 h.
- [ ] Decisión documentada sobre proxy/1.8.

## 15. Enlaces

- [Estándar de parches](PATCH_STANDARD.md)
- [Registro de parches](PATCHES.md)
- [Actualizador de binario](UPDATER.md)
- [Política de upstream y rebase](UPSTREAM.md)
- [Playbook de agentes](AGENT_PLAYBOOK.md)
- [Volver al índice](README.md)

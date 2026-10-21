# HCF-CORE-021 — `Player::tick` HCF

- **Estado:** implementado 021a (C1 pendiente)
- **Fase:** 2
- **Upstreamable:** parcial (extracción de helpers sí; perfil HCF no)
- **Riesgo:** medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`Player::tick` (`crates/pumpkin/src/entity/player.rs:2641-2953` en `204a94e`)
hace trabajo fijo por jugador y por tick, aunque el perfil HCF no use la fase:

- `process_inbound_packets` resuelve el jugador por UUID incluso sin paquetes
  (`player.rs:2581`); `get_player_by_uuid` es O(P)
  (`world/mod.rs:4704` en el core base).
- cámara (`:2651-2675`), pantalla (`:2677-2694`), stats con 5 incrementos y
  hashing (`:2696-2711`), warden con lock bloqueante (`:2718-2721`).
- `tick_chunk_send` (`:2722-2807`) lockea listener/sender varias veces, crea un
  `FxHashMap` por jugador/tick (`:2754`) y llama `pair_entities_in_chunks`, que
  vuelve a resolver el jugador por UUID (`:1068-1080`).
- mining (`:2834-2882`) vuelve a llamar `get_player_by_uuid` (`:2835`) aunque
  ya está en `&self`.
- advancements (`:2914-2917`), `tick_maps` (`:2923`, `:5170-5175`) y
  `tick_raid_omen` (`:2922`, `:4420-4467`) se ejecutan siempre.
- idle timeout (`:2938-2952`) y chat spam se calculan por jugador.

El perfil HCF ya desactiva `Statistics`, `Advancements`, `Warden`, `Maps` y
`Raids` (`server/hcf.rs:31-77`, defaults de `hcf-1.8`). 016a es el parche que
hace el batching/early-return de stats y advancements; 021 compone el tick del
jugador y evita el resto del trabajo sin duplicar `Player::tick`.

## Cambio

1. Extraer helpers mecánicos de `Player::tick`, conservando el orden exacto de
   la ruta vanilla:
   - `tick_camera` (`:2651-2675`)
   - `tick_screen_handler` (`:2677-2694`)
   - `tick_statistics` (`:2696-2711`; delega en 016a si ya está)
   - `tick_warden_tracker` (`:2718-2721`)
   - `tick_chunk_send` (`:2722-2807`)
   - `tick_mining` (`:2834-2882`)
   - `tick_advancements` (`:2914-2917`; incluye
     `check_inventory_advancements` y `flush_dirty`)
   - `tick_maps` (`:2923`)
   - `tick_raid_omen` (`:2922`)
   - `tick_idle_timeout` (`:2938-2952`)
2. Añadir early-returns por feature **dentro de los helpers**, no un
   `if` gigante alrededor de `Player::tick`:
   ```rust
   // HCF-CORE-021: HCF no actualiza mapas.
   if server.hcf_profile().disabled(HcfFeature::Maps) {
       return;
   }
   ```
   Aplicar a `Statistics` (si 016a no lo hizo ya), `Advancements`, `Warden`,
   `Maps` y `Raids` (raid omen). El resto de fases se conservan.
3. `tick_screen_handler`: lockear solo si hay una pantalla abierta (handler
   distinto del `player_screen_handler` base) o si `inventory_changed` está
   dirty; no eliminar la sincronización del handler base sin un flag dirty
   (eso requiere coordinación con 016/028 y puede perder sincronización). Si
   hay pantalla, conservar la validación de merchant y el cierre actual. No
   separar `screen_dirty` de `inventory_changed` en 021.
4. `tick_chunk_send`:
   - cachear `world`, `player_chunk`, `version` y `view_distance` una vez por
     tick;
   - comprobar listener/pending/epoch antes de lockear;
   - no hacer el `try_lock` extra para `sent_chunks_count`; usar
     `sent.len()` del `commit_batch`;
   - no crear el `FxHashMap` si no hay batch preparado (ya ocurre, pero
     asegurarlo con un early-return);
   - conservar el filtro por epoch y `mark_delivered`.
5. `tick_mining`: eliminar el `get_player_by_uuid` de `:2835`; usar `self`
   directamente (`self.world()`, `self.continue_mining`,
   `world.break_block(&pos, Some(self), flags)`, `self.increment_stat`). Es el
   mismo objeto y evita una búsqueda O(P) por jugador que mina.
6. Cachear por tick el `HcfProfile` desde `server.hcf_profile()` (005). No
   añadir un campo nuevo a `Player` ni un lock.
7. No cambiar la firma `pub fn tick(&self, server: &Server)`. **No se toca
   `world/mod.rs`** (`world/mod.rs:1561` en base) ni `server/mod.rs:1124`; no se
   introduce `tick_arc` en este parche. Si más adelante se quiere pasar
   `&Arc<Player>` para eliminar el `get_player_by_uuid` de
   `process_inbound_packets`, será un parche separado con sus llamadores.
8. Lo que **no** se puede saltar: `process_inbound_packets` (siempre primero),
   `living_entity.tick`, breath, hunger, health, experience, `update_player_pose`,
   mining, chat spam, idle timeout y el ACK de predicciones.

### Comportamiento

- Con `hcf.enabled = false`, la ruta vanilla ejecuta los mismos helpers en el
  mismo orden y con la misma semántica.
- Con HCF on, stats/advancements/warden/maps/raid omen dejan de actualizarse
  (o se batchean si 016a está on). Los plugins que lean esos valores verán
  datos congelados o con retraso; es una decisión del perfil, no un bug.
- La pantalla abierta sigue funcionando; solo se evita el trabajo cuando no hay
  pantalla. Un jugador normal (sin cámara, sin pantalla, sin vehículo, sin
  espectador) recorre la mayoría de helpers con early-return barato.

## Patrón

- [x] Homólogo con early-returns (sin duplicar `Player::tick`)
- [x] Early-return por feature
- [x] Config (`player_tick_fast_path`, features de 005)
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/entity/player.rs` — extracción de helpers y early-returns.
  **021 es dueño de `player.rs` en esta oleada.** 016a también toca
  `player.rs` (stats/advancements); 016a va antes. 022 **no** toca este archivo.
- `crates/pumpkin/src/server/hcf.rs` — `fast_players()` y features ya existen
  (005); no se añade API.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.
- No se toca `world/mod.rs`, `server/mod.rs`, WIT ni `entity_tracker.rs`.

## Compatibilidad

- APIs públicas: no cambian; `Player::tick` conserva firma. No se toca WIT.
- Plugins v0.1: no se toca ningún contrato. Los plugins que dependan de stats,
  advancements, mapas o warden deben saber que el perfil HCF los apaga.
- Jugadores vanilla: con `enabled = false`, orden y llamadas idénticos.
- Datos: no se borran stats/advancements/mapas; solo se dejan de actualizar o
  se batchean. Al volver a vanilla, los valores siguen siendo válidos.
- Gameplay HCF: mapas no se actualizan, advancements no progresan, warden
  tracker no avanza. Es opt-in por perfil.

## Métricas

- **Antes:** sección `Players` de 002, tiempo por helper, número de
  `get_player_by_uuid`, locks de `chunk_sender`/`stats`/`warden`, MSPT p99. El
  baseline B21 (18,99 ms) es global; no atribuye `Player::tick`.
- **Después:** menos tiempo en `Players`, menos locks y una búsqueda UUID menos
  por jugador que mina. `process_inbound_packets` conserva el lookup cuando hay
  paquetes, pero evita `get_player_by_uuid` y el upgrade del servidor si la cola
  está vacía (después del ACK de predicciones).
- **Método:** A/B 3 × 10 min, 100 bots, perfil off/on; comparar con 016a ya
  aplicado para no atribuir a 021 el batching de stats. **Ganancia: a medir.**
- Medir por separado un jugador idle, uno moviéndose y uno con pantalla abierta;
  el fast path no debe romper ninguno.

## Tests

- [ ] Unitario: perfil off ⇒ la secuencia de helpers es la misma que
      `Player::tick` actual.
- [ ] Unitario: `Statistics` off/016a on ⇒ no se incrementan stats por tick o
      se hace en batch, según 016a.
- [ ] Unitario: `Advancements` off ⇒ no se llama `check_inventory_advancements`
      ni `flush_dirty`.
- [ ] Unitario: `Warden` off ⇒ no se lockea `warden_spawn_tracker`.
- [ ] Unitario: `Maps` off ⇒ no se llama `tick_maps`.
- [ ] Unitario: `Raids` off ⇒ no se llama `tick_raid_omen`.
- [ ] Unitario: pantalla abierta ⇒ `send_content_updates` sigue ejecutándose;
      sin pantalla ⇒ no se lockea.
- [ ] Unitario: mining sigue rompiendo el bloque y sumando stats sin
      `get_player_by_uuid`.
- [ ] Unitario: cola inbound vacía ⇒ no se resuelve el jugador; con un paquete
      ⇒ se resuelve y el ACK de predicciones sigue enviándose.
- [ ] Integración: salud, hambre, respiración, pose, experiencia y timeout no
      cambian con el perfil on.
- [ ] Integración: chunks pendientes siguen enviándose y `mark_delivered` se
      respeta.
- [ ] Regresión: cámara/espectador/vehículo siguen funcionando.
- [ ] Soak: 30 min, 100 bots, sin desync de pantalla ni chunks atascados.
- [ ] Loadtest: 100/200 bots; MSPT p99 y sección `Players`.
- [ ] Suite ejecutada y verde en T1.

## Estado de implementación (2026-10-20)

- Implementado 021a en `hcf/core-021`, commit `ace9cb8`, mergeado en
  `hcf/foundation`.
- Extraídos `tick_camera`, `tick_screen_handler`, `tick_statistics`,
  `tick_warden_tracker`, `tick_mining`, `tick_advancements` y
  `tick_idle_timeout`; el orden de `Player::tick` se conserva.
- Guards por feature: stats, warden, advancements, maps y raids. El perfil se
  cachea una vez por tick y los guards usan `disabled(...)`; `hcf.enabled=false`
  restaura el comportamiento vanilla.
- `tick_screen_handler` evita el lock interno y `send_content_updates` cuando
  el handler actual es el `player_screen_handler` base y `inventory_changed`
  está limpio. Si está dirty, sincroniza el handler base sin consumir el flag,
  para que `check_inventory_advancements` lo siga viendo en el mismo tick.
- `tick_mining` ya no resuelve el jugador por UUID; solo conserva el lookup en
  la ruptura Bedrock, que necesita `Arc<Player>`.
- `process_inbound_packets` retorna después del ACK de predicciones si
  `inbound_packets` está vacío. Con paquetes pendientes conserva el lookup y el
  límite de 64/tick. Evita el coste O(P) en jugadores idle.
- El bloque `chunk_send` se dejó inline. El único cambio es saltar
  `prepare_batch` cuando el fast path está activo y `pending_chunks` está vacío
  tras drenar el listener; `commit_batch`/`mark_delivered` no cambian.
- C1/T1 pendientes; no se ejecutó compilación local.

## Rollback

`hcf.player_tick_fast_path = false` o `hcf.enabled = false` y reinicio. Los
helpers extraídos siguen siendo válidos y la ruta vanilla los llama igual. No
hay datos que migrar. Si el problema es una feature concreta, activar su flag
(`disable_statistics`, `disable_advancements`, `disable_warden`,
`disable_maps`, `disable_raids`) sin revertir 021.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivo con conflicto probable: `entity/player.rs` (`tick` y helpers).
- Orden: después de 005 y 016a; antes de la medición final de 022. 021 puede
  empezar con la extracción mecánica (021a) y dejar los fast paths (021b) para
  un segundo commit, pero la ficha es una sola.
- **No se toca `world/mod.rs`**: se conserva `Player::tick(&self, server)` y
  los llamadores actuales. Un `tick_arc` o un cambio de receiver requiere otro
  parche y coordinación con el dueño World.
- 022 no puede tocar `player.rs`; si 022 necesita un contexto de chunk-ready,
  lo implementa dentro del tracker (ver su ficha).
- Si upstream cambia `Player::tick`, conservar los helpers y el orden; no
  reescribir la función completa.

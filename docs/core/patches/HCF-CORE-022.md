# HCF-CORE-022 — Entity tracker HCF

- **Estado:** implementado 022a (C1 pendiente)
- **Fase:** 2
- **Upstreamable:** parcial (cachés genéricas sí; índice HCF no)
- **Riesgo:** medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`EntityTracker::update_all` (`crates/pumpkin/src/world/entity_tracker.rs:606-642`
en `204a94e`) recorre **tres veces** todas las entidades:

1. entidades que cambiaron de sección (`:610-628`);
2. todos los jugadores movidos contra todas las entidades (`:630-634`);
3. entidades con metadata dirty (`:636-641`).

Cada par pasa por `TrackedEntity::update_player` (`:100-140`), que:

- calcula `get_effective_range` (`:80-91`), crea un `Vec` y recorre/clona
  pasajeros **por par** (con pasajeros puede alocar);
- llama `get_view_distance(player)` (`chunker.rs:15-29`), que hace
  `world()`/`server.upgrade()`/config por par;
- lockea `player.chunk_sender` de forma bloqueante (`:127-131`) por par.

Además:

- `update_player_position` (`:572-590`) escanea todas las entidades por cambio
  de sección y recarga `world.players` dentro del bucle;
- `update_player_chunks` (`:542-557`) crea un `FxHashSet` y escanea todas las
  entidades por batch de chunks;
- `pair_new_player_with_tracked_entities` (`:532-539`) escanea E por login;
- `add_entity` (`:509-529`) hace spawns O(P) por entidad;
- `update_all` resuelve `get_player_by_uuid` (`:622`) para cada jugador movido.

Con solo jugadores se paga el coste genérico de mobs. La hipótesis del profiler
(10–20 % de CPU en tracker, no confirmada) debe validarse con 002 antes de
aceptar 022b. **No se inventa una ganancia:** 022a puede entrar si el A/B
demuestra que la sección `Tracker` pesa; 022b solo si 022a no basta.

## Cambio

### 022a — caché y contexto por jugador (sin índice espacial)

1. `update_all` conserva su firma y su cuerpo vanilla; al principio:
   ```rust
   // HCF-CORE-022: dispatch interno, sin tocar world/mod.rs.
   if world
       .server
       .upgrade()
       .is_some_and(|server| server.hcf_profile().fast_tracker())
   {
       self.update_all_hcf(world);
       return;
   }
   ```
   El `upgrade()` es una vez por tick; no se hace por entidad/par. Si 029 o 020
   añaden un accessor `World::hcf_profile()`, se puede usar en un rebase, pero
   022 no depende de ello.
2. `get_effective_range` fast path: si `passengers` está vacío, devolver
   `tracking_range` sin alocar `Vec` ni clonar `Arc`. Solo recorrer pasajeros si
   los hay; cachear el resultado en `TrackedEntity` únicamente si el A/B
   demuestra que los pasajeros son frecuentes. La invalidación al montar/
   desmontar debe ser explícita; sin ella, no cachear.
3. Contexto por jugador una vez por tick: `view_distance`, `watched_section`,
   epoch y un `FxHashMap<Uuid, Arc<Player>>` para `update_all_hcf`. No resolver
   `get_player_by_uuid` por entidad movida.
4. `update_player_position` mueve `world.players.load()` fuera del bucle y
   reutiliza el contexto; `update_player_chunks` reutiliza el contexto y el
   índice en vez de re-escanear. No cambian sus firmas públicas.
5. `chunk_ready`: en los métodos bulk (`update_all_hcf`,
   `update_player_position`, `update_player_chunks`) lockear `chunk_sender`
   **una vez por jugador** y comprobar `is_chunk_ready` de los chunks
   candidatos antes de soltar; no una vez por par. `update_player` individual
   (usado por `add_entity`/`pair_new_player`) conserva el lock actual para no
   cambiar su contrato.
6. `TrackedEntity::seen_by`: conservar `DashSet` en 022a. Un índice inverso o
   `DashMap` por jugador es 022b, con medición.

### 022b — índice espacial y `update_all_hcf` (solo si 022a no basta)

1. Construir un índice `HashMap<Vector2<i32>, SmallVec<Arc<TrackedEntity>>>`
   por sección, reconstruido O(E) al inicio del tick o actualizado de forma
   incremental en `add_entity`/`remove_entity`/cambio de sección. El índice debe
   aplicar después la distancia exacta, `broadcast_to_player`, `watched_section`
   y `is_chunk_ready`; el índice solo reduce candidatos, no decide visibilidad.
2. `update_all_hcf` player-major: para cada jugador movido, consultar solo las
   secciones dentro de su view distance; no recorrer todas las entidades. Evitar
   el doble par entidad/jugador movido de `:630-634`.
3. `update_player_position` usa `loading_chunks`/`unloading_chunks` de
   `chunker::update_position` (`chunker.rs:76-79`, `:149`) en vez de escanear
   todas las entidades.
4. `update_player_chunks` consulta el índice por sección; elimina el
   `FxHashSet` intermedio (`:548`) solo si el índice ya deduplica.
5. `pair_new_player_with_tracked_entities` usa el índice; no escanea E por
   login.
6. `add_entity`/`send_to_tracking_players*`: si el A/B muestra presión, usar
   `seen_by`/índice inverso para no recorrer P por paquete. No cambiar el orden
   de spawn/despawn ni el filtro por versión (`world/mod.rs:944-958`).

### Lo que NO se toca

- **No se toca `entity/player.rs`.** 022 no añade campos a `Player`, no cambia
  `pair_entities_in_chunks` ni `chunk_sender`. La resolución UUID de
  `pair_entities_in_chunks` (`player.rs:1068-1080`) queda para 021 o un parche
  posterior de `player.rs`.
- No se toca `world/mod.rs`: el dispatch vive dentro de `EntityTracker`.
- No se cambia `update_all` vanilla: se conserva entero y se usa con el perfil
  off o `entity_tracker_fast_path = false`.
- No se cambia la semántica de pairing: mismos spawn/despawn, mismos paquetes,
  mismo `seen_by`, mismo filtro de espectador/versión.
- No se usa `unsafe` ni un lock nuevo en el tick path.

## Patrón

- [x] Homólogo por composición (dispatch dentro del tracker)
- [x] Caché/índice
- [x] Config (`entity_tracker_fast_path`)
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/world/entity_tracker.rs` — `update_all` dispatch,
  `update_all_hcf`, contexto, índice y cachés. **022 es dueño de este archivo.**
  028b también toca buffers del tracker; 022 va antes y 028b después, no en
  paralelo.
- `crates/pumpkin/src/net/chunk_sender.rs` — solo si hace falta un método de
  consulta/snapshot de `is_chunk_ready`; no se cambia la semántica de
  `commit_batch`.
- `crates/pumpkin/src/server/hcf.rs` — `fast_tracker()` ya existe (005).
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.
- **No se toca** `entity/player.rs` ni `world/mod.rs`.

## Compatibilidad

- APIs públicas: no cambian; `update_all`, `update_player_position` y
  `update_player_chunks` conservan firma. No se toca WIT.
- Plugins v0.1: no se toca ningún contrato. Los plugins ven los mismos
  spawn/despawn y metadata; si el índice se equivoca, aparecen fantasmas o
  despawns prematuros, que es el fallo a evitar.
- Jugadores vanilla: con `enabled = false` o `entity_tracker_fast_path = false`,
  `update_all` vanilla intacto.
- Datos: no se guardan ni migran estructuras del tracker; es estado efímero.
- 029: al permitir apilar jugadores, la densidad de pares **aumenta**; 022 se
  mide después de 029, con bots apilados y separados. No se atribuye a 022 una
  mejora que en realidad venga de 029.

## Métricas

- **Antes:** sección `Tracker` de 002, pares comprobados/visibles, locks de
  `chunk_sender`, `get_effective_range` calls, `get_player_by_uuid` calls,
  MSPT p50/p95/p99. La hipótesis 10–20 % del profiler es eso, una hipótesis.
- **Después (022a):** menos allocs en `get_effective_range`, un lock por
  jugador en los métodos bulk y menos búsquedas UUID. **A medir.**
- **Después (022b):** menos pares candidatos por jugador movido. **A medir.**
- **Método:** A/B 3 × 10 min, 100 bots, perfil off/on; 100/200 bots; apilados
  vs separados (029 ya aplicado). Medir p99 y estabilidad de pairing, no solo
  media. **Criterio de aceptación:** 022b solo si 022a no reduce el coste de
  tracker de forma clara y estable; si no, se descarta o se reduce el alcance.
  No se acepta por TPS medio.

## Tests

- [ ] Unitario: perfil off ⇒ `update_all` ejecuta el cuerpo vanilla y llama a
      `update_player` con los mismos pares.
- [ ] Unitario: `entity_tracker_fast_path = false` con HCF on ⇒ ruta vanilla.
- [ ] Unitario: `get_effective_range` sin pasajeros no aloca/recorre.
- [ ] Unitario: entidad con pasajeros conserva el rango efectivo máximo.
- [ ] Unitario: jugador que cambia de sección empareja/desempareja una sola vez.
- [ ] Unitario: jugador movido en ráfaga no genera spawns duplicados.
- [ ] Unitario: chunk no listo ⇒ no se spawnnea la entidad; chunk listo ⇒ sí.
- [ ] Unitario: espectador, vehículo y pasajeros conservan su comportamiento.
- [ ] Integración: 100 bots apilados y separados; sin fantasmas ni despawns
      prematuros.
- [ ] Integración: login/logout/respawn y cambio de dimensión siguen paired.
- [ ] Soak: 30 min; `seen_by` no crece sin límite.
- [ ] Loadtest: 100/200 bots; sección `Tracker`, p99, paquetes/s.
- [ ] Suite ejecutada y verde en T1.

## Estado de implementación (2026-10-20)

- Implementado 022a en `hcf/core-022`, commit `2334c44`, mergeado en
  `hcf/foundation`.
- `update_all` despacha a `update_all_hcf` cuando `fast_tracker()` está activo;
  reproduce las tres fases vanilla con contextos por jugador cacheados.
- `PlayerTrackerContext` cachea view distance, watched section y epoch; el
  snapshot de chunks listos se toma como máximo una vez por jugador y pase,
  mediante `ChunkSender::ready_chunks_snapshot()` (API aditiva).
- `get_effective_range` evita el `Vec`/clones cuando no hay pasajeros; si hay,
  conserva el máximo. `update_player_position` y `update_player_chunks`
  reutilizan contextos y no hacen lookup por UUID en el camino común.
- No se implementó el índice espacial; 022b queda condicionado al A/B de 022a.
- C1/T1 pendientes; no se ejecutó compilación local.

## Rollback

`hcf.entity_tracker_fast_path = false` o `hcf.enabled = false` y reinicio: se
usa `update_all` vanilla. No hay datos que migrar. Si 022b introduce fantasmas y
022a es estable, se puede desactivar solo el índice con un flag interno de
desarrollo, pero el rollback público es el flag de perfil.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivo con conflicto probable: `world/entity_tracker.rs`. También puede
  haberlo en `net/chunk_sender.rs` si se añade una consulta.
- Orden: después de 005/002 y de 021 (para estabilizar el tick del jugador);
  la medición final va **después de 029** porque apilar jugadores cambia la
  densidad de pares. 022b solo después de 022a y de la medición.
- **No se toca `entity/player.rs`**: si se necesita un epoch de chunk-ready,
  se implementa como caché local del tracker o se coordina con 021; no se añade
  un campo a `Player` desde 022.
- **No se toca `world/mod.rs`**: el dispatch es interno a `update_all`. Si 020 o
  029 añaden `World::hcf_profile()`, se puede usar como optimización de rebase,
  pero no es un requisito.
- 028b (buffers del tracker) no puede ir en paralelo con 022; primero se
  estabiliza 022, luego se miden allocations.

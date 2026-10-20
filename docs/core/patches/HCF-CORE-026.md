# HCF-CORE-026 — Filtros de eventos y movimiento

- **Estado:** en diseño
- **Fase:** 2
- **Upstreamable:** parcial (filtros genéricos sí; política HCF no)
- **Riesgo:** medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

El dispatch de eventos construye y entrega eventos aunque no hagan falta:

- `PacketReceivedEvent` se crea con `player.clone()` y `payload.clone()` antes
  de cada paquete de juego, incluso si no hay handler
  (`crates/pumpkin/src/net/java/mod.rs:887-895` en `204a94e`;
  `fire_blocking` en `plugin/mod.rs:1243-1268`).
- `PlayerMoveEvent` se dispara en posición (`player_position.rs:90-97`) y en
  posición-rotación (`:239-245`) sin comparar `from == to` ni el bloque;
  rotación y micro-movimiento pagan el dispatch blocking completo.
- `fire` recorre los handlers y llama `block_in_place`/`runtime.block_on` si
  hay alguno (`plugin/mod.rs:1213-1268`).
- `BlockPhysicsEvent` se emite por cada dirección de vecino
  (`world/mod.rs:6082-6097`, `:6140-6146`) sin comprobar cambio de estado;
  `furnace`/`hopper` construyen strings antes de `fire_blocking`
  (`furnace_like_block_entity.rs:400-489`, `hopper.rs:395-405`).
- `PacketSentEvent` tiene helpers (`entity/player.rs:3081-3117`) pero **no
  tiene call sites**; conectarlo sin filtro multiplicaría eventos por glass/tab.

HCF necesita poder registrar handlers que solo reciban lo que les importa,
sin cambiar la semántica de los handlers existentes ni el orden actual.

## Cambio

### 026.1 API aditiva de registro

1. Añadir:
   ```rust
   pub struct HandlerOptions {
       pub priority: EventPriority,
       pub blocking: bool,
       pub filter: Option<EventFilter>,
   }

   pub enum EventFilter {
       PlayerMove(PlayerMoveFilter),
       PacketReceived(PacketIdFilter),
   }

   pub enum PlayerMoveFilter {
       Any,
       BlockChanged,
       PositionChanged,
   }

   pub struct PacketIdFilter { /* Any | Ids(SmallVec<[i32; 4]>) */ }
   ```
2. Añadir `register_with_options<E, H>(handler, options)`. `register` se
   mantiene con la misma firma y se implementa como
   `register_with_options(handler, HandlerOptions::default())`; el default no
   tiene filtro, así que un handler viejo recibe **todos** los eventos.
3. `TypedEventHandler` gana un campo `filter: Option<EventFilter>`; no se
   cambian los traits públicos ni `EventHandler`.
4. `fire_blocking` consulta un `has_matching_handlers` barato antes de
   `block_in_place`; si todos los handlers están filtrados para ese evento,
   retorna sin entrar al runtime. `fire` aplica el mismo filtro antes de
   downcast/llamada.
5. El filtro es **por handler y opt-in**. No se aplica globalmente ni por
   perfil. Un handler de warmup/combat tag que dependa de movimiento sub-bloque
   debe seguir registrándose sin filtro.

### 026.2 `PlayerMoveEvent`

1. En `player_position.rs:90-97` y `:239-245`, antes de construir el evento,
   consultar un helper barato del `PluginManager`:
   ```rust
   if !server.plugin_manager.has_matching_player_move(from, to) {
       // No hay handler que quiera este movimiento: se aplica igual,
       // pero no se construye el evento ni se clona el Arc<Player>.
   } else {
       // send_cancellable_blocking! actual
   }
   ```
   El movimiento se aplica siempre; el filtro solo decide si se dispara el
   evento. Si un handler con filtro `BlockChanged` está registrado y el jugador
   se mueve dentro del mismo bloque, no se le llama.
2. `BlockChanged` compara la posición de bloque de `from` y `to`;
   `PositionChanged` compara el vector completo; `Any` equivale a un handler
   sin filtro. La rotación sola no debe disparar `BlockChanged`.
3. El filtro se evalúa **antes** de `send_cancellable_blocking!`; si no hay
   handler que matchee, no se paga el `Arc::clone` ni el `block_in_place`.
4. Si hay handler y el evento se cancela, se conserva la semántica actual: el
   movimiento no se aplica. Un handler filtrado que no recibe el evento no
   puede cancelarlo; es el precio de opt-in.

### 026.3 `PacketReceivedEvent`

1. En `net/java/mod.rs:887-895`, antes de `player.clone()`/`payload.clone()`:
   ```rust
   if !server
       .plugin_manager
       .has_matching_packet_received(packet.id)
   {
       // seguir al handler de play packet sin construir el evento
   } else {
       // construir y fire_blocking actual
   }
   ```
2. `PacketIdFilter` puede ser `Any` o una lista pequeña de IDs. Un handler sin
   filtro sigue viendo todos los paquetes; uno filtrado solo los IDs elegidos.
3. No cambiar el `packet_id` ni el payload que ve el handler; no cambiar la
   semántica de cancelación para los que sí matchean.
4. La ruta Bedrock construye el evento en `entity/player.rs:2616-2623`. 026
   **no toca `player.rs`**; esa ruta queda para 021 o un follow-up. Documentar
   que el fast path lazy de 026 es Java primero.
5. `PacketSentEvent` **no se conecta en 026**. Sigue sin call sites. Si en el
   futuro se necesita, debe tener filtro por ID/tipo antes de construir el
   payload; conectarlo sin filtro con glass/tab es un multiplicador de coste.

### 026.4 `BlockPhysicsEvent` y otros

- El filtro “solo si cambió el bloque” para `BlockPhysicsEvent` requiere tocar
  `world/mod.rs` (`:6082-6097`, `:6140-6146`), que es el long pole de 020.
  **Queda fuera de 026** para no serializar el mundo por un evento de bajo
  impacto; se puede añadir después de 020/029 con medición.
- `furnace`/`hopper`: evitar strings y construir el evento solo si hay handler
  es una optimización local de 028, no de 026. No se toca en esta ficha.

### 026.5 WIT y plugins

- WIT v0.1 está congelado: no se cambia la firma de registro de eventos ni se
  añaden variantes. Los filtros de 026 son API del core/Host para handlers
  Rust/nativos.
- Un plugin WASM (Lazarus) no puede optar a `BlockChanged`/`PacketIdFilter`
  hasta que 030 exponga `HandlerOptions` en WIT v0.2. Mientras tanto, el
  beneficio para WASM es el “no handler ⇒ no evento” y el lazy construction;
  no se promete filtrado por bloque a Lazarus en 026.
- `register`, `fire` y `fire_blocking` conservan firma y semántica. No se sube
  `PLUGIN_API_VERSION`.

## Patrón

- [x] API aditiva
- [x] Early-return / lazy construction
- [x] Config por handler (opt-in)
- [ ] Homólogo
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/plugin/mod.rs` — `HandlerOptions`, `EventFilter`,
  `register_with_options`, consulta barata y comprobación en `fire`.
  **026 comparte `plugin/mod.rs` con 025; 025 va antes.**
- `crates/pumpkin/src/plugin/api/events/player/player_move.rs` — tipos de
  filtro de movimiento (o módulo nuevo `event_filter.rs`).
- `crates/pumpkin/src/net/java/play/player_position.rs` — consulta antes de
  construir `PlayerMoveEvent`.
- `crates/pumpkin/src/net/java/mod.rs` — consulta antes de construir
  `PacketReceivedEvent`. **024 y 026 comparten este archivo en zonas distintas;
  orden 024 → 026.**
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.
- **No se toca** `entity/player.rs`, `world/mod.rs` ni WIT v0.1.

## Compatibilidad

- APIs públicas: `register`, `fire` y `fire_blocking` no cambian. Se añade una
  variante de registro; los handlers existentes siguen recibiendo todo.
- Plugins v0.1: no se toca WIT; no hay recompilación obligatoria. Un plugin que
  no use filtros no nota diferencia.
- Plugins que dependan de `PlayerMoveEvent` sub-bloque: siguen recibiendo todo
  porque su handler no tiene filtro. Solo se filtran los handlers que lo piden.
- Cancelación: un handler filtrado que no recibe el evento no puede cancelarlo;
  es una decisión explícita del handler. Los handlers sin filtro conservan la
  semántica actual.
- Jugadores vanilla: con `hcf.enabled = false`, los filtros siguen siendo
  opt-in por handler; el core no cambia el comportamiento por defecto.
- Bedrock: la ruta de `PacketReceivedEvent` en `player.rs` no se toca; no hay
  regresión, pero tampoco el fast path lazy en 026.

## Métricas

- **Antes:** eventos despachados por tipo (025/002), paquetes recibidos/s,
  `PlayerMoveEvent`/s, tiempo de `PluginEvents` y `MovementBroadcast`, MSPT
  p99. La hipótesis del profiler de 15–30 % para inbound y 5–10 % para
  `Player::tick` **no está confirmada**; no se usa como ganancia.
- **Después:** menos construcciones/clones de eventos y menos
  `block_in_place` cuando no hay handler que matchee. **Ganancia: a medir.**
- **Método:** A/B 3 × 10 min, 100 bots; comparar handler sin filtro vs
  `BlockChanged` vs sin handler; medir p99 y CPU del plugin. Medir también
  paquetes/s y eventos/s para detectar que el filtro no se come eventos
  necesarios.
- **Criterio:** si el filtro no reduce el coste o aumenta bugs de movimiento,
  se mantiene solo el lazy construction por `has_handlers` y se posponen los
  filtros por bloque/ID.

## Tests

- [ ] Unitario: handler registrado con `register` recibe todos los
      `PlayerMoveEvent`, incluidos mismo bloque y solo rotación.
- [ ] Unitario: handler `BlockChanged` no recibe movimientos dentro del mismo
      bloque; sí al cambiar de bloque.
- [ ] Unitario: handler `PacketReceived` con IDs recibe solo esos IDs; el
      handler viejo recibe todos.
- [ ] Unitario: sin handlers, no se construye `PlayerMoveEvent` ni
      `PacketReceivedEvent` (contador de construcciones).
- [ ] Unitario: un handler filtrado no puede cancelar un evento que no recibe;
      uno no filtrado sí.
- [ ] Unitario: `register`, `fire` y `fire_blocking` conservan su firma y su
      orden actual.
- [ ] Integración: movimiento normal, teleport y rotación siguen funcionando;
      no hay desync por saltar el evento.
- [ ] Integración: un plugin de warmup/combat tag sin filtro sigue viendo
      micro-movimiento.
- [ ] Integración: bedrock sigue recibiendo `PacketReceivedEvent` como antes.
- [ ] Regresión: 025 (scheduler/guarda) sigue funcionando; los contadores de
      eventos no se duplican.
- [ ] Soak: 30 min, 100 bots; sin eventos perdidos que rompan timers.
- [ ] Loadtest: 100/200 bots; p99, eventos/s, CPU del plugin.
- [ ] Suite ejecutada y verde en T1.

## Rollback

No hace falta config: los filtros son opt-in por handler. Para volver atrás,
registrar el handler con `register` (sin filtro) o desactivar el plugin que usa
`register_with_options`; reinicio. No hay datos que migrar. Si el lazy
construction causa una regresión, se puede volver a construir el evento siempre
sin cambiar el contrato de los handlers.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivos con conflicto probable: `plugin/mod.rs`,
  `net/java/play/player_position.rs`, `net/java/mod.rs`.
- Orden: **025 → 026** (comparten `plugin/mod.rs`; 025 añade contadores y
  guarda). **024 → 026** para `net/java/mod.rs` (024 toca la cola de envío;
  026 toca la recepción). No en paralelo con el mismo dueño de archivo.
- **No se toca `world/mod.rs` ni `entity/player.rs`.** `BlockPhysicsEvent` y
  el lazy de Bedrock quedan para follow-ups con sus dueños.
- WIT v0.1 congelado: los filtros no se exponen a WASM hasta 030/WIT v0.2.
  No cambiar `PLUGIN_API_VERSION` ni la firma de registro v0.1.
- Si upstream cambia `fire`/`fire_blocking`, conservar el early-out y la
  consulta de filtros; no cambiar el orden blocking/no-blocking existente.

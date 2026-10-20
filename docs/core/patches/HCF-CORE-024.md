# HCF-CORE-024 — Batch de paquetes y serializers player-info/multi-block

- **Estado:** en diseño
- **Fase:** 2
- **Upstreamable:** parcial (codegen y serializers sí; batch/raw puede upstreamearse)
- **Riesgo:** medio/alto (codegen global y orden de paquetes)
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

El WIT v0.1 ya declara `c-multi-block-update` y `c-player-info-update`
(`crates/pumpkin-plugin-wit/v0.1/java-packets.wit:1096-1100`, `:1187-1194`;
enum en `:1763`, `:1774`), pero el generador no los serializa:

- `tools/pumpkin-codegen/src/wit/packet_mapping.rs:195-223` (`get_type_info`)
  solo devuelve el último segmento del tipo: `&[Player]` → `Player`,
  `Vec<(BlockPos, BlockStateId)>` → `Vec`; ignora genéricos, slices y tuplas.
- `convert_value` (`packet_mapping.rs:229-539`) no tiene ramas para `Player`,
  `Vec` ni tuplas y cae en `possible = false` (`:537`).
- `process_struct` (`packet_mapping.rs:638-735`) aborta el struct entero al
  primer campo no soportado (`:705`, `:719`).
- En el generado, `generated_packets.rs:1002` cae en `_ => None` y no hay
  `CMultiBlockUpdate`/`CPlayerInfoUpdate`; el WIT `send-packet`
  (`wasm_host/wit/v0_1/player.rs:3739-3762`) solo envía si
  `serialize_java_packet` devuelve `Some`, así que el paquete se pierde en
  silencio.
- El generador usa `unwrap()` al serializar
  (`generated_packets.rs:28` y siguientes); una versión no soportada puede
  hacer panic en la task del plugin.

Además, el coste de cardinalidad es alto:

- Glass envía un `CBlockUpdate` por bloque falso
  (`entity/player.rs:2446-2454`; plugin `src/glass/mod.rs:64`, `:218-229`).
- Tab nativo envía un `CPlayerInfoUpdate` por slot
  (`native-tab/src/lib.rs:315-395`; `SLOTS = 80`).
- El WIT `send-packet` usa `send_packet_now_data` y espera **una** confirmación
  por paquete (`net/java/mod.rs:640-685`).
- El writer agrupa hasta `MAX_BATCH_SIZE = 64` (`net/java/mod.rs:743`) y el
  frame máximo es `MAX_PACKET_SIZE = 2 MiB`
  (`net/java/mod.rs:147`, `pumpkin-protocol/src/lib.rs:36`).

## Cambio

### 024.1 Codegen raíz

1. Extender `get_type_info` para `Vec<T>`, `&[T]`, `Option<T>` y tuplas,
   preservando el tipo interno y la mutabilidad.
2. Extender `convert_value` con ramas para:
   - `Player`/`&Player` (UUID, acciones, propiedades, gamemode, latency,
     display name, etc. según `player_info_update.rs:44-137`);
   - `Vec<T>`/slices (map + `collect`);
   - tuplas `(BlockPos, BlockStateId)` y `Vector3<i32>`.
3. `process_struct` no debe abortar todo el fichero por un campo no soportado:
   debe emitir `None`/error para ese paquete y seguir generando el resto.
4. Regenerar con la herramienta; **no editar `generated_packets.rs` a mano**.
5. Verificar los brazos generados de `CMultiBlockUpdate` y
   `CPlayerInfoUpdate`, sus `ToWit`/downcast y el `_ => None`.

### 024.2 Serializer fallible

1. `serialize_java_packet` debe devolver `Result<Option<Bytes>, _>` (o
   `Option<Bytes>` con error logueado) en vez de `unwrap()`/panic.
2. El host WIT debe propagar el error o loguear y devolver error al guest; no
   puede perder el paquete en silencio (`player.rs:3757-3761`).
3. `CMultiBlockUpdate` y `CPlayerInfoUpdate` deben serializar con la versión
   del cliente, igual que el resto de paquetes.

### 024.3 Batch interno

1. Añadir `JavaClient::send_packets_now_data(Vec<Bytes>)`:
   - reservar el total en `pending_bytes` **antes** de encolar;
   - encolar todos como high-priority;
   - poner **una** completion en el último paquete y esperar una sola vez;
   - si la reserva supera `MAX_PENDING_BYTES` (`net/mod.rs:126`), hacer rollback
     y cerrar/errorear como hoy;
   - respetar `MAX_BATCH_SIZE = 64` y `MAX_FRAME_BATCH_DATA_SIZE`; un batch
     grande se parte en varios frames/flushes, no se promete un único flush.
2. Añadir `try_enqueue_packets(Vec<Bytes>)` para la cola normal: comprobar el
   total contra `MAX_PENDING_BYTES`, encolar de forma atómica y hacer rollback
   si no cabe. No cambiar `try_enqueue_packet`/`enqueue_packet`.
3. No tocar `send_packet`, `send_packet_now_data`, `send_chunks` ni
   `flush_block_updates` vanilla. El batch es una ruta nueva.

### 024.4 APIs aditivas

**`send_packet_batch`** (WIT `java-packets-v2`)

- Firma conceptual: `send-packet-batch(player, list<clientbound-packet>)`.
- Serializa todos los paquetes **antes** de encolar. Si uno falla, no se envía
  ninguno (todo-o-nada) y se devuelve error visible.
- Semántica de high-priority, igual que `send-packet`: conserva el orden de la
  lista y espera una confirmación por batch.
- No cambia `send-packet`; un plugin v0.1 sigue usándolo.

**`send_block_changes_raw`** (WIT `java-packets-v2`)

- Firma conceptual:
  `send-block-changes-raw(player, list<tuple<s32,s32,s32,u16>>)`.
- Agrupa por `chunk_section_from_pos` y emite **un `CMultiBlockUpdate` por
  sección** con `CMultiBlockUpdate::new_with_section`
  (`multi_block_update.rs:46-56`). No mezcla secciones en un mismo paquete.
- Usa la cola normal (no high-priority) para conservar la semántica de glass:
  los block changes no deben adelantar chunks/teleports.
- Lista vacía ⇒ no-op. Duplicados: se conserva el orden de entrada dentro de la
  sección; el cliente aplica el último estado. Documentarlo.
- Si el batch supera el presupuesto de bytes, dividir en varios paquetes **por
  sección**, nunca partiendo una sección entre dos `CMultiBlockUpdate`
  incompatibles.

**`send_player_info_batch`** (WIT `player-v2`)

- Firma conceptual:
  `send-player-info-batch(player, list<player-info-entry>)`.
- Agrupa entradas por **bitmask de acciones idéntico** y emite un
  `CPlayerInfoUpdate` por grupo (`player_info_update.rs:59-137`). No mezclar
  entradas con acciones distintas en un mismo bitmask.
- Preservar el orden de `PlayerAction` dentro de cada entrada.
- Cola normal para tab; no usar high-priority.
- Si una entrada es inválida, todo-o-nada antes de encolar.

### 024.5 Consumidores

- Glass: recolectar los bloques falsos bajo el lock del plugin y llamar a
  `send_block_changes_raw` **fuera** del `state().write()`; no enviar un
  `CBlockUpdate` por bloque.
- Tab nativo: agrupar por flags y emitir 1–2 `CPlayerInfoUpdate` por grupo en
  vez de 80.
- Estos consumidores viven en el repo del plugin; 024 define la API y el core,
  no cambia el plugin sin autorización separada.

### 024.6 WIT v0.2

1. Crear `crates/pumpkin-plugin-wit/v0.2` con `java-packets-v2` y `player-v2`
   para las tres llamadas. **No modificar WIT v0.1**: ni variantes del enum
   (`java-packets.wit:1763`, `:1774`), ni records, ni `send-packet`,
   ni `send-block-change`.
2. El record v0.1 de multi-block es lossy (`chunk-section: tuple<f64,f64,f64>`,
   `updates: list<tuple<tuple<s32,s32,s32>, string>>`); no se usa para raw. La
   v0.2 usa `u16` real para el estado y tipos no lossy.
3. 024 define las interfaces y las funciones host; **030** hace el host dual,
   linker, capabilities y el test de compatibilidad v0.1. Hasta que 030 esté,
   v0.2 no se anuncia como disponible.
4. Añadir una capacidad detectable (`packet.batch.v1`,
   `raw-block-changes.v1`, `player-info-batch.v1`) en el handshake de 030; 024
   puede dejar las constantes, pero no cambiar `PLUGIN_API_VERSION`.

### Riesgo específico: mezclar secciones

`CMultiBlockUpdate` empaqueta posiciones locales con
`pack_local_chunk_section` y máscaras de 12 bits
(`multi_block_update.rs:73-79`). Si un paquete contiene posiciones de dos
secciones, las coordenadas locales se interpretan mal y el cliente corrompe
bloques. Por eso `send_block_changes_raw` **debe** agrupar por sección y emitir
un paquete por sección; el test cross-section es obligatorio. El campo
`chunk_section` del record v0.1 no es fiable para esto por ser `f64`/lossy.

### Riesgo específico: player info

`CPlayerInfoUpdate` usa un bitmask de acciones y un orden de campos por acción
(`player_info_update.rs:83-137`). Si se agrupan entradas con bitmasks distintos,
el cliente lee campos que no existen o en orden incorrecto. Agrupar solo por
bitmask idéntico y preservar `PlayerAction`.

## Patrón

- [x] API aditiva
- [x] Build/codegen
- [x] Config (`packet_batch` para consumidores)
- [ ] Homólogo
- [ ] Early-return

## Archivos

- `tools/pumpkin-codegen/src/wit/packet_mapping.rs` — tipos, conversión y
  proceso de structs. **Dueño único del codegen en esta oleada.**
- `crates/pumpkin/src/plugin/loader/wasm/wasm_host/wit/v0_1/generated_packets.rs`
  — regenerado, no editado a mano.
- `crates/pumpkin/src/net/java/mod.rs` — `send_packets_now_data`,
  `try_enqueue_packets`; no cambiar `send_packet_now_data`.
- `crates/pumpkin/src/entity/player.rs` — `send_block_change`/helper de batch.
  **021 y 024 comparten `player.rs`: 021 va antes; no en paralelo.**
- `crates/pumpkin-plugin-wit/v0.2/*` — interfaces nuevas. No tocar `v0.1`.
- `crates/pumpkin/src/plugin/loader/wasm/wasm_host/**` — host de las llamadas
  v0.2 (la carga dual es 030).
- Consumidores: `native-tab`, plugin Lazarus (repo aparte, autorización
  separada).
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.

## Compatibilidad

- APIs públicas: `send_packet`, `send_block_change`, `send_packet_now_data` y
  `send_chunks` no cambian de firma ni de bytes.
- WIT v0.1: **congelado**. Un plugin v0.1 sigue compilando y cargando; no se
  añaden variantes al enum ni se cambian records.
- WIT v0.2: opt-in; un plugin que no la declare usa v0.1.
- Plugins v0.1: el fix de serializers hace que `CMultiBlockUpdate` y
  `CPlayerInfoUpdate` dejen de perderse; es una corrección, no un cambio de
  contrato. Aun así, el record v0.1 multi-block sigue siendo lossy y no se
  recomienda para raw.
- Jugadores vanilla: con `packet_batch = false`, los consumidores siguen por la
  ruta actual; el core no cambia el orden de paquetes existente.
- Orden: mezclar high-priority (`send_packet_batch`) y normal
  (`send_block_changes_raw`/`send_player_info_batch`) puede reordenar paquetes;
  documentar que un plugin no debe asumir orden entre ambas colas. Para
  paquetes críticos, usar `send_packet_now_data`.

## Métricas

- **Antes:** `PacketEncode`/`packet_encode` de 002, paquetes/s, bytes/s,
  confirmaciones por paquete, flushes del writer, tiempo de glass/tab.
- **Después:** menos serializaciones y confirmaciones; glass de O(bloques) a
  O(secciones), tab de 80 a 1–2 paquetes por grupo, WIT de N confirmaciones a
  1 por batch. **Ganancia: a medir**; el writer sigue haciendo
  `ceil(N/64)` flushes y el frame máximo de 2 MiB puede partir el batch.
- **Método:** A/B 3 × 10 min, 100 bots, con glass/tab reales; medir p99,
  paquetes/s, bytes/s y CPU del plugin. No medir solo TPS.
- **Criterio:** si el batch no reduce el coste de glass/tab o introduce
  reordenaciones, se mantiene solo el fix de serializers y se pospone el batch.

## Tests

- [ ] Unitario: bytes de `CMultiBlockUpdate` batch == suma de paquetes
      individuales de la misma sección.
- [ ] Unitario: bytes de `CPlayerInfoUpdate` batch == paquetes individuales con
      el mismo bitmask.
- [ ] Unitario: dos secciones distintas ⇒ dos `CMultiBlockUpdate`, cada uno con
      posiciones locales correctas; nunca uno mezclado.
- [ ] Unitario: lista vacía ⇒ no-op, sin paquete.
- [ ] Unitario: batch > 64 paquetes ⇒ se parte en frames, sin corrupción.
- [ ] Unitario: batch > 2 MiB ⇒ se parte por tamaño, sin panic.
- [ ] Unitario: `MAX_PENDING_BYTES` superado ⇒ rollback y error, sin envío
      parcial.
- [ ] Unitario: fallo de serialización de un paquete ⇒ no se envía ninguno.
- [ ] Unitario: player info con bitmasks distintos ⇒ paquetes separados y
      acciones en orden.
- [ ] Unitario: `serialize_java_packet` no hace panic con versión no soportada.
- [ ] Integración: glass se ve igual y no mezcla secciones.
- [ ] Integración: tab se ve igual con 1–2 paquetes por grupo.
- [ ] Compatibilidad: plugin v0.1 compila/carga en host v0.2 (con 030).
- [ ] Regresión: `send_packet`, `send_chunks` y `flush_block_updates` no cambian
      sus bytes.
- [ ] Loadtest: 100/200 bots; paquetes/s, p99, CPU del plugin.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.packet_batch = false` desactiva los consumidores que usan batch; las APIs
nuevas quedan sin usar. Si el problema es codegen/serializer, revertir el commit
de codegen y regenerar; no hay datos que migrar. WIT v0.2 solo se anuncia con
030; desactivar la capacidad deja a los plugins en v0.1.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivos con conflicto probable: `tools/pumpkin-codegen`,
  `generated_packets.rs` (regenerado), `net/java/mod.rs`,
  `entity/player.rs`, WIT v0.2.
- Orden: **024.1 codegen → 024.2 serializers → 024.3 batch → 024.4 APIs →
  024.5 consumidores → 030**. No empezar 030 hasta que 024.1–024.5 estén
  verdes.
- 021 y 024 comparten `entity/player.rs`: 021 va antes; no en paralelo.
- 024 y 026 comparten `net/java/mod.rs`, pero en zonas distintas (envío vs
  recepción de paquetes); se mergean en orden 024 → 026. No en paralelo si el
  mismo agente/rama toca el archivo.
- No tocar `send_chunks` ni `flush_block_updates` vanilla; el batch de chunks
  es otro parche.
- Los ficheros generados no se editan a mano: se regeneran con la herramienta.
  El diff del generado debe revisarse completo.
- WIT v0.1 está congelado: no añadir variantes ni cambiar records, aunque el
  codegen necesite tocar el generado.

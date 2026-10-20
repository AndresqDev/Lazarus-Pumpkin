# HCF-CORE-002 — Métricas por sección y p99

- **Estado:** implementado (C1 verde en espejo CI; T1 pendiente)
- **Fase:** 0
- **Upstreamable:** parcial (instrumentación genérica sí)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

El baseline solo tiene MSPT global y `/debug` (ticks/segundo). No se sabe qué
sección del tick consume los ~19 ms, ni si el coste está en chunks, jugadores,
entidades, tracker o paquetes. Sin métricas por sección, cualquier optimización
es a ciegas y el gate de 3 % no se puede evaluar.

## Cambio

1. Añadir `HcfMetrics` con contadores atómicos por sección, sin locks:
   - `count`, `total_nanos`, `max_nanos`, histograma de 128 buckets de 1 ms.
   - `record(section, nanos)` con `Ordering::Relaxed`.
   - `snapshot()` calcula media, máximo y p99 aproximado a 1 ms.
2. Añadir `HcfSection` con las fases medidas:
   - `TickTotal`, `DatapackTick`, `Scheduler`, `ScheduledFunctions`,
     `Worlds`, `PlayerData`.
   - `WorldFlush`, `ActiveChunks`, `Environment`, `Raids`, `Chunks`,
     `Players`, `Entities`, `Tracker`, `BlockEntities`, `DragonFight`.
   - `BlockTicks`, `FluidTicks`, `RandomTicks`, `NaturalSpawning`,
     `InhabitedTime`.
   - `PushEntities`, `MovementBroadcast`, `PluginEvents`.
3. Instrumentar con un guard RAII:
   ```rust
   let _section = metrics.section(HcfSection::Chunks);
   ```
   El guard mide en `Drop`; si HCF está apagado no hace `Instant::now()`.
4. Loguear cada 600 ticks (30 s) una línea estable por sección:
   ```text
   HCF_METRIC section=chunks count=600 avg_ms=7.42 p99_ms=12 max_ms=31
   ```
   El log se emite solo si `hcf.enabled`; no se añade comando nuevo en 002.
5. Exponer `HcfMetrics::snapshot()` para un futuro comando/CI. No se serializa
   a JSON todavía.

### Estado de implementación (2026-10-20)

- Implementado en `hcf/foundation` (`e8a3328`): 24 secciones, histograma de 128
  buckets, guard RAII y log `HCF_METRIC` cada 600 ticks.
- `HcfSnapshot.sections` usa `Vec<SectionSnapshot>` y el registro usa
  `Box<[SectionMetrics]>`: el array fijo de 24 × 128 atómicos superaba el límite
  de `clippy::large_stack_arrays`. La asignación ocurre una sola vez al arrancar;
  no hay asignación por tick.
- `PluginEvents` se mide en `PluginManager::fire`; el resto de secciones sigue la
  tabla de puntos de instrumentación.
- Los bloques instrumentados terminan en `};` para satisfacer
  `clippy::semicolon_outside_block` sin cambiar el alcance del guard.
- C1 se ejecutó en un espejo privado de GitHub Actions con Rust `1.96.0`,
  `RUST_MIN_STACK=64 MiB` y clippy solo sobre `pumpkin`; el árbol del core no
  incluye esos ajustes de CI. Run `39495116559`: formato, clippy debug/release y
  tests Linux x64/ARM64, Windows x64/ARM64 y macOS verdes. Android AArch64 falló
  porque el emulador no arrancó (`adb: device 'emulator-5554' not found`,
  `Timeout waiting for emulator to boot`), no por el código; los builds de
  release quedaron saltados por esa dependencia.
- `WasmJobs`, contadores por packet-id y export JSON quedan para `002b`.

### Diseño sin locks

```rust
struct SectionMetrics {
    count: AtomicU64,
    total_nanos: AtomicU64,
    max_nanos: AtomicU64,
    histogram: [AtomicU64; 128], // 0..127 ms; >=127 ms en el último
}
```

- `count`/`total_nanos`/`histogram` con `fetch_add(Relaxed)`.
- `max_nanos` con `fetch_max(Relaxed)`.
- El histograma se indexa con `nanos / 1_000_000`, saturado a 127.
- `p99` se calcula acumulando buckets hasta `ceil(count * 0.99)`; el valor es
  el límite superior del bucket (aprox. 1 ms). La media y el máximo son exactos.
- No hay `Mutex` ni `RwLock` en la ruta de tick.
- Si HCF está apagado, `section()` devuelve un guard sin `Instant` y `Drop` no
  escribe.

### Puntos de instrumentación

| Sección | Punto |
|---|---|
| `TickTotal` | `server/ticker.rs:26` a `:70` |
| `DatapackTick` | `server/mod.rs:1136-1141` |
| `Scheduler` | `server/mod.rs:1143` |
| `ScheduledFunctions` | `server/mod.rs:1144-1147` |
| `Worlds` | `server/mod.rs:1149-1157` |
| `PlayerData` | `server/mod.rs:1159` |
| `WorldFlush` | `world/mod.rs:1511-1512` |
| `ActiveChunks` | `world/mod.rs:1513` |
| `Environment` | `world/mod.rs:1514` |
| `Raids` | `world/mod.rs:1515-1533` |
| `Chunks` | `world/mod.rs:1536` |
| `Players` | `world/mod.rs:1559-1562` |
| `Entities` | `world/mod.rs:1565-1620` |
| `Tracker` | `world/mod.rs:1623` |
| `BlockEntities` | `world/mod.rs:1625-1637` |
| `DragonFight` | `world/mod.rs:1661` |
| `BlockTicks` | `world/mod.rs:1962-1982` |
| `FluidTicks` | `world/mod.rs:1984-2000` |
| `RandomTicks` | `world/mod.rs:2002-2042` |
| `NaturalSpawning` | `world/mod.rs:2044-2087` |
| `InhabitedTime` | `world/mod.rs:2089-2100` |
| `PushEntities` | `entity/living.rs:1417-1459` |
| `MovementBroadcast` | `net/java/play/player_position.rs:127-151`, `:285-318` |
| `PluginEvents` | `plugin/mod.rs` (`PluginManager::fire`) |

No se instrumenta cada jugador individual en 002; `Players` mide el bloque
paralelo completo. Si hace falta, 021 añade un contador por jugador.

## Patrón

- [x] Instrumentación aditiva
- [ ] Homólogo
- [ ] Early-return
- [ ] Solo configuración
- [x] Build/CI (parseo de logs)

## Archivos

- `crates/pumpkin/src/server/hcf_metrics.rs` — `HcfMetrics`, `HcfSection`,
  histograma, snapshot.
- `crates/pumpkin/src/server/mod.rs` — campo `hcf_metrics`, log periódico.
- `crates/pumpkin/src/server/ticker.rs` — `TickTotal` y llamada al log.
- `crates/pumpkin/src/world/mod.rs` — secciones del mundo/chunks.
- `crates/pumpkin/src/entity/living.rs` — `PushEntities`.
- `crates/pumpkin/src/net/java/play/player_position.rs` — `MovementBroadcast`.
- `.github/workflows/loadtest-server.yml` — parsear `HCF_METRIC` si existe.
- `docs/core/patches/HCF-CORE-002.md`, `PATCHES.md`.

## Compatibilidad

- APIs públicas: no cambian; solo se añade un campo y logs.
- Plugins v0.1: no se toca WIT.
- Jugadores vanilla: con `enabled = false` no hay `Instant` ni escrituras.
- Migración: no.
- Overhead esperado: < 0.2 % con HCF on; 0 con HCF off.

## Métricas

- **Antes:** MSPT B21 ~19 ms, sin desglose.
- **Después:** media/p99 por sección; el log permite ver si `Chunks`,
  `Players`, `Tracker` o `MovementBroadcast` dominan.
- **Método:** A/B 3 × 10 min, 100 bots; perfil off/on; misma seed y view
  distance. No se cambia comportamiento en este parche.

## Tests

- [ ] Unitario: histograma vacío ⇒ p99 0.
- [ ] Unitario: 100 muestras conocidas ⇒ media/máx/p99 esperados.
- [ ] Unitario: `>=127 ms` cae en el último bucket.
- [ ] Unitario: `enabled = false` ⇒ `count` no incrementa.
- [ ] Unitario: `snapshot()` con `count = 0` no divide por cero.
- [ ] Integración: `HCF_METRIC` aparece cada 600 ticks con HCF on y no con off.
- [ ] Compatibilidad: logs vanilla no cambian de formato.
- [ ] Soak: 30 min, sin crecimiento de memoria (histograma fijo).
- [ ] Loadtest: B21 100 bots; comparar suma de secciones vs `TickTotal`.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.enabled = false` elimina la instrumentación. Si se quiere medir sin HCF,
se puede añadir `hcf.metrics = true` en una iteración posterior; en 002 las
métricas dependen de `enabled`.

## Notas de rebase

- Commit base: `204a94e`.
- Archivos con conflicto probable: `server/mod.rs`, `server/ticker.rs`,
  `world/mod.rs`, `entity/living.rs`, `player_position.rs`.
- Orden: después de 001/005; antes de 011–029 para que el A/B tenga secciones.
- `server/mod.rs` se congela tras 001/002/005; los consumidores no lo editan.

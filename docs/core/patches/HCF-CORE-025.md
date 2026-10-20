# HCF-CORE-025 — Scheduler con guarda y presupuesto

- **Estado:** implementado (C1 pendiente)
- **Fase:** 2
- **Upstreamable:** parcial (guarda/métricas genéricas sí)
- **Riesgo:** bajo/medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`TaskScheduler::tick` lanza cada tarea vencida con `spawn_task` y, si es
periódica, la reprograma en `current_tick + period` **antes** de que el guest
termine (`server/scheduler.rs:200-242`). Una tarea de 50–500 ms se solapa
consigo misma y acumula ejecuciones; el heap y los locks se drenan sin
presupuesto. Además, `tick` mantiene `tasks` y `cancelled` bloqueados durante
todo el drenaje (`:160-183`) y no hay métricas para atribuir el coste.

## Cambio

1. Añadir `running: Arc<AtomicBool>` a `ScheduledTask`.
   - Antes de lanzar una tarea periódica, `compare_exchange(false, true)`.
   - Si ya está `true`, no lanzar; incrementar `skipped`/`overrun` y reprogramar
     el siguiente tick.
   - Un guard RAII dentro del futuro limpia `running` también en error/panic.
   - Las tareas one-shot (`period = None`) no usan el flag.
2. Añadir `TaskSchedulerMetrics` con `AtomicU64`:
   - `ticks`, `due`, `launched`, `skipped`, `max_task_nanos`, `total_task_nanos`.
   - `tick_duration_nanos` del drenaje.
   - Se exponen por `HcfMetrics` o por un log `HCF_METRIC` si 002 ya está.
3. Reducir retención de locks:
   - Soltar `cancelled` antes de procesar `tasks_to_run`.
   - Cargar `disabled_plugins` una vez por tick o reutilizar el guard.
   - No cambiar la semántica del heap ni el orden de tareas del mismo tick.
4. Presupuesto opcional `scheduler_budget_ms`:
   - Medir el drenaje; si supera el presupuesto, dejar el resto para el siguiente
     tick (no ejecutar) y loguear `overrun`.
   - Default 5 ms en HCF; `0` = sin límite.
   - No aplicar a `ScheduledFunctionQueue` (datapack) en 025.
5. Instrumentación de `fire_blocking`:
   - Contador de eventos despachados, handlers ejecutados y duración.
   - No cambiar `register`, `fire` ni `fire_blocking`; solo contadores.
6. `campfire.rs:106-129`: liberar el mutex del slot antes de `fire_blocking` y
   revalidar después. Es un bug de contención, no solo rendimiento.

### Guarda RAII

```rust
struct RunningGuard(Arc<AtomicBool>);
impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
```

El guard se crea dentro del futuro, después de que el `compare_exchange` haya
ganado; si el futuro no llega a ejecutarse, el flag no se marca. Si el guest
hace panic, `Drop` lo limpia.

## Patrón

- [x] API aditiva interna
- [x] Config (`scheduler_budget_ms`)
- [ ] Homólogo
- [ ] Early-return
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/server/scheduler.rs` — flag, guard, métricas, locks.
- `crates/pumpkin/src/server/hcf_metrics.rs` — contadores si 002.
- `crates/pumpkin/src/plugin/mod.rs` — contadores de dispatch (sin cambiar API).
- `crates/pumpkin/src/block/entities/campfire.rs` — lock fuera del guest.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md`.

## Compatibilidad

- APIs de plugins: no cambian; `schedule_delayed_task`/`schedule_repeating_task`
  conservan firma.
- Comportamiento: una tarea periódica que antes se solapaba deja de hacerlo.
  Es un cambio observable para plugins que dependían del solapamiento; debe ser
  opt-in por perfil (`scheduler_guard = true`), default HCF on.
- Jugadores vanilla: `enabled = false` ⇒ scheduler actual.
- Migración: no.

## Métricas

- **Antes:** p99 de MSPT y tareas vencidas sin atribución.
- **Después:** `skipped`, `overrun`, `max_task_ms`, `duration_ms` del drenaje.
- **Método:** A/B 100 bots; forzar una tarea de 100 ms con período 1 tick y
  verificar que no se solapa.

## Tests

- [ ] Unitario: tarea periódica que duerme más que el período no se solapa.
- [ ] Unitario: el guard limpia `running` en error/panic.
- [ ] Unitario: one-shot no se ve afectada.
- [ ] Unitario: `scheduler_budget_ms = 0` no limita.
- [ ] Unitario: `disabled_plugins` se respeta y no se lockea por tarea.
- [ ] Integración: tareas de Lazarus (timers, glass) siguen ejecutándose.
- [ ] Compatibilidad: `register`/`fire`/`fire_blocking` sin cambios.
- [ ] Soak: 30 min, sin tareas huérfanas ni `running` trabado.
- [ ] Loadtest: 100/200 bots; p99 y overruns.
- [ ] Suite ejecutada y verde en T1.

## Estado de implementación (2026-10-20)

- Implementado en `hcf/core-025`, commit `2e553ee`, mergeado en
  `hcf/foundation`.
- `ScheduledTask` tiene `running: Arc<AtomicBool>`; las tareas periódicas hacen
  CAS antes de lanzarse y un `RunningGuard` limpia el flag al terminar, con
  error o panic. Al saltar una ejecución se incrementa `skipped` y se
  reprograma el siguiente periodo.
- `scheduler_guard` es una clave nueva de `[hcf]` (default `true` en
  `hcf-1.8`); `scheduler_guard()` devuelve false con HCF off.
- `scheduler_budget_ms` limita el bucle de lanzamiento (default 5 ms; `0` = sin
  límite). Las tareas no procesadas se reencolan y se reintentan en el
  siguiente tick. El drenaje inicial y el procesamiento ya no mantienen el lock
  de `cancelled`; `disabled_plugins` se carga una vez por tick.
- Métricas `HCF_METRIC scheduler` cada 600 ticks: due, launched, skipped,
  overrun, duración del tick, máximo y total de tiempo de guest.
- `campfire.rs` ya no mantiene el mutex del slot durante
  `CampfireStartEvent`; revalida antes de escribir el total.
- Los contadores de `fire_blocking` se difieren a 026/002b para no compartir
  `plugin/mod.rs`. No se tocó `ScheduledFunctionQueue`.
- C1/T1 pendientes; no se ejecutó compilación local.

## Rollback

`hcf.scheduler_guard = false` o `hcf.enabled = false`. No hay datos que migrar.
Si una tarea crítica se salta, desactivar la guarda solo para ese plugin
(whitelist) o volver al binario anterior.

## Notas de rebase

- Commit base: `204a94e`.
- Archivos con conflicto probable: `server/scheduler.rs`, `plugin/mod.rs`,
  `block/entities/campfire.rs`.
- Orden: después de 002; antes de 026. No tocar `ScheduledFunctionQueue` en 025.
- `PLUGIN_API_VERSION` no sube; Lazarus es WASM y el chequeo de versión solo
  aplica al loader nativo (`loader/native.rs:35`).

# Registro de parches del core HCF

Estados: `Propuesto` → `En diseño` → `Implementado` → `Upstream` / `Descartado`.

La ficha detallada de cada parche implementado vive en
`docs/core/patches/HCF-CORE-###.md`.

La asignación por oleadas, agentes y archivos permitidos está en
[`AGENT_PLAYBOOK.md`](AGENT_PLAYBOOK.md). `HCF-CORE-060` y `HCF-CORE-061` quedan
**diferidos**: primero se optimiza el MSPT del backend.

> **Nota de contexto (2026-10-22):** C1 está verde en `4bb3c75` (run
> `39699544519`). Las filas que dicen “C1 pendiente” conservan el estado
> histórico del merge; no son el estado actual. El A/B vigente y sus límites
> están en `pumpkin-hcf/wave2/B1.md`; la auditoría de tablist/scoreboard en
> `pumpkin-hcf/wave0/DOC_AUDIT.md`.

## Fase 0 — Fundamentos

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-001 | Perfil HCF, build ID y capacidades | Config + API aditiva | `pumpkin-config`, `server/mod.rs`, `main.rs`, `pumpkin.toml` | Habilita todo lo demás | Bajo | Implementado |
| HCF-CORE-002 | Métricas por sección y p99 | Instrumentación | `server/hcf_metrics.rs`, `ticker.rs`, `world/mod.rs`, `entity/living.rs` | Diagnóstico | Bajo | Implementado (C1 verde; T1 pendiente) |
| HCF-CORE-003 | Estándar de parches y documentación | Documentación | `docs/core/*` | Mantenibilidad | Bajo | Propuesto |
| HCF-CORE-004 | Build PGO/CPU/allocator | CI/build | `Cargo.toml`, `.github/workflows`, `docs/core/BUILD.md` | 10–25 % CPU | Bajo | Propuesto |
| HCF-CORE-005 | Helpers de perfil y feature flags | Config | `server/hcf.rs`, `pumpkin-config/src/hcf.rs` | Base de dispatch | Bajo | Implementado |

## Fase 1 — Eliminar trabajo no usado

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-010 | Solo dimensiones necesarias | Config + early-return | `server/mod.rs` | Alta si no hay Nether/End | Bajo | Implementado (C1 en curso) |
| HCF-CORE-011 | Clima desactivable | Early-return | `world/mod.rs` (`tick_environment`) | Baja/media | Bajo | Implementado (C1 pendiente) |
| HCF-CORE-012 | Raids desactivables | Early-return | `world/mod.rs` (`tick`) | Media | Bajo | Implementado (C1 pendiente) |
| HCF-CORE-013 | Spawning natural fuera | Early-return | `world/mod.rs` (`tick_chunks`) | Alta (a medir) | Bajo | Implementado (C1 pendiente) |
| HCF-CORE-014 | Random ticks fuera + inhabited time 1 Hz | Early-return + time-slice | `world/mod.rs`, `pumpkin-world/src/level.rs` | Media/alta (a medir) | Medio | Implementado (C1 pendiente) |
| HCF-CORE-015 | Política de IA de mobs | Clasificación + whitelist | `entity/mod.rs` | Alta | Medio | Propuesto |
| HCF-CORE-016 | Advancements/stats/recipe book/chat signing | Early-return + config | `entity/player.rs`, `server/mod.rs`, config | Media | Bajo/medio | Propuesto |
| HCF-CORE-017 | View/sim distance, autosave y logging | Config + async save | `pumpkin.toml`, `world/mod.rs`, `pumpkin-world` | Alta | Bajo | Propuesto |

## Fase 2 — Homólogos y hot paths

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-020 | Composición de `tick_chunks_hcf` | Homólogo por composición | `world/mod.rs` | Alta (a medir) | Medio | Implementado (C1 pendiente) |
| HCF-CORE-021 | `Player::tick` HCF | Homólogo + early-returns | `entity/player.rs` | Media/alta (a medir) | Medio | Implementado 021a (C1 pendiente) |
| HCF-CORE-022 | Entity tracker HCF | Homólogo + caché/índice | `world/entity_tracker.rs` | Media/alta (a medir) | Medio | Implementado 022a (C1 pendiente) |
| HCF-CORE-023 | Colisiones restantes con spatial hash (después de 029) | Homólogo | `world/mod.rs` | Media/baja | Medio | Propuesto |
| HCF-CORE-024 | Batch de paquetes + serializer player info/multi-block | API aditiva | `plugin/.../generated_packets.rs`, `net/java/mod.rs`, WIT v0.2 | Alta en plugin (a medir) | Medio/alto | En diseño |
| HCF-CORE-025 | Scheduler con guarda y presupuesto | API aditiva interna | `server/scheduler.rs` | p99 / estabilidad | Bajo | Implementado (C1 pendiente) |
| HCF-CORE-026 | Filtros de eventos y movimiento | API aditiva | `plugin/api/events`, `plugin/mod.rs`, `net/java` | Media (a medir) | Medio | Implementado (C1 pendiente) |
| HCF-CORE-027 | Política de tick por tipo de entidad | Config + early-return | `entity/mod.rs`, `world/mod.rs` | Media | Medio | Propuesto |
| HCF-CORE-028 | Buffers reutilizados y menos allocs por tick | Interno | `world/mod.rs`, `entity_tracker.rs` | Media | Medio | Propuesto |
| HCF-CORE-029 | Colisiones jugador-jugador y jugador-mob off | Early-return + config | `world/mod.rs`, `entity/living.rs` | Alta con 100+ jugadores | Medio | Implementado (C1 en curso) |

`HCF-CORE-029` se implementa antes que `HCF-CORE-023`. Si desactivar colisiones
de jugador elimina la mayoría de pares, 023 se reduce o se descarta.

`HCF-CORE-013` → `014` → `020` se implementan en serie: los tres comparten
`world/mod.rs` y 020 es el dueño único de `tick_chunks_hcf`. `world/mod.rs`
sigue siendo el long pole: 020 no puede ir en paralelo con 029, 023, 011, 012,
017b, 027 ni 028a; si 029 ya está en la rama, 020 se rebasa encima. `021`,
`022`, `024` y `026` no tocan `world/mod.rs`; 025 va antes que 026 y 024 antes
que 030.

## Fase 3 — APIs y compatibilidad

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-030 | WIT v0.2 con batch/raw/capabilities | API aditiva versionada | `pumpkin-plugin-wit/v0.2`, wasm host | Alta en plugin | Medio | Propuesto |
| HCF-CORE-031 | ABI C para plugins nativos | API aditiva | `pumpkin-plugin-api`, loader nativo | Compatibilidad | Alto | Propuesto |
| HCF-CORE-032 | Test de compatibilidad v0.1 en host v0.2 | Tests | CI de plugins | Confianza | Bajo | Propuesto |

## Fase 4 — Actualización y estructura

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-040 | `core-update` host API | API aditiva + permisos | `server/updater/*`, WIT v0.2 | Comodidad | Alto | Propuesto |
| HCF-CORE-041 | Manifiesto firmado y tooling | Tooling | `tools/core-manifest`, CI | Seguridad | Medio | Propuesto |
| HCF-CORE-042 | Launcher/restart helper + rollback | Proceso externo | `tools/pumpkin-hcf`, docs | Disponibilidad | Alto | Propuesto |
| HCF-CORE-050 | Executor de plugins en un solo hilo | Estructural | `server/scheduler.rs`, plugin runtime | p99/locks | Alto | Propuesto |
| HCF-CORE-051 | Tick regionizado | Estructural | `world/*` | 300+ | Muy alto | Propuesto |
| HCF-CORE-060 | Gateway 1.8 nativo | Proyecto separado | nuevo crate/servicio | Stack total | Muy alto | Propuesto |
| HCF-CORE-061 | Data plane nativo del plugin | `.so`/IPC | `native-*` | Media | Alto | Propuesto |

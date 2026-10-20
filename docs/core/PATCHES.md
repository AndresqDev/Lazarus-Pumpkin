# Registro de parches del core HCF

Estados: `Propuesto` → `En diseño` → `Implementado` → `Upstream` / `Descartado`.

La ficha detallada de cada parche implementado vive en
`docs/core/patches/HCF-CORE-###.md`.

La asignación por oleadas, agentes y archivos permitidos está en
[`AGENT_PLAYBOOK.md`](AGENT_PLAYBOOK.md). `HCF-CORE-060` y `HCF-CORE-061` quedan
**diferidos**: primero se optimiza el MSPT del backend.

## Fase 0 — Fundamentos

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-001 | Perfil HCF, build ID y capacidades | Config + API aditiva | `pumpkin-config`, `server/mod.rs`, `main.rs`, `pumpkin.toml` | Habilita todo lo demás | Bajo | En diseño |
| HCF-CORE-002 | Métricas por sección y p99 | Instrumentación | `server/hcf_metrics.rs`, `ticker.rs`, `world/mod.rs`, `entity/living.rs` | Diagnóstico | Bajo | En diseño |
| HCF-CORE-003 | Estándar de parches y documentación | Documentación | `docs/core/*` | Mantenibilidad | Bajo | Propuesto |
| HCF-CORE-004 | Build PGO/CPU/allocator | CI/build | `Cargo.toml`, `.github/workflows`, `docs/core/BUILD.md` | 10–25 % CPU | Bajo | Propuesto |
| HCF-CORE-005 | Helpers de perfil y feature flags | Config | `server/hcf.rs`, `pumpkin-config/src/hcf.rs` | Base de dispatch | Bajo | En diseño |

## Fase 1 — Eliminar trabajo no usado

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-010 | Solo dimensiones necesarias | Config + early-return | `server/mod.rs` | Alta si no hay Nether/End | Bajo | Propuesto |
| HCF-CORE-011 | Clima desactivable | Early-return | `world/mod.rs` (`tick_environment`) | Baja/media | Bajo | Propuesto |
| HCF-CORE-012 | Raids desactivables | Early-return | `world/mod.rs` (`tick`) | Media | Bajo | Propuesto |
| HCF-CORE-013 | Spawning natural fuera | Early-return | `world/mod.rs` (`tick_chunks`) | Alta | Bajo | Propuesto |
| HCF-CORE-014 | Random ticks fuera + inhabited time 1 Hz | Early-return + time-slice | `world/mod.rs`, `pumpkin-world/src/level.rs` | Media/alta | Medio | Propuesto |
| HCF-CORE-015 | Política de IA de mobs | Clasificación + whitelist | `entity/mod.rs` | Alta | Medio | Propuesto |
| HCF-CORE-016 | Advancements/stats/recipe book/chat signing | Early-return + config | `entity/player.rs`, `server/mod.rs`, config | Media | Bajo/medio | Propuesto |
| HCF-CORE-017 | View/sim distance, autosave y logging | Config + async save | `pumpkin.toml`, `world/mod.rs`, `pumpkin-world` | Alta | Bajo | Propuesto |

## Fase 2 — Homólogos y hot paths

| ID | Título | Patrón | Archivos principales | Ganancia esperada | Riesgo | Estado |
|---|---|---|---|---|---|---|
| HCF-CORE-020 | Composición de `tick_chunks_hcf` | Homólogo por composición | `world/mod.rs` | Alta | Medio | Propuesto |
| HCF-CORE-021 | `Player::tick` HCF | Homólogo + early-returns | `entity/player.rs` | Media/alta | Medio | Propuesto |
| HCF-CORE-022 | Entity tracker HCF | Homólogo + caché/índice | `world/entity_tracker.rs` | Media/alta | Medio | Propuesto |
| HCF-CORE-023 | Colisiones restantes con spatial hash (después de 029) | Homólogo | `world/mod.rs` | Media/baja | Medio | Propuesto |
| HCF-CORE-024 | Batch de paquetes + serializer player info/multi-block | API aditiva | `plugin/.../generated_packets.rs`, `net/java/mod.rs`, WIT v0.2 | Alta en plugin | Medio | Propuesto |
| HCF-CORE-025 | Scheduler con guarda y presupuesto | API aditiva interna | `server/scheduler.rs` | p99 / estabilidad | Bajo | Propuesto |
| HCF-CORE-026 | Filtros de eventos y movimiento | API aditiva | `plugin/api/events`, `plugin/mod.rs` | Media | Medio | Propuesto |
| HCF-CORE-027 | Política de tick por tipo de entidad | Config + early-return | `entity/mod.rs`, `world/mod.rs` | Media | Medio | Propuesto |
| HCF-CORE-028 | Buffers reutilizados y menos allocs por tick | Interno | `world/mod.rs`, `entity_tracker.rs` | Media | Medio | Propuesto |
| HCF-CORE-029 | Colisiones jugador-jugador y jugador-mob off | Early-return + config | `world/mod.rs`, `entity/mod.rs` | Alta con 100+ jugadores | Medio | En diseño |

`HCF-CORE-029` se implementa antes que `HCF-CORE-023`. Si desactivar colisiones
de jugador elimina la mayoría de pares, 023 se reduce o se descarta.

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

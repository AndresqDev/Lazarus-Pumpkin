# Playbook de ejecución con agentes

## 0. Objetivo

Reducir el tiempo de pared de las implementaciones independientes usando varios
agentes en paralelo, sin sacrificar la calidad ni la validez de las mediciones.
El objetivo técnico es **bajar MSPT y CPU del backend**; el gateway 1.8 queda
diferido.

La regla es simple:

> **Paralelizar código y análisis. Serializar benchmarks, merges y decisiones.**

Los agentes pueden comprimir un día de implementación a **3–5 horas** de reloj
en una oleada bien separada. No pueden comprimir el tiempo de compilación,
tests, carga y validación por debajo de su mínimo real. El objetivo realista es
**2–3× de aceleración**, no 10×.

## 1. Modelo de trabajo

```text
Orquestador (agente principal)
  ├── Oleada 0: scouts + profiler + baseline
  ├── Oleada 1: contratos/fundamentos (1 implementador)
  ├── Oleada 2: implementadores en paralelo (máx. 3–4)
  ├── Oleada 3: integración + revisión
  ├── Oleada 4: benchmark exclusivo
  └── Oleada 5: documentación + siguiente wave
```

Principios:

1. **Contratos primero.** Antes de paralelizar, el orquestador mergea las
   interfaces (`HcfProfile`, `HcfMetrics`, firmas de helpers, API de paquetes).
   Sin contratos, los agentes se pisan.
2. **Un dueño por archivo.** Cada agente tiene una lista de archivos permitidos
   y prohibidos. Si necesita tocar un archivo compartido, pide permiso.
3. **Worktrees aislados.** Cada agente trabaja en su propio `git worktree` y su
   propio `CARGO_TARGET_DIR`.
4. **Benchmark exclusivo.** Solo un agente corre loadtests a la vez. Mientras
   tanto, no se compilan builds grandes en la misma máquina.
5. **Timebox.** Cada agente tiene un presupuesto de tiempo. Si se bloquea,
   devuelve hallazgos y el orquestador decide; no entra en bucle.
6. **Nada se acepta sin medir.** Un parche que “debería” mejorar no se queda si
   el A/B no lo demuestra.

## 2. Roles

| Rol | Subagente | Escribe código | Salida | Timebox |
|---|---|---|---|---|
| Orquestador | principal | Solo integración | Plan, merges, decisiones | Toda la sesión |
| Scout | `explore` | No | Mapa de hot paths, líneas, riesgos | 30–45 min |
| Profiler | `general` | No | `perf`, flamegraph, top symbols | 45–60 min |
| Implementador | `general` | Sí, en su worktree | Branch + commit + tests | 60–90 min |
| Verifier | `general` | Tests en su worktree | Tests, vanilla parity | 30–45 min |
| Reviewer | `general` | No | Hallazgos, blockers | 20–30 min |
| Benchmark runner | `general` | No | CSV, p50/p95/p99, veredicto | 90–120 min |
| Docs scribe | `general`/`explore` | Solo docs | Fichas y tracker | 30 min |
| Integrador | orquestador | Sí | Merge queue, CI, conflictos | 45–90 min |

Un agente de implementación **no** hace de reviewer de su propio código. El
reviewer es otro agente o el orquestador.

### 2.1 Cómo se lanzan

- **Scouts**: `subagent` con `agent=explore`, `background=true`, prompt
  read-only. Se pueden lanzar 3–5 a la vez.
- **Implementadores**: `subagent` con `agent=general`, `background=true`, uno
  por worktree y por conjunto de archivos. Máximo 3–4 simultáneos.
- **Profiler/benchmark**: `agent=general`, pero **no** en background durante un
  benchmark; el benchmark es exclusivo.
- **Fix**: continuar la misma sesión con `sessionID` y un prompt corto. No
  crear un agente nuevo para un ajuste del mismo parche.
- **Modelo**: no fijar `model` salvo que el usuario lo pida; usar el modelo por
  defecto del harness.
- El orquestador espera las notificaciones de los agentes en background; no
  hace polling ni duerme.

## 3. Workspace y worktrees

Repos:

- Core: `/tmp/pumpkin-native` (clon de Pumpkin).
- Plugin: `/home/andresq/IdeaProjects/Lazarus-Pumpkin`.
- Worktrees de trabajo: `/tmp/pumpkin-hcf/<patch-id>`.
- Artefactos de perf: `/tmp/pumpkin-hcf/perf/<run-id>`.
- Lock de benchmark: `/tmp/pumpkin-hcf/bench.lock`.

Ejemplo:

```bash
cd /tmp/pumpkin-native
git fetch --all --tags
git worktree add /tmp/pumpkin-hcf/core-001 -b hcf/core-001 204a94e
git worktree add /tmp/pumpkin-hcf/core-020 -b hcf/core-020 204a94e
```

Build aislado:

```bash
export CARGO_TARGET_DIR=/tmp/pumpkin-hcf/target/core-020
export CARGO_BUILD_JOBS=2
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=/tmp/pumpkin-hcf/sccache
```

Reglas:

- No compartir `target/` entre agentes: Cargo se bloquea y se corrompe la
  medición.
- `sccache` sí se comparte; reduce compilaciones repetidas.
- Limpiar worktrees viejos: `git worktree remove --force <path>`.
- No hacer `git push` desde un agente. El orquestador decide.

## 4. Contratos antes de paralelizar

La Oleada 1 debe dejar mergeados estos contratos, aunque sea con
implementaciones vacías o mínimas:

### 4.1 Perfil

```rust
pub struct HcfProfile { /* ... */ }

impl HcfProfile {
    #[inline] pub fn disabled(&self, feature: HcfFeature) -> bool;
    #[inline] pub fn fast_chunks(&self) -> bool;
    #[inline] pub fn fast_players(&self) -> bool;
    #[inline] pub fn fast_tracker(&self) -> bool;
    #[inline] pub fn packet_batch(&self) -> bool;
}
```

### 4.2 Métricas

```rust
pub struct HcfMetrics { /* ... */ }

impl HcfMetrics {
    pub fn record_tick(&self, section: TickSection, duration: Duration);
    pub fn snapshot(&self) -> MetricsSnapshot;
}
```

Las llamadas de instrumentación se hacen en la Oleada 0/1. Los agentes de la
Oleada 2 **no** añaden instrumentación nueva en archivos calientes; usan la que
ya existe.

### 4.3 Helpers del mundo

Firmas acordadas antes de que el agente World empiece:

```rust
fn tick_scheduled_block_ticks(&self, server: &Arc<Server>);
fn tick_fluid_ticks(&self, server: &Arc<Server>);
fn tick_random_ticks(&self, server: &Arc<Server>);
fn tick_natural_spawning(&self, server: &Arc<Server>);
fn tick_inhabited_time(&self, server: &Arc<Server>);
fn tick_inhabited_time_hcf(&self, server: &Arc<Server>);
```

### 4.4 API de paquetes

Nombres y tipos acordados:

```text
send_packet_batch(list<clientbound-packet>)
send_block_changes_raw(list<tuple<s32,s32,s32,u16>>)
send_player_info_batch(player-info-update)
```

### 4.5 Updater

`CoreUpdateManifest`, `UpdateTicket`, `SERVER_EXIT_CODE = 75`. Ver
[UPDATER.md](UPDATER.md).

Si un agente necesita cambiar un contrato, debe pedirlo al orquestador. No se
cambia unilateralmente.

## 5. Propiedad de archivos

| Área | Archivos | Dueño | Notas |
|---|---|---|---|
| Perfil/config | `pumpkin-config/src/hcf.rs`, `lib.rs`, `pumpkin.toml` | Foundation | Oleada 1 |
| Métricas | `server/metrics.rs`, `ticker.rs` | Foundation | Oleada 1; instrumentación antes de congelar |
| Servidor/dimensiones | `server/mod.rs`, `main.rs` | Foundation | Incluye 010 y 016b; después congelado |
| Build/PGO | `Cargo.toml`, `.github/workflows` | Foundation | Oleada 1 |
| Mundo | `world/mod.rs` | World | Un solo dueño; long pole |
| Nivel/autosave | `pumpkin-world/src/level.rs` | World | Incluye la parte de 017b |
| Jugador | `entity/player.rs` | Player | Incluye 016a; no tocar `world/mod.rs` |
| Tracker | `world/entity_tracker.rs` | Tracker | Incluye 028b; no tocar `player.rs` |
| Entidades/política | `entity/mod.rs` | World | Incluye 015 y 027; no tocar `player.rs` |
| Paquetes/WIT | `plugin/.../generated_packets.rs`, `net/java/mod.rs`, `pumpkin-plugin-wit/v0.2/*` | Packet | v0.1 congelado |
| Scheduler/eventos | `server/scheduler.rs`, `plugin/mod.rs`, `plugin/api/events/*` | Scheduler | Coordinar con Packet si toca `plugin/mod.rs` |
| Updater | `server/updater/*`, `tools/*`, WIT v0.2 | Updater | Oleada posterior |
| Docs | `docs/core/*` | Docs | No toca código |

Archivos compartidos que requieren serialización:

- `world/mod.rs`
- `server/mod.rs`
- `entity/mod.rs`
- `Cargo.toml` / `Cargo.lock`
- `plugin/mod.rs`
- `pumpkin-plugin-wit/v0.1/*` (congelado: no se toca)

### 5.1 Partición de parches que tocan varios archivos

Algunos IDs del plan original tocan más de un dueño. Se reparten así para no
crear conflictos:

| ID | Parte | Dueño | Archivos |
|---|---|---|---|
| `HCF-CORE-010` | completa | Foundation | `server/mod.rs` |
| `HCF-CORE-016` | `016a` stats/advancements | Player | `entity/player.rs` |
| `HCF-CORE-016` | `016b` recipe book/chat signing | Foundation | `server/mod.rs`, config |
| `HCF-CORE-017` | `017a` config/autosave/logging | Foundation | `pumpkin.toml`, `server/mod.rs` |
| `HCF-CORE-017` | `017b` view/sim distance y nivel | World | `world/mod.rs`, `pumpkin-world` |
| `HCF-CORE-028` | `028a` buffers del mundo | World | `world/mod.rs` |
| `HCF-CORE-028` | `028b` buffers del tracker | Tracker | `world/entity_tracker.rs` |

La ficha del parche puede seguir siendo una sola (`HCF-CORE-016.md`) con dos
commits; lo importante es que cada commit tenga un dueño claro.

## 6. Oleadas

### Oleada 0 — Reconocimiento y baseline (60–90 min)

Agentes en paralelo, todos read-only:

1. **Scout World**: mapea `World::tick`, `tick_environment`, `tick_chunks`,
   `update_active_chunks`, colisiones. Devuelve funciones/líneas y puntos de
   early-return.
2. **Scout Player/Tracker**: mapea `Player::tick`, stats, warden, chunk batch,
   `entity_tracker::update_all/update_player`.
3. **Scout Packets/WIT**: mapea `send_packet`, `generated_packets.rs`, WIT v0.1,
   `send_packet_now_data`, oportunidades de batch.
4. **Profiler**: corre `perf record`/`perf stat` con 100 bots o analiza el
   artefacto de B21; entrega top symbols y flamegraph.
5. **Baseline runner**: si hay CI disponible, lanza el run baseline; si no,
   congela los artefactos B21 y define el método.

Salida de cada scout:

```text
STATUS: done
FILES:
HOT_PATHS:
PATCH_POINTS:
RISKS:
RECOMMENDED_ORDER:
```

**Gate G0:** top 10 símbolos identificados; baseline con p99; contratos
propuestos.

### Oleada 1 — Fundamentos (90–120 min)

- 1 implementador Foundation: `HCF-CORE-001`, `HCF-CORE-002`, `HCF-CORE-004`,
  `HCF-CORE-005`, `HCF-CORE-010`, `HCF-CORE-016b`, `HCF-CORE-017a`.
- 1 verifier: tests de perfil off == vanilla, config round-trip, dimensiones.
- 1 docs scribe: fichas `HCF-CORE-001/002/004/005/010`.
- El orquestador mergea y congela contratos (`HcfProfile`, `HcfMetrics`,
  `server/mod.rs`).

**Gate G1a:** build vanilla y HCF compilan; `/version` y `/tps` funcionan.

### Oleada 2 — Implementación paralela (3–4 h)

Máximo 4 implementadores. El agente World es el long pole; los demás avanzan
independientes.

| Agente | Patches | Archivos | Timebox |
|---|---|---|---|
| World | `011–015`, `017b`, `020`, `023`, `027`, `028a` | `world/mod.rs`, `entity/mod.rs`, `pumpkin-world/src/level.rs` | 3–4 h, por subpasos |
| Player | `016a`, `021` | `entity/player.rs` | 2–3 h |
| Tracker | `022`, `028b` | `world/entity_tracker.rs` | 2 h |
| Packet | `024`, `030` | `generated_packets.rs`, `net/java/mod.rs`, WIT v0.2 | 2–3 h |
| Scheduler | `025`, `026` | `server/scheduler.rs`, `plugin/mod.rs` | 2 h |
| Docs/Reviewer | — | docs, read-only | continuo |

Reglas:

- Foundation deja congelados `server/mod.rs`, la config y los contratos; no se
  reabren en la Oleada 2.
- World empieza por early-outs y extracción de helpers; cuando mergea, sigue con
  `tick_chunks_hcf`, colisiones y política de entidades.
- Player y Tracker no tocan `world/mod.rs`.
- Packet no toca WIT v0.1.
- Scheduler no toca `world/mod.rs`.
- Si World excede el timebox, entrega primero `011–014` + `020` y deja
  `015/023/027/028a` para la siguiente oleada. No se mergea a medias.
- Cada agente commitea en su branch y devuelve el hash.

### Oleada 3 — Integración (60–90 min)

- Merge queue en orden: Foundation → World (early-outs) → Player → Tracker →
  Packet → Scheduler → World (chunk/collisions) → Docs.
- `cargo fmt`, `clippy`, `test`, build vanilla y HCF.
- Reviewer agent revisa:
  - ¿Se tocó la ruta vanilla?
  - ¿Se cambiaron APIs públicas?
  - ¿Se añadieron locks/unsafe?
  - ¿Se respetó el file ownership?
  - ¿Hay tests de paridad?
- Si hay conflicto en un archivo, lo resuelve el dueño del archivo, no el
  integrador a ciegas.

**Gate G2:** build y tests verdes; ningún plugin v0.1 roto.

### Oleada 4 — Benchmark (2–3 h, exclusivo)

- Un solo benchmark runner. Lock `/tmp/pumpkin-hcf/bench.lock`.
- No compilar durante el benchmark.
- 3 repeticiones × 10 min, 100 bots; si G2 se cumple, 200 bots.
- Comparar baseline vs candidato.
- Aceptar si:
  - MSPT p99 mejora ≥3 %.
  - CPU no empeora >1 %.
  - No hay timeouts/errores nuevos.
  - No hay regresión vanilla.
- Si falla, volver a la sesión del agente con un fix concreto (30–60 min), no
  abrir un agente nuevo.

### Oleada 5 — Documentación y cierre (45–60 min)

- Actualizar `PATCHES.md`.
- Crear/actualizar fichas `docs/core/patches/HCF-CORE-###.md`.
- Añadir nota de rebase en `UPSTREAM.md`.
- `CHANGELOG-HCF.md` si existe.
- Preparar la siguiente oleada.

## 7. Plantilla de prompt para agentes

```text
Eres un agente de implementación del core HCF de Pumpkin.

Repositorio: /tmp/pumpkin-hcf/core-020
Commit base: 204a94e
Branch: hcf/core-020
Patch: HCF-CORE-020

Objetivo:
<una frase medible>

Contexto obligatorio:
- Lee docs/core/PATCH_STANDARD.md.
- Lee docs/core/patches/HCF-CORE-020.md.
- Lee docs/core/HCF_CORE_PLAN.md, sección de tu patch.

Archivos permitidos:
- crates/pumpkin/src/world/mod.rs

Archivos prohibidos:
- Todo lo demás. No toques Cargo.toml, WIT, player.rs, entity_tracker.rs.

Contratos:
- HcfProfile ya existe y está mergeado.
- No cambies firmas de helpers acordadas.

Tareas:
1. Extrae los helpers indicados sin cambiar la ruta vanilla.
2. Añade tick_chunks_hcf con composición.
3. Añade early-returns solo donde se indique.
4. Añade tests.

Criterios de aceptación:
- cargo fmt --check
- cargo clippy (sin warnings nuevos)
- cargo test
- Perfil off == comportamiento vanilla.
- Perfil on no ejecuta random ticks/spawning.

Medición:
- No corras loadtest. El benchmark lo hace el orquestador.
- Si puedes, mide con `cargo test` o un microbench, pero no bloquees.

Timebox: 90 minutos. Si te bloqueas, para y devuelve el estado.

Salida obligatoria:
STATUS:
BRANCH:
COMMIT:
FILES:
TESTS:
PERF:
BLOCKERS:
RISKS:
NEXT:
```

### Ejemplo de scout

```text
Eres un scout read-only. No edites archivos.

Repo: /tmp/pumpkin-native
Objetivo: mapear el hot path de World::tick y tick_chunks.
Devuelve:
- funciones y líneas exactas
- allocations por tick
- locks por tick
- bucles O(n²)
- puntos de early-return seguros
- riesgos de compatibilidad
No propongas código extenso; solo hallazgos y referencias.
```

### Ejemplo de reviewer

```text
Eres un reviewer read-only. No edites.

Revisa el diff de hcf/core-020 contra 204a94e.
Comprueba:
- La ruta vanilla no cambió de semántica.
- No se tocaron archivos prohibidos.
- No hay locks nuevos en tick.
- No hay unsafe.
- Los tests cubren perfil off/on.
- La API pública no cambió.
Devuelve blockers, no estilo.
```

## 8. Contrato de salida de agentes

Todo agente debe terminar con este bloque:

```text
STATUS: done | blocked | partial
BRANCH:
COMMIT:
FILES:
TESTS:
PERF:
BLOCKERS:
RISKS:
NEXT:
```

El orquestador no acepta “listo” sin commit, tests y lista de archivos.

## 9. Merge queue

Orden recomendado:

1. Foundation (`001`, `002`, `004`, `005`, `010`, `016b`, `017a`).
2. World early-outs (`011`–`015`, `017b`).
3. Player (`016a`, `021`).
4. Tracker (`022`, `028b`).
5. Packet (`024`, `030`).
6. Scheduler (`025`, `026`).
7. World chunk/collisions (`020`, `023`, `027`, `028a`).
8. Docs.

Reglas:

- `git diff --name-only` debe coincidir con los archivos permitidos.
- Si un agente tocó un archivo prohibido, se rechaza el commit; no se “arregla”
  a mano en el merge.
- Cada merge corre `cargo fmt`, `clippy`, `test` y build.
- Si hay conflicto, el dueño del archivo lo resuelve en su worktree y vuelve a
  entregar.
- No se hace squash de todo en un solo commit; se conservan los commits por
  patch.

## 10. Protocolo de benchmark

- Lock exclusivo: `/tmp/pumpkin-hcf/bench.lock`.
- Mismo mundo, mismos bots, mismo view-distance, mismo hardware.
- 3 repeticiones × 10 min; descartar el primer minuto de calentamiento.
- Métricas: MSPT p50/p95/p99, CPU backend, CPU proxy, RSS, TPS, timeouts,
  paquetes/s.
- `perf record` antes/después; guardar flamegraph en
  `/tmp/pumpkin-hcf/perf/<run-id>`.
- Criterio de aceptación: p99 ≥3 % mejor, CPU ≤1 % peor, sin errores nuevos.
- Si no se cumple, rollback del patch y nota en `PATCHES.md`.

### 10.1 Bucle de caza de rendimiento

Ningún agente implementa una optimización “porque suena bien”. El bucle es:

```text
perf baseline
  → ranking de símbolos por self time
  → hipótesis con ms esperados
  → parche mínimo
  → A/B (p99 y CPU)
  → keep / revert
  → actualizar backlog
```

Reglas:

1. **Evidencia primero.** El profiler entrega top symbols, callers y flamegraph.
   El orquestador prioriza por `self time × frecuencia`, no por intuición.
2. **Una hipótesis por parche.** “Quitar random ticks” es una hipótesis;
   “optimizar el mundo” no lo es.
3. **Medir el efecto aislado y combinado.** Si dos parches interactúan, se mide
   cada uno y luego el conjunto.
4. **Microbench solo para funciones puras.** El tick del servidor se valida con
   loadtest, no con un bench sintético.
5. **Perf diff.** Comparar flamegraphs antes/después para confirmar que el
   tiempo se movió a donde se esperaba, no a otro cuello.
6. **Allocator después de CPU.** Solo se investiga allocator/locks si `perf`
   muestra que dominan; no antes.

Comandos de referencia:

```bash
# Muestreo de CPU del servidor (ajustar PID/ruta)
perf record -F 999 -g -- ./target/release/pumpkin
perf report --stdio --sort comm,dso,symbol | head -80

# Flamegraph (si inferno/flamegraph está instalado)
perf script | inferno-collapse-perf | inferno-flamegraph > /tmp/pumpkin-hcf/perf/run.svg

# Comparación de dos perfiles
perf diff perf.data.before perf.data.after
```

El benchmark runner es el único que decide si un parche se queda. Un agente
puede reportar “mejora en mi microbench”, pero eso no es un veredicto.

## 11. Matemática de compresión

Ejemplo con 5 parches independientes de 90 minutos:

| Modo | Cálculo | Tiempo |
|---|---|---|
| Serial | 5 × 90 min | 7.5 h |
| 3 agentes | 2 h de implementación + 1 h integración + 2 h benchmark | ~5 h |
| 4 agentes | 2 h + 1 h + 2 h | ~5 h |

Con 10 parches:

| Modo | Cálculo | Tiempo |
|---|---|---|
| Serial | 10 × 90 min | 15 h |
| 4 agentes | 3–4 h + 2 h integración + 2 h benchmark | ~7–8 h |

La aceleración realista es **2–3×**. Los límites duros son:

- Compilación de Rust.
- Tests de integración.
- Loadtest y repeticiones.
- Rebase/merge.
- Revisión humana de decisiones de gameplay.

No se debe prometer “todo en 2 horas” si hay que medir 100/200 bots, validar
vanilla y hacer rollback.

## 12. Guardrails

- Nada de producción ni jugadores reales.
- Nada de ejecutar el jar viejo de Paper en local (riesgo JavaAssist).
- Nada de `git push` desde agentes.
- Nada de benchmarks concurrentes.
- Nada de locks nuevos en tick sin revisión.
- Nada de `unsafe` por microganancias.
- Nada de cambios en APIs públicas v0.1.
- Nada de editar archivos generados a mano.
- Nada de “optimizar” sin `perf` o métrica.
- Nada de mezclar refactor + optimización + feature en el mismo commit.
- Si un agente se bloquea 15 minutos, debe parar y reportar.

## 13. Checklist para lanzar una oleada

- [ ] Baseline congelado (commit/tag/hash).
- [ ] Contratos mergeados.
- [ ] Worktrees creados y `CARGO_TARGET_DIR` aislado.
- [ ] Ficha `docs/core/patches/HCF-CORE-###.md` creada desde la plantilla.
- [ ] Archivos permitidos/prohibidos escritos en el prompt.
- [ ] Tests de aceptación definidos.
- [ ] Slot de benchmark reservado.
- [ ] Agentes lanzados con timebox.
- [ ] Merge queue owner asignado.
- [ ] Plan de rollback claro.

## 14. Asignación inicial concreta

| Oleada | Agente | Tipo | Tarea | Salida |
|---|---|---|---|---|
| 0 | Scout World | `explore` | Mapa de `tick_chunks`/`World::tick` | Hot paths + líneas |
| 0 | Scout Player | `explore` | Mapa de `Player::tick`/tracker | Hot paths + locks |
| 0 | Scout Packets | `explore` | Mapa de packet/WIT | Batch points |
| 0 | Profiler | `general` | `perf`/B21 | Top symbols + flamegraph |
| 1 | Foundation | `general` | 001/002/004/005/010/016b/017a | Branch + tests |
| 1 | Verifier | `general` | Paridad vanilla | Tests |
| 2 | World | `general` | 011–015, 017b, 020, 023, 027, 028a | Branch + tests |
| 2 | Player | `general` | 016a, 021 | Branch + tests |
| 2 | Tracker | `general` | 022, 028b | Branch + tests |
| 2 | Packet | `general` | 024, 030 | Branch + tests |
| 2 | Scheduler | `general` | 025, 026 | Branch + tests |
| 3 | Reviewer | `general` | Revisión de diffs | Blockers |
| 4 | Benchmark | `general` | A/B 100/200 | CSV + veredicto |
| 5 | Docs | `general` | Fichas/tracker | Docs actualizadas |

## 15. Lo que no se paraleliza

- Benchmarks.
- Ediciones concurrentes de `world/mod.rs`.
- `Cargo.toml`/`Cargo.lock`.
- Archivos generados de WIT.
- Integración final.
- Decisiones de gameplay (por ejemplo, desactivar redstone o fluidos).
- Validación con jugadores reales.

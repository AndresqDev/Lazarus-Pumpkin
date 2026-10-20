# Playbook de ejecución con agentes

## 0. Objetivo

Reducir el tiempo de pared de las implementaciones independientes usando varios
agentes en paralelo, sin sacrificar la calidad ni la validez de las mediciones.
El objetivo técnico es **bajar MSPT y CPU del backend**; el gateway 1.8 queda
diferido.

La regla es simple:

> **Paralelizar código y análisis. Serializar compilación final, tests,
> benchmarks, merges y decisiones.**

Los agentes pueden comprimir un día de implementación a **3–5 horas** de reloj
en una oleada bien separada. No pueden comprimir el tiempo de compilación,
tests, carga y validación por debajo de su mínimo real. El objetivo realista es
**2–3× de aceleración**, no 10×.

## 1. Modelo de trabajo

```text
Orquestador (agente principal)
  ├── Oleada 0: scouts + profiler + baseline
  ├── Oleada 1: contratos/fundamentos (1 implementador)
  ├── Oleada 2: implementación code-first (máx. 3–4, sin build/test)
  ├── Oleada 3: merge + primera compilación completa
  ├── Oleada 4: test-and-fix (tests, paridad, fixes)
  ├── Oleada 5: benchmark exclusivo
  └── Oleada 6: documentación + siguiente wave
```

Principios:

1. **Contratos primero.** Antes de paralelizar, el orquestador mergea las
   interfaces (`HcfProfile`, `HcfMetrics`, firmas de helpers, API de paquetes).
   Sin contratos, los agentes se pisan.
2. **Code-first en la implementación.** En la Oleada 2 los agentes escriben y
   commitean código; **no** corren builds completos, tests ni benchmarks. La
   validación se concentra en las Oleadas 3–4. Así no se pagan 4–5 compilaciones
   completas en paralelo.
3. **Un dueño por archivo.** Cada agente tiene una lista de archivos permitidos
   y prohibidos. Si necesita tocar un archivo compartido, pide permiso.
4. **Worktrees aislados.** Cada agente trabaja en su propio `git worktree` y su
   propio `CARGO_TARGET_DIR`.
5. **Benchmark exclusivo.** Solo un agente corre loadtests a la vez. Mientras
   tanto, no se compilan builds grandes en la misma máquina.
6. **Timebox.** Cada agente tiene un presupuesto de tiempo. Si se bloquea,
   devuelve hallazgos y el orquestador decide; no entra en bucle.
7. **Nada se acepta sin medir.** Un parche que “debería” mejorar no se queda si
   el A/B no lo demuestra. La ausencia de tests en la Oleada 2 no es una
   exención: se pagan en la Oleada 4.

## 2. Roles

| Rol | Subagente | Escribe código | Salida | Timebox |
|---|---|---|---|---|
| Orquestador | principal | Solo integración | Plan, merges, decisiones | Toda la sesión |
| Scout | `explore` | No | Mapa de hot paths, líneas, riesgos | 30–45 min |
| Profiler | `general` | No | `perf`, flamegraph, top symbols | 45–60 min |
| Implementador | `general` | Sí, en su worktree | Branch + commit, sin build/test | 60–90 min |
| Fixer | `general` | Sí, misma sesión | Errores de compilación/tests corregidos | 30–60 min por ciclo |
| Verifier | `general` | Tests en su worktree | Tests, vanilla parity | 45–90 min |
| Reviewer | `general` | No | Hallazgos, blockers | 20–30 min |
| Benchmark runner | `general` | No | CSV, p50/p95/p99, veredicto | 90–120 min |
| Docs scribe | `general`/`explore` | Solo docs | Fichas y tracker | 30 min |
| Integrador | orquestador | Sí | Merge queue, compilación, conflictos | 60–120 min |

Un agente de implementación **no** hace de reviewer de su propio código. El
reviewer es otro agente o el orquestador.

### 2.1 Cómo se lanzan

- **Scouts**: `subagent` con `agent=explore`, `background=true`, prompt
  read-only. Se pueden lanzar 3–5 a la vez.
- **Implementadores**: `subagent` con `agent=general`, `background=true`, uno
  por worktree y por conjunto de archivos. Máximo 3–4 simultáneos. En la
  Oleada 2 el prompt dice explícitamente: **no compilar, no testear, no
  benchmark**; solo código + commit.
- **Fixers**: continuar la misma sesión del implementador con `sessionID` y un
  prompt corto para corregir compilación/tests. No crear un agente nuevo para un
  ajuste del mismo parche.
- **Profiler/benchmark**: `agent=general`, pero **no** en background durante un
  benchmark; el benchmark es exclusivo y va después de tests verdes.
- **Modelo**: no fijar `model` salvo que el usuario lo pida; usar el modelo por
  defecto del harness.
- El orquestador espera las notificaciones de los agentes en background; no
  hace polling ni duerme.

## 3. Workspace y worktrees

Repos:

- Core: `/tmp/pumpkin-native` (clon de Pumpkin).
- Plugin: `/home/andresq/IdeaProjects/Lazarus-Pumpkin`.
- Worktrees de trabajo: `/home/andresq/pumpkin-hcf/<patch-id>`.
- Artefactos de perf: `/home/andresq/pumpkin-hcf/perf/<run-id>`.
- Lock de benchmark: `/home/andresq/pumpkin-hcf/bench.lock`.

> **Storage:** en esta máquina `/tmp` es tmpfs de ~8 GB y suele estar al 90 %.
> Los worktrees y `target/` van en `/home/andresq/pumpkin-hcf`, no en `/tmp`.
> Como el modo code-first no compila por agente, solo se necesitan dos target
> dirs: uno para el `cargo check` de contratos y otro para C1/B1.

Ejemplo:

```bash
cd /tmp/pumpkin-native
git fetch --all --tags
git worktree add /home/andresq/pumpkin-hcf/core-001 -b hcf/core-001 204a94e
git worktree add /home/andresq/pumpkin-hcf/core-020 -b hcf/core-020 204a94e
```

Build aislado:

```bash
export CARGO_TARGET_DIR=/home/andresq/pumpkin-hcf/target/core-020
export CARGO_BUILD_JOBS=2
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=/home/andresq/pumpkin-hcf/sccache
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

**Única compilación temprana:** la Oleada 1 hace un `cargo check` único para
confirmar que los contratos existen y tipan. No es un build completo, no corre
tests y no genera binario de benchmark. Es más barato que cuatro agentes
escribiendo contra una API que no compila.

Si un agente necesita cambiar un contrato, debe pedirlo al orquestador. No se
cambia unilateralmente.

## 5. Propiedad de archivos

| Área | Archivos | Dueño | Notas |
|---|---|---|---|
| Perfil/config | `pumpkin-config/src/hcf.rs`, `lib.rs`, `pumpkin.toml` | Foundation | Oleada 1 |
| Métricas | `server/metrics.rs`, `ticker.rs` | Foundation | Oleada 1; instrumentación antes de congelar |
| Servidor/dimensiones | `server/mod.rs`, `main.rs` | Foundation | Incluye 010 y 016b; después congelado |
| Build/PGO | `Cargo.toml`, `.github/workflows` | Foundation | Oleada 1 |
| Mundo | `world/mod.rs` | World | Un solo dueño; incluye 020, 023 y 029 |
| Nivel/autosave | `pumpkin-world/src/level.rs` | World | Incluye la parte de 017b |
| Jugador | `entity/player.rs` | Player | Incluye 016a; no tocar `world/mod.rs` |
| Tracker | `world/entity_tracker.rs` | Tracker | Incluye 028b; no tocar `player.rs` |
| Entidades/política | `entity/mod.rs` | World | Incluye 015, 027 y flags de 029; no tocar `player.rs` |
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

### Oleada 1 — Fundamentos y contratos (90–120 min)

- 1 implementador Foundation: `HCF-CORE-001`, `HCF-CORE-002`, `HCF-CORE-004`,
  `HCF-CORE-005`, `HCF-CORE-010`, `HCF-CORE-016b`, `HCF-CORE-017a`.
  `HCF-CORE-004` aquí solo deja perfiles/CI; el build PGO real se hace en C1/B1.
- 1 reviewer read-only: revisa contratos, no tests.
- 1 docs scribe: fichas `HCF-CORE-001/002/004/005/010`.
- El orquestador mergea y congela contratos (`HcfProfile`, `HcfMetrics`,
  `server/mod.rs`).

**Excepción de compilación:** un único `cargo check` para confirmar que los
contratos tipan. No es build completo, no corre tests, no genera binario de
benchmark.

**Gate C0:** contratos compilan; APIs congeladas; `/version` y `/tps` tienen
sitio donde engancharse.

### Oleada 2 — Implementación code-first (3–4 h)

Máximo 4 implementadores. El agente World es el long pole; los demás avanzan
independientes. **Nadie compila, testea ni benchmarkea en esta oleada.**

| Agente | Patches | Archivos | Timebox |
|---|---|---|---|
| World | `011–015`, `017b`, `020`, `029`, `023`, `027`, `028a` | `world/mod.rs`, `entity/mod.rs`, `pumpkin-world/src/level.rs` | 3–4 h, por subpasos |
| Player | `016a`, `021` | `entity/player.rs` | 2–3 h |
| Tracker | `022`, `028b` | `world/entity_tracker.rs` | 2 h |
| Packet | `024`, `030` | `generated_packets.rs`, `net/java/mod.rs`, WIT v0.2 | 2–3 h |
| Scheduler | `025`, `026` | `server/scheduler.rs`, `plugin/mod.rs` | 2 h |
| Docs/Reviewer | — | docs, read-only | continuo |

Reglas:

- Foundation deja congelados `server/mod.rs`, la config y los contratos; no se
  reabren en la Oleada 2.
- World empieza por early-outs y extracción de helpers; después `tick_chunks_hcf`,
  `029` y, solo si sigue haciendo falta, `023`.
- **029 va antes que 023.** Si desactivar colisiones de jugador elimina la
  mayoría de pares, `023` se reduce o se descarta; no se optimiza código muerto.
- Player y Tracker no tocan `world/mod.rs`.
- Packet no toca WIT v0.1.
- Scheduler no toca `world/mod.rs`.
- Si World excede el timebox, entrega primero `011–014` + `020` + `029` y deja
  `015/023/027/028a` para la siguiente oleada. No se mergea a medias.
- Cada agente commitea en su branch y devuelve el hash. El contrato de salida
  debe decir `TESTS: not run (code-first)`.

**Gate I1:** todos los branches entregados y revisados; no se exige compilación
todavía.

### Oleada 3 — Merge y primera compilación (60–120 min)

- Merge queue en orden: Foundation → World (early-outs/029) → Player → Tracker →
  Packet → Scheduler → World (chunk/023/027/028a) → Docs.
- Resolver conflictos textuales por dueño de archivo.
- Una sola compilación completa: `cargo check --workspace` y luego build de
  debug/release según haga falta.
- Triage de errores por archivo/dueño; los fixers continúan la sesión original
  con `sessionID` y corrigen solo su parte.
- Repetir compilación hasta que todo tipa.
- No correr tests funcionales todavía; no benchmark.

**Gate C1:** workspace completo compila, vanilla y HCF.

### Oleada 4 — Test-and-fix (2–4 h)

- `cargo fmt --check`, `clippy`, `cargo test`, build vanilla y HCF.
- Tests de paridad: perfil off == vanilla; perfil on aplica flags.
- Tests de gameplay HCF: colisiones de jugador off, ataques on, bloques on,
  packet batch, scheduler, stats.
- Tests de compatibilidad: plugin v0.1 en host v0.2; plugin nativo viejo falla
  limpio.
- La suite completa corre en serie; los fixes se reparten por archivo/módulo y
  se re-ejecutan tests dirigidos.
- Reviewer agent revisa:
  - ¿Se tocó la ruta vanilla?
  - ¿Se cambiaron APIs públicas?
  - ¿Se añadieron locks/unsafe?
  - ¿Se respetó el file ownership?
  - ¿Hay tests de paridad?
- Si hay conflicto en un archivo, lo resuelve el dueño del archivo, no el
  integrador a ciegas.

**Gate T1:** tests y paridad verdes; ningún plugin v0.1 roto; sin deadlocks en
smoke.

### Oleada 5 — Benchmark (2–3 h, exclusivo)

- Solo después de T1. Un solo benchmark runner. Lock
  `/home/andresq/pumpkin-hcf/bench.lock`.
- No compilar durante el benchmark.
- 3 repeticiones × 10 min, 100 bots; si T1 se cumple, 200 bots.
- Comparar baseline vs candidato.
- Aceptar si:
  - MSPT p99 mejora ≥3 %.
  - CPU no empeora >1 %.
  - No hay timeouts/errores nuevos.
  - No hay regresión vanilla.
- Si falla, volver a la sesión del agente con un fix concreto (30–60 min), no
  abrir un agente nuevo.

**Gate B1:** benchmark aceptado o parche revertido.

### Oleada 6 — Documentación y cierre (45–60 min)

- Actualizar `PATCHES.md`.
- Crear/actualizar fichas `docs/core/patches/HCF-CORE-###.md`.
- Añadir nota de rebase en `UPSTREAM.md`.
- `CHANGELOG-HCF.md` si existe.
- Preparar la siguiente oleada.

### 6.1 Protocolo test-and-fix

La fase T1 no es “correr tests y ver qué pasa”. Es una secuencia de ciclos
cortos:

1. **T1.1 — Unitarios por módulo.** Cada fixer corre los tests de su área.
   Corrige sin cambiar comportamiento ni APIs.
2. **T1.2 — Integración/gametest.** random ticks, spawning, weather, raids,
   stats, packet batch, colisiones de jugador off.
3. **T1.3 — Compatibilidad.** plugin v0.1 en host v0.2; nativo viejo falla
   limpio; config vieja migra o warning.
4. **T1.4 — Paridad vanilla.** perfil off == vanilla; perfil on aplica flags.
5. **T1.5 — Suite completa + smoke de carga.** Recién aquí se declara T1 verde.

Reglas del ciclo:

- Un ciclo = correr, triage, asignar al dueño, corregir, re-ejecutar lo dirigido.
- No se añaden features ni refactors en T1; solo fixes.
- Si un parche falla 3 ciclos, se revierte o se parte en un parche más pequeño.
- La suite completa se corre en serie; los tests dirigidos pueden repartirse.
- Cada fix deja una línea en la ficha del parche: qué falló, qué se cambió y
  qué test lo cubre.

## 7. Plantilla de prompt para agentes

```text
Eres un agente de implementación del core HCF de Pumpkin.

Repositorio: /home/andresq/pumpkin-hcf/core-020
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
4. No añadas tests en esta oleada; deja el código y las notas para T1.

Modo code-first:
- NO ejecutes `cargo build`, `cargo test`, `cargo clippy` ni benchmarks.
- NO esperes a que compile. La compilación completa es la Oleada 3.
- Si necesitas desbloquearte por una duda de tipos, puedes hacer un
  `cargo check` puntual con `sccache`, pero no es obligatorio ni es gate.
- No declares “terminado y testeado”; declara “código entregado”.

Criterios de aceptación:
- Respeta los contratos y las firmas acordadas.
- No toca archivos prohibidos.
- La ruta vanilla queda semánticamente intacta.
- Perfil off == comportamiento vanilla (revisión estática).
- Perfil on no ejecuta random ticks/spawning (revisión estática).
- Commit limpio en la branch.

Medición:
- No corras loadtest. El benchmark lo hace el orquestador en la Oleada 5.

Timebox: 90 minutos. Si te bloqueas, para y devuelve el estado.

Salida obligatoria:
STATUS:
BRANCH:
COMMIT:
FILES:
TESTS: not run (code-first)
BUILD: not run (code-first)
PERF: not run (code-first)
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
- Hay tests/notas previstos para T1 (no se exige ejecutarlos en code-first).
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
TESTS: not run (code-first) | pass/fail (T1)
BUILD: not run (code-first) | pass/fail (C1)
PERF: not run (code-first) | p50/p95/p99 (B1)
BLOCKERS:
RISKS:
NEXT:
```

En code-first, el orquestador no acepta “listo” sin commit, branch y lista de
archivos. Los tests y el build se exigen recién en C1/T1.

## 9. Merge queue

Orden recomendado:

1. Foundation (`001`, `002`, `004`, `005`, `010`, `016b`, `017a`).
2. World early-outs (`011`–`015`, `017b`).
3. Player (`016a`, `021`).
4. Tracker (`022`, `028b`).
5. Packet (`024`, `030`).
6. Scheduler (`025`, `026`).
7. World chunk/collisions (`020`, `029`, `023`, `027`, `028a`).
8. Docs.

Reglas:

- `git diff --name-only` debe coincidir con los archivos permitidos.
- Si un agente tocó un archivo prohibido, se rechaza el commit; no se “arregla”
  a mano en el merge.
- El merge es textual: **no** se corre `cargo test`/build por branch. La primera
  compilación completa y la suite se hacen en C1/T1. La única excepción es el
  `cargo check` de contratos de la Oleada 1.
- Si hay conflicto, el dueño del archivo lo resuelve en su worktree y vuelve a
  entregar.
- No se hace squash de todo en un solo commit; se conservan los commits por
  patch.
- `029` se mergea antes que `023`; si `029` deja pocos pares, `023` puede
  descartarse.

## 10. Protocolo de benchmark

- Solo después de T1: no se mide un árbol que no compila o que tiene tests
  rojos.
- Lock exclusivo: `/home/andresq/pumpkin-hcf/bench.lock`.
- Mismo mundo, mismos bots, mismo view-distance, mismo hardware.
- 3 repeticiones × 10 min; descartar el primer minuto de calentamiento.
- Métricas: MSPT p50/p95/p99, CPU backend, CPU proxy, RSS, TPS, timeouts,
  paquetes/s.
- `perf record` antes/después; guardar flamegraph en
  `/home/andresq/pumpkin-hcf/perf/<run-id>`.
- Criterio de aceptación: p99 ≥3 % mejor, CPU ≤1 % peor, sin errores nuevos.
- Excepción: los cambios de gameplay aprobados (`HCF-CORE-029`, colisiones off)
  se aceptan por correctitud; el A/B mide la ganancia, pero no los revierte
  salvo bug o regresión.
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
perf script | inferno-collapse-perf | inferno-flamegraph > /home/andresq/pumpkin-hcf/perf/run.svg

# Comparación de dos perfiles
perf diff perf.data.before perf.data.after
```

El benchmark runner es el único que decide si un parche **de rendimiento** se
queda. Los cambios de gameplay aprobados (`029`) se aceptan por correctitud; el
runner mide su impacto, no los revierte por un 2 % de ganancia. Un agente puede
reportar “mejora en mi microbench”, pero eso no es un veredicto.

## 11. Matemática de compresión

Ejemplo con 5–10 parches independientes:

| Fase | Serial | Code-first con 4 agentes |
|---|---:|---:|
| Implementación inicial | 7.5–15 h | 2–4 h |
| Merge + primera compilación | 0.5–1 h | 1–2 h |
| Test-and-fix | 2–4 h | 2–4 h |
| Benchmark | 2 h | 2 h |
| **Total** | **12–22 h** | **7–12 h** |

La ganancia fuerte está en la **implementación inicial**: se escribe en horas y
no se espera a que cada agente compile. La validación no desaparece; se
concentra en una sola compilación y una fase de test-and-fix. Por eso el total
realista es **7–12 h**, no 2 h.

La aceleración realista es **2–3×**. Los límites duros son:

- Compilación de Rust.
- Tests de integración.
- Loadtest y repeticiones.
- Rebase/merge.
- Revisión humana de decisiones de gameplay.

No se debe prometer “todo en 2 horas” si hay que medir 100/200 bots, validar
vanilla y hacer rollback. Lo que sí se puede prometer es que **la primera
implementación de todo el código** pase de una jornada a 2–4 h.

## 12. Guardrails

- Nada de producción ni jugadores reales.
- Nada de ejecutar el jar viejo de Paper en local (riesgo JavaAssist).
- Nada de `git push` desde agentes.
- Nada de builds/tests/benchmarks en la Oleada 2; eso es code-first.
- En C1/T1 solo se corrige; no se añaden features ni refactors nuevos.
- Nada de benchmark antes de que T1 esté verde.
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
- [ ] Tests de aceptación definidos (se ejecutan en T1, no en code-first).
- [ ] Presupuesto de test-and-fix reservado (2–4 h).
- [ ] Slot de compilación C1 y benchmark B1 reservados.
- [ ] Agentes lanzados con timebox y modo code-first explícito.
- [ ] Merge queue owner asignado.
- [ ] Plan de rollback claro.

## 14. Asignación inicial concreta

| Oleada | Agente | Tipo | Tarea | Salida |
|---|---|---|---|---|
| 0 | Scout World | `explore` | Mapa de `tick_chunks`/`World::tick` | Hot paths + líneas |
| 0 | Scout Player | `explore` | Mapa de `Player::tick`/tracker | Hot paths + locks |
| 0 | Scout Packets | `explore` | Mapa de packet/WIT | Batch points |
| 0 | Profiler | `general` | `perf`/B21 | Top symbols + flamegraph |
| 1 | Foundation | `general` | 001/002/004/005/010/016b/017a | Branch + `cargo check` único |
| 1 | Reviewer | `general` | Contratos y ownership | Blockers |
| 2 | World | `general` | 011–015, 017b, 020, 029, 023, 027, 028a | Branch + commit, sin tests |
| 2 | Player | `general` | 016a, 021 | Branch + commit, sin tests |
| 2 | Tracker | `general` | 022, 028b | Branch + commit, sin tests |
| 2 | Packet | `general` | 024, 030 | Branch + commit, sin tests |
| 2 | Scheduler | `general` | 025, 026 | Branch + commit, sin tests |
| 3 | Integrador | orquestador | Merge + `cargo check`/build | Árbol compilando |
| 3 | Fixers | `general` | Errores de compilación por archivo | Commits de fix |
| 4 | Verifier | `general` | Tests/paridad | Suite verde |
| 4 | Fixers | `general` | Fallos de tests por módulo | Commits de fix |
| 5 | Benchmark | `general` | A/B 100/200 | CSV + veredicto |
| 6 | Docs | `general` | Fichas/tracker | Docs actualizadas |

## 15. Lo que no se paraleliza

- Benchmarks.
- La primera compilación completa y la suite completa de tests.
- Ediciones concurrentes de `world/mod.rs`.
- `Cargo.toml`/`Cargo.lock`.
- Archivos generados de WIT.
- Integración final.
- Decisiones de gameplay (por ejemplo, desactivar colisiones, redstone o fluidos).
- Validación con jugadores reales.

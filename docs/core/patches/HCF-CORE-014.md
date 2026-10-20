# HCF-CORE-014 — Random ticks fuera e inhabited time a 1 Hz

- **Estado:** en diseño
- **Fase:** 1
- **Upstreamable:** parcial (el corte de random ticks en `Level` es genérico; el perfil HCF no)
- **Riesgo:** medio (cambia la velocidad de local difficulty)
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

Hay dos costes independientes dentro de `tick_chunks`:

1. **Random ticks.** `Level::get_tick_data`
   (`crates/pumpkin-world/src/level.rs:519-599` en `204a94e`) recorre todos los
   chunks activos, entra en `block_sections.read()`, consulta la máscara y
   muestrea `random_tick_speed` veces por sección
   (`level.rs:524-579`). Aunque `random_tick_speed = 0`, sigue recorriendo
   chunks/secciones y tomando el lock de `block_sections`; además reserva
   `active_chunks.len() * 3` para `random_ticks` (`level.rs:529`). Después,
   `World::tick_chunks` ejecuta la fase de random ticks siempre
   (`world/mod.rs:2002-2042`), aunque el vector esté vacío.
2. **Inhabited time.** `World::tick_chunks` recorre **todos** los chunks activos
   cada tick y hace `fetch_add(1)` sobre `chunk.inhabited_time`
   (`world/mod.rs:2089-2100`). Con 100 jugadores y cientos de chunks activos
   son cientos/miles de incrementos atómicos por tick, además de un `collect`
   a `Vec` (`:2092`) y el wake de Rayon.

HCF no usa random ticks salvo farms concretas, y no necesita inhabited time a
20 Hz: la dificultad local usa una escala de 3.600.000 ticks
(`entity/mob/equipment.rs:585-624`) y 1 Hz es suficiente. La feature
`HcfFeature::RandomTicks` y `HcfFeature::InhabitedTime` ya existen
(`server/hcf.rs:45-47`), al igual que `[hcf] disable_random_ticks` e
`inhabited_time_hz` (`pumpkin-config/src/hcf.rs:33,58`).

## Cambio

### 1. Random ticks

1. En `Level::get_tick_data`, si `samples_per_section == 0`, **no recorrer** el
   bucle de secciones para random ticks (`level.rs:533-579`). Devolver
   `random_ticks: Vec::new()` y seguir con los scheduled block/fluid ticks
   (`level.rs:581-599`). La firma no cambia: `random_tick_speed = 0` ya
   significa “sin samples”.
2. En `World::tick_chunks`, calcular la velocidad efectiva una sola vez:
   ```rust
   let random_tick_speed = if server.hcf_profile().disabled(HcfFeature::RandomTicks) {
       0
   } else {
       self.level_info.load().game_rules.random_tick_speed
   };
   let tick_data = self.level.get_tick_data(&active_chunks, random_tick_speed);
   ```
   Así no se muestrea ni se aloca la lista cuando HCF apaga random ticks.
3. Extraer la fase `world/mod.rs:2002-2042` a `tick_random_ticks(...)` y poner
   un early-return defensivo al principio:
   ```rust
   // HCF-CORE-014: sin random ticks no se despierta Rayon.
   if server.hcf_profile().disabled(HcfFeature::RandomTicks) {
       return;
   }
   ```
   La ruta vanilla llama al helper en el mismo orden (después de fluid ticks).
4. No tocar block ticks ni fluid ticks: se siguen recolectando y ejecutando
   siempre.

### 2. Inhabited time a `inhabited_time_hz`

1. Extraer el bloque `world/mod.rs:2089-2100` a
   `tick_inhabited_time_hcf(active_chunks, hz)`.
2. Actualizar a la frecuencia configurada, no a 20 Hz:
   - `inhabited_time_hz` se clampiza a `1..=20` en 005 (`server/hcf.rs:219`).
   - `20` equivale a vanilla (un incremento por tick).
   - `1` (default HCF) incrementa cada chunk **una vez por segundo**.
3. Preferir **time-slice** antes que un pico cada 20 ticks: repartir los chunks
   activos en `20 / hz` slices y procesar un slice por tick. Cada chunk recibe
   exactamente un incremento por periodo, pero el coste se reparte. Si `hz` no
   divide 20, usar un acumulador de ticks y procesar el conjunto cuando
   corresponda; no hace falta precisión sub-tick.
4. No tocar `ChunkData::inhabited_time` (`pumpkin-world/src/chunk/mod.rs:83`),
   no resetearlo, no desmontar chunks y no cambiar el guardado NBT. Solo se
   reduce la frecuencia de incremento.
5. Con `hcf.enabled = false` o `inhabited_time_hz = 20`, el incremento vuelve a
   ser el actual.

### Comportamiento y decisión de gameplay

- Random ticks off detiene cultivos, fuego, oxidación de cobre, crecimiento de
  árboles por random tick y demás mecánicas que dependan de ellos. Es una
  decisión del perfil HCF; el admin puede poner
  `disable_random_ticks = false` para conservarlos.
- Inhabited time a 1 Hz hace que la dificultad local crezca **20 veces más
  despacio** que en vanilla. Es intencional en HCF, pero debe documentarse: si
  un modo de juego depende de subir dificultad por estancia, subir
  `inhabited_time_hz`.
- `randomTickSpeed = 0` en la gamerule ya producía el mismo resultado visible;
  014 evita además el coste de muestreo.
- Los chunks ya cargados no se recargan ni se invalidan; solo se salta trabajo.

## Patrón

- [x] Early-return
- [x] Time-slice (`inhabited_time_hz`)
- [x] Config (`disable_random_ticks`, `inhabited_time_hz`)
- [ ] Homólogo por composición (lo hace 020)
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/world/mod.rs` — velocidad efectiva, extracción de
  `tick_random_ticks` y `tick_inhabited_time_hcf`. **Dueño único de
  `world/mod.rs`: 014 comparte archivo con 013 y 020; se aplican en serie.**
- `crates/pumpkin-world/src/level.rs` — corte del muestreo cuando
  `random_tick_speed = 0`. **014 es dueño de `get_tick_data`;** 017 puede tocar
  autosave en `pumpkin-world`, pero no esta función.
- `crates/pumpkin/src/server/hcf.rs` — features y `inhabited_time_hz()` ya
  existen (005).
- `crates/pumpkin-config/src/hcf.rs` — claves ya existen; no se cambia el
  default.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.

## Compatibilidad

- APIs públicas: no cambian. No se toca WIT.
- Plugins v0.1: no se toca ningún contrato. Un plugin que lea
  `inhabited_time` verá un valor que crece más despacio; debe ser consciente
  del perfil. No hay API nueva para leerlo en 014.
- Jugadores vanilla: con `enabled = false`, random ticks e inhabited time son
  exactamente los actuales.
- Datos: no se borra ni se migra `InhabitedTime`; los chunks conservan su valor
  y siguen guardándose. Al volver a `inhabited_time_hz = 20`, recuperan el ritmo
  vanilla sin saltos artificiales.
- Gameplay HCF: cultivos/fuego pueden no avanzar con random ticks off. Es
  opt-in y reversible por config.

## Métricas

- **Antes:** sección `RandomTicks` y `InhabitedTime` de 002, tamaño de
  `random_ticks`, MSPT p50/p95/p99 y tiempo de `get_tick_data`. El baseline B21
  (18,99 ms) no desglosa estas fases.
- **Después:** `get_tick_data` no debe recorrer secciones con random off; la
  sección `InhabitedTime` debe procesar solo el slice configurado (con
  time-slice a 1 Hz, una fracción del conjunto por tick), sin picos de p99.
  Medir el efecto real; no se asume un porcentaje.
- **Método:** A/B 3 × 10 min, 100 bots; comparar `random_tick_speed` normal vs
  0, y `inhabited_time_hz` 20 vs 1. Medir también el p99, no solo la media.
  **Ganancia: a medir.** No se acepta por TPS medio.

## Tests

- [ ] Unitario: `get_tick_data(..., 0)` devuelve `random_ticks` vacío y sigue
      devolviendo block/fluid ticks.
- [ ] Unitario: `get_tick_data(..., 3)` mantiene el muestreo actual.
- [ ] Unitario: `RandomTicks` off ⇒ no se ejecuta `tick_random_ticks` ni se
      llama a `random_tick` de un bloque de prueba.
- [ ] Unitario: `RandomTicks` on ⇒ un cultivo/fuego de prueba avanza como en
      vanilla.
- [ ] Unitario: `inhabited_time_hz = 1` ⇒ un chunk recibe 1 incremento cada 20
      ticks, no 20.
- [ ] Unitario: `inhabited_time_hz = 20` ⇒ incremento por tick, idéntico a
      vanilla.
- [ ] Unitario: chunk descargado y recargado conserva su `inhabited_time`.
- [ ] Unitario: `active_chunks` vacío ⇒ sin pánico, sin allocations de random
      ticks.
- [ ] Integración: perfil off ⇒ mismas llamadas y mismo orden que antes.
- [ ] Integración: random off + bloque/fluido programado ⇒ el block/fluid tick
      sigue ejecutándose.
- [ ] Soak: 30 min; sin chunks atascados ni `inhabited_time` negativo/overflow.
- [ ] Loadtest: 100/200 bots; MSPT p99 y tiempo de `get_tick_data`.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.disable_random_ticks = false` y `hcf.inhabited_time_hz = 20`, o
`hcf.enabled = false`, y reinicio. No hay datos que migrar: el valor de
`inhabited_time` es acumulativo y sigue siendo válido. Si se revierte el
binario, el mundo vuelve al ritmo vanilla sin recalcular nada.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivos con conflicto probable: `world/mod.rs` (`tick_chunks`) y
  `pumpkin-world/src/level.rs` (`get_tick_data`).
- Orden obligatorio: **013 → 014 → 020**. 013 y 014 extraen helpers del mismo
  `tick_chunks`; 020 compone la ruta HCF encima. No implementar 014 en paralelo
  con 013 ni con 020.
- `world/mod.rs` es el long pole: 014 tampoco puede ir en paralelo con 029,
  023, 011, 012, 017b, 027 ni 028a. 021/022/024/026 no tocan este archivo.
- `pumpkin-world/src/level.rs` puede recibir cambios de 017 (autosave) o de
  generación de mundo; mantener el corte de random ticks como un early-return
  local y no reordenar block/fluid ticks.
- Si upstream cambia `get_tick_data`, conservar la semántica de
  `random_tick_speed = 0` y el orden block/fluid; no convertir el corte en un
  cambio de firma si se puede evitar.

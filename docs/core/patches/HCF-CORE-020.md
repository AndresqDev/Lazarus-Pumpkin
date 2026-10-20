# HCF-CORE-020 — Composición de `tick_chunks_hcf`

- **Estado:** implementado (C1 pendiente)
- **Fase:** 2
- **Upstreamable:** no (ruta HCF); la extracción de helpers es mecánica y revisable
- **Riesgo:** medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

Después de 013 y 014, `World::tick_chunks`
(`crates/pumpkin/src/world/mod.rs:1950-2100` en `204a94e`) tendrá las fases
extraídas, pero sigue siendo una sola ruta monolítica: cualquier combinación de
features se resuelve con early-returns dispersos y el orden de las fases no
está garantizado por un único punto de composición. Eso tiene dos riesgos:

- que un cambio futuro invierta o duplique fases (por ejemplo, llamar dos veces
  a `get_tick_data` o ejecutar spawning antes de random ticks);
- que la ruta HCF acabe siendo una copia de 150 líneas de la vanilla, justo lo
  que prohíbe `PATCH_STANDARD.md`.

HCF necesita una ruta propia que ejecute **solo** las fases que el perfil usa,
con el mismo orden vanilla para las que se conservan. La ruta vanilla debe
seguir existiendo y ejecutándose igual con `hcf.enabled = false`.

Depende de **010** (menos dimensiones/chunks activos), **013** (spawning
extraído) y **014** (random ticks/inhabited extraídos y con frecuencia
reducida). 020 no repite esos cambios: los compone.

## Cambio

1. Extraer de `tick_chunks` estos helpers privados (013 y 014 ya extraen los
   dos últimos; 020 completa la composición):
   - `tick_scheduled_block_ticks(&Arc<Self>, &TickData, &Handle)`
   - `tick_fluid_ticks(&Arc<Self>, &TickData, &Handle)`
   - `tick_random_ticks(&Arc<Self>, &TickData, &Handle)`
   - `tick_natural_spawning(&Arc<Self>, &FxHashSet<Vector2<i32>>, &Handle)`
   - `tick_inhabited_time_hcf(&Arc<Self>, &FxHashSet<Vector2<i32>>, hz)`
2. La ruta vanilla llama a los cinco helpers **en el mismo orden y con la misma
   semántica** que hoy:
   ```rust
   fn tick_chunks(self: &Arc<Self>, server: &Arc<Server>) {
       // HCF-CORE-020: ruta HCF; la vanilla sigue intacta debajo.
       if server.hcf_profile().fast_chunks() {
           self.tick_chunks_hcf(server);
           return;
       }

       let active_chunks = ...;
       let tick_data = self.level.get_tick_data(&active_chunks, random_tick_speed);
       self.tick_scheduled_block_ticks(&tick_data, &handle);
       self.tick_fluid_ticks(&tick_data, &handle);
       self.tick_random_ticks(&tick_data, &handle);
       self.tick_natural_spawning(&active_chunks, &handle);
       self.tick_inhabited_time(&active_chunks);
   }
   ```
3. La ruta HCF compone, no copia:
   ```rust
   fn tick_chunks_hcf(self: &Arc<Self>, server: &Arc<Server>) {
       let active_chunks = ...;
       let effective_speed = if server.hcf_profile().disabled(HcfFeature::RandomTicks) {
           0
       } else {
           self.level_info.load().game_rules.random_tick_speed
       };

       // Una sola recolección de TickData para block/fluid/random.
       let tick_data = self.level.get_tick_data(&active_chunks, effective_speed);

       self.tick_scheduled_block_ticks(&tick_data, &handle); // siempre
       self.tick_fluid_ticks(&tick_data, &handle);           // siempre

       if !server.hcf_profile().disabled(HcfFeature::RandomTicks) {
           self.tick_random_ticks(&tick_data, &handle);
       }
       if !server.hcf_profile().disabled(HcfFeature::NaturalSpawning) {
           self.tick_natural_spawning(&active_chunks, &handle);
       }
       if !server.hcf_profile().disabled(HcfFeature::InhabitedTime) {
           self.tick_inhabited_time_hcf(&active_chunks, server.hcf_profile().inhabited_time_hz());
       }
   }
   ```
4. `get_tick_data` se llama **una vez** por tick en ambas rutas. No se puede
   reconstruir `TickData` para random ticks ni pedir dos veces los block ticks.
5. No cambiar el orden vanilla de block → fluid → random. Spawning e inhabited
   mantienen su posición relativa actual (después de random). Si una feature
   está off, se omite la llamada, no se mueve el resto.
6. No duplicar los cuerpos de los helpers en `tick_chunks_hcf`; el homólogo es
   la composición. Cualquier `if` de feature vive en la ruta HCF o en el
   early-return del helper, no dentro de la lógica vanilla compartida.
7. No añadir allocations nuevas. Si se necesita un `Vec` de chunks activos para
   inhabited, reutilizar el buffer de 028 o pasarlo por referencia; 020 no
   introduce `collect()` adicionales. Cualquier ordenación que se añada debe
   ser no-op con vector vacío.
8. Si `active_chunks` está vacío y `TickData` no tiene block/fluid/random
   ticks, salir después de `get_tick_data` sin despertar Rayon. No salir antes:
   `chunks_with_scheduled_ticks` puede contener trabajo aunque el set de activos
   esté vacío en una transición.
9. Mantener las secciones de 002 (`HcfSection::BlockTicks`, `FluidTicks`,
   `RandomTicks`, `NaturalSpawning`, `InhabitedTime`) dentro de los helpers, no
   envolver la llamada al helper en otra sección: si no, se cuenta dos veces.

### Lo que NO se toca

- La ruta vanilla no se reescribe: se mueven bloques a helpers de forma
  mecánica y se llaman en el mismo orden.
- No se cambia `Level::get_tick_data` en 020 (eso es 014).
- No se toca `tick`, `update_active_chunks`, `tick_environment`, raids,
  entidades, tracker ni block entities.
- No se cambia la firma pública de `tick_chunks` ni se añade API de plugins.
- No se toca `world/mod.rs` fuera de `tick_chunks` y sus helpers inmediatos.

## Patrón

- [x] Homólogo por composición
- [x] Early-return en el dispatcher
- [ ] API aditiva
- [x] Config (`chunk_tick_fast_path`)
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/world/mod.rs` — dispatcher `tick_chunks`, helpers y
  `tick_chunks_hcf`. **Dueño único de `tick_chunks_hcf` y del archivo durante
  esta oleada.** No se implementa en paralelo con 013/014 (van antes) ni con
  029/023/011/012/017b/027/028a.
- `crates/pumpkin/src/server/hcf.rs` — `fast_chunks()` y features ya existen
  (005); 020 no añade claves.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.

## Compatibilidad

- APIs públicas: no cambian; `tick_chunks` conserva firma. No se toca WIT.
- Plugins v0.1: no se toca ningún contrato. Los plugins no ven el dispatcher.
- Jugadores vanilla: con `enabled = false`, `tick_chunks` ejecuta exactamente
  las mismas llamadas y en el mismo orden que antes.
- HCF: con `chunk_tick_fast_path = false`, aunque HCF esté on, se usa la ruta
  vanilla; es el interruptor de rollback.
- Datos: no se tocan chunks, entidades, ticks programados ni NBT.
- Métricas: las secciones de 002 deben seguir existiendo con el mismo nombre y
  significado; si un helper se lleva su sección, el total de `Chunks` no cambia.

## Métricas

- **Antes:** `HcfSection::Chunks` y sus subsecciones (002), MSPT p50/p95/p99,
  CPU, número de llamadas a `get_tick_data` y de despertares de Rayon.
- **Después:** mismo trabajo con las features off omitidas; menos despertares de
  Rayon y menos allocs de `TickData`/listas. **Ganancia: a medir** contra el
  estado 013+014, no contra el baseline sin parches.
- **Método:** A/B 3 × 10 min, 100 bots, perfil off/on; comparar 013+014 solos
  vs 013+014+020. El gate de Fase 2 del plan (0,35–0,45 core/100 y p99 ≤ 12 ms)
  es un objetivo, no un dato medido; no se cita como resultado.
- Medir también el coste del dispatcher con `chunk_tick_fast_path = false` para
  demostrar que la ruta vanilla no ha empeorado.

## Tests

- [ ] Unitario: perfil off ⇒ el orden de llamadas es block → fluid → random →
      spawning → inhabited, idéntico al actual.
- [ ] Unitario: perfil on con todas las features ⇒ mismo orden y mismos
      resultados que la ruta vanilla.
- [ ] Unitario: `RandomTicks` off ⇒ no se llama `tick_random_ticks` y
      `get_tick_data` recibe velocidad 0.
- [ ] Unitario: `NaturalSpawning` off ⇒ no se llama `tick_natural_spawning`.
- [ ] Unitario: `InhabitedTime` off ⇒ no se llama `tick_inhabited_time_hcf`.
- [ ] Unitario: block y fluid ticks se ejecutan con todas las features off.
- [ ] Unitario: `get_tick_data` se llama exactamente una vez por tick (contador
      de test).
- [ ] Unitario: `active_chunks` vacío ⇒ sin pánico y sin despertar Rayon.
- [ ] Unitario: las secciones de 002 no se cuentan dos veces.
- [ ] Integración: 100 bots, perfil off/on; no hay doble spawn, doble block
      tick ni chunks atascados.
- [ ] Regresión: 013 y 014 siguen funcionando por separado con
      `chunk_tick_fast_path = false`.
- [ ] Soak: 30 min; sin deadlocks ni crecimiento de memoria.
- [ ] Loadtest: 100/200 bots; MSPT p99 y CPU.
- [ ] Suite ejecutada y verde en T1.

## Estado de implementación (2026-10-20)

- Implementado en `hcf/core-020`, commit `419d823`, mergeado en
  `hcf/foundation`.
- `tick_chunks` conserva la ruta vanilla y despacha a `tick_chunks_hcf` cuando
  `fast_chunks()` está activo. Se extrajeron helpers para block ticks, fluid
  ticks, random ticks, spawning e inhabited time; `get_tick_data` se llama una
  sola vez por ruta y `active_chunks` se lee una vez.
- La ruta HCF omite random/spawning según perfil. Si `InhabitedTime` está
  desactivada se usa el helper vanilla (+1/tick); si está activa se usa el
  intervalo de 014. `chunk_tick_fast_path = false` mantiene la ruta vanilla.
- La ficha asumía que 013/014 ya habían extraído helpers; se extrajeron en 020
  sin cambiar la semántica de los guards inline.
- C1/T1 pendientes; no se ejecutó compilación local.

## Rollback

`hcf.chunk_tick_fast_path = false` o `hcf.enabled = false` y reinicio: se usa la
ruta vanilla compuesta por los mismos helpers. Los helpers extraídos siguen
siendo válidos; no hay datos que migrar. Si el problema es una fase concreta,
se puede apagar su feature (013/014) sin revertir 020.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivo con conflicto probable: `world/mod.rs`, zona
  `tick_chunks`/helpers (`:1950-2100`).
- Orden obligatorio: **013 → 014 → 020**. 020 depende de los helpers de 013 y
  014; no se puede escribir antes ni en paralelo.
- **`world/mod.rs` es el long pole y 020 es su dueño único.** 020 no se
  implementa en paralelo con 029 (colisiones), 023 (spatial hash), 011/012
  (clima/raids), 017b (view/sim distance), 027 (política de tick) ni 028a
  (buffers del mundo). Si 029 ya está en la rama, 020 se rebasa encima y
  conserva sus cambios; si no, 029 espera. 021/022/024/026 no tocan este
  archivo.
- 010 solo toca `server/mod.rs`; no compite. 013/014 son prerrequisitos, no
  parches paralelos.
- Si upstream reescribe `tick_chunks`, conservar la composición y los helpers;
  no copiar la función vanilla dentro de `tick_chunks_hcf`.

# HCF-CORE-013 — Spawning natural fuera

- **Estado:** en diseño
- **Fase:** 1
- **Upstreamable:** parcial (la extracción del helper sí; el perfil HCF no)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`World::tick_chunks` (`crates/pumpkin/src/world/mod.rs:1950` en `204a94e`)
ejecuta la fase de spawning natural en **todos** los ticks y en **todos** los
chunks activos, aunque el perfil HCF no la use:

- calcula `spawn_state`, gamerules y `spawn_passives`
  (`world/mod.rs:2044-2056`);
- construye `spawn_list` con
  `natural_spawner::get_filtered_spawning_categories`
  (`world/mod.rs:2058-2063`, `world/natural_spawner.rs:478-505`), que aloca un
  `Vec`;
- recorre `active_chunks`, hace `read_chunk_sync` y clona el `Arc<ChunkData>`
  de cada chunk (`world/mod.rs:2065-2072`);
- mezcla la lista con `shuffle` (`world/mod.rs:2074`);
- lanza `tick_spawning_chunk` en Rayon (`world/mod.rs:2078-2086`,
  `tick_spawning_chunk` en `world/mod.rs:2410-2460`), que además puede generar
  el caso especial de tormenta (caballo esqueleto + rayo,
  `world/mod.rs:2417-2456`).

HCF 1.8 no necesita spawning natural. El coste no es solo “spawnear mobs”: es
leer chunks, alocar listas, barajar y despertar workers de Rayon cada tick. Aun
así, **los mobs que ya existen deben seguir viviendo y tickeando**; este parche
no los borra, no los despawna y no cambia su IA (eso queda para 015/027).

## Cambio

1. Extraer el bloque `world/mod.rs:2044-2087` a un helper privado
   `tick_natural_spawning(self: &Arc<Self>, active_chunks: &FxHashSet<Vector2<i32>>, handle: &Handle)`
   (nombre orientativo, sin `_hcf`: es una fase vanilla extraída).
2. La ruta vanilla de `tick_chunks` llama al helper **en la misma posición y
   con el mismo orden** que hoy: después de los random ticks y antes de
   inhabited time.
3. Al principio del helper, antes de leer gamerules, `spawn_state` o chunks:
   ```rust
   // HCF-CORE-013: HCF no genera mobs naturales; los existentes siguen.
   if server.hcf_profile().disabled(HcfFeature::NaturalSpawning) {
       return;
   }
   ```
   El helper necesita `&Arc<Server>` (o el `HcfProfile` ya resuelto) para no
   hacer `upgrade()` por tick.
4. No llamar a `get_filtered_spawning_categories`, no alocar `spawn_list`, no
   hacer `read_chunk_sync` y no despertar Rayon cuando la feature está off.
5. No tocar `spawn_state`, `SpawnState`, `spawn_entity_non_save`,
   `tick_spawning_chunk` ni el tick de entidades. No hay despawn ni limpieza.
6. `HcfFeature::NaturalSpawning` y `[hcf] disable_natural_spawning` ya existen
   en 001/005 (`server/hcf.rs:37`, `pumpkin-config/src/hcf.rs:25`); 013 solo
   los aplica.

### Comportamiento

- Con `hcf.enabled = false`, la ruta es exactamente la actual: mismo helper,
  mismas llamadas y mismo orden.
- Con HCF on y `disable_natural_spawning = true`, no aparecen mobs nuevos por
  spawning natural. Los mobs ya cargados siguen tickeando hasta que mueran,
  se despawnen por las reglas existentes o se descargue el chunk.
- Las gamerules siguen siendo la fuente de verdad cuando la feature está on:
  `doMobSpawning = false` ya hacía que no se generaran mobs; 013 no las cambia.
- El caso de tormenta de `tick_spawning_chunk` (`world/mod.rs:2417-2456`)
  también deja de ejecutarse al saltar la fase completa. En HCF el clima está
  off por 011, así que el efecto práctico es nulo; se documenta para que nadie
  lo confunda con un bug.
- Los mobs invocados por comando, plugins, huevos de spawn o estructuras no se
  ven afectados: no pasan por esta fase.

## Patrón

- [x] Early-return
- [x] Config (`[hcf] disable_natural_spawning`)
- [ ] Homólogo por composición (lo hace 020)
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/world/mod.rs` — extracción de
  `tick_natural_spawning` y early-return. **Dueño único de `world/mod.rs`:
  este parche comparte archivo con 014 y 020; se aplican en serie.**
- `crates/pumpkin/src/server/hcf.rs` — `HcfFeature::NaturalSpawning` ya existe
  (005); no se añade API nueva.
- `crates/pumpkin-config/src/hcf.rs` — clave `disable_natural_spawning` ya
  existe; no se cambia el default.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y orden.

## Compatibilidad

- APIs públicas: no cambian. No se toca WIT.
- Plugins v0.1: no se toca ningún contrato; un plugin que espere mobs naturales
  en un servidor HCF debe asumir que no se generan. Es opt-in por perfil.
- Jugadores vanilla: con `enabled = false`, comportamiento idéntico.
- Datos: no se borran entidades, chunks, POIs ni NBT. No hay migración.
- Gameplay HCF: los mobs existentes siguen siendo válidos; solo se corta la
  producción de nuevos mobs naturales. Si un evento necesita mobs, debe
  spawnearlos explícitamente o whitelistear la feature.

## Métricas

- **Antes:** tiempo de la sección `NaturalSpawning` de 002, MSPT p50/p95/p99,
  número de entidades y lecturas de chunk por tick. El baseline B21
  (18,99 ms de MSPT medio) es global; no atribuye esta fase.
- **Después:** la sección debe quedar cerca de cero con la feature off; bajarán
  las lecturas de chunk y la presión sobre Rayon. La reducción de entidades se
  ve a lo largo de varios minutos, no en el primer tick.
- **Método:** A/B 3 × 10 min, 100 bots, mundo premade, perfil off/on. Contar
  spawns nuevos con un contador de 002 o un log temporal; no contar solo TPS.
  **Ganancia: a medir.** No vender un porcentaje hasta tener el A/B.

## Tests

- [ ] Unitario: `enabled = false` ⇒ `tick_natural_spawning` ejecuta las mismas
      llamadas que la ruta actual (spawn list, lectura de chunks, shuffle).
- [ ] Unitario: `NaturalSpawning` off ⇒ no se llama
      `get_filtered_spawning_categories` ni `read_chunk_sync` para spawn.
- [ ] Unitario: `NaturalSpawning` off ⇒ un mob ya existente sigue tickeando y
      no se despawna por este parche.
- [ ] Integración: mundo plano controlado, perfil off ⇒ aparece un mob en
      condiciones de spawn; perfil on ⇒ no aparece.
- [ ] Integración: `doMobSpawning = false` ⇒ sin spawns en ambas rutas.
- [ ] Integración: mob invocado por comando/plugin sigue apareciendo con la
      feature off.
- [ ] Regresión: bloque/fluido/random ticks siguen ejecutándose con spawning
      off.
- [ ] Soak: 30 min con 100 bots; sin errores de chunks ni entidades huérfanas.
- [ ] Loadtest: 100/200 bots; comparar MSPT p99, entidades y lecturas de chunk.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.disable_natural_spawning = false` o `hcf.enabled = false` y reinicio. No
hay datos que migrar ni entidades que restaurar. Si se revierte el binario, los
mobs que ya existían siguen en el mundo; los que no llegaron a spawnear no se
pueden recuperar, pero es el comportamiento esperado del perfil.

## Notas de rebase

- Commit base: `204a94e`. Las líneas citadas son de ese commit.
- Archivo con conflicto probable: `world/mod.rs`, bloque `tick_chunks`
  (`:1950-2100`).
- Orden obligatorio: **013 → 014 → 020**. Los tres tocan `tick_chunks`; no se
  pueden implementar en paralelo. 020 es el dueño único de
  `tick_chunks_hcf`.
- `world/mod.rs` es el long pole: 013 tampoco puede ir en paralelo con 029,
  023, 011, 012, 017b, 027 ni 028a. 021/022/024/026 no tocan este archivo.
- 010 solo toca `server/mod.rs`; no compite con 013.
- Si upstream cambia `tick_chunks`, conservar los helpers extraídos y reaplicar
  el early-return; no “ganar” el conflicto borrando la ruta vanilla.

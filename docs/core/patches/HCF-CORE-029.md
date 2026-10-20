# HCF-CORE-029 — Colisiones físicas jugador-jugador y jugador-mob off

- **Estado:** implementado (C1 pendiente)
- **Fase:** 2 (se implementa antes que 023)
- **Upstreamable:** no (gameplay HCF)
- **Riesgo:** medio
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

Con 100+ jugadores, `LivingEntity::push_entities` escanea y clona todas las
entidades del AABB de cada jugador y de cada mob, y aplica empuje
jugador-jugador y jugador-mob. HCF 1.8 no necesita ese empuje: los jugadores
pueden ocupar el mismo bloque y atravesar mobs. Es coste O(living × entidades)
por tick y además genera velocity updates y paquetes.

## Cambio

Desactivar **solo el empuje físico** en `LivingEntity::push_entities`
(`crates/pumpkin/src/entity/living.rs:1417-1459`):

- Si el que empuja es un jugador:
  - no incluir jugadores como candidatos si `player_player_collisions = false`;
  - no incluir mobs como candidatos si `player_mob_collisions = false`.
- Si el que empuja es un mob:
  - no incluir jugadores como candidatos si `player_mob_collisions = false`;
  - mob-mob se conserva.
- Aplicar el filtro **antes** del chequeo de `MaxEntityCramming` y del bucle
  `entity.push(dyn_self)`.
- Si la lista queda vacía, salir antes de leer la gamerule.

`HcfProfile` se añade a `World` como `Arc<HcfProfile>` inmutable para no hacer
`Weak::upgrade` por entidad/tick. `World::load` lo obtiene de
`server.upgrade()`; si no hay servidor (tests), usa un perfil vanilla.

### Estado de implementación (2026-10-20)

- Implementado en `hcf/core-029`, commit `7a1543e` (rebase sobre 010),
  mergeado en `hcf/foundation`.
- `World` guarda `Arc<HcfProfile>`; `World::load` lo toma del servidor y usa
  `HcfConfig::default()` (HCF off) cuando no hay servidor, para tests.
- El filtro de `push_entities` se calcula una vez por empujador
  (`block_players`/`block_mobs`) y solo clasifica candidatos con
  `get_player()`/`get_mob()` cuando hay algo que bloquear. Con HCF off o ambas
  colisiones activas no hay llamadas virtuales extra.
- Se conserva `pushable.is_empty()` antes de leer `MaxEntityCramming`; el filtro
  va antes del cramming y del bucle `push`.
- C1/T1 pendientes; no se ejecutó compilación local.

### Lo que NO se toca

- `Entity::push` sigue igual para los pares permitidos.
- `World::tick` (`world/mod.rs:1607-1618`) y `on_player_collision` siguen
  ejecutándose. En particular, el daño por contacto de slime/magma
  (`mob/mod.rs:1502`, `slime.rs:338`, `magma_cube.rs:76`) **se conserva**.
- Colisión con bloques (`Entity::tick_block_collisions`), movimiento vertical,
  pose, `can_fit_pose`, raycast, ataques, proyectiles, perlas, flechas, items,
  orbes, vehículos y minecarts no cambian.
- `MaxEntityCramming` mob-mob se conserva; solo deja de contar los pares
  filtrados. En 1.8 los jugadores no reciben cramming de jugadores.
- El dragón tiene su propio push (`ender_dragon.rs:583-607`); no se toca en 029.

## Patrón

- [x] Early-return / filtro en helper existente
- [x] Config (`[hcf] player_player_collisions`, `player_mob_collisions`)
- [ ] Homólogo por composición
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/entity/living.rs` — filtro de `push_entities`.
- `crates/pumpkin/src/world/mod.rs` — campo `hcf_profile` en `World` y
  construcción en `World::load`.
- `crates/pumpkin/src/server/hcf.rs` — helpers de colisiones (005).
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado y decisión de gameplay.

## Compatibilidad

- APIs públicas: no cambian; `World` gana un campo.
- Plugins v0.1: no se toca WIT.
- Jugadores vanilla: `enabled = false` ⇒ `push_entities` idéntico.
- HCF: cambio de gameplay visible; documentar en el README del perfil.
- Migración: no; las claves nuevas tienen default HCF `false` solo con perfil
  activo. Con `enabled = false` el default es vanilla `true`.

## Métricas

- **Antes:** coste de `push_entities` y `PushEntities` (002); pares
  entidad-jugador; CPU backend B21.
- **Después:** reducción esperada de allocations/clones en `get_all_at_box` y
  de velocity updates; medir con 100 bots **apilados** y **separados**.
- **Método:** A/B 3 × 10 min, 100 bots, perfil off/on. No se acepta solo por
  TPS medio; mirar MSPT p99, CPU y paquetes/s. Al ser decisión de gameplay, se
  acepta por correctitud aunque el gate de 3 % no se cumpla; solo se revierte
  por bug o regresión clara.

## Tests

- [ ] Unitario: jugador-jugador off ⇒ dos jugadores en el mismo bloque no se
      empujan.
- [ ] Unitario: jugador-mob off ⇒ el jugador atraviesa el mob, pero el mob
      sigue empujando a otros mobs.
- [ ] Unitario: jugador-mob on ⇒ el empuje vanilla se conserva.
- [ ] Unitario: slime/magma sigue haciendo daño de contacto con 029 off.
- [ ] Unitario: `enabled = false` ⇒ `push_entities` llama a los mismos
      candidatos que antes.
- [ ] Integración: dos bots apilados no se separan; un bot y un mob se
      atraviesan; bloques/proyectiles siguen colisionando.
- [ ] Compatibilidad: `MaxEntityCramming` mob-mob sigue funcionando.
- [ ] Soak: 30 min con 100 bots apilados, sin velocity infinita ni desync.
- [ ] Loadtest: 100/200 bots; comparar apilados vs separados.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.player_player_collisions = true` y `hcf.player_mob_collisions = true`, o
`hcf.enabled = false`. No hay datos que migrar. Si se revierte el binario,
restaurar el backup y reiniciar.

## Notas de rebase

- Commit base: `204a94e`.
- Archivos con conflicto probable: `entity/living.rs`, `world/mod.rs`
  (`World` struct/`load`).
- Orden: después de 005 y 002; antes de 023. Si 029 elimina la mayoría de
  pares, 023 se reduce o se descarta.
- `world/mod.rs` es el long pole: 029 se coordina con el dueño World y no se
  implementa en paralelo con 020/023.

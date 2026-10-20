# HCF-CORE-010 — Solo dimensiones necesarias

- **Estado:** implementado (C1 pendiente)
- **Fase:** 1
- **Upstreamable:** parcial (config de dimensiones sí; perfil HCF no)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`Server::new` construye `dimensions` con `allow_nether` y `allow_end`
(`server/mod.rs:254-263`). HCF normalmente solo usa el overworld; cargar Nether
y End mantiene chunks, entidades, POIs, raids y tareas de tick que no aportan.
No se trata de borrar datos, solo de no cargar esas dimensiones.

## Cambio

1. En `Server::new`, después de construir `hcf_profile`, aplicar:
   ```rust
   let dimensions = if hcf_profile.only_overworld() {
       vec![Dimension::OVERWORLD]
   } else {
       // ruta vanilla actual
   };
   ```
2. Si `hcf.only_overworld = true` y `basic_config.allow_nether`/`allow_end`
   estaban activos, emitir un `warn!` una vez al arrancar. No modificar
   `pumpkin.toml` ni `level.dat`.
3. `HcfFeature::Dimensions` se considera activa cuando `only_overworld` está
   activo; el resto de parches pueden consultarlo.
4. No tocar `World::create_world`: sigue iterando `server.dimensions`
   (`server/mod.rs:388`).
5. Con `hcf.enabled = false`, la lista es exactamente la de hoy.

### Estado de implementación (2026-10-20)

- Implementado en `hcf/core-010`, commit `7f41404`, mergeado en
  `hcf/foundation`.
- `HcfProfile::only_overworld()` se deriva de `HcfFeature::Dimensions`; el
  perfil `hcf-1.8` la activa por defecto y `only_overworld = false` la apaga.
- `hcf_profile` se construye antes de la lista de dimensiones y se mueve una
  sola vez al `Server`.
- El warning se emite una vez si `allow_nether`/`allow_end` estaban activos.
- C1/T1 pendientes; no se ejecutó compilación local.

### Comportamiento

- Overworld siempre se carga.
- Nether/End no se cargan si el perfil lo pide.
- Los portales a una dimensión no cargada usan el fallback existente de
  `get_world_from_dimension` (`server/mod.rs:459-470`), que ya se usa cuando
  `allow_nether`/`allow_end` están en `false`. Documentar que un portal al
  Nether/End en HCF no debe usarse.
- No se borran regiones, POIs ni datos de jugador.

## Patrón

- [x] Early-return / selección de lista
- [x] Config (`[hcf] only_overworld`)
- [ ] Homólogo
- [ ] API aditiva
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/server/mod.rs` — construcción de `dimensions` y warning.
- `crates/pumpkin/src/server/hcf.rs` — `only_overworld()`.
- `crates/pumpkin-config/src/hcf.rs` — clave.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado.

## Compatibilidad

- APIs públicas: no cambian.
- Plugins v0.1: no se toca WIT. Un plugin que asuma Nether/End fallará al
  buscar la dimensión; debe comprobar `server.dimensions`.
- Jugadores vanilla: con `enabled = false`, sin cambios.
- Migración: no; el perfil es opt-in.

## Métricas

- **Antes:** B21 con Overworld/Nether/End configurados.
- **Después:** menos chunks activos, entidades y POIs; medir MSPT p50/p95/p99,
  CPU y RSS.
- **Método:** A/B 100 bots, 3 × 10 min; perfil off/on. Si el mundo premade no
  tiene Nether/End cargados, la ganancia será baja; no vender más de lo medido.

## Tests

- [ ] Unitario: `only_overworld = true` ⇒ lista `[OVERWORLD]`.
- [ ] Unitario: `enabled = false` ⇒ misma lista que `allow_nether`/`allow_end`.
- [ ] Unitario: warning una sola vez si había Nether/End activos.
- [ ] Integración: arranque con mundo premade HCF; no se crean carpetas
      `DIM-1`/`DIM1` nuevas.
- [ ] Compatibilidad: `get_world_from_dimension` no entra en pánico.
- [ ] Soak: 30 min; no errores de portal/POI.
- [ ] Loadtest: 100 bots; comparar RSS y MSPT.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.only_overworld = false` o `hcf.enabled = false` y reinicio. Los datos de
Nether/End no se tocan; vuelven a cargarse si se activan.

## Notas de rebase

- Commit base: `204a94e`.
- Archivos con conflicto probable: `server/mod.rs` (zona de `Server::new`).
- Orden: después de 001/005; antes de 020/029. `server/mod.rs` se congela
  después de 001/002/005/010.

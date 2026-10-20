# HCF-CORE-001 — Perfil HCF, build ID y capacidades

- **Estado:** en diseño
- **Fase:** 0
- **Upstreamable:** parcial (config y build ID sí; defaults HCF no)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

Pumpkin no tiene un perfil runtime que permita activar la ruta HCF sin cambiar
el comportamiento vanilla. Tampoco expone un build ID estable que el plugin
pueda validar antes de reemplazar el binario. Sin esto, cada parche tendría que
leer `advanced_config` en caliente o duplicar flags, y el actualizador no podría
fallar cerrado.

## Cambio

1. Añadir `HcfConfig` en `pumpkin-config/src/hcf.rs` y `pub hcf: HcfConfig` a
   `AdvancedConfiguration`. La sección TOML es `[hcf]`.
2. Añadir `HcfFeature` y `HcfProfile` en `pumpkin/src/server/hcf.rs`.
   `HcfProfile` es inmutable, se construye una vez y se guarda como
   `Arc<HcfProfile>` en `Server`.
3. Añadir `PUMPKIN_BUILD_ID` y `PUMPKIN_HCF_ABI` con `CARGO_PKG_VERSION` y
   `GIT_HASH` (ya generado por `crates/pumpkin/build.rs`).
4. Loguear el perfil y el build ID al arrancar. `enabled = false` por defecto.
5. Exponer capacidades como bitmask de `HcfFeature` para que el plugin pueda
   consultarlas en una fase posterior.

### Contrato de configuración

```toml
[hcf]
enabled = false
profile = "hcf-1.8"

# Dimensiones
only_overworld = true

# Trabajo que HCF no usa. `Option<bool>` distingue "no especificado" de
# "false"; el perfil aporta el default.
disable_mob_ai = true
disable_natural_spawning = true
disable_raids = true
disable_villages = true
disable_weather = true
disable_random_ticks = true
disable_advancements = true
disable_statistics = true
disable_recipe_book = true
disable_chat_signing = true
disable_warden = true
disable_maps = true

# Fast paths
chunk_tick_fast_path = true
player_tick_fast_path = true
entity_tracker_fast_path = true
packet_batch = true
scheduler_budget_ms = 5
inhabited_time_hz = 1

# Colisiones (positivas: `true` = vanilla)
player_player_collisions = false
player_mob_collisions = false
```

`HcfConfig` usa `Option<bool>` para las claves que el perfil puede rellenar.
`enabled` y `profile` son explícitos. `HcfProfile::from_config` aplica, en
orden: defaults del perfil, luego las claves explícitas. `inhabited_time_hz` se
limita a `1..=20`; `scheduler_budget_ms` a `0..=50`.

### Contrato de `HcfProfile`

```rust
#[inline]
pub const fn disabled(&self, feature: HcfFeature) -> bool {
    self.enabled && self.disabled_features.contains(feature)
}

#[inline]
pub const fn fast_path(&self, feature: HcfFeature) -> bool {
    self.enabled && self.enabled_features.contains(feature)
}
```

`enabled = false` ⇒ `disabled()` y `fast_path()` devuelven `false`; la ruta
vanilla no cambia. `HcfFeature` cabe en `u64` (21 bits iniciales). No se usan
locks ni `server.advanced_config` en bucles calientes.

### Build ID

```text
PUMPKIN_BUILD_ID = "<CARGO_PKG_VERSION>+<GIT_HASH>"
PUMPKIN_HCF_ABI  = 1
```

Si `GIT_HASH` es `unknown`, el build ID sigue siendo válido pero el actualizador
debe tratarlo como no reproducible. El log de arranque incluye `hcf_profile` y
`build_id` en una línea estable para CI.

## Patrón

- [x] Config + API aditiva
- [ ] Homólogo por composición
- [x] Early-return (lo usan los parches que consumen el perfil)
- [ ] Solo configuración
- [ ] Build/CI

## Archivos

- `crates/pumpkin-config/src/lib.rs` — módulo y campo `hcf`.
- `crates/pumpkin-config/src/hcf.rs` — `HcfConfig` y defaults.
- `crates/pumpkin/src/server/mod.rs` — campo `hcf_profile`, construcción.
- `crates/pumpkin/src/server/hcf.rs` — `HcfFeature`, `HcfProfile`, build ID,
  capacidades.
- `crates/pumpkin/src/main.rs` — log de arranque.
- `pumpkin.toml` (sample) — sección `[hcf]` desactivada.
- `docs/core/HCF_CORE_PLAN.md`, `PATCHES.md` — estado.

## Compatibilidad

- APIs públicas: no cambian; solo se añade configuración y un campo público.
- Plugins v0.1: no se toca WIT.
- Jugadores vanilla: `enabled = false` ⇒ comportamiento idéntico.
- Migración: no; el merge de config rellena `[hcf]` con defaults.
- `HcfProfile` se construye antes de mover `advanced_config` a `Server`.

## Métricas

- **Antes:** línea base B21; sin perfil no hay forma de medir A/B.
- **Después:** el perfil permite comparar `enabled = false/true` con el mismo
  binario y build ID.
- **Método:** arranque con `[hcf] enabled = false` y `true`; verificar que el
  log muestra el mismo `build_id` y que la ruta vanilla no cambia.

## Tests

- [ ] Unitario: `HcfConfig` parsea `[hcf]`, claves desconocidas y defaults.
- [ ] Unitario: `enabled = false` ⇒ `disabled()`/`fast_path()` falsos.
- [ ] Unitario: overrides explícitos ganan al perfil; rangos se clampean.
- [ ] Unitario: build ID no vacío y ABI = 1.
- [ ] Integración: arranque vanilla con `enabled = false` y un mundo premade.
- [ ] Compatibilidad: el plugin no requiere cambios.
- [ ] Soak: N/A.
- [ ] Loadtest: N/A (solo 002 mide).
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.enabled = false` en `pumpkin.toml` y reinicio. Si el binario no arranca,
restaurar el backup del binario y el `pumpkin.toml` anterior; no hay migración
de datos.

## Notas de rebase

- Commit base de Pumpkin: `204a94e`.
- Archivos con conflicto probable: `pumpkin-config/src/lib.rs`,
  `pumpkin/src/server/mod.rs`, `pumpkin/src/main.rs`.
- Orden respecto a otros parches: primero; 002, 005 y todos los demás dependen
  de este contrato. `server/mod.rs` se congela después de 001/002/005.

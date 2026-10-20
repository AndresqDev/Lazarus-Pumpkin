# HCF-CORE-005 — Helpers de perfil y feature flags

- **Estado:** en diseño
- **Fase:** 0
- **Upstreamable:** parcial (helpers genéricos sí; flags HCF no)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`HCF-CORE-001` aporta la configuración y el build ID, pero los módulos
calientes necesitan una consulta barata y uniforme: “¿HCF apaga esta fase?” y
“¿HCF usa este homólogo?”. Sin un contrato único, cada parche inventaría su
propio `bool` o leería `advanced_config` en el tick.

## Cambio

1. Definir `HcfFeature` (21 bits) y `HcfFeatureSet` (`u64`) en
   `pumpkin/src/server/hcf.rs`.
2. Completar `HcfProfile` con `disabled()`, `fast_path()`, `active_features()` y
   `capabilities()`.
3. Añadir helpers de gameplay:
   - `player_player_collisions_enabled()`
   - `player_mob_collisions_enabled()`
   - `inhabited_time_hz()`
   - `scheduler_budget_ms()`
4. Añadir `Server::hcf_profile()` y `Server::hcf_metrics()` como accesores
   `#[inline]`; el campo sigue siendo público para evitar ruido en módulos
   existentes.
5. Mapear `HcfConfig` a bits una sola vez en `HcfProfile::from_config`.
6. Exponer `capabilities()` como bitmask de features activas para el futuro
   handshake del plugin (WIT v0.2, `HCF-CORE-030`).

### `HcfFeature`

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HcfFeature {
    Weather = 0,
    MobAi,
    NaturalSpawning,
    Raids,
    Villages,
    Dimensions,
    RandomTicks,
    InhabitedTime,
    Advancements,
    Statistics,
    RecipeBook,
    ChatSigning,
    Warden,
    Maps,
    ChunkTickFastPath,
    PlayerTickFastPath,
    EntityTrackerFastPath,
    PacketBatch,
    SchedulerBudget,
    PlayerPlayerCollisions,
    PlayerMobCollisions,
}
```

`HcfFeatureSet` es un `u64` con `insert`, `remove`, `contains`, `bits` y
`all()`. `HcfFeature::bit()` usa `1u64 << (self as u8)`; no se añade `bitflags`
ni otra dependencia.

### Semántica

| Consulta | `enabled = false` | `enabled = true` |
|---|---|---|
| `disabled(feature)` | `false` | `true` si el perfil la apaga |
| `fast_path(feature)` | `false` | `true` si el perfil usa el homólogo |
| `player_*_collisions_enabled()` | `true` | valor del perfil (HCF: `false`) |
| `capabilities()` | `0` | bits de features activas |

Regla: un early-return `if profile.disabled(feature) { return; }` **nunca** se
dispara con HCF apagado. Un dispatch `if profile.fast_path(feature) { ... }`
tampoco. La ruta vanilla queda intacta.

### Defaults del perfil `hcf-1.8`

Apagados: `MobAi`, `NaturalSpawning`, `Raids`, `Villages`, `Weather`,
`RandomTicks`, `Advancements`, `Statistics`, `RecipeBook`, `ChatSigning`,
`Warden`, `Maps`, `PlayerPlayerCollisions`, `PlayerMobCollisions`.

Activados: `ChunkTickFastPath`, `PlayerTickFastPath`,
`EntityTrackerFastPath`, `PacketBatch`, `SchedulerBudget`, `InhabitedTime`
(a 1 Hz), `Dimensions` (solo overworld).

`Villages` no tiene aún un early-return global; se marca como feature para que
los parches de spawning/POI la consulten cuando existan. `Dimensions` la consume
`HCF-CORE-010`.

### Capabilities

`HcfProfile::capabilities()` devuelve los bits de las features que el perfil
considera activas (no las apagadas). El valor se loguea al arrancar y se
expondrá por WIT v0.2. No es un ABI de plugins v0.1.

## Patrón

- [x] Config + API aditiva
- [x] Early-return
- [ ] Homólogo por composición
- [ ] Solo configuración
- [ ] Build/CI

## Archivos

- `crates/pumpkin/src/server/hcf.rs` — `HcfFeature`, `HcfFeatureSet`,
  `HcfProfile`, helpers.
- `crates/pumpkin/src/server/mod.rs` — accesores y campo.
- `crates/pumpkin-config/src/hcf.rs` — claves que alimentan el perfil.
- `crates/pumpkin/src/main.rs` — log de perfil/capacidades.

## Compatibilidad

- APIs públicas: aditivas; no cambia firmas existentes.
- Plugins v0.1: no se toca WIT; `capabilities()` es interno hasta 030.
- Jugadores vanilla: sin cambios con `enabled = false`.
- Migración: no.

## Métricas

- **Antes:** sin perfil no hay A/B.
- **Después:** permite medir `enabled = false/true` con el mismo binario.
- **Método:** arranque A/B; verificar que las features activas del log coinciden
  con la config.

## Tests

- [ ] Unitario: cada `HcfFeature` tiene un bit único y cabe en `u64`.
- [ ] Unitario: `enabled = false` ⇒ `disabled`/`fast_path` falsos y colisiones
      vanilla.
- [ ] Unitario: `hcf-1.8` ⇒ bits esperados; overrides cambian solo su bit.
- [ ] Unitario: `capabilities()` = features activas; `0` si HCF off.
- [ ] Unitario: colisiones off ⇒ `player_*_collisions_enabled()` falsos; on ⇒
      verdaderos.
- [ ] Integración: perfil off ejecuta la ruta vanilla (test de 002).
- [ ] Compatibilidad: sin cambios de WIT.
- [ ] Soak/loadtest: N/A.
- [ ] Suite ejecutada y verde en T1.

## Rollback

`hcf.enabled = false`; el perfil desaparece de la ruta caliente. No hay datos
que migrar. Si un parche consumidor falla, se puede apagar solo su feature sin
revertir el binario.

## Notas de rebase

- Commit base: `204a94e`.
- Archivos con conflicto probable: `server/mod.rs`, `server/hcf.rs`.
- Orden: después de 001; antes de 002 y de todos los consumidores. `server/mod.rs`
  se congela tras 001/002/005.

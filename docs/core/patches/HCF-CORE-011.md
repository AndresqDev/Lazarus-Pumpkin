# HCF-CORE-011 — Clima desactivable

- **Estado:** implementado (C1 pendiente)
- **Fase:** 1
- **Upstreamable:** sí (early-return genérico)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`World::tick_environment` ejecuta `Weather::tick_weather` cada tick, incluido
el ciclo de lluvia/tormenta, rayos y el reset de clima en el salto de noche.
HCF no usa clima; el coste no aporta gameplay y añade aleatoriedad al tick.

## Cambio

1. Extraer `World::tick_weather()` con el bloque original:
   lock de `weather`, `tick_weather(self)` y lectura de
   `raining`/`thundering`/`weather_cycle_enabled`.
2. En `tick_environment`, si `HcfFeature::Weather` está desactivada, usar
   `(false, false)` y no tocar el lock de `weather`.
3. El tiempo del mundo, el autosave, `should_skip_night`, el envío de tiempo y
   el despertar de jugadores siguen ejecutándose.
4. El reset de clima del salto de noche queda desactivado junto con el clima.

## Archivos

- `crates/pumpkin/src/world/mod.rs` (`tick_environment`, `tick_weather`).

## Compatibilidad

- Con `hcf.enabled = false` o `disable_weather = false` se ejecuta exactamente
  el bloque vanilla.
- El estado existente de `Weather` no se borra; simplemente deja de avanzar.
- No se envían paquetes de clima nuevos. Si un mundo ya estaba lloviendo, los
  clientes conservan el estado visual hasta el siguiente cambio de dimensión o
  reconexión.
- No cambia el formato de guardado ni la configuración.

## Métricas

- `HcfSection::Environment` sigue midiendo el tick completo.
- Se espera una reducción pequeña del coste de `tick_environment`; se valida
  con el A/B de B1, no con una cifra asumida.

## Tests

- T1: con HCF off, el clima avanza igual que vanilla.
- T1: con HCF on y `disable_weather = true`, el tiempo avanza y
  `Weather::raining`/`thundering` no cambian.
- T1: salto de noche con clima off no llama a `reset_weather_cycle`.

## Rollback

`hcf.disable_weather = false` o `hcf.enabled = false`. El helper se conserva
para que el rollback sea solo de configuración.

## Estado de implementación (2026-10-20)

- Implementado en `hcf/core-011-012`, commit `520fe81`, mergeado en
  `hcf/foundation`.
- El helper se llama `World::tick_weather`; el guard está en
  `tick_environment` y usa `HcfFeature::Weather`.
- C1/T1 pendientes; no se ejecutó compilación local.

# HCF-CORE-012 — Raids desactivables

- **Estado:** implementado (C1 pendiente)
- **Fase:** 1
- **Upstreamable:** sí (early-return genérico)
- **Riesgo:** bajo
- **Autor:** equipo HCF
- **Fecha:** 2026-10-20

## Problema

`World::tick` saca el `Raids` completo del mutex, ejecuta `raids.tick(self)` y
vuelve a insertarlo cada tick. HCF no usa raids ni aldeas; el trabajo de IA,
patrullas y barridos de chunks no aporta gameplay.

## Cambio

1. Extraer el bloque inline a `World::tick_raids()`.
2. Si `HcfFeature::Raids` está desactivada, retornar sin hacer `mem::take`,
   sin `raids.tick` y sin reinsertar.
3. Si hay raids existentes en el primer tick desactivado, emitir un `warn!`
   una sola vez y conservar los datos; no se borran ni se modifican.
4. La sección `HcfSection::Raids` sigue midiendo la llamada al helper.

## Archivos

- `crates/pumpkin/src/world/mod.rs` (`tick`, `tick_raids`,
  `raids_disabled_warned`).

## Compatibilidad

- Con `hcf.enabled = false` o `disable_raids = false` se ejecuta exactamente el
  bloque vanilla, incluido el `mem::take`, el tick y la reinserción.
- Los raids existentes no se tickean mientras el feature esté off. No se
  eliminan del mapa ni del guardado.
- No se cambia el formato de datos ni la API pública.

## Métricas

- `HcfSection::Raids` permite comparar el coste antes/después.
- La ganancia depende de si el mundo tiene raids activos; en HCF normalmente
  es cero trabajo, pero se elimina el lock y el `Raids::tick` vacío.

## Tests

- T1: mundo sin raids, HCF off/on, no cambia el estado.
- T1: mundo con un raid existente, HCF on con raids off, el raid no avanza y no
  se pierde al guardar.
- T1: HCF off, el raid avanza como vanilla.

## Rollback

`hcf.disable_raids = false` o `hcf.enabled = false`. El helper se conserva.

## Estado de implementación (2026-10-20)

- Implementado en `hcf/core-011-012`, commit `520fe81`, mergeado en
  `hcf/foundation`.
- El warning usa `raids_disabled_warned: AtomicBool` para no repetirse.
- C1/T1 pendientes; no se ejecutó compilación local.

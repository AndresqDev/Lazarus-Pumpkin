# Relación con upstream y política de rebase

## Base actual

| Elemento | Valor |
|---|---|
| Repositorio upstream | `Pumpkin-MC/Pumpkin` |
| Tag/commit base | `0.2.0+26.3-26.51` (`204a94e`) |
| Rama de trabajo | `hcf` (o `hcf/<tema>` por parche) |
| Plugin compatible | Lazarus-Pumpkin `7ee9e40` |

Este archivo se actualiza cada vez que se rebasa la pila de parches.

## Estrategia

- La rama HCF se rebasa **semanalmente** sobre el tag/commit estable de
  Pumpkin, no sobre `main` sin probar.
- Cada `HCF-CORE-###` es un commit independiente; se puede reordenar, revertir
  o upstreamear por separado.
- No se usan merge commits para integrar upstream; se usa `rebase`.
- Los parches genéricos (métricas, batch de paquetes, scheduler, serializer)
  se intentan upstream primero.
- Los parches HCF-specific van al final y detrás de `HcfProfile`.
- Si un parche se descarta, se marca en `PATCHES.md` y se documenta el motivo.

## Resolución de conflictos

1. Conservar el código de upstream.
2. Reaplicar el dispatcher/helper HCF encima.
3. No borrar código nuevo de upstream para “ganar” el conflicto.
4. Si el conflicto es en una función que se iba a extraer, extraer de nuevo
   desde la versión upstream.
5. Ejecutar `cargo fmt`, `cargo clippy`, `cargo test`, build vanilla y build
   HCF.
6. Smoke test de carga con 100 bots.
7. Actualizar la tabla de base y las notas del parche.

## Archivos delicados

Estos archivos cambian con frecuencia y requieren revisión manual:

- `crates/pumpkin/src/world/mod.rs`
- `crates/pumpkin/src/entity/player.rs`
- `crates/pumpkin/src/world/entity_tracker.rs`
- `crates/pumpkin/src/server/mod.rs`
- `crates/pumpkin/src/server/scheduler.rs`
- `crates/pumpkin/src/plugin/loader/wasm/wasm_host/wit/v0_1/generated_packets.rs`
- `crates/pumpkin-plugin-wit/v0.1/*.wit`
- `Cargo.toml` (perfil de release)

## Reglas

- No editar archivos generados a mano; regenerar con la herramienta.
- No añadir dependencias sin justificar tamaño, licencia y mantenimiento.
- No cambiar el perfil de release sin A/B.
- No actualizar el tag base sin correr la matriz de loadtest.
- No publicar un binario HCF sin `PUMPKIN_BUILD_ID` y hash.

## Registro de rebases

| Fecha | Base anterior | Base nueva | Conflictos | Estado |
|---|---|---|---|---|
| 2026-10-20 | — | `204a94e` | — | Inicial |

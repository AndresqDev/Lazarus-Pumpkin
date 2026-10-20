# HCF-CORE-### — Título corto

- **Estado:** propuesto | en diseño | implementado | upstream | descartado
- **Fase:** 0 | 1 | 2 | 3 | 4 | 5
- **Upstreamable:** sí | no | parcial
- **Riesgo:** bajo | medio | alto
- **Autor:**
- **Fecha:**

## Problema

Qué cuesta CPU/MSPT, qué se rompe o qué falta. Incluir evidencia (`perf`,
métricas, traza) si existe.

## Cambio

Descripción del cambio, sin código completo.

## Patrón

- [ ] Homólogo por composición (`*_hcf`)
- [ ] Early-return (`if disabled { return; }`)
- [ ] API aditiva (`*_v2`, `*_batch`, `*_raw`)
- [ ] Solo configuración
- [ ] Build/CI

## Archivos

- `ruta/al/archivo.rs` — qué se toca.

## Compatibilidad

- ¿Cambia APIs públicas? No / Sí, aditiva / Sí, breaking (justificar).
- ¿Afecta plugins v0.1? No / Sí.
- ¿Afecta jugadores vanilla? No / Sí (justificar).
- ¿Requiere migración de config? No / Sí.

## Métricas

- **Antes:** MSPT p50/p95/p99, CPU, RSS, timeouts.
- **Después:** ...
- **Método:** A/B, 3 × 10 min, 100/200 bots, perfil off/on.

## Tests

Casos definidos en code-first; se implementan/ejecutan en T1:

- [ ] Unitario:
- [ ] Integración/gametest:
- [ ] Compatibilidad:
- [ ] Soak:
- [ ] Loadtest:
- [ ] Suite ejecutada y verde en T1.

## Rollback

Cómo se desactiva o revierte sin borrar datos. Si es config, indicar la clave.
Si es build, indicar el backup/hash.

## Notas de rebase

- Commit base de Pumpkin:
- Archivos con conflicto probable:
- Orden respecto a otros parches:

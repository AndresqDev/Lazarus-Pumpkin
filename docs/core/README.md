# Core HCF

Documentación de las modificaciones de rendimiento y compatibilidad que se
harán sobre **Pumpkin** para el caso de uso HCF de Lazarus.

El objetivo no es reescribir Pumpkin ni mantener un fork profundo. Es mantener
una **pila de parches pequeña, aditiva y documentada**, con una ruta vanilla
intacta y una ruta HCF opt-in.

## Estado

| Elemento | Valor |
|---|---|
| Core base | Pumpkin `0.2.0+26.3-26.51` (`204a94e`) |
| Plugin base | Lazarus-Pumpkin `7ee9e40` |
| Baseline de carga | B21: 100 bots, backend ~0.82 core, MSPT ~19 ms |
| Perfil HCF | `hcf-1.8` (propuesto, desactivado por defecto) |
| Estado del plan | Propuesto; Fase 0 pendiente de ejecución |

## Documentos

| Documento | Contenido |
|---|---|
| [HCF_CORE_PLAN.md](HCF_CORE_PLAN.md) | Plan maestro: fases, cambios, metas y gates |
| [PATCH_STANDARD.md](PATCH_STANDARD.md) | Reglas obligatorias para homólogos, early-returns, APIs y documentación |
| [PATCHES.md](PATCHES.md) | Registro vivo de parches y estado |
| [UPDATER.md](UPDATER.md) | Reemplazo de binario, reinicio, firma, health check y rollback |
| [UPSTREAM.md](UPSTREAM.md) | Base de Pumpkin, política de rebase y resolución de conflictos |

## Regla principal

Ningún cambio entra al core sin:

1. Un ID `HCF-CORE-###`.
2. Una ficha en `docs/core/patches/` (se crea al implementar).
3. Tests que demuestren que la ruta vanilla no cambió.
4. Una medición A/B antes/después.
5. Una nota de compatibilidad y rollback.

## Alcance

Este plan optimiza el **backend**. El coste de Velocity + ViaVersion para
clientes 1.8 es un proyecto separado; ningún parche del core elimina ese coste.
La decisión de atacar el proxy o un camino 1.8 nativo se toma en la Fase 4,
con números en la mano.

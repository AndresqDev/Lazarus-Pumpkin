# Estándar de parches del core HCF

Este documento define cómo se modifican el core de Pumpkin y los plugins
nativos/WASM para que el fork siga siendo actualizable. Es obligatorio para
cualquier cambio etiquetado `HCF-CORE-###`.

## 1. Reglas de oro

### 1.1 La ruta vanilla no se toca

La lógica original de Pumpkin debe seguir existiendo y ejecutándose cuando el
perfil HCF está desactivado. No se permite “optimizar” una función vanilla
cambiando su comportamiento por defecto. Sí se permite extraer helpers de forma
mecánica, siempre que la ruta vanilla los llame en el mismo orden y con la misma
semántica.

Patrón correcto:

```rust
// HCF-CORE-014: la ruta HCF se va antes; la vanilla sigue abajo sin cambios.
if self.hcf_profile().fast_chunks() {
    self.tick_chunks_hcf(server);
    return;
}

// ... ruta vanilla: helpers extraídos, mismo orden y misma semántica ...
```

Patrón incorrecto:

```rust
// No hacer: se cambia la semántica vanilla para todos.
self.tick_chunks_fast(server);
```

### 1.2 Homólogo por composición, no por copia

Un homólogo no es duplicar 200 líneas y cambiar dos condiciones. Es extraer las
fases existentes a helpers privados y componer una ruta HCF con las fases que
HCF necesita.

Ejemplo:

```rust
fn tick_chunks(self: &Arc<Self>, server: &Arc<Server>) {
    if self.hcf_profile().fast_chunks() {
        self.tick_chunks_hcf(server);
        return;
    }

    // Ruta vanilla: mismas llamadas, mismo orden.
    self.tick_scheduled_block_ticks(server);
    self.tick_fluid_ticks(server);
    self.tick_random_ticks(server);
    self.tick_natural_spawning(server);
    self.tick_inhabited_time(server);
}

fn tick_chunks_hcf(self: &Arc<Self>, server: &Arc<Server>) {
    self.tick_scheduled_block_ticks(server);
    self.tick_fluid_ticks(server);

    if !self.hcf_profile().disabled(HcfFeature::RandomTicks) {
        self.tick_random_ticks(server);
    }
    if !self.hcf_profile().disabled(HcfFeature::NaturalSpawning) {
        self.tick_natural_spawning(server);
    }
    if !self.hcf_profile().disabled(HcfFeature::InhabitedTime) {
        self.tick_inhabited_time_hcf(server);
    }
}
```

La extracción de helpers es un cambio mecánico y revisable. La ruta HCF se
construye encima; no se reescribe la vanilla.

### 1.3 Para deshabilitar, early-return

Si una fase completa no aplica, el helper debe salir arriba:

```rust
fn tick_weather(&self) {
    if self.hcf_profile().disabled(HcfFeature::Weather) {
        return;
    }

    // ... lógica original ...
}
```

Si el código está inline en una función más grande, **primero se extrae a un
helper** y luego se le pone el early-return. No se ponen `if` gigantes alrededor
de bloques existentes si eso aumenta el diff o dificulta el rebase.

### 1.4 APIs públicas: aditivas y versionadas

Nunca se cambia una firma existente, se elimina una función ni se añade una
variante a un enum WIT que los guests puedan estar matcheando de forma
exhaustiva.

Permitido:

- `send_packet` se queda igual.
- Se añade `send_packet_batch`, `send_block_changes_raw`,
  `send_player_info_batch`.
- Se añade una interfaz WIT `java-packets-v2` o `player-v2`.
- Se añade una capacidad detectable: `packet.batch.v1`.

No permitido:

- Cambiar `send_packet(packet)` por `send_packet(packet, options)`.
- Añadir un caso nuevo a un `enum` WIT v0.1.
- Cambiar el ABI de un plugin nativo sin subir versión.

### 1.5 Runtime primero; compile-time solo para módulos completos

La mayoría de los cambios deben ser **runtime y reversibles**:

```toml
[hcf]
enabled = true
disable_weather = true
disable_natural_spawning = true
```

El `#[cfg(feature = "hcf")]` se reserva para módulos que se pueden excluir
completos del binario (por ejemplo, IA de mobs o aldeas) y que no forman parte
de la ruta HCF. No se usa `cfg` para esconder un `if` en una función caliente:
eso complica el build y no aporta nada en runtime.

### 1.6 Los checks del perfil son baratos

`HcfProfile` se carga una vez y se guarda como `Arc<HcfProfile>` inmutable o con
flags atómicos. Los accesos deben ser `#[inline]` y sin locks:

```rust
#[inline]
pub fn disabled(&self, feature: HcfFeature) -> bool {
    // Solo desactiva si HCF está activo. Con `enabled = false` la ruta
    // vanilla debe seguir exactamente igual.
    self.enabled && self.disabled_features.contains(feature)
}

#[inline]
pub fn fast_path(&self, feature: HcfFeature) -> bool {
    self.enabled && self.enabled_features.contains(feature)
}
```

`disabled()` responde “HCF apaga esta fase”; `fast_path()` responde “HCF usa
este homólogo”. Un early-return con `disabled()` nunca debe dispararse con
`hcf.enabled = false`.

No se llama a `server.advanced_config` ni se lockea un `Mutex` dentro de un
bucle por jugador/entidad.

## 2. Nomenclatura

| Tipo | Convención | Ejemplo |
|---|---|---|
| Ruta interna HCF | sufijo `_hcf` | `tick_chunks_hcf` |
| Helper extraído de vanilla | nombre descriptivo, sin `_hcf` | `tick_scheduled_block_ticks` |
| API pública aditiva | sufijo `_v2`, `_batch`, `_raw` | `send_packet_batch` |
| Flag de perfil | `HcfFeature::PascalCase` | `HcfFeature::Weather` |
| Config | `[hcf]` + `snake_case` | `disable_weather` |
| Parche | `HCF-CORE-###` | `HCF-CORE-021` |

No se usa `_hcf` en APIs públicas que puedan upstreamearse. `_hcf` es para
código interno del core.

## 3. Documentación inline

Cada bloque no trivial lleva un comentario con el ID:

```rust
// HCF-CORE-014: early-return de la fase de clima.
// La ruta vanilla sigue en tick_environment().
if self.hcf_profile().disabled(HcfFeature::Weather) {
    return;
}
```

Cada módulo nuevo lleva un doc-comment `//!` que explica:

- Qué problema resuelve.
- Si es ruta HCF, vanilla o compartida.
- Qué config lo controla.
- Qué test lo cubre.

## 4. Ficha obligatoria por parche

Al implementar un parche se crea
`docs/core/patches/HCF-CORE-###.md`. La plantilla viva está en
[`patches/_TEMPLATE.md`](patches/_TEMPLATE.md); el contenido mínimo es:

```markdown
# HCF-CORE-021 — Título corto

- Estado: propuesto | en diseño | implementado | upstream | descartado
- Fase: 0 | 1 | 2 | 3
- Upstreamable: sí | no | parcial
- Riesgo: bajo | medio | alto
- Autor:
- Fecha:

## Problema

## Cambio

## Patrón

- [ ] Homólogo por composición
- [ ] Early-return
- [ ] API aditiva
- [ ] Solo configuración
- [ ] Build/CI

## Archivos

## Compatibilidad

## Métricas

- Antes:
- Después:
- Método:

## Tests

## Rollback

## Notas de rebase
```

## 5. Definition of Done

En modo **code-first**, un commit de implementación no es “Done”: es “código
entregado, pendiente de C1/T1”. El agente deja en la ficha los casos de test que
hay que cubrir, pero no está obligado a ejecutarlos en la oleada de
implementación. Un parche no está terminado hasta que:

- [ ] `cargo fmt --check` pasa.
- [ ] `cargo clippy` no añade warnings nuevos.
- [ ] `cargo test` pasa.
- [ ] El build vanilla sigue compilando y pasando tests.
- [ ] El build HCF compila.
- [ ] Un test demuestra que con el perfil apagado el comportamiento es el
      original.
- [ ] Hay medición A/B o, si no aplica, una justificación.
- [ ] La ficha del parche está actualizada.
- [ ] `PATCHES.md` está actualizado.
- [ ] Hay nota de rollback.
- [ ] Si toca API de plugins, el test de compatibilidad v0.1 pasa.

## 6. Rebase y upstream

- La rama HCF se rebasa **semanalmente** sobre el tag/commit de Pumpkin.
- Cada parche es un commit independiente y se puede reordenar.
- Los parches genéricos (métricas, batch de paquetes, scheduler) se intentan
  upstream primero.
- Los parches HCF-specific van al final y detrás del perfil.
- Si hay conflicto, se conserva el código de upstream y se reaplica el
  dispatcher/helper; no se “gana” el conflicto borrando código nuevo.
- No se editan archivos generados a mano; se regeneran con la herramienta.
- `docs/core/UPSTREAM.md` (cuando exista) registra el commit base y los
  conflictos conocidos.

## 7. Prohibiciones

- No duplicar funciones grandes.
- No cambiar locks/allocator “porque sí”; solo con `perf` que lo justifique.
- No desactivar redstone, fluidos, TNT o pistones sin confirmar que HCF no los
  usa.
- No meter `unsafe` por microganancias.
- No añadir dependencias nuevas sin justificar tamaño, licencia y mantenimiento.
- No hacer que el plugin reemplace binarios sin firma, backup y rollback.
- No medir solo TPS medio; usar MSPT p50/p95/p99, CPU y timeouts.

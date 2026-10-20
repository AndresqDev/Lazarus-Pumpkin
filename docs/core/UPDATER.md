# Actualización del binario del core desde el plugin

## 1. Decisión

Se acepta que el plugin de Lazarus pueda **verificar la versión de Pumpkin,
instalar un binario HCF parcheado y reiniciar** para comodidad de despliegue.
El plugin publicará una advertencia clara: esta función reemplaza el ejecutable
del servidor y debe usarse solo con artefactos firmados y de confianza.

Aun así, la implementación **no debe ser un `exec` arbitrario dentro de
`on_load`**. El diseño separa tres responsabilidades:

1. **Plugin**: comprueba versión/capacidades y solicita la actualización.
2. **Core**: verifica firma/hash, prepara el artefacto local, hace backup y
   solicita un reinicio ordenado.
3. **Launcher/supervisor**: arranca el binario nuevo, comprueba salud y hace
   rollback si falla.

El objetivo es que el usuario solo instale el plugin y active
`[hcf.updater] enabled = true`, pero con red de seguridad.

## 2. Arquitectura

```
┌──────────────────────────┐
│ Plugin Lazarus (WASM)    │
│  - lee BUILD_ID          │
│  - valida HCF_ABI        │
│  - llama core-update     │
└────────────┬─────────────┘
             │ host API (permiso core:update)
┌────────────▼─────────────┐
│ Core Pumpkin             │
│  - verifica manifiesto   │
│  - lee artefacto local   │
│  - SHA-256 + firma       │
│  - backup + rename       │
│  - guarda pending.json   │
│  - shutdown + exit 75    │
└────────────┬─────────────┘
             │ supervisor / launcher
┌────────────▼─────────────┐
│ pumpkin-hcf launcher     │
│  - lee pending.json      │
│  - arranca binario nuevo │
│  - health check          │
│  - rollback si falla     │
└──────────────────────────┘
```

Si el host no puede hacer operaciones de fichero/proceso por el sandbox WASM, o
si el WASM no puede manejar la descarga de un binario grande, se usa un plugin
nativo pequeño `lazarus-updater.so`. Ese `.so` **solo** descarga, verifica y hace
operaciones de fichero/proceso; no contiene lógica HCF ni se enlaza contra el
core. El plugin WASM sigue siendo el cerebro.

## 3. Manifiesto

Formato recomendado: JSON canónico, firmado con Ed25519.

```json
{
  "schema": 1,
  "target_build_id": "hcf-0.2.0+26.3-26.51-20261020",
  "pumpkin_version": "0.2.0+26.3-26.51",
  "hcf_abi": 1,
  "plugin_api": 2,
  "artifact": "pumpkin-hcf-linux-x86_64",
  "sha256": "…",
  "min_current_build": "hcf-0.2.0+26.3-26.51-20261001",
  "created_at": "2026-10-20T00:00:00Z",
  "expires_at": "2026-11-20T00:00:00Z",
  "signature": "base64(ed25519(canonical_json_without_signature))"
}
```

Reglas:

- La clave pública está pinneada en la config del plugin/core, no en el
  manifiesto.
- `sha256` se verifica sobre el artefacto descargado.
- `expires_at` evita replay de manifiestos viejos.
- `min_current_build` impide downgrade accidental.
- `hcf_abi` y `plugin_api` deben coincidir exactamente con el runtime.
- Tamaño máximo de artefacto configurable; se rechaza lo que lo supere.
- El manifiesto y el artefacto se guardan en `updates/` con nombres derivados
  del `target_build_id`, nunca de una ruta arbitraria del manifiesto.

## 4. API del core

Añadir una capacidad de host, no acceso libre al filesystem:

```text
interface core-update {
  record manifest {
    target-build-id: string,
    pumpkin-version: string,
    hcf-abi: u32,
    plugin-api: u32,
    artifact: string,
    sha256: string,
    min-current-build: string,
    created-at: string,
    expires-at: string,
    signature: string,
  }

  request-update: func(manifest: manifest) -> result<update-ticket, string>;
  update-status: func(ticket: update-ticket) -> update-status;
}
```

El host:

- Exige el permiso `core:update` (y `core:restart` para el reinicio).
- Solo lo permite a plugins allowlisted en `[hcf.updater] allowed_plugins`.
- Rechaza si `[hcf.updater] enabled = false`.
- Resuelve `artifact` como basename dentro de `updates/`; rechaza `..`, rutas
  absolutas, symlinks y cualquier fichero fuera de ese directorio.
- No descarga URLs. La descarga la hace el plugin/updater con su permiso de red.
- Registra todo en `logs/core-updater.log` con build actual, objetivo, hash,
  resultado y motivo de rechazo.

`request-update` no reinicia inmediatamente. Devuelve un ticket y el plugin
decide si programa el reinicio (`core:restart`) o solo deja el artefacto
preparado.

## 5. Flujo de actualización

1. El plugin lee `PUMPKIN_BUILD_ID`, `PUMPKIN_VERSION`, `PUMPKIN_HCF_ABI` y
   `PUMPKIN_CAPABILITIES`.
2. Compara con el manifiesto. Si el build actual ya es el objetivo, no hace
   nada.
3. Si falta `core:update`, avisa y no intenta nada.
4. El plugin/updater descarga el artefacto a `updates/<build>.part` con permiso
   de red; el core **no descarga URLs**. El core valida que el nombre sea un
   basename seguro dentro de `updates/`, que el tamaño no supere el máximo y que
   SHA-256/firma coincidan.
5. El core mueve el artefacto validado a `updates/<build>.bin`; si hay que
   escribir, usa `fsync` y `rename` atómico.
6. Copia el binario actual a `updates/backup-<build-anterior>.bin` y guarda
   `updates/pending.json` con el objetivo, backup y hash.
7. Si el plugin pidió reinicio, el core:
   - Comprueba que no haya jugadores o que `allow_restart_with_players=true`.
   - Guarda el mundo (`Server::save_all()`), desconecta con mensaje y hace
     shutdown ordenado.
   - Pone `SERVER_EXIT_CODE = 75` (`EX_TEMPFAIL`) y llama a `stop_server()`.
8. El launcher/supervisor ve el código 75, lee `pending.json`, sustituye el
   binario de forma atómica y arranca el nuevo.
9. El nuevo proceso escribe `updates/health.ok` tras:
   - Bind del puerto.
   - Carga de plugins.
   - 100 ticks sin panic.
10. Si no hay `health.ok` en `health_timeout_seconds`, el launcher mata el
    proceso y restaura el backup. Si hay 3 fallos consecutivos, se queda en el
    backup y no reintenta.

## 6. Modos de reinicio

### 6.1 Launcher/supervisor (recomendado)

`pumpkin-hcf` es el `ExecStart` de systemd/Docker. El plugin solo escribe
`pending.json` y sale con 75. El launcher hace el swap. Es la opción más segura
y la que funciona igual en Linux y Windows.

### 6.2 Plugin-only (Linux)

Si no hay launcher, el core puede lanzar un helper detached:

```text
pumpkin-restart --pid <pid> --binary <nuevo> --backup <viejo> --health <archivo>
```

El helper espera a que el PID termine, hace el swap y arranca el nuevo proceso.
En Windows no se puede reemplazar el ejecutable en caliente; este modo se
rechaza y se pide launcher o reinicio manual.

### 6.3 Sin reinicio

`request-update` puede dejar el artefacto preparado y el admin reinicia cuando
quiera. Útil para no reiniciar con jugadores online.

## 7. Seguridad y defaults

```toml
[hcf.updater]
enabled = false
allow_restart_with_players = false
allowed_plugins = ["lazarus"]
trusted_public_key = "…"
max_artifact_mb = 200
health_timeout_seconds = 120
max_consecutive_failures = 3
allow_downgrade = false
require_signature = true
require_empty_server = true
```

- `enabled = false` por defecto.
- `require_signature = true`; no se permite desactivar en el plugin publicado.
- `allow_restart_with_players = false` por defecto. Si se activa, el plugin
  muestra una advertencia y hace countdown/kick.
- El core no acepta URLs ni rutas absolutas desde el manifiesto; solo un
  basename dentro de `updates/`. La descarga HTTPS, si existe, la hace el
  plugin/updater con `network:outbound` y allowlist de host.
- No se ejecuta shell. El helper recibe rutas y argumentos ya validados.
- El binario nuevo se escribe en el mismo filesystem que el actual para que el
  `rename` sea atómico.
- Se conservan al menos dos backups.
- El `pending.json` incluye un token de health check para que un proceso viejo
  no marque salud por error.

## 8. Advertencia pública del plugin

Texto sugerido para el README/descripción del plugin:

> **Aviso:** Lazarus puede descargar e instalar una build parcheada de Pumpkin
> y reiniciar el servidor para aplicarla. Esta función está desactivada por
> defecto, exige manifiestos firmados y conserva un backup con rollback. No la
> actives si no confías en el origen del artefacto o si tu proveedor de hosting
> no permite reemplazar el ejecutable. Con jugadores online, el reinicio solo
> ocurre si lo autorizas explícitamente.

## 9. Tests obligatorios

- Manifiesto válido + artefacto válido → actualiza y arranca.
- Firma inválida → rechaza, no toca el binario.
- SHA-256 inválido → rechaza, no toca el binario.
- Manifiesto expirado → rechaza.
- `hcf_abi`/`plugin_api` incompatibles → rechaza.
- Artefacto demasiado grande → rechaza.
- Filesystem read-only → error claro, servidor sigue corriendo.
- Fallo de health check → rollback al backup.
- 3 fallos consecutivos → queda en backup y no reintenta.
- Reinicio con jugadores sin permiso → rechaza.
- Reinicio con jugadores con permiso → guarda, kick con mensaje, exit 75.
- Windows sin launcher → rechaza el modo plugin-only.
- Plugin sin permiso `core:update` → rechaza.

## 10. Riesgos aceptados

- El binario nuevo no se puede validar en ejecución antes de reiniciar; por eso
  el health check y el rollback son obligatorios.
- Reemplazar el ejecutable desde un plugin es una capacidad sensible. Se
  mitiga con firma, allowlist, opt-in, backups y logs.
- El modo plugin-only es Linux-only y depende de permisos del filesystem.
- Un artefacto firmado pero defectuoso puede tumbar el servidor; el rollback
  reduce el impacto, no lo elimina.

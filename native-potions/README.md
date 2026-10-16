# native-potions — native potion-component factory

Status: **redesigned 2026-10-16** — no server patches, no server rebuild. This
`.so` is a regular Pumpkin native plugin that only produces *data*; the WASM
plugin applies it through Pumpkin's built-in `set-component` path.

The WASM plugin gives potion items as plain potions carrying a **marker** in
their lore. Right before the item is handed to the player, the marker is
detected and the item is converted into the **real potion** (potion type taken
from the marker). The player receives directly the indicated potion, never the
plain one, and never sees the marker.

## Why the heavy lifting is native (root cause of the old crash)

Pumpkin's WASM API has no typed way to *build* the `potion-contents` component:
the only setter, `item.set-component`, takes the network codec's binary form
(`list<u8>`), which a guest cannot produce. And a separate `.so` cannot inject a
component object either — the old design
(`stack.patch.push((DataComponent::PotionContents, Some(Box::new(PotionContentsImpl {…})))))`)
**panicked the server** as soon as the item was serialised for the client
(`crash-2026-10-16_20.44.02.txt`): the `.so` statically links its own copy of
`pumpkin-data`, so the plugin's `Box<dyn DataComponentImpl>` failed the server's
`TypeId` downcast — `you are trying to cast minecraft:potion_contents …`.
**Trait objects cannot cross the dylib boundary; bytes and data can.**

The only path that produces components with *server* types is server code. The
one built-in server code path that does it for third parties is the WASM host's
`set-component`, which **deserialises the value with server types**. So:

| Piece | Where | Job |
|---|---|---|
| `native_potions` | `plugins/native_potions.so` | Builds the network-codec bytes for **every** potion, writes the lookup table (heavy/version-sensitive part) |
| WASM plugin | `lazarus_pumpkin.wasm` | Detects the marker in the item's lore, looks the potion up, calls `set-component` with the bytes |

The server binary is **not** modified or rebuilt; only its built-in
`set-component` (which itself is what the server uses to sync items) is used.

```
kits::give()
  └─ builds item + appends marker lore line
       └─ potion_components::apply_marker(item)        (WASM)
            ├─ marker found in lore?  -> lookup <data_folder>/potion_components.json
            ├─ item.set-component(potion-contents, <bytes>)   (host deserialises, server types)
            └─ marker line removed
       └─ player.set_inventory_item(...)   -> client sees the real potion
```

## Marker protocol

When the WASM plugin gives a potion it appends one lore line whose **plain text**
is the marker JSON (wrapped as a text component, invisible to players because it
is stripped during conversion):

```jsonc
// lore line (as JSON text component):
{"text":"{\"lzp\":\"eW9zb3llbHB1Z2lu\",\"potion\":\"strong_healing\"}"}

// the marker JSON itself:
{
  "lzp": "eW9zb3llbHB1Z2lu", // base64 of the plugin's binary code: "yosoyelplugin"
  "potion": "strong_healing" // modern potion id (as accepted by Potion::from_name)
}
```

* `lzp` must base64-decode to **exactly** `b"yosoyelplugin"` (the specific binary
  code that identifies this plugin); anything else is ignored.
* `potion` is applied as the item's `potion-contents` component
  (`strong_healing`, `long_swiftness`, …). Splash/lingering comes from the item
  itself (`splash_potion` / `lingering_potion`), not from the marker.
* The marker line is removed when the item is converted.

Constants to keep in sync:

| Constant | Value | Files |
|---|---|---|
| binary code | `yosoyelplugin` | `native-potions/src/lib.rs` (`.so`), WASM `src/potion_components.rs` |
| base64 | `eW9zb3llbHB1Z2lu` | WASM `src/kits.rs` (`POTION_MARKER_B64`), `native-potions/src/lib.rs`, docs |
| table version | `1` | `native-potions/src/lib.rs`, WASM `src/potion_components.rs` |

## Native table

At server start the `.so` writes, next to the WASM plugin's data:

`plugins/data/lazarus/potion_components.json`
```jsonc
{
  "version": 1,
  "lzp": "eW9zb3llbHB1Z2lu",
  "potion_components": {
    "strong_healing": "<base64 of the component's network bytes>",
    // …every potion in the registry (46 in 26.3)
  }
}
```

Each entry is the output of `pumpkin_protocol::codec::data_component::serialize`
for `PotionContentsImpl { potion_id: Some(id), … }` — the exact format the host's
`set-component` deserialises. The `.so` round-trips every entry through the codec
as a self-check before writing (and again in its unit tests).

## Build / deploy

Build from the same Pumpkin tag as the server binary (the byte layout is stable
inside a tag). Only the plugin target is compiled — the server binary is not
built nor touched:

```bash
bash native-potions/build-in-pumpkin.sh [workdir]   # default: /tmp/pumpkin-native
```

Deploy only the `.so`:

```bash
cp <work>/target/<triple>/release/libnative_potions.so <server-dir>/plugins/native_potions.so
```

Tests (inside the workspace clone):

```bash
cargo test --release -p native_potions   # registry coverage + codec round-trip
```

## Degraded modes

* `.so` missing / table not written yet → a warning is logged and marked items
  are delivered untouched (plain potion + visible marker line), retrying the
  table load on the next give. The WASM plugin never panics.
* Unknown `potion` id → warning, item given untouched (marker stays, debuggable).
* The `.so` only writes a file — it cannot take the server down.

## Files

- `src/lib.rs` — the `.so`: builds the component table for the whole registry and
  writes it on load.
- `build-in-pumpkin.sh` / `.github/workflows/native-potions.yml` — build + CI
  (plugin target only).
- WASM side: `src/kits.rs` (`potion_marker_line`, marker constant) and
  `src/potion_components.rs` (`apply_marker`, table loader, base64 validation).

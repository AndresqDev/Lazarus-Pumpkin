# native-potions — bridge for the item `potion-contents` component

Status: **disabled** (the current implementation crashes the server, see below).
Do not deploy `native_potions.so` until the byte-based design is implemented.

## Why a native bridge

Pumpkin's WASM API cannot set an item's `potion-contents` data component:
the only setter is `set-component(component, value: list<u8>)`, and the host
decodes `value` with `pumpkin_protocol::codec::data_component::deserialize`
(the network codec's binary form), which the WASM guest cannot produce.

So an item given by the plugin (e.g. a kit) can only be a *plain* potion
(no effect).

## Why the direct approach crashes (root cause)

The first attempt had the native plugin patch the item directly:

```rust
stack.patch.push((
    DataComponent::PotionContents,
    Some(Box::new(PotionContentsImpl { potion_id: Some(id), .. })),
));
player.inventory().set_slot(slot, stack);
```

This panics the **server**:

```
Thread 'Rayon-Worker-2' panicked at crates/pumpkin-data/src/data_component_impl/mod.rs:44:
you are trying to cast minecraft:potion_contents (...) to
minecraft:potion_contents (pumpkin_data::data_component_impl::food::PotionContentsImpl)
```

Because the native plugin is a **separate `.so`**: it statically links its own
copy of `pumpkin-data`, so `PotionContentsImpl` has a **different `TypeId`** than
the server's. Passing a `Box<dyn DataComponentImpl>` (a trait object with the
plugin's vtable) into the server's inventory makes the server's
`downcast_ref::<PotionContentsImpl>()` fail. **Trait objects cannot cross the
dylib boundary.**

## Correct design (byte-based)

Bytes are type-agnostic, so the native produces them and the WASM applies them
through the host (which deserializes into the *server's* type):

1. **Native** (on load): for every potion type, build the component and
   `pumpkin_protocol::codec::data_component::serialize` it, then write a JSON map
   of base64 bytes, e.g. `plugins/data/lazarus/potion_components.json`:
   ```jsonc
   { "strong_healing": "<base64 bytes>", "swiftness": "<base64 bytes>", ... }
   ```
2. **WASM**: when giving a potion item, look up the bytes and call
   `item.set_component(pumpkin_plugin_api::data_components::DataComponent::PotionContents, bytes)`
   — the host `deserialize`s them into its own `PotionContentsImpl`.

This keeps all trait objects on the side that created them.

## Files

- `src/lib.rs` — native companion (currently: reads `potion_requests.json` and
  patches the item → **this is the crashing path**, to be replaced by the byte
  producer).
- `.github/workflows/native-potions.yml` — CI (build inside Pumpkin's workspace).
- WASM side: `src/potion_api.rs` (request writer) + `kits::give`.

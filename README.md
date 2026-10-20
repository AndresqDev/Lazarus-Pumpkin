# Lazarus — Pumpkin port

A port of the **Lazarus** advanced HCF core (originally a Bukkit/Spigot 1.8 plugin)
to the [Pumpkin](https://pumpkinmc.org) Minecraft server, written as a Rust
WebAssembly component against the [Pumpkin plugin API](https://docs.pumpkinmc.org).

> Pumpkin plugins are compiled to `.wasm` components — Java/Bukkit plugins cannot
> run on Pumpkin. This repository is a **rewrite** of the Lazarus feature set, not
> a recompilation. See [`PORTING.md`](./PORTING.md) for the full mapping.
>
> The core performance and compatibility plan lives in
> [`docs/core/HCF_CORE_PLAN.md`](./docs/core/HCF_CORE_PLAN.md).

## Features (ported)

* **Factions** — create, disband, invite, join, leave, kick, promote/demote,
  transfer leadership, rename, open/closed, ally/unally.
* **Claims** — chunk-based claiming with overlap, buffer, connectivity, per-faction
  limits and price; enemy building only when a faction is raidable.
* **DTR** — per-faction DTR, death loss per dimension, regeneration, freeze state.
* **Territory** — wilderness / warzone / safezone / faction resolution with
  enter messages.
* **Timers** — combat tag, PvP protection, golden apple, enderpearl, home, stuck
  (warmups cancel on movement), all shown on the scoreboard.
* **Factions chat** — public, faction and ally channels.
* **Economy** — internal player/faction balance (`/f deposit`, `/f withdraw`,
  `/f show`).
* **Sidebar scoreboard** with DTR, online members, stats and active timers
  (configurable via `scoreboard.toml` with placeholders).
* **Tab list** with configurable header/footer and coloured names (`tab.toml`).
* **Client-side claim rendering** — selection pillars, claim maps and combat/PvP
  border walls sent as fake blocks (`send_block_change`), never touching the world.
* **Selection wand** — `/f claim` gives a wand: left/right click to set corners,
  sneak + left-click to purchase.
* **Deathbans & lives** — `/lives`, deathban storage on userdata.
* **Kits** — capture/give inventories with lore + vanilla enchantments.
* **Holograms** — `TextDisplay`-based, managed with `/hologram`.
* **Vanish & staff mode** — `/vanish`, `/staffmode`.
* **Setting toggles** — `/togglescoreboard`, `/togglechat`, `/togglefoundore`,
  `/togglemessages`, `/togglesounds`, `/toggledeathmessages`, `/togglecobble`,
  `/togglelightning`.

## Omitted (as requested)

* Knockback configuration.
* LunarClient integration (waypoints, team view, nametags, cooldown packets).

## Build

```bash
rustup target add wasm32-wasip2
cargo build --release
```

The plugin binary is produced at:

```
target/wasm32-wasip2/release/lazarus_pumpkin.wasm
```

## Install

1. Copy `lazarus_pumpkin.wasm` into the server's `plugins/` directory.
2. Allow filesystem permissions and unsigned plugins in `pumpkin.toml`:

   ```toml
   [plugins]
   enabled = true
   allow_unsigned = true
   allowed_permissions = ["fs.read.data", "fs.write.data"]
   ```

3. Start the server. On first run the plugin creates `config.toml`, `lang.toml`,
   `factions.json`, `userdata.json`, `claims.json` and `timers.json` in its
   private data folder.

## Commands

| Command | Description |
| :--- | :--- |
| `/f create <name>` | Create a faction |
| `/f disband` | Disband your faction |
| `/f invite <player>` / `/f join <name>` | Invite / join |
| `/f leave` / `/f kick <player>` | Leave / kick |
| `/f promote` / `/f demote` / `/f leader` | Manage ranks |
| `/f rename <name>` / `/f open` | Rename / toggle open |
| `/f ally <faction>` / `/f unally <faction>` | Diplomacy |
| `/f claim` / `/f unclaim` / `/f unclaimall` | Manage claims (wand-based claiming) |
| `/f sethome` / `/f home` / `/f stuck` | Teleportation |
| `/f map` | Toggle the client-side claim map |
| `/f chat` | Cycle public → faction → ally chat |
| `/f show [faction]` / `/f list` / `/f top [metric]` | Information |
| `/f deposit <amount>` / `/f withdraw <amount>` | Faction bank |
| `/f setdtr\|setpoints\|setbalance\|setlives <faction> <value>` | Admin faction management |
| `/f forcekick\|forceleader <player>` / `/f freeze\|thaw <faction>` / `/f save` | Admin faction management |
| `/lives [add\|set\|revive <player> <amount>]` | Lives / revive |
| `/kit [name]` / `/kit create\|remove\|delay\|give ...` | Kits |
| `/vanish` / `/staffmode` | Staff visibility |
| `/hologram create\|delete\|addline\|removeline\|tphere\|list` | Holograms |
| `/lazarus` / `/lazarus reload` / `/lazarus save` | Plugin management |

## Configuration

`config.toml` mirrors `config.yml` (DTR values, claims, faction limits, warzone
radii, relation colours, timers, economy, deathban). `lang.toml` mirrors the
messages from `language.yml` using the same `&`-colour codes and `<placeholder>`
tokens. `tab.toml` and `scoreboard.toml` control the tab list and sidebar.

> **Legacy IDs:** the original used 1.8 numeric material IDs (`"294:0"`). Pumpkin
> is a modern server, so this port uses registry keys (e.g. `minecraft:diamond`)
> and does not ship the item/sign-shop systems yet.

## License / credits

Original Lazarus core by **qIooIp**. Pumpkin port by the community. See the
upstream project for the original license.

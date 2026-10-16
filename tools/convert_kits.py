#!/usr/bin/env python3
"""Convierte `original-configs/kits.yml` (Bukkit Java-serializado en base64) al
formato JSON del plugin (`kits.json`).

Uso:
    /tmp/kitsve/bin/python tools/convert_kits.py original-configs/kits.yml kits.json

Dependencias: `javaobj-py3`, `pyyaml`.
"""
import base64
import io
import json
import struct
import sys

import javaobj
import yaml

# Bukkit Material -> modern registry key.
MATERIAL_SPECIAL = {
    "GOLD_": "golden_",
    "WOOD_": "wooden_",
    "GRILLED_PORK": "cooked_porkchop",
    "COOKED_BEEF": "cooked_beef",
    "COOKED_CHICKEN": "cooked_chicken",
    "COOKED_FISH": "cooked_cod",
    "RAW_FISH": "cod",
    "RAW_BEEF": "beef",
    "RAW_CHICKEN": "chicken",
    "SLIME_BALL": "slime_ball",
    "SNOW_BALL": "snowball",
    "WATCH": "clock",
    "SEEDS": "wheat_seeds",
    "GRASS": "grass_block",
    "SULPHUR": "gunpowder",
    "NETHER_STALK": "nether_wart",
    "WORKBENCH": "crafting_table",
    "WOOD": "oak_planks",
    "LOG": "oak_log",
    "BOOK_AND_QUILL": "writable_book",
    "INK_SACK": "ink_sac",
    "EXP_BOTTLE": "experience_bottle",
    "EYE_OF_ENDER": "ender_eye",
    "GOLDEN_APPLE": "golden_apple",
    "LEASH": "lead",
    "FIREWORK": "firework_rocket",
    "FIREWORK_CHARGE": "firework_star",
    "FIREBALL": "fire_charge",
    "STORAGE_MINECART": "chest_minecart",
    "POWERED_MINECART": "furnace_minecart",
    "EXPLOSIVE_MINECART": "tnt_minecart",
    "HOPPER_MINECART": "hopper_minecart",
}

# Bukkit enchantment -> modern registry key.
ENCHANT_SPECIAL = {
    "PROTECTION_ENVIRONMENTAL": "protection",
    "PROTECTION_EXPLOSIONS": "blast_protection",
    "PROTECTION_FALL": "feather_falling",
    "PROTECTION_FIRE": "fire_protection",
    "PROTECTION_PROJECTILE": "projectile_protection",
    "DAMAGE_ALL": "sharpness",
    "DAMAGE_UNDEAD": "smite",
    "DAMAGE_ARTHROPODS": "bane_of_arthropods",
    "DIG_SPEED": "efficiency",
    "DURABILITY": "unbreaking",
    "LOOT_BONUS_BLOCKS": "fortune",
    "LOOT_BONUS_MOBS": "looting",
    "ARROW_DAMAGE": "power",
    "ARROW_KNOCKBACK": "punch",
    "ARROW_FIRE": "flame",
    "ARROW_INFINITE": "infinity",
    "WATER_WORKER": "aqua_affinity",
    "OXYGEN": "respiration",
    "SILK_TOUCH": "silk_touch",
    "SWEEPING_EDGE": "sweeping_edge",
}

# 1.8 potion damage -> modern potion type (best-effort).
POTION_BASE = {
    1: "regeneration",
    2: "swiftness",
    3: "fire_resistance",
    4: "poison",
    5: "healing",
    6: "night_vision",
    7: "weakness",
    8: "strength",
    9: "slowness",
    10: "leaping",
    11: "harming",
    12: "water_breathing",
    13: "invisibility",
}


def material_key(name):
    for prefix, repl in MATERIAL_SPECIAL.items():
        if name.startswith(prefix):
            return (repl + name[len(prefix):]).lower()
    if name in MATERIAL_SPECIAL:
        return MATERIAL_SPECIAL[name]
    return name.lower()


def enchant_key(name):
    return ENCHANT_SPECIAL.get(name, name.lower())


def potion_info(damage):
    """Returns (item_key, potion_id) for a 1.8 potion damage value."""
    if damage is None:
        return ("potion", None)
    splash = bool(damage & 0x4000)
    d = damage & ~0x4000
    upgraded = bool(d & 0x20)
    extended = bool(d & 0x40)
    base = d & 0x1F
    name = POTION_BASE.get(base)
    if name is None:
        return ("splash_potion" if splash else "potion", None)
    if upgraded:
        potion = f"strong_{name}"
    elif extended:
        potion = f"long_{name}"
    else:
        potion = name
    return ("splash_potion" if splash else "potion", potion)


def read_array(b64):
    """Reads a Bukkit-serialized ItemStack[] (TC_BLOCKDATA size + N objects)."""
    raw = base64.b64decode(b64)
    assert raw[:4] == b"\xac\xed\x00\x05", "not a java serialization stream"
    pos = 4
    assert raw[pos] == 0x77, f"expected block data, got {raw[pos]:#x}"
    pos += 1
    ln = raw[pos]
    pos += 1
    size = struct.unpack(">i", raw[pos:pos + ln])[0]
    pos += ln

    # Skip the block-data header: only the object stream (with the magic header).
    stream = b"\xac\xed\x00\x05" + raw[pos:]
    unmarshaller = javaobj.JavaObjectUnmarshaller(io.BytesIO(stream))
    out = []
    for _ in range(size):
        try:
            obj = unmarshaller.readObject()
        except Exception:
            break
        out.append(obj)
    return out


def to_num(v):
    """Unwraps a javaobj number (java.lang.Integer/Short/…) to a Python number."""
    if v is None:
        return None
    if hasattr(v, "value"):
        return int(v.value)
    try:
        return int(v)
    except Exception:
        return None


def parse_item(obj):
    m = getattr(obj, "map", None)
    if m is None:
        return None
    keys = list(m.keys)
    values = list(m.values)

    data = dict(zip(keys, values))
    mat = data.get("type")
    amount = to_num(data.get("amount")) or 1
    damage = to_num(data.get("damage"))

    name = None
    lore = []
    enchants = []
    meta = data.get("meta")
    if meta is not None:
        mm = getattr(meta, "map", None)
        if mm is not None:
            md = dict(zip(mm.keys, mm.values))
            if "display-name" in md:
                name = str(md["display-name"])
            if "lore" in md:
                lore = [str(line) for line in md["lore"]]
            ench = md.get("enchants")
            if ench is not None:
                em = getattr(ench, "keys", None)
                ev = getattr(ench, "values", None)
                if em is not None:
                    for k, v in zip(em, ev):
                        level = to_num(v)
                        if level is not None:
                            enchants.append([enchant_key(str(k)), level])

    key = material_key(str(mat))
    potion = None
    if str(mat) == "POTION":
        item_key, potion = potion_info(damage)
        key = item_key

    return {
        "key": f"minecraft:{key}",
        "count": amount,
        "name": name,
        "lore": lore,
        "enchants": enchants,
        "potion": potion,
        "legacy_damage": damage if str(mat) == "POTION" else None,
    }


def main(src, dst):
    data = yaml.safe_load(open(src))
    kits = {}
    for kit_name, kit in data.items():
        if not isinstance(kit, dict) or "type" not in kit:
            continue
        items = []

        contents = kit.get("contents")
        if contents:
            for slot, obj in enumerate(read_array(contents)):
                parsed = parse_item(obj)
                if parsed is None:
                    continue
                parsed["slot"] = slot
                items.append(parsed)

        armor = kit.get("armor")
        if armor:
            for i, obj in enumerate(read_array(armor)):
                parsed = parse_item(obj)
                if parsed is None:
                    continue
                parsed["slot"] = 36 + i  # boots, leggings, chestplate, helmet
                items.append(parsed)

        kits[kit_name.lower()] = {
            "name": kit_name,
            "kind": str(kit["type"]),
            "delay_seconds": int(kit.get("delay", 0)),
            "items": items,
        }

    json.dump({"kits": kits}, open(dst, "w"), indent=2)
    print(f"wrote {dst}: {len(kits)} kits, "
          f"{sum(len(k['items']) for k in kits.values())} items")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "original-configs/kits.yml",
         sys.argv[2] if len(sys.argv) > 2 else "kits.json")

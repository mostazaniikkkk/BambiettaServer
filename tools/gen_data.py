"""Generates the server's embedded data from PrismarineJS/minecraft-data.

Usage: python tools/gen_data.py <path to minecraft-data>/data
1.20.4 paths according to dataPaths.json (blocks: 1.20.4, items/entities/protocol: 1.20.3,
loginPacket: 1.20.2).
"""
import json
import os
import struct
import sys

src = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")
pc = lambda *p: os.path.join(src, "pc", *p)

TAG = {"end": 0, "byte": 1, "short": 2, "int": 3, "long": 4, "float": 5, "double": 6,
       "byteArray": 7, "string": 8, "list": 9, "compound": 10, "intArray": 11, "longArray": 12}


def s(v):
    b = v.encode("utf-8")
    return struct.pack(">H", len(b)) + b


def payload(t, v):
    if t == "byte": return struct.pack(">b", v)
    if t == "short": return struct.pack(">h", v)
    if t == "int": return struct.pack(">i", v)
    if t == "long": return struct.pack(">q", (v[0] << 32) | (v[1] & 0xFFFFFFFF) if isinstance(v, list) else v)
    if t == "float": return struct.pack(">f", v)
    if t == "double": return struct.pack(">d", v)
    if t == "string": return s(v)
    if t == "list":
        it, items = v["type"], v["value"]
        return struct.pack(">bi", TAG[it] if items else 0, len(items)) + b"".join(payload(it, x) for x in items)
    if t == "compound":
        r = b""
        for k, c in v.items():
            r += struct.pack(">b", TAG[c["type"]]) + s(k) + payload(c["type"], c["value"])
        return r + b"\x00"
    raise ValueError(t)


codec = json.load(open(pc("1.20.2", "loginPacket.json")))["dimensionCodec"]
# Network NBT (1.20.2+): unnamed root compound
open(os.path.join(out, "registry_codec.nbt"), "wb").write(b"\x0a" + payload("compound", codec["value"]))

biomes = codec["value"]["minecraft:worldgen/biome"]["value"]["value"]["value"]["value"]
biomes = sorted(biomes, key=lambda b: b["id"]["value"])
blocks = json.load(open(pc("1.20.4", "blocks.json")))
items = json.load(open(pc("1.20.3", "items.json")))
entities = json.load(open(pc("1.20.3", "entities.json")))
json.dump({
    "biomes": [b["name"]["value"] for b in biomes],
    "blocks": [[b["name"], b["minStateId"], b["maxStateId"], b["defaultState"],
                [[st["name"], st.get("values") or ["true", "false"]] for st in b.get("states", [])],
                b.get("filterLight", 15), b.get("emitLight", 0), b.get("transparent", False),
                b.get("hardness") if b.get("hardness") is not None else -1]
               for b in sorted(blocks, key=lambda b: b["id"])],
    "items": [i["name"] for i in sorted(items, key=lambda i: i["id"])],
    "entities": {e["name"]: e["id"] for e in entities},
}, open(os.path.join(out, "registries.json"), "w"), separators=(",", ":"))

# --- Gameplay data (survival) ---
item_names = {i["id"]: i["name"] for i in items}
damage_types = codec["value"]["minecraft:damage_type"]["value"]["value"]["value"]["value"]
shapes = json.load(open(pc("1.20.3", "blockCollisionShapes.json")))
shape_ids = sorted(shapes["shapes"], key=int)
shape_index = {sid: i for i, sid in enumerate(shape_ids)}
state_shapes = []
for b in sorted(blocks, key=lambda b: b["id"]):
    n = b["maxStateId"] - b["minStateId"] + 1
    v = shapes["blocks"].get(b["name"], 0)
    v = v if isinstance(v, list) else [v] * n
    state_shapes += [shape_index[str(x)] for x in v[:n]] + [shape_index["0"]] * (n - len(v))


def merge_recipes(lst):
    """Merges variants that only differ in the material of each slot (equivalent to tags)."""
    shaped, shapeless = {}, []
    for r in lst:
        res = [r["result"]["id"], r["result"]["count"]]
        if "inShape" in r:
            rows = r["inShape"]
            key = (len(rows), max(len(x) for x in rows), tuple(tuple(c is None for c in row) for row in rows), tuple(res))
            cells = [[c for c in row] + [None] * (key[1] - len(row)) for row in rows]
            if key in shaped:
                for y, row in enumerate(cells):
                    for x, c in enumerate(row):
                        if c is not None and c not in shaped[key][y][x]:
                            shaped[key][y][x].append(c)
            else:
                shaped[key] = [[[c] if c is not None else None for c in row] for row in cells]
        else:
            ing = r["ingredients"]
            merged = False
            if len(ing) == 1:
                for s in shapeless:
                    if s["r"] == res and len(s["i"]) == 1:
                        if ing[0] not in s["i"][0]:
                            s["i"][0].append(ing[0])
                        merged = True
                        break
            if not merged:
                shapeless.append({"i": [[x] for x in ing], "r": res})
    return [{"s": v, "r": list(k[3])} for k, v in shaped.items()] + shapeless


recipes = []
for lst in json.load(open(pc("1.20.3", "recipes.json"))).values():
    recipes += merge_recipes(lst)

loot = {}
for entry in json.load(open(pc("1.20", "blockLoot.json"))):
    drops = []
    for d in entry["drops"]:
        # silk: 1 = only with Silk Touch, -1 = only without it, 0 = always
        silk = 1 if d.get("silkTouch") else -1 if d.get("noSilkTouch") else 0
        chance = 1.0 if silk else d.get("dropChance", 1)
        rng = [v for v in (d.get("stackSizeRange") or []) if v is not None] or [1]
        lo, hi = rng[0], rng[-1] if len(rng) > 1 else max(rng[0], 1)
        drops.append([d["item"], chance, lo, hi, d.get("blockAge"), silk])
    loot[entry["block"]] = drops

json.dump({
    "items": [[i["name"], i["stackSize"], i.get("maxDurability", 0)] for i in sorted(items, key=lambda i: i["id"])],
    "foods": {f["name"]: [f["foodPoints"], f["saturation"]] for f in json.load(open(pc("1.20.3", "foods.json")))},
    "materials": {k: {item_names[int(i)]: m for i, m in v.items()} for k, v in json.load(open(pc("1.20.3", "materials.json"))).items()},
    "blocks": {b["name"]: [b.get("material", "default"), sorted(item_names[int(t)] for t in (b.get("harvestTools") or {})), b.get("resistance", 0)] for b in blocks},
    "shapes": [shapes["shapes"][sid] for sid in shape_ids],
    "stateShapes": state_shapes,
    "loot": loot,
    "recipes": recipes,
    "damageTypes": [d["name"]["value"] for d in sorted(damage_types, key=lambda d: d["id"]["value"])],
    "enchantments": [[e["name"], e["maxLevel"], e["minCost"]["a"], e["minCost"]["b"], e["maxCost"]["a"], e["maxCost"]["b"],
                      e["treasureOnly"], e["curse"], e["exclude"], e["category"], e["weight"], e["discoverable"]]
                     for e in sorted(json.load(open(pc("1.20.2", "enchantments.json"))), key=lambda e: e["id"])],
    "entityLoot": {e["entity"]: [[d["item"], d.get("dropChance", 1), *((d.get("stackSizeRange") or [1, 1]) + [1])[:2], bool(d.get("playerKill"))]
                                 for d in e["drops"]] for e in json.load(open(pc("1.20", "entityLoot.json")))},
}, open(os.path.join(out, "gameplay.json"), "w"), separators=(",", ":"))
print("ok")

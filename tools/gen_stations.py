"""Generates data/stations.json (stonecutter, smithing, loom and piglin bartering) from the
vanilla 1.20.4 server's data.

Usage: java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --server --reports --output gen
     python tools/gen_stations.py gen/data/minecraft
"""
import glob
import json
import os
import sys

src = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data", "stations.json")
strip = lambda s: s.replace("minecraft:", "")
load = lambda *p: json.load(open(os.path.join(src, *p)))


def tag(kind, name):
    return [strip(v) for v in load("tags", kind, name + ".json")["values"]]


stonecutting, smithing = [], []
for f in sorted(glob.glob(os.path.join(src, "recipes", "*.json"))):
    r = json.load(open(f))
    if r["type"] == "minecraft:stonecutting":
        stonecutting.append([strip(r["ingredient"]["item"]), strip(r["result"]), r["count"]])
    elif r["type"] == "minecraft:smithing_transform":
        smithing.append([strip(r["template"]["item"]), strip(r["base"]["item"]), strip(r["addition"]["item"]), strip(r["result"]["item"])])

trim_patterns = {}
for f in glob.glob(os.path.join(src, "trim_pattern", "*.json")):
    p = json.load(open(f))
    trim_patterns[strip(p["template_item"])] = p["asset_id"]
trim_materials = {}
for f in glob.glob(os.path.join(src, "trim_material", "*.json")):
    m = json.load(open(f))
    trim_materials[strip(m["ingredient"])] = "minecraft:" + os.path.basename(f)[:-5]

pattern_items = {
    "flower_banner_pattern": "flower", "creeper_banner_pattern": "creeper", "skull_banner_pattern": "skull",
    "mojang_banner_pattern": "mojang", "globe_banner_pattern": "globe", "piglin_banner_pattern": "piglin",
}
banner = {
    "none": tag("banner_pattern", "no_item_required"),
    "items": {item: tag("banner_pattern", "pattern_item/" + t) for item, t in pattern_items.items()},
}

barter = []
for e in load("loot_tables", "gameplay", "piglin_bartering.json")["pools"][0]["entries"]:
    lo = hi = 1
    potion = enchant = None
    for fn in e.get("functions", []):
        if fn["function"] == "minecraft:set_count":
            c = fn["count"]
            lo, hi = (int(c["min"]), int(c["max"])) if isinstance(c, dict) else (int(c), int(c))
        elif fn["function"] == "minecraft:set_potion":
            potion = strip(fn["id"])
        elif fn["function"] == "minecraft:enchant_randomly":
            enchant = strip(fn["enchantments"][0])
    barter.append([strip(e["name"]), e["weight"], lo, hi, potion, enchant])

json.dump({
    "stonecutting": stonecutting,
    "smithing": smithing,
    "trimPatterns": trim_patterns,
    "trimMaterials": trim_materials,
    "trimmable": tag("items", "trimmable_armor"),
    "banner": banner,
    "barter": barter,
}, open(out, "w"), separators=(",", ":"))
print("ok", len(stonecutting), len(smithing), len(barter))

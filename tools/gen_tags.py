"""Generates data/tags.json (update_tags packet) with the official 1.20.4 tags.

Requires the report from the vanilla server's data generator:
    java -DbundlerMainClass=net.minecraft.data.Main -jar server-1.20.4.jar --server --reports --output gen
Usage: python tools/gen_tags.py <gen folder>
"""
import json
import os
import sys

gen = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")
reports = json.load(open(os.path.join(gen, "reports", "registries.json")))
tags_dir = os.path.join(gen, "data", "minecraft", "tags")

# Dynamic registries: the ids follow the order of the registry_data we send.
codec_path = os.path.join(out, "registries.json")
biomes = json.load(open(codec_path))["biomes"]
damage = json.load(open(os.path.join(out, "gameplay.json")))["damageTypes"]
dynamic = {"minecraft:worldgen/biome": biomes, "minecraft:damage_type": damage}

DIRS = {
    "blocks": "minecraft:block", "items": "minecraft:item", "fluids": "minecraft:fluid",
    "entity_types": "minecraft:entity_type", "game_events": "minecraft:game_event",
    "damage_type": "minecraft:damage_type", "banner_pattern": "minecraft:banner_pattern",
    "cat_variant": "minecraft:cat_variant", "instrument": "minecraft:instrument",
    "painting_variant": "minecraft:painting_variant", "point_of_interest_type": "minecraft:point_of_interest_type",
    "worldgen/biome": "minecraft:worldgen/biome",
}

result = {}
for folder, registry in DIRS.items():
    base = os.path.join(tags_dir, folder)
    if not os.path.isdir(base):
        continue
    if registry in dynamic:
        ids = {n: i for i, n in enumerate(dynamic[registry])}
    else:
        ids = {k: v["protocol_id"] for k, v in reports[registry]["entries"].items()}
    raw = {}
    for root, _, files in os.walk(base):
        for f in files:
            if f.endswith(".json"):
                rel = os.path.relpath(os.path.join(root, f), base).replace("\\", "/")[:-5]
                raw["minecraft:" + rel] = json.load(open(os.path.join(root, f)))["values"]

    cache = {}

    def expand(tag, stack=()):
        if tag in cache:
            return cache[tag]
        out_ids = []
        for v in raw.get(tag, []):
            name, required = (v["id"], v.get("required", True)) if isinstance(v, dict) else (v, True)
            if name.startswith("#"):
                if name[1:] not in stack:
                    out_ids += expand(name[1:], stack + (tag,))
            elif name in ids:
                out_ids.append(ids[name])
            elif required:
                print(f"aviso: {name} no existe en {registry}")
        cache[tag] = sorted(set(out_ids))
        return cache[tag]

    result[registry] = {t: expand(t) for t in sorted(raw)}

json.dump(result, open(os.path.join(out, "tags.json"), "w"), separators=(",", ":"))
print("ok", {k: len(v) for k, v in result.items()})

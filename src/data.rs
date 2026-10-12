//! Embedded 1.20.4 registries (generated with `tools/gen_data.py` from minecraft-data).

#![allow(dead_code)]

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub static REGISTRY_CODEC: &[u8] = include_bytes!("../data/registry_codec.nbt");
static REGISTRIES_JSON: &str = include_str!("../data/registries.json");
static TAGS_JSON: &str = include_str!("../data/tags.json");

/// Body of the `update_tags` packet with the official 1.20.4 tags (without the packet id).
/// Without them the vanilla client doesn't recognize water (can't swim) or ladders.
pub fn tags_payload() -> &'static [u8] {
    use crate::protocol::Writer;
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    P.get_or_init(|| {
        let tags: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<i32>>> =
            serde_json::from_str(TAGS_JSON).expect("tags.json inválido");
        let mut p = Vec::new();
        p.varint(tags.len() as i32);
        for (registry, list) in &tags {
            p.string(registry).varint(list.len() as i32);
            for (name, ids) in list {
                p.string(name).varint(ids.len() as i32);
                for id in ids {
                    p.varint(*id);
                }
            }
        }
        p
    })
}

#[derive(Deserialize)]
struct RawRegistries {
    biomes: Vec<String>,
    #[allow(clippy::type_complexity)]
    blocks: Vec<(String, u16, u16, u16, Vec<(String, Vec<String>)>, u8, u8, bool, f32)>,
    items: Vec<String>,
    entities: HashMap<String, i32>,
}

pub struct Block {
    pub name: String,
    pub min_state: u16,
    pub max_state: u16,
    pub default_state: u16,
    pub props: Vec<(String, Vec<String>)>,
    /// Light it absorbs (0 = transparent, 15 = opaque).
    pub filter_light: u8,
    pub emit_light: u8,
    pub transparent: bool,
    /// Hardness (0 = breaks instantly, -1 = unbreakable).
    pub hardness: f32,
}

pub struct Registries {
    pub blocks: Vec<Block>,
    pub items: Vec<String>,
    block_by_name: HashMap<String, usize>,
    item_by_name: HashMap<String, i32>,
    state_to_block: Vec<u16>,
    /// Light (filter, emission) per state.
    light: Vec<(u8, u8)>,
    air: Vec<bool>,
    pub plains_biome: i32,
    biomes: Vec<String>,
    pub player_entity: i32,
    entity_ids: HashMap<String, i32>,
}

/// Items whose block has a different name.
const ITEM_BLOCK_ALIASES: &[(&str, &str)] = &[
    ("redstone", "redstone_wire"),
    ("string", "tripwire"),
    ("wheat_seeds", "wheat"),
    ("carrot", "carrots"),
    ("potato", "potatoes"),
    ("beetroot_seeds", "beetroots"),
    ("melon_seeds", "melon_stem"),
    ("pumpkin_seeds", "pumpkin_stem"),
    ("sweet_berries", "sweet_berry_bush"),
    ("glow_berries", "cave_vines"),
    ("cocoa_beans", "cocoa"),
    ("torchflower_seeds", "torchflower_crop"),
    ("pitcher_pod", "pitcher_crop"),
    ("water_bucket", "water"),
    ("lava_bucket", "lava"),
    ("powder_snow_bucket", "powder_snow"),
];

pub fn get() -> &'static Registries {
    static R: OnceLock<Registries> = OnceLock::new();
    R.get_or_init(|| {
        let raw: RawRegistries = serde_json::from_str(REGISTRIES_JSON).expect("registries.json inválido");
        let blocks: Vec<Block> = raw
            .blocks
            .into_iter()
            .map(|(name, min_state, max_state, default_state, props, filter_light, emit_light, transparent, hardness)| Block {
                name,
                min_state,
                max_state,
                default_state,
                props,
                filter_light,
                emit_light,
                transparent,
                hardness,
            })
            .collect();
        let n = blocks.last().map_or(0, |b| b.max_state as usize + 1);
        let mut state_to_block = vec![0u16; n];
        let mut light = vec![(0u8, 0u8); n];
        let mut air = vec![false; n];
        for (i, b) in blocks.iter().enumerate() {
            for s in b.min_state..=b.max_state {
                state_to_block[s as usize] = i as u16;
                light[s as usize] = (b.filter_light, b.emit_light);
                air[s as usize] = matches!(b.name.as_str(), "air" | "cave_air" | "void_air");
            }
        }
        Registries {
            block_by_name: blocks.iter().enumerate().map(|(i, b)| (b.name.clone(), i)).collect(),
            item_by_name: raw.items.iter().enumerate().map(|(i, n)| (n.clone(), i as i32)).collect(),
            plains_biome: raw.biomes.iter().position(|b| b == "minecraft:plains").unwrap_or(0) as i32,
            biomes: raw.biomes.clone(),
            player_entity: raw.entities["player"],
            entity_ids: raw.entities,
            blocks,
            items: raw.items,
            state_to_block,
            light,
            air,
        }
    })
}

impl Registries {
    pub fn block(&self, name: &str) -> Option<&Block> {
        self.block_by_name.get(name.strip_prefix("minecraft:").unwrap_or(name)).map(|&i| &self.blocks[i])
    }

    /// Default state of a block by name (air if it doesn't exist).
    pub fn state(&self, name: &str) -> u16 {
        self.block(name).map_or(0, |b| b.default_state)
    }

    /// Number of block states.
    pub fn state_count(&self) -> usize {
        self.state_to_block.len()
    }

    pub fn block_of_state(&self, state: u16) -> &Block {
        &self.blocks[*self.state_to_block.get(state as usize).unwrap_or(&0) as usize]
    }

    #[inline]
    pub fn light(&self, state: u16) -> (u8, u8) {
        self.light.get(state as usize).copied().unwrap_or((15, 0))
    }

    #[inline]
    pub fn is_air(&self, state: u16) -> bool {
        self.air.get(state as usize).copied().unwrap_or(false)
    }

    /// Numeric block id (`minecraft:block` registry).
    pub fn block_index(&self, name: &str) -> i32 {
        self.block_by_name.get(name).map_or(0, |&i| i as i32)
    }

    /// Biome id (order of the registry_data that is sent).
    pub fn biome_id(&self, name: &str) -> u8 {
        let full = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        self.biomes.iter().position(|b| *b == full).unwrap_or(self.plains_biome as usize) as u8
    }

    pub fn biome_name(&self, id: u8) -> &str {
        self.biomes.get(id as usize).map_or("minecraft:plains", String::as_str)
    }

    pub fn entity(&self, name: &str) -> i32 {
        self.entity_ids.get(name).copied().unwrap_or(0)
    }

    pub fn has_entity(&self, name: &str) -> bool {
        self.entity_ids.contains_key(name)
    }

    pub fn item_id(&self, name: &str) -> Option<i32> {
        self.item_by_name.get(name.strip_prefix("minecraft:").unwrap_or(name)).copied()
    }

    /// Block placed by an item, if any.
    pub fn block_for_item(&self, item: i32) -> Option<&Block> {
        let name = self.items.get(item as usize)?;
        let name = ITEM_BLOCK_ALIASES.iter().find(|(i, _)| i == name).map_or(name.as_str(), |(_, b)| b);
        self.block(name)
    }

    /// Wall variant (`torch` -> `wall_torch`, `oak_sign` -> `oak_wall_sign`...).
    pub fn wall_variant(&self, block: &Block) -> Option<&Block> {
        if let Some(wood) = block.name.strip_suffix("_hanging_sign") {
            return self.block(&format!("{wood}_wall_hanging_sign"));
        }
        let name = match block.name.rsplit_once('_') {
            Some((head, tail)) => format!("{head}_wall_{tail}"),
            None => format!("wall_{}", block.name),
        };
        self.block(&name)
    }
}

impl Block {
    fn stride(&self, prop: usize) -> u16 {
        self.props[prop + 1..].iter().map(|(_, v)| v.len() as u16).product()
    }

    pub fn has(&self, prop: &str) -> bool {
        self.props.iter().any(|(n, _)| n == prop)
    }

    pub fn get<'a>(&'a self, state: u16, prop: &str) -> Option<&'a str> {
        let i = self.props.iter().position(|(n, _)| n == prop)?;
        let values = &self.props[i].1;
        let idx = (state - self.min_state) / self.stride(i) % values.len() as u16;
        Some(&values[idx as usize])
    }

    /// Returns `state` with the property changed (or unchanged if it doesn't apply).
    pub fn with(&self, state: u16, prop: &str, value: &str) -> u16 {
        let Some(i) = self.props.iter().position(|(n, _)| n == prop) else { return state };
        let values = &self.props[i].1;
        let Some(new) = values.iter().position(|v| v == value) else { return state };
        let stride = self.stride(i);
        let cur = (state - self.min_state) / stride % values.len() as u16;
        state - cur * stride + new as u16 * stride
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn states() {
        let r = super::get();
        assert_eq!(r.state("grass_block"), 9);
        assert_eq!(r.state("bedrock"), 79);
        let leaves = r.block("oak_leaves").unwrap();
        assert_eq!(leaves.get(leaves.default_state, "distance"), Some("7"));
        let s = leaves.with(leaves.default_state, "persistent", "true");
        assert_eq!(leaves.get(s, "persistent"), Some("true"));
        assert_eq!(leaves.get(s, "distance"), Some("7"));
        let torch = r.block("torch").unwrap();
        assert_eq!(r.wall_variant(torch).unwrap().name, "wall_torch");
        assert_eq!(r.block_for_item(r.item_id("oak_log").unwrap()).unwrap().name, "oak_log");
    }
}

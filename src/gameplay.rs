//! Survival data (generated in `data/gameplay.json`): items, food, tools,
//! collision shapes, block and mob loot, recipes, enchantments, armor, furnaces and
//! damage types.

#![allow(dead_code)]

use crate::data::{self, Block};
use crate::item::ItemStack;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

static GAMEPLAY_JSON: &str = include_str!("../data/gameplay.json");

#[derive(Deserialize)]
struct Raw {
    items: Vec<(String, i8, i32)>,
    foods: HashMap<String, (f32, f32)>,
    materials: HashMap<String, HashMap<String, f32>>,
    blocks: HashMap<String, (String, Vec<String>, f32)>,
    shapes: Vec<Vec<[f64; 6]>>,
    #[serde(rename = "stateShapes")]
    state_shapes: Vec<u16>,
    loot: HashMap<String, Vec<(String, f32, i32, i32, Option<i32>, i32)>>,
    #[serde(rename = "entityLoot")]
    entity_loot: HashMap<String, Vec<(String, f32, i32, i32, bool)>>,
    recipes: Vec<RawRecipe>,
    #[serde(rename = "damageTypes")]
    damage_types: Vec<String>,
    #[allow(clippy::type_complexity)]
    enchantments: Vec<(String, i32, i32, i32, i32, i32, bool, bool, Vec<String>, String, i32, bool)>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawRecipe {
    Shaped { s: Vec<Vec<Option<Vec<i32>>>>, r: (i32, i8) },
    Shapeless { i: Vec<Vec<i32>>, r: (i32, i8) },
}

pub struct ItemInfo {
    pub name: String,
    pub stack_size: i8,
    pub max_durability: i32,
}

pub struct Drop {
    pub item: i32,
    pub chance: f32,
    pub min: i32,
    pub max: i32,
    /// Only if the crop has this age.
    pub age: Option<i32>,
    /// 1 = only with Silk Touch, -1 = only without it, 0 = always.
    pub silk: i32,
}

pub struct MobDrop {
    pub item: i32,
    pub chance: f32,
    pub min: i32,
    pub max: i32,
    pub player_kill: bool,
}

pub enum Recipe {
    /// `cells` row by row (`w * h`); each cell accepts any of its items.
    Shaped { w: usize, h: usize, cells: Vec<Option<Vec<i32>>>, result: (i32, i8) },
    Shapeless { ingredients: Vec<Vec<i32>>, result: (i32, i8) },
}

pub struct Enchantment {
    pub id: i32,
    pub name: String,
    pub max_level: i32,
    /// Minimum/maximum power cost: `a * level + b`.
    pub min_cost: (i32, i32),
    pub max_cost: (i32, i32),
    pub treasure: bool,
    pub curse: bool,
    pub exclude: Vec<String>,
    pub category: String,
    pub weight: i32,
    pub discoverable: bool,
}

pub struct Gameplay {
    pub items: Vec<ItemInfo>,
    foods: HashMap<i32, (i32, f32)>,
    /// material -> (tool item -> speed multiplier)
    materials: HashMap<String, HashMap<i32, f32>>,
    /// block -> (material, tools that harvest it; empty = any)
    block_tools: HashMap<String, (String, Vec<i32>)>,
    /// Explosion resistance per block.
    resistance: HashMap<String, f32>,
    shapes: Vec<Vec<[f64; 6]>>,
    state_shapes: Vec<u16>,
    loot: HashMap<String, Vec<Drop>>,
    mob_loot: HashMap<String, Vec<MobDrop>>,
    pub recipes: Vec<Recipe>,
    damage_types: Vec<String>,
    pub enchantments: Vec<Enchantment>,
}

pub fn get() -> &'static Gameplay {
    static G: OnceLock<Gameplay> = OnceLock::new();
    G.get_or_init(|| {
        let raw: Raw = serde_json::from_str(GAMEPLAY_JSON).expect("gameplay.json inválido");
        let reg = data::get();
        let id = |n: &str| reg.item_id(n);
        Gameplay {
            foods: raw.foods.iter().filter_map(|(n, &(f, s))| Some((id(n)?, (f as i32, s)))).collect(),
            materials: raw
                .materials
                .into_iter()
                .map(|(m, tools)| (m, tools.iter().filter_map(|(n, &v)| Some((id(n)?, v))).collect()))
                .collect(),
            resistance: raw.blocks.iter().map(|(b, (_, _, r))| (b.clone(), *r)).collect(),
            block_tools: raw
                .blocks
                .into_iter()
                .map(|(b, (m, tools, _))| (b, (m, tools.iter().filter_map(|t| id(t)).collect())))
                .collect(),
            shapes: raw.shapes,
            state_shapes: raw.state_shapes,
            loot: raw
                .loot
                .into_iter()
                .map(|(b, drops)| {
                    let drops = drops
                        .into_iter()
                        .filter_map(|(item, chance, min, max, age, silk)| Some(Drop { item: id(&item)?, chance, min, max, age, silk }))
                        .collect();
                    (b, drops)
                })
                .collect(),
            mob_loot: raw
                .entity_loot
                .into_iter()
                .map(|(e, drops)| {
                    let drops = drops
                        .into_iter()
                        .filter_map(|(item, chance, min, max, player_kill)| Some(MobDrop { item: id(&item)?, chance, min, max, player_kill }))
                        .collect();
                    (e, drops)
                })
                .collect(),
            recipes: raw
                .recipes
                .into_iter()
                .map(|r| match r {
                    RawRecipe::Shaped { s, r } => {
                        let (h, w) = (s.len(), s.iter().map(Vec::len).max().unwrap_or(0));
                        let cells = s.into_iter().flat_map(|row| row.into_iter().chain(std::iter::repeat(None)).take(w)).collect();
                        Recipe::Shaped { w, h, cells, result: r }
                    }
                    RawRecipe::Shapeless { i, r } => Recipe::Shapeless { ingredients: i, result: r },
                })
                .collect(),
            damage_types: raw.damage_types,
            enchantments: raw
                .enchantments
                .into_iter()
                .enumerate()
                .map(|(i, (name, max_level, a1, b1, a2, b2, treasure, curse, exclude, category, weight, discoverable))| Enchantment {
                    id: i as i32,
                    name,
                    max_level,
                    min_cost: (a1, b1),
                    max_cost: (a2, b2),
                    treasure,
                    curse,
                    exclude,
                    category,
                    weight,
                    discoverable,
                })
                .collect(),
            items: raw.items.into_iter().map(|(name, stack_size, max_durability)| ItemInfo { name, stack_size, max_durability }).collect(),
        }
    })
}

/// Items that drop only a single unit (Fortune multiplies on top of that base).
const SINGLE_ORE_DROPS: &[&str] = &["coal", "diamond", "emerald", "raw_iron", "raw_gold", "quartz"];

impl Gameplay {
    pub fn item(&self, id: i32) -> Option<&ItemInfo> {
        self.items.get(id as usize)
    }

    pub fn item_name(&self, id: i32) -> &str {
        self.item(id).map_or("air", |i| i.name.as_str())
    }

    pub fn max_stack(&self, id: i32) -> i8 {
        self.item(id).map_or(64, |i| i.stack_size.max(1))
    }

    /// (food points, saturation) if the item is edible.
    pub fn food(&self, id: i32) -> Option<(i32, f32)> {
        self.foods.get(&id).copied()
    }

    /// Collision boxes of the state, relative to the block.
    pub fn shape(&self, state: u16) -> &[[f64; 6]] {
        let i = self.state_shapes.get(state as usize).copied().unwrap_or(1);
        &self.shapes[i as usize]
    }

    pub fn damage_type(&self, name: &str) -> i32 {
        self.damage_types.iter().position(|d| d.trim_start_matches("minecraft:") == name).unwrap_or(16) as i32
    }

    pub fn resistance(&self, block: &str) -> f32 {
        self.resistance.get(block).copied().unwrap_or(0.0)
    }

    pub fn enchantment(&self, name: &str) -> Option<&Enchantment> {
        self.enchantments.iter().find(|e| e.name == name)
    }

    /// Can `tool` harvest (get loot from) `block`?
    pub fn can_harvest(&self, block: &Block, tool: Option<i32>) -> bool {
        match self.block_tools.get(&block.name) {
            Some((_, tools)) if !tools.is_empty() => tool.is_some_and(|t| tools.contains(&t)),
            _ => true,
        }
    }

    /// Ticks to break `block` with `tool` (vanilla formula with Efficiency; no potion effects).
    pub fn break_ticks(&self, block: &Block, tool: Option<&ItemStack>, on_ground: bool, in_water: bool) -> u32 {
        if block.hardness < 0.0 {
            return u32::MAX;
        }
        if block.hardness == 0.0 {
            return 0;
        }
        let tool_id = tool.map(|t| t.id);
        let mut speed = 1.0f32;
        if let (Some((material, _)), Some(t)) = (self.block_tools.get(&block.name), tool_id) {
            for m in material.split(';') {
                if let Some(&mult) = self.materials.get(m).and_then(|tools| tools.get(&t)) {
                    speed = speed.max(mult);
                }
            }
        }
        let efficiency = tool.map_or(0, |t| t.enchantment("efficiency"));
        if speed > 1.0 && efficiency > 0 {
            speed += (efficiency * efficiency + 1) as f32;
        }
        if in_water {
            speed /= 5.0;
        }
        if !on_ground {
            speed /= 5.0;
        }
        let per_tick = speed / block.hardness / if self.can_harvest(block, tool_id) { 30.0 } else { 100.0 };
        if per_tick >= 1.0 {
            0
        } else {
            (1.0 / per_tick).ceil() as u32
        }
    }

    /// Loot from breaking `block` in state `state` with `tool` (Silk Touch and Fortune included).
    pub fn drops(&self, block: &Block, state: u16, tool: Option<&ItemStack>, rnd: &mut impl FnMut() -> f32) -> Vec<(i32, i8)> {
        let reg = data::get();
        let id = |n: &str| reg.item_id(n).unwrap_or(0);
        let name = block.name.as_str();
        let silk = tool.is_some_and(|t| t.enchantment("silk_touch") > 0);
        let shears = tool.is_some_and(|t| t.name() == "shears");
        let fortune = tool.map_or(0, |t| t.enchantment("fortune"));
        if block.get(state, "half") == Some("upper") || block.get(state, "part") == Some("head") {
            return vec![];
        }
        // minecraft-data does not model these probabilities well.
        if name.ends_with("_leaves") {
            if silk || shears {
                return reg.item_id(name).map(|i| vec![(i, 1)]).unwrap_or_default();
            }
            let mut out = Vec::new();
            let sapling = name.replace("_leaves", "_sapling");
            if let Some(s) = reg.item_id(&sapling).filter(|_| rnd() < 0.05) {
                out.push((s, 1));
            }
            if rnd() < 0.02 {
                out.push((id("stick"), 1 + (rnd() * 2.0) as i8));
            }
            if matches!(name, "oak_leaves" | "dark_oak_leaves") && rnd() < 0.005 {
                out.push((id("apple"), 1));
            }
            return out;
        }
        if matches!(name, "short_grass" | "tall_grass" | "fern" | "large_fern") {
            if shears {
                return reg.item_id(name).map(|i| vec![(i, 1)]).unwrap_or_default();
            }
            return if rnd() < 0.125 { vec![(id("wheat_seeds"), 1)] } else { vec![] };
        }
        let age: Option<i32> = block.get(state, "age").and_then(|a| a.parse().ok());
        let Some(drops) = self.loot.get(name) else {
            return reg.item_id(name).map(|i| vec![(i, 1)]).unwrap_or_default();
        };
        // For crops the 0.5 probabilities are a data artifact: they always drop.
        let crop = drops.iter().any(|d| d.age.is_some());
        let ore = name.ends_with("_ore");
        let mut out = Vec::new();
        for d in drops.iter().filter(|d| d.age.is_none() || d.age == age) {
            if (d.silk == 1 && !silk) || (d.silk == -1 && silk) {
                continue;
            }
            if crop || d.chance >= 1.0 || rnd() < d.chance {
                let item_name = self.item_name(d.item);
                let (lo, hi) = if ore && SINGLE_ORE_DROPS.contains(&item_name) { (1, 1) } else { (d.min, d.max) };
                let mut n = (lo + (rnd() * (hi - lo + 1) as f32) as i32).clamp(lo, hi);
                // Fortune (vanilla ore_drops formula).
                if ore && d.silk != 1 && fortune > 0 {
                    let bonus = ((rnd() * (fortune + 2) as f32) as i32 - 1).max(0);
                    n *= bonus + 1;
                }
                if n > 0 {
                    out.push((d.item, n.min(64) as i8));
                }
            }
        }
        out
    }

    /// Experience a block drops when broken without Silk Touch.
    pub fn block_xp(&self, block: &Block, rnd: &mut impl FnMut() -> f32) -> i32 {
        let range = |lo: i32, hi: i32, r: f32| lo + (r * (hi - lo + 1) as f32) as i32;
        let n = block.name.trim_start_matches("deepslate_");
        match n {
            "coal_ore" => range(0, 2, rnd()),
            "diamond_ore" | "emerald_ore" => range(3, 7, rnd()),
            "lapis_ore" | "nether_quartz_ore" => range(2, 5, rnd()),
            "redstone_ore" => range(1, 5, rnd()),
            "nether_gold_ore" => range(0, 1, rnd()),
            "spawner" => range(15, 43, rnd()),
            _ => 0,
        }
    }

    /// Mob loot (with Looting).
    pub fn mob_drops(&self, mob: &str, killed_by_player: bool, looting: i32, rnd: &mut impl FnMut() -> f32) -> Vec<(i32, i8)> {
        let Some(drops) = self.mob_loot.get(mob) else { return vec![] };
        let mut out = Vec::new();
        for d in drops {
            if d.player_kill && !killed_by_player {
                continue;
            }
            let chance = if d.chance < 1.0 { d.chance + 0.01 * looting as f32 } else { 1.0 };
            if rnd() >= chance {
                continue;
            }
            // Vanilla: 0..2 of almost everything (cows 0..2 leather, 1..3 beef...).
            let (lo, hi) = match self.item_name(d.item) {
                "leather" | "feather" | "rotten_flesh" | "bone" | "arrow" | "gunpowder" | "string" => (0, 2),
                "beef" | "porkchop" => (1, 3),
                "mutton" => (1, 2),
                _ => (d.min, d.max),
            };
            let n = lo + (rnd() * (hi - lo + 1 + looting) as f32) as i32;
            if n > 0 {
                out.push((d.item, n as i8));
            }
        }
        out
    }
}

// ---------- armor ----------

/// (armor points, toughness) of a piece.
pub fn armor_points(item: &ItemStack) -> (f32, f32) {
    let name = item.name();
    let (mat, piece) = match name.rsplit_once('_') {
        Some((m, p)) => (m, p),
        None => return (0.0, 0.0),
    };
    let idx = match piece {
        "helmet" => 0,
        "chestplate" => 1,
        "leggings" => 2,
        "boots" => 3,
        _ => return (0.0, 0.0),
    };
    let (table, tough): ([f32; 4], f32) = match mat {
        "leather" => ([1.0, 3.0, 2.0, 1.0], 0.0),
        "chainmail" => ([2.0, 5.0, 4.0, 1.0], 0.0),
        "iron" => ([2.0, 6.0, 5.0, 2.0], 0.0),
        "golden" => ([2.0, 5.0, 3.0, 1.0], 0.0),
        "diamond" => ([3.0, 8.0, 6.0, 3.0], 2.0),
        "netherite" => ([3.0, 8.0, 6.0, 3.0], 3.0),
        "turtle" => ([2.0, 0.0, 0.0, 0.0], 0.0),
        _ => return (0.0, 0.0),
    };
    (table[idx], tough)
}

/// Item enchantability (0 = not enchantable at the table).
pub fn enchantability(item: &ItemStack) -> i32 {
    let n = item.name();
    let armor = ["_helmet", "_chestplate", "_leggings", "_boots"].iter().any(|s| n.ends_with(s));
    let tool = ["_sword", "_pickaxe", "_axe", "_shovel", "_hoe"].iter().any(|s| n.ends_with(s));
    let mat = n.split('_').next().unwrap_or("");
    match () {
        _ if tool => match mat {
            "wooden" | "netherite" => 15,
            "stone" => 5,
            "iron" => 14,
            "golden" => 22,
            "diamond" => 10,
            _ => 0,
        },
        _ if armor => match mat {
            "leather" | "netherite" => 15,
            "chainmail" => 12,
            "iron" => 9,
            "golden" => 25,
            "diamond" => 10,
            "turtle" => 9,
            _ => 0,
        },
        _ => match n {
            "book" | "bow" | "crossbow" | "trident" | "fishing_rod" => 1,
            _ => 0,
        },
    }
}

/// Does the item accept this enchantment category?
pub fn enchant_category_fits(category: &str, item: &ItemStack) -> bool {
    let n = item.name();
    if n == "book" || n == "enchanted_book" {
        return true;
    }
    let ends = |s: &str| n.ends_with(s);
    let armor = ends("_helmet") || ends("_chestplate") || ends("_leggings") || ends("_boots");
    let digger = ends("_pickaxe") || ends("_axe") || ends("_shovel") || ends("_hoe");
    match category {
        "armor" => armor,
        "armor_head" => ends("_helmet"),
        "armor_chest" => ends("_chestplate"),
        "armor_legs" => ends("_leggings"),
        "armor_feet" => ends("_boots"),
        "weapon" => ends("_sword"),
        "digger" => digger,
        "breakable" => item.max_durability() > 0,
        "vanishable" | "wearable" => item.max_durability() > 0 || armor,
        "bow" => n == "bow",
        "crossbow" => n == "crossbow",
        "trident" => n == "trident",
        "fishing_rod" => n == "fishing_rod",
        _ => false,
    }
}

// ---------- furnaces ----------

/// Result of smelting an item: (item, experience per unit).
pub fn smelt(input: &str) -> Option<(String, f32)> {
    let r: (&str, f32) = match input {
        "raw_iron" | "iron_ore" | "deepslate_iron_ore" => ("iron_ingot", 0.7),
        "raw_gold" | "gold_ore" | "deepslate_gold_ore" | "nether_gold_ore" => ("gold_ingot", 1.0),
        "raw_copper" | "copper_ore" | "deepslate_copper_ore" => ("copper_ingot", 0.7),
        "coal_ore" | "deepslate_coal_ore" => ("coal", 0.1),
        "diamond_ore" | "deepslate_diamond_ore" => ("diamond", 1.0),
        "emerald_ore" | "deepslate_emerald_ore" => ("emerald", 1.0),
        "lapis_ore" | "deepslate_lapis_ore" => ("lapis_lazuli", 0.2),
        "redstone_ore" | "deepslate_redstone_ore" => ("redstone", 0.7),
        "nether_quartz_ore" => ("quartz", 0.2),
        "ancient_debris" => ("netherite_scrap", 2.0),
        "sand" | "red_sand" => ("glass", 0.1),
        "cobblestone" => ("stone", 0.1),
        "stone" => ("smooth_stone", 0.1),
        "cobbled_deepslate" => ("deepslate", 0.1),
        "sandstone" => ("smooth_sandstone", 0.1),
        "red_sandstone" => ("smooth_red_sandstone", 0.1),
        "quartz_block" => ("smooth_quartz", 0.1),
        "stone_bricks" => ("cracked_stone_bricks", 0.1),
        "clay_ball" => ("brick", 0.3),
        "clay" => ("terracotta", 0.35),
        "netherrack" => ("nether_brick", 0.1),
        "basalt" => ("smooth_basalt", 0.1),
        "cactus" => ("green_dye", 1.0),
        "sea_pickle" => ("lime_dye", 0.1),
        "kelp" => ("dried_kelp", 0.1),
        "wet_sponge" => ("sponge", 0.15),
        "chorus_fruit" => ("popped_chorus_fruit", 0.1),
        "beef" => ("cooked_beef", 0.35),
        "porkchop" => ("cooked_porkchop", 0.35),
        "chicken" => ("cooked_chicken", 0.35),
        "mutton" => ("cooked_mutton", 0.35),
        "rabbit" => ("cooked_rabbit", 0.35),
        "cod" => ("cooked_cod", 0.35),
        "salmon" => ("cooked_salmon", 0.35),
        "potato" => ("baked_potato", 0.35),
        n if n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_stem") || n.ends_with("_hyphae") => {
            if n.contains("crimson") || n.contains("warped") {
                return None;
            }
            ("charcoal", 0.15)
        }
        n if n.ends_with("_terracotta") && !n.contains("glazed") && n != "terracotta" => {
            return Some((format!("{}_glazed_terracotta", n.trim_end_matches("_terracotta")), 0.1));
        }
        _ => return None,
    };
    Some((r.0.to_string(), r.1))
}

/// Fuel ticks provided by an item.
pub fn fuel_ticks(name: &str) -> i32 {
    match name {
        "lava_bucket" => 20000,
        "coal_block" => 16000,
        "dried_kelp_block" => 4001,
        "blaze_rod" => 2400,
        "coal" | "charcoal" => 1600,
        "bamboo" | "scaffolding" => 50,
        "stick" => 100,
        "bowl" => 100,
        "crafting_table" | "bookshelf" | "chest" | "trapped_chest" | "barrel" | "jukebox" | "note_block" | "lectern" | "composter" => 300,
        n if n.ends_with("_planks") || n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_fence") || n.ends_with("_fence_gate") || n.ends_with("_stairs") && n.contains("oak") => 300,
        n if n.ends_with("_slab") && (n.contains("oak") || n.contains("spruce") || n.contains("birch") || n.contains("jungle") || n.contains("acacia") || n.contains("mangrove") || n.contains("cherry") || n.contains("bamboo")) => 150,
        n if n.starts_with("wooden_") => 200,
        n if n.ends_with("_sapling") || n.ends_with("_wool") => 100,
        n if n.ends_with("_carpet") => 67,
        n if n.ends_with("_door") && !n.starts_with("iron") || n.ends_with("_trapdoor") && !n.starts_with("iron") || n.ends_with("_sign") || n.ends_with("_boat") => 200,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::data;
    use crate::item::ItemStack;

    fn item(n: &str) -> ItemStack {
        ItemStack::new(data::get().item_id(n).unwrap(), 1)
    }

    #[test]
    fn digging_and_loot() {
        let (g, r) = (super::get(), data::get());
        let stone = r.block("stone").unwrap();
        let pick = item("wooden_pickaxe");
        // Vanilla: stone by hand 7.5 s (150 ticks); with a wooden pickaxe 1.15 s (23 ticks).
        assert_eq!(g.break_ticks(stone, None, true, false), 150);
        assert_eq!(g.break_ticks(stone, Some(&pick), true, false), 23);
        let mut fast = item("diamond_pickaxe");
        fast.enchant("efficiency", 5);
        // Diamond with Efficiency V: speed 8 + 26 = 34 -> 2 ticks (without Haste it is not instant).
        assert_eq!(g.break_ticks(stone, Some(&fast), true, false), 2);
        assert!(!g.can_harvest(stone, None));
        assert!(g.can_harvest(stone, Some(pick.id)));
        let mut rnd = || 0.3f32;
        let names = |d: Vec<(i32, i8)>| d.into_iter().map(|(i, n)| (r.items[i as usize].clone(), n)).collect::<Vec<_>>();
        assert_eq!(names(g.drops(stone, stone.default_state, Some(&pick), &mut rnd)), vec![("cobblestone".to_string(), 1)]);
        let mut silk = item("diamond_pickaxe");
        silk.enchant("silk_touch", 1);
        assert_eq!(names(g.drops(stone, stone.default_state, Some(&silk), &mut rnd)), vec![("stone".to_string(), 1)]);
        let grass = r.block("grass_block").unwrap();
        assert_eq!(names(g.drops(grass, grass.default_state, None, &mut rnd)), vec![("dirt".to_string(), 1)]);
        let ore = r.block("diamond_ore").unwrap();
        assert_eq!(names(g.drops(ore, ore.default_state, Some(&pick), &mut rnd)), vec![("diamond".to_string(), 1)]);
        let mut lucky = item("iron_pickaxe");
        lucky.enchant("fortune", 3);
        assert_eq!(names(g.drops(ore, ore.default_state, Some(&lucky), &mut || 0.99)), vec![("diamond".to_string(), 4)]);
        let wheat = r.block("wheat").unwrap();
        let ripe = wheat.with(wheat.default_state, "age", "7");
        assert!(names(g.drops(wheat, ripe, None, &mut rnd)).iter().any(|(n, _)| n == "wheat"));
        assert!(!names(g.drops(wheat, wheat.default_state, None, &mut rnd)).iter().any(|(n, _)| n == "wheat"));
        assert_eq!(g.food(r.item_id("bread").unwrap()), Some((5, 6.0)));
        assert_eq!(super::smelt("raw_iron").unwrap().0, "iron_ingot");
        assert_eq!(super::smelt("oak_log").unwrap().0, "charcoal");
        assert_eq!(super::fuel_ticks("coal"), 1600);
        assert_eq!(super::fuel_ticks("oak_planks"), 300);
        assert_eq!(super::armor_points(&item("diamond_chestplate")), (8.0, 2.0));
        assert!(names(g.mob_drops("cow", true, 0, &mut || 0.99)).iter().any(|(n, _)| n == "beef"));
    }
}

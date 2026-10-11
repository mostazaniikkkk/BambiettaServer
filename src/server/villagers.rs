//! Villagers: professions, per-level offers (vanilla VillagerTrades tables, somewhat
//! condensed), trading via the merchant window, leveling up, restocking and spawning in
//! generated villages.

use super::inventory::WindowKind;
use super::mobs::{MobKind, Seen};
use super::stations::Offer;
use super::{entities, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, text, Writer};
use crate::world::Dimension;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Professions with offers (`minecraft:villager_profession` registry).
const PROFESSIONS: [(&str, i32, &str); 13] = [
    ("armorer", 1, "Armero"),
    ("butcher", 2, "Carnicero"),
    ("cartographer", 3, "Cartógrafo"),
    ("cleric", 4, "Clérigo"),
    ("farmer", 5, "Granjero"),
    ("fisherman", 6, "Pescador"),
    ("fletcher", 7, "Flechero"),
    ("leatherworker", 8, "Peletero"),
    ("librarian", 9, "Bibliotecario"),
    ("mason", 10, "Albañil"),
    ("shepherd", 12, "Pastor"),
    ("toolsmith", 13, "Herrero de herramientas"),
    ("weaponsmith", 14, "Herrero de armas"),
];

/// Villager experience required for each level (novice..master).
const LEVEL_XP: [i32; 5] = [0, 10, 70, 150, 250];

pub fn profession_id(name: &str) -> i32 {
    PROFESSIONS.iter().find(|p| p.0 == name).map_or(0, |p| p.1)
}

fn profession_title(name: &str) -> &'static str {
    PROFESSIONS.iter().find(|p| p.0 == name).map_or("Aldeano", |p| p.2)
}

/// Table offer: (level, cost A, count, cost B, count, result, count, uses, xp, multiplier).
type Trade = (i32, &'static str, i8, &'static str, i8, &'static str, i8, i32, i32, f32);

fn trades(profession: &str) -> &'static [Trade] {
    match profession {
        "farmer" => &[
            (1, "wheat", 20, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "potato", 26, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "carrot", 22, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "beetroot", 15, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "bread", 6, 16, 1, 0.05),
            (2, "pumpkin", 6, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 1, "", 0, "pumpkin_pie", 4, 12, 5, 0.05),
            (2, "emerald", 1, "", 0, "apple", 4, 16, 5, 0.05),
            (3, "emerald", 3, "", 0, "cookie", 18, 12, 10, 0.05),
            (3, "melon", 4, "", 0, "emerald", 1, 12, 20, 0.05),
            (4, "emerald", 1, "", 0, "cake", 1, 12, 15, 0.05),
            (5, "emerald", 3, "", 0, "golden_carrot", 3, 12, 30, 0.05),
            (5, "emerald", 4, "", 0, "glistering_melon_slice", 3, 12, 30, 0.05),
        ],
        "librarian" => &[
            (1, "paper", 24, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "book", 1, "enchanted_book", 1, 12, 1, 0.2),
            (1, "emerald", 9, "", 0, "bookshelf", 1, 12, 1, 0.05),
            (2, "book", 4, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 1, "book", 1, "enchanted_book", 1, 12, 5, 0.2),
            (2, "emerald", 1, "", 0, "lantern", 1, 12, 5, 0.05),
            (3, "ink_sac", 5, "", 0, "emerald", 1, 12, 20, 0.05),
            (3, "emerald", 1, "", 0, "glass", 4, 12, 10, 0.05),
            (3, "emerald", 1, "book", 1, "enchanted_book", 1, 12, 10, 0.2),
            (4, "writable_book", 2, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 5, "", 0, "clock", 1, 12, 15, 0.05),
            (4, "emerald", 4, "", 0, "compass", 1, 12, 15, 0.05),
            (5, "emerald", 20, "", 0, "name_tag", 1, 12, 30, 0.05),
        ],
        "armorer" => &[
            (1, "coal", 15, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 7, "", 0, "iron_leggings", 1, 12, 1, 0.2),
            (1, "emerald", 4, "", 0, "iron_boots", 1, 12, 1, 0.2),
            (1, "emerald", 5, "", 0, "iron_helmet", 1, 12, 1, 0.2),
            (1, "emerald", 9, "", 0, "iron_chestplate", 1, 12, 1, 0.2),
            (2, "iron_ingot", 4, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 36, "", 0, "bell", 1, 12, 5, 0.2),
            (2, "emerald", 1, "", 0, "chainmail_boots", 1, 12, 5, 0.2),
            (2, "emerald", 3, "", 0, "chainmail_leggings", 1, 12, 5, 0.2),
            (3, "lava_bucket", 1, "", 0, "emerald", 1, 12, 20, 0.05),
            (3, "diamond", 1, "", 0, "emerald", 1, 12, 20, 0.05),
            (3, "emerald", 1, "", 0, "chainmail_helmet", 1, 12, 10, 0.2),
            (3, "emerald", 4, "", 0, "chainmail_chestplate", 1, 12, 10, 0.2),
            (3, "emerald", 5, "", 0, "shield", 1, 12, 10, 0.2),
            (4, "emerald", 19, "", 0, "diamond_leggings", 1, 3, 15, 0.2),
            (4, "emerald", 13, "", 0, "diamond_boots", 1, 3, 15, 0.2),
            (5, "emerald", 13, "", 0, "diamond_helmet", 1, 3, 30, 0.2),
            (5, "emerald", 21, "", 0, "diamond_chestplate", 1, 3, 30, 0.2),
        ],
        "butcher" => &[
            (1, "chicken", 14, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "porkchop", 7, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "rabbit", 4, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "rabbit_stew", 1, 12, 1, 0.05),
            (2, "coal", 15, "", 0, "emerald", 1, 16, 2, 0.05),
            (2, "emerald", 1, "", 0, "cooked_porkchop", 5, 16, 5, 0.05),
            (2, "emerald", 1, "", 0, "cooked_chicken", 8, 16, 5, 0.05),
            (3, "mutton", 7, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "beef", 10, "", 0, "emerald", 1, 16, 20, 0.05),
            (4, "dried_kelp_block", 10, "", 0, "emerald", 1, 12, 30, 0.05),
            (5, "sweet_berries", 10, "", 0, "emerald", 1, 12, 30, 0.05),
        ],
        "cartographer" => &[
            (1, "paper", 24, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 7, "", 0, "map", 1, 12, 1, 0.05),
            (2, "glass_pane", 11, "", 0, "emerald", 1, 16, 10, 0.05),
            (3, "compass", 1, "", 0, "emerald", 1, 12, 20, 0.05),
            (4, "emerald", 7, "", 0, "item_frame", 1, 12, 15, 0.05),
            (4, "emerald", 3, "", 0, "white_banner", 1, 12, 15, 0.05),
            (5, "emerald", 8, "", 0, "globe_banner_pattern", 1, 12, 30, 0.05),
        ],
        "cleric" => &[
            (1, "rotten_flesh", 32, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "redstone", 2, 12, 1, 0.05),
            (2, "gold_ingot", 3, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 1, "", 0, "lapis_lazuli", 1, 12, 5, 0.05),
            (3, "rabbit_foot", 2, "", 0, "emerald", 1, 12, 20, 0.05),
            (3, "emerald", 4, "", 0, "glowstone", 1, 12, 10, 0.05),
            (4, "scute", 4, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "glass_bottle", 9, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 5, "", 0, "ender_pearl", 1, 12, 15, 0.05),
            (5, "nether_wart", 22, "", 0, "emerald", 1, 12, 30, 0.05),
            (5, "emerald", 3, "", 0, "experience_bottle", 1, 12, 30, 0.05),
        ],
        "fisherman" => &[
            (1, "string", 20, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "coal", 10, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "cod", 6, "cooked_cod", 6, 16, 1, 0.05),
            (1, "emerald", 3, "", 0, "cod_bucket", 1, 16, 1, 0.05),
            (2, "cod", 15, "", 0, "emerald", 1, 16, 10, 0.05),
            (2, "emerald", 1, "salmon", 6, "cooked_salmon", 6, 16, 5, 0.05),
            (2, "emerald", 2, "", 0, "campfire", 1, 12, 5, 0.05),
            (3, "salmon", 13, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "emerald", 8, "", 0, "fishing_rod", 1, 3, 10, 0.2),
            (4, "tropical_fish", 6, "", 0, "emerald", 1, 12, 30, 0.05),
            (5, "pufferfish", 4, "", 0, "emerald", 1, 12, 30, 0.05),
        ],
        "fletcher" => &[
            (1, "stick", 32, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "arrow", 16, 12, 1, 0.05),
            (1, "emerald", 1, "gravel", 10, "flint", 10, 12, 1, 0.05),
            (2, "flint", 26, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 2, "", 0, "bow", 1, 12, 5, 0.05),
            (3, "string", 14, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "emerald", 3, "", 0, "crossbow", 1, 12, 10, 0.05),
            (4, "feather", 24, "", 0, "emerald", 1, 16, 30, 0.05),
            (5, "tripwire_hook", 8, "", 0, "emerald", 1, 12, 30, 0.05),
        ],
        "leatherworker" => &[
            (1, "leather", 6, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 3, "", 0, "leather_leggings", 1, 12, 1, 0.2),
            (1, "emerald", 7, "", 0, "leather_chestplate", 1, 12, 1, 0.2),
            (2, "flint", 26, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 5, "", 0, "leather_helmet", 1, 12, 5, 0.2),
            (2, "emerald", 4, "", 0, "leather_boots", 1, 12, 5, 0.2),
            (3, "rabbit_hide", 9, "", 0, "emerald", 1, 12, 20, 0.05),
            (4, "scute", 4, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 6, "", 0, "leather_horse_armor", 1, 12, 15, 0.05),
            (5, "emerald", 6, "", 0, "saddle", 1, 12, 30, 0.2),
        ],
        "mason" => &[
            (1, "clay_ball", 10, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "brick", 10, 16, 1, 0.05),
            (2, "stone", 20, "", 0, "emerald", 1, 16, 10, 0.05),
            (2, "emerald", 1, "", 0, "chiseled_stone_bricks", 4, 16, 5, 0.05),
            (3, "granite", 16, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "andesite", 16, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "diorite", 16, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "emerald", 1, "", 0, "polished_andesite", 4, 16, 10, 0.05),
            (4, "quartz", 12, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 1, "", 0, "orange_terracotta", 1, 12, 15, 0.05),
            (5, "emerald", 1, "", 0, "quartz_pillar", 1, 12, 30, 0.05),
            (5, "emerald", 1, "", 0, "quartz_block", 1, 12, 30, 0.05),
        ],
        "shepherd" => &[
            (1, "white_wool", 18, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "brown_wool", 18, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "black_wool", 18, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 2, "", 0, "shears", 1, 12, 1, 0.05),
            (2, "white_dye", 12, "", 0, "emerald", 1, 16, 10, 0.05),
            (2, "gray_dye", 12, "", 0, "emerald", 1, 16, 10, 0.05),
            (2, "emerald", 1, "", 0, "white_wool", 1, 16, 5, 0.05),
            (2, "emerald", 1, "", 0, "white_carpet", 4, 16, 5, 0.05),
            (3, "yellow_dye", 12, "", 0, "emerald", 1, 16, 20, 0.05),
            (3, "emerald", 3, "", 0, "white_bed", 1, 12, 10, 0.05),
            (4, "brown_dye", 12, "", 0, "emerald", 1, 16, 30, 0.05),
            (4, "emerald", 3, "", 0, "white_banner", 1, 12, 15, 0.05),
            (5, "emerald", 2, "", 0, "painting", 3, 12, 30, 0.05),
        ],
        "toolsmith" => &[
            (1, "coal", 15, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 1, "", 0, "stone_axe", 1, 12, 1, 0.2),
            (1, "emerald", 1, "", 0, "stone_shovel", 1, 12, 1, 0.2),
            (1, "emerald", 1, "", 0, "stone_pickaxe", 1, 12, 1, 0.2),
            (1, "emerald", 1, "", 0, "stone_hoe", 1, 12, 1, 0.2),
            (2, "iron_ingot", 4, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 36, "", 0, "bell", 1, 12, 5, 0.2),
            (3, "flint", 30, "", 0, "emerald", 1, 12, 20, 0.05),
            (3, "emerald", 3, "", 0, "iron_pickaxe", 1, 3, 10, 0.2),
            (3, "emerald", 4, "", 0, "diamond_hoe", 1, 3, 10, 0.2),
            (4, "diamond", 1, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 12, "", 0, "diamond_axe", 1, 3, 15, 0.2),
            (4, "emerald", 5, "", 0, "diamond_shovel", 1, 3, 15, 0.2),
            (5, "emerald", 13, "", 0, "diamond_pickaxe", 1, 3, 30, 0.2),
        ],
        "weaponsmith" => &[
            (1, "coal", 15, "", 0, "emerald", 1, 16, 2, 0.05),
            (1, "emerald", 3, "", 0, "iron_axe", 1, 12, 1, 0.2),
            (1, "emerald", 2, "", 0, "iron_sword", 1, 3, 1, 0.05),
            (2, "iron_ingot", 4, "", 0, "emerald", 1, 12, 10, 0.05),
            (2, "emerald", 36, "", 0, "bell", 1, 12, 5, 0.2),
            (3, "flint", 24, "", 0, "emerald", 1, 12, 20, 0.05),
            (4, "diamond", 1, "", 0, "emerald", 1, 12, 30, 0.05),
            (4, "emerald", 12, "", 0, "diamond_axe", 1, 3, 15, 0.2),
            (5, "emerald", 8, "", 0, "diamond_sword", 1, 3, 30, 0.2),
        ],
        _ => &[],
    }
}

fn stack(name: &str, count: i8) -> Option<ItemStack> {
    data::get().item_id(name).map(|id| ItemStack::new(id, count.max(1)))
}

/// Random enchanted book with the vanilla price (2 + rand(5 + level*10) + 3*level).
fn enchanted_book_offer(t: &Trade) -> Option<Offer> {
    let g = crate::gameplay::get();
    let pool: Vec<_> = g.enchantments.iter().filter(|e| e.discoverable && !e.curse).collect();
    let e = pool.get((entities::random() * pool.len() as f32) as usize % pool.len().max(1))?;
    let level = 1 + (entities::random() * e.max_level as f32) as i32 % e.max_level.max(1);
    let mut price = 2 + (entities::random() * (5 + level * 10) as f32) as i32 + 3 * level;
    if e.treasure {
        price *= 2;
    }
    let mut book = stack("enchanted_book", 1)?;
    book.enchant(&e.name, level);
    Some(Offer { cost_a: stack("emerald", price.min(64) as i8)?, cost_b: stack("book", 1), result: book, uses: 0, max_uses: t.7, xp: t.8, price_mult: t.9, demand: 0, special: 0 })
}

fn make_offer(t: &Trade) -> Option<Offer> {
    if t.5 == "enchanted_book" {
        return enchanted_book_offer(t);
    }
    Some(Offer {
        cost_a: stack(t.1, t.2)?,
        cost_b: if t.3.is_empty() { None } else { stack(t.3, t.4) },
        result: stack(t.5, t.6)?,
        uses: 0,
        max_uses: t.7,
        xp: t.8,
        price_mult: t.9,
        demand: 0,
        special: 0,
    })
}

#[derive(Clone, Debug)]
pub struct VillagerData {
    pub profession: String,
    pub level: i32,
    pub xp: i32,
    /// Type by biome (`villager_type` registry: 0 desert ... 2 plains ... 6 taiga).
    pub vtype: i32,
    pub offers: Vec<Offer>,
    restock_at: u32,
}

impl VillagerData {
    pub fn random() -> Self {
        let p = PROFESSIONS[(entities::random() * PROFESSIONS.len() as f32) as usize % PROFESSIONS.len()].0;
        Self::with_profession(p)
    }

    pub fn with_profession(p: &str) -> Self {
        let mut v = Self { profession: p.to_string(), level: 1, xp: 0, vtype: 2, offers: vec![], restock_at: 0 };
        v.add_offers(1);
        v
    }

    /// Two random offers of the level (like vanilla).
    fn add_offers(&mut self, level: i32) {
        let mut pool: Vec<&Trade> = trades(&self.profession).iter().filter(|t| t.0 == level).collect();
        for _ in 0..2 {
            if pool.is_empty() {
                break;
            }
            let i = (entities::random() * pool.len() as f32) as usize % pool.len();
            if let Some(o) = make_offer(pool.remove(i)) {
                self.offers.push(o);
            }
        }
    }

    pub fn save(&self) -> SavedVillager {
        let hex = |i: &ItemStack| i.nbt.as_ref().map(|n| n.to_network().iter().map(|b| format!("{b:02x}")).collect::<String>());
        SavedVillager {
            profession: self.profession.clone(),
            level: self.level,
            xp: self.xp,
            vtype: self.vtype,
            offers: self
                .offers
                .iter()
                .map(|o| SavedOffer {
                    a: (o.cost_a.name().into(), o.cost_a.count),
                    b: o.cost_b.as_ref().map(|b| (b.name().into(), b.count)),
                    result: (o.result.name().into(), o.result.count),
                    result_nbt: hex(&o.result),
                    uses: o.uses,
                    max_uses: o.max_uses,
                    xp: o.xp,
                    mult: o.price_mult,
                })
                .collect(),
        }
    }

    pub fn load(s: SavedVillager) -> Self {
        let offers = s
            .offers
            .into_iter()
            .filter_map(|o| {
                let mut result = stack(&o.result.0, o.result.1)?;
                result.nbt = o.result_nbt.and_then(|h| {
                    let bytes: Option<Vec<u8>> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect();
                    crate::protocol::Nbt::from_network(&bytes?).map(Box::new)
                });
                Some(Offer { cost_a: stack(&o.a.0, o.a.1)?, cost_b: o.b.and_then(|b| stack(&b.0, b.1)), result, uses: o.uses, max_uses: o.max_uses, xp: o.xp, price_mult: o.mult, demand: 0, special: 0 })
            })
            .collect();
        Self { profession: s.profession, level: s.level.clamp(1, 5), xp: s.xp, vtype: s.vtype, offers, restock_at: 0 }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedVillager {
    profession: String,
    level: i32,
    xp: i32,
    vtype: i32,
    offers: Vec<SavedOffer>,
}

#[derive(Serialize, Deserialize)]
struct SavedOffer {
    a: (String, i8),
    b: Option<(String, i8)>,
    result: (String, i8),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result_nbt: Option<String>,
    uses: i32,
    max_uses: i32,
    xp: i32,
    mult: f32,
}

/// The offers as this player sees them: Hero of the Village takes
/// (0.3 + 0.0625 x level) of the first price off (at least one), like vanilla.
fn for_player(player: &Player, offers: &[Offer]) -> Vec<Offer> {
    let hero = super::effects::level(&player.state.lock().unwrap(), "hero_of_the_village");
    offers
        .iter()
        .map(|o| {
            let mut o = o.clone();
            if let Some(amp) = hero {
                let off = ((0.3 + 0.0625 * amp as f32) * o.cost_a.count as f32).floor() as i32;
                o.special = -off.max(1);
            }
            o
        })
        .collect()
}

fn trade_list(window: u8, offers: &[Offer], v: &VillagerData) -> Vec<u8> {
    let mut p = packet(id::play::TRADE_LIST);
    p.varint(window as i32).varint(offers.len() as i32);
    for o in offers {
        crate::item::write_slot(&mut p, &Some(o.cost_a.clone()));
        crate::item::write_slot(&mut p, &Some(o.result.clone()));
        crate::item::write_slot(&mut p, &o.cost_b);
        p.bool(o.out_of_stock()).i32(o.uses).i32(o.max_uses).i32(o.xp).i32(o.special).f32(o.price_mult).i32(o.demand);
    }
    p.varint(v.level).varint(v.xp).bool(true).bool(true);
    p
}

/// Right click on a villager: opens its trading window.
pub fn open_trades(server: &Arc<Server>, player: &Arc<Player>, eid: i32) {
    let data = {
        let mut mobs = server.mobs.lock().unwrap();
        let Some(m) = mobs.get_mut(&eid).filter(|m| m.kind == MobKind::Villager) else { return };
        let ppos = {
            let st = player.state.lock().unwrap();
            [st.x, st.y, st.z]
        };
        let Some(v) = m.villager.clone() else { return };
        if v.offers.is_empty() {
            return;
        }
        m.busy = 1200;
        let (dx, dz) = (ppos[0] - m.pos[0], ppos[2] - m.pos[2]);
        m.yaw = (-dx.atan2(dz)).to_degrees() as f32;
        (v, m.pos)
    };
    let (v, pos) = data;
    let title = format!("{} - {}", profession_title(&v.profession), ["Novato", "Aprendiz", "Oficial", "Experto", "Maestro"][(v.level - 1).clamp(0, 4) as usize]);
    let offers = for_player(player, &v.offers);
    let mut st = player.state.lock().unwrap();
    let packets = st.inv.open(WindowKind::Merchant, crate::protocol::BlockPos::new(pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32), vec![None; 3], &title);
    let wid = st.inv.window_id();
    if let Some(w) = st.inv.window.as_mut() {
        w.station.offers = offers.clone();
        w.station.merchant = eid;
    }
    for p in packets {
        player.send(p);
    }
    player.send(trade_list(wid, &offers, &v));
    let _ = text;
}

/// `select_trade`: picks an offer and fills the payment slots with what is needed from the inventory.
pub fn select(player: &Arc<Player>, index: i32) {
    let mut st = player.state.lock().unwrap();
    let (levels, creative) = (st.inv.levels, st.inv.creative);
    let Some(w) = st.inv.window.as_mut().filter(|w| w.kind == WindowKind::Merchant) else { return };
    let Some(offer) = usize::try_from(index).ok().and_then(|i| w.station.offers.get(i)).cloned() else { return };
    w.station.selected = index;
    // Whatever was in the payment slots goes back to the inventory.
    let back: Vec<ItemStack> = w.slots[0..2].iter_mut().filter_map(Option::take).collect();
    for b in back {
        if let (Some(rest), _) = st.inv.add(b) {
            let w = st.inv.window.as_mut().unwrap();
            if w.slots[0].is_none() {
                w.slots[0] = Some(rest);
            } else {
                w.slots[1] = Some(rest);
            }
        }
    }
    // And they are refilled with what the offer requires.
    let wants: Vec<(usize, ItemStack)> = std::iter::once((0, offer.cost_a.clone())).chain(offer.cost_b.clone().map(|b| (1, b))).collect();
    for (slot, want) in wants {
        let mut need = want.max_stack() as i32;
        let mut got = 0;
        for i in (9..45).rev() {
            if need == 0 {
                break;
            }
            if let Some(s) = &mut st.inv.slots[i] {
                if s.id == want.id && s.nbt.is_none() {
                    let n = (s.count as i32).min(need);
                    s.count -= n as i8;
                    got += n;
                    need -= n;
                    if s.count <= 0 {
                        st.inv.slots[i] = None;
                    }
                }
            }
        }
        if got > 0 {
            st.inv.window.as_mut().unwrap().slots[slot] = Some(want.with_count(got as i8));
        }
    }
    super::stations::compute(st.inv.window.as_mut().unwrap(), levels, creative);
    let p = st.inv.window_items();
    player.send(p);
}

/// After a trade: offer uses, villager (and player) experience, and level.
pub fn traded(server: &Arc<Server>, player: &Arc<Player>, index: usize, _xp: i32) {
    super::advancements::grant(server, player, "adventure/trade");
    let Some(eid) = player.state.lock().unwrap().inv.window.as_ref().filter(|w| w.kind == WindowKind::Merchant).map(|w| w.station.merchant) else { return };
    let (leveled, data, pos, dim) = {
        let mut mobs = server.mobs.lock().unwrap();
        let Some(m) = mobs.get_mut(&eid) else { return };
        let Some(v) = m.villager.as_mut() else { return };
        let Some(o) = v.offers.get_mut(index) else { return };
        o.uses += 1;
        v.xp += o.xp;
        v.restock_at = m.age + 2400;
        let mut leveled = false;
        while v.level < 5 && v.xp >= LEVEL_XP[v.level as usize] {
            v.level += 1;
            let lvl = v.level;
            v.add_offers(lvl);
            leveled = true;
        }
        m.busy = 1200;
        (leveled, (**v).clone(), m.center(), m.dim)
    };
    // Experience for the player (3-6 per trade, like vanilla).
    entities::spawn_xp(server, dim, [pos[0], pos[1] + 0.5, pos[2]], 3 + (entities::random() * 4.0) as i32);
    if leveled {
        super::sound::entity(server, eid, "entity.villager.celebrate", super::sound::NEUTRAL, 1.0, 1.0);
        if let Some(m) = server.mobs.lock().unwrap().get(&eid) {
            server.broadcast_tracked(eid, &super::mobs::metadata(m), true);
        }
    } else {
        super::sound::entity(server, eid, "entity.villager.yes", super::sound::NEUTRAL, 1.0, 1.0);
    }
    let offers = for_player(player, &data.offers);
    let mut st = player.state.lock().unwrap();
    let wid = st.inv.window_id();
    if let Some(w) = st.inv.window.as_mut() {
        w.station.offers = offers.clone();
    }
    player.send(trade_list(wid, &offers, &data));
}

/// Restock: a few minutes after trading, sold-out offers come back.
pub fn restock(server: &Server) {
    let mut mobs = server.mobs.lock().unwrap();
    for m in mobs.values_mut().filter(|m| m.kind == MobKind::Villager) {
        let age = m.age;
        if let Some(v) = m.villager.as_mut() {
            if v.restock_at != 0 && age >= v.restock_at {
                v.restock_at = 0;
                for o in &mut v.offers {
                    o.uses = 0;
                }
            }
        }
    }
}

// ---------- villages ----------

fn villages_file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("villages.json")
}

/// Already populated villages (so they are not repopulated if their villagers are killed).
fn populated(server: &Server) -> std::sync::MutexGuard<'_, Option<std::collections::HashSet<(i32, i32)>>> {
    let mut g = server.villages.lock().unwrap();
    if g.is_none() {
        let list: Vec<(i32, i32)> = std::fs::read_to_string(villages_file(server)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        *g = Some(list.into_iter().collect());
    }
    g
}

/// The first time a player approaches a generated village, its villagers spawn.
pub fn populate_villages(server: &Arc<Server>, seen: &[Seen]) {
    let mut todo = Vec::new();
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld) {
        let centers = server.world(Dimension::Overworld).lock().unwrap().generator().village_centers_near(s.pos[0] as i32, s.pos[2] as i32, 64);
        let mut done = populated(server);
        for c in centers {
            if done.as_mut().unwrap().insert(c) {
                todo.push(c);
            }
        }
    }
    if todo.is_empty() {
        return;
    }
    let list: Vec<(i32, i32)> = populated(server).as_ref().unwrap().iter().copied().collect();
    let _ = std::fs::write(villages_file(server), serde_json::to_string(&list).unwrap_or_default());
    for (x, z) in todo {
        let (spots, vtype) = {
            let mut world = server.world(Dimension::Overworld).lock().unwrap();
            let biome = world.generator().biome_name(x, z);
            let vtype = match biome {
                "desert" => 0,
                "jungle" | "sparse_jungle" | "bamboo_jungle" => 1,
                "savanna" | "savanna_plateau" => 3,
                b if b.contains("snowy") || b.contains("frozen") => 4,
                "swamp" => 5,
                "taiga" | "old_growth_pine_taiga" => 6,
                _ => 2,
            };
            let mut spots = Vec::new();
            for i in 0..5 {
                let a = i as f64 * 1.3;
                let (sx, sz) = (x + (a.cos() * 6.0) as i32, z + (a.sin() * 6.0) as i32);
                let y = world.generator().surface(sx, sz);
                let reg = data::get();
                let mut yy = y + 4;
                while yy > y - 8 && reg.is_air(world.get_block(crate::protocol::BlockPos::new(sx, yy - 1, sz))) {
                    yy -= 1;
                }
                spots.push([sx as f64 + 0.5, yy as f64, sz as f64 + 0.5]);
            }
            (spots, vtype)
        };
        tracing::info!("aldea en {x} {z}: aparecen {} aldeanos", spots.len());
        for p in spots {
            let eid = super::mobs::spawn(server, Dimension::Overworld, MobKind::Villager, p);
            if let Some(v) = server.mobs.lock().unwrap().get_mut(&eid).and_then(|m| m.villager.as_mut()) {
                v.vtype = vtype;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_tables_use_real_items() {
        for (p, _, _) in PROFESSIONS {
            for t in trades(p) {
                for n in [t.1, t.3, t.5] {
                    assert!(n.is_empty() || data::get().item_id(n).is_some(), "{p}: {n}");
                }
            }
            let v = VillagerData::with_profession(p);
            assert!(!v.offers.is_empty(), "{p} sin ofertas");
            let back = VillagerData::load(v.save());
            assert_eq!(back.offers, v.offers);
        }
    }
}

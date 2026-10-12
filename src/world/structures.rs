//! Generated structures: their blocks, loot chests and mob spawners.
//!
//! A structure is computed once, from the seed, as a list of blocks plus the chests and
//! spawners it contains; each chunk takes the part that falls inside it while it is being
//! generated (so a structure can span many chunks and still line up). Chest contents come
//! from simplified vanilla loot tables, rolled deterministically from the chest position.
//!
//! - Dungeons: a mossy cobblestone room underground with a zombie, skeleton or spider
//!   spawner and one or two chests.
//! - Strongholds (3 in a first ring at 1280-2816 blocks from the origin, 6 in a second one
//!   at 4352-5888): stone brick corridors, a crossing, a library, a storeroom, cells, and the
//!   End portal room (12 frames, some with eyes, lava under the portal, a silverfish
//!   spawner).
//! - Desert pyramids, pillager outposts (Overworld) and Nether fortresses, one candidate per
//!   region of chunks (see `region_spot`).

use super::blockentity::BlockEntity;
use super::chunk::Chunk;
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::BlockPos;

pub fn hash(seed: u64, a: i64, b: i64, c: i64) -> u64 {
    let mut z = seed
        ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

pub(crate) fn st(name: &str) -> u16 {
    data::get().state(name)
}

pub(crate) fn with(name: &str, props: &[(&str, &str)]) -> u16 {
    let reg = data::get();
    let Some(b) = reg.block(name) else { return 0 };
    props.iter().fold(b.default_state, |s, (k, v)| b.with(s, k, v))
}

/// Which loot table a chest uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Loot {
    Dungeon,
    StrongholdCorridor,
    StrongholdLibrary,
    StrongholdCrossing,
    DesertPyramid,
    NetherFortress,
    Outpost,
    EndCity,
    Mineshaft,
    JungleTemple,
    ShipwreckSupply,
    ShipwreckTreasure,
    OceanRuin,
    RuinedPortal,
    Bastion,
    BastionTreasure,
    AncientCity,
    Igloo,
    Mansion,
}

/// (item, weight, min, max)
type Table = &'static [(&'static str, u32, i8, i8)];

impl Loot {
    /// Pools: (table, min rolls, max rolls).
    fn pools(self) -> Vec<(Table, u32, u32)> {
        match self {
            Loot::Dungeon => vec![
                (
                    &[
                        ("saddle", 20, 1, 1),
                        ("golden_apple", 15, 1, 1),
                        ("enchanted_golden_apple", 2, 1, 1),
                        ("music_disc_13", 15, 1, 1),
                        ("music_disc_cat", 15, 1, 1),
                        ("name_tag", 20, 1, 1),
                        ("golden_horse_armor", 10, 1, 1),
                        ("iron_horse_armor", 15, 1, 1),
                        ("diamond_horse_armor", 5, 1, 1),
                        ("book", 10, 1, 1),
                    ],
                    1,
                    3,
                ),
                (
                    &[
                        ("iron_ingot", 10, 1, 4),
                        ("gold_ingot", 5, 1, 4),
                        ("bread", 20, 1, 1),
                        ("wheat", 20, 1, 4),
                        ("bucket", 10, 1, 1),
                        ("redstone", 15, 1, 4),
                        ("coal", 15, 1, 4),
                        ("melon_seeds", 10, 2, 4),
                        ("pumpkin_seeds", 10, 2, 4),
                        ("beetroot_seeds", 10, 2, 4),
                    ],
                    1,
                    4,
                ),
                (&[("bone", 10, 1, 8), ("gunpowder", 10, 1, 8), ("rotten_flesh", 10, 1, 8), ("string", 10, 1, 8)], 3, 3),
            ],
            Loot::StrongholdCorridor => vec![(
                &[
                    ("ender_pearl", 10, 1, 1),
                    ("diamond", 3, 1, 3),
                    ("iron_ingot", 10, 1, 5),
                    ("gold_ingot", 5, 1, 3),
                    ("redstone", 5, 4, 9),
                    ("bread", 15, 1, 3),
                    ("apple", 15, 1, 3),
                    ("iron_pickaxe", 5, 1, 1),
                    ("iron_sword", 5, 1, 1),
                    ("iron_chestplate", 5, 1, 1),
                    ("iron_helmet", 5, 1, 1),
                    ("iron_leggings", 5, 1, 1),
                    ("iron_boots", 5, 1, 1),
                    ("golden_apple", 1, 1, 1),
                    ("saddle", 1, 1, 1),
                    ("iron_horse_armor", 1, 1, 1),
                    ("book", 1, 1, 1),
                ],
                2,
                3,
            )],
            Loot::StrongholdCrossing => vec![(
                &[
                    ("iron_ingot", 10, 1, 5),
                    ("gold_ingot", 5, 1, 3),
                    ("redstone", 5, 4, 9),
                    ("coal", 10, 3, 8),
                    ("bread", 15, 1, 3),
                    ("apple", 15, 1, 3),
                    ("iron_pickaxe", 1, 1, 1),
                    ("book", 1, 1, 1),
                ],
                1,
                4,
            )],
            Loot::StrongholdLibrary => vec![(&[("book", 20, 1, 3), ("paper", 20, 2, 7), ("map", 1, 1, 1), ("compass", 1, 1, 1), ("writable_book", 5, 1, 1)], 2, 10)],
            Loot::DesertPyramid => vec![
                (
                    &[
                        ("diamond", 5, 1, 3),
                        ("iron_ingot", 15, 1, 5),
                        ("gold_ingot", 15, 2, 7),
                        ("emerald", 15, 1, 3),
                        ("bone", 25, 4, 6),
                        ("spider_eye", 25, 1, 3),
                        ("rotten_flesh", 25, 3, 7),
                        ("saddle", 20, 1, 1),
                        ("iron_horse_armor", 15, 1, 1),
                        ("golden_horse_armor", 10, 1, 1),
                        ("diamond_horse_armor", 5, 1, 1),
                        ("book", 20, 1, 1),
                        ("golden_apple", 20, 1, 1),
                        ("enchanted_golden_apple", 2, 1, 1),
                    ],
                    2,
                    4,
                ),
                (&[("bone", 10, 1, 8), ("gunpowder", 10, 1, 8), ("rotten_flesh", 10, 1, 8), ("string", 10, 1, 8), ("sand", 10, 1, 8)], 4, 4),
            ],
            Loot::NetherFortress => vec![(
                &[
                    ("diamond", 5, 1, 3),
                    ("iron_ingot", 5, 1, 5),
                    ("gold_ingot", 15, 1, 3),
                    ("golden_sword", 5, 1, 1),
                    ("golden_chestplate", 5, 1, 1),
                    ("flint_and_steel", 5, 1, 1),
                    ("nether_wart", 5, 3, 7),
                    ("saddle", 10, 1, 1),
                    ("golden_horse_armor", 8, 1, 1),
                    ("iron_horse_armor", 5, 1, 1),
                    ("diamond_horse_armor", 3, 1, 1),
                    ("obsidian", 2, 2, 4),
                ],
                2,
                4,
            )],
            Loot::EndCity => vec![(
                &[
                    ("diamond", 5, 2, 7),
                    ("iron_ingot", 10, 4, 8),
                    ("gold_ingot", 15, 2, 7),
                    ("emerald", 2, 2, 6),
                    ("beetroot_seeds", 5, 1, 10),
                    ("saddle", 3, 1, 1),
                    ("iron_horse_armor", 1, 1, 1),
                    ("golden_horse_armor", 1, 1, 1),
                    ("diamond_horse_armor", 1, 1, 1),
                    ("diamond_sword", 3, 1, 1),
                    ("diamond_boots", 3, 1, 1),
                    ("diamond_chestplate", 3, 1, 1),
                    ("diamond_leggings", 3, 1, 1),
                    ("diamond_helmet", 3, 1, 1),
                    ("diamond_pickaxe", 3, 1, 1),
                    ("diamond_shovel", 3, 1, 1),
                    ("iron_sword", 3, 1, 1),
                    ("iron_chestplate", 3, 1, 1),
                    ("iron_pickaxe", 3, 1, 1),
                    ("spire_armor_trim_smithing_template", 1, 1, 1),
                ],
                2,
                6,
            )],
            Loot::Mineshaft => vec![
                (
                    &[
                        ("golden_apple", 20, 1, 1),
                        ("enchanted_golden_apple", 1, 1, 1),
                        ("name_tag", 30, 1, 1),
                        ("book", 10, 1, 1),
                        ("iron_pickaxe", 5, 1, 1),
                        ("iron_ingot", 10, 1, 5),
                        ("gold_ingot", 5, 1, 3),
                        ("redstone", 5, 4, 9),
                        ("lapis_lazuli", 5, 4, 9),
                        ("diamond", 3, 1, 2),
                        ("coal", 10, 3, 8),
                        ("bread", 15, 1, 3),
                        ("glow_berries", 15, 3, 6),
                        ("melon_seeds", 10, 2, 4),
                        ("pumpkin_seeds", 10, 2, 4),
                    ],
                    2,
                    4,
                ),
                (&[("rail", 20, 4, 8), ("powered_rail", 5, 1, 4), ("detector_rail", 5, 1, 4), ("activator_rail", 5, 1, 4), ("torch", 15, 1, 16)], 3, 3),
            ],
            Loot::JungleTemple => vec![(
                &[
                    ("diamond", 3, 1, 3),
                    ("iron_ingot", 10, 1, 5),
                    ("gold_ingot", 15, 2, 7),
                    ("emerald", 2, 1, 3),
                    ("bone", 20, 4, 6),
                    ("rotten_flesh", 16, 3, 7),
                    ("saddle", 3, 1, 1),
                    ("iron_horse_armor", 1, 1, 1),
                    ("golden_horse_armor", 1, 1, 1),
                    ("diamond_horse_armor", 1, 1, 1),
                    ("book", 1, 1, 1),
                    ("wild_armor_trim_smithing_template", 2, 1, 1),
                ],
                2,
                6,
            )],
            Loot::ShipwreckSupply => vec![(
                &[
                    ("paper", 8, 1, 12),
                    ("potato", 7, 2, 6),
                    ("poisonous_potato", 7, 2, 6),
                    ("carrot", 7, 4, 8),
                    ("wheat", 7, 8, 21),
                    ("coal", 6, 2, 8),
                    ("rotten_flesh", 5, 5, 24),
                    ("pumpkin", 2, 1, 3),
                    ("bamboo", 2, 1, 3),
                    ("gunpowder", 3, 1, 5),
                    ("tnt", 1, 1, 2),
                    ("leather_helmet", 3, 1, 1),
                    ("leather_chestplate", 3, 1, 1),
                    ("moss_block", 2, 1, 4),
                ],
                3,
                10,
            )],
            Loot::ShipwreckTreasure => vec![
                (&[("iron_ingot", 90, 1, 5), ("gold_ingot", 10, 1, 5), ("emerald", 40, 1, 5), ("diamond", 5, 1, 1), ("experience_bottle", 5, 1, 1)], 3, 6),
                (&[("iron_nugget", 50, 1, 10), ("gold_nugget", 10, 1, 10), ("lapis_lazuli", 20, 1, 10)], 2, 5),
                (&[("coast_armor_trim_smithing_template", 1, 1, 1), ("map", 2, 1, 1)], 0, 1),
            ],
            Loot::OceanRuin => vec![(
                &[
                    ("coal", 10, 1, 4),
                    ("stone_axe", 2, 1, 1),
                    ("rotten_flesh", 5, 1, 1),
                    ("emerald", 1, 1, 1),
                    ("wheat", 10, 2, 3),
                    ("gold_nugget", 5, 1, 3),
                    ("golden_apple", 2, 1, 1),
                    ("book", 1, 1, 1),
                    ("fishing_rod", 1, 1, 1),
                    ("map", 1, 1, 1),
                ],
                2,
                8,
            )],
            Loot::RuinedPortal => vec![(
                &[
                    ("obsidian", 40, 1, 2),
                    ("flint", 40, 1, 4),
                    ("iron_nugget", 40, 9, 18),
                    ("flint_and_steel", 40, 1, 1),
                    ("fire_charge", 40, 1, 1),
                    ("golden_apple", 15, 1, 1),
                    ("gold_nugget", 15, 4, 24),
                    ("golden_sword", 15, 1, 1),
                    ("golden_axe", 15, 1, 1),
                    ("golden_hoe", 15, 1, 1),
                    ("golden_shovel", 15, 1, 1),
                    ("golden_pickaxe", 15, 1, 1),
                    ("golden_boots", 15, 1, 1),
                    ("golden_chestplate", 15, 1, 1),
                    ("golden_helmet", 15, 1, 1),
                    ("golden_leggings", 15, 1, 1),
                    ("glistering_melon_slice", 5, 4, 12),
                    ("golden_horse_armor", 5, 1, 1),
                    ("light_weighted_pressure_plate", 5, 1, 1),
                    ("golden_carrot", 5, 4, 12),
                    ("clock", 5, 1, 1),
                    ("gold_ingot", 5, 2, 8),
                    ("bell", 1, 1, 1),
                    ("enchanted_golden_apple", 1, 1, 1),
                    ("gold_block", 1, 1, 2),
                ],
                4,
                8,
            )],
            Loot::Bastion => vec![
                (
                    &[
                        ("gold_ingot", 12, 1, 6),
                        ("gold_nugget", 12, 2, 8),
                        ("magma_cream", 8, 2, 6),
                        ("crying_obsidian", 8, 1, 5),
                        ("spectral_arrow", 8, 4, 8),
                        ("gilded_blackstone", 6, 2, 7),
                        ("chain", 6, 2, 10),
                        ("cooked_porkchop", 6, 2, 7),
                        ("crossbow", 4, 1, 1),
                        ("golden_axe", 4, 1, 1),
                        ("iron_ingot", 6, 1, 4),
                        ("piglin_banner_pattern", 1, 1, 1),
                        ("music_disc_pigstep", 1, 1, 1),
                        ("snout_armor_trim_smithing_template", 2, 1, 1),
                    ],
                    4,
                    6,
                ),
            ],
            Loot::BastionTreasure => vec![
                (&[("netherite_ingot", 15, 1, 1), ("ancient_debris", 10, 1, 2), ("netherite_scrap", 8, 1, 1), ("diamond", 10, 2, 6), ("enchanted_golden_apple", 2, 1, 1), ("diamond_sword", 5, 1, 1), ("diamond_chestplate", 5, 1, 1)], 3, 3),
                (&[("gold_block", 10, 2, 5), ("gold_ingot", 10, 3, 9), ("crying_obsidian", 5, 3, 5), ("spectral_arrow", 5, 12, 25), ("netherite_upgrade_smithing_template", 3, 1, 1)], 3, 4),
            ],
            Loot::AncientCity => vec![
                (
                    &[
                        ("enchanted_golden_apple", 1, 1, 2),
                        ("music_disc_otherside", 1, 1, 1),
                        ("compass", 2, 1, 1),
                        ("sculk_catalyst", 2, 1, 2),
                        ("name_tag", 2, 1, 1),
                        ("diamond_hoe", 2, 1, 1),
                        ("lead", 2, 1, 1),
                        ("diamond_horse_armor", 2, 1, 1),
                        ("saddle", 2, 1, 1),
                        ("music_disc_13", 2, 1, 1),
                        ("music_disc_cat", 2, 1, 1),
                        ("diamond_leggings", 2, 1, 1),
                        ("book", 3, 3, 3),
                        ("sculk", 3, 4, 10),
                        ("sculk_sensor", 3, 1, 3),
                        ("candle", 3, 1, 4),
                        ("amethyst_shard", 3, 1, 15),
                        ("experience_bottle", 3, 1, 3),
                        ("glow_berries", 3, 1, 15),
                        ("echo_shard", 4, 1, 3),
                        ("disc_fragment_5", 4, 1, 3),
                        ("ward_armor_trim_smithing_template", 1, 1, 1),
                        ("silence_armor_trim_smithing_template", 1, 1, 1),
                    ],
                    5,
                    10,
                ),
                (&[("soul_torch", 1, 1, 15), ("bone", 1, 1, 15), ("coal", 1, 6, 15)], 1, 1),
            ],
            Loot::Igloo => vec![
                (&[("apple", 3, 1, 3), ("coal", 15, 1, 4), ("gold_nugget", 10, 1, 3), ("stone_axe", 2, 1, 1), ("rotten_flesh", 10, 1, 1), ("emerald", 1, 1, 1), ("wheat", 10, 2, 3)], 2, 8),
                (&[("golden_apple", 1, 1, 1)], 1, 1),
            ],
            Loot::Mansion => vec![
                (&[("lead", 20, 1, 1), ("golden_apple", 15, 1, 1), ("enchanted_golden_apple", 2, 1, 1), ("music_disc_13", 15, 1, 1), ("music_disc_cat", 15, 1, 1), ("name_tag", 20, 1, 1), ("chainmail_chestplate", 10, 1, 1), ("diamond_hoe", 15, 1, 1), ("diamond_chestplate", 5, 1, 1), ("book", 10, 1, 3)], 1, 3),
                (&[("iron_ingot", 10, 1, 4), ("gold_ingot", 5, 1, 4), ("bread", 20, 1, 1), ("wheat", 20, 1, 4), ("bucket", 10, 1, 1), ("redstone", 15, 1, 4), ("coal", 15, 1, 4), ("melon_seeds", 10, 2, 4), ("pumpkin_seeds", 10, 2, 4), ("beetroot_seeds", 10, 2, 4)], 1, 4),
                (&[("bone", 10, 1, 8), ("gunpowder", 10, 1, 8), ("rotten_flesh", 10, 1, 8), ("string", 10, 1, 8)], 3, 3),
            ],
            Loot::Outpost => vec![
                (&[("crossbow", 1, 1, 1)], 0, 1),
                (&[("wheat", 7, 3, 5), ("potato", 5, 2, 5), ("carrot", 5, 3, 5)], 2, 3),
                (&[("dark_oak_log", 1, 2, 3)], 1, 3),
                (&[("experience_bottle", 7, 1, 1), ("string", 4, 1, 6), ("arrow", 4, 2, 7), ("tripwire_hook", 3, 1, 3), ("iron_ingot", 3, 1, 3), ("book", 1, 1, 1)], 2, 3),
            ],
        }
    }

    /// The 27 slots of a chest, rolled from `seed` (the same chest always gets the same loot).
    pub fn fill(self, seed: u64) -> Vec<Slot> {
        let reg = data::get();
        let mut out: Vec<Slot> = vec![None; 27];
        let mut n = 0i64;
        let mut next = || {
            n += 1;
            hash(seed, n, 7, 13)
        };
        for (table, lo, hi) in self.pools() {
            let rolls = lo + (next() % (hi - lo + 1) as u64) as u32;
            let total: u32 = table.iter().map(|t| t.1).sum();
            for _ in 0..rolls {
                let mut pick = (next() % total as u64) as u32;
                let Some(&(item, _, min, max)) = table.iter().find(|t| {
                    if pick < t.1 {
                        true
                    } else {
                        pick -= t.1;
                        false
                    }
                }) else {
                    continue;
                };
                let Some(id) = reg.item_id(item) else { continue };
                let count = min + (next() % (max - min + 1) as u64) as i8;
                // A free random slot.
                for _ in 0..27 {
                    let slot = (next() % 27) as usize;
                    if out[slot].is_none() {
                        out[slot] = Some(ItemStack::new(id, count));
                        break;
                    }
                }
            }
        }
        out
    }
}

/// Blocks, chests and spawners of a structure (world coordinates).
#[derive(Default)]
pub struct Structure {
    pub blocks: Vec<(i32, i32, i32, u16)>,
    pub chests: Vec<(BlockPos, Loot)>,
    pub spawners: Vec<(BlockPos, &'static str)>,
    /// Bounding box (min, max), for mobs that only spawn inside it.
    pub bounds: Option<(BlockPos, BlockPos)>,
    /// Mobs that live there, placed the first time a player comes close (shulkers, elder
    /// guardians, piglins...).
    pub mobs: Vec<([f64; 3], &'static str)>,
    /// Item frames with what they hold: (block the frame is in, direction it faces, item).
    pub frames: Vec<(BlockPos, i32, &'static str)>,
    /// What kind of structure (for `/locate` and mob spawning).
    pub kind: &'static str,
    /// Dispensers loaded with arrows (traps).
    pub traps: Vec<BlockPos>,
}

impl Structure {
    pub(crate) fn of(kind: &'static str) -> Self {
        Self { kind, ..Default::default() }
    }

    pub(crate) fn set(&mut self, x: i32, y: i32, z: i32, s: u16) {
        self.blocks.push((x, y, z, s));
        self.grow(x, y, z);
    }

    fn grow(&mut self, x: i32, y: i32, z: i32) {
        let b = self.bounds.get_or_insert((BlockPos::new(x, y, z), BlockPos::new(x, y, z)));
        b.0 = BlockPos::new(b.0.x.min(x), b.0.y.min(y), b.0.z.min(z));
        b.1 = BlockPos::new(b.1.x.max(x), b.1.y.max(y), b.1.z.max(z));
    }

    /// Fills a box with what `f` says for each block.
    pub(crate) fn fill(&mut self, a: (i32, i32, i32), b: (i32, i32, i32), mut f: impl FnMut(i32, i32, i32) -> u16) {
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                for z in a.2.min(b.2)..=a.2.max(b.2) {
                    let s = f(x, y, z);
                    self.set(x, y, z, s);
                }
            }
        }
    }

    pub(crate) fn chest(&mut self, p: BlockPos, facing: &str, loot: Loot) {
        self.set(p.x, p.y, p.z, with("chest", &[("facing", facing)]));
        self.chests.push((p, loot));
    }

    pub(crate) fn spawner(&mut self, p: BlockPos, mob: &'static str) {
        self.set(p.x, p.y, p.z, st("spawner"));
        self.spawners.push((p, mob));
    }

    /// A stable identity (its bounding box corner), to remember it was populated.
    pub fn id(&self) -> (i32, i32, i32) {
        self.bounds.map_or((0, 0, 0), |(lo, _)| (lo.x, lo.y, lo.z))
    }

    /// Whether a point is inside it (with a margin).
    pub fn contains(&self, x: i32, y: i32, z: i32, margin: i32) -> bool {
        self.bounds.is_some_and(|(lo, hi)| x >= lo.x - margin && x <= hi.x + margin && y >= lo.y - margin && y <= hi.y + margin && z >= lo.z - margin && z <= hi.z + margin)
    }

    /// Writes the part inside chunk (cx, cz).
    pub fn apply(&self, c: &mut Chunk, cx: i32, cz: i32, seed: u64) {
        let (x0, z0) = (cx * 16, cz * 16);
        if let Some((lo, hi)) = self.bounds {
            if hi.x < x0 || lo.x > x0 + 15 || hi.z < z0 || lo.z > z0 + 15 {
                return;
            }
        }
        for &(x, y, z, s) in &self.blocks {
            if x >> 4 == cx && z >> 4 == cz {
                c.set((x - x0) as usize, y, (z - z0) as usize, s);
            }
        }
        for &(p, loot) in &self.chests {
            if p.x >> 4 == cx && p.z >> 4 == cz {
                c.block_entities.insert(p, BlockEntity::Container(loot.fill(hash(seed, p.x as i64, p.y as i64, p.z as i64))));
            }
        }
        for &(p, mob) in &self.spawners {
            if p.x >> 4 == cx && p.z >> 4 == cz {
                c.block_entities.insert(p, BlockEntity::Spawner { mob: mob.to_string(), delay: 20 });
            }
        }
        for &p in &self.traps {
            if p.x >> 4 == cx && p.z >> 4 == cz {
                let mut items = vec![None; 9];
                items[0] = data::get().item_id("arrow").map(|i| ItemStack::new(i, 9));
                c.block_entities.insert(p, BlockEntity::Container(items));
            }
        }
    }
}

/// Stone bricks with some mossy and cracked ones.
fn brick(r: u64) -> u16 {
    match r % 20 {
        0..=2 => st("mossy_stone_bricks"),
        3..=4 => st("cracked_stone_bricks"),
        _ => st("stone_bricks"),
    }
}

// ---------- dungeons ----------

/// A dungeon in chunk (cx, cz), about one chunk in twelve, below `surface` (the lowest
/// surface height around the chunk centre). It fits inside the chunk.
pub fn dungeon(seed: u64, cx: i32, cz: i32, surface: i32) -> Option<Structure> {
    let r = hash(seed ^ 0xD0D0, cx as i64, 0, cz as i64);
    if r % 12 != 0 {
        return None;
    }
    let top = (surface - 10).min(40);
    if top < -45 {
        return None;
    }
    let y0 = -45 + ((r >> 8) % (top + 46) as u64) as i32;
    let (hx, hz) = (2 + ((r >> 20) % 2) as i32, 2 + ((r >> 24) % 2) as i32);
    let (x, z) = (cx * 16 + 8, cz * 16 + 8);
    let mut s = Structure::of("dungeon");
    let mut i = 0i64;
    // Walls, floor and ceiling of (mossy) cobblestone; air inside.
    s.fill((x - hx - 1, y0 - 1, z - hz - 1), (x + hx + 1, y0 + 3, z + hz + 1), |bx, by, bz| {
        i += 1;
        let inside = (bx - x).abs() <= hx && (bz - z).abs() <= hz && by >= y0 && by <= y0 + 2;
        if inside {
            0
        } else if by == y0 - 1 && hash(r, i, 1, 1) % 4 == 0 {
            st("mossy_cobblestone")
        } else {
            st("cobblestone")
        }
    });
    let mob = match (r >> 32) % 4 {
        0 => "skeleton",
        1 => "spider",
        _ => "zombie",
    };
    s.spawner(BlockPos::new(x, y0, z), mob);
    // One or two chests against the walls.
    let chests = 1 + ((r >> 40) % 2) as i32;
    let spots = [(x - hx, z, "east"), (x + hx, z, "west"), (x, z - hz, "south"), (x, z + hz, "north")];
    for k in 0..chests {
        let (px, pz, facing) = spots[((r >> (44 + k * 2)) % 4) as usize];
        if s.chests.iter().any(|c| c.0.x == px && c.0.z == pz) {
            continue;
        }
        s.chest(BlockPos::new(px, y0, pz), facing, Loot::Dungeon);
    }
    Some(s)
}

// ---------- strongholds ----------

/// Stronghold centres: 3 in a first ring, 6 in a second one (like vanilla's first rings).
pub fn stronghold_positions(seed: u64) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (ring, count, lo, hi) in [(0, 3, 1280.0, 2816.0), (1, 6, 4352.0, 5888.0)] {
        let base = unit(hash(seed ^ 0x5701, ring, 0, 0)) * std::f64::consts::TAU;
        for i in 0..count {
            let r = hash(seed ^ 0x5702, ring, i, 0);
            let a = base + i as f64 * std::f64::consts::TAU / count as f64 + (unit(r) - 0.5) * 0.4;
            let d = lo + unit(r >> 7) * (hi - lo);
            out.push(((a.cos() * d) as i32, (a.sin() * d) as i32));
        }
    }
    out
}

/// A stronghold centred at (x, z) with its floor at `y0`.
pub fn stronghold(seed: u64, x: i32, z: i32, y0: i32) -> Structure {
    let mut s = Structure::of("stronghold");
    let r = hash(seed ^ 0x5703, x as i64, y0 as i64, z as i64);
    let mut n = 0i64;
    let mut rnd = move || {
        n += 1;
        hash(r, n, 3, 5)
    };
    // Pieces: (min, max) inner boxes; each is built as a solid shell first and hollowed
    // afterwards, so overlapping pieces open into each other.
    let pieces: Vec<((i32, i32, i32), (i32, i32, i32))> = vec![
        // Crossing.
        ((x - 4, y0, z - 4), (x + 4, y0 + 5, z + 4)),
        // Corridors north, east, west, south.
        ((x - 1, y0, z - 26), (x + 1, y0 + 3, z - 4)),
        ((x + 4, y0, z - 1), (x + 26, y0 + 3, z + 1)),
        ((x - 26, y0, z - 1), (x - 4, y0 + 3, z + 1)),
        ((x - 1, y0, z + 4), (x + 1, y0 + 3, z + 21)),
        // Library (north), storeroom (east), cells (west), portal room (south).
        ((x - 7, y0, z - 40), (x + 7, y0 + 7, z - 27)),
        ((x + 27, y0, z - 4), (x + 35, y0 + 4, z + 4)),
        ((x - 35, y0, z - 4), (x - 27, y0 + 4, z + 4)),
        ((x - 5, y0, z + 22), (x + 5, y0 + 7, z + 36)),
    ];
    for &(a, b) in &pieces {
        s.fill((a.0 - 1, a.1 - 1, a.2 - 1), (b.0 + 1, b.1 + 1, b.2 + 1), |_, _, _| brick(rnd()));
    }
    for &(a, b) in &pieces {
        s.fill(a, b, |_, _, _| 0);
    }
    // Torches along the corridors.
    let torch = |f: &str| with("wall_torch", &[("facing", f)]);
    for k in (8..26).step_by(6) {
        s.set(x - 1, y0 + 2, z - k, torch("east"));
        s.set(x + k, y0 + 2, z - 1, torch("south"));
        s.set(x - k, y0 + 2, z + 1, torch("north"));
        s.set(x + 1, y0 + 2, z + k.min(20), torch("west"));
    }
    // Crossing: a small fountain and a chest.
    s.fill((x - 1, y0, z - 1), (x + 1, y0, z + 1), |bx, _, bz| if bx == x && bz == z { st("water") } else { st("stone_brick_slab") });
    s.chest(BlockPos::new(x + 4, y0, z + 3), "west", Loot::StrongholdCrossing);
    // Library: shelves along the walls, cobwebs, a chest.
    s.fill((x - 7, y0, z - 40), (x + 7, y0 + 3, z - 27), |bx, by, bz| {
        let wall = bx == x - 7 || bx == x + 7 || bz == z - 40;
        let rows = (bx - x).abs() >= 2 && (bx - x).abs() <= 5 && (bz - (z - 37)).rem_euclid(4) == 0 && bz < z - 29;
        if (wall || rows) && by <= y0 + 3 { st("bookshelf") } else { 0 }
    });
    for k in 0..4 {
        let (cx2, cz2) = (x - 6 + (rnd() % 13) as i32, z - 39 + (rnd() % 11) as i32);
        s.set(cx2, y0 + 6, cz2, st("cobweb"));
        let _ = k;
    }
    s.set(x, y0 + 7, z - 33, st("lantern"));
    s.chest(BlockPos::new(x, y0, z - 39), "south", Loot::StrongholdLibrary);
    // Storeroom: two chests, torches.
    s.chest(BlockPos::new(x + 35, y0, z - 2), "west", Loot::StrongholdCorridor);
    s.chest(BlockPos::new(x + 35, y0, z + 2), "west", Loot::StrongholdCorridor);
    s.set(x + 31, y0 + 2, z - 4, torch("south"));
    // Cells: iron bars in front of two small cells, a chest in one of them.
    s.fill((x - 35, y0, z - 1), (x - 31, y0 + 2, z - 1), |_, _, _| st("iron_bars"));
    s.fill((x - 35, y0, z + 1), (x - 31, y0 + 2, z + 1), |_, _, _| st("iron_bars"));
    s.chest(BlockPos::new(x - 34, y0, z - 3), "south", Loot::StrongholdCorridor);
    // Portal room.
    portal_room(&mut s, x, y0, z + 22, &mut rnd);
    s
}

/// The End portal room, its front wall at `z0` (it extends to z0 + 14).
fn portal_room(s: &mut Structure, x: i32, y0: i32, z0: i32, rnd: &mut impl FnMut() -> u64) {
    // Raised platform with lava where the portal goes.
    let c = z0 + 9; // portal centre
    s.fill((x - 3, y0, c - 3), (x + 3, y0, c + 3), |bx, _, bz| if (bx - x).abs() <= 1 && (bz - c).abs() <= 1 { st("lava") } else { brick(0) });
    s.fill((x - 1, y0, c - 4), (x + 1, y0, c - 4), |_, _, _| with("stone_brick_stairs", &[("facing", "south")]));
    // The 12 frames, facing the centre; each has an eye one time in ten.
    let frame = |facing: &str, eye: bool| with("end_portal_frame", &[("facing", facing), ("eye", if eye { "true" } else { "false" })]);
    for d in -1..=1 {
        let e = [rnd() % 10 == 0, rnd() % 10 == 0, rnd() % 10 == 0, rnd() % 10 == 0];
        s.set(x + d, y0 + 1, c - 2, frame("south", e[0]));
        s.set(x + d, y0 + 1, c + 2, frame("north", e[1]));
        s.set(x - 2, y0 + 1, c + d, frame("east", e[2]));
        s.set(x + 2, y0 + 1, c + d, frame("west", e[3]));
    }
    // Lava pools at the sides, the silverfish spawner by the entrance, light.
    s.fill((x - 5, y0, c - 1), (x - 4, y0, c + 1), |_, _, _| st("lava"));
    s.fill((x + 4, y0, c - 1), (x + 5, y0, c + 1), |_, _, _| st("lava"));
    s.spawner(BlockPos::new(x, y0, z0 + 2), "silverfish");
    s.set(x - 5, y0 + 3, z0 + 3, with("wall_torch", &[("facing", "east")]));
    s.set(x + 5, y0 + 3, z0 + 3, with("wall_torch", &[("facing", "west")]));
    s.set(x, y0 + 7, c, st("lantern"));
}

// ---------- placement by regions ----------

/// The candidate spot of region (rx, rz) for a kind of structure (`salt`): one per region of
/// `region` x `region` chunks, away from its edges. Returns (x, z, random bits).
pub fn region_spot(seed: u64, salt: u64, region: i32, rx: i32, rz: i32) -> (i32, i32, u64) {
    let r = hash(seed ^ salt, rx as i64, 0, rz as i64);
    let span = (region - 8).max(1) as u64;
    let x = (rx * region + 4 + (r % span) as i32) * 16 + 8;
    let z = (rz * region + 4 + ((r >> 16) % span) as i32) * 16 + 8;
    (x, z, r)
}

// ---------- desert pyramid ----------

/// A desert pyramid on the ground at height `g`: the hall, and under its blue centre a hidden
/// chamber with four chests and a TNT trap under a pressure plate.
pub fn desert_pyramid(x: i32, z: i32, g: i32) -> Structure {
    let mut s = Structure::of("pyramid");
    let (sand, cut, orange, blue) = (st("sandstone"), st("cut_sandstone"), st("orange_terracotta"), st("blue_terracotta"));
    // Foundation and the stepped body (solid first, hollowed afterwards).
    s.fill((x - 10, g - 4, z - 10), (x + 10, g, z + 10), |_, _, _| sand);
    for k in 0..10 {
        let h = 10 - k;
        s.fill((x - h, g + 1 + k, z - h), (x + h, g + 1 + k, z + h), |_, _, _| sand);
    }
    // Hall and entrance (north).
    s.fill((x - 5, g + 1, z - 5), (x + 5, g + 5, z + 5), |_, _, _| 0);
    s.fill((x - 1, g + 1, z - 10), (x + 1, g + 3, z - 5), |_, _, _| 0);
    for k in 0..4 {
        s.set(x - 2, g + 1 + k, z - 10, cut);
        s.set(x + 2, g + 1 + k, z - 10, cut);
    }
    // Terracotta star on the floor; blue in the centre.
    s.fill((x - 2, g, z - 2), (x + 2, g, z + 2), |bx, _, bz| if bx == x && bz == z { blue } else if (bx - x).abs() == (bz - z).abs() { orange } else { sand });
    // Hidden chamber below.
    let cy = g - 13;
    s.fill((x - 5, cy - 2, z - 5), (x + 5, cy + 4, z + 5), |_, _, _| cut);
    s.fill((x - 4, cy, z - 4), (x + 4, cy + 3, z + 4), |_, _, _| 0);
    s.fill((x - 1, cy - 1, z - 1), (x + 1, cy - 1, z + 1), |_, _, _| st("tnt"));
    s.set(x, cy, z, st("stone_pressure_plate"));
    s.chest(BlockPos::new(x, cy, z - 4), "south", Loot::DesertPyramid);
    s.chest(BlockPos::new(x, cy, z + 4), "north", Loot::DesertPyramid);
    s.chest(BlockPos::new(x - 4, cy, z), "east", Loot::DesertPyramid);
    s.chest(BlockPos::new(x + 4, cy, z), "west", Loot::DesertPyramid);
    s
}

// ---------- pillager outpost ----------

/// A pillager outpost: a dark oak tower with three floors, a ladder and a lookout with the
/// loot chest at the top.
pub fn outpost(x: i32, z: i32, g: i32) -> Structure {
    let mut s = Structure::of("outpost");
    let (planks, cobble, log) = (st("dark_oak_planks"), st("cobblestone"), with("dark_oak_log", &[("axis", "y")]));
    s.fill((x - 4, g - 3, z - 4), (x + 4, g, z + 4), |_, _, _| cobble);
    // Walls with log corners, floors every 5 blocks.
    s.fill((x - 4, g + 1, z - 4), (x + 4, g + 15, z + 4), |bx, _, bz| if (bx - x).abs() == 4 && (bz - z).abs() == 4 { log } else { planks });
    s.fill((x - 3, g + 1, z - 3), (x + 3, g + 15, z + 3), |_, by, _| if (by - g) % 5 == 0 { planks } else { 0 });
    // Door, windows, ladder and its hole through the floors.
    s.fill((x, g + 1, z - 4), (x, g + 2, z - 4), |_, _, _| 0);
    for f in [g + 7, g + 12] {
        for (wx, wz) in [(x, z - 4), (x, z + 4), (x - 4, z), (x + 4, z)] {
            s.set(wx, f, wz, 0);
            s.set(wx, f + 1, wz, 0);
        }
    }
    let ladder = with("ladder", &[("facing", "west")]);
    for y in g + 1..=g + 16 {
        s.set(x + 3, y, z, ladder);
    }
    // Lookout: a wider floor, fences, a slab roof on log posts, the chest.
    s.fill((x - 5, g + 16, z - 5), (x + 5, g + 16, z + 5), |bx, _, bz| if bx == x + 3 && bz == z { ladder } else { planks });
    s.fill((x - 5, g + 17, z - 5), (x + 5, g + 17, z + 5), |bx, _, bz| if (bx - x).abs() == 5 || (bz - z).abs() == 5 { st("dark_oak_fence") } else { 0 });
    for (px, pz) in [(x - 5, z - 5), (x + 5, z - 5), (x - 5, z + 5), (x + 5, z + 5)] {
        for y in g + 17..=g + 20 {
            s.set(px, y, pz, log);
        }
    }
    s.fill((x - 5, g + 21, z - 5), (x + 5, g + 21, z + 5), |_, _, _| st("dark_oak_slab"));
    s.chest(BlockPos::new(x, g + 17, z - 2), "south", Loot::Outpost);
    s.set(x, g + 20, z, st("lantern"));
    s
}

// ---------- nether fortress ----------

/// A Nether fortress around (x, z) with its deck at `y0`: four bridges on pillars, a covered
/// crossing with a chest, two blaze spawner platforms and a nether wart garden.
pub fn nether_fortress(seed: u64, x: i32, z: i32, y0: i32) -> Structure {
    let mut s = Structure::of("fortress");
    let r = hash(seed ^ 0xF047, x as i64, y0 as i64, z as i64);
    let (brick, fence) = (st("nether_bricks"), st("nether_brick_fence"));
    const ARM: i32 = 36;
    // Bridges: deck 5 wide, fences on both edges, air above, pillars down every 8 blocks.
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        for d in 5..=ARM {
            for w in -2i32..=2 {
                let (bx, bz) = if dx != 0 { (x + dx * d, z + w) } else { (x + w, z + dz * d) };
                s.set(bx, y0 - 1, bz, brick);
                for y in y0..=y0 + 3 {
                    s.set(bx, y, bz, if y == y0 && w.abs() == 2 { fence } else { 0 });
                }
                if d % 8 == 0 && w.abs() == 2 {
                    for y in 32..y0 - 1 {
                        s.set(bx, y, bz, brick);
                    }
                }
            }
        }
    }
    // Covered crossing in the middle with a chest.
    s.fill((x - 5, y0 - 1, z - 5), (x + 5, y0 + 5, z + 5), |_, _, _| brick);
    s.fill((x - 4, y0, z - 4), (x + 4, y0 + 4, z + 4), |_, _, _| 0);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        for w in -1..=1 {
            for y in y0..=y0 + 2 {
                s.set(x + dx * 5 + if dx == 0 { w } else { 0 }, y, z + dz * 5 + if dz == 0 { w } else { 0 }, 0);
            }
        }
    }
    for y in 32..y0 - 1 {
        for (px, pz) in [(x - 5, z - 5), (x + 5, z - 5), (x - 5, z + 5), (x + 5, z + 5)] {
            s.set(px, y, pz, brick);
        }
    }
    s.chest(BlockPos::new(x + 4, y0, z + 4), "west", Loot::NetherFortress);
    // Ends of the bridges: two blaze platforms (east and south), the wart garden (west),
    // a small room with another chest (north).
    for (cx, cz) in [(x + ARM + 5, z), (x, z + ARM + 5)] {
        s.fill((cx - 5, y0 - 1, cz - 5), (cx + 5, y0 - 1, cz + 5), |_, _, _| brick);
        s.fill((cx - 5, y0, cz - 5), (cx + 5, y0 + 4, cz + 5), |bx, by, bz| if by == y0 && ((bx - cx).abs() == 5 || (bz - cz).abs() == 5) { fence } else { 0 });
        s.fill((cx - 1, y0, cz - 1), (cx + 1, y0, cz + 1), |_, _, _| brick);
        s.spawner(BlockPos::new(cx, y0 + 1, cz), "blaze");
        for y in 32..y0 - 1 {
            for (px, pz) in [(cx - 5, cz - 5), (cx + 5, cz - 5), (cx - 5, cz + 5), (cx + 5, cz + 5)] {
                s.set(px, y, pz, brick);
            }
        }
    }
    // Wart garden (west).
    let (gx, gz) = (x - ARM - 5, z);
    s.fill((gx - 5, y0 - 1, gz - 5), (gx + 5, y0 + 5, gz + 5), |_, _, _| brick);
    s.fill((gx - 4, y0, gz - 4), (gx + 4, y0 + 4, gz + 4), |_, _, _| 0);
    s.fill((gx + 5, y0, gz - 1), (gx + 5, y0 + 2, gz + 1), |_, _, _| 0);
    if let Some(w) = data::get().block("nether_wart") {
        let mut k = 0i64;
        for bx in gx - 3..=gx + 3 {
            for bz in [gz - 3, gz - 2, gz + 2, gz + 3] {
                k += 1;
                s.set(bx, y0 - 1, bz, st("soul_sand"));
                s.set(bx, y0, bz, w.with(w.default_state, "age", &(hash(r, k, 1, 2) % 4).to_string()));
            }
        }
    }
    // Small room with a chest (north).
    let (nx, nz) = (x, z - ARM - 4);
    s.fill((nx - 4, y0 - 1, nz - 4), (nx + 4, y0 + 4, nz + 4), |_, _, _| brick);
    s.fill((nx - 3, y0, nz - 3), (nx + 3, y0 + 3, nz + 3), |_, _, _| 0);
    s.fill((nx - 1, y0, nz + 4), (nx + 1, y0 + 2, nz + 4), |_, _, _| 0);
    s.chest(BlockPos::new(nx, y0, nz - 3), "south", Loot::NetherFortress);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loot_is_deterministic_and_not_empty() {
        let a = Loot::Dungeon.fill(42);
        assert_eq!(a, Loot::Dungeon.fill(42));
        assert!(a.iter().flatten().count() >= 4);
        assert!(Loot::StrongholdLibrary.fill(7).iter().flatten().count() >= 2);
    }

    #[test]
    fn strongholds_in_rings() {
        let p = stronghold_positions(20200420);
        assert_eq!(p.len(), 9);
        for (i, &(x, z)) in p.iter().enumerate() {
            let d = ((x * x + z * z) as f64).sqrt();
            if i < 3 { assert!((1270.0..2830.0).contains(&d), "{d}") } else { assert!((4340.0..5900.0).contains(&d), "{d}") }
        }
    }

    #[test]
    fn portal_room_has_twelve_frames() {
        let s = stronghold(1, 0, 20, 0);
        let frame = data::get().block("end_portal_frame").unwrap();
        let mut frames: Vec<_> = s.blocks.iter().filter(|b| b.3 >= frame.min_state && b.3 <= frame.max_state).map(|b| (b.0, b.1, b.2)).collect();
        frames.sort();
        frames.dedup();
        assert_eq!(frames.len(), 12);
        assert!(s.spawners.iter().any(|sp| sp.1 == "silverfish"));
        assert!(s.chests.len() >= 4);
    }
}

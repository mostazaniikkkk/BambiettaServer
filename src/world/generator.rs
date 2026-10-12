//! Terrain generators.
//!
//! - Overworld (`noise`): noise-driven climate (continentalness, erosion, temperature, humidity,
//!   rivers), ~20 biomes with their own surface and vegetation, caves (caverns and tunnels from 3D noise),
//!   lava at the bottom, ores and stone variants, per-biome trees that cross chunk
//!   borders, and villages.
//! - Nether: netherrack with caverns, a lava sea, quartz, gold, glowstone and soul sand
//!   valleys.
//! - End: central end stone island with obsidian pillars and an active exit portal,
//!   plus outer islands.
//! - `flat`: superflat.
//!
//! Everything is deterministic from the seed and the coordinates: each chunk can be
//! generated independently and structures (trees, villages) line up across borders.

use super::chunk::{Chunk, MIN_Y};
use super::Dimension;
use crate::config::Generation;
use crate::data::{self, Block};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const SEA_LEVEL: i32 = 62;

pub enum Generator {
    Flat(Vec<(u16, i32)>),
    Overworld(Box<Overworld>),
    Nether(Box<NetherGen>),
    End(Box<EndGen>),
}

impl Generator {
    pub fn for_dimension(dim: Dimension, cfg: &Generation) -> Self {
        let reg = data::get();
        let seed = cfg.options.seed as u64;
        match dim {
            Dimension::Nether => Generator::Nether(Box::new(NetherGen::new(seed))),
            Dimension::End => Generator::End(Box::new(EndGen::new(seed))),
            Dimension::Overworld => match cfg.name.as_str() {
                "flat" | "superflat" => Generator::Flat(cfg.options.layers.iter().map(|(name, n)| (reg.state(name), *n)).collect()),
                _ => Generator::Overworld(Box::new(Overworld::new(seed))),
            },
        }
    }

    pub fn generate(&self, cx: i32, cz: i32) -> Chunk {
        let mut c = match self {
            Generator::Flat(layers) => {
                let mut c = Chunk::new();
                let mut y = MIN_Y;
                for &(state, n) in layers {
                    for _ in 0..n {
                        for z in 0..16 {
                            for x in 0..16 {
                                c.set(x, y, z, state);
                            }
                        }
                        y += 1;
                    }
                }
                c
            }
            Generator::Overworld(g) => g.generate(cx, cz),
            Generator::Nether(g) => g.generate(cx, cz),
            Generator::End(g) => g.generate(cx, cz),
        };
        c.compact();
        c.dirty = false;
        c
    }

    /// Safe spawn height (first free block) at (x, z).
    pub fn surface(&self, x: i32, z: i32) -> i32 {
        match self {
            Generator::Flat(layers) => MIN_Y + layers.iter().map(|l| l.1).sum::<i32>(),
            Generator::Overworld(g) => g.height(x, z).max(SEA_LEVEL) + 1,
            Generator::Nether(_) => 64,
            Generator::End(_) => 65,
        }
    }

    /// Stronghold centres (normal overworld only; for eyes of ender).
    pub fn strongholds(&self) -> Vec<(i32, i32)> {
        match self {
            Generator::Overworld(g) => g.strongholds().to_vec(),
            _ => vec![],
        }
    }

    /// Nearest structure of a kind (`stronghold`, `dungeon`, `village`) to (x, z): a point
    /// of it (the spawner for dungeons).
    pub fn locate(&self, kind: &str, x: i32, z: i32) -> Option<(i32, i32, i32)> {
        if let Generator::Nether(n) = self {
            return match kind {
                "fortress" => n.nearest_fortress(x, z, 6),
                "bastion" => n.bastions.nearest(n.seed, x, z, 6, &|x, z, r| n.build_bastion(x, z, r)),
                _ => None,
            };
        }
        if let Generator::End(e) = self {
            return (kind == "end_city").then(|| e.cities.nearest(e.seed, x, z, 8, &|x, z, r| e.build_city(x, z, r))).flatten();
        }
        let Generator::Overworld(g) = self else { return None };
        match kind {
            k if Feature::from_name(k).is_some() => g.nearest_feature(Feature::from_name(k).unwrap(), x, z, 8),
            "stronghold" => g.strongholds().iter().min_by_key(|(sx, sz)| ((*sx - x) as i64).pow(2) + ((*sz - z) as i64).pow(2)).map(|&(sx, sz)| (sx, g.stronghold_floor(sx, sz), sz)),
            "village" => g.village_centers_near(x, z, 2000).into_iter().min_by_key(|(vx, vz)| ((*vx - x) as i64).pow(2) + ((*vz - z) as i64).pow(2)).map(|(vx, vz)| (vx, g.height(vx, vz), vz)),
            "dungeon" => g.dungeon_near(x, z, 24),
            _ => None,
        }
    }

    /// Whether a point is inside a structure where special mobs spawn (`fortress` in the
    /// Nether, `outpost` in the Overworld).
    pub fn in_structure(&self, kind: &str, x: i32, y: i32, z: i32) -> bool {
        match (self, kind) {
            (Generator::Nether(n), "fortress") => n.in_fortress(x, y, z),
            (Generator::Nether(n), "bastion") => n.bastions.near(n.seed, x >> 4, z >> 4, &|x, z, r| n.build_bastion(x, z, r)).iter().any(|s| s.contains(x, y, z, 2)),
            (Generator::End(e), "end_city") => e.cities.near(e.seed, x >> 4, z >> 4, &|x, z, r| e.build_city(x, z, r)).iter().any(|s| s.contains(x, y, z, 2)),
            (Generator::Overworld(g), "outpost") => g.in_feature(Feature::Outpost, x, z, 24),
            (Generator::Overworld(g), k) => Feature::from_name(k).is_some_and(|f| g.in_feature_3d(f, x, y, z, 2)),
            _ => false,
        }
    }

    /// Structures around (x, z) (to place the mobs and item frames that live in them).
    pub fn sites_near(&self, x: i32, z: i32) -> Vec<Arc<super::structures::Structure>> {
        let (cx, cz) = (x >> 4, z >> 4);
        match self {
            Generator::Overworld(g) => g.features_near(cx, cz),
            Generator::Nether(n) => n.bastions.near(n.seed, cx, cz, &|x, z, r| n.build_bastion(x, z, r)),
            Generator::End(e) => e.cities.near(e.seed, cx, cz, &|x, z, r| e.build_city(x, z, r)),
            Generator::Flat(_) => vec![],
        }
    }

    /// Top of the End island at (x, z), if there is one (for gateway exits).
    pub fn end_island_top(&self, x: i32, z: i32) -> Option<i32> {
        match self {
            Generator::End(e) => e.island_top(x, z),
            _ => None,
        }
    }

    /// Wells of nearby villages (normal overworld only).
    pub fn village_centers_near(&self, x: i32, z: i32, radius: i32) -> Vec<(i32, i32)> {
        match self {
            Generator::Overworld(g) => g.village_centers_near(x, z, radius),
            _ => vec![],
        }
    }

    /// Biome name at (x, z) (for `/locate`-like use and tests).
    pub fn biome_name(&self, x: i32, z: i32) -> &'static str {
        match self {
            Generator::Overworld(g) => {
                let c = g.climate(x, z);
                g.biome(&c, g.height_from(&c, x, z)).name()
            }
            Generator::Nether(_) => "nether_wastes",
            Generator::End(_) => "the_end",
            Generator::Flat(_) => "plains",
        }
    }
}

// ---------- region sites ----------

/// Structures placed one per region (at most): a cache of what each region holds, built on
/// demand from the seed.
pub struct Sites {
    region: i32,
    salt: u64,
    /// How far from its spot a structure of this kind can reach (to know which chunks it touches).
    reach: i32,
    cache: Mutex<HashMap<(i32, i32), Option<Arc<super::structures::Structure>>>>,
}

type Build<'a> = &'a dyn Fn(i32, i32, u64) -> Option<super::structures::Structure>;

impl Sites {
    pub fn new(region: i32, salt: u64, reach: i32) -> Self {
        Self { region, salt, reach, cache: Mutex::new(HashMap::new()) }
    }

    fn get(&self, seed: u64, rx: i32, rz: i32, build: Build) -> Option<Arc<super::structures::Structure>> {
        if let Some(s) = self.cache.lock().unwrap().get(&(rx, rz)) {
            return s.clone();
        }
        let (x, z, r) = super::structures::region_spot(seed, self.salt, self.region, rx, rz);
        let built = build(x, z, r).map(Arc::new);
        self.cache.lock().unwrap().insert((rx, rz), built.clone());
        built
    }

    /// Those that may touch chunk (cx, cz).
    pub fn near(&self, seed: u64, cx: i32, cz: i32, build: Build) -> Vec<Arc<super::structures::Structure>> {
        let (rx, rz) = (cx.div_euclid(self.region), cz.div_euclid(self.region));
        let span = 1 + self.reach / (self.region * 16);
        let mut out = Vec::new();
        for dx in -span..=span {
            for dz in -span..=span {
                let (x, z, _) = super::structures::region_spot(seed, self.salt, self.region, rx + dx, rz + dz);
                if (x - (cx * 16 + 8)).abs() > self.reach + 8 || (z - (cz * 16 + 8)).abs() > self.reach + 8 {
                    continue;
                }
                if let Some(s) = self.get(seed, rx + dx, rz + dz, build) {
                    out.push(s);
                }
            }
        }
        out
    }

    /// The nearest one within `regions` regions: a point in it.
    pub fn nearest(&self, seed: u64, x: i32, z: i32, regions: i32, build: Build) -> Option<(i32, i32, i32)> {
        let (rx, rz) = ((x >> 4).div_euclid(self.region), (z >> 4).div_euclid(self.region));
        let mut best: Option<(i64, (i32, i32, i32))> = None;
        for dx in -regions..=regions {
            for dz in -regions..=regions {
                let Some(s) = self.get(seed, rx + dx, rz + dz, build) else { continue };
                let (fx, fz, _) = super::structures::region_spot(seed, self.salt, self.region, rx + dx, rz + dz);
                let y = s.bounds.map_or(64, |(lo, hi)| (lo.y + hi.y) / 2);
                let d = ((fx - x) as i64).pow(2) + ((fz - z) as i64).pow(2);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, (fx, y, fz)));
                }
            }
        }
        best.map(|b| b.1)
    }
}

// ---------- noise ----------

fn hash(seed: u64, a: i64, b: i64, c: i64) -> u64 {
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

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy)]
struct Noise {
    seed: u64,
}

impl Noise {
    fn lattice2(&self, o: i64, x: i64, z: i64) -> f64 {
        unit(hash(self.seed, o * 7919 + x, z, o))
    }

    /// 2D value noise in [0, 1).
    fn n2(&self, o: i64, x: f64, z: f64) -> f64 {
        let (x0, z0) = (x.floor(), z.floor());
        let (sx, sz) = (smooth(x - x0), smooth(z - z0));
        let (xi, zi) = (x0 as i64, z0 as i64);
        let a = self.lattice2(o, xi, zi);
        let b = self.lattice2(o, xi + 1, zi);
        let c = self.lattice2(o, xi, zi + 1);
        let d = self.lattice2(o, xi + 1, zi + 1);
        let top = a + (b - a) * sx;
        top + (c + (d - c) * sx - top) * sz
    }

    /// 2D fractal noise (3 octaves) in [0, 1).
    fn fbm2(&self, o: i64, x: f64, z: f64) -> f64 {
        (self.n2(o, x, z) * 4.0 + self.n2(o + 100, x * 2.0, z * 2.0) * 2.0 + self.n2(o + 200, x * 4.0, z * 4.0)) / 7.0
    }

    fn lattice3(&self, o: i64, x: i64, y: i64, z: i64) -> f64 {
        unit(hash(self.seed ^ (o as u64).wrapping_mul(0x632B_E59B_D9B4_E019), x, y, z))
    }

    /// 3D value noise in [0, 1).
    fn n3(&self, o: i64, x: f64, y: f64, z: f64) -> f64 {
        let (x0, y0, z0) = (x.floor(), y.floor(), z.floor());
        let (sx, sy, sz) = (smooth(x - x0), smooth(y - y0), smooth(z - z0));
        let (xi, yi, zi) = (x0 as i64, y0 as i64, z0 as i64);
        let l = |dx, dy, dz| self.lattice3(o, xi + dx, yi + dy, zi + dz);
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        let x00 = lerp(l(0, 0, 0), l(1, 0, 0), sx);
        let x10 = lerp(l(0, 1, 0), l(1, 1, 0), sx);
        let x01 = lerp(l(0, 0, 1), l(1, 0, 1), sx);
        let x11 = lerp(l(0, 1, 1), l(1, 1, 1), sx);
        lerp(lerp(x00, x10, sy), lerp(x01, x11, sy), sz)
    }
}

/// 3D field sampled on a 4-block lattice and interpolated (fast and deterministic: the
/// value at a point does not depend on which chunk computes it).
struct Lattice<const N: usize> {
    origin: (i32, i32, i32),
    size: (usize, usize, usize),
    values: Vec<[f32; N]>,
}

impl<const N: usize> Lattice<N> {
    fn build(x0: i32, y0: i32, z0: i32, w: usize, h: usize, f: impl Fn(i32, i32, i32) -> [f32; N]) -> Self {
        let (lx, ly, lz) = (x0.div_euclid(4), y0.div_euclid(4), z0.div_euclid(4));
        let size = (w / 4 + 2, h / 4 + 2, w / 4 + 2);
        let mut values = Vec::with_capacity(size.0 * size.1 * size.2);
        for i in 0..size.0 {
            for j in 0..size.1 {
                for k in 0..size.2 {
                    values.push(f(lx + i as i32, ly + j as i32, lz + k as i32));
                }
            }
        }
        Lattice { origin: (lx, ly, lz), size, values }
    }

    fn at(&self, x: i32, y: i32, z: i32) -> [f32; N] {
        let (fx, fy, fz) = (x as f32 / 4.0, y as f32 / 4.0, z as f32 / 4.0);
        let (ix, iy, iz) = (fx.floor() as i32, fy.floor() as i32, fz.floor() as i32);
        let (tx, ty, tz) = (fx - ix as f32, fy - iy as f32, fz - iz as f32);
        let (i, j, k) = ((ix - self.origin.0) as usize, (iy - self.origin.1) as usize, (iz - self.origin.2) as usize);
        let g = |a: usize, b: usize, c: usize| &self.values[((i + a) * self.size.1 + (j + b)) * self.size.2 + (k + c)];
        let mut out = [0.0; N];
        for (n, o) in out.iter_mut().enumerate() {
            let v = |a, b, c| g(a, b, c)[n];
            let x00 = v(0, 0, 0) + (v(1, 0, 0) - v(0, 0, 0)) * tx;
            let x10 = v(0, 1, 0) + (v(1, 1, 0) - v(0, 1, 0)) * tx;
            let x01 = v(0, 0, 1) + (v(1, 0, 1) - v(0, 0, 1)) * tx;
            let x11 = v(0, 1, 1) + (v(1, 1, 1) - v(0, 1, 1)) * tx;
            let y0 = x00 + (x10 - x00) * ty;
            let y1 = x01 + (x11 - x01) * ty;
            *o = y0 + (y1 - y0) * tz;
        }
        out
    }
}

/// Interpolated value of a 3D field at a single point (same result as `Lattice::at`).
fn lattice_point<const N: usize>(x: i32, y: i32, z: i32, f: impl Fn(i32, i32, i32) -> [f32; N]) -> [f32; N] {
    Lattice::build(x, y, z, 1, 1, f).at(x, y, z)
}

fn st(name: &str) -> u16 {
    data::get().state(name)
}

fn with(name: &str, props: &[(&str, &str)]) -> u16 {
    let reg = data::get();
    let Some(b) = reg.block(name) else { return 0 };
    props.iter().fold(b.default_state, |s, (k, v)| b.with(s, k, v))
}

fn block_of(state: u16) -> &'static Block {
    data::get().block_of_state(state)
}

// ---------- biomes ----------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Biome {
    Ocean,
    DeepOcean,
    FrozenOcean,
    WarmOcean,
    Beach,
    SnowyBeach,
    River,
    FrozenRiver,
    Plains,
    SunflowerPlains,
    Forest,
    FlowerForest,
    BirchForest,
    DarkForest,
    Taiga,
    SnowyTaiga,
    SnowyPlains,
    Desert,
    Savanna,
    Jungle,
    Swamp,
    WindsweptHills,
    SnowySlopes,
    StonyPeaks,
}

impl Biome {
    pub fn name(self) -> &'static str {
        use Biome::*;
        match self {
            Ocean => "ocean",
            DeepOcean => "deep_ocean",
            FrozenOcean => "frozen_ocean",
            WarmOcean => "warm_ocean",
            Beach => "beach",
            SnowyBeach => "snowy_beach",
            River => "river",
            FrozenRiver => "frozen_river",
            Plains => "plains",
            SunflowerPlains => "sunflower_plains",
            Forest => "forest",
            FlowerForest => "flower_forest",
            BirchForest => "birch_forest",
            DarkForest => "dark_forest",
            Taiga => "taiga",
            SnowyTaiga => "snowy_taiga",
            SnowyPlains => "snowy_plains",
            Desert => "desert",
            Savanna => "savanna",
            Jungle => "jungle",
            Swamp => "swamp",
            WindsweptHills => "windswept_hills",
            SnowySlopes => "snowy_slopes",
            StonyPeaks => "stony_peaks",
        }
    }

    pub fn water(self) -> bool {
        matches!(self, Biome::Ocean | Biome::DeepOcean | Biome::FrozenOcean | Biome::WarmOcean | Biome::River | Biome::FrozenRiver)
    }

    fn snowy(self) -> bool {
        matches!(self, Biome::SnowyPlains | Biome::SnowyTaiga | Biome::SnowyBeach | Biome::FrozenOcean | Biome::FrozenRiver | Biome::SnowySlopes)
    }

    /// Trees per chunk (average) and type.
    fn trees(self) -> (f64, &'static [TreeKind]) {
        use Biome::{BirchForest, DarkForest, FlowerForest, Forest, Plains, Savanna, SnowyPlains, SnowyTaiga, SunflowerPlains, Swamp, Taiga, WindsweptHills};
        use TreeKind::{Acacia, Birch, Bush, DarkOak, Oak, Spruce, SwampOak};
        match self {
            Plains | SunflowerPlains => (0.4, &[Oak]),
            Forest => (7.0, &[Oak, Oak, Oak, Birch]),
            FlowerForest => (4.0, &[Oak, Birch]),
            BirchForest => (7.0, &[Birch]),
            DarkForest => (9.0, &[DarkOak, DarkOak, Oak]),
            Taiga => (6.0, &[Spruce]),
            SnowyTaiga => (5.0, &[Spruce]),
            SnowyPlains => (0.25, &[Spruce]),
            Savanna => (1.2, &[Acacia, Acacia, Oak]),
            Biome::Jungle => (10.0, &[TreeKind::Jungle, TreeKind::Jungle, Bush]),
            Swamp => (2.5, &[SwampOak]),
            WindsweptHills => (1.5, &[Spruce, Oak]),
            _ => (0.0, &[]),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeKind {
    Oak,
    Birch,
    Spruce,
    DarkOak,
    Jungle,
    Acacia,
    SwampOak,
    Bush,
}

struct Tree {
    x: i32,
    y: i32,
    z: i32,
    kind: TreeKind,
    seed: u64,
}

#[derive(Clone, Copy)]
pub struct Climate {
    cont: f64,
    erosion: f64,
    temp: f64,
    humid: f64,
    river: f64,
}

// ---------- overworld ----------

pub struct Overworld {
    seed: u64,
    noise: Noise,
    b: Blocks,
    villages: Mutex<HashMap<(i32, i32), Arc<Vec<(i32, i32, i32, u16)>>>>,
    /// Stronghold centres, and their layouts once computed.
    strongholds: Vec<(i32, i32)>,
    stronghold_cache: Mutex<HashMap<usize, Arc<super::structures::Structure>>>,
    /// Surface structures by (kind, region): desert pyramids and pillager outposts.
    features: Mutex<HashMap<(Feature, i32, i32), Option<Arc<super::structures::Structure>>>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Feature {
    Pyramid,
    Outpost,
    Mineshaft,
    JungleTemple,
    Shipwreck,
    OceanRuin,
    RuinedPortal,
    Monument,
    AncientCity,
}

impl Feature {
    pub const ALL: [Feature; 9] = [
        Feature::Pyramid,
        Feature::Outpost,
        Feature::Mineshaft,
        Feature::JungleTemple,
        Feature::Shipwreck,
        Feature::OceanRuin,
        Feature::RuinedPortal,
        Feature::Monument,
        Feature::AncientCity,
    ];

    /// (region size in chunks, salt)
    fn placement(self) -> (i32, u64) {
        match self {
            Feature::Pyramid => (24, 0xDE5E),
            Feature::Outpost => (30, 0x0075),
            Feature::Mineshaft => (12, 0x3157),
            Feature::JungleTemple => (24, 0x7e3b),
            Feature::Shipwreck => (16, 0x5419),
            Feature::OceanRuin => (14, 0x0c3a),
            Feature::RuinedPortal => (25, 0x9071),
            Feature::Monument => (32, 0x3013),
            Feature::AncientCity => (24, 0xa1c1),
        }
    }

    /// How far it reaches from its spot.
    fn reach(self) -> i32 {
        match self {
            Feature::Mineshaft => 110,
            Feature::AncientCity => 48,
            Feature::Monument => 40,
            _ => 32,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Feature::Pyramid => "pyramid",
            Feature::Outpost => "outpost",
            Feature::Mineshaft => "mineshaft",
            Feature::JungleTemple => "jungle_temple",
            Feature::Shipwreck => "shipwreck",
            Feature::OceanRuin => "ocean_ruin",
            Feature::RuinedPortal => "ruined_portal",
            Feature::Monument => "monument",
            Feature::AncientCity => "ancient_city",
        }
    }

    pub fn from_name(n: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.name() == n)
    }
}

/// States used for each fill block (precomputed).
struct Blocks {
    air: u16,
    bedrock: u16,
    stone: u16,
    deepslate: u16,
    dirt: u16,
    grass: u16,
    snowy_grass: u16,
    sand: u16,
    sandstone: u16,
    gravel: u16,
    clay: u16,
    water: u16,
    lava: u16,
    ice: u16,
    snow_block: u16,
    snow_layer: u16,
    podzol: u16,
    mud: u16,
    variants: [u16; 3],
    ores: [(u16, u16, f32, i32, i32); 7],
}

impl Overworld {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            noise: Noise { seed },
            b: Blocks {
                air: 0,
                bedrock: st("bedrock"),
                stone: st("stone"),
                deepslate: st("deepslate"),
                dirt: st("dirt"),
                grass: st("grass_block"),
                snowy_grass: with("grass_block", &[("snowy", "true")]),
                sand: st("sand"),
                sandstone: st("sandstone"),
                gravel: st("gravel"),
                clay: st("clay"),
                water: st("water"),
                lava: st("lava"),
                ice: st("ice"),
                snow_block: st("snow_block"),
                snow_layer: st("snow"),
                podzol: st("podzol"),
                mud: st("mud"),
                variants: [st("granite"), st("diorite"), st("andesite")],
                // (ore, deepslate ore, chance, min y, max y)
                ores: [
                    (st("coal_ore"), st("deepslate_coal_ore"), 0.012, 0, 192),
                    (st("iron_ore"), st("deepslate_iron_ore"), 0.009, -64, 80),
                    (st("copper_ore"), st("deepslate_copper_ore"), 0.008, -16, 112),
                    (st("gold_ore"), st("deepslate_gold_ore"), 0.0025, -64, 32),
                    (st("redstone_ore"), st("deepslate_redstone_ore"), 0.004, -64, 16),
                    (st("lapis_ore"), st("deepslate_lapis_ore"), 0.0015, -64, 64),
                    (st("diamond_ore"), st("deepslate_diamond_ore"), 0.0012, -64, 16),
                ],
            },
            villages: Mutex::new(HashMap::new()),
            strongholds: super::structures::stronghold_positions(seed),
            stronghold_cache: Mutex::new(HashMap::new()),
            features: Mutex::new(HashMap::new()),
        }
    }

    /// The surface structure of a kind in region (rx, rz), if the terrain there suits it.
    fn feature(&self, kind: Feature, rx: i32, rz: i32) -> Option<Arc<super::structures::Structure>> {
        if let Some(f) = self.features.lock().unwrap().get(&(kind, rx, rz)) {
            return f.clone();
        }
        let (region, salt) = kind.placement();
        let (x, z, r) = super::structures::region_spot(self.seed, salt, region, rx, rz);
        let cl = self.climate(x, z);
        let g = self.height_from(&cl, x, z);
        let biome = self.biome(&cl, g);
        let heights: Vec<i32> = [(-10, -10), (10, -10), (-10, 10), (10, 10), (0, 0)].iter().map(|(dx, dz)| self.height(x + dx, z + dz)).collect();
        let flat = heights.iter().max().unwrap() - heights.iter().min().unwrap();
        use Biome::*;
        let low = *heights.iter().min().unwrap();
        let ocean = matches!(biome, Ocean | DeepOcean | WarmOcean | FrozenOcean);
        let seed = self.seed ^ r;
        use super::buildings as b;
        let built = match kind {
            Feature::Mineshaft if r % 2 == 0 => {
                let y = 10 + ((r >> 8) % 30) as i32;
                (low - 12 > y).then(|| b::mineshaft(x, y, z, seed))
            }
            Feature::JungleTemple if biome == Jungle && flat <= 6 && g > SEA_LEVEL => Some(b::jungle_temple(x, z, low, seed)),
            Feature::Shipwreck if ocean && g < SEA_LEVEL - 4 => Some(b::shipwreck(x, z, g, true, seed)),
            Feature::Shipwreck if matches!(biome, Beach | SnowyBeach) && (r >> 30) % 2 == 0 => Some(b::shipwreck(x, z, g, false, seed)),
            Feature::OceanRuin if ocean && g < SEA_LEVEL - 3 => Some(b::ocean_ruin(x, z, g, biome == WarmOcean, seed)),
            Feature::RuinedPortal if !ocean && !biome.water() && g > SEA_LEVEL && (r >> 32) % 2 == 0 => Some(b::ruined_portal(x, z, low, seed)),
            Feature::Monument if matches!(biome, Ocean | DeepOcean) && g < SEA_LEVEL - 12 => Some(b::monument(x, z, g, SEA_LEVEL, seed)),
            Feature::AncientCity if (r >> 24) % 2 == 0 => Some(b::ancient_city(x, z, -51, seed)),
            Feature::Pyramid if biome == Desert && flat <= 5 && g > SEA_LEVEL => Some(super::structures::desert_pyramid(x, z, *heights.iter().min().unwrap())),
            Feature::Outpost
                if matches!(biome, Plains | SunflowerPlains | Savanna | Desert | Taiga | SnowyPlains) && flat <= 6 && g > SEA_LEVEL && (r >> 40) % 3 != 0 && self.village_centers_near(x, z, 200).is_empty() =>
            {
                Some(super::structures::outpost(x, z, g))
            }
            _ => None,
        }
        .map(Arc::new);
        self.features.lock().unwrap().insert((kind, rx, rz), built.clone());
        built
    }

    /// Structures whose area touches chunk (cx, cz).
    pub fn features_near(&self, cx: i32, cz: i32) -> Vec<Arc<super::structures::Structure>> {
        let mut out = Vec::new();
        for kind in Feature::ALL {
            let (region, salt) = kind.placement();
            let reach = kind.reach() + 8;
            let span = 1 + kind.reach() / (region * 16);
            let (rx, rz) = (cx.div_euclid(region), cz.div_euclid(region));
            for dx in -span..=span {
                for dz in -span..=span {
                    let (x, z, _) = super::structures::region_spot(self.seed, salt, region, rx + dx, rz + dz);
                    if (x - (cx * 16 + 8)).abs() > reach || (z - (cz * 16 + 8)).abs() > reach {
                        continue;
                    }
                    if let Some(f) = self.feature(kind, rx + dx, rz + dz) {
                        out.push(f);
                    }
                }
            }
        }
        out
    }

    /// Nearest surface structure of a kind within `regions` regions: its centre.
    pub fn nearest_feature(&self, kind: Feature, x: i32, z: i32, regions: i32) -> Option<(i32, i32, i32)> {
        let (region, salt) = kind.placement();
        let (rx, rz) = ((x >> 4).div_euclid(region), (z >> 4).div_euclid(region));
        let mut best: Option<(i64, (i32, i32, i32))> = None;
        for dx in -regions..=regions {
            for dz in -regions..=regions {
                if self.feature(kind, rx + dx, rz + dz).is_none() {
                    continue;
                }
                let (fx, fz, _) = super::structures::region_spot(self.seed, salt, region, rx + dx, rz + dz);
                let d = ((fx - x) as i64).pow(2) + ((fz - z) as i64).pow(2);
                let fy = match kind {
                    Feature::Mineshaft | Feature::AncientCity => self.feature(kind, rx + dx, rz + dz).and_then(|s| s.bounds).map_or(0, |(lo, hi)| (lo.y + hi.y) / 2),
                    _ => self.height(fx, fz),
                };
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, (fx, fy, fz)));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Whether (x, z) lies within a structure of that kind (with some margin around it).
    pub fn in_feature(&self, kind: Feature, x: i32, z: i32, margin: i32) -> bool {
        self.features_near(x >> 4, z >> 4).iter().any(|f| {
            f.bounds.is_some_and(|(lo, hi)| x >= lo.x - margin && x <= hi.x + margin && z >= lo.z - margin && z <= hi.z + margin)
                && self.feature_kind_matches(f, kind)
        })
    }

    fn feature_kind_matches(&self, f: &super::structures::Structure, kind: Feature) -> bool {
        f.kind == kind.name()
    }

    /// Whether (x, y, z) is inside a structure of that kind.
    pub fn in_feature_3d(&self, kind: Feature, x: i32, y: i32, z: i32, margin: i32) -> bool {
        self.features_near(x >> 4, z >> 4).iter().any(|f| f.kind == kind.name() && f.contains(x, y, z, margin))
    }

    pub fn strongholds(&self) -> &[(i32, i32)] {
        &self.strongholds
    }

    /// Floor height of the stronghold at (x, z): deep enough under the lowest ground around.
    pub fn stronghold_floor(&self, x: i32, z: i32) -> i32 {
        let ground = [(0, 0), (40, 0), (-40, 0), (0, 40), (0, -40)].iter().map(|(dx, dz)| self.height(x + dx, z + dz)).min().unwrap_or(40);
        (ground - 30).clamp(-40, 20)
    }

    /// The spawner of the nearest dungeon within `radius` chunks.
    pub fn dungeon_near(&self, x: i32, z: i32, radius: i32) -> Option<(i32, i32, i32)> {
        let (cx0, cz0) = (x >> 4, z >> 4);
        for r in 0..=radius {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let (cx, cz) = (cx0 + dx, cz0 + dz);
                    let (x0, z0) = (cx * 16, cz * 16);
                    let surface = [(0, 0), (15, 0), (0, 15), (15, 15)].iter().map(|(a, b)| self.height(x0 + a, z0 + b)).min().unwrap_or(SEA_LEVEL);
                    if let Some(d) = super::structures::dungeon(self.seed, cx, cz, surface) {
                        return d.spawners.first().map(|s| (s.0.x, s.0.y, s.0.z));
                    }
                }
            }
        }
        None
    }

    /// Strongholds whose area touches chunk (cx, cz).
    fn strongholds_near(&self, cx: i32, cz: i32) -> Vec<Arc<super::structures::Structure>> {
        let mut out = Vec::new();
        for (i, &(x, z)) in self.strongholds.iter().enumerate() {
            let (bx, bz) = (cx * 16 + 8, cz * 16 + 8);
            if (x - bx).abs() > 64 || (z - bz).abs() > 64 {
                continue;
            }
            let mut cache = self.stronghold_cache.lock().unwrap();
            let s = cache.entry(i).or_insert_with(|| Arc::new(super::structures::stronghold(self.seed, x, z, self.stronghold_floor(x, z))));
            out.push(s.clone());
        }
        out
    }

    pub fn climate(&self, x: i32, z: i32) -> Climate {
        let n = &self.noise;
        let (x, z) = (x as f64, z as f64);
        Climate {
            cont: n.fbm2(1, x / 640.0, z / 640.0),
            erosion: n.fbm2(5, x / 300.0, z / 300.0),
            temp: n.fbm2(6, x / 520.0, z / 520.0),
            humid: n.fbm2(7, x / 440.0, z / 440.0),
            river: (n.fbm2(8, x / 260.0, z / 260.0) - 0.5).abs(),
        }
    }

    pub fn height(&self, x: i32, z: i32) -> i32 {
        let c = self.climate(x, z);
        self.height_from(&c, x, z)
    }

    fn height_from(&self, c: &Climate, x: i32, z: i32) -> i32 {
        let n = &self.noise;
        let (fx, fz) = (x as f64, z as f64);
        // Continentalness: deep oceans, coasts and inland.
        // Slightly more land than sea (~30 % ocean).
        let cont = ((c.cont - 0.44) * 2.6).clamp(-1.0, 1.0);
        let base = 66.0 + cont * 24.0;
        let hills = n.fbm2(2, fx / 90.0, fz / 90.0) * 0.7 + n.n2(4, fx / 18.0, fz / 18.0) * 0.3;
        let rugged = (1.0 - c.erosion).powi(3) * 2.2;
        let amp = if cont > 0.0 { 6.0 + rugged * 55.0 * cont.min(1.0) } else { 6.0 };
        let mut h = base + (hills - 0.5) * amp * 2.0;
        // Rivers: a valley dipping slightly below sea level.
        let r = c.river / 0.035;
        if r < 1.0 && h > SEA_LEVEL as f64 - 3.0 && cont > -0.2 {
            let bed = SEA_LEVEL as f64 - 4.0;
            h = bed + (h - bed) * smooth(r);
        }
        h as i32
    }

    pub fn biome(&self, c: &Climate, h: i32) -> Biome {
        use Biome::*;
        let cold = c.temp < 0.38;
        if h < SEA_LEVEL - 1 {
            if c.river < 0.02 && c.cont > 0.42 {
                return if cold { FrozenRiver } else { River };
            }
            return match () {
                _ if cold => FrozenOcean,
                _ if c.temp > 0.66 => WarmOcean,
                _ if h < 40 => DeepOcean,
                _ => Ocean,
            };
        }
        if h <= SEA_LEVEL + 2 && c.river >= 0.02 {
            return if cold { SnowyBeach } else { Beach };
        }
        if h > 130 {
            return if c.temp < 0.5 { SnowySlopes } else { StonyPeaks };
        }
        if h > 100 {
            return if cold { SnowySlopes } else { WindsweptHills };
        }
        match () {
            _ if cold && c.humid > 0.5 => SnowyTaiga,
            _ if cold => SnowyPlains,
            _ if c.temp < 0.46 => Taiga,
            _ if c.temp > 0.62 && c.humid < 0.42 => Desert,
            _ if c.temp > 0.6 && c.humid > 0.6 => Jungle,
            _ if c.temp > 0.58 => Savanna,
            _ if c.humid > 0.64 && h < 70 => Swamp,
            _ if c.humid > 0.6 => DarkForest,
            _ if c.humid > 0.54 => BirchForest,
            _ if c.humid > 0.47 => Forest,
            _ if c.humid > 0.44 => FlowerForest,
            _ if c.humid < 0.36 => SunflowerPlains,
            _ => Plains,
        }
    }

    /// Cave fields at a lattice cell: [caverns, tunnel A, tunnel B, stone variant].
    fn cave_lattice(&self, lx: i32, ly: i32, lz: i32) -> [f32; 4] {
        let n = &self.noise;
        let (x, y, z) = (lx as f64, ly as f64, lz as f64);
        [
            n.n3(9, x / 12.0, y / 7.0, z / 12.0) as f32,
            n.n3(10, x / 7.0, y / 4.0, z / 7.0) as f32,
            n.n3(11, x / 7.0, y / 4.0, z / 7.0) as f32,
            n.n3(12, x / 5.0, y / 5.0, z / 5.0) as f32,
        ]
    }

    /// Is there a cave void? `surface`: terrain height in that column.
    fn carved(&self, v: [f32; 4], y: i32, surface: i32, open_top: bool) -> bool {
        if y <= MIN_Y + 4 {
            return false;
        }
        let cavern = v[0] > 0.79 && y < surface - 8;
        let tunnel = (v[1] - 0.5).abs() < 0.035 && (v[2] - 0.5).abs() < 0.07;
        // Tunnels may open to the surface on dry land; caverns may not.
        let depth_ok = y < surface - 4 || (open_top && y <= surface);
        (cavern || tunnel) && depth_ok
    }

    fn generate(&self, cx: i32, cz: i32) -> Chunk {
        let reg = data::get();
        let b = &self.b;
        let mut c = Chunk::new();
        let (x0, z0) = (cx * 16, cz * 16);

        // 1. Climate, height and biome per column.
        let mut heights = [[0i32; 16]; 16];
        let mut biomes = [[Biome::Plains; 16]; 16];
        for x in 0..16 {
            for z in 0..16 {
                let cl = self.climate(x0 + x as i32, z0 + z as i32);
                let h = self.height_from(&cl, x0 + x as i32, z0 + z as i32);
                heights[x][z] = h;
                biomes[x][z] = self.biome(&cl, h);
            }
        }
        for x4 in 0..4 {
            for z4 in 0..4 {
                c.set_biome_column(x4, z4, reg.biome_id(biomes[x4 * 4 + 2][z4 * 4 + 2].name()));
            }
        }

        // 2. 3D cave and stone-variant fields on the 4-block lattice.
        let max_h = heights.iter().flatten().copied().max().unwrap().max(SEA_LEVEL);
        let lattice = Lattice::build(x0, MIN_Y, z0, 16, (max_h - MIN_Y + 8) as usize, |x, y, z| self.cave_lattice(x, y, z));

        // 3. Fill each column.
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = (x0 + x as i32, z0 + z as i32);
                let h = heights[x][z];
                let biome = biomes[x][z];
                let land = !biome.water() && h >= SEA_LEVEL;
                let top_y = h.max(SEA_LEVEL);
                for y in MIN_Y..=top_y {
                    let r = unit(hash(self.seed, wx as i64, y as i64, wz as i64)) as f32;
                    let state = if y == MIN_Y || (y < MIN_Y + 5 && r < 0.2 * (MIN_Y + 5 - y) as f32) {
                        b.bedrock
                    } else if y > h {
                        // Water up to sea level (frozen in cold biomes).
                        if y == SEA_LEVEL && biome.snowy() { b.ice } else { b.water }
                    } else {
                        let v = lattice.at(wx, y, wz);
                        if self.carved(v, y, h, land && h > SEA_LEVEL + 2) {
                            if y < -54 { b.lava } else { b.air }
                        } else {
                            self.solid_at(biome, h, y, wx, wz, r, v[3])
                        }
                    };
                    if state != b.air {
                        c.set(x, y, z, state);
                    }
                }
                // Snow on top of cold surfaces.
                if biome.snowy() && land && c.get(x, h, z) != b.air {
                    c.set(x, h + 1, z, b.snow_layer);
                }
                self.decorate_column(&mut c, x, z, wx, wz, h, biome);
            }
        }

        // 4. Trees (from this chunk and from neighbors that reach into it).
        let leaves_like = |s: u16| {
            let n = &block_of(s).name;
            n.ends_with("_leaves") || reg.is_air(s) || n == "short_grass" || n == "fern" || n == "snow" || n == "vine" || n.ends_with("_tulip") || n == "dandelion" || n == "poppy"
        };
        for dx in -1..=1 {
            for dz in -1..=1 {
                for t in self.trees_in_chunk(cx + dx, cz + dz) {
                    for (bx, by, bz, s) in self.tree_blocks(&t) {
                        if bx >> 4 != cx || bz >> 4 != cz {
                            continue;
                        }
                        let (lx, lz) = ((bx - x0) as usize, (bz - z0) as usize);
                        let cur = c.get(lx, by, lz);
                        let is_log = block_of(s).name.ends_with("_log") || block_of(s).name.ends_with("_wood");
                        if leaves_like(cur) || (is_log && block_of(cur).name.ends_with("_leaves")) || block_of(s).name == "dirt" {
                            c.set(lx, by, lz, s);
                        }
                    }
                }
            }
        }

        // Bee nests from the trees come with three bees inside.
        if let Some(nest) = data::get().block("bee_nest") {
            for (pos, s) in c.iter_states() {
                if s >= nest.min_state && s <= nest.max_state {
                    c.block_entities.insert(crate::protocol::BlockPos::new(x0 + pos.0 as i32, pos.1, z0 + pos.2 as i32), super::blockentity::BlockEntity::Hive { bees: 3 });
                }
            }
        }
        // 5. Villages.
        for v in self.villages_near(cx, cz) {
            for &(bx, by, bz, s) in v.iter() {
                if bx >> 4 == cx && bz >> 4 == cz {
                    c.set((bx - x0) as usize, by, (bz - z0) as usize, s);
                }
            }
        }
        // 6. Structures: desert pyramids, pillager outposts, temples, wrecks, ruins, portals,
        //    monuments, mineshafts and ancient cities.
        for f in self.features_near(cx, cz) {
            f.apply(&mut c, cx, cz, self.seed);
        }
        // 7. Underground structures: dungeons and strongholds.
        let surface = [(0, 0), (15, 0), (0, 15), (15, 15)].iter().map(|(dx, dz)| self.height(x0 + dx, z0 + dz)).min().unwrap_or(SEA_LEVEL);
        if let Some(d) = super::structures::dungeon(self.seed, cx, cz, surface) {
            d.apply(&mut c, cx, cz, self.seed);
        }
        for s in self.strongholds_near(cx, cz) {
            s.apply(&mut c, cx, cz, self.seed);
        }
        c
    }

    /// Solid subsurface and surface block for the biome.
    fn solid_at(&self, biome: Biome, h: i32, y: i32, wx: i32, wz: i32, r: f32, variant: f32) -> u16 {
        use Biome::*;
        let b = &self.b;
        let depth = h - y;
        let steep = matches!(biome, WindsweptHills | StonyPeaks) && (self.height(wx + 2, wz) - self.height(wx - 2, wz)).abs() > 4;
        // Surface and the layers below it.
        if depth < 4 {
            let surface = match biome {
                Desert => Some(if depth < 3 { b.sand } else { b.sandstone }),
                Beach | SnowyBeach => Some(if depth < 3 { b.sand } else { b.sandstone }),
                Ocean | WarmOcean | FrozenOcean => Some(if h < 48 { b.gravel } else { b.sand }),
                DeepOcean => Some(b.gravel),
                River | FrozenRiver => Some(if r < 0.15 { b.clay } else if depth < 2 { b.sand } else { b.gravel }),
                StonyPeaks => Some(b.stone),
                SnowySlopes => Some(if depth == 0 { b.snow_block } else { b.stone }),
                WindsweptHills if steep || h > 120 => Some(b.stone),
                Swamp if depth == 0 && r < 0.1 => Some(b.mud),
                _ => None,
            };
            if let Some(s) = surface {
                return s;
            }
            if depth == 0 {
                return match biome {
                    SnowyPlains | SnowyTaiga => b.snowy_grass,
                    Taiga if r < 0.15 => b.podzol,
                    _ if h < SEA_LEVEL => b.dirt,
                    _ => b.grass,
                };
            }
            return b.dirt;
        }
        // Subsurface: stone or deepslate (transition between 0 and 8), ores and variants.
        let deep = y < 0 || (y < 8 && r < (8 - y) as f32 / 8.0);
        let ore_r = unit(hash(self.seed ^ 0xa5, wx as i64, y as i64, wz as i64)) as f32;
        for &(ore, deep_ore, p, lo, hi) in &b.ores {
            if (lo..=hi).contains(&y) && ore_r < p {
                return if deep { deep_ore } else { ore };
            }
        }
        if !deep && variant > 0.72 {
            return b.variants[(hash(self.seed, (wx >> 5) as i64, (y >> 5) as i64, (wz >> 5) as i64) % 3) as usize];
        }
        if deep { b.deepslate } else { b.stone }
    }

    /// Grass, flowers, cactus, dead bushes, seagrass and lily pads.
    fn decorate_column(&self, c: &mut Chunk, x: usize, z: usize, wx: i32, wz: i32, h: i32, biome: Biome) {
        use Biome::*;
        let r = unit(hash(self.seed ^ 0x77, wx as i64, 0, wz as i64));
        let top = block_of(c.get(x, h, z)).name.as_str();
        let above_free = data::get().is_air(c.get(x, h + 1, z)) || block_of(c.get(x, h + 1, z)).name == "snow";
        if biome.water() || h < SEA_LEVEL {
            // Seagrass on the seabed.
            if h < SEA_LEVEL - 1 && r < 0.08 && matches!(top, "sand" | "gravel" | "dirt" | "clay") && block_of(c.get(x, h + 1, z)).name == "water" {
                c.set(x, h + 1, z, st("seagrass"));
            }
            if biome == Swamp && h == SEA_LEVEL - 1 && r < 0.05 {
                c.set(x, SEA_LEVEL + 1, z, st("lily_pad"));
            }
            return;
        }
        if !above_free {
            return;
        }
        let plant = match biome {
            Desert if top == "sand" => {
                if r < 0.004 {
                    // Cactus 1 to 3 blocks tall.
                    let n = 1 + (r * 1000.0) as i32 % 3;
                    for i in 1..=n {
                        c.set(x, h + i, z, st("cactus"));
                    }
                    None
                } else if r < 0.012 {
                    Some("dead_bush")
                } else {
                    None
                }
            }
            _ if !matches!(top, "grass_block" | "podzol") => None,
            Plains | SunflowerPlains | Savanna => match r {
                r if r < 0.22 => Some("short_grass"),
                r if r < 0.235 => Some(["dandelion", "poppy", "oxeye_daisy", "azure_bluet"][(r * 10000.0) as usize % 4]),
                r if r < 0.245 && biome == SunflowerPlains => Some("sunflower"),
                _ => None,
            },
            FlowerForest => match r {
                r if r < 0.12 => Some(["allium", "red_tulip", "orange_tulip", "white_tulip", "pink_tulip", "cornflower", "lily_of_the_valley"][(r * 10000.0) as usize % 7]),
                r if r < 0.2 => Some("short_grass"),
                _ => None,
            },
            Forest | BirchForest | DarkForest | WindsweptHills => match r {
                r if r < 0.1 => Some("short_grass"),
                r if r < 0.11 => Some(["dandelion", "poppy", "lily_of_the_valley"][(r * 10000.0) as usize % 3]),
                _ => None,
            },
            Taiga | SnowyTaiga => match r {
                r if r < 0.08 => Some("fern"),
                r if r < 0.1 => Some("sweet_berry_bush"),
                _ => None,
            },
            Jungle => match r {
                r if r < 0.3 => Some("short_grass"),
                r if r < 0.36 => Some("fern"),
                r if r < 0.362 => Some("melon"),
                _ => None,
            },
            Swamp => match r {
                r if r < 0.06 => Some("short_grass"),
                r if r < 0.07 => Some("blue_orchid"),
                _ => None,
            },
            _ => None,
        };
        if let Some(p) = plant {
            if p == "sunflower" {
                c.set(x, h + 1, z, with("sunflower", &[("half", "lower")]));
                c.set(x, h + 2, z, with("sunflower", &[("half", "upper")]));
            } else {
                c.set(x, h + 1, z, st(p));
            }
        }
    }

    /// Trees whose trunk is in chunk (cx, cz); deterministic.
    fn trees_in_chunk(&self, cx: i32, cz: i32) -> Vec<Tree> {
        let (mx, mz) = (cx * 16 + 8, cz * 16 + 8);
        let center = self.climate(mx, mz);
        let biome = self.biome(&center, self.height_from(&center, mx, mz));
        let (density, kinds) = biome.trees();
        if kinds.is_empty() {
            return vec![];
        }
        let base = hash(self.seed ^ 0x1234, cx as i64, 0, cz as i64);
        let mut count = density.floor() as u64;
        if unit(base) < density.fract() {
            count += 1;
        }
        let mut out = Vec::new();
        for i in 0..count {
            let r = hash(self.seed ^ 0x5678, cx as i64, i as i64, cz as i64);
            let (x, z) = (cx * 16 + (r % 16) as i32, cz * 16 + ((r >> 8) % 16) as i32);
            let cl = self.climate(x, z);
            let h = self.height_from(&cl, x, z);
            let b = self.biome(&cl, h);
            if b.water() || h <= SEA_LEVEL || matches!(b, Biome::Desert | Biome::Beach | Biome::SnowyBeach | Biome::StonyPeaks | Biome::SnowySlopes) {
                continue;
            }
            // The ground must not have been carved out by a cave.
            let v = lattice_point(x, h, z, |a, b2, c| self.cave_lattice(a, b2, c));
            let v2 = lattice_point(x, h - 1, z, |a, b2, c| self.cave_lattice(a, b2, c));
            if self.carved(v, h, h, h > SEA_LEVEL + 2) || self.carved(v2, h - 1, h, h > SEA_LEVEL + 2) {
                continue;
            }
            // On hills, only where there is no stone on the surface.
            if b == Biome::WindsweptHills && (h > 120 || (self.height(x + 2, z) - self.height(x - 2, z)).abs() > 4) {
                continue;
            }
            let kind = kinds[((r >> 16) % kinds.len() as u64) as usize];
            out.push(Tree { x, y: h + 1, z, kind, seed: r >> 20 });
        }
        out
    }

    /// Blocks of a tree in absolute coordinates.
    fn tree_blocks(&self, t: &Tree) -> Vec<(i32, i32, i32, u16)> {
        let mut out = Vec::new();
        let r = t.seed;
        let leaves = |name: &str| with(name, &[("persistent", "true"), ("distance", "1")]);
        let blob = |out: &mut Vec<(i32, i32, i32, u16)>, cx: i32, cy: i32, cz: i32, radius: i32, leaf: u16, trim: bool| {
            for dx in -radius..=radius {
                for dz in -radius..=radius {
                    if trim && dx.abs() == radius && dz.abs() == radius && (r >> ((dx + dz + cy) & 15) as u64) & 1 == 0 {
                        continue;
                    }
                    out.push((cx + dx, cy, cz + dz, leaf));
                }
            }
        };
        let (x, y, z) = (t.x, t.y, t.z);
        out.push((x, y - 1, z, st("dirt")));
        match t.kind {
            TreeKind::Oak | TreeKind::Birch | TreeKind::SwampOak => {
                let (log, leaf) = match t.kind {
                    TreeKind::Birch => (st("birch_log"), leaves("birch_leaves")),
                    _ => (st("oak_log"), leaves("oak_leaves")),
                };
                let trunk = if t.kind == TreeKind::Birch { 5 + (r % 3) as i32 } else { 4 + (r % 3) as i32 };
                let top = y + trunk - 1;
                for ly in top - 2..=top - 1 {
                    blob(&mut out, x, ly, z, 2, leaf, true);
                }
                blob(&mut out, x, top, z, 1, leaf, true);
                out.extend([(x, top + 1, z, leaf), (x + 1, top + 1, z, leaf), (x - 1, top + 1, z, leaf), (x, top + 1, z + 1, leaf), (x, top + 1, z - 1, leaf)]);
                for ly in y..=top {
                    out.push((x, ly, z, log));
                }
                // Now and then a bee nest on the trunk, under the leaves.
                if t.kind != TreeKind::SwampOak && (r >> 40) % 25 == 0 {
                    out.push((x, top - 3, z - 1, with("bee_nest", &[("facing", "north")])));
                }
                if t.kind == TreeKind::SwampOak {
                    // Vines hanging from the leaves.
                    for (dx, dz, face) in [(3, 0, "west"), (-3, 0, "east"), (0, 3, "north"), (0, -3, "south")] {
                        if (r >> (dx + dz + 8i32).unsigned_abs()) & 1 == 1 {
                            let vine = with("vine", &[(face, "true")]);
                            for ly in top - 4..top - 1 {
                                out.push((x + dx, ly, z + dz, vine));
                            }
                        }
                    }
                }
            }
            TreeKind::Spruce => {
                let trunk = 7 + (r % 4) as i32;
                let leaf = leaves("spruce_leaves");
                let top = y + trunk;
                out.push((x, top, z, leaf));
                let mut radius = 0;
                for ly in (y + 2..top).rev() {
                    radius = match radius {
                        0 => 1,
                        1 if (top - ly) % 2 == 0 => 2,
                        2 if (top - ly) > 5 => 3,
                        r2 => r2.min(if (top - ly) % 3 == 0 { 1 } else { 3 }),
                    };
                    for dx in -radius..=radius {
                        for dz in -radius..=radius {
                            if i32::abs(dx) + i32::abs(dz) <= radius + 1 {
                                out.push((x + dx, ly, z + dz, leaf));
                            }
                        }
                    }
                }
                for ly in y..top {
                    out.push((x, ly, z, st("spruce_log")));
                }
            }
            TreeKind::DarkOak => {
                let trunk = 6 + (r % 3) as i32;
                let (log, leaf) = (st("dark_oak_log"), leaves("dark_oak_leaves"));
                let top = y + trunk;
                for ly in top - 2..=top {
                    let rad = if ly == top { 2 } else { 3 };
                    for dx in -rad..=rad + 1 {
                        for dz in -rad..=rad + 1 {
                            out.push((x + dx, ly, z + dz, leaf));
                        }
                    }
                }
                for ly in y..top {
                    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        out.push((x + dx, ly, z + dz, log));
                    }
                }
                for (dx, dz) in [(1, 0), (0, 1), (1, 1)] {
                    out.push((x + dx, y - 1, z + dz, st("dirt")));
                }
            }
            TreeKind::Jungle => {
                let trunk = 9 + (r % 6) as i32;
                let (log, leaf) = (st("jungle_log"), leaves("jungle_leaves"));
                let top = y + trunk;
                for ly in top - 2..=top {
                    blob(&mut out, x, ly, z, if ly == top { 2 } else { 3 }, leaf, true);
                }
                for ly in y..top {
                    out.push((x, ly, z, log));
                }
                // Vines on the trunk.
                for (dx, dz, face) in [(1, 0, "west"), (-1, 0, "east")] {
                    if (r >> 3) & 1 == 1 {
                        for ly in y + 2..top - 2 {
                            out.push((x + dx, ly, z + dz, with("vine", &[(face, "true")])));
                        }
                    }
                }
            }
            TreeKind::Bush => {
                out.push((x, y, z, st("jungle_log")));
                blob(&mut out, x, y, z, 2, leaves("oak_leaves"), true);
                blob(&mut out, x, y + 1, z, 1, leaves("oak_leaves"), false);
            }
            TreeKind::Acacia => {
                let trunk = 4 + (r % 2) as i32;
                let (dx, dz) = [(1, 0), (-1, 0), (0, 1), (0, -1)][(r % 4) as usize];
                let log = st("acacia_log");
                let leaf = leaves("acacia_leaves");
                for ly in y..y + trunk {
                    out.push((x, ly, z, log));
                }
                // Bent trunk and flat canopy.
                let (bx, bz) = (x + dx * 2, z + dz * 2);
                out.push((x + dx, y + trunk, z + dz, log));
                out.push((bx, y + trunk + 1, bz, log));
                let ty = y + trunk + 2;
                for ddx in -3i32..=3 {
                    for ddz in -3i32..=3 {
                        if ddx.abs() + ddz.abs() <= 4 {
                            out.push((bx + ddx, ty, bz + ddz, leaf));
                        }
                    }
                }
                blob(&mut out, bx, ty + 1, bz, 1, leaf, false);
            }
        }
        out
    }

    // ---------- villages ----------

    /// Centers (wells) of villages within `radius` blocks of (x, z).
    pub fn village_centers_near(&self, x: i32, z: i32, radius: i32) -> Vec<(i32, i32)> {
        const REGION: i32 = 32;
        let (rx, rz) = ((x >> 4).div_euclid(REGION), (z >> 4).div_euclid(REGION));
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (vx, vz) = (rx + dx, rz + dz);
                let r = hash(self.seed ^ 0x7111, vx as i64, 0, vz as i64);
                let center = ((vx * REGION + 4 + (r % 24) as i32) * 16 + 8, (vz * REGION + 4 + ((r >> 8) % 24) as i32) * 16 + 8);
                if (center.0 - x).abs() > radius || (center.1 - z).abs() > radius {
                    continue;
                }
                let mut cache = self.villages.lock().unwrap();
                let v = cache.entry((vx, vz)).or_insert_with(|| Arc::new(self.village(center.0, center.1, r))).clone();
                drop(cache);
                if !v.is_empty() {
                    out.push(center);
                }
            }
        }
        out
    }

    /// Villages whose area (radius 56) touches the chunk.
    fn villages_near(&self, cx: i32, cz: i32) -> Vec<Arc<Vec<(i32, i32, i32, u16)>>> {
        const REGION: i32 = 32; // chunks
        let mut out = Vec::new();
        let (rx, rz) = (cx.div_euclid(REGION), cz.div_euclid(REGION));
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (vx, vz) = (rx + dx, rz + dz);
                let r = hash(self.seed ^ 0x7111, vx as i64, 0, vz as i64);
                let center = ((vx * REGION + 4 + (r % 24) as i32) * 16 + 8, (vz * REGION + 4 + ((r >> 8) % 24) as i32) * 16 + 8);
                let (bx, bz) = (cx * 16, cz * 16);
                if (center.0 - (bx + 8)).abs() > 64 || (center.1 - (bz + 8)).abs() > 64 {
                    continue;
                }
                let mut cache = self.villages.lock().unwrap();
                let v = cache.entry((vx, vz)).or_insert_with(|| Arc::new(self.village(center.0, center.1, r))).clone();
                drop(cache);
                if !v.is_empty() {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Blocks of a village centered at (x, z), or empty if the terrain is unsuitable.
    fn village(&self, x: i32, z: i32, r: u64) -> Vec<(i32, i32, i32, u16)> {
        use Biome::*;
        let cl = self.climate(x, z);
        let h0 = self.height_from(&cl, x, z);
        let biome = self.biome(&cl, h0);
        if !matches!(biome, Plains | SunflowerPlains | Savanna | Desert | SnowyPlains | Taiga | Forest | FlowerForest) || unit(r) < 0.2 {
            return vec![];
        }
        // Reasonably flat, dry terrain.
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for (dx, dz) in [(0, 0), (30, 0), (-30, 0), (0, 30), (0, -30), (20, 20), (-20, -20), (20, -20), (-20, 20)] {
            let h = self.height(x + dx, z + dz);
            lo = lo.min(h);
            hi = hi.max(h);
        }
        if hi - lo > 10 || lo <= SEA_LEVEL {
            return vec![];
        }
        let (wood, log, path, wall) = match biome {
            Desert => ("smooth_sandstone", "cut_sandstone", "smooth_sandstone", "sandstone"),
            Savanna => ("acacia_planks", "acacia_log", "dirt_path", "acacia_planks"),
            SnowyPlains | Taiga => ("spruce_planks", "spruce_log", "dirt_path", "spruce_planks"),
            _ => ("oak_planks", "oak_log", "dirt_path", "oak_planks"),
        };
        let mut out: Vec<(i32, i32, i32, u16)> = Vec::new();
        let ground = |gx: i32, gz: i32| self.height(gx, gz);

        // Cross-shaped streets from the well.
        let arm = 28 + (r % 12) as i32;
        let path_state = st(path);
        for d in -arm..=arm {
            for w in -1..=1 {
                for (px, pz) in [(x + d, z + w), (x + w, z + d)] {
                    let g = ground(px, pz);
                    out.push((px, g, pz, path_state));
                    for clear in 1..=3 {
                        out.push((px, g + clear, pz, 0));
                    }
                }
            }
        }
        // Well.
        let g = ground(x, z);
        for dx in -2i32..=2 {
            for dz in -2i32..=2 {
                let ring = dx.abs() == 2 || dz.abs() == 2;
                for y in g - 2..=g {
                    out.push((x + dx, y, z + dz, if ring { st("cobblestone") } else { st("water") }));
                }
                if dx.abs() == 2 && dz.abs() == 2 {
                    for y in g + 1..=g + 3 {
                        out.push((x + dx, y, z + dz, st("oak_fence")));
                    }
                }
                out.push((x + dx, g + 4, z + dz, st("cobblestone_slab")));
            }
        }
        // Houses along the streets, with the door facing the street.
        let mut i = 0;
        for d in (10..arm - 4).step_by(13) {
            for (dir, side) in [((1, 0), 1), ((1, 0), -1), ((-1, 0), 1), ((-1, 0), -1), ((0, 1), 1), ((0, 1), -1), ((0, -1), 1), ((0, -1), -1)] {
                i += 1;
                let rr = hash(r, i, d as i64, side as i64);
                if rr % 3 == 0 {
                    continue;
                }
                // House center 6 blocks from the street.
                let (ax, az) = (x + dir.0 * d, z + dir.1 * d);
                let (hx, hz) = if dir.0 != 0 { (ax, az + side * 6) } else { (ax + side * 6, az) };
                let facing = match (dir.0 != 0, side) {
                    (true, 1) => "north",
                    (true, _) => "south",
                    (false, 1) => "west",
                    (false, _) => "east",
                };
                if rr % 5 == 0 {
                    self.farm(&mut out, hx, hz);
                } else {
                    self.house(&mut out, hx, hz, facing, wood, log, wall, rr);
                }
                // Lamp post on the corner.
                let (lx, lz) = if dir.0 != 0 { (ax, az + side * 2) } else { (ax + side * 2, az) };
                let lg = ground(lx, lz);
                for y in lg + 1..=lg + 3 {
                    out.push((lx, y, lz, st("oak_fence")));
                }
                out.push((lx, lg + 4, lz, st("lantern")));
            }
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn house(&self, out: &mut Vec<(i32, i32, i32, u16)>, cx: i32, cz: i32, facing: &str, wood: &str, log: &str, wall: &str, r: u64) {
        let floor_y = self.height(cx, cz);
        let size = 2 + (r % 2) as i32; // half side: 5x5 or 7x7
        let (planks, logs, walls) = (st(wood), with(log, &[("axis", "y")]), st(wall));
        for dx in -size..=size {
            for dz in -size..=size {
                // Foundations down to the ground.
                let g = self.height(cx + dx, cz + dz);
                for y in g.min(floor_y - 1)..floor_y {
                    out.push((cx + dx, y, cz + dz, st("cobblestone")));
                }
                out.push((cx + dx, floor_y, cz + dz, planks));
                let edge = dx.abs() == size || dz.abs() == size;
                let corner = dx.abs() == size && dz.abs() == size;
                for y in floor_y + 1..=floor_y + 3 {
                    let s = if corner {
                        logs
                    } else if edge {
                        if y == floor_y + 2 && (dx == 0 || dz == 0) { st("glass_pane") } else { walls }
                    } else {
                        0
                    };
                    out.push((cx + dx, y, cz + dz, s));
                }
                // Flat roof with a slab rim.
                out.push((cx + dx, floor_y + 4, cz + dz, planks));
                if edge {
                    let slab = with(&format!("{}_slab", wood.trim_end_matches("_planks")), &[]);
                    out.push((cx + dx, floor_y + 5, cz + dz, if slab == 0 { st("oak_slab") } else { slab }));
                }
            }
        }
        // Door facing the street.
        let (dx, dz) = match facing {
            "north" => (0, -size),
            "south" => (0, size),
            "west" => (-size, 0),
            _ => (size, 0),
        };
        let door = if wood.contains("sandstone") { "oak_door" } else { &format!("{}_door", wood.trim_end_matches("_planks")) };
        let door = if data::get().block(door).is_some() { door.to_string() } else { "oak_door".to_string() };
        out.push((cx + dx, floor_y + 1, cz + dz, with(&door, &[("facing", facing), ("half", "lower")])));
        out.push((cx + dx, floor_y + 2, cz + dz, with(&door, &[("facing", facing), ("half", "upper")])));
        // Interior: a crafting table, a bed and a torch.
        out.push((cx - dx.signum() * (size - 1) + 1, floor_y + 1, cz - dz.signum() * (size - 1), st("crafting_table")));
        let bed_dir = match facing {
            "north" => "south",
            "south" => "north",
            "west" => "east",
            _ => "west",
        };
        let (bx, bz) = (cx - dz.signum() * (size - 1), cz + dx.signum() * (size - 1));
        out.push((bx, floor_y + 1, bz, with("red_bed", &[("facing", bed_dir), ("part", "foot")])));
        let (hx, hz) = match bed_dir {
            "north" => (bx, bz - 1),
            "south" => (bx, bz + 1),
            "west" => (bx - 1, bz),
            _ => (bx + 1, bz),
        };
        out.push((hx, floor_y + 1, hz, with("red_bed", &[("facing", bed_dir), ("part", "head")])));
        out.push((cx, floor_y + 3, cz, st("lantern")));
    }

    fn farm(&self, out: &mut Vec<(i32, i32, i32, u16)>, cx: i32, cz: i32) {
        let y = self.height(cx, cz);
        let reg = data::get();
        for dx in -4i32..=4 {
            for dz in -3i32..=3 {
                let edge = dx.abs() == 4 || dz.abs() == 3;
                let (px, pz) = (cx + dx, cz + dz);
                let g = self.height(px, pz);
                for fy in g.min(y - 1)..y {
                    out.push((px, fy, pz, st("dirt")));
                }
                if edge {
                    out.push((px, y, pz, with("oak_log", &[("axis", if dx.abs() == 4 { "z" } else { "x" })])));
                    out.push((px, y + 1, pz, 0));
                } else if dx == 0 {
                    out.push((px, y, pz, st("water")));
                    out.push((px, y + 1, pz, 0));
                } else {
                    out.push((px, y, pz, with("farmland", &[("moisture", "7")])));
                    let crop = ["wheat", "carrots", "potatoes", "beetroots"][(hash(self.seed, cx as i64, 0, cz as i64) % 4) as usize];
                    let b = reg.block(crop).unwrap();
                    let max_age = b.props.iter().find(|(n, _)| n == "age").map_or(0, |(_, v)| v.len() - 1);
                    let age = (hash(self.seed, px as i64, 1, pz as i64) % (max_age as u64 + 1)).to_string();
                    out.push((px, y + 1, pz, b.with(b.default_state, "age", &age)));
                }
                out.push((px, y + 2, pz, 0));
            }
        }
    }
}

// ---------- Nether ----------

pub struct NetherGen {
    seed: u64,
    noise: Noise,
    fortresses: Mutex<HashMap<(i32, i32), Option<Arc<super::structures::Structure>>>>,
    /// Bastion remnants.
    pub bastions: Sites,
}

/// Nether fortresses: one in two of every three regions of 20x20 chunks, deck at y 66.
const FORTRESS_REGION: i32 = 20;
const FORTRESS_Y: i32 = 66;

impl NetherGen {
    fn new(seed: u64) -> Self {
        Self { seed: seed ^ 0x6e65_7468, noise: Noise { seed: seed ^ 0x6e65_7468 }, fortresses: Mutex::new(HashMap::new()), bastions: Sites::new(22, 0xba57, 24) }
    }

    /// A bastion in one of every two regions, away from fortresses.
    fn build_bastion(&self, x: i32, z: i32, r: u64) -> Option<super::structures::Structure> {
        if r % 2 != 0 || self.in_fortress(x, FORTRESS_Y, z) || !self.fortresses_near(x >> 4, z >> 4).iter().all(|f| !f.contains(x, FORTRESS_Y, z, 40)) {
            return None;
        }
        Some(super::buildings::bastion(x, z, 48, self.seed ^ r))
    }

    fn fortress(&self, rx: i32, rz: i32) -> Option<Arc<super::structures::Structure>> {
        let mut cache = self.fortresses.lock().unwrap();
        cache
            .entry((rx, rz))
            .or_insert_with(|| {
                let (x, z, r) = super::structures::region_spot(self.seed, 0xF0F0, FORTRESS_REGION, rx, rz);
                ((r >> 40) % 3 != 0).then(|| Arc::new(super::structures::nether_fortress(self.seed, x, z, FORTRESS_Y)))
            })
            .clone()
    }

    fn fortresses_near(&self, cx: i32, cz: i32) -> Vec<Arc<super::structures::Structure>> {
        let (rx, rz) = (cx.div_euclid(FORTRESS_REGION), cz.div_euclid(FORTRESS_REGION));
        let mut out = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let (x, z, _) = super::structures::region_spot(self.seed, 0xF0F0, FORTRESS_REGION, rx + dx, rz + dz);
                if (x - (cx * 16 + 8)).abs() > 72 || (z - (cz * 16 + 8)).abs() > 72 {
                    continue;
                }
                if let Some(f) = self.fortress(rx + dx, rz + dz) {
                    out.push(f);
                }
            }
        }
        out
    }

    /// Whether (x, y, z) is inside a fortress (with some margin).
    pub fn in_fortress(&self, x: i32, y: i32, z: i32) -> bool {
        self.fortresses_near(x >> 4, z >> 4).iter().any(|f| f.bounds.is_some_and(|(lo, hi)| x >= lo.x - 2 && x <= hi.x + 2 && z >= lo.z - 2 && z <= hi.z + 2 && y >= FORTRESS_Y - 4 && y <= hi.y + 2))
    }

    pub fn nearest_fortress(&self, x: i32, z: i32, regions: i32) -> Option<(i32, i32, i32)> {
        let (rx, rz) = ((x >> 4).div_euclid(FORTRESS_REGION), (z >> 4).div_euclid(FORTRESS_REGION));
        let mut best: Option<(i64, (i32, i32, i32))> = None;
        for dx in -regions..=regions {
            for dz in -regions..=regions {
                if self.fortress(rx + dx, rz + dz).is_none() {
                    continue;
                }
                let (fx, fz, _) = super::structures::region_spot(self.seed, 0xF0F0, FORTRESS_REGION, rx + dx, rz + dz);
                let d = ((fx - x) as i64).pow(2) + ((fz - z) as i64).pow(2);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, (fx, FORTRESS_Y, fz)));
                }
            }
        }
        best.map(|b| b.1)
    }

    fn generate(&self, cx: i32, cz: i32) -> Chunk {
        let reg = data::get();
        let wastes = reg.biome_id("nether_wastes");
        let valley = reg.biome_id("soul_sand_valley");
        let mut c = Chunk::new_dim(0, 256, wastes);
        let (x0, z0) = (cx * 16, cz * 16);
        let (netherrack, lava, bedrock, quartz, gold, glow, soul_sand, soul_soil, gravel) =
            (st("netherrack"), st("lava"), st("bedrock"), st("nether_quartz_ore"), st("nether_gold_ore"), st("glowstone"), st("soul_sand"), st("soul_soil"), st("gravel"));
        let n = self.noise;
        let lattice = Lattice::build(x0, 0, z0, 16, 128, |x, y, z| {
            let (fx, fy, fz) = (x as f64, y as f64, z as f64);
            [(n.n3(1, fx / 10.0, fy / 6.0, fz / 10.0) * 0.7 + n.n3(2, fx / 4.0, fy / 3.0, fz / 4.0) * 0.3) as f32]
        });
        for x4 in 0..4 {
            for z4 in 0..4 {
                let v = n.fbm2(3, (x0 + x4 as i32 * 4) as f64 / 160.0, (z0 + z4 as i32 * 4) as f64 / 160.0);
                c.set_biome_column(x4, z4, if v > 0.62 { valley } else { wastes });
            }
        }
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = (x0 + x as i32, z0 + z as i32);
                let in_valley = c.biome(x, 40, z) == valley;
                for y in 0..128 {
                    let r = unit(hash(self.seed, wx as i64, y, wz as i64)) as f32;
                    let s = if y == 0 || y == 127 || (y < 5 && r < 0.2 * (5 - y) as f32) || (y > 122 && r < 0.2 * (y - 122) as f32) {
                        bedrock
                    } else {
                        // More open in the middle, closed off at the top and bottom.
                        let fy = y as f32;
                        let gradient = ((fy - 64.0).abs() / 64.0).powi(2) * 0.6;
                        let density = lattice.at(wx, y as i32, wz)[0] + gradient;
                        if density > 0.55 {
                            match r {
                                r if r < 0.012 => quartz,
                                r if r < 0.016 => gold,
                                _ if in_valley && y < 40 => {
                                    if r < 0.5 { soul_sand } else { soul_soil }
                                }
                                _ if y < 34 && r < 0.03 => gravel,
                                _ => netherrack,
                            }
                        } else if y <= 31 {
                            lava
                        } else {
                            0
                        }
                    };
                    if s != 0 {
                        c.set(x, y as i32, z, s);
                    }
                }
                // Glowstone hanging from the ceiling.
                let gr = unit(hash(self.seed ^ 0x91, wx as i64, 0, wz as i64));
                if gr < 0.01 {
                    for y in (70..122).rev() {
                        if c.get(x, y, z) == 0 && c.get(x, y + 1, z) == netherrack {
                            for d in 0..1 + (gr * 400.0) as i32 % 4 {
                                c.set(x, y - d, z, glow);
                            }
                            break;
                        }
                    }
                }
            }
        }
        for f in self.fortresses_near(cx, cz) {
            f.apply(&mut c, cx, cz, self.seed);
        }
        for b in self.bastions.near(self.seed, cx, cz, &|x, z, r| self.build_bastion(x, z, r)) {
            b.apply(&mut c, cx, cz, self.seed);
        }
        c
    }
}

// ---------- End ----------

pub struct EndGen {
    seed: u64,
    noise: Noise,
    /// End cities on the outer islands.
    pub cities: Sites,
}

/// Obsidian pillars around the central island: (x, z, radius, height).
pub fn end_pillars() -> Vec<(i32, i32, i32, i32)> {
    (0..10)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 10.0;
            let (x, z) = ((42.0 * a.cos()) as i32, (42.0 * a.sin()) as i32);
            (x, z, 2 + i % 4, 76 + 3 * i)
        })
        .collect()
}

impl EndGen {
    fn new(seed: u64) -> Self {
        Self { seed: seed ^ 0x656e_6400, noise: Noise { seed: seed ^ 0x656e_6400 }, cities: Sites::new(20, 0xe9d0, 40) }
    }

    /// Top of the island at (x, z), if any (the same shapes `generate` builds).
    pub fn island_top(&self, x: i32, z: i32) -> Option<i32> {
        let d = ((x as f64).powi(2) + (z as f64).powi(2)).sqrt();
        let wobble = self.noise.fbm2(1, x as f64 / 30.0, z as f64 / 30.0) * 30.0;
        if d < 90.0 + wobble {
            let k = 1.0 - d / (90.0 + wobble);
            return Some((58.0 + k * 6.0) as i32);
        }
        if d > 1000.0 {
            let v = self.noise.fbm2(2, x as f64 / 90.0, z as f64 / 90.0);
            if v > 0.6 {
                return Some((55.0 + (v - 0.6) / 0.4 * 10.0) as i32);
            }
        }
        None
    }

    /// An End city on a big enough outer island.
    fn build_city(&self, x: i32, z: i32, r: u64) -> Option<super::structures::Structure> {
        let far = ((x as f64).powi(2) + (z as f64).powi(2)).sqrt() > 1100.0;
        let solid = [(0, 0), (6, 0), (-6, 0), (0, 6), (0, -6)].iter().all(|(dx, dz)| self.island_top(x + dx, z + dz).is_some());
        (far && solid && r % 3 != 0).then(|| super::buildings::end_city(x, z, self.island_top(x, z).unwrap_or(60), self.seed ^ r))
    }

    fn generate(&self, cx: i32, cz: i32) -> Chunk {
        let reg = data::get();
        let main = reg.biome_id("the_end");
        let outer = reg.biome_id("small_end_islands");
        let (x0, z0) = (cx * 16, cz * 16);
        let far = ((x0 as i64).pow(2) + (z0 as i64).pow(2)) > 400 * 400;
        let mut c = Chunk::new_dim(0, 256, if far { outer } else { main });
        let end_stone = st("end_stone");
        let obsidian = st("obsidian");
        for x in 0..16 {
            for z in 0..16 {
                let (wx, wz) = (x0 + x as i32, z0 + z as i32);
                let d = ((wx * wx + wz * wz) as f64).sqrt();
                let wobble = self.noise.fbm2(1, wx as f64 / 30.0, wz as f64 / 30.0) * 30.0;
                // Central island.
                let r = 90.0 + wobble;
                let mut columns = Vec::new();
                if d < r {
                    let k = 1.0 - d / r;
                    columns.push(((60.0 - k * 40.0) as i32, (58.0 + k * 6.0) as i32));
                }
                // Outer islands (beyond 1000 blocks, as in vanilla).
                if d > 1000.0 {
                    let v = self.noise.fbm2(2, wx as f64 / 90.0, wz as f64 / 90.0);
                    if v > 0.6 {
                        let k = (v - 0.6) / 0.4;
                        columns.push(((55.0 - k * 30.0) as i32, (55.0 + k * 10.0) as i32));
                    }
                }
                for (bottom, top) in columns {
                    for y in bottom..=top {
                        c.set(x, y, z, end_stone);
                    }
                }
                for &(px, pz, pr, ph) in &end_pillars() {
                    if (wx - px).pow(2) + (wz - pz).pow(2) <= pr * pr {
                        for y in 55..ph {
                            c.set(x, y, z, obsidian);
                        }
                        // Bedrock on top: the end crystal goes there.
                        if wx == px && wz == pz {
                            c.set(x, ph, z, st("bedrock"));
                        }
                    }
                }
            }
        }
        // Exit portal, inactive until the dragon dies (`dragon` lights it).
        if cx.abs() <= 1 && cz.abs() <= 1 {
            let (bedrock, portal) = (st("bedrock"), 0);
            for dx in -3i32..=3 {
                for dz in -3i32..=3 {
                    let d2 = dx * dx + dz * dz;
                    let (wx, wz) = (dx, dz);
                    if wx >> 4 != cx || wz >> 4 != cz || d2 > 12 {
                        continue;
                    }
                    let (lx, lz) = ((wx - x0) as usize, (wz - z0) as usize);
                    c.set(lx, 63, lz, bedrock);
                    c.set(lx, 64, lz, if d2 <= 6 { portal } else { bedrock });
                    if dx == 0 && dz == 0 {
                        for y in 64..68 {
                            c.set(lx, y, lz, bedrock);
                        }
                    }
                }
            }
        }
        for city in self.cities.near(self.seed, cx, cz, &|x, z, r| self.build_city(x, z, r)) {
            city.apply(&mut c, cx, cz, self.seed);
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biomes_and_trees_are_deterministic() {
        let g = Overworld::new(20_200_420);
        let mut seen = std::collections::HashSet::new();
        for i in -40..40 {
            for j in -40..40 {
                let (x, z) = (i * 97, j * 97);
                let c = g.climate(x, z);
                seen.insert(g.biome(&c, g.height_from(&c, x, z)));
            }
        }
        assert!(seen.len() >= 12, "variedad de biomas: {seen:?}");
        // The same chunk generates identically twice.
        let a = g.generate(3, -2);
        let b = g.generate(3, -2);
        assert!(a.sections.iter().zip(&b.sections).all(|(p, q)| p.blocks.to_vec() == q.blocks.to_vec()));
    }

    #[test]
    fn trees_cross_chunk_borders() {
        let g = Overworld::new(20_200_420);
        // An oak right at the edge of its chunk: all its leaves that fall into the neighboring chunk
        // must be there (except where another tree placed a trunk or leaves).
        let mut checked = 0;
        for cx in -40..40 {
            for cz in -40..40 {
                for t in g.trees_in_chunk(cx, cz) {
                    if t.x.rem_euclid(16) != 15 || t.kind != TreeKind::Oak {
                        continue;
                    }
                    let neighbor = g.generate(cx + 1, cz);
                    let outside: Vec<_> = g.tree_blocks(&t).into_iter().filter(|b| b.0 >> 4 == cx + 1 && b.2 >> 4 == cz).collect();
                    assert!(!outside.is_empty());
                    let mut own = 0;
                    for (bx, by, bz, _) in &outside {
                        let name = &block_of(neighbor.get((bx - (cx + 1) * 16) as usize, *by, (bz - cz * 16) as usize)).name;
                        assert!(name.ends_with("_leaves") || name.ends_with("_log"), "en {bx} {by} {bz} hay {name}");
                        own += usize::from(name == "oak_leaves");
                    }
                    assert!(own > 0, "el vecino tiene hojas de este roble");
                    checked += 1;
                    if checked >= 3 {
                        return;
                    }
                }
            }
        }
        assert!(checked > 0, "no se encontró ningún roble en el borde");
    }

    #[test]
    fn caves_exist_underground() {
        let g = Overworld::new(20_200_420);
        let mut air = 0;
        for cx in 0..6 {
            let c = g.generate(cx, 0);
            for y in -50..30 {
                for x in 0..16 {
                    if c.get(x, y, 8) == 0 || block_of(c.get(x, y, 8)).name == "lava" {
                        air += 1;
                    }
                }
            }
        }
        assert!(air > 50, "huecos de cueva: {air}");
    }

    #[test]
    fn some_village_exists() {
        let g = Overworld::new(20_200_420);
        let found = (-6..6).flat_map(|rx| (-6..6).map(move |rz| (rx, rz))).any(|(rx, rz)| !g.villages_near(rx * 32 + 16, rz * 32 + 16).is_empty());
        assert!(found, "alguna aldea en 12x12 regiones");
    }

    #[test]
    fn nether_and_end() {
        let n = NetherGen::new(1).generate(0, 0);
        assert_eq!(block_of(n.get(0, 0, 0)).name, "bedrock");
        assert!((0..16).any(|x| block_of(n.get(x, 20, 3)).name == "lava" || block_of(n.get(x, 20, 3)).name == "netherrack"));
        let e = EndGen::new(1).generate(0, 0);
        assert_eq!(block_of(e.get(0, 64, 0)).name, "bedrock");
        assert_eq!(block_of(e.get(1, 64, 1)).name, "air");
        assert_eq!(block_of(e.get(1, 63, 1)).name, "bedrock");
    }
}

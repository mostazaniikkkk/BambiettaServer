//! World: dimensions, in-memory chunks, on-demand generation and saving in the
//! Anvil format (`region/`, `DIM-1/region/`, `DIM1/region/` and `level.dat`, like vanilla).

pub mod anvil;
pub mod blockentity;
pub mod chunk;
pub mod generator;
pub mod physics;

use crate::config::Config;
use crate::protocol::BlockPos;
use anyhow::{ensure, Result};
use chunk::Chunk;
use flate2::read::ZlibDecoder;
use generator::Generator;
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Overworld, Dimension::Nether, Dimension::End];

    /// Name of the world and of the dimension type (the same in vanilla).
    pub fn name(self) -> &'static str {
        match self {
            Dimension::Overworld => "minecraft:overworld",
            Dimension::Nether => "minecraft:the_nether",
            Dimension::End => "minecraft:the_end",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn min_y(self) -> i32 {
        match self {
            Dimension::Overworld => chunk::MIN_Y,
            _ => 0,
        }
    }

    pub fn height(self) -> i32 {
        match self {
            Dimension::Overworld => chunk::HEIGHT,
            _ => 256,
        }
    }

    pub fn has_sky(self) -> bool {
        self == Dimension::Overworld
    }

    pub fn default_biome(self) -> &'static str {
        match self {
            Dimension::Overworld => "plains",
            Dimension::Nether => "nether_wastes",
            Dimension::End => "the_end",
        }
    }

    /// Dimension folder inside the world (like vanilla).
    fn folder(self) -> &'static str {
        match self {
            Dimension::Overworld => "",
            Dimension::Nether => "DIM-1",
            Dimension::End => "DIM1",
        }
    }

    pub fn in_bounds(self, y: i32) -> bool {
        (self.min_y()..self.min_y() + self.height()).contains(&y)
    }
}

pub struct World {
    pub dim: Dimension,
    chunks: HashMap<(i32, i32), Chunk>,
    generator: Generator,
    /// World root folder (`level.dat`, `playerdata/`, `mobs.json`).
    dir: PathBuf,
    /// Unload rounds (to know which chunks are still in use).
    epoch: u64,
}

impl World {
    pub fn new(cfg: &Config, dim: Dimension) -> Result<Self> {
        let dir = PathBuf::from(&cfg.world_folder);
        std::fs::create_dir_all(dir.join(dim.folder()).join("region"))?;
        Ok(Self { dim, chunks: HashMap::new(), generator: Generator::for_dimension(dim, &cfg.generation), dir, epoch: 0 })
    }

    /// Flat world in a temporary folder (tests).
    #[cfg(test)]
    pub fn new_flat_for_tests() -> Self {
        let mut cfg = Config::default();
        cfg.generation.name = "flat".into();
        cfg.generation.options.layers = vec![("bedrock".into(), 1), ("stone".into(), 3)];
        cfg.world_folder = std::env::temp_dir().join(format!("bambietta-test-{}-{}", std::process::id(), crate::server::entities::random())).display().to_string();
        World::new(&cfg, Dimension::Overworld).unwrap()
    }

    pub fn dir(&self) -> &PathBuf {
        &self.dir
    }

    fn dim_dir(&self) -> PathBuf {
        self.dir.join(self.dim.folder())
    }

    pub fn generator(&self) -> &Generator {
        &self.generator
    }

    fn chunk(&mut self, cx: i32, cz: i32) -> &mut Chunk {
        if !self.chunks.contains_key(&(cx, cz)) {
            let c = match self.load(cx, cz) {
                Ok(Some(c)) => c,
                Ok(None) => self.generator.generate(cx, cz),
                Err(e) => {
                    warn!("chunk {cx},{cz} de {:?} ilegible, se regenera: {e:#}", self.dim);
                    self.generator.generate(cx, cz)
                }
            };
            self.chunks.insert((cx, cz), c);
        }
        let epoch = self.epoch;
        let c = self.chunks.get_mut(&(cx, cz)).unwrap();
        c.used = epoch;
        c
    }

    /// Chunk packet, with the light coming in from its four neighbors.
    pub fn chunk_packet(&mut self, cx: i32, cz: i32) -> Arc<Vec<u8>> {
        let sky = self.dim.has_sky();
        let neighbors = [(0, -1), (0, 1), (-1, 0), (1, 0)].map(|(dx, dz)| Some(self.chunk(cx + dx, cz + dz).faces(sky)));
        self.chunk(cx, cz).packet(cx, cz, sky, neighbors)
    }

    pub fn get_block(&mut self, p: BlockPos) -> u16 {
        self.chunk(p.x >> 4, p.z >> 4).get((p.x & 15) as usize, p.y, (p.z & 15) as usize)
    }

    /// Changes a block and returns the previous state.
    pub fn set_block(&mut self, p: BlockPos, state: u16) -> u16 {
        self.chunk(p.x >> 4, p.z >> 4).set((p.x & 15) as usize, p.y, (p.z & 15) as usize, state)
    }

    /// Spawn point: the solid ground closest to (0, 0).
    pub fn spawn_point(&mut self) -> BlockPos {
        let reg = crate::data::get();
        let water = reg.state("water");
        let (x, z) = (0..64)
            .flat_map(|r| (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (dx * 8, dz * 8))))
            .find(|&(x, z)| {
                let y = self.generator.surface(x, z);
                self.get_block(BlockPos::new(x, y - 1, z)) != water
            })
            .unwrap_or((0, 0));
        let mut y = self.generator.surface(x, z);
        while y < 319 && !reg.is_air(self.get_block(BlockPos::new(x, y, z))) {
            y += 1;
        }
        BlockPos::new(x, y, z)
    }

    fn load(&self, cx: i32, cz: i32) -> Result<Option<Chunk>> {
        if let Some(n) = anvil::read_chunk(&self.dim_dir(), cx, cz)? {
            return anvil::chunk_from_nbt(&n, self.dim).map(Some);
        }
        if self.dim == Dimension::Overworld {
            return self.load_legacy(cx, cz);
        }
        Ok(None)
    }

    /// Previous custom format (`chunks/c.X.Z.bin`): read in order to migrate to Anvil.
    fn load_legacy(&self, cx: i32, cz: i32) -> Result<Option<Chunk>> {
        let path = self.dir.join("chunks").join(format!("c.{cx}.{cz}.bin"));
        if !path.exists() {
            return Ok(None);
        }
        let mut raw = Vec::new();
        ZlibDecoder::new(std::fs::File::open(path)?).read_to_end(&mut raw)?;
        let sections = (chunk::HEIGHT / 16) as usize;
        let body = 4 + sections * 4096 * 2;
        ensure!(raw.len() >= body && (&raw[..4] == b"BCH2" || &raw[..4] == b"BCH1"), "formato desconocido");
        let mut c = Chunk::new();
        for (s, bytes) in raw[4..body].chunks(4096 * 2).enumerate() {
            let values: Vec<u16> = bytes.chunks(2).map(|v| u16::from_le_bytes([v[0], v[1]])).collect();
            c.sections[s].blocks = chunk::Blocks::from_slice(&values);
        }
        if &raw[..4] == b"BCH2" && raw.len() >= body + 4 {
            let n = u32::from_le_bytes(raw[body..body + 4].try_into().unwrap()) as usize;
            let saved: Vec<blockentity::SavedBlockEntity> = serde_json::from_slice(&raw[body + 4..body + 4 + n])?;
            for s in saved {
                c.block_entities.insert(BlockPos::new(s.pos[0], s.pos[1], s.pos[2]), s.into_block_entity());
            }
        }
        c.compact();
        // It will be written as Anvil on the next save.
        c.dirty = true;
        Ok(Some(c))
    }

    /// Copies the modified chunks and marks them as saved, so they can be converted and written
    /// outside the world lock. If writing fails, `mark_dirty` must be called.
    pub fn take_dirty(&mut self) -> (PathBuf, Vec<((i32, i32), Chunk)>) {
        let mut out = Vec::new();
        for (&(x, z), c) in self.chunks.iter_mut().filter(|(_, c)| c.dirty) {
            c.dirty = false;
            out.push(((x, z), c.clone()));
        }
        (self.dim_dir(), out)
    }

    /// Y of the highest block in column (x, z).
    pub fn top_y(&mut self, x: i32, z: i32) -> i32 {
        self.chunk(x >> 4, z >> 4).top_y((x & 15) as usize, (z & 15) as usize)
    }

    pub fn is_loaded(&self, cx: i32, cz: i32) -> bool {
        self.chunks.contains_key(&(cx, cz))
    }

    pub fn mark_dirty(&mut self, keys: &[(i32, i32)]) {
        for k in keys {
            if let Some(c) = self.chunks.get_mut(k) {
                c.dirty = true;
            }
        }
    }

    /// Saves the modified chunks to their region files. Returns how many.
    #[allow(dead_code)]
    pub fn save(&mut self) -> Result<usize> {
        let dirty: Vec<((i32, i32), crate::protocol::Nbt)> =
            self.chunks.iter().filter(|(_, c)| c.dirty).map(|(&(x, z), c)| ((x, z), anvil::chunk_to_nbt(c, x, z))).collect();
        let n = dirty.len();
        if n > 0 {
            let keys: Vec<(i32, i32)> = dirty.iter().map(|d| d.0).collect();
            anvil::write_chunks(&self.dim_dir(), dirty)?;
            for k in keys {
                if let Some(c) = self.chunks.get_mut(&k) {
                    c.dirty = false;
                }
            }
        }
        Ok(n)
    }

    pub fn block_entity(&mut self, p: BlockPos) -> Option<&mut blockentity::BlockEntity> {
        let c = self.chunk(p.x >> 4, p.z >> 4);
        c.dirty = true;
        c.block_entities.get_mut(&p)
    }

    pub fn set_block_entity(&mut self, p: BlockPos, be: blockentity::BlockEntity) {
        let c = self.chunk(p.x >> 4, p.z >> 4);
        c.dirty = true;
        // The chunk packet carries the text of signs and banners.
        if be.client_nbt().is_some() || c.block_entities.get(&p).is_some_and(|o| o.client_nbt().is_some()) {
            c.cache = None;
        }
        c.block_entities.insert(p, be);
    }

    pub fn take_block_entity(&mut self, p: BlockPos) -> Option<blockentity::BlockEntity> {
        let c = self.chunk(p.x >> 4, p.z >> 4);
        let be = c.block_entities.remove(&p);
        if be.as_ref().is_some_and(|b| b.client_nbt().is_some()) {
            c.cache = None;
        }
        if be.is_some() {
            c.dirty = true;
        }
        be
    }

    /// Positions of the furnaces and brewing stands loaded in memory.
    pub fn loaded_furnaces(&self) -> Vec<BlockPos> {
        self.chunks
            .values()
            .flat_map(|c| {
                c.block_entities
                    .iter()
                    .filter(|(_, be)| matches!(be, blockentity::BlockEntity::Furnace(_) | blockentity::BlockEntity::Brewing(_)))
                    .map(|(p, _)| *p)
            })
            .collect()
    }

    /// Frees saved chunks that nobody needs from memory.
    /// Approximate bytes of the loaded chunks.
    pub fn memory(&self) -> usize {
        self.chunks.values().map(Chunk::memory).sum()
    }

    /// Chunks that stay (no pending changes to save but out of everyone's view)
    /// drop their packet and border light, which are recomputed if needed.
    pub fn drop_caches_outside(&mut self, visible: impl Fn(i32, i32) -> bool) {
        for (&(x, z), c) in self.chunks.iter_mut() {
            if !visible(x, z) {
                c.drop_caches();
            }
        }
    }

    /// Unloads chunks nobody sees, except modified ones (they wait for the save) and those
    /// something is still using (flowing water, redstone, mobs): those, after a few unused rounds.
    pub fn unload_unused(&mut self, keep: impl Fn(i32, i32) -> bool) {
        let epoch = self.epoch;
        self.chunks.retain(|&(x, z), c| c.dirty || keep(x, z) || c.used + 6 > epoch);
        self.epoch += 1;
    }

    pub fn loaded_chunks(&self) -> usize {
        self.chunks.len()
    }

    /// Warns once if a world in the previous format is being migrated.
    pub fn note_legacy(&self) {
        if self.dim == Dimension::Overworld && self.dir.join("chunks").exists() {
            info!("se encontró el formato anterior (chunks/*.bin): los chunks se migran a Anvil al guardarse");
        }
    }
}

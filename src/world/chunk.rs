//! Chunk columns and their serialization into the 1.20.4 `map_chunk` packet.
//!
//! Height depends on the dimension (Overworld: -64..320; Nether and End: 0..256). Biomes
//! are stored per 4x4x4 cell, and light is computed within the column and then completed
//! with the light coming in from the four neighboring chunks.

use crate::data;
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use std::collections::VecDeque;
use std::sync::Arc;

/// Overworld bounds (other dimensions store their own in the chunk).
pub const MIN_Y: i32 = -64;
pub const HEIGHT: i32 = 384;
/// log2 of the total number of block states (direct palette).
const GLOBAL_BITS: u8 = 15;
/// Bits of the direct biome palette (64 biomes in 1.20.4).
const BIOME_BITS: u8 = 6;

/// Blocks of a 16x16x16 section in memory, like vanilla's palette: a uniform section
/// (all air, all stone) takes 2 bytes; with up to 16 distinct states (typical underground),
/// half a byte per block; with up to 256, 1 byte; only very varied sections store
/// 2 bytes per block.
#[derive(Clone, PartialEq, Debug)]
pub enum Blocks {
    Uniform(u16),
    Nibble(Vec<u16>, Box<[u8; 2048]>),
    Paletted(Vec<u16>, Box<[u8; 4096]>),
    Full(Box<[u16; 4096]>),
}

#[inline]
fn nib_get(idx: &[u8; 2048], i: usize) -> usize {
    ((idx[i >> 1] >> ((i & 1) * 4)) & 15) as usize
}

#[inline]
fn nib_set(idx: &mut [u8; 2048], i: usize, v: usize) {
    let shift = (i & 1) * 4;
    idx[i >> 1] = (idx[i >> 1] & !(15 << shift)) | ((v as u8 & 15) << shift);
}

impl Blocks {
    #[inline]
    pub fn get(&self, i: usize) -> u16 {
        match self {
            Blocks::Uniform(v) => *v,
            Blocks::Nibble(p, idx) => p[nib_get(idx, i)],
            Blocks::Paletted(p, idx) => p[idx[i] as usize],
            Blocks::Full(a) => a[i],
        }
    }

    /// Sets a block and returns the previous one (the section upgrades its format if needed).
    pub fn set(&mut self, i: usize, v: u16) -> u16 {
        match self {
            Blocks::Uniform(u) => {
                let old = *u;
                if old != v {
                    let mut idx = Box::new([0u8; 2048]);
                    nib_set(&mut idx, i, 1);
                    *self = Blocks::Nibble(vec![old, v], idx);
                }
                old
            }
            Blocks::Nibble(p, idx) => {
                let old = p[nib_get(idx, i)];
                if old == v {
                    return old;
                }
                let k = match p.iter().position(|&x| x == v) {
                    Some(k) => k,
                    None if p.len() < 16 => {
                        p.push(v);
                        p.len() - 1
                    }
                    None => {
                        // More than 16 states: switch to 1 byte per block.
                        let mut big = Box::new([0u8; 4096]);
                        for (j, slot) in big.iter_mut().enumerate() {
                            *slot = nib_get(idx, j) as u8;
                        }
                        let mut pal = std::mem::take(p);
                        pal.push(v);
                        big[i] = (pal.len() - 1) as u8;
                        *self = Blocks::Paletted(pal, big);
                        return old;
                    }
                };
                nib_set(idx, i, k);
                old
            }
            Blocks::Paletted(p, idx) => {
                let old = p[idx[i] as usize];
                if old == v {
                    return old;
                }
                let k = match p.iter().position(|&x| x == v) {
                    Some(k) => k,
                    None if p.len() < 256 => {
                        p.push(v);
                        p.len() - 1
                    }
                    None => {
                        let mut a = Box::new([0u16; 4096]);
                        for (j, slot) in a.iter_mut().enumerate() {
                            *slot = p[idx[j] as usize];
                        }
                        a[i] = v;
                        *self = Blocks::Full(a);
                        return old;
                    }
                };
                idx[i] = k as u8;
                old
            }
            Blocks::Full(a) => std::mem::replace(&mut a[i], v),
        }
    }

    pub fn fill(&mut self, v: u16) {
        *self = Blocks::Uniform(v);
    }

    pub fn uniform(&self) -> Option<u16> {
        match self {
            Blocks::Uniform(v) => Some(*v),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        (0..4096).map(move |i| self.get(i))
    }

    pub fn to_vec(&self) -> Vec<u16> {
        self.iter().collect()
    }

    /// From the 4096 states, in the smallest format.
    pub fn from_slice(values: &[u16]) -> Self {
        let mut b = Blocks::Full(Box::new(values.try_into().unwrap_or([0; 4096])));
        b.compact();
        b
    }

    /// Shrinks back to the smallest format that fits (after generating or loading, or if emptied).
    pub fn compact(&mut self) {
        let mut palette: Vec<u16> = Vec::new();
        for v in self.iter() {
            if !palette.contains(&v) {
                palette.push(v);
                if palette.len() > 256 {
                    break;
                }
            }
        }
        *self = match palette.len() {
            1 => Blocks::Uniform(palette[0]),
            n if n <= 16 => {
                let mut idx = Box::new([0u8; 2048]);
                for i in 0..4096 {
                    nib_set(&mut idx, i, palette.iter().position(|&p| p == self.get(i)).unwrap());
                }
                Blocks::Nibble(palette, idx)
            }
            n if n <= 256 => {
                let mut idx = Box::new([0u8; 4096]);
                for (i, slot) in idx.iter_mut().enumerate() {
                    *slot = palette.iter().position(|&p| p == self.get(i)).unwrap() as u8;
                }
                Blocks::Paletted(palette, idx)
            }
            _ => return,
        };
    }

    /// Bytes used in memory (approximate).
    pub fn bytes(&self) -> usize {
        match self {
            Blocks::Uniform(_) => 2,
            Blocks::Nibble(p, _) => 2048 + p.len() * 2,
            Blocks::Paletted(p, _) => 4096 + p.len() * 2,
            Blocks::Full(_) => 8192,
        }
    }
}

#[derive(Clone)]
pub struct Section {
    pub blocks: Blocks,
    /// Biome per 4x4x4 cell (index `(y << 4) | (z << 2) | x`).
    pub biomes: [u8; 64],
}

impl Section {
    fn new(biome: u8) -> Self {
        Self { blocks: Blocks::Uniform(0), biomes: [biome; 64] }
    }
}

#[inline]
fn index(x: usize, y: usize, z: usize) -> usize {
    (y << 8) | (z << 4) | x
}

/// Light on the four faces of the column (north z=0, south z=15, west x=0, east x=15), so
/// that neighbors can propagate it inward. Each face: `height * 16` values.
/// (Each light value takes half a byte.)
#[derive(Clone)]
pub struct LightFaces {
    pub sky: [Vec<u8>; 4],
    pub block: [Vec<u8>; 4],
}

#[inline]
fn nib(v: &[u8], i: usize) -> u8 {
    (v[i >> 1] >> ((i & 1) * 4)) & 15
}

#[derive(Clone)]
pub struct Chunk {
    pub min_y: i32,
    pub sections: Vec<Section>,
    /// Chests, furnaces... by absolute position.
    pub block_entities: std::collections::HashMap<BlockPos, super::blockentity::BlockEntity>,
    pub dirty: bool,
    /// Already serialized `map_chunk` packet, zlib-compressed (original size, data);
    /// invalidated when a block changes.
    pub cache: Option<Arc<(usize, Vec<u8>)>>,
    /// Own edge light (for neighbors); invalidated when a block changes.
    faces: Option<Arc<LightFaces>>,
    /// Height of the highest block in each column (maps); invalidated when a block changes.
    tops: Option<Box<[u16; 256]>>,
    /// Last unload round in which it was used (water, redstone, mobs... near nobody).
    pub used: u64,
}

impl Chunk {
    /// Empty column `height` blocks tall starting at `min_y`.
    pub fn new_dim(min_y: i32, height: i32, biome: u8) -> Self {
        let n = (height / 16) as usize;
        Self { min_y, sections: vec![Section::new(biome); n], block_entities: Default::default(), dirty: false, cache: None, faces: None, tops: None, used: 0 }
    }

    /// Empty Overworld column.
    pub fn new() -> Self {
        Self::new_dim(MIN_Y, HEIGHT, data::get().plains_biome as u8)
    }

    pub fn height(&self) -> i32 {
        self.sections.len() as i32 * 16
    }

    /// Local coordinates: x,z in 0..16, absolute y.
    pub fn get(&self, x: usize, y: i32, z: usize) -> u16 {
        let ry = y - self.min_y;
        if ry < 0 || ry >= self.height() {
            return 0;
        }
        let ry = ry as usize;
        self.sections[ry >> 4].blocks.get(index(x, ry & 15, z))
    }

    pub fn set(&mut self, x: usize, y: i32, z: usize, state: u16) -> u16 {
        let ry = y - self.min_y;
        if ry < 0 || ry >= self.height() {
            return 0;
        }
        let ry = ry as usize;
        let old = self.sections[ry >> 4].blocks.set(index(x, ry & 15, z), state);
        if old != state {
            self.dirty = true;
            self.cache = None;
            self.faces = None;
            self.tops = None;
        }
        old
    }

    /// Biome of the 4x4x4 cell containing local (x, y, z).
    pub fn biome(&self, x: usize, y: i32, z: usize) -> u8 {
        let ry = (y - self.min_y).clamp(0, self.height() - 1) as usize;
        self.sections[ry >> 4].biomes[((ry & 15) >> 2 << 4) | (z >> 2 << 2) | (x >> 2)]
    }

    /// Sets the biome of the whole column of cells at (x4, z4) (cell coordinates 0..4).
    pub fn set_biome_column(&mut self, x4: usize, z4: usize, biome: u8) {
        for s in &mut self.sections {
            for y4 in 0..4 {
                s.biomes[(y4 << 4) | (z4 << 2) | x4] = biome;
            }
        }
    }

    /// `map_chunk` packet (unframed body) for the column at (cx, cz).
    /// `neighbors`: edge light of the north, south, west and east chunks.
    pub fn packet(&mut self, cx: i32, cz: i32, has_sky: bool, neighbors: [Option<Arc<LightFaces>>; 4]) -> Arc<Vec<u8>> {
        use std::io::{Read, Write};
        if let Some(c) = &self.cache {
            let mut out = Vec::with_capacity(c.0);
            if flate2::read::ZlibDecoder::new(&c.1[..]).read_to_end(&mut out).is_ok() && out.len() == c.0 {
                return Arc::new(out);
            }
        }
        let p = self.encode(cx, cz, has_sky, &neighbors);
        // Stored compressed: the packet carries light for 26 sections (~110 KB) and most of it
        // repeats; compressed it is about 10 times smaller.
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        if enc.write_all(&p).is_ok() {
            if let Ok(z) = enc.finish() {
                self.cache = Some(Arc::new((p.len(), z)));
            }
        }
        Arc::new(p)
    }

    /// Frees whatever can be recomputed (packet and edge light).
    pub fn drop_caches(&mut self) {
        self.cache = None;
        self.faces = None;
        self.tops = None;
    }

    /// Puts each section into its smallest format.
    pub fn compact(&mut self) {
        for s in &mut self.sections {
            s.blocks.compact();
        }
    }

    /// Approximate bytes used in memory.
    pub fn memory(&self) -> usize {
        let blocks: usize = self.sections.iter().map(|s| s.blocks.bytes() + 64).sum();
        let cache = self.cache.as_ref().map_or(0, |c| c.1.len());
        let faces = self.faces.as_ref().map_or(0, |f| f.sky.iter().chain(f.block.iter()).map(Vec::len).sum());
        blocks + cache + faces + if self.tops.is_some() { 512 } else { 0 }
    }

    /// Own edge light (computed once and kept until a block changes).
    pub fn faces(&mut self, has_sky: bool) -> Arc<LightFaces> {
        if let Some(f) = &self.faces {
            return f.clone();
        }
        let (sky, block) = self.compute_light(has_sky);
        let f = Arc::new(faces_of(&sky, &block, self.height() as usize));
        self.faces = Some(f.clone());
        f
    }

    /// Y of the highest (non-air) block in local column (x, z); `min_y - 1` if none.
    pub fn top_y(&mut self, x: usize, z: usize) -> i32 {
        if self.tops.is_none() {
            self.tops = Some(Box::new(self.heights()));
        }
        self.min_y + self.tops.as_ref().unwrap()[z * 16 + x] as i32 - 1
    }

    fn heights(&self) -> [u16; 256] {
        let reg = data::get();
        let mut heights = [0u16; 256];
        for z in 0..16 {
            for x in 0..16 {
                heights[z * 16 + x] = (0..self.height()).rev().find(|&ry| !reg.is_air(self.get(x, ry + self.min_y, z))).map_or(0, |ry| ry as u16 + 1);
            }
        }
        heights
    }

    fn encode(&mut self, cx: i32, cz: i32, has_sky: bool, neighbors: &[Option<Arc<LightFaces>>; 4]) -> Vec<u8> {
        let reg = data::get();
        let mut out = packet(id::play::MAP_CHUNK);
        out.i32(cx).i32(cz);

        // MOTION_BLOCKING heightmap: 256 values of ceil(log2(height+1)) bits.
        let heights = self.heights();
        let bits = (usize::BITS - (self.height() as usize).leading_zeros()) as usize;
        let per_long = 64 / bits;
        let longs = heights
            .chunks(per_long)
            .map(|c| c.iter().enumerate().fold(0i64, |acc, (i, &h)| acc | (h as i64) << (i * bits)))
            .collect();
        Nbt::Compound(vec![("MOTION_BLOCKING".into(), Nbt::LongArray(longs))]).write_network(&mut out);

        let mut sections = Vec::with_capacity(16 * 1024);
        for s in &self.sections {
            match s.blocks.uniform() {
                // Uniform section: single-value palette, no data.
                Some(v) => {
                    sections.i16(if reg.is_air(v) { 0 } else { 4096 });
                    sections.u8(0).varint(v as i32).varint(0);
                }
                None => {
                    let values = s.blocks.to_vec();
                    let non_air = values.iter().filter(|&&b| !reg.is_air(b)).count();
                    sections.i16(non_air as i16);
                    write_paletted(&mut sections, &values, 4, 8, GLOBAL_BITS);
                }
            }
            let biomes: Vec<u16> = s.biomes.iter().map(|&b| b as u16).collect();
            write_paletted(&mut sections, &biomes, 1, 3, BIOME_BITS);
        }
        out.varint(sections.len() as i32);
        out.extend_from_slice(&sections);
        // Block entities: the client needs them to render chests, beds, signs...
        let types = be_types();
        let mut entries = Vec::new();
        for (s, sec) in self.sections.iter().enumerate() {
            if sec.blocks.uniform().is_some_and(|v| types.get(v as usize).copied().unwrap_or(-1) < 0) {
                continue;
            }
            for (i, state) in sec.blocks.iter().enumerate() {
                let t = types.get(state as usize).copied().unwrap_or(-1);
                if t >= 0 {
                    entries.push(((i & 15) as u8, ((i >> 4) & 15) as u8, (s * 16 + (i >> 8)) as i32 + self.min_y, t));
                }
            }
        }
        out.varint(entries.len() as i32);
        for (x, z, y, t) in entries {
            out.u8((x << 4) | z).i16(y as i16).varint(t as i32);
            let pos = BlockPos::new(cx * 16 + x as i32, y, cz * 16 + z as i32);
            match self.block_entities.get(&pos).and_then(|be| be.client_nbt()) {
                Some(n) => n.write_network(&mut out),
                None => {
                    out.u8(0);
                }
            }
        }

        let (mut sky, mut block) = self.compute_light(has_sky);
        self.faces = Some(Arc::new(faces_of(&sky, &block, self.height() as usize)));
        self.light_from_neighbors(&mut sky, &mut block, neighbors);
        write_light(&mut out, &sky, &block, self.sections.len(), has_sky);
        out
    }

    fn filters(&self) -> Vec<u8> {
        let reg = data::get();
        let h = self.height() as usize;
        (0..h * 256).map(|i| reg.light(self.get(i & 15, (i >> 8) as i32 + self.min_y, (i >> 4) & 15)).0).collect()
    }

    /// Sky and block light computed only within the column.
    fn compute_light(&self, has_sky: bool) -> (Vec<u8>, Vec<u8>) {
        let reg = data::get();
        let h = self.height() as usize;
        let filter = self.filters();
        let mut sky = vec![0u8; h * 256];
        let mut block = vec![0u8; h * 256];

        if has_sky {
            // Sky: falls vertically, attenuated only by filtering blocks. Horizontal propagation is
            // only needed below the highest surface (shadows, caves).
            let heights = self.heights();
            let max_h = *heights.iter().max().unwrap() as usize;
            let mut queue = VecDeque::new();
            for z in 0..16 {
                for x in 0..16 {
                    let mut l = 15u8;
                    for y in (0..h).rev() {
                        let i = index(x, y, z);
                        l = l.saturating_sub(filter[i]);
                        if l == 0 {
                            break;
                        }
                        sky[i] = l;
                        if y <= max_h {
                            queue.push_back(i);
                        }
                    }
                }
            }
            flood(&mut sky, &filter, queue);
        }

        let mut queue = VecDeque::new();
        for (i, b) in block.iter_mut().enumerate() {
            let e = reg.light(self.get(i & 15, (i >> 8) as i32 + self.min_y, (i >> 4) & 15)).1;
            if e > 0 {
                *b = e;
                queue.push_back(i);
            }
        }
        flood(&mut block, &filter, queue);
        (sky, block)
    }

    /// Completes the light with what enters through the edges from the neighbors.
    fn light_from_neighbors(&self, sky: &mut [u8], block: &mut [u8], neighbors: &[Option<Arc<LightFaces>>; 4]) {
        let filter = self.filters();
        let h = self.height() as usize;
        for (light, is_sky) in [(sky, true), (block, false)] {
            let mut queue = VecDeque::new();
            for (side, n) in neighbors.iter().enumerate() {
                let Some(n) = n else { continue };
                // The neighbor's face touching this chunk is the opposite one: north<->south, west<->east.
                let face = if is_sky { &n.sky[side ^ 1] } else { &n.block[side ^ 1] };
                if face.len() != h * 8 {
                    continue;
                }
                for y in 0..h {
                    for a in 0..16 {
                        let incoming = nib(face, y * 16 + a);
                        if incoming <= 1 {
                            continue;
                        }
                        let (x, z) = match side {
                            0 => (a, 0),
                            1 => (a, 15),
                            2 => (0, a),
                            _ => (15, a),
                        };
                        let i = index(x, y, z);
                        let nl = incoming.saturating_sub(1 + filter[i]);
                        if nl > light[i] {
                            light[i] = nl;
                            queue.push_back(i);
                        }
                    }
                }
            }
            flood(light, &filter, queue);
        }
    }
}

fn faces_of(sky: &[u8], block: &[u8], h: usize) -> LightFaces {
    let face = |light: &[u8], side: usize| -> Vec<u8> {
        let mut v = vec![0u8; h * 8];
        for y in 0..h {
            for a in 0..16 {
                let (x, z) = match side {
                    0 => (a, 0),
                    1 => (a, 15),
                    2 => (0, a),
                    _ => (15, a),
                };
                let i = y * 16 + a;
                v[i >> 1] |= (light[index(x, y, z)] & 15) << ((i & 1) * 4);
            }
        }
        v
    };
    LightFaces { sky: std::array::from_fn(|s| face(sky, s)), block: std::array::from_fn(|s| face(block, s)) }
}

/// Block entity type per state (-1 = none).
fn be_types() -> &'static [i16] {
    static T: std::sync::OnceLock<Vec<i16>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let reg = data::get();
        let n = reg.blocks.last().map_or(0, |b| b.max_state as usize + 1);
        let mut v = vec![-1i16; n];
        for b in &reg.blocks {
            if let Some(t) = super::blockentity::type_id(&b.name) {
                for s in b.min_state..=b.max_state {
                    v[s as usize] = t as i16;
                }
            }
        }
        v
    })
}

/// BFS light propagation within the column.
fn flood(light: &mut [u8], filter: &[u8], mut queue: VecDeque<usize>) {
    let h = light.len() >> 8;
    while let Some(i) = queue.pop_front() {
        let l = light[i];
        if l <= 1 {
            continue;
        }
        let (x, y, z) = (i & 15, i >> 8, (i >> 4) & 15);
        let mut visit = |j: usize| {
            let nl = l.saturating_sub(1 + filter[j]);
            if nl > light[j] {
                light[j] = nl;
                queue.push_back(j);
            }
        };
        if x > 0 {
            visit(i - 1);
        }
        if x < 15 {
            visit(i + 1);
        }
        if z > 0 {
            visit(i - 16);
        }
        if z < 15 {
            visit(i + 16);
        }
        if y > 0 {
            visit(i - 256);
        }
        if y + 1 < h {
            visit(i + 256);
        }
    }
}

fn write_light(out: &mut Vec<u8>, sky: &[u8], block: &[u8], sections: usize, has_sky: bool) {
    // Section 0 = the one below the world (no light), 1..=n the world, n+1 the one above.
    let light_sections = sections + 2;
    let mut sky_arrays: Vec<(usize, Vec<u8>)> = Vec::new();
    let mut block_arrays: Vec<(usize, Vec<u8>)> = Vec::new();
    for s in 0..sections {
        let range = s * 4096..(s + 1) * 4096;
        if has_sky && sky[range.clone()].iter().any(|&l| l > 0) {
            sky_arrays.push((s + 1, nibbles(&sky[range.clone()])));
        }
        if block[range.clone()].iter().any(|&l| l > 0) {
            block_arrays.push((s + 1, nibbles(&block[range])));
        }
    }
    if has_sky {
        sky_arrays.push((light_sections - 1, vec![0xff; 2048]));
    }

    let mask = |arrays: &[(usize, Vec<u8>)]| arrays.iter().fold(0i64, |m, (s, _)| m | 1 << s);
    let all = (1i64 << light_sections) - 1;
    let (sky_mask, block_mask) = (mask(&sky_arrays), mask(&block_arrays));
    // Without sky (Nether, End) no sky light is sent or flagged.
    let empty_sky = if has_sky { all & !sky_mask } else { 0 };
    for m in [sky_mask, block_mask, empty_sky, all & !block_mask] {
        if m == 0 {
            out.varint(0);
        } else {
            out.varint(1).i64(m);
        }
    }
    for arrays in [sky_arrays, block_arrays] {
        out.varint(arrays.len() as i32);
        for (_, a) in arrays {
            out.varint(a.len() as i32);
            out.extend_from_slice(&a);
        }
    }
}

fn nibbles(light: &[u8]) -> Vec<u8> {
    light.chunks(2).map(|p| (p[0] & 15) | (p[1] << 4)).collect()
}

/// Paletted container (block states or biomes): single value, indirect palette of
/// `min_bits..=max_bits` bits, or direct palette of `global_bits`.
fn write_paletted(out: &mut Vec<u8>, values: &[u16], min_bits: u8, max_bits: u8, global_bits: u8) {
    let limit = 1usize << max_bits;
    let mut palette: Vec<u16> = Vec::new();
    for &v in values {
        if !palette.contains(&v) {
            palette.push(v);
            if palette.len() > limit {
                break;
            }
        }
    }
    if palette.len() == 1 {
        out.u8(0).varint(palette[0] as i32).varint(0);
        return;
    }
    let (bits, indirect) = match palette.len() {
        n if n <= limit => ((usize::BITS - (n - 1).leading_zeros()).max(min_bits as u32) as u8, true),
        _ => (global_bits, false),
    };
    out.u8(bits);
    if indirect {
        out.varint(palette.len() as i32);
        for &p in &palette {
            out.varint(p as i32);
        }
    }
    let per_long = 64 / bits as usize;
    let longs = values.len().div_ceil(per_long);
    out.varint(longs as i32);
    let lookup = |v: u16| if indirect { palette.iter().position(|&p| p == v).unwrap() as u64 } else { v as u64 };
    for chunk in values.chunks(per_long) {
        let mut l = 0u64;
        for (i, &v) in chunk.iter().enumerate() {
            l |= lookup(v) << (i * bits as usize);
        }
        out.i64(l as i64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_change_format() {
        let mut b = Blocks::Uniform(0);
        assert_eq!(b.set(5, 1), 0);
        assert!(matches!(b, Blocks::Nibble(..)));
        for i in 0..20 {
            b.set(i, i as u16 + 100);
        }
        assert!(matches!(b, Blocks::Paletted(..)), "más de 16 estados");
        assert_eq!(b.get(19), 119);
        assert_eq!(b.get(5), 105);
        for i in 0..300 {
            b.set(i, i as u16 + 10);
        }
        assert!(matches!(b, Blocks::Full(_)), "más de 256 estados");
        assert_eq!(b.get(299), 309);
        for i in 0..4096 {
            b.set(i, 7);
        }
        b.compact();
        assert_eq!(b, Blocks::Uniform(7));
        let mut v = vec![1u16; 4096];
        v[100] = 2;
        let p = Blocks::from_slice(&v);
        assert!(matches!(p, Blocks::Nibble(ref pal, _) if pal.len() == 2));
        assert_eq!(p.to_vec(), v);
    }

    #[test]
    fn light_crosses_chunk_borders() {
        let reg = data::get();
        let glow = reg.state("glowstone");
        let stone = reg.state("stone");
        // Two chunks closed at the top; a light source against the east edge of the first one.
        let mut a = Chunk::new();
        let mut b = Chunk::new();
        for c in [&mut a, &mut b] {
            for x in 0..16 {
                for z in 0..16 {
                    c.set(x, 0, z, stone);
                    c.set(x, 5, z, stone);
                }
            }
        }
        a.set(15, 2, 8, glow);
        let fa = a.faces(true);
        let (_, mut block) = b.compute_light(true);
        let (mut sky, _) = b.compute_light(true);
        assert_eq!(block[index(0, (2 - MIN_Y) as usize, 8)], 0, "sin vecinos no llega luz");
        b.light_from_neighbors(&mut sky, &mut block, &[None, None, Some(fa), None]);
        // In the neighboring chunk (west = a), 1 block from the glowstone: 15 - 1 = 14.
        assert_eq!(block[index(0, (2 - MIN_Y) as usize, 8)], 14);
        assert_eq!(block[index(3, (2 - MIN_Y) as usize, 8)], 11);
    }

    #[test]
    fn nether_height() {
        let c = Chunk::new_dim(0, 256, 0);
        assert_eq!(c.sections.len(), 16);
        assert_eq!(c.get(0, 300, 0), 0);
    }
}

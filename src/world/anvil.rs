//! Vanilla Anvil format (DataVersion 3700 = 1.20.4): region files `r.X.Z.mca` with
//! chunks in NBT (per-section block and biome palettes, block entities) and `level.dat`.
//! This way the world can be opened with vanilla or edited with MCA Selector, Amulet, NBTExplorer...

use super::blockentity::{banner_from_nbt, BlockEntity, Brewing, Furnace, Sign};
use super::chunk::Chunk;
use super::Dimension;
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::{BlockPos, Nbt, Reader};
use anyhow::{bail, Context, Result};
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const DATA_VERSION: i32 = 3700;
const SECTOR: usize = 4096;

fn region_path(dir: &Path, cx: i32, cz: i32) -> PathBuf {
    dir.join("region").join(format!("r.{}.{}.mca", cx >> 5, cz >> 5))
}

fn index(cx: i32, cz: i32) -> usize {
    ((cx & 31) + (cz & 31) * 32) as usize
}

/// Reads a chunk's NBT from its region file (or `None` if it doesn't exist).
pub fn read_chunk(dir: &Path, cx: i32, cz: i32) -> Result<Option<Nbt>> {
    let path = region_path(dir, cx, cz);
    let Ok(file) = std::fs::read(&path) else { return Ok(None) };
    if file.len() < 2 * SECTOR {
        return Ok(None);
    }
    let i = index(cx, cz) * 4;
    let loc = u32::from_be_bytes(file[i..i + 4].try_into().unwrap());
    let (offset, count) = ((loc >> 8) as usize * SECTOR, (loc & 0xff) as usize);
    if offset == 0 || count == 0 || offset + 5 > file.len() {
        return Ok(None);
    }
    let len = u32::from_be_bytes(file[offset..offset + 4].try_into().unwrap()) as usize;
    let kind = file[offset + 4];
    let payload = file.get(offset + 5..offset + 4 + len).context("chunk truncado")?;
    let mut raw = Vec::new();
    match kind {
        1 => {
            GzDecoder::new(payload).read_to_end(&mut raw)?;
        }
        2 => {
            ZlibDecoder::new(payload).read_to_end(&mut raw)?;
        }
        3 => raw = payload.to_vec(),
        k => bail!("compresión de chunk desconocida: {k}"),
    }
    Ok(Some(Nbt::read_named(&mut Reader::new(&raw))?.1))
}

/// Writes several chunks (already in NBT) by rewriting their region files.
pub fn write_chunks(dir: &Path, chunks: Vec<((i32, i32), Nbt)>) -> Result<()> {
    std::fs::create_dir_all(dir.join("region"))?;
    let mut by_region: HashMap<(i32, i32), Vec<((i32, i32), Nbt)>> = HashMap::new();
    for c in chunks {
        by_region.entry((c.0 .0 >> 5, c.0 .1 >> 5)).or_default().push(c);
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as u32;
    for ((rx, rz), list) in by_region {
        let path = region_path(dir, rx * 32, rz * 32);
        // Existing blobs (as-is, compressed) to preserve the remaining chunks.
        let mut blobs: Vec<Option<(Vec<u8>, u32)>> = vec![None; 1024];
        if let Ok(file) = std::fs::read(&path) {
            if file.len() >= 2 * SECTOR {
                for (i, blob) in blobs.iter_mut().enumerate() {
                    let loc = u32::from_be_bytes(file[i * 4..i * 4 + 4].try_into().unwrap());
                    let ts = u32::from_be_bytes(file[SECTOR + i * 4..SECTOR + i * 4 + 4].try_into().unwrap());
                    let offset = (loc >> 8) as usize * SECTOR;
                    if offset >= 2 * SECTOR && offset + 4 <= file.len() {
                        let len = u32::from_be_bytes(file[offset..offset + 4].try_into().unwrap()) as usize;
                        if let Some(b) = file.get(offset..offset + 4 + len) {
                            *blob = Some((b.to_vec(), ts));
                        }
                    }
                }
            }
        }
        for ((cx, cz), nbt) in list {
            let mut raw = Vec::new();
            nbt.write_named("", &mut raw);
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&raw)?;
            let data = enc.finish()?;
            let mut blob = Vec::with_capacity(data.len() + 5);
            blob.extend_from_slice(&(data.len() as u32 + 1).to_be_bytes());
            blob.push(2);
            blob.extend_from_slice(&data);
            blobs[index(cx, cz)] = Some((blob, now));
        }
        let mut header = vec![0u8; 2 * SECTOR];
        let mut body = Vec::new();
        for (i, blob) in blobs.iter().enumerate() {
            let Some((b, ts)) = blob else { continue };
            let sectors = b.len().div_ceil(SECTOR);
            if sectors > 255 {
                bail!("chunk demasiado grande para Anvil ({} bytes)", b.len());
            }
            let offset = 2 + body.len() / SECTOR;
            header[i * 4..i * 4 + 4].copy_from_slice(&(((offset as u32) << 8) | sectors as u32).to_be_bytes());
            header[SECTOR + i * 4..SECTOR + i * 4 + 4].copy_from_slice(&ts.to_be_bytes());
            body.extend_from_slice(b);
            body.resize(body.len().div_ceil(SECTOR) * SECTOR, 0);
        }
        header.extend_from_slice(&body);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, header)?;
        std::fs::rename(tmp, &path)?;
    }
    Ok(())
}

// ---------- palettes ----------

/// Packs indices without crossing long boundaries (format since 1.16).
fn pack(indices: &[usize], bits: usize) -> Vec<i64> {
    let per = 64 / bits;
    indices
        .chunks(per)
        .map(|c| c.iter().enumerate().fold(0u64, |acc, (i, &v)| acc | (v as u64) << (i * bits)) as i64)
        .collect()
}

fn unpack(data: &[i64], bits: usize, n: usize) -> Vec<usize> {
    let per = 64 / bits;
    let mask = (1u64 << bits) - 1;
    (0..n).map(|i| data.get(i / per).map_or(0, |&l| ((l as u64) >> ((i % per) * bits) & mask) as usize)).collect()
}

/// Block state -> {Name, Properties}.
fn state_to_nbt(state: u16) -> Nbt {
    let reg = data::get();
    let b = reg.block_of_state(state);
    let mut c = Nbt::compound().with("Name", Nbt::str(&format!("minecraft:{}", b.name)));
    if !b.props.is_empty() {
        let props = b.props.iter().map(|(k, _)| (k.clone(), Nbt::str(b.get(state, k).unwrap_or("")))).collect();
        c.set("Properties", Nbt::Compound(props));
    }
    c
}

/// {Name, Properties} -> block state (air if it doesn't exist).
fn nbt_to_state(n: &Nbt) -> u16 {
    let reg = data::get();
    let Some(b) = n.get_str("Name").and_then(|name| reg.block(name)) else { return 0 };
    let mut s = b.default_state;
    if let Some(Nbt::Compound(props)) = n.get("Properties") {
        for (k, v) in props {
            if let Some(v) = v.as_str() {
                s = b.with(s, k, v);
            }
        }
    }
    s
}

// ---------- items and block entities ----------

fn item_to_nbt(slot: usize, item: &ItemStack) -> Nbt {
    let mut c = Nbt::compound()
        .with("Slot", Nbt::Byte(slot as i8))
        .with("id", Nbt::str(&format!("minecraft:{}", item.name())))
        .with("Count", Nbt::Byte(item.count));
    if let Some(tag) = &item.nbt {
        c.set("tag", (**tag).clone());
    }
    c
}

/// An item stored on its own (a jukebox record, a lectern book).
fn single_item(n: Option<&Nbt>) -> Slot {
    let n = n?;
    let id = data::get().item_id(n.get_str("id")?)?;
    Some(ItemStack { id, count: n.get_i64("Count").unwrap_or(1).max(1) as i8, nbt: n.get("tag").cloned().map(Box::new) })
}

fn items_from_nbt(list: Option<&Nbt>, len: usize) -> Vec<Slot> {
    let reg = data::get();
    let mut out = vec![None; len];
    for e in list.map(Nbt::as_list).unwrap_or(&[]) {
        let slot = e.get_i64("Slot").unwrap_or(-1);
        let Some(id) = e.get_str("id").and_then(|n| reg.item_id(n)) else { continue };
        let count = e.get_i64("Count").unwrap_or(1) as i8;
        if (0..len as i64).contains(&slot) && count > 0 {
            out[slot as usize] = Some(ItemStack { id, count, nbt: e.get("tag").cloned().map(Box::new) });
        }
    }
    out
}

/// Block entity id in the save format (all signs are `sign`...).
fn be_id(block: &str) -> &str {
    if block.ends_with("_hanging_sign") {
        "hanging_sign"
    } else if block.ends_with("_sign") {
        "sign"
    } else if block.ends_with("_banner") {
        "banner"
    } else if block == "spawner" {
        "mob_spawner"
    } else if block == "bee_nest" {
        "beehive"
    } else {
        block
    }
}

fn block_entity_to_nbt(pos: BlockPos, be: &BlockEntity, block: &str) -> Nbt {
    let base = Nbt::compound()
        .with("id", Nbt::str(&format!("minecraft:{}", be_id(block))))
        .with("x", Nbt::Int(pos.x))
        .with("y", Nbt::Int(pos.y))
        .with("z", Nbt::Int(pos.z))
        .with("keepPacked", Nbt::Byte(0));
    let items = |slots: &[Slot]| Nbt::List(slots.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|it| item_to_nbt(i, it))).collect());
    match be {
        BlockEntity::Container(slots) => base.with("Items", items(slots)),
        BlockEntity::Furnace(f) => base
            .with("Items", items(&f.items))
            .with("BurnTime", Nbt::Short(f.burn as i16))
            .with("CookTime", Nbt::Short(f.cook as i16))
            .with("CookTimeTotal", Nbt::Short(if f.kind == 0 { 200 } else { 100 }))
            .with("bambietta_burn_total", Nbt::Int(f.burn_total))
            .with("bambietta_xp", Nbt::Float(f.xp)),
        BlockEntity::Brewing(b) => base
            .with("Items", items(&b.items))
            .with("BrewTime", Nbt::Short(b.brew_time as i16))
            .with("Fuel", Nbt::Byte(b.fuel as i8)),
        BlockEntity::Hive { bees } => base.with("Bees", Nbt::List(vec![])).with("bambietta_bees", Nbt::Int(*bees as i32)),
        BlockEntity::Jukebox(record) => match record {
            Some(r) => base.with("RecordItem", item_to_nbt(0, r)),
            None => base,
        },
        BlockEntity::Lectern { book, page } => match book {
            Some(b) => base.with("Book", item_to_nbt(0, b)).with("Page", Nbt::Int(*page)),
            None => base,
        },
        BlockEntity::Comparator(out) => base.with("OutputSignal", Nbt::Int(*out as i32)),
        BlockEntity::Spawner { delay, .. } => {
            let mut n = base.with("Delay", Nbt::Short(*delay as i16));
            if let Some(Nbt::Compound(fields)) = be.client_nbt() {
                for (k, v) in fields {
                    n.set(&k, v);
                }
            }
            n
        }
        BlockEntity::Sign(_) | BlockEntity::Banner(_) => {
            let mut n = base;
            if let Some(Nbt::Compound(fields)) = be.client_nbt() {
                for (k, v) in fields {
                    n.set(&k, v);
                }
            }
            n
        }
    }
}

fn block_entity_from_nbt(n: &Nbt) -> Option<(BlockPos, BlockEntity)> {
    let pos = BlockPos::new(n.get_i64("x")? as i32, n.get_i64("y")? as i32, n.get_i64("z")? as i32);
    let id = n.get_str("id")?.trim_start_matches("minecraft:");
    let be = match id {
        "chest" | "trapped_chest" | "barrel" => BlockEntity::Container(items_from_nbt(n.get("Items"), 27)),
        "hopper" => BlockEntity::Container(items_from_nbt(n.get("Items"), 5)),
        "dispenser" | "dropper" => BlockEntity::Container(items_from_nbt(n.get("Items"), 9)),
        "jukebox" => BlockEntity::Jukebox(single_item(n.get("RecordItem"))),
        "lectern" => BlockEntity::Lectern { book: single_item(n.get("Book")), page: n.get_i64("Page").unwrap_or(0) as i32 },
        "comparator" => BlockEntity::Comparator(n.get_i64("OutputSignal").unwrap_or(0).clamp(0, 15) as u8),
        "furnace" | "smoker" | "blast_furnace" => {
            let burn = n.get_i64("BurnTime").unwrap_or(0) as i32;
            BlockEntity::Furnace(Furnace {
                items: items_from_nbt(n.get("Items"), 3),
                burn,
                burn_total: n.get_i64("bambietta_burn_total").map_or(burn, |v| v as i32),
                cook: n.get_i64("CookTime").unwrap_or(0) as i32,
                xp: match n.get("bambietta_xp") {
                    Some(Nbt::Float(f)) => *f,
                    _ => 0.0,
                },
                kind: Furnace::kind_of(id).unwrap_or(0),
            })
        }
        "brewing_stand" => BlockEntity::Brewing(Brewing {
            items: items_from_nbt(n.get("Items"), 5),
            brew_time: n.get_i64("BrewTime").unwrap_or(0) as i32,
            fuel: n.get_i64("Fuel").unwrap_or(0) as i32,
            ingredient: 0,
        }),
        "sign" | "hanging_sign" => BlockEntity::Sign(Sign::from_nbt(n)),
        "banner" => BlockEntity::Banner(banner_from_nbt(Some(n))),
        "beehive" => BlockEntity::Hive { bees: n.get_i64("bambietta_bees").unwrap_or(0) as u32 },
        "mob_spawner" => {
            let mob = n.get("SpawnData").and_then(|d| d.get("entity")).and_then(|e| e.get_str("id")).unwrap_or("minecraft:pig").trim_start_matches("minecraft:").to_string();
            BlockEntity::Spawner { mob, delay: n.get_i64("Delay").unwrap_or(20) as i32 }
        }
        _ => return None,
    };
    Some((pos, be))
}

// ---------- chunks ----------

pub fn chunk_to_nbt(c: &Chunk, cx: i32, cz: i32) -> Nbt {
    let reg = data::get();
    let mut sections = Vec::new();
    for (i, s) in c.sections.iter().enumerate() {
        // Block palette.
        let mut palette: Vec<u16> = Vec::new();
        let mut map = HashMap::new();
        let idx: Vec<usize> = s
            .blocks
            .iter()
            .map(|b| {
                *map.entry(b).or_insert_with(|| {
                    palette.push(b);
                    palette.len() - 1
                })
            })
            .collect();
        let mut states = Nbt::compound().with("palette", Nbt::List(palette.iter().map(|&p| state_to_nbt(p)).collect()));
        if palette.len() > 1 {
            let bits = ((usize::BITS - (palette.len() - 1).leading_zeros()) as usize).max(4);
            states.set("data", Nbt::LongArray(pack(&idx, bits)));
        }
        // Biome palette.
        let mut bpal: Vec<u8> = Vec::new();
        let bidx: Vec<usize> = s
            .biomes
            .iter()
            .map(|b| {
                bpal.iter().position(|x| x == b).unwrap_or_else(|| {
                    bpal.push(*b);
                    bpal.len() - 1
                })
            })
            .collect();
        let mut biomes = Nbt::compound().with("palette", Nbt::List(bpal.iter().map(|&b| Nbt::str(reg.biome_name(b))).collect()));
        if bpal.len() > 1 {
            let bits = (usize::BITS - (bpal.len() - 1).leading_zeros()) as usize;
            biomes.set("data", Nbt::LongArray(pack(&bidx, bits)));
        }
        sections.push(Nbt::compound().with("Y", Nbt::Byte((c.min_y / 16 + i as i32) as i8)).with("block_states", states).with("biomes", biomes));
    }
    let bes = c
        .block_entities
        .iter()
        .map(|(p, be)| {
            let name = reg.block_of_state(c.get((p.x & 15) as usize, p.y, (p.z & 15) as usize)).name.clone();
            block_entity_to_nbt(*p, be, &name)
        })
        .collect();
    Nbt::compound()
        .with("DataVersion", Nbt::Int(DATA_VERSION))
        .with("xPos", Nbt::Int(cx))
        .with("zPos", Nbt::Int(cz))
        .with("yPos", Nbt::Int(c.min_y / 16))
        .with("Status", Nbt::str("minecraft:full"))
        .with("LastUpdate", Nbt::Long(0))
        .with("InhabitedTime", Nbt::Long(0))
        // Let vanilla recompute lighting when opening it.
        .with("isLightOn", Nbt::Byte(0))
        .with("sections", Nbt::List(sections))
        .with("block_entities", Nbt::List(bes))
}

pub fn chunk_from_nbt(n: &Nbt, dim: Dimension) -> Result<Chunk> {
    let reg = data::get();
    let status = n.get_str("Status").unwrap_or("minecraft:full");
    if !status.ends_with("full") {
        bail!("chunk sin terminar ({status})");
    }
    let mut c = Chunk::new_dim(dim.min_y(), dim.height(), reg.biome_id(dim.default_biome()));
    for s in n.get("sections").map(Nbt::as_list).unwrap_or(&[]) {
        let Some(y) = s.get_i64("Y") else { continue };
        let i = y as i32 - dim.min_y() / 16;
        if i < 0 || i as usize >= c.sections.len() {
            continue;
        }
        let sec = &mut c.sections[i as usize];
        if let Some(states) = s.get("block_states") {
            let palette: Vec<u16> = states.get("palette").map(Nbt::as_list).unwrap_or(&[]).iter().map(nbt_to_state).collect();
            if palette.len() == 1 {
                sec.blocks.fill(palette[0]);
            } else if let Some(Nbt::LongArray(d)) = states.get("data") {
                let bits = ((usize::BITS - (palette.len() - 1).leading_zeros()) as usize).max(4);
                let values: Vec<u16> = unpack(d, bits, 4096).into_iter().map(|v| palette.get(v).copied().unwrap_or(0)).collect();
                sec.blocks = super::chunk::Blocks::from_slice(&values);
            }
        }
        if let Some(biomes) = s.get("biomes") {
            let palette: Vec<u8> = biomes.get("palette").map(Nbt::as_list).unwrap_or(&[]).iter().map(|b| reg.biome_id(b.as_str().unwrap_or("plains"))).collect();
            if palette.len() == 1 {
                sec.biomes = [palette[0]; 64];
            } else if let Some(Nbt::LongArray(d)) = biomes.get("data") {
                let bits = (usize::BITS - (palette.len() - 1).leading_zeros()) as usize;
                for (k, v) in unpack(d, bits, 64).into_iter().enumerate() {
                    sec.biomes[k] = palette.get(v).copied().unwrap_or(0);
                }
            }
        }
    }
    for be in n.get("block_entities").map(Nbt::as_list).unwrap_or(&[]) {
        if let Some((p, e)) = block_entity_from_nbt(be) {
            c.block_entities.insert(p, e);
        }
    }
    c.dirty = false;
    Ok(c)
}

// ---------- level.dat ----------

pub struct LevelInfo {
    pub spawn: Option<BlockPos>,
    pub time: i64,
    pub day_time: i64,
    /// `DragonFight.DragonKilled` (worlds without saved fight data: no).
    pub dragon_killed: bool,
    /// (raining, rainTime, thundering, thunderTime, clearWeatherTime).
    pub weather: (bool, i32, bool, i32, i32),
    /// `GameRules` (name, value as text).
    pub rules: Vec<(String, String)>,
    /// World border, if saved.
    pub border: Option<LevelBorder>,
}

/// World border in `level.dat` (vanilla's `Border*` keys).
#[derive(Clone, Debug, PartialEq)]
pub struct LevelBorder {
    pub center: [f64; 2],
    pub size: f64,
    pub target: f64,
    pub lerp_ms: i64,
    pub safe_zone: f64,
    pub damage_per_block: f64,
    pub warning_blocks: i32,
    pub warning_time: i32,
}

pub fn read_level(dir: &Path) -> Option<LevelInfo> {
    let file = std::fs::read(dir.join("level.dat")).ok()?;
    let mut raw = Vec::new();
    GzDecoder::new(&file[..]).read_to_end(&mut raw).ok()?;
    let (_, root) = Nbt::read_named(&mut Reader::new(&raw)).ok()?;
    let d = root.get("Data")?;
    let spawn = match (d.get_i64("SpawnX"), d.get_i64("SpawnY"), d.get_i64("SpawnZ")) {
        (Some(x), Some(y), Some(z)) => Some(BlockPos::new(x as i32, y as i32, z as i32)),
        _ => None,
    };
    let dragon_killed = d.get("DragonFight").and_then(|f| f.get_i64("DragonKilled")).unwrap_or(0) != 0;
    let int = |k: &str| d.get_i64(k).unwrap_or(0);
    let weather = (int("raining") != 0, int("rainTime") as i32, int("thundering") != 0, int("thunderTime") as i32, int("clearWeatherTime") as i32);
    let rules = match d.get("GameRules") {
        Some(Nbt::Compound(fields)) => fields.iter().filter_map(|(k, v)| match v {
            Nbt::String(s) => Some((k.clone(), s.clone())),
            _ => None,
        }).collect(),
        _ => Vec::new(),
    };
    let f = |k: &str| match d.get(k) {
        Some(Nbt::Double(v)) => Some(*v),
        _ => None,
    };
    let border = f("BorderSize").map(|size| LevelBorder {
        center: [f("BorderCenterX").unwrap_or(0.0), f("BorderCenterZ").unwrap_or(0.0)],
        size,
        target: f("BorderSizeLerpTarget").unwrap_or(size),
        lerp_ms: d.get_i64("BorderSizeLerpTime").unwrap_or(0),
        safe_zone: f("BorderSafeZone").unwrap_or(5.0),
        damage_per_block: f("BorderDamagePerBlock").unwrap_or(0.2),
        warning_blocks: f("BorderWarningBlocks").map_or(5, |v| v as i32),
        warning_time: f("BorderWarningTime").map_or(15, |v| v as i32),
    });
    Some(LevelInfo { spawn, time: d.get_i64("Time").unwrap_or(0), day_time: d.get_i64("DayTime").unwrap_or(1000), dragon_killed, weather, rules, border })
}

#[allow(clippy::too_many_arguments)]
pub fn write_level(dir: &Path, name: &str, seed: i64, spawn: BlockPos, time: i64, day_time: i64, game_mode: u8, difficulty: u8, dragon: (bool, bool), weather: (bool, i32, bool, i32, i32), rules: &[(String, String)], border: &LevelBorder) -> Result<()> {
    let dims = ["overworld", "the_nether", "the_end"]
        .iter()
        .map(|d| {
            let generator = Nbt::compound().with("type", Nbt::str("minecraft:noise")).with("settings", Nbt::str(&format!("minecraft:{}", if *d == "the_nether" { "nether" } else if *d == "the_end" { "end" } else { "overworld" })));
            let source = match *d {
                "the_end" => Nbt::compound().with("type", Nbt::str("minecraft:the_end")),
                _ => Nbt::compound().with("type", Nbt::str("minecraft:multi_noise")).with("preset", Nbt::str(&format!("minecraft:{}", if *d == "the_nether" { "nether" } else { "overworld" }))),
            };
            (format!("minecraft:{d}"), Nbt::compound().with("type", Nbt::str(&format!("minecraft:{d}"))).with("generator", generator.with("biome_source", source)))
        })
        .collect();
    let data = Nbt::compound()
        .with("DataVersion", Nbt::Int(DATA_VERSION))
        // `version` is the storage format version (Anvil = 19133);
        // `Version` (capitalized) describes the game version.
        .with("version", Nbt::Int(19133))
        .with("Version", Nbt::compound().with("Id", Nbt::Int(DATA_VERSION)).with("Name", Nbt::str("1.20.4")).with("Series", Nbt::str("main")).with("Snapshot", Nbt::Byte(0)))
        .with("LevelName", Nbt::str(name))
        .with("SpawnX", Nbt::Int(spawn.x))
        .with("SpawnY", Nbt::Int(spawn.y))
        .with("SpawnZ", Nbt::Int(spawn.z))
        .with("SpawnAngle", Nbt::Float(0.0))
        .with("Time", Nbt::Long(time))
        .with("DayTime", Nbt::Long(day_time))
        .with("GameType", Nbt::Int(game_mode as i32))
        .with("Difficulty", Nbt::Byte(difficulty as i8))
        .with("hardcore", Nbt::Byte(0))
        .with("allowCommands", Nbt::Byte(1))
        .with("initialized", Nbt::Byte(1))
        .with("raining", Nbt::Byte(weather.0 as i8))
        .with("rainTime", Nbt::Int(weather.1))
        .with("thundering", Nbt::Byte(weather.2 as i8))
        .with("thunderTime", Nbt::Int(weather.3))
        .with("clearWeatherTime", Nbt::Int(weather.4))
        .with("GameRules", Nbt::Compound(rules.iter().map(|(k, v)| (k.clone(), Nbt::str(v))).collect()))
        .with("BorderCenterX", Nbt::Double(border.center[0]))
        .with("BorderCenterZ", Nbt::Double(border.center[1]))
        .with("BorderSize", Nbt::Double(border.size))
        .with("BorderSizeLerpTarget", Nbt::Double(border.target))
        .with("BorderSizeLerpTime", Nbt::Long(border.lerp_ms))
        .with("BorderSafeZone", Nbt::Double(border.safe_zone))
        .with("BorderDamagePerBlock", Nbt::Double(border.damage_per_block))
        .with("BorderWarningBlocks", Nbt::Double(border.warning_blocks as f64))
        .with("BorderWarningTime", Nbt::Double(border.warning_time as f64))
        .with("LastPlayed", Nbt::Long(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64))
        .with("DragonFight", Nbt::compound().with("DragonKilled", Nbt::Byte(dragon.0 as i8)).with("PreviouslyKilled", Nbt::Byte(dragon.1 as i8)).with("NeedsStateScanning", Nbt::Byte(0)))
        .with("DataPacks", Nbt::compound().with("Enabled", Nbt::List(vec![Nbt::str("vanilla")])).with("Disabled", Nbt::List(vec![])))
        .with("WorldGenSettings", Nbt::compound().with("seed", Nbt::Long(seed)).with("generate_features", Nbt::Byte(1)).with("bonus_chest", Nbt::Byte(0)).with("dimensions", Nbt::Compound(dims)));
    let mut raw = Vec::new();
    Nbt::compound().with("Data", data).write_named("", &mut raw);
    let mut gz = GzEncoder::new(Vec::new(), Compression::default());
    gz.write_all(&raw)?;
    let tmp = dir.join("level.dat_new");
    std::fs::write(&tmp, gz.finish()?)?;
    std::fs::rename(tmp, dir.join("level.dat"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_roundtrip_through_region_file() {
        let reg = data::get();
        let dir = std::env::temp_dir().join(format!("bambietta-anvil-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut c = Chunk::new();
        let stairs = reg.block("oak_stairs").unwrap();
        let s = stairs.with(stairs.with(stairs.default_state, "facing", "east"), "half", "top");
        c.set(3, 70, 5, s);
        c.set(0, -64, 0, reg.state("bedrock"));
        c.set_biome_column(1, 2, reg.biome_id("desert"));
        let chest = BlockPos::new(-31 * 16 + 4, 65, 7 * 16 + 2);
        let mut items = vec![None; 27];
        let mut sword = ItemStack::new(reg.item_id("diamond_sword").unwrap(), 1);
        sword.enchant("sharpness", 5);
        items[4] = Some(sword.clone());
        c.block_entities.insert(chest, BlockEntity::Container(items));
        c.set(4, 65, 2, reg.state("chest"));
        write_chunks(&dir, vec![((-31, 7), chunk_to_nbt(&c, -31, 7)), ((-30, 7), chunk_to_nbt(&Chunk::new(), -30, 7))]).unwrap();
        let back = chunk_from_nbt(&read_chunk(&dir, -31, 7).unwrap().unwrap(), Dimension::Overworld).unwrap();
        assert_eq!(back.get(3, 70, 5), s);
        assert_eq!(back.get(0, -64, 0), reg.state("bedrock"));
        assert_eq!(back.biome(4, 0, 8), reg.biome_id("desert"));
        match back.block_entities.get(&chest) {
            Some(BlockEntity::Container(items)) => assert_eq!(items[4].as_ref().unwrap().enchantment("sharpness"), 5),
            other => panic!("cofre perdido: {other:?}"),
        }
        assert!(read_chunk(&dir, -29, 7).unwrap().is_none());
        // Rewriting the region preserves the other chunk.
        write_chunks(&dir, vec![((-31, 7), chunk_to_nbt(&back, -31, 7))]).unwrap();
        assert!(read_chunk(&dir, -30, 7).unwrap().is_some());
        write_level(&dir, "Prueba", 42, BlockPos::new(1, 70, 2), 100, 6000, 0, 1, (true, true), (true, 5000, false, 9000, 0), &[("keepInventory".into(), "true".into())], &LevelBorder { center: [10.0, -5.0], size: 500.0, target: 500.0, lerp_ms: 0, safe_zone: 5.0, damage_per_block: 0.2, warning_blocks: 5, warning_time: 15 }).unwrap();
        let l = read_level(&dir).unwrap();
        assert_eq!((l.spawn, l.day_time), (Some(BlockPos::new(1, 70, 2)), 6000));
        assert_eq!(l.weather, (true, 5000, false, 9000, 0));
        assert_eq!(l.rules, vec![("keepInventory".to_string(), "true".to_string())]);
        assert_eq!(l.border.map(|b| (b.center, b.size)), Some(([10.0, -5.0], 500.0)));
        std::fs::remove_dir_all(&dir).ok();
    }
}

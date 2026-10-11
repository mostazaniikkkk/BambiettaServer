//! Maps: creating a map from an empty map, drawing it around whoever holds it in hand,
//! zooming out, copying and locking it at the cartography table, and saving them like vanilla
//! (`data/map_<id>.dat` and `idcounts.dat`).

use super::{Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use crate::world::Dimension;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub struct MapData {
    pub scale: i8,
    pub locked: bool,
    pub dim: Dimension,
    pub cx: i32,
    pub cz: i32,
    pub colors: Vec<u8>,
    dirty: bool,
}

#[derive(Clone, Copy)]
pub struct MapInfo {
    pub scale: i8,
    pub locked: bool,
}

#[derive(Default)]
struct Maps {
    dir: Option<PathBuf>,
    next: i32,
    maps: HashMap<i32, MapData>,
}

fn maps() -> &'static Mutex<Maps> {
    static M: OnceLock<Mutex<Maps>> = OnceLock::new();
    M.get_or_init(Default::default)
}

fn data_dir(world: &Path) -> PathBuf {
    world.join("data")
}

/// Loads the world's maps at startup.
pub fn load(world: &Path) {
    let mut m = maps().lock().unwrap();
    m.dir = Some(world.to_path_buf());
    let dir = data_dir(world);
    if let Some(n) = read_gz(&dir.join("idcounts.dat")).and_then(|n| n.get("data").and_then(|d| d.get_i64("map"))) {
        m.next = n as i32 + 1;
    }
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_prefix("map_").and_then(|s| s.strip_suffix(".dat")).and_then(|s| s.parse::<i32>().ok()) else { continue };
        let Some(n) = read_gz(&e.path()) else { continue };
        let Some(d) = n.get("data") else { continue };
        let dim = match d.get_str("dimension").unwrap_or("minecraft:overworld") {
            "minecraft:the_nether" => Dimension::Nether,
            "minecraft:the_end" => Dimension::End,
            _ => Dimension::Overworld,
        };
        let colors = match d.get("colors") {
            Some(Nbt::ByteArray(b)) if b.len() == 16384 => b.iter().map(|&x| x as u8).collect(),
            _ => vec![0; 16384],
        };
        m.maps.insert(
            id,
            MapData {
                scale: d.get_i64("scale").unwrap_or(0) as i8,
                locked: d.get_i64("locked").unwrap_or(0) != 0,
                dim,
                cx: d.get_i64("xCenter").unwrap_or(0) as i32,
                cz: d.get_i64("zCenter").unwrap_or(0) as i32,
                colors,
                dirty: false,
            },
        );
        m.next = m.next.max(id + 1);
    }
}

fn read_gz(path: &Path) -> Option<Nbt> {
    use std::io::Read;
    let raw = std::fs::read(path).ok()?;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(&raw[..]).read_to_end(&mut out).ok()?;
    let mut r = crate::protocol::Reader::new(&out);
    Nbt::read_named(&mut r).ok().map(|x| x.1)
}

fn write_gz(path: &Path, n: &Nbt) -> std::io::Result<()> {
    use std::io::Write;
    let mut raw = Vec::new();
    n.write_named("", &mut raw);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw)?;
    std::fs::write(path, enc.finish()?)
}

/// Saves the modified maps.
pub fn save() {
    let mut m = maps().lock().unwrap();
    let Some(world) = m.dir.clone() else { return };
    let dir = data_dir(&world);
    if m.maps.values().any(|d| d.dirty) {
        let _ = std::fs::create_dir_all(&dir);
    }
    let next = m.next;
    let mut wrote = false;
    for (id, d) in m.maps.iter_mut().filter(|(_, d)| d.dirty) {
        let data = Nbt::compound()
            .with("scale", Nbt::Byte(d.scale))
            .with("dimension", Nbt::str(&format!("minecraft:{}", d.dim.name().trim_start_matches("minecraft:"))))
            .with("xCenter", Nbt::Int(d.cx))
            .with("zCenter", Nbt::Int(d.cz))
            .with("locked", Nbt::Byte(d.locked as i8))
            .with("trackingPosition", Nbt::Byte(1))
            .with("unlimitedTracking", Nbt::Byte(0))
            .with("banners", Nbt::List(vec![]))
            .with("frames", Nbt::List(vec![]))
            .with("colors", Nbt::ByteArray(d.colors.iter().map(|&c| c as i8).collect()));
        let root = Nbt::compound().with("data", data).with("DataVersion", Nbt::Int(3700));
        if let Err(e) = write_gz(&dir.join(format!("map_{id}.dat")), &root) {
            tracing::warn!("no se pudo guardar el mapa {id}: {e}");
        } else {
            d.dirty = false;
            wrote = true;
        }
    }
    if wrote {
        let root = Nbt::compound().with("data", Nbt::compound().with("map", Nbt::Int(next - 1))).with("DataVersion", Nbt::Int(3700));
        let _ = write_gz(&dir.join("idcounts.dat"), &root);
    }
}

pub fn map_id(item: &ItemStack) -> Option<i32> {
    if item.name() != "filled_map" {
        return None;
    }
    item.nbt.as_ref()?.get_i64("map").map(|v| v as i32)
}

pub fn info(id: i32) -> Option<MapInfo> {
    maps().lock().unwrap().maps.get(&id).map(|d| MapInfo { scale: d.scale, locked: d.locked })
}

/// Center aligned to the map grid of that scale (like MapItemSavedData).
fn aligned(v: f64, scale: i8) -> i32 {
    let size = 128 * (1 << scale);
    ((v + 64.0) / size as f64).floor() as i32 * size + size / 2 - 64
}

fn create(dim: Dimension, x: f64, z: f64, scale: i8, colors: Option<Vec<u8>>, locked: bool, center: Option<(i32, i32)>) -> i32 {
    let mut m = maps().lock().unwrap();
    let id = m.next;
    m.next += 1;
    let (cx, cz) = center.unwrap_or((aligned(x, scale), aligned(z, scale)));
    m.maps.insert(id, MapData { scale, locked, dim, cx, cz, colors: colors.unwrap_or_else(|| vec![0; 16384]), dirty: true });
    id
}

fn filled_map(id: i32) -> Option<ItemStack> {
    let mut s = ItemStack::new(data::get().item_id("filled_map")?, 1);
    s.nbt = Some(Box::new(Nbt::compound().with("map", Nbt::Int(id))));
    Some(s)
}

/// Using an empty map: turns it into a new one centered on the player.
pub fn use_empty_map(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (pos, dim) = {
        let st = player.state.lock().unwrap();
        if st.inv.held(hand, st.held_slot).as_ref().is_none_or(|i| i.name() != "map") {
            return false;
        }
        ([st.x, st.z], player.dim())
    };
    let id = create(dim, pos[0], pos[1], 0, None, false, None);
    let Some(map) = filled_map(id) else { return false };
    {
        let mut st = player.state.lock().unwrap();
        let creative = st.game_mode == 1;
        let held = st.held_slot;
        let slot = if hand == 1 { super::inventory::OFFHAND } else { super::inventory::HOTBAR + held };
        let count = st.inv.slots[slot].as_ref().map_or(0, |i| i.count);
        if count <= 1 && !creative {
            st.inv.slots[slot] = Some(map);
        } else {
            if !creative {
                st.inv.consume_held(hand, held, 1, None);
            }
            let (rest, changed) = st.inv.add(map);
            for s in changed {
                st.sync_slot(player, s);
            }
            if let Some(r) = rest {
                drop(st);
                super::entities::throw_from(server, player, r);
                st = player.state.lock().unwrap();
            }
        }
        st.sync_slot(player, slot);
    }
    super::sound::at(server, dim, [pos[0], 64.0, pos[1]], "ui.cartography_table.take_result", super::sound::PLAYER, 1.0, 1.0);
    render_for(server, player);
    true
}

/// Turns the «provisional» maps from the cartography table (zoom out/lock) into new maps.
pub fn finalize_items(_server: &Arc<Server>, player: &Arc<Player>) {
    let mut st = player.state.lock().unwrap();
    let dim = player.dim();
    let mut changed = Vec::new();
    let fix = |s: &mut Option<ItemStack>| -> bool {
        let Some(item) = s else { return false };
        let Some(tag) = item.nbt.as_ref() else { return false };
        let zoom = tag.get("map_scale_direction").is_some();
        let lock = tag.get("map_to_lock").is_some();
        if !zoom && !lock {
            return false;
        }
        let Some(old) = map_id(item) else { return false };
        let src = {
            let m = maps().lock().unwrap();
            m.maps.get(&old).map(|d| (d.scale, d.cx, d.cz, d.colors.clone(), d.dim))
        };
        let Some((scale, cx, cz, colors, mdim)) = src else { return false };
        let _ = dim;
        let new = if zoom {
            let s = (scale + 1).min(4);
            create(mdim, cx as f64, cz as f64, s, None, false, None)
        } else {
            create(mdim, 0.0, 0.0, scale, Some(colors), true, Some((cx, cz)))
        };
        let count = item.count;
        if let Some(mut m) = filled_map(new) {
            m.count = count;
            *s = Some(m);
        }
        true
    };
    if fix(&mut st.inv.cursor) {
        changed.push(None);
    }
    for i in 0..46 {
        if fix(&mut st.inv.slots[i]) {
            changed.push(Some(i));
        }
    }
    if !changed.is_empty() {
        let p = st.inv.window_items();
        player.send(p);
    }
}

// ---------- drawing ----------

/// Base map color (MapColor) approximated from the block name.
fn base_color(name: &str) -> u8 {
    const WOOL: [(&str, u8); 16] = [
        ("white", 8), ("orange", 15), ("magenta", 16), ("light_blue", 17), ("yellow", 18), ("lime", 19), ("pink", 20), ("gray", 21),
        ("light_gray", 22), ("cyan", 23), ("purple", 24), ("blue", 25), ("brown", 26), ("green", 27), ("red", 28), ("black", 29),
    ];
    let n = name;
    if n.ends_with("air") || n == "light" || n == "barrier" || n.contains("glass") {
        return 0;
    }
    if n.ends_with("_wool") || n.ends_with("_carpet") || n.ends_with("_concrete") || n.ends_with("_concrete_powder") {
        if let Some((_, c)) = WOOL.iter().find(|(col, _)| n.starts_with(&format!("{col}_"))) {
            return *c;
        }
    }
    if n.ends_with("_terracotta") && !n.contains("glazed") {
        if let Some(i) = WOOL.iter().position(|(col, _)| n.starts_with(&format!("{col}_"))) {
            return 36 + i as u8;
        }
    }
    match n {
        "grass_block" | "slime_block" => 1,
        "sand" | "sandstone" | "birch_planks" | "birch_log" | "end_stone" | "glowstone" | "bone_block" => 2,
        "cobweb" | "mushroom_stem" => 3,
        "lava" | "fire" | "tnt" | "redstone_block" => 4,
        "ice" | "packed_ice" | "blue_ice" | "frosted_ice" => 5,
        "iron_block" | "anvil" | "brewing_stand" | "lantern" => 6,
        "snow" | "snow_block" | "powder_snow" | "white_bed" => 8,
        "clay" => 9,
        "dirt" | "coarse_dirt" | "farmland" | "dirt_path" | "rooted_dirt" | "granite" | "jungle_planks" => 10,
        "water" | "bubble_column" | "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" => 12,
        "quartz_block" | "diorite" | "sea_lantern" | "calcite" => 14,
        "acacia_planks" | "acacia_log" | "pumpkin" | "red_sand" | "red_sandstone" | "honey_block" => 15,
        "gold_block" | "bell" | "raw_gold_block" => 30,
        "diamond_block" | "prismarine_bricks" | "dark_prismarine" | "beacon" => 31,
        "lapis_block" => 32,
        "emerald_block" => 33,
        "podzol" | "spruce_planks" | "spruce_log" | "oak_log" | "campfire" => 34,
        "netherrack" | "nether_quartz_ore" | "magma_block" | "nether_wart_block" | "crimson_fungus" => 35,
        "crimson_nylium" => 52,
        "crimson_stem" | "crimson_planks" => 53,
        "warped_nylium" => 55,
        "warped_stem" | "warped_planks" => 56,
        "warped_wart_block" => 58,
        "deepslate" | "cobbled_deepslate" | "tuff" => 59,
        "raw_iron_block" => 60,
        "obsidian" | "crying_obsidian" | "coal_block" | "black_wool" | "basalt" | "blackstone" => 29,
        "soul_sand" | "soul_soil" | "dark_oak_planks" | "dark_oak_log" => 26,
        "mycelium" | "amethyst_block" => 24,
        "moss_block" | "cactus" => 27,
        "melon" | "sugar_cane" => 19,
        "bamboo" => 7,
        _ if n.ends_with("_leaves") || n.contains("grass") || n.contains("fern") || n.ends_with("_sapling") || n.contains("vine") || n == "lily_pad" || n.ends_with("bush") => 7,
        _ if n.ends_with("_planks") || n.contains("oak") || n.contains("mangrove") || n.contains("cherry") || n.ends_with("_log") || n.ends_with("_wood") || n.contains("chest") || n == "crafting_table" || n == "bookshelf" => 13,
        _ if n.contains("stone") || n.contains("ore") || n.contains("andesite") || n.contains("gravel") || n == "furnace" || n.contains("brick") || n == "bedrock" || n == "dispenser" => 11,
        _ if n.contains("flower") || n.contains("tulip") || n == "dandelion" || n == "poppy" => 7,
        _ => 11,
    }
}

/// Draws the maps the player holds in hand and sends them the changes.
pub fn render_for(server: &Arc<Server>, player: &Arc<Player>) {
    // Like vanilla: maps carried by the player are sent to them; only those in hand are drawn.
    let (ids, held, x, z, yaw, vd) = {
        let st = player.state.lock().unwrap();
        let held: Vec<i32> = [st.inv.held(0, st.held_slot), st.inv.held(1, st.held_slot)].iter().filter_map(|s| s.as_ref().and_then(map_id)).collect();
        let mut ids: Vec<i32> = st.inv.slots.iter().chain(std::iter::once(&st.inv.cursor)).filter_map(|s| s.as_ref().and_then(map_id)).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.retain(|id| held.contains(id) || !st.maps_seen.contains(id));
        (ids, held, st.x, st.z, st.yaw, st.view_distance)
    };
    let dim = player.dim();
    for id in ids {
        let Some((scale, cx, cz, mdim, locked)) = maps().lock().unwrap().maps.get(&id).map(|d| (d.scale, d.cx, d.cz, d.dim, d.locked)) else { continue };
        let locked = locked || !held.contains(&id) || mdim != dim;
        let step = 1i32 << scale;
        let mut changed = None;
        if !locked {
            // Pixels within the player's view range.
            let radius_px = (64.min((vd * 16).max(32) / step)).max(8);
            let ppx = ((x - cx as f64) / step as f64 + 64.0).floor() as i32;
            let ppz = ((z - cz as f64) / step as f64 + 64.0).floor() as i32;
            let (x0, x1) = ((ppx - radius_px).max(0), (ppx + radius_px).min(127));
            let (z0, z1) = ((ppz - radius_px).max(0), (ppz + radius_px).min(127));
            if x0 <= x1 && z0 <= z1 {
                let mut pixels = Vec::new();
                {
                    let mut world = server.world(dim).lock().unwrap();
                    let reg = data::get();
                    let top = dim.min_y() + dim.height() - 1;
                    let column = |world: &mut crate::world::World, bx: i32, bz: i32| -> (u8, i32, i32) {
                        // From the highest block of the column (with per-chunk cache).
                        let mut y = if dim == Dimension::Nether { top } else { world.top_y(bx, bz) };
                        if dim == Dimension::Nether {
                            // Below the bedrock ceiling: the first gap from the top.
                            while y > dim.min_y() && !reg.is_air(world.get_block(BlockPos::new(bx, y, bz))) {
                                y -= 1;
                            }
                        }
                        while y > dim.min_y() {
                            let s = world.get_block(BlockPos::new(bx, y, bz));
                            let c = base_color(&reg.block_of_state(s).name);
                            if c != 0 {
                                let mut depth = 0;
                                if c == 12 {
                                    while depth < 10 && reg.block_of_state(world.get_block(BlockPos::new(bx, y - depth - 1, bz))).name == "water" {
                                        depth += 1;
                                    }
                                }
                                return (c, y, depth);
                            }
                            y -= 1;
                        }
                        (0, dim.min_y(), 0)
                    };
                    for pz in z0..=z1 {
                        for px in x0..=x1 {
                            let bx = cx + (px - 64) * step;
                            let bz = cz + (pz - 64) * step;
                            // Only what is already loaded: drawing a map does not generate terrain.
                            if !world.is_loaded(bx >> 4, bz >> 4) || !world.is_loaded(bx >> 4, (bz - step) >> 4) {
                                continue;
                            }
                            let (c, h, depth) = column(&mut world, bx, bz);
                            if c == 0 {
                                pixels.push((px, pz, 0u8));
                                continue;
                            }
                            let shade = if c == 12 {
                                let d = depth as f64 * 0.1 + ((px + pz) & 1) as f64 * 0.2;
                                if d < 0.5 { 2 } else if d > 0.9 { 0 } else { 1 }
                            } else {
                                let (_, hn, _) = column(&mut world, bx, bz - step);
                                if h > hn { 2 } else if h < hn { 0 } else { 1 }
                            };
                            pixels.push((px, pz, c * 4 + shade));
                        }
                    }
                }
                let mut m = maps().lock().unwrap();
                if let Some(d) = m.maps.get_mut(&id) {
                    let mut any = false;
                    for (px, pz, c) in pixels {
                        let i = (pz * 128 + px) as usize;
                        if d.colors[i] != c {
                            d.colors[i] = c;
                            any = true;
                        }
                    }
                    if any {
                        d.dirty = true;
                    }
                    changed = Some((x0, z0, x1, z1));
                }
            }
        }
        // Always the full map the first time; afterwards, the redrawn area.
        let first = player.state.lock().unwrap().maps_seen.insert(id);
        let rect = if first { Some((0, 0, 127, 127)) } else { changed };
        let icon_x = (((x - cx as f64) / step as f64) * 2.0).clamp(-128.0, 127.0) as i8;
        let icon_z = (((z - cz as f64) / step as f64) * 2.0).clamp(-128.0, 127.0) as i8;
        let dir = (((yaw + 180.0 + 11.25) / 22.5).floor() as i32).rem_euclid(16) as i8;
        let m = maps().lock().unwrap();
        let Some(d) = m.maps.get(&id) else { continue };
        let mut p = packet(id::play::MAP);
        p.varint(id).i8(d.scale).bool(d.locked);
        p.bool(true).varint(1).varint(0).i8(icon_x).i8(icon_z).i8(dir).bool(false);
        match rect {
            Some((x0, z0, x1, z1)) => {
                let (w, h) = (x1 - x0 + 1, z1 - z0 + 1);
                p.u8(w as u8).u8(h as u8).u8(x0 as u8).u8(z0 as u8).varint(w * h);
                for pz in z0..=z1 {
                    for px in x0..=x1 {
                        p.u8(d.colors[(pz * 128 + px) as usize]);
                    }
                }
            }
            None => {
                p.u8(0);
            }
        }
        player.send(p);
    }
}

/// Every second, redraws the maps held in hand.
pub fn tick(server: &Arc<Server>, age: i64) {
    if age % 20 != 7 {
        return;
    }
    for p in server.online() {
        let holding = {
            let st = p.state.lock().unwrap();
            st.inv.slots.iter().any(|s| s.as_ref().is_some_and(|i| i.name() == "filled_map"))
        };
        if holding {
            render_for(server, &p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_centers_align_like_vanilla() {
        assert_eq!(aligned(0.0, 0), 0);
        assert_eq!(aligned(70.0, 0), 128);
        assert_eq!(aligned(-70.0, 0), -128);
        assert_eq!(aligned(100.0, 1), 64);
        assert_eq!(base_color("grass_block"), 1);
        assert_eq!(base_color("water"), 12);
        assert_eq!(base_color("red_wool"), 28);
        assert_eq!(base_color("white_terracotta"), 36);
    }
}

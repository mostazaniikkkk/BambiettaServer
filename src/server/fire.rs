//! Fire: spreads to flammable blocks, burns blocks, ages and goes out like vanilla's
//! FireBlock (per-block flammability, age 0-15, eternal on netherrack). Lava sets fire
//! to things nearby using the "random ticks" of chunks near players.

use super::Server;
use crate::data;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Default)]
pub struct Fire {
    /// (tick, dimension, x, y, z) ordered by tick.
    queue: BinaryHeap<Reverse<(i64, usize, i32, i32, i32)>>,
}

/// Vanilla (encouragement, flammability): chance that fire spreads next to it and
/// that the block itself burns.
pub fn flammability(name: &str) -> (i32, i32) {
    let n = name;
    let nether_wood = n.starts_with("crimson") || n.starts_with("warped");
    let wood = !nether_wood
        && ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo"].iter().any(|w| n.starts_with(w) || n.starts_with(&format!("stripped_{w}")));
    match () {
        _ if wood && (n.ends_with("_planks") || n.ends_with("_slab") || n.ends_with("_stairs") || n.ends_with("_fence") || n.ends_with("_fence_gate") || n.ends_with("_mosaic")) => (5, 20),
        _ if wood && (n.ends_with("_log") || n.ends_with("_wood") || n.ends_with("_block")) => (5, 5),
        _ if n.ends_with("_leaves") => (30, 60),
        _ if n.ends_with("_wool") => (30, 60),
        _ if n.ends_with("_carpet") => (60, 20),
        _ => match n {
            "bookshelf" | "chiseled_bookshelf" | "lectern" => (30, 20),
            "tnt" => (15, 100),
            "short_grass" | "tall_grass" | "fern" | "large_fern" | "dead_bush" | "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet"
            | "red_tulip" | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "wither_rose"
            | "sunflower" | "lilac" | "rose_bush" | "peony" | "sweet_berry_bush" | "spore_blossom" | "torchflower" | "pink_petals" => (60, 100),
            "vine" | "cave_vines" | "cave_vines_plant" => (15, 100),
            "coal_block" => (5, 5),
            "hay_block" | "scaffolding" | "bamboo" => (60, 20),
            "target" => (15, 20),
            "dried_kelp_block" => (30, 60),
            "composter" | "beehive" => (5, 20),
            "bee_nest" => (30, 20),
            "moss_block" | "moss_carpet" => (5, 100),
            "azalea" | "flowering_azalea" => (30, 60),
            "big_dripleaf" | "small_dripleaf" => (15, 100),
            "hanging_roots" => (30, 60),
            _ => (0, 0),
        },
    }
}

fn is_fire(name: &str) -> bool {
    name == "fire"
}

/// Does it burn forever on this block?
fn infiniburn(dim: Dimension, below: &str) -> bool {
    matches!(below, "netherrack" | "magma_block") || (dim == Dimension::End && below == "bedrock")
}

fn solid_top(state: u16) -> bool {
    let shape = crate::gameplay::get().shape(state);
    shape.iter().any(|s| s[4] >= 1.0 && s[0] <= 0.0 && s[2] <= 0.0 && s[3] >= 1.0 && s[5] >= 1.0)
}

fn name_at(world: &mut World, p: BlockPos) -> &'static str {
    data::get().block_of_state(world.get_block(p)).name.as_str()
}

const SIDES: [(i32, i32, i32, &str); 5] = [(0, 1, 0, "up"), (0, 0, -1, "north"), (0, 0, 1, "south"), (-1, 0, 0, "west"), (1, 0, 0, "east")];

fn off(p: BlockPos, d: (i32, i32, i32)) -> BlockPos {
    BlockPos::new(p.x + d.0, p.y + d.1, p.z + d.2)
}

/// Fire state for this position: on solid or flammable ground, flat; otherwise, attached to
/// the adjacent flammable faces.
pub fn state_for(world: &mut World, p: BlockPos, age: i32) -> u16 {
    let reg = data::get();
    let below = world.get_block(off(p, (0, -1, 0)));
    if reg.block_of_state(below).name.starts_with("soul_s") {
        return reg.state("soul_fire");
    }
    let fire = reg.block("fire").unwrap();
    let mut s = fire.with(fire.default_state, "age", &age.clamp(0, 15).to_string());
    if !solid_top(below) && flammability(&reg.block_of_state(below).name).1 == 0 {
        for (dx, dy, dz, side) in SIDES {
            if flammability(name_at(world, off(p, (dx, dy, dz)))).1 > 0 {
                s = fire.with(s, side, "true");
            }
        }
    }
    s
}

/// Can there be fire here (solid ground or something flammable next to it)?
fn can_survive(world: &mut World, p: BlockPos) -> bool {
    let below = world.get_block(off(p, (0, -1, 0)));
    solid_top(below) || neighbors_flammable(world, p)
}

fn neighbors_flammable(world: &mut World, p: BlockPos) -> bool {
    [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)].iter().any(|d| flammability(name_at(world, off(p, *d))).1 > 0)
}

/// Highest encouragement among the neighbors of an air gap (0 if it isn't air).
fn ignite_odds(world: &mut World, p: BlockPos) -> i32 {
    if !data::get().is_air(world.get_block(p)) {
        return 0;
    }
    [(0, -1, 0), (0, 1, 0), (0, 0, -1), (0, 0, 1), (-1, 0, 0), (1, 0, 0)].iter().map(|d| flammability(name_at(world, off(p, *d))).0).max().unwrap_or(0)
}

/// Schedules the next fire tick at `p` (30-39 ticks, like vanilla).
pub fn schedule(server: &Server, dim: Dimension, p: BlockPos) {
    let due = server.age.load(Ordering::Relaxed) + 30 + (super::entities::random() * 10.0) as i64;
    server.fire.lock().unwrap().queue.push(Reverse((due, dim.index(), p.x, p.y, p.z)));
}

/// Sets fire at `p` and schedules it (flint and steel, lava, spreading...).
pub fn ignite(server: &Server, dim: Dimension, p: BlockPos) -> bool {
    let state = {
        let mut world = server.world(dim).lock().unwrap();
        if !data::get().is_air(world.get_block(p)) || !can_survive(&mut world, p) {
            return false;
        }
        let s = state_for(&mut world, p, 0);
        world.set_block(p, s);
        s
    };
    server.broadcast_dim(dim, &super::blocks::block_change(p, state), None);
    schedule(server, dim, p);
    true
}

fn rand(n: i32) -> i32 {
    if n <= 0 { 0 } else { ((super::entities::random() * n as f32) as i32).min(n - 1) }
}

pub fn tick(server: &Arc<Server>, age: i64) {
    let due: Vec<(Dimension, BlockPos)> = {
        let mut f = server.fire.lock().unwrap();
        if age % 200 == 0 && !f.queue.is_empty() {
            tracing::debug!("fuego: {} ticks programados", f.queue.len());
        }
        let mut out = Vec::new();
        while let Some(Reverse((t, d, x, y, z))) = f.queue.peek().copied() {
            if t > age || out.len() > 512 {
                break;
            }
            f.queue.pop();
            out.push((Dimension::ALL[d], BlockPos::new(x, y, z)));
        }
        out
    };
    let mut changes: Vec<(Dimension, BlockPos, u16)> = Vec::new();
    let mut again = Vec::new();
    let mut tnt = Vec::new();
    let difficulty = server.config.difficulty as i32;
    for (dim, p) in due {
        let mut world = server.world(dim).lock().unwrap();
        let reg = data::get();
        let state = world.get_block(p);
        let block = reg.block_of_state(state);
        if !is_fire(&block.name) {
            continue;
        }
        let set = |world: &mut World, q: BlockPos, s: u16, changes: &mut Vec<(Dimension, BlockPos, u16)>| {
            world.set_block(q, s);
            changes.push((dim, q, s));
        };
        if !can_survive(&mut world, p) {
            set(&mut world, p, 0, &mut changes);
            continue;
        }
        again.push((dim, p));
        let below = name_at(&mut world, off(p, (0, -1, 0)));
        let forever = infiniburn(dim, below);
        let age: i32 = block.get(state, "age").and_then(|a| a.parse().ok()).unwrap_or(0);
        let new_age = (age + rand(3) / 2).min(15);
        if new_age != age {
            let s = block.with(state, "age", &new_age.to_string());
            set(&mut world, p, s, &mut changes);
        }
        if !forever {
            if !neighbors_flammable(&mut world, p) {
                let below_state = world.get_block(off(p, (0, -1, 0)));
                if !solid_top(below_state) || age > 3 {
                    set(&mut world, p, 0, &mut changes);
                }
                continue;
            }
            if age == 15 && rand(4) == 0 && flammability(below).1 == 0 {
                set(&mut world, p, 0, &mut changes);
                continue;
            }
        }
        // Burn the surrounding blocks.
        for (d, chance) in [((1, 0, 0), 300), ((-1, 0, 0), 300), ((0, -1, 0), 250), ((0, 1, 0), 250), ((0, 0, -1), 300), ((0, 0, 1), 300)] {
            let q = off(p, d);
            let n = name_at(&mut world, q);
            let burn = flammability(n).1;
            if rand(chance) < burn {
                if n == "tnt" {
                    set(&mut world, q, 0, &mut changes);
                    tnt.push((dim, q));
                } else if rand(age + 10) < 5 {
                    set(&mut world, q, 0, &mut changes);
                    let s = state_for(&mut world, q, (age + rand(5) / 4).min(15));
                    set(&mut world, q, s, &mut changes);
                    again.push((dim, q));
                } else {
                    set(&mut world, q, 0, &mut changes);
                }
            }
        }
        // Jump to nearby gaps next to something flammable.
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in -1..=4 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let q = off(p, (dx, dy, dz));
                    if !dim.in_bounds(q.y) {
                        continue;
                    }
                    let l = if dy > 1 { 100 + (dy - 1) * 100 } else { 100 };
                    let odds = ignite_odds(&mut world, q);
                    if odds > 0 {
                        let j = (odds + 40 + difficulty * 7) / (age + 30);
                        if j > 0 && rand(l) <= j {
                            let s = state_for(&mut world, q, (age + rand(5) / 4).min(15));
                            set(&mut world, q, s, &mut changes);
                            again.push((dim, q));
                        }
                    }
                }
            }
        }
    }
    for (dim, p, s) in changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
        super::redstone::changed(server, dim, p);
    }
    for (dim, p) in again {
        schedule(server, dim, p);
    }
    for (dim, p) in tnt {
        super::redstone::prime_tnt(server, dim, p, 10 + rand(20));
    }
    lava_random_ticks(server, age);
}

/// Lava random ticks in the chunks around players (LavaFluid.randomTick).
fn lava_random_ticks(server: &Arc<Server>, age: i64) {
    if age % 4 != 0 {
        return;
    }
    let players: Vec<(Dimension, i32, i32, i32)> = server
        .online()
        .iter()
        .map(|p| {
            let st = p.state.lock().unwrap();
            (p.dim(), (st.x.floor() as i32) >> 4, st.y.floor() as i32, (st.z.floor() as i32) >> 4)
        })
        .collect();
    let mut ignite_at = Vec::new();
    for (dim, pcx, py, pcz) in players {
        let mut world = server.world(dim).lock().unwrap();
        let reg = data::get();
        for cx in pcx - 3..=pcx + 3 {
            for cz in pcz - 3..=pcz + 3 {
                // A few random positions per chunk, near the player's height.
                for _ in 0..12 {
                    let p = BlockPos::new(cx * 16 + rand(16), py - 24 + rand(48), cz * 16 + rand(16));
                    if !dim.in_bounds(p.y) {
                        continue;
                    }
                    if reg.block_of_state(world.get_block(p)).name != "lava" {
                        continue;
                    }
                    let i = rand(3);
                    if i > 0 {
                        let mut q = p;
                        for _ in 0..i {
                            q = BlockPos::new(q.x + rand(3) - 1, q.y + 1, q.z + rand(3) - 1);
                            let s = world.get_block(q);
                            if reg.is_air(s) {
                                if neighbors_flammable(&mut world, q) {
                                    ignite_at.push((dim, q));
                                    break;
                                }
                            } else if !crate::gameplay::get().shape(s).is_empty() {
                                break;
                            }
                        }
                    } else {
                        for _ in 0..3 {
                            let q = BlockPos::new(p.x + rand(3) - 1, p.y, p.z + rand(3) - 1);
                            let above = off(q, (0, 1, 0));
                            if reg.is_air(world.get_block(above)) && flammability(name_at(&mut world, q)).1 > 0 {
                                ignite_at.push((dim, above));
                            }
                        }
                    }
                }
            }
        }
    }
    for (dim, p) in ignite_at {
        ignite(server, dim, p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flammability_table() {
        assert_eq!(flammability("oak_planks"), (5, 20));
        assert_eq!(flammability("spruce_log"), (5, 5));
        assert_eq!(flammability("oak_leaves"), (30, 60));
        assert_eq!(flammability("crimson_planks"), (0, 0));
        assert_eq!(flammability("stone"), (0, 0));
        assert_eq!(flammability("tnt"), (15, 100));
        assert!(infiniburn(Dimension::Nether, "netherrack"));
    }
}

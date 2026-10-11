//! A* over blocks for walking mobs: walks on solid ground, steps up one block, drops up to
//! three (any height into water), swims, climbs ladders and vines, and avoids what hurts
//! (lava, fire, cactus...). Fences and walls (1.5 tall) cannot be stepped over, as in vanilla.
//! The heuristic is weighted (x1.5): paths are not always the shortest, but the search reaches
//! much further with the same number of explored cells.

use super::physics::Aabb;
use super::World;
use crate::{data, gameplay};
use crate::protocol::BlockPos;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

/// Cheap hash for block coordinates (the default one dominated the search time).
#[derive(Default)]
struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u64(i as u32 as u64);
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type Map<V> = HashMap<BlockPos, V, BuildHasherDefault<Fx>>;

/// Highest drop a mob takes on purpose (onto ground; into water it can be any height).
const MAX_DROP: i32 = 3;
const MAX_WATER_DROP: i32 = 24;

/// Blocks a mob can climb.
pub fn climbable(name: &str) -> bool {
    matches!(name, "ladder" | "vine" | "scaffolding" | "twisting_vines" | "twisting_vines_plant" | "weeping_vines" | "weeping_vines_plant" | "cave_vines" | "cave_vines_plant")
}

struct Walker<'a> {
    world: &'a mut World,
    width: f64,
    height: f64,
    fits: Map<bool>,
    blocks: Map<u16>,
    /// Closed wooden doors count as passable (zombies break them), at an extra cost.
    doors: bool,
}

/// Wooden doors (the ones a zombie can break; iron and copper ones resist).
pub fn wooden_door(name: &str) -> bool {
    name.ends_with("_door") && !name.contains("iron") && !name.contains("copper")
}

impl Walker<'_> {
    /// The mob's box, standing at the center of the cell, touches no block.
    fn fits(&mut self, p: BlockPos) -> bool {
        if let Some(&f) = self.fits.get(&p) {
            return f;
        }
        let a = Aabb::entity(p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5, self.width, self.height);
        let f = !self.column_hits(p, &a) && !self.hurts(p);
        self.fits.insert(p, f);
        f
    }

    /// Fits and has something to stand on (or water to swim in).
    fn stands(&mut self, p: BlockPos) -> bool {
        if !self.fits(p) {
            return false;
        }
        let r = self.width / 2.0;
        let below = Aabb { min: [p.x as f64 + 0.5 - r, p.y as f64 - 0.1, p.z as f64 + 0.5 - r], max: [p.x as f64 + 0.5 + r, p.y as f64, p.z as f64 + 0.5 + r] };
        self.column_hits(p, &below) || matches!(self.name(p), "water" | "bubble_column") || climbable(self.name(p))
    }

    /// Whether `a` touches a block of column `p` (the box is narrower than a block and centered
    /// on it, so no other column matters). From one block below, for fences and walls.
    fn column_hits(&mut self, p: BlockPos, a: &Aabb) -> bool {
        let g = gameplay::get();
        let top = a.max[1].ceil() as i32;
        (a.min[1].floor() as i32 - 1..top).any(|y| {
            let state = self.block(BlockPos::new(p.x, y, p.z));
            let block = data::get().block_of_state(state);
            // A door is a thin panel on one edge of its cell: measured at the centre the mob
            // would "fit" beside it, but walking through the cell it bumps into it. Closed, it
            // blocks the whole cell (except wooden ones for mobs that break them).
            if block.name.ends_with("_door") && y >= a.min[1].floor() as i32 {
                let closed = block.get(state, "open") != Some("true");
                return closed && !(self.doors && wooden_door(&block.name));
            }
            g.shape(state).iter().any(|s| {
                let b = Aabb { min: [p.x as f64 + s[0], y as f64 + s[1], p.z as f64 + s[2]], max: [p.x as f64 + s[3], y as f64 + s[4], p.z as f64 + s[5]] };
                b.intersects(a)
            })
        })
    }

    fn block(&mut self, p: BlockPos) -> u16 {
        let world = &mut *self.world;
        *self.blocks.entry(p).or_insert_with(|| world.get_block(p))
    }

    fn name(&mut self, p: BlockPos) -> &'static str {
        data::get().block_of_state(self.block(p)).name.as_str()
    }

    fn hurts(&mut self, p: BlockPos) -> bool {
        let below = BlockPos::new(p.x, p.y - 1, p.z);
        matches!(self.name(p), "lava" | "fire" | "soul_fire" | "cactus" | "sweet_berry_bush" | "powder_snow" | "wither_rose")
            || matches!(self.name(below), "lava" | "magma_block" | "cactus" | "campfire" | "soul_campfire")
    }

    /// Cells reachable in one move from `p`, with their cost.
    fn moves(&mut self, p: BlockPos, out: &mut Vec<(BlockPos, u32)>) {
        out.clear();
        const DIRS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];
        let head_free = self.fits(BlockPos::new(p.x, p.y + 1, p.z));
        for (dx, dz) in DIRS {
            let n = BlockPos::new(p.x + dx, p.y, p.z + dz);
            if dx != 0 && dz != 0 {
                // Diagonal only on flat ground and without cutting a corner.
                if self.fits(BlockPos::new(p.x + dx, p.y, p.z)) && self.fits(BlockPos::new(p.x, p.y, p.z + dz)) && self.stands(n) {
                    out.push((n, 14));
                }
                continue;
            }
            if self.stands(n) {
                // Swimming is slower than walking; breaking a door slower still.
                let door = self.doors && (wooden_door(self.name(n)) || wooden_door(self.name(BlockPos::new(n.x, n.y + 1, n.z))));
                out.push((n, if door { 40 } else if self.name(n) == "water" { 16 } else { 10 }));
            } else if !self.fits(n) {
                let up = BlockPos::new(n.x, n.y + 1, n.z);
                if head_free && self.stands(up) {
                    out.push((up, 20));
                }
            } else {
                for k in 1..=MAX_WATER_DROP {
                    let d = BlockPos::new(n.x, n.y - k, n.z);
                    if !self.fits(d) {
                        break;
                    }
                    let water = self.name(d) == "water";
                    if k > MAX_DROP && !water {
                        continue;
                    }
                    if self.stands(d) {
                        out.push((d, 10 + 5 * k.min(MAX_DROP + 2) as u32));
                        break;
                    }
                }
            }
        }
        // Up and down ladders and vines, and up out of the water.
        let up = BlockPos::new(p.x, p.y + 1, p.z);
        let here = self.name(p);
        if (climbable(here) || here == "water") && self.stands(up) {
            out.push((up, 15));
        }
        let down = BlockPos::new(p.x, p.y - 1, p.z);
        if self.fits(down) && (climbable(self.name(down)) || self.name(down) == "water") {
            out.push((down, 12));
        }
    }
}

/// Whether a mob of that size can stand at `p` (safe ground, room above).
pub fn standable(world: &mut World, p: BlockPos, width: f64, height: f64) -> bool {
    Walker { world, width: width.min(0.98), height, fits: Map::default(), blocks: Map::default(), doors: false }.stands(p)
}

fn estimate(a: BlockPos, b: BlockPos) -> u32 {
    let (dx, dz) = ((a.x - b.x).unsigned_abs(), (a.z - b.z).unsigned_abs());
    let (lo, hi) = (dx.min(dz), dx.max(dz));
    14 * lo + 10 * (hi - lo) + 10 * (a.y - b.y).unsigned_abs()
}

/// Path from `start` towards `goal` (cells where the feet go, without `start`). If the goal
/// cannot be reached within `max_nodes` explored cells, the path to the closest cell found;
/// `None` if there is no progress at all. `doors`: through closed wooden doors (zombies).
pub fn find(world: &mut World, start: BlockPos, goal: BlockPos, width: f64, height: f64, max_nodes: usize, doors: bool) -> Option<Vec<BlockPos>> {
    let mut w = Walker { world, width: width.min(0.98), height, fits: Map::default(), blocks: Map::default(), doors };
    let mut open = BinaryHeap::new();
    let mut came: Map<(BlockPos, u32)> = Map::default();
    came.insert(start, (start, 0));
    open.push(Reverse((estimate(start, goal) * 3 / 2, 0u32, start.x, start.y, start.z)));
    let mut best = (estimate(start, goal), start);
    let mut moves = Vec::new();
    let mut explored = 0;
    while let Some(Reverse((_, g, x, y, z))) = open.pop() {
        let p = BlockPos::new(x, y, z);
        if came.get(&p).is_some_and(|c| c.1 < g) {
            continue;
        }
        let h = estimate(p, goal);
        if h < best.0 {
            best = (h, p);
        }
        // Next to the goal is enough (the target is a player, not a cell).
        if (p.x - goal.x).abs() <= 1 && (p.z - goal.z).abs() <= 1 && (p.y - goal.y).abs() <= 1 {
            best = (0, p);
            break;
        }
        explored += 1;
        if explored >= max_nodes {
            break;
        }
        w.moves(p, &mut moves);
        for &(n, cost) in &moves {
            let ng = g + cost;
            if came.get(&n).is_none_or(|c| ng < c.1) {
                came.insert(n, (p, ng));
                open.push(Reverse((ng + estimate(n, goal) * 3 / 2, ng, n.x, n.y, n.z)));
            }
        }
    }
    if best.1 == start {
        return None;
    }
    let mut path = vec![best.1];
    let mut p = best.1;
    while let Some(&(prev, _)) = came.get(&p) {
        if prev == start || prev == p {
            break;
        }
        path.push(prev);
        p = prev;
    }
    path.reverse();
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Flat test world: the ground surface is at y = -60.
    const Y: i32 = -60;

    fn set(w: &mut World, x: i32, y: i32, z: i32, name: &str) {
        w.set_block(BlockPos::new(x, y, z), data::get().state(name));
    }

    #[test]
    fn walks_straight_on_flat_ground() {
        let mut w = World::new_flat_for_tests();
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(8, Y, 0), 0.6, 1.95, 500, false).unwrap();
        assert_eq!(p.last().unwrap().x, 7);
        assert!(p.iter().all(|c| c.y == Y));
    }

    #[test]
    fn goes_around_a_wall_it_cannot_jump() {
        let mut w = World::new_flat_for_tests();
        // A 2-tall wall from z = -5 to 5 at x = 3, with a gap only beyond its ends.
        for z in -5..=5 {
            for y in Y..Y + 2 {
                set(&mut w, 3, y, z, "stone");
            }
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(6, Y, 0), 0.6, 1.95, 1000, false).unwrap();
        let end = *p.last().unwrap();
        assert!((end.x - 6).abs() <= 1 && end.z.abs() <= 1, "llega al otro lado: {end:?}");
        assert!(p.iter().any(|c| c.z.abs() > 5), "rodea la muralla");
        assert!(p.iter().all(|c| c.y == Y), "no la salta");
    }

    #[test]
    fn steps_up_one_block_and_drops_down() {
        let mut w = World::new_flat_for_tests();
        for z in -20..=20 {
            set(&mut w, 3, Y, z, "stone");
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(6, Y, 0), 0.6, 1.95, 500, false).unwrap();
        assert!(p.iter().any(|c| c.x == 3 && c.y == Y + 1), "sube el escalón: {p:?}");
        assert!(p.iter().all(|c| c.z.abs() <= 1), "no da la vuelta");
    }

    #[test]
    fn does_not_cross_a_fence() {
        let mut w = World::new_flat_for_tests();
        for z in -3..=3 {
            set(&mut w, 3, Y, z, "oak_fence");
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(6, Y, 0), 0.6, 1.95, 1000, false).unwrap();
        assert!(p.iter().any(|c| c.z.abs() > 3), "rodea la valla: {p:?}");
    }

    #[test]
    fn closed_room_gives_the_closest_cell() {
        let mut w = World::new_flat_for_tests();
        for i in -3..=3 {
            for y in Y..Y + 3 {
                for (x, z) in [(i, -3), (i, 3), (-3, i), (3, i)] {
                    set(&mut w, x, y, z, "stone");
                }
            }
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(10, Y, 0), 0.6, 1.95, 1000, false).unwrap();
        assert_eq!(p.last().unwrap().x, 2, "se acerca a la pared más cercana al objetivo");
    }

    #[test]
    fn zombies_go_through_doors() {
        let mut w = World::new_flat_for_tests();
        // A closed room with an oak door; the goal is inside.
        for i in -3..=3 {
            for y in Y..Y + 3 {
                for (x, z) in [(i, -3), (i, 3), (-3, i), (3, i)] {
                    set(&mut w, x, y, z, "stone");
                }
            }
        }
        let door = data::get().block("oak_door").unwrap();
        let closed = door.with(door.default_state, "facing", "east");
        w.set_block(BlockPos::new(3, Y, 0), closed);
        w.set_block(BlockPos::new(3, Y + 1, 0), door.with(closed, "half", "upper"));
        let without = find(&mut w, BlockPos::new(8, Y, 0), BlockPos::new(0, Y, 0), 0.6, 1.95, 1000, false).unwrap();
        assert!(without.last().unwrap().x > 3, "sin romper puertas no entra: {without:?}");
        let with = find(&mut w, BlockPos::new(8, Y, 0), BlockPos::new(0, Y, 0), 0.6, 1.95, 1000, true).unwrap();
        assert!(with.iter().any(|c| c.x == 3 && c.z == 0), "pasa por la puerta: {with:?}");
        assert!(with.last().unwrap().x <= 1, "llega adentro: {with:?}");
    }

    #[test]
    fn around_the_end_of_a_wall_from_its_corner() {
        let mut w = World::new_flat_for_tests();
        for z in -7..=7 {
            for y in Y..Y + 2 {
                set(&mut w, 2, y, z, "cobblestone");
            }
        }
        for x in -12..=12 {
            for z in -12..=12 {
                set(&mut w, x, Y + 4, z, "stone");
            }
        }
        for start in [BlockPos::new(1, Y, -7), BlockPos::new(1, Y, -6), BlockPos::new(1, Y, 0)] {
            let p = find(&mut w, start, BlockPos::new(5, Y, 0), 0.6, 1.95, 800, true).unwrap();
            let end = *p.last().unwrap();
            assert!((end.x - 5).abs() <= 1 && end.z.abs() <= 1, "desde {start:?} llega: {p:?}");
        }
    }

    #[test]
    fn climbs_a_ladder() {
        let mut w = World::new_flat_for_tests();
        // A 4-block wall with a ladder on its west face; the goal is on top.
        for z in -20..=20 {
            for y in Y..Y + 4 {
                set(&mut w, 3, y, z, "stone");
            }
        }
        for y in Y..Y + 4 {
            set(&mut w, 2, y, 0, "ladder");
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(3, Y + 4, 0), 0.6, 1.95, 1000, false).unwrap();
        assert!(p.iter().any(|c| c.x == 2 && c.y == Y + 2), "sube por la escalera: {p:?}");
        assert!(p.last().unwrap().y >= Y + 3, "llega arriba: {p:?}");
    }

    #[test]
    fn swims_across_water() {
        let mut w = World::new_flat_for_tests();
        // A 5-wide, 2-deep channel between start and goal.
        for x in 2..=6 {
            for z in -30..=30 {
                set(&mut w, x, Y - 1, z, "water");
                set(&mut w, x, Y - 2, z, "water");
            }
        }
        let p = find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(9, Y, 0), 0.6, 1.95, 1000, false).unwrap();
        assert!((p.last().unwrap().x - 9).abs() <= 1, "cruza el canal: {p:?}");
        assert!(p.iter().all(|c| c.z.abs() <= 3), "no lo rodea (30 bloques): {p:?}");
    }

    #[test]
    #[ignore]
    fn bench_unreachable() {
        let mut w = World::new_flat_for_tests();
        for z in -30..=30 {
            for y in Y..Y + 3 {
                set(&mut w, 3, y, z, "stone");
            }
        }
        let t = std::time::Instant::now();
        for _ in 0..100 {
            find(&mut w, BlockPos::new(0, Y, 0), BlockPos::new(10, Y, 0), 0.6, 1.95, 600, false);
        }
        println!("{:?} por búsqueda de 600 nodos", t.elapsed() / 100);
    }
}

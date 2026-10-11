//! Farming and plant growth, like vanilla:
//! - the hoe tills grass and dirt into farmland; farmland is moist near water (or in the
//!   rain), dries up otherwise, and turns back to dirt when trampled;
//! - random ticks (3 random blocks per section and tick, around the players) grow crops,
//!   pumpkin and melon stems (and their fruit), saplings into trees, sugar cane, cactus,
//!   sweet berry bushes and nether wart;
//! - bone meal speeds up crops and saplings and sprouts grass and flowers;
//! - plants can only be placed where they can live (crops on farmland, cane next to water...);
//! - grass (and mycelium) spreads to nearby dirt in the light and dies under opaque blocks;
//!   ice and snow melt next to strong light (torches, lava...); bamboo grows.

use super::{blocks, entities, health, sound, Player, Server};
use crate::data::{self, Block};
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::sync::{Arc, OnceLock};

/// Chunks around each player that get random ticks.
const RADIUS: i32 = 6;

fn rand(n: u32) -> u32 {
    (entities::random() * n as f32) as u32 % n.max(1)
}

/// Full-range random integers for picking blocks (an `f32` scaled up to `u32` only gives
/// multiples of 256, which would always pick the same columns).
fn rand_u32() -> u32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 32) as u32
}

const CROPS: &[&str] = &["wheat", "carrots", "potatoes", "beetroots", "torchflower_crop"];
const SAPLINGS: &[&str] = &["oak_sapling", "spruce_sapling", "birch_sapling", "jungle_sapling", "acacia_sapling", "dark_oak_sapling", "cherry_sapling"];
const SOIL: &[&str] = &["grass_block", "dirt", "coarse_dirt", "podzol", "rooted_dirt", "farmland", "moss_block", "mud", "muddy_mangrove_roots", "mycelium"];

/// What a broken plant drops (no tool).
fn gameplay_drops(state: u16) -> Vec<ItemStack> {
    let block = data::get().block_of_state(state);
    crate::gameplay::get().drops(block, state, None, &mut entities::random).into_iter().map(|(item, n)| ItemStack::new(item, n)).collect()
}

/// States that do something on a random tick.
fn tickable() -> &'static Vec<bool> {
    static T: OnceLock<Vec<bool>> = OnceLock::new();
    T.get_or_init(|| {
        let reg = data::get();
        (0..reg.state_count() as u16)
            .map(|s| {
                let n = reg.block_of_state(s).name.as_str();
                CROPS.contains(&n)
                    || SAPLINGS.contains(&n)
                    || matches!(n, "farmland" | "pumpkin_stem" | "melon_stem" | "sweet_berry_bush" | "nether_wart" | "sugar_cane" | "cactus" | "grass_block" | "mycelium" | "ice" | "snow" | "bamboo" | "bamboo_sapling")
            })
            .collect()
    })
}

fn name(world: &mut World, p: BlockPos) -> &'static str {
    data::get().block_of_state(world.get_block(p)).name.as_str()
}

fn age_of(block: &Block, state: u16) -> i32 {
    block.get(state, "age").and_then(|a| a.parse().ok()).unwrap_or(0)
}

fn max_age(block: &Block) -> i32 {
    block.props.iter().find(|(n, _)| n == "age").map_or(0, |(_, v)| v.len() as i32 - 1)
}

fn with_age(block: &Block, state: u16, age: i32) -> u16 {
    block.with(state, "age", &age.to_string())
}

/// Enough light to grow (vanilla: brightness 9): open sky, or a light source close enough.
fn bright(world: &mut World, p: BlockPos) -> bool {
    if world.top_y(p.x, p.z) <= p.y {
        return true;
    }
    let reg = data::get();
    for dx in -6..=6i32 {
        for dy in -6..=6i32 {
            for dz in -6..=6i32 {
                let d = dx.abs() + dy.abs() + dz.abs();
                if d > 6 {
                    continue;
                }
                let emit = reg.light(world.get_block(BlockPos::new(p.x + dx, p.y + dy, p.z + dz))).1 as i32;
                if emit - d >= 9 {
                    return true;
                }
            }
        }
    }
    false
}

fn moist(world: &mut World, farmland: BlockPos) -> bool {
    let s = world.get_block(farmland);
    data::get().block_of_state(s).get(s, "moisture").is_some_and(|m| m != "0")
}

/// Water within 4 blocks (same level or one above), like vanilla.
fn water_near(world: &mut World, p: BlockPos) -> bool {
    for dx in -4..=4 {
        for dz in -4..=4 {
            for dy in 0..=1 {
                if matches!(name(world, BlockPos::new(p.x + dx, p.y + dy, p.z + dz)), "water") {
                    return true;
                }
            }
        }
    }
    false
}

/// Every tick: random ticks around the players.
pub fn tick(server: &Arc<Server>) {
    let views: Vec<(Dimension, i32, i32)> = server
        .online()
        .iter()
        .map(|p| {
            let st = p.state.lock().unwrap();
            (p.dim(), (st.x.floor() as i32) >> 4, (st.z.floor() as i32) >> 4)
        })
        .collect();
    for dim in Dimension::ALL {
        let mut chunks: Vec<(i32, i32)> = views
            .iter()
            .filter(|v| v.0 == dim)
            .flat_map(|&(_, cx, cz)| (-RADIUS..=RADIUS).flat_map(move |dx| (-RADIUS..=RADIUS).map(move |dz| (cx + dx, cz + dz))))
            .collect();
        if chunks.is_empty() {
            continue;
        }
        chunks.sort_unstable();
        chunks.dedup();
        let wanted = tickable();
        let per_section = server.rule_int("randomTickSpeed").clamp(0, 4096) as usize;
        let raining = dim == Dimension::Overworld && server.weather.lock().unwrap().is_raining();
        let mut changes: Vec<(BlockPos, u16)> = Vec::new();
        let mut popped: Vec<(BlockPos, u16)> = Vec::new();
        {
            let mut world = server.world(dim).lock().unwrap();
            let mut picked = Vec::new();
            for (cx, cz) in chunks {
                world.random_blocks(cx, cz, per_section, &|s| wanted.get(s as usize).copied().unwrap_or(false), &mut rand_u32, &mut picked);
            }
            for (p, state) in picked {
                random_tick(&mut world, p, state, raining, &mut changes, &mut popped);
            }
        }
        apply(server, dim, changes, popped);
    }
}

/// Broadcasts the changes and drops the loot of plants that broke.
fn apply(server: &Server, dim: Dimension, changes: Vec<(BlockPos, u16)>, popped: Vec<(BlockPos, u16)>) {
    for (p, s) in changes {
        server.broadcast_dim(dim, &blocks::block_change(p, s), None);
    }
    for (p, s) in popped {
        server.broadcast_dim(dim, &blocks::block_change(p, 0), None);
        for stack in gameplay_drops(s) {
            entities::drop_at_block(server, dim, p.x, p.y, p.z, stack);
        }
    }
}

fn set(world: &mut World, p: BlockPos, s: u16, changes: &mut Vec<(BlockPos, u16)>) {
    if world.set_block(p, s) != s {
        changes.push((p, s));
    }
}

fn random_tick(world: &mut World, p: BlockPos, state: u16, raining: bool, changes: &mut Vec<(BlockPos, u16)>, popped: &mut Vec<(BlockPos, u16)>) {
    let reg = data::get();
    let block = reg.block_of_state(state);
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    let above = BlockPos::new(p.x, p.y + 1, p.z);
    let n = block.name.as_str();
    match n {
        "farmland" => {
            let m: i32 = block.get(state, "moisture").and_then(|v| v.parse().ok()).unwrap_or(0);
            let wet = water_near(world, p) || (raining && world.top_y(p.x, p.z) <= p.y);
            if wet {
                if m < 7 {
                    set(world, p, block.with(state, "moisture", "7"), changes);
                }
            } else if m > 0 {
                set(world, p, block.with(state, "moisture", &(m - 1).to_string()), changes);
            } else if !is_plant_on_farmland(name(world, above)) {
                // Dry and with nothing planted: back to dirt.
                set(world, p, reg.state("dirt"), changes);
            }
        }
        _ if CROPS.contains(&n) || n == "pumpkin_stem" || n == "melon_stem" => {
            if name(world, below) != "farmland" {
                world.set_block(p, 0);
                popped.push((p, state));
                return;
            }
            if !bright(world, p) {
                return;
            }
            // Vanilla: 1 in (25 / speed + 1), speed higher on moist farmland.
            let speed = if moist(world, below) { 4 } else { 2 };
            if rand(25 / speed + 1) != 0 {
                return;
            }
            let age = age_of(block, state);
            if age < max_age(block) {
                set(world, p, with_age(block, state, age + 1), changes);
            } else if n.ends_with("_stem") {
                grow_fruit(world, p, n, changes);
            }
        }
        "sweet_berry_bush" => {
            let age = age_of(block, state);
            if age < 3 && rand(5) == 0 && bright(world, p) {
                set(world, p, with_age(block, state, age + 1), changes);
            }
        }
        "nether_wart" => {
            let age = age_of(block, state);
            if age < 3 && rand(10) == 0 {
                set(world, p, with_age(block, state, age + 1), changes);
            }
        }
        "grass_block" | "mycelium" => {
            let up = world.get_block(above);
            // Smothered by an opaque block (or water) on top: back to dirt.
            if reg.light(up).0 >= 15 || reg.block_of_state(up).name == "water" {
                set(world, p, reg.state("dirt"), changes);
                return;
            }
            if !bright(world, above) {
                return;
            }
            // Spreads to dirt around (one block up, three down) that has light and air on top.
            for _ in 0..4 {
                let q = BlockPos::new(p.x + rand(3) as i32 - 1, p.y + rand(5) as i32 - 3, p.z + rand(3) as i32 - 1);
                let q_up = BlockPos::new(q.x, q.y + 1, q.z);
                let top = world.get_block(q_up);
                if name(world, q) == "dirt" && reg.light(top).0 < 15 && reg.block_of_state(top).name != "water" {
                    let s = if n == "mycelium" { reg.state("mycelium") } else { reg.state("grass_block") };
                    set(world, q, s, changes);
                }
            }
        }
        "ice" | "snow" => {
            // Strong block light (more than 11) melts them: ice into water, snow away.
            if block_light(world, p) > 11 {
                let below = name(world, below);
                let s = if n == "ice" && below != "air" { reg.state("water") } else { 0 };
                set(world, p, s, changes);
            }
        }
        "bamboo_sapling" => {
            if bright(world, p) && rand(3) == 0 {
                set(world, p, reg.state("bamboo"), changes);
            }
        }
        "bamboo" => {
            if !reg.is_air(world.get_block(above)) || !bright(world, above) || rand(3) != 0 {
                return;
            }
            let mut height = 1;
            while height < 16 && name(world, BlockPos::new(p.x, p.y - height, p.z)) == "bamboo" {
                height += 1;
            }
            // Each stalk stops somewhere between 12 and 16.
            let max = 12 + (crate::world::structures::hash(0xBA, p.x as i64, 0, p.z as i64) % 5) as i32;
            if height < max {
                let leaves = if height >= 2 { "large" } else { "small" };
                if let Some(b) = reg.block("bamboo") {
                    set(world, above, b.with(b.default_state, "leaves", leaves), changes);
                }
            }
        }
        "sugar_cane" | "cactus" => {
            if !reg.is_air(world.get_block(above)) {
                return;
            }
            let mut height = 1;
            while height < 3 && name(world, BlockPos::new(p.x, p.y - height, p.z)) == n {
                height += 1;
            }
            if height >= 3 {
                return;
            }
            let age = age_of(block, state);
            if age >= 15 {
                set(world, above, block.default_state, changes);
                set(world, p, with_age(block, state, 0), changes);
            } else {
                set(world, p, with_age(block, state, age + 1), changes);
            }
        }
        _ if SAPLINGS.contains(&n) => {
            if bright(world, above) && rand(7) == 0 {
                advance_sapling(world, p, block, state, changes);
            }
        }
        _ => {}
    }
}

fn is_plant_on_farmland(n: &str) -> bool {
    CROPS.contains(&n) || matches!(n, "pumpkin_stem" | "melon_stem" | "attached_pumpkin_stem" | "attached_melon_stem")
}

/// A ripe stem puts a pumpkin or melon next to it (on dirt, grass or farmland).
fn grow_fruit(world: &mut World, p: BlockPos, stem: &str, changes: &mut Vec<(BlockPos, u16)>) {
    let reg = data::get();
    let (fruit, attached) = if stem == "pumpkin_stem" { ("pumpkin", "attached_pumpkin_stem") } else { ("melon", "attached_melon_stem") };
    let dirs = [("north", 0, -1), ("south", 0, 1), ("west", -1, 0), ("east", 1, 0)];
    let (facing, dx, dz) = dirs[rand(4) as usize];
    let spot = BlockPos::new(p.x + dx, p.y, p.z + dz);
    let ground = name(world, BlockPos::new(spot.x, spot.y - 1, spot.z));
    if reg.is_air(world.get_block(spot)) && (SOIL.contains(&ground) || ground == "farmland") {
        set(world, spot, reg.state(fruit), changes);
        if let Some(b) = reg.block(attached) {
            set(world, p, b.with(b.default_state, "facing", facing), changes);
        }
    }
}

/// Sapling: stage 0 -> 1, and from stage 1 it becomes a tree (if there is room).
fn advance_sapling(world: &mut World, p: BlockPos, block: &Block, state: u16, changes: &mut Vec<(BlockPos, u16)>) {
    if block.get(state, "stage") == Some("0") {
        set(world, p, block.with(state, "stage", "1"), changes);
        return;
    }
    let wood = block.name.trim_end_matches("_sapling");
    grow_tree(world, p, wood, changes);
}

/// Grows a tree of `wood` at `p` (the sapling's spot). Returns whether there was room.
pub fn grow_tree(world: &mut World, p: BlockPos, wood: &str, changes: &mut Vec<(BlockPos, u16)>) -> bool {
    let reg = data::get();
    let (Some(log), Some(leaves)) = (reg.block(&format!("{wood}_log")), reg.block(&format!("{wood}_leaves"))) else { return false };
    let log = log.default_state;
    let leaves = leaves.with(leaves.default_state, "distance", "1");
    let spruce = wood == "spruce";
    let height = match wood {
        "birch" => 5 + rand(3) as i32,
        "spruce" => 6 + rand(4) as i32,
        "jungle" => 5 + rand(5) as i32,
        _ => 4 + rand(3) as i32,
    };
    let free = |world: &mut World, q: BlockPos| {
        let n = name(world, q);
        reg.is_air(world.get_block(q)) || n.ends_with("_leaves") || n.ends_with("_sapling") || matches!(n, "short_grass" | "fern" | "snow")
    };
    // Room for the trunk.
    for dy in 0..height {
        if !free(world, BlockPos::new(p.x, p.y + dy, p.z)) {
            return false;
        }
    }
    if !SOIL.contains(&name(world, BlockPos::new(p.x, p.y - 1, p.z))) {
        return false;
    }
    // Leaves first (the trunk overwrites them).
    let top = p.y + height;
    let put_leaf = |world: &mut World, q: BlockPos, changes: &mut Vec<(BlockPos, u16)>| {
        if reg.is_air(world.get_block(q)) || matches!(name(world, q), "short_grass" | "fern" | "snow") {
            set(world, q, leaves, changes);
        }
    };
    if spruce {
        // Cone: wide at the bottom, a point at the top.
        let mut r: i32 = 0;
        for y in (p.y + 2..=top).rev() {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() + dz.abs() <= r + 1 && !(dx.abs() == r && dz.abs() == r && r > 0) {
                        put_leaf(world, BlockPos::new(p.x + dx, y, p.z + dz), changes);
                    }
                }
            }
            r = if r >= 2 { 1 } else { r + 1 };
        }
    } else {
        // Oak-like blob: two wide layers and two narrow ones on top, random corners.
        for y in top - 3..=top {
            let r: i32 = if y >= top - 1 { 1 } else { 2 };
            for dx in -r..=r {
                for dz in -r..=r {
                    let corner = dx.abs() == r && dz.abs() == r;
                    if corner && (y == top || rand(2) == 0) {
                        continue;
                    }
                    put_leaf(world, BlockPos::new(p.x + dx, y, p.z + dz), changes);
                }
            }
        }
    }
    for dy in 0..height {
        set(world, BlockPos::new(p.x, p.y + dy, p.z), log, changes);
    }
    set(world, BlockPos::new(p.x, p.y - 1, p.z), reg.state("dirt"), changes);
    true
}

// ---------- player actions ----------

/// The hoe on grass or dirt (with air above): farmland. Returns whether it was used.
pub fn till(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, face: i32, stack: &ItemStack) -> bool {
    if !stack.name().ends_with("_hoe") || face == 0 {
        return false;
    }
    let dim = player.dim();
    let reg = data::get();
    let result = {
        let mut world = server.world(dim).lock().unwrap();
        let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
        if !reg.is_air(world.get_block(above)) {
            return false;
        }
        let new = match name(&mut world, pos) {
            "grass_block" | "dirt" | "dirt_path" => reg.state("farmland"),
            "coarse_dirt" | "rooted_dirt" => reg.state("dirt"),
            _ => return false,
        };
        world.set_block(pos, new);
        new
    };
    server.broadcast_dim(dim, &blocks::block_change(pos, result), None);
    sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 1.0, pos.z as f64 + 0.5], "item.hoe.till", sound::BLOCK, 1.0, 1.0);
    if player.state.lock().unwrap().survival_like() {
        health::wear_held(server, player, 1);
    }
    true
}

/// Bone meal on a plant. Returns whether it was used (and must be consumed).
pub fn bone_meal(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let mut changes = Vec::new();
    let used = {
        let mut world = server.world(dim).lock().unwrap();
        let state = world.get_block(pos);
        let block = reg.block_of_state(state);
        let n = block.name.as_str();
        if CROPS.contains(&n) || n.ends_with("_stem") && !n.starts_with("attached") || n == "sweet_berry_bush" {
            let age = age_of(block, state);
            let max = max_age(block);
            if age >= max {
                if n.ends_with("_stem") {
                    grow_fruit(&mut world, pos, n, &mut changes);
                    true
                } else {
                    false
                }
            } else {
                let step = if n == "sweet_berry_bush" || n == "beetroots" { 1 } else { 2 + rand(4) as i32 };
                set(&mut world, pos, with_age(block, state, (age + step).min(max)), &mut changes);
                true
            }
        } else if SAPLINGS.contains(&n) {
            // 45% of the time it advances a stage.
            if rand(100) < 45 {
                advance_sapling(&mut world, pos, block, state, &mut changes);
            }
            true
        } else if n == "bamboo" || n == "bamboo_sapling" {
            // One or two more segments on top of the stalk.
            let mut top = pos;
            while name(&mut world, BlockPos::new(top.x, top.y + 1, top.z)) == "bamboo" {
                top = BlockPos::new(top.x, top.y + 1, top.z);
            }
            for k in 1..=1 + rand(2) as i32 {
                let q = BlockPos::new(top.x, top.y + k, top.z);
                if reg.is_air(world.get_block(q)) {
                    set(&mut world, q, reg.state("bamboo"), &mut changes);
                }
            }
            true
        } else if n == "grass_block" {
            // Grass and some flowers sprout around.
            let (grass, flowers) = (reg.state("short_grass"), [reg.state("dandelion"), reg.state("poppy")]);
            for _ in 0..24 {
                let q = BlockPos::new(pos.x + rand(7) as i32 - 3, pos.y, pos.z + rand(7) as i32 - 3);
                let up = BlockPos::new(q.x, q.y + 1, q.z);
                if name(&mut world, q) == "grass_block" && reg.is_air(world.get_block(up)) {
                    let s = if rand(8) == 0 { flowers[rand(2) as usize] } else { grass };
                    set(&mut world, up, s, &mut changes);
                }
            }
            true
        } else {
            false
        }
    };
    if !used {
        return false;
    }
    apply(server, dim, changes, Vec::new());
    // Green particles (level event 1505).
    let mut ev = packet(id::play::WORLD_EVENT);
    ev.i32(1505).position(pos).i32(15).bool(false);
    server.broadcast_dim(dim, &ev, None);
    true
}

/// Approximate block light at `p`: the strongest light source minus its distance.
fn block_light(world: &mut World, p: BlockPos) -> i32 {
    let reg = data::get();
    let mut best = 0;
    for dx in -3..=3i32 {
        for dy in -3..=3i32 {
            for dz in -3..=3i32 {
                let d = dx.abs() + dy.abs() + dz.abs();
                let emit = reg.light(world.get_block(BlockPos::new(p.x + dx, p.y + dy, p.z + dz))).1 as i32;
                best = best.max(emit - d);
            }
        }
    }
    best
}

/// Can this plant live at `p`?
pub fn can_place(world: &mut World, block: &str, p: BlockPos) -> bool {
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    let ground = name(world, below);
    match block {
        _ if CROPS.contains(&block) || matches!(block, "pumpkin_stem" | "melon_stem" | "pitcher_crop") => ground == "farmland",
        _ if SAPLINGS.contains(&block) || block == "sweet_berry_bush" => SOIL.contains(&ground),
        "nether_wart" => ground == "soul_sand",
        "bamboo" | "bamboo_sapling" => ground == "bamboo" || SOIL.contains(&ground) || matches!(ground, "sand" | "red_sand" | "gravel" | "suspicious_sand"),
        "sugar_cane" => {
            ground == "sugar_cane"
                || (matches!(ground, "grass_block" | "dirt" | "coarse_dirt" | "podzol" | "rooted_dirt" | "sand" | "red_sand" | "mud" | "moss_block")
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| matches!(name(world, BlockPos::new(below.x + dx, below.y, below.z + dz)), "water" | "frosted_ice")))
        }
        "cactus" => {
            matches!(ground, "cactus" | "sand" | "red_sand")
                && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().all(|(dx, dz)| {
                    let s = world.get_block(BlockPos::new(p.x + dx, p.y, p.z + dz));
                    crate::gameplay::get().shape(s).is_empty()
                })
        }
        _ => true,
    }
}

/// A player lands on farmland: it may turn back into dirt (and what was planted pops off).
pub fn trample(server: &Arc<Server>, player: &Arc<Player>, fall: f64, feet: [f64; 3]) {
    if entities::random() as f64 >= fall - 0.5 {
        return;
    }
    let dim = player.dim();
    let p = BlockPos::new(feet[0].floor() as i32, (feet[1] - 0.07).floor() as i32, feet[2].floor() as i32);
    let reg = data::get();
    let mut changes = Vec::new();
    let mut popped = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        if name(&mut world, p) != "farmland" {
            return;
        }
        let above = BlockPos::new(p.x, p.y + 1, p.z);
        let crop = world.get_block(above);
        if is_plant_on_farmland(reg.block_of_state(crop).name.as_str()) {
            world.set_block(above, 0);
            popped.push((above, crop));
        }
        set(&mut world, p, reg.state("dirt"), &mut changes);
    }
    apply(server, dim, changes, popped);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Flat test world: the surface (stone) is at y = -61; plants go at y = -60.
    const Y: i32 = -60;

    fn put(w: &mut World, x: i32, y: i32, z: i32, name: &str) {
        w.set_block(BlockPos::new(x, y, z), data::get().state(name));
    }

    #[test]
    fn crops_need_farmland() {
        let mut w = World::new_flat_for_tests();
        put(&mut w, 0, Y - 1, 0, "dirt");
        assert!(!can_place(&mut w, "wheat", BlockPos::new(0, Y, 0)));
        put(&mut w, 0, Y - 1, 0, "farmland");
        assert!(can_place(&mut w, "wheat", BlockPos::new(0, Y, 0)));
        assert!(!can_place(&mut w, "sugar_cane", BlockPos::new(5, Y, 5)), "caña sin agua");
        put(&mut w, 5, Y - 1, 5, "sand");
        put(&mut w, 6, Y - 1, 5, "water");
        assert!(can_place(&mut w, "sugar_cane", BlockPos::new(5, Y, 5)), "caña junto al agua");
    }

    #[test]
    fn wheat_grows_to_ripe() {
        let mut w = World::new_flat_for_tests();
        put(&mut w, 0, Y - 1, 0, "farmland");
        put(&mut w, 0, Y, 0, "wheat");
        let p = BlockPos::new(0, Y, 0);
        for _ in 0..2000 {
            let s = w.get_block(p);
            random_tick(&mut w, p, s, false, &mut Vec::new(), &mut Vec::new());
        }
        let s = w.get_block(p);
        assert_eq!(data::get().block_of_state(s).get(s, "age"), Some("7"));
    }

    #[test]
    fn sapling_becomes_a_tree() {
        let mut w = World::new_flat_for_tests();
        put(&mut w, 0, Y - 1, 0, "grass_block");
        put(&mut w, 0, Y, 0, "oak_sapling");
        let p = BlockPos::new(0, Y, 0);
        for _ in 0..500 {
            let s = w.get_block(p);
            if data::get().block_of_state(s).name != "oak_sapling" {
                break;
            }
            random_tick(&mut w, p, s, false, &mut Vec::new(), &mut Vec::new());
        }
        assert_eq!(name(&mut w, p), "oak_log");
        assert_eq!(name(&mut w, BlockPos::new(0, Y + 3, 0)), "oak_log");
        assert!((Y + 3..Y + 8).any(|y| name(&mut w, BlockPos::new(1, y, 0)) == "oak_leaves"));
    }

    #[test]
    fn dry_farmland_turns_to_dirt() {
        let mut w = World::new_flat_for_tests();
        put(&mut w, 0, Y - 1, 0, "farmland");
        let p = BlockPos::new(0, Y - 1, 0);
        let s = w.get_block(p);
        random_tick(&mut w, p, s, false, &mut Vec::new(), &mut Vec::new());
        assert_eq!(name(&mut w, p), "dirt");
        // Next to water it stays moist.
        put(&mut w, 0, Y - 1, 0, "farmland");
        put(&mut w, 3, Y - 1, 0, "water");
        let s = w.get_block(p);
        random_tick(&mut w, p, s, false, &mut Vec::new(), &mut Vec::new());
        let s = w.get_block(p);
        assert_eq!(data::get().block_of_state(s).get(s, "moisture"), Some("7"));
    }
}

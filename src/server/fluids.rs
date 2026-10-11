//! Flowing water and lava (a simplification of vanilla's FlowingFluid), interaction between
//! the two (obsidian, cobblestone, stone) and buckets.
//!
//! Block levels: 0 = source, 1..7 = flowing (higher = farther), 8..15 = falling.

use super::{blocks::block_change, Player, Server};
use crate::data::{self, Block};
use crate::gameplay;
use crate::item::ItemStack;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Maximum fluid updates per tick (so the server doesn't freeze).
const BUDGET: usize = 512;
const HORIZONTAL: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Default)]
pub struct Fluids {
    queue: BTreeMap<i64, Vec<(Dimension, BlockPos)>>,
    scheduled: HashSet<(Dimension, BlockPos)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Water,
    Lava,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Water => "water",
            Kind::Lava => "lava",
        }
    }
    /// In the Nether (ultrawarm) lava is as fast as water, as in vanilla.
    fn delay(self, d: Dimension) -> i64 {
        match self {
            Kind::Water => 5,
            Kind::Lava if d == Dimension::Nether => 10,
            Kind::Lava => 30,
        }
    }
    /// How much the level drops per block and how far it searches for holes.
    fn drop(self, d: Dimension) -> i32 {
        match self {
            Kind::Lava if d != Dimension::Nether => 2,
            _ => 1,
        }
    }
    fn slope(self, d: Dimension) -> i32 {
        match self {
            Kind::Lava if d != Dimension::Nether => 2,
            _ => 4,
        }
    }
}

fn kind_of(b: &Block) -> Option<Kind> {
    match b.name.as_str() {
        "water" => Some(Kind::Water),
        "lava" => Some(Kind::Lava),
        _ => None,
    }
}

/// (type, level) of the fluid at `p`.
fn fluid_at(world: &mut World, p: BlockPos) -> Option<(Kind, i32)> {
    let state = world.get_block(p);
    let b = data::get().block_of_state(state);
    let k = kind_of(b)?;
    Some((k, b.get(state, "level").and_then(|l| l.parse().ok()).unwrap_or(0)))
}

/// Fluid amount (8 = source or falling).
fn amount(level: i32) -> i32 {
    if level == 0 || level >= 8 { 8 } else { 8 - level }
}

fn state_for(kind: Kind, level: i32) -> u16 {
    let b = data::get().block(kind.name()).unwrap();
    b.with(b.default_state, "level", &level.to_string())
}

/// Can the fluid occupy this block (air, or a plant/torch it washes away)?
fn passable(world: &mut World, p: BlockPos) -> bool {
    if !world.dim.in_bounds(p.y) {
        return false;
    }
    let state = world.get_block(p);
    let reg = data::get();
    if reg.is_air(state) {
        return true;
    }
    let b = reg.block_of_state(state);
    kind_of(b).is_none() && b.hardness == 0.0 && gameplay::get().shape(state).is_empty() && !b.name.contains("fire") && !b.name.ends_with("portal")
}

/// Schedules a fluid update.
pub fn schedule(server: &Server, dim: Dimension, p: BlockPos, delay: i64) {
    let mut f = server.fluids.lock().unwrap();
    if f.scheduled.insert((dim, p)) {
        let at = server.age.load(Ordering::Relaxed) + delay;
        f.queue.entry(at).or_default().push((dim, p));
    }
}

/// A block changed: wakes up the surrounding fluids (and itself, if it is one).
pub fn neighbor_changed(server: &Server, dim: Dimension, p: BlockPos) {
    let around = [p, BlockPos::new(p.x, p.y + 1, p.z), BlockPos::new(p.x, p.y - 1, p.z)]
        .into_iter()
        .chain(HORIZONTAL.iter().map(|&(dx, dz)| BlockPos::new(p.x + dx, p.y, p.z + dz)));
    let found: Vec<(BlockPos, Kind)> = {
        let mut world = server.world(dim).lock().unwrap();
        around.filter_map(|q| fluid_at(&mut world, q).map(|(k, _)| (q, k))).collect()
    };
    for (q, k) in found {
        schedule(server, dim, q, k.delay(dim));
    }
}

pub fn tick(server: &Arc<Server>, age: i64) {
    let due: Vec<(Dimension, BlockPos)> = {
        let mut f = server.fluids.lock().unwrap();
        let mut out = Vec::new();
        while out.len() < BUDGET {
            let Some((&at, _)) = f.queue.iter().next() else { break };
            if at > age {
                break;
            }
            let list = f.queue.remove(&at).unwrap();
            out.extend(list);
        }
        for p in &out {
            f.scheduled.remove(p);
        }
        out
    };
    if due.is_empty() {
        return;
    }
    for dim in Dimension::ALL {
        let here: Vec<BlockPos> = due.iter().filter(|(d, _)| *d == dim).map(|(_, p)| *p).collect();
        if here.is_empty() {
            continue;
        }
        let mut changes: Vec<(BlockPos, u16)> = Vec::new();
        let mut follow: Vec<(BlockPos, i64)> = Vec::new();
        {
            let mut world = server.world(dim).lock().unwrap();
            for p in here {
                update(&mut world, p, &mut changes, &mut follow);
            }
        }
        for (p, s) in changes {
            server.broadcast_dim(dim, &block_change(p, s), None);
        }
        for (p, d) in follow {
            schedule(server, dim, p, d);
        }
    }
}

fn set(world: &mut World, p: BlockPos, state: u16, changes: &mut Vec<(BlockPos, u16)>, follow: &mut Vec<(BlockPos, i64)>) {
    if world.set_block(p, state) != state {
        changes.push((p, state));
        // Wake up neighboring fluids.
        for q in [BlockPos::new(p.x, p.y + 1, p.z), BlockPos::new(p.x, p.y - 1, p.z)]
            .into_iter()
            .chain(HORIZONTAL.iter().map(|&(dx, dz)| BlockPos::new(p.x + dx, p.y, p.z + dz)))
        {
            if let Some((k, _)) = fluid_at(world, q) {
                follow.push((q, k.delay(world.dim)));
            }
        }
        if let Some((k, _)) = fluid_at(world, p) {
            follow.push((p, k.delay(world.dim)));
        }
    }
}

/// Lava touching water: source -> obsidian, flowing -> cobblestone. Returns whether it solidified.
fn lava_meets_water(world: &mut World, p: BlockPos, level: i32, changes: &mut Vec<(BlockPos, u16)>, follow: &mut Vec<(BlockPos, i64)>) -> bool {
    let touching = [BlockPos::new(p.x, p.y + 1, p.z)]
        .into_iter()
        .chain(HORIZONTAL.iter().map(|&(dx, dz)| BlockPos::new(p.x + dx, p.y, p.z + dz)))
        .any(|q| matches!(fluid_at(world, q), Some((Kind::Water, _))));
    if !touching {
        return false;
    }
    let reg = data::get();
    let into = if level == 0 { reg.state("obsidian") } else { reg.state("cobblestone") };
    set(world, p, into, changes, follow);
    true
}

fn update(world: &mut World, p: BlockPos, changes: &mut Vec<(BlockPos, u16)>, follow: &mut Vec<(BlockPos, i64)>) {
    let Some((kind, level)) = fluid_at(world, p) else { return };
    if kind == Kind::Lava && lava_meets_water(world, p, level, changes, follow) {
        return;
    }

    // 1. Recompute the level of flowing blocks from whatever feeds them.
    let mut level = level;
    if level != 0 {
        let above = fluid_at(world, BlockPos::new(p.x, p.y + 1, p.z));
        let new_level = if matches!(above, Some((k, _)) if k == kind) {
            8
        } else {
            let mut best = 0;
            let mut sources = 0;
            for (dx, dz) in HORIZONTAL {
                if let Some((k, l)) = fluid_at(world, BlockPos::new(p.x + dx, p.y, p.z + dz)) {
                    if k == kind {
                        if l == 0 {
                            sources += 1;
                        }
                        best = best.max(amount(l) - kind.drop(world.dim));
                    }
                }
            }
            // Infinite water source: two adjacent sources and solid ground (or a source) below.
            let below = BlockPos::new(p.x, p.y - 1, p.z);
            let below_supports = !passable(world, below) && fluid_at(world, below).is_none() || matches!(fluid_at(world, below), Some((Kind::Water, 0)));
            if kind == Kind::Water && sources >= 2 && below_supports {
                0
            } else if best <= 0 {
                -1
            } else {
                8 - best
            }
        };
        if new_level < 0 {
            set(world, p, 0, changes, follow);
            return;
        }
        if new_level != level {
            set(world, p, state_for(kind, new_level), changes, follow);
            level = new_level;
        }
    }

    // 2. Fall.
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    match fluid_at(world, below) {
        Some((k, _)) if k != kind => {
            // Water falling onto lava (or lava onto water): stone.
            if kind == Kind::Lava {
                set(world, below, data::get().state("stone"), changes, follow);
            } else {
                follow.push((below, Kind::Lava.delay(world.dim)));
            }
            return;
        }
        Some((_, l)) if l != 0 && l < 8 => {
            set(world, below, state_for(kind, 8), changes, follow);
            return;
        }
        Some(_) => {}
        None if passable(world, below) => {
            set(world, below, state_for(kind, 8), changes, follow);
            // A falling source also spreads sideways; a flowing block does not.
            if level != 0 {
                return;
            }
        }
        None => {}
    }

    // 3. Spread sideways, toward the nearest hole.
    let new_amount = amount(level) - kind.drop(world.dim);
    if new_amount <= 0 {
        return;
    }
    let mut best = i32::MAX;
    let mut dirs = Vec::new();
    for (dx, dz) in HORIZONTAL {
        let n = BlockPos::new(p.x + dx, p.y, p.z + dz);
        let open = passable(world, n) || matches!(fluid_at(world, n), Some((k, l)) if k == kind && l != 0 && amount(l) < new_amount);
        if !open {
            continue;
        }
        let d = slope_distance(world, n, kind);
        match d.cmp(&best) {
            std::cmp::Ordering::Less => {
                best = d;
                dirs = vec![n];
            }
            std::cmp::Ordering::Equal => dirs.push(n),
            _ => {}
        }
    }
    for n in dirs {
        set(world, n, state_for(kind, 8 - new_amount), changes, follow);
    }
}

/// Distance (in blocks) to the nearest hole to fall through, or a large value if none.
fn slope_distance(world: &mut World, start: BlockPos, kind: Kind) -> i32 {
    let mut seen = HashSet::new();
    let mut q = VecDeque::from([(start, 0)]);
    while let Some((p, d)) = q.pop_front() {
        if passable(world, BlockPos::new(p.x, p.y - 1, p.z)) {
            return d;
        }
        if d >= kind.slope(world.dim) {
            continue;
        }
        for (dx, dz) in HORIZONTAL {
            let n = BlockPos::new(p.x + dx, p.y, p.z + dz);
            if seen.insert(n) && passable(world, n) {
                q.push_back((n, d + 1));
            }
        }
    }
    1000
}

/// Ray from the eyes: first solid or fluid block (source if `sources_only`).
/// Returns (block hit, previous free cell).
pub fn raycast(world: &mut World, eye: [f64; 3], yaw: f32, pitch: f32, sources_only: bool) -> Option<(BlockPos, BlockPos)> {
    let (yaw, pitch) = (yaw.to_radians() as f64, pitch.to_radians() as f64);
    let dir = [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()];
    let cell = |t: f64| BlockPos::new((eye[0] + dir[0] * t).floor() as i32, (eye[1] + dir[1] * t).floor() as i32, (eye[2] + dir[2] * t).floor() as i32);
    let mut prev = cell(0.0);
    let mut t = 0.0;
    while t <= 5.0 {
        let c = cell(t);
        if c != prev || t == 0.0 {
            let fluid = fluid_at(world, c);
            let solid = !gameplay::get().shape(world.get_block(c)).is_empty();
            match fluid {
                Some((_, 0)) => return Some((c, prev)),
                Some(_) if !sources_only => return Some((c, prev)),
                _ if solid => return Some((c, prev)),
                _ => {}
            }
            prev = c;
        }
        t += 0.05;
    }
    None
}

/// Uses a bucket (empty: pick up a source; full: pour). Returns whether it was a bucket.
pub fn use_bucket(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let reg = data::get();
    let (item, eye, yaw, pitch, game_mode) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.eye(), st.yaw, st.pitch, st.game_mode)
    };
    let Some(item) = item else { return false };
    let name = item.name();
    if !matches!(name, "bucket" | "water_bucket" | "lava_bucket") || game_mode == 3 {
        return false;
    }
    let dim = player.dim();
    // In the Nether water evaporates.
    if name == "water_bucket" && dim == Dimension::Nether {
        let pos = [eye[0], eye[1], eye[2]];
        super::sound::at(server, dim, pos, "block.fire.extinguish", super::sound::BLOCK, 0.5, 2.6);
        return true;
    }
    let mut world = server.world(dim).lock().unwrap();
    let hit = raycast(&mut world, eye, yaw, pitch, true);
    let mut changes = Vec::new();
    let mut follow = Vec::new();
    let result = match (name, hit) {
        ("bucket", Some((p, _))) => match fluid_at(&mut world, p) {
            Some((k, 0)) => {
                set(&mut world, p, 0, &mut changes, &mut follow);
                Some(match k {
                    Kind::Water => "water_bucket",
                    Kind::Lava => "lava_bucket",
                })
            }
            _ => None,
        },
        ("water_bucket" | "lava_bucket", Some((hit, prev))) => {
            let kind = if name == "water_bucket" { Kind::Water } else { Kind::Lava };
            // Onto a fluid it replaces it; against a solid, in the previous cell.
            let target = if fluid_at(&mut world, hit).is_some() { hit } else { prev };
            if passable(&mut world, target) || fluid_at(&mut world, target).is_some() {
                set(&mut world, target, state_for(kind, 0), &mut changes, &mut follow);
                follow.push((target, kind.delay(dim)));
                Some("bucket")
            } else {
                None
            }
        }
        _ => None,
    };
    drop(world);
    for (p, s) in changes {
        server.broadcast_dim(dim, &block_change(p, s), None);
    }
    for (p, d) in follow {
        schedule(server, dim, p, d);
    }
    let Some(new_item) = result else {
        // Rejected: the client may have predicted something; resend the slot.
        let mut st = player.state.lock().unwrap();
        let slot = if hand == 1 { super::inventory::OFFHAND } else { super::inventory::HOTBAR + st.held_slot };
        st.sync_slot(player, slot);
        return true;
    };
    if game_mode != 1 {
        let filled = ItemStack::new(reg.item_id(new_item).unwrap_or(0), 1);
        let mut st = player.state.lock().unwrap();
        let held = st.held_slot;
        st.inv.consume_held(hand, held, 1, Some(filled));
        // If there were more empty buckets left, the filled one went to another slot: resend everything.
        let p = st.inv.window_items();
        player.send(p);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn flat_world() -> World {
        let mut cfg = Config::default();
        cfg.generation.name = "flat".into();
        cfg.generation.options.layers = vec![("bedrock".into(), 1), ("stone".into(), 3)];
        cfg.world_folder = std::env::temp_dir().join(format!("bambietta-fluids-{}", std::process::id())).display().to_string();
        World::new(&cfg, crate::world::Dimension::Overworld).unwrap()
    }

    /// Runs updates until it stabilizes.
    fn settle(world: &mut World, start: BlockPos) {
        let mut queue = vec![start];
        for _ in 0..200 {
            if queue.is_empty() {
                break;
            }
            let mut changes = Vec::new();
            let mut follow = Vec::new();
            for p in std::mem::take(&mut queue) {
                update(world, p, &mut changes, &mut follow);
            }
            queue = follow.into_iter().map(|f| f.0).collect::<HashSet<_>>().into_iter().collect();
        }
    }

    #[test]
    fn water_spreads_and_drains() {
        let mut w = flat_world();
        let src = BlockPos::new(0, -60, 0);
        w.set_block(src, state_for(Kind::Water, 0));
        settle(&mut w, src);
        // Water 7 blocks away at level 7, nothing at 8.
        assert_eq!(fluid_at(&mut w, BlockPos::new(7, -60, 0)), Some((Kind::Water, 7)));
        assert_eq!(fluid_at(&mut w, BlockPos::new(8, -60, 0)), None);
        // Removing the source dries up the flow.
        w.set_block(src, 0);
        for (dx, dz) in HORIZONTAL {
            settle(&mut w, BlockPos::new(dx, -60, dz));
        }
        assert_eq!(fluid_at(&mut w, BlockPos::new(3, -60, 0)), None);
    }

    #[test]
    fn lava_and_water_make_obsidian() {
        let mut w = flat_world();
        let lava = BlockPos::new(20, -60, 20);
        w.set_block(lava, state_for(Kind::Lava, 0));
        w.set_block(BlockPos::new(21, -60, 20), state_for(Kind::Water, 0));
        settle(&mut w, lava);
        assert_eq!(data::get().block_of_state(w.get_block(lava)).name, "obsidian");
    }
}

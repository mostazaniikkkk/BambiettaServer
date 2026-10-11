//! Redstone: sources (levers, buttons, pressure plates, redstone block, torches,
//! repeaters, comparators, observers, lecterns), wire with 0-15 power and its connections,
//! strong/weak power through solid blocks, and consumers (lamps, doors, trapdoors, fence
//! gates, powered rails, note blocks, TNT, pistons, hoppers, droppers, dispensers and bells).
//!
//! Update-driven: when a block changes, nearby wire and components are recomputed;
//! torches, repeaters, comparators, observers, buttons and TNT use scheduled ticks.
//! Comparators read the strength behind them, or how full the container behind them is
//! (also through a solid block), and compare or subtract the strongest side input.

use super::{Player, Server};
use crate::data::{self, Block};
use crate::gameplay;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const DIRS: [(&str, i32, i32, i32); 6] = [("down", 0, -1, 0), ("up", 0, 1, 0), ("north", 0, 0, -1), ("south", 0, 0, 1), ("west", -1, 0, 0), ("east", 1, 0, 0)];
const HORIZONTAL: [(&str, i32, i32); 4] = [("north", 0, -1), ("south", 0, 1), ("west", -1, 0), ("east", 1, 0)];
/// Blocks a piston cannot move.
const IMMOVABLE: &[&str] = &["obsidian", "crying_obsidian", "bedrock", "end_portal_frame", "end_portal", "nether_portal", "barrier", "piston_head", "moving_piston", "enchanting_table", "respawn_anchor", "reinforced_deepslate"];

#[derive(Default)]
pub struct Redstone {
    queue: BTreeMap<i64, Vec<(Dimension, BlockPos)>>,
    scheduled: HashSet<(Dimension, BlockPos)>,
    dirty: Vec<(Dimension, BlockPos)>,
    plates: HashSet<(Dimension, BlockPos)>,
    tnt: Vec<Tnt>,
}

pub struct Tnt {
    pub eid: i32,
    pub dim: Dimension,
    pub pos: [f64; 3],
    vel: [f64; 3],
    fuse: i32,
    on_ground: bool,
    sent: [f64; 3],
}

fn off(p: BlockPos, d: (i32, i32, i32)) -> BlockPos {
    BlockPos::new(p.x + d.0, p.y + d.1, p.z + d.2)
}

fn dir_vec(name: &str) -> (i32, i32, i32) {
    DIRS.iter().find(|d| d.0 == name).map_or((0, 0, 0), |d| (d.1, d.2, d.3))
}

fn opposite(name: &str) -> &'static str {
    match name {
        "down" => "up",
        "up" => "down",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    }
}

/// Notifies that a block changed (processed on the next redstone tick).
pub fn changed(server: &Server, dim: Dimension, pos: BlockPos) {
    server.redstone.lock().unwrap().dirty.push((dim, pos));
}

/// Schedules a tick for the block at `pos` (repeaters, observers, a composter getting ready...).
pub fn schedule(server: &Server, dim: Dimension, pos: BlockPos, delay: i64) {
    let mut r = server.redstone.lock().unwrap();
    if r.scheduled.insert((dim, pos)) {
        let at = server.age.load(Ordering::Relaxed) + delay.max(1);
        r.queue.entry(at).or_default().push((dim, pos));
    }
}

// ---------- signal queries ----------

struct Ctx<'a> {
    world: &'a mut World,
}

impl Ctx<'_> {
    fn state(&mut self, p: BlockPos) -> u16 {
        self.world.get_block(p)
    }

    fn block(&mut self, p: BlockPos) -> (&'static Block, u16) {
        let s = self.state(p);
        (data::get().block_of_state(s), s)
    }

    /// Solid block that conducts signal (full, opaque cube)?
    fn conductor(&mut self, p: BlockPos) -> bool {
        let (b, s) = self.block(p);
        let full = gameplay::get().shape(s) == [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        full && b.filter_light == 15 && !matches!(b.name.as_str(), "redstone_block" | "piston" | "sticky_piston" | "observer" | "tnt" | "target")
    }

    /// Block a mounted component is attached to (lever, button, torch...).
    fn attached(&mut self, p: BlockPos) -> Option<BlockPos> {
        let (b, s) = self.block(p);
        let n = b.name.as_str();
        if n.ends_with("_pressure_plate") || n == "redstone_torch" || n == "redstone_wire" || n == "repeater" {
            return Some(off(p, (0, -1, 0)));
        }
        if n == "redstone_wall_torch" {
            return Some(off(p, dir_vec(opposite(b.get(s, "facing")?))));
        }
        if n == "lever" || n.ends_with("_button") {
            return Some(match b.get(s, "face")? {
                "floor" => off(p, (0, -1, 0)),
                "ceiling" => off(p, (0, 1, 0)),
                _ => off(p, dir_vec(opposite(b.get(s, "facing")?))),
            });
        }
        None
    }

    /// Signal the component at `from` gives to neighboring position `to` (wire excluded).
    /// `strong`: only strong power (the kind that passes through solid blocks).
    fn component_signal(&mut self, from: BlockPos, to: BlockPos, strong: bool) -> u8 {
        let (b, s) = self.block(from);
        let n = b.name.as_str();
        let on = |k: &str| b.get(s, k) == Some("true");
        let attached = self.attached(from);
        let into_attached = attached == Some(to);
        match n {
            "redstone_block" if !strong => 15,
            "lever" if on("powered") && (!strong || into_attached) => 15,
            _ if n.ends_with("_button") && on("powered") && (!strong || into_attached) => 15,
            _ if n.ends_with("_pressure_plate") => {
                let level = match b.get(s, "power") {
                    Some(p) => p.parse().unwrap_or(0),
                    None if on("powered") => 15,
                    None => 0,
                };
                if !strong || into_attached { level } else { 0 }
            }
            "redstone_torch" | "redstone_wall_torch" if on("lit") && !into_attached => {
                // Strong power only upwards.
                if !strong || to == off(from, (0, 1, 0)) { 15 } else { 0 }
            }
            "repeater" if on("powered") => {
                let facing = b.get(s, "facing").unwrap_or("north");
                if to == off(from, dir_vec(opposite(facing))) { 15 } else { 0 }
            }
            "comparator" => {
                let facing = b.get(s, "facing").unwrap_or("north");
                if to == off(from, dir_vec(opposite(facing))) { self.comparator_stored(from) } else { 0 }
            }
            // The observer looks towards `facing` and powers the block behind it.
            "observer" if on("powered") => {
                let facing = b.get(s, "facing").unwrap_or("north");
                if to == off(from, dir_vec(opposite(facing))) { 15 } else { 0 }
            }
            "lectern" if on("powered") => {
                if !strong || to == off(from, (0, -1, 0)) { 15 } else { 0 }
            }
            _ => 0,
        }
    }

    fn comparator_stored(&mut self, p: BlockPos) -> u8 {
        match self.world.peek_block_entity(p) {
            Some(crate::world::blockentity::BlockEntity::Comparator(o)) => *o,
            _ => 0,
        }
    }

    /// Strength `from` gives into `to`: components, wire, or a powered solid block.
    fn input_level(&mut self, from: BlockPos, to: BlockPos) -> u8 {
        let mut level = self.component_signal(from, to, false).max(self.wire_signal(from, to));
        if self.conductor(from) {
            level = level.max(self.block_power(from, Some(to), false));
        }
        level
    }

    /// What a comparator outputs (vanilla ComparatorBlock.calculateOutputSignal).
    fn comparator_output(&mut self, p: BlockPos, b: &Block, s: u16) -> u8 {
        let facing = b.get(s, "facing").unwrap_or("north");
        let back = off(p, dir_vec(facing));
        let mut rear = self.input_level(back, p);
        // A container (or another readable block) behind it, or behind a solid block.
        if let Some(a) = super::mechanisms::analog_signal(self.world, back) {
            rear = a;
        } else if rear < 15 && self.conductor(back) {
            if let Some(a) = super::mechanisms::analog_signal(self.world, off(back, dir_vec(facing))) {
                rear = a;
            }
        }
        if rear == 0 {
            return 0;
        }
        // Side inputs: only wire, redstone blocks, repeaters and comparators.
        let sides: [&str; 2] = if matches!(facing, "north" | "south") { ["east", "west"] } else { ["north", "south"] };
        let mut side = 0;
        for d in sides {
            let n = off(p, dir_vec(d));
            let (nb, ns) = self.block(n);
            side = side.max(match nb.name.as_str() {
                "redstone_wire" => nb.get(ns, "power").and_then(|v| v.parse().ok()).unwrap_or(0),
                "redstone_block" => 15,
                "repeater" | "comparator" => self.component_signal(n, p, false),
                _ => 0,
            });
        }
        if side > rear {
            0
        } else if b.get(s, "mode") == Some("subtract") {
            rear - side
        } else {
            rear
        }
    }

    /// Signal the wire at `wire` gives to position `to` (downwards or where it connects).
    fn wire_signal(&mut self, wire: BlockPos, to: BlockPos) -> u8 {
        let (b, s) = self.block(wire);
        if b.name != "redstone_wire" {
            return 0;
        }
        let power: u8 = b.get(s, "power").and_then(|p| p.parse().ok()).unwrap_or(0);
        if power == 0 {
            return 0;
        }
        if to == off(wire, (0, -1, 0)) {
            return power;
        }
        for (name, dx, dz) in HORIZONTAL {
            if to == off(wire, (dx, 0, dz)) {
                return if b.get(s, name) != Some("none") { power } else { 0 };
            }
        }
        0
    }

    /// Power received by a solid block. `for_wire`: wire only takes strong power.
    fn block_power(&mut self, p: BlockPos, except: Option<BlockPos>, for_wire: bool) -> u8 {
        let mut best = 0;
        for d in DIRS {
            let n = off(p, (d.1, d.2, d.3));
            if Some(n) == except {
                continue;
            }
            best = best.max(self.component_signal(n, p, true));
            if !for_wire {
                best = best.max(self.wire_signal(n, p));
            }
        }
        best
    }

    /// Does a consumer (lamp, piston, door...) at `p` receive signal?
    fn powered(&mut self, p: BlockPos, skip: Option<&str>) -> bool {
        for d in DIRS {
            if Some(d.0) == skip {
                continue;
            }
            let n = off(p, (d.1, d.2, d.3));
            if self.component_signal(n, p, false) > 0 || self.wire_signal(n, p) > 0 {
                return true;
            }
            if self.conductor(n) && self.block_power(n, Some(p), false) > 0 {
                return true;
            }
        }
        false
    }

    /// Repeater input power (from behind).
    fn repeater_input(&mut self, p: BlockPos, facing: &str) -> bool {
        let back = off(p, dir_vec(facing));
        self.component_signal(back, p, false) > 0 || self.wire_signal(back, p) > 0 || (self.conductor(back) && self.block_power(back, Some(p), false) > 0)
    }

    // ---------- wire ----------

    /// Connected neighboring wires (same level, one step up or down).
    fn wire_neighbors(&mut self, w: BlockPos) -> Vec<BlockPos> {
        let mut out = Vec::new();
        let above_solid = self.conductor(off(w, (0, 1, 0)));
        for (_, dx, dz) in HORIZONTAL {
            let side = off(w, (dx, 0, dz));
            if self.block(side).0.name == "redstone_wire" {
                out.push(side);
            }
            let up = off(side, (0, 1, 0));
            if !above_solid && self.block(up).0.name == "redstone_wire" {
                out.push(up);
            }
            let down = off(side, (0, -1, 0));
            if !self.conductor(side) && self.block(down).0.name == "redstone_wire" {
                out.push(down);
            }
        }
        out
    }

    /// Power a wire receives from non-wire sources.
    fn wire_source_power(&mut self, w: BlockPos) -> u8 {
        let mut best = 0;
        for d in DIRS {
            let n = off(w, (d.1, d.2, d.3));
            best = best.max(self.component_signal(n, w, false));
            if self.conductor(n) {
                best = best.max(self.block_power(n, Some(w), true));
            }
        }
        best
    }

    /// What does the wire connect to towards `dir`? ("side", "up" or "none").
    fn wire_connection(&mut self, w: BlockPos, dir: &str, dx: i32, dz: i32) -> &'static str {
        let side = off(w, (dx, 0, dz));
        let up = off(side, (0, 1, 0));
        if !self.conductor(off(w, (0, 1, 0))) && self.block(up).0.name == "redstone_wire" && self.conductor(side) {
            return "up";
        }
        let (b, s) = self.block(side);
        let n = b.name.as_str();
        let connects = match n {
            "redstone_wire" => true,
            "redstone_block" | "lever" | "redstone_torch" | "redstone_wall_torch" | "target" | "daylight_detector" | "observer" => true,
            _ if n.ends_with("_button") || n.ends_with("_pressure_plate") => true,
            "repeater" | "comparator" => matches!(b.get(s, "facing"), Some(f) if f == dir || f == opposite(dir)),
            _ => false,
        };
        if connects {
            return "side";
        }
        let down = off(side, (0, -1, 0));
        if !self.conductor(side) && self.block(down).0.name == "redstone_wire" {
            return "side";
        }
        "none"
    }

    /// Wire state with its power and connections (cross if unconnected, line if connected to one).
    fn wire_state(&mut self, w: BlockPos, power: u8) -> u16 {
        let reg = data::get();
        let b = reg.block("redstone_wire").unwrap();
        let mut conns: Vec<(&str, &str)> = HORIZONTAL.iter().map(|&(n, dx, dz)| (n, self.wire_connection(w, n, dx, dz))).collect();
        let count = conns.iter().filter(|c| c.1 != "none").count();
        if count == 0 {
            for c in &mut conns {
                c.1 = "side";
            }
        } else if count == 1 {
            let only = conns.iter().find(|c| c.1 != "none").unwrap().0;
            let opp = opposite(only);
            for c in &mut conns {
                if c.0 == opp {
                    c.1 = "side";
                }
            }
        }
        let mut s = b.with(b.default_state, "power", &power.to_string());
        for (n, v) in conns {
            s = b.with(s, n, v);
        }
        s
    }

    /// Recomputes the whole wire network touching `start`. Returns the changes.
    fn update_wires(&mut self, start: BlockPos, out: &mut Vec<(BlockPos, u16)>) {
        let mut network = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([start]);
        while let Some(w) = queue.pop_front() {
            if !seen.insert(w) || network.len() > 2048 || self.block(w).0.name != "redstone_wire" {
                continue;
            }
            network.push(w);
            for n in self.wire_neighbors(w) {
                queue.push_back(n);
            }
        }
        if network.is_empty() {
            return;
        }
        // Power: sources first, then propagation decreasing by 1 per block.
        let mut power: HashMap<BlockPos, u8> = network.iter().map(|&w| (w, 0)).collect();
        // Priority queue: highest power first.
        let mut heap = std::collections::BinaryHeap::new();
        for &w in &network {
            let p = self.wire_source_power(w);
            if p > 0 {
                power.insert(w, p);
                heap.push((p, w.x, w.y, w.z));
            }
        }
        while let Some((p, x, y, z)) = heap.pop() {
            let w = BlockPos::new(x, y, z);
            if p <= 1 || power.get(&w).copied().unwrap_or(0) > p {
                continue;
            }
            for n in self.wire_neighbors(w) {
                if let Some(cur) = power.get_mut(&n) {
                    if *cur < p - 1 {
                        *cur = p - 1;
                        heap.push((p - 1, n.x, n.y, n.z));
                    }
                }
            }
        }
        for &w in &network {
            let s = self.wire_state(w, power[&w]);
            if s != self.state(w) {
                out.push((w, s));
            }
        }
    }
}

// ---------- tick ----------

pub fn tick(server: &Arc<Server>, age: i64) {
    tick_plates(server, age);
    tick_tnt(server);
    // Observers whose block changed fire 2 ticks later.
    for dim in Dimension::ALL {
        let hits = std::mem::take(&mut server.world(dim).lock().unwrap().observer_hits);
        for q in hits {
            schedule(server, dim, q, 2);
        }
    }
    let (due, dirty) = {
        let mut r = server.redstone.lock().unwrap();
        let mut due = Vec::new();
        while let Some((&at, _)) = r.queue.iter().next() {
            if at > age {
                break;
            }
            due.extend(r.queue.remove(&at).unwrap());
        }
        for d in &due {
            r.scheduled.remove(d);
        }
        (due, std::mem::take(&mut r.dirty))
    };
    for (dim, pos) in due {
        scheduled_tick(server, dim, pos);
    }
    // Chained updates (wire -> lamp -> ...) are resolved within the same tick.
    let mut pending = dirty;
    for _ in 0..16 {
        if pending.is_empty() {
            break;
        }
        let mut next = Vec::new();
        for dim in Dimension::ALL {
            let here: HashSet<BlockPos> = pending.iter().filter(|(d, _)| *d == dim).map(|(_, p)| *p).collect();
            if !here.is_empty() {
                next.extend(update_area(server, dim, &here).into_iter().map(|p| (dim, p)));
            }
        }
        pending = next;
    }
    if !pending.is_empty() {
        server.redstone.lock().unwrap().dirty.extend(pending);
    }
}

/// Recomputes wire and components around the changed positions. Returns what changed.
fn update_area(server: &Arc<Server>, dim: Dimension, positions: &HashSet<BlockPos>) -> Vec<BlockPos> {
    let mut changes: Vec<(BlockPos, u16)> = Vec::new();
    let mut to_schedule: Vec<(BlockPos, i64)> = Vec::new();
    let mut notes: Vec<(BlockPos, u16)> = Vec::new();
    let mut primed: Vec<BlockPos> = Vec::new();
    let mut pistons: Vec<(BlockPos, bool)> = Vec::new();
    let mut bells: Vec<(BlockPos, i32)> = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        let mut c = Ctx { world: &mut world };
        // Affected area: 2 blocks around (signal passes through a solid block).
        let mut area = HashSet::new();
        for p in positions {
            for dx in -2..=2 {
                for dy in -2..=2 {
                    for dz in -2..=2 {
                        if dx * dx + dy * dy + dz * dz <= 6 {
                            area.insert(BlockPos::new(p.x + dx, p.y + dy, p.z + dz));
                        }
                    }
                }
            }
        }
        let mut wires_done = HashSet::new();
        for &p in &area {
            if !dim.in_bounds(p.y) {
                continue;
            }
            let (b, s) = c.block(p);
            let name = b.name.as_str();
            let on = |k: &str| b.get(s, k) == Some("true");
            match name {
                "redstone_wire" if !wires_done.contains(&p) => {
                    let mut out = Vec::new();
                    c.update_wires(p, &mut out);
                    wires_done.insert(p);
                    for (w, st) in out {
                        wires_done.insert(w);
                        changes.push((w, st));
                    }
                }
                "redstone_lamp" => {
                    let want = c.powered(p, None);
                    if want != on("lit") {
                        changes.push((p, b.with(s, "lit", if want { "true" } else { "false" })));
                    }
                }
                "redstone_torch" | "redstone_wall_torch" => {
                    let att = c.attached(p).unwrap();
                    let input = c.conductor(att) && c.block_power(att, Some(p), false) > 0;
                    if input == on("lit") {
                        to_schedule.push((p, 2));
                    }
                }
                "repeater" => {
                    let facing = b.get(s, "facing").unwrap_or("north");
                    if c.repeater_input(p, facing) != on("powered") {
                        let delay: i64 = b.get(s, "delay").and_then(|d| d.parse().ok()).unwrap_or(1);
                        to_schedule.push((p, delay * 2));
                    }
                }
                "powered_rail" | "activator_rail" => {
                    let want = c.powered(p, None);
                    if want != on("powered") {
                        changes.push((p, b.with(s, "powered", if want { "true" } else { "false" })));
                    }
                }
                "note_block" => {
                    let want = c.powered(p, None);
                    if want != on("powered") {
                        changes.push((p, b.with(s, "powered", if want { "true" } else { "false" })));
                        if want {
                            notes.push((p, s));
                        }
                    }
                }
                "tnt" => {
                    if c.powered(p, None) {
                        primed.push(p);
                    }
                }
                "comparator" => {
                    if c.comparator_output(p, b, s) != c.comparator_stored(p) {
                        to_schedule.push((p, 2));
                    }
                }
                "hopper" => {
                    // A powered hopper stops.
                    let want = !c.powered(p, None);
                    if want != on("enabled") {
                        changes.push((p, b.with(s, "enabled", if want { "true" } else { "false" })));
                    }
                }
                "dispenser" | "dropper" => {
                    // Also powered from the block above (like vanilla).
                    let want = c.powered(p, None) || c.powered(off(p, (0, 1, 0)), None);
                    if want && !on("triggered") {
                        changes.push((p, b.with(s, "triggered", "true")));
                        to_schedule.push((p, 4));
                    } else if !want && on("triggered") {
                        changes.push((p, b.with(s, "triggered", "false")));
                    }
                }
                "bell" => {
                    let want = c.powered(p, None);
                    if want != on("powered") {
                        changes.push((p, b.with(s, "powered", if want { "true" } else { "false" })));
                        if want {
                            let face = ["down", "up", "north", "south", "west", "east"].iter().position(|f| Some(*f) == b.get(s, "facing")).unwrap_or(2);
                            bells.push((p, face as i32));
                        }
                    }
                }
                "piston" | "sticky_piston" => {
                    let facing = b.get(s, "facing").unwrap_or("north");
                    let want = c.powered(p, Some(facing));
                    if want != on("extended") {
                        pistons.push((p, want));
                    }
                }
                _ if (b.has("open") && b.has("powered")) => {
                    // Doors, trapdoors and fence gates (iron ones only with redstone).
                    if name.ends_with("_door") && b.get(s, "half") == Some("upper") {
                        continue;
                    }
                    let mut want = c.powered(p, None);
                    if name.ends_with("_door") {
                        want |= c.powered(off(p, (0, 1, 0)), None);
                    }
                    if want != on("powered") {
                        let ns = b.with(b.with(s, "powered", if want { "true" } else { "false" }), "open", if want { "true" } else { "false" });
                        changes.push((p, ns));
                        if name.ends_with("_door") {
                            let up = off(p, (0, 1, 0));
                            let us = c.state(up);
                            if data::get().block_of_state(us).name == name {
                                changes.push((up, b.with(b.with(us, "powered", if want { "true" } else { "false" }), "open", if want { "true" } else { "false" })));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for &(p, s) in &changes {
            c.world.set_block(p, s);
        }
    }
    for (p, d) in to_schedule {
        schedule(server, dim, p, d);
    }
    for &(p, s) in &changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
    for (p, s) in notes {
        let note: i32 = data::get().block_of_state(s).get(s, "note").and_then(|n| n.parse().ok()).unwrap_or(0);
        let pitch = 2f32.powf((note - 12) as f32 / 12.0);
        super::sound::at(server, dim, [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5], "block.note_block.harp", super::sound::BLOCK, 3.0, pitch);
    }
    for p in primed {
        prime_tnt(server, dim, p, 80);
    }
    for (p, face) in bells {
        super::mechanisms::ring(server, dim, p, face);
    }
    let mut moved = Vec::new();
    for (p, extend) in pistons {
        moved.extend(move_piston(server, dim, p, extend));
    }
    changes.into_iter().map(|c| c.0).chain(moved).collect()
}

/// Scheduled tick: torches, repeaters, comparators, observers, buttons, droppers and
/// dispensers, lecterns and composters.
fn scheduled_tick(server: &Arc<Server>, dim: Dimension, pos: BlockPos) {
    let mut fire = false;
    let mut signal_changed = false;
    let mut again = None;
    let mut ready = false;
    let change = {
        let mut world = server.world(dim).lock().unwrap();
        let mut c = Ctx { world: &mut world };
        let (b, s) = c.block(pos);
        let on = |k: &str| b.get(s, k) == Some("true");
        match b.name.as_str() {
            "redstone_torch" | "redstone_wall_torch" => {
                let att = c.attached(pos).unwrap();
                let input = c.conductor(att) && c.block_power(att, Some(pos), false) > 0;
                (input == on("lit")).then(|| b.with(s, "lit", if input { "false" } else { "true" }))
            }
            "repeater" => {
                let facing = b.get(s, "facing").unwrap_or("north");
                let input = c.repeater_input(pos, facing);
                (input != on("powered")).then(|| b.with(s, "powered", if input { "true" } else { "false" }))
            }
            n if n.ends_with("_button") && on("powered") => Some(b.with(s, "powered", "false")),
            "comparator" => {
                let out = c.comparator_output(pos, b, s);
                if out != c.comparator_stored(pos) {
                    c.world.set_block_entity(pos, crate::world::blockentity::BlockEntity::Comparator(out));
                    signal_changed = true;
                }
                ((out > 0) != on("powered")).then(|| b.with(s, "powered", if out > 0 { "true" } else { "false" }))
            }
            // A 2-tick pulse.
            "observer" if on("powered") => Some(b.with(s, "powered", "false")),
            "observer" => {
                again = Some(2);
                Some(b.with(s, "powered", "true"))
            }
            "dispenser" | "dropper" => {
                fire = true;
                None
            }
            "lectern" if on("powered") => Some(b.with(s, "powered", "false")),
            "composter" => {
                let next = super::mechanisms::composter_ready(s);
                ready = next.is_some();
                next
            }
            _ => None,
        }
    };
    if let Some(s) = change {
        server.world(dim).lock().unwrap().set_block(pos, s);
        server.broadcast_dim(dim, &super::blocks::block_change(pos, s), None);
        changed(server, dim, pos);
    } else if signal_changed {
        changed(server, dim, pos);
    }
    if ready {
        super::sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], "block.composter.ready", super::sound::BLOCK, 1.0, 1.0);
    }
    if let Some(d) = again {
        schedule(server, dim, pos, d);
    }
    if fire {
        super::hoppers::dispense(server, dim, pos);
    }
}

/// Pressing a button: it turns on and switches itself off (1 s for stone, 1.5 s for wood).
pub fn press_button(server: &Arc<Server>, dim: Dimension, pos: BlockPos, block: &Block, state: u16) {
    if block.get(state, "powered") == Some("true") {
        return;
    }
    let s = block.with(state, "powered", "true");
    server.world(dim).lock().unwrap().set_block(pos, s);
    server.broadcast_dim(dim, &super::blocks::block_change(pos, s), None);
    changed(server, dim, pos);
    let wooden = !block.name.contains("stone") && !block.name.contains("blackstone");
    schedule(server, dim, pos, if wooden { 30 } else { 20 });
    super::sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], "block.stone_button.click_on", super::sound::BLOCK, 0.3, 0.6);
}

// ---------- pressure plates ----------

fn tick_plates(server: &Arc<Server>, age: i64) {
    if age % 4 != 0 {
        return;
    }
    // Who steps on what: players (not spectators), mobs and items.
    let mut occupied: HashMap<(Dimension, BlockPos), u32> = HashMap::new();
    let block_at = |x: f64, y: f64, z: f64| BlockPos::new(x.floor() as i32, y.floor() as i32, z.floor() as i32);
    for p in server.online() {
        let st = p.state.lock().unwrap();
        if st.game_mode != 3 && !st.dead {
            *occupied.entry((p.dim(), block_at(st.x, st.y + 0.05, st.z))).or_default() += 1;
        }
    }
    for m in server.mobs.lock().unwrap().values() {
        *occupied.entry((m.dim, block_at(m.pos[0], m.pos[1] + 0.05, m.pos[2]))).or_default() += 1;
    }
    for e in server.items.lock().unwrap().values() {
        *occupied.entry((e.dim, block_at(e.pos[0], e.pos[1] + 0.05, e.pos[2]))).or_default() += 1;
    }
    let active: Vec<(Dimension, BlockPos)> = server.redstone.lock().unwrap().plates.iter().copied().collect();
    let mut updates: Vec<(Dimension, BlockPos, u16)> = Vec::new();
    let mut now_active = HashSet::new();
    for dim in Dimension::ALL {
        let mut world = server.world(dim).lock().unwrap();
        let candidates: HashSet<BlockPos> = occupied.keys().filter(|k| k.0 == dim).map(|k| k.1).chain(active.iter().filter(|a| a.0 == dim).map(|a| a.1)).collect();
        for p in candidates {
            let s = world.get_block(p);
            let b = data::get().block_of_state(s);
            if !b.name.ends_with("_pressure_plate") {
                continue;
            }
            let count = occupied.get(&(dim, p)).copied().unwrap_or(0);
            let new = if b.has("power") {
                // Weighted plates: more things on top, more power.
                let heavy = b.name.starts_with("heavy");
                let level = if heavy { count.div_ceil(10) } else { count };
                b.with(s, "power", &level.min(15).to_string())
            } else {
                b.with(s, "powered", if count > 0 { "true" } else { "false" })
            };
            if count > 0 {
                now_active.insert((dim, p));
            }
            if new != s {
                world.set_block(p, new);
                updates.push((dim, p, new));
            }
        }
    }
    server.redstone.lock().unwrap().plates = now_active;
    for (dim, p, s) in updates {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
        changed(server, dim, p);
    }
}

// ---------- TNT ----------

/// Ignites the TNT at `pos` (the block disappears and the entity spawns).
pub fn prime_tnt(server: &Arc<Server>, dim: Dimension, pos: BlockPos, fuse: i32) {
    {
        let mut world = server.world(dim).lock().unwrap();
        if data::get().block_of_state(world.get_block(pos)).name != "tnt" {
            return;
        }
        world.set_block(pos, 0);
    }
    server.broadcast_dim(dim, &super::blocks::block_change(pos, 0), None);
    spawn_tnt(server, dim, [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5], fuse);
}

/// Primed TNT at a position (from a TNT block or a dispenser).
pub fn spawn_tnt(server: &Server, dim: Dimension, at: [f64; 3], fuse: i32) {
    let a = super::entities::random() as f64 * std::f64::consts::TAU;
    let t = Tnt {
        eid: server.next_entity_id(),
        dim,
        pos: at,
        vel: [-a.sin() * 0.02, 0.2, -a.cos() * 0.02],
        fuse,
        on_ground: false,
        sent: [0.0; 3],
    };
    super::sound::at(server, dim, t.pos, "entity.tnt.primed", super::sound::BLOCK, 1.0, 1.0);
    server.redstone.lock().unwrap().tnt.push(t);
}

fn tick_tnt(server: &Arc<Server>) {
    let mut boom = Vec::new();
    let mut packets = Vec::new();
    {
        let mut r = server.redstone.lock().unwrap();
        if r.tnt.is_empty() {
            return;
        }
        for t in r.tnt.iter_mut() {
            t.fuse -= 1;
            {
                let mut world = server.world(t.dim).lock().unwrap();
                super::entities::step(&mut world, &mut t.pos, &mut t.vel, &mut t.on_ground, 0.98, 0.04);
            }
            if t.fuse <= 0 {
                boom.push((t.dim, t.pos));
            } else if (0..3).any(|i| (t.pos[i] - t.sent[i]).abs() > 0.01) {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(t.eid).f64(t.pos[0]).f64(t.pos[1]).f64(t.pos[2]).u8(0).u8(0).bool(t.on_ground);
                packets.push((t.eid, p));
                t.sent = t.pos;
            }
        }
        r.tnt.retain(|t| t.fuse > 0);
    }
    for (eid, p) in packets {
        server.broadcast_tracked(eid, &p, true);
    }
    for (dim, pos) in boom {
        super::explosion::explode(server, dim, [pos[0], pos[1] + 0.06, pos[2]], 4.0, None);
    }
}

pub fn tnt_positions(server: &Server) -> Vec<(i32, [f64; 3], Dimension)> {
    server.redstone.lock().unwrap().tnt.iter().map(|t| (t.eid, t.pos, t.dim)).collect()
}

pub fn tnt_spawn_packets(server: &Server, eids: &[i32]) -> HashMap<i32, Vec<Vec<u8>>> {
    let r = server.redstone.lock().unwrap();
    r.tnt
        .iter()
        .filter(|t| eids.contains(&t.eid))
        .map(|t| {
            let mut p = packet(id::play::SPAWN_ENTITY);
            p.varint(t.eid).uuid(t.eid as u128 | 0x7470 << 64).varint(data::get().entity("tnt"));
            p.f64(t.pos[0]).f64(t.pos[1]).f64(t.pos[2]).u8(0).u8(0).u8(0).varint(0);
            for v in t.vel {
                p.i16((v * 8000.0) as i16);
            }
            // Metadata 8: remaining fuse (for the flashing).
            let mut m = packet(id::play::ENTITY_METADATA);
            m.varint(t.eid).u8(8).varint(1).varint(t.fuse).u8(0xff);
            (t.eid, vec![p, m])
        })
        .collect()
}

// ---------- pistons ----------

/// Extends or retracts a piston. Returns the positions that changed.
fn move_piston(server: &Arc<Server>, dim: Dimension, pos: BlockPos, extend: bool) -> Vec<BlockPos> {
    let reg = data::get();
    let mut changes: Vec<(BlockPos, u16)> = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        let s = world.get_block(pos);
        let b = reg.block_of_state(s);
        let facing = b.get(s, "facing").unwrap_or("north").to_string();
        let d = dir_vec(&facing);
        let sticky = b.name == "sticky_piston";
        let head = reg.block("piston_head").unwrap();
        let head_state = head.with(head.with(head.default_state, "facing", &facing), "type", if sticky { "sticky" } else { "normal" });
        let movable = |w: &mut World, p: BlockPos| -> Option<bool> {
            let st = w.get_block(p);
            let bl = reg.block_of_state(st);
            if reg.is_air(st) || matches!(bl.name.as_str(), "water" | "lava" | "short_grass" | "tall_grass" | "fern" | "snow" | "fire" | "torch" | "dead_bush") || bl.hardness == 0.0 && gameplay::get().shape(st).is_empty() {
                return Some(false); // free space (it breaks)
            }
            let has_be = crate::world::blockentity::type_id(&bl.name).is_some();
            if IMMOVABLE.contains(&bl.name.as_str()) || bl.hardness < 0.0 || has_be || ((bl.name == "piston" || bl.name == "sticky_piston") && bl.get(st, "extended") == Some("true")) {
                return None;
            }
            Some(true)
        };
        if extend {
            // Blocks to push (12 max).
            let mut line = Vec::new();
            let mut p = off(pos, d);
            loop {
                if !dim.in_bounds(p.y) {
                    return vec![];
                }
                match movable(&mut world, p) {
                    None => return vec![],
                    Some(false) => break,
                    Some(true) => {
                        line.push(p);
                        if line.len() > 12 {
                            return vec![];
                        }
                        p = off(p, d);
                    }
                }
            }
            for &q in line.iter().rev() {
                let st = world.get_block(q);
                changes.push((off(q, d), st));
            }
            changes.push((off(pos, d), head_state));
            changes.push((pos, b.with(s, "extended", "true")));
        } else {
            let front = off(pos, d);
            if reg.block_of_state(world.get_block(front)).name == "piston_head" {
                changes.push((front, 0));
            }
            if sticky {
                let pulled = off(front, d);
                if movable(&mut world, pulled) == Some(true) {
                    let st = world.get_block(pulled);
                    changes.push((front, st));
                    changes.push((pulled, 0));
                }
            }
            changes.push((pos, b.with(s, "extended", "false")));
        }
        for &(p, st) in &changes {
            world.set_block(p, st);
        }
    }
    for &(p, st) in &changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, st), None);
    }
    let sound = if extend { "block.piston.extend" } else { "block.piston.contract" };
    super::sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], sound, super::sound::BLOCK, 0.5, 0.7);
    changes.into_iter().map(|c| c.0).collect()
}

/// Flint and steel on TNT: it ignites.
pub fn ignite_tnt(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) {
    prime_tnt(server, player.dim(), pos, 80);
    super::health::wear_held(server, player, 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn world() -> World {
        let mut cfg = Config::default();
        cfg.generation.name = "flat".into();
        cfg.generation.options.layers = vec![("bedrock".into(), 1), ("stone".into(), 3)];
        cfg.world_folder = std::env::temp_dir().join(format!("bambietta-rs-{}-{}", std::process::id(), crate::server::entities::random())).display().to_string();
        World::new(&cfg, Dimension::Overworld).unwrap()
    }

    fn st(name: &str, props: &[(&str, &str)]) -> u16 {
        let b = data::get().block(name).unwrap();
        props.iter().fold(b.default_state, |s, (k, v)| b.with(s, k, v))
    }

    fn power(w: &mut World, p: BlockPos) -> String {
        let s = w.get_block(p);
        data::get().block_of_state(s).get(s, "power").unwrap_or("-").to_string()
    }

    #[test]
    fn wire_carries_power_and_decays() {
        let mut w = world();
        let y = -60;
        w.set_block(BlockPos::new(0, y, 0), st("redstone_block", &[]));
        for x in 1..=20 {
            w.set_block(BlockPos::new(x, y, 0), st("redstone_wire", &[]));
        }
        let mut c = Ctx { world: &mut w };
        let mut out = Vec::new();
        c.update_wires(BlockPos::new(1, y, 0), &mut out);
        for (p, s) in out {
            w.set_block(p, s);
        }
        assert_eq!(power(&mut w, BlockPos::new(1, y, 0)), "15");
        assert_eq!(power(&mut w, BlockPos::new(15, y, 0)), "1");
        assert_eq!(power(&mut w, BlockPos::new(16, y, 0)), "0");
        // A straight wire connects towards both sides.
        let s = w.get_block(BlockPos::new(5, y, 0));
        let b = data::get().block_of_state(s);
        assert_eq!((b.get(s, "east"), b.get(s, "west"), b.get(s, "north")), (Some("side"), Some("side"), Some("none")));
    }

    #[test]
    fn lamp_lever_and_torch_inverter() {
        let mut w = world();
        let y = -60;
        // Powered lever attached to the lamp: the lamp receives signal.
        let lamp = BlockPos::new(0, y, 0);
        w.set_block(lamp, st("redstone_lamp", &[]));
        w.set_block(BlockPos::new(1, y, 0), st("lever", &[("face", "wall"), ("facing", "east"), ("powered", "true")]));
        let mut c = Ctx { world: &mut w };
        assert!(c.powered(lamp, None));
        // Inverter: lever attached to a solid block turns off the torch on the other side.
        let block = BlockPos::new(10, y, 0);
        w.set_block(block, st("stone", &[]));
        w.set_block(BlockPos::new(11, y, 0), st("lever", &[("face", "wall"), ("facing", "east"), ("powered", "true")]));
        w.set_block(BlockPos::new(9, y, 0), st("redstone_wall_torch", &[("facing", "west"), ("lit", "true")]));
        let mut c = Ctx { world: &mut w };
        assert_eq!(c.attached(BlockPos::new(9, y, 0)), Some(block));
        assert!(c.block_power(block, Some(BlockPos::new(9, y, 0)), false) > 0, "el bloque recibe potencia fuerte de la palanca");
    }
}

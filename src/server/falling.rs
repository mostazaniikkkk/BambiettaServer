//! Blocks that fall: sand, gravel, concrete powder (it sets into concrete in water), anvils
//! and dragon eggs fall when nothing holds them up; a stalactite (pointed dripstone hanging
//! down) falls as a whole when what it hangs from goes. Falling anvils and stalactites hurt
//! whatever they land on, more the further they fell; a stalactite shatters when it lands.
//!
//! The world records, on every block change, the spots worth a look (`World::falls`).

use super::{entities, sound, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::physics::{move_aabb, Aabb};
use crate::world::{Dimension, World};
use std::sync::{Arc, Mutex};

pub struct Falling {
    eid: i32,
    dim: Dimension,
    state: u16,
    pos: [f64; 3],
    vel: [f64; 3],
    start_y: f64,
    age: u32,
    sent: [f64; 3],
}

static FALLING: Mutex<Vec<Falling>> = Mutex::new(Vec::new());

fn gravity(name: &str) -> bool {
    crate::world::gravity_block(name)
}

fn hangs_down(world: &World, p: BlockPos) -> bool {
    world.peek_block(p).is_some_and(|s| {
        let b = data::get().block_of_state(s);
        b.name == "pointed_dripstone" && b.get(s, "vertical_direction") == Some("down")
    })
}

/// Can a falling block move into this state?
fn free(state: u16) -> bool {
    let reg = data::get();
    let n = reg.block_of_state(state).name.as_str();
    reg.is_air(state) || matches!(n, "water" | "lava" | "fire" | "short_grass" | "tall_grass" | "fern" | "snow" | "dead_bush" | "bubble_column")
}

fn spawn_packets(f: &Falling) -> Vec<u8> {
    let mut p = packet(id::play::SPAWN_ENTITY);
    p.varint(f.eid).uuid(0x4641_4c4c_0000_0000_0000_0000_0000_0000 | f.eid as u32 as u128).varint(data::get().entity("falling_block"));
    p.f64(f.pos[0]).f64(f.pos[1]).f64(f.pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(f.state as i32);
    p.i16(0).i16(0).i16(0);
    p
}

/// Turns the block at `p` into a falling block.
fn drop_block(server: &Server, world: &mut World, dim: Dimension, p: BlockPos, out: &mut Vec<Vec<u8>>, changes: &mut Vec<BlockPos>) {
    let state = world.get_block(p);
    world.set_block(p, 0);
    changes.push(p);
    let f = Falling { eid: server.next_entity_id(), dim, state, pos: [p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5], vel: [0.0; 3], start_y: p.y as f64, age: 0, sent: [0.0; 3] };
    out.push(spawn_packets(&f));
    FALLING.lock().unwrap().push(f);
}

/// Every tick: looks at the spots the world flagged, and moves the falling blocks.
pub fn tick(server: &Arc<Server>) {
    let reg = data::get();
    for dim in Dimension::ALL {
        let mut spawns = Vec::new();
        let mut changes = Vec::new();
        {
            let mut world = server.world(dim).lock().unwrap();
            let spots = std::mem::take(&mut world.falls);
            let scaffold = std::mem::take(&mut world.scaffold);
            if !scaffold.is_empty() {
                drop(world);
                super::blockmech::scaffold_update(server, dim, scaffold);
                world = server.world(dim).lock().unwrap();
            }
            for p in spots {
                let Some(s) = world.peek_block(p) else { continue };
                let b = reg.block_of_state(s);
                // A gravity block with nothing below.
                if gravity(&b.name) && world.peek_block(BlockPos::new(p.x, p.y - 1, p.z)).is_some_and(free) && dim.in_bounds(p.y - 1) {
                    drop_block(server, &mut world, dim, p, &mut spawns, &mut changes);
                    continue;
                }
                // A stalactite whose support went: the whole of it falls.
                if b.name == "pointed_dripstone" && b.get(s, "vertical_direction") == Some("down") {
                    // Held by the block above (or by the rest of the stalactite).
                    let above = BlockPos::new(p.x, p.y + 1, p.z);
                    let held = world.peek_block(above).is_some_and(|a| !free(a));
                    if !held {
                        let mut q = p;
                        while hangs_down(&world, q) {
                            drop_block(server, &mut world, dim, q, &mut spawns, &mut changes);
                            q = BlockPos::new(q.x, q.y - 1, q.z);
                        }
                    }
                }
            }
        }
        for p in &spawns {
            server.broadcast_dim(dim, p, None);
        }
        for p in changes {
            server.broadcast_dim(dim, &super::blocks::block_change(p, 0), None);
        }
    }
    step_all(server);
}

fn step_all(server: &Arc<Server>) {
    let reg = data::get();
    let mut landed: Vec<Falling> = Vec::new();
    let mut moves: Vec<(Dimension, Vec<u8>)> = Vec::new();
    {
        let mut list = FALLING.lock().unwrap();
        if list.is_empty() {
            return;
        }
        let mut keep = Vec::new();
        for mut f in list.drain(..) {
            f.age += 1;
            f.vel[1] = (f.vel[1] - 0.04) * 0.98;
            let d = {
                let mut world = server.world(f.dim).lock().unwrap();
                move_aabb(&mut world, &Aabb::entity(f.pos[0], f.pos[1], f.pos[2], 0.98, 0.98), f.vel)
            };
            for i in 0..3 {
                f.pos[i] += d[i];
            }
            let hit_ground = d[1] > f.vel[1] + 1e-9;
            if hit_ground || f.age > 600 || f.pos[1] < f.dim.min_y() as f64 - 10.0 {
                landed.push(f);
                continue;
            }
            if (0..3).any(|i| (f.pos[i] - f.sent[i]).abs() > 0.01) {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(f.eid).f64(f.pos[0]).f64(f.pos[1]).f64(f.pos[2]).u8(0).u8(0).bool(false);
                moves.push((f.dim, p));
                f.sent = f.pos;
            }
            keep.push(f);
        }
        *list = keep;
    }
    for (dim, p) in moves {
        server.broadcast_dim(dim, &p, None);
    }
    for f in landed {
        land(server, f, reg);
    }
}

fn land(server: &Arc<Server>, f: Falling, reg: &crate::data::Registries) {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(f.eid);
    server.broadcast_dim(f.dim, &d, None);
    let name = reg.block_of_state(f.state).name.as_str();
    let fell = (f.start_y - f.pos[1]).max(0.0);
    // Anvils and stalactites hurt what they land on.
    let hurts = name.ends_with("anvil") || name == "pointed_dripstone";
    if hurts && fell > 1.0 {
        let (per, max) = if name == "pointed_dripstone" { (2.0, 40.0) } else { (2.0, 40.0) };
        let dmg = ((fell - 1.0).ceil() * per).min(max) as f32;
        let bx = Aabb::entity(f.pos[0], f.pos[1], f.pos[2], 0.98, 0.98).grow(0.1, 0.5, 0.1);
        for p in server.online().into_iter().filter(|p| p.dim() == f.dim) {
            let at = {
                let s = p.state.lock().unwrap();
                Aabb::entity(s.x, s.y, s.z, 0.6, 1.8)
            };
            if at.intersects(&bx) {
                let kind = if name == "pointed_dripstone" { "falling_stalactite" } else { "falling_anvil" };
                super::health::damage(server, &p, dmg, kind, None);
            }
        }
        let hit: Vec<i32> = super::mobs::hit_boxes(server).into_iter().filter(|(_, (dim, b))| *dim == f.dim && b.intersects(&bx)).map(|(e, _)| e).collect();
        for e in hit {
            super::mobs::hurt(server, e, dmg, "falling_block", None, None, 0);
        }
    }
    let at = BlockPos::new(f.pos[0].floor() as i32, (f.pos[1] + 0.2).floor() as i32, f.pos[2].floor() as i32);
    let c = [at.x as f64 + 0.5, at.y as f64 + 0.5, at.z as f64 + 0.5];
    if name == "pointed_dripstone" {
        // It shatters.
        sound::at(server, f.dim, c, "block.pointed_dripstone.land", sound::BLOCK, 2.0, 1.0);
        if let Some(i) = reg.item_id("pointed_dripstone") {
            entities::spawn(server, f.dim, c, [0.0, 0.1, 0.0], ItemStack::new(i, 1), 10);
        }
        return;
    }
    // Concrete powder sets in water.
    let mut state = f.state;
    let placed = {
        let mut world = server.world(f.dim).lock().unwrap();
        let here = world.get_block(at);
        if free(here) {
            if name.ends_with("_concrete_powder") && (reg.block_of_state(here).name == "water" || touches_water(&mut world, at)) {
                state = reg.state(&name.replace("_concrete_powder", "_concrete"));
            }
            world.set_block(at, state);
            true
        } else {
            false
        }
    };
    if placed {
        server.broadcast_dim(f.dim, &super::blocks::block_change(at, state), None);
        if name.ends_with("anvil") {
            sound::at(server, f.dim, c, "block.anvil.land", sound::BLOCK, 1.0, 1.0);
        }
    } else if let Some(item) = reg.item_id(name) {
        // No room: it drops as an item.
        entities::spawn(server, f.dim, c, [0.0, 0.1, 0.0], ItemStack::new(item, 1), 10);
    }
}

fn touches_water(world: &mut World, p: BlockPos) -> bool {
    let reg = data::get();
    [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)].iter().any(|(dx, dy, dz)| reg.block_of_state(world.get_block(BlockPos::new(p.x + dx, p.y + dy, p.z + dz))).name == "water")
}

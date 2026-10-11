//! Boats and minecarts: placing, mounting and dismounting, breaking by hitting, and moving them.
//!
//! The boat is moved by the client driving it (`vehicle_move`, like vanilla) and the server
//! only validates and relays. Minecarts are moved by the server along rails with
//! AbstractMinecart physics (curves, slopes, powered rails on and off).

use super::{entities, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::physics::{move_aabb, Aabb};
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VehicleKind {
    /// Boat (wood variant, with chest).
    Boat(i32, bool),
    Minecart,
}

const BOAT_WOODS: [&str; 9] = ["oak", "spruce", "birch", "jungle", "acacia", "cherry", "dark_oak", "mangrove", "bamboo"];

impl VehicleKind {
    pub fn from_item(name: &str) -> Option<Self> {
        if name == "minecart" {
            return Some(VehicleKind::Minecart);
        }
        let (wood, chest) = match name {
            "bamboo_raft" => ("bamboo", false),
            "bamboo_chest_raft" => ("bamboo", true),
            n => match n.strip_suffix("_chest_boat") {
                Some(w) => (w, true),
                None => (n.strip_suffix("_boat")?, false),
            },
        };
        BOAT_WOODS.iter().position(|w| *w == wood).map(|i| VehicleKind::Boat(i as i32, chest))
    }

    fn item(self) -> String {
        match self {
            VehicleKind::Minecart => "minecart".into(),
            VehicleKind::Boat(8, false) => "bamboo_raft".into(),
            VehicleKind::Boat(8, true) => "bamboo_chest_raft".into(),
            VehicleKind::Boat(v, chest) => format!("{}_{}boat", BOAT_WOODS[v as usize], if chest { "chest_" } else { "" }),
        }
    }

    fn entity(self) -> &'static str {
        match self {
            VehicleKind::Minecart => "minecart",
            VehicleKind::Boat(_, true) => "chest_boat",
            VehicleKind::Boat(..) => "boat",
        }
    }

    fn size(self) -> (f64, f64) {
        match self {
            VehicleKind::Minecart => (0.98, 0.7),
            VehicleKind::Boat(..) => (1.375, 0.5625),
        }
    }

    /// Seat height above the vehicle's feet.
    fn seat(self) -> f64 {
        match self {
            VehicleKind::Minecart => 0.0,
            VehicleKind::Boat(..) => -0.1,
        }
    }
}

#[derive(Clone)]
pub struct Vehicle {
    pub eid: i32,
    uuid: u128,
    pub dim: Dimension,
    pub kind: VehicleKind,
    pub pos: [f64; 3],
    vel: [f64; 3],
    yaw: f32,
    pub passenger: Option<i32>,
    damage: f32,
    hurt_dir: i32,
    /// Passenger input (forward > 0).
    forward: f32,
    paddles: (bool, bool),
    on_ground: bool,
    sent: [f64; 3],
    sent_yaw: f32,
}

// ---------- packets ----------

fn metadata(v: &Vehicle) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(v.eid);
    // 8: hurt time, 9: hurt direction, 10: damage.
    p.u8(8).varint(1).varint(if v.damage > 0.0 { 10 } else { 0 });
    p.u8(9).varint(1).varint(v.hurt_dir);
    p.u8(10).varint(3).f32(v.damage);
    if let VehicleKind::Boat(variant, _) = v.kind {
        p.u8(11).varint(1).varint(variant);
        p.u8(12).varint(8).bool(v.paddles.0);
        p.u8(13).varint(8).bool(v.paddles.1);
    }
    p.u8(0xff);
    p
}

fn passengers_packet(v: &Vehicle) -> Vec<u8> {
    let mut p = packet(id::play::SET_PASSENGERS);
    p.varint(v.eid);
    match v.passenger {
        Some(e) => p.varint(1).varint(e),
        None => p.varint(0),
    };
    p
}

fn spawn_packets(v: &Vehicle) -> Vec<Vec<u8>> {
    let mut p = packet(id::play::SPAWN_ENTITY);
    p.varint(v.eid).uuid(v.uuid).varint(data::get().entity(v.kind.entity()));
    p.f64(v.pos[0]).f64(v.pos[1]).f64(v.pos[2]).angle(0.0).angle(v.yaw).angle(v.yaw).varint(0);
    p.i16(0).i16(0).i16(0);
    let mut out = vec![p, metadata(v)];
    if v.passenger.is_some() {
        out.push(passengers_packet(v));
    }
    out
}

pub fn positions(server: &Server) -> Vec<(i32, [f64; 3], Dimension)> {
    server.vehicles.lock().unwrap().values().map(|v| (v.eid, v.pos, v.dim)).collect()
}

pub fn spawn_packets_for(server: &Server, eids: &[i32]) -> HashMap<i32, Vec<Vec<u8>>> {
    if eids.is_empty() {
        return HashMap::new();
    }
    let vs = server.vehicles.lock().unwrap();
    eids.iter().filter_map(|e| vs.get(e).map(|v| (*e, spawn_packets(v)))).collect()
}

/// `set_passengers` for vehicles related to the entities that just spawned
/// for a player (must be sent after both vehicle and passenger exist).
pub fn passenger_packets_for(server: &Server, added: &[i32]) -> Vec<Vec<u8>> {
    if added.is_empty() {
        return vec![];
    }
    let vs = server.vehicles.lock().unwrap();
    vs.values().filter(|v| v.passenger.is_some_and(|p| added.contains(&p)) || (v.passenger.is_some() && added.contains(&v.eid))).map(passengers_packet).collect()
}

// ---------- placing ----------

pub fn spawn(server: &Server, dim: Dimension, kind: VehicleKind, pos: [f64; 3], yaw: f32) -> i32 {
    let r = || entities::random().to_bits() as u128;
    let v = Vehicle {
        eid: server.next_entity_id(),
        uuid: (r() << 96) | (r() << 64) | (r() << 32) | r(),
        dim,
        kind,
        pos,
        vel: [0.0; 3],
        yaw,
        passenger: None,
        damage: 0.0,
        hurt_dir: 1,
        forward: 0.0,
        paddles: (false, false),
        on_ground: false,
        sent: pos,
        sent_yaw: yaw,
    };
    let eid = v.eid;
    server.vehicles.lock().unwrap().insert(eid, v);
    eid
}

fn consume_held(player: &Player, hand: i32) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode != 1 {
        let h = st.held_slot;
        let slot = st.inv.consume_held(hand, h, 1, None);
        st.sync_slot(player, slot);
    }
}

pub fn is_rail(name: &str) -> bool {
    matches!(name, "rail" | "powered_rail" | "detector_rail" | "activator_rail")
}

/// Minecart on a clicked rail.
pub fn place_minecart(server: &Arc<Server>, player: &Arc<Player>, rail: BlockPos, hand: i32) -> bool {
    let dim = player.dim();
    let name = data::get().block_of_state(server.world(dim).lock().unwrap().get_block(rail)).name.clone();
    if !is_rail(&name) {
        return false;
    }
    let yaw = player.state.lock().unwrap().yaw;
    spawn(server, dim, VehicleKind::Minecart, [rail.x as f64 + 0.5, rail.y as f64 + 0.0625, rail.z as f64 + 0.5], yaw);
    consume_held(player, hand);
    true
}

/// Using a boat: it is placed on the water (or ground) being looked at.
pub fn use_boat(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (item, eye, yaw, pitch) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.eye(), st.yaw, st.pitch)
    };
    let Some(kind) = item.as_ref().and_then(|i| VehicleKind::from_item(i.name())).filter(|k| *k != VehicleKind::Minecart) else { return false };
    let dim = player.dim();
    let hit = {
        let mut world = server.world(dim).lock().unwrap();
        super::fluids::raycast(&mut world, eye, yaw, pitch, false).map(|(c, _)| {
            let water = data::get().block_of_state(world.get_block(c)).name == "water";
            (c, water)
        })
    };
    let Some((c, water)) = hit else { return true };
    let y = if water { c.y as f64 + 0.9 } else { c.y as f64 + 1.0 };
    spawn(server, dim, kind, [c.x as f64 + 0.5, y, c.z as f64 + 0.5], yaw);
    consume_held(player, hand);
    true
}

// ---------- mount, dismount, break ----------

/// Right click on a vehicle: mount it.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32) -> bool {
    let (sneaking, ppos) = {
        let st = player.state.lock().unwrap();
        (st.sneaking, [st.x, st.y, st.z])
    };
    let packet_out = {
        let mut vs = server.vehicles.lock().unwrap();
        let Some(v) = vs.get_mut(&eid) else { return false };
        if sneaking || v.passenger.is_some() || v.dim != player.dim() || dist2(&v.pos, &ppos) > 6.0 * 6.0 {
            return true;
        }
        v.passenger = Some(player.eid);
        v.forward = 0.0;
        passengers_packet(v)
    };
    {
        let mut st = player.state.lock().unwrap();
        st.vehicle = Some(eid);
        st.sleeping = None;
    }
    server.broadcast_tracked(eid, &packet_out, false);
    player.send(packet_out);
    true
}

/// Dismount (sneaking, dying, disconnecting, teleporting).
pub fn dismount(server: &Arc<Server>, player: &Arc<Player>, teleport: bool) {
    let Some(eid) = player.state.lock().unwrap().vehicle.take() else { return };
    let out = {
        let mut vs = server.vehicles.lock().unwrap();
        vs.get_mut(&eid).filter(|v| v.passenger == Some(player.eid)).map(|v| {
            v.passenger = None;
            v.forward = 0.0;
            (passengers_packet(v), v.pos, v.kind)
        })
    };
    let Some((p, pos, kind)) = out else { return };
    server.broadcast_tracked(eid, &p, false);
    player.send(p);
    if teleport {
        let (_, h) = kind.size();
        super::play::teleport(server, player, pos[0], pos[1] + h + 0.05, pos[2]);
    }
}

/// A player hits a vehicle. Returns whether it was one.
pub fn attacked(server: &Arc<Server>, player: &Arc<Player>, eid: i32) -> bool {
    let creative = player.state.lock().unwrap().game_mode == 1;
    let broken = {
        let mut vs = server.vehicles.lock().unwrap();
        let Some(v) = vs.get_mut(&eid) else { return false };
        v.hurt_dir = -v.hurt_dir;
        v.damage += 10.0;
        server.broadcast_tracked(eid, &metadata(v), true);
        if creative || v.damage > 40.0 {
            vs.remove(&eid)
        } else {
            None
        }
    };
    if let Some(v) = broken {
        if let Some(p) = v.passenger.and_then(|p| server.players.read().unwrap().get(&p).cloned()) {
            p.state.lock().unwrap().vehicle = None;
            server.broadcast_tracked(eid, &passengers_packet(&Vehicle { passenger: None, ..v.clone() }), true);
            p.send(passengers_packet(&Vehicle { passenger: None, ..v.clone() }));
        }
        let mut d = packet(id::play::ENTITY_DESTROY);
        d.varint(1).varint(eid);
        for p in server.online() {
            if p.tracked.lock().unwrap().remove(&eid) {
                p.send(d.clone());
            }
        }
        if !creative {
            if let Some(item) = data::get().item_id(&v.kind.item()) {
                entities::spawn(server, v.dim, [v.pos[0], v.pos[1] + 0.3, v.pos[2]], [0.0, 0.2, 0.0], ItemStack::new(item, 1), 10);
            }
        }
    }
    true
}

fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

// ---------- driver input ----------

/// `vehicle_move`: the client driving a boat reports where it is.
pub fn client_moved(server: &Arc<Server>, player: &Arc<Player>, pos: [f64; 3], yaw: f32) {
    let Some(eid) = player.state.lock().unwrap().vehicle else { return };
    if !pos.iter().all(|v| v.is_finite()) {
        return;
    }
    let ok = {
        let mut vs = server.vehicles.lock().unwrap();
        let Some(v) = vs.get_mut(&eid).filter(|v| matches!(v.kind, VehicleKind::Boat(..)) && v.passenger == Some(player.eid)) else { return };
        // Like vanilla: more than 10 blocks in one packet is impossible.
        if dist2(&v.pos, &pos) > 100.0 {
            false
        } else {
            v.pos = pos;
            v.yaw = yaw;
            true
        }
    };
    if !ok {
        let v = server.vehicles.lock().unwrap().get(&eid).cloned();
        if let Some(v) = v {
            let mut p = packet(id::play::VEHICLE_MOVE);
            p.f64(v.pos[0]).f64(v.pos[1]).f64(v.pos[2]).f32(v.yaw).f32(0.0);
            player.send(p);
        }
        return;
    }
    follow(player, pos, VehicleKind::Boat(0, false));
}

/// The player goes where its vehicle goes (for chunks, tracking and anti-cheat).
fn follow(player: &Player, pos: [f64; 3], kind: VehicleKind) {
    let mut st = player.state.lock().unwrap();
    let p = [pos[0], pos[1] + kind.seat(), pos[2]];
    (st.x, st.y, st.z) = (p[0], p[1], p[2]);
    st.fall_distance = 0.0;
    st.ac.last_valid = p;
    st.ac.last_ground = p;
}

/// `steer_vehicle`: forward/backward and dismount (flag 0x02).
pub fn steer(server: &Arc<Server>, player: &Arc<Player>, forward: f32, flags: u8) {
    let Some(eid) = player.state.lock().unwrap().vehicle else { return };
    if flags & 0x02 != 0 {
        dismount(server, player, true);
        return;
    }
    if let Some(v) = server.vehicles.lock().unwrap().get_mut(&eid) {
        v.forward = forward;
    }
}

/// `steer_boat`: paddles (animation only, for others).
pub fn paddles(server: &Arc<Server>, player: &Arc<Player>, left: bool, right: bool) {
    let Some(eid) = player.state.lock().unwrap().vehicle else { return };
    let meta = {
        let mut vs = server.vehicles.lock().unwrap();
        let Some(v) = vs.get_mut(&eid) else { return };
        if v.paddles == (left, right) {
            return;
        }
        v.paddles = (left, right);
        metadata(v)
    };
    server.broadcast_tracked(eid, &meta, false);
}

// ---------- physics ----------

/// Rail: exits (dx, dy, dz) of each shape (AbstractMinecart.EXITS).
fn exits(shape: &str) -> ((i32, i32, i32), (i32, i32, i32)) {
    match shape {
        "east_west" => ((-1, 0, 0), (1, 0, 0)),
        "ascending_east" => ((-1, -1, 0), (1, 0, 0)),
        "ascending_west" => ((-1, 0, 0), (1, -1, 0)),
        "ascending_north" => ((0, 0, -1), (0, -1, 1)),
        "ascending_south" => ((0, -1, -1), (0, 0, 1)),
        "south_east" => ((0, 0, 1), (1, 0, 0)),
        "south_west" => ((0, 0, 1), (-1, 0, 0)),
        "north_west" => ((0, 0, -1), (-1, 0, 0)),
        "north_east" => ((0, 0, -1), (1, 0, 0)),
        _ => ((0, 0, -1), (0, 0, 1)),
    }
}

/// (shape, powered, is booster?) of the rail at `p`.
fn rail_at(world: &mut World, p: BlockPos) -> Option<(String, bool, bool)> {
    let reg = data::get();
    let s = world.get_block(p);
    let b = reg.block_of_state(s);
    if !is_rail(&b.name) {
        return None;
    }
    let shape = b.get(s, "shape").unwrap_or("north_south").to_string();
    Some((shape, b.get(s, "powered") == Some("true"), b.name == "powered_rail"))
}

/// Track point for a position (AbstractMinecart.getPos).
fn track_pos(world: &mut World, x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
    let (i, mut j, k) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    if rail_at(world, BlockPos::new(i, j - 1, k)).is_some() {
        j -= 1;
    }
    let (shape, _, _) = rail_at(world, BlockPos::new(i, j, k))?;
    let (a, b) = exits(&shape);
    let d0 = i as f64 + 0.5 + a.0 as f64 * 0.5;
    let d1 = j as f64 + 0.0625 + a.1 as f64 * 0.5;
    let d2 = k as f64 + 0.5 + a.2 as f64 * 0.5;
    let d3 = i as f64 + 0.5 + b.0 as f64 * 0.5;
    let d4 = j as f64 + 0.0625 + b.1 as f64 * 0.5;
    let d5 = k as f64 + 0.5 + b.2 as f64 * 0.5;
    let (d6, d7, d8) = (d3 - d0, (d4 - d1) * 2.0, d5 - d2);
    let t = if d6 == 0.0 {
        z - k as f64
    } else if d8 == 0.0 {
        x - i as f64
    } else {
        ((x - d0) * d6 + (z - d2) * d8) * 2.0
    };
    let mut out = [d0 + d6 * t, d1 + d7 * t, d2 + d8 * t];
    if d7 < 0.0 {
        out[1] += 1.0;
    } else if d7 > 0.0 {
        out[1] += 0.5;
    }
    Some(out)
}

fn solid(world: &mut World, p: BlockPos) -> bool {
    let s = world.get_block(p);
    crate::gameplay::get().shape(s).len() == 1 && crate::gameplay::get().shape(s)[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
}

fn hlen(v: &[f64; 3]) -> f64 {
    (v[0] * v[0] + v[2] * v[2]).sqrt()
}

/// One minecart tick (AbstractMinecart.tick + moveAlongTrack).
fn tick_minecart(world: &mut World, v: &mut Vehicle, rider_yaw: Option<f32>) {
    let (w, h) = v.kind.size();
    let in_water = data::get().block_of_state(world.get_block(BlockPos::new(v.pos[0].floor() as i32, v.pos[1].floor() as i32, v.pos[2].floor() as i32))).name == "water";
    v.vel[1] -= if in_water { 0.005 } else { 0.04 };
    let (bx, mut by, bz) = (v.pos[0].floor() as i32, v.pos[1].floor() as i32, v.pos[2].floor() as i32);
    if rail_at(world, BlockPos::new(bx, by - 1, bz)).is_some() {
        by -= 1;
    }
    let block = BlockPos::new(bx, by, bz);
    let max_speed = if in_water { 0.2 } else { 0.4 };
    let before = v.pos;
    match rail_at(world, block) {
        Some((shape, powered, is_powered_rail)) => {
            let start = track_pos(world, v.pos[0], v.pos[1], v.pos[2]);
            let accel = is_powered_rail && powered;
            let mut brake = is_powered_rail && !powered;
            let mut d3 = 0.0078125;
            if in_water {
                d3 *= 0.2;
            }
            let mut y = block.y as f64;
            match shape.as_str() {
                "ascending_east" => {
                    v.vel[0] -= d3;
                    y += 1.0;
                }
                "ascending_west" => {
                    v.vel[0] += d3;
                    y += 1.0;
                }
                "ascending_north" => {
                    v.vel[2] += d3;
                    y += 1.0;
                }
                "ascending_south" => {
                    v.vel[2] -= d3;
                    y += 1.0;
                }
                _ => {}
            }
            let (a, b) = exits(&shape);
            let (mut dx, mut dz) = ((b.0 - a.0) as f64, (b.2 - a.2) as f64);
            let len = (dx * dx + dz * dz).sqrt();
            if v.vel[0] * dx + v.vel[2] * dz < 0.0 {
                dx = -dx;
                dz = -dz;
            }
            let speed = hlen(&v.vel).min(2.0);
            v.vel = [speed * dx / len, v.vel[1], speed * dz / len];
            // The passenger pushes towards where it is looking if moving slowly.
            if let Some(yaw) = rider_yaw.filter(|_| v.forward > 0.0) {
                let (s, c) = (yaw.to_radians() as f64).sin_cos();
                if v.vel[0] * v.vel[0] + v.vel[2] * v.vel[2] < 0.01 {
                    v.vel[0] += -s * 0.1;
                    v.vel[2] += c * 0.1;
                    brake = false;
                }
            }
            if brake {
                if hlen(&v.vel) < 0.03 {
                    v.vel = [0.0; 3];
                } else {
                    v.vel[0] *= 0.5;
                    v.vel[2] *= 0.5;
                }
            }
            let (ax, az) = (block.x as f64 + 0.5 + a.0 as f64 * 0.5, block.z as f64 + 0.5 + a.2 as f64 * 0.5);
            let (bx2, bz2) = (block.x as f64 + 0.5 + b.0 as f64 * 0.5, block.z as f64 + 0.5 + b.2 as f64 * 0.5);
            let (ex, ez) = (bx2 - ax, bz2 - az);
            let t = if ex == 0.0 {
                v.pos[2] - block.z as f64
            } else if ez == 0.0 {
                v.pos[0] - block.x as f64
            } else {
                ((v.pos[0] - ax) * ex + (v.pos[2] - az) * ez) * 2.0
            };
            v.pos = [ax + ex * t, y, az + ez * t];
            let k = if v.passenger.is_some() { 0.75 } else { 1.0 };
            let step = [(k * v.vel[0]).clamp(-max_speed, max_speed), 0.0, (k * v.vel[2]).clamp(-max_speed, max_speed)];
            let d = move_aabb(world, &Aabb::entity(v.pos[0], v.pos[1], v.pos[2], w, h), step);
            for i in 0..3 {
                v.pos[i] += d[i];
            }
            let (fx, fz) = (v.pos[0].floor() as i32 - block.x, v.pos[2].floor() as i32 - block.z);
            if a.1 != 0 && fx == a.0 && fz == a.2 {
                v.pos[1] += a.1 as f64;
            } else if b.1 != 0 && fx == b.0 && fz == b.2 {
                v.pos[1] += b.1 as f64;
            }
            // Friction.
            let f = if v.passenger.is_some() { 0.997 } else { 0.96 };
            v.vel = [v.vel[0] * f, 0.0, v.vel[2] * f];
            if let (Some(s0), Some(s1)) = (start, track_pos(world, v.pos[0], v.pos[1], v.pos[2])) {
                let dy = (s0[1] - s1[1]) * 0.05;
                let l = hlen(&v.vel);
                if l > 0.0 {
                    v.vel[0] *= (l + dy) / l;
                    v.vel[2] *= (l + dy) / l;
                }
                v.pos[1] = s1[1];
            }
            let (nx, nz) = (v.pos[0].floor() as i32, v.pos[2].floor() as i32);
            if nx != block.x || nz != block.z {
                let l = hlen(&v.vel);
                v.vel = [l * (nx - block.x) as f64, v.vel[1], l * (nz - block.z) as f64];
            }
            if accel {
                let l = hlen(&v.vel);
                if l > 0.01 {
                    v.vel[0] += v.vel[0] / l * 0.06;
                    v.vel[2] += v.vel[2] / l * 0.06;
                } else if shape == "east_west" {
                    if solid(world, BlockPos::new(block.x - 1, block.y, block.z)) {
                        v.vel[0] = 0.02;
                    } else if solid(world, BlockPos::new(block.x + 1, block.y, block.z)) {
                        v.vel[0] = -0.02;
                    }
                } else if shape == "north_south" {
                    if solid(world, BlockPos::new(block.x, block.y, block.z - 1)) {
                        v.vel[2] = 0.02;
                    } else if solid(world, BlockPos::new(block.x, block.y, block.z + 1)) {
                        v.vel[2] = -0.02;
                    }
                }
            }
            v.on_ground = true;
        }
        None => {
            v.vel[0] = v.vel[0].clamp(-max_speed, max_speed);
            v.vel[2] = v.vel[2].clamp(-max_speed, max_speed);
            if v.on_ground {
                v.vel[0] *= 0.5;
                v.vel[1] *= 0.5;
                v.vel[2] *= 0.5;
            }
            let want = v.vel;
            let d = move_aabb(world, &Aabb::entity(v.pos[0], v.pos[1], v.pos[2], w, h), v.vel);
            for i in 0..3 {
                v.pos[i] += d[i];
            }
            v.on_ground = want[1] < 0.0 && d[1] > want[1] + 1e-9;
            if v.on_ground {
                v.vel[1] = 0.0;
            } else {
                for x in &mut v.vel {
                    *x *= 0.95;
                }
            }
        }
    }
    let (mx, mz) = (before[0] - v.pos[0], before[2] - v.pos[2]);
    if mx * mx + mz * mz > 0.001 {
        v.yaw = (mz.atan2(mx).to_degrees()) as f32;
    }
}

/// Driverless boat: floats on water and falls outside it.
fn tick_boat(world: &mut World, v: &mut Vehicle) {
    let (w, h) = v.kind.size();
    let reg = data::get();
    let here = BlockPos::new(v.pos[0].floor() as i32, (v.pos[1] + 0.1).floor() as i32, v.pos[2].floor() as i32);
    let water = reg.block_of_state(world.get_block(here)).name == "water";
    if water {
        let surface = here.y as f64 + 0.9;
        v.vel[1] = ((surface - v.pos[1]) * 0.2).clamp(-0.05, 0.08);
        v.vel[0] *= 0.9;
        v.vel[2] *= 0.9;
    } else {
        v.vel[1] -= 0.04;
        v.vel[0] *= 0.5;
        v.vel[2] *= 0.5;
    }
    let want = v.vel;
    let d = move_aabb(world, &Aabb::entity(v.pos[0], v.pos[1], v.pos[2], w, h), v.vel);
    for i in 0..3 {
        v.pos[i] += d[i];
    }
    v.on_ground = want[1] < 0.0 && d[1] > want[1] + 1e-9;
    if (d[1] - want[1]).abs() > 1e-9 {
        v.vel[1] = 0.0;
    }
}

pub fn tick(server: &Arc<Server>) {
    let riders: HashMap<i32, Arc<Player>> = server.online().into_iter().map(|p| (p.eid, p)).collect();
    let mut packets: Vec<(i32, Vec<u8>, bool)> = Vec::new();
    let mut follows: Vec<(Arc<Player>, [f64; 3], VehicleKind)> = Vec::new();
    let mut lost: Vec<i32> = Vec::new();
    {
        let mut vs = server.vehicles.lock().unwrap();
        if vs.is_empty() {
            return;
        }
        for dim in Dimension::ALL {
            if !vs.values().any(|v| v.dim == dim) {
                continue;
            }
            let mut world = server.world(dim).lock().unwrap();
            for v in vs.values_mut().filter(|v| v.dim == dim) {
                if v.damage > 0.0 {
                    v.damage -= 1.0;
                }
                // Passenger disconnected or in another dimension.
                let rider = v.passenger.and_then(|p| riders.get(&p)).filter(|p| p.dim() == dim).cloned();
                if v.passenger.is_some() && rider.is_none() {
                    v.passenger = None;
                    lost.push(v.eid);
                }
                let driven = matches!(v.kind, VehicleKind::Boat(..)) && rider.is_some();
                if !driven {
                    match v.kind {
                        VehicleKind::Minecart => {
                            let yaw = rider.as_ref().map(|r| r.state.lock().unwrap().yaw);
                            tick_minecart(&mut world, v, yaw);
                        }
                        VehicleKind::Boat(..) => tick_boat(&mut world, v),
                    }
                }
                if v.pos[1] < dim.min_y() as f64 - 64.0 {
                    lost.push(-v.eid);
                    continue;
                }
                let moved = (0..3).any(|i| (v.pos[i] - v.sent[i]).abs() > 0.001) || (v.yaw - v.sent_yaw).abs() > 1.0;
                if moved {
                    let mut p = packet(id::play::ENTITY_TELEPORT);
                    p.varint(v.eid).f64(v.pos[0]).f64(v.pos[1]).f64(v.pos[2]).angle(v.yaw).angle(0.0).bool(v.on_ground);
                    // The boat is not resent to whoever is driving it.
                    packets.push((v.eid, p, !driven));
                    v.sent = v.pos;
                    v.sent_yaw = v.yaw;
                    if let Some(r) = &rider {
                        if !driven {
                            follows.push((r.clone(), v.pos, v.kind));
                        }
                    }
                }
            }
        }
        for e in &lost {
            if *e < 0 {
                vs.remove(&-e);
            }
        }
    }
    for e in lost {
        if e > 0 {
            if let Some(v) = server.vehicles.lock().unwrap().get(&e).cloned() {
                server.broadcast_tracked(e, &passengers_packet(&v), false);
            }
        }
    }
    for (eid, p, include_rider) in packets {
        if include_rider {
            server.broadcast_tracked(eid, &p, false);
        } else {
            let rider = server.vehicles.lock().unwrap().get(&eid).and_then(|v| v.passenger);
            for pl in server.online() {
                if Some(pl.eid) != rider && pl.tracked.lock().unwrap().contains(&eid) {
                    pl.send(p.clone());
                }
            }
        }
    }
    for (p, pos, kind) in follows {
        follow(&p, pos, kind);
    }
}

// ---------- persistence ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    item: String,
    dim: String,
    pos: [f64; 3],
    yaw: f32,
}

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("vehicles.json")
}

pub fn save(server: &Server) {
    let saved: Vec<Saved> = server.vehicles.lock().unwrap().values().map(|v| Saved { item: v.kind.item(), dim: v.dim.name().into(), pos: v.pos, yaw: v.yaw }).collect();
    if let Err(e) = std::fs::write(file(server), serde_json::to_string(&saved).unwrap()) {
        tracing::warn!("no se pudieron guardar los vehículos: {e}");
    }
}

pub fn load(server: &Server) {
    let Ok(text) = std::fs::read_to_string(file(server)) else { return };
    let Ok(saved) = serde_json::from_str::<Vec<Saved>>(&text) else { return };
    for s in saved {
        let Some(kind) = VehicleKind::from_item(&s.item) else { continue };
        let dim = Dimension::ALL.into_iter().find(|d| d.name() == s.dim).unwrap_or(Dimension::Overworld);
        spawn(server, dim, kind, s.pos, s.yaw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_and_kinds() {
        assert_eq!(VehicleKind::from_item("oak_boat"), Some(VehicleKind::Boat(0, false)));
        assert_eq!(VehicleKind::from_item("dark_oak_chest_boat"), Some(VehicleKind::Boat(6, true)));
        assert_eq!(VehicleKind::from_item("bamboo_raft"), Some(VehicleKind::Boat(8, false)));
        assert_eq!(VehicleKind::from_item("minecart"), Some(VehicleKind::Minecart));
        assert_eq!(VehicleKind::Boat(6, true).item(), "dark_oak_chest_boat");
        for k in [VehicleKind::Boat(0, false), VehicleKind::Boat(1, true), VehicleKind::Minecart] {
            assert!(data::get().item_id(&k.item()).is_some(), "{}", k.item());
            assert!(data::get().entity(k.entity()) > 0);
        }
    }

    #[test]
    fn minecart_rolls_along_rails() {
        let mut world = World::new_flat_for_tests();
        let reg = data::get();
        let rail = reg.block("rail").unwrap();
        let powered = reg.block("powered_rail").unwrap();
        for x in 0..20 {
            world.set_block(BlockPos::new(x, 64, 0), reg.state("stone"));
            let s = if x < 2 { powered.with(powered.with(powered.default_state, "shape", "east_west"), "powered", "true") } else { rail.with(rail.default_state, "shape", "east_west") };
            world.set_block(BlockPos::new(x, 65, 0), s);
        }
        let mut v = Vehicle {
            eid: 1,
            uuid: 1,
            dim: Dimension::Overworld,
            kind: VehicleKind::Minecart,
            pos: [0.5, 65.0625, 0.5],
            vel: [0.0; 3],
            yaw: 0.0,
            passenger: None,
            damage: 0.0,
            hurt_dir: 1,
            forward: 0.0,
            paddles: (false, false),
            on_ground: false,
            sent: [0.0; 3],
            sent_yaw: 0.0,
        };
        // Next to a solid block, the powered rail launches it.
        world.set_block(BlockPos::new(-1, 65, 0), reg.state("stone"));
        for _ in 0..40 {
            tick_minecart(&mut world, &mut v, None);
        }
        assert!(v.pos[0] > 5.0, "avanzó por la vía: {:?}", v.pos);
        assert!((v.pos[1] - 65.0625).abs() < 0.01, "sigue sobre la vía: {:?}", v.pos);
        assert!((v.pos[2] - 0.5).abs() < 0.01);
    }
}

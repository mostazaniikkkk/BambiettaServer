//! Dropped items and experience orbs: spawning, physics, merging, pickup and despawning.

use super::{Player, Server};
use crate::data;
use crate::item::{write_slot, ItemStack};
use crate::protocol::{id, packet, Writer};
use crate::world::physics::{move_aabb, Aabb};
use std::sync::atomic::{AtomicU64, Ordering};
use crate::world::Dimension;
use std::collections::HashMap;
use std::sync::Arc;

/// Ticks until an item despawns (5 minutes).
const DESPAWN: u32 = 6000;
const SIZE: f64 = 0.25;

pub struct XpOrb {
    pub eid: i32,
    pub dim: Dimension,
    pub value: i32,
    pub pos: [f64; 3],
    vel: [f64; 3],
    on_ground: bool,
    age: u32,
    sent: [f64; 3],
}

pub struct ItemEntity {
    pub eid: i32,
    pub dim: Dimension,
    uuid: u128,
    pub item: ItemStack,
    pub pos: [f64; 3],
    vel: [f64; 3],
    on_ground: bool,
    age: u32,
    pickup_delay: u32,
    sent: [f64; 3],
}

/// Simple global pseudo-random generator (no cryptographic quality needed).
pub fn random() -> f32 {
    static STATE: AtomicU64 = AtomicU64::new(0x2545_f491_4f6c_dd1d);
    let mut x = STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 40) as f32 / (1u64 << 24) as f32
}

fn spawn_packets(e: &ItemEntity) -> Vec<Vec<u8>> {
    let mut p = packet(id::play::SPAWN_ENTITY);
    p.varint(e.eid).uuid(e.uuid).varint(data::get().entity("item"));
    p.f64(e.pos[0]).f64(e.pos[1]).f64(e.pos[2]).u8(0).u8(0).u8(0).varint(1);
    for v in e.vel {
        p.i16((v.clamp(-3.9, 3.9) * 8000.0) as i16);
    }
    vec![p, metadata(e)]
}

fn metadata(e: &ItemEntity) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(e.eid).u8(8).varint(7);
    write_slot(&mut p, &Some(e.item.clone()));
    p.u8(0xff);
    p
}


/// Drops an item into the world.
pub fn spawn(server: &Server, dim: Dimension, pos: [f64; 3], vel: [f64; 3], item: ItemStack, pickup_delay: u32) {
    if item.count <= 0 {
        return;
    }
    let e = ItemEntity {
        eid: server.next_entity_id(),
        dim,
        uuid: ((random().to_bits() as u128) << 96) | ((random().to_bits() as u128) << 64) | (server.age.load(Ordering::Relaxed) as u128) << 1,
        item,
        pos,
        vel,
        on_ground: false,
        age: 0,
        pickup_delay,
        sent: pos,
    };
    tracing::debug!("ítem {} x{} en {:.2} {:.2} {:.2} (eid {})", e.item.name(), e.item.count, pos[0], pos[1], pos[2], e.eid);
    server.items.lock().unwrap().insert(e.eid, e);
}

/// Takes dropped items lying inside the box (a hopper sucking them in): `take` says how many
/// of each stack it can hold. Returns whether it took anything.
pub fn take_in_box(server: &Server, dim: Dimension, min: [f64; 3], max: [f64; 3], mut take: impl FnMut(&ItemStack) -> i8) -> bool {
    let mut items = server.items.lock().unwrap();
    let inside: Vec<i32> = items.values().filter(|e| e.dim == dim && (0..3).all(|i| e.pos[i] >= min[i] && e.pos[i] <= max[i])).map(|e| e.eid).collect();
    let mut took = false;
    for eid in inside {
        let Some(e) = items.get_mut(&eid) else { continue };
        let n = take(&e.item).min(e.item.count);
        if n <= 0 {
            continue;
        }
        took = true;
        e.item.count -= n;
        if e.item.count <= 0 {
            items.remove(&eid);
        } else {
            server.broadcast_tracked(eid, &metadata(e), true);
        }
    }
    took
}

/// Where the nearest dropped item matching `pred` lies (that can already be picked up).
pub fn nearest_item(server: &Server, dim: Dimension, pos: [f64; 3], radius: f64, pred: impl Fn(&ItemStack) -> bool) -> Option<[f64; 3]> {
    let d2 = |p: &[f64; 3]| (0..3).map(|i| (p[i] - pos[i]).powi(2)).sum::<f64>();
    server
        .items
        .lock()
        .unwrap()
        .values()
        .filter(|e| e.dim == dim && e.pickup_delay == 0 && d2(&e.pos) < radius * radius && pred(&e.item))
        .map(|e| e.pos)
        .min_by(|a, b| d2(a).total_cmp(&d2(b)))
}

/// Drops a broken block's loot at its center.
pub fn drop_at_block(server: &Server, dim: Dimension, x: i32, y: i32, z: i32, item: ItemStack) {
    let pos = [x as f64 + 0.5 + (random() as f64 - 0.5) * 0.5, y as f64 + 0.25, z as f64 + 0.5 + (random() as f64 - 0.5) * 0.5];
    let vel = [random() as f64 * 0.2 - 0.1, 0.2, random() as f64 * 0.2 - 0.1];
    spawn(server, dim, pos, vel, item, 10);
}

/// Throws an item from the player in the look direction (Q or outside the inventory).
pub fn throw_from(server: &Server, player: &Player, item: ItemStack) {
    let (pos, vel) = {
        let s = player.state.lock().unwrap();
        let (yaw, pitch) = (s.yaw.to_radians() as f64, s.pitch.to_radians() as f64);
        let eye = s.eye();
        let speed = 0.3;
        let vel = [
            -yaw.sin() * pitch.cos() * speed + (random() as f64 - 0.5) * 0.02,
            -pitch.sin() * speed + 0.1,
            yaw.cos() * pitch.cos() * speed + (random() as f64 - 0.5) * 0.02,
        ];
        ([eye[0], eye[1] - 0.3, eye[2]], vel)
    };
    spawn(server, player.dim(), pos, vel, item, 40);
}

/// One physics step: gravity, block collision, friction and bounce.
pub fn step(world: &mut crate::world::World, pos: &mut [f64; 3], vel: &mut [f64; 3], on_ground: &mut bool, size: f64, gravity: f64) {
    vel[1] -= gravity;
    // In water they float slowly upward.
    let here = crate::protocol::BlockPos::new(pos[0].floor() as i32, (pos[1] + size / 2.0).floor() as i32, pos[2].floor() as i32);
    if data::get().block_of_state(world.get_block(here)).name == "water" {
        vel[1] = (vel[1] + gravity + 0.01).min(0.06);
        vel[0] *= 0.99;
        vel[2] *= 0.99;
    }
    let aabb = Aabb::entity(pos[0], pos[1], pos[2], size, size);
    let d = move_aabb(world, &aabb, *vel);
    for i in 0..3 {
        pos[i] += d[i];
    }
    *on_ground = vel[1] < 0.0 && d[1] > vel[1] + 1e-9;
    for i in [0, 2] {
        if (d[i] - vel[i]).abs() > 1e-9 {
            vel[i] = 0.0;
        }
    }
    let friction = if *on_ground { 0.6 * 0.98 } else { 0.98 };
    vel[0] *= friction;
    vel[2] *= friction;
    vel[1] *= 0.98;
    if *on_ground {
        vel[1] *= -0.5;
        if vel[1].abs() < 0.01 {
            vel[1] = 0.0;
        }
    }
}

/// Spawn packets for the requested items and orbs (for per-player tracking).
pub fn spawn_packets_for(server: &Server, items: &[i32], orbs: &[i32]) -> HashMap<i32, Vec<Vec<u8>>> {
    let mut out = HashMap::new();
    if !items.is_empty() {
        let map = server.items.lock().unwrap();
        out.extend(items.iter().filter_map(|e| map.get(e).map(|i| (*e, spawn_packets(i)))));
    }
    if !orbs.is_empty() {
        let map = server.orbs.lock().unwrap();
        out.extend(orbs.iter().filter_map(|e| map.get(e).map(|o| (*e, vec![orb_spawn(o)]))));
    }
    out
}

fn orb_spawn(o: &XpOrb) -> Vec<u8> {
    let mut p = packet(id::play::SPAWN_XP_ORB);
    p.varint(o.eid).f64(o.pos[0]).f64(o.pos[1]).f64(o.pos[2]).i16(o.value as i16);
    p
}

/// Vanilla orb sizes: experience is split into orbs of these values.
fn orb_split(xp: i32) -> i32 {
    for v in [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3, 1] {
        if xp >= v {
            return v;
        }
    }
    1
}

/// Drops `xp` experience points at `pos`.
pub fn spawn_xp(server: &Server, dim: Dimension, pos: [f64; 3], mut xp: i32) {
    while xp > 0 {
        let value = orb_split(xp);
        xp -= value;
        let o = XpOrb {
            eid: server.next_entity_id(),
            dim,
            value,
            pos,
            vel: [(random() as f64 - 0.5) * 0.2, random() as f64 * 0.2, (random() as f64 - 0.5) * 0.2],
            on_ground: false,
            age: 0,
            sent: pos,
        };
        server.orbs.lock().unwrap().insert(o.eid, o);
    }
}

fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

/// Physics, attraction toward the nearby player and pickup of orbs.
fn tick_orbs(server: &Arc<Server>, age: i64) {
    let mut orbs = server.orbs.lock().unwrap();
    if orbs.is_empty() {
        return;
    }
    let players: Vec<(Arc<Player>, [f64; 3])> = server
        .online()
        .into_iter()
        .filter_map(|p| {
            let pos = {
                let s = p.state.lock().unwrap();
                (!s.dead && s.game_mode != 3).then_some([s.x, s.y, s.z])
            };
            pos.map(|pos| (p, pos))
        })
        .collect();
    let mut removed = Vec::new();
    for dim in Dimension::ALL {
        if !orbs.values().any(|o| o.dim == dim) {
            continue;
        }
        let mut world = server.world(dim).lock().unwrap();
        let players: Vec<&(Arc<Player>, [f64; 3])> = players.iter().filter(|(p, _)| p.dim() == dim).collect();
        for o in orbs.values_mut().filter(|o| o.dim == dim) {
            o.age += 1;
            if o.age >= DESPAWN || o.pos[1] < dim.min_y() as f64 - 64.0 {
                removed.push((o.eid, None));
                continue;
            }
            // Vanilla: orbs fly toward the nearest player within 8 blocks.
            if let Some((p, pp)) = players.iter().map(|x| (&x.0, &x.1)).min_by(|a, b| dist2(a.1, &o.pos).total_cmp(&dist2(b.1, &o.pos))) {
                let d = [pp[0] - o.pos[0], pp[1] + 0.9 - o.pos[1], pp[2] - o.pos[2]];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                if len < 8.0 && len > 1e-6 {
                    let f = (1.0 - len / 8.0).powi(2);
                    for i in 0..3 {
                        o.vel[i] += d[i] / len * f * 0.1;
                    }
                }
                let reach = Aabb::entity(pp[0], pp[1], pp[2], 0.6, 1.8).grow(1.0, 0.5, 1.0);
                if o.age > 10 && reach.intersects(&Aabb::entity(o.pos[0], o.pos[1], o.pos[2], 0.5, 0.5)) {
                    removed.push((o.eid, Some((p.clone(), o.value))));
                    continue;
                }
            }
            step(&mut world, &mut o.pos, &mut o.vel, &mut o.on_ground, 0.5, 0.03);
        }
    }
    for (eid, picker) in removed {
        orbs.remove(&eid);
        if let Some((p, value)) = picker {
            let mut c = packet(id::play::COLLECT);
            c.varint(eid).varint(p.eid).varint(1);
            server.broadcast_tracked(eid, &c, true);
            super::experience::pickup_orb(&p, value);
        }
    }
    if age % 2 == 0 {
        for o in orbs.values_mut() {
            if (0..3).any(|i| (o.pos[i] - o.sent[i]).abs() > 1.0 / 64.0) {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(o.eid).f64(o.pos[0]).f64(o.pos[1]).f64(o.pos[2]).u8(0).u8(0).bool(o.on_ground);
                server.broadcast_tracked(o.eid, &p, true);
                o.sent = o.pos;
            }
        }
    }
}

pub fn tick(server: &Arc<Server>, age: i64) {
    tick_orbs(server, age);
    let mut items = server.items.lock().unwrap();
    if items.is_empty() {
        return;
    }
    let mut removed = Vec::new();
    for dim in Dimension::ALL {
        if !items.values().any(|e| e.dim == dim) {
            continue;
        }
        let mut world = server.world(dim).lock().unwrap();
        for e in items.values_mut().filter(|e| e.dim == dim) {
            e.age += 1;
            e.pickup_delay = e.pickup_delay.saturating_sub(1);
            if e.age >= DESPAWN || e.pos[1] < dim.min_y() as f64 - 64.0 {
                removed.push(e.eid);
                continue;
            }
            // Vanilla ItemEntity physics: gravity, collision, friction and bounce.
            let resting = e.on_ground && e.vel.iter().all(|v| v.abs() < 1e-3);
            if resting && e.age % 20 != 0 {
                continue;
            }
            step(&mut world, &mut e.pos, &mut e.vel, &mut e.on_ground, SIZE, 0.04);
        }
    }
    for eid in &removed {
        items.remove(eid);
    }

    // Merge nearby identical stacks (every 10 ticks).
    if age % 10 == 0 {
        let ids: Vec<i32> = items.keys().copied().collect();
        for (n, &a) in ids.iter().enumerate() {
            for &b in &ids[n + 1..] {
                let (Some(ea), Some(eb)) = (items.get(&a), items.get(&b)) else { continue };
                let close = (0..3).all(|i| (ea.pos[i] - eb.pos[i]).abs() < 0.5 + if i == 1 { 0.0 } else { 0.5 });
                if close && ea.dim == eb.dim && ea.item.stacks_with(&eb.item) && ea.item.count as i32 + eb.item.count as i32 <= ea.item.max_stack() as i32 {
                    let add = eb.item.count;
                    items.remove(&b);
                    let ea = items.get_mut(&a).unwrap();
                    ea.item.count += add;
                    ea.age = ea.age.min(DESPAWN / 2);
                    server.broadcast_tracked(ea.eid, &metadata(ea), true);
                }
            }
        }
    }

    // Pickup by living players who aren't spectators.
    for player in server.online() {
        let mut st = player.state.lock().unwrap();
        if st.dead || st.game_mode == 3 {
            continue;
        }
        let reach = Aabb::entity(st.x, st.y, st.z, 0.6, 1.8).grow(1.0, 0.5, 1.0);
        let near: Vec<i32> = items
            .values()
            .filter(|e| e.dim == player.dim() && e.pickup_delay == 0 && Aabb::entity(e.pos[0], e.pos[1], e.pos[2], SIZE, SIZE).intersects(&reach))
            .map(|e| e.eid)
            .collect();
        for eid in near {
            let e = items.get_mut(&eid).unwrap();
            let (left, changed) = st.inv.add(e.item.clone());
            let taken = e.item.count - left.as_ref().map_or(0, |l| l.count);
            if taken <= 0 {
                continue;
            }
            for slot in changed {
                st.sync_slot(&player, slot);
            }
            tracing::debug!("{} recoge {} x{taken}", player.name, e.item.name());
            let mut p = packet(id::play::COLLECT);
            p.varint(eid).varint(player.eid).varint(taken as i32);
            server.broadcast_tracked(eid, &p, true);
            match left {
                Some(l) => {
                    e.item = l;
                    server.broadcast_tracked(e.eid, &metadata(e), true);
                }
                None => {
                    items.remove(&eid);
                }
            }
        }
    }

    // Positions of those that moved.
    if age % 2 == 0 {
        for e in items.values_mut() {
            if (0..3).any(|i| (e.pos[i] - e.sent[i]).abs() > 1.0 / 64.0) {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(e.eid).f64(e.pos[0]).f64(e.pos[1]).f64(e.pos[2]).u8(0).u8(0).bool(e.on_ground);
                server.broadcast_tracked(e.eid, &p, true);
                e.sent = e.pos;
            }
        }
    }
}

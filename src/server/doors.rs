//! Zombies breaking wooden doors. A zombie whose path goes through a closed wooden door stops
//! at it and hits it: cracks appear, it bangs, and when the door gives way both halves are
//! gone. The damage belongs to the door, not to the zombie, so several zombies at the same
//! door break it faster; if they all leave it, it "heals". Iron and copper doors resist.
//! Time for a single zombie: 12 s on hard (like vanilla), 18 s on normal, 24 s on easy.

use super::mobs::{self, Action, Mob};
use super::{entities, sound, Server};
use crate::data;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::pathfind::wooden_door;
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::Mutex;

struct Breaking {
    progress: u32,
    /// Server tick of the last hit (to forget doors nobody is hitting).
    last: i64,
    /// Crack stage shown to the clients (0-9).
    stage: i8,
}

static DOORS: Mutex<Option<HashMap<(Dimension, BlockPos), Breaking>>> = Mutex::new(None);

fn break_time(difficulty: u8) -> u32 {
    match difficulty {
        0 | 1 => 480,
        2 => 360,
        _ => 240,
    }
}

/// Id used for the crack animation of a door (negative: never a real entity).
fn crack_id(p: BlockPos) -> i32 {
    -1 - ((p.x.wrapping_mul(31).wrapping_add(p.y)).wrapping_mul(31).wrapping_add(p.z)).rem_euclid(1 << 24)
}

fn crack(p: BlockPos, stage: i8) -> Vec<u8> {
    let mut pk = packet(id::play::BLOCK_BREAK_ANIMATION);
    pk.varint(crack_id(p)).position(p).i8(stage);
    pk
}

fn level_event(event: i32, p: BlockPos, data: i32) -> Vec<u8> {
    let mut ev = packet(id::play::WORLD_EVENT);
    ev.i32(event).position(p).i32(data).bool(false);
    ev
}

/// Lower half of a closed wooden door in the mob's way (on its next path step), if it is
/// right next to it.
pub fn in_the_way(world: &mut World, m: &Mob) -> Option<BlockPos> {
    let next = *m.path.front()?;
    let reg = data::get();
    for y in [next.y, next.y + 1] {
        let p = BlockPos::new(next.x, y, next.z);
        let state = world.get_block(p);
        let block = reg.block_of_state(state);
        if !wooden_door(&block.name) || block.get(state, "open") == Some("true") {
            continue;
        }
        let lower = if block.get(state, "half") == Some("upper") { BlockPos::new(p.x, p.y - 1, p.z) } else { p };
        let c = [lower.x as f64 + 0.5, m.pos[1], lower.z as f64 + 0.5];
        return (mobs::dist2(&c, &m.pos) < 1.7 * 1.7).then_some(lower);
    }
    None
}

/// A zombie hits the door at `door` (one tick of work).
pub fn hit(server: &Server, world: &mut World, m: &mut Mob, door: BlockPos, age: i64, actions: &mut Vec<Action>) {
    m.vel[0] = 0.0;
    m.vel[2] = 0.0;
    // Busy with the door: it does not forget who is behind it.
    m.unseen = 0;
    mobs::face(m, [door.x as f64 + 0.5, m.pos[1], door.z as f64 + 0.5]);
    let max = break_time(server.config.difficulty);
    let dim = m.dim;
    let (stage, broken) = {
        let mut map = DOORS.lock().unwrap();
        let b = map.get_or_insert_with(HashMap::new).entry((dim, door)).or_insert(Breaking { progress: 0, last: age, stage: -1 });
        b.progress += 1;
        b.last = age;
        let stage = (b.progress * 10 / max).min(9) as i8;
        let changed = (stage != b.stage).then_some(stage);
        b.stage = stage;
        let broken = b.progress >= max;
        if broken {
            map.as_mut().unwrap().remove(&(dim, door));
        }
        (changed, broken)
    };
    // Banging on it now and then (like vanilla: about once a second per zombie).
    if entities::random() < 1.0 / 20.0 {
        actions.push(Action::Sound(m.eid, "entity.zombie.attack_wooden_door".into(), sound::HOSTILE));
        let mut swing = packet(id::play::ANIMATION);
        swing.varint(m.eid).u8(0);
        actions.push(Action::DimPacket(dim, swing));
    }
    if let Some(s) = stage {
        actions.push(Action::DimPacket(dim, crack(door, s)));
    }
    if broken {
        let state = world.get_block(door);
        let upper = BlockPos::new(door.x, door.y + 1, door.z);
        for p in [door, upper] {
            if wooden_door(&data::get().block_of_state(world.get_block(p)).name) {
                world.set_block(p, 0);
                actions.push(Action::DimPacket(dim, super::blocks::block_change(p, 0)));
            }
        }
        actions.push(Action::DimPacket(dim, crack(door, -1)));
        // 1021: a zombie breaks a wooden door (sound); 2001: the block's particles.
        actions.push(Action::DimPacket(dim, level_event(1021, door, 0)));
        actions.push(Action::DimPacket(dim, level_event(2001, door, state as i32)));
    }
}

/// Doors nobody has hit for 2 s lose their damage.
pub fn decay(server: &Server, age: i64) {
    let stale: Vec<(Dimension, BlockPos)> = {
        let mut map = DOORS.lock().unwrap();
        let Some(m) = map.as_mut() else { return };
        let stale: Vec<_> = m.iter().filter(|(_, b)| age - b.last > 40).map(|(k, _)| *k).collect();
        for k in &stale {
            m.remove(k);
        }
        stale
    };
    for (dim, p) in stale {
        server.broadcast_dim(dim, &crack(p, -1), None);
    }
}

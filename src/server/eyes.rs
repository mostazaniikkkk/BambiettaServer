//! Eyes of ender: used in the air in the Overworld, the eye flies off towards the nearest
//! stronghold (rising, at most 12 blocks per throw), floats for a moment and then drops to
//! be picked up again (or shatters, one time in five), like vanilla.

use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use std::sync::{Arc, Mutex};

struct Eye {
    eid: i32,
    pos: [f64; 3],
    target: [f64; 3],
    age: u32,
}

static EYES: Mutex<Vec<Eye>> = Mutex::new(Vec::new());

/// Using an eye of ender in the air. Returns whether it was one (and was thrown).
pub fn throw(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (item, eye, creative) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.eye(), st.game_mode == 1)
    };
    if item.as_ref().map(|i| i.name()) != Some("ender_eye") || player.dim() != Dimension::Overworld {
        return false;
    }
    let strongholds = server.world(Dimension::Overworld).lock().unwrap().generator().strongholds();
    let Some(&(sx, sz)) = strongholds.iter().min_by_key(|(x, z)| {
        let (dx, dz) = (*x as f64 - eye[0], *z as f64 - eye[2]);
        (dx * dx + dz * dz) as i64
    }) else {
        return false;
    };
    // Towards it, at most 12 blocks away, and 8 up.
    let (dx, dz) = (sx as f64 - eye[0], sz as f64 - eye[2]);
    let d = (dx * dx + dz * dz).sqrt().max(1e-6);
    let reach = d.min(12.0);
    let target = [eye[0] + dx / d * reach, eye[1] + 8.0, eye[2] + dz / d * reach];
    if !creative {
        let mut st = player.state.lock().unwrap();
        let h = st.held_slot;
        let slot = st.inv.consume_held(hand, h, 1, None);
        st.sync_slot(player, slot);
    }
    let eid = server.next_entity_id();
    let mut p = packet(id::play::SPAWN_ENTITY);
    let r = || entities::random().to_bits() as u128;
    p.varint(eid).uuid((r() << 96) | (r() << 64) | (r() << 32) | r()).varint(data::get().entity("eye_of_ender"));
    p.f64(eye[0]).f64(eye[1] - 0.1).f64(eye[2]).angle(0.0).angle(0.0).angle(0.0).varint(0);
    p.i16(0).i16(0).i16(0);
    server.broadcast_dim(Dimension::Overworld, &p, None);
    sound::at(server, Dimension::Overworld, eye, "entity.ender_eye.launch", sound::NEUTRAL, 0.5, 0.4);
    EYES.lock().unwrap().push(Eye { eid, pos: [eye[0], eye[1] - 0.1, eye[2]], target, age: 0 });
    true
}

/// Every tick: the eyes fly, and after 4 s they drop or shatter.
pub fn tick(server: &Arc<Server>) {
    let mut done = Vec::new();
    let mut moves = Vec::new();
    {
        let mut eyes = EYES.lock().unwrap();
        for e in eyes.iter_mut() {
            e.age += 1;
            // Glides towards the target, slowing down as it gets there.
            for i in 0..3 {
                let step = (e.target[i] - e.pos[i]) * 0.06;
                e.pos[i] += step.clamp(-0.4, 0.4);
            }
            moves.push((e.eid, e.pos));
        }
        eyes.retain(|e| {
            if e.age >= 80 {
                done.push((e.eid, e.pos));
                false
            } else {
                true
            }
        });
    }
    for (eid, pos) in moves {
        let mut p = packet(id::play::ENTITY_TELEPORT);
        p.varint(eid).f64(pos[0]).f64(pos[1]).f64(pos[2]).angle(0.0).angle(0.0).bool(false);
        server.broadcast_dim(Dimension::Overworld, &p, None);
    }
    for (eid, pos) in done {
        let mut d = packet(id::play::ENTITY_DESTROY);
        d.varint(1).varint(eid);
        server.broadcast_dim(Dimension::Overworld, &d, None);
        if entities::random() < 0.8 {
            if let Some(id) = data::get().item_id("ender_eye") {
                entities::spawn(server, Dimension::Overworld, pos, [0.0, 0.0, 0.0], ItemStack::new(id, 1), 10);
            }
        } else {
            // Shatters: particles (level event 2003) and sound.
            let mut ev = packet(id::play::WORLD_EVENT);
            ev.i32(2003).position(BlockPos::new(pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32)).i32(0).bool(false);
            server.broadcast_dim(Dimension::Overworld, &ev, None);
            sound::at(server, Dimension::Overworld, pos, "entity.ender_eye.death", sound::NEUTRAL, 1.0, 1.0);
        }
    }
}

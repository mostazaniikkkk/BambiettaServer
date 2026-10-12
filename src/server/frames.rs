//! Item frames: hung on the face of a block (placing an item frame), they hold an item
//! (right click to put one in, again to turn it, hit to take it out, hit an empty one to
//! take the frame down). End ships carry one with an elytra. Saved in `frames.json`.

use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Frame {
    eid: i32,
    dim: Dimension,
    pos: BlockPos,
    /// Direction it faces (0 down, 1 up, 2 north, 3 south, 4 west, 5 east).
    facing: i32,
    item: Option<ItemStack>,
    rotation: i32,
}

static FRAMES: Mutex<Vec<Frame>> = Mutex::new(Vec::new());

fn spawn_packets(f: &Frame) -> Vec<Vec<u8>> {
    let mut p = packet(id::play::SPAWN_ENTITY);
    p.varint(f.eid).uuid(0x4652_414d_0000_0000_0000_0000_0000_0000 | f.eid as u32 as u128).varint(data::get().entity("item_frame"));
    p.f64(f.pos.x as f64 + 0.5).f64(f.pos.y as f64 + 0.5).f64(f.pos.z as f64 + 0.5).angle(0.0).angle(0.0).angle(0.0).varint(f.facing);
    p.i16(0).i16(0).i16(0);
    vec![p, metadata(f)]
}

fn metadata(f: &Frame) -> Vec<u8> {
    let mut m = packet(id::play::ENTITY_METADATA);
    m.varint(f.eid).u8(8).varint(7);
    crate::item::write_slot(&mut m, &f.item);
    m.u8(9).varint(1).varint(f.rotation);
    m.u8(0xff);
    m
}

fn destroy(eid: i32) -> Vec<u8> {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(eid);
    d
}

/// A frame with an item (a structure's, or one placed by a player).
pub fn add(server: &Server, dim: Dimension, pos: BlockPos, facing: i32, item: Option<ItemStack>) {
    let mut frames = FRAMES.lock().unwrap();
    if frames.iter().any(|f| f.dim == dim && f.pos == pos && f.facing == facing) {
        return;
    }
    let f = Frame { eid: server.next_entity_id(), dim, pos, facing, item, rotation: 0 };
    for p in spawn_packets(&f) {
        server.broadcast_dim(dim, &p, None);
    }
    frames.push(f);
}

/// The frames of a dimension (joining, changing dimension).
pub fn send_all(player: &Player) {
    let dim = player.dim();
    for f in FRAMES.lock().unwrap().iter().filter(|f| f.dim == dim) {
        for p in spawn_packets(f) {
            player.send(p);
        }
    }
}

/// Placing an item frame against a block face.
pub fn place(server: &Arc<Server>, player: &Arc<Player>, against: BlockPos, face: i32, hand: i32) -> bool {
    let at = against.offset(face);
    let free = data::get().is_air(server.world(player.dim()).lock().unwrap().get_block(at));
    if !free {
        return true;
    }
    add(server, player.dim(), at, face, None);
    let mut st = player.state.lock().unwrap();
    if st.game_mode != 1 {
        let h = st.held_slot;
        let slot = st.inv.consume_held(hand, h, 1, None);
        st.sync_slot(player, slot);
    }
    drop(st);
    sound::at(server, player.dim(), [at.x as f64 + 0.5, at.y as f64 + 0.5, at.z as f64 + 0.5], "entity.item_frame.place", sound::BLOCK, 1.0, 1.0);
    true
}

/// Right click: put the held item in, or turn the one inside.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32) -> bool {
    let held = {
        let st = player.state.lock().unwrap();
        st.inv.held(hand, st.held_slot).clone()
    };
    let (meta, placed, at, dim) = {
        let mut frames = FRAMES.lock().unwrap();
        let Some(f) = frames.iter_mut().find(|f| f.eid == eid) else { return false };
        let placed = if f.item.is_none() {
            let Some(h) = held else { return true };
            f.item = Some(h.with_count(1));
            true
        } else {
            f.rotation = (f.rotation + 1) % 8;
            false
        };
        (metadata(f), placed, f.pos, f.dim)
    };
    server.broadcast_dim(dim, &meta, None);
    let c = [at.x as f64 + 0.5, at.y as f64 + 0.5, at.z as f64 + 0.5];
    if placed {
        let mut st = player.state.lock().unwrap();
        if st.game_mode != 1 {
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
        drop(st);
        sound::at(server, dim, c, "entity.item_frame.add_item", sound::BLOCK, 1.0, 1.0);
    } else {
        sound::at(server, dim, c, "entity.item_frame.rotate_item", sound::BLOCK, 1.0, 1.0);
    }
    true
}

/// A hit: the item comes out; an empty frame comes down.
pub fn attacked(server: &Arc<Server>, player: &Arc<Player>, eid: i32) -> bool {
    let creative = player.state.lock().unwrap().game_mode == 1;
    let (drop, gone, dim, at) = {
        let mut frames = FRAMES.lock().unwrap();
        let Some(i) = frames.iter().position(|f| f.eid == eid) else { return false };
        let f = &mut frames[i];
        let at = [f.pos.x as f64 + 0.5, f.pos.y as f64 + 0.5, f.pos.z as f64 + 0.5];
        let dim = f.dim;
        if let Some(item) = f.item.take() {
            f.rotation = 0;
            let meta = metadata(f);
            server.broadcast_dim(dim, &meta, None);
            (Some(item), false, dim, at)
        } else {
            frames.remove(i);
            let frame = (!creative).then(|| data::get().item_id("item_frame").map(|i| ItemStack::new(i, 1))).flatten();
            (frame, true, dim, at)
        }
    };
    if gone {
        server.broadcast_dim(dim, &destroy(eid), None);
        sound::at(server, dim, at, "entity.item_frame.break", sound::BLOCK, 1.0, 1.0);
    } else {
        sound::at(server, dim, at, "entity.item_frame.remove_item", sound::BLOCK, 1.0, 1.0);
    }
    if let Some(item) = drop {
        entities::spawn(server, dim, at, [0.0, 0.1, 0.0], item, 10);
    }
    true
}

/// The block it hangs on was broken (or the frame's own space filled): it falls.
pub fn block_changed(server: &Server, dim: Dimension, pos: BlockPos) {
    let fallen: Vec<Frame> = {
        let mut frames = FRAMES.lock().unwrap();
        let (gone, keep): (Vec<Frame>, Vec<Frame>) = frames.drain(..).partition(|f| f.dim == dim && f.pos.offset(opposite(f.facing)) == pos);
        *frames = keep;
        gone
    };
    for f in fallen {
        server.broadcast_dim(dim, &destroy(f.eid), None);
        let at = [f.pos.x as f64 + 0.5, f.pos.y as f64 + 0.5, f.pos.z as f64 + 0.5];
        if let Some(i) = data::get().item_id("item_frame") {
            entities::spawn(server, dim, at, [0.0, 0.1, 0.0], ItemStack::new(i, 1), 10);
        }
        if let Some(item) = f.item {
            entities::spawn(server, dim, at, [0.0, 0.1, 0.0], item, 10);
        }
    }
}

fn opposite(face: i32) -> i32 {
    face ^ 1
}

// ---------- saving ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    dim: String,
    pos: [i32; 3],
    facing: i32,
    item: Option<String>,
    #[serde(default)]
    rotation: i32,
}

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("frames.json")
}

pub fn save(server: &Server) {
    let saved: Vec<Saved> = FRAMES
        .lock()
        .unwrap()
        .iter()
        .map(|f| Saved { dim: f.dim.name().into(), pos: [f.pos.x, f.pos.y, f.pos.z], facing: f.facing, item: f.item.as_ref().map(|i| i.name().to_string()), rotation: f.rotation })
        .collect();
    if !saved.is_empty() {
        let _ = std::fs::write(file(server), serde_json::to_string(&saved).unwrap_or_default());
    }
}

pub fn load(server: &Server) {
    let Ok(text) = std::fs::read_to_string(file(server)) else { return };
    let Ok(saved) = serde_json::from_str::<Vec<Saved>>(&text) else { return };
    let mut frames = FRAMES.lock().unwrap();
    for s in saved {
        let dim = Dimension::ALL.into_iter().find(|d| d.name() == s.dim).unwrap_or(Dimension::Overworld);
        let item = s.item.and_then(|n| data::get().item_id(&n)).map(|i| ItemStack::new(i, 1));
        frames.push(Frame { eid: server.next_entity_id(), dim, pos: BlockPos::new(s.pos[0], s.pos[1], s.pos[2]), facing: s.facing, item, rotation: s.rotation });
    }
}

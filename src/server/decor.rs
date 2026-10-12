//! Armor stands and paintings.
//! - An armor stand is placed on the top of a block, facing the player. Right click with a
//!   piece of armor (or any item for its hand) dresses it; right click with an empty hand
//!   takes back what is in the slot you point at (head, chest, legs, feet, hand). Two quick
//!   hits knock it down (it drops, with what it wore).
//! - A painting is hung on a wall: the biggest that fits there (picked at random among
//!   those). A hit takes it down; it falls if its wall goes.
//!
//! Both are saved in `decor.json`.

use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use std::sync::{Arc, Mutex};

/// Paintings (`painting_variant` registry order): (name, width, height).
const VARIANTS: [(&str, i32, i32); 30] = [
    ("kebab", 1, 1),
    ("aztec", 1, 1),
    ("alban", 1, 1),
    ("aztec2", 1, 1),
    ("bomb", 1, 1),
    ("plant", 1, 1),
    ("wasteland", 1, 1),
    ("pool", 2, 1),
    ("courbet", 2, 1),
    ("sea", 2, 1),
    ("sunset", 2, 1),
    ("creebet", 2, 1),
    ("wanderer", 1, 2),
    ("graham", 1, 2),
    ("match", 2, 2),
    ("bust", 2, 2),
    ("stage", 2, 2),
    ("void", 2, 2),
    ("skull_and_roses", 2, 2),
    ("wither", 2, 2),
    ("fighters", 4, 2),
    ("pointer", 4, 4),
    ("pigscene", 4, 4),
    ("burning_skull", 4, 4),
    ("skeleton", 4, 3),
    ("earth", 2, 2),
    ("wind", 2, 2),
    ("water", 2, 2),
    ("fire", 2, 2),
    ("donkey_kong", 4, 3),
];

#[derive(Clone)]
enum Kind {
    /// Equipment: main hand, off hand, feet, legs, chest, head (the protocol's slots 0-5).
    Stand { yaw: f32, gear: [Slot; 6], hit_at: i64 },
    Painting { facing: i32, variant: usize },
}

#[derive(Clone)]
struct Decor {
    eid: i32,
    dim: Dimension,
    /// Stand: feet position. Painting: the block it hangs from (its anchor).
    pos: [f64; 3],
    kind: Kind,
}

static DECOR: Mutex<Vec<Decor>> = Mutex::new(Vec::new());

fn uuid(eid: i32) -> u128 {
    0x4445_434f_0000_0000_0000_0000_0000_0000 | eid as u32 as u128
}

fn spawn_packets(d: &Decor) -> Vec<Vec<u8>> {
    let reg = data::get();
    let mut out = Vec::new();
    match &d.kind {
        Kind::Stand { yaw, gear, .. } => {
            let mut p = packet(id::play::SPAWN_ENTITY);
            p.varint(d.eid).uuid(uuid(d.eid)).varint(reg.entity("armor_stand"));
            p.f64(d.pos[0]).f64(d.pos[1]).f64(d.pos[2]).angle(0.0).angle(*yaw).angle(*yaw).varint(0);
            p.i16(0).i16(0).i16(0);
            out.push(p);
            // 15: client flags (0x04 show arms).
            let mut m = packet(id::play::ENTITY_METADATA);
            m.varint(d.eid).u8(15).varint(0).i8(0x04).u8(0xff);
            out.push(m);
            out.push(equipment(d.eid, gear));
        }
        Kind::Painting { facing, variant } => {
            let mut p = packet(id::play::SPAWN_ENTITY);
            p.varint(d.eid).uuid(uuid(d.eid)).varint(reg.entity("painting"));
            p.f64(d.pos[0]).f64(d.pos[1]).f64(d.pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(*facing);
            p.i16(0).i16(0).i16(0);
            out.push(p);
            let mut m = packet(id::play::ENTITY_METADATA);
            m.varint(d.eid).u8(8).varint(24).varint(*variant as i32).u8(0xff);
            out.push(m);
        }
    }
    out
}

fn equipment(eid: i32, gear: &[Slot; 6]) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_EQUIPMENT);
    p.varint(eid);
    for (i, g) in gear.iter().enumerate() {
        p.u8(if i < 5 { i as u8 | 0x80 } else { i as u8 });
        crate::item::write_slot(&mut p, g);
    }
    p
}

fn destroy(eid: i32) -> Vec<u8> {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(eid);
    d
}

pub fn send_all(player: &Player) {
    let dim = player.dim();
    for d in DECOR.lock().unwrap().iter().filter(|d| d.dim == dim) {
        for p in spawn_packets(d) {
            player.send(p);
        }
    }
}

fn add(server: &Server, d: Decor) {
    for p in spawn_packets(&d) {
        server.broadcast_dim(d.dim, &p, None);
    }
    DECOR.lock().unwrap().push(d);
}

fn consume(player: &Player, hand: i32) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 {
        return;
    }
    let h = st.held_slot;
    let slot = st.inv.consume_held(hand, h, 1, None);
    st.sync_slot(player, slot);
}

/// Equipment slot for an item on a stand (armor by type, anything else in the hand).
fn slot_for(item: &ItemStack) -> usize {
    match super::inventory::armor_slot(item) {
        Some(5) => 5,
        Some(6) => 4,
        Some(7) => 3,
        Some(8) => 2,
        _ => 0,
    }
}

// ---------- placing ----------

/// Placing an armor stand (on top of the clicked block) or a painting (on its face).
pub fn place(server: &Arc<Server>, player: &Arc<Player>, item: &str, against: BlockPos, face: i32, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    match item {
        "armor_stand" => {
            let at = if face == 1 { BlockPos::new(against.x, against.y + 1, against.z) } else { against.offset(face) };
            let free = {
                let mut w = server.world(dim).lock().unwrap();
                reg.is_air(w.get_block(at)) && reg.is_air(w.get_block(BlockPos::new(at.x, at.y + 1, at.z)))
            };
            if !free {
                return true;
            }
            // Facing the player, in steps of 45 degrees.
            let yaw = player.state.lock().unwrap().yaw;
            let yaw = (((yaw + 180.0) / 45.0).round() * 45.0) % 360.0;
            add(server, Decor { eid: server.next_entity_id(), dim, pos: [at.x as f64 + 0.5, at.y as f64, at.z as f64 + 0.5], kind: Kind::Stand { yaw, gear: Default::default(), hit_at: 0 } });
            consume(player, hand);
            sound::at(server, dim, [at.x as f64 + 0.5, at.y as f64, at.z as f64 + 0.5], "entity.armor_stand.place", sound::BLOCK, 1.0, 1.0);
            true
        }
        "painting" => {
            if !(2..=5).contains(&face) {
                return true;
            }
            let anchor = against.offset(face);
            // Fits: the wall behind is solid and the space in front free, all over it.
            let fits = |w: &mut crate::world::World, width: i32, height: i32| {
                let (rx, rz) = match face {
                    2 => (-1, 0),
                    3 => (1, 0),
                    4 => (0, 1),
                    _ => (0, -1),
                };
                // Like vanilla, wide paintings extend to the left of the anchor.
                let off_w = (width - 1) / 2;
                let off_h = (height - 1) / 2;
                (0..width).all(|i| {
                    (0..height).all(|j| {
                        let k = i - off_w;
                        let p = BlockPos::new(anchor.x + rx * k, anchor.y + j - off_h, anchor.z + rz * k);
                        let wall = p.offset(face ^ 1);
                        reg.is_air(w.get_block(p)) && !crate::gameplay::get().shape(w.get_block(wall)).is_empty()
                    })
                })
            };
            let options: Vec<usize> = {
                let mut w = server.world(dim).lock().unwrap();
                let ok: Vec<usize> = (0..VARIANTS.len()).filter(|&i| fits(&mut w, VARIANTS[i].1, VARIANTS[i].2)).collect();
                let best = ok.iter().map(|&i| VARIANTS[i].1 * VARIANTS[i].2).max().unwrap_or(0);
                ok.into_iter().filter(|&i| VARIANTS[i].1 * VARIANTS[i].2 == best).collect()
            };
            if options.is_empty() {
                return true;
            }
            let variant = options[(entities::random() * options.len() as f32) as usize % options.len()];
            add(server, Decor { eid: server.next_entity_id(), dim, pos: [anchor.x as f64 + 0.5, anchor.y as f64 + 0.5, anchor.z as f64 + 0.5], kind: Kind::Painting { facing: face, variant } });
            consume(player, hand);
            sound::at(server, dim, [anchor.x as f64 + 0.5, anchor.y as f64 + 0.5, anchor.z as f64 + 0.5], "entity.painting.place", sound::BLOCK, 1.0, 1.0);
            true
        }
        _ => false,
    }
}

// ---------- using and breaking ----------

/// Right click on a stand: dress it, or take something back.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32) -> bool {
    let (held, eye_y, creative) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.eye()[1], st.game_mode == 1)
    };
    let give: Option<ItemStack>;
    let mut used = false;
    let packets = {
        let mut list = DECOR.lock().unwrap();
        let Some(d) = list.iter_mut().find(|d| d.eid == eid) else { return false };
        let Kind::Stand { gear, .. } = &mut d.kind else { return true };
        match &held {
            Some(h) => {
                let s = slot_for(h);
                give = gear[s].replace(h.with_count(1));
                used = true;
            }
            None => {
                // The slot at the height the player points at.
                let rel = (eye_y - d.pos[1]).clamp(0.0, 2.0);
                let s = if rel > 1.6 { 5 } else if rel > 1.2 { 4 } else if rel > 0.6 { 3 } else { 2 };
                let s = if gear[s].is_none() { 0 } else { s };
                give = gear[s].take();
            }
        }
        (equipment(eid, gear), d.dim)
    };
    server.broadcast_dim(packets.1, &packets.0, None);
    if used && !creative {
        consume(player, hand);
    }
    if let Some(g) = give {
        let left = {
            let mut st = player.state.lock().unwrap();
            let left = st.inv.add(g).0;
            let p = st.inv.window_items();
            player.send(p);
            left
        };
        if let Some(l) = left {
            entities::throw_from(server, player, l);
        }
    }
    true
}

/// A hit: a painting comes down; a stand wobbles, and a second hit soon after breaks it.
pub fn attacked(server: &Arc<Server>, player: &Arc<Player>, eid: i32) -> bool {
    let now = server.age.load(std::sync::atomic::Ordering::Relaxed);
    let creative = player.state.lock().unwrap().game_mode == 1;
    let broken = {
        let mut list = DECOR.lock().unwrap();
        let Some(i) = list.iter().position(|d| d.eid == eid) else { return false };
        let breaks = match &mut list[i].kind {
            Kind::Stand { hit_at, .. } => {
                let quick = now - *hit_at < 5 || creative;
                *hit_at = now;
                quick
            }
            Kind::Painting { .. } => true,
        };
        if !breaks {
            // The wobble.
            let mut s = packet(id::play::ENTITY_STATUS);
            s.i32(eid).i8(32);
            server.broadcast_dim(list[i].dim, &s, None);
            sound::at(server, list[i].dim, list[i].pos, "entity.armor_stand.hit", sound::BLOCK, 1.0, 1.0);
            return true;
        }
        list.remove(i)
    };
    drop_decor(server, broken, !creative);
    true
}

fn drop_decor(server: &Server, d: Decor, item: bool) {
    server.broadcast_dim(d.dim, &destroy(d.eid), None);
    let reg = data::get();
    let (name, snd) = match &d.kind {
        Kind::Stand { .. } => ("armor_stand", "entity.armor_stand.break"),
        Kind::Painting { .. } => ("painting", "entity.painting.break"),
    };
    sound::at(server, d.dim, d.pos, snd, sound::BLOCK, 1.0, 1.0);
    if item {
        if let Some(i) = reg.item_id(name) {
            entities::spawn(server, d.dim, d.pos, [0.0, 0.1, 0.0], ItemStack::new(i, 1), 10);
        }
    }
    if let Kind::Stand { gear, .. } = d.kind {
        for g in gear.into_iter().flatten() {
            entities::spawn(server, d.dim, d.pos, [0.0, 0.2, 0.0], g, 10);
        }
    }
}

/// A block broke: a painting hanging on it falls.
pub fn block_changed(server: &Server, dim: Dimension, pos: BlockPos) {
    let fallen: Vec<Decor> = {
        let mut list = DECOR.lock().unwrap();
        let (gone, keep): (Vec<Decor>, Vec<Decor>) = list.drain(..).partition(|d| {
            d.dim == dim
                && match d.kind {
                    Kind::Painting { facing, variant } => {
                        let anchor = BlockPos::new(d.pos[0].floor() as i32, d.pos[1].floor() as i32, d.pos[2].floor() as i32);
                        let (w, h) = (VARIANTS[variant].1, VARIANTS[variant].2);
                        let wall = anchor.offset(facing ^ 1);
                        (pos.y - wall.y).abs() <= h && (pos.x - wall.x).abs() + (pos.z - wall.z).abs() <= w && (pos.x == wall.x || pos.z == wall.z) && pos != anchor && {
                            // Only the wall plane behind it.
                            match facing {
                                2 | 3 => pos.z == wall.z,
                                _ => pos.x == wall.x,
                            }
                        }
                    }
                    _ => false,
                }
        });
        *list = keep;
        gone
    };
    for d in fallen {
        drop_decor(server, d, true);
    }
}

/// The blocks behind a painting that hold it up.
fn wall_of(anchor: BlockPos, facing: i32, variant: usize) -> Vec<BlockPos> {
    let (width, height) = (VARIANTS[variant].1, VARIANTS[variant].2);
    let (rx, rz) = match facing {
        2 => (-1, 0),
        3 => (1, 0),
        4 => (0, 1),
        _ => (0, -1),
    };
    let (off_w, off_h) = ((width - 1) / 2, (height - 1) / 2);
    let mut out = Vec::new();
    for i in 0..width {
        for j in 0..height {
            let k = i - off_w;
            out.push(BlockPos::new(anchor.x + rx * k, anchor.y + j - off_h, anchor.z + rz * k).offset(facing ^ 1));
        }
    }
    out
}

/// Every 5 seconds, like vanilla: paintings whose wall is gone (however it went) fall.
pub fn tick(server: &Server) {
    if server.age.load(std::sync::atomic::Ordering::Relaxed) % 100 != 0 {
        return;
    }
    let paintings: Vec<(i32, Dimension, Vec<BlockPos>)> = DECOR
        .lock()
        .unwrap()
        .iter()
        .filter_map(|d| match d.kind {
            Kind::Painting { facing, variant } => Some((d.eid, d.dim, wall_of(BlockPos::new(d.pos[0].floor() as i32, d.pos[1].floor() as i32, d.pos[2].floor() as i32), facing, variant))),
            _ => None,
        })
        .collect();
    let shapes = crate::gameplay::get();
    let fallen: Vec<i32> = paintings
        .into_iter()
        .filter(|(_, dim, wall)| {
            let world = server.world(*dim).lock().unwrap();
            // Unloaded walls are left alone.
            wall.iter().any(|p| world.peek_block(*p).is_some_and(|s| shapes.shape(s).is_empty()))
        })
        .map(|(eid, ..)| eid)
        .collect();
    if fallen.is_empty() {
        return;
    }
    let gone: Vec<Decor> = {
        let mut list = DECOR.lock().unwrap();
        let (gone, keep): (Vec<Decor>, Vec<Decor>) = list.drain(..).partition(|d| fallen.contains(&d.eid));
        *list = keep;
        gone
    };
    for d in gone {
        drop_decor(server, d, true);
    }
}

// ---------- saving ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    dim: String,
    pos: [f64; 3],
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    gear: Vec<crate::world::blockentity::SavedSlot>,
    #[serde(default)]
    painting: Option<(i32, usize)>,
}

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("decor.json")
}

pub fn save(server: &Server) {
    let saved: Vec<Saved> = DECOR
        .lock()
        .unwrap()
        .iter()
        .map(|d| match &d.kind {
            Kind::Stand { yaw, gear, .. } => Saved { dim: d.dim.name().into(), pos: d.pos, yaw: *yaw, gear: crate::world::blockentity::save_slots(gear), painting: None },
            Kind::Painting { facing, variant } => Saved { dim: d.dim.name().into(), pos: d.pos, yaw: 0.0, gear: vec![], painting: Some((*facing, *variant)) },
        })
        .collect();
    if !saved.is_empty() {
        let _ = std::fs::write(file(server), serde_json::to_string(&saved).unwrap_or_default());
    }
}

pub fn load(server: &Server) {
    let Ok(text) = std::fs::read_to_string(file(server)) else { return };
    let Ok(saved) = serde_json::from_str::<Vec<Saved>>(&text) else { return };
    let mut list = DECOR.lock().unwrap();
    for s in saved {
        let dim = Dimension::ALL.into_iter().find(|d| d.name() == s.dim).unwrap_or(Dimension::Overworld);
        let kind = match s.painting {
            Some((facing, variant)) => Kind::Painting { facing, variant: variant.min(VARIANTS.len() - 1) },
            None => {
                let mut gear: [Slot; 6] = Default::default();
                for (i, g) in crate::world::blockentity::load_slots(&s.gear, 6).into_iter().enumerate() {
                    gear[i] = g;
                }
                Kind::Stand { yaw: s.yaw, gear, hit_at: 0 }
            }
        };
        list.push(Decor { eid: server.next_entity_id(), dim, pos: s.pos, kind });
    }
}

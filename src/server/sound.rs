//! Sounds sent by the server (by name, without depending on registry ids).

use super::Server;
use crate::protocol::{id, packet, Writer};

/// Categories (`soundSource`).
pub const BLOCK: i32 = 4;
pub const HOSTILE: i32 = 5;
pub const NEUTRAL: i32 = 6;
pub const PLAYER: i32 = 7;

fn seed() -> i64 {
    (super::entities::random() * i32::MAX as f32) as i64
}

/// Sound at a position, for the players in that dimension.
pub fn at(server: &Server, dim: crate::world::Dimension, pos: [f64; 3], name: &str, category: i32, volume: f32, pitch: f32) {
    let mut p = packet(id::play::SOUND_EFFECT);
    // ItemSoundHolder: 0 = inline event (name + optional range).
    p.varint(0).string(&format!("minecraft:{name}")).bool(false);
    p.varint(category).i32((pos[0] * 8.0) as i32).i32((pos[1] * 8.0) as i32).i32((pos[2] * 8.0) as i32);
    p.f32(volume).f32(pitch).i64(seed());
    server.broadcast_dim(dim, &p, None);
}

/// Sound that follows an entity.
pub fn entity(server: &Server, eid: i32, name: &str, category: i32, volume: f32, pitch: f32) {
    let mut p = packet(id::play::ENTITY_SOUND_EFFECT);
    p.varint(0).string(&format!("minecraft:{name}")).bool(false);
    p.varint(category).varint(eid).f32(volume).f32(pitch).i64(seed());
    server.broadcast_tracked(eid, &p, true);
}

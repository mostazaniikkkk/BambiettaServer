//! Mob spawners (dungeons, strongholds, fortresses), like vanilla: one works while a player
//! is within 16 blocks; every 10-40 s it spawns up to 4 of its mob around it (within 4
//! blocks, where there is room), unless 6 of them are already close by.

use super::mobs::{self, MobKind};
use super::{entities, Server};
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::collections::HashSet;
use std::sync::Arc;

const RANGE: f64 = 16.0;

/// Every tick.
pub fn tick(server: &Arc<Server>) {
    let players: Vec<(Dimension, [f64; 3])> = server
        .online()
        .iter()
        .filter_map(|p| {
            let st = p.state.lock().unwrap();
            (!st.dead && st.game_mode != 3).then(|| (p.dim(), [st.x, st.y, st.z]))
        })
        .collect();
    for dim in Dimension::ALL {
        let here: Vec<[f64; 3]> = players.iter().filter(|p| p.0 == dim).map(|p| p.1).collect();
        if here.is_empty() {
            continue;
        }
        let mut spawns: Vec<(MobKind, [f64; 3], BlockPos)> = Vec::new();
        {
            let mut world = server.world(dim).lock().unwrap();
            let mut seen = HashSet::new();
            for p in &here {
                let (cx, cz) = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        for (pos, mob, delay) in world.spawners_in(cx + dx, cz + dz) {
                            let c = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
                            if !seen.insert(pos) || !here.iter().any(|q| mobs::dist2(q, &c) < RANGE * RANGE) {
                                continue;
                            }
                            if delay > 0 {
                                world.set_spawner_delay(pos, delay - 1);
                                continue;
                            }
                            world.set_spawner_delay(pos, 200 + (entities::random() * 600.0) as i32);
                            let Some(kind) = MobKind::from_name(&mob) else { continue };
                            for _ in 0..4 {
                                if let Some(at) = spot(&mut world, pos, kind) {
                                    spawns.push((kind, at, pos));
                                }
                            }
                        }
                    }
                }
            }
        }
        // Not if there are already 6 of that mob around the spawner.
        for (kind, at, pos) in spawns {
            let c = [pos.x as f64 + 0.5, pos.y as f64, pos.z as f64 + 0.5];
            let crowd = server.mobs.lock().unwrap().values().filter(|m| m.dim == dim && m.kind == kind && m.death_ticks.is_none() && mobs::dist2(&m.pos, &c) < 9.0 * 9.0).count();
            if crowd >= 6 {
                continue;
            }
            mobs::spawn(server, dim, kind, at);
            // Smoke and flames where it appears (level event 2004).
            let mut ev = packet(id::play::WORLD_EVENT);
            ev.i32(2004).position(BlockPos::new(at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32)).i32(0).bool(false);
            server.broadcast_dim(dim, &ev, None);
        }
    }
}

/// A random spot around the spawner where the mob fits (air for its height, ground below).
fn spot(world: &mut World, pos: BlockPos, kind: MobKind) -> Option<[f64; 3]> {
    let reg = crate::data::get();
    let g = crate::gameplay::get();
    let h = if matches!(kind, MobKind::Silverfish | MobKind::CaveSpider | MobKind::Spider) { 1 } else { 2 };
    for _ in 0..4 {
        let x = pos.x + (entities::random() * 9.0) as i32 - 4;
        let z = pos.z + (entities::random() * 9.0) as i32 - 4;
        let y = pos.y + (entities::random() * 3.0) as i32 - 1;
        let free = (0..h).all(|dy| g.shape(world.get_block(BlockPos::new(x, y + dy, z))).is_empty() && !reg.block_of_state(world.get_block(BlockPos::new(x, y + dy, z))).name.contains("lava"));
        let ground = !g.shape(world.get_block(BlockPos::new(x, y - 1, z))).is_empty();
        if free && ground && (x, y, z) != (pos.x, pos.y, pos.z) {
            return Some([x as f64 + 0.5, y as f64, z as f64 + 0.5]);
        }
    }
    None
}

//! Explosions (creepers): vanilla rays against block resistance, damage and
//! knockback by distance, loot from destroyed blocks and an `explosion` packet with particles and sound.

use super::health::Attacker;
use super::{entities, fluids, health, Server};
use crate::data;
use crate::gameplay;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use std::collections::HashSet;
use std::sync::Arc;

/// `explosion` and `explosion_emitter` particles (ids from the official 1.20.4 server report).
const PARTICLE_EXPLOSION: i32 = 23;
const PARTICLE_EMITTER: i32 = 22;

/// Blocks destroyed by an explosion of `power` at `center` (vanilla ray algorithm).
pub fn affected_blocks(world: &mut crate::world::World, center: [f64; 3], power: f32) -> Vec<BlockPos> {
    let reg = data::get();
    let g = gameplay::get();
    let mut out = HashSet::new();
    for i in 0..16 {
        for j in 0..16 {
            for k in 0..16 {
                if !(i == 0 || i == 15 || j == 0 || j == 15 || k == 0 || k == 15) {
                    continue;
                }
                let mut d = [i as f64 / 15.0 * 2.0 - 1.0, j as f64 / 15.0 * 2.0 - 1.0, k as f64 / 15.0 * 2.0 - 1.0];
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                for v in &mut d {
                    *v /= len;
                }
                let mut intensity = power * (0.7 + entities::random() * 0.6);
                let mut p = center;
                while intensity > 0.0 {
                    let bp = BlockPos::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
                    if !world.dim.in_bounds(bp.y) {
                        break;
                    }
                    let state = world.get_block(bp);
                    if !reg.is_air(state) {
                        let r = g.resistance(&reg.block_of_state(state).name);
                        intensity -= (r + 0.3) * 0.3;
                    }
                    if intensity > 0.0 && !reg.is_air(state) {
                        out.insert(bp);
                    }
                    for (a, b) in p.iter_mut().zip(d) {
                        *a += b * 0.3;
                    }
                    intensity -= 0.225;
                }
            }
        }
    }
    out.into_iter().collect()
}

pub fn explode(server: &Arc<Server>, dim: Dimension, center: [f64; 3], power: f32, source: Option<Attacker>) {
    explode_ex(server, dim, center, power, source, false);
}

/// Explosion; with `fire` it leaves fire in some of the gaps (ghast fireball, beds).
pub fn explode_ex(server: &Arc<Server>, dim: Dimension, center: [f64; 3], power: f32, source: Option<Attacker>, fire: bool) {
    let reg = data::get();
    tracing::debug!("explosión de potencia {power} en {:.1} {:.1} {:.1}", center[0], center[1], center[2]);
    // 1. Blocks.
    let (blocks, contents, tnt) = {
        let mut world = server.world(dim).lock().unwrap();
        let blocks = affected_blocks(&mut world, center, power);
        let mut contents = Vec::new();
        let mut destroyed = Vec::new();
        let mut tnt = Vec::new();
        for &p in &blocks {
            let state = world.get_block(p);
            let block = reg.block_of_state(state);
            if block.name == "water" || block.name == "lava" {
                continue;
            }
            // Any TNT hit is lit with a short fuse (done afterwards).
            if block.name == "tnt" {
                tnt.push(p);
                continue;
            }
            if let Some(be) = world.take_block_entity(p) {
                contents.extend(be.contents().into_iter().map(|i| (p, i)));
            }
            // Vanilla: blocks destroyed by a creeper drop with probability 1/power.
            if entities::random() < 1.0 / power {
                for (item, n) in gameplay::get().drops(block, state, None, &mut entities::random) {
                    contents.push((p, ItemStack::new(item, n)));
                }
            }
            world.set_block(p, 0);
            destroyed.push(p);
        }
        (destroyed, contents, tnt)
    };
    for p in tnt {
        super::redstone::prime_tnt(server, dim, p, 10 + (entities::random() * 20.0) as i32);
    }
    for p in &blocks {
        server.broadcast_dim(dim, &super::blocks::block_change(*p, 0), None);
        fluids::neighbor_changed(server, dim, *p);
    }
    for (p, item) in contents {
        entities::drop_at_block(server, dim, p.x, p.y, p.z, item);
    }
    if fire {
        for p in &blocks {
            if entities::random() < 1.0 / 3.0 {
                super::fire::ignite(server, dim, *p);
            }
        }
    }

    // 2. Entities within 2*power: damage and knockback.
    let reach = power as f64 * 2.0;
    let impact = |pos: [f64; 3]| {
        let d = ((pos[0] - center[0]).powi(2) + (pos[1] - center[1]).powi(2) + (pos[2] - center[2]).powi(2)).sqrt();
        (d < reach).then(|| {
            let i = 1.0 - d / reach;
            let dir = if d > 1e-6 { [(pos[0] - center[0]) / d, (pos[1] - center[1]) / d, (pos[2] - center[2]) / d] } else { [0.0, 1.0, 0.0] };
            (i, dir, ((i * i + i) / 2.0 * 7.0 * reach + 1.0) as f32)
        })
    };
    for player in server.online().into_iter().filter(|p| p.dim() == dim) {
        let pos = {
            let s = player.state.lock().unwrap();
            [s.x, s.y + 0.9, s.z]
        };
        let motion = match impact(pos) {
            Some((i, dir, dmg)) => {
                tracing::debug!("explosión alcanza a {} ({dmg:.1} de daño, impacto {i:.2})", player.name);
                let kind = if source.as_ref().is_some_and(|a| server.players.read().unwrap().contains_key(&a.eid)) { "player_explosion" } else { "explosion" };
                let dmg = super::mobs::difficulty_damage(server, dmg);
                let hurt = health::damage(server, &player, dmg, kind, source.clone());
                if hurt {
                    player.state.lock().unwrap().ac.grace = 40;
                }
                [dir[0] * i, dir[1] * i, dir[2] * i]
            }
            None => [0.0; 3],
        };
        player.send(explosion_packet(center, power, &blocks, motion));
    }
    let mob_hits: Vec<(i32, [f64; 3], f32, [f64; 3])> = server
        .mobs
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.dim == dim && source.as_ref().is_none_or(|s| s.eid != m.eid))
        .filter_map(|m| impact(m.pos).map(|(i, dir, dmg)| (m.eid, dir, dmg, [dir[0] * i, dir[1] * i + 0.2, dir[2] * i])))
        .collect();
    for (eid, _, dmg, knock) in mob_hits {
        super::mobs::hurt(server, eid, dmg, "explosion", None, Some(knock), 0);
    }
}

fn explosion_packet(center: [f64; 3], power: f32, blocks: &[BlockPos], motion: [f64; 3]) -> Vec<u8> {
    let (cx, cy, cz) = (center[0].floor() as i32, center[1].floor() as i32, center[2].floor() as i32);
    let mut p = packet(id::play::EXPLOSION);
    p.f64(center[0]).f64(center[1]).f64(center[2]).f32(power);
    let near: Vec<&BlockPos> = blocks.iter().filter(|b| (b.x - cx).abs() < 128 && (b.y - cy).abs() < 128 && (b.z - cz).abs() < 128).collect();
    p.varint(near.len() as i32);
    for b in near {
        p.i8((b.x - cx) as i8).i8((b.y - cy) as i8).i8((b.z - cz) as i8);
    }
    p.f32(motion[0] as f32).f32(motion[1] as f32).f32(motion[2] as f32);
    // Interaction: 2 = destroy_with_decay (what a creeper does).
    p.varint(2);
    p.varint(PARTICLE_EXPLOSION);
    p.varint(PARTICLE_EMITTER);
    p.varint(0).string("minecraft:entity.generic.explode").bool(false);
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn creeper_crater() {
        let mut cfg = Config::default();
        cfg.generation.name = "flat".into();
        cfg.generation.options.layers = vec![("bedrock".into(), 1), ("dirt".into(), 10), ("obsidian".into(), 1)];
        cfg.world_folder = std::env::temp_dir().join(format!("bambietta-boom-{}", std::process::id())).display().to_string();
        let mut w = crate::world::World::new(&cfg, crate::world::Dimension::Overworld).unwrap();
        // Explosion right above the obsidian (y = -53): doesn't break it.
        let hit = affected_blocks(&mut w, [0.5, -52.0, 0.5], 3.0);
        assert!(hit.is_empty(), "la obsidiana resiste");
        // Inside the dirt it does make a crater.
        let hit = affected_blocks(&mut w, [0.5, -58.0, 0.5], 3.0);
        assert!(hit.len() > 20, "{}", hit.len());
        assert!(!hit.iter().any(|b| b.y == -64), "la roca madre resiste");
    }
}

//! The Wither. Built from four soul sand (or soul soil) blocks in a T with three wither
//! skeleton skulls on top: it appears small and invulnerable, grows to full health in 11 s and
//! then explodes. It flies a few blocks above its target (players, and any living thing that
//! is not undead), fires wither skulls from its three heads, regenerates, breaks the blocks
//! around it when it is hurt, and below half health its armor makes it immune to arrows.
//! A purple boss bar follows the fight; it drops a nether star.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, sound, Server};
use crate::data;
use crate::protocol::{id, packet, text, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Ticks it takes to come to life.
pub const BIRTH: u32 = 220;

fn r() -> f64 {
    entities::random() as f64
}

pub fn ai(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], others: &[super::animals::Other], actions: &mut Vec<Action>) {
    // Coming to life: grows, then explodes.
    if m.convert > 0 {
        m.convert -= 1;
        m.vel = [0.0; 3];
        m.health = (m.max_health() * (1.0 - m.convert as f32 / BIRTH as f32)).max(1.0);
        if m.convert == 0 {
            let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.center()) };
            actions.push(Action::Explode(m.dim, m.center(), 7.0, by, false));
            actions.push(Action::WitherBorn(m.dim, m.pos));
        }
        return;
    }
    if m.age % 20 == 0 {
        m.health = (m.health + 1.0).min(m.max_health());
    }
    // Breaks the blocks around it after being hurt.
    if m.hurt_time == 9 && server.rule("mobGriefing") {
        let reg = data::get();
        let c = [m.pos[0].floor() as i32, m.pos[1].floor() as i32, m.pos[2].floor() as i32];
        for dx in -1..=1 {
            for dy in 0..=3 {
                for dz in -1..=1 {
                    let p = BlockPos::new(c[0] + dx, c[1] + dy, c[2] + dz);
                    let s = world.get_block(p);
                    let n = reg.block_of_state(s).name.as_str();
                    let hard = matches!(n, "bedrock" | "barrier" | "end_portal" | "end_portal_frame" | "end_gateway" | "command_block" | "reinforced_deepslate");
                    if !reg.is_air(s) && !hard && !matches!(n, "water" | "lava") {
                        actions.push(Action::Break(m.dim, p, s));
                    }
                }
            }
        }
    }
    // Target: players first, then any living thing that is not undead.
    let player = seen
        .iter()
        .filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 40.0 * 40.0)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
    let (aim, eid) = match player {
        Some(p) => (p.pos, p.player.eid),
        None => match others
            .iter()
            .filter(|o| o.eid != m.eid && !o.kind.undead() && !matches!(o.kind, MobKind::EndCrystal | MobKind::Wither) && mobs::dist2(&o.pos, &m.pos) < 24.0 * 24.0)
            .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
        {
            Some(o) => (o.pos, o.eid),
            None => {
                m.target = None;
                // Drifts about.
                if m.wander.is_none_or(|w| mobs::dist2(&w, &m.pos) < 2.0) {
                    m.wander = Some([m.pos[0] + (r() - 0.5) * 20.0, m.pos[1] + (r() - 0.5) * 4.0, m.pos[2] + (r() - 0.5) * 20.0]);
                }
                let w = m.wander.unwrap();
                let dir = mobs::normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
                m.vel = [dir[0] * 0.1, dir[1] * 0.1, dir[2] * 0.1];
                return;
            }
        },
    };
    m.target = Some(eid);
    // Hovers five blocks above, circling at about ten blocks.
    m.approach += 0.02;
    let hover = [aim[0] + m.approach.cos() * 9.0, aim[1] + 5.0, aim[2] + m.approach.sin() * 9.0];
    let dir = mobs::normalize([hover[0] - m.pos[0], hover[1] - m.pos[1], hover[2] - m.pos[2]]);
    let far = mobs::dist2(&hover, &m.pos).sqrt();
    let speed = if far > 2.0 { 0.25 } else { far * 0.1 };
    for i in 0..3 {
        m.vel[i] += (dir[i] * speed - m.vel[i]) * 0.2;
    }
    mobs::face(m, aim);
    // A skull every 2 s (1 s on hard) from one of its heads.
    let every = if server.config.difficulty >= 3 { 20 } else { 40 };
    if m.attack_cooldown == 0 {
        m.attack_cooldown = every;
        let head = [m.pos[0] + (r() - 0.5) * 1.3, m.pos[1] + 3.0, m.pos[2] + (r() - 0.5) * 1.3];
        let d = mobs::normalize([aim[0] - head[0], aim[1] + 0.9 - head[1], aim[2] - head[2]]);
        actions.push(Action::Shoot(m.dim, ProjectileKind::WitherSkull, m.eid, head, [d[0] * 0.6, d[1] * 0.6, d[2] * 0.6]));
    }
}

/// Below half health its armor turns arrows away.
pub fn armored(m: &Mob) -> bool {
    m.kind == MobKind::Wither && m.health <= m.max_health() / 2.0
}

// ---------- boss bar ----------

static BARS: Mutex<Option<HashMap<i32, Vec<i32>>>> = Mutex::new(None);

fn bar_uuid(eid: i32) -> u128 {
    0x5749_5448_4552_0000_0000_0000_0000_0000 | eid as u32 as u128
}

/// Every second: players within 64 blocks see the bar of the Withers about.
pub fn bars(server: &Arc<Server>) {
    let withers: Vec<(i32, Dimension, [f64; 3], f32, f32)> = server.mobs.lock().unwrap().values().filter(|m| m.kind == MobKind::Wither).map(|m| (m.eid, m.dim, m.pos, m.health.max(0.0), m.max_health())).collect();
    let mut guard = BARS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    // Gone withers: their bars go.
    let gone: Vec<i32> = map.keys().copied().filter(|e| !withers.iter().any(|w| w.0 == *e)).collect();
    for e in gone {
        if let Some(viewers) = map.remove(&e) {
            let mut p = packet(id::play::BOSS_BAR);
            p.uuid(bar_uuid(e)).varint(1);
            for v in viewers {
                if let Some(pl) = server.players.read().unwrap().get(&v) {
                    pl.send(p.clone());
                }
            }
        }
    }
    for (eid, dim, pos, health, max) in withers {
        let viewers = map.entry(eid).or_default();
        for p in server.online() {
            let near = p.dim() == dim && {
                let st = p.state.lock().unwrap();
                mobs::dist2(&[st.x, st.y, st.z], &pos) < 64.0 * 64.0
            };
            let has = viewers.contains(&p.eid);
            if near && !has {
                viewers.push(p.eid);
                let mut add = packet(id::play::BOSS_BAR);
                add.uuid(bar_uuid(eid)).varint(0);
                crate::protocol::Nbt::compound().with("translate", crate::protocol::Nbt::str("entity.minecraft.wither")).write_network(&mut add);
                // Purple, solid, darkens the sky.
                add.f32((health / max).clamp(0.0, 1.0)).varint(5).varint(0).u8(0x01);
                p.send(add);
            } else if !near && has {
                viewers.retain(|v| *v != p.eid);
                let mut rm = packet(id::play::BOSS_BAR);
                rm.uuid(bar_uuid(eid)).varint(1);
                p.send(rm);
            } else if near {
                let mut h = packet(id::play::BOSS_BAR);
                h.uuid(bar_uuid(eid)).varint(2).f32((health / max).clamp(0.0, 1.0));
                p.send(h);
            }
        }
    }
    let _ = text;
}

// ---------- summoning ----------

/// A wither skeleton skull placed at `at`: if it completes the T of soul sand with three
/// skulls, the Wither is born.
pub fn try_build(server: &Arc<Server>, dim: Dimension, at: BlockPos) -> bool {
    let reg = data::get();
    let found = {
        let mut world = server.world(dim).lock().unwrap();
        let name = |world: &mut World, p: BlockPos| reg.block_of_state(world.get_block(p)).name.clone();
        let soul = |n: &str| n == "soul_sand" || n == "soul_soil";
        let mut found = None;
        'search: for (dx, dz) in [(1, 0), (0, 1)] {
            // The placed skull may be any of the three.
            for k in -1..=1 {
                let mid = BlockPos::new(at.x - k * dx, at.y, at.z - k * dz);
                let skulls = [-1, 0, 1].map(|i| BlockPos::new(mid.x + i * dx, mid.y, mid.z + i * dz));
                let body = [-1, 0, 1].map(|i| BlockPos::new(mid.x + i * dx, mid.y - 1, mid.z + i * dz));
                let stem = BlockPos::new(mid.x, mid.y - 2, mid.z);
                let ok = skulls.iter().all(|&p| name(&mut world, p) == "wither_skeleton_skull")
                    && body.iter().all(|&p| soul(&name(&mut world, p)))
                    && soul(&name(&mut world, stem))
                    && [-1, 1].iter().all(|&i| reg.is_air(world.get_block(BlockPos::new(stem.x + i * dx, stem.y, stem.z + i * dz))));
                if ok {
                    let cells: Vec<BlockPos> = skulls.iter().chain(body.iter()).copied().chain(std::iter::once(stem)).collect();
                    found = Some((cells, stem));
                    break 'search;
                }
            }
        }
        if let Some((cells, _)) = &found {
            for c in cells {
                world.take_block_entity(*c);
                world.set_block(*c, 0);
            }
        }
        found
    };
    let Some((cells, stem)) = found else { return false };
    for c in cells {
        server.broadcast_dim(dim, &super::blocks::block_change(c, 0), None);
    }
    let eid = mobs::spawn(server, dim, MobKind::Wither, [stem.x as f64 + 0.5, stem.y as f64, stem.z as f64 + 0.5]);
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.convert = BIRTH;
        m.health = 1.0;
    }
    sound::entity(server, eid, "entity.wither.ambient", sound::HOSTILE, 1.0, 1.0);
    true
}

/// The Wither comes to life: everyone hears it (level event 1023, global).
pub fn born(server: &Server, dim: Dimension, pos: [f64; 3]) {
    let mut p = packet(id::play::WORLD_EVENT);
    p.i32(1023).position(BlockPos::new(pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32)).i32(0).bool(true);
    server.broadcast_dim(dim, &p, None);
}

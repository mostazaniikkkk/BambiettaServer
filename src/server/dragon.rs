//! The Ender Dragon: spawns the first time someone enters the End, flies in circles,
//! charges, shoots fireballs, perches on the portal, heals from the pillar crystals,
//! and on death opens the exit portal and leaves the egg. Boss bar for whoever
//! is in the End.

use super::health::Attacker;
use super::mobs::{dist2, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, Server};
use crate::data;
use crate::protocol::{id, packet, text, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::collections::HashSet;
use std::sync::Arc;

/// Top of the exit portal pillar (where the dragon perches and the egg appears).
const PERCH: [f64; 3] = [0.5, 68.0, 0.5];

/// Dragon phases (EnderDragonPhase): the client uses them to animate it.
pub mod phase {
    pub const HOLDING: i32 = 0;
    pub const STRAFE: i32 = 1;
    pub const LANDING_APPROACH: i32 = 2;
    pub const LANDING: i32 = 3;
    pub const TAKEOFF: i32 = 4;
    pub const SITTING_SCANNING: i32 = 6;
    pub const SITTING_ATTACKING: i32 = 7;
    pub const CHARGING: i32 = 8;
    pub const DYING: i32 = 9;
}

#[derive(Clone, Default, Debug)]
pub struct DragonState {
    pub phase: i32,
    angle: f64,
    timer: u32,
    charge_target: Option<[f64; 3]>,
    shot: bool,
    /// Player who dealt the final blow.
    pub killer: Option<i32>,
}

/// Fight state (saved in `level.dat`, like `DragonFight`).
#[derive(Default)]
pub struct Fight {
    pub killed: bool,
    /// Already killed at least once (the egg only drops the first time).
    pub previously_killed: bool,
    bar: HashSet<i32>,
    portal_checked: bool,
}

impl Fight {
    pub fn new(killed: bool) -> Self {
        Self { killed, previously_killed: killed, ..Default::default() }
    }
}

fn bar_uuid(dragon: &Mob) -> u128 {
    dragon.uuid
}

fn bar_add(dragon: &Mob) -> Vec<u8> {
    let mut p = packet(id::play::BOSS_BAR);
    p.uuid(bar_uuid(dragon)).varint(0);
    text("Dragón del End").write_network(&mut p);
    // Pink, solid bar, boss music and fog.
    p.f32((dragon.health / 200.0).clamp(0.0, 1.0)).varint(0).varint(0).u8(0x02 | 0x04);
    p
}

fn bar_health(dragon: &Mob) -> Vec<u8> {
    let mut p = packet(id::play::BOSS_BAR);
    p.uuid(bar_uuid(dragon)).varint(2).f32((dragon.health / 200.0).clamp(0.0, 1.0));
    p
}

fn bar_remove(uuid: u128) -> Vec<u8> {
    let mut p = packet(id::play::BOSS_BAR);
    p.uuid(uuid).varint(1);
    p
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// One tick of dragon AI (flight, attacks, crystal healing).
pub fn ai(server: &Server, _world: &mut World, m: &mut Mob, seen: &[Seen], crystals: &[(i32, Dimension, [f64; 3])], actions: &mut Vec<Action>) {
    let Some(d) = m.dragon.as_deref_mut() else { return };
    d.timer += 1;
    // Healing from the nearest crystal.
    let near_crystal = crystals.iter().filter(|c| c.1 == m.dim).any(|c| dist2(&c.2, &m.pos) < 32.0 * 32.0);
    if near_crystal && m.age % 10 == 0 && m.health < 200.0 {
        m.health = (m.health + 1.0).min(200.0);
    }
    let players: Vec<&Seen> = seen.iter().filter(|s| s.targetable && dist2(&s.pos, &[0.0, 64.0, 0.0]) < 150.0 * 150.0).collect();
    let nearest = players.iter().min_by(|a, b| dist2(&a.pos, &m.pos).total_cmp(&dist2(&b.pos, &m.pos))).copied();
    let rnd = || entities::random() as f64;

    // Target point to fly to depending on the phase.
    let (target, speed) = match d.phase {
        phase::STRAFE => match nearest {
            Some(p) => {
                let t = [p.pos[0], p.pos[1] + 12.0, p.pos[2]];
                let dd = dist2(&t, &m.pos).sqrt();
                if dd < 40.0 && !d.shot {
                    d.shot = true;
                    let from = [m.pos[0], m.pos[1] - 1.0, m.pos[2]];
                    let dir = normalize([p.pos[0] - from[0], p.pos[1] + 1.0 - from[1], p.pos[2] - from[2]]);
                    actions.push(Action::Shoot(m.dim, ProjectileKind::DragonFireball, m.eid, from, [dir[0] * 1.2, dir[1] * 1.2, dir[2] * 1.2]));
                }
                if d.shot && d.timer > 40 || d.timer > 200 {
                    d.phase = phase::HOLDING;
                    d.timer = 0;
                }
                (t, 0.7)
            }
            None => {
                d.phase = phase::HOLDING;
                ([0.0, 80.0, 0.0], 0.6)
            }
        },
        phase::CHARGING => {
            let t = d.charge_target.unwrap_or([0.0, 70.0, 0.0]);
            if dist2(&t, &m.pos) < 9.0 || d.timer > 100 {
                d.phase = phase::HOLDING;
                d.timer = 0;
            }
            (t, 1.1)
        }
        phase::LANDING_APPROACH => {
            let t = [PERCH[0], PERCH[1] + 12.0, PERCH[2]];
            if dist2(&t, &m.pos) < 16.0 {
                d.phase = phase::LANDING;
                d.timer = 0;
            }
            (t, 0.6)
        }
        phase::LANDING => {
            if dist2(&PERCH, &m.pos) < 1.0 || d.timer > 120 {
                d.phase = phase::SITTING_SCANNING;
                d.timer = 0;
                actions.push(Action::Sound(m.eid, "entity.ender_dragon.growl".into(), super::sound::HOSTILE));
            }
            (PERCH, 0.3)
        }
        phase::SITTING_SCANNING | phase::SITTING_ATTACKING => {
            if d.phase == phase::SITTING_SCANNING && d.timer > 60 {
                d.phase = phase::SITTING_ATTACKING;
                d.timer = 0;
            } else if d.phase == phase::SITTING_ATTACKING && d.timer > 100 {
                d.phase = phase::TAKEOFF;
                d.timer = 0;
            }
            if let Some(p) = nearest {
                let (dx, dz) = (p.pos[0] - m.pos[0], p.pos[2] - m.pos[2]);
                m.yaw = (-dx.atan2(dz)).to_degrees() as f32;
            }
            m.vel = [0.0; 3];
            m.pos = PERCH;
            (PERCH, 0.0)
        }
        phase::TAKEOFF => {
            let t = [PERCH[0] + 20.0, PERCH[1] + 25.0, PERCH[2]];
            if dist2(&t, &m.pos) < 16.0 || d.timer > 120 {
                d.phase = phase::HOLDING;
                d.timer = 0;
            }
            (t, 0.6)
        }
        _ => {
            // Circling the island; on reaching each point it decides what to do.
            let t = [d.angle.cos() * 50.0, 80.0 + (d.angle * 2.0).sin() * 8.0, d.angle.sin() * 50.0];
            if dist2(&t, &m.pos) < 100.0 {
                d.angle += 0.6;
                let crystals_left = crystals.iter().filter(|c| c.1 == m.dim).count();
                let r = rnd();
                if let Some(p) = nearest {
                    if r < 1.0 / (crystals_left as f64 + 3.0) {
                        d.phase = phase::LANDING_APPROACH;
                    } else if r < 0.35 {
                        d.phase = phase::STRAFE;
                        d.shot = false;
                    } else if r < 0.5 {
                        d.phase = phase::CHARGING;
                        d.charge_target = Some(p.pos);
                    }
                    d.timer = 0;
                }
            }
            (t, 0.6)
        }
    };
    // Smooth flight toward the point.
    if speed > 0.0 {
        let dir = normalize([target[0] - m.pos[0], target[1] - m.pos[1], target[2] - m.pos[2]]);
        for i in 0..3 {
            m.vel[i] = m.vel[i] * 0.85 + dir[i] * speed * 0.15;
        }
        for i in 0..3 {
            m.pos[i] += m.vel[i];
        }
        let h = (m.vel[0] * m.vel[0] + m.vel[2] * m.vel[2]).sqrt();
        if h > 1e-3 {
            m.yaw = (-m.vel[0].atan2(m.vel[2])).to_degrees() as f32;
        }
        m.pitch = (-(m.vel[1].atan2(h.max(1e-3))).to_degrees() as f32).clamp(-60.0, 60.0);
    }
    // Collision with players: the head bites, the wings push.
    if m.attack_cooldown > 0 {
        m.attack_cooldown -= 1;
    } else {
        let (s, c) = (m.yaw.to_radians() as f64).sin_cos();
        let head = [m.pos[0] - s * 6.5, m.pos[1] + 1.0, m.pos[2] + c * 6.5];
        for p in seen.iter().filter(|p| p.targetable) {
            let body = (p.pos[0] - m.pos[0]).abs() < 5.0 && (p.pos[2] - m.pos[2]).abs() < 5.0 && p.pos[1] > m.pos[1] - 4.0 && p.pos[1] < m.pos[1] + 4.0;
            let bitten = dist2(&p.pos, &head) < 3.0 * 3.0;
            if body || bitten {
                let by = Attacker { eid: m.eid, name: MobKind::EnderDragon.display().to_string(), pos: Some(m.pos) };
                let dmg = if bitten { 10.0 } else { 5.0 };
                actions.push(Action::Hurt(p.player.clone(), super::mobs::difficulty_damage(server, dmg), "mob_attack", by));
                let away = normalize([p.pos[0] - m.pos[0], 0.0, p.pos[2] - m.pos[2]]);
                actions.push(Action::Knock(p.player.clone(), [away[0] * 1.5, 0.6, away[2] * 1.5]));
                m.attack_cooldown = 10;
            }
        }
    }
}

/// Death animation (200 ticks): rises slowly and releases the experience at the end.
pub fn dying_tick(m: &mut Mob, t: u32, actions: &mut Vec<Action>) {
    m.pos[1] += 0.1;
    m.sent = [f64::MAX; 3];
    // 12000 experience in total, like vanilla's first kill.
    if t > 150 && t % 5 == 0 {
        actions.push(Action::Xp(m.dim, m.pos, 960));
    }
    if t == 199 {
        actions.push(Action::Xp(m.dim, m.pos, 2400));
    }
    if t == 1 {
        actions.push(Action::Sound(m.eid, "entity.ender_dragon.death".into(), super::sound::HOSTILE));
    }
}

/// Exit portal: active (with portal blocks) or off.
fn exit_portal(world: &mut World, lit: bool) -> Vec<(BlockPos, u16)> {
    let reg = data::get();
    let state = if lit { reg.state("end_portal") } else { 0 };
    let mut out = Vec::new();
    for dx in -3i32..=3 {
        for dz in -3i32..=3 {
            let d2 = dx * dx + dz * dz;
            if (1..=6).contains(&d2) {
                let p = BlockPos::new(dx, 64, dz);
                if world.get_block(p) != state {
                    world.set_block(p, state);
                    out.push((p, state));
                }
            }
        }
    }
    out
}

/// The dragon finished dying: exit portal, egg (the first time) and end of the fight.
pub fn died(server: &Arc<Server>, m: &Mob) {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(m.eid);
    server.broadcast_tracked(m.eid, &d, true);
    let first = {
        let mut f = server.dragon.lock().unwrap();
        f.killed = true;
        let first = !f.previously_killed;
        f.previously_killed = true;
        for eid in f.bar.drain() {
            if let Some(p) = server.players.read().unwrap().get(&eid) {
                p.send(bar_remove(bar_uuid(m)));
            }
        }
        first
    };
    let mut changes = {
        let mut world = server.world(Dimension::End).lock().unwrap();
        exit_portal(&mut world, true)
    };
    if first {
        let egg = BlockPos::new(0, PERCH[1] as i32, 0);
        let s = data::get().state("dragon_egg");
        server.world(Dimension::End).lock().unwrap().set_block(egg, s);
        changes.push((egg, s));
    }
    for (p, s) in changes {
        server.broadcast_dim(Dimension::End, &super::blocks::block_change(p, s), None);
    }
    super::sound::at(server, Dimension::End, PERCH, "block.end_portal.spawn", super::sound::BLOCK, 1.0, 1.0);
    server.broadcast_message("§d¡El dragón del End ha sido derrotado!");
    tracing::info!("el dragón del End murió");
}

/// Every second: spawn the dragon and the crystals, light the portal if it is already dead and
/// manage the boss bar of those in the End.
pub fn tick_fight(server: &Arc<Server>, seen: &[Seen], age: i64) {
    if age % 10 != 0 {
        return;
    }
    let in_end: Vec<&Seen> = seen.iter().filter(|s| s.dim == Dimension::End).collect();
    let dragon = server.mobs.lock().unwrap().values().find(|m| m.kind == MobKind::EnderDragon && m.dim == Dimension::End).cloned();
    let killed = server.dragon.lock().unwrap().killed;
    if in_end.is_empty() {
        // Nobody in the End: remove any leftover bars.
        if let Some(d) = &dragon {
            let mut f = server.dragon.lock().unwrap();
            for eid in f.bar.drain() {
                if let Some(p) = server.players.read().unwrap().get(&eid) {
                    p.send(bar_remove(bar_uuid(d)));
                }
            }
        }
        return;
    }
    if killed {
        let check = !std::mem::replace(&mut server.dragon.lock().unwrap().portal_checked, true);
        if check {
            let changes = exit_portal(&mut server.world(Dimension::End).lock().unwrap(), true);
            for (p, s) in changes {
                server.broadcast_dim(Dimension::End, &super::blocks::block_change(p, s), None);
            }
        }
        return;
    }
    let Some(dragon) = dragon else {
        // First visit: dragon and a crystal on top of each pillar.
        let crystals = server.mobs.lock().unwrap().values().filter(|m| m.kind == MobKind::EndCrystal && m.dim == Dimension::End).count();
        if crystals == 0 {
            for (x, z, _, h) in crate::world::generator::end_pillars() {
                super::mobs::spawn(server, Dimension::End, MobKind::EndCrystal, [x as f64 + 0.5, (h + 1) as f64, z as f64 + 0.5]);
            }
        }
        let changes = exit_portal(&mut server.world(Dimension::End).lock().unwrap(), false);
        for (p, s) in changes {
            server.broadcast_dim(Dimension::End, &super::blocks::block_change(p, s), None);
        }
        super::mobs::spawn(server, Dimension::End, MobKind::EnderDragon, [0.5, 100.0, 40.5]);
        tracing::info!("aparece el dragón del End");
        return;
    };
    if dragon.death_ticks.is_some() {
        return;
    }
    // Boss bar.
    let mut f = server.dragon.lock().unwrap();
    let here: HashSet<i32> = in_end.iter().map(|s| s.player.eid).collect();
    for s in &in_end {
        if f.bar.insert(s.player.eid) {
            s.player.send(bar_add(&dragon));
        } else if age % 20 == 0 {
            s.player.send(bar_health(&dragon));
        }
    }
    let gone: Vec<i32> = f.bar.difference(&here).copied().collect();
    for eid in gone {
        f.bar.remove(&eid);
        if let Some(p) = server.players.read().unwrap().get(&eid) {
            p.send(bar_remove(bar_uuid(&dragon)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_portal_lights_ring() {
        let mut w = World::new_flat_for_tests();
        let lit = exit_portal(&mut w, true);
        assert_eq!(lit.len(), 20);
        assert!(lit.iter().all(|(p, _)| p.y == 64 && !(p.x == 0 && p.z == 0)));
        assert!(exit_portal(&mut w, true).is_empty(), "ya estaba encendido");
        assert_eq!(exit_portal(&mut w, false).len(), 20);
    }
}

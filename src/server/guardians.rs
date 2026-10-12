//! Guardians and elder guardians (deep oceans, as there are no ocean monuments yet): they
//! swim about, lock a laser on a player they can see (it takes 4 s to charge, 3 s for the
//! elder) and hurt whoever hits them while their spikes are out. Elder guardians curse the
//! players around them with Mining Fatigue III every minute.
//!
//! Shulkers (on the outer End islands, as there are no End cities yet): they sit on the
//! ground, open their shell to fire bullets that home in on their target (Levitation), and
//! are hard to hurt while closed; when hurt they may teleport away.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, sound, Server};
use crate::data;
use crate::protocol::BlockPos;
use crate::world::World;

fn r() -> f64 {
    entities::random() as f64
}

fn wet(world: &mut World, p: [f64; 3]) -> bool {
    data::get().block_of_state(world.get_block(BlockPos::new(p[0].floor() as i32, (p[1] + 0.3).floor() as i32, p[2].floor() as i32))).name == "water"
}

/// Laser charge time.
fn charge_time(kind: MobKind) -> u32 {
    if kind == MobKind::ElderGuardian { 60 } else { 80 }
}

pub fn guardian(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], actions: &mut Vec<Action>) {
    let in_water = wet(world, m.pos);
    if !in_water {
        // Flops about on land, and dries out.
        m.air -= 1;
        if m.air <= -20 {
            m.air = 0;
            m.health -= 2.0;
            m.hurt_time = 10;
        }
        if m.on_ground && r() < 0.1 {
            m.vel = [(r() - 0.5) * 0.3, 0.4, (r() - 0.5) * 0.3];
        }
        m.target = None;
        m.drink = 0;
        return;
    }
    m.air = 300;
    // The elder's curse.
    if m.kind == MobKind::ElderGuardian && m.age % 1200 == 0 {
        for s in seen.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 50.0 * 50.0) {
            actions.push(Action::Effect(s.player.clone(), "mining_fatigue", 2, 6000));
            actions.push(Action::ElderCurse(s.player.clone()));
        }
    }
    let eye = [m.pos[0], m.pos[1] + m.size().1 * 0.5, m.pos[2]];
    let target = m.target.and_then(|e| seen.iter().find(|s| s.player.eid == e && s.targetable && mobs::dist2(&s.pos, &m.pos) < 16.0 * 16.0 && mobs::line_of_sight(world, eye, s.eye))).or_else(|| {
        if m.age % 10 != 0 {
            return None;
        }
        seen.iter()
            .filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 16.0 * 16.0 && mobs::line_of_sight(world, eye, s.eye))
            .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
    });
    match target {
        Some(t) if m.attack_cooldown == 0 => {
            // Holds still and charges the laser.
            m.target = Some(t.player.eid);
            m.vel = [m.vel[0] * 0.5, m.vel[1] * 0.5, m.vel[2] * 0.5];
            mobs::face(m, t.pos);
            m.drink += 1;
            if m.drink >= charge_time(m.kind) {
                m.drink = 0;
                m.attack_cooldown = 60;
                let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
                let dmg = if m.kind == MobKind::ElderGuardian { 8.0 } else { 6.0 };
                actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, dmg), "magic", by));
            }
        }
        _ => {
            if m.target.is_some() {
                m.target = None;
                m.drink = 0;
            }
            // Swims about.
            if m.wander.is_none_or(|w| mobs::dist2(&w, &m.pos) < 1.5) || r() < 0.01 {
                let p = [m.pos[0] + (r() - 0.5) * 16.0, m.pos[1] + (r() - 0.5) * 6.0, m.pos[2] + (r() - 0.5) * 16.0];
                m.wander = wet(world, p).then_some(p);
            }
            if let Some(w) = m.wander {
                let dir = mobs::normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
                let speed = m.kind.walk_speed();
                m.vel = [dir[0] * speed, dir[1] * speed, dir[2] * speed];
                mobs::face(m, w);
            }
        }
    }
    // Spikes go in while it swims.
    m.charging = m.vel[0].hypot(m.vel[2]) > 0.02;
}

/// Hitting a guardian with its spikes out hurts.
pub fn thorns(m: &Mob) -> bool {
    matches!(m.kind, MobKind::Guardian | MobKind::ElderGuardian) && !m.charging
}

// ---------- shulkers ----------

pub fn shulker(world: &mut World, m: &mut Mob, seen: &[Seen], actions: &mut Vec<Action>) {
    m.vel[0] = 0.0;
    m.vel[2] = 0.0;
    let eye = [m.pos[0], m.pos[1] + 0.5, m.pos[2]];
    let target = seen
        .iter()
        .filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 16.0 * 16.0 && mobs::line_of_sight(world, eye, s.eye))
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
    m.target = target.map(|t| t.player.eid);
    let want: u32 = match target {
        Some(t) => {
            mobs::face(m, t.pos);
            if m.attack_cooldown == 0 {
                m.attack_cooldown = 20 + (r() * 90.0) as u32;
                let from = [m.pos[0], m.pos[1] + 0.6, m.pos[2]];
                actions.push(Action::ShootAt(m.dim, ProjectileKind::ShulkerBullet, m.eid, from, [0.0, 0.15, 0.0], t.player.eid));
            }
            100
        }
        // Peeks out now and then.
        None if r() < 0.005 => 30,
        None if m.puff > 0 && r() < 0.02 => 0,
        None => m.puff,
    };
    if want != m.puff {
        m.puff = want;
        actions.push(Action::Sound(m.eid, if want > 0 { "entity.shulker.open".into() } else { "entity.shulker.close".into() }, sound::HOSTILE));
    }
}

/// A shulker hurt (or left without ground) may teleport to another spot nearby.
pub fn shulker_teleport(world: &mut World, m: &mut Mob) -> bool {
    for _ in 0..8 {
        let x = (m.pos[0] + (r() - 0.5) * 16.0).floor() as i32;
        let z = (m.pos[2] + (r() - 0.5) * 16.0).floor() as i32;
        if let Some(spot) = mobs::spawn_spot_at(world, x, z, m.pos[1] as i32 + 4, 1) {
            m.pos = spot;
            m.sent = [f64::MAX; 3];
            return true;
        }
    }
    false
}

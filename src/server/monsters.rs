//! Overworld monsters with their own tricks:
//! - Witches throw splash potions (slowness from afar, poison, weakness up close, harming)
//!   and drink their own: fire resistance when burning, water breathing under water,
//!   healing when hurt. Magic barely hurts them.
//! - Drowned swim at any depth, some throw tridents; a zombie that stays under water turns
//!   into one (a husk turns into a zombie).
//! - Phantoms circle high above players who have not slept for three nights and swoop down
//!   on them; they burn in the sun and keep away from cats.
//! - Slimes come out of swamps at night and out of "slime chunks" deep underground.
//! - Zombie villagers: villagers killed by zombies turn into them (half the time on normal,
//!   always on hard); Weakness plus a golden apple cures them in 3 to 5 minutes.
//! - Husks (deserts) make you hungry; strays (snowy biomes) shoot slowing arrows.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, sound, Server};
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::sync::Arc;

/// A zombie under water starts shaking after 30 s and turns into a drowned 15 s later.
pub const SHAKE: u32 = 600;
const DROWN: u32 = 900;
/// Ticks without sleeping before phantoms come (three days).
pub const INSOMNIA: i64 = 72000;

fn head_in_water(world: &mut World, m: &Mob) -> bool {
    let p = BlockPos::new(m.pos[0].floor() as i32, (m.pos[1] + m.size().1 * 0.85).floor() as i32, m.pos[2].floor() as i32);
    crate::data::get().block_of_state(world.get_block(p)).name == "water"
}

/// Every tick, before the rest of the AI: conversions, Weakness wearing off, witches drinking.
pub fn tick(world: &mut World, m: &mut Mob, actions: &mut Vec<Action>) {
    m.weak = m.weak.saturating_sub(1);
    m.resist = m.resist.saturating_sub(1);
    match m.kind {
        MobKind::Zombie | MobKind::Husk => {
            // Once it starts shaking there is no way back.
            if head_in_water(world, m) || m.convert >= SHAKE {
                m.convert += 1;
                if m.convert >= DROWN {
                    let to = if m.kind == MobKind::Husk { MobKind::Zombie } else { MobKind::Drowned };
                    actions.push(Action::Convert(m.eid, to));
                }
            } else {
                m.convert = 0;
            }
        }
        MobKind::ZombieVillager if m.convert > 0 => {
            m.convert -= 1;
            if m.convert == 0 {
                actions.push(Action::Convert(m.eid, MobKind::Villager));
            }
        }
        MobKind::Witch => witch_drink(world, m, actions),
        _ => {}
    }
}

// ---------- witch ----------

fn witch_drink(world: &mut World, m: &mut Mob, actions: &mut Vec<Action>) {
    if m.drink > 0 {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        m.drink -= 1;
        if m.drink == 0 {
            match m.drinking.take() {
                Some("healing") => m.health = (m.health + 4.0).min(m.max_health()),
                Some("fire_resistance") => {
                    m.fire = 0;
                    m.resist = 3600;
                }
                _ => {}
            }
            actions.push(Action::Meta(mobs::equipment_stack(m.eid, 0, None), m.eid));
            actions.push(Action::Sound(m.eid, "entity.witch.drink".into(), sound::HOSTILE));
        }
        return;
    }
    let potion = if m.fire > 0 && m.resist == 0 {
        Some("fire_resistance")
    } else if head_in_water(world, m) && entities::random() < 0.15 {
        Some("water_breathing")
    } else if m.health < m.max_health() && entities::random() < 0.05 {
        Some("healing")
    } else {
        None
    };
    if let Some(p) = potion {
        m.drink = 32;
        m.drinking = Some(p);
        actions.push(Action::Meta(mobs::equipment_stack(m.eid, 0, super::effects::potion_item("potion", p)), m.eid));
    }
}

/// A witch fighting a player: keeps within 10 blocks and throws a potion every 3 s.
pub fn witch_attack(server: &Server, world: &mut World, m: &mut Mob, t: &Seen, d: f64, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if m.drink > 0 {
        return Some((m.pos, 0.0));
    }
    let eye = [m.pos[0], m.pos[1] + 1.62, m.pos[2]];
    let sees = d < 16.0 && mobs::line_of_sight(world, eye, t.eye);
    if sees && d <= 10.0 && m.attack_cooldown == 0 {
        m.attack_cooldown = 60;
        let potion = if d >= 8.0 {
            "slowness"
        } else if entities::random() < 0.5 {
            "poison"
        } else if d <= 3.0 && entities::random() < 0.25 {
            "weakness"
        } else {
            "harming"
        };
        // Thrown with an arc, like a player throwing it upwards.
        let to = [t.pos[0] - m.pos[0], t.eye[1] - 1.1 - eye[1], t.pos[2] - m.pos[2]];
        let horiz = (to[0] * to[0] + to[2] * to[2]).sqrt();
        let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
        let vel = [dir[0] * 0.75, dir[1] * 0.75, dir[2] * 0.75];
        if let Some(item) = super::effects::potion_item("splash_potion", potion) {
            actions.push(Action::Shoot(m.dim, ProjectileKind::Potion(item), m.eid, eye, vel));
        }
    }
    let _ = server;
    if !sees || d > 9.0 {
        Some((mobs::follow_path(world, m, t.pos), m.kind.walk_speed()))
    } else {
        Some((m.pos, 0.0))
    }
}

// ---------- drowned ----------

pub fn throw_trident(world: &mut World, m: &mut Mob, t: &Seen, actions: &mut Vec<Action>) {
    if m.attack_cooldown > 0 {
        return;
    }
    let from = [m.pos[0], m.pos[1] + 1.5, m.pos[2]];
    if !mobs::line_of_sight(world, from, t.eye) {
        return;
    }
    m.attack_cooldown = 60;
    let to = [t.pos[0] - from[0], t.pos[1] + 0.6 - from[1], t.pos[2] - from[2]];
    let horiz = (to[0] * to[0] + to[2] * to[2]).sqrt();
    let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
    actions.push(Action::Shoot(m.dim, ProjectileKind::Trident, m.eid, from, [dir[0] * 1.6, dir[1] * 1.6, dir[2] * 1.6]));
}

/// A spot in the water (two water blocks tall) in this column, for a drowned.
pub fn water_spot(world: &mut World, x: i32, z: i32, from_y: i32) -> Option<[f64; 3]> {
    let reg = crate::data::get();
    let water = |world: &mut World, y: i32| reg.block_of_state(world.get_block(BlockPos::new(x, y, z))).name == "water";
    (from_y - 24..from_y + 16).rev().find(|&y| water(world, y) && water(world, y + 1) && !water(world, y - 1)).map(|y| [x as f64 + 0.5, y as f64, z as f64 + 0.5])
}

// ---------- phantom ----------

/// Phantoms: circle 20-30 blocks above, swoop at the player, climb back up. Without anyone to
/// hunt they circle where they are.
pub fn phantom(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[super::animals::Other], actions: &mut Vec<Action>, server: &Server) {
    let target = m.target.and_then(|e| seen.iter().find(|s| s.player.eid == e && s.targetable)).or_else(|| {
        seen.iter().filter(|s| s.targetable && (s.pos[0] - m.pos[0]).hypot(s.pos[2] - m.pos[2]) < 64.0).min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
    });
    m.target = target.map(|t| t.player.eid);
    // Cats scare them off.
    let cat = others.iter().any(|o| o.kind == MobKind::Cat && mobs::dist2(&o.pos, &m.pos) < 16.0 * 16.0);
    let center = match target {
        Some(t) => [t.pos[0], t.pos[1] + 22.0, t.pos[2]],
        None => *m.anchor.get_or_insert(m.pos),
    };
    let swooping = m.burst == 1 && target.is_some() && !cat;
    let want = if swooping {
        let t = target.unwrap();
        let aim = [t.pos[0], t.pos[1] + 0.9, t.pos[2]];
        // A hit (or being hurt) ends the swoop and it climbs back up.
        if mobs::dist2(&aim, &m.center()) < 1.6 * 1.6 {
            let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
            actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, 6.0), "mob_attack", by));
            m.burst = 0;
            m.attack_cooldown = 80 + (entities::random() * 60.0) as u32;
        } else if m.hurt_time > 0 {
            m.burst = 0;
            m.attack_cooldown = 60;
        }
        aim
    } else {
        // Circling: around the center, 10-16 blocks away.
        m.approach += 0.035;
        let r = 12.0 + (m.eid % 5) as f64;
        if target.is_some() && m.attack_cooldown == 0 && !cat && entities::random() < 0.02 {
            m.burst = 1;
            actions.push(Action::Sound(m.eid, "entity.phantom.swoop".into(), sound::HOSTILE));
        }
        [center[0] + m.approach.cos() * r, center[1] + (m.approach * 0.5).sin() * 2.0, center[2] + m.approach.sin() * r]
    };
    let speed = if swooping { 0.45 } else { 0.25 };
    let dir = mobs::normalize([want[0] - m.pos[0], want[1] - m.pos[1], want[2] - m.pos[2]]);
    for i in 0..3 {
        m.vel[i] += (dir[i] * speed - m.vel[i]) * 0.15;
    }
    m.yaw = (-m.vel[0].atan2(m.vel[2])).to_degrees() as f32;
    m.pitch = (-m.vel[1].atan2(m.vel[0].hypot(m.vel[2]))).to_degrees() as f32;
    let _ = world;
}

/// Every minute at night: players who have not slept for three nights may get a visit.
pub fn spawn_phantoms(server: &Arc<Server>, seen: &[Seen], night: bool) {
    if !night || !server.config.spawn_monsters || server.config.difficulty == 0 || !server.rule("doMobSpawning") {
        return;
    }
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld && s.targetable) {
        let rest = s.player.state.lock().unwrap().rest;
        if rest < INSOMNIA || (entities::random() as f64 * rest as f64) < INSOMNIA as f64 {
            continue;
        }
        let open = {
            let mut world = server.world(Dimension::Overworld).lock().unwrap();
            mobs::sky_exposed(&mut world, s.pos)
        };
        if !open {
            continue;
        }
        let n = 1 + (entities::random() * (server.config.difficulty as f32 + 1.0)) as i32;
        for _ in 0..n {
            let at = [s.pos[0] + (entities::random() as f64 - 0.5) * 20.0, s.pos[1] + 20.0 + entities::random() as f64 * 14.0, s.pos[2] + (entities::random() as f64 - 0.5) * 20.0];
            mobs::spawn(server, Dimension::Overworld, MobKind::Phantom, at);
        }
    }
}

// ---------- slimes ----------

/// java.util.Random, for vanilla's slime chunks.
struct JavaRandom(i64);

impl JavaRandom {
    fn new(seed: i64) -> Self {
        Self((seed ^ 0x5DEECE66D) & ((1 << 48) - 1))
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5DEECE66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }

    fn next_int(&mut self, bound: i32) -> i32 {
        loop {
            let r = self.next(31);
            let m = r % bound;
            if r - m + (bound - 1) >= 0 {
                return m;
            }
        }
    }
}

/// Vanilla's slime chunks (WorldgenRandom.seedSlimeChunk with salt 987234911): a chunk in ten.
pub fn slime_chunk(seed: i64, cx: i32, cz: i32) -> bool {
    let s = seed
        .wrapping_add(cx.wrapping_mul(cx).wrapping_mul(4987142) as i64)
        .wrapping_add(cx.wrapping_mul(5947611) as i64)
        .wrapping_add((cz.wrapping_mul(cz) as i64).wrapping_mul(4392871))
        .wrapping_add(cz.wrapping_mul(389711) as i64)
        ^ 987234911;
    JavaRandom::new(s).next_int(10) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches() {
        // new java.util.Random(42).nextInt(10) == 0 and new Random(0).nextInt(100) == 60.
        assert_eq!(JavaRandom::new(42).next_int(10), 0);
        assert_eq!(JavaRandom::new(0).next_int(100), 60);
        // About one chunk in ten is a slime chunk.
        let n = (0..100).flat_map(|x| (0..100).map(move |z| (x, z))).filter(|&(x, z)| slime_chunk(12345, x, z)).count();
        assert!((800..1200).contains(&n), "{n}");
    }
}

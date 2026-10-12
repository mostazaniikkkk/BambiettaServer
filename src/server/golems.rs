//! Iron golems. They guard villages (one shows up when a village with at least three
//! villagers has none), can be built (four iron blocks in a T with a carved pumpkin or a
//! jack o'lantern on top), hunt monsters other than creepers, hit back at players who hit
//! them, throw whatever they hit into the air, and are repaired with iron ingots.
//! Monsters hit by a golem (or any other mob) fight back at it.

use super::animals::Other;
use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{entities, Server};
use crate::data;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::sync::Arc;

/// How far a golem looks for monsters.
const GUARD: f64 = 16.0;

/// A golem's blow: 7.5 to 22.5 (vanilla: half its attack damage plus a random part).
pub fn blow() -> f32 {
    7.5 + entities::random() * 15.0
}

/// The golem without a player to fight: it goes for the nearest monster.
pub fn guard(world: &mut World, m: &mut Mob, others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let current = m.prey.and_then(|e| others.iter().find(|o| o.eid == e)).filter(|o| mobs::dist2(&o.pos, &m.pos) < 24.0 * 24.0);
    let prey = current.or_else(|| {
        others
            .iter()
            .filter(|o| o.kind.hostile() && o.kind != MobKind::Creeper && mobs::dist2(&o.pos, &m.pos) < GUARD * GUARD)
            .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
    });
    let Some(prey) = prey else {
        m.prey = None;
        return None;
    };
    m.prey = Some(prey.eid);
    strike_mob(world, m, prey, blow(), actions)
}

/// Walks up to a mob and hits it when in reach (golems toss it upwards).
pub(super) fn strike_mob(world: &mut World, m: &mut Mob, prey: &Other, damage: f32, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    mobs::face(m, prey.pos);
    let d = mobs::dist2(&prey.pos, &m.pos).sqrt();
    let reach = 1.4 + m.size().0 / 2.0;
    if d < reach && m.attack_cooldown == 0 {
        m.attack_cooldown = 20;
        let away = mobs::normalize([prey.pos[0] - m.pos[0], 0.0, prey.pos[2] - m.pos[2]]);
        let knock = if m.kind == MobKind::IronGolem { [away[0] * 0.3, 0.5, away[2] * 0.3] } else { [away[0] * 0.4, 0.3, away[2] * 0.4] };
        actions.push(Action::HurtMob(prey.eid, damage, m.eid, Some(knock)));
        if m.kind == MobKind::IronGolem {
            actions.push(Action::Meta(super::animals::status(m.eid, 4), m.eid));
            actions.push(Action::Sound(m.eid, "entity.iron_golem.attack".into(), super::sound::NEUTRAL));
        }
    }
    Some((mobs::follow_path(world, m, prey.pos), m.kind.walk_speed() * 1.3))
}

/// A monster that a mob (a golem, a wolf) hit fights back at it, when no player is in sight.
pub fn fight_back(world: &mut World, m: &mut Mob, others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let prey = m.prey.and_then(|e| others.iter().find(|o| o.eid == e)).filter(|o| mobs::dist2(&o.pos, &m.pos) < 20.0 * 20.0);
    let Some(prey) = prey else {
        m.prey = None;
        return None;
    };
    let damage = m.kind.melee_damage();
    strike_mob(world, m, prey, damage, actions)
}

/// The golem hits a player (one who attacked it).
pub fn strike_player(server: &Server, m: &mut Mob, t: &Seen, d: f64, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let reach = 1.6 + m.size().0 / 2.0;
    if d < reach && m.attack_cooldown == 0 {
        m.attack_cooldown = 20;
        let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
        actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, blow()), "mob_attack", by));
        actions.push(Action::Knock(t.player.clone(), [0.0, 0.6, 0.0]));
        actions.push(Action::Meta(super::animals::status(m.eid, 4), m.eid));
        actions.push(Action::Sound(m.eid, "entity.iron_golem.attack".into(), super::sound::NEUTRAL));
    }
    Some((t.pos, m.kind.walk_speed() * 1.3))
}

/// Every half minute: villages with three or more villagers and no golem get one.
pub fn village_watch(server: &Arc<Server>, seen: &[Seen]) {
    if !server.config.spawn_animals {
        return;
    }
    let mut centers: Vec<(i32, i32)> = Vec::new();
    {
        let world = server.world(Dimension::Overworld).lock().unwrap();
        for s in seen.iter().filter(|s| s.dim == Dimension::Overworld) {
            for c in world.generator().village_centers_near(s.pos[0] as i32, s.pos[2] as i32, 96) {
                if !centers.contains(&c) {
                    centers.push(c);
                }
            }
        }
    }
    for (cx, cz) in centers {
        let c = [cx as f64 + 0.5, 0.0, cz as f64 + 0.5];
        let flat = |p: &[f64; 3]| (p[0] - c[0]).hypot(p[2] - c[2]);
        let (villagers, golems) = {
            let mobs = server.mobs.lock().unwrap();
            let here = mobs.values().filter(|m| m.dim == Dimension::Overworld && m.death_ticks.is_none());
            let (mut v, mut g) = (0, 0);
            for m in here {
                match m.kind {
                    MobKind::Villager if flat(&m.pos) < 48.0 => v += 1,
                    MobKind::IronGolem if flat(&m.pos) < 80.0 => g += 1,
                    _ => {}
                }
            }
            (v, g)
        };
        if villagers < 3 || golems > 0 || entities::random() > 0.35 {
            continue;
        }
        let spot = {
            let mut world = server.world(Dimension::Overworld).lock().unwrap();
            if !world.is_loaded(cx >> 4, cz >> 4) {
                continue;
            }
            let x = cx + (entities::random() * 16.0) as i32 - 8;
            let z = cz + (entities::random() * 16.0) as i32 - 8;
            let top = world.generator().surface(x, z);
            mobs::spawn_spot_at(&mut world, x, z, top + 8, 3)
        };
        if let Some(at) = spot {
            mobs::spawn(server, Dimension::Overworld, MobKind::IronGolem, at);
        }
    }
}

/// A carved pumpkin (or jack o'lantern) placed at `head`: if it crowns a T of iron blocks,
/// the blocks become an iron golem.
pub fn try_build(server: &Arc<Server>, dim: Dimension, head: BlockPos) -> bool {
    let reg = data::get();
    // Two snow blocks under it: a snow golem.
    let snowman = {
        let mut world = server.world(dim).lock().unwrap();
        let a = BlockPos::new(head.x, head.y - 1, head.z);
        let b = BlockPos::new(head.x, head.y - 2, head.z);
        let snow = |world: &mut World, p: BlockPos| reg.block_of_state(world.get_block(p)).name == "snow_block";
        (snow(&mut world, a) && snow(&mut world, b)).then(|| {
            for c in [head, a, b] {
                world.set_block(c, 0);
            }
            b
        })
    };
    if let Some(b) = snowman {
        for c in [head, BlockPos::new(head.x, head.y - 1, head.z), b] {
            server.broadcast_dim(dim, &super::blocks::block_change(c, 0), None);
        }
        mobs::spawn(server, dim, MobKind::SnowGolem, [b.x as f64 + 0.5, b.y as f64, b.z as f64 + 0.5]);
        return true;
    }
    let found = {
        let mut world = server.world(dim).lock().unwrap();
        let name = |world: &mut World, p: BlockPos| reg.block_of_state(world.get_block(p)).name.clone();
        let body = BlockPos::new(head.x, head.y - 1, head.z);
        let legs = BlockPos::new(head.x, head.y - 2, head.z);
        if name(&mut world, body) != "iron_block" || name(&mut world, legs) != "iron_block" {
            return false;
        }
        let mut arms = None;
        for (dx, dz) in [(1, 0), (0, 1)] {
            let a = BlockPos::new(body.x + dx, body.y, body.z + dz);
            let b = BlockPos::new(body.x - dx, body.y, body.z - dz);
            // The spaces beside the legs must be free.
            let la = BlockPos::new(legs.x + dx, legs.y, legs.z + dz);
            let lb = BlockPos::new(legs.x - dx, legs.y, legs.z - dz);
            if name(&mut world, a) == "iron_block" && name(&mut world, b) == "iron_block" && reg.is_air(world.get_block(la)) && reg.is_air(world.get_block(lb)) {
                arms = Some((a, b));
            }
        }
        let Some((a, b)) = arms else { return false };
        let cells = [head, body, legs, a, b];
        for c in cells {
            world.set_block(c, 0);
        }
        Some((cells, legs))
    };
    let Some((cells, legs)) = found else { return false };
    for c in cells {
        server.broadcast_dim(dim, &super::blocks::block_change(c, 0), None);
    }
    let eid = mobs::spawn(server, dim, MobKind::IronGolem, [legs.x as f64 + 0.5, legs.y as f64, legs.z as f64 + 0.5]);
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.built = true;
    }
    true
}

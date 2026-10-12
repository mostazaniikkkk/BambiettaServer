//! The warden. It is blind: it finds players by the vibrations they make walking (not
//! sneaking) and by sniffing them out, and grows angry with each one; once angry enough it
//! roars and hunts them down with crushing blows, or a sonic boom (through shields and
//! armor) when it cannot reach them. It casts Darkness around itself, and after a minute
//! with nothing to hunt it digs back into the ground.
//!
//! It comes from sculk shriekers that can summon (`sculk_shrieker[can_summon=true]`): a
//! player walking near one makes it shriek (Darkness), and the fourth shriek in a while
//! brings a warden up out of the ground.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{entities, sound, Server};
use crate::data;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Poses (entity pose registry).
pub const POSE_ROARING: i32 = 11;
pub const POSE_SNIFFING: i32 = 12;
pub const POSE_EMERGING: i32 = 13;
pub const POSE_DIGGING: i32 = 14;

/// Anger needed to hunt someone.
const ANGRY: u32 = 80;

fn r() -> f64 {
    entities::random() as f64
}

fn suspect(m: &mut Mob, eid: i32, pos: [f64; 3], add: u32) -> u32 {
    match m.suspects.iter_mut().find(|s| s.0 == eid) {
        Some(s) => {
            s.1 = (s.1 + add).min(150);
            s.2 = pos;
            s.1
        }
        None => {
            m.suspects.push((eid, add.min(150), pos));
            add
        }
    }
}

/// Someone hurt it: they become its prime suspect.
pub fn hurt_by(m: &mut Mob, eid: i32, pos: [f64; 3]) {
    suspect(m, eid, pos, 100);
}

pub fn ai(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Emerging, roaring, sniffing or digging: stands still until done.
    if m.burst > 0 {
        m.burst -= 1;
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        if m.burst == 0 {
            if m.variant == POSE_DIGGING {
                actions.push(Action::Despawn(m.eid));
            } else if m.variant == POSE_SNIFFING {
                // It smells whoever is close, sneaking or not.
                if let Some(s) = seen.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 6.0 * 6.0).min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos))) {
                    suspect(m, s.player.eid, s.pos, 35);
                }
            }
            m.variant = 0;
        }
        return Some((m.pos, 0.0));
    }
    // Darkness around it.
    if m.age % 120 == 0 {
        for s in seen.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 20.0 * 20.0) {
            actions.push(Action::Effect(s.player.clone(), "darkness", 0, 260));
        }
    }
    // Vibrations: players moving about (not sneaking) within 16 blocks.
    if m.age % 10 == 0 {
        let here = m.pos;
        for s in seen.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &here) < 16.0 * 16.0) {
            let moved = m.suspects.iter().find(|x| x.0 == s.player.eid).is_none_or(|x| mobs::dist2(&x.2, &s.pos) > 0.2 * 0.2);
            if moved && !s.sneaking {
                let before = m.suspects.iter().find(|x| x.0 == s.player.eid).map_or(0, |x| x.1);
                let now = suspect(m, s.player.eid, s.pos, 35);
                actions.push(Action::Sound(m.eid, "entity.warden.listening".into(), sound::HOSTILE));
                // Angry for the first time: a roar.
                if before < ANGRY && now >= ANGRY {
                    m.variant = POSE_ROARING;
                    m.burst = 84;
                    actions.push(Action::Sound(m.eid, "entity.warden.roar".into(), sound::HOSTILE));
                    return Some((m.pos, 0.0));
                }
            } else if let Some(x) = m.suspects.iter_mut().find(|x| x.0 == s.player.eid) {
                x.2 = s.pos;
            }
        }
    }
    // Anger fades.
    if m.age % 20 == 0 {
        for s in &mut m.suspects {
            s.1 = s.1.saturating_sub(1);
        }
        m.suspects.retain(|s| s.1 > 0);
    }
    let prey = m.suspects.iter().filter(|s| s.1 >= ANGRY).max_by_key(|s| s.1).and_then(|s| seen.iter().find(|x| x.player.eid == s.0 && x.targetable));
    m.target = prey.map(|p| p.player.eid);
    let Some(t) = prey else {
        // Nothing to hunt: sniffs about, and digs away after a minute.
        m.approach += 1.0;
        if m.approach > 1200.0 {
            m.variant = POSE_DIGGING;
            m.burst = 100;
            actions.push(Action::Sound(m.eid, "entity.warden.dig".into(), sound::HOSTILE));
            return Some((m.pos, 0.0));
        }
        if r() < 1.0 / 200.0 {
            m.variant = POSE_SNIFFING;
            m.burst = 84;
            actions.push(Action::Sound(m.eid, "entity.warden.sniff".into(), sound::HOSTILE));
            return Some((m.pos, 0.0));
        }
        // Goes to where it last heard something.
        let heard = m.suspects.iter().max_by_key(|s| s.1).map(|s| s.2);
        return heard.map(|h| (mobs::follow_path(world, m, h), m.kind.walk_speed() * 0.6));
    };
    m.approach = 0.0;
    let d = mobs::dist2(&t.pos, &m.pos).sqrt();
    mobs::face(m, t.pos);
    let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
    m.drink = m.drink.saturating_sub(1);
    if d < 1.6 + m.size().0 / 2.0 && (t.pos[1] - m.pos[1]).abs() < 3.0 {
        if m.attack_cooldown == 0 {
            m.attack_cooldown = 18;
            actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, 30.0), "mob_attack", by));
            actions.push(Action::Meta(super::animals::status(m.eid, 4), m.eid));
        }
    } else if d < 15.0 && d > 4.0 && m.drink == 0 && m.attack_cooldown == 0 {
        // Sonic boom: through anything, ignores shields and armor.
        m.drink = 40;
        m.attack_cooldown = 34;
        actions.push(Action::Meta(super::animals::status(m.eid, 62), m.eid));
        actions.push(Action::Sound(m.eid, "entity.warden.sonic_boom".into(), sound::HOSTILE));
        actions.push(Action::Hurt(t.player.clone(), 10.0, "sonic_boom", by));
        let dir = mobs::normalize([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]);
        actions.push(Action::Knock(t.player.clone(), [dir[0] * 2.5, 0.5, dir[2] * 2.5]));
    }
    Some((mobs::follow_path(world, m, t.pos), m.kind.walk_speed() * 1.2))
}

/// The client's heartbeat follows the warden's anger.
pub fn anger(m: &Mob) -> i32 {
    m.suspects.iter().map(|s| s.1).max().unwrap_or(0) as i32
}

// ---------- shriekers ----------

/// Warning level per player (shrieks heard lately) and the time of the last shriek.
static WARNINGS: Mutex<Option<HashMap<u128, (u32, i64)>>> = Mutex::new(None);

/// Every second: players walking (not sneaking) near a summoning shrieker set it off.
pub fn shriekers(server: &Arc<Server>, seen: &[Seen], age: i64) {
    let reg = data::get();
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld && s.targetable && !s.sneaking) {
        let found = {
            let mut world = server.world(Dimension::Overworld).lock().unwrap();
            let (x, y, z) = (s.pos[0].floor() as i32, s.pos[1].floor() as i32, s.pos[2].floor() as i32);
            let mut found = None;
            'scan: for dx in -8..=8 {
                for dy in -4..=4 {
                    for dz in -8..=8 {
                        let p = BlockPos::new(x + dx, y + dy, z + dz);
                        let st = world.get_block(p);
                        let b = reg.block_of_state(st);
                        if b.name == "sculk_shrieker" && b.get(st, "can_summon") == Some("true") && b.get(st, "shrieking") != Some("true") {
                            found = Some((p, st));
                            break 'scan;
                        }
                    }
                }
            }
            found
        };
        let Some((p, _)) = found else { continue };
        let level = {
            let mut w = WARNINGS.lock().unwrap();
            let map = w.get_or_insert_with(HashMap::new);
            let e = map.entry(s.player.uuid).or_insert((0, 0));
            // One shriek every 10 s at most; the warnings fade after 10 minutes.
            if age - e.1 < 200 {
                continue;
            }
            if age - e.1 > 12000 {
                e.0 = 0;
            }
            e.0 += 1;
            e.1 = age;
            e.0
        };
        sound::at(server, Dimension::Overworld, [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5], "block.sculk_shrieker.shriek", sound::BLOCK, 2.0, 1.0);
        super::effects::add(server, &s.player, "darkness", 0, 260, 1.0);
        if level >= 4 {
            WARNINGS.lock().unwrap().as_mut().unwrap().remove(&s.player.uuid);
            let busy = server.mobs.lock().unwrap().values().any(|m| m.kind == MobKind::Warden && mobs::dist2(&m.pos, &s.pos) < 48.0 * 48.0);
            if !busy {
                let spot = {
                    let mut world = server.world(Dimension::Overworld).lock().unwrap();
                    (0..10).find_map(|_| {
                        let x = p.x + (r() * 10.0) as i32 - 5;
                        let z = p.z + (r() * 10.0) as i32 - 5;
                        mobs::spawn_spot_at(&mut world, x, z, p.y + 4, 3)
                    })
                };
                if let Some(at) = spot {
                    let eid = mobs::spawn(server, Dimension::Overworld, MobKind::Warden, at);
                    sound::entity(server, eid, "entity.warden.emerge", sound::HOSTILE, 1.0, 1.0);
                }
            }
        } else {
            let warn = ["", "entity.warden.nearby_close", "entity.warden.nearby_closer", "entity.warden.nearby_closest"][level.min(3) as usize];
            if !warn.is_empty() {
                sound::at(server, Dimension::Overworld, s.pos, warn, sound::HOSTILE, 1.0, 1.0);
            }
        }
    }
}

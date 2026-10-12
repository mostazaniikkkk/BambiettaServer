//! Smaller creatures:
//! - Bats flutter about caves and hang upside down from ceilings (they wake up when someone
//!   comes close).
//! - Snow golems (two snow blocks and a pumpkin) throw snowballs at monsters, leave a trail
//!   of snow in cold places, melt in hot biomes and in the rain; shears take their pumpkin.
//! - Sniffers sniff about, dig up ancient seeds (torchflower seeds and pitcher pods) and lay
//!   eggs that hatch into snifflets.
//! - Camels (deserts) sit down now and then; they are ridden with a saddle (see `horses`).
//! - Piglins and piglin brutes taken out of the Nether turn into zombified piglins.

use super::animals::Other;
use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, sound, Server};
use crate::data;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};

fn r() -> f64 {
    entities::random() as f64
}

fn name_at(world: &mut World, p: [f64; 3], dy: f64) -> &'static str {
    data::get().block_of_state(world.get_block(BlockPos::new(p[0].floor() as i32, (p[1] + dy).floor() as i32, p[2].floor() as i32))).name.as_str()
}

fn solid(world: &mut World, p: [f64; 3], dy: f64) -> bool {
    let s = world.get_block(BlockPos::new(p[0].floor() as i32, (p[1] + dy).floor() as i32, p[2].floor() as i32));
    crate::gameplay::get().shape(s) == [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
}

// ---------- bats ----------

pub const BAT_HANGING: u32 = 0x01;

pub fn bat(world: &mut World, m: &mut Mob, seen: &[Seen]) {
    let near = seen.iter().any(|s| s.present && mobs::dist2(&s.pos, &m.pos) < 4.0 * 4.0);
    if m.flags & BAT_HANGING != 0 {
        m.vel = [0.0; 3];
        if near || !solid(world, m.pos, m.size().1 + 0.1) || r() < 0.002 {
            m.flags &= !BAT_HANGING;
        }
        return;
    }
    // Erratic flight: a new spot every so often (never into the sunlight).
    if m.wander.is_none_or(|w| mobs::dist2(&w, &m.pos) < 1.0) || r() < 0.03 {
        // Tends to go up, where it can find a ceiling to hang from.
        m.wander = Some([m.pos[0] + (r() - 0.5) * 12.0, m.pos[1] + (r() - 0.3) * 6.0, m.pos[2] + (r() - 0.5) * 12.0]);
    }
    let w = m.wander.unwrap();
    let dir = mobs::normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
    for i in 0..3 {
        m.vel[i] += (dir[i] * 0.12 - m.vel[i]) * 0.2;
    }
    mobs::face(m, w);
    // Close under a ceiling it may hang from it.
    let h = m.size().1;
    if !near && solid(world, m.pos, h + 0.4) && r() < 0.05 {
        m.flags |= BAT_HANGING;
        m.vel = [0.0; 3];
        m.pos[1] = (m.pos[1] + h + 0.4).floor() - h;
    }
}

// ---------- snow golems ----------

fn hot(biome: &str) -> bool {
    matches!(biome, "desert" | "savanna" | "jungle" | "badlands" | "warm_ocean")
}

pub fn snow_golem(server: &Server, world: &mut World, m: &mut Mob, others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let biome = if m.dim == Dimension::Overworld { world.generator().biome_name(m.pos[0].floor() as i32, m.pos[2].floor() as i32) } else { "nether" };
    // Melts in hot places, in the rain and in water.
    let wet = name_at(world, m.pos, 0.3) == "water" || super::weather::rain_at(server, world, m.dim, m.pos);
    if (hot(biome) || m.dim == Dimension::Nether || wet) && m.age % 20 == 0 {
        m.health -= 1.0;
        m.hurt_time = 10;
    }
    // A trail of snow where it walks.
    if !hot(biome) && m.dim == Dimension::Overworld && m.on_ground && server.rule("mobGriefing") && name_at(world, m.pos, 0.0) == "air" && solid(world, m.pos, -0.5) {
        let at = BlockPos::new(m.pos[0].floor() as i32, m.pos[1].floor() as i32, m.pos[2].floor() as i32);
        actions.push(Action::PlaceBlock(m.dim, at, "snow".into()));
    }
    // Snowballs at monsters within 10 blocks.
    let target = others
        .iter()
        .filter(|o| o.kind.hostile() && mobs::dist2(&o.pos, &m.pos) < 10.0 * 10.0)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))?;
    mobs::face(m, target.pos);
    if m.attack_cooldown == 0 {
        m.attack_cooldown = 20;
        let from = [m.pos[0], m.pos[1] + 1.5, m.pos[2]];
        let to = [target.pos[0] - from[0], target.pos[1] + 0.8 - from[1], target.pos[2] - from[2]];
        let horiz = (to[0] * to[0] + to[2] * to[2]).sqrt();
        let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
        actions.push(Action::Shoot(m.dim, ProjectileKind::Snowball, m.eid, from, [dir[0] * 1.6, dir[1] * 1.6, dir[2] * 1.6]));
    }
    Some((m.pos, 0.0))
}

// ---------- sniffers ----------

/// Sniffer states (client `sniffer_state`).
pub const IDLE: i32 = 0;
const SNIFFING: i32 = 3;
const SEARCHING: i32 = 4;
const DIGGING: i32 = 5;
const RISING: i32 = 6;

fn diggable(name: &str) -> bool {
    matches!(name, "grass_block" | "dirt" | "coarse_dirt" | "podzol" | "rooted_dirt" | "moss_block" | "mud" | "muddy_mangrove_roots")
}

pub fn sniffer(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Laying its egg.
    if m.egg && m.on_ground && name_at(world, m.pos, 0.0) == "air" && diggable(name_at(world, m.pos, -0.5)) {
        m.egg = false;
        let at = BlockPos::new(m.pos[0].floor() as i32, m.pos[1].floor() as i32, m.pos[2].floor() as i32);
        actions.push(Action::PlaceBlock(m.dim, at, "sniffer_egg".into()));
        actions.push(Action::Sound(m.eid, "block.sniffer_egg.plop".into(), sound::NEUTRAL));
    }
    if m.grow < 0 {
        return super::animals::behave(world, m, seen, others, actions);
    }
    m.burst -= 1;
    match m.variant {
        SNIFFING if m.burst <= 0 => {
            // Sniffed out a spot to dig.
            m.variant = SEARCHING;
            m.burst = 200;
            m.wander = None;
            for _ in 0..12 {
                let p = [m.pos[0] + (r() - 0.5) * 16.0, m.pos[1], m.pos[2] + (r() - 0.5) * 16.0];
                for dy in [0.0, 1.0, -1.0] {
                    let q = [p[0].floor() + 0.5, (p[1] + dy).floor(), p[2].floor() + 0.5];
                    if diggable(name_at(world, q, -0.5)) && name_at(world, q, 0.0) == "air" {
                        m.wander = Some(q);
                        break;
                    }
                }
                if m.wander.is_some() {
                    break;
                }
            }
            if m.wander.is_none() {
                m.variant = IDLE;
                m.burst = 600;
            }
            None
        }
        SNIFFING => Some((m.pos, 0.0)),
        SEARCHING => {
            let w = m.wander?;
            if (w[0] - m.pos[0]).hypot(w[2] - m.pos[2]) < 0.8 || m.burst <= 0 {
                m.variant = DIGGING;
                m.burst = 120;
                actions.push(Action::Sound(m.eid, "entity.sniffer.digging".into(), sound::NEUTRAL));
                return Some((m.pos, 0.0));
            }
            Some((mobs::follow_path(world, m, w), m.kind.walk_speed()))
        }
        DIGGING => {
            if m.burst <= 0 {
                m.variant = RISING;
                m.burst = 20;
                if diggable(name_at(world, m.pos, -0.5)) {
                    let seed = if r() < 0.5 { "torchflower_seeds" } else { "pitcher_pod" };
                    actions.push(Action::Drop(m.dim, m.pos, seed.into()));
                    actions.push(Action::Sound(m.eid, "entity.sniffer.drop_seed".into(), sound::NEUTRAL));
                }
            }
            Some((m.pos, 0.0))
        }
        RISING => {
            if m.burst <= 0 {
                m.variant = IDLE;
                m.burst = 400 + (r() * 800.0) as i32;
            }
            Some((m.pos, 0.0))
        }
        _ => {
            if m.burst <= 0 && m.love == 0 {
                m.variant = SNIFFING;
                m.burst = 80;
                actions.push(Action::Sound(m.eid, "entity.sniffer.sniffing".into(), sound::NEUTRAL));
                return Some((m.pos, 0.0));
            }
            super::animals::behave(world, m, seen, others, actions)
        }
    }
}

// ---------- camels ----------

/// Camels sit down for a while now and then (not while ridden).
pub fn camel_rest(m: &mut Mob) -> bool {
    if m.rider.is_some() {
        m.flags &= !CAMEL_SITTING;
        return false;
    }
    if m.flags & CAMEL_SITTING != 0 {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        m.burst -= 1;
        if m.burst <= 0 {
            m.flags &= !CAMEL_SITTING;
            m.pose_tick = m.age as i64;
        }
        return true;
    }
    if r() < 0.0008 {
        m.flags |= CAMEL_SITTING;
        m.burst = 600 + (r() * 1200.0) as i32;
        m.pose_tick = m.age as i64;
        return true;
    }
    false
}

pub const CAMEL_SITTING: u32 = 0x01;

// ---------- piglins out of the Nether ----------

/// Piglins and brutes outside the Nether shake and become zombified piglins after 15 s.
pub fn zombify(m: &mut Mob, actions: &mut Vec<Action>) {
    if !matches!(m.kind, MobKind::Piglin | MobKind::PiglinBrute) || m.dim == Dimension::Nether {
        return;
    }
    m.convert += 1;
    if m.convert >= 300 {
        actions.push(Action::Convert(m.eid, MobKind::ZombifiedPiglin));
    }
}

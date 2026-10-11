//! Wild animals with habits of their own:
//! - Water: cod, salmon, tropical fish and pufferfish (puff up and poison whoever touches
//!   them), squid and glow squid, dolphins (swim with players and give Dolphin's Grace),
//!   tadpoles (grow into frogs). They suffocate out of the water; fish, tadpoles and
//!   axolotls go into a water bucket.
//! - Rabbits hop and run from players; foxes sleep by day, hunt chickens and rabbits and
//!   run from players; turtles go back to their beach to lay eggs, and their babies drop a
//!   scute when they grow up; goats jump and ram; frogs eat small slimes and magma cubes
//!   (magma cubes leave a froglight) and lay frogspawn; axolotls hunt fish and drowned and
//!   play dead; polar bears defend their cubs; pandas sit, roll and sneeze; parrots fly
//!   around, can be tamed with seeds and ride on their owner's shoulder.

use super::animals::Other;
use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{entities, sound, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::BlockPos;
use crate::world::{Dimension, World};
use std::sync::Arc;

pub fn handles(kind: MobKind) -> bool {
    kind.aquatic() || matches!(kind, MobKind::Rabbit | MobKind::Fox | MobKind::Turtle | MobKind::Goat | MobKind::Frog | MobKind::Axolotl | MobKind::PolarBear | MobKind::Panda | MobKind::Parrot)
}

fn r() -> f64 {
    entities::random() as f64
}

fn block(world: &mut World, p: [f64; 3], dy: f64) -> &'static str {
    data::get().block_of_state(world.get_block(BlockPos::new(p[0].floor() as i32, (p[1] + dy).floor() as i32, p[2].floor() as i32))).name.as_str()
}

fn wet(world: &mut World, m: &Mob) -> bool {
    block(world, m.pos, 0.2) == "water"
}

fn nearest<'a>(m: &Mob, it: impl Iterator<Item = &'a Other>) -> Option<&'a Other> {
    it.min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
}

/// Runs away from `from`.
fn flee(world: &mut World, m: &mut Mob, from: [f64; 3], speed: f64) -> Option<([f64; 3], f64)> {
    let away = mobs::normalize([m.pos[0] - from[0], 0.0, m.pos[2] - from[2]]);
    let dest = [m.pos[0] + away[0] * 8.0, m.pos[1], m.pos[2] + away[2] * 8.0];
    Some((mobs::follow_path(world, m, dest), speed))
}

fn scared_of_players<'a>(m: &Mob, seen: &'a [Seen], food: &dyn Fn(&str) -> bool, range: f64) -> Option<&'a Seen> {
    seen.iter()
        .filter(|s| s.present && !s.hands.iter().any(|h| food(h)) && mobs::dist2(&s.pos, &m.pos) < range * range)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
}

/// Per-tick behaviour. `Some(goal)` when it decides where to go.
#[allow(clippy::too_many_arguments)]
pub fn behave(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], day: bool, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if m.kind.aquatic() && !matches!(m.kind, MobKind::Axolotl | MobKind::Turtle | MobKind::Frog) {
        return aquatic(server, world, m, seen, actions);
    }
    match m.kind {
        MobKind::Rabbit => rabbit(world, m, seen, others, actions),
        MobKind::Fox => fox(world, m, seen, others, day, actions),
        MobKind::Turtle => turtle(world, m, seen, others, actions),
        MobKind::Goat => goat(world, m, seen, others, actions),
        MobKind::Frog => frog(world, m, seen, others, actions),
        MobKind::Axolotl => axolotl(world, m, seen, others, actions),
        MobKind::PolarBear => polar_bear(world, m, seen, others, actions),
        MobKind::Panda => panda(world, m, seen, others, actions),
        MobKind::Parrot => parrot(world, m, seen, others, actions),
        _ => None,
    }
}

/// What the client needs to redraw (when it changes, the metadata is sent again).
pub fn look(m: &Mob) -> (u32, u32, bool, Option<i32>) {
    (puff_state(m), m.flags, m.stand > 0, m.tongue)
}

// ---------- in the water ----------

pub fn puff_state(m: &Mob) -> u32 {
    match m.puff {
        0 => 0,
        p if p > 80 || p < 20 => 1,
        _ => 2,
    }
}

/// A random spot in the water within reach (for swimmers to head to).
fn water_goal(world: &mut World, m: &Mob, range: f64) -> Option<[f64; 3]> {
    for _ in 0..6 {
        let p = [m.pos[0] + (r() - 0.5) * 2.0 * range, m.pos[1] + (r() - 0.5) * 6.0, m.pos[2] + (r() - 0.5) * 2.0 * range];
        if block(world, p, 0.0) == "water" {
            return Some([p[0].floor() + 0.5, p[1].floor() + 0.3, p[2].floor() + 0.5]);
        }
    }
    None
}

/// Out of the water: fish flop about and everything that breathes water slowly suffocates.
fn stranded(m: &mut Mob, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    m.air -= 1;
    if m.air <= -20 {
        m.air = 0;
        m.health -= 2.0;
        m.hurt_time = 10;
    }
    if m.on_ground && r() < 0.1 {
        let a = r() * std::f64::consts::TAU;
        m.vel = [a.cos() * 0.15, 0.4, a.sin() * 0.15];
        m.knockback_ticks = 1;
        if m.kind.fish() {
            actions.push(Action::Sound(m.eid, format!("entity.{}.flop", m.kind.name()), sound::NEUTRAL));
        }
    }
    Some((m.pos, 0.0))
}

fn aquatic(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if !wet(world, m) {
        return stranded(m, actions);
    }
    m.air = m.max_air();
    let speed = m.kind.walk_speed();
    // Pufferfish puff up when someone comes close, and poison whoever touches them.
    if m.kind == MobKind::Pufferfish {
        let close = seen.iter().filter(|s| s.present && mobs::dist2(&s.pos, &m.pos) < 2.5 * 2.5).collect::<Vec<_>>();
        if !close.is_empty() {
            m.puff = m.puff.max(60);
        }
        m.puff = m.puff.saturating_sub(1);
        if puff_state(m) == 2 {
            for s in close.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 1.3 * 1.3) {
                if m.attack_cooldown == 0 {
                    m.attack_cooldown = 20;
                    actions.push(Action::Effect(s.player.clone(), "poison", 0, 60 * server.config.difficulty.max(1) as i32));
                    actions.push(Action::Sound(m.eid, "entity.puffer_fish.sting".into(), sound::NEUTRAL));
                }
            }
        }
    }
    // Dolphins swim alongside players in the water and lend them speed.
    if m.kind == MobKind::Dolphin {
        let swimmer = seen.iter().filter(|s| s.present && mobs::dist2(&s.pos, &m.pos) < 10.0 * 10.0).find(|s| block(world, s.pos, 0.5) == "water");
        if let Some(s) = swimmer {
            if mobs::dist2(&s.pos, &m.pos) < 5.0 * 5.0 && m.age % 20 == 0 {
                actions.push(Action::Effect(s.player.clone(), "dolphins_grace", 0, 100));
            }
            mobs::face(m, s.pos);
            if mobs::dist2(&s.pos, &m.pos) > 3.0 * 3.0 {
                return Some((s.pos, speed * 1.2));
            }
        }
    }
    // Panic after a hit: swims off fast.
    let speed = if m.panic > 0 { speed * 2.0 } else { speed };
    let reached = m.wander.is_some_and(|w| mobs::dist2(&w, &m.pos) < 1.0);
    if m.wander.is_none() || reached || r() < 1.0 / 100.0 || m.panic % 20 == 19 {
        m.wander = water_goal(world, m, if m.kind == MobKind::Dolphin { 12.0 } else { 6.0 });
    }
    if let Some(w) = m.wander {
        mobs::face(m, w);
        return Some((w, speed));
    }
    Some((m.pos, 0.0))
}

// ---------- rabbits ----------

fn hop(m: &mut Mob, goal: &Option<([f64; 3], f64)>) {
    if let Some((g, speed)) = goal {
        if *speed > 0.0 && m.on_ground && (g[0] - m.pos[0]).hypot(g[2] - m.pos[2]) > 0.5 && m.age % 6 == 0 {
            m.vel[1] = 0.33;
        }
    }
}

fn rabbit(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let goal = if let Some(s) = scared_of_players(m, seen, &|h| super::animals::is_food(MobKind::Rabbit, h), 8.0) {
        flee(world, m, s.pos, m.kind.walk_speed() * 2.0)
    } else if let Some(w) = nearest(m, others.iter().filter(|o| matches!(o.kind, MobKind::Wolf | MobKind::Fox) && !o.tame && mobs::dist2(&o.pos, &m.pos) < 10.0 * 10.0)) {
        flee(world, m, w.pos, m.kind.walk_speed() * 2.2)
    } else {
        super::animals::behave(world, m, seen, others, actions)
    };
    hop(m, &goal);
    goal
}

// ---------- foxes ----------

pub const FOX_SLEEPING: u32 = 0x20;
pub const FOX_POUNCING: u32 = 0x10;

fn fox(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], day: bool, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if m.on_ground && m.knockback_ticks == 0 {
        m.flags &= !FOX_POUNCING;
    }
    let threat = if m.owner.is_none() { scared_of_players(m, seen, &|h| matches!(h, "sweet_berries" | "glow_berries"), 8.0) } else { None };
    if let Some(s) = threat {
        m.flags &= !FOX_SLEEPING;
        return flee(world, m, s.pos, m.kind.walk_speed() * 1.8);
    }
    // By day it naps in the shade of the trees (unless in love or hunting).
    if day && m.love == 0 && m.prey.is_none() && m.panic == 0 {
        if m.flags & FOX_SLEEPING == 0 && r() < 0.01 {
            m.flags |= FOX_SLEEPING;
        }
        if m.flags & FOX_SLEEPING != 0 {
            m.vel[0] = 0.0;
            m.vel[2] = 0.0;
            return Some((m.pos, 0.0));
        }
    } else {
        m.flags &= !FOX_SLEEPING;
    }
    // Hunts chickens and rabbits.
    let current = m.prey.and_then(|e| others.iter().find(|o| o.eid == e)).filter(|o| mobs::dist2(&o.pos, &m.pos) < 20.0 * 20.0);
    let prey = current.or_else(|| nearest(m, others.iter().filter(|o| matches!(o.kind, MobKind::Chicken | MobKind::Rabbit) && mobs::dist2(&o.pos, &m.pos) < 12.0 * 12.0)));
    if let Some(p) = prey {
        m.prey = Some(p.eid);
        mobs::face(m, p.pos);
        let d = mobs::dist2(&p.pos, &m.pos).sqrt();
        if d < 4.0 && d > 1.5 && m.on_ground && m.attack_cooldown == 0 {
            // Pounce.
            let dir = mobs::normalize([p.pos[0] - m.pos[0], 0.0, p.pos[2] - m.pos[2]]);
            m.vel = [dir[0] * 0.4, 0.45, dir[2] * 0.4];
            m.knockback_ticks = 6;
            m.attack_cooldown = 15;
            m.flags |= FOX_POUNCING;
        }
        if d < 1.5 && m.attack_cooldown <= 5 {
            m.attack_cooldown = 20;
            actions.push(Action::HurtMob(p.eid, 2.0, m.eid, None));
        }
        return Some((mobs::follow_path(world, m, p.pos), m.kind.walk_speed() * 1.4));
    }
    m.prey = None;
    super::animals::behave(world, m, seen, others, actions)
}

// ---------- turtles ----------

fn turtle(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let home = *m.home.get_or_insert(BlockPos::new(m.pos[0].floor() as i32, m.pos[1].floor() as i32, m.pos[2].floor() as i32));
    let speed = if wet(world, m) { 0.1 } else { m.kind.walk_speed() };
    if m.egg {
        let h = [home.x as f64 + 0.5, home.y as f64, home.z as f64 + 0.5];
        if mobs::dist2(&h, &m.pos) < 4.0 * 4.0 && !wet(world, m) && block(world, m.pos, -0.5) == "sand" && block(world, m.pos, 0.0) == "air" {
            m.egg = false;
            let at = BlockPos::new(m.pos[0].floor() as i32, m.pos[1].floor() as i32, m.pos[2].floor() as i32);
            let n = 1 + (r() * 4.0) as i32;
            actions.push(Action::PlaceBlock(m.dim, at, format!("turtle_egg[eggs={n}]")));
            actions.push(Action::Sound(m.eid, "entity.turtle.lay_egg".into(), sound::NEUTRAL));
            return Some((m.pos, 0.0));
        }
        return Some((if wet(world, m) { h } else { mobs::follow_path(world, m, h) }, speed));
    }
    if let Some(g) = super::animals::behave(world, m, seen, others, actions) {
        return Some(g);
    }
    if wet(world, m) {
        let reached = m.wander.is_some_and(|w| mobs::dist2(&w, &m.pos) < 1.5);
        if m.wander.is_none() || reached || r() < 0.005 {
            m.wander = water_goal(world, m, 8.0);
        }
        return m.wander.map(|w| (w, speed));
    }
    None
}

// ---------- goats ----------

fn goat(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Ramming: every now and then it charges at someone nearby and knocks them back.
    if m.burst > 0 {
        m.burst -= 1;
        let target = m.target.and_then(|e| seen.iter().find(|s| s.player.eid == e && s.present));
        if let Some(t) = target {
            mobs::face(m, t.pos);
            if mobs::dist2(&t.pos, &m.pos) < 1.6 * 1.6 {
                let dir = mobs::normalize([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]);
                let k = if m.variant == 1 { 3.0 } else { 2.0 };
                actions.push(Action::Knock(t.player.clone(), [dir[0] * k * 0.5, 0.4, dir[2] * k * 0.5]));
                if t.targetable {
                    let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
                    actions.push(Action::Hurt(t.player.clone(), if m.variant == 1 { 2.0 } else { 1.0 }, "mob_attack", by));
                }
                actions.push(Action::Sound(m.eid, "entity.goat.ram_impact".into(), sound::NEUTRAL));
                m.burst = 0;
                m.target = None;
                m.approach = 600.0 + r() * 5400.0;
                return Some((m.pos, 0.0));
            }
            return Some((t.pos, 0.25));
        }
        m.burst = 0;
    }
    m.approach -= 1.0;
    if m.approach <= 0.0 {
        m.approach = 600.0 + r() * 5400.0;
        // Screaming goats ram far more often.
        if m.variant == 1 {
            m.approach /= 3.0;
        }
        if let Some(t) = seen.iter().filter(|s| s.present && mobs::dist2(&s.pos, &m.pos) < 16.0 * 16.0 && mobs::dist2(&s.pos, &m.pos) > 4.0 * 4.0).min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos))) {
            m.target = Some(t.player.eid);
            m.burst = 60;
            actions.push(Action::Sound(m.eid, "entity.goat.prepare_ram".into(), sound::NEUTRAL));
        }
    }
    let goal = super::animals::behave(world, m, seen, others, actions);
    // Goats leap about now and then.
    if m.on_ground && r() < 0.004 {
        let a = r() * std::f64::consts::TAU;
        m.vel = [a.cos() * 0.25, 0.7, a.sin() * 0.25];
        m.knockback_ticks = 10;
        actions.push(Action::Sound(m.eid, "entity.goat.long_jump".into(), sound::NEUTRAL));
    }
    goal
}

// ---------- frogs ----------

fn frog(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Tongue out: the prey is gone.
    if let Some(prey) = m.tongue {
        if m.attack_cooldown <= 10 {
            m.tongue = None;
            if let Some(o) = others.iter().find(|o| o.eid == prey) {
                let drop = match o.kind {
                    MobKind::MagmaCube => ["ochre_froglight", "verdant_froglight", "pearlescent_froglight"][m.variant.clamp(0, 2) as usize],
                    _ => "slime_ball",
                };
                actions.push(Action::Eaten(o.eid, m.dim, o.pos, drop.into()));
            }
        }
        return Some((m.pos, 0.0));
    }
    // Small slimes and magma cubes within reach of its tongue.
    let prey = nearest(m, others.iter().filter(|o| matches!(o.kind, MobKind::Slime | MobKind::MagmaCube) && o.size == 1 && mobs::dist2(&o.pos, &m.pos) < 10.0 * 10.0));
    if let Some(p) = prey {
        mobs::face(m, p.pos);
        if mobs::dist2(&p.pos, &m.pos) < 2.5 * 2.5 && m.attack_cooldown == 0 {
            m.tongue = Some(p.eid);
            m.attack_cooldown = 20;
            actions.push(Action::Sound(m.eid, "entity.frog.tongue".into(), sound::NEUTRAL));
            return Some((m.pos, 0.0));
        }
        return Some((mobs::follow_path(world, m, p.pos), m.kind.walk_speed() * 1.3));
    }
    // With spawn to lay: to the water's surface.
    if m.egg {
        for _ in 0..8 {
            let p = BlockPos::new((m.pos[0] + (r() - 0.5) * 8.0).floor() as i32, m.pos[1].floor() as i32 + (r() * 3.0) as i32 - 2, (m.pos[2] + (r() - 0.5) * 8.0).floor() as i32);
            let reg = data::get();
            if reg.block_of_state(world.get_block(p)).name == "water" && reg.is_air(world.get_block(BlockPos::new(p.x, p.y + 1, p.z))) {
                m.egg = false;
                actions.push(Action::PlaceBlock(m.dim, BlockPos::new(p.x, p.y + 1, p.z), "frogspawn".into()));
                actions.push(Action::Sound(m.eid, "entity.frog.lay_spawn".into(), sound::NEUTRAL));
                break;
            }
        }
    }
    let goal = super::animals::behave(world, m, seen, others, actions);
    if m.on_ground && goal.is_some_and(|g| g.1 > 0.0) && r() < 0.05 {
        m.vel[1] = 0.45;
    }
    goal
}

// ---------- axolotls ----------

fn axolotl(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let in_water = wet(world, m);
    if !in_water {
        // Dries out slowly (5 minutes).
        m.air -= 1;
        if m.air <= -20 {
            m.air = 0;
            m.health -= 1.0;
            m.hurt_time = 10;
        }
    } else {
        m.air = m.max_air();
    }
    // Playing dead: lies still and heals.
    if m.stand > 0 {
        m.stand -= 1;
        if m.age % 20 == 0 {
            m.health = (m.health + 1.0).min(m.max_health());
        }
        return Some((m.pos, 0.0));
    }
    if m.hurt_time == 9 && m.health < m.max_health() * 0.6 && r() < 0.33 {
        m.stand = 200;
        return Some((m.pos, 0.0));
    }
    // Hunts fish, squid and drowned in the water.
    let current = m.prey.and_then(|e| others.iter().find(|o| o.eid == e)).filter(|o| mobs::dist2(&o.pos, &m.pos) < 16.0 * 16.0);
    let prey = current.or_else(|| {
        nearest(m, others.iter().filter(|o| (o.kind.fish() || matches!(o.kind, MobKind::Squid | MobKind::GlowSquid | MobKind::Drowned)) && mobs::dist2(&o.pos, &m.pos) < 8.0 * 8.0))
    });
    if let (Some(p), true) = (prey, in_water) {
        m.prey = Some(p.eid);
        mobs::face(m, p.pos);
        if mobs::dist2(&p.pos, &m.pos) < 1.5 * 1.5 && m.attack_cooldown == 0 {
            m.attack_cooldown = 20;
            actions.push(Action::HurtMob(p.eid, 2.0, m.eid, None));
        }
        return Some((p.pos, 0.12));
    }
    m.prey = None;
    if let Some(g) = super::animals::behave(world, m, seen, others, actions) {
        return Some(g);
    }
    if in_water {
        let reached = m.wander.is_some_and(|w| mobs::dist2(&w, &m.pos) < 1.0);
        if m.wander.is_none() || reached || r() < 0.01 {
            m.wander = water_goal(world, m, 6.0);
        }
        return m.wander.map(|w| (w, 0.08));
    }
    None
}

// ---------- polar bears ----------

fn polar_bear(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // A mother whose cub has a player close turns on them.
    if m.grow >= 0 && m.angry == 0 {
        let cubs: Vec<[f64; 3]> = others.iter().filter(|o| o.kind == MobKind::PolarBear && o.grow < 0 && mobs::dist2(&o.pos, &m.pos) < 16.0 * 16.0).map(|o| o.pos).collect();
        if let Some(s) = seen.iter().find(|s| s.targetable && cubs.iter().any(|c| mobs::dist2(c, &s.pos) < 6.0 * 6.0)) {
            m.angry = 400;
            m.target = Some(s.player.eid);
        }
    }
    m.stand = m.stand.saturating_sub(1);
    super::animals::behave(world, m, seen, others, actions)
}

// ---------- pandas ----------

pub const PANDA_SNEEZING: u32 = 0x02;
pub const PANDA_ROLLING: u32 = 0x04;
pub const PANDA_SITTING: u32 = 0x08;

/// Main gene (the one that shows): 0 normal, 1 lazy, 2 worried, 3 playful, 4 brown, 5 weak, 6 aggressive.
pub fn panda_gene(m: &Mob) -> i32 {
    m.variant & 0xff
}

fn panda(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Sitting down, rolling over (playful ones) and sneezing (cubs, which drop a slimeball).
    if m.flags & (PANDA_SITTING | PANDA_ROLLING | PANDA_SNEEZING) != 0 {
        m.burst -= 1;
        if m.burst <= 0 {
            if m.flags & PANDA_SNEEZING != 0 && r() < 0.5 {
                actions.push(Action::Drop(m.dim, m.pos, "slime_ball".into()));
            }
            m.flags &= !(PANDA_SITTING | PANDA_ROLLING | PANDA_SNEEZING);
        }
        if m.flags & PANDA_ROLLING != 0 {
            let a = (m.yaw as f64).to_radians();
            return Some(([m.pos[0] - a.sin(), m.pos[1], m.pos[2] + a.cos()], 0.05));
        }
        return Some((m.pos, 0.0));
    }
    let lazy = panda_gene(m) == 1;
    if r() < if lazy { 0.004 } else { 0.0015 } {
        m.flags |= PANDA_SITTING;
        m.burst = 100 + (r() * 200.0) as i32;
    } else if panda_gene(m) == 3 && r() < 0.002 {
        m.flags |= PANDA_ROLLING;
        m.burst = 30;
    } else if m.grow < 0 && r() < 0.0005 {
        m.flags |= PANDA_SNEEZING;
        m.burst = 20;
        actions.push(Action::Sound(m.eid, "entity.panda.sneeze".into(), sound::NEUTRAL));
    }
    super::animals::behave(world, m, seen, others, actions)
}

// ---------- parrots ----------

fn parrot(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if m.sitting {
        m.vel = [0.0, -0.08, 0.0];
        return Some((m.pos, 0.0));
    }
    if let Some(owner) = m.owner.and_then(|u| seen.iter().find(|s| s.player.uuid == u && s.present)) {
        let d = mobs::dist2(&owner.pos, &m.pos).sqrt();
        // Lands on its owner's shoulder.
        if d < 1.5 && m.attack_cooldown == 0 && owner.on_ground && !owner.shoulder {
            actions.push(Action::Shoulder(owner.player.clone(), m.eid));
            m.attack_cooldown = 100;
            return Some((m.pos, 0.0));
        }
        if d > 12.0 {
            m.pos = [owner.pos[0], owner.pos[1] + 1.5, owner.pos[2]];
            m.sent = [f64::MAX; 3];
        }
        if d > 3.0 {
            return Some(([owner.pos[0], owner.pos[1] + 1.5, owner.pos[2]], 0.15));
        }
    }
    let _ = others;
    // Flutters between nearby spots, a little above the ground.
    let reached = m.wander.is_some_and(|w| mobs::dist2(&w, &m.pos) < 1.0);
    if m.wander.is_none() || reached || r() < 0.005 {
        let g = [m.pos[0] + (r() - 0.5) * 12.0, m.pos[1] + (r() - 0.4) * 4.0, m.pos[2] + (r() - 0.5) * 12.0];
        m.wander = (block(world, g, 0.0) == "air" || block(world, g, 0.0).ends_with("leaves")).then_some(g);
    }
    if let Some(w) = m.wander {
        mobs::face(m, w);
        let dir = mobs::normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
        m.vel[1] += dir[1] * 0.02;
        return Some((w, 0.1));
    }
    let _ = seen;
    Some((m.pos, 0.0))
}

// ---------- hatching ----------

/// Turtle eggs and frogspawn hatched (from the blocks' random ticks).
pub fn hatch(server: &Arc<Server>, dim: Dimension, hatched: Vec<(BlockPos, &'static str, i32)>) {
    for (p, what, n) in hatched {
        let at = [p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5];
        let kind = if what == "turtle_egg" { MobKind::Turtle } else { MobKind::Tadpole };
        for _ in 0..n {
            let eid = mobs::spawn(server, dim, kind, [at[0] + (r() - 0.5) * 0.4, at[1], at[2] + (r() - 0.5) * 0.4]);
            if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
                m.grow = super::animals::BABY;
                if kind == MobKind::Turtle {
                    m.home = Some(p);
                }
            }
        }
        let snd = if kind == MobKind::Turtle { "entity.turtle.egg_hatch" } else { "block.frogspawn.hatch" };
        sound::at(server, dim, at, snd, sound::BLOCK, 1.0, 1.0);
    }
}

/// Frog colour from the biome where it grew up: 0 temperate, 1 warm, 2 cold.
pub fn frog_variant(biome: &str) -> i32 {
    match biome {
        "desert" | "savanna" | "jungle" | "warm_ocean" => 1,
        b if b.starts_with("snowy") || b.starts_with("frozen") => 2,
        _ => 0,
    }
}

/// Variants that depend on where the animal is (rabbits, foxes, frogs).
pub fn biome_variant(kind: MobKind, biome: &str) -> Option<i32> {
    let snowy = biome.starts_with("snowy") || biome.starts_with("frozen");
    Some(match kind {
        MobKind::Rabbit if biome == "desert" => 4,
        MobKind::Rabbit if snowy => {
            if r() < 0.8 {
                1
            } else {
                3
            }
        }
        MobKind::Rabbit => [0, 0, 0, 0, 0, 5, 5, 5, 5, 2][(r() * 10.0) as usize % 10],
        MobKind::Fox => i32::from(snowy),
        MobKind::Frog => frog_variant(biome),
        _ => return None,
    })
}

/// The item a fish, a tadpole or an axolotl turns into when scooped up with a water bucket.
pub fn bucket_of(m: &Mob) -> Option<ItemStack> {
    let name = match m.kind {
        MobKind::Cod => "cod_bucket",
        MobKind::Salmon => "salmon_bucket",
        MobKind::TropicalFish => "tropical_fish_bucket",
        MobKind::Pufferfish => "pufferfish_bucket",
        MobKind::Axolotl => "axolotl_bucket",
        MobKind::Tadpole => "tadpole_bucket",
        _ => return None,
    };
    let mut it = ItemStack::new(data::get().item_id(name)?, 1);
    match m.kind {
        MobKind::TropicalFish => it.tag_mut().set("BucketVariantTag", crate::protocol::Nbt::Int(m.variant)),
        MobKind::Axolotl => it.tag_mut().set("Variant", crate::protocol::Nbt::Int(m.variant)),
        _ => {}
    }
    Some(it)
}

/// The mob inside a bucket item, and its variant.
pub fn from_bucket(item: &ItemStack) -> Option<(MobKind, Option<i32>)> {
    let kind = match item.name() {
        "cod_bucket" => MobKind::Cod,
        "salmon_bucket" => MobKind::Salmon,
        "tropical_fish_bucket" => MobKind::TropicalFish,
        "pufferfish_bucket" => MobKind::Pufferfish,
        "axolotl_bucket" => MobKind::Axolotl,
        "tadpole_bucket" => MobKind::Tadpole,
        _ => return None,
    };
    let tag = |k: &str| item.nbt.as_ref().and_then(|n| n.get(k)).and_then(crate::protocol::Nbt::as_i64).map(|v| v as i32);
    Some((kind, tag("BucketVariantTag").or_else(|| tag("Variant"))))
}

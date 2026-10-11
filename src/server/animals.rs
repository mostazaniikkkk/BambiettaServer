//! Animals: breeding, babies, following food, and tamed wolves.
//! - Feeding an adult its food (wheat for cows and sheep, carrots/potatoes/beetroot for pigs,
//!   seeds for chickens, meat for tamed wolves) puts it in love: it looks for a partner in love
//!   and they have a baby (5 min cooldown, experience for the player). Babies grow up in 20
//!   min; feeding them speeds it up.
//! - Animals follow a player holding their food.
//! - Wolves: tamed with bones (1 in 3). A tamed wolf sits or stands when its owner uses it,
//!   follows the owner (teleporting when left behind), attacks whatever the owner hits or
//!   whatever hurts the owner, and heals with meat. Wolves, wild or tamed, hunt skeletons
//!   above anything else and bite them for double damage.
//! - Cats (villages): tamed with raw cod or salmon (1 in 3); they sit and follow like wolves.
//!   Ocelots (jungles) run from players until fed fish, then trust them. Creepers fear both.
//! - Who hunts whom: cats go for creepers, stray cats fight each other, wild wolves go for
//!   cats; a cat or a wolf attacked by another mob fights back.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{entities, Player, Server};
use crate::protocol::{id, packet, Writer};
use crate::world::World;
use std::sync::Arc;

/// Ticks a baby takes to grow up, and the cooldown after breeding.
pub const BABY: i32 = -24000;
const COOLDOWN: i32 = 6000;
const LOVE: u32 = 600;

/// What other mobs of the dimension look like this tick (partners, prey, owners' wolves).
#[derive(Clone)]
pub struct Other {
    pub eid: i32,
    pub kind: MobKind,
    pub pos: [f64; 3],
    pub love: u32,
    pub grow: i32,
    /// Has an owner (tamed).
    pub tame: bool,
    /// Slime or magma cube size.
    pub size: i32,
}

pub fn snapshot<'a>(mobs: impl Iterator<Item = &'a Mob>) -> Vec<Other> {
    mobs.filter(|m| m.death_ticks.is_none()).map(|m| Other { eid: m.eid, kind: m.kind, pos: m.pos, love: m.love, grow: m.grow, tame: m.owner.is_some(), size: m.size }).collect()
}

pub fn breeds(kind: MobKind) -> bool {
    matches!(
        kind,
        MobKind::Cow
            | MobKind::Pig
            | MobKind::Sheep
            | MobKind::Chicken
            | MobKind::Wolf
            | MobKind::Cat
            | MobKind::Ocelot
            | MobKind::Horse
            | MobKind::Donkey
            | MobKind::Mule
            | MobKind::Llama
            | MobKind::Rabbit
            | MobKind::Fox
            | MobKind::Turtle
            | MobKind::Goat
            | MobKind::Frog
            | MobKind::Axolotl
            | MobKind::PolarBear
            | MobKind::Panda
    )
}

/// Food that makes an animal fall in love (and that it follows).
pub fn is_food(kind: MobKind, item: &str) -> bool {
    match kind {
        MobKind::Cow | MobKind::Sheep => item == "wheat",
        MobKind::Pig => matches!(item, "carrot" | "potato" | "beetroot"),
        MobKind::Chicken => matches!(item, "wheat_seeds" | "melon_seeds" | "pumpkin_seeds" | "beetroot_seeds" | "torchflower_seeds"),
        MobKind::Wolf => is_meat(item),
        MobKind::Cat | MobKind::Ocelot => matches!(item, "cod" | "salmon"),
        MobKind::Bee => super::bees::is_flower(item),
        k if super::horses::is_horse(k) => super::horses::is_food(k, item),
        MobKind::Rabbit => matches!(item, "carrot" | "golden_carrot" | "dandelion"),
        MobKind::Fox => matches!(item, "sweet_berries" | "glow_berries"),
        MobKind::Turtle => item == "seagrass",
        MobKind::Goat => item == "wheat",
        MobKind::Frog => item == "slime_ball",
        MobKind::Axolotl => item == "tropical_fish_bucket",
        MobKind::Panda => item == "bamboo",
        _ => false,
    }
}

fn is_meat(item: &str) -> bool {
    matches!(item, "beef" | "cooked_beef" | "porkchop" | "cooked_porkchop" | "chicken" | "cooked_chicken" | "mutton" | "cooked_mutton" | "rabbit" | "cooked_rabbit" | "rotten_flesh")
}

pub fn status(eid: i32, s: i8) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_STATUS);
    p.i32(eid).i8(s);
    p
}

/// Per-tick behaviour of animals and tamed wolves. `Some(goal)` when it decides where to go.
pub fn behave(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    // Growing up / breeding cooldown.
    if m.grow < 0 {
        m.grow += 1;
        if m.grow == 0 {
            actions.push(Action::Meta(mobs::metadata(m), m.eid));
            // A turtle that grows up sheds a scute.
            if m.kind == MobKind::Turtle {
                actions.push(Action::Drop(m.dim, m.pos, "scute".into()));
            }
        }
    } else if m.grow > 0 {
        m.grow -= 1;
    }
    let speed = m.kind.walk_speed();
    // A wild ocelot runs from players (unless they hold fish).
    if m.kind == MobKind::Ocelot && m.owner.is_none() {
        let near = seen.iter().filter(|s| s.present && !s.hands.iter().any(|h| matches!(*h, "cod" | "salmon")) && mobs::dist2(&s.pos, &m.pos) < 8.0 * 8.0).min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
        if let Some(s) = near {
            let away = mobs::normalize([m.pos[0] - s.pos[0], 0.0, m.pos[2] - s.pos[2]]);
            let dest = [m.pos[0] + away[0] * 8.0, m.pos[1], m.pos[2] + away[2] * 8.0];
            return Some((mobs::follow_path(world, m, dest), m.kind.walk_speed() * 1.8));
        }
    }
    if matches!(m.kind, MobKind::Wolf | MobKind::Cat) && m.owner.is_some() {
        if let Some(g) = tamed_wolf(world, m, seen, others, actions) {
            return Some(g);
        }
    } else if matches!(m.kind, MobKind::Wolf | MobKind::Cat) && m.target.is_none() {
        // Wild wolves and stray cats hunt too.
        if let Some(g) = hunt(world, m, others, actions) {
            return Some(g);
        }
    }
    if !breeds(m.kind) || m.panic > 0 {
        return None;
    }
    // In love: hearts, and off to the closest partner in love.
    if m.love > 0 {
        m.love -= 1;
        if m.love % 10 == 0 {
            actions.push(Action::Meta(status(m.eid, 18), m.eid));
        }
        let partner = others
            .iter()
            .filter(|o| o.eid != m.eid && (o.kind == m.kind || matches!((m.kind, o.kind), (MobKind::Horse, MobKind::Donkey) | (MobKind::Donkey, MobKind::Horse))) && o.love > 0 && o.grow == 0 && mobs::dist2(&o.pos, &m.pos) < 8.0 * 8.0)
            .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
        if let Some(p) = partner {
            if mobs::dist2(&p.pos, &m.pos) < 1.6 * 1.6 {
                // One of the two (the lower eid) has the baby.
                if m.eid < p.eid {
                    let at = [(m.pos[0] + p.pos[0]) / 2.0, m.pos[1].max(p.pos[1]), (m.pos[2] + p.pos[2]) / 2.0];
                    actions.push(Action::Breed(m.dim, m.kind, at, (m.eid, p.eid), m.love_by, m.owner));
                }
                return Some((m.pos, 0.0));
            }
            mobs::face(m, p.pos);
            return Some((p.pos, speed * 1.2));
        }
    }
    // Someone holding its food: follow (keeping a couple of blocks away).
    let food = seen
        .iter()
        .filter(|s| s.present && s.hands.iter().any(|h| is_food(m.kind, h)) && mobs::dist2(&s.pos, &m.pos) < 10.0 * 10.0)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
    if let Some(s) = food {
        mobs::face(m, s.pos);
        if mobs::dist2(&s.pos, &m.pos) < 2.5 * 2.5 {
            m.vel[0] = 0.0;
            m.vel[2] = 0.0;
            return Some((m.pos, 0.0));
        }
        return Some((mobs::follow_path(world, m, s.pos), speed * 1.2));
    }
    None
}

/// What a hunter goes for, best first: wolves (wild or tamed) want skeletons above all, and
/// wild wolves also cats; cats want creepers, and stray cats fight each other.
fn wants(m: &Mob, o: &Other) -> Option<u8> {
    match (m.kind, o.kind) {
        (MobKind::Wolf, MobKind::Skeleton | MobKind::Stray) => Some(0),
        (MobKind::Wolf, MobKind::Cat) if m.owner.is_none() => Some(1),
        (MobKind::Cat, MobKind::Creeper) => Some(0),
        (MobKind::Cat, MobKind::Cat) if m.owner.is_none() && !o.tame && o.eid != m.eid => Some(1),
        _ => None,
    }
}

/// A wolf or a cat goes for its prey: what it is fighting already (its owner's enemy, or
/// whoever attacked it) unless something it wants more is close (see `wants`). Wolves bite
/// skeletons for double damage.
fn hunt(world: &mut World, m: &mut Mob, others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let best = others
        .iter()
        .filter_map(|o| wants(m, o).map(|w| (w, o)))
        .filter(|(_, o)| mobs::dist2(&o.pos, &m.pos) < 14.0 * 14.0)
        .min_by(|a, b| a.0.cmp(&b.0).then(mobs::dist2(&a.1.pos, &m.pos).total_cmp(&mobs::dist2(&b.1.pos, &m.pos))));
    let current = m.prey.and_then(|e| others.iter().find(|o| o.eid == e)).filter(|o| mobs::dist2(&o.pos, &m.pos) < 24.0 * 24.0);
    let prey = match (current, best) {
        // The favourite (rank 0) wins over anything else.
        (Some(c), Some((0, b))) if wants(m, c) != Some(0) => Some(b),
        (Some(c), _) => Some(c),
        (None, Some((_, b))) => Some(b),
        (None, None) => None,
    };
    let Some(prey) = prey else {
        m.prey = None;
        return None;
    };
    m.prey = Some(prey.eid);
    let d = mobs::dist2(&prey.pos, &m.pos).sqrt();
    mobs::face(m, prey.pos);
    if d < 1.8 && m.attack_cooldown == 0 {
        m.attack_cooldown = 20;
        let bite = match (m.kind, prey.kind) {
            (MobKind::Wolf, MobKind::Skeleton | MobKind::Stray) => 8.0,
            (MobKind::Wolf, _) => 4.0,
            _ => 3.0,
        };
        actions.push(Action::HurtMob(prey.eid, bite, m.eid, None));
    }
    Some((mobs::follow_path(world, m, prey.pos), m.kind.walk_speed() * 1.4))
}

/// A tamed wolf: sits, fights its owner's enemies, follows the owner.
fn tamed_wolf(world: &mut World, m: &mut Mob, seen: &[Seen], others: &[Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let speed = m.kind.walk_speed();
    if m.sitting {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        m.prey = None;
        return Some((m.pos, 0.0));
    }
    // Its owner's enemy, or its favourite prey.
    if let Some(g) = hunt(world, m, others, actions) {
        return Some(g);
    }
    // A tamed cat with a bed within 20 blocks stays around it instead of following.
    if m.kind == MobKind::Cat {
        if (m.age as i32 + m.eid) % 100 == 0 {
            m.anchor = find_bed(world, m.pos, 20);
        }
        if let Some(bed) = m.anchor {
            if mobs::dist2(&bed, &m.pos) > 6.0 * 6.0 {
                return Some((mobs::follow_path(world, m, bed), speed));
            }
            // Close enough: it strolls around the bed, never more than 4 blocks off.
            if m.wander.is_none_or(|w| mobs::dist2(&w, &bed) > 4.5 * 4.5 || (w[0] - m.pos[0]).powi(2) + (w[2] - m.pos[2]).powi(2) < 1.0) {
                m.wander = None;
                if entities::random() < 0.02 {
                    let a = entities::random() as f64 * std::f64::consts::TAU;
                    let r = entities::random() as f64 * 4.0;
                    m.wander = Some([bed[0] + a.cos() * r, bed[1], bed[2] + a.sin() * r]);
                }
            }
            return Some(match m.wander {
                Some(w) => (mobs::follow_path(world, m, w), speed * 0.6),
                None => (m.pos, 0.0),
            });
        }
    }
    let owner = seen.iter().find(|s| Some(s.player.uuid) == m.owner)?;
    let d = mobs::dist2(&owner.pos, &m.pos).sqrt();
    if d > 12.0 {
        // Left behind: appears next to the owner.
        let a = entities::random() as f64 * std::f64::consts::TAU;
        m.pos = [owner.pos[0] + a.cos() * 1.5, owner.pos[1], owner.pos[2] + a.sin() * 1.5];
        m.vel = [0.0; 3];
        m.path.clear();
        return Some((m.pos, 0.0));
    }
    if d > 4.0 {
        return Some((mobs::follow_path(world, m, owner.pos), speed * 1.4));
    }
    None
}

/// The nearest bed within `r` blocks (centre of its foot block).
fn find_bed(world: &mut World, p: [f64; 3], r: i32) -> Option<[f64; 3]> {
    let reg = crate::data::get();
    let (cx, cy, cz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
    let mut best: Option<(i32, [f64; 3])> = None;
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -4..=4 {
                let q = crate::protocol::BlockPos::new(cx + dx, cy + dy, cz + dz);
                if reg.block_of_state(world.get_block(q)).name.ends_with("_bed") {
                    let d = dx * dx + dz * dz + dy * dy;
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, [q.x as f64 + 0.5, q.y as f64, q.z as f64 + 0.5]));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// Right click on an animal. Returns whether something happened (so nothing else does).
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32, held: Option<&str>) -> bool {
    let (gm, consume) = {
        let st = player.state.lock().unwrap();
        (st.game_mode, st.game_mode != 1)
    };
    if gm == 3 {
        return false;
    }
    let use_one = |player: &Arc<Player>| {
        if consume {
            let mut st = player.state.lock().unwrap();
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
    };
    // Decided with the mobs locked; the item is used afterwards (never the player's lock
    // inside the mobs' one).
    let mut out: Vec<(Vec<u8>, bool)> = Vec::new();
    let mut eat = false;
    let mut tamed_now = false;
    let done = {
        let mut mobs_map = server.mobs.lock().unwrap();
        let Some(m) = mobs_map.get_mut(&eid).filter(|m| m.death_ticks.is_none()) else { return false };
        let mine = m.owner == Some(player.uuid);
        match (m.kind, held) {
            // Taming a wild wolf with a bone.
            (MobKind::Wolf, Some("bone")) if m.owner.is_none() && m.angry == 0 => {
                eat = true;
                if entities::random() < 1.0 / 3.0 {
                    m.owner = Some(player.uuid);
                    tamed_now = true;
                    m.sitting = true;
                    m.target = None;
                    m.health = m.max_health();
                    out.push((status(m.eid, 7), true));
                } else {
                    out.push((status(m.eid, 6), true));
                }
                out.push((mobs::metadata(m), true));
                true
            }
            // Taming a stray cat, or winning an ocelot's trust, with raw fish.
            (MobKind::Cat | MobKind::Ocelot, Some("cod" | "salmon")) if m.owner.is_none() => {
                eat = true;
                if entities::random() < 1.0 / 3.0 {
                    m.owner = Some(player.uuid);
                    tamed_now = true;
                    m.sitting = m.kind == MobKind::Cat;
                    out.push((status(m.eid, 7), true));
                } else {
                    out.push((status(m.eid, 6), true));
                }
                out.push((mobs::metadata(m), true));
                true
            }
            // Own cat: fish makes it fall in love; anything else sits / stands up.
            (MobKind::Cat, h) if mine => {
                if h.is_some_and(|h| matches!(h, "cod" | "salmon")) && m.grow == 0 && m.love == 0 {
                    eat = true;
                    m.love = LOVE;
                    m.love_by = Some(player.eid);
                } else {
                    m.sitting = !m.sitting;
                    m.path.clear();
                    out.push((mobs::metadata(m), true));
                }
                true
            }
            // Own wolf: meat heals (or makes it fall in love), anything else sits / stands up.
            (MobKind::Wolf, h) if mine => {
                if h.is_some_and(is_meat) && m.health < m.max_health() {
                    eat = true;
                    m.health = (m.health + 4.0).min(m.max_health());
                    out.push((mobs::metadata(m), true));
                } else if h.is_some_and(is_meat) && m.grow == 0 && m.love == 0 {
                    eat = true;
                    m.love = LOVE;
                    m.love_by = Some(player.eid);
                } else {
                    m.sitting = !m.sitting;
                    m.path.clear();
                    out.push((mobs::metadata(m), true));
                }
                true
            }
            (k, Some(h)) if breeds(k) && !matches!(k, MobKind::Wolf | MobKind::Cat) && is_food(k, h) => {
                if m.grow < 0 {
                    // A baby grows 10% of what it has left.
                    eat = true;
                    m.grow += -m.grow / 10;
                    out.push((status(m.eid, 18), true));
                    true
                } else if m.grow == 0 && m.love == 0 {
                    eat = true;
                    m.love = LOVE;
                    m.love_by = Some(player.eid);
                    out.push((status(m.eid, 18), true));
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    if eat {
        use_one(player);
    }
    if tamed_now {
        super::advancements::grant(server, player, "husbandry/tame_an_animal");
    }
    for (p, _) in out {
        server.broadcast_tracked(eid, &p, true);
    }
    done
}

/// Two parents have a baby.
pub fn breed(server: &Arc<Server>, dim: crate::world::Dimension, kind: MobKind, at: [f64; 3], parents: (i32, i32), by: Option<i32>, owner: Option<u128>) {
    {
        let mut mobs_map = server.mobs.lock().unwrap();
        // Both still in love (the other may have bred meanwhile).
        let ok = [parents.0, parents.1].iter().all(|e| mobs_map.get(e).is_some_and(|m| m.love > 0 && m.grow == 0 && m.death_ticks.is_none()));
        if !ok {
            return;
        }
        for e in [parents.0, parents.1] {
            if let Some(m) = mobs_map.get_mut(&e) {
                m.love = 0;
                m.grow = COOLDOWN;
            }
        }
    }
    // Turtles and frogs lay eggs instead (the mother goes off to lay them).
    if matches!(kind, MobKind::Turtle | MobKind::Frog) {
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&parents.0) {
            m.egg = true;
        }
    } else {
        // A horse and a donkey have a mule.
        let kinds: Vec<MobKind> = {
            let map = server.mobs.lock().unwrap();
            [parents.0, parents.1].iter().filter_map(|e| map.get(e).map(|m| m.kind)).collect()
        };
        let kind = if kinds.contains(&MobKind::Horse) && kinds.contains(&MobKind::Donkey) { MobKind::Mule } else { kind };
        let baby = mobs::spawn(server, dim, kind, at);
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&baby) {
            m.grow = BABY;
            m.owner = owner.filter(|_| !super::horses::is_horse(kind));
        }
    }
    let player = by.and_then(|e| server.players.read().unwrap().get(&e).cloned());
    if let Some(p) = &player {
        entities::spawn_xp(server, dim, at, 1 + (entities::random() * 7.0) as i32);
        super::advancements::grant(server, p, "husbandry/breed_an_animal");
    }
    let _ = player;
}

/// The owner hit `target` (or `target` hurt the owner): the owner's wolves go for it.
pub fn sic(server: &Server, owner: u128, target: i32) {
    let mut mobs_map = server.mobs.lock().unwrap();
    if mobs_map.get(&target).is_some_and(|t| t.owner == Some(owner)) {
        return;
    }
    for m in mobs_map.values_mut().filter(|m| m.kind == MobKind::Wolf && m.owner == Some(owner) && !m.sitting && m.death_ticks.is_none()) {
        m.prey = Some(target);
    }
}

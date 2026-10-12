//! Bees, like vanilla: they live in bee nests (generated on some oak and birch trees) and
//! beehives, come out by day when it is not raining, fly to flowers, collect nectar and take
//! it home (each load raises the hive's honey level, up to 5); flying over crops with nectar
//! they make them grow. Hit one and the bees of its hive turn on you: a sting poisons, and a
//! bee that has stung dies a while later. A full hive gives 3 honeycomb with shears or a honey
//! bottle with a glass bottle; that angers the bees unless a campfire smokes under it.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{entities, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::BlockPos;
use crate::world::blockentity::BlockEntity;
use crate::world::{Dimension, World};
use std::sync::Arc;

pub const MAX_IN_HIVE: u32 = 3;

pub fn is_flower(name: &str) -> bool {
    matches!(
        name,
        "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip" | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "sunflower" | "lilac" | "rose_bush" | "peony" | "flowering_azalea" | "torchflower" | "pink_petals"
    )
}

pub fn is_hive(name: &str) -> bool {
    name == "bee_nest" || name == "beehive"
}

fn name(world: &mut World, p: BlockPos) -> &'static str {
    data::get().block_of_state(world.get_block(p)).name.as_str()
}

fn center(p: BlockPos) -> [f64; 3] {
    [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5]
}

/// The spot in front of a hive's opening.
fn front(world: &mut World, hive: BlockPos) -> [f64; 3] {
    let s = world.get_block(hive);
    let (dx, dz) = match data::get().block_of_state(s).get(s, "facing") {
        Some("north") => (0, -1),
        Some("south") => (0, 1),
        Some("west") => (-1, 0),
        _ => (1, 0),
    };
    [hive.x as f64 + 0.5 + dx as f64, hive.y as f64 + 0.3, hive.z as f64 + 0.5 + dz as f64]
}

/// Flies towards `to` (3D). Stuck against something (a trunk, leaves) for a second, it goes
/// up and to one side for a moment before trying again.
pub(super) fn fly(m: &mut Mob, to: [f64; 3], speed: f64) {
    let moved = m.last_target.map_or(1.0, |p| mobs::dist2(&p, &m.pos).sqrt());
    m.last_target = Some(m.pos);
    if moved < 0.01 {
        m.path_stuck += 1;
    } else {
        m.path_stuck = 0;
    }
    if m.path_stuck > 20 && m.detour.is_none() {
        let a = entities::random() as f64 * std::f64::consts::TAU;
        m.detour = Some(([a.cos(), a.sin()], 15));
        m.path_stuck = 0;
    }
    let to = match m.detour {
        Some((dir, left)) => {
            m.detour = (left > 1).then_some((dir, left - 1));
            [m.pos[0] + dir[0] * 2.0, m.pos[1] + 1.5, m.pos[2] + dir[1] * 2.0]
        }
        None => to,
    };
    let d = [to[0] - m.pos[0], to[1] - m.pos[1], to[2] - m.pos[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
    let s = speed.min(len);
    for i in 0..3 {
        m.vel[i] += (d[i] / len * s - m.vel[i]) * 0.3;
    }
    mobs::face(m, to);
}

/// A bee's tick (it handles its own flight). `day`, `rain`: whether it should be out.
pub fn ai(world: &mut World, m: &mut Mob, seen: &[Seen], day: bool, rain: bool, server: &Server, actions: &mut Vec<Action>) {
    // After stinging it dies a while later.
    if m.stung > 0 {
        m.stung += 1;
        if m.stung > 600 + (m.eid.unsigned_abs() % 600) {
            m.health = 0.0;
        }
    }
    // Angry: after whoever upset it, and stings.
    if m.angry > 0 && m.stung == 0 {
        if let Some(t) = m.target.and_then(|e| seen.iter().find(|s| s.player.eid == e && s.targetable)) {
            let aim = [t.pos[0], t.pos[1] + 1.0, t.pos[2]];
            fly(m, aim, 0.16);
            if mobs::dist2(&aim, &m.pos) < 1.4 * 1.4 && m.attack_cooldown == 0 {
                m.attack_cooldown = 20;
                actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, 2.0), "sting", super::health::Attacker { eid: m.eid, name: "Abeja".into(), pos: Some(m.pos) }));
                let poison = match server.config.difficulty {
                    2 => 200,
                    3 => 360,
                    _ => 0,
                };
                if poison > 0 {
                    actions.push(Action::Effect(t.player.clone(), "poison", 0, poison));
                }
                m.stung = 1;
                m.angry = 0;
                m.target = None;
            }
            return;
        }
    }
    let hive = m.hive.filter(|&h| is_hive(name(world, h)));
    if m.hive.is_some() && hive.is_none() {
        m.hive = None;
    }
    // Home: with nectar, at night or in the rain.
    if let Some(h) = hive.filter(|_| m.nectar || !day || rain) {
        let door = front(world, h);
        if mobs::dist2(&door, &m.pos) < 1.2 * 1.2 {
            actions.push(Action::EnterHive(m.eid, m.dim, h, m.nectar));
            m.vel = [0.0; 3];
            return;
        }
        fly(m, door, 0.1);
        pollinate_below(world, m, actions);
        return;
    }
    // A flower: fly there and gather nectar for 5 s.
    if m.flower.is_none() && m.age % 40 == (m.eid.unsigned_abs() % 40) && !m.nectar {
        m.flower = find_flower(world, m.pos, 16);
    }
    if let Some(f) = m.flower {
        if !is_flower(name(world, f)) {
            m.flower = None;
            return;
        }
        let spot = [f.x as f64 + 0.5, f.y as f64 + 0.6, f.z as f64 + 0.5];
        fly(m, spot, 0.1);
        if mobs::dist2(&spot, &m.pos) < 0.8 * 0.8 {
            m.pollinate += 1;
            if m.pollinate > 100 {
                m.nectar = true;
                m.pollinate = 0;
                m.flower = None;
            }
        }
        return;
    }
    // Otherwise it buzzes around its hive (or where it is).
    let home = hive.map(center).unwrap_or(m.pos);
    if m.wander.is_none_or(|w| mobs::dist2(&w, &m.pos) < 1.0) || m.age % 100 == 0 {
        let r = || (entities::random() as f64 - 0.5) * 12.0;
        m.wander = Some([home[0] + r(), home[1] + 1.0 + entities::random() as f64 * 3.0, home[2] + r()]);
    }
    if let Some(w) = m.wander {
        fly(m, w, 0.07);
    }
}

/// With nectar, flying over crops makes them grow (now and then).
fn pollinate_below(world: &mut World, m: &mut Mob, actions: &mut Vec<Action>) {
    if !m.nectar || m.age % 20 != 0 || entities::random() > 0.3 {
        return;
    }
    let reg = data::get();
    for dy in 1..=3 {
        let p = BlockPos::new(m.pos[0].floor() as i32, m.pos[1].floor() as i32 - dy, m.pos[2].floor() as i32);
        let s = world.get_block(p);
        let b = reg.block_of_state(s);
        if matches!(b.name.as_str(), "wheat" | "carrots" | "potatoes" | "beetroots" | "sweet_berry_bush" | "pumpkin_stem" | "melon_stem") {
            let age: i32 = b.get(s, "age").and_then(|a| a.parse().ok()).unwrap_or(0);
            let max = b.props.iter().find(|(n, _)| n == "age").map_or(0, |(_, v)| v.len() as i32 - 1);
            if age < max {
                let ns = b.with(s, "age", &(age + 1).to_string());
                world.set_block(p, ns);
                actions.push(Action::DimPacket(m.dim, super::blocks::block_change(p, ns)));
            }
            return;
        }
    }
}

fn find_flower(world: &mut World, p: [f64; 3], r: i32) -> Option<BlockPos> {
    let (cx, cy, cz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
    let mut best: Option<(i32, BlockPos)> = None;
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -5..=2 {
                let q = BlockPos::new(cx + dx, cy + dy, cz + dz);
                if is_flower(name(world, q)) {
                    let d = dx * dx + dz * dz + dy * dy;
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, q));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// A bee goes into its hive: one more inside, and honey if it brought nectar.
pub fn enter(server: &Server, dim: Dimension, hive: BlockPos, nectar: bool) {
    let change = {
        let mut world = server.world(dim).lock().unwrap();
        let s = world.get_block(hive);
        let b = data::get().block_of_state(s);
        if !is_hive(&b.name) {
            return;
        }
        if let Some(BlockEntity::Hive { bees }) = world.block_entity(hive) {
            *bees = (*bees + 1).min(MAX_IN_HIVE);
        } else {
            world.set_block_entity(hive, BlockEntity::Hive { bees: 1 });
        }
        let level: i32 = b.get(s, "honey_level").and_then(|v| v.parse().ok()).unwrap_or(0);
        if nectar && level < 5 {
            let ns = b.with(s, "honey_level", &(level + 1).to_string());
            world.set_block(hive, ns);
            Some(ns)
        } else {
            None
        }
    };
    if let Some(ns) = change {
        server.broadcast_dim(dim, &super::blocks::block_change(hive, ns), None);
    }
}

/// Every second: hives near players let a bee out by day, when it is not raining.
pub fn tick_hives(server: &Arc<Server>, day: bool) {
    if !day || server.weather.lock().unwrap().is_raining() {
        return;
    }
    let players: Vec<[f64; 3]> = server
        .online()
        .iter()
        .filter(|p| p.dim() == Dimension::Overworld)
        .map(|p| {
            let st = p.state.lock().unwrap();
            [st.x, st.y, st.z]
        })
        .collect();
    let mut out = Vec::new();
    {
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        let mut seen = std::collections::HashSet::new();
        for p in &players {
            let (cx, cz) = ((p[0].floor() as i32) >> 4, (p[2].floor() as i32) >> 4);
            for dx in -3..=3 {
                for dz in -3..=3 {
                    for (pos, bees) in world.hives_in(cx + dx, cz + dz) {
                        if bees == 0 || !seen.insert(pos) || entities::random() > 0.3 {
                            continue;
                        }
                        world.set_hive_bees(pos, bees - 1);
                        let at = front(&mut world, pos);
                        out.push((pos, at));
                    }
                }
            }
        }
    }
    for (hive, at) in out {
        let eid = mobs::spawn(server, Dimension::Overworld, MobKind::Bee, at);
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
            m.hive = Some(hive);
        }
    }
}

/// Shears or a glass bottle on a full hive. Returns whether it was used.
pub fn harvest(server: &Arc<Server>, player: &Arc<Player>, hive: BlockPos, item: &ItemStack, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let tool = item.name();
    if tool != "shears" && tool != "glass_bottle" {
        return false;
    }
    let (state, smoked) = {
        let mut world = server.world(dim).lock().unwrap();
        let s = world.get_block(hive);
        let b = reg.block_of_state(s);
        if !is_hive(&b.name) || b.get(s, "honey_level") != Some("5") {
            return false;
        }
        let ns = b.with(s, "honey_level", "0");
        world.set_block(hive, ns);
        // A campfire up to 5 blocks below calms them.
        let smoked = (1..=5).any(|d| matches!(name(&mut world, BlockPos::new(hive.x, hive.y - d, hive.z)), "campfire" | "soul_campfire"));
        (ns, smoked)
    };
    server.broadcast_dim(dim, &super::blocks::block_change(hive, state), None);
    let survival = player.state.lock().unwrap().survival_like();
    if tool == "shears" {
        if let Some(id) = reg.item_id("honeycomb") {
            entities::drop_at_block(server, dim, hive.x, hive.y, hive.z, ItemStack::new(id, 3));
        }
        if survival {
            super::health::wear_held(server, player, 1);
        }
    } else if let Some(id) = reg.item_id("honey_bottle") {
        let mut st = player.state.lock().unwrap();
        if survival {
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
        let (rest, _) = st.inv.add(ItemStack::new(id, 1));
        let p = st.inv.window_items();
        player.send(p);
        drop(st);
        if let Some(r) = rest {
            entities::throw_from(server, player, r);
        }
    }
    super::sound::at(server, dim, center(hive), "block.beehive.shear", super::sound::BLOCK, 1.0, 1.0);
    if !smoked {
        anger_hive(server, dim, hive, player.eid);
    }
    true
}

/// The bees of a hive (and those around it) turn on a player.
pub fn anger_hive(server: &Server, dim: Dimension, hive: BlockPos, player: i32) {
    let c = center(hive);
    for m in server.mobs.lock().unwrap().values_mut().filter(|m| m.kind == MobKind::Bee && m.dim == dim && m.stung == 0 && (m.hive == Some(hive) || mobs::dist2(&m.pos, &c) < 16.0 * 16.0)) {
        m.angry = 400 + (entities::random() * 400.0) as u32;
        m.target = Some(player);
    }
}

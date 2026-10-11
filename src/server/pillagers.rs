//! Pillager patrols: a captain carrying the ominous banner and its escort.
//!
//! Without a target the captain leads the patrol to far points and the rest follow it in
//! formation. When anyone in the patrol spots a player, the whole patrol goes for it with
//! roles: two guards stand between the captain and the player, the others spread out to
//! flank it at crossbow range, and the captain keeps back behind its guards. They do not
//! shoot when an ally is in the line of fire, and they take turns to shoot depending on the
//! difficulty (one at a time on easy, two on normal, all of them on hard). Killing the captain
//! drops the banner and gives Bad Omen, and the patrol falls apart: with no captain each one
//! fights on its own, exactly like a skeleton (no roles, no shared target, no taking turns).
//!
//! Each patrol brings a vindicator with an escort (a pillager that sticks to it): while the
//! patrol has nobody in sight, the vindicator looks for buildings with a wooden door nearby,
//! breaks the door down and goes in to search them; in a fight it charges, breaking any door
//! in its way, and its escort covers it.

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{BlockPos, Nbt};
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::atomic::Ordering;

/// Ticks to load the crossbow.
const CHARGE: u32 = 25;
/// Distance at which flankers and lone pillagers keep the player.
const RANGE: f64 = 8.0;

/// What a pillager knows about its patrol this tick.
pub struct Group {
    pub captain: Option<(i32, [f64; 3], f32)>,
    /// The other members (by eid, so each one keeps its place).
    pub members: Vec<(i32, [f64; 3])>,
    /// Player someone in the patrol is fighting.
    pub target: Option<i32>,
    /// The vindicator that breaks into buildings.
    pub breacher: Option<(i32, [f64; 3])>,
}

/// The patrols of a dimension, from the state of their members.
pub fn groups<'a>(mobs: impl Iterator<Item = &'a Mob>) -> HashMap<u32, Group> {
    let mut out: HashMap<u32, Group> = HashMap::new();
    for m in mobs.filter(|m| m.patrol != 0 && m.death_ticks.is_none()) {
        let g = out.entry(m.patrol).or_insert(Group { captain: None, members: Vec::new(), target: None, breacher: None });
        if m.kind == MobKind::Vindicator {
            g.breacher = Some((m.eid, m.pos));
            g.target = g.target.or(m.target);
        } else if m.captain {
            g.captain = Some((m.eid, m.pos, m.yaw));
            g.target = m.target.or(g.target);
        } else {
            g.members.push((m.eid, m.pos));
            g.target = g.target.or(m.target);
        }
    }
    for g in out.values_mut() {
        g.members.sort_by_key(|m| m.0);
    }
    out
}

/// The player the patrol is fighting, if this pillager can reach it too (only while the
/// captain leads it).
pub fn shared_target<'a>(m: &Mob, group: Option<&Group>, seen: &'a [Seen]) -> Option<&'a Seen> {
    let g = group?;
    g.captain?;
    let t = g.target?;
    seen.iter().find(|s| s.player.eid == t && s.targetable && mobs::dist2(&s.pos, &m.pos) < 48.0 * 48.0)
}

enum Role {
    Captain,
    Guard(f64),
    Flanker(usize, usize),
    /// Sticks to the vindicator.
    Escort,
    Alone,
}

fn role(m: &Mob, group: Option<&Group>) -> Role {
    let Some(g) = group else { return Role::Alone };
    if m.captain {
        return Role::Captain;
    }
    if g.captain.is_none() {
        return Role::Alone;
    }
    let i = g.members.iter().position(|x| x.0 == m.eid).unwrap_or(0);
    // With a vindicator, the first pillager is its escort and the roles shift by one.
    let (i, n) = if g.breacher.is_some() {
        if i == 0 {
            return Role::Escort;
        }
        (i - 1, g.members.len() - 1)
    } else {
        (i, g.members.len())
    };
    match i {
        0 => Role::Guard(1.0),
        1 => Role::Guard(-1.0),
        _ => Role::Flanker(i - 2, n.saturating_sub(2)),
    }
}

/// Where the escort goes: next to the vindicator, between it and the player.
fn escort_spot(group: Option<&Group>, toward: Option<[f64; 3]>) -> Option<[f64; 3]> {
    let (_, v) = group?.breacher?;
    let off = match toward {
        Some(t) => flat([v[0] - t[0], 0.0, v[2] - t[2]]),
        None => [0.0, 0.0, 0.0],
    };
    Some([v[0] + off[0] * 2.5, v[1], v[2] + off[2] * 2.5])
}

fn flat(v: [f64; 3]) -> [f64; 3] {
    mobs::normalize([v[0], 0.0, v[2]])
}

/// Fire discipline: whether it is this pillager's turn to shoot. The patrol takes turns in
/// windows of `TURN` ticks; on easy one shooter per window, on normal two, on hard all.
fn my_turn(server: &Server, m: &Mob, group: Option<&Group>) -> bool {
    const TURN: i64 = 50;
    let Some(g) = group.filter(|g| g.captain.is_some()) else { return true };
    let shooters = match server.config.difficulty {
        0 | 1 => 1,
        2 => 2,
        _ => return true,
    };
    let mut order: Vec<i32> = g.members.iter().map(|x| x.0).chain(g.captain.map(|c| c.0)).collect();
    order.sort_unstable();
    let n = order.len() as i64;
    let Some(i) = order.iter().position(|&e| e == m.eid) else { return true };
    let turn = (server.age.load(Ordering::Relaxed) / TURN).rem_euclid(n);
    (0..shooters.min(n)).any(|k| (turn + k * n / shooters.min(n)).rem_euclid(n) == i as i64)
}

/// Fighting a player: where to go (next path point and speed) and when to shoot.
pub fn combat(server: &Server, world: &mut World, m: &mut Mob, t: &Seen, group: Option<&Group>, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let speed = MobKind::Pillager.walk_speed();
    let d = mobs::dist2(&m.pos, &t.pos).sqrt();
    let eye = [m.pos[0], m.pos[1] + 1.62, m.pos[2]];
    let sees = d < 24.0 && mobs::line_of_sight(world, eye, t.eye);
    let around = |a: f64, r: f64| [t.pos[0] + a.cos() * r, t.pos[1], t.pos[2] + a.sin() * r];
    // On hard they fight like skeletons (back off, flank shields, cover from archers); the
    // guards stay with their captain.
    let hard = server.config.difficulty >= 3;
    let tactic = if hard && !matches!(role(m, group), Role::Guard(_)) { super::skeletons::tactics(world, m, t, d, sees, m.drawing >= CHARGE) } else { None };
    let dest = match role(m, group) {
        Role::Captain => {
            let away = flat([m.pos[0] - t.pos[0], 0.0, m.pos[2] - t.pos[2]]);
            if sees { [t.pos[0] + away[0] * 13.0, m.pos[1], t.pos[2] + away[2] * 13.0] } else { t.pos }
        }
        Role::Guard(side) => {
            // Between the captain and the player, one on each side.
            let c = group.and_then(|g| g.captain).map_or(m.pos, |c| c.1);
            let to = flat([t.pos[0] - c[0], 0.0, t.pos[2] - c[2]]);
            [c[0] + to[0] * 3.0 - to[2] * side * 1.5, c[1], c[2] + to[2] * 3.0 + to[0] * side * 1.5]
        }
        Role::Flanker(j, n) => {
            // Around the player, to its sides as seen from the captain (the first pair at 90°,
            // the next ones further round towards its back).
            let from = group.and_then(|g| g.captain).map_or(m.pos, |c| c.1);
            let base = (from[2] - t.pos[2]).atan2(from[0] - t.pos[0]);
            let pairs = n.div_ceil(2).max(1) as f64;
            let k = PI / 2.0 + (j / 2) as f64 * (PI * 0.4 / pairs);
            let a = if j % 2 == 0 { base + k } else { base - k };
            if sees || d < RANGE + 2.0 { around(a, RANGE) } else { t.pos }
        }
        Role::Escort => escort_spot(group, Some(t.pos)).unwrap_or(t.pos),
        Role::Alone => {
            // Like a skeleton: closes in until it sees the player, then circles around it
            // keeping its distance.
            if !sees || d > 12.0 {
                t.pos
            } else {
                if entities::random() < 0.05 {
                    m.strafe = -m.strafe;
                }
                let to = flat([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]);
                let back = if d < 6.0 { -0.7 } else if d > 10.0 { 0.5 } else { 0.0 };
                let dir = [to[0] * back - to[2] * m.strafe, 0.0, to[2] * back + to[0] * m.strafe];
                [m.pos[0] + dir[0] * 2.0, m.pos[1], m.pos[2] + dir[2] * 2.0]
            }
        }
    };
    // Standing still is a goal too: without one the generic AI would make it wander off.
    let goal = if let Some(g) = tactic {
        Some(g.unwrap_or((m.pos, 0.0)))
    } else if mobs::dist2(&dest, &m.pos) > 1.2 * 1.2 {
        let next = mobs::follow_path(world, m, dest);
        Some((next, if mobs::dist2(&dest, &m.pos) > 8.0 * 8.0 { speed * 1.3 } else { speed }))
    } else {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        Some((m.pos, 0.0))
    };
    mobs::face(m, t.pos);

    // Crossbow: load (also in cover, out of sight), wait for a clear shot (no ally in the way,
    // not into a raised shield), fire.
    let reach = if m.captain { 16.0 } else { 14.0 };
    let hidden = hard && m.cover.is_some();
    if (sees || hidden) && d < reach && m.drawing < CHARGE {
        m.drawing += 1;
    }
    let shielded = t.shield_up && super::skeletons::shield_covers(t, m.pos);
    if sees && d < reach && m.drawing >= CHARGE && !shielded {
        if m.attack_cooldown == 0 && my_turn(server, m, group) && !ally_in_the_way(m, group, eye, t.eye) {
            m.drawing = 0;
            m.attack_cooldown = match server.config.difficulty {
                0 | 1 => 60,
                2 => 40,
                _ => 20,
            } + (entities::random() * 20.0) as u32;
            let from = [eye[0], eye[1] - 0.1, eye[2]];
            let to = [t.pos[0] - from[0], t.pos[1] + 0.6 - from[1], t.pos[2] - from[2]];
            let horiz = to[0].hypot(to[2]);
            let spread = 0.04 * (4 - server.config.difficulty.min(3) as i32) as f64;
            let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
            let vel = [dir[0] * 1.6 + (entities::random() as f64 - 0.5) * spread, dir[1] * 1.6, dir[2] * 1.6 + (entities::random() as f64 - 0.5) * spread];
            actions.push(Action::Shoot(m.dim, ProjectileKind::Arrow, m.eid, from, vel));
        }
    }
    goal
}

/// Whether another pillager of the patrol is near the line of fire (and closer than the target).
fn ally_in_the_way(m: &Mob, group: Option<&Group>, from: [f64; 3], to: [f64; 3]) -> bool {
    // Without a captain nobody minds the others.
    let Some(g) = group.filter(|g| g.captain.is_some()) else { return false };
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let len2 = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).max(1e-6);
    g.members.iter().map(|x| (x.0, x.1)).chain(g.captain.map(|c| (c.0, c.1))).chain(g.breacher).filter(|x| x.0 != m.eid).any(|(_, p)| {
        let c = [p[0], p[1] + 1.0, p[2]];
        let f = ((c[0] - from[0]) * d[0] + (c[1] - from[1]) * d[1] + (c[2] - from[2]) * d[2]) / len2;
        if !(0.0..1.0).contains(&f) {
            return false;
        }
        let q = [from[0] + d[0] * f - c[0], from[1] + d[1] * f - c[1], from[2] + d[2] * f - c[2]];
        q[0] * q[0] + q[1] * q[1] + q[2] * q[2] < 1.2 * 1.2
    })
}

/// No target: the captain leads to far points, the escort follows in two rows behind it.
pub fn patrol(world: &mut World, m: &mut Mob, group: Option<&Group>) -> Option<([f64; 3], f64)> {
    m.drawing = 0;
    let speed = MobKind::Pillager.walk_speed();
    match role(m, group) {
        Role::Captain => {
            if m.wander.is_none_or(|w| (w[0] - m.pos[0]).powi(2) + (w[2] - m.pos[2]).powi(2) < 4.0) {
                let a = entities::random() as f64 * PI * 2.0;
                let r = 16.0 + entities::random() as f64 * 16.0;
                m.wander = Some([m.pos[0] + a.cos() * r, m.pos[1], m.pos[2] + a.sin() * r]);
            }
            // Waits for stragglers.
            let behind = group.is_some_and(|g| g.members.iter().any(|x| mobs::dist2(&x.1, &m.pos) > 10.0 * 10.0));
            let w = m.wander?;
            if behind {
                m.vel[0] = 0.0;
                m.vel[2] = 0.0;
                return Some((m.pos, 0.0));
            }
            Some((mobs::follow_path(world, m, w), speed * 0.6))
        }
        Role::Escort if group.and_then(|g| g.breacher.zip(g.captain)).is_some_and(|((_, v), (_, c, _))| mobs::dist2(&v, &c) > 7.0 * 7.0) => {
            // The vindicator is off searching a building: stay with it.
            let spot = escort_spot(group, None)?;
            if mobs::dist2(&spot, &m.pos) < 2.5 * 2.5 {
                m.vel[0] = 0.0;
                m.vel[2] = 0.0;
                return Some((m.pos, 0.0));
            }
            Some((mobs::follow_path(world, m, spot), speed))
        }
        Role::Guard(_) | Role::Flanker(..) | Role::Escort => {
            let g = group?;
            let (_, c, yaw) = g.captain?;
            let i = g.members.iter().position(|x| x.0 == m.eid).unwrap_or(0);
            let (s, co) = (yaw as f64).to_radians().sin_cos();
            // Captain's forward is (-sin, cos); slots in rows behind it.
            let back = [s, 0.0, -co];
            let right = [-co, 0.0, -s];
            let row = (i / 2 + 1) as f64 * 1.8;
            let side = if i % 2 == 0 { 1.2 } else { -1.2 };
            let slot = [c[0] + back[0] * row + right[0] * side, c[1], c[2] + back[2] * row + right[2] * side];
            let far = mobs::dist2(&slot, &m.pos);
            if far < 1.0 {
                m.vel[0] = 0.0;
                m.vel[2] = 0.0;
                m.yaw = yaw;
                return Some((m.pos, 0.0));
            }
            Some((mobs::follow_path(world, m, slot), if far > 6.0 * 6.0 { speed * 1.2 } else { speed * 0.7 }))
        }
        Role::Alone => None,
    }
}

/// The vindicator without a fight: searches buildings near the patrol (breaking their door),
/// or else walks behind the captain.
pub fn vindicator(world: &mut World, m: &mut Mob, group: Option<&Group>) -> Option<([f64; 3], f64)> {
    let g = group?;
    let (_, c, yaw) = g.captain?;
    let speed = MobKind::Vindicator.walk_speed();
    if let Some(door) = m.raid_door {
        if m.explore == 0 {
            // On the way to the door (the zombies' door code breaks it when it is in the way).
            let state = world.get_block(door);
            let still = crate::world::pathfind::wooden_door(&data::get().block_of_state(state).name) && data::get().block_of_state(state).get(state, "open") != Some("true");
            if still && mobs::dist2(&m.pos, &[door.x as f64 + 0.5, m.pos[1], door.z as f64 + 0.5]) < 40.0 * 40.0 {
                // Aim past the door, so the path goes through it (and the door gets broken).
                let d = flat([door.x as f64 + 0.5 - m.pos[0], 0.0, door.z as f64 + 0.5 - m.pos[2]]);
                let inside = [door.x as f64 + 0.5 + d[0] * 3.0, door.y as f64, door.z as f64 + 0.5 + d[2] * 3.0];
                return Some((mobs::follow_path(world, m, inside), speed));
            }
            // Open now: in, a few blocks past it, and search for 15 s.
            let d = flat([door.x as f64 + 0.5 - m.pos[0], 0.0, door.z as f64 + 0.5 - m.pos[2]]);
            m.anchor = Some([door.x as f64 + 0.5 + d[0] * 3.0, door.y as f64, door.z as f64 + 0.5 + d[2] * 3.0]);
            m.wander = m.anchor;
            m.explore = 300;
        }
        m.explore = m.explore.saturating_sub(1);
        if m.explore == 0 || mobs::dist2(&m.pos, &c) > 48.0 * 48.0 {
            m.raid_door = None;
            m.explore = 0;
            m.anchor = None;
            return None;
        }
        let a = m.anchor.unwrap_or(m.pos);
        if m.wander.is_none_or(|w| (w[0] - m.pos[0]).powi(2) + (w[2] - m.pos[2]).powi(2) < 1.0) {
            let ang = entities::random() as f64 * PI * 2.0;
            let r = entities::random() as f64 * 3.0;
            m.wander = Some([a[0] + ang.cos() * r, a[1], a[2] + ang.sin() * r]);
        }
        let w = m.wander.unwrap_or(a);
        return Some((mobs::follow_path(world, m, w), speed * 0.7));
    }
    // Every few seconds, a look for doors around.
    if (m.age as i32 + m.eid) % 60 == 0 {
        m.raid_door = find_door(world, m.pos, 20);
        if m.raid_door.is_some() {
            return None;
        }
    }
    // In formation, right behind the captain.
    let (s, co) = (yaw as f64).to_radians().sin_cos();
    let slot = [c[0] + s * 2.0, c[1], c[2] - co * 2.0];
    if mobs::dist2(&slot, &m.pos) < 1.0 {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        return Some((m.pos, 0.0));
    }
    Some((mobs::follow_path(world, m, slot), speed * 0.8))
}

/// The nearest closed wooden door (its lower half) within `r` blocks.
fn find_door(world: &mut World, p: [f64; 3], r: i32) -> Option<BlockPos> {
    let reg = data::get();
    let (cx, cy, cz) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
    let mut best: Option<(i32, BlockPos)> = None;
    for dx in -r..=r {
        for dz in -r..=r {
            for dy in -3..=3 {
                let q = BlockPos::new(cx + dx, cy + dy, cz + dz);
                let s = world.get_block(q);
                let b = reg.block_of_state(s);
                if crate::world::pathfind::wooden_door(&b.name) && b.get(s, "half") == Some("lower") && b.get(s, "open") != Some("true") {
                    let d = dx * dx + dz * dz + dy * dy;
                    if best.is_none_or(|x| d < x.0) {
                        best = Some((d, q));
                    }
                }
            }
        }
    }
    best.map(|b| b.1)
}

/// The ominous banner (the captain wears it on its head and drops it).
pub fn ominous_banner() -> Option<ItemStack> {
    let pattern = |p: &str, c: i32| Nbt::Compound(vec![("Pattern".into(), Nbt::String(p.into())), ("Color".into(), Nbt::Int(c))]);
    let patterns = vec![pattern("mr", 9), pattern("bs", 8), pattern("cs", 7), pattern("bo", 8), pattern("ms", 15), pattern("hh", 8), pattern("mc", 8), pattern("bo", 15)];
    let mut stack = ItemStack::new(data::get().item_id("white_banner")?, 1);
    stack.nbt = Some(Box::new(Nbt::Compound(vec![
        ("BlockEntityTag".into(), Nbt::Compound(vec![("Patterns".into(), Nbt::List(patterns))])),
        ("HideFlags".into(), Nbt::Int(32)),
        ("display".into(), Nbt::Compound(vec![("Name".into(), Nbt::String(r#"{"color":"gold","translate":"block.minecraft.ominous_banner"}"#.into()))])),
    ])));
    Some(stack)
}

/// A patrol: a captain and `escort` pillagers around `pos`.
pub fn spawn_patrol(server: &Server, dim: Dimension, pos: [f64; 3], escort: usize) -> Vec<i32> {
    let id = 1 + (entities::random() * 1e9) as u32;
    let mut eids = Vec::new();
    for i in 0..=escort {
        let off = if i == 0 { [0.0, 0.0] } else { [((i % 3) as f64 - 1.0) * 1.2, -1.5 * (i.div_ceil(3)) as f64] };
        let kind = if i == 1 && escort >= 2 { MobKind::Vindicator } else { MobKind::Pillager };
        let eid = mobs::spawn(server, dim, kind, [pos[0] + off[0], pos[1], pos[2] + off[1]]);
        if i == 0 {
            make_captain(server, eid, id);
        } else if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
            m.patrol = id;
        }
        eids.push(eid);
    }
    eids
}

/// Makes a pillager the captain of patrol `id` (with the banner on its head).
pub fn make_captain(server: &Server, eid: i32, id: u32) {
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.patrol = id;
        m.captain = true;
    }
    server.broadcast_tracked(eid, &mobs::equipment_stack(eid, 5, ominous_banner()), true);
}

/// Pillager outposts are guarded: while a player is near one, pillagers keep turning up around
/// the tower (up to 4).
pub fn outpost_garrison(server: &Server, seen: &[Seen]) {
    if !server.config.spawn_monsters || server.config.difficulty == 0 {
        return;
    }
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld && s.present) {
        let (px, pz) = (s.pos[0].floor() as i32, s.pos[2].floor() as i32);
        let spot = {
            let mut world = server.world(Dimension::Overworld).lock().unwrap();
            let Some((ox, _, oz)) = world.generator().locate("outpost", px, pz) else { continue };
            if ((ox - px) as f64).hypot((oz - pz) as f64) > 80.0 {
                continue;
            }
            let guards = server.mobs.lock().unwrap().values().filter(|m| m.kind == MobKind::Pillager && m.patrol == 0 && mobs::dist2(&m.pos, &[ox as f64, m.pos[1], oz as f64]) < 48.0 * 48.0).count();
            if guards >= 4 {
                continue;
            }
            let a = entities::random() as f64 * PI * 2.0;
            let r = 7.0 + entities::random() as f64 * 12.0;
            mobs::open_sky_spot(&mut world, ox + (a.cos() * r) as i32, oz + (a.sin() * r) as i32, s.pos[1] as i32)
        };
        if let Some(at) = spot {
            mobs::spawn(server, Dimension::Overworld, MobKind::Pillager, at);
        }
    }
}

/// Now and then a patrol shows up near a player in the Overworld (one at a time per area).
pub fn natural_patrols(server: &Server, seen: &[Seen]) {
    if !server.config.spawn_monsters || server.config.difficulty == 0 {
        return;
    }
    let mut spawn = Vec::new();
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld) {
        if entities::random() > 0.08 {
            continue;
        }
        let busy = server.mobs.lock().unwrap().values().any(|m| m.kind == MobKind::Pillager && mobs::dist2(&m.pos, &s.pos) < 128.0 * 128.0);
        if busy {
            continue;
        }
        let a = entities::random() as f64 * PI * 2.0;
        let r = 24.0 + entities::random() as f64 * 16.0;
        let (x, z) = ((s.pos[0] + a.cos() * r).floor() as i32, (s.pos[2] + a.sin() * r).floor() as i32);
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        if let Some(spot) = mobs::open_sky_spot(&mut world, x, z, s.pos[1] as i32) {
            spawn.push(spot);
        }
    }
    for spot in spawn {
        spawn_patrol(server, Dimension::Overworld, spot, 1 + server.config.difficulty as usize);
    }
}

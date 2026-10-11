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

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::projectiles::ProjectileKind;
use super::{entities, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::Nbt;
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
}

/// The patrols of a dimension, from the state of their members.
pub fn groups<'a>(mobs: impl Iterator<Item = &'a Mob>) -> HashMap<u32, Group> {
    let mut out: HashMap<u32, Group> = HashMap::new();
    for m in mobs.filter(|m| m.patrol != 0 && m.death_ticks.is_none()) {
        let g = out.entry(m.patrol).or_insert(Group { captain: None, members: Vec::new(), target: None });
        if m.captain {
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
    match i {
        0 => Role::Guard(1.0),
        1 => Role::Guard(-1.0),
        _ => Role::Flanker(i - 2, g.members.len() - 2),
    }
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
    g.members.iter().map(|x| (x.0, x.1)).chain(g.captain.map(|c| (c.0, c.1))).filter(|x| x.0 != m.eid).any(|(_, p)| {
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
        Role::Guard(_) | Role::Flanker(..) => {
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
        let eid = mobs::spawn(server, dim, MobKind::Pillager, [pos[0] + off[0], pos[1], pos[2] + off[1]]);
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

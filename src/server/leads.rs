//! Leads. Right click an animal (or a golem, a horse...) with a lead to put it on a lead;
//! it follows you and is pulled along, and the lead snaps (dropping it) if you get more than
//! 10 blocks away. Right click a fence while leading animals to tie them to it (a knot
//! appears); right click the knot to tie more, or hit it to untie them all. Right click an
//! animal you lead to let it go. Animals tied to fences stay tied after a restart.

use super::mobs::{self, Mob, MobKind, Seen};
use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// What a mob is tied to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Leash {
    Player(i32),
    Knot(BlockPos),
}

/// Knots on fences: (dimension, fence) -> knot entity id.
static KNOTS: Mutex<Option<HashMap<(Dimension, BlockPos), i32>>> = Mutex::new(None);

/// Lead snaps beyond this distance; pulled beyond `SLACK`.
const SNAP: f64 = 10.0;
const SLACK: f64 = 6.0;

pub fn leashable(kind: MobKind) -> bool {
    (!kind.hostile() || kind == MobKind::Hoglin)
        && !kind.fish()
        && !matches!(kind, MobKind::Villager | MobKind::Turtle | MobKind::Squid | MobKind::GlowSquid | MobKind::Tadpole | MobKind::Bat | MobKind::EnderDragon | MobKind::EndCrystal)
}

pub fn attach(eid: i32, holder: Option<i32>) -> Vec<u8> {
    let mut p = packet(id::play::ATTACH_ENTITY);
    p.i32(eid).i32(holder.unwrap_or(-1));
    p
}

fn knot_pos(p: BlockPos) -> [f64; 3] {
    [p.x as f64 + 0.5, p.y as f64 + 0.375, p.z as f64 + 0.5]
}

fn knot_spawn(eid: i32, p: BlockPos) -> Vec<u8> {
    let pos = knot_pos(p);
    let mut s = packet(id::play::SPAWN_ENTITY);
    s.varint(eid).uuid(0x4b4e_4f54_0000_0000_0000_0000_0000_0000 | eid as u32 as u128).varint(data::get().entity("leash_knot"));
    s.f64(pos[0]).f64(pos[1]).f64(pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(0);
    s.i16(0).i16(0).i16(0);
    s
}

/// The entity a mob is held by (for the attach packet).
pub fn holder_eid(leash: Option<Leash>, dim: Dimension) -> Option<i32> {
    match leash? {
        Leash::Player(e) => Some(e),
        Leash::Knot(p) => KNOTS.lock().unwrap().as_ref()?.get(&(dim, p)).copied(),
    }
}

fn knot_for(server: &Server, dim: Dimension, fence: BlockPos) -> i32 {
    let mut guard = KNOTS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if let Some(&e) = map.get(&(dim, fence)) {
        return e;
    }
    let eid = server.next_entity_id();
    map.insert((dim, fence), eid);
    drop(guard);
    server.broadcast_dim(dim, &knot_spawn(eid, fence), None);
    sound::at(server, dim, knot_pos(fence), "entity.leash_knot.place", sound::BLOCK, 1.0, 1.0);
    eid
}

fn remove_knot(server: &Server, dim: Dimension, fence: BlockPos) {
    let eid = KNOTS.lock().unwrap().as_mut().and_then(|m| m.remove(&(dim, fence)));
    if let Some(e) = eid {
        let mut d = packet(id::play::ENTITY_DESTROY);
        d.varint(1).varint(e);
        server.broadcast_dim(dim, &d, None);
        sound::at(server, dim, knot_pos(fence), "entity.leash_knot.break", sound::BLOCK, 1.0, 1.0);
    }
}

/// The knots of a dimension (on joining or changing dimension).
pub fn send_knots(server: &Server, player: &Player) {
    let dim = player.dim();
    let list: Vec<(BlockPos, i32)> = KNOTS.lock().unwrap().as_ref().map(|m| m.iter().filter(|(k, _)| k.0 == dim).map(|(k, e)| (k.1, *e)).collect()).unwrap_or_default();
    for (p, e) in list {
        player.send(knot_spawn(e, p));
    }
    let _ = server;
}

fn drop_lead(server: &Server, dim: Dimension, at: [f64; 3]) {
    if let Some(i) = data::get().item_id("lead") {
        entities::spawn(server, dim, at, [0.0, 0.2, 0.0], ItemStack::new(i, 1), 10);
    }
}

/// Lets a mob go (dropping the lead if `drop`).
pub fn unleash(server: &Server, eid: i32, drop: bool) {
    let info = {
        let mut map = server.mobs.lock().unwrap();
        map.get_mut(&eid).and_then(|m| m.leash.take().map(|_| (m.dim, m.center())))
    };
    let Some((dim, at)) = info else { return };
    server.broadcast_tracked(eid, &attach(eid, None), true);
    if drop {
        drop_lead(server, dim, at);
    }
}

fn set_leash(server: &Server, m: &mut Mob, leash: Leash) {
    m.leash = Some(leash);
    let holder = holder_eid(m.leash, m.dim);
    server.broadcast_tracked(m.eid, &attach(m.eid, holder), true);
}

/// Right click on a mob. Returns whether the lead did something.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32, held: Option<&ItemStack>) -> bool {
    let creative = player.state.lock().unwrap().game_mode == 1;
    let what = {
        let mut map = server.mobs.lock().unwrap();
        let Some(m) = map.get_mut(&eid).filter(|m| m.death_ticks.is_none()) else { return false };
        if m.leash == Some(Leash::Player(player.eid)) {
            // Let go.
            m.leash = None;
            server.broadcast_tracked(eid, &attach(eid, None), true);
            Some((false, m.dim, m.center()))
        } else if held.is_some_and(|h| h.name() == "lead") && leashable(m.kind) && m.leash.is_none() && m.rider.is_none() {
            set_leash(server, m, Leash::Player(player.eid));
            Some((true, m.dim, m.center()))
        } else {
            None
        }
    };
    let Some((tied, dim, at)) = what else { return false };
    if tied {
        if !creative {
            let mut st = player.state.lock().unwrap();
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
        sound::at(server, dim, at, "entity.leash_knot.place", sound::NEUTRAL, 1.0, 1.0);
    } else if !creative {
        drop_lead(server, dim, at);
    }
    true
}

/// Right click on a fence: the mobs the player leads are tied to it.
pub fn tie(server: &Arc<Server>, player: &Arc<Player>, fence: BlockPos) -> bool {
    let dim = player.dim();
    let led: Vec<i32> = server.mobs.lock().unwrap().values().filter(|m| m.leash == Some(Leash::Player(player.eid)) && m.dim == dim).map(|m| m.eid).collect();
    if led.is_empty() {
        return false;
    }
    knot_for(server, dim, fence);
    let mut map = server.mobs.lock().unwrap();
    for e in led {
        if let Some(m) = map.get_mut(&e) {
            set_leash(server, m, Leash::Knot(fence));
        }
    }
    true
}

/// Clicking a knot: with mobs on the lead, they are tied there too; hitting it (or
/// clicking it empty-handed) unties everything and drops the leads.
pub fn knot_interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, attack: bool) -> bool {
    let dim = player.dim();
    let fence = KNOTS.lock().unwrap().as_ref().and_then(|m| m.iter().find(|(k, e)| **e == eid && k.0 == dim).map(|(k, _)| k.1));
    let Some(fence) = fence else { return false };
    if !attack && tie(server, player, fence) {
        return true;
    }
    let tied: Vec<i32> = server.mobs.lock().unwrap().values().filter(|m| m.leash == Some(Leash::Knot(fence)) && m.dim == dim).map(|m| m.eid).collect();
    let creative = player.state.lock().unwrap().game_mode == 1;
    for e in tied {
        unleash(server, e, !creative);
    }
    remove_knot(server, dim, fence);
    true
}

/// Where the mob is held from, if it is on a lead.
fn anchor(m: &Mob, seen: &[Seen]) -> Option<[f64; 3]> {
    match m.leash? {
        Leash::Player(e) => seen.iter().find(|s| s.player.eid == e).map(|s| [s.pos[0], s.pos[1] + 1.0, s.pos[2]]),
        Leash::Knot(p) => Some(knot_pos(p)),
    }
}

/// A mob on a lead: pulled towards its holder beyond the slack; snaps when too far.
/// Returns a goal to walk to, and whether the lead broke.
pub fn follow(m: &mut Mob, seen: &[Seen]) -> (Option<([f64; 3], f64)>, bool) {
    if m.leash.is_none() {
        return (None, false);
    }
    let Some(at) = anchor(m, seen) else {
        // The holder is gone.
        return (None, true);
    };
    let d = mobs::dist2(&at, &m.center()).sqrt();
    if d > SNAP {
        return (None, true);
    }
    if d > SLACK {
        // Pulled along.
        let dir = mobs::normalize([at[0] - m.pos[0], at[1] - m.pos[1], at[2] - m.pos[2]]);
        let k = (d - SLACK) * 0.04;
        m.vel[0] += dir[0] * k;
        m.vel[2] += dir[2] * k;
        if m.kind.flies() || m.kind.swims() {
            m.vel[1] += dir[1] * k;
        }
        m.knockback_ticks = m.knockback_ticks.max(1);
    }
    if d > 3.0 {
        return (Some((at, m.kind.walk_speed() * 1.2)), false);
    }
    (None, false)
}

/// Every tick: leads that snapped (holder gone or too far) drop.
pub fn tick(server: &Arc<Server>, broken: Vec<i32>) {
    for e in broken {
        unleash(server, e, true);
        sound::entity(server, e, "entity.leash_knot.break", sound::NEUTRAL, 1.0, 1.0);
    }
    // Knots with nothing tied to them go away.
    let knots: Vec<(Dimension, BlockPos)> = KNOTS.lock().unwrap().as_ref().map(|m| m.keys().copied().collect()).unwrap_or_default();
    if knots.is_empty() {
        return;
    }
    let used: Vec<(Dimension, BlockPos)> = server.mobs.lock().unwrap().values().filter_map(|m| match m.leash {
        Some(Leash::Knot(p)) => Some((m.dim, p)),
        _ => None,
    }).collect();
    for k in knots {
        if !used.contains(&k) {
            remove_knot(server, k.0, k.1);
        }
    }
}

/// After loading the mobs: the knots of those tied to fences come back.
pub fn restore(server: &Server) {
    let tied: Vec<(Dimension, BlockPos)> = server.mobs.lock().unwrap().values().filter_map(|m| match m.leash {
        Some(Leash::Knot(p)) => Some((m.dim, p)),
        _ => None,
    }).collect();
    for (dim, p) in tied {
        knot_for(server, dim, p);
    }
}

/// The fence was broken: whatever was tied to it goes free (the leads drop).
pub fn fence_broken(server: &Server, dim: Dimension, fence: BlockPos) {
    let tied: Vec<i32> = server.mobs.lock().unwrap().values().filter(|m| m.leash == Some(Leash::Knot(fence)) && m.dim == dim).map(|m| m.eid).collect();
    for e in tied {
        unleash(server, e, true);
    }
    remove_knot(server, dim, fence);
}

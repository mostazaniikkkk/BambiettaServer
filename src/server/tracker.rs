//! Per-player entity tracking: each client only receives the entities near it
//! (players within its view distance, mobs within 80 blocks, items/orbs/arrows
//! within 64). They spawn and despawn automatically when entering or leaving range, and
//! movement/state packets are sent only to those tracking them (`Server::broadcast_tracked`).

use super::{entities, mobs, Player, Server};
use crate::protocol::{id, packet, Writer};
use crate::world::Dimension;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Player,
    Mob,
    Item,
    Orb,
    Projectile,
    Tnt,
    Vehicle,
    /// The dragon: visible from far away (160 blocks, like vanilla).
    Boss,
}

impl Kind {
    fn range(self) -> f64 {
        match self {
            Kind::Player => 512.0, // also limited by the view distance
            Kind::Mob | Kind::Vehicle => 80.0,
            Kind::Boss => 160.0,
            _ => 64.0,
        }
    }
}

/// Forces `eid` to respawn for everyone (e.g. when a player respawns).
pub fn respawn_for_others(server: &Server, eid: i32) {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(eid);
    for p in server.online() {
        if p.eid != eid && p.tracked.lock().unwrap().remove(&eid) {
            p.send(d.clone());
        }
    }
}

pub fn tick(server: &Arc<Server>) {
    // Snapshot of all entities: (id, position, kind).
    let players = server.online();
    let mut all: Vec<(i32, [f64; 3], Kind, Dimension)> = Vec::new();
    let mut viewers = Vec::new();
    for p in &players {
        let (pos, vd) = {
            let s = p.state.lock().unwrap();
            ([s.x, s.y, s.z], s.view_distance)
        };
        all.push((p.eid, pos, Kind::Player, p.dim()));
        viewers.push((p.clone(), pos, vd));
    }
    all.extend(server.mobs.lock().unwrap().values().map(|m| (m.eid, m.pos, if m.kind == mobs::MobKind::EnderDragon { Kind::Boss } else { Kind::Mob }, m.dim)));
    all.extend(server.items.lock().unwrap().values().map(|e| (e.eid, e.pos, Kind::Item, e.dim)));
    all.extend(server.orbs.lock().unwrap().values().map(|o| (o.eid, o.pos, Kind::Orb, o.dim)));
    all.extend(super::projectiles::positions(server).into_iter().map(|(eid, pos, d)| (eid, pos, Kind::Projectile, d)));
    all.extend(super::fishing::positions(server).into_iter().map(|(eid, pos, d)| (eid, pos, Kind::Projectile, d)));
    all.extend(super::vehicles::positions(server).into_iter().map(|(eid, pos, d)| (eid, pos, Kind::Vehicle, d)));
    all.extend(super::redstone::tnt_positions(server).into_iter().map(|(eid, pos, d)| (eid, pos, Kind::Tnt, d)));
    let kinds: HashMap<i32, Kind> = all.iter().map(|e| (e.0, e.2)).collect();

    // What has to spawn and despawn for each player.
    let mut plans: Vec<(Arc<Player>, Vec<i32>, Vec<i32>)> = Vec::new();
    let mut needed: HashSet<i32> = HashSet::new();
    for (viewer, pos, vd) in &viewers {
        let view = (*vd as f64 + 1.0) * 16.0;
        let vdim = viewer.dim();
        let visible: HashSet<i32> = all
            .iter()
            .filter(|(eid, epos, kind, edim)| {
                if *eid == viewer.eid || *edim != vdim {
                    return false;
                }
                let r = kind.range().min(view);
                (epos[0] - pos[0]).powi(2) + (epos[2] - pos[2]).powi(2) <= r * r
            })
            .map(|e| e.0)
            .collect();
        let mut tracked = viewer.tracked.lock().unwrap();
        let add: Vec<i32> = visible.difference(&tracked).copied().collect();
        let remove: Vec<i32> = tracked.difference(&visible).copied().collect();
        for e in &add {
            tracked.insert(*e);
        }
        for e in &remove {
            tracked.remove(e);
        }
        drop(tracked);
        needed.extend(add.iter().copied());
        if !add.is_empty() || !remove.is_empty() {
            plans.push((viewer.clone(), add, remove));
        }
    }
    if plans.is_empty() {
        return;
    }

    // Spawn packets, built once per entity.
    let mut spawn: HashMap<i32, Vec<Vec<u8>>> = HashMap::new();
    for p in &players {
        if needed.contains(&p.eid) {
            spawn.insert(p.eid, super::play::spawn_packets(p));
        }
    }
    let want = |k: Kind| needed.iter().filter(|e| kinds.get(e) == Some(&k)).copied().collect::<Vec<_>>();
    spawn.extend(mobs::spawn_packets_for(server, &want(Kind::Mob)));
    spawn.extend(mobs::spawn_packets_for(server, &want(Kind::Boss)));
    spawn.extend(entities::spawn_packets_for(server, &want(Kind::Item), &want(Kind::Orb)));
    spawn.extend(super::projectiles::spawn_packets_for(server, &want(Kind::Projectile)));
    spawn.extend(super::vehicles::spawn_packets_for(server, &want(Kind::Vehicle)));
    spawn.extend(super::fishing::spawn_packets_for(server, &want(Kind::Projectile)));
    spawn.extend(super::redstone::tnt_spawn_packets(server, &want(Kind::Tnt)));

    for (viewer, add, remove) in plans {
        if !remove.is_empty() {
            let mut d = packet(id::play::ENTITY_DESTROY);
            d.varint(remove.len() as i32);
            for e in &remove {
                d.varint(*e);
            }
            viewer.send(d);
        }
        for e in &add {
            for p in spawn.get(e).into_iter().flatten() {
                viewer.send(p.clone());
            }
        }
        // Who is riding what, once both exist.
        for p in super::vehicles::passenger_packets_for(server, &add) {
            viewer.send(p);
        }
    }
}

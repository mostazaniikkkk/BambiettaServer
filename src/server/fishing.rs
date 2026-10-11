//! Fishing: casting the bobber, floating on water, bites with bubbles (Lure speeds them up),
//! reeling in with loot from the vanilla tables (Luck of the Sea improves treasure), hooking and
//! pulling entities, and fishing rod wear.

use super::{entities, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::physics::{move_aabb, Aabb};
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct Bobber {
    pub eid: i32,
    pub owner: i32,
    pub dim: Dimension,
    pub pos: [f64; 3],
    vel: [f64; 3],
    in_water: bool,
    on_ground: bool,
    /// Hooked entity.
    hooked: Option<i32>,
    /// Ticks until a bite.
    wait: i32,
    /// Ticks left in the current bite (0 = no bite).
    nibble: i32,
    lure: i32,
    luck: i32,
    age: u32,
    ground_ticks: u32,
    sent: [f64; 3],
}

fn rand(n: i32) -> i32 {
    if n <= 0 { 0 } else { ((entities::random() * n as f32) as i32).min(n - 1) }
}

/// Time until a bite: 100-600 ticks minus 100 per Lure level (like vanilla).
fn wait_time(lure: i32) -> i32 {
    (100 + rand(500) - lure * 100).max(20)
}

pub fn positions(server: &Server) -> Vec<(i32, [f64; 3], Dimension)> {
    server.bobbers.lock().unwrap().values().map(|b| (b.eid, b.pos, b.dim)).collect()
}

fn metadata(b: &Bobber) -> Vec<u8> {
    // 8: hooked entity (+1), 9: biting.
    let mut m = packet(id::play::ENTITY_METADATA);
    m.varint(b.eid).u8(8).varint(1).varint(b.hooked.map_or(0, |e| e + 1));
    m.u8(9).varint(8).bool(b.nibble > 0);
    m.u8(0xff);
    m
}

pub fn spawn_packets_for(server: &Server, eids: &[i32]) -> HashMap<i32, Vec<Vec<u8>>> {
    if eids.is_empty() {
        return HashMap::new();
    }
    let bs = server.bobbers.lock().unwrap();
    eids.iter()
        .filter_map(|e| bs.get(e))
        .map(|b| {
            let mut p = packet(id::play::SPAWN_ENTITY);
            // The packet data is the owner's id: without it, the client doesn't draw the line.
            p.varint(b.eid).uuid(((b.owner as u128) << 64) | b.eid as u128).varint(data::get().entity("fishing_bobber"));
            p.f64(b.pos[0]).f64(b.pos[1]).f64(b.pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(b.owner);
            for v in b.vel {
                p.i16((v.clamp(-3.9, 3.9) * 8000.0) as i16);
            }
            (b.eid, vec![p, metadata(b)])
        })
        .collect()
}

fn rod_hand(player: &Player) -> Option<i32> {
    let st = player.state.lock().unwrap();
    [0, 1].into_iter().find(|&h| st.inv.held(h, st.held_slot).as_ref().is_some_and(|i| i.name() == "fishing_rod"))
}

/// `use_item` with the rod: casts or reels in the bobber. Returns whether it was a rod.
pub fn use_rod(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (rod, existing, eye, yaw, pitch) = {
        let st = player.state.lock().unwrap();
        let Some(rod) = st.inv.held(hand, st.held_slot).clone().filter(|i| i.name() == "fishing_rod") else { return false };
        (rod, st.bobber, st.eye(), st.yaw, st.pitch)
    };
    let dim = player.dim();
    if let Some(eid) = existing {
        retrieve(server, player, eid, hand);
        return true;
    }
    // Cast (FishingHook: a bit more than 1 block per tick in the look direction).
    let d = super::projectiles::look(yaw, pitch);
    let speed = 1.1 + (entities::random() as f64 - 0.5) * 0.02;
    let b = Bobber {
        eid: server.next_entity_id(),
        owner: player.eid,
        dim,
        pos: [eye[0] + d[0] * 0.3, eye[1], eye[2] + d[2] * 0.3],
        vel: [d[0] * speed, d[1] * speed, d[2] * speed],
        in_water: false,
        on_ground: false,
        hooked: None,
        wait: 0,
        nibble: 0,
        lure: rod.enchantment("lure"),
        luck: rod.enchantment("luck_of_the_sea"),
        age: 0,
        ground_ticks: 0,
        sent: [0.0; 3],
    };
    let eid = b.eid;
    server.bobbers.lock().unwrap().insert(eid, b);
    player.state.lock().unwrap().bobber = Some(eid);
    super::sound::at(server, dim, eye, "entity.fishing_bobber.throw", super::sound::NEUTRAL, 0.5, 0.4);
    true
}

fn remove(server: &Server, eid: i32) -> Option<Bobber> {
    let b = server.bobbers.lock().unwrap().remove(&eid)?;
    if let Some(p) = server.players.read().unwrap().get(&b.owner) {
        let mut st = p.state.lock().unwrap();
        if st.bobber == Some(eid) {
            st.bobber = None;
        }
    }
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(eid);
    for p in server.online() {
        if p.tracked.lock().unwrap().remove(&eid) {
            p.send(d.clone());
        }
    }
    Some(b)
}

/// Reel in the line: a fish if it was biting, pull the hooked entity, or nothing.
fn retrieve(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32) {
    let Some(b) = remove(server, eid) else { return };
    let owner = {
        let st = player.state.lock().unwrap();
        [st.x, st.y, st.z]
    };
    let mut wear = 0;
    if let Some(target) = b.hooked {
        // Pull the entity toward the player.
        let pull = |p: [f64; 3]| [(owner[0] - p[0]) * 0.1, (owner[1] - p[1]) * 0.1 + 0.2, (owner[2] - p[2]) * 0.1];
        if let Some(p) = server.players.read().unwrap().get(&target).cloned() {
            let tp = {
                let st = p.state.lock().unwrap();
                [st.x, st.y, st.z]
            };
            let v = pull(tp);
            p.state.lock().unwrap().ac.grace = 30;
            let mut pk = packet(id::play::ENTITY_VELOCITY);
            pk.varint(p.eid);
            for x in v {
                pk.i16((x.clamp(-3.9, 3.9) * 8000.0) as i16);
            }
            server.broadcast_tracked(p.eid, &pk, true);
        } else {
            super::mobs::pull(server, target, owner);
        }
        wear = 5;
    } else if b.nibble > 0 {
        // Fish (or junk, or treasure): flies toward the player.
        let biome = server.world(b.dim).lock().unwrap().generator().biome_name(b.pos[0] as i32, b.pos[2] as i32);
        let open = open_water(&mut server.world(b.dim).lock().unwrap(), b.pos);
        let item = catch(b.luck, open, biome.contains("jungle"));
        let d = [owner[0] - b.pos[0], owner[1] - b.pos[1], owner[2] - b.pos[2]];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let vel = [d[0] * 0.1, d[1] * 0.1 + dist.sqrt() * 0.08, d[2] * 0.1];
        tracing::debug!("{} pescó {} x{}", player.name, item.name(), item.count);
        entities::spawn(server, b.dim, b.pos, vel, item, 0);
        entities::spawn_xp(server, b.dim, [owner[0], owner[1] + 0.5, owner[2] + 0.5], 1 + rand(6));
        wear = 1;
    } else if b.on_ground {
        wear = 2;
    }
    if wear > 0 {
        let st_hand = rod_hand(player).unwrap_or(hand);
        if st_hand == 0 {
            super::health::wear_held(server, player, wear);
        } else {
            let mut st = player.state.lock().unwrap();
            if st.game_mode != 1 {
                let slot = super::inventory::OFFHAND;
                if let Some(i) = &mut st.inv.slots[slot] {
                    if i.wear(wear, &mut entities::random) {
                        st.inv.slots[slot] = None;
                    }
                }
                st.sync_slot(player, slot);
            }
        }
    }
    super::sound::at(server, b.dim, owner, "entity.fishing_bobber.retrieve", super::sound::NEUTRAL, 1.0, 1.0);
}

/// Open water (for treasure): water below the bobber with some depth.
fn open_water(world: &mut World, p: [f64; 3]) -> bool {
    let reg = data::get();
    let (x, y, z) = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
    (-2..=2).all(|dx| (-2..=2).all(|dz| {
        let n = &reg.block_of_state(world.get_block(BlockPos::new(x + dx, y, z + dz))).name;
        n == "water" || n == "air"
    })) && reg.block_of_state(world.get_block(BlockPos::new(x, y - 1, z))).name == "water"
}

/// Fishing loot (vanilla 1.20.4 loot_tables/gameplay/fishing).
pub fn catch(luck: i32, open_water: bool, jungle: bool) -> ItemStack {
    let reg = data::get();
    let item = |n: &str| ItemStack::new(reg.item_id(n).unwrap_or(0), 1);
    let damaged = |n: &str, max_frac: f32| {
        let mut i = item(n);
        let dmg = (i.max_durability() as f32 * entities::random() * max_frac) as i32;
        if dmg > 0 {
            i.wear(dmg, &mut || 1.0);
        }
        i
    };
    // Weights with each group's quality: junk 10 (-2), treasure 5 (+2), fish 85 (-1).
    let junk_w = (10 - 2 * luck).max(0);
    let treasure_w = if open_water { 5 + 2 * luck } else { 0 };
    let fish_w = (85 - luck).max(0);
    let mut r = rand(junk_w + treasure_w + fish_w);
    if r < fish_w {
        let fish = [("cod", 60), ("salmon", 25), ("tropical_fish", 2), ("pufferfish", 13)];
        let mut k = rand(100);
        for (n, w) in fish {
            if k < w {
                return item(n);
            }
            k -= w;
        }
        return item("cod");
    }
    r -= fish_w;
    if r < treasure_w {
        return match rand(6) {
            0 => item("name_tag"),
            1 => item("saddle"),
            2 => super::enchanting::enchant_with_levels(damaged("bow", 0.25), 30, true),
            3 => super::enchanting::enchant_with_levels(damaged("fishing_rod", 0.25), 30, true),
            4 => super::enchanting::enchant_with_levels(item("book"), 30, true),
            _ => item("nautilus_shell"),
        };
    }
    let mut junk: Vec<(&str, i32)> = vec![
        ("lily_pad", 17), ("leather_boots", 10), ("leather", 10), ("bone", 10), ("potion", 10), ("string", 5), ("fishing_rod", 2),
        ("bowl", 10), ("stick", 5), ("ink_sac", 1), ("tripwire_hook", 10), ("rotten_flesh", 10),
    ];
    if jungle {
        junk.push(("bamboo", 10));
    }
    let total: i32 = junk.iter().map(|j| j.1).sum();
    let mut k = rand(total);
    for (n, w) in junk {
        if k < w {
            return match n {
                "leather_boots" | "fishing_rod" => damaged(n, 0.9),
                "potion" => super::effects::potion_item("potion", "water").unwrap_or_else(|| item("glass_bottle")),
                "ink_sac" => item("ink_sac").with_count(10),
                _ => item(n),
            };
        }
        k -= w;
    }
    item("stick")
}

fn in_water(world: &mut World, p: [f64; 3]) -> Option<f64> {
    let b = BlockPos::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
    let reg = data::get();
    let s = world.get_block(b);
    let block = reg.block_of_state(s);
    (block.name == "water").then(|| {
        // Surface height according to the water level.
        let level: f64 = block.get(s, "level").and_then(|l| l.parse().ok()).unwrap_or(0.0);
        b.y as f64 + if level >= 8.0 { 1.0 } else { (8.0 - level) / 9.0 }
    })
}

pub fn tick(server: &Arc<Server>) {
    let players: HashMap<i32, Arc<Player>> = server.online().into_iter().map(|p| (p.eid, p)).collect();
    let mut gone = Vec::new();
    let mut bites: Vec<(i32, Dimension, [f64; 3])> = Vec::new();
    let mut packets: Vec<(i32, Vec<u8>)> = Vec::new();
    {
        let mut bs = server.bobbers.lock().unwrap();
        if bs.is_empty() {
            return;
        }
        let mob_boxes = super::mobs::hit_boxes(server);
        for b in bs.values_mut() {
            b.age += 1;
            // The line breaks if the owner leaves, moves away or stops holding the rod.
            let Some(owner) = players.get(&b.owner) else {
                gone.push(b.eid);
                continue;
            };
            let opos = {
                let st = owner.state.lock().unwrap();
                if st.dead {
                    gone.push(b.eid);
                    continue;
                }
                [st.x, st.y, st.z]
            };
            let far = (0..3).map(|i| (opos[i] - b.pos[i]).powi(2)).sum::<f64>() > 32.0 * 32.0;
            if owner.dim() != b.dim || far || rod_hand(owner).is_none() || b.ground_ticks > 1200 {
                gone.push(b.eid);
                continue;
            }
            let mut world = server.world(b.dim).lock().unwrap();
            // Hooked to an entity: follows it.
            if let Some(h) = b.hooked {
                let target = players.get(&h).map(|p| {
                    let st = p.state.lock().unwrap();
                    [st.x, st.y + 0.9, st.z]
                });
                let target = target.or_else(|| mob_boxes.iter().find(|m| m.0 == h).map(|m| [(m.1 .1.min[0] + m.1 .1.max[0]) / 2.0, m.1 .1.max[1] - 0.2, (m.1 .1.min[2] + m.1 .1.max[2]) / 2.0]));
                match target {
                    Some(t) => b.pos = t,
                    None => b.hooked = None,
                }
            } else if let Some(surface) = in_water(&mut world, b.pos) {
                // Floating: rises to the surface and slows down.
                b.in_water = true;
                b.vel[0] *= 0.9;
                b.vel[2] *= 0.9;
                b.vel[1] = ((surface - 0.1 - b.pos[1]) * 0.2).clamp(-0.1, 0.1);
                for i in 0..3 {
                    b.pos[i] += b.vel[i];
                }
                // Bite.
                if b.nibble > 0 {
                    b.nibble -= 1;
                    if b.nibble == 0 {
                        b.wait = wait_time(b.lure);
                        packets.push((b.eid, metadata(b)));
                    }
                } else {
                    if b.wait <= 0 {
                        b.wait = wait_time(b.lure);
                    }
                    b.wait -= 1;
                    if b.wait <= 0 {
                        b.nibble = 20 + rand(20);
                        b.vel[1] = -0.4 * (0.6 + entities::random() as f64 * 0.4);
                        bites.push((b.eid, b.dim, b.pos));
                        packets.push((b.eid, metadata(b)));
                    }
                }
            } else {
                b.in_water = false;
                // In the air: gravity, collisions and hooking entities.
                let next = [b.pos[0] + b.vel[0], b.pos[1] + b.vel[1], b.pos[2] + b.vel[2]];
                if !b.on_ground {
                    let hit_player = players.values().find(|p| {
                        if p.eid == b.owner || p.dim() != b.dim {
                            return false;
                        }
                        let st = p.state.lock().unwrap();
                        !st.dead && super::projectiles::segment_hits(b.pos, next, &Aabb::entity(st.x, st.y, st.z, 0.6, 1.8).grow(0.2, 0.2, 0.2))
                    });
                    let hit = hit_player.map(|p| p.eid).or_else(|| mob_boxes.iter().find(|m| m.1 .0 == b.dim && super::projectiles::segment_hits(b.pos, next, &m.1 .1.grow(0.2, 0.2, 0.2))).map(|m| m.0));
                    if let Some(h) = hit {
                        b.hooked = Some(h);
                        packets.push((b.eid, metadata(b)));
                        continue;
                    }
                }
                let want = b.vel;
                let d = move_aabb(&mut world, &Aabb::entity(b.pos[0], b.pos[1], b.pos[2], 0.25, 0.25), b.vel);
                for i in 0..3 {
                    b.pos[i] += d[i];
                }
                b.on_ground = want[1] < 0.0 && d[1] > want[1] + 1e-9;
                if b.on_ground {
                    b.vel = [0.0; 3];
                    b.ground_ticks += 1;
                } else {
                    for v in &mut b.vel {
                        *v *= 0.92;
                    }
                    b.vel[1] -= 0.03;
                }
            }
            if (0..3).any(|i| (b.pos[i] - b.sent[i]).abs() > 0.01) {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(b.eid).f64(b.pos[0]).f64(b.pos[1]).f64(b.pos[2]).u8(0).u8(0).bool(b.on_ground);
                packets.push((b.eid, p));
                b.sent = b.pos;
            }
        }
    }
    for e in gone {
        remove(server, e);
    }
    for (eid, p) in packets {
        server.broadcast_tracked(eid, &p, true);
    }
    for (_, dim, pos) in bites {
        // Bubbles (the same 6 as vanilla, so bots know there is a bite) and splash.
        let mut p = packet(id::play::WORLD_PARTICLES);
        p.varint(4).bool(false).f64(pos[0]).f64(pos[1]).f64(pos[2]).f32(0.25).f32(0.0).f32(0.25).f32(0.2).i32(6);
        server.broadcast_dim(dim, &p, None);
        let mut f = packet(id::play::WORLD_PARTICLES);
        f.varint(29).bool(false).f64(pos[0]).f64(pos[1]).f64(pos[2]).f32(0.25).f32(0.0).f32(0.25).f32(0.2).i32(6);
        server.broadcast_dim(dim, &f, None);
        super::sound::at(server, dim, pos, "entity.fishing_bobber.splash", super::sound::NEUTRAL, 0.25, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fishing_loot() {
        let mut fish = 0;
        let mut names = std::collections::HashSet::new();
        for _ in 0..2000 {
            let i = catch(0, true, false);
            if matches!(i.name(), "cod" | "salmon" | "tropical_fish" | "pufferfish") {
                fish += 1;
            }
            names.insert(i.name());
        }
        assert!((1500..1900).contains(&fish), "unos 85 % peces: {fish}");
        assert!(names.len() > 8, "también basura y tesoro: {names:?}");
        // No open water means no treasure.
        assert!((0..500).all(|_| !matches!(catch(3, false, false).name(), "name_tag" | "saddle" | "nautilus_shell")));
        assert!(wait_time(3) >= 20);
    }
}

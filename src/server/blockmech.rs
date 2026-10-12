//! Blocks with a mechanic of their own:
//! - Powder snow freezes whoever is inside it (unless they wear a piece of leather armor):
//!   after 7 s it starts to hurt; skeletons caught in it turn into strays. A bucket scoops it
//!   up and pours it back.
//! - Scaffolding: placed against scaffolding it climbs to the top of the tower (or, against
//!   the top face, reaches out sideways); it holds up to six blocks out from a support and
//!   whatever is left without support falls (as items).
//! - Sculk sensors notice vibrations within 8 blocks (players walking without sneaking,
//!   blocks placed and broken), light up and give a redstone signal that is stronger the
//!   closer the vibration; a summoning shrieker near the sensor then shrieks.

use super::mobs::{Mob, MobKind};
use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ---------- powder snow ----------

/// Ticks of freezing before it starts to hurt.
pub const FROZEN: u32 = 140;

fn in_powder_snow(world: &mut World, pos: [f64; 3], height: f64) -> bool {
    let reg = data::get();
    [0.1, height * 0.9].iter().any(|dy| reg.block_of_state(world.get_block(BlockPos::new(pos[0].floor() as i32, (pos[1] + dy).floor() as i32, pos[2].floor() as i32))).name == "powder_snow")
}

/// Every tick for each player: freezing in powder snow.
pub fn freeze_player(server: &Arc<Server>, player: &Arc<Player>, age: i64) {
    let (pos, leather, frozen, creative) = {
        let st = player.state.lock().unwrap();
        let leather = (5..=8).any(|i| st.inv.slots[i].as_ref().is_some_and(|it| it.name().starts_with("leather_")));
        ([st.x, st.y, st.z], leather, st.frozen, !st.survival_like() || st.dead)
    };
    let inside = in_powder_snow(&mut server.world(player.dim()).lock().unwrap(), pos, 1.8);
    let next = if inside && !leather && !creative { (frozen + 1).min(FROZEN + 20) } else { frozen.saturating_sub(2) };
    if next != frozen {
        player.state.lock().unwrap().frozen = next;
        // The frost on the screen (and the shivering others see).
        if next % 4 == 0 || next == 0 {
            server.broadcast_tracked(player.eid, &frozen_meta(player.eid, next), true);
        }
    }
    // Fully frozen (and still in it): it hurts every two seconds.
    if inside && !leather && !creative && next >= FROZEN && age % 40 == 0 {
        super::health::damage(server, player, 1.0, "freeze", None);
    }
}

pub fn frozen_meta(eid: i32, ticks: u32) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(eid).u8(7).varint(1).varint(ticks as i32).u8(0xff);
    p
}

/// Freezing for mobs (inside the mob tick). Returns true when it should send its metadata.
pub fn freeze_mob(world: &mut World, m: &mut Mob, actions: &mut Vec<super::mobs::Action>) -> bool {
    let immune = matches!(m.kind, MobKind::Stray | MobKind::PolarBear | MobKind::SnowGolem | MobKind::Wither | MobKind::EnderDragon | MobKind::EndCrystal | MobKind::Goat);
    let inside = !immune && in_powder_snow(world, m.pos, m.size().1);
    let before = m.frozen;
    m.frozen = if inside { (m.frozen + 1).min(FROZEN + 20) } else { m.frozen.saturating_sub(2) };
    if m.frozen >= FROZEN && m.age % 40 == 0 {
        // Fire mobs suffer it five times over.
        let dmg = if matches!(m.kind, MobKind::Blaze | MobKind::MagmaCube | MobKind::Strider) { 5.0 } else { 1.0 };
        m.health -= dmg;
        m.hurt_time = 10;
    }
    // A skeleton stuck in powder snow becomes a stray.
    if m.kind == MobKind::Skeleton {
        if m.frozen >= FROZEN {
            m.convert += 1;
            if m.convert >= 300 {
                actions.push(super::mobs::Action::Convert(m.eid, MobKind::Stray));
            }
        } else {
            m.convert = 0;
        }
    }
    (before == 0) != (m.frozen == 0) || (m.frozen > 0 && m.frozen % 10 == 0 && m.frozen != before)
}

/// A bucket on powder snow, or a powder snow bucket on a block. Returns whether it did.
pub fn powder_bucket(server: &Arc<Server>, player: &Arc<Player>, clicked: BlockPos, face: i32, hand: i32, item: &str) -> bool {
    let reg = data::get();
    let dim = player.dim();
    match item {
        "bucket" => {
            let taken = {
                let mut world = server.world(dim).lock().unwrap();
                (reg.block_of_state(world.get_block(clicked)).name == "powder_snow").then(|| world.set_block(clicked, 0))
            };
            if taken.is_none() {
                return false;
            }
            server.broadcast_dim(dim, &super::blocks::block_change(clicked, 0), None);
            give_back(player, hand, "powder_snow_bucket");
            sound::at(server, dim, [clicked.x as f64 + 0.5, clicked.y as f64 + 0.5, clicked.z as f64 + 0.5], "item.bucket.fill_powder_snow", sound::BLOCK, 1.0, 1.0);
            true
        }
        "powder_snow_bucket" => {
            let at = clicked.offset(face);
            let s = reg.state("powder_snow");
            let placed = {
                let mut world = server.world(dim).lock().unwrap();
                reg.is_air(world.get_block(at)) && {
                    world.set_block(at, s);
                    true
                }
            };
            if placed {
                server.broadcast_dim(dim, &super::blocks::block_change(at, s), None);
                give_back(player, hand, "bucket");
                sound::at(server, dim, [at.x as f64 + 0.5, at.y as f64 + 0.5, at.z as f64 + 0.5], "item.bucket.empty_powder_snow", sound::BLOCK, 1.0, 1.0);
            }
            true
        }
        _ => false,
    }
}

fn give_back(player: &Player, hand: i32, item: &str) {
    let mut st = player.state.lock().unwrap();
    let rem = data::get().item_id(item).map(|i| ItemStack::new(i, 1));
    if st.game_mode == 1 {
        // In creative the bucket stays, and a filled one is added if there is none.
        if let Some(r) = rem.filter(|r| !st.inv.slots.iter().flatten().any(|s| s.id == r.id)) {
            st.inv.add(r);
            let p = st.inv.window_items();
            player.send(p);
        }
        return;
    }
    let held = st.held_slot;
    st.inv.consume_held(hand, held, 1, rem);
    let p = st.inv.window_items();
    player.send(p);
}

// ---------- scaffolding ----------

fn scaffold_distance(world: &mut World, p: BlockPos) -> i32 {
    let reg = data::get();
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    let bs = world.get_block(below);
    let bb = reg.block_of_state(bs);
    if bb.name == "scaffolding" {
        return bb.get(bs, "distance").and_then(|v| v.parse().ok()).unwrap_or(7);
    }
    if crate::gameplay::get().shape(bs).iter().any(|s| s[4] >= 1.0) {
        return 0;
    }
    let mut best = 7;
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let q = BlockPos::new(p.x + dx, p.y, p.z + dz);
        let s = world.get_block(q);
        let b = reg.block_of_state(s);
        if b.name == "scaffolding" {
            let d: i32 = b.get(s, "distance").and_then(|v| v.parse().ok()).unwrap_or(7);
            best = best.min(d + 1);
        }
    }
    best
}

/// The state scaffolding should have at `p` (None: it cannot stand there).
pub fn scaffold_state(world: &mut World, p: BlockPos, current: u16) -> Option<u16> {
    let reg = data::get();
    let b = reg.block("scaffolding")?;
    let d = scaffold_distance(world, p);
    if d >= 7 {
        return None;
    }
    let below_is = reg.block_of_state(world.get_block(BlockPos::new(p.x, p.y - 1, p.z))).name == "scaffolding";
    let bottom = d > 0 && !below_is;
    Some(b.with(b.with(current, "distance", &d.to_string()), "bottom", if bottom { "true" } else { "false" }))
}

/// Where scaffolding placed against scaffolding goes: up the tower (clicking a side), or out
/// sideways (clicking the top face). None if there is no room.
pub fn scaffold_target(world: &mut World, clicked: BlockPos, face: i32, look: (i32, i32)) -> Option<BlockPos> {
    let reg = data::get();
    let (dx, dy, dz) = if face == 1 { (look.0, 0, look.1) } else { (0, 1, 0) };
    let mut p = clicked;
    for _ in 0..if dy == 1 { 256 } else { 7 } {
        p = BlockPos::new(p.x + dx, p.y + dy, p.z + dz);
        if !world.dim.in_bounds(p.y) {
            return None;
        }
        let s = world.get_block(p);
        let n = reg.block_of_state(s).name.as_str();
        if n == "scaffolding" {
            continue;
        }
        return (reg.is_air(s) || matches!(n, "water" | "short_grass" | "snow")).then_some(p);
    }
    None
}

/// Scaffolding around a change: new distances, and what lost its support drops.
pub fn scaffold_update(server: &Server, dim: Dimension, spots: Vec<BlockPos>) {
    let reg = data::get();
    let mut changes = Vec::new();
    let mut drops = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        for p in spots {
            let s = world.get_block(p);
            if reg.block_of_state(s).name != "scaffolding" {
                continue;
            }
            match scaffold_state(&mut world, p, s) {
                Some(n) if n != s => {
                    world.set_block(p, n);
                    changes.push((p, n));
                }
                Some(_) => {}
                None => {
                    world.set_block(p, 0);
                    changes.push((p, 0));
                    drops.push(p);
                }
            }
        }
    }
    for (p, s) in changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
    if let Some(i) = reg.item_id("scaffolding") {
        for p in drops {
            entities::drop_at_block(server, dim, p.x, p.y, p.z, ItemStack::new(i, 1));
        }
    }
}

// ---------- sculk sensors ----------

/// Where each player was at the last check (to know who moved).
static LAST: Mutex<Option<HashMap<i32, [f64; 3]>>> = Mutex::new(None);

fn is_sensor(name: &str) -> bool {
    name == "sculk_sensor" || name == "calibrated_sculk_sensor"
}

/// A vibration at `at` (a step, a block placed or broken): the sensors within 8 blocks fire.
pub fn vibration(server: &Arc<Server>, dim: Dimension, at: [f64; 3], by: Option<&Arc<Player>>) {
    let reg = data::get();
    let c = BlockPos::new(at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32);
    let mut fired: Vec<(BlockPos, u16)> = Vec::new();
    let mut shriekers = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        for dx in -8..=8 {
            for dy in -8..=8 {
                for dz in -8..=8 {
                    let p = BlockPos::new(c.x + dx, c.y + dy, c.z + dz);
                    let Some(s) = world.peek_block(p) else { continue };
                    let b = reg.block_of_state(s);
                    if is_sensor(&b.name) && b.get(s, "sculk_sensor_phase") == Some("inactive") {
                        let dist = ((dx * dx + dy * dy + dz * dz) as f64).sqrt();
                        if dist > 8.0 {
                            continue;
                        }
                        let power = (15.0 - (dist * 15.0 / 8.0).floor()).max(1.0) as i32;
                        let n = b.with(b.with(s, "sculk_sensor_phase", "active"), "power", &power.to_string());
                        world.set_block(p, n);
                        fired.push((p, n));
                    }
                }
            }
        }
        if !fired.is_empty() {
            for &(p, _) in &fired {
                for dx in -8..=8 {
                    for dy in -4..=4 {
                        for dz in -8..=8 {
                            let q = BlockPos::new(p.x + dx, p.y + dy, p.z + dz);
                            if let Some(s) = world.peek_block(q) {
                                let b = reg.block_of_state(s);
                                if b.name == "sculk_shrieker" && b.get(s, "can_summon") == Some("true") && !shriekers.contains(&q) {
                                    shriekers.push(q);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    for &(p, s) in &fired {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
        super::redstone::changed(server, dim, p);
        super::redstone::schedule(server, dim, p, 30);
        sound::at(server, dim, [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5], "block.sculk_sensor.clicking", sound::BLOCK, 1.0, 1.0);
    }
    if let (Some(player), Some(&q)) = (by, shriekers.first()) {
        let age = server.age.load(std::sync::atomic::Ordering::Relaxed);
        super::warden::shriek(server, player, at, q, age);
    }
}

/// Scheduled tick of a sensor: active -> cooldown (signal off) -> inactive.
pub fn sensor_tick(state: u16) -> Option<(u16, Option<i64>)> {
    let b = data::get().block_of_state(state);
    if !is_sensor(&b.name) {
        return None;
    }
    match b.get(state, "sculk_sensor_phase") {
        Some("active") => Some((b.with(b.with(state, "sculk_sensor_phase", "cooldown"), "power", "0"), Some(10))),
        Some("cooldown") => Some((b.with(state, "sculk_sensor_phase", "inactive"), None)),
        _ => None,
    }
}

/// Every 10 ticks: players moving about without sneaking make vibrations.
pub fn footsteps(server: &Arc<Server>) {
    let mut moved = Vec::new();
    {
        let mut last = LAST.lock().unwrap();
        let map = last.get_or_insert_with(HashMap::new);
        for p in server.online() {
            let (pos, quiet) = {
                let st = p.state.lock().unwrap();
                ([st.x, st.y, st.z], st.sneaking || st.dead || st.game_mode == 3)
            };
            let before = map.insert(p.eid, pos);
            if !quiet && before.is_some_and(|b| (b[0] - pos[0]).abs() + (b[2] - pos[2]).abs() + (b[1] - pos[1]).abs() > 0.3) {
                moved.push((p, pos));
            }
        }
    }
    for (p, pos) in moved {
        vibration(server, p.dim(), pos, Some(&p));
    }
}

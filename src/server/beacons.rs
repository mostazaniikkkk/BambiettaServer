//! Beacons and conduits.
//! - A beacon on a pyramid of iron, gold, emerald, diamond or netherite blocks (1 to 4
//!   levels) with clear sky above gives its powers to the players around it every 4 s:
//!   chosen in its window by paying an ingot, an emerald or a diamond (speed or haste from
//!   level 1, resistance or jump boost from 2, strength from 3, and at 4 regeneration or the
//!   first power at level II). The range grows with the pyramid.
//! - A conduit under water, in a frame of prismarine and sea lanterns (16 blocks at least),
//!   gives Conduit Power to players in the water nearby, further the bigger its frame; with
//!   the full frame (42) it also strikes monsters in the water close to it.

use super::inventory::WindowKind;
use super::mobs;
use super::{effects, sound, Player, Server};
use crate::data;
use crate::protocol::{BlockPos, Reader};
use crate::world::blockentity::BlockEntity;
use crate::world::{Dimension, World};
use std::sync::Arc;

const BASE: [&str; 5] = ["iron_block", "gold_block", "emerald_block", "diamond_block", "netherite_block"];
const PAYMENT: [&str; 5] = ["iron_ingot", "gold_ingot", "emerald", "diamond", "netherite_ingot"];

pub fn is_payment(item: &str) -> bool {
    PAYMENT.contains(&item)
}

/// Powers available at each level.
fn allowed(effect: &str, levels: i32, secondary: bool) -> bool {
    match effect {
        "speed" | "haste" => levels >= 1 && !secondary,
        "resistance" | "jump_boost" => levels >= 2 && !secondary,
        "strength" => levels >= 3 && !secondary,
        "regeneration" => levels >= 4 && secondary,
        _ => false,
    }
}

/// Pyramid levels under the beacon (0 if none), and whether the beam reaches the sky.
pub fn levels(world: &mut World, p: BlockPos) -> (i32, bool) {
    let reg = data::get();
    let mut levels = 0;
    'outer: for k in 1..=4 {
        for x in p.x - k..=p.x + k {
            for z in p.z - k..=p.z + k {
                let n = reg.block_of_state(world.get_block(BlockPos::new(x, p.y - k, z))).name.as_str();
                if !BASE.contains(&n) {
                    break 'outer;
                }
            }
        }
        levels = k;
    }
    let top = world.dim.min_y() + world.dim.height();
    let sky = (p.y + 1..top).all(|y| {
        let s = world.get_block(BlockPos::new(p.x, y, p.z));
        reg.block_of_state(s).filter_light < 15 || reg.block_of_state(s).name == "bedrock"
    });
    (levels, sky)
}

fn props(player: &Player, levels: i32, primary: Option<i32>, secondary: Option<i32>) {
    let st = player.state.lock().unwrap();
    // The client counts effects from 1 (0 = none).
    for (i, v) in [levels, primary.map_or(0, |e| e + 1), secondary.map_or(0, |e| e + 1)].into_iter().enumerate() {
        player.send(st.inv.property_packet(i as i16, v as i16));
    }
}

/// Right click on a beacon: its window.
pub fn open(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) {
    let dim = player.dim();
    let (lv, prim, sec) = {
        let mut world = server.world(dim).lock().unwrap();
        if world.peek_block_entity(pos).is_none() {
            world.set_block_entity(pos, BlockEntity::Beacon { primary: None, secondary: None });
        }
        let (lv, _) = levels(&mut world, pos);
        match world.peek_block_entity(pos) {
            Some(BlockEntity::Beacon { primary, secondary }) => (lv, *primary, *secondary),
            _ => (lv, None, None),
        }
    };
    {
        let mut st = player.state.lock().unwrap();
        for p in st.inv.open(WindowKind::Beacon, pos, vec![None], "Faro") {
            player.send(p);
        }
    }
    props(player, lv, prim, sec);
}

/// `set_beacon_effect`: the player paid and chose the powers.
pub fn choose(server: &Arc<Server>, player: &Arc<Player>, r: &mut Reader) -> anyhow::Result<()> {
    let primary = if r.bool()? { Some(r.varint()?) } else { None };
    let secondary = if r.bool()? { Some(r.varint()?) } else { None };
    let dim = player.dim();
    let Some(pos) = player.state.lock().unwrap().inv.window.as_ref().filter(|w| w.kind == WindowKind::Beacon && w.slots[0].as_ref().is_some_and(|i| is_payment(i.name()))).map(|w| w.pos) else {
        return Ok(());
    };
    let lv = levels(&mut server.world(dim).lock().unwrap(), pos).0;
    let ok_primary = primary.is_some_and(|e| allowed(effects::name(e), lv, false));
    let ok_secondary = secondary.is_none_or(|e| lv >= 4 && (allowed(effects::name(e), lv, true) || Some(e) == primary));
    if !ok_primary || !ok_secondary {
        return Ok(());
    }
    server.world(dim).lock().unwrap().set_block_entity(pos, BlockEntity::Beacon { primary, secondary });
    {
        let mut st = player.state.lock().unwrap();
        if let Some(w) = st.inv.window.as_mut() {
            w.slots[0] = None;
        }
        let p = st.inv.window_items();
        player.send(p);
    }
    props(player, lv, primary, secondary);
    sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], "block.beacon.power_select", sound::BLOCK, 1.0, 1.0);
    Ok(())
}

/// Every 4 s: beacons give their powers, conduits theirs.
pub fn tick(server: &Arc<Server>, age: i64) {
    if age % 80 == 0 {
        tick_beacons(server);
    }
    if age % 40 == 20 {
        tick_conduits(server);
    }
}

fn tick_beacons(server: &Arc<Server>) {
    for dim in Dimension::ALL {
        let active: Vec<(BlockPos, i32, Option<i32>, Option<i32>)> = {
            let mut world = server.world(dim).lock().unwrap();
            let list = world.loaded_where(|be| matches!(be, BlockEntity::Beacon { .. }));
            list.into_iter()
                .filter_map(|p| {
                    let (prim, sec) = match world.peek_block_entity(p) {
                        Some(BlockEntity::Beacon { primary, secondary }) => (*primary, *secondary),
                        _ => return None,
                    };
                    let (lv, sky) = levels(&mut world, p);
                    (lv > 0 && sky && prim.is_some()).then_some((p, lv, prim, sec))
                })
                .collect()
        };
        for (p, lv, prim, sec) in active {
            let range = (lv * 10 + 10) as f64;
            let duration = (9 + lv * 2) * 20;
            let c = [p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5];
            for pl in server.online().into_iter().filter(|pl| pl.dim() == dim) {
                let at = {
                    let st = pl.state.lock().unwrap();
                    [st.x, st.y, st.z]
                };
                if (at[0] - c[0]).abs() > range || (at[2] - c[2]).abs() > range {
                    continue;
                }
                let Some(e) = prim else { continue };
                let amp = i32::from(sec == Some(e));
                effects::add(server, &pl, effects::name(e), amp, duration, 1.0);
                if let Some(s) = sec.filter(|s| *s != e) {
                    effects::add(server, &pl, effects::name(s), 0, duration, 1.0);
                }
            }
        }
    }
}

/// Frame blocks around a conduit (vanilla's shape), or None if it is not under water.
fn conduit_frame(world: &mut World, p: BlockPos) -> Option<i32> {
    let reg = data::get();
    let wet = |s: u16| {
        let b = reg.block_of_state(s);
        b.name == "water" || b.get(s, "waterlogged") == Some("true")
    };
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if (dx, dy, dz) != (0, 0, 0) && !wet(world.get_block(BlockPos::new(p.x + dx, p.y + dy, p.z + dz))) {
                    return None;
                }
            }
        }
    }
    let mut n = 0;
    for x in -2i32..=2 {
        for y in -2i32..=2 {
            for z in -2i32..=2 {
                let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
                let shell = ax > 1 || ay > 1 || az > 1;
                let ring = (x == 0 && (ay == 2 || az == 2)) || (y == 0 && (ax == 2 || az == 2)) || (z == 0 && (ax == 2 || ay == 2));
                if shell && ring {
                    let name = reg.block_of_state(world.get_block(BlockPos::new(p.x + x, p.y + y, p.z + z))).name.as_str();
                    if matches!(name, "prismarine" | "prismarine_bricks" | "dark_prismarine" | "sea_lantern") {
                        n += 1;
                    }
                }
            }
        }
    }
    Some(n)
}

fn tick_conduits(server: &Arc<Server>) {
    for dim in Dimension::ALL {
        let active: Vec<(BlockPos, i32)> = {
            let mut world = server.world(dim).lock().unwrap();
            let list = world.loaded_where(|be| matches!(be, BlockEntity::Conduit));
            list.into_iter().filter_map(|p| conduit_frame(&mut world, p).filter(|n| *n >= 16).map(|n| (p, n))).collect()
        };
        for (p, frames) in active {
            let range = (frames / 7 * 16) as f64;
            let c = [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5];
            for pl in server.online().into_iter().filter(|pl| pl.dim() == dim) {
                let at = {
                    let st = pl.state.lock().unwrap();
                    [st.x, st.y + 0.5, st.z]
                };
                let wet = data::get().block_of_state(server.world(dim).lock().unwrap().get_block(BlockPos::new(at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32))).name == "water";
                if wet && mobs::dist2(&at, &c) < range * range {
                    effects::add(server, &pl, "conduit_power", 0, 260, 1.0);
                }
            }
            // The full frame strikes the nearest monster in the water.
            if frames >= 42 {
                let prey = {
                    let map = server.mobs.lock().unwrap();
                    map.values()
                        .filter(|m| m.dim == dim && m.kind.hostile() && m.death_ticks.is_none() && mobs::dist2(&m.pos, &c) < 8.0 * 8.0)
                        .min_by(|a, b| mobs::dist2(&a.pos, &c).total_cmp(&mobs::dist2(&b.pos, &c)))
                        .map(|m| m.eid)
                };
                if let Some(e) = prey {
                    mobs::hurt(server, e, 4.0, "magic", None, None, 0);
                    sound::at(server, dim, c, "block.conduit.attack.target", sound::BLOCK, 1.0, 1.0);
                }
            }
        }
    }
}

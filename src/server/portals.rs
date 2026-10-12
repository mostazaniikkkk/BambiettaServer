//! Nether portals (obsidian frame lit with flint and steel), End portal (frame with
//! eyes of ender) and travel between dimensions.

use super::{entities, health, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Ticks inside the portal before traveling in survival (4 s, like vanilla).
const PORTAL_DELAY: u32 = 80;
/// Ticks during which a portal cannot be used again after traveling.
const COOLDOWN: u32 = 100;
/// End arrival platform (like vanilla).
const END_PLATFORM: BlockPos = BlockPos::new(100, 48, 0);

fn name_at(world: &mut World, p: BlockPos) -> String {
    data::get().block_of_state(world.get_block(p)).name.clone()
}

fn free(world: &mut World, p: BlockPos) -> bool {
    let n = name_at(world, p);
    n == "air" || n == "cave_air" || n == "fire"
}

/// Looks for a valid obsidian frame around `inside` and returns its portal blocks.
pub fn find_frame(world: &mut World, inside: BlockPos) -> Option<Vec<(BlockPos, u16)>> {
    let reg = data::get();
    let portal = reg.block("nether_portal")?;
    for (axis, dx, dz) in [("x", 1, 0), ("z", 0, 1)] {
        // Go down to the obsidian floor.
        let mut b = inside;
        let mut steps = 0;
        while free(world, BlockPos::new(b.x, b.y - 1, b.z)) && steps < 21 {
            b.y -= 1;
            steps += 1;
        }
        if name_at(world, BlockPos::new(b.x, b.y - 1, b.z)) != "obsidian" {
            continue;
        }
        // Left and right ends.
        let at = |i: i32, y: i32| BlockPos::new(b.x + dx * i, y, b.z + dz * i);
        let mut left = 0;
        while left > -22 && free(world, at(left - 1, b.y)) {
            left -= 1;
        }
        let mut right = 0;
        while right < 22 && free(world, at(right + 1, b.y)) {
            right += 1;
        }
        let width = right - left + 1;
        if !(2..=21).contains(&width) || name_at(world, at(left - 1, b.y)) != "obsidian" || name_at(world, at(right + 1, b.y)) != "obsidian" {
            continue;
        }
        // Height.
        let mut height = 0;
        while height < 22 && free(world, at(left, b.y + height)) {
            height += 1;
        }
        if !(3..=21).contains(&height) {
            continue;
        }
        // Check the full frame and that the interior is clear.
        let mut ok = true;
        for i in left..=right {
            ok &= name_at(world, at(i, b.y - 1)) == "obsidian" && name_at(world, at(i, b.y + height)) == "obsidian";
            for y in b.y..b.y + height {
                ok &= free(world, at(i, y));
            }
        }
        for y in b.y..b.y + height {
            ok &= name_at(world, at(left - 1, y)) == "obsidian" && name_at(world, at(right + 1, y)) == "obsidian";
        }
        if !ok {
            continue;
        }
        let state = portal.with(portal.default_state, "axis", axis);
        let mut out = Vec::new();
        for i in left..=right {
            for y in b.y..b.y + height {
                out.push((at(i, y), state));
            }
        }
        return Some(out);
    }
    None
}

fn set_and_send(server: &Server, dim: Dimension, changes: &[(BlockPos, u16)]) {
    {
        let mut world = server.world(dim).lock().unwrap();
        for &(p, s) in changes {
            world.set_block(p, s);
        }
    }
    for &(p, s) in changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
}

/// Flint and steel or fire charge on a block: lights a portal or places fire.
/// Returns whether the item was used.
pub fn ignite(server: &Arc<Server>, player: &Arc<Player>, target: BlockPos, item: &ItemStack, hand: i32) -> bool {
    let dim = player.dim();
    let portal = {
        let mut world = server.world(dim).lock().unwrap();
        if !free(&mut world, target) {
            return false;
        }
        find_frame(&mut world, target)
    };
    match portal {
        Some(blocks) if dim != Dimension::End => {
            set_and_send(server, dim, &blocks);
            super::sound::at(server, dim, [target.x as f64 + 0.5, target.y as f64, target.z as f64 + 0.5], "block.portal.trigger", super::sound::BLOCK, 0.6, 1.0);
        }
        _ => {
            super::fire::ignite(server, dim, target);
            super::sound::at(server, dim, [target.x as f64 + 0.5, target.y as f64, target.z as f64 + 0.5], "item.flintandsteel.use", super::sound::BLOCK, 1.0, 1.0);
        }
    }
    if item.name() == "fire_charge" {
        let mut st = player.state.lock().unwrap();
        if st.game_mode != 1 {
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
    } else {
        health::wear_held(server, player, 1);
    }
    true
}

/// When breaking obsidian or a portal block, the whole portal disappears.
pub fn break_portal_near(server: &Server, dim: Dimension, pos: BlockPos) {
    let reg = data::get();
    let mut removed = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        let mut stack: Vec<BlockPos> = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1), (0, 0, 0)]
            .iter()
            .map(|(x, y, z)| BlockPos::new(pos.x + x, pos.y + y, pos.z + z))
            .collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(p) = stack.pop() {
            if !seen.insert(p) || seen.len() > 1000 {
                continue;
            }
            if reg.block_of_state(world.get_block(p)).name != "nether_portal" {
                continue;
            }
            world.set_block(p, 0);
            removed.push(p);
            for (x, y, z) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                stack.push(BlockPos::new(p.x + x, p.y + y, p.z + z));
            }
        }
    }
    for p in removed {
        server.broadcast_dim(dim, &super::blocks::block_change(p, 0), None);
    }
}

/// Eye of ender on an End portal frame: places it and, if the ring is complete,
/// activates the portal. Returns whether it was used.
pub fn place_eye(server: &Arc<Server>, player: &Arc<Player>, frame: BlockPos, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let Some(fb) = reg.block("end_portal_frame") else { return false };
    let state = server.world(dim).lock().unwrap().get_block(frame);
    if reg.block_of_state(state).name != "end_portal_frame" || fb.get(state, "eye") == Some("true") {
        return false;
    }
    set_and_send(server, dim, &[(frame, fb.with(state, "eye", "true"))]);
    {
        let mut st = player.state.lock().unwrap();
        if st.game_mode != 1 {
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
    }
    // Ring of 12 frames with eyes around a 3x3 gap?
    let ring = |c: BlockPos| -> Vec<BlockPos> {
        let mut v = Vec::new();
        for i in -1..=1 {
            v.extend([BlockPos::new(c.x + i, c.y, c.z - 2), BlockPos::new(c.x + i, c.y, c.z + 2), BlockPos::new(c.x - 2, c.y, c.z + i), BlockPos::new(c.x + 2, c.y, c.z + i)]);
        }
        v
    };
    let mut activated = None;
    {
        let mut world = server.world(dim).lock().unwrap();
        'search: for dx in -2..=2 {
            for dz in -2..=2 {
                let c = BlockPos::new(frame.x + dx, frame.y, frame.z + dz);
                let cells = ring(c);
                if !cells.contains(&frame) {
                    continue;
                }
                let all = cells.iter().all(|&p| {
                    let s = world.get_block(p);
                    reg.block_of_state(s).name == "end_portal_frame" && fb.get(s, "eye") == Some("true")
                });
                if all {
                    activated = Some(c);
                    break 'search;
                }
            }
        }
    }
    if let Some(c) = activated {
        let portal = reg.state("end_portal");
        let cells: Vec<(BlockPos, u16)> = (-1..=1).flat_map(|x| (-1..=1).map(move |z| (BlockPos::new(c.x + x, c.y, c.z + z), portal))).collect();
        set_and_send(server, dim, &cells);
        super::sound::at(server, dim, [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5], "block.end_portal.spawn", super::sound::BLOCK, 1.0, 1.0);
    }
    true
}

/// Every tick: is the player inside a portal? Starts the travel when due.
pub fn tick_player(server: &Arc<Server>, player: &Arc<Player>) {
    let dim = player.dim();
    let (feet, head, quick) = {
        let mut st = player.state.lock().unwrap();
        st.portal_cooldown = st.portal_cooldown.saturating_sub(1);
        if st.dead || st.ac.pending_teleport.is_some() {
            return;
        }
        (BlockPos::new(st.x.floor() as i32, st.y.floor() as i32, st.z.floor() as i32), BlockPos::new(st.x.floor() as i32, (st.y + 1.0).floor() as i32, st.z.floor() as i32), st.game_mode == 1)
    };
    let (in_nether_portal, in_end_portal) = {
        let mut world = server.world(dim).lock().unwrap();
        let a = name_at(&mut world, feet);
        let b = name_at(&mut world, head);
        (a == "nether_portal" || b == "nether_portal", a == "end_portal")
    };
    let go = {
        let mut st = player.state.lock().unwrap();
        if in_nether_portal || in_end_portal {
            st.portal_ticks += 1;
        } else {
            st.portal_ticks = 0;
        }
        let needed = if in_end_portal || quick { 1 } else { PORTAL_DELAY };
        let ready = st.portal_ticks >= needed && st.portal_cooldown == 0;
        if ready {
            st.portal_ticks = 0;
            st.portal_cooldown = COOLDOWN;
        }
        ready
    };
    if !go {
        return;
    }
    let server = server.clone();
    let player = player.clone();
    tokio::spawn(async move {
        if in_end_portal {
            travel_end(&server, &player).await;
        } else {
            travel_nether(&server, &player, feet).await;
        }
    });
}

/// Nearest Nether portal to `target` (radius `r`) in `world`.
fn nearest_portal(world: &mut World, target: BlockPos, r: i32) -> Option<BlockPos> {
    let reg = data::get();
    let dim = world.dim;
    let mut best: Option<(i64, BlockPos)> = None;
    for dx in -r..=r {
        for dz in -r..=r {
            for y in dim.min_y()..dim.min_y() + dim.height() {
                let p = BlockPos::new(target.x + dx, y, target.z + dz);
                if reg.block_of_state(world.get_block(p)).name == "nether_portal" {
                    let d = (dx * dx + dz * dz) as i64 + ((y - target.y) as i64).pow(2);
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, p));
                    }
                }
            }
        }
    }
    // Go down to the lowest portal block in that column.
    best.map(|(_, mut p)| {
        while reg.block_of_state(world.get_block(BlockPos::new(p.x, p.y - 1, p.z))).name == "nether_portal" {
            p.y -= 1;
        }
        p
    })
}

/// Builds a portal (4x5 obsidian frame) near `target` and returns its interior.
fn build_portal(world: &mut World, target: BlockPos) -> (BlockPos, Vec<(BlockPos, u16)>) {
    let reg = data::get();
    let dim = world.dim;
    let lo = if dim == Dimension::Nether { 32 } else { dim.min_y() + 8 };
    let hi = if dim == Dimension::Nether { 110 } else { 250 };
    // A spot with ground and enough air; otherwise a platform is forced.
    let mut base = None;
    'find: for r in 0..12i32 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                for y in (lo..hi).rev() {
                    let p = BlockPos::new(target.x + dx, y, target.z + dz);
                    let floor_ok = (-1..=2).all(|i| !crate::gameplay::get().shape(world.get_block(BlockPos::new(p.x + i, p.y - 1, p.z))).is_empty());
                    let air_ok = (-1..=2).all(|i| (0..4).all(|h| reg.is_air(world.get_block(BlockPos::new(p.x + i, p.y + h, p.z)))));
                    if floor_ok && air_ok {
                        base = Some(p);
                        break 'find;
                    }
                }
            }
        }
    }
    let base = base.unwrap_or(BlockPos::new(target.x, target.y.clamp(lo, hi - 6), target.z));
    let (obsidian, portal) = (reg.state("obsidian"), reg.block("nether_portal").unwrap().default_state);
    let portal = reg.block("nether_portal").unwrap().with(portal, "axis", "x");
    let mut changes = Vec::new();
    // Platform and clear space on the sides (in case it had to be forced).
    for dx in -2..=3 {
        for dz in -1..=1 {
            changes.push((BlockPos::new(base.x + dx, base.y - 1, base.z + dz), obsidian));
            for h in 0..4 {
                changes.push((BlockPos::new(base.x + dx, base.y + h, base.z + dz), 0));
            }
        }
    }
    for dx in -1..=2 {
        for h in -1..=3 {
            let edge = dx == -1 || dx == 2 || h == -1 || h == 3;
            changes.push((BlockPos::new(base.x + dx, base.y + h, base.z), if edge { obsidian } else { portal }));
        }
    }
    for &(p, s) in &changes {
        world.set_block(p, s);
    }
    (base, changes)
}

async fn travel_nether(server: &Arc<Server>, player: &Arc<Player>, from: BlockPos) {
    let src = player.dim();
    let dst = if src == Dimension::Nether { Dimension::Overworld } else { Dimension::Nether };
    let scale = |v: i32| if dst == Dimension::Nether { v.div_euclid(8) } else { v * 8 };
    let target = BlockPos::new(scale(from.x), from.y.clamp(dst.min_y() + 10, dst.min_y() + dst.height() - 20), scale(from.z));
    let srv = server.clone();
    let (arrival, built) = tokio::task::spawn_blocking(move || {
        let mut world = srv.world(dst).lock().unwrap();
        let radius = if dst == Dimension::Nether { 16 } else { 32 };
        match nearest_portal(&mut world, target, radius) {
            Some(p) => (p, Vec::new()),
            None => build_portal(&mut world, target),
        }
    })
    .await
    .unwrap();
    for (p, s) in built {
        server.broadcast_dim(dst, &super::blocks::block_change(p, s), None);
    }
    change_dimension(server, player, dst, [arrival.x as f64 + 0.5, arrival.y as f64, arrival.z as f64 + 0.5]).await;
}

async fn travel_end(server: &Arc<Server>, player: &Arc<Player>) {
    if player.dim() == Dimension::End {
        // Exit portal: to the spawn point.
        let spawn = server.spawn;
        let pos = super::beds::respawn_point(server, player).unwrap_or([spawn.x as f64 + 0.5, spawn.y as f64, spawn.z as f64 + 0.5]);
        change_dimension(server, player, Dimension::Overworld, pos).await;
    } else {
        let p = END_PLATFORM;
        let reg = data::get();
        let mut changes = Vec::new();
        {
            let mut world = server.world(Dimension::End).lock().unwrap();
            for dx in -2..=2 {
                for dz in -2..=2 {
                    changes.push((BlockPos::new(p.x + dx, p.y, p.z + dz), reg.state("obsidian")));
                    for h in 1..=3 {
                        changes.push((BlockPos::new(p.x + dx, p.y + h, p.z + dz), 0));
                    }
                }
            }
            for &(q, s) in &changes {
                world.set_block(q, s);
            }
        }
        for (q, s) in changes {
            server.broadcast_dim(Dimension::End, &super::blocks::block_change(q, s), None);
        }
        change_dimension(server, player, Dimension::End, [p.x as f64 + 0.5, p.y as f64 + 1.0, p.z as f64 + 0.5]).await;
    }
}

/// Takes the player to another dimension: `respawn` packet, new chunks and position.
pub async fn change_dimension(server: &Arc<Server>, player: &Arc<Player>, dim: Dimension, pos: [f64; 3]) {
    super::beds::wake(server, player);
    super::vehicles::dismount(server, player, false);
    let (leftovers, game_mode) = {
        let mut st = player.state.lock().unwrap();
        let l = st.inv.close();
        [st.x, st.y, st.z] = pos;
        st.loaded.clear();
        st.view_epoch = st.view_epoch.wrapping_add(1);
        st.fall_distance = 0.0;
        st.digging = None;
        st.eating = None;
        (l, st.game_mode)
    };
    for item in leftovers {
        entities::throw_from(server, player, item);
    }
    player.set_dim(dim);
    player.tracked.lock().unwrap().clear();
    super::tracker::respawn_for_others(server, player.eid);

    let cfg = &server.config;
    let mut p = packet(id::play::RESPAWN);
    p.string(dim.name()).string(dim.name()).i64(cfg.generation.options.seed);
    p.i8(game_mode as i8).u8(255).bool(false).bool(dim == Dimension::Overworld && cfg.generation.name == "flat");
    super::gadgets::death_location(&mut p, player.state.lock().unwrap().last_death);
    p.varint(0);
    // Keep attributes and metadata (0x01 | 0x02).
    p.u8(0x03);
    player.send(p);
    player.send(super::play::abilities(game_mode));
    let (inv, xp, hp) = {
        let mut st = player.state.lock().unwrap();
        (st.inv.window_items(), super::experience::packet_for(&st), health::health_packet(st.health, st.food, st.saturation))
    };
    player.send(hp);
    player.send(inv);
    player.send(xp);
    // The client creates a new player: its effects must be resent.
    super::effects::resend(player);
    super::weather::send_state(server, player);
    super::border::send_state(server, player);
    super::leads::send_knots(server, player);
    super::frames::send_all(player);
    super::decor::send_all(player);
    super::advancements::entered(server, player, dim);
    player.send(super::update_time(server.age.load(Ordering::Relaxed), server.day_time.load(Ordering::Relaxed)));
    let mut ev = packet(id::play::GAME_STATE_CHANGE);
    ev.u8(13).f32(0.0);
    player.send(ev);
    super::play::update_view(server, player, true).await;
    super::play::teleport(server, player, pos[0], pos[1], pos[2]);
    tracing::info!("{} viajó a {}", player.name, dim.name());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn frames_are_detected() {
        let mut cfg = Config::default();
        cfg.generation.name = "flat".into();
        cfg.generation.options.layers = vec![("bedrock".into(), 1), ("stone".into(), 3)];
        cfg.world_folder = std::env::temp_dir().join(format!("bambietta-portal-{}", std::process::id())).display().to_string();
        let mut w = World::new(&cfg, Dimension::Overworld).unwrap();
        let obsidian = data::get().state("obsidian");
        // 4x5 frame (2x3 interior) along x, resting on y = -60.
        for dx in -1..=2 {
            for h in -1..=3 {
                if dx == -1 || dx == 2 || h == -1 || h == 3 {
                    w.set_block(BlockPos::new(dx, -59 + h, 0), obsidian);
                }
            }
        }
        let blocks = find_frame(&mut w, BlockPos::new(0, -58, 0)).expect("marco válido");
        assert_eq!(blocks.len(), 6);
        // Without one corner of the side frame it is no longer valid.
        w.set_block(BlockPos::new(2, -58, 0), 0);
        assert!(find_frame(&mut w, BlockPos::new(0, -58, 0)).is_none());
    }
}

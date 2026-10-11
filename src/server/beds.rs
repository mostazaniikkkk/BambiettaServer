//! Beds: sleeping at night, skipping the night when everyone sleeps, and the respawn point.

use super::{Player, Server};
use crate::data::{self, Block};
use crate::gameplay;
use crate::protocol::{id, packet, text, BlockPos, Writer};
use crate::world::Dimension;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Ticks asleep before the night is skipped (like vanilla).
const SLEEP_TICKS: u32 = 100;

fn step(p: BlockPos, dir: &str, n: i32) -> BlockPos {
    let (dx, dz) = match dir {
        "north" => (0, -1),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => (1, 0),
    };
    BlockPos::new(p.x + dx * n, p.y, p.z + dz * n)
}

/// Head of the bed from either of its halves.
fn head_of(block: &Block, state: u16, pos: BlockPos) -> BlockPos {
    let facing = block.get(state, "facing").unwrap_or("north");
    if block.get(state, "part") == Some("foot") { step(pos, facing, 1) } else { pos }
}

fn action_bar(player: &Player, msg: &str) {
    let mut p = packet(id::play::SYSTEM_CHAT);
    text(msg).write_network(&mut p);
    p.bool(true);
    player.send(p);
}

/// Can you sleep? (at night, like vanilla: time 12542..23460).
pub fn is_night(server: &Server) -> bool {
    (12542..23460).contains(&server.day_time.load(Ordering::Relaxed).rem_euclid(24000))
}

/// Pose metadata: sleeping (with the bed position) or standing.
fn pose_packet(eid: i32, bed: Option<BlockPos>) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(eid);
    p.u8(6).varint(20).varint(if bed.is_some() { 2 } else { 0 });
    // 14: bed position (optional_block_pos).
    p.u8(14).varint(11);
    match bed {
        Some(b) => {
            p.bool(true).position(b);
        }
        None => {
            p.bool(false);
        }
    }
    p.u8(0xff);
    p
}

fn set_occupied(server: &Server, dim: Dimension, head: BlockPos, occupied: bool) {
    let reg = data::get();
    let mut changes = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        let state = world.get_block(head);
        let b = reg.block_of_state(state);
        if !b.name.ends_with("_bed") {
            return;
        }
        let facing = b.get(state, "facing").unwrap_or("north");
        for p in [head, step(head, facing, -1)] {
            let s = world.get_block(p);
            let pb = reg.block_of_state(s);
            if pb.name == b.name {
                let ns = pb.with(s, "occupied", if occupied { "true" } else { "false" });
                world.set_block(p, ns);
                changes.push((p, ns));
            }
        }
    }
    for (p, s) in changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
}

/// Right click on a bed.
pub fn use_bed(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, block: &Block, state: u16) {
    let dim = player.dim();
    let head = head_of(block, state, pos);
    // Like vanilla: beds explode in the Nether and the End.
    if dim != Dimension::Overworld {
        let foot = step(head, block.get(state, "facing").unwrap_or("north"), -1);
        for p in [head, foot] {
            server.world(dim).lock().unwrap().set_block(p, 0);
            server.broadcast_dim(dim, &super::blocks::block_change(p, 0), None);
        }
        super::explosion::explode_ex(server, dim, [head.x as f64 + 0.5, head.y as f64 + 0.5, head.z as f64 + 0.5], 5.0, None, true);
        return;
    }
    let head_state = server.world(dim).lock().unwrap().get_block(head);
    if data::get().block_of_state(head_state).get(head_state, "occupied") == Some("true") {
        action_bar(player, "Esta cama está ocupada");
        return;
    }
    let (eye, survival) = {
        let st = player.state.lock().unwrap();
        (st.eye(), st.survival_like())
    };
    let d2 = (eye[0] - head.x as f64 - 0.5).powi(2) + (eye[1] - head.y as f64 - 0.5).powi(2) + (eye[2] - head.z as f64 - 0.5).powi(2);
    if d2 > 3.5 * 3.5 {
        action_bar(player, "Estás demasiado lejos de la cama");
        return;
    }
    // Using the bed always sets the respawn point.
    {
        let mut st = player.state.lock().unwrap();
        if st.spawn_point != Some(head) {
            st.spawn_point = Some(head);
            player.message("§7Punto de reaparición establecido");
        }
    }
    if !is_night(server) {
        action_bar(player, "Solo puedes dormir de noche o durante las tormentas");
        return;
    }
    // Monsters within 8 blocks horizontally and 5 vertically.
    let near_monsters = survival
        && server.mobs.lock().unwrap().values().any(|m| {
            m.kind.hostile() && m.dim == dim && (m.pos[0] - head.x as f64).abs() < 8.0 && (m.pos[1] - head.y as f64).abs() < 5.0 && (m.pos[2] - head.z as f64).abs() < 8.0
        });
    if near_monsters {
        action_bar(player, "No puedes dormir ahora; hay monstruos cerca");
        return;
    }
    {
        let mut st = player.state.lock().unwrap();
        st.sleeping = Some(head);
        st.sleep_ticks = 0;
        st.eating = None;
    }
    super::play::teleport(server, player, head.x as f64 + 0.5, head.y as f64 + 0.5625, head.z as f64 + 0.5);
    set_occupied(server, dim, head, true);
    let p = pose_packet(player.eid, Some(head));
    player.send(p.clone());
    server.broadcast_tracked(player.eid, &p, false);
}

/// Getting out of bed ("Leave Bed" button, damage, or it became day).
pub fn wake(server: &Arc<Server>, player: &Arc<Player>) {
    let Some(head) = player.state.lock().unwrap().sleeping.take() else { return };
    set_occupied(server, player.dim(), head, false);
    let p = pose_packet(player.eid, None);
    player.send(p.clone());
    server.broadcast_tracked(player.eid, &p, false);
    // Animation 2: leave bed.
    let mut a = packet(id::play::ANIMATION);
    a.varint(player.eid).u8(2);
    server.broadcast_tracked(player.eid, &a, true);
}

/// Every tick: if everyone (except spectators) has slept long enough, it becomes day.
pub fn tick(server: &Arc<Server>) {
    let players = server.online();
    let mut awake = 0;
    let mut ready = 0;
    for p in &players {
        let mut st = p.state.lock().unwrap();
        if st.game_mode == 3 {
            continue;
        }
        match st.sleeping {
            Some(_) => {
                st.sleep_ticks += 1;
                if st.sleep_ticks >= SLEEP_TICKS {
                    ready += 1;
                }
            }
            None => awake += 1,
        }
    }
    if ready == 0 || awake > 0 {
        return;
    }
    let t = server.day_time.load(Ordering::Relaxed);
    let morning = t - t.rem_euclid(24000) + 24000;
    server.day_time.store(morning, Ordering::Relaxed);
    server.broadcast(&super::update_time(server.age.load(Ordering::Relaxed), morning), None);
    for p in &players {
        wake(server, p);
    }
    server.broadcast_message("§eBuenos días");
}

/// Where to respawn: next to the bed if it still exists; otherwise `None` (and it is forgotten).
pub fn respawn_point(server: &Server, player: &Player) -> Option<[f64; 3]> {
    let head = player.state.lock().unwrap().spawn_point?;
    let reg = data::get();
    let g = gameplay::get();
    let mut world = server.world(Dimension::Overworld).lock().unwrap();
    let state = world.get_block(head);
    let bed = reg.block_of_state(state);
    if !bed.name.ends_with("_bed") {
        drop(world);
        player.state.lock().unwrap().spawn_point = None;
        player.message("§cNo tenías cama o estaba obstruida");
        return None;
    }
    let facing = bed.get(state, "facing").unwrap_or("north");
    let foot = step(head, facing, -1);
    // A 2-block clear gap with ground around the bed.
    for base in [head, foot] {
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
            let p = BlockPos::new(base.x + dx, base.y, base.z + dz);
            let free = |q: BlockPos, w: &mut crate::world::World| g.shape(w.get_block(q)).is_empty();
            let floor = BlockPos::new(p.x, p.y - 1, p.z);
            if free(p, &mut world) && free(BlockPos::new(p.x, p.y + 1, p.z), &mut world) && !g.shape(world.get_block(floor)).is_empty() {
                return Some([p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5]);
            }
        }
    }
    // No gap: on top of the bed.
    Some([head.x as f64 + 0.5, head.y as f64 + 0.5625, head.z as f64 + 0.5])
}

//! End gateways. Each time the dragon dies a new gateway appears on a ring around the main
//! island (up to 20, in vanilla's order). Walking into one (or throwing a pearl in) takes you
//! about a thousand blocks out, to the outer islands, where a gateway back is built over the
//! island you land on (a small island is made if there is none). The links are saved in
//! `gateways.json`.
//!
//! The dragon comes back with four End crystals placed on the edges of the exit portal.

use super::{sound, Player, Server};
use crate::data;
use crate::protocol::BlockPos;
use crate::world::Dimension;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct State {
    /// Gateways still to appear (indices around the ring), in order.
    remaining: Vec<i32>,
    /// Gateway -> where it leads (exit gateway).
    links: Vec<([i32; 3], [i32; 3])>,
    started: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Players who just went through (no bouncing back at once).
static COOLDOWN: Mutex<Option<HashMap<i32, i64>>> = Mutex::new(None);

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("gateways.json")
}

fn state(server: &Server) -> std::sync::MutexGuard<'static, Option<State>> {
    let mut g = STATE.lock().unwrap();
    if g.is_none() {
        let mut s: State = std::fs::read_to_string(file(server)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if !s.started {
            // Vanilla shuffles the 20 positions with the world seed.
            let seed = server.config.generation.options.seed as u64;
            let mut order: Vec<i32> = (0..20).collect();
            for i in (1..order.len()).rev() {
                let j = (crate::world::structures::hash(seed, i as i64, 0x6a7e, 1) % (i as u64 + 1)) as usize;
                order.swap(i, j);
            }
            s.remaining = order;
            s.started = true;
        }
        *g = Some(s);
    }
    g
}

fn save(server: &Server, s: &State) {
    let _ = std::fs::write(file(server), serde_json::to_string(s).unwrap_or_default());
}

fn place(server: &Server, center: BlockPos) {
    let changes = crate::world::buildings::gateway_blocks(center);
    {
        let mut world = server.world(Dimension::End).lock().unwrap();
        for &(p, s) in &changes {
            world.set_block(p, s);
        }
    }
    for (p, s) in changes {
        server.broadcast_dim(Dimension::End, &super::blocks::block_change(p, s), None);
    }
}

/// The dragon died: the next gateway on the ring (96 blocks out, at y 75).
pub fn spawn_next(server: &Server) {
    let next = {
        let mut g = state(server);
        let s = g.as_mut().unwrap();
        if s.remaining.is_empty() {
            return;
        }
        let i = s.remaining.remove(0);
        save(server, s);
        i
    };
    let a = 2.0 * std::f64::consts::PI * (-(next as f64)) / 20.0;
    let c = BlockPos::new((96.0 * a.cos()).floor() as i32, 75, (96.0 * a.sin()).floor() as i32);
    place(server, c);
    sound::at(server, Dimension::End, [c.x as f64 + 0.5, 75.5, c.z as f64 + 0.5], "block.end_gateway.spawn", sound::BLOCK, 16.0, 1.0);
}

/// Where a gateway leads, building the exit the first time.
fn destination(server: &Server, from: BlockPos) -> Option<[f64; 3]> {
    if let Some((_, to)) = state(server).as_ref().unwrap().links.iter().find(|(a, _)| *a == [from.x, from.y, from.z]) {
        // Back next to the exit gateway, on the ground below it.
        let top = server.world(Dimension::End).lock().unwrap().generator().end_island_top(to[0], to[2]);
        return Some(match top {
            Some(t) => [to[0] as f64 + 2.5, t as f64 + 1.0, to[2] as f64 + 0.5],
            None => [to[0] as f64 + 0.5, to[1] as f64 - 2.0, to[2] as f64 + 2.5],
        });
    }
    // Inner gateway: about 1024 blocks out in its direction, on the first island found.
    let dist = ((from.x as f64).powi(2) + (from.z as f64).powi(2)).sqrt();
    if dist > 500.0 {
        return None;
    }
    let dir = [from.x as f64 / dist.max(1.0), from.z as f64 / dist.max(1.0)];
    let (island, top) = {
        let world = server.world(Dimension::End).lock().unwrap();
        let g = world.generator();
        let mut found = None;
        'search: for k in 0..64 {
            let d = 1024.0 + k as f64 * 16.0;
            let (x, z) = ((dir[0] * d) as i32, (dir[1] * d) as i32);
            for (ox, oz) in [(0, 0), (8, 0), (-8, 0), (0, 8), (0, -8)] {
                if let Some(t) = g.end_island_top(x + ox, z + oz) {
                    found = Some(((x + ox, z + oz), t));
                    break 'search;
                }
            }
        }
        found.unwrap_or((((dir[0] * 1024.0) as i32, (dir[1] * 1024.0) as i32), 0))
    };
    let top = if top == 0 {
        // No island out there: a small one is made.
        let mut world = server.world(Dimension::End).lock().unwrap();
        let es = data::get().state("end_stone");
        for dx in -4i32..=4 {
            for dz in -4i32..=4 {
                if dx * dx + dz * dz <= 16 {
                    for y in 58..=60 - (dx.abs() + dz.abs()) / 3 {
                        world.set_block(BlockPos::new(island.0 + dx, y, island.1 + dz), es);
                    }
                }
            }
        }
        60
    } else {
        top
    };
    let exit = BlockPos::new(island.0, top + 10, island.1);
    place(server, exit);
    {
        let mut g = state(server);
        let s = g.as_mut().unwrap();
        s.links.push(([from.x, from.y, from.z], [exit.x, exit.y, exit.z]));
        s.links.push(([exit.x, exit.y, exit.z], [from.x, from.y, from.z]));
        save(server, s);
    }
    Some([island.0 as f64 + 0.5, top as f64 + 1.0, island.1 as f64 + 0.5])
}

/// Every tick: a player in a gateway block goes through it.
pub fn check(server: &Arc<Server>, player: &Arc<Player>) {
    if player.dim() != Dimension::End {
        return;
    }
    let (pos, now) = {
        let st = player.state.lock().unwrap();
        ([st.x, st.y, st.z], server.age.load(std::sync::atomic::Ordering::Relaxed))
    };
    let gateway = {
        let mut world = server.world(Dimension::End).lock().unwrap();
        let reg = data::get();
        [0.2, 1.2].into_iter().map(|dy| BlockPos::new(pos[0].floor() as i32, (pos[1] + dy).floor() as i32, pos[2].floor() as i32)).find(|&p| reg.block_of_state(world.get_block(p)).name == "end_gateway")
    };
    let Some(g) = gateway else { return };
    {
        let mut cd = COOLDOWN.lock().unwrap();
        let map = cd.get_or_insert_with(HashMap::new);
        if map.get(&player.eid).is_some_and(|&t| now - t < 40) {
            return;
        }
        map.insert(player.eid, now);
    }
    let Some(to) = destination(server, g) else { return };
    super::vehicles::dismount(server, player, false);
    super::play::teleport(server, player, to[0], to[1], to[2]);
    player.state.lock().unwrap().fall_distance = 0.0;
    sound::at(server, Dimension::End, to, "block.portal.travel", sound::BLOCK, 0.25, 1.0);
}

/// Exit portal edges where the four crystals go to bring the dragon back.
pub const RESPAWN_SPOTS: [[f64; 3]; 4] = [[3.5, 65.0, 0.5], [-2.5, 65.0, 0.5], [0.5, 65.0, 3.5], [0.5, 65.0, -2.5]];

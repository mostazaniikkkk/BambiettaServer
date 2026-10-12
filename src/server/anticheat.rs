//! Movement validation: the server no longer trusts just any position.
//!
//! Checks (can be disabled with `anti-cheat: false`):
//! - impossible displacement in a single packet (> 10 blocks, like vanilla);
//! - sustained horizontal speed (token bucket with a margin for lag);
//! - passing through blocks (noclip), using the real collision shapes;
//! - flying / hovering in survival (rising more than a jump allows or staying still in midair).
//!
//! Ground is computed on the server: the client's `onGround` is not used for fall damage.

use super::PlayerState;
use crate::data;
use crate::protocol::BlockPos;
use crate::world::physics::{block_boxes, on_ground, Aabb};
use crate::world::World;
use std::time::Instant;

/// Accumulable horizontal distance tokens (blocks).
const BURST: f64 = 10.0;

pub struct MoveState {
    pub last_valid: [f64; 3],
    pub last_ground: [f64; 3],
    allowance: f64,
    last: Instant,
    air_ticks: u32,
    hover: u32,
    /// Server teleport pending confirmation: movement is ignored.
    pub pending_teleport: Option<i32>,
    /// Extra tolerance ticks after a knockback (hit, slime bounce...).
    pub grace: u32,
    pub violations: u32,
}

impl MoveState {
    pub fn new(pos: [f64; 3]) -> Self {
        Self { last_valid: pos, last_ground: pos, allowance: BURST, last: Instant::now(), air_ticks: 0, hover: 0, pending_teleport: None, grace: 0, violations: 0 }
    }

    /// Resets tracking after a server teleport.
    pub fn reset(&mut self, pos: [f64; 3], teleport_id: i32) {
        *self = Self { pending_teleport: Some(teleport_id), violations: self.violations, ..Self::new(pos) };
    }
}

/// Player surroundings at the new position.
#[derive(Default, Clone, Copy)]
pub struct Env {
    pub ground: bool,
    pub in_water: bool,
    pub in_lava: bool,
    pub eyes_in_water: bool,
    pub climbing: bool,
    /// Block that cushions falls (slime, bed, hay, cobweb...): damage multiplier.
    pub fall_factor: f64,
    /// Landing on a stalagmite (pointed dripstone pointing up): falls hurt far more.
    pub stalagmite: bool,
}

pub enum Verdict {
    Accept(Env),
    Setback([f64; 3], &'static str),
}

pub fn player_box(x: f64, y: f64, z: f64, sneaking: bool) -> Aabb {
    Aabb::entity(x, y, z, 0.6, if sneaking { 1.5 } else { 1.8 })
}

/// Inspects liquids, ladders and ground around the box.
pub fn environment(world: &mut World, pos: [f64; 3], sneaking: bool, eye_height: f64) -> Env {
    let reg = data::get();
    let aabb = player_box(pos[0], pos[1], pos[2], sneaking);
    let mut env = Env { ground: on_ground(world, &aabb), fall_factor: 1.0, ..Default::default() };
    let lo = |v: f64| v.floor() as i32;
    for x in lo(aabb.min[0] + 0.001)..=lo(aabb.max[0] - 0.001) {
        for z in lo(aabb.min[2] + 0.001)..=lo(aabb.max[2] - 0.001) {
            for y in lo(aabb.min[1] - 0.2)..=lo(aabb.max[1] - 0.001) {
                let state = world.get_block(BlockPos::new(x, y, z));
                let block = reg.block_of_state(state);
                let inside = y >= lo(aabb.min[1]);
                let wet = block.name == "water" || block.name == "bubble_column" || block.name.contains("kelp") || block.name.contains("seagrass") || block.get(state, "waterlogged") == Some("true");
                if inside && wet {
                    env.in_water = true;
                    if y == lo(pos[1] + eye_height) {
                        env.eyes_in_water = true;
                    }
                }
                if inside && block.name == "lava" {
                    env.in_lava = true;
                }
                if inside && matches!(block.name.as_str(), "ladder" | "vine" | "scaffolding" | "twisting_vines" | "twisting_vines_plant" | "weeping_vines" | "weeping_vines_plant" | "cave_vines" | "cave_vines_plant" | "cobweb" | "powder_snow" | "sweet_berry_bush") {
                    env.climbing = true;
                }
                if !inside || y == lo(pos[1] - 0.01) {
                    match block.name.as_str() {
                        "slime_block" => env.fall_factor = 0.0,
                        "pointed_dripstone" if block.get(state, "vertical_direction") == Some("up") => env.stalagmite = true,
                        "hay_block" | "honey_block" => env.fall_factor = env.fall_factor.min(0.2),
                        n if n.ends_with("_bed") => env.fall_factor = env.fall_factor.min(0.5),
                        _ => {}
                    }
                }
            }
        }
    }
    env
}

/// Validates the step from the current position to `new`.
pub fn check(world: &mut World, st: &mut PlayerState, new: [f64; 3], enabled: bool) -> Verdict {
    let old = [st.x, st.y, st.z];
    let eye = if st.sneaking { 1.27 } else { 1.62 };
    let env = environment(world, new, st.sneaking, eye);
    let now = Instant::now();
    let dt = now.duration_since(st.ac.last).as_secs_f64().min(1.0);
    st.ac.last = now;
    st.ac.grace = st.ac.grace.saturating_sub(1);

    if !enabled || st.game_mode == 3 || st.vehicle.is_some() {
        st.ac.last_valid = new;
        return Verdict::Accept(env);
    }
    let d = [new[0] - old[0], new[1] - old[1], new[2] - old[2]];
    let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let setback = |st: &mut PlayerState, why| {
        st.ac.violations += 1;
        Verdict::Setback(st.ac.last_valid, why)
    };

    // 1. Impossible jump in one packet.
    if dist2 > 100.0 {
        return setback(st, "se movió demasiado rápido");
    }

    // 2. Sustained horizontal speed.
    let flying = st.game_mode == 1;
    // Speed (potion) allows going faster: +20 % per level.
    let boost = 1.0 + super::effects::level(st, "speed").map_or(0.0, |a| 0.2 * (a + 1) as f64);
    let max_speed = if flying { 30.0 } else { 12.0 * boost } + if st.ac.grace > 0 { 20.0 } else { 0.0 };
    st.ac.allowance = (st.ac.allowance + dt * max_speed).min(BURST + if st.ac.grace > 0 { 10.0 } else { 0.0 });
    st.ac.allowance -= (d[0] * d[0] + d[2] * d[2]).sqrt();
    if st.ac.allowance < 0.0 {
        st.ac.allowance = BURST / 2.0;
        return setback(st, "velocidad excesiva");
    }

    // 3. Passing through blocks: only if it wasn't already inside one before.
    let shrink = |a: Aabb| a.grow(-0.05, -0.05, -0.05);
    let new_box = shrink(player_box(new[0], new[1], new[2], st.sneaking));
    if !block_boxes(world, &new_box).is_empty() {
        let old_box = shrink(player_box(old[0], old[1], old[2], st.sneaking));
        if block_boxes(world, &old_box).is_empty() {
            return setback(st, "atravesó un bloque");
        }
    }

    // 4. Flying in survival.
    let elytra = st.inv.slots[6].as_ref().is_some_and(|i| i.name() == "elytra");
    let floating = super::effects::has(st, "levitation") || super::effects::has(st, "slow_falling");
    if flying || elytra || floating || env.in_water || env.in_lava || env.climbing || env.ground {
        st.ac.air_ticks = 0;
        st.ac.hover = 0;
        st.ac.last_ground = new;
    } else {
        st.ac.air_ticks += 1;
        let rise = new[1] - st.ac.last_ground[1];
        // Jump Boost: each level adds a bit more than half a block.
        let jump = super::effects::level(st, "jump_boost").map_or(0.0, |a| 0.75 * (a + 1) as f64);
        let max_rise = 1.3 + jump + if st.ac.grace > 0 { 4.0 } else { 0.0 };
        if rise > max_rise {
            let back = st.ac.last_ground;
            st.ac.violations += 1;
            st.ac.air_ticks = 0;
            return Verdict::Setback(back, "vuelo (subió demasiado)");
        }
        if d[1].abs() < 0.005 && st.ac.grace == 0 {
            st.ac.hover += 1;
            if st.ac.hover > 10 {
                let back = st.ac.last_ground;
                st.ac.violations += 1;
                st.ac.hover = 0;
                return Verdict::Setback(back, "vuelo (flotando)");
            }
        } else {
            st.ac.hover = 0;
        }
    }

    st.ac.last_valid = new;
    Verdict::Accept(env)
}

/// Squared distance from the eyes to the center of a block.
pub fn reach2(st: &PlayerState, p: BlockPos) -> f64 {
    let e = st.eye();
    let c = [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5];
    (0..3).map(|i| (e[i] - c[i]).powi(2)).sum()
}

/// Maximum reach (squared) for interacting with blocks and entities.
pub fn max_reach2(st: &PlayerState) -> f64 {
    if st.game_mode == 1 { 8.0 * 8.0 } else { 6.0 * 6.0 }
}

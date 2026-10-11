//! World border (`/worldborder`), like vanilla: a square centred anywhere, that can grow or
//! shrink over time; the client draws it and does not let players through, and the server
//! hurts whoever ends up outside (beyond a safe margin). Saved in `level.dat`. In the Nether
//! its centre is scaled by 1/8, like vanilla.

use super::{health, Player, Server};
use crate::protocol::{id, packet, Writer};
use crate::world::Dimension;
use std::sync::Arc;

pub struct Border {
    pub center: [f64; 2],
    /// Current size, and the size it is moving towards in `lerp_ticks`.
    pub size: f64,
    pub target: f64,
    pub lerp_ticks: i64,
    pub damage_per_block: f64,
    pub safe_zone: f64,
    pub warning_blocks: i32,
    pub warning_time: i32,
}

pub const MAX: f64 = 59_999_968.0;

impl Default for Border {
    fn default() -> Self {
        Self { center: [0.0, 0.0], size: MAX, target: MAX, lerp_ticks: 0, damage_per_block: 0.2, safe_zone: 5.0, warning_blocks: 5, warning_time: 15 }
    }
}

impl Border {
    /// Centre as seen from a dimension (scaled in the Nether).
    fn center_in(&self, dim: Dimension) -> [f64; 2] {
        if dim == Dimension::Nether { [self.center[0] / 8.0, self.center[1] / 8.0] } else { self.center }
    }

    /// How far outside the border (x, z) is (0 if inside).
    pub fn outside(&self, dim: Dimension, x: f64, z: f64) -> f64 {
        let c = self.center_in(dim);
        let half = self.size / 2.0;
        let dx = (x - c[0]).abs() - half;
        let dz = (z - c[1]).abs() - half;
        dx.max(dz).max(0.0)
    }

    fn init_packet(&self, dim: Dimension) -> Vec<u8> {
        let c = self.center_in(dim);
        let mut p = packet(id::play::INITIALIZE_WORLD_BORDER);
        p.f64(c[0]).f64(c[1]).f64(self.size).f64(self.target).varint((self.lerp_ticks * 50) as i32);
        p.varint(29_999_984).varint(self.warning_blocks).varint(self.warning_time);
        p
    }
}

/// The border, as it is now, for a player joining or changing dimension.
pub fn send_state(server: &Server, player: &Player) {
    player.send(server.border.lock().unwrap().init_packet(player.dim()));
}

fn resend_all(server: &Server) {
    let b = server.border.lock().unwrap();
    for p in server.online() {
        p.send(b.init_packet(p.dim()));
    }
}

/// Every tick: the size moves towards its target; once a second, players outside get hurt.
pub fn tick(server: &Arc<Server>, age: i64) {
    {
        let mut b = server.border.lock().unwrap();
        if b.lerp_ticks > 0 {
            b.size += (b.target - b.size) / b.lerp_ticks as f64;
            b.lerp_ticks -= 1;
            if b.lerp_ticks == 0 {
                b.size = b.target;
            }
        }
    }
    if age % 20 != 0 {
        return;
    }
    for p in server.online() {
        let (x, z) = {
            let st = p.state.lock().unwrap();
            (st.x, st.z)
        };
        let (out, safe, per_block) = {
            let b = server.border.lock().unwrap();
            (b.outside(p.dim(), x, z), b.safe_zone, b.damage_per_block)
        };
        if out > safe {
            health::damage(server, &p, ((out - safe) * per_block).max(1.0) as f32, "outside_border", None);
        }
    }
}

/// `/worldborder ...`
pub fn command(server: &Arc<Server>, player: &Arc<Player>, args: &str) -> Result<(), String> {
    let a: Vec<&str> = args.split_whitespace().collect();
    let num = |i: usize| a.get(i).ok_or_else(String::new).and_then(|v| v.parse::<f64>().map_err(|_| format!("Número inválido: {v}")));
    let secs = |i: usize| a.get(i).map_or(Ok(0.0), |v| v.parse::<f64>().map_err(|_| format!("Segundos inválidos: {v}")));
    {
        let mut b = server.border.lock().unwrap();
        match a.first().copied() {
            Some("get") | None => {
                player.message(&format!("§aEl borde mide §f{:.0}§a bloques, centrado en §f{:.1} {:.1}", b.size, b.center[0], b.center[1]));
                return Ok(());
            }
            Some(op @ ("set" | "add")) => {
                let v = num(1)?;
                let size = if op == "add" { b.target + v } else { v }.clamp(1.0, MAX);
                let t = secs(2)?;
                b.target = size;
                if t > 0.0 {
                    b.lerp_ticks = (t * 20.0) as i64;
                } else {
                    b.size = size;
                    b.lerp_ticks = 0;
                }
                player.message(&format!("§aBorde: §f{size:.0}§a bloques{}", if t > 0.0 { format!(" en {t:.0} s") } else { String::new() }));
            }
            Some("center") => {
                b.center = [num(1)?, num(2)?];
                player.message(&format!("§aCentro del borde: §f{:.1} {:.1}", b.center[0], b.center[1]));
            }
            Some("damage") => match a.get(1).copied() {
                Some("amount") => b.damage_per_block = num(2)?,
                Some("buffer") => b.safe_zone = num(2)?,
                _ => return Err(String::new()),
            },
            Some("warning") => match a.get(1).copied() {
                Some("distance") => b.warning_blocks = num(2)? as i32,
                Some("time") => b.warning_time = num(2)? as i32,
                _ => return Err(String::new()),
            },
            _ => return Err(String::new()),
        }
    }
    resend_all(server);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_and_nether_scale() {
        let b = Border { center: [80.0, 0.0], size: 100.0, target: 100.0, ..Default::default() };
        assert_eq!(b.outside(Dimension::Overworld, 100.0, 0.0), 0.0);
        assert_eq!(b.outside(Dimension::Overworld, 140.0, 0.0), 10.0);
        // In the Nether the centre is at x = 10.
        assert_eq!(b.outside(Dimension::Nether, 70.0, 0.0), 10.0);
    }
}

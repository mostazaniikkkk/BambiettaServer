//! Ranged tactics on hard difficulty (skeletons, and pillagers other than the captain's guards):
//! - if the player closes in, it backs off (and keeps shooting);
//! - if the player carries a shield, it moves round to where the shield does not cover
//!   (behind or beside the player) and never shoots into a raised shield;
//! - if the player holds a bow or crossbow, it takes cover out of the player's sight, draws
//!   the bow there, steps out only to shoot and goes back.

use super::mobs::{self, Mob, MobKind, Seen};
use crate::protocol::BlockPos;
use crate::world::pathfind;
use crate::world::World;

/// Height of the shooter's eyes.
fn eye(m: &Mob) -> f64 {
    if m.kind == MobKind::Pillager { 1.62 } else { 1.74 }
}

fn flat(v: [f64; 3]) -> [f64; 3] {
    mobs::normalize([v[0], 0.0, v[2]])
}

fn center(c: BlockPos) -> [f64; 3] {
    [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5]
}

/// Whether a raised shield would stop an arrow coming from `from` (it covers the front half).
pub fn shield_covers(t: &Seen, from: [f64; 3]) -> bool {
    let look = flat(t.look);
    let to = flat([from[0] - t.pos[0], 0.0, from[2] - t.pos[2]]);
    look[0] * to[0] + look[2] * to[2] > -0.2
}

/// Movement for the hard-difficulty tactics: `Some(goal)` when one of them applies
/// (`Some(None)`: stay put). `drawn`: the bow is ready to shoot.
pub fn tactics(world: &mut World, m: &mut Mob, t: &Seen, d: f64, sees: bool, drawn: bool) -> Option<Option<([f64; 3], f64)>> {
    let speed = m.kind.walk_speed();
    let eye_h = eye(m);
    // How fast the player is closing in (blocks per tick, averaged).
    if let Some(prev) = m.last_target {
        let before = mobs::dist2(&prev, &m.pos).sqrt();
        m.approach = m.approach * 0.8 + (before - d) * 0.2;
    }
    m.last_target = Some(t.pos);

    // 1. Shield: go round to its uncovered side, about 9 blocks from the player.
    if t.shield {
        m.cover = None;
        if !shield_covers(t, m.pos) && d < 14.0 {
            return Some(None);
        }
        let look = flat(t.look);
        // Behind the player, on the side the skeleton already is.
        let side = if look[0] * (m.pos[2] - t.pos[2]) - look[2] * (m.pos[0] - t.pos[0]) > 0.0 { 1.0 } else { -1.0 };
        let a = look[2].atan2(look[0]) + side * 2.4;
        let dest = [t.pos[0] + a.cos() * 9.0, t.pos[1], t.pos[2] + a.sin() * 9.0];
        return Some(Some((mobs::follow_path(world, m, dest), speed * 1.4)));
    }

    // 2. Closing in: back off, away from the player.
    if d < 7.0 || (m.approach > 0.08 && d < 12.0) {
        m.cover = None;
        let away = flat([m.pos[0] - t.pos[0], 0.0, m.pos[2] - t.pos[2]]);
        let dest = [m.pos[0] + away[0] * 6.0, m.pos[1], m.pos[2] + away[2] * 6.0];
        return Some(Some((mobs::follow_path(world, m, dest), speed * 1.4)));
    }

    // 3. Archer: cover out of its sight, step out only to shoot.
    if t.archer {
        let still_hidden = m.cover.is_some_and(|(c, _)| hidden(world, t, c, eye_h));
        if !still_hidden && m.age % 10 == 0 {
            m.cover = find_cover(world, m, t);
        }
        if let Some((cover, peek)) = m.cover {
            let dest = if drawn { center(peek) } else { center(cover) };
            let there = mobs::dist2(&dest, &m.pos) < 0.2 * 0.2;
            // At the peek spot and still no line of sight: that cover is no good.
            if drawn && there && !sees {
                m.cover = None;
                return None;
            }
            if there {
                m.vel[0] = 0.0;
                m.vel[2] = 0.0;
                return Some(None);
            }
            return Some(Some((mobs::follow_path(world, m, dest), speed * 1.3)));
        }
    } else {
        m.cover = None;
    }
    None
}

/// The player cannot see any of the body standing at `c`: head and both sides, at eye and
/// waist height.
fn hidden(world: &mut World, t: &Seen, c: BlockPos, eye_h: f64) -> bool {
    let p = center(c);
    let side = flat([p[2] - t.pos[2], 0.0, t.pos[0] - p[0]]);
    [eye_h, eye_h * 0.5].iter().all(|&h| {
        [-0.35, 0.0, 0.35].iter().all(|&o| !mobs::line_of_sight(world, t.eye, [p[0] + side[0] * o, p[1] + h, p[2] + side[2] * o]))
    })
}

/// The player can be shot from anywhere around `c` (the centre and 0.35 to each side), so a
/// mob that stops a little off the centre still has a clear shot.
fn exposed(world: &mut World, t: &Seen, c: BlockPos, eye_h: f64) -> bool {
    let p = center(c);
    let side = flat([p[2] - t.pos[2], 0.0, t.pos[0] - p[0]]);
    [-0.35, 0.0, 0.35].iter().all(|&o| mobs::line_of_sight(world, [p[0] + side[0] * o, p[1] + eye_h, p[2] + side[2] * o], t.eye))
}

/// Somewhere close the player cannot see (cover), next to a spot from where it can be shot
/// (peek). The closest such pair within 8 blocks.
fn find_cover(world: &mut World, m: &Mob, t: &Seen) -> Option<(BlockPos, BlockPos)> {
    let (w, h) = m.size();
    let base = BlockPos::new(m.pos[0].floor() as i32, (m.pos[1] + 0.01).floor() as i32, m.pos[2].floor() as i32);
    let h_eye = eye(m);
    let mut best: Option<(i32, BlockPos, BlockPos)> = None;
    for r in 1..=8i32 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                for dy in [0, 1, -1] {
                    let c = BlockPos::new(base.x + dx, base.y + dy, base.z + dz);
                    if !pathfind::standable(world, c, w, h) {
                        continue;
                    }
                    // Hidden from the player, and not right next to it.
                    let far = mobs::dist2(&center(c), &t.pos) > 6.0 * 6.0;
                    if !far || !hidden(world, t, c, h_eye) {
                        break;
                    }
                    // A spot one or two steps away from where it sees the player.
                    let peek = [(1, 0), (-1, 0), (0, 1), (0, -1), (2, 0), (-2, 0), (0, 2), (0, -2)].into_iter().map(|(px, pz)| BlockPos::new(c.x + px, c.y, c.z + pz)).find(|&p| {
                        pathfind::standable(world, p, w, h) && exposed(world, t, p, h_eye)
                    });
                    if let Some(p) = peek {
                        let cost = dx * dx + dz * dz;
                        if best.is_none_or(|b| cost < b.0) {
                            best = Some((cost, c, p));
                        }
                    }
                    break;
                }
            }
        }
        if best.is_some() {
            break;
        }
    }
    best.map(|b| (b.1, b.2))
}

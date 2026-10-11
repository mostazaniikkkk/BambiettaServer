//! Block collisions using the real shapes of each state (blockCollisionShapes).

use super::World;
use crate::gameplay;
use crate::protocol::BlockPos;

const EPS: f64 = 1e-7;

#[derive(Clone, Copy, Debug)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Aabb {
    /// Box of an entity of width `w` and height `h` with its feet at (x, y, z).
    pub fn entity(x: f64, y: f64, z: f64, w: f64, h: f64) -> Self {
        let r = w / 2.0;
        Self { min: [x - r, y, z - r], max: [x + r, y + h, z + r] }
    }

    pub fn offset(&self, d: [f64; 3]) -> Self {
        Self {
            min: [self.min[0] + d[0], self.min[1] + d[1], self.min[2] + d[2]],
            max: [self.max[0] + d[0], self.max[1] + d[1], self.max[2] + d[2]],
        }
    }

    pub fn grow(&self, x: f64, y: f64, z: f64) -> Self {
        Self { min: [self.min[0] - x, self.min[1] - y, self.min[2] - z], max: [self.max[0] + x, self.max[1] + y, self.max[2] + z] }
    }

    pub fn intersects(&self, o: &Aabb) -> bool {
        (0..3).all(|i| self.min[i] < o.max[i] - EPS && self.max[i] > o.min[i] + EPS)
    }

    /// Clips the displacement `d` along axis `axis` so it doesn't pass through `o`.
    fn clip(&self, o: &Aabb, axis: usize, mut d: f64) -> f64 {
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let overlap = |i: usize| self.max[i] > o.min[i] + EPS && self.min[i] < o.max[i] - EPS;
        if !overlap(a) || !overlap(b) {
            return d;
        }
        if d > 0.0 && self.max[axis] <= o.min[axis] + EPS {
            d = d.min(o.min[axis] - self.max[axis]);
        } else if d < 0.0 && self.min[axis] >= o.max[axis] - EPS {
            d = d.max(o.max[axis] - self.min[axis]);
        }
        d
    }
}

/// Collision boxes of the blocks touching `area`.
pub fn block_boxes(world: &mut World, area: &Aabb) -> Vec<Aabb> {
    let g = gameplay::get();
    let mut out = Vec::new();
    let lo = |v: f64| v.floor() as i32;
    for x in lo(area.min[0])..=lo(area.max[0]) {
        for z in lo(area.min[2])..=lo(area.max[2]) {
            // -1 on Y for fences and walls (1.5 tall).
            for y in lo(area.min[1]) - 1..=lo(area.max[1]) {
                let state = world.get_block(BlockPos::new(x, y, z));
                for s in g.shape(state) {
                    let b = Aabb { min: [x as f64 + s[0], y as f64 + s[1], z as f64 + s[2]], max: [x as f64 + s[3], y as f64 + s[4], z as f64 + s[5]] };
                    if b.intersects(area) {
                        out.push(b);
                    }
                }
            }
        }
    }
    out
}

pub fn collides(world: &mut World, aabb: &Aabb) -> bool {
    !block_boxes(world, aabb).is_empty()
}

/// Moves the box (Y, then X, then Z) stopping against blocks.
/// Returns the actual displacement.
pub fn move_aabb(world: &mut World, aabb: &Aabb, d: [f64; 3]) -> [f64; 3] {
    let sweep = Aabb {
        min: [aabb.min[0] + d[0].min(0.0), aabb.min[1] + d[1].min(0.0), aabb.min[2] + d[2].min(0.0)],
        max: [aabb.max[0] + d[0].max(0.0), aabb.max[1] + d[1].max(0.0), aabb.max[2] + d[2].max(0.0)],
    };
    let boxes = block_boxes(world, &sweep.grow(EPS, EPS, EPS));
    let mut cur = *aabb;
    let mut out = [0.0; 3];
    for axis in [1, 0, 2] {
        let mut v = d[axis];
        for b in &boxes {
            v = cur.clip(b, axis, v);
        }
        let mut delta = [0.0; 3];
        delta[axis] = v;
        cur = cur.offset(delta);
        out[axis] = v;
    }
    out
}

/// Is there a solid block right below the box?
pub fn on_ground(world: &mut World, aabb: &Aabb) -> bool {
    let below = Aabb { min: [aabb.min[0], aabb.min[1] - 0.06, aabb.min[2]], max: [aabb.max[0], aabb.min[1], aabb.max[2]] };
    collides(world, &below)
}

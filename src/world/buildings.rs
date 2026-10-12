//! More structures: End cities (with their ships), mineshafts, jungle temples, shipwrecks,
//! ocean ruins, ruined portals, ocean monuments, ancient cities and bastion remnants, plus
//! the shape of End gateways. Simplified next to vanilla's, but with their materials, their
//! loot and the mobs that live in them (placed when a player first comes close).

use super::structures::{hash, st, with, Loot, Structure};
use crate::protocol::BlockPos;

fn rng(seed: u64) -> impl FnMut() -> u64 {
    let mut n = 0i64;
    move || {
        n += 1;
        hash(seed, n, 31, 7)
    }
}

/// Picks one of `options` with `r`.
fn one(r: u64, options: &[u16]) -> u16 {
    options[(r % options.len() as u64) as usize]
}

// ---------- End gateways ----------

/// An End gateway centred on `c`: the gateway block, a cross of bedrock above and below it
/// and bedrock caps two blocks up and down (like vanilla's EndGatewayFeature).
pub fn gateway_blocks(c: BlockPos) -> Vec<(BlockPos, u16)> {
    let (bedrock, gateway) = (st("bedrock"), st("end_gateway"));
    let mut out = Vec::new();
    for dx in -1..=1 {
        for dy in -2..=2i32 {
            for dz in -1..=1 {
                let p = BlockPos::new(c.x + dx, c.y + dy, c.z + dz);
                let s = if dx == 0 && dy == 0 && dz == 0 {
                    gateway
                } else if dy == 0 {
                    0
                } else if dy.abs() == 2 {
                    if dx == 0 && dz == 0 { bedrock } else { 0 }
                } else if dx == 0 || dz == 0 {
                    bedrock
                } else {
                    0
                };
                out.push((p, s));
            }
        }
    }
    out
}

// ---------- End city ----------

/// An End city on an outer island at ground height `g`: a house, a four-storey tower with a
/// lookout on top, and sometimes an End ship floating beside it with an elytra in an item
/// frame. Chests with the End city loot; shulkers at the doors, on the tower and on the ship.
pub fn end_city(x: i32, z: i32, g: i32, seed: u64) -> Structure {
    let mut s = Structure::of("end_city");
    let mut r = rng(seed ^ x as u64 ^ (z as u64) << 20);
    let (pur, bricks, glass, rod) = (st("purpur_block"), st("end_stone_bricks"), st("magenta_stained_glass"), with("end_rod", &[("facing", "up")]));
    let pillar = with("purpur_pillar", &[("axis", "y")]);
    let slab = st("purpur_slab");
    // Foundation into the island.
    s.fill((x - 6, g - 3, z - 6), (x + 6, g, z + 6), |_, _, _| bricks);
    // House: 11 x 11, five high.
    let y0 = g + 1;
    s.fill((x - 5, y0, z - 5), (x + 5, y0 + 5, z + 5), |bx, by, bz| {
        let edge_x = (bx - x).abs() == 5;
        let edge_z = (bz - z).abs() == 5;
        if edge_x && edge_z {
            pillar
        } else if edge_x || edge_z {
            if by == y0 + 2 && ((bx - x).abs() == 2 || (bz - z).abs() == 2) { glass } else { pur }
        } else {
            0
        }
    });
    s.fill((x - 6, y0 + 6, z - 6), (x + 6, y0 + 6, z + 6), |_, _, _| slab);
    // Door (south).
    s.fill((x - 1, y0, z + 5), (x + 1, y0 + 2, z + 5), |_, _, _| 0);
    s.chest(BlockPos::new(x - 4, y0, z - 4), "south", Loot::EndCity);
    s.chest(BlockPos::new(x + 4, y0, z - 4), "south", Loot::EndCity);
    s.mobs.push(([x as f64 - 3.5, y0 as f64, z as f64 + 3.5], "shulker"));
    s.mobs.push(([x as f64 + 0.5, y0 as f64 + 7.0, z as f64 + 7.5], "shulker"));
    // Tower: 7 x 7, four storeys of six, with a ladder up the north wall.
    let top = y0 + 6 + 24;
    s.fill((x - 3, y0 + 6, z - 3), (x + 3, top, z + 3), |bx, by, bz| {
        let edge_x = (bx - x).abs() == 3;
        let edge_z = (bz - z).abs() == 3;
        let level = (by - y0 - 6) % 6;
        if edge_x && edge_z {
            pillar
        } else if edge_x || edge_z {
            if level == 3 && ((bx - x).abs() == 1 || (bz - z).abs() == 1 || bx == x || bz == z) { glass } else { pur }
        } else if level == 0 && by > y0 + 6 {
            pur
        } else {
            0
        }
    });
    // The ladder: a column inside the house to lean on, then up the tower's north wall.
    let ladder = with("ladder", &[("facing", "south")]);
    for y in y0..=y0 + 5 {
        s.set(x, y, z - 3, pur);
    }
    for y in y0..top + 1 {
        s.set(x, y, z - 2, ladder);
    }
    // Lookout: a wide platform with end rods and battlements.
    s.fill((x - 5, top + 1, z - 5), (x + 5, top + 1, z + 5), |bx, _, bz| if bx == x && bz == z - 2 { ladder } else { pur });
    s.fill((x - 5, top + 2, z - 5), (x + 5, top + 2, z + 5), |bx, _, bz| if ((bx - x).abs() == 5 || (bz - z).abs() == 5) && (bx + bz) % 2 == 0 { pur } else { 0 });
    for (cx, cz) in [(x - 5, z - 5), (x + 5, z - 5), (x - 5, z + 5), (x + 5, z + 5)] {
        s.set(cx, top + 3, cz, rod);
    }
    s.chest(BlockPos::new(x - 3, top + 2, z + 3), "north", Loot::EndCity);
    s.chest(BlockPos::new(x + 3, top + 2, z + 3), "north", Loot::EndCity);
    s.mobs.push(([x as f64 + 3.5, top as f64 + 2.0, z as f64 - 3.5], "shulker"));
    s.mobs.push(([x as f64 - 3.5, top as f64 + 2.0, z as f64 - 3.5], "shulker"));
    // Sometimes a ship.
    if r() % 2 == 0 {
        end_ship(&mut s, x + 20, top - 6, z, &mut r);
    }
    s
}

/// The End ship: a purpur hull with a cabin (two chests and the elytra in an item frame),
/// a mast and a dragon head at the bow.
fn end_ship(s: &mut Structure, x: i32, y: i32, z: i32, r: &mut impl FnMut() -> u64) {
    let (pur, slab, obsidian) = (st("purpur_block"), st("purpur_slab"), st("obsidian"));
    let len = 14;
    for i in 0..=len {
        // The hull narrows towards bow and stern.
        let half = if i == 0 || i == len { 0 } else if i == 1 || i == len - 1 { 1 } else { 2 };
        s.fill((x + i, y, z - half), (x + i, y, z + half), |_, _, _| pur);
        for dy in 1..=2 {
            s.set(x + i, y + dy, z - half, pur);
            s.set(x + i, y + dy, z + half, pur);
            if half == 2 {
                s.fill((x + i, y + dy, z - 1), (x + i, y + dy, z + 1), |_, _, _| 0);
            }
        }
        s.fill((x + i, y + 3, z - half), (x + i, y + 3, z + half), |_, _, _| slab);
    }
    // Cabin walls at both ends, a hatch in the deck.
    s.fill((x + 3, y + 1, z - 1), (x + 3, y + 2, z + 1), |_, _, _| pur);
    s.fill((x + 11, y + 1, z - 1), (x + 11, y + 2, z + 1), |_, _, _| pur);
    s.set(x + 5, y + 3, z, 0);
    s.set(x + 5, y + 2, z, with("ladder", &[("facing", "south")]));
    s.set(x + 5, y + 2, z - 1, pur);
    s.set(x + 5, y + 1, z, with("ladder", &[("facing", "south")]));
    s.set(x + 5, y + 1, z - 1, pur);
    s.chest(BlockPos::new(x + 9, y + 1, z - 1), "south", Loot::EndCity);
    s.chest(BlockPos::new(x + 9, y + 1, z + 1), "north", Loot::EndCity);
    // The elytra, in a frame on the cabin's end wall (facing west).
    s.frames.push((BlockPos::new(x + 10, y + 2, z), 4, "elytra"));
    // Mast and bow.
    for dy in 4..=10 {
        s.set(x + 7, y + dy, z, obsidian);
    }
    s.set(x + len + 1, y + 2, z, with("dragon_head", &[("rotation", "12")]));
    s.mobs.push(([x as f64 + 7.5, y as f64 + 4.0, z as f64 + 1.5], "shulker"));
    let _ = r();
}

// ---------- mineshaft ----------

/// A mineshaft: a dirt room and corridors (3 x 3, oak supports every five blocks, rails,
/// cobwebs) branching out in every direction, with chests and now and then a cave spider
/// spawner in a cobweb nest. Bridges where it crosses a cave.
pub fn mineshaft(x: i32, y: i32, z: i32, seed: u64) -> Structure {
    let mut s = Structure::of("mineshaft");
    let mut r = rng(seed ^ 0x3157);
    let (planks, fence, web, dirt) = (st("oak_planks"), st("oak_fence"), st("cobweb"), st("dirt"));
    s.fill((x - 5, y - 1, z - 5), (x + 5, y + 4, z + 5), |_, by, _| if by == y - 1 { dirt } else { 0 });
    let mut todo: Vec<(i32, i32, i32, (i32, i32), u32)> = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|&d| (x + d.0 * 5, y, z + d.1 * 5, d, 0)).collect();
    while let Some((sx, sy, sz, d, depth)) = todo.pop() {
        let len = 10 + (r() % 4) as i32 * 5;
        let rail = with("rail", &[("shape", if d.0 != 0 { "east_west" } else { "north_south" })]);
        let side = (d.1.abs(), d.0.abs());
        let chest_at = (r() % 3 == 0).then(|| 3 + (r() % (len as u64 - 4)) as i32);
        let spawner_at = (r() % 12 == 0).then(|| len / 2);
        for i in 0..len {
            let (cx, cz) = (sx + d.0 * i, sz + d.1 * i);
            for w in -1..=1 {
                let (wx, wz) = (cx + side.0 * w, cz + side.1 * w);
                // Floor (a bridge of planks over caves), the tunnel, a little ceiling.
                s.set(wx, sy - 1, wz, if r() % 9 == 0 { st("oak_planks") } else { st("stone") });
                for dy in 0..3 {
                    s.set(wx, sy + dy, wz, if dy == 2 && w != 0 && r() % 14 == 0 { web } else { 0 });
                }
            }
            if i % 5 == 0 {
                for w in [-1, 1] {
                    let (wx, wz) = (cx + side.0 * w, cz + side.1 * w);
                    s.set(wx, sy, wz, fence);
                    s.set(wx, sy + 1, wz, fence);
                }
                for w in -1..=1 {
                    s.set(cx + side.0 * w, sy + 2, cz + side.1 * w, planks);
                }
            } else if r() % 10 < 7 {
                s.set(cx, sy, cz, rail);
            }
            if chest_at == Some(i) {
                let w = if r() % 2 == 0 { 1 } else { -1 };
                let facing = match (side.0 * -w, side.1 * -w) {
                    (1, _) => "east",
                    (-1, _) => "west",
                    (_, 1) => "south",
                    _ => "north",
                };
                s.chest(BlockPos::new(cx + side.0 * w, sy, cz + side.1 * w), facing, Loot::Mineshaft);
            }
            if spawner_at == Some(i) {
                s.spawner(BlockPos::new(cx, sy, cz), "cave_spider");
                for w in -1..=1 {
                    for dy in 0..3 {
                        if !(w == 0 && dy == 0) {
                            s.set(cx + side.0 * w, sy + dy, cz + side.1 * w, web);
                        }
                    }
                }
            }
        }
        // Branches at the end.
        if depth < 3 {
            let (ex, ez) = (sx + d.0 * len, sz + d.1 * len);
            for nd in [d, (d.1, d.0), (-d.1, -d.0)] {
                if r() % 3 != 0 {
                    let dy = if r() % 4 == 0 { if r() % 2 == 0 { 4 } else { -4 } } else { 0 };
                    todo.push((ex, (sy + dy).clamp(-50, 50), ez, nd, depth + 1));
                }
            }
            // A crossing.
            s.fill((ex - 1, sy, ez - 1), (ex + 1, sy + 3, ez + 1), |_, _, _| 0);
        }
    }
    s
}

// ---------- jungle temple ----------

/// A jungle temple of mossy cobblestone: an entrance hall, an upper floor, and below, a
/// passage guarded by an arrow trap (dispenser and pressure plate) and two hidden chests.
pub fn jungle_temple(x: i32, z: i32, g: i32, seed: u64) -> Structure {
    let mut s = Structure::of("jungle_temple");
    let mut r = rng(seed ^ 0x7e3b);
    let mut stone = move || one(r(), &[st("mossy_cobblestone"), st("cobblestone"), st("cobblestone"), st("mossy_cobblestone"), st("chiseled_stone_bricks")]);
    s.fill((x - 6, g - 4, z - 7), (x + 5, g + 9, z + 7), |_, _, _| stone());
    // Ground floor, upper floor, roof.
    s.fill((x - 5, g + 1, z - 6), (x + 4, g + 3, z + 6), |_, _, _| 0);
    s.fill((x - 4, g + 5, z - 5), (x + 3, g + 7, z + 5), |_, _, _| 0);
    s.fill((x - 4, g + 10, z - 5), (x + 3, g + 11, z + 5), |_, _, _| stone());
    s.fill((x - 3, g + 10, z - 4), (x + 2, g + 10, z + 4), |_, _, _| 0);
    // Entrance (north) and stairs up.
    s.fill((x - 1, g + 1, z - 7), (x, g + 3, z - 7), |_, _, _| 0);
    let up = with("cobblestone_stairs", &[("facing", "south")]);
    for k in 0..4 {
        s.set(x + 3, g + 1 + k, z - 2 + k, up);
        s.set(x + 3, g + 2 + k, z - 2 + k, 0);
        s.set(x + 3, g + 3 + k, z - 2 + k, 0);
    }
    // Windows.
    for wz in [z - 4, z, z + 4] {
        s.set(x - 6, g + 6, wz, 0);
        s.set(x + 5, g + 6, wz, 0);
    }
    // Basement: stairs down at the back, the trap passage and the chests.
    s.fill((x - 4, g - 3, z - 5), (x + 3, g - 1, z + 5), |_, _, _| 0);
    let down = with("cobblestone_stairs", &[("facing", "north")]);
    for k in 0..4 {
        s.set(x - 4, g - k, z + 2 + k, down);
        s.set(x - 4, g + 1 - k, z + 2 + k, 0);
        s.set(x - 4, g + 2 - k, z + 2 + k, 0);
    }
    s.set(x + 1, g - 3, z - 2, st("stone_pressure_plate"));
    s.set(x + 1, g - 3, z - 3, with("dispenser", &[("facing", "south")]));
    s.traps.push(BlockPos::new(x + 1, g - 3, z - 3));
    s.chest(BlockPos::new(x + 2, g - 3, z - 5), "south", Loot::JungleTemple);
    s.chest(BlockPos::new(x - 3, g - 3, z - 5), "south", Loot::JungleTemple);
    s
}

// ---------- shipwreck ----------

/// A wrecked wooden ship lying on the sea floor (or on a beach): spruce hull, a broken mast,
/// and the supply, map and treasure chests. Inside it is water when it lies under the sea.
pub fn shipwreck(x: i32, z: i32, floor: i32, underwater: bool, seed: u64) -> Structure {
    let mut s = Structure::of("shipwreck");
    let mut r = rng(seed ^ 0x5419);
    let (planks, log) = (st("spruce_planks"), with("spruce_log", &[("axis", "y")]));
    let inside = if underwater { st("water") } else { 0 };
    let y = floor + 1;
    let len = 20;
    let broken = r() % 3 == 0;
    for i in 0..=len {
        if broken && i > len - 5 {
            break;
        }
        let half = if i == 0 || i == len { 0 } else if i <= 2 || i >= len - 2 { 1 } else { 2 };
        s.fill((x + i, y, z - half), (x + i, y, z + half), |_, _, _| planks);
        for dy in 1..=3 {
            s.set(x + i, y + dy, z - half, if r() % 7 == 0 { inside } else { planks });
            s.set(x + i, y + dy, z + half, if r() % 7 == 0 { inside } else { planks });
            if half >= 1 {
                s.fill((x + i, y + dy, z - half + 1), (x + i, y + dy, z + half - 1), |_, _, _| inside);
            }
        }
        // Deck, with gaps.
        s.fill((x + i, y + 4, z - half), (x + i, y + 4, z + half), |_, _, _| if r() % 5 == 0 { inside } else { planks });
    }
    // Broken mast.
    for dy in 5..5 + 3 + (r() % 5) as i32 {
        s.set(x + 10, y + dy, z, log);
    }
    s.chest(BlockPos::new(x + 2, y + 1, z), "east", Loot::ShipwreckSupply);
    s.chest(BlockPos::new(x + 8, y + 1, z + 1), "north", Loot::ShipwreckSupply);
    if !broken {
        s.chest(BlockPos::new(x + len - 2, y + 1, z), "west", Loot::ShipwreckTreasure);
    }
    s
}

// ---------- ocean ruins ----------

/// Ruins on the sea floor: broken stone-brick walls (sandstone in warm oceans) around a
/// chest. Drowned gather around them.
pub fn ocean_ruin(x: i32, z: i32, floor: i32, warm: bool, seed: u64) -> Structure {
    let mut s = Structure::of("ocean_ruin");
    let mut r = rng(seed ^ 0x0c3a);
    let mats: Vec<u16> = if warm {
        vec![st("sandstone"), st("cut_sandstone"), st("chiseled_sandstone"), st("smooth_sandstone")]
    } else {
        vec![st("stone_bricks"), st("mossy_stone_bricks"), st("cracked_stone_bricks"), st("chiseled_stone_bricks")]
    };
    let size = 3 + (r() % 3) as i32;
    let y = floor;
    s.fill((x - size, y, z - size), (x + size, y, z + size), |_, _, _| one(r(), &mats));
    for bx in x - size..=x + size {
        for bz in z - size..=z + size {
            if (bx - x).abs() == size || (bz - z).abs() == size {
                let h = (r() % 4) as i32;
                for dy in 1..=h {
                    s.set(bx, y + dy, bz, one(r(), &mats));
                }
            }
        }
    }
    s.chest(BlockPos::new(x, y + 1, z), "north", Loot::OceanRuin);
    for _ in 0..2 {
        s.mobs.push(([x as f64 + 0.5 + (r() % 5) as f64 - 2.0, y as f64 + 1.0, z as f64 + 0.5], "drowned"));
    }
    s
}

// ---------- ruined portal ----------

/// A ruined Nether portal: an obsidian frame with blocks missing and some crying obsidian,
/// netherrack and magma around it, and a chest.
pub fn ruined_portal(x: i32, z: i32, g: i32, seed: u64) -> Structure {
    let mut s = Structure::of("ruined_portal");
    let mut r = rng(seed ^ 0x9071);
    let (obs, crying) = (st("obsidian"), st("crying_obsidian"));
    let ground = [st("netherrack"), st("netherrack"), st("magma_block"), st("blackstone")];
    for dx in -4..=5 {
        for dz in -3..=3 {
            if r() % 3 != 0 && dx * dx + dz * dz <= 18 {
                s.set(x + dx, g, z + dz, one(r(), &ground));
            }
        }
    }
    for dx in 0..4 {
        for dy in 0..5 {
            if (dx == 0 || dx == 3 || dy == 0 || dy == 4) && r() % 4 != 0 {
                s.set(x + dx, g + 1 + dy, z, if r() % 5 == 0 { crying } else { obs });
            }
        }
    }
    if r() % 3 == 0 {
        s.set(x + 1, g + 1, z + 2, st("gold_block"));
    }
    s.chest(BlockPos::new(x - 2, g + 1, z + 1), "east", Loot::RuinedPortal);
    s
}

// ---------- ocean monument ----------

/// An ocean monument on the deep sea floor: a big hollow prismarine building full of water,
/// with sea lanterns, two wings, and in its heart a chamber with eight gold blocks. Three
/// elder guardians live in it; guardians keep spawning inside.
pub fn monument(x: i32, z: i32, floor: i32, sea: i32, seed: u64) -> Structure {
    let mut s = Structure::of("monument");
    let mut r = rng(seed ^ 0x3013);
    let (pri, bricks, dark, lantern, water) = (st("prismarine"), st("prismarine_bricks"), st("dark_prismarine"), st("sea_lantern"), st("water"));
    let top = (sea - 3).min(floor + 22);
    let w = 20;
    // Base platform.
    s.fill((x - w - 2, floor, z - w - 2), (x + w + 2, floor, z + w + 2), |_, _, _| pri);
    // The main hall: walls of bricks with dark prismarine bands and lanterns, water inside.
    s.fill((x - w, floor + 1, z - w), (x + w, top, z + w), |bx, by, bz| {
        let edge = (bx - x).abs() == w || (bz - z).abs() == w;
        let roof = by == top;
        if edge || roof {
            if (by - floor) % 6 == 0 { dark } else if (bx + bz + by) % 7 == 0 { lantern } else { bricks }
        } else if (by - floor) % 8 == 0 && ((bx - x).abs() > 4 || (bz - z).abs() > 4) {
            // Floors with openings around the core.
            if (bx - x) % 6 == 0 || (bz - z) % 6 == 0 { water } else { pri }
        } else {
            water
        }
    });
    // Entrance (south), and two wings.
    s.fill((x - 2, floor + 1, z + w), (x + 2, floor + 5, z + w), |_, _, _| water);
    for side in [-1, 1] {
        let wx = x + side * (w + 6);
        s.fill((wx - 5, floor + 1, z - 6), (wx + 5, floor + 10, z + 6), |bx, by, bz| if (bx - wx).abs() == 5 || (bz - z).abs() == 6 || by == floor + 10 { bricks } else { water });
        s.fill((x + side * w, floor + 2, z - 2), (x + side * (w + 1), floor + 5, z + 2), |_, _, _| water);
        s.mobs.push(([wx as f64 + 0.5, floor as f64 + 4.0, z as f64 + 0.5], "elder_guardian"));
    }
    // The core: dark prismarine around eight gold blocks.
    s.fill((x - 3, floor + 3, z - 3), (x + 3, floor + 8, z + 3), |bx, by, bz| {
        if (bx - x).abs() <= 1 && (bz - z).abs() <= 1 && by >= floor + 5 && by <= floor + 6 && !(bx == x + 1 || bz == z + 1) {
            st("gold_block")
        } else if (bx - x).abs() == 3 || (bz - z).abs() == 3 || by == floor + 3 || by == floor + 8 {
            dark
        } else {
            water
        }
    });
    s.mobs.push(([x as f64 + 0.5, top as f64 - 3.0, z as f64 + 0.5], "elder_guardian"));
    for _ in 0..4 {
        s.mobs.push(([x as f64 + (r() % 30) as f64 - 15.0, floor as f64 + 4.0, z as f64 + (r() % 30) as f64 - 15.0], "guardian"));
    }
    s
}

// ---------- ancient city ----------

/// An ancient city deep in the dark: a great cavern floored with deepslate tiles, sculk
/// spreading over it, a reinforced deepslate gate in the middle, deepslate-brick houses with
/// chests, soul lanterns, and sculk shriekers that can summon the warden.
pub fn ancient_city(x: i32, z: i32, y: i32, seed: u64) -> Structure {
    let mut s = Structure::of("ancient_city");
    let mut r = rng(seed ^ 0xa1c1);
    let (tiles, bricks, polished, reinforced) = (st("deepslate_tiles"), st("deepslate_bricks"), st("polished_deepslate"), st("reinforced_deepslate"));
    let (sculk, lantern, wool, shrieker, sensor, catalyst) = (
        st("sculk"),
        st("soul_lantern"),
        st("gray_wool"),
        with("sculk_shrieker", &[("can_summon", "true")]),
        st("sculk_sensor"),
        st("sculk_catalyst"),
    );
    let (hw, hd, h) = (34, 24, 16);
    // The cavern.
    s.fill((x - hw, y - 1, z - hd), (x + hw, y + h, z + hd), |_, by, _| if by == y - 1 { tiles } else { 0 });
    // Sculk patches on the floor.
    for _ in 0..90 {
        let (px, pz) = (x - hw + (r() % (2 * hw as u64)) as i32, z - hd + (r() % (2 * hd as u64)) as i32);
        for dx in -1..=1 {
            for dz in -1..=1 {
                if r() % 3 != 0 {
                    s.set(px + dx, y - 1, pz + dz, sculk);
                }
            }
        }
    }
    // The gate.
    s.fill((x - 7, y, z - 1), (x + 7, y + 10, z + 1), |bx, by, _| if (bx - x).abs() == 7 || by >= y + 9 || (by >= y + 8 && (bx - x).abs() >= 5) { reinforced } else { 0 });
    s.fill((x - 8, y - 1, z - 2), (x + 8, y - 1, z + 2), |_, _, _| polished);
    // Houses.
    for i in 0..6 {
        let hx = x - 26 + (i % 3) * 26;
        let hz = if i < 3 { z - 14 } else { z + 14 };
        s.fill((hx - 3, y, hz - 3), (hx + 3, y + 4, hz + 3), |bx, by, bz| if (bx - hx).abs() == 3 || (bz - hz).abs() == 3 || by == y + 4 { bricks } else if by == y { wool } else { 0 });
        let door = if i < 3 { hz + 3 } else { hz - 3 };
        s.fill((hx, y + 1, door), (hx, y + 2, door), |_, _, _| 0);
        s.chest(BlockPos::new(hx - 2, y + 1, if i < 3 { hz - 2 } else { hz + 2 }), if i < 3 { "south" } else { "north" }, Loot::AncientCity);
        s.set(hx, y + 3, hz, lantern);
        s.set(hx + 2, y + 1, door - if i < 3 { 1 } else { -1 }, shrieker);
    }
    // Sensors, catalysts and more shriekers around the plaza; soul lanterns on posts.
    for k in 0..10 {
        let (px, pz) = (x - 20 + (r() % 40) as i32, z - 8 + (r() % 16) as i32);
        s.set(px, y, pz, if k < 4 { shrieker } else if k < 8 { sensor } else { catalyst });
    }
    for (px, pz) in [(x - 12, z - 6), (x + 12, z - 6), (x - 12, z + 6), (x + 12, z + 6)] {
        for dy in 0..3 {
            s.set(px, y + dy, pz, polished);
        }
        s.set(px, y + 3, pz, lantern);
    }
    s
}

// ---------- bastion remnant ----------

/// A bastion remnant in the Nether: a keep of blackstone and polished blackstone bricks
/// with ramparts, three floors joined by stairs, gilded blackstone here and there, and a
/// treasure room with gold blocks, the treasure chest and a magma cube spawner. Piglins,
/// piglin brutes and hoglins live in it.
pub fn bastion(x: i32, z: i32, y: i32, seed: u64) -> Structure {
    let mut s = Structure::of("bastion");
    let mut r = rng(seed ^ 0xba57);
    let (bricks, cracked, black, gilded, basalt, chain) =
        (st("polished_blackstone_bricks"), st("cracked_polished_blackstone_bricks"), st("blackstone"), st("gilded_blackstone"), with("basalt", &[("axis", "y")]), with("chain", &[("axis", "y")]));
    let w = 13;
    let top = y + 18;
    // Clear the space, then the keep.
    s.fill((x - w - 3, y - 4, z - w - 3), (x + w + 3, top + 4, z + w + 3), |_, _, _| 0);
    s.fill((x - w, y - 4, z - w), (x + w, top, z + w), |bx, by, bz| {
        let edge = (bx - x).abs() >= w - 1 || (bz - z).abs() >= w - 1;
        let floor = by == y - 4 || by == y + 2 || by == y + 8 || by == y + 14;
        if edge || floor {
            match r() % 20 {
                0 => gilded,
                1..=3 => cracked,
                4..=6 => black,
                _ => bricks,
            }
        } else {
            0
        }
    });
    // Ramparts.
    s.fill((x - w, top + 1, z - w), (x + w, top + 1, z + w), |bx, _, bz| if ((bx - x).abs() == w || (bz - z).abs() == w) && (bx + bz) % 2 == 0 { bricks } else { 0 });
    // Doorways on every side of the ground floor, and openings between floors.
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        for k in -1..=1 {
            for dy in 0..3 {
                for t in 0..2 {
                    let wall = w - t;
                    s.set(x + dx * wall + dz * k, y - 3 + dy, z + dz * wall + dx * k, 0);
                }
            }
        }
    }
    for fy in [y + 2, y + 8, y + 14] {
        s.fill((x + 6, fy, z + 6), (x + 8, fy, z + 8), |_, _, _| 0);
        for k in 0..6 {
            s.set(x + 6 - k.min(5), fy - 5 + k, z + 7, with("polished_blackstone_brick_stairs", &[("facing", "west")]));
        }
    }
    // Basalt pillars and hanging chains.
    for (px, pz) in [(x - 6, z - 6), (x + 6, z - 6), (x - 6, z + 6)] {
        for by in y - 3..y + 2 {
            s.set(px, by, pz, basalt);
        }
        s.set(px, y + 7, pz, chain);
    }
    // Treasure room on the ground floor: gold, the treasure chest, a magma cube spawner, lava.
    s.fill((x - 3, y - 3, z - 3), (x + 3, y - 3, z + 3), |bx, _, bz| if (bx - x).abs() == 3 || (bz - z).abs() == 3 { st("lava") } else { 0 });
    s.fill((x - 1, y - 3, z - 1), (x + 1, y - 2, z + 1), |_, _, _| st("gold_block"));
    s.chest(BlockPos::new(x, y - 1, z), "south", Loot::BastionTreasure);
    s.spawner(BlockPos::new(x + 5, y - 3, z - 5), "magma_cube");
    for (cx, cz) in [(x - 10, z - 10), (x + 10, z + 10)] {
        s.chest(BlockPos::new(cx, y + 3, cz), "south", Loot::Bastion);
    }
    s.chest(BlockPos::new(x - 10, y + 9, z + 10), "north", Loot::Bastion);
    for k in 0..5 {
        s.mobs.push(([(x - 8 + k * 4) as f64 + 0.5, y as f64 - 3.0, (z - 9) as f64 + 0.5], "piglin"));
    }
    for k in 0..3 {
        s.mobs.push(([(x - 4 + k * 4) as f64 + 0.5, y as f64 + 3.0, (z + 9) as f64 + 0.5], "piglin_brute"));
    }
    s.mobs.push(([x as f64 + 9.5, y as f64 + 9.0, z as f64 - 9.5], "hoglin"));
    s.mobs.push(([x as f64 - 9.5, y as f64 + 9.0, z as f64 - 9.5], "hoglin"));
    s
}

// ---------- Nether ruined portal ----------

/// A ruined portal in the Nether: the same broken frame on a platform of netherrack,
/// blackstone and magma, with room cleared around it (it sits in a cave at height `y`).
pub fn ruined_portal_nether(x: i32, z: i32, y: i32, seed: u64) -> Structure {
    let mut s = Structure::of("ruined_portal");
    // Room above, solid ground under the platform (so it does not hang over the lava sea).
    s.fill((x - 5, y + 1, z - 4), (x + 6, y + 7, z + 4), |_, _, _| 0);
    s.fill((x - 4, y - 3, z - 3), (x + 5, y - 1, z + 3), |_, _, _| st("netherrack"));
    s.fill((x - 4, y, z - 3), (x + 5, y, z + 3), |_, _, _| st("netherrack"));
    let portal = ruined_portal(x, z, y, seed);
    s.blocks.extend(portal.blocks);
    s.chests.extend(portal.chests);
    s
}

// ---------- igloo ----------

/// An igloo on snowy ground: a dome of snow blocks with ice windows, a bed, a furnace and a
/// crafting table, and under a carpet a trapdoor and ladder down to a basement where a
/// villager and a zombie villager are locked up, with a brewing stand, a chest and a
/// cauldron.
pub fn igloo(x: i32, z: i32, g: i32, seed: u64) -> Structure {
    let mut s = Structure::of("igloo");
    let mut r = rng(seed ^ 0x1910);
    let (snow, ice) = (st("snow_block"), st("ice"));
    // The dome: an ellipsoid shell 4 high.
    s.fill((x - 4, g + 1, z - 4), (x + 4, g + 4, z + 4), |bx, by, bz| {
        let (dx, dy, dz) = ((bx - x) as f64, (by - g - 1) as f64, (bz - z) as f64);
        let d = (dx * dx) / 16.0 + (dy * dy) / 12.0 + (dz * dz) / 16.0;
        if d <= 0.75 {
            0
        } else if d <= 1.25 {
            snow
        } else {
            0
        }
    });
    // Floor, door (south) and windows.
    s.fill((x - 3, g, z - 3), (x + 3, g, z + 3), |_, _, _| snow);
    s.fill((x, g + 1, z + 3), (x, g + 2, z + 4), |_, _, _| 0);
    s.set(x - 3, g + 2, z, ice);
    s.set(x + 3, g + 2, z, ice);
    // Furniture.
    s.set(x - 2, g + 1, z - 2, with("red_bed", &[("facing", "north"), ("part", "head")]));
    s.set(x - 2, g + 1, z - 1, with("red_bed", &[("facing", "north"), ("part", "foot")]));
    s.set(x + 2, g + 1, z - 2, with("furnace", &[("facing", "south")]));
    s.set(x + 2, g + 1, z - 1, st("crafting_table"));
    s.set(x + 2, g + 1, z + 1, st("redstone_torch"));
    // The way down, hidden under a carpet... in half of them.
    if r() % 2 == 0 {
        s.set(x, g + 1, z, st("white_carpet"));
        s.set(x, g, z, with("oak_trapdoor", &[("half", "top"), ("facing", "north")]));
        let deep = g - 9;
        for y in deep + 1..g {
            s.set(x, y, z, with("ladder", &[("facing", "south")]));
            s.set(x, y, z - 1, st("stone_bricks"));
        }
        // The basement: stone bricks, two cells with iron bars.
        s.fill((x - 4, deep - 1, z - 1), (x + 4, deep + 3, z + 8), |bx, by, bz| {
            if bx == x - 4 || bx == x + 4 || by == deep - 1 || by == deep + 3 || bz == z - 1 || bz == z + 8 {
                st("stone_bricks")
            } else {
                0
            }
        });
        s.fill((x - 3, deep, z + 5), (x + 3, deep + 2, z + 5), |bx, _, _| if bx == x { st("stone_bricks") } else { st("iron_bars") });
        s.fill((x, deep, z + 6), (x, deep + 2, z + 7), |_, _, _| st("stone_bricks"));
        s.set(x - 3, deep, z + 1, st("brewing_stand"));
        s.set(x + 3, deep, z + 1, st("cauldron"));
        s.chest(BlockPos::new(x + 3, deep, z + 3), "west", Loot::Igloo);
        s.set(x - 1, deep + 2, z + 2, st("lantern"));
        s.mobs.push(([x as f64 - 2.0 + 0.5, deep as f64, z as f64 + 6.5], "villager"));
        s.mobs.push(([x as f64 + 2.0 + 0.5, deep as f64, z as f64 + 6.5], "zombie_villager"));
    }
    s
}

// ---------- swamp hut ----------

/// A witch's hut on stilts over the swamp: spruce planks, oak log stilts, a flower pot with a
/// red mushroom, a crafting table and a cauldron. A witch and her black cat live in it, and
/// witches keep spawning inside it.
pub fn swamp_hut(x: i32, z: i32, g: i32) -> Structure {
    let mut s = Structure::of("swamp_hut");
    let (planks, log) = (st("spruce_planks"), with("oak_log", &[("axis", "y")]));
    let floor = g.max(62) + 2;
    // Stilts down to the ground.
    for (dx, dz) in [(0, 0), (4, 0), (0, 6), (4, 6)] {
        for y in g - 2..floor {
            s.set(x + dx, y, z + dz, log);
        }
    }
    // Floor, walls, roof.
    s.fill((x, floor, z), (x + 4, floor, z + 6), |_, _, _| planks);
    s.fill((x, floor + 1, z + 1), (x + 4, floor + 3, z + 6), |bx, _, bz| if bx == x || bx == x + 4 || bz == z + 1 || bz == z + 6 { planks } else { 0 });
    for (dx, dz) in [(0, 1), (4, 1), (0, 6), (4, 6)] {
        s.fill((x + dx, floor + 1, z + dz), (x + dx, floor + 3, z + dz), |_, _, _| log);
    }
    s.fill((x - 1, floor + 4, z - 1), (x + 5, floor + 4, z + 7), |_, _, _| st("spruce_slab"));
    // Door, windows and porch.
    s.fill((x + 2, floor + 1, z + 1), (x + 2, floor + 2, z + 1), |_, _, _| 0);
    s.set(x, floor + 2, z + 3, st("oak_fence"));
    s.set(x + 4, floor + 2, z + 3, st("oak_fence"));
    s.set(x, floor + 1, z - 0, st("oak_fence"));
    s.set(x + 4, floor + 1, z - 0, st("oak_fence"));
    // Inside.
    s.set(x + 1, floor + 1, z + 5, st("crafting_table"));
    s.set(x + 3, floor + 1, z + 5, st("cauldron"));
    s.set(x + 1, floor + 2, z + 2, st("potted_red_mushroom"));
    s.mobs.push(([x as f64 + 2.5, floor as f64 + 1.0, z as f64 + 3.5], "witch"));
    s.mobs.push(([x as f64 + 2.5, floor as f64 + 1.0, z as f64 + 4.5], "cat:all_black"));
    s
}

// ---------- woodland mansion ----------

/// A woodland mansion in the dark forest: a big two-storey house of dark oak with cobblestone
/// foundations, birch floors and glass windows, halls, a dining room, bedrooms with chests,
/// and jail cells upstairs with allays locked in them. Vindicators and evokers guard it.
pub fn mansion(x: i32, z: i32, g: i32, seed: u64) -> Structure {
    let mut s = Structure::of("mansion");
    let mut r = rng(seed ^ 0x3a45);
    let (cobble, planks, log, birch, glass, carpet) =
        (st("cobblestone"), st("dark_oak_planks"), with("dark_oak_log", &[("axis", "y")]), st("birch_planks"), st("glass_pane"), st("red_carpet"));
    let (w, d) = (18, 14);
    let floors = [g + 1, g + 8];
    // Foundations down to the ground.
    s.fill((x - w, g - 4, z - d), (x + w, g, z + d), |_, _, _| cobble);
    for (i, &f) in floors.iter().enumerate() {
        let top = f + 6;
        s.fill((x - w, f, z - d), (x + w, top, z + d), |bx, by, bz| {
            let edge = bx == x - w || bx == x + w || bz == z - d || bz == z + d;
            let corner = (bx == x - w || bx == x + w) && (bz == z - d || bz == z + d);
            let post = edge && ((bx - x) % 6 == 0 || (bz - z) % 7 == 0);
            if by == f {
                if edge { cobble } else { birch }
            } else if by == top {
                planks
            } else if corner || post {
                log
            } else if edge {
                if by == f + 2 || by == f + 3 { glass } else { planks }
            } else if (bx - x) % 9 == 0 || bz == z {
                // Inner walls: rooms on both sides of a long hall.
                if (bz - z).abs() <= 1 || (bx - x).abs() % 9 == 0 && (bz - z).abs() == 4 && by <= f + 2 {
                    0
                } else {
                    planks
                }
            } else {
                0
            }
        });
        // The long hall through the middle, carpeted.
        s.fill((x - w + 1, f + 1, z - 1), (x + w - 1, f + 3, z + 1), |_, by, _| if by == f + 1 { carpet } else { 0 });
        // Chests in the rooms.
        for k in 0..3 {
            let cx = x - w + 4 + k * 12;
            s.chest(BlockPos::new(cx, f + 1, z - d + 1), "south", Loot::Mansion);
            if r() % 2 == 0 {
                s.chest(BlockPos::new(cx, f + 1, z + d - 1), "north", Loot::Mansion);
            }
        }
        // Lights.
        for k in -2..=2 {
            s.set(x + k * 7, f + 5, z, st("lantern"));
        }
        if i == 0 {
            // Front door (south) and a dining table.
            s.fill((x - 1, f + 1, z + d), (x + 1, f + 3, z + d), |_, _, _| 0);
            s.fill((x - 3, f + 1, z - 6), (x + 3, f + 1, z - 6), |_, _, _| st("dark_oak_fence"));
            s.fill((x - 3, f + 2, z - 6), (x + 3, f + 2, z - 6), |_, _, _| st("white_carpet"));
        } else {
            // Jail cells along the back with allays inside.
            for k in 0..3 {
                let cx = x - 12 + k * 12;
                s.fill((cx - 1, f + 1, z + d - 4), (cx + 1, f + 3, z + d - 1), |bx, _, bz| if bz == z + d - 4 || bx == cx - 1 || bx == cx + 1 { st("iron_bars") } else { 0 });
                s.mobs.push(([cx as f64 + 0.5, f as f64 + 1.5, (z + d - 2) as f64 + 0.5], "allay"));
            }
        }
    }
    // Stairs between the floors (west end).
    for k in 0..7 {
        s.set(x - w + 2 + k, g + 2 + k, z - 3, with("dark_oak_stairs", &[("facing", "east")]));
        s.fill((x - w + 2 + k, g + 3 + k, z - 3), (x - w + 2 + k, g + 5 + k, z - 3), |_, _, _| 0);
        s.set(x - w + 2 + k, floors[1], z - 3, 0);
    }
    // A cobblestone roof edge.
    s.fill((x - w - 1, g + 15, z - d - 1), (x + w + 1, g + 15, z + d + 1), |_, _, _| st("dark_oak_slab"));
    // Its guards.
    for k in 0..4 {
        let px = x - w + 4 + k * 10;
        s.mobs.push(([px as f64 + 0.5, floors[k as usize % 2] as f64 + 1.0, z as f64 + 0.5], "vindicator"));
    }
    s.mobs.push(([x as f64 + 0.5, floors[1] as f64 + 1.0, z as f64 + 0.5], "evoker"));
    s.mobs.push(([x as f64 - 8.5, floors[0] as f64 + 1.0, z as f64 + 0.5], "evoker"));
    s
}

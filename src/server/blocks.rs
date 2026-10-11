//! Breaking, placing and interacting with blocks (equivalent to digging.js / placeBlock.js).
//!
//! In survival the server validates reach and break time, drops the loot
//! and removes placed items from the inventory.

use super::anticheat::{max_reach2, player_box, reach2};
use super::{entities, Player, Server};
use crate::data::{self, Block};
use crate::gameplay;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::Dimension;
use crate::world::physics::Aabb;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const REPLACEABLE: &[&str] = &[
    "air", "cave_air", "void_air", "water", "lava", "short_grass", "tall_grass", "fern", "large_fern", "dead_bush",
    "seagrass", "tall_seagrass", "snow", "vine", "glow_lichen", "fire", "soul_fire", "structure_void", "light",
];
const DIRECTIONS: [&str; 4] = ["south", "west", "north", "east"];

fn replaceable(b: &Block) -> bool {
    REPLACEABLE.contains(&b.name.as_str())
}

fn in_world(dim: Dimension, p: BlockPos) -> bool {
    dim.in_bounds(p.y)
}

pub fn block_change(pos: BlockPos, state: u16) -> Vec<u8> {
    let mut p = packet(id::play::BLOCK_CHANGE);
    p.position(pos).varint(state as i32);
    p
}

pub fn ack(player: &Player, sequence: i32) {
    let mut p = packet(id::play::ACK_DIGGING);
    p.varint(sequence);
    player.send(p);
}

fn set(server: &Server, dim: Dimension, pos: BlockPos, state: u16) -> u16 {
    let old = server.world(dim).lock().unwrap().set_block(pos, state);
    if old != state {
        server.broadcast_dim(dim, &block_change(pos, state), None);
        super::redstone::changed(server, dim, pos);
        if data::get().block_of_state(state).name == "fire" {
            super::fire::schedule(server, dim, pos);
        }
    }
    old
}

/// Hitting the face of a block with fire on top extinguishes it (like Level.extinguishFire).
pub fn extinguish(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, face: i32) -> bool {
    let dim = player.dim();
    let fire = pos.offset(face);
    let name = data::get().block_of_state(get(server, dim, fire)).name.clone();
    if name != "fire" && name != "soul_fire" {
        return false;
    }
    if reach2(&player.state.lock().unwrap(), fire) > 8.0 * 8.0 {
        return false;
    }
    set(server, dim, fire, 0);
    let mut ev = packet(id::play::WORLD_EVENT);
    ev.i32(1009).position(fire).i32(0).bool(false);
    server.broadcast_dim(dim, &ev, None);
    true
}

fn get(server: &Server, dim: Dimension, pos: BlockPos) -> u16 {
    server.world(dim).lock().unwrap().get_block(pos)
}

/// `block_dig` packet: dig, cancel, finish, drop items, release use and swap hands.
pub fn handle_dig(server: &Arc<Server>, player: &Arc<Player>, status: i32, pos: BlockPos) {
    let dim = player.dim();
    let age = server.age.load(Ordering::Relaxed);
    match status {
        0 | 2 => {
            let (game_mode, tool, too_far, digging) = {
                let mut st = player.state.lock().unwrap();
                let too_far = reach2(&st, pos) > max_reach2(&st) || st.dead;
                let tool = st.inv.held(0, st.held_slot).clone();
                let digging = if status == 0 { None } else { st.digging.take() };
                (st.game_mode, tool, too_far, digging)
            };
            let state = get(server, dim, pos);
            let reg = data::get();
            let block = reg.block_of_state(state);
            if too_far || game_mode >= 2 || reg.is_air(state) || !in_world(dim, pos) {
                player.send(block_change(pos, state));
                return;
            }
            if game_mode == 1 {
                if status == 0 {
                    break_block(server, player, pos, false);
                }
                return;
            }
            // Survival: the minimum possible is with the tool in hand, on the ground and out of water.
            let needed = gameplay::get().break_ticks(block, tool.as_ref(), true, false);
            let ok = match status {
                0 if needed == 0 => true,
                0 => {
                    player.state.lock().unwrap().digging = Some((pos, age));
                    return;
                }
                _ => matches!(digging, Some((p, start)) if p == pos && (age - start) as f64 + 2.0 >= needed as f64 * 0.75),
            };
            if ok {
                break_block(server, player, pos, true);
            } else {
                player.send(block_change(pos, state));
            }
        }
        1 => player.state.lock().unwrap().digging = None,
        // Drop the whole stack (3) or one item (4) from the hand.
        3 | 4 => {
            let item = {
                let mut st = player.state.lock().unwrap();
                let slot = super::inventory::HOTBAR + st.held_slot;
                let Some(s) = st.inv.slots[slot].clone() else { return };
                let n = if status == 3 { s.count } else { 1 };
                st.inv.slots[slot] = (s.count > n).then(|| s.with_count(s.count - n));
                st.sync_slot(player, slot);
                s.with_count(n)
            };
            entities::throw_from(server, player, item);
            server.broadcast_tracked(player.eid, &super::play::equipment(player), false);
        }
        5 => {
            player.state.lock().unwrap().eating = None;
            super::ranged::release(server, player);
        }
        // F: swap main hand and off hand.
        6 => {
            {
                let mut st = player.state.lock().unwrap();
                let main = super::inventory::HOTBAR + st.held_slot;
                st.inv.slots.swap(main, super::inventory::OFFHAND);
                st.sync_slot(player, main);
                st.sync_slot(player, super::inventory::OFFHAND);
            }
            server.broadcast_tracked(player.eid, &super::play::equipment(player), false);
        }
        _ => {}
    }
}

/// Breaks a block (and the other half of doors/tall plants), dropping loot if applicable.
fn break_block(server: &Arc<Server>, player: &Player, pos: BlockPos, drops: bool) {
    let dim = player.dim();
    let reg = data::get();
    let state = get(server, dim, pos);
    let block = reg.block_of_state(state);
    // Banner layers travel with the dropped item.
    let banner = match server.world(dim).lock().unwrap().block_entity(pos) {
        Some(crate::world::blockentity::BlockEntity::Banner(p)) if !p.is_empty() => Some(p.clone()),
        _ => None,
    };
    // Chests and furnaces drop their contents (also in creative, like vanilla).
    let contents = super::containers::on_break(server, dim, pos);
    set(server, dim, pos, 0);
    if block.name == "obsidian" || block.name == "nether_portal" {
        super::portals::break_portal_near(server, dim, pos);
    }
    // Extended piston: the head goes too (and breaking the head removes the piston).
    if let Some(facing) = block.get(state, "facing") {
        let (dx, dy, dz) = match facing {
            "down" => (0, -1, 0),
            "up" => (0, 1, 0),
            "north" => (0, 0, -1),
            "south" => (0, 0, 1),
            "west" => (-1, 0, 0),
            _ => (1, 0, 0),
        };
        if (block.name == "piston" || block.name == "sticky_piston") && block.get(state, "extended") == Some("true") {
            let head = BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz);
            if reg.block_of_state(get(server, dim, head)).name == "piston_head" {
                set(server, dim, head, 0);
            }
        }
        if block.name == "piston_head" {
            let base = BlockPos::new(pos.x - dx, pos.y - dy, pos.z - dz);
            let bs = get(server, dim, base);
            let bn = reg.block_of_state(bs).name.clone();
            if bn == "piston" || bn == "sticky_piston" {
                set(server, dim, base, 0);
                if drops {
                    if let Some(id) = reg.item_id(&bn) {
                        entities::drop_at_block(server, dim, base.x, base.y, base.z, ItemStack::new(id, 1));
                    }
                }
            }
        }
    }
    super::fluids::neighbor_changed(server, dim, pos);
    for item in contents {
        entities::drop_at_block(server, dim, pos.x, pos.y, pos.z, item);
    }
    // Particles and sound for the others (the breaker already generated them).
    let mut ev = packet(id::play::WORLD_EVENT);
    ev.i32(2001).position(pos).i32(state as i32).bool(false);
    server.broadcast_dim(dim, &ev, Some(player.eid));

    let mut broken = vec![(pos, state)];
    if let Some(half) = block.get(state, "half").filter(|h| *h == "upper" || *h == "lower") {
        let other = if half == "lower" { BlockPos::new(pos.x, pos.y + 1, pos.z) } else { BlockPos::new(pos.x, pos.y - 1, pos.z) };
        let os = get(server, dim, other);
        if reg.block_of_state(os).name == block.name {
            set(server, dim, other, 0);
            broken.push((other, os));
        }
    }
    // Unsupported plants on top.
    let above = BlockPos::new(pos.x, pos.y + 1, pos.z);
    let above_state = get(server, dim, above);
    let ab = reg.block_of_state(above_state);
    if ab.hardness == 0.0 && ab.filter_light == 0 && !reg.is_air(ab.default_state) && !ab.name.contains("torch") {
        set(server, dim, above, 0);
        broken.push((above, above_state));
        let top = BlockPos::new(pos.x, pos.y + 2, pos.z);
        if ab.get(get(server, dim, top), "half") == Some("upper") {
            set(server, dim, top, 0);
        }
    }

    if !drops {
        return;
    }
    let tool = {
        let mut st = player.state.lock().unwrap();
        st.exhaustion += 0.005;
        st.inv.held(0, st.held_slot).clone()
    };
    let g = gameplay::get();
    // Durability: 1 per block with hardness (swords, 2).
    if block.hardness > 0.0 {
        if let Some(t) = &tool {
            let amount = if t.name().ends_with("_sword") { 2 } else { 1 };
            super::health::wear_held(server, player, amount);
        }
    }
    let silk = tool.as_ref().is_some_and(|t| t.enchantment("silk_touch") > 0);
    for (p, s) in broken {
        let b = reg.block_of_state(s);
        // A plant that falls for lack of support always drops its loot.
        if p == pos && !g.can_harvest(b, tool.as_ref().map(|t| t.id)) {
            continue;
        }
        for (item, count) in g.drops(b, s, tool.as_ref(), &mut entities::random) {
            let mut stack = ItemStack::new(item, count);
            if let (Some(layers), true) = (&banner, stack.name().ends_with("_banner")) {
                stack.nbt = Some(Box::new(crate::protocol::Nbt::compound().with("BlockEntityTag", crate::world::blockentity::banner_nbt(layers))));
            }
            entities::drop_at_block(server, dim, p.x, p.y, p.z, stack);
        }
        if p == pos && !silk {
            let xp = g.block_xp(b, &mut entities::random);
            if xp > 0 {
                entities::spawn_xp(server, dim, [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5], xp);
            }
        }
    }
}

pub struct PlaceRequest {
    pub hand: i32,
    pub location: BlockPos,
    pub face: i32,
    pub cursor_y: f32,
}

/// Places the block of the held item or interacts with the clicked block.
pub fn place(server: &Arc<Server>, player: &Arc<Player>, req: PlaceRequest) {
    let dim = player.dim();
    let reg = data::get();
    let (item, yaw, pitch, sneaking, game_mode, too_far, held) = {
        let s = player.state.lock().unwrap();
        let held = if req.hand == 0 { super::inventory::HOTBAR + s.held_slot } else { super::inventory::OFFHAND };
        let too_far = reach2(&s, req.location) > max_reach2(&s) || s.dead;
        (s.inv.slots[held].clone(), s.yaw, s.pitch, s.sneaking, s.game_mode, too_far, held)
    };
    // If rejected, the client reverts its block prediction with the ack; the slot must be resent.
    let reject = |why: &str| {
        tracing::debug!("{}: colocación rechazada en {:?} ({why})", player.name, req.location);
        player.state.lock().unwrap().sync_slot(player, held)
    };
    if game_mode == 3 || too_far {
        reject("modo espectador o fuera de alcance");
        return;
    }
    let clicked_state = get(server, dim, req.location);
    let clicked = reg.block_of_state(clicked_state);

    // Interaction: signs, containers, tables, doors, trapdoors, fence gates and levers.
    if !sneaking || item.is_none() {
        if super::signs::interact(server, player, req.location, clicked, clicked_state, req.hand) {
            return;
        }
        if super::containers::open(server, player, req.location, clicked) {
            return;
        }
        if clicked.name.ends_with("_bed") {
            super::beds::use_bed(server, player, req.location, clicked, clicked_state);
            return;
        }
        if interact(server, dim, req.location, clicked, clicked_state) {
            return;
        }
    }
    let Some(stack) = item else { return };
    // Flint and steel on TNT: it ignites.
    if matches!(stack.name(), "flint_and_steel" | "fire_charge") && clicked.name == "tnt" {
        super::redstone::ignite_tnt(server, player, req.location);
        return;
    }
    // Flint and steel and fire charge: Nether portals or fire.
    if matches!(stack.name(), "flint_and_steel" | "fire_charge") && game_mode != 2 {
        let target = if replaceable(clicked) { req.location } else { req.location.offset(req.face) };
        if !super::portals::ignite(server, player, target, &stack, req.hand) {
            reject("no se puede encender ahí");
        }
        return;
    }
    // Eye of ender on an End portal frame.
    if stack.name() == "ender_eye" && clicked.name == "end_portal_frame" {
        super::portals::place_eye(server, player, req.location, req.hand);
        return;
    }
    // Buckets are used via `use_item` (like vanilla), not by placing against a block.
    if stack.name().ends_with("_bucket") {
        return;
    }
    // Minecart on a rail.
    if stack.name() == "minecart" {
        if !super::vehicles::place_minecart(server, player, req.location, req.hand) {
            reject("la vagoneta va sobre un raíl");
        }
        return;
    }
    // Spawn eggs.
    if let Some(mob) = stack.name().strip_suffix("_spawn_egg").and_then(super::mobs::MobKind::from_name) {
        let at = if replaceable(clicked) { req.location } else { req.location.offset(req.face) };
        super::mobs::spawn(server, dim, mob, [at.x as f64 + 0.5, at.y as f64, at.z as f64 + 0.5]);
        consume(player, game_mode, req.hand, &stack);
        return;
    }
    let Some(mut block) = reg.block_for_item(stack.id) else { return };
    if game_mode == 2 {
        reject("modo aventura");
        return;
    }

    // Slab on a slab of the same type -> double.
    if block.name == clicked.name && block.has("type") {
        let t = clicked.get(clicked_state, "type");
        if (t == Some("bottom") && req.face == 1) || (t == Some("top") && req.face == 0) {
            set(server, dim, req.location, block.with(clicked_state, "type", "double"));
            consume(player, game_mode, req.hand, &stack);
            return;
        }
    }

    let target = if replaceable(clicked) { req.location } else { req.location.offset(req.face) };
    if !in_world(dim, target) || !replaceable(reg.block_of_state(get(server, dim, target))) {
        reject("destino ocupado o fuera del mundo");
        return;
    }

    let look = DIRECTIONS[((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) as usize];
    let opposite = |d: &str| match d {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        "west" => "east",
        "up" => "down",
        _ => "up",
    };
    let face_dir = ["down", "up", "north", "south", "west", "east"].get(req.face as usize).copied().unwrap_or("up");
    let horizontal_face = (2..=5).contains(&req.face);
    let wall = if horizontal_face { reg.wall_variant(block) } else { None };
    if let Some(w) = wall {
        block = w;
    }

    let mut state = block.default_state;
    // Levers and buttons: floor, wall or ceiling.
    if block.has("face") {
        let face = match req.face {
            0 => "ceiling",
            1 => "floor",
            _ => "wall",
        };
        state = block.with(state, "face", face);
    }
    if block.has("axis") {
        state = block.with(state, "axis", ["y", "y", "z", "z", "x", "x"][req.face.clamp(0, 5) as usize]);
    }
    if block.has("facing") {
        let six = block.props.iter().any(|(n, v)| n == "facing" && v.iter().any(|x| x == "up"));
        let facing = if wall.is_some() || (block.name.contains("trapdoor") && horizontal_face) || (block.has("face") && horizontal_face) {
            face_dir
        } else if six {
            if pitch > 45.0 {
                "up"
            } else if pitch < -45.0 {
                "down"
            } else {
                opposite(look)
            }
        } else if ["stairs", "door", "fence_gate", "bed"].iter().any(|k| block.name.contains(k)) {
            look
        } else {
            opposite(look)
        };
        state = block.with(state, "facing", facing);
    }
    let top = req.face == 0 || (horizontal_face && req.cursor_y > 0.5);
    if block.has("half") {
        // Stairs/trapdoors use top|bottom; doors and tall plants upper|lower.
        let two_blocks = block.props.iter().any(|(n, v)| n == "half" && v.iter().any(|x| x == "lower"));
        let half = if two_blocks { "lower" } else if top { "top" } else { "bottom" };
        state = block.with(state, "half", half);
    }
    if block.has("type") {
        state = block.with(state, "type", if top { "top" } else { "bottom" });
    }
    if block.has("rotation") {
        let rot = (((yaw + 180.0) * 16.0 / 360.0 + 0.5).floor() as i32).rem_euclid(16);
        state = block.with(state, "rotation", &rot.to_string());
    }

    // Two-part blocks.
    let second = if block.get(state, "half") == Some("lower") {
        Some((BlockPos::new(target.x, target.y + 1, target.z), block.with(state, "half", "upper")))
    } else if block.get(state, "part") == Some("foot") {
        Some((target.offset(["", "", "north", "south", "west", "east"].iter().position(|d| *d == look).unwrap_or(3) as i32), block.with(state, "part", "head")))
    } else {
        None
    };
    if let Some((pos, _)) = second {
        if !in_world(dim, pos) || !replaceable(reg.block_of_state(get(server, dim, pos))) {
            reject("segunda mitad ocupada");
            return;
        }
    }
    // Do not place solid blocks inside a player.
    let blocked = |pos: BlockPos, state: u16| {
        let boxes: Vec<Aabb> = gameplay::get()
            .shape(state)
            .iter()
            .map(|s| Aabb { min: [pos.x as f64 + s[0], pos.y as f64 + s[1], pos.z as f64 + s[2]], max: [pos.x as f64 + s[3], pos.y as f64 + s[4], pos.z as f64 + s[5]] })
            .collect();
        !boxes.is_empty()
            && server.online().iter().any(|p| {
                let s = p.state.lock().unwrap();
                !s.dead && s.game_mode != 3 && boxes.iter().any(|b| b.intersects(&player_box(s.x, s.y, s.z, s.sneaking)))
            })
    };
    if blocked(target, state) || second.is_some_and(|(p, s)| blocked(p, s)) {
        reject("dentro de un jugador");
        return;
    }
    // Chests: join with a single chest next to it facing the same way (unless sneaking).
    let (state, joined) = if (block.name == "chest" || block.name == "trapped_chest") && !sneaking {
        super::containers::connect_chest(&mut server.world(dim).lock().unwrap(), target, state)
    } else {
        (state, None)
    };
    if let Some((p, s)) = joined {
        // If someone had the old half open as a single chest, close it for them.
        super::containers::close_viewers(server, dim, p, false);
        set(server, dim, p, s);
    }
    set(server, dim, target, state);
    if let Some(mut be) = crate::world::blockentity::BlockEntity::for_block(&block.name) {
        if let crate::world::blockentity::BlockEntity::Banner(layers) = &mut be {
            *layers = super::stations::banner_layers(&stack);
        }
        let shown = be.client_nbt();
        server.world(dim).lock().unwrap().set_block_entity(target, be);
        // Patterned banner: clients need its layers.
        if let (Some(n), Some(t)) = (shown, crate::world::blockentity::type_id(&block.name)) {
            let mut p = packet(id::play::TILE_ENTITY_DATA);
            p.position(target).varint(t);
            n.write_network(&mut p);
            server.broadcast_dim(dim, &p, None);
        }
    }
    if let Some((pos, s)) = second {
        set(server, dim, pos, s);
    }
    super::fluids::neighbor_changed(server, dim, target);
    consume(player, game_mode, req.hand, &stack);
    // Placing a sign opens its editor.
    if block.name.ends_with("_sign") {
        super::signs::open_editor(server, player, target, true);
    }
}

/// Consumes the placed item (buckets are emptied).
fn consume(player: &Player, game_mode: u8, hand: i32, stack: &ItemStack) {
    if game_mode == 1 {
        return;
    }
    let reg = data::get();
    let rem = stack.name().ends_with("_bucket").then(|| reg.item_id("bucket")).flatten().map(|id| ItemStack::new(id, 1));
    let mut st = player.state.lock().unwrap();
    let held = st.held_slot;
    let slot = st.inv.consume_held(hand, held, 1, rem);
    st.sync_slot(player, slot);
}

fn interact(server: &Arc<Server>, dim: Dimension, pos: BlockPos, block: &Block, state: u16) -> bool {
    if block.name.ends_with("_button") {
        super::redstone::press_button(server, dim, pos, block, state);
        return true;
    }
    // Repeater: changes the delay; note block: raises the pitch and plays.
    if block.name == "repeater" {
        let d: i32 = block.get(state, "delay").and_then(|d| d.parse().ok()).unwrap_or(1);
        set(server, dim, pos, block.with(state, "delay", &(d % 4 + 1).to_string()));
        return true;
    }
    if block.name == "note_block" {
        let n: i32 = block.get(state, "note").and_then(|d| d.parse().ok()).unwrap_or(0);
        let next = (n + 1) % 25;
        set(server, dim, pos, block.with(state, "note", &next.to_string()));
        let pitch = 2f32.powf((next - 12) as f32 / 12.0);
        super::sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], "block.note_block.harp", super::sound::BLOCK, 3.0, pitch);
        return true;
    }
    let toggle = if block.has("open") && !block.name.starts_with("iron_") {
        "open"
    } else if block.name == "lever" {
        "powered"
    } else {
        return false;
    };
    let new_value = if block.get(state, toggle) == Some("true") { "false" } else { "true" };
    set(server, dim, pos, block.with(state, toggle, new_value));
    if let Some(half) = block.get(state, "half").filter(|h| *h == "upper" || *h == "lower") {
        let other = BlockPos::new(pos.x, if half == "lower" { pos.y + 1 } else { pos.y - 1 }, pos.z);
        let os = get(server, dim, other);
        if data::get().block_of_state(os).name == block.name {
            set(server, dim, other, block.with(os, toggle, new_value));
        }
    }
    true
}

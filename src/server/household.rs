//! Blocks you use by hand around the house:
//! - Ender chests: one personal inventory per player, the same in every ender chest.
//! - Shulker boxes: chests that keep what they hold when broken (the item carries it, and
//!   placing it puts it back); a shulker box does not fit in another.
//! - Cakes, eaten a slice at a time; a candle on a whole cake makes a candle cake.
//! - Candles: up to four on a block, lit with flint and steel or a fire charge, put out by hand.
//! - Cauldrons: filled and emptied with buckets (water, lava, powder snow) and bottles; water
//!   washes the dye out of leather armor.
//! - Respawn anchors: charged with glowstone; in the Nether they set your respawn point (each
//!   respawn uses a charge); anywhere else they blow up.
//! - Copper: weathers with time (exposed, weathered, oxidized), honeycomb waxes it and an axe
//!   scrapes off the wax or a stage of rust.

use super::inventory::WindowKind;
use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use crate::world::blockentity::BlockEntity;
use crate::world::{Dimension, World};
use std::sync::Arc;

fn center(p: BlockPos) -> [f64; 3] {
    [p.x as f64 + 0.5, p.y as f64 + 0.5, p.z as f64 + 0.5]
}

fn set_block(server: &Server, dim: Dimension, pos: BlockPos, state: u16) {
    let old = server.world(dim).lock().unwrap().set_block(pos, state);
    if old != state {
        server.broadcast_dim(dim, &super::blocks::block_change(pos, state), None);
        super::redstone::changed(server, dim, pos);
    }
}

fn world_event(server: &Server, dim: Dimension, pos: BlockPos, event: i32, data: i32) {
    let mut p = packet(id::play::WORLD_EVENT);
    p.i32(event).position(pos).i32(data).bool(false);
    server.broadcast_dim(dim, &p, None);
}

fn item(name: &str) -> Option<ItemStack> {
    data::get().item_id(name).map(|i| ItemStack::new(i, 1))
}

/// Uses up one of the held item, giving `back` in exchange (in creative the item stays, and
/// `back` is added unless there is one already, like vanilla's buckets).
fn swap_held(player: &Player, hand: i32, back: Option<ItemStack>) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 {
        if let Some(b) = back.filter(|b| !st.inv.slots.iter().flatten().any(|s| s.stacks_with(b))) {
            st.inv.add(b);
        }
    } else {
        let h = st.held_slot;
        st.inv.consume_held(hand, h, 1, back);
    }
    let p = st.inv.window_items();
    player.send(p);
}

/// Right click on one of these blocks. Returns whether it was used.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &Slot, hand: i32) -> bool {
    let name = data::get().block_of_state(state).name.as_str();
    let held_name = held.as_ref().map_or("", |h| h.name());
    match name {
        "ender_chest" => open_ender(server, player, pos),
        n if n.ends_with("shulker_box") => open_shulker(server, player, pos, state),
        "cake" => cake(server, player, pos, state, held_name, hand),
        n if n.ends_with("candle_cake") => candle_cake(server, player, pos, state, held_name, hand),
        n if n.ends_with("candle") => candle(server, player, pos, state, held_name, hand),
        "cauldron" | "water_cauldron" | "lava_cauldron" | "powder_snow_cauldron" => cauldron(server, player, pos, state, held, hand),
        "respawn_anchor" => anchor(server, player, pos, state, held_name, hand),
        n if copper_parts(n).is_some() => copper(server, player, pos, state, held_name, hand),
        _ => false,
    }
}

// ---------- ender chests ----------

fn open_ender(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) -> bool {
    let dim = player.dim();
    {
        let mut st = player.state.lock().unwrap();
        if st.ender.len() != 27 {
            st.ender.resize(27, None);
        }
        let slots = st.ender.clone();
        for p in st.inv.open(WindowKind::Ender, pos, slots, "Cofre de ender") {
            player.send(p);
        }
    }
    sound::at(server, dim, center(pos), "block.ender_chest.open", sound::BLOCK, 0.5, 1.0);
    super::containers::update_lid(server, dim, pos);
    true
}

/// After a click: the ender chest's contents follow the window.
pub fn after_click(player: &Player) {
    let mut st = player.state.lock().unwrap();
    if let Some(slots) = st.inv.window.as_ref().filter(|w| w.kind == WindowKind::Ender).map(|w| w.slots.clone()) {
        st.ender = slots;
    }
}

// ---------- shulker boxes ----------

fn open_shulker(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let facing = reg.block_of_state(state).get(state, "facing").unwrap_or("up");
    let face = ["down", "up", "north", "south", "west", "east"].iter().position(|f| *f == facing).unwrap_or(1) as i32;
    let slots = {
        let mut world = server.world(dim).lock().unwrap();
        // The lid cannot open against a full block.
        let front = world.get_block(pos.offset(face));
        if crate::gameplay::get().shape(front) == [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]] {
            return true;
        }
        if !matches!(world.block_entity(pos), Some(BlockEntity::Container(_))) {
            world.set_block_entity(pos, BlockEntity::Container(vec![None; 27]));
        }
        match world.block_entity(pos) {
            Some(BlockEntity::Container(items)) => items.clone(),
            _ => return true,
        }
    };
    {
        let mut st = player.state.lock().unwrap();
        for p in st.inv.open(WindowKind::Shulker, pos, slots, "Caja de shulker") {
            player.send(p);
        }
    }
    sound::at(server, dim, center(pos), "block.shulker_box.open", sound::BLOCK, 0.5, 1.0);
    super::containers::update_lid(server, dim, pos);
    true
}

/// A shulker box being broken: its block entity becomes the item's tag. Returns the item
/// and whether it held anything.
pub fn take_shulker(server: &Server, dim: Dimension, pos: BlockPos, block: &str) -> Option<(ItemStack, bool)> {
    if !block.ends_with("shulker_box") {
        return None;
    }
    super::containers::close_viewers(server, dim, pos, true);
    let items = match server.world(dim).lock().unwrap().take_block_entity(pos) {
        Some(BlockEntity::Container(items)) => items,
        _ => vec![],
    };
    let mut stack = item(block)?;
    let full = items.iter().any(|s| s.is_some());
    if full {
        let list = items.iter().enumerate().filter_map(|(i, s)| s.as_ref().map(|it| crate::world::anvil::item_to_nbt(i, it))).collect();
        stack.nbt = Some(Box::new(Nbt::compound().with("BlockEntityTag", Nbt::compound().with("Items", Nbt::List(list)))));
    }
    Some((stack, full))
}

/// A shulker box placed from an item that carries contents: they go back in.
pub fn placed(server: &Server, dim: Dimension, pos: BlockPos, block: &str, stack: &ItemStack) {
    if !block.ends_with("shulker_box") {
        return;
    }
    let list = stack.nbt.as_ref().and_then(|n| n.get("BlockEntityTag")).and_then(|t| t.get("Items"));
    if list.is_some() {
        let items = crate::world::anvil::items_from_nbt(list, 27);
        server.world(dim).lock().unwrap().set_block_entity(pos, BlockEntity::Container(items));
    }
}

// ---------- cakes and candles ----------

fn level(state: u16, key: &str) -> i32 {
    let b = data::get().block_of_state(state);
    b.get(state, key).and_then(|v| v.parse().ok()).unwrap_or(0)
}

/// One slice: 2 hunger points, 0.4 saturation. Returns whether it could eat.
fn eat_slice(player: &Player) -> bool {
    {
        let mut st = player.state.lock().unwrap();
        if st.food >= 20 && st.game_mode != 1 {
            return false;
        }
        st.food = (st.food + 2).min(20);
        st.saturation = (st.saturation + 0.4).min(st.food as f32);
    }
    super::health::send_health(player);
    true
}

fn cake(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let bites = level(state, "bites");
    // A candle on a whole cake.
    if bites == 0 && (held == "candle" || held.ends_with("_candle")) {
        if let Some(b) = reg.block(&format!("{held}_cake")) {
            set_block(server, dim, pos, b.default_state);
            swap_held(player, hand, None);
            sound::at(server, dim, center(pos), "block.cake.add_candle", sound::BLOCK, 1.0, 1.0);
            return true;
        }
    }
    if !eat_slice(player) {
        return false;
    }
    sound::at(server, dim, center(pos), "entity.generic.eat", sound::PLAYER, 1.0, 1.0);
    let b = reg.block_of_state(state);
    set_block(server, dim, pos, if bites >= 6 { 0 } else { b.with(state, "bites", &(bites + 1).to_string()) });
    true
}

fn candle_cake(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let b = reg.block_of_state(state);
    let lit = b.get(state, "lit") == Some("true");
    if !lit && light(server, player, pos, held, hand) {
        set_block(server, dim, pos, b.with(state, "lit", "true"));
        return true;
    }
    if !eat_slice(player) {
        return false;
    }
    // The candle comes off, and the cake is one slice down.
    let candle = b.name.trim_end_matches("_cake").to_string();
    if let Some(c) = item(&candle) {
        entities::drop_at_block(server, dim, pos.x, pos.y, pos.z, c);
    }
    if let Some(cake) = reg.block("cake") {
        set_block(server, dim, pos, cake.with(cake.default_state, "bites", "1"));
    }
    sound::at(server, dim, center(pos), "entity.generic.eat", sound::PLAYER, 1.0, 1.0);
    true
}

/// Flint and steel or a fire charge in hand: it lights (and is used). Returns whether it did.
fn light(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    match held {
        "flint_and_steel" => {
            super::health::wear_held(server, player, 1);
            sound::at(server, dim, center(pos), "item.flintandsteel.use", sound::BLOCK, 1.0, 1.0);
            true
        }
        "fire_charge" => {
            swap_held(player, hand, None);
            sound::at(server, dim, center(pos), "item.firecharge.use", sound::BLOCK, 1.0, 1.0);
            true
        }
        _ => false,
    }
}

fn candle(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    let b = data::get().block_of_state(state);
    let lit = b.get(state, "lit") == Some("true");
    let wet = b.get(state, "waterlogged") == Some("true");
    let n = level(state, "candles");
    // Another candle of the same colour joins them.
    if held == b.name && n < 4 {
        set_block(server, dim, pos, b.with(state, "candles", &(n + 1).to_string()));
        swap_held(player, hand, None);
        sound::at(server, dim, center(pos), "block.candle.place", sound::BLOCK, 1.0, 1.0);
        return true;
    }
    if !lit && !wet && light(server, player, pos, held, hand) {
        set_block(server, dim, pos, b.with(state, "lit", "true"));
        return true;
    }
    if lit && held.is_empty() {
        set_block(server, dim, pos, b.with(state, "lit", "false"));
        sound::at(server, dim, center(pos), "block.candle.extinguish", sound::BLOCK, 1.0, 1.0);
        return true;
    }
    false
}

// ---------- cauldrons ----------

fn is_water_bottle(it: &ItemStack) -> bool {
    it.name() == "potion" && it.nbt.as_ref().and_then(|n| n.get_str("Potion")) == Some("minecraft:water")
}

fn cauldron(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &Slot, hand: i32) -> bool {
    let dim = player.dim();
    let reg = data::get();
    let Some(it) = held else { return false };
    let b = reg.block_of_state(state);
    let lvl = level(state, "level");
    let full = |name: &str| reg.block(name).map(|c| if c.has("level") { c.with(c.default_state, "level", "3") } else { c.default_state });
    let with_level = |name: &str, l: i32| reg.block(name).map(|c| c.with(c.default_state, "level", &l.to_string()));
    let empty = reg.state("cauldron");
    // (new block state, item given back, sound)
    let result: Option<(u16, Option<ItemStack>, &str)> = match (b.name.as_str(), it.name()) {
        (_, "water_bucket") => full("water_cauldron").map(|s| (s, item("bucket"), "item.bucket.empty")),
        (_, "lava_bucket") => full("lava_cauldron").map(|s| (s, item("bucket"), "item.bucket.empty_lava")),
        (_, "powder_snow_bucket") => full("powder_snow_cauldron").map(|s| (s, item("bucket"), "item.bucket.empty_powder_snow")),
        ("water_cauldron", "bucket") if lvl == 3 => Some((empty, item("water_bucket"), "item.bucket.fill")),
        ("lava_cauldron", "bucket") => Some((empty, item("lava_bucket"), "item.bucket.fill_lava")),
        ("powder_snow_cauldron", "bucket") if lvl == 3 => Some((empty, item("powder_snow_bucket"), "item.bucket.fill_powder_snow")),
        ("water_cauldron", "glass_bottle") => {
            let next = if lvl <= 1 { Some(empty) } else { with_level("water_cauldron", lvl - 1) };
            next.map(|s| (s, super::effects::potion_item("potion", "water"), "item.bottle.fill"))
        }
        ("cauldron", _) if is_water_bottle(it) => with_level("water_cauldron", 1).map(|s| (s, item("glass_bottle"), "item.bottle.empty")),
        ("water_cauldron", _) if is_water_bottle(it) && lvl < 3 => with_level("water_cauldron", lvl + 1).map(|s| (s, item("glass_bottle"), "item.bottle.empty")),
        // Washing dyed leather armor.
        ("water_cauldron", n) if n.starts_with("leather_") && it.nbt.as_ref().and_then(|t| t.get("display")).and_then(|d| d.get("color")).is_some() => {
            let mut clean = it.clone();
            if let Some(tag) = clean.nbt.as_mut() {
                if let Some(d) = tag.get_mut("display") {
                    d.remove("color");
                }
                if tag.get("display").is_some_and(Nbt::is_empty_compound) {
                    tag.remove("display");
                }
            }
            if clean.nbt.as_ref().is_some_and(|t| t.is_empty_compound()) {
                clean.nbt = None;
            }
            {
                let mut st = player.state.lock().unwrap();
                let slot = if hand == 1 { super::inventory::OFFHAND } else { super::inventory::HOTBAR + st.held_slot };
                st.inv.slots[slot] = Some(clean);
                st.sync_slot(player, slot);
            }
            let next = if lvl <= 1 { Some(empty) } else { with_level("water_cauldron", lvl - 1) };
            if let Some(s) = next {
                set_block(server, dim, pos, s);
            }
            world_event(server, dim, pos, 1047, 0);
            return true;
        }
        _ => None,
    };
    let Some((s, back, snd)) = result else { return false };
    set_block(server, dim, pos, s);
    swap_held(player, hand, back);
    sound::at(server, dim, center(pos), snd, sound::BLOCK, 1.0, 1.0);
    true
}

// ---------- respawn anchors ----------

fn anchor(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    let b = data::get().block_of_state(state);
    let charges = level(state, "charges");
    if held == "glowstone" && charges < 4 {
        set_block(server, dim, pos, b.with(state, "charges", &(charges + 1).to_string()));
        swap_held(player, hand, None);
        sound::at(server, dim, center(pos), "block.respawn_anchor.charge", sound::BLOCK, 1.0, 1.0);
        return true;
    }
    if charges == 0 {
        return false;
    }
    if dim != Dimension::Nether {
        // Outside the Nether it blows up, like a bed in the Nether.
        set_block(server, dim, pos, 0);
        super::explosion::explode_ex(server, dim, center(pos), 5.0, None, true);
        return true;
    }
    let new = {
        let mut st = player.state.lock().unwrap();
        let new = st.spawn_point != Some(pos) || st.spawn_dim != Dimension::Nether;
        st.spawn_point = Some(pos);
        st.spawn_dim = Dimension::Nether;
        new
    };
    if new {
        player.message("§ePunto de reaparición establecido");
        sound::at(server, dim, center(pos), "block.respawn_anchor.set_spawn", sound::BLOCK, 1.0, 1.0);
    }
    true
}

/// Where to respawn at the anchor (using up a charge), or `None` if it is gone, empty or
/// blocked in (then it is forgotten).
pub fn anchor_point(server: &Server, player: &Player) -> Option<[f64; 3]> {
    let pos = player.state.lock().unwrap().spawn_point?;
    let reg = data::get();
    let g = crate::gameplay::get();
    let spot = {
        let mut world = server.world(Dimension::Nether).lock().unwrap();
        let state = world.get_block(pos);
        let b = reg.block_of_state(state);
        let charges = level(state, "charges");
        let mut spot = None;
        if b.name == "respawn_anchor" && charges > 0 {
            'find: for dy in [0, 1, -1] {
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    let p = BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz);
                    let free = |q: BlockPos, w: &mut World| g.shape(w.get_block(q)).is_empty() && !reg.block_of_state(w.get_block(q)).name.contains("lava");
                    if free(p, &mut world) && free(BlockPos::new(p.x, p.y + 1, p.z), &mut world) && !g.shape(world.get_block(BlockPos::new(p.x, p.y - 1, p.z))).is_empty() {
                        spot = Some(([p.x as f64 + 0.5, p.y as f64, p.z as f64 + 0.5], b.with(state, "charges", &(charges - 1).to_string())));
                        break 'find;
                    }
                }
            }
        }
        spot
    };
    match spot {
        Some((at, less)) => {
            set_block(server, Dimension::Nether, pos, less);
            sound::at(server, Dimension::Nether, center(pos), "block.respawn_anchor.deplete", sound::BLOCK, 1.0, 1.0);
            Some(at)
        }
        None => {
            let mut st = player.state.lock().unwrap();
            st.spawn_point = None;
            st.spawn_dim = Dimension::Overworld;
            drop(st);
            player.message("§cTu ancla de reaparición no tenía cargas o estaba obstruida");
            None
        }
    }
}

// ---------- copper ----------

const STAGES: [&str; 4] = ["", "exposed_", "weathered_", "oxidized_"];

fn copper_base(b: &str) -> bool {
    matches!(b, "copper" | "cut_copper" | "cut_copper_stairs" | "cut_copper_slab" | "chiseled_copper" | "copper_door" | "copper_trapdoor" | "copper_grate" | "copper_bulb")
}

/// (waxed, stage 0-3, base name) of a copper block.
pub fn copper_parts(name: &str) -> Option<(bool, usize, &str)> {
    let (waxed, rest) = match name.strip_prefix("waxed_") {
        Some(r) => (true, r),
        None => (false, name),
    };
    if rest == "copper_block" {
        return Some((waxed, 0, "copper"));
    }
    for (i, p) in STAGES.iter().enumerate().skip(1) {
        if let Some(b) = rest.strip_prefix(p) {
            return copper_base(b).then_some((waxed, i, b));
        }
    }
    (copper_base(rest) && rest != "copper").then_some((waxed, 0, rest))
}

pub fn copper_name(waxed: bool, stage: usize, base: &str) -> String {
    let core = if base == "copper" && stage == 0 { "copper_block".to_string() } else { format!("{}{base}", STAGES[stage]) };
    if waxed { format!("waxed_{core}") } else { core }
}

/// The same block state (facing, half...) in another copper block.
fn as_copper(state: u16, name: &str) -> Option<u16> {
    let reg = data::get();
    let from = reg.block_of_state(state);
    let to = reg.block(name)?;
    let mut s = to.default_state;
    for (prop, _) in &from.props {
        if let Some(v) = from.get(state, prop) {
            s = to.with(s, prop, v);
        }
    }
    Some(s)
}

/// The other half of a copper door.
fn door_half(world: &mut World, pos: BlockPos, state: u16) -> Option<(BlockPos, u16)> {
    let b = data::get().block_of_state(state);
    let other = match b.get(state, "half")? {
        "lower" => BlockPos::new(pos.x, pos.y + 1, pos.z),
        "upper" => BlockPos::new(pos.x, pos.y - 1, pos.z),
        _ => return None,
    };
    let os = world.get_block(other);
    (data::get().block_of_state(os).name == b.name).then_some((other, os))
}

/// Turns the block (and the other half of a door) into another copper block.
fn recopper(world: &mut World, pos: BlockPos, state: u16, name: &str, changes: &mut Vec<(BlockPos, u16)>) {
    let half = door_half(world, pos, state);
    for (p, s) in std::iter::once((pos, state)).chain(half) {
        if let Some(new) = as_copper(s, name) {
            world.set_block(p, new);
            changes.push((p, new));
        }
    }
}

fn copper(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &str, hand: i32) -> bool {
    let dim = player.dim();
    let name = data::get().block_of_state(state).name.as_str();
    let Some((waxed, stage, base)) = copper_parts(name) else { return false };
    let (target, event, snd) = match held {
        "honeycomb" if !waxed => (copper_name(true, stage, base), 3003, "item.honeycomb.wax_on"),
        h if h.ends_with("_axe") && waxed => (copper_name(false, stage, base), 3004, "item.axe.wax_off"),
        h if h.ends_with("_axe") && stage > 0 => (copper_name(false, stage - 1, base), 3005, "item.axe.scrape"),
        _ => return false,
    };
    let mut changes = Vec::new();
    recopper(&mut server.world(dim).lock().unwrap(), pos, state, &target, &mut changes);
    for (p, s) in changes {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
    world_event(server, dim, pos, event, 0);
    sound::at(server, dim, center(pos), snd, sound::BLOCK, 1.0, 1.0);
    if held == "honeycomb" {
        swap_held(player, hand, None);
    } else {
        super::health::wear_held(server, player, 1);
    }
    true
}

/// Copper that can still weather (random ticks).
pub fn weathers(name: &str) -> bool {
    copper_parts(name).is_some_and(|(waxed, stage, _)| !waxed && stage < 3)
}

/// A random tick on weathering copper: now and then it goes one stage further.
pub fn weather(world: &mut World, pos: BlockPos, state: u16, roll: u32, changes: &mut Vec<(BlockPos, u16)>) {
    let b = data::get().block_of_state(state);
    // A door weathers from its lower half (both halves at once).
    if b.get(state, "half") == Some("upper") || roll % 18 != 0 {
        return;
    }
    let Some((_, stage, base)) = copper_parts(&b.name) else { return };
    recopper(world, pos, state, &copper_name(false, stage + 1, base), changes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copper_names_round_trip() {
        let reg = data::get();
        for n in ["copper_block", "exposed_copper", "weathered_cut_copper_stairs", "waxed_oxidized_cut_copper_slab", "waxed_copper_block", "cut_copper"] {
            let (w, s, b) = copper_parts(n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(copper_name(w, s, b), n);
            assert!(reg.block(n).is_some(), "{n}");
        }
        assert_eq!(copper_name(false, 1, "copper"), "exposed_copper");
        assert!(copper_parts("copper_ore").is_none());
        assert!(copper_parts("raw_copper_block").is_none());
        assert!(weathers("weathered_copper") && !weathers("oxidized_copper") && !weathers("waxed_copper_block"));
    }
}

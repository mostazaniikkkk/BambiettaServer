//! Chests, barrels and furnaces: opening, sharing contents between players, smelting and
//! animating the chest lid.

use super::inventory::WindowKind;
use super::{entities, experience, Player, Server};
use crate::data::{self, Block};
use crate::gameplay;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::blockentity::{BlockEntity, Brewing, Furnace};
use crate::world::Dimension;
use std::sync::Arc;

/// Smelting time of an item in a regular furnace (10 s).
const COOK_TIME: i32 = 200;

fn clockwise(d: &str) -> &'static str {
    match d {
        "north" => "east",
        "east" => "south",
        "south" => "west",
        _ => "north",
    }
}

fn counter_clockwise(d: &str) -> &'static str {
    match d {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        _ => "north",
    }
}

fn step(p: BlockPos, dir: &str) -> BlockPos {
    let (dx, dz) = match dir {
        "north" => (0, -1),
        "south" => (0, 1),
        "west" => (-1, 0),
        _ => (1, 0),
    };
    BlockPos::new(p.x + dx, p.y, p.z + dz)
}

fn is_chest(name: &str) -> bool {
    name == "chest" || name == "trapped_chest"
}

/// The other half of a double chest, if connected (like ChestBlock.getConnectedDirection).
pub fn chest_partner(world: &mut crate::world::World, pos: BlockPos) -> Option<BlockPos> {
    let reg = data::get();
    let state = world.get_block(pos);
    let b = reg.block_of_state(state);
    if !is_chest(&b.name) {
        return None;
    }
    let facing = b.get(state, "facing")?;
    let dir = match b.get(state, "type")? {
        "left" => clockwise(facing),
        "right" => counter_clockwise(facing),
        _ => return None,
    };
    let other = step(pos, dir);
    let os = world.get_block(other);
    let ob = reg.block_of_state(os);
    let expected = if b.get(state, "type") == Some("left") { "right" } else { "left" };
    (ob.name == b.name && ob.get(os, "facing") == Some(facing) && ob.get(os, "type") == Some(expected)).then_some(other)
}

/// When placing a chest: if another single chest next to it faces the same way, they join into a double chest.
/// Returns the new chest's state and, if they joined, the neighbor's position and state.
pub fn connect_chest(world: &mut crate::world::World, pos: BlockPos, state: u16) -> (u16, Option<(BlockPos, u16)>) {
    let reg = data::get();
    let b = reg.block_of_state(state);
    let Some(facing) = b.get(state, "facing") else { return (state, None) };
    for (dir, mine, theirs) in [(clockwise(facing), "left", "right"), (counter_clockwise(facing), "right", "left")] {
        let n = step(pos, dir);
        let ns = world.get_block(n);
        let nb = reg.block_of_state(ns);
        if nb.name == b.name && nb.get(ns, "type") == Some("single") && nb.get(ns, "facing") == Some(facing) {
            return (b.with(state, "type", mine), Some((n, nb.with(ns, "type", theirs))));
        }
    }
    (state, None)
}

/// Tries to open the clicked container. Returns `true` if it was one.
pub fn open(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, block: &Block) -> bool {
    let dim = player.dim();
    if is_chest(&block.name) {
        let partner = chest_partner(&mut server.world(dim).lock().unwrap(), pos);
        if let Some(other) = partner {
            open_double(server, player, pos, other, block);
            return true;
        }
    }
    let (kind, title) = match block.name.as_str() {
        "chest" | "trapped_chest" => (WindowKind::Chest(3), "Cofre"),
        "barrel" => (WindowKind::Chest(3), "Barril"),
        "furnace" => (WindowKind::Furnace, "Horno"),
        "brewing_stand" => (WindowKind::Brewing, "Soporte para pociones"),
        n if super::stations::open(server, player, pos, n) => return true,
        "enchanting_table" => {
            super::enchanting::open(server, player, pos);
            return true;
        }
        "crafting_table" => {
            let packets = player.state.lock().unwrap().inv.open_crafting_table(pos);
            for p in packets {
                player.send(p);
            }
            return true;
        }
        _ => return false,
    };
    // The contents live in the block; created if missing (old worlds, generated blocks).
    let (slots, furnace) = {
        let mut world = server.world(dim).lock().unwrap();
        if world.block_entity(pos).is_none() {
            if let Some(be) = BlockEntity::for_block(&block.name) {
                world.set_block_entity(pos, be);
            }
        }
        match world.block_entity(pos) {
            Some(BlockEntity::Container(items)) => (items.clone(), None),
            Some(BlockEntity::Furnace(f)) => (f.items.clone(), Some(Ticking::Furnace(f.clone()))),
            Some(BlockEntity::Brewing(b)) => (b.items.clone(), Some(Ticking::Brewing(b.clone()))),
            _ => return false,
        }
    };
    {
        let mut st = player.state.lock().unwrap();
        for p in st.inv.open(kind, pos, slots, title) {
            player.send(p);
        }
        if let Some(t) = furnace {
            for p in t.properties(&st.inv) {
                player.send(p);
            }
        }
    }
    if block.name.ends_with("chest") {
        update_lid(server, dim, pos);
    }
    true
}

/// Double chest: the «right» half goes first (top), like vanilla's CompoundContainer.
fn open_double(server: &Arc<Server>, player: &Arc<Player>, a: BlockPos, b: BlockPos, block: &Block) {
    let dim = player.dim();
    let (first, second, slots) = {
        let mut world = server.world(dim).lock().unwrap();
        let first_is_a = block.get(world.get_block(a), "type") == Some("right");
        let (first, second) = if first_is_a { (a, b) } else { (b, a) };
        let mut slots = Vec::with_capacity(54);
        for p in [first, second] {
            if world.block_entity(p).is_none() {
                world.set_block_entity(p, BlockEntity::Container(vec![None; 27]));
            }
            match world.block_entity(p) {
                Some(BlockEntity::Container(items)) => slots.extend(items.iter().cloned()),
                _ => slots.extend(std::iter::repeat_n(None, 27)),
            }
        }
        (first, second, slots)
    };
    {
        let mut st = player.state.lock().unwrap();
        let packets = st.inv.open(WindowKind::Chest(6), first, slots, "Cofre grande");
        if let Some(w) = st.inv.window.as_mut() {
            w.pos2 = Some(second);
        }
        for p in packets {
            player.send(p);
        }
    }
    update_lid(server, dim, first);
}

/// Players with the window at `pos` open (either half of a double chest).
fn viewers(server: &Server, dim: Dimension, pos: BlockPos) -> Vec<Arc<Player>> {
    server
        .online()
        .into_iter()
        .filter(|p| p.dim() == dim)
        .filter(|p| p.state.lock().unwrap().inv.window.as_ref().is_some_and(|w| w.kind.shared() && (w.pos == pos || w.pos2 == Some(pos))))
        .collect()
}

/// Lid animation: the parameter is the number of players looking inside.
pub fn update_lid(server: &Server, dim: Dimension, pos: BlockPos) {
    let n = viewers(server, dim, pos).len();
    let (state, partner) = {
        let mut world = server.world(dim).lock().unwrap();
        (world.get_block(pos), chest_partner(&mut world, pos))
    };
    let block = data::get().block_of_state(state);
    if !block.name.ends_with("chest") {
        return;
    }
    for at in std::iter::once(pos).chain(partner) {
        let mut p = packet(id::play::BLOCK_ACTION);
        p.position(at).u8(1).u8(n.min(255) as u8).varint(data::get().block_index(&block.name));
        server.broadcast_dim(dim, &p, None);
    }
}

/// After a click in a shared window: saves the contents to the block and updates
/// the others who have it open. If something smelted was taken out, grants its experience.
pub fn after_click(server: &Arc<Server>, player: &Arc<Player>, smelted_taken: i32) {
    let dim = player.dim();
    let Some((pos, pos2, slots)) = ({
        let st = player.state.lock().unwrap();
        st.inv.window.as_ref().filter(|w| w.kind.shared()).map(|w| (w.pos, w.pos2, w.slots.clone()))
    }) else {
        return;
    };
    let mut xp = 0.0;
    {
        let mut world = server.world(dim).lock().unwrap();
        if let Some(p2) = pos2 {
            if let Some(BlockEntity::Container(items)) = world.block_entity(p2) {
                *items = slots[27..].to_vec();
            }
        }
        match world.block_entity(pos) {
            Some(BlockEntity::Container(items)) => *items = slots[..items.len().min(slots.len())].to_vec(),
            Some(BlockEntity::Furnace(f)) => {
                f.items = slots.clone();
                if smelted_taken > 0 {
                    xp = std::mem::take(&mut f.xp);
                }
            }
            Some(BlockEntity::Brewing(b)) => b.items = slots.clone(),
            _ => {}
        }
    }
    if xp > 0.0 {
        // The fractional part is granted with that probability, like vanilla.
        let whole = xp.floor() as i32 + i32::from(entities::random() < xp.fract());
        if whole > 0 {
            let mut st = player.state.lock().unwrap();
            experience::add_points(player, &mut st, whole);
        }
    }
    for other in viewers(server, dim, pos) {
        if other.eid != player.eid {
            let p = other.state.lock().unwrap().inv.refresh_shared(pos, &slots);
            if let Some(p) = p {
                other.send(p);
            }
        }
    }
}

/// When breaking the block: closes open windows and returns the contents to drop.
/// The other half of a double chest becomes a single chest again.
pub fn on_break(server: &Server, dim: Dimension, pos: BlockPos) -> Vec<ItemStack> {
    let partner = {
        let mut world = server.world(dim).lock().unwrap();
        chest_partner(&mut world, pos).map(|p| {
            let s = world.get_block(p);
            let single = data::get().block_of_state(s).with(s, "type", "single");
            world.set_block(p, single);
            (p, single)
        })
    };
    if let Some((p, s)) = partner {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
    }
    close_viewers(server, dim, pos, true);
    server.world(dim).lock().unwrap().take_block_entity(pos).map(|be| be.contents()).unwrap_or_default()
}

/// Closes the window for whoever has the container at `pos` open. With `discard` the
/// copy of the contents is discarded (the block is gone); otherwise what is stored in the block is kept.
pub fn close_viewers(server: &Server, dim: Dimension, pos: BlockPos, discard: bool) {
    for p in viewers(server, dim, pos) {
        let mut st = p.state.lock().unwrap();
        let id = st.inv.window_id();
        if let Some(w) = st.inv.window.as_mut().filter(|_| discard) {
            w.slots.iter_mut().for_each(|s| *s = None);
        }
        let left = st.inv.close();
        let mut c = packet(id::play::CLOSE_WINDOW);
        c.u8(id);
        p.send(c);
        let inv = st.inv.window_items();
        p.send(inv);
        drop(st);
        for item in left {
            entities::throw_from(server, &p, item);
        }
    }
}

/// Closes any window open on `pos` (the block disappeared: broken anvil...).
pub fn close_viewers_of(server: &Server, dim: Dimension, pos: BlockPos) {
    for p in server.online().into_iter().filter(|p| p.dim() == dim) {
        let left = {
            let mut st = p.state.lock().unwrap();
            if st.inv.window.as_ref().is_none_or(|w| w.pos != pos) {
                continue;
            }
            let id = st.inv.window_id();
            let left = st.inv.close();
            let mut c = packet(id::play::CLOSE_WINDOW);
            c.u8(id);
            p.send(c);
            let inv = st.inv.window_items();
            p.send(inv);
            left
        };
        for item in left {
            entities::throw_from(server, &p, item);
        }
    }
}

/// Block with contents that progresses on its own (furnace or brewing stand).
#[derive(Clone, PartialEq)]
enum Ticking {
    Furnace(Furnace),
    Brewing(Brewing),
}

impl Ticking {
    fn items(&self) -> &[Slot] {
        match self {
            Ticking::Furnace(f) => &f.items,
            Ticking::Brewing(b) => &b.items,
        }
    }

    fn properties(&self, inv: &super::inventory::Inventory) -> Vec<Vec<u8>> {
        match self {
            Ticking::Furnace(f) => vec![
                inv.property_packet(0, f.burn as i16),
                inv.property_packet(1, f.burn_total as i16),
                inv.property_packet(2, f.cook as i16),
                inv.property_packet(3, COOK_TIME as i16),
            ],
            Ticking::Brewing(b) => vec![inv.property_packet(0, b.brew_time as i16), inv.property_packet(1, b.fuel as i16)],
        }
    }
}

/// One smelting step on a copy of the furnace (vanilla formula from AbstractFurnaceBlockEntity).
fn step_furnace(f: &mut Furnace) {
    let reg = data::get();
    let name_of = |s: &Slot| s.as_ref().map(|i| i.name());
    let result = name_of(&f.items[0]).and_then(gameplay::smelt);
    let can_smelt = match (&result, &f.items[2]) {
        (Some(_), None) => true,
        (Some((name, _)), Some(out)) => out.name() == name && out.count < out.max_stack(),
        _ => false,
    };
    if f.burn > 0 {
        f.burn -= 1;
    }
    if f.burn == 0 && can_smelt {
        let fuel = name_of(&f.items[1]).map_or(0, gameplay::fuel_ticks);
        if fuel > 0 {
            f.burn = fuel;
            f.burn_total = fuel;
            let lava = name_of(&f.items[1]) == Some("lava_bucket");
            let rem = lava.then(|| reg.item_id("bucket")).flatten().map(|b| ItemStack::new(b, 1));
            if let Some(s) = &mut f.items[1] {
                s.count -= 1;
                if s.count <= 0 {
                    f.items[1] = rem;
                }
            }
        }
    }
    if f.burn > 0 && can_smelt {
        f.cook += 1;
        if f.cook >= COOK_TIME {
            f.cook = 0;
            let (name, xp) = result.unwrap();
            match &mut f.items[2] {
                Some(o) => o.count += 1,
                None => f.items[2] = Some(ItemStack::new(reg.item_id(&name).unwrap_or(0), 1)),
            }
            if let Some(i) = &mut f.items[0] {
                i.count -= 1;
                if i.count <= 0 {
                    f.items[0] = None;
                }
            }
            f.xp += xp;
        }
    } else if f.cook > 0 {
        f.cook = (f.cook - 2).max(0);
    }
}

/// One tick of all loaded furnaces.
pub fn tick_furnaces(server: &Arc<Server>) {
    for dim in Dimension::ALL {
        tick_furnaces_in(server, dim);
    }
}

fn tick_furnaces_in(server: &Arc<Server>, dim: Dimension) {
    let reg = data::get();
    let mut changed: Vec<(BlockPos, Ticking, Option<u16>)> = Vec::new();
    let mut brewed = Vec::new();
    {
        let mut world = server.world(dim).lock().unwrap();
        for pos in world.loaded_furnaces() {
            match world.block_entity(pos).cloned() {
                Some(BlockEntity::Furnace(f)) => {
                    let mut next = f.clone();
                    step_furnace(&mut next);
                    if next == f {
                        continue;
                    }
                    // The block turns on/off (`lit` property).
                    let new_state = if (f.burn > 0) != (next.burn > 0) {
                        let state = world.get_block(pos);
                        let lit = reg.block_of_state(state).with(state, "lit", if next.burn > 0 { "true" } else { "false" });
                        world.set_block(pos, lit);
                        Some(lit)
                    } else {
                        None
                    };
                    world.set_block_entity(pos, BlockEntity::Furnace(next.clone()));
                    changed.push((pos, Ticking::Furnace(next), new_state));
                }
                Some(BlockEntity::Brewing(b)) => {
                    let mut next = b.clone();
                    if super::brewing::step(&mut next) {
                        brewed.push(pos);
                    }
                    if next == b {
                        continue;
                    }
                    // Bottles are shown on the stand (has_bottle_0..2).
                    let state = world.get_block(pos);
                    let block = reg.block_of_state(state);
                    let mut s = state;
                    for i in 0..3 {
                        s = block.with(s, &format!("has_bottle_{i}"), if next.items[i].is_some() { "true" } else { "false" });
                    }
                    let new_state = (s != state).then(|| {
                        world.set_block(pos, s);
                        s
                    });
                    world.set_block_entity(pos, BlockEntity::Brewing(next.clone()));
                    changed.push((pos, Ticking::Brewing(next), new_state));
                }
                _ => {}
            }
        }
    }
    for pos in brewed {
        let mut p = packet(id::play::WORLD_EVENT);
        p.i32(1035).position(pos).i32(0).bool(false);
        server.broadcast_dim(dim, &p, None);
    }
    for (pos, t, new_state) in changed {
        if let Some(s) = new_state {
            server.broadcast_dim(dim, &super::blocks::block_change(pos, s), None);
        }
        for v in viewers(server, dim, pos) {
            let mut st = v.state.lock().unwrap();
            if let Some(p) = st.inv.refresh_shared(pos, t.items()) {
                v.send(p);
            }
            for p in t.properties(&st.inv) {
                v.send(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: &str, c: i8) -> Slot {
        Some(ItemStack::new(data::get().item_id(n).unwrap(), c))
    }

    #[test]
    fn smelting() {
        let mut f = Furnace { items: vec![item("raw_iron", 2), item("coal", 1), None], ..Default::default() };
        for _ in 0..COOK_TIME {
            step_furnace(&mut f);
        }
        assert_eq!(f.items[2].as_ref().map(|i| (i.name(), i.count)), Some(("iron_ingot", 1)));
        assert_eq!(f.items[0].as_ref().unwrap().count, 1);
        assert!(f.items[1].is_none(), "el carbón se consumió");
        // The tick in which it ignites does not consume fuel (like vanilla).
        assert_eq!(f.burn, 1600 - COOK_TIME + 1);
        assert!((f.xp - 0.7).abs() < 1e-6);
        // Without fuel it does not progress.
        let mut g = Furnace { items: vec![item("sand", 1), None, None], ..Default::default() };
        step_furnace(&mut g);
        assert_eq!(g.cook, 0);
    }
}

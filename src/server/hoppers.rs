//! Moving items between blocks: hoppers (pull from the block above or the items lying on
//! it, push where they point, stop while powered), droppers (pass items to the container
//! in front or drop them) and dispensers (shoot arrows, potions and fire charges, pour and
//! pick up fluids, light fires, prime TNT, use bone meal, shear sheep, hatch spawn eggs).
//!
//! Each kind of block takes and gives items through its faces like vanilla: furnaces take
//! what to smelt from above and fuel from the sides and give the result downwards; brewing
//! stands take the ingredient from above and bottles and blaze powder from the sides; a
//! composter takes compostable items and gives bone meal when full.

use super::{containers, entities, mechanisms, mobs, projectiles, sound, Server};
use crate::data;
use crate::gameplay;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::blockentity::BlockEntity;
use crate::world::{Dimension, World};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Ticks a hopper waits after moving an item (vanilla: 8).
const COOLDOWN: i64 = 8;

/// When each hopper may move again.
static NEXT: Mutex<Option<HashMap<(Dimension, BlockPos), i64>>> = Mutex::new(None);

fn dir(face: &str) -> (i32, i32, i32) {
    match face {
        "down" => (0, -1, 0),
        "up" => (0, 1, 0),
        "north" => (0, 0, -1),
        "south" => (0, 0, 1),
        "west" => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

fn opposite(face: &str) -> &'static str {
    match face {
        "down" => "up",
        "up" => "down",
        "north" => "south",
        "south" => "north",
        "west" => "east",
        _ => "west",
    }
}

/// Direction index for world events (Direction.get3DDataValue).
fn face_index(face: &str) -> i32 {
    ["down", "up", "north", "south", "west", "east"].iter().position(|f| *f == face).unwrap_or(0) as i32
}

fn step(p: BlockPos, face: &str) -> BlockPos {
    let (dx, dy, dz) = dir(face);
    BlockPos::new(p.x + dx, p.y + dy, p.z + dz)
}

fn name_at(world: &mut World, p: BlockPos) -> &'static str {
    data::get().block_of_state(world.get_block(p)).name.as_str()
}

fn is_furnace(name: &str) -> bool {
    matches!(name, "furnace" | "smoker" | "blast_furnace")
}

/// The items a block holds (a copy), if it holds any.
fn items_of(world: &mut World, p: BlockPos) -> Option<Vec<Slot>> {
    if world.peek_block_entity(p).is_none() {
        // Generated or old blocks may lack their entity.
        let be = BlockEntity::for_block(name_at(world, p))?;
        if !matches!(be, BlockEntity::Container(_) | BlockEntity::Furnace(_) | BlockEntity::Brewing(_)) {
            return None;
        }
        world.set_block_entity(p, be);
    }
    match world.peek_block_entity(p)? {
        BlockEntity::Container(items) => Some(items.clone()),
        BlockEntity::Furnace(f) => Some(f.items.clone()),
        BlockEntity::Brewing(b) => Some(b.items.clone()),
        _ => None,
    }
}

fn set_items(world: &mut World, p: BlockPos, items: Vec<Slot>) {
    match world.block_entity(p) {
        Some(BlockEntity::Container(v)) => *v = items,
        Some(BlockEntity::Furnace(f)) => f.items = items,
        Some(BlockEntity::Brewing(b)) => b.items = items,
        _ => {}
    }
}

/// Slots of block `name` an item may enter through its `face`.
fn insert_slots(name: &str, face: &str, item: &ItemStack, len: usize) -> Vec<usize> {
    let n = item.name();
    if is_furnace(name) {
        return match face {
            "up" => vec![0],
            "down" => vec![],
            _ if gameplay::fuel_ticks(n) > 0 => vec![1],
            _ => vec![],
        };
    }
    if name == "brewing_stand" {
        return match face {
            "up" if super::brewing::is_ingredient(n) => vec![3],
            "up" | "down" => vec![],
            _ if n == "blaze_powder" => vec![4],
            _ if matches!(n, "potion" | "splash_potion" | "lingering_potion" | "glass_bottle") => vec![0, 1, 2],
            _ => vec![],
        };
    }
    (0..len).collect()
}

/// Slots of block `name` that can be taken out through its `face`.
fn extract_slots(name: &str, face: &str, items: &[Slot]) -> Vec<usize> {
    if is_furnace(name) {
        let mut v = vec![];
        if face == "down" {
            v.push(2);
            if items.get(1).and_then(|s| s.as_ref()).is_some_and(|i| i.name() == "bucket") {
                v.push(1);
            }
        }
        return v;
    }
    if name == "brewing_stand" {
        return if face == "down" { vec![0, 1, 2] } else { vec![] };
    }
    (0..items.len()).collect()
}

fn slot_max(name: &str, slot: usize, item: &ItemStack) -> i8 {
    if name == "brewing_stand" && slot < 3 { 1 } else { item.max_stack() }
}

/// Adds one unit of `item` to the first of `slots` where it fits.
fn add_one(name: &str, items: &mut [Slot], slots: &[usize], item: &ItemStack) -> bool {
    // First onto a matching stack, then into an empty slot.
    for &i in slots {
        if let Some(s) = items.get_mut(i).and_then(|s| s.as_mut()) {
            if s.stacks_with(item) && s.count < slot_max(name, i, s) {
                s.count += 1;
                return true;
            }
        }
    }
    for &i in slots {
        if let Some(s) = items.get_mut(i).filter(|s| s.is_none()) {
            *s = Some(item.with_count(1));
            return true;
        }
    }
    false
}

fn take_one(items: &mut [Slot], i: usize) -> Option<ItemStack> {
    let s = items.get_mut(i)?;
    let it = s.as_ref()?.with_count(1);
    let left = s.as_ref()?.count - 1;
    if left <= 0 {
        *s = None;
    } else if let Some(x) = s.as_mut() {
        x.count = left;
    }
    Some(it)
}

/// What a transfer leaves to do once the world is released.
#[derive(Default)]
struct Effects {
    /// Containers whose contents changed (windows and comparators).
    changed: Vec<BlockPos>,
    /// Block states that changed (composter levels).
    blocks: Vec<(BlockPos, u16)>,
    /// Composters that received an item (and which).
    composted: Vec<(BlockPos, &'static str)>,
}

/// Passes one item from `from` into the block at `to` (entering through `face`).
fn push_one(world: &mut World, from: BlockPos, to: BlockPos, face: &str, fx: &mut Effects) -> bool {
    let Some(mut mine) = items_of(world, from) else { return false };
    let target = name_at(world, to);
    if target == "composter" {
        let state = world.get_block(to);
        let level = mechanisms::composter_level(state);
        if level >= 7 {
            return false;
        }
        for i in 0..mine.len() {
            if let Some(it) = mine[i].as_ref().filter(|it| mechanisms::compost_chance(it.name()).is_some()) {
                let name = it.name();
                take_one(&mut mine, i);
                set_items(world, from, mine);
                fx.changed.push(from);
                fx.composted.push((to, name));
                return true;
            }
        }
        return false;
    }
    let Some(mut theirs) = items_of(world, to) else { return false };
    for i in 0..mine.len() {
        let Some(it) = mine[i].clone() else { continue };
        let slots = insert_slots(target, face, &it, theirs.len());
        if add_one(target, &mut theirs, &slots, &it) {
            take_one(&mut mine, i);
            set_items(world, from, mine);
            set_items(world, to, theirs);
            fx.changed.extend([from, to]);
            return true;
        }
    }
    false
}

/// The hopper at `pos` takes one item from the block above.
fn pull_one(world: &mut World, pos: BlockPos, fx: &mut Effects) -> bool {
    let src = step(pos, "up");
    let Some(mut mine) = items_of(world, pos) else { return false };
    let name = name_at(world, src);
    if name == "composter" {
        let state = world.get_block(src);
        if mechanisms::composter_level(state) < 8 {
            return false;
        }
        let Some(bone) = data::get().item_id("bone_meal").map(|i| ItemStack::new(i, 1)) else { return false };
        if !add_one("hopper", &mut mine, &[0, 1, 2, 3, 4], &bone) {
            return false;
        }
        set_items(world, pos, mine);
        let empty = mechanisms::with_composter_level(state, 0);
        world.set_block(src, empty);
        fx.blocks.push((src, empty));
        fx.changed.push(pos);
        return true;
    }
    let Some(mut theirs) = items_of(world, src) else { return false };
    for i in extract_slots(name, "down", &theirs) {
        let Some(it) = theirs.get(i).cloned().flatten() else { continue };
        if add_one("hopper", &mut mine, &[0, 1, 2, 3, 4], &it) {
            take_one(&mut theirs, i);
            set_items(world, pos, mine);
            set_items(world, src, theirs);
            fx.changed.extend([pos, src]);
            return true;
        }
    }
    false
}

fn apply(server: &Server, dim: Dimension, fx: Effects) {
    for (p, s) in fx.blocks {
        server.broadcast_dim(dim, &super::blocks::block_change(p, s), None);
        super::redstone::changed(server, dim, p);
    }
    let mut seen = Vec::new();
    for p in fx.changed {
        if !seen.contains(&p) {
            seen.push(p);
            containers::sync(server, dim, p);
        }
    }
    for (p, item) in fx.composted {
        mechanisms::compost_one(server, dim, p, item);
    }
}

/// Every tick: hoppers whose cooldown is over try to move an item.
pub fn tick(server: &Arc<Server>) {
    let now = server.age.load(Ordering::Relaxed);
    for dim in Dimension::ALL {
        let hoppers = server.world(dim).lock().unwrap().loaded_containers(&["hopper"]);
        if hoppers.is_empty() {
            continue;
        }
        for pos in hoppers {
            let ready = NEXT.lock().unwrap().get_or_insert_with(HashMap::new).get(&(dim, pos)).is_none_or(|&t| t <= now);
            if ready && hopper_step(server, dim, pos) {
                NEXT.lock().unwrap().get_or_insert_with(HashMap::new).insert((dim, pos), now + COOLDOWN);
            }
        }
    }
    // Forget hoppers that are long gone.
    if now % 1200 == 0 {
        if let Some(m) = NEXT.lock().unwrap().as_mut() {
            m.retain(|_, t| *t > now - 1200);
        }
    }
}

fn hopper_step(server: &Server, dim: Dimension, pos: BlockPos) -> bool {
    let reg = data::get();
    let mut fx = Effects::default();
    let (moved, suck) = {
        let mut world = server.world(dim).lock().unwrap();
        let state = world.get_block(pos);
        let b = reg.block_of_state(state);
        if b.get(state, "enabled") == Some("false") {
            return false;
        }
        let facing = b.get(state, "facing").unwrap_or("down");
        let pushed = push_one(&mut world, pos, step(pos, facing), opposite(facing), &mut fx);
        let pulled = pull_one(&mut world, pos, &mut fx);
        // With nothing to pull from above, it sucks in items lying on it (unless a solid block covers it).
        let above = step(pos, "up");
        let covered = items_of(&mut world, above).is_some() || gameplay::get().shape(world.get_block(above)) == [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        (pushed || pulled, !pulled && !covered)
    };
    let mut sucked = false;
    if suck {
        let mut mine = items_of(&mut server.world(dim).lock().unwrap(), pos).unwrap_or_default();
        let min = [pos.x as f64, pos.y as f64 + 0.6, pos.z as f64];
        let max = [pos.x as f64 + 1.0, pos.y as f64 + 2.0, pos.z as f64 + 1.0];
        sucked = entities::take_in_box(server, dim, min, max, |it| {
            let mut n = 0;
            while n < it.count && add_one("hopper", &mut mine, &[0, 1, 2, 3, 4], it) {
                n += 1;
            }
            n
        });
        if sucked {
            set_items(&mut server.world(dim).lock().unwrap(), pos, mine);
            fx.changed.push(pos);
        }
    }
    apply(server, dim, fx);
    moved || sucked
}

// ---------- droppers and dispensers ----------

/// What using the item did.
enum Used {
    /// One unit is spent.
    Consume,
    /// One unit is spent and this comes back (an empty or filled bucket, a worn tool).
    Replace(ItemStack),
    /// It could not be used: dropped as an item instead.
    Drop,
}

fn world_event(server: &Server, dim: Dimension, pos: BlockPos, event: i32, data: i32) {
    let mut p = packet(id::play::WORLD_EVENT);
    p.i32(event).position(pos).i32(data).bool(false);
    server.broadcast_dim(dim, &p, None);
}

/// A dropper or dispenser received a redstone pulse.
pub fn dispense(server: &Arc<Server>, dim: Dimension, pos: BlockPos) {
    let reg = data::get();
    let picked = {
        let mut world = server.world(dim).lock().unwrap();
        let state = world.get_block(pos);
        let b = reg.block_of_state(state);
        let facing = b.get(state, "facing").unwrap_or("north");
        let items = items_of(&mut world, pos).unwrap_or_default();
        let full: Vec<usize> = (0..items.len()).filter(|&i| items[i].is_some()).collect();
        if full.is_empty() {
            None
        } else {
            let i = full[(entities::random() * full.len() as f32) as usize % full.len()];
            Some((b.name.as_str() == "dropper", facing, i, items[i].clone().unwrap()))
        }
    };
    let Some((dropper, facing, slot, item)) = picked else {
        world_event(server, dim, pos, 1001, 0);
        return;
    };
    let front = step(pos, facing);
    let d = dir(facing);
    let center = [pos.x as f64 + 0.5 + d.0 as f64 * 0.7, pos.y as f64 + 0.5 + d.1 as f64 * 0.7, pos.z as f64 + 0.5 + d.2 as f64 * 0.7];
    // A dropper (or a dispenser with a plain item) passes it to the container in front.
    if dropper {
        let mut fx = Effects::default();
        let (passed, container) = {
            let mut world = server.world(dim).lock().unwrap();
            let passed = push_slot(&mut world, pos, slot, front, opposite(facing), &mut fx);
            (passed, passed || items_of(&mut world, front).is_some())
        };
        if passed {
            apply(server, dim, fx);
            world_event(server, dim, pos, 1000, 0);
            return;
        }
        // A full container in front: nothing comes out.
        if container {
            world_event(server, dim, pos, 1001, 0);
            return;
        }
    }
    let used = if dropper { Used::Drop } else { use_item(server, dim, front, facing, center, &item) };
    let event = match &used {
        Used::Drop => {
            drop_item(server, dim, center, d, item.with_count(1));
            1000
        }
        _ if is_projectile(item.name()) => 1002,
        _ => 1000,
    };
    {
        let mut world = server.world(dim).lock().unwrap();
        let mut items = items_of(&mut world, pos).unwrap_or_default();
        take_one(&mut items, slot);
        if let Used::Replace(back) = used {
            if items[slot].is_none() {
                items[slot] = Some(back);
            } else if !add_one("dispenser", &mut items, &(0..9).collect::<Vec<_>>(), &back) {
                drop(world);
                drop_item(server, dim, center, d, back);
                world = server.world(dim).lock().unwrap();
            }
        }
        set_items(&mut world, pos, items);
    }
    containers::sync(server, dim, pos);
    world_event(server, dim, pos, event, 0);
    world_event(server, dim, pos, 2000, face_index(facing));
}

fn is_projectile(name: &str) -> bool {
    matches!(name, "arrow" | "tipped_arrow" | "spectral_arrow" | "splash_potion" | "lingering_potion" | "experience_bottle" | "fire_charge")
}

/// A dropper passes one item from `slot` into the block in front.
fn push_slot(world: &mut World, from: BlockPos, slot: usize, to: BlockPos, face: &str, fx: &mut Effects) -> bool {
    let Some(mut mine) = items_of(world, from) else { return false };
    let Some(it) = mine.get(slot).cloned().flatten() else { return false };
    let target = name_at(world, to);
    let Some(mut theirs) = items_of(world, to) else { return false };
    let slots = insert_slots(target, face, &it, theirs.len());
    if !add_one(target, &mut theirs, &slots, &it) {
        return false;
    }
    take_one(&mut mine, slot);
    set_items(world, from, mine);
    set_items(world, to, theirs);
    fx.changed.extend([from, to]);
    true
}

/// An item thrown out of the block's front, like vanilla's DefaultDispenseItemBehavior.
fn drop_item(server: &Server, dim: Dimension, at: [f64; 3], d: (i32, i32, i32), item: ItemStack) {
    let speed = 0.1 + entities::random() as f64 * 0.2;
    let jitter = || (entities::random() as f64 - 0.5) * 0.045;
    let vel = [d.0 as f64 * speed + jitter(), if d.1 == 0 { 0.2 } else { d.1 as f64 * speed } + jitter(), d.2 as f64 * speed + jitter()];
    let y = if d.1 == 0 { at[1] - 0.15625 } else { at[1] - 0.125 };
    entities::spawn(server, dim, [at[0], y, at[2]], vel, item, 10);
}

/// What a dispenser does with each item.
fn use_item(server: &Arc<Server>, dim: Dimension, front: BlockPos, facing: &str, at: [f64; 3], item: &ItemStack) -> Used {
    let reg = data::get();
    let d = dir(facing);
    let aim = |speed: f64, lift: f64| {
        let spread = || (entities::random() as f64 - 0.5) * 0.0172 * 6.0;
        [d.0 as f64 * speed + spread(), d.1 as f64 * speed + lift + spread(), d.2 as f64 * speed + spread()]
    };
    let one = item.with_count(1);
    let name = item.name();
    match name {
        "arrow" | "tipped_arrow" | "spectral_arrow" => {
            let data = projectiles::ArrowData::new(one);
            projectiles::shoot_with(server, dim, projectiles::ProjectileKind::Arrow, -1, at, aim(1.1, 0.1), Some(data));
            Used::Consume
        }
        "splash_potion" | "lingering_potion" => {
            projectiles::shoot(server, dim, projectiles::ProjectileKind::Potion(one), -1, at, aim(0.69, 0.1));
            Used::Consume
        }
        "experience_bottle" => {
            projectiles::shoot(server, dim, projectiles::ProjectileKind::XpBottle, -1, at, aim(0.69, 0.1));
            Used::Consume
        }
        "fire_charge" => {
            projectiles::shoot(server, dim, projectiles::ProjectileKind::SmallFireball, -1, at, aim(1.0, 0.0));
            sound::at(server, dim, at, "entity.blaze.shoot", sound::BLOCK, 1.0, 1.0);
            Used::Consume
        }
        "bucket" | "water_bucket" | "lava_bucket" => match super::fluids::bucket_at(server, dim, front, name) {
            Some(back) => Used::Replace(ItemStack::new(reg.item_id(back).unwrap_or(0), 1)),
            None => Used::Drop,
        },
        "flint_and_steel" => {
            let target = name_at(&mut server.world(dim).lock().unwrap(), front);
            let lit = if target == "tnt" {
                super::redstone::prime_tnt(server, dim, front, 80);
                true
            } else {
                super::fire::ignite(server, dim, front)
            };
            if !lit {
                return Used::Drop;
            }
            let mut worn = item.clone();
            if worn.wear(1, &mut || entities::random()) { Used::Consume } else { Used::Replace(worn) }
        }
        "tnt" => {
            super::redstone::spawn_tnt(server, dim, [front.x as f64 + 0.5, front.y as f64, front.z as f64 + 0.5], 80);
            Used::Consume
        }
        "bone_meal" => {
            if super::farming::bone_meal_at(server, dim, front) {
                Used::Consume
            } else {
                Used::Drop
            }
        }
        "shears" => {
            if !shear_in_front(server, dim, front) {
                return Used::Drop;
            }
            let mut worn = item.clone();
            if worn.wear(1, &mut || entities::random()) { Used::Consume } else { Used::Replace(worn) }
        }
        n => match n.strip_suffix("_spawn_egg").and_then(mobs::MobKind::from_name) {
            Some(kind) => {
                mobs::spawn(server, dim, kind, [front.x as f64 + 0.5, front.y as f64, front.z as f64 + 0.5]);
                Used::Consume
            }
            None => Used::Drop,
        },
    }
}

/// Shears a sheep standing in front of the dispenser.
fn shear_in_front(server: &Server, dim: Dimension, front: BlockPos) -> bool {
    let at = {
        let mut mobs = server.mobs.lock().unwrap();
        let Some(m) = mobs.values_mut().find(|m| {
            m.dim == dim && m.kind == mobs::MobKind::Sheep && !m.sheared && m.grow >= 0 && m.pos[0].floor() as i32 == front.x && m.pos[2].floor() as i32 == front.z && (m.pos[1] - front.y as f64).abs() < 1.0
        }) else {
            return false;
        };
        m.sheared = true;
        server.broadcast_tracked(m.eid, &mobs::metadata(m), true);
        m.center()
    };
    if let Some(w) = data::get().item_id("white_wool") {
        let n = 1 + (entities::random() * 3.0) as i8;
        entities::spawn(server, dim, at, [0.0, 0.2, 0.0], ItemStack::new(w, n), 10);
    }
    sound::at(server, dim, at, "entity.sheep.shear", sound::BLOCK, 1.0, 1.0);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: &str, c: i8) -> Slot {
        Some(ItemStack::new(data::get().item_id(n).unwrap(), c))
    }

    #[test]
    fn faces() {
        let coal = item("coal", 1).unwrap();
        let iron = item("raw_iron", 1).unwrap();
        assert_eq!(insert_slots("furnace", "up", &iron, 3), vec![0]);
        assert_eq!(insert_slots("furnace", "north", &coal, 3), vec![1]);
        assert!(insert_slots("furnace", "north", &iron, 3).is_empty(), "lo que no es combustible no entra por el costado");
        assert_eq!(insert_slots("chest", "north", &iron, 27).len(), 27);
        let mut f = vec![item("raw_iron", 1), Some(ItemStack::new(data::get().item_id("bucket").unwrap(), 1)), item("iron_ingot", 3)];
        assert_eq!(extract_slots("furnace", "down", &f), vec![2, 1]);
        // Stacks first, then empty slots.
        let mut hopper = vec![None, item("iron_ingot", 63), None, None, None];
        assert!(add_one("hopper", &mut hopper, &[0, 1, 2, 3, 4], &f[2].clone().unwrap()));
        assert_eq!(hopper[1].as_ref().unwrap().count, 64);
        assert!(add_one("hopper", &mut hopper, &[0, 1, 2, 3, 4], &f[2].clone().unwrap()));
        assert_eq!(hopper[0].as_ref().unwrap().count, 1);
        assert_eq!(take_one(&mut f, 2).map(|i| i.count), Some(1));
        assert_eq!(f[2].as_ref().unwrap().count, 2);
    }
}

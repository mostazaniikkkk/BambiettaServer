//! Blocks you use by hand: the composter (compostable items fill it up, full it gives bone
//! meal), the jukebox (plays the record put in it), the lectern (holds a book that everyone
//! can read and whose pages are turned together; turning one gives a redstone pulse) and the
//! bell (rung by hand or by redstone, makes nearby raiders glow).

use super::inventory::WindowKind;
use super::{containers, entities, mobs, sound, Player, Server};
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use crate::world::blockentity::BlockEntity;
use crate::world::Dimension;
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

/// Takes one of the held item (not in creative).
fn use_held(player: &Player, hand: i32) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 {
        return;
    }
    let held = st.held_slot;
    let slot = st.inv.consume_held(hand, held, 1, None);
    st.sync_slot(player, slot);
}

/// Pops an item out of the top of a block.
fn pop_out(server: &Server, dim: Dimension, pos: BlockPos, item: ItemStack) {
    let r = || (entities::random() as f64 - 0.5) * 0.1;
    entities::spawn(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 1.01, pos.z as f64 + 0.5], [r(), 0.15, r()], item, 10);
}

// ---------- composter ----------

/// Chance that an item raises the composter's level (vanilla ComposterBlock.COMPOSTABLES).
pub fn compost_chance(item: &str) -> Option<f32> {
    Some(match item {
        "cake" | "pumpkin_pie" => 1.0,
        "baked_potato" | "bread" | "cookie" | "flowering_azalea" | "hay_block" | "brown_mushroom_block" | "red_mushroom_block" | "nether_wart_block" | "warped_wart_block" => 0.85,
        "apple" | "azalea" | "beetroot" | "big_dripleaf" | "carrot" | "cocoa_beans" | "fern" | "large_fern" | "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "oxeye_daisy" | "cornflower"
        | "lily_of_the_valley" | "wither_rose" | "sunflower" | "lilac" | "rose_bush" | "peony" | "torchflower" | "pitcher_plant" | "lily_pad" | "melon" | "moss_block" | "brown_mushroom" | "red_mushroom"
        | "mushroom_stem" | "nether_wart" | "potato" | "pumpkin" | "carved_pumpkin" | "sea_pickle" | "shroomlight" | "spore_blossom" | "wheat" | "crimson_fungus" | "warped_fungus" | "crimson_roots"
        | "warped_roots" => 0.65,
        "dried_kelp_block" | "tall_grass" | "flowering_azalea_leaves" | "cactus" | "sugar_cane" | "twisting_vines" | "weeping_vines" | "vine" | "melon_slice" | "nether_sprouts" | "glow_lichen" => 0.5,
        "mangrove_propagule" | "mangrove_roots" | "dried_kelp" | "short_grass" | "kelp" | "sweet_berries" | "glow_berries" | "seagrass" | "small_dripleaf" | "hanging_roots" | "pitcher_pod"
        | "pink_petals" | "moss_carpet" => 0.3,
        n if n.ends_with("_tulip") => 0.65,
        n if n.ends_with("_leaves") || n.ends_with("_sapling") || n.ends_with("_seeds") => 0.3,
        _ => return None,
    })
}

pub fn composter_level(state: u16) -> i32 {
    let b = data::get().block_of_state(state);
    b.get(state, "level").and_then(|l| l.parse().ok()).unwrap_or(0)
}

pub fn with_composter_level(state: u16, level: i32) -> u16 {
    data::get().block_of_state(state).with(state, "level", &level.to_string())
}

/// One compostable item went into the composter at `pos` (already taken from whoever gave it).
pub fn compost_one(server: &Server, dim: Dimension, pos: BlockPos, item: &str) {
    let state = server.world(dim).lock().unwrap().get_block(pos);
    if data::get().block_of_state(state).name != "composter" {
        return;
    }
    let level = composter_level(state);
    let chance = compost_chance(item).unwrap_or(0.0);
    // An empty composter always takes the first item.
    let success = level < 7 && ((level == 0 && chance > 0.0) || entities::random() < chance);
    if success {
        set_block(server, dim, pos, with_composter_level(state, level + 1));
        if level + 1 == 7 {
            super::redstone::schedule(server, dim, pos, 20);
        }
    }
    world_event(server, dim, pos, 1500, i32::from(success));
}

/// After 1 s at level 7 the compost is ready (level 8). Called from the scheduled tick.
pub fn composter_ready(state: u16) -> Option<u16> {
    (composter_level(state) == 7).then(|| with_composter_level(state, 8))
}

fn use_composter(server: &Server, player: &Player, pos: BlockPos, state: u16, held: &Slot, hand: i32) -> bool {
    let dim = player.dim();
    let level = composter_level(state);
    if level == 8 {
        if let Some(bone) = data::get().item_id("bone_meal") {
            pop_out(server, dim, pos, ItemStack::new(bone, 1));
        }
        set_block(server, dim, pos, with_composter_level(state, 0));
        sound::at(server, dim, center(pos), "block.composter.empty", sound::BLOCK, 1.0, 1.0);
        return true;
    }
    let Some(item) = held.as_ref().filter(|i| compost_chance(i.name()).is_some()) else { return false };
    if level >= 7 {
        return false;
    }
    use_held(player, hand);
    compost_one(server, dim, pos, item.name());
    true
}

// ---------- jukebox ----------

/// Comparator signal of each record (vanilla RecordItem analog output).
fn record_signal(name: &str) -> u8 {
    match name.trim_start_matches("music_disc_") {
        "13" => 1,
        "cat" => 2,
        "blocks" => 3,
        "chirp" => 4,
        "far" => 5,
        "mall" => 6,
        "mellohi" => 7,
        "stal" => 8,
        "strad" => 9,
        "ward" => 10,
        "11" => 11,
        "wait" => 12,
        "pigstep" => 13,
        "otherside" | "relic" => 14,
        "5" => 15,
        _ => 0,
    }
}

fn use_jukebox(server: &Server, player: &Player, pos: BlockPos, state: u16, held: &Slot, hand: i32) -> bool {
    let dim = player.dim();
    let block = data::get().block_of_state(state);
    let inside = match server.world(dim).lock().unwrap().block_entity(pos) {
        Some(BlockEntity::Jukebox(r)) => r.take(),
        _ => None,
    };
    if let Some(record) = inside {
        // Out comes the record and the music stops.
        world_event(server, dim, pos, 1011, 0);
        pop_out(server, dim, pos, record);
        set_block(server, dim, pos, block.with(state, "has_record", "false"));
        return true;
    }
    let Some(disc) = held.as_ref().filter(|i| i.name().starts_with("music_disc_")) else { return false };
    server.world(dim).lock().unwrap().set_block_entity(pos, BlockEntity::Jukebox(Some(disc.with_count(1))));
    use_held(player, hand);
    set_block(server, dim, pos, block.with(state, "has_record", "true"));
    world_event(server, dim, pos, 1010, disc.id);
    true
}

// ---------- lectern ----------

fn pages(book: &ItemStack) -> i32 {
    book.nbt.as_ref().and_then(|n| n.get("pages")).map_or(0, |p| Nbt::as_list(p).len() as i32)
}

fn page_signal(book: &ItemStack, page: i32) -> u8 {
    let n = pages(book);
    if n <= 1 {
        return 15;
    }
    (1 + (page.clamp(0, n - 1) as f32 / (n - 1) as f32 * 14.0).floor() as i32) as u8
}

fn use_lectern(server: &Server, player: &Arc<Player>, pos: BlockPos, state: u16, held: &Slot, hand: i32) -> bool {
    let dim = player.dim();
    let on_it = match server.world(dim).lock().unwrap().peek_block_entity(pos) {
        Some(BlockEntity::Lectern { book: Some(b), page }) => Some((b.clone(), *page)),
        _ => None,
    };
    if let Some((book, page)) = on_it {
        let mut st = player.state.lock().unwrap();
        for p in st.inv.open(WindowKind::Lectern, pos, vec![Some(book)], "Atril") {
            player.send(p);
        }
        let p = st.inv.property_packet(0, page as i16);
        player.send(p);
        return true;
    }
    let Some(book) = held.as_ref().filter(|i| matches!(i.name(), "writable_book" | "written_book")) else { return false };
    server.world(dim).lock().unwrap().set_block_entity(pos, BlockEntity::Lectern { book: Some(book.with_count(1)), page: 0 });
    use_held(player, hand);
    set_block(server, dim, pos, data::get().block_of_state(state).with(state, "has_book", "true"));
    sound::at(server, dim, center(pos), "item.book.put", sound::BLOCK, 1.0, 1.0);
    true
}

/// A button in the lectern window: 1 previous page, 2 next, 3 take the book, 100+n page n.
pub fn lectern_button(server: &Arc<Server>, player: &Arc<Player>, button: i32) {
    let dim = player.dim();
    let Some(pos) = player.state.lock().unwrap().inv.window.as_ref().filter(|w| w.kind == WindowKind::Lectern).map(|w| w.pos) else { return };
    let (book, page) = match server.world(dim).lock().unwrap().peek_block_entity(pos) {
        Some(BlockEntity::Lectern { book: Some(b), page }) => (b.clone(), *page),
        _ => return,
    };
    if button == 3 {
        take_book(server, player, pos);
        return;
    }
    let last = (pages(&book) - 1).max(0);
    let new = match button {
        1 => page - 1,
        2 => page + 1,
        b if b >= 100 => b - 100,
        _ => return,
    }
    .clamp(0, last);
    if new == page {
        return;
    }
    if let Some(BlockEntity::Lectern { page, .. }) = server.world(dim).lock().unwrap().block_entity(pos) {
        *page = new;
    }
    for v in server.online().into_iter().filter(|v| v.dim() == dim) {
        let st = v.state.lock().unwrap();
        if st.inv.window.as_ref().is_some_and(|w| w.kind == WindowKind::Lectern && w.pos == pos) {
            v.send(st.inv.property_packet(0, new as i16));
        }
    }
    // Turning a page gives a short redstone pulse.
    let state = server.world(dim).lock().unwrap().get_block(pos);
    set_block(server, dim, pos, data::get().block_of_state(state).with(state, "powered", "true"));
    super::redstone::schedule(server, dim, pos, 2);
}

fn take_book(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) {
    let dim = player.dim();
    if player.state.lock().unwrap().game_mode == 2 {
        return;
    }
    let book = match server.world(dim).lock().unwrap().block_entity(pos) {
        Some(BlockEntity::Lectern { book, page }) => {
            *page = 0;
            book.take()
        }
        _ => None,
    };
    let Some(book) = book else { return };
    containers::close_viewers_of(server, dim, pos);
    let left = {
        let mut st = player.state.lock().unwrap();
        let left = st.inv.add(book).0;
        let p = st.inv.window_items();
        player.send(p);
        left
    };
    if let Some(l) = left {
        entities::throw_from(server, player, l);
    }
    let state = server.world(dim).lock().unwrap().get_block(pos);
    set_block(server, dim, pos, data::get().block_of_state(state).with(state, "has_book", "false"));
    sound::at(server, dim, center(pos), "item.book.page_turn", sound::BLOCK, 1.0, 1.0);
}

// ---------- bell ----------

/// Rings the bell: the swing, the sound, and raiders within 48 blocks start glowing a few
/// seconds later.
pub fn ring(server: &Server, dim: Dimension, pos: BlockPos, face: i32) {
    let mut p = packet(id::play::BLOCK_ACTION);
    p.position(pos).u8(1).u8(face.clamp(0, 5) as u8).varint(data::get().block_index("bell"));
    server.broadcast_dim(dim, &p, None);
    sound::at(server, dim, center(pos), "block.bell.use", sound::BLOCK, 2.0, 1.0);
    let c = center(pos);
    let mut mobs = server.mobs.lock().unwrap();
    for m in mobs.values_mut().filter(|m| m.dim == dim && matches!(m.kind, mobs::MobKind::Pillager | mobs::MobKind::Vindicator)) {
        let d2: f64 = (0..3).map(|i| (m.pos[i] - c[i]).powi(2)).sum();
        if d2 < 48.0 * 48.0 {
            m.glowing = mobs::BELL_GLOW;
        }
    }
}

// ---------- shared ----------

/// Right click on one of these blocks. Returns whether it was handled.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, state: u16, held: &Slot, hand: i32, face: i32, sneaking: bool) -> bool {
    let name = data::get().block_of_state(state).name.as_str();
    // Sneaking with something in hand places it instead of using the block.
    if sneaking && held.is_some() {
        return false;
    }
    match name {
        "composter" => use_composter(server, player, pos, state, held, hand),
        "jukebox" => use_jukebox(server, player, pos, state, held, hand),
        "lectern" => use_lectern(server, player, pos, state, held, hand),
        "bell" => {
            ring(server, player.dim(), pos, face);
            true
        }
        _ => false,
    }
}

/// The block was broken: a jukebox stops playing, a lectern's readers lose the book.
pub fn broken(server: &Server, dim: Dimension, pos: BlockPos, name: &str) {
    match name {
        "jukebox" => world_event(server, dim, pos, 1011, 0),
        "lectern" => containers::close_viewers_of(server, dim, pos),
        _ => {}
    }
}

/// Comparator reading of these blocks (and of containers), if the block gives one.
pub fn analog_signal(world: &mut crate::world::World, pos: BlockPos) -> Option<u8> {
    let reg = data::get();
    let state = world.get_block(pos);
    let b = reg.block_of_state(state);
    let level = |k: &str| b.get(state, k).and_then(|v| v.parse::<i32>().ok()).unwrap_or(0);
    Some(match b.name.as_str() {
        "composter" => level("level") as u8,
        "cake" => ((7 - level("bites")) * 2) as u8,
        "beehive" | "bee_nest" => level("honey_level") as u8,
        "end_portal_frame" => if b.get(state, "eye") == Some("true") { 15 } else { 0 },
        "water_cauldron" | "powder_snow_cauldron" => level("level") as u8,
        "lava_cauldron" => 3,
        "cauldron" => 0,
        "respawn_anchor" => (level("charges") * 15 / 4) as u8,
        "jukebox" => match world.peek_block_entity(pos) {
            Some(BlockEntity::Jukebox(Some(r))) => record_signal(r.name()),
            _ => 0,
        },
        "lectern" => match world.peek_block_entity(pos) {
            Some(BlockEntity::Lectern { book: Some(bk), page }) => page_signal(bk, *page),
            _ => 0,
        },
        n => {
            let items: Vec<Slot> = match world.peek_block_entity(pos) {
                Some(BlockEntity::Container(v)) => v.clone(),
                Some(BlockEntity::Furnace(f)) => f.items.clone(),
                Some(BlockEntity::Brewing(br)) => br.items.clone(),
                _ if BlockEntity::for_block(n).is_some_and(|be| matches!(be, BlockEntity::Container(_) | BlockEntity::Furnace(_) | BlockEntity::Brewing(_))) => vec![None; 27],
                _ => return None,
            };
            // A double chest counts both halves.
            let mut all = items;
            if n == "chest" || n == "trapped_chest" {
                if let Some(other) = containers::chest_partner(world, pos) {
                    if let Some(BlockEntity::Container(v)) = world.peek_block_entity(other) {
                        all.extend(v.iter().cloned());
                    }
                }
            }
            container_signal(&all)
        }
    })
}

/// Vanilla AbstractContainerMenu.getRedstoneSignalFromContainer.
fn container_signal(items: &[Slot]) -> u8 {
    if items.is_empty() {
        return 0;
    }
    let mut fill = 0.0f32;
    let mut any = false;
    for it in items.iter().flatten() {
        fill += it.count as f32 / it.max_stack().min(64) as f32;
        any = true;
    }
    if !any {
        return 0;
    }
    (fill / items.len() as f32 * 14.0).floor() as u8 + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: &str, c: i8) -> Slot {
        Some(ItemStack::new(data::get().item_id(n).unwrap(), c))
    }

    #[test]
    fn comparator_levels() {
        // Vanilla: one item in a chest gives 1, a full chest 15.
        let mut chest = vec![None; 27];
        assert_eq!(container_signal(&chest), 0);
        chest[0] = item("cobblestone", 1);
        assert_eq!(container_signal(&chest), 1);
        let full: Vec<Slot> = (0..27).map(|_| item("cobblestone", 64)).collect();
        assert_eq!(container_signal(&full), 15);
        // Two full stacks in a hopper: 2/5 of it.
        let mut hopper = vec![None; 5];
        hopper[0] = item("cobblestone", 64);
        hopper[1] = item("cobblestone", 64);
        assert_eq!(container_signal(&hopper), 6);
        assert_eq!(record_signal("music_disc_cat"), 2);
        assert_eq!(compost_chance("wheat_seeds"), Some(0.3));
        assert_eq!(compost_chance("pumpkin_pie"), Some(1.0));
        assert_eq!(compost_chance("stone"), None);
    }
}

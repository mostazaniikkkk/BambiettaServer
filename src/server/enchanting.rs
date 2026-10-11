//! Enchanting table: costs based on nearby bookshelves and enchantment selection
//! with the vanilla algorithm (modified power, weights, incompatibilities, extras).

use super::inventory::{Offer, WindowKind};
use super::{experience, Player, Server};
use crate::data;
use crate::gameplay::{self, enchant_category_fits, enchantability};
use crate::item::ItemStack;
use crate::protocol::BlockPos;
use std::sync::Arc;

/// Deterministic PRNG (SplitMix64): the offers shown match what gets applied.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Integer in [0, n).
    fn int(&mut self, n: i32) -> i32 {
        if n <= 0 { 0 } else { (self.next() % n as u64) as i32 }
    }
    fn float(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Bookshelves powering the table (ring 2 blocks away, at its height and one above).
pub fn bookshelves(server: &Server, dim: crate::world::Dimension, pos: BlockPos) -> i32 {
    let mut world = server.world(dim).lock().unwrap();
    let reg = data::get();
    let mut n = 0;
    for dx in -2..=2i32 {
        for dz in -2..=2i32 {
            if dx.abs() < 2 && dz.abs() < 2 {
                continue;
            }
            for dy in 0..=1 {
                let shelf = BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz);
                // The gap between the table and the bookshelf must be clear.
                let gap = BlockPos::new(pos.x + dx / 2, pos.y + dy, pos.z + dz / 2);
                if reg.block_of_state(world.get_block(shelf)).name == "bookshelf" && reg.is_air(world.get_block(gap)) {
                    n += 1;
                }
            }
        }
    }
    n.min(15)
}

/// Level cost of each of the three options.
fn costs(rng: &mut Rng, power: i32, item: &ItemStack) -> [i32; 3] {
    if enchantability(item) <= 0 {
        return [0; 3];
    }
    let mut out = [0; 3];
    for (slot, c) in out.iter_mut().enumerate() {
        let j = rng.int(8) + 1 + (power >> 1) + rng.int(power + 1);
        let cost = match slot {
            0 => (j / 3).max(1),
            1 => j * 2 / 3 + 1,
            _ => j.max(power * 2),
        };
        *c = if cost < slot as i32 + 1 { 0 } else { cost };
    }
    out
}

pub fn compatible(a: &str, b: &str) -> bool {
    let g = gameplay::get();
    a != b && !g.enchantment(a).is_some_and(|e| e.exclude.iter().any(|x| x == b)) && !g.enchantment(b).is_some_and(|e| e.exclude.iter().any(|x| x == a))
}

/// Enchantments for a given cost (vanilla EnchantmentHelper.selectEnchantment).
fn select(rng: &mut Rng, item: &ItemStack, cost: i32, treasure: bool) -> Vec<(String, i32)> {
    let ench = enchantability(item);
    if ench <= 0 {
        return vec![];
    }
    let mut level = cost + 1 + rng.int(ench / 4 + 1) + rng.int(ench / 4 + 1);
    let f = (rng.float() + rng.float() - 1.0) * 0.15;
    level = ((level as f32 + level as f32 * f).round() as i32).max(1);
    let book = item.name() == "book";
    let mut pool: Vec<(String, i32, i32)> = gameplay::get()
        .enchantments
        .iter()
        .filter(|e| (treasure || !e.treasure) && e.discoverable && (book || enchant_category_fits(&e.category, item)))
        .filter_map(|e| {
            (1..=e.max_level).rev().find(|&l| level >= e.min_cost.0 * l + e.min_cost.1 && level <= e.max_cost.0 * l + e.max_cost.1).map(|l| (e.name.clone(), l, e.weight))
        })
        .collect();
    let mut chosen = Vec::new();
    let pick = |rng: &mut Rng, pool: &[(String, i32, i32)]| -> Option<(String, i32)> {
        let total: i32 = pool.iter().map(|p| p.2).sum();
        let mut r = rng.int(total);
        for p in pool {
            r -= p.2;
            if r < 0 {
                return Some((p.0.clone(), p.1));
            }
        }
        None
    };
    if let Some(first) = pick(rng, &pool) {
        chosen.push(first);
        while rng.int(50) <= level {
            pool.retain(|p| chosen.iter().all(|c: &(String, i32)| compatible(&c.0, &p.0)));
            let Some(next) = pick(rng, &pool) else { break };
            chosen.push(next);
            level /= 2;
        }
    }
    // Books lose one at random if several come out.
    if book && chosen.len() > 1 {
        let i = rng.int(chosen.len() as i32) as usize;
        chosen.remove(i);
    }
    chosen
}

/// Offers for `item` with `power` bookshelves and the player's seed.
pub fn offers(item: &ItemStack, power: i32, seed: i32) -> [Option<Offer>; 3] {
    let mut rng = Rng::new(seed as u64);
    let c = costs(&mut rng, power, item);
    std::array::from_fn(|i| {
        if c[i] == 0 {
            return None;
        }
        let mut r = Rng::new(seed as u64 + i as u64 * 7919);
        let enchants = select(&mut r, item, c[i], false);
        (!enchants.is_empty()).then_some(Offer { cost: c[i], enchants })
    })
}

/// Random enchant as if it cost `levels` levels (fishing loot: `enchant_with_levels`).
pub fn enchant_with_levels(item: ItemStack, levels: i32, treasure: bool) -> ItemStack {
    let mut rng = Rng::new((super::entities::random() * u32::MAX as f32) as u64);
    let list = select(&mut rng, &item, levels, treasure);
    let mut out = item;
    if out.name() == "book" && !list.is_empty() {
        out = ItemStack::new(data::get().item_id("enchanted_book").unwrap_or(out.id), 1);
    }
    for (n, l) in list {
        out.enchant(&n, l);
    }
    out
}

pub fn open(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) {
    let packets = player.state.lock().unwrap().inv.open(WindowKind::Enchanting, pos, vec![None; 2], "Encantar");
    for p in packets {
        player.send(p);
    }
    refresh(server, player);
}

/// Recomputes and sends the offers (after each click on the table).
pub fn refresh(server: &Arc<Server>, player: &Arc<Player>) {
    let Some(pos) = player.state.lock().unwrap().inv.window.as_ref().filter(|w| w.kind == WindowKind::Enchanting).map(|w| w.pos) else {
        return;
    };
    let power = bookshelves(server, player.dim(), pos);
    let mut st = player.state.lock().unwrap();
    let seed = st.xp_seed;
    let Some(w) = st.inv.window.as_mut() else { return };
    w.offers = match &w.slots[0] {
        Some(item) if !item.is_enchanted() => offers(item, power, seed),
        _ => [None, None, None],
    };
    let offers = w.offers.clone();
    let reg = gameplay::get();
    let mut props = Vec::new();
    for (i, o) in offers.iter().enumerate() {
        let i = i as i16;
        props.push((i, o.as_ref().map_or(0, |o| o.cost)));
        let hint = o.as_ref().and_then(|o| o.enchants.first());
        props.push((4 + i, hint.and_then(|(n, _)| reg.enchantment(n)).map_or(-1, |e| e.id)));
        props.push((7 + i, hint.map_or(-1, |h| h.1)));
    }
    props.push((3, seed & !15));
    for (k, v) in props {
        let p = st.inv.property_packet(k, v as i16);
        player.send(p);
    }
}

/// The player clicked one of the three options.
pub fn enchant(server: &Arc<Server>, player: &Arc<Player>, button: usize) {
    {
        let mut st = player.state.lock().unwrap();
        let creative = st.game_mode == 1;
        let level = st.xp_level;
        let Some(w) = st.inv.window.as_mut().filter(|w| w.kind == WindowKind::Enchanting) else { return };
        let Some(offer) = w.offers.get(button).cloned().flatten() else { return };
        let lapis = w.slots[1].as_ref().map_or(0, |l| l.count as i32);
        let need = button as i32 + 1;
        if !creative && (lapis < need || level < offer.cost) {
            return;
        }
        let Some(mut item) = w.slots[0].take() else { return };
        if item.name() == "book" {
            item = ItemStack::new(data::get().item_id("enchanted_book").unwrap_or(item.id), 1);
        }
        for (name, lvl) in &offer.enchants {
            item.enchant(name, *lvl);
        }
        w.slots[0] = Some(item);
        if !creative {
            if let Some(l) = &mut w.slots[1] {
                l.count -= need as i8;
                if l.count <= 0 {
                    w.slots[1] = None;
                }
            }
        }
        w.offers = [None, None, None];
        if !creative {
            experience::spend_levels(player, &mut st, need);
        }
        st.xp_seed = (super::entities::random() * i32::MAX as f32) as i32;
        let p = st.inv.window_items();
        player.send(p);
    }
    refresh(server, player);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enchanting_offers() {
        let id = data::get().item_id("diamond_sword").unwrap();
        let sword = ItemStack::new(id, 1);
        let o = offers(&sword, 15, 1234);
        let last = o[2].as_ref().expect("con 15 librerías siempre hay oferta de nivel 30");
        assert_eq!(last.cost, 30);
        assert!(!last.enchants.is_empty());
        // Everything offered applies to a sword and is mutually compatible.
        for (n, _) in &last.enchants {
            let e = gameplay::get().enchantment(n).unwrap();
            assert!(enchant_category_fits(&e.category, &sword) || e.category == "breakable" || e.category == "vanishable", "{n}");
        }
        // Deterministic.
        assert_eq!(format!("{:?}", offers(&sword, 15, 1234)), format!("{o:?}"));
        // A non-enchantable item has no offers.
        let dirt = ItemStack::new(data::get().item_id("dirt").unwrap(), 1);
        assert!(offers(&dirt, 15, 1).iter().all(Option::is_none));
    }
}

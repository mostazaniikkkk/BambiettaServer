//! Authoritative inventory: window clicks with vanilla semantics, 2x2 and 3x3 crafting,
//! container windows (crafting table, enchanting, chests, furnaces) and syncing.
//!
//! Player window indices (id 0): 0 result, 1-4 2x2 grid, 5-8 armor
//! (helmet..boots), 9-35 inventory, 36-44 hotbar, 45 offhand.

use crate::gameplay::{self, Recipe};
use crate::item::{write_slot, ItemStack, Slot};
use crate::protocol::{id, packet, text, BlockPos, Writer};

pub const HOTBAR: usize = 36;
pub const OFFHAND: usize = 45;
/// Menu types (`minecraft:menu` registry) in 1.20.3/1.20.4.
const MENU_GENERIC_9X3: i32 = 2;
const MENU_GENERIC_9X6: i32 = 5;
const MENU_GENERIC_3X3: i32 = 6;
const MENU_BEACON: i32 = 9;
pub const MENU_BLAST_FURNACE: i32 = 10;
const MENU_HOPPER: i32 = 16;
const MENU_SHULKER_BOX: i32 = 20;
const MENU_LECTERN: i32 = 17;
pub const MENU_SMOKER: i32 = 22;
const MENU_CRAFTING: i32 = 12;
const MENU_ENCHANTMENT: i32 = 13;
const MENU_FURNACE: i32 = 14;
const MENU_ANVIL: i32 = 8;
const MENU_BREWING: i32 = 11;
const MENU_GRINDSTONE: i32 = 15;
const MENU_LOOM: i32 = 18;
const MENU_MERCHANT: i32 = 19;
const MENU_SMITHING: i32 = 21;
const MENU_CARTOGRAPHY: i32 = 23;
const MENU_STONECUTTER: i32 = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// 0 result, 1-9 grid.
    Crafting,
    /// 0 item, 1 lapis lazuli.
    Enchanting,
    /// Chest with `rows` rows (contents stored in the block).
    Chest(u8),
    /// 0 input, 1 fuel, 2 output (contents stored in the block).
    Furnace,
    /// 0-2 bottles, 3 ingredient, 4 blaze powder (contents stored in the block).
    Brewing,
    /// 0 item, 1 sacrifice/material, 2 result.
    Anvil,
    /// 0 and 1 items, 2 result.
    Grindstone,
    /// 0 template, 1 base, 2 material, 3 result.
    Smithing,
    /// 0 input, 1 result.
    Stonecutter,
    /// 0 banner, 1 dye, 2 pattern, 3 result.
    Loom,
    /// 0 map, 1 paper/empty map/glass pane, 2 result.
    Cartography,
    /// 0 and 1 payment, 2 result.
    Merchant,
    /// 5 slots (contents stored in the block).
    Hopper,
    /// Dispenser or dropper: 3x3 (contents stored in the block).
    Dispenser,
    /// The book on a lectern (read only; pages are turned with buttons).
    Lectern,
    /// A horse's or llama's inventory: 0 saddle, 1 armor or carpet, then its chest (the
    /// contents belong to the mob; `station.merchant` is its entity id).
    Horse,
    /// The beacon's payment slot.
    Beacon,
    /// A minecart's or boat's chest (false) or a hopper minecart (true): the contents belong
    /// to the vehicle (`station.merchant` is its entity id).
    Cart(bool),
    /// A shulker box (contents stored in the block; no shulker box fits in it).
    Shulker,
    /// An ender chest: the player's own ender inventory.
    Ender,
}

/// Enchanting table offer.
#[derive(Clone, Debug)]
pub struct Offer {
    pub cost: i32,
    pub enchants: Vec<(String, i32)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Loc {
    Player(usize),
    Container(usize),
}

pub struct OpenWindow {
    pub id: u8,
    pub kind: WindowKind,
    pub slots: Vec<Slot>,
    pub pos: BlockPos,
    /// Second half of a double chest (its 27 slots come after those of `pos`).
    pub pos2: Option<BlockPos>,
    pub offers: [Option<Offer>; 3],
    /// Anvil, stonecutter, loom and villager state.
    pub station: super::stations::StationState,
}

impl WindowKind {
    /// Do the contents live in the block (shared between players)?
    pub fn shared(self) -> bool {
        matches!(self, WindowKind::Chest(_) | WindowKind::Furnace | WindowKind::Brewing | WindowKind::Hopper | WindowKind::Dispenser | WindowKind::Shulker)
    }

    /// Workstations whose result is computed by `stations`.
    pub fn station(self) -> bool {
        matches!(
            self,
            WindowKind::Anvil | WindowKind::Grindstone | WindowKind::Smithing | WindowKind::Stonecutter | WindowKind::Loom | WindowKind::Cartography | WindowKind::Merchant
        )
    }

    /// Result slot (nothing can be placed in it).
    pub fn result_slot(self) -> Option<usize> {
        match self {
            WindowKind::Crafting => Some(0),
            WindowKind::Furnace => Some(2),
            WindowKind::Anvil | WindowKind::Grindstone | WindowKind::Cartography | WindowKind::Merchant => Some(2),
            WindowKind::Smithing | WindowKind::Loom => Some(3),
            WindowKind::Stonecutter => Some(1),
            _ => None,
        }
    }
}

pub struct Inventory {
    pub slots: [Slot; 46],
    pub cursor: Slot,
    pub state_id: i32,
    pub window: Option<OpenWindow>,
    next_window: u8,
    drag: Option<(i8, Vec<i16>)>,
    /// Available experience levels and creative mode (for the anvil).
    pub levels: i32,
    pub creative: bool,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { slots: std::array::from_fn(|_| None), cursor: None, state_id: 0, window: None, next_window: 0, drag: None, levels: 0, creative: false }
    }
}

/// What a click produces besides changing the inventory.
#[derive(Default)]
pub struct ClickOutcome {
    /// Items thrown out of the window.
    pub drops: Vec<ItemStack>,
    /// Items taken from the furnace output (to grant their experience).
    pub smelted_taken: i32,
    /// Results taken from a workstation (levels spent, experience, trades...).
    pub taken: Vec<super::stations::Taken>,
}

pub fn armor_slot(item: &ItemStack) -> Option<usize> {
    let n = item.name();
    if n.ends_with("_helmet") || n == "turtle_helmet" || n == "carved_pumpkin" || n.ends_with("_head") || n.ends_with("_skull") {
        Some(5)
    } else if n.ends_with("_chestplate") || n == "elytra" {
        Some(6)
    } else if n.ends_with("_leggings") {
        Some(7)
    } else if n.ends_with("_boots") {
        Some(8)
    } else {
        None
    }
}

/// What remains in the grid after consuming an ingredient.
fn remainder(item: &ItemStack) -> Slot {
    let name = match item.name() {
        "milk_bucket" | "water_bucket" | "lava_bucket" | "powder_snow_bucket" => "bucket",
        "honey_bottle" | "dragon_breath" => "glass_bottle",
        _ => return None,
    };
    crate::data::get().item_id(name).map(|id| ItemStack::new(id, 1))
}

/// Finds the recipe matching grid `grid` of width `w`.
pub fn craft_result(w: usize, grid: &[Slot]) -> Slot {
    let h = grid.len() / w;
    let filled: Vec<(usize, usize)> = (0..grid.len()).filter(|&i| grid[i].is_some()).map(|i| (i % w, i / w)).collect();
    if filled.is_empty() {
        return None;
    }
    let (x0, x1) = (filled.iter().map(|p| p.0).min()?, filled.iter().map(|p| p.0).max()?);
    let (y0, y1) = (filled.iter().map(|p| p.1).min()?, filled.iter().map(|p| p.1).max()?);
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let at = |x: usize, y: usize| grid[(y0 + y) * w + x0 + x].as_ref().map(|s| s.id);
    let items: Vec<i32> = grid.iter().flatten().map(|s| s.id).collect();

    for r in &gameplay::get().recipes {
        match r {
            Recipe::Shaped { w: rw, h: rh, cells, result } if *rw == bw && *rh == bh && bw <= w && bh <= h => {
                let matches = |mirror: bool| {
                    (0..bh).all(|y| {
                        (0..bw).all(|x| {
                            let cx = if mirror { bw - 1 - x } else { x };
                            match (&cells[y * rw + cx], at(x, y)) {
                                (None, None) => true,
                                (Some(opts), Some(id)) => opts.contains(&id),
                                _ => false,
                            }
                        })
                    })
                };
                if matches(false) || matches(true) {
                    return Some(ItemStack::new(result.0, result.1));
                }
            }
            Recipe::Shapeless { ingredients, result } if ingredients.len() == items.len() => {
                // Backtracking matching (at most 9 ingredients).
                fn assign(ings: &[Vec<i32>], items: &[i32], used: &mut [bool]) -> bool {
                    let Some((first, rest)) = ings.split_first() else { return true };
                    for i in 0..items.len() {
                        if !used[i] && first.contains(&items[i]) {
                            used[i] = true;
                            if assign(rest, items, used) {
                                return true;
                            }
                            used[i] = false;
                        }
                    }
                    false
                }
                if assign(ingredients, &items, &mut vec![false; items.len()]) {
                    return Some(ItemStack::new(result.0, result.1));
                }
            }
            _ => {}
        }
    }
    None
}

impl Inventory {
    // ---------- access by location ----------

    fn slot(&self, loc: Loc) -> &Slot {
        match loc {
            Loc::Player(i) => &self.slots[i],
            Loc::Container(i) => &self.window.as_ref().unwrap().slots[i],
        }
    }

    fn slot_mut(&mut self, loc: Loc) -> &mut Slot {
        match loc {
            Loc::Player(i) => &mut self.slots[i],
            Loc::Container(i) => &mut self.window.as_mut().unwrap().slots[i],
        }
    }

    pub fn window_id(&self) -> u8 {
        self.window.as_ref().map_or(0, |w| w.id)
    }

    pub fn window_kind(&self) -> Option<WindowKind> {
        self.window.as_ref().map(|w| w.kind)
    }

    fn container_len(&self) -> usize {
        self.window.as_ref().map_or(0, |w| w.slots.len())
    }

    /// Player window: 46 slots; containers: their slots + 36 of the player's.
    fn window_len(&self) -> usize {
        match &self.window {
            None => 46,
            Some(w) => w.slots.len() + 36,
        }
    }

    /// Translates an index of the open window to a location.
    pub fn loc(&self, slot: i16) -> Option<Loc> {
        if slot < 0 || slot as usize >= self.window_len() {
            return None;
        }
        let s = slot as usize;
        let n = self.container_len();
        Some(match self.window {
            None => Loc::Player(s),
            Some(_) if s < n => Loc::Container(s),
            Some(_) => Loc::Player(s - n + 9),
        })
    }

    /// Index in the open window of a player slot (if visible in it).
    fn window_index(&self, player_slot: usize) -> Option<i16> {
        match self.window {
            None => Some(player_slot as i16),
            Some(_) if (9..=44).contains(&player_slot) => Some((self.container_len() + player_slot - 9) as i16),
            Some(_) => None,
        }
    }

    /// Result slot: nothing can be placed in it.
    fn is_result(&self, loc: Loc) -> bool {
        match (loc, self.window_kind()) {
            (Loc::Player(0), None) => true,
            (Loc::Container(i), Some(k)) => k.result_slot() == Some(i),
            _ => false,
        }
    }

    fn is_station_result(&self, loc: Loc) -> bool {
        self.window_kind().is_some_and(WindowKind::station) && self.is_result(loc)
    }

    fn has_grid(&self) -> bool {
        matches!(self.window_kind(), None | Some(WindowKind::Crafting))
    }

    fn grid_locs(&self) -> Vec<Loc> {
        match self.window_kind() {
            None => (1..=4).map(Loc::Player).collect(),
            Some(WindowKind::Crafting) => (1..=9).map(Loc::Container).collect(),
            _ => vec![],
        }
    }

    fn accepts(&self, loc: Loc, item: &ItemStack) -> bool {
        if self.is_result(loc) {
            return false;
        }
        match (loc, self.window_kind()) {
            (Loc::Player(i @ 5..=8), _) => armor_slot(item) == Some(i),
            (Loc::Container(1), Some(WindowKind::Furnace)) => gameplay::fuel_ticks(item.name()) > 0 || item.name() == "bucket",
            (Loc::Container(0), Some(WindowKind::Horse)) => item.name() == "saddle",
            (Loc::Container(0), Some(WindowKind::Beacon)) => super::beacons::is_payment(item.name()),
            (Loc::Container(1), Some(WindowKind::Horse)) => item.name().ends_with("_horse_armor") || item.name().ends_with("_carpet"),
            (Loc::Container(0), Some(WindowKind::Enchanting)) => {
                (gameplay::enchantability(item) > 0 && !item.is_enchanted()) || item.name() == "book"
            }
            (Loc::Container(1), Some(WindowKind::Enchanting)) => item.name() == "lapis_lazuli",
            (Loc::Container(_), Some(WindowKind::Shulker)) => !item.name().ends_with("shulker_box"),
            (Loc::Container(i), Some(k)) if k == WindowKind::Brewing || k.station() => super::stations::accepts(k, i, item),
            _ => true,
        }
    }

    fn max_in(&self, loc: Loc, item: &ItemStack) -> i8 {
        match (loc, self.window_kind()) {
            (Loc::Player(5..=8), _) | (Loc::Container(0), Some(WindowKind::Enchanting)) | (Loc::Container(0..=2), Some(WindowKind::Brewing)) | (Loc::Container(0 | 1), Some(WindowKind::Horse)) | (Loc::Container(0), Some(WindowKind::Beacon)) => 1,
            _ => item.max_stack(),
        }
    }

    // ---------- crafting ----------

    /// Recomputes the result of the active grid.
    fn update_result(&mut self) {
        if self.window_kind().is_some_and(WindowKind::station) {
            let (levels, creative) = (self.levels, self.creative);
            super::stations::compute(self.window.as_mut().unwrap(), levels, creative);
            return;
        }
        if !self.has_grid() {
            return;
        }
        let grid: Vec<Slot> = self.grid_locs().iter().map(|&l| self.slot(l).clone()).collect();
        let w = if self.window.is_some() { 3 } else { 2 };
        let result = craft_result(w, &grid);
        let rloc = if self.window.is_some() { Loc::Container(0) } else { Loc::Player(0) };
        *self.slot_mut(rloc) = result;
    }

    /// Consumes one ingredient from each cell after taking the result.
    fn consume_grid(&mut self) {
        for l in self.grid_locs() {
            let slot = self.slot_mut(l);
            if let Some(s) = slot {
                let rem = remainder(s);
                s.count -= 1;
                if s.count <= 0 {
                    *slot = rem;
                }
            }
        }
        self.update_result();
    }

    // ---------- add/remove ----------

    /// Adds to the inventory like vanilla (hotbar first). Returns what didn't fit
    /// and the modified player slots.
    pub fn add(&mut self, item: ItemStack) -> (Option<ItemStack>, Vec<usize>) {
        self.add_in((HOTBAR..HOTBAR + 9).chain(9..36).collect(), item)
    }

    /// Like `add` but from the end (hotbar right to left, then the
    /// inventory bottom to top): this is how vanilla inserts a shift-click result.
    pub fn add_reverse(&mut self, item: ItemStack) -> (Option<ItemStack>, Vec<usize>) {
        self.add_in((HOTBAR..HOTBAR + 9).rev().chain((9..36).rev()).collect(), item)
    }

    fn add_in(&mut self, order: Vec<usize>, mut item: ItemStack) -> (Option<ItemStack>, Vec<usize>) {
        let mut changed = Vec::new();
        for pass in 0..2 {
            for &i in &order {
                if item.count <= 0 {
                    break;
                }
                let max = item.max_stack();
                match &mut self.slots[i] {
                    Some(s) if pass == 0 && s.stacks_with(&item) && s.count < max => {
                        let n = item.count.min(max - s.count);
                        s.count += n;
                        item.count -= n;
                        changed.push(i);
                    }
                    slot @ None if pass == 1 => {
                        let n = item.count.min(max);
                        *slot = Some(item.with_count(n));
                        item.count -= n;
                        changed.push(i);
                    }
                    _ => {}
                }
            }
        }
        ((item.count > 0).then_some(item), changed)
    }

    /// Moves `loc` into the `targets` locations (stacking first, then into empty slots).
    fn move_to(&mut self, loc: Loc, targets: &[Loc]) {
        let Some(mut item) = self.slot(loc).clone() else { return };
        for pass in 0..2 {
            for &t in targets {
                if item.count <= 0 || t == loc || !self.accepts(t, &item) {
                    continue;
                }
                let max = self.max_in(t, &item);
                let dst = self.slot_mut(t);
                match dst {
                    Some(s) if pass == 0 && s.stacks_with(&item) && s.count < max => {
                        let n = item.count.min(max - s.count);
                        s.count += n;
                        item.count -= n;
                    }
                    None if pass == 1 => {
                        let n = item.count.min(max);
                        *dst = Some(item.with_count(n));
                        item.count -= n;
                    }
                    _ => {}
                }
            }
        }
        *self.slot_mut(loc) = (item.count > 0).then_some(item);
    }

    fn shift_targets(&self, loc: Loc) -> Vec<Loc> {
        let p = |r: std::ops::Range<usize>| r.map(Loc::Player).collect::<Vec<_>>();
        let item = self.slot(loc).clone();
        let armor = item.as_ref().and_then(armor_slot).filter(|&a| self.slots[a].is_none());
        // From the player into the open container.
        if let (Loc::Player(_), Some(kind), Some(it)) = (loc, self.window_kind(), &item) {
            let to = |v: Vec<usize>| v.into_iter().map(Loc::Container).collect::<Vec<_>>();
            match kind {
                WindowKind::Chest(rows) => return to((0..rows as usize * 9).collect()),
                WindowKind::Hopper | WindowKind::Cart(true) => return to((0..5).collect()),
                WindowKind::Shulker if it.name().ends_with("shulker_box") => return vec![],
                WindowKind::Shulker | WindowKind::Ender | WindowKind::Cart(false) => return to((0..27).collect()),
                WindowKind::Beacon if super::beacons::is_payment(it.name()) => return to(vec![0]),
                WindowKind::Horse if it.name() == "saddle" => return to(vec![0]),
                WindowKind::Horse if self.accepts(Loc::Container(1), it) => return to(vec![1]),
                WindowKind::Horse => return to((2..self.container_len()).collect()),
                WindowKind::Dispenser => return to((0..9).collect()),
                WindowKind::Furnace if gameplay::smelt(it.name()).is_some() => return to(vec![0]),
                WindowKind::Furnace if self.accepts(Loc::Container(1), it) => return to(vec![1]),
                WindowKind::Enchanting if it.name() == "lapis_lazuli" => return to(vec![1]),
                WindowKind::Enchanting if self.accepts(Loc::Container(0), it) => return to(vec![0]),
                k if k == WindowKind::Brewing || k.station() => {
                    let t = super::stations::shift_targets(k, it);
                    if !t.is_empty() {
                        return to(t);
                    }
                }
                _ => {}
            }
        }
        match (self.window.is_some(), loc) {
            // From the container to the player: from the hotbar backwards, like vanilla.
            (_, Loc::Container(_)) => (9..45).rev().map(Loc::Player).collect(),
            (false, Loc::Player(1..=8)) | (false, Loc::Player(OFFHAND)) => p(9..45),
            (_, Loc::Player(i)) if (9..36).contains(&i) => match armor {
                Some(a) if self.window.is_none() => vec![Loc::Player(a)],
                _ => p(HOTBAR..HOTBAR + 9),
            },
            (_, Loc::Player(i)) if (HOTBAR..HOTBAR + 9).contains(&i) => match armor {
                Some(a) if self.window.is_none() => vec![Loc::Player(a)],
                _ => p(9..36),
            },
            _ => vec![],
        }
    }

    // ---------- clicks ----------

    /// Applies a `window_click` with vanilla semantics.
    pub fn click(&mut self, creative: bool, slot: i16, button: i8, mode: i32) -> ClickOutcome {
        let mut out = ClickOutcome::default();
        let loc = self.loc(slot);
        match mode {
            // Normal click (left/right) or outside the window.
            0 => match loc {
                None if slot == -999 => {
                    if let Some(c) = self.cursor.take() {
                        if button == 1 && c.count > 1 {
                            out.drops.push(c.with_count(1));
                            self.cursor = Some(c.with_count(c.count - 1));
                        } else {
                            out.drops.push(c);
                        }
                    }
                }
                Some(l @ Loc::Container(2)) if self.window_kind() == Some(WindowKind::Furnace) => {
                    let before = self.slot(l).as_ref().map_or(0, |s| s.count) as i32;
                    self.take_output(l);
                    out.smelted_taken += before - self.slot(l).as_ref().map_or(0, |s| s.count) as i32;
                }
                Some(l) if self.is_station_result(l) => {
                    self.take_station(l, false, &mut out);
                }
                Some(l) if self.is_result(l) => self.take_result(),
                Some(l) => self.click_slot(l, button == 1),
                None => {}
            },
            // Shift-click
            1 => match loc {
                Some(l @ Loc::Container(2)) if self.window_kind() == Some(WindowKind::Furnace) => {
                    let before = self.slot(l).as_ref().map_or(0, |s| s.count) as i32;
                    let targets = self.shift_targets(l);
                    self.move_to(l, &targets);
                    out.smelted_taken += before - self.slot(l).as_ref().map_or(0, |s| s.count) as i32;
                }
                Some(l) if self.is_station_result(l) => {
                    for _ in 0..64 {
                        if !self.take_station(l, true, &mut out) {
                            break;
                        }
                    }
                }
                Some(l) if self.is_result(l) => {
                    for _ in 0..64 {
                        let Some(r) = self.slot(l).clone() else { break };
                        let mut probe = Inventory { slots: self.slots.clone(), ..Default::default() };
                        if probe.add(r.clone()).0.is_some() {
                            break;
                        }
                        self.add_reverse(r);
                        self.consume_grid();
                    }
                }
                Some(l) => {
                    let targets = self.shift_targets(l);
                    self.move_to(l, &targets);
                }
                None => {}
            },
            // Number key (0-8) or F (40): swap with the hotbar/offhand.
            2 => {
                let hot = match button {
                    0..=8 => Loc::Player(HOTBAR + button as usize),
                    40 => Loc::Player(OFFHAND),
                    _ => return out,
                };
                let Some(l) = loc else { return out };
                if self.is_station_result(l) {
                    return out;
                }
                if self.is_result(l) {
                    if self.slot(hot).is_none() {
                        if let Some(r) = self.slot(l).clone() {
                            *self.slot_mut(hot) = Some(r);
                            self.consume_grid();
                        }
                    }
                } else {
                    let (a, b) = (self.slot(l).clone(), self.slot(hot).clone());
                    let ok_a = b.as_ref().is_none_or(|i| self.accepts(l, i));
                    let ok_b = a.as_ref().is_none_or(|i| self.accepts(hot, i));
                    if ok_a && ok_b {
                        *self.slot_mut(l) = b;
                        *self.slot_mut(hot) = a;
                    }
                }
            }
            // Middle click: clone (creative only).
            3 => {
                if let (true, None, Some(l)) = (creative, &self.cursor, loc) {
                    if let Some(s) = self.slot(l) {
                        self.cursor = Some(s.with_count(s.max_stack()));
                    }
                }
            }
            // Q / Ctrl+Q over a slot.
            4 => {
                let Some(l) = loc else { return out };
                if self.is_station_result(l) {
                    return out;
                }
                if self.is_result(l) {
                    if let Some(r) = self.slot(l).clone() {
                        out.drops.push(r);
                        self.consume_grid();
                    }
                } else if let Some(s) = self.slot_mut(l).take() {
                    if button == 0 && s.count > 1 {
                        out.drops.push(s.with_count(1));
                        *self.slot_mut(l) = Some(s.with_count(s.count - 1));
                    } else {
                        out.drops.push(s);
                    }
                }
            }
            // Drag: start (0/4/8), add slot (1/5/9), end (2/6/10).
            5 => match button {
                0 | 4 | 8 => self.drag = Some((button / 4, Vec::new())),
                1 | 5 | 9 => {
                    if let Some((_, slots)) = &mut self.drag {
                        if !slots.contains(&slot) {
                            slots.push(slot);
                        }
                    }
                }
                2 | 6 | 10 => {
                    if let Some((kind, slots)) = self.drag.take() {
                        self.finish_drag(creative, kind, &slots);
                    }
                }
                _ => {}
            },
            // Double click: gather into the cursor.
            6 => {
                let Some(mut c) = self.cursor.clone() else { return out };
                let max = c.max_stack();
                let locs: Vec<Loc> = (0..self.window_len() as i16).filter_map(|s| self.loc(s)).filter(|&l| !self.is_result(l)).collect();
                for pass in 0..2 {
                    for &l in &locs {
                        if c.count >= max {
                            break;
                        }
                        if let Some(s) = self.slot_mut(l) {
                            if s.stacks_with(&c) && (pass == 1 || s.count < s.max_stack()) {
                                let n = s.count.min(max - c.count);
                                c.count += n;
                                s.count -= n;
                                if s.count <= 0 {
                                    *self.slot_mut(l) = None;
                                }
                            }
                        }
                    }
                }
                self.cursor = Some(c);
            }
            _ => {}
        }
        self.update_result();
        out
    }

    fn click_slot(&mut self, l: Loc, right: bool) {
        let cur = self.cursor.take();
        let slot = self.slot(l).clone();
        let (new_cursor, new_slot) = match (cur, slot) {
            (None, None) => (None, None),
            (None, Some(s)) if right => {
                let take = (s.count + 1) / 2;
                (Some(s.with_count(take)), (s.count - take > 0).then(|| s.with_count(s.count - take)))
            }
            (None, Some(s)) => (Some(s), None),
            (Some(c), None) => {
                if !self.accepts(l, &c) {
                    (Some(c), None)
                } else {
                    let n = if right { 1 } else { c.count.min(self.max_in(l, &c)) };
                    ((c.count - n > 0).then(|| c.with_count(c.count - n)), Some(c.with_count(n)))
                }
            }
            (Some(c), Some(s)) if c.stacks_with(&s) => {
                let max = self.max_in(l, &s);
                let n = if right { 1.min(max - s.count) } else { c.count.min(max - s.count) }.max(0);
                ((c.count - n > 0).then(|| c.with_count(c.count - n)), Some(s.with_count(s.count + n)))
            }
            (Some(c), Some(s)) => {
                if self.accepts(l, &c) && c.count <= self.max_in(l, &c) {
                    (Some(s), Some(c))
                } else {
                    (Some(c), Some(s))
                }
            }
        };
        self.cursor = new_cursor;
        *self.slot_mut(l) = new_slot;
    }

    /// Take from the furnace output: like a normal click but nothing can be placed.
    fn take_output(&mut self, l: Loc) {
        let Some(s) = self.slot(l).clone() else { return };
        match &mut self.cursor {
            None => {
                self.cursor = Some(s);
                *self.slot_mut(l) = None;
            }
            Some(c) if c.stacks_with(&s) => {
                let n = s.count.min(c.max_stack() - c.count).max(0);
                c.count += n;
                *self.slot_mut(l) = (s.count > n).then(|| s.with_count(s.count - n));
            }
            _ => {}
        }
    }

    fn take_result(&mut self) {
        let rloc = if self.window.is_some() { Loc::Container(0) } else { Loc::Player(0) };
        let Some(r) = self.slot(rloc).clone() else { return };
        match &mut self.cursor {
            None => self.cursor = Some(r),
            Some(c) if c.stacks_with(&r) && c.count + r.count <= c.max_stack() => c.count += r.count,
            _ => return,
        }
        self.consume_grid();
    }

    /// Takes the result from a workstation (to the cursor or, with shift, to the inventory).
    /// Returns whether it succeeded.
    fn take_station(&mut self, l: Loc, shift: bool, out: &mut ClickOutcome) -> bool {
        let Some(r) = self.slot(l).clone() else { return false };
        let (levels, creative) = (self.levels, self.creative);
        if !super::stations::can_take(self.window.as_ref().unwrap(), levels, creative) {
            return false;
        }
        if shift {
            let mut probe = Inventory { slots: self.slots.clone(), ..Default::default() };
            if probe.add(r.clone()).0.is_some() {
                return false;
            }
            self.add_reverse(r.clone());
        } else {
            match &mut self.cursor {
                None => self.cursor = Some(r.clone()),
                Some(c) if c.stacks_with(&r) && c.count + r.count <= c.max_stack() => c.count += r.count,
                _ => return false,
            }
        }
        let taken = super::stations::consume(self.window.as_mut().unwrap(), &r, creative);
        self.levels -= taken.levels;
        out.taken.push(taken);
        self.update_result();
        true
    }

    fn finish_drag(&mut self, creative: bool, kind: i8, slots: &[i16]) {
        let Some(c) = self.cursor.clone() else { return };
        let locs: Vec<Loc> = slots
            .iter()
            .filter_map(|&s| self.loc(s))
            .filter(|&l| self.accepts(l, &c) && self.slot(l).as_ref().is_none_or(|s| s.stacks_with(&c)))
            .collect();
        if locs.is_empty() {
            return;
        }
        let mut remaining = c.count;
        match kind {
            // Left: split evenly.
            0 => {
                let per = (c.count as usize / locs.len()) as i8;
                if per == 0 {
                    return;
                }
                for l in locs {
                    let max = self.max_in(l, &c);
                    let have = self.slot(l).as_ref().map_or(0, |s| s.count);
                    let n = per.min(max - have).max(0);
                    remaining -= n;
                    *self.slot_mut(l) = Some(c.with_count(have + n));
                }
            }
            // Right: one in each slot.
            1 => {
                for l in locs {
                    if remaining == 0 {
                        break;
                    }
                    let have = self.slot(l).as_ref().map_or(0, |s| s.count);
                    if have < self.max_in(l, &c) {
                        remaining -= 1;
                        *self.slot_mut(l) = Some(c.with_count(have + 1));
                    }
                }
            }
            // Middle (creative): full stack in each slot.
            _ if creative => {
                for l in locs {
                    *self.slot_mut(l) = Some(c.with_count(c.max_stack()));
                }
                return;
            }
            _ => return,
        }
        self.cursor = (remaining > 0).then(|| c.with_count(remaining));
    }

    // ---------- windows ----------

    /// Opens a container window with its initial contents.
    pub fn open(&mut self, kind: WindowKind, pos: BlockPos, slots: Vec<Slot>, title: &str) -> Vec<Vec<u8>> {
        self.open_menu(kind, None, pos, slots, title)
    }

    /// Like `open`, with another menu of the same layout (smoker, blast furnace).
    pub fn open_menu(&mut self, kind: WindowKind, menu: Option<i32>, pos: BlockPos, slots: Vec<Slot>, title: &str) -> Vec<Vec<u8>> {
        self.next_window = self.next_window % 100 + 1;
        let id = self.next_window;
        self.window = Some(OpenWindow { id, kind, slots, pos, pos2: None, offers: [None, None, None], station: Default::default() });
        let menu = menu.unwrap_or(match kind {
            WindowKind::Crafting => MENU_CRAFTING,
            WindowKind::Enchanting => MENU_ENCHANTMENT,
            WindowKind::Furnace => MENU_FURNACE,
            WindowKind::Chest(6) => MENU_GENERIC_9X6,
            WindowKind::Chest(_) => MENU_GENERIC_9X3,
            WindowKind::Brewing => MENU_BREWING,
            WindowKind::Anvil => MENU_ANVIL,
            WindowKind::Grindstone => MENU_GRINDSTONE,
            WindowKind::Smithing => MENU_SMITHING,
            WindowKind::Stonecutter => MENU_STONECUTTER,
            WindowKind::Loom => MENU_LOOM,
            WindowKind::Cartography => MENU_CARTOGRAPHY,
            WindowKind::Merchant => MENU_MERCHANT,
            WindowKind::Hopper => MENU_HOPPER,
            WindowKind::Dispenser => MENU_GENERIC_3X3,
            WindowKind::Lectern => MENU_LECTERN,
            WindowKind::Horse => MENU_GENERIC_9X3,
            WindowKind::Beacon => MENU_BEACON,
            WindowKind::Cart(true) => MENU_HOPPER,
            WindowKind::Cart(false) => MENU_GENERIC_9X3,
            WindowKind::Shulker => MENU_SHULKER_BOX,
            WindowKind::Ender => MENU_GENERIC_9X3,
        });
        let mut p = packet(id::play::OPEN_WINDOW);
        p.varint(id as i32).varint(menu);
        text(title).write_network(&mut p);
        vec![p, self.window_items()]
    }

    /// A horse's inventory (its own packet: window, number of slots and the horse).
    pub fn open_horse(&mut self, eid: i32, slots: Vec<Slot>) -> Vec<Vec<u8>> {
        self.next_window = self.next_window % 100 + 1;
        let id = self.next_window;
        let n = slots.len() as i32;
        let mut station = super::stations::StationState::default();
        station.merchant = eid;
        self.window = Some(OpenWindow { id, kind: WindowKind::Horse, slots, pos: BlockPos::new(0, 0, 0), pos2: None, offers: [None, None, None], station });
        let mut p = packet(id::play::OPEN_HORSE_WINDOW);
        p.u8(id).varint(n).i32(eid);
        vec![p, self.window_items()]
    }

    /// A vehicle's chest or hopper.
    pub fn open_cart(&mut self, eid: i32, hopper: bool, slots: Vec<Slot>, title: &str) -> Vec<Vec<u8>> {
        let out = self.open(WindowKind::Cart(hopper), BlockPos::new(0, 0, 0), slots, title);
        if let Some(w) = self.window.as_mut() {
            w.station.merchant = eid;
        }
        out
    }

    pub fn open_crafting_table(&mut self, pos: BlockPos) -> Vec<Vec<u8>> {
        self.open(WindowKind::Crafting, pos, vec![None; 10], "Mesa de crafteo")
    }

    /// Closes the window: returns grid, enchanting table and cursor to the inventory.
    /// Returns what doesn't fit (to drop it).
    pub fn close(&mut self) -> Vec<ItemStack> {
        let mut back: Vec<ItemStack> = Vec::new();
        // Whatever was put in tables and workstations goes back to the player (except the result).
        if let Some(w) = self.window.as_mut().filter(|w| !w.kind.shared() && !matches!(w.kind, WindowKind::Lectern | WindowKind::Horse | WindowKind::Cart(_) | WindowKind::Ender)) {
            let result = w.kind.result_slot();
            back.extend(w.slots.iter_mut().enumerate().filter(|(i, _)| Some(*i) != result).filter_map(|(_, s)| s.take()));
        }
        back.extend(self.cursor.take());
        self.window = None;
        // The 2x2 grid is also emptied when closing the inventory.
        back.extend((1..=4).filter_map(|i| self.slots[i].take()));
        self.slots[0] = None;
        back.into_iter().filter_map(|i| self.add(i).0).collect()
    }

    // ---------- packets ----------

    pub fn window_items(&mut self) -> Vec<u8> {
        self.state_id = self.state_id.wrapping_add(1) & 0x7fff;
        let mut p = packet(id::play::WINDOW_ITEMS);
        p.u8(self.window_id()).varint(self.state_id).varint(self.window_len() as i32);
        for s in 0..self.window_len() as i16 {
            write_slot(&mut p, self.slot(self.loc(s).unwrap()));
        }
        write_slot(&mut p, &self.cursor);
        p
    }

    /// `set_slot` of a player slot for whatever window they have open.
    pub fn slot_packet(&mut self, player_slot: usize) -> Option<Vec<u8>> {
        let idx = self.window_index(player_slot)?;
        self.state_id = self.state_id.wrapping_add(1) & 0x7fff;
        let mut p = packet(id::play::SET_SLOT);
        p.i8(self.window_id() as i8).varint(self.state_id).i16(idx);
        write_slot(&mut p, &self.slots[player_slot]);
        Some(p)
    }

    /// `set_slot` of the active grid's result.
    pub fn result_packet(&mut self) -> Option<Vec<u8>> {
        let (rloc, idx) = match self.window_kind() {
            None => (Loc::Player(0), 0),
            Some(k) if k == WindowKind::Crafting || k.station() => (Loc::Container(k.result_slot()?), k.result_slot()? as i16),
            _ => return None,
        };
        let item = self.slot(rloc).clone();
        self.state_id = self.state_id.wrapping_add(1) & 0x7fff;
        let mut p = packet(id::play::SET_SLOT);
        p.i8(self.window_id() as i8).varint(self.state_id).i16(idx);
        write_slot(&mut p, &item);
        Some(p)
    }

    /// All slots of the open window (to see what a click changed).
    pub fn snapshot(&self) -> Vec<Slot> {
        (0..self.window_len() as i16).map(|s| self.slot_at(s)).collect()
    }

    /// `set_slot` of an index of the open window.
    pub fn window_slot_packet(&mut self, idx: i16) -> Vec<u8> {
        self.state_id = self.state_id.wrapping_add(1) & 0x7fff;
        let mut p = packet(id::play::SET_SLOT);
        p.i8(self.window_id() as i8).varint(self.state_id).i16(idx);
        write_slot(&mut p, &self.slot_at(idx));
        p
    }

    /// Contents of an index of the open window.
    pub fn slot_at(&self, s: i16) -> Slot {
        self.loc(s).and_then(|l| self.slot(l).clone())
    }

    /// Does what the client predicted match the server state?
    pub fn matches_prediction(&self, changed: &[(i16, Slot)], cursor: &Slot) -> bool {
        let same = |a: &Slot, b: &Slot| match (a, b) {
            (None, None) => true,
            (Some(x), Some(y)) => x.id == y.id && x.count == y.count,
            _ => false,
        };
        same(&self.cursor, cursor) && changed.iter().all(|(s, item)| self.loc(*s).is_some_and(|l| self.is_result(l) || same(self.slot(l), item)))
    }

    pub fn held(&self, hand: i32, held_slot: usize) -> &Slot {
        &self.slots[if hand == 1 { OFFHAND } else { HOTBAR + held_slot }]
    }

    /// Subtracts `n` from the held item; returns the affected player slot.
    pub fn consume_held(&mut self, hand: i32, held_slot: usize, n: i8, replace_with: Slot) -> usize {
        let i = if hand == 1 { OFFHAND } else { HOTBAR + held_slot };
        if let Some(s) = &mut self.slots[i] {
            s.count -= n;
            if s.count <= 0 {
                self.slots[i] = replace_with;
            } else if let Some(r) = replace_with {
                let _ = self.add(r);
            }
        }
        i
    }

    /// Empties everything (on death) and returns the items.
    pub fn drain(&mut self) -> Vec<ItemStack> {
        let mut out: Vec<ItemStack> = self.slots.iter_mut().filter_map(Option::take).collect();
        out.extend(self.cursor.take());
        // Chest and furnace contents belong to the block, not the player.
        if let Some(w) = self.window.as_mut().filter(|w| !w.kind.shared() && !matches!(w.kind, WindowKind::Lectern | WindowKind::Horse | WindowKind::Cart(_) | WindowKind::Ender)) {
            let result = w.kind.result_slot();
            out.extend(w.slots.iter_mut().enumerate().filter(|(i, _)| Some(*i) != result).filter_map(|(_, s)| s.take()));
            w.slots.iter_mut().for_each(|s| *s = None);
        }
        out
    }

    /// Window property (`craft_progress_bar`): furnace progress, enchanting offers...
    pub fn property_packet(&self, property: i16, value: i16) -> Vec<u8> {
        let mut p = packet(id::play::WINDOW_PROPERTY);
        p.u8(self.window_id()).i16(property).i16(value);
        p
    }

    /// Replaces the contents of the open shared window (another player or the furnace changed it).
    pub fn refresh_shared(&mut self, pos: BlockPos, slots: &[Slot]) -> Option<Vec<u8>> {
        let w = self.window.as_mut().filter(|w| w.kind.shared() && w.pos == pos)?;
        if w.slots == slots {
            return None;
        }
        w.slots = slots.to_vec();
        Some(self.window_items())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;

    fn item(name: &str, n: i8) -> Slot {
        Some(ItemStack::new(data::get().item_id(name).unwrap(), n))
    }

    #[test]
    fn crafting() {
        // Planks from any log (1-ingredient shapeless)
        let r = craft_result(2, &[None, item("birch_log", 1), None, None]).unwrap();
        assert_eq!((r.name(), r.count), ("birch_planks", 4));
        // Crafting table with mixed planks (merged tag)
        let r = craft_result(2, &[item("oak_planks", 1), item("birch_planks", 1), item("spruce_planks", 1), item("oak_planks", 1)]).unwrap();
        assert_eq!(r.name(), "crafting_table");
        // Sticks in any column of a 3x3
        let r = craft_result(3, &[None, None, item("oak_planks", 1), None, None, item("oak_planks", 1), None, None, None]).unwrap();
        assert_eq!((r.name(), r.count), ("stick", 4));
        // Wooden pickaxe
        let p = item("oak_planks", 1);
        let s = item("stick", 1);
        let r = craft_result(3, &[p.clone(), p.clone(), p, None, s.clone(), None, None, s, None]).unwrap();
        assert_eq!(r.name(), "wooden_pickaxe");
        assert!(craft_result(3, &[item("dirt", 1), None, None, None, None, None, None, None, None]).is_none());
    }

    #[test]
    fn clicks() {
        let mut inv = Inventory::default();
        inv.slots[36] = item("dirt", 10);
        inv.click(false, 36, 1, 0); // right: picks up half
        assert_eq!(inv.cursor.as_ref().unwrap().count, 5);
        inv.click(false, 9, 0, 0); // left: drops everything
        assert_eq!(inv.slots[9].as_ref().unwrap().count, 5);
        assert!(inv.cursor.is_none());
        inv.click(false, 9, 0, 1); // shift: to the hotbar, stacks with the other 5
        assert_eq!(inv.slots[36].as_ref().unwrap().count, 10);
        // Crafting in the 2x2 grid
        inv.slots[1] = item("oak_log", 2);
        inv.click(false, 1, 0, 0);
        inv.click(false, 1, 0, 0); // picks up and puts back down
        assert_eq!(inv.slots[0].as_ref().unwrap().name(), "oak_planks");
        inv.click(false, 0, 0, 1); // shift on the result: crafts everything
        assert!(inv.slots[1].is_none());
        let planks: i32 = inv.slots.iter().flatten().filter(|s| s.name() == "oak_planks").map(|s| s.count as i32).sum();
        assert_eq!(planks, 8);
        // Armor only goes into its own slot
        inv.cursor = item("iron_helmet", 1);
        inv.click(false, 8, 0, 0);
        assert!(inv.slots[8].is_none());
        inv.click(false, 5, 0, 0);
        assert_eq!(inv.slots[5].as_ref().unwrap().name(), "iron_helmet");
    }
}

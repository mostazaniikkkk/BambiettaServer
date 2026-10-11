//! Workstation blocks: anvil, grindstone, smithing table, stonecutter, loom, cartography
//! table and villager trading. They compute the result like vanilla from the
//! inputs; the contents belong to the player (returned to the inventory on close).

use super::inventory::{OpenWindow, WindowKind};
use super::{entities, experience, Player, Server};
use crate::data;
use crate::gameplay;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

static STATIONS_JSON: &str = include_str!("../../data/stations.json");

#[derive(Deserialize)]
struct Raw {
    stonecutting: Vec<(String, String, i8)>,
    smithing: Vec<(String, String, String, String)>,
    #[serde(rename = "trimPatterns")]
    trim_patterns: HashMap<String, String>,
    #[serde(rename = "trimMaterials")]
    trim_materials: HashMap<String, String>,
    trimmable: Vec<String>,
    banner: RawBanner,
    barter: Vec<(String, i32, i32, i32, Option<String>, Option<String>)>,
}

#[derive(Deserialize)]
struct RawBanner {
    none: Vec<String>,
    items: HashMap<String, Vec<String>>,
}

pub struct Stations {
    /// input -> recipes (result, count), already in the order the client lists them.
    pub stonecutting: HashMap<String, Vec<(String, i8)>>,
    pub smithing: Vec<(String, String, String, String)>,
    pub trim_patterns: HashMap<String, String>,
    pub trim_materials: HashMap<String, String>,
    pub trimmable: Vec<String>,
    pub banner_none: Vec<String>,
    pub banner_items: HashMap<String, Vec<String>>,
    /// (item, weight, min, max, potion, enchantment)
    pub barter: Vec<(String, i32, i32, i32, Option<String>, Option<String>)>,
}

pub fn get() -> &'static Stations {
    static S: OnceLock<Stations> = OnceLock::new();
    S.get_or_init(|| {
        let raw: Raw = serde_json::from_str(STATIONS_JSON).expect("stations.json inválido");
        let mut stonecutting: HashMap<String, Vec<(String, i8)>> = HashMap::new();
        for (input, result, count) in raw.stonecutting {
            stonecutting.entry(input).or_default().push((result, count));
        }
        // The client sorts by the result's translation key (block.minecraft.x / item.minecraft.x).
        for list in stonecutting.values_mut() {
            list.sort_by_key(|(r, _)| translation_key(r));
        }
        Stations {
            stonecutting,
            smithing: raw.smithing,
            trim_patterns: raw.trim_patterns,
            trim_materials: raw.trim_materials,
            trimmable: raw.trimmable,
            banner_none: raw.banner.none,
            banner_items: raw.banner.items,
            barter: raw.barter,
        }
    })
}

fn translation_key(item: &str) -> String {
    let reg = data::get();
    let is_block = reg.item_id(item).and_then(|i| reg.block_for_item(i)).is_some();
    format!("{}.minecraft.{item}", if is_block { "block" } else { "item" })
}

/// A villager offer.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub cost_a: ItemStack,
    pub cost_b: Option<ItemStack>,
    pub result: ItemStack,
    pub uses: i32,
    pub max_uses: i32,
    pub xp: i32,
    pub price_mult: f32,
    pub demand: i32,
}

impl Offer {
    pub fn out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// Cost A with demand applied (like MerchantOffer.getCostA).
    pub fn price_a(&self) -> i8 {
        let base = self.cost_a.count as i32;
        let extra = ((base * self.demand) as f32 * self.price_mult).floor() as i32;
        (base + extra.max(0)).clamp(1, self.cost_a.max_stack() as i32) as i8
    }
}

#[derive(Clone, Debug)]
pub struct StationState {
    /// Name typed into the anvil.
    pub rename: Option<String>,
    /// Anvil cost in levels.
    pub cost: i32,
    repair_units: i32,
    /// Selected stonecutter recipe, loom pattern or villager offer (-1 = none).
    pub selected: i32,
    last_input: Option<i32>,
    pub offers: Vec<Offer>,
    /// Offer matching what is placed in the payment slots.
    pub active_offer: Option<usize>,
    /// Villager being traded with.
    pub merchant: i32,
}

impl Default for StationState {
    fn default() -> Self {
        Self { rename: None, cost: 0, repair_units: 0, selected: -1, last_input: None, offers: vec![], active_offer: None, merchant: 0 }
    }
}

/// What happened when taking a result (applied outside the inventory lock).
#[derive(Clone, Debug)]
pub struct Taken {
    pub kind: WindowKind,
    pub levels: i32,
    pub xp: i32,
    pub trade: Option<usize>,
}

// ---------- item utilities ----------

fn item(name: &str, count: i8) -> Option<ItemStack> {
    data::get().item_id(name).map(|id| ItemStack::new(id, count))
}

pub fn repair_cost(i: &ItemStack) -> i32 {
    i.nbt.as_ref().and_then(|n| n.get_i64("RepairCost")).unwrap_or(0) as i32
}

fn set_tag(i: &mut ItemStack, key: &str, v: Option<Nbt>) {
    let tag = i.nbt.get_or_insert_with(|| Box::new(Nbt::compound()));
    match v {
        Some(v) => tag.set(key, v),
        None => tag.remove(key),
    }
    if tag.is_empty_compound() {
        i.nbt = None;
    }
}

fn set_damage(i: &mut ItemStack, dmg: i32) {
    set_tag(i, "Damage", (dmg > 0).then_some(Nbt::Int(dmg)));
}

/// Name set with the anvil (plain text), if any.
pub fn custom_name(i: &ItemStack) -> Option<String> {
    let json = i.nbt.as_ref()?.get("display")?.get_str("Name")?;
    Some(crate::world::blockentity::plain_text(json))
}

fn set_custom_name(i: &mut ItemStack, name: Option<&str>) {
    let tag = i.nbt.get_or_insert_with(|| Box::new(Nbt::compound()));
    let mut display = tag.get("display").cloned().unwrap_or_else(Nbt::compound);
    match name {
        Some(n) => display.set("Name", Nbt::str(&serde_json::json!({ "text": n }).to_string())),
        None => display.remove("Name"),
    }
    if display.is_empty_compound() {
        tag.remove("display");
    } else {
        tag.set("display", display);
    }
    if tag.is_empty_compound() {
        i.nbt = None;
    }
}

fn set_enchantments(i: &mut ItemStack, list: &[(String, i32)]) {
    let key = if i.name() == "enchanted_book" { "StoredEnchantments" } else { "Enchantments" };
    set_tag(i, "Enchantments", None);
    set_tag(i, "StoredEnchantments", None);
    if list.is_empty() {
        return;
    }
    let entries = list.iter().map(|(n, l)| Nbt::compound().with("id", Nbt::str(&format!("minecraft:{n}"))).with("lvl", Nbt::Short(*l as i16))).collect();
    set_tag(i, key, Some(Nbt::List(entries)));
}

/// Material that repairs the item in the anvil.
fn repairs(target: &ItemStack, material: &ItemStack) -> bool {
    let (t, m) = (target.name(), material.name());
    let mat = t.split('_').next().unwrap_or("");
    match () {
        _ if t == "elytra" => m == "phantom_membrane",
        _ if t == "turtle_helmet" => m == "scute",
        _ if t == "shield" => m.ends_with("_planks"),
        _ => match mat {
            "wooden" => m.ends_with("_planks"),
            "stone" => matches!(m, "cobblestone" | "blackstone" | "cobbled_deepslate"),
            "iron" | "chainmail" => m == "iron_ingot",
            "golden" => m == "gold_ingot",
            "diamond" => m == "diamond",
            "netherite" => m == "netherite_ingot",
            "leather" => m == "leather",
            _ => false,
        },
    }
}

/// Anvil multiplier based on the enchantment's rarity (weight).
fn rarity_cost(weight: i32) -> i32 {
    match weight {
        w if w >= 10 => 1,
        w if w >= 5 => 2,
        w if w >= 2 => 4,
        _ => 8,
    }
}

fn is_curse(name: &str) -> bool {
    gameplay::get().enchantment(name).is_some_and(|e| e.curse)
}

const DYES: [&str; 16] = [
    "white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black",
];

pub fn dye_color(item: &str) -> Option<i32> {
    let c = item.strip_suffix("_dye")?;
    DYES.iter().position(|d| *d == c).map(|i| i as i32)
}

/// Short code for each banner pattern (the one stored in NBT).
fn pattern_code(name: &str) -> &'static str {
    match name.trim_start_matches("minecraft:") {
        "base" => "b",
        "square_bottom_left" => "bl",
        "square_bottom_right" => "br",
        "square_top_left" => "tl",
        "square_top_right" => "tr",
        "stripe_bottom" => "bs",
        "stripe_top" => "ts",
        "stripe_left" => "ls",
        "stripe_right" => "rs",
        "stripe_center" => "cs",
        "stripe_middle" => "ms",
        "stripe_downright" => "drs",
        "stripe_downleft" => "dls",
        "small_stripes" => "ss",
        "cross" => "cr",
        "straight_cross" => "sc",
        "triangle_bottom" => "bt",
        "triangle_top" => "tt",
        "triangles_bottom" => "bts",
        "triangles_top" => "tts",
        "diagonal_left" => "ld",
        "diagonal_up_right" => "rd",
        "diagonal_up_left" => "lud",
        "diagonal_right" => "rud",
        "circle" => "mc",
        "rhombus" => "mr",
        "half_vertical" => "vh",
        "half_horizontal" => "hh",
        "half_vertical_right" => "vhr",
        "half_horizontal_bottom" => "hhb",
        "border" => "bo",
        "curly_border" => "cbo",
        "gradient" => "gra",
        "gradient_up" => "gru",
        "bricks" => "bri",
        "globe" => "glb",
        "creeper" => "cre",
        "skull" => "sku",
        "flower" => "flo",
        "mojang" => "moj",
        "piglin" => "pig",
        _ => "b",
    }
}

fn loom_patterns(pattern_item: &Slot) -> Vec<String> {
    let s = get();
    match pattern_item {
        Some(p) => s.banner_items.get(p.name()).cloned().unwrap_or_default(),
        None => s.banner_none.clone(),
    }
}

pub fn banner_layers(banner: &ItemStack) -> Vec<(String, i32)> {
    crate::world::blockentity::banner_from_nbt(banner.nbt.as_ref().and_then(|n| n.get("BlockEntityTag")))
}

// ---------- slot rules ----------

pub fn accepts(kind: WindowKind, slot: usize, it: &ItemStack) -> bool {
    let n = it.name();
    match (kind, slot) {
        (WindowKind::Brewing, 0..=2) => matches!(n, "potion" | "splash_potion" | "lingering_potion" | "glass_bottle"),
        (WindowKind::Brewing, 3) => super::brewing::is_ingredient(n),
        (WindowKind::Brewing, 4) => n == "blaze_powder",
        (WindowKind::Grindstone, 0 | 1) => it.max_durability() > 0 || it.is_enchanted() || n == "enchanted_book",
        (WindowKind::Smithing, 0) => n.ends_with("_smithing_template"),
        (WindowKind::Loom, 0) => n.ends_with("_banner"),
        (WindowKind::Loom, 1) => dye_color(n).is_some(),
        (WindowKind::Loom, 2) => n.ends_with("_banner_pattern"),
        (WindowKind::Cartography, 0) => n == "filled_map",
        (WindowKind::Cartography, 1) => matches!(n, "paper" | "map" | "glass_pane"),
        _ => true,
    }
}

/// Where a player's item goes on shift-click.
pub fn shift_targets(kind: WindowKind, it: &ItemStack) -> Vec<usize> {
    let n = it.name();
    match kind {
        WindowKind::Brewing if n == "blaze_powder" => vec![4, 3],
        WindowKind::Brewing if accepts(kind, 0, it) => vec![0, 1, 2],
        WindowKind::Brewing if accepts(kind, 3, it) => vec![3],
        WindowKind::Anvil | WindowKind::Grindstone | WindowKind::Merchant => vec![0, 1],
        WindowKind::Smithing if accepts(kind, 0, it) => vec![0],
        WindowKind::Smithing => vec![1, 2],
        WindowKind::Stonecutter => vec![0],
        WindowKind::Loom => (0..3).filter(|&i| accepts(kind, i, it)).collect(),
        WindowKind::Cartography => (0..2).filter(|&i| accepts(kind, i, it)).collect(),
        _ => vec![],
    }
}

// ---------- result computation ----------

pub fn compute(w: &mut OpenWindow, _levels: i32, creative: bool) {
    let result = match w.kind {
        WindowKind::Anvil => anvil(w, creative),
        WindowKind::Grindstone => grindstone(&w.slots[0], &w.slots[1]),
        WindowKind::Smithing => smithing(&w.slots[0], &w.slots[1], &w.slots[2]),
        WindowKind::Stonecutter => stonecutter(w),
        WindowKind::Loom => loom(w),
        WindowKind::Cartography => cartography(&w.slots[0], &w.slots[1]),
        WindowKind::Merchant => merchant(w),
        _ => return,
    };
    if let Some(r) = w.kind.result_slot() {
        w.slots[r] = result;
    }
}

fn anvil(w: &mut OpenWindow, creative: bool) -> Slot {
    let st = &mut w.station;
    st.cost = 0;
    st.repair_units = 0;
    let left = w.slots[0].clone()?;
    let right = w.slots[1].clone();
    let mut res = left.clone();
    let mut ench = res.enchantments();
    let base = repair_cost(&left) + right.as_ref().map_or(0, repair_cost);
    let mut cost = 0;
    if let Some(right) = &right {
        let book = right.name() == "enchanted_book" && !right.enchantments().is_empty();
        let max = res.max_durability();
        if max > 0 && repairs(&res, right) {
            let mut step = res.damage().min(max / 4);
            if step <= 0 {
                return None;
            }
            let mut used = 0;
            while step > 0 && used < right.count as i32 {
                let d = res.damage() - step;
                set_damage(&mut res, d);
                cost += 1;
                used += 1;
                step = res.damage().min(max / 4);
            }
            st.repair_units = used;
        } else {
            if !book && (res.id != right.id || max <= 0) {
                return None;
            }
            if max > 0 && !book {
                let remaining = (max - left.damage()) + (max - right.damage()) + max * 12 / 100;
                let new = (max - remaining).max(0);
                if new < res.damage() {
                    set_damage(&mut res, new);
                    cost += 2;
                }
            }
            let (mut applied, mut rejected) = (false, false);
            for (name, lvl) in right.enchantments() {
                let Some(e) = gameplay::get().enchantment(&name) else { continue };
                let have = ench.iter().find(|(n, _)| *n == name).map_or(0, |x| x.1);
                let mut new = if have == lvl { lvl + 1 } else { lvl.max(have) };
                let mut ok = creative || left.name() == "enchanted_book" || gameplay::enchant_category_fits(&e.category, &left);
                for (other, _) in &ench {
                    if *other != name && !super::enchanting::compatible(&name, other) {
                        ok = false;
                        cost += 1;
                    }
                }
                if !ok {
                    rejected = true;
                    continue;
                }
                applied = true;
                new = new.min(e.max_level);
                match ench.iter_mut().find(|(n, _)| *n == name) {
                    Some(x) => x.1 = new,
                    None => ench.push((name.clone(), new)),
                }
                let mut k = rarity_cost(e.weight);
                if book {
                    k = (k / 2).max(1);
                }
                cost += k * new;
                if left.count > 1 {
                    cost = 40;
                }
            }
            if rejected && !applied {
                return None;
            }
        }
    }
    // Rename (empty string = remove the name).
    let mut rename_cost = 0;
    match st.rename.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => {
            if custom_name(&left).as_deref() != Some(name) {
                rename_cost = 1;
                set_custom_name(&mut res, Some(name));
            }
        }
        None => {
            if custom_name(&left).is_some() {
                rename_cost = 1;
                set_custom_name(&mut res, None);
            }
        }
    }
    cost += rename_cost;
    st.cost = base + cost;
    if cost <= 0 {
        st.cost = 0;
        return None;
    }
    if rename_cost == cost && rename_cost > 0 && st.cost >= 40 {
        st.cost = 39;
    }
    if st.cost >= 40 && !creative {
        return None;
    }
    let mut rc = repair_cost(&res).max(right.as_ref().map_or(0, repair_cost));
    if rename_cost != cost || rename_cost == 0 {
        rc = rc * 2 + 1;
    }
    set_tag(&mut res, "RepairCost", Some(Nbt::Int(rc)));
    set_enchantments(&mut res, &ench);
    Some(res)
}

fn grindstone(top: &Slot, bottom: &Slot) -> Slot {
    let useful = |s: &Slot| s.as_ref().is_some_and(|i| i.name() == "enchanted_book" || i.is_enchanted());
    let (t, b) = (top.as_ref(), bottom.as_ref());
    if t.is_none() && b.is_none() {
        return None;
    }
    if t.is_some_and(|i| i.count > 1) || b.is_some_and(|i| i.count > 1) {
        return None;
    }
    let both = t.is_some() && b.is_some();
    if !both && !useful(top) && !useful(bottom) {
        return None;
    }
    let mut res;
    if both {
        let (t, b) = (t.unwrap(), b.unwrap());
        if t.id != b.id {
            return None;
        }
        let max = t.max_durability();
        if max <= 0 && t.name() != "enchanted_book" {
            return None;
        }
        res = t.clone();
        if max > 0 {
            let remaining = (max - t.damage()) + (max - b.damage()) + max * 5 / 100;
            set_damage(&mut res, (max - remaining).max(0));
        }
        // Curses from both items are kept.
        let mut curses: Vec<(String, i32)> = t.enchantments().into_iter().filter(|(n, _)| is_curse(n)).collect();
        for c in b.enchantments().into_iter().filter(|(n, _)| is_curse(n)) {
            if !curses.iter().any(|x| x.0 == c.0) {
                curses.push(c);
            }
        }
        set_enchantments(&mut res, &curses);
    } else {
        res = t.or(b).unwrap().clone();
        let curses: Vec<(String, i32)> = res.enchantments().into_iter().filter(|(n, _)| is_curse(n)).collect();
        set_enchantments(&mut res, &curses);
    }
    set_tag(&mut res, "RepairCost", None);
    if res.name() == "enchanted_book" && res.enchantments().is_empty() {
        let mut book = item("book", 1)?;
        book.nbt = res.nbt.clone();
        res = book;
    }
    res.count = 1;
    Some(res)
}

fn smithing(template: &Slot, base: &Slot, addition: &Slot) -> Slot {
    let (t, b, a) = (template.as_ref()?, base.as_ref()?, addition.as_ref()?);
    let s = get();
    if let Some(r) = s.smithing.iter().find(|r| r.0 == t.name() && r.1 == b.name() && r.2 == a.name()) {
        let mut out = item(&r.3, 1)?;
        out.nbt = b.nbt.clone();
        return Some(out);
    }
    let pattern = s.trim_patterns.get(t.name())?;
    let material = s.trim_materials.get(a.name())?;
    if !s.trimmable.iter().any(|x| x == b.name()) {
        return None;
    }
    let trim = Nbt::compound().with("pattern", Nbt::str(pattern)).with("material", Nbt::str(material));
    if b.nbt.as_ref().and_then(|n| n.get("Trim")) == Some(&trim) {
        return None;
    }
    let mut out = b.clone();
    out.count = 1;
    set_tag(&mut out, "Trim", Some(trim));
    Some(out)
}

fn stonecutter(w: &mut OpenWindow) -> Slot {
    let input = w.slots[0].as_ref().map(|i| i.id);
    if input != w.station.last_input {
        w.station.selected = -1;
        w.station.last_input = input;
    }
    let input = w.slots[0].as_ref()?;
    let list = get().stonecutting.get(input.name())?;
    let (name, count) = list.get(usize::try_from(w.station.selected).ok()?)?;
    item(name, *count)
}

fn loom(w: &mut OpenWindow) -> Slot {
    let pattern = w.slots[2].as_ref().map(|i| i.id);
    if pattern != w.station.last_input {
        w.station.selected = -1;
        w.station.last_input = pattern;
    }
    let banner = w.slots[0].as_ref()?;
    let color = dye_color(w.slots[1].as_ref()?.name())?;
    let patterns = loom_patterns(&w.slots[2]);
    let name = patterns.get(usize::try_from(w.station.selected).ok()?)?;
    let mut layers = banner_layers(banner);
    if layers.len() >= 6 {
        return None;
    }
    layers.push((pattern_code(name).to_string(), color));
    let mut out = banner.with_count(1);
    let mut bet = out.nbt.as_ref().and_then(|n| n.get("BlockEntityTag")).cloned().unwrap_or_else(Nbt::compound);
    if let Nbt::Compound(fields) = crate::world::blockentity::banner_nbt(&layers) {
        for (k, v) in fields {
            bet.set(&k, v);
        }
    }
    set_tag(&mut out, "BlockEntityTag", Some(bet));
    Some(out)
}

fn cartography(map: &Slot, extra: &Slot) -> Slot {
    let (m, e) = (map.as_ref()?, extra.as_ref()?);
    let id = super::maps::map_id(m)?;
    let info = super::maps::info(id)?;
    let mut out = m.with_count(1);
    match e.name() {
        "paper" if info.scale < 4 && !info.locked => set_tag(&mut out, "map_scale_direction", Some(Nbt::Int(1))),
        "glass_pane" if !info.locked => set_tag(&mut out, "map_to_lock", Some(Nbt::Byte(1))),
        "map" => out.count = 2,
        _ => return None,
    }
    Some(out)
}

fn merchant(w: &mut OpenWindow) -> Slot {
    let st = &mut w.station;
    st.active_offer = None;
    let (a, b) = (&w.slots[0], &w.slots[1]);
    let fits = |o: &Offer, x: &Slot, y: &Slot| {
        let need_a = o.price_a();
        let ok_a = x.as_ref().is_some_and(|i| i.id == o.cost_a.id && i.count >= need_a);
        let ok_b = match &o.cost_b {
            Some(cb) => y.as_ref().is_some_and(|i| i.id == cb.id && i.count >= cb.count),
            None => true,
        };
        ok_a && ok_b
    };
    let order: Vec<usize> = usize::try_from(st.selected).ok().into_iter().chain(0..st.offers.len()).collect();
    for i in order {
        let Some(o) = st.offers.get(i) else { continue };
        if o.out_of_stock() {
            continue;
        }
        if fits(o, a, b) || (o.cost_b.is_some() && fits(o, b, a)) {
            st.active_offer = Some(i);
            return Some(o.result.clone());
        }
    }
    None
}

// ---------- taking the result ----------

pub fn can_take(w: &OpenWindow, levels: i32, creative: bool) -> bool {
    match w.kind {
        WindowKind::Anvil => w.station.cost > 0 && (creative || levels >= w.station.cost),
        WindowKind::Merchant => w.station.active_offer.is_some(),
        _ => true,
    }
}

fn shrink(s: &mut Slot, n: i32) {
    if let Some(i) = s {
        i.count -= n.min(i.count as i32) as i8;
        if i.count <= 0 {
            *s = None;
        }
    }
}

/// Consumes the inputs after taking the result `taken`.
pub fn consume(w: &mut OpenWindow, _taken: &ItemStack, creative: bool) -> Taken {
    let mut out = Taken { kind: w.kind, levels: 0, xp: 0, trade: None };
    match w.kind {
        WindowKind::Anvil => {
            if !creative {
                out.levels = w.station.cost;
            }
            w.slots[0] = None;
            if w.station.repair_units > 0 {
                shrink(&mut w.slots[1], w.station.repair_units);
            } else {
                w.slots[1] = None;
            }
            w.station.cost = 0;
        }
        WindowKind::Grindstone => {
            let mut total = 0;
            for s in &w.slots[0..2] {
                for (n, l) in s.iter().flat_map(|i| i.enchantments()) {
                    if let Some(e) = gameplay::get().enchantment(&n).filter(|e| !e.curse) {
                        total += e.min_cost.0 * l + e.min_cost.1;
                    }
                }
            }
            if total > 0 {
                let half = (total + 1) / 2;
                out.xp = half + (entities::random() * half as f32) as i32;
            }
            w.slots[0] = None;
            w.slots[1] = None;
        }
        WindowKind::Smithing => {
            for i in 0..3 {
                shrink(&mut w.slots[i], 1);
            }
        }
        WindowKind::Stonecutter => shrink(&mut w.slots[0], 1),
        WindowKind::Loom => {
            shrink(&mut w.slots[0], 1);
            shrink(&mut w.slots[1], 1);
        }
        WindowKind::Cartography => {
            shrink(&mut w.slots[0], 1);
            shrink(&mut w.slots[1], 1);
        }
        WindowKind::Merchant => {
            if let Some(i) = w.station.active_offer {
                let o = w.station.offers[i].clone();
                let need_a = o.price_a() as i32;
                // Cost A can be in either of the two slots.
                let (sa, sb) = if w.slots[0].as_ref().is_some_and(|s| s.id == o.cost_a.id && s.count as i32 >= need_a) { (0, 1) } else { (1, 0) };
                shrink(&mut w.slots[sa], need_a);
                if let Some(cb) = &o.cost_b {
                    shrink(&mut w.slots[sb], cb.count as i32);
                }
                w.station.offers[i].uses += 1;
                out.trade = Some(i);
                out.xp = o.xp;
            }
        }
        _ => {}
    }
    out
}

// ---------- opening and packets ----------

/// Opens the clicked workstation block. Returns `true` if it was one.
pub fn open(_server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, block: &str) -> bool {
    let (kind, n, title) = match block {
        "anvil" | "chipped_anvil" | "damaged_anvil" => (WindowKind::Anvil, 3, "Reparar y nombrar"),
        "grindstone" => (WindowKind::Grindstone, 3, "Reparar y desencantar"),
        "smithing_table" => (WindowKind::Smithing, 4, "Mejorar equipo"),
        "stonecutter" => (WindowKind::Stonecutter, 2, "Cortapiedras"),
        "loom" => (WindowKind::Loom, 4, "Telar"),
        "cartography_table" => (WindowKind::Cartography, 3, "Mesa de cartografía"),
        _ => return false,
    };
    let mut st = player.state.lock().unwrap();
    for p in st.inv.open(kind, pos, vec![None; n], title) {
        player.send(p);
    }
    true
}

/// Window button (`container_button_click`): stonecutter recipe or loom pattern.
pub fn button(player: &Arc<Player>, button: i32) {
    let mut st = player.state.lock().unwrap();
    let Some(w) = st.inv.window.as_mut().filter(|w| matches!(w.kind, WindowKind::Stonecutter | WindowKind::Loom)) else { return };
    let max = match w.kind {
        WindowKind::Stonecutter => w.slots[0].as_ref().and_then(|i| get().stonecutting.get(i.name())).map_or(0, Vec::len),
        _ => loom_patterns(&w.slots[2]).len(),
    };
    if button < 0 || button as usize >= max {
        return;
    }
    w.station.selected = button;
    let (levels, creative) = (st.inv.levels, st.inv.creative);
    let w = st.inv.window.as_mut().unwrap();
    compute(w, levels, creative);
    if let Some(p) = st.inv.result_packet() {
        player.send(p);
    }
}

/// Anvil `name_item` packet.
pub fn rename(player: &Arc<Player>, name: &str) {
    let mut st = player.state.lock().unwrap();
    let Some(w) = st.inv.window.as_mut().filter(|w| w.kind == WindowKind::Anvil) else { return };
    let name: String = name.chars().filter(|c| *c != '§' && !c.is_control()).take(50).collect();
    w.station.rename = Some(name);
    let (levels, creative) = (st.inv.levels, st.inv.creative);
    compute(st.inv.window.as_mut().unwrap(), levels, creative);
    if let Some(p) = st.inv.result_packet() {
        player.send(p);
    }
    let cost = st.inv.window.as_ref().map_or(0, |w| w.station.cost);
    let p = st.inv.property_packet(0, cost as i16);
    player.send(p);
}

/// After each click in a workstation: anvil cost, levels, experience, sounds,
/// anvil wear, trades and new maps.
pub fn after_click(server: &Arc<Server>, player: &Arc<Player>, taken: Vec<Taken>) {
    let dim = player.dim();
    let (kind, pos) = {
        let st = player.state.lock().unwrap();
        match &st.inv.window {
            Some(w) => (Some(w.kind), w.pos),
            None => (None, BlockPos::new(0, 0, 0)),
        }
    };
    if kind == Some(WindowKind::Anvil) {
        let st = player.state.lock().unwrap();
        let cost = st.inv.window.as_ref().map_or(0, |w| w.station.cost);
        let p = st.inv.property_packet(0, cost as i16);
        player.send(p);
    }
    for t in taken {
        {
            let mut st = player.state.lock().unwrap();
            if t.levels > 0 {
                experience::spend_levels(player, &mut st, t.levels);
            }
            if t.xp > 0 && t.kind != WindowKind::Merchant {
                experience::add_points(player, &mut st, t.xp);
            }
            st.inv.levels = st.xp_level;
        }
        let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
        match t.kind {
            WindowKind::Anvil => anvil_used(server, player, pos),
            WindowKind::Grindstone => super::sound::at(server, dim, center, "block.grindstone.use", super::sound::BLOCK, 1.0, 1.0),
            WindowKind::Smithing => super::sound::at(server, dim, center, "block.smithing_table.use", super::sound::BLOCK, 1.0, 1.0),
            WindowKind::Stonecutter => super::sound::at(server, dim, center, "ui.stonecutter.take_result", super::sound::BLOCK, 1.0, 1.0),
            WindowKind::Loom => super::sound::at(server, dim, center, "ui.loom.take_result", super::sound::BLOCK, 1.0, 1.0),
            WindowKind::Cartography => {
                super::maps::finalize_items(server, player);
                super::sound::at(server, dim, center, "ui.cartography_table.take_result", super::sound::BLOCK, 1.0, 1.0);
            }
            WindowKind::Merchant => {
                if let Some(i) = t.trade {
                    super::villagers::traded(server, player, i, t.xp);
                }
            }
            _ => {}
        }
    }
}

/// The anvil wears down (12 % per use) and may break.
fn anvil_used(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos) {
    let dim = player.dim();
    let creative = player.state.lock().unwrap().game_mode == 1;
    let reg = data::get();
    let state = server.world(dim).lock().unwrap().get_block(pos);
    let b = reg.block_of_state(state);
    let mut event = 1030; // use sound
    if !creative && entities::random() < 0.12 {
        let next = match b.name.as_str() {
            "anvil" => Some("chipped_anvil"),
            "chipped_anvil" => Some("damaged_anvil"),
            _ => None,
        };
        match next {
            Some(n) => {
                let nb = reg.block(n).unwrap();
                let facing = b.get(state, "facing").unwrap_or("north").to_string();
                let s = nb.with(nb.default_state, "facing", &facing);
                server.world(dim).lock().unwrap().set_block(pos, s);
                server.broadcast_dim(dim, &super::blocks::block_change(pos, s), None);
            }
            None => {
                server.world(dim).lock().unwrap().set_block(pos, 0);
                server.broadcast_dim(dim, &super::blocks::block_change(pos, 0), None);
                event = 1029; // breaks
                super::containers::close_viewers_of(server, dim, pos);
            }
        }
    }
    let mut p = packet(id::play::WORLD_EVENT);
    p.i32(event).position(pos).i32(0).bool(false);
    server.broadcast_dim(dim, &p, None);
}

/// `declare_recipes` with the recipes the client needs to know to display the
/// stonecutter and validate the smithing table.
pub fn declare_recipes() -> Vec<u8> {
    let s = get();
    let reg = data::get();
    let mut p = packet(id::play::DECLARE_RECIPES);
    let ing = |p: &mut Vec<u8>, names: &[&str]| {
        let ids: Vec<i32> = names.iter().filter_map(|n| reg.item_id(n)).collect();
        p.varint(ids.len() as i32);
        for id in ids {
            crate::item::write_slot(p, &Some(ItemStack::new(id, 1)));
        }
    };
    let mut entries: Vec<Vec<u8>> = Vec::new();
    let mut inputs: Vec<&String> = s.stonecutting.keys().collect();
    inputs.sort();
    for input in inputs {
        for (result, count) in &s.stonecutting[input] {
            let mut e = Vec::new();
            e.string("minecraft:stonecutting").string(&format!("minecraft:{result}_from_{input}_stonecutting")).string("");
            ing(&mut e, &[input.as_str()]);
            crate::item::write_slot(&mut e, &item(result, *count));
            entries.push(e);
        }
    }
    for (t, b, a, r) in &s.smithing {
        let mut e = Vec::new();
        e.string("minecraft:smithing_transform").string(&format!("minecraft:{r}_smithing"));
        ing(&mut e, &[t.as_str()]);
        ing(&mut e, &[b.as_str()]);
        ing(&mut e, &[a.as_str()]);
        crate::item::write_slot(&mut e, &item(r, 1));
        entries.push(e);
    }
    let trimmable: Vec<&str> = s.trimmable.iter().map(String::as_str).collect();
    let materials: Vec<&str> = s.trim_materials.keys().map(String::as_str).collect();
    let mut templates: Vec<&String> = s.trim_patterns.keys().collect();
    templates.sort();
    for t in templates {
        let mut e = Vec::new();
        e.string("minecraft:smithing_trim").string(&format!("minecraft:{t}_smithing_trim"));
        ing(&mut e, &[t.as_str()]);
        ing(&mut e, &trimmable);
        ing(&mut e, &materials);
        entries.push(e);
    }
    p.varint(entries.len() as i32);
    for e in entries {
        p.extend_from_slice(&e);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(n: &str, c: i8) -> Slot {
        item(n, c)
    }

    fn window(kind: WindowKind, slots: Vec<Slot>) -> OpenWindow {
        OpenWindow { id: 1, kind, slots, pos: BlockPos::new(0, 0, 0), pos2: None, offers: [None, None, None], station: Default::default() }
    }

    #[test]
    fn anvil_repairs_combines_and_renames() {
        // Repair an iron pickaxe with ingots.
        let mut pick = it("iron_pickaxe", 1).unwrap();
        set_damage(&mut pick, 200);
        let mut w = window(WindowKind::Anvil, vec![Some(pick.clone()), it("iron_ingot", 5), None]);
        compute(&mut w, 30, false);
        let r = w.slots[2].clone().expect("resultado");
        assert_eq!(r.damage(), 200 - 62 - 62 - 62 - 14);
        assert_eq!(w.station.cost, 4);
        assert_eq!(w.station.repair_units, 4);
        let t = consume(&mut w, &r, false);
        assert_eq!(t.levels, 4);
        assert_eq!(w.slots[1].as_ref().unwrap().count, 1);
        // Sharpness V book on a sword.
        let sword = it("diamond_sword", 1).unwrap();
        let mut book = it("enchanted_book", 1).unwrap();
        book.enchant("sharpness", 5);
        let mut w = window(WindowKind::Anvil, vec![Some(sword.clone()), Some(book), None]);
        compute(&mut w, 30, false);
        let r = w.slots[2].clone().unwrap();
        assert_eq!(r.enchantment("sharpness"), 5);
        assert_eq!(w.station.cost, 5); // common (1) / 2 for a book -> 1 * 5
        assert_eq!(repair_cost(&r), 1);
        // Incompatible book: nothing.
        let mut prot = it("enchanted_book", 1).unwrap();
        prot.enchant("protection", 4);
        let mut w = window(WindowKind::Anvil, vec![Some(sword.clone()), Some(prot), None]);
        compute(&mut w, 30, false);
        assert!(w.slots[2].is_none());
        // Rename.
        let mut w = window(WindowKind::Anvil, vec![Some(sword), None, None]);
        w.station.rename = Some("Excalibur".into());
        compute(&mut w, 30, false);
        let r = w.slots[2].clone().unwrap();
        assert_eq!(custom_name(&r).as_deref(), Some("Excalibur"));
        assert_eq!(w.station.cost, 1);
        assert!(!can_take(&w, 0, false));
        assert!(can_take(&w, 1, false));
    }

    #[test]
    fn grindstone_smithing_stonecutter_loom() {
        let mut sword = it("iron_sword", 1).unwrap();
        sword.enchant("sharpness", 3);
        sword.enchant("vanishing_curse", 1);
        let r = grindstone(&Some(sword), &None).unwrap();
        assert_eq!(r.enchantments(), vec![("vanishing_curse".to_string(), 1)]);
        assert!(grindstone(&it("iron_sword", 1), &None).is_none(), "sin encantar no hace nada");
        let mut book = it("enchanted_book", 1).unwrap();
        book.enchant("mending", 1);
        assert_eq!(grindstone(&Some(book), &None).unwrap().name(), "book");

        let r = smithing(&it("netherite_upgrade_smithing_template", 1), &it("diamond_sword", 1), &it("netherite_ingot", 1)).unwrap();
        assert_eq!(r.name(), "netherite_sword");
        let r = smithing(&it("coast_armor_trim_smithing_template", 1), &it("iron_chestplate", 1), &it("gold_ingot", 1)).unwrap();
        assert_eq!(r.nbt.as_ref().unwrap().get("Trim").unwrap().get_str("material"), Some("minecraft:gold"));

        let list = &get().stonecutting["stone"];
        assert!(list.iter().any(|(r, _)| r == "stone_bricks"));
        let mut keys: Vec<String> = list.iter().map(|(r, _)| translation_key(r)).collect();
        let sorted = {
            let mut k = keys.clone();
            k.sort();
            k
        };
        assert_eq!(keys, sorted);
        keys.clear();
        let mut w = window(WindowKind::Stonecutter, vec![it("stone", 3), None]);
        compute(&mut w, 0, false);
        assert!(w.slots[1].is_none());
        w.station.selected = list.iter().position(|(r, _)| r == "stone_slab").unwrap() as i32;
        compute(&mut w, 0, false);
        assert_eq!(w.slots[1].as_ref().map(|i| (i.name(), i.count)), Some(("stone_slab", 2)));

        let mut w = window(WindowKind::Loom, vec![it("white_banner", 1), it("red_dye", 1), None, None]);
        compute(&mut w, 0, false);
        w.station.selected = get().banner_none.iter().position(|p| p == "stripe_bottom").unwrap() as i32;
        compute(&mut w, 0, false);
        let r = w.slots[3].clone().unwrap();
        assert_eq!(banner_layers(&r), vec![("bs".to_string(), 14)]);
    }
}

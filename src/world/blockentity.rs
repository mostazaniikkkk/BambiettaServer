//! Block entities: chest contents and furnace state, plus the types the client
//! needs in the chunk packet to render chests, beds, signs, banners...

use crate::item::{ItemStack, Slot};
use crate::protocol::Nbt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq)]
pub enum BlockEntity {
    Container(Vec<Slot>),
    Furnace(Furnace),
    Brewing(Brewing),
    Sign(Sign),
    /// Banner layers: (pattern code, dye color).
    Banner(Vec<(String, i32)>),
}

/// Brewing stand: 0-2 bottles, 3 ingredient, 4 blaze powder.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Brewing {
    pub items: Vec<Slot>,
    pub brew_time: i32,
    pub fuel: i32,
    /// Ingredient brewing started with (if it changes, it is cancelled).
    pub ingredient: i32,
}

/// Text of one side of a sign.
#[derive(Clone, Debug, PartialEq)]
pub struct SignText {
    pub lines: [String; 4],
    pub color: String,
    pub glowing: bool,
}

impl Default for SignText {
    fn default() -> Self {
        Self { lines: Default::default(), color: "black".into(), glowing: false }
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Sign {
    pub front: SignText,
    pub back: SignText,
    pub waxed: bool,
}

impl SignText {
    pub fn to_nbt(&self) -> Nbt {
        // Lines are sent as JSON text components, just like in vanilla 1.20.4.
        let msgs = self.lines.iter().map(|l| Nbt::str(&serde_json::json!({ "text": l }).to_string())).collect();
        Nbt::compound()
            .with("messages", Nbt::List(msgs))
            .with("color", Nbt::str(&self.color))
            .with("has_glowing_text", Nbt::Byte(self.glowing as i8))
    }

    pub fn from_nbt(n: Option<&Nbt>) -> Self {
        let Some(n) = n else { return Self::default() };
        let mut t = Self::default();
        for (i, m) in n.get("messages").map(Nbt::as_list).unwrap_or(&[]).iter().take(4).enumerate() {
            t.lines[i] = m.as_str().map(plain_text).unwrap_or_default();
        }
        t.color = n.get_str("color").unwrap_or("black").to_string();
        t.glowing = n.get_i64("has_glowing_text").unwrap_or(0) != 0;
        t
    }
}

/// Plain text of a JSON component (or the text itself if it isn't JSON).
pub fn plain_text(json: &str) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::String(s) => out.push_str(s),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => {
                if let Some(t) = o.get("text") {
                    walk(t, out);
                }
                if let Some(e) = o.get("extra") {
                    walk(e, out);
                }
            }
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(v) => {
            let mut s = String::new();
            walk(&v, &mut s);
            s
        }
        Err(_) => json.to_string(),
    }
}

impl Sign {
    pub fn to_nbt(&self) -> Nbt {
        Nbt::compound().with("front_text", self.front.to_nbt()).with("back_text", self.back.to_nbt()).with("is_waxed", Nbt::Byte(self.waxed as i8))
    }

    pub fn from_nbt(n: &Nbt) -> Self {
        Self { front: SignText::from_nbt(n.get("front_text")), back: SignText::from_nbt(n.get("back_text")), waxed: n.get_i64("is_waxed").unwrap_or(0) != 0 }
    }
}

pub fn banner_nbt(patterns: &[(String, i32)]) -> Nbt {
    let list = patterns.iter().map(|(p, c)| Nbt::compound().with("Pattern", Nbt::str(p)).with("Color", Nbt::Int(*c))).collect();
    Nbt::compound().with("Patterns", Nbt::List(list))
}

pub fn banner_from_nbt(n: Option<&Nbt>) -> Vec<(String, i32)> {
    n.and_then(|n| n.get("Patterns"))
        .map(Nbt::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| Some((p.get_str("Pattern")?.to_string(), p.get_i64("Color").unwrap_or(0) as i32)))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Furnace {
    /// 0 input, 1 fuel, 2 output.
    pub items: Vec<Slot>,
    pub burn: i32,
    pub burn_total: i32,
    pub cook: i32,
    /// Accumulated experience from smelting (given when taking the output).
    pub xp: f32,
}

impl BlockEntity {
    /// Empty entity for a freshly placed block, if it needs one.
    pub fn for_block(name: &str) -> Option<BlockEntity> {
        match name {
            "chest" | "trapped_chest" | "barrel" => Some(BlockEntity::Container(vec![None; 27])),
            "furnace" => Some(BlockEntity::Furnace(Furnace { items: vec![None; 3], ..Default::default() })),
            "brewing_stand" => Some(BlockEntity::Brewing(Brewing { items: vec![None; 5], ..Default::default() })),
            n if n.ends_with("_sign") => Some(BlockEntity::Sign(Sign::default())),
            n if n.ends_with("_banner") => Some(BlockEntity::Banner(vec![])),
            _ => None,
        }
    }

    /// All the items it contains (to drop them when the block is broken).
    pub fn contents(&self) -> Vec<ItemStack> {
        match self {
            BlockEntity::Container(items) => items.iter().flatten().cloned().collect(),
            BlockEntity::Furnace(f) => f.items.iter().flatten().cloned().collect(),
            BlockEntity::Brewing(b) => b.items.iter().flatten().cloned().collect(),
            BlockEntity::Sign(_) | BlockEntity::Banner(_) => vec![],
        }
    }

    /// Data the client needs to render it (sign text, banner layers).
    pub fn client_nbt(&self) -> Option<Nbt> {
        match self {
            BlockEntity::Sign(s) => Some(s.to_nbt()),
            BlockEntity::Banner(p) if !p.is_empty() => Some(banner_nbt(p)),
            _ => None,
        }
    }
}

/// Block entity type id (1.20.4 `minecraft:block_entity_type` registry).
pub fn type_id(block: &str) -> Option<i32> {
    let n = block;
    Some(match n {
        "furnace" => 0,
        "chest" => 1,
        "trapped_chest" => 2,
        "ender_chest" => 3,
        "jukebox" => 4,
        "dispenser" => 5,
        "dropper" => 6,
        "spawner" => 9,
        "brewing_stand" => 11,
        "enchanting_table" => 12,
        "end_portal" => 13,
        "beacon" => 14,
        "daylight_detector" => 16,
        "hopper" => 17,
        "comparator" => 18,
        "conduit" => 25,
        "barrel" => 26,
        "smoker" => 27,
        "blast_furnace" => 28,
        "lectern" => 29,
        "bell" => 30,
        "campfire" | "soul_campfire" => 32,
        "beehive" | "bee_nest" => 33,
        "chiseled_bookshelf" => 38,
        "decorated_pot" => 40,
        _ if n.ends_with("_hanging_sign") => 8,
        _ if n.ends_with("_sign") => 7,
        _ if (n.ends_with("_skull") || n.ends_with("_head")) && n != "piston_head" => 15,
        _ if n.ends_with("_banner") => 19,
        _ if n.ends_with("shulker_box") => 23,
        _ if n.ends_with("_bed") => 24,
        _ => return None,
    })
}

// ---------- persistence ----------

#[derive(Serialize, Deserialize)]
pub struct SavedSlot {
    pub slot: usize,
    pub item: String,
    pub count: i8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbt: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedBlockEntity {
    pub pos: [i32; 3],
    pub kind: String,
    pub items: Vec<SavedSlot>,
    #[serde(default)]
    pub burn: i32,
    #[serde(default)]
    pub burn_total: i32,
    #[serde(default)]
    pub cook: i32,
    #[serde(default)]
    pub xp: f32,
}

pub fn load_slots(saved: &[SavedSlot], len: usize) -> Vec<Slot> {
    let reg = crate::data::get();
    let mut out = vec![None; len];
    for s in saved {
        let Some(id) = reg.item_id(&s.item) else { continue };
        if s.slot < len && s.count > 0 {
            let nbt = s.nbt.as_ref().and_then(|h| {
                let bytes: Option<Vec<u8>> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect();
                Nbt::from_network(&bytes?)
            });
            out[s.slot] = Some(ItemStack { id, count: s.count, nbt: nbt.map(Box::new) });
        }
    }
    out
}

impl SavedBlockEntity {
    pub fn into_block_entity(self) -> BlockEntity {
        match self.kind.as_str() {
            "furnace" => BlockEntity::Furnace(Furnace { items: load_slots(&self.items, 3), burn: self.burn, burn_total: self.burn_total, cook: self.cook, xp: self.xp }),
            _ => BlockEntity::Container(load_slots(&self.items, 27)),
        }
    }
}

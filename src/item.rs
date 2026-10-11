//! Item stacks, their 1.20.4 `Slot` format, durability and enchantments (NBT `Damage` and
//! `Enchantments`, as in vanilla 1.20.4).

use crate::gameplay;
use crate::protocol::{Nbt, Reader, Writer};
use anyhow::Result;

#[derive(Clone, Debug, PartialEq)]
pub struct ItemStack {
    pub id: i32,
    pub count: i8,
    /// Item NBT tag (damage, enchantments, name...).
    pub nbt: Option<Box<Nbt>>,
}

pub type Slot = Option<ItemStack>;

impl ItemStack {
    pub fn new(id: i32, count: i8) -> Self {
        Self { id, count, nbt: None }
    }

    pub fn max_stack(&self) -> i8 {
        gameplay::get().max_stack(self.id)
    }

    pub fn name(&self) -> &'static str {
        gameplay::get().item_name(self.id)
    }

    /// Same item and same data: they can stack.
    pub fn stacks_with(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.nbt == other.nbt
    }

    pub fn with_count(&self, count: i8) -> Self {
        Self { count, ..self.clone() }
    }

    fn tag_mut(&mut self) -> &mut Nbt {
        self.nbt.get_or_insert_with(|| Box::new(Nbt::Compound(vec![])))
    }

    /// Removes the tag if it ended up empty (so it stacks with "clean" items again).
    fn tidy(&mut self) {
        if self.nbt.as_ref().is_some_and(|n| n.is_empty_compound()) {
            self.nbt = None;
        }
    }

    // ---------- durability ----------

    pub fn max_durability(&self) -> i32 {
        gameplay::get().item(self.id).map_or(0, |i| i.max_durability)
    }

    pub fn damage(&self) -> i32 {
        self.nbt.as_ref().and_then(|n| n.get("Damage")).and_then(Nbt::as_i64).unwrap_or(0) as i32
    }

    /// Consumes durability (accounting for Unbreaking). Returns `true` if the item breaks.
    pub fn wear(&mut self, amount: i32, rnd: &mut impl FnMut() -> f32) -> bool {
        let max = self.max_durability();
        if max <= 0 || amount <= 0 || self.nbt.as_ref().and_then(|n| n.get("Unbreakable")).and_then(Nbt::as_i64) == Some(1) {
            return false;
        }
        let unbreaking = self.enchantment("unbreaking");
        let armor = crate::server::inventory::armor_slot(self).is_some();
        let mut spent = 0;
        for _ in 0..amount {
            // Vanilla: tools skip wear with prob. lvl/(lvl+1); armor 0.6*lvl/(lvl+1).
            let skip = if unbreaking == 0 {
                false
            } else if armor {
                rnd() < 0.6 * unbreaking as f32 / (unbreaking as f32 + 1.0)
            } else {
                rnd() < unbreaking as f32 / (unbreaking as f32 + 1.0)
            };
            if !skip {
                spent += 1;
            }
        }
        let new = self.damage() + spent;
        self.tag_mut().set("Damage", Nbt::Int(new));
        new >= max
    }

    // ---------- enchantments ----------

    /// Enchantments `(name without "minecraft:", level)`; also reads those on books.
    pub fn enchantments(&self) -> Vec<(String, i32)> {
        let Some(n) = &self.nbt else { return vec![] };
        let list = n.get("Enchantments").or_else(|| n.get("StoredEnchantments"));
        list.map(Nbt::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|e| {
                let id = e.get("id")?.as_str()?.trim_start_matches("minecraft:").to_string();
                Some((id, e.get("lvl")?.as_i64()? as i32))
            })
            .collect()
    }

    pub fn enchantment(&self, name: &str) -> i32 {
        self.enchantments().into_iter().find(|(n, _)| n == name).map_or(0, |(_, l)| l)
    }

    /// Adds or upgrades an enchantment (on books it goes into `StoredEnchantments`).
    pub fn enchant(&mut self, name: &str, level: i32) {
        let key = if self.name() == "enchanted_book" { "StoredEnchantments" } else { "Enchantments" };
        let entry = Nbt::Compound(vec![("id".into(), Nbt::String(format!("minecraft:{name}"))), ("lvl".into(), Nbt::Short(level as i16))]);
        let tag = self.tag_mut();
        let mut list: Vec<Nbt> = tag.get(key).map(|l| l.as_list().to_vec()).unwrap_or_default();
        list.retain(|e| e.get("id").and_then(Nbt::as_str).map(|s| s.trim_start_matches("minecraft:")) != Some(name));
        list.push(entry);
        tag.set(key, Nbt::List(list));
        self.tidy();
    }

    pub fn is_enchanted(&self) -> bool {
        !self.enchantments().is_empty()
    }
}

pub fn read_slot(r: &mut Reader) -> Result<Slot> {
    if !r.bool()? {
        return Ok(None);
    }
    let id = r.varint()?;
    let count = r.i8()?;
    let nbt = Nbt::read_network(r)?.filter(|n| !n.is_empty_compound()).map(Box::new);
    Ok((count > 0 && id > 0).then_some(ItemStack { id, count, nbt }))
}

pub fn write_slot(out: &mut Vec<u8>, slot: &Slot) {
    match slot {
        Some(s) if s.count > 0 => {
            out.bool(true).varint(s.id).i8(s.count);
            match &s.nbt {
                Some(n) => n.write_network(out),
                None => {
                    out.u8(0);
                }
            }
        }
        _ => {
            out.bool(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_and_enchantments() {
        let id = crate::data::get().item_id("diamond_pickaxe").unwrap();
        let mut pick = ItemStack::new(id, 1);
        assert_eq!(pick.max_durability(), 1561);
        assert!(!pick.wear(10, &mut || 0.5));
        assert_eq!(pick.damage(), 10);
        pick.enchant("efficiency", 5);
        pick.enchant("unbreaking", 3);
        pick.enchant("efficiency", 4);
        assert_eq!(pick.enchantment("efficiency"), 4);
        assert_eq!(pick.enchantments().len(), 2);
        // With Unbreaking III and a high roll wear is never skipped; with a low roll it always is.
        pick.wear(1, &mut || 0.99);
        assert_eq!(pick.damage(), 11);
        pick.wear(1, &mut || 0.01);
        assert_eq!(pick.damage(), 11);
        // Round trip through the Slot format
        let mut buf = Vec::new();
        write_slot(&mut buf, &Some(pick.clone()));
        assert_eq!(read_slot(&mut Reader::new(&buf)).unwrap(), Some(pick));
    }
}

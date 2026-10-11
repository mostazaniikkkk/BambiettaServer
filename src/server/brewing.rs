//! Brewing stand: vanilla brewing recipes (PotionBrewing), blaze powder
//! fuel and the 20 s brew time.

use crate::data;
use crate::item::{ItemStack, Slot};
use crate::world::blockentity::Brewing;

/// Ticks a brew takes.
pub const BREW_TIME: i32 = 400;

/// Potion recipes: (input potion, ingredient, output potion).
const MIXES: &[(&str, &str, &str)] = &[
    ("water", "nether_wart", "awkward"),
    ("water", "glowstone_dust", "thick"),
    ("water", "redstone", "mundane"),
    ("water", "sugar", "mundane"),
    ("water", "rabbit_foot", "mundane"),
    ("water", "glistering_melon_slice", "mundane"),
    ("water", "spider_eye", "mundane"),
    ("water", "magma_cream", "mundane"),
    ("water", "blaze_powder", "mundane"),
    ("water", "ghast_tear", "mundane"),
    ("water", "fermented_spider_eye", "weakness"),
    ("awkward", "golden_carrot", "night_vision"),
    ("night_vision", "redstone", "long_night_vision"),
    ("night_vision", "fermented_spider_eye", "invisibility"),
    ("long_night_vision", "fermented_spider_eye", "long_invisibility"),
    ("invisibility", "redstone", "long_invisibility"),
    ("awkward", "magma_cream", "fire_resistance"),
    ("fire_resistance", "redstone", "long_fire_resistance"),
    ("awkward", "rabbit_foot", "leaping"),
    ("leaping", "redstone", "long_leaping"),
    ("leaping", "glowstone_dust", "strong_leaping"),
    ("leaping", "fermented_spider_eye", "slowness"),
    ("long_leaping", "fermented_spider_eye", "long_slowness"),
    ("slowness", "redstone", "long_slowness"),
    ("slowness", "glowstone_dust", "strong_slowness"),
    ("awkward", "turtle_helmet", "turtle_master"),
    ("turtle_master", "redstone", "long_turtle_master"),
    ("turtle_master", "glowstone_dust", "strong_turtle_master"),
    ("swiftness", "fermented_spider_eye", "slowness"),
    ("long_swiftness", "fermented_spider_eye", "long_slowness"),
    ("awkward", "sugar", "swiftness"),
    ("swiftness", "redstone", "long_swiftness"),
    ("swiftness", "glowstone_dust", "strong_swiftness"),
    ("awkward", "pufferfish", "water_breathing"),
    ("water_breathing", "redstone", "long_water_breathing"),
    ("awkward", "glistering_melon_slice", "healing"),
    ("healing", "glowstone_dust", "strong_healing"),
    ("healing", "fermented_spider_eye", "harming"),
    ("strong_healing", "fermented_spider_eye", "strong_harming"),
    ("harming", "glowstone_dust", "strong_harming"),
    ("poison", "fermented_spider_eye", "harming"),
    ("long_poison", "fermented_spider_eye", "harming"),
    ("strong_poison", "fermented_spider_eye", "strong_harming"),
    ("awkward", "spider_eye", "poison"),
    ("poison", "redstone", "long_poison"),
    ("poison", "glowstone_dust", "strong_poison"),
    ("awkward", "ghast_tear", "regeneration"),
    ("regeneration", "redstone", "long_regeneration"),
    ("regeneration", "glowstone_dust", "strong_regeneration"),
    ("awkward", "blaze_powder", "strength"),
    ("strength", "redstone", "long_strength"),
    ("strength", "glowstone_dust", "strong_strength"),
    ("weakness", "redstone", "long_weakness"),
    ("awkward", "phantom_membrane", "slow_falling"),
    ("slow_falling", "redstone", "long_slow_falling"),
];

pub fn is_ingredient(item: &str) -> bool {
    matches!(item, "gunpowder" | "dragon_breath") || MIXES.iter().any(|m| m.1 == item)
}

/// Result of brewing `bottle` with `ingredient`, if it changes.
pub fn mix(bottle: &ItemStack, ingredient: &str) -> Option<ItemStack> {
    let kind = bottle.name();
    if !matches!(kind, "potion" | "splash_potion" | "lingering_potion") {
        return None;
    }
    let potion = super::effects::potion_of(bottle).unwrap_or_else(|| "water".into());
    // Gunpowder: splash; dragon's breath: lingering.
    match (kind, ingredient) {
        ("potion", "gunpowder") => return super::effects::potion_item("splash_potion", &potion),
        ("splash_potion", "dragon_breath") => return super::effects::potion_item("lingering_potion", &potion),
        _ => {}
    }
    let out = MIXES.iter().find(|m| m.0 == potion && m.1 == ingredient)?.2;
    super::effects::potion_item(kind, out)
}

fn brewable(b: &Brewing) -> bool {
    let Some(ing) = &b.items[3] else { return false };
    b.items[0..3].iter().flatten().any(|bottle| mix(bottle, ing.name()).is_some())
}

/// One brewing stand tick (BrewingStandBlockEntity.serverTick). Returns whether a batch finished.
pub fn step(b: &mut Brewing) -> bool {
    let reg = data::get();
    if b.fuel <= 0 && b.items[4].as_ref().is_some_and(|f| f.name() == "blaze_powder") {
        b.fuel = 20;
        if let Some(f) = &mut b.items[4] {
            f.count -= 1;
            if f.count <= 0 {
                b.items[4] = None;
            }
        }
    }
    let can = brewable(b);
    let ingredient_id = b.items[3].as_ref().map_or(0, |i| i.id);
    if b.brew_time > 0 {
        b.brew_time -= 1;
        if b.brew_time == 0 && can {
            let ing = b.items[3].clone().unwrap();
            for i in 0..3 {
                if let Some(r) = b.items[i].as_ref().and_then(|bottle| mix(bottle, ing.name())) {
                    b.items[i] = Some(r);
                }
            }
            // Dragon's breath leaves the bottle behind.
            let rem: Slot = (ing.name() == "dragon_breath").then(|| reg.item_id("glass_bottle")).flatten().map(|id| ItemStack::new(id, 1));
            if let Some(s) = &mut b.items[3] {
                s.count -= 1;
                if s.count <= 0 {
                    b.items[3] = rem;
                }
            }
            return true;
        } else if !can || ingredient_id != b.ingredient {
            b.brew_time = 0;
        }
    } else if can && b.fuel > 0 {
        b.fuel -= 1;
        b.brew_time = BREW_TIME;
        b.ingredient = ingredient_id;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn it(n: &str, c: i8) -> Slot {
        Some(ItemStack::new(data::get().item_id(n).unwrap(), c))
    }

    #[test]
    fn brews_awkward_then_swiftness() {
        let water = super::super::effects::potion_item("potion", "water");
        let mut b = Brewing { items: vec![water.clone(), water.clone(), None, it("nether_wart", 2), it("blaze_powder", 1)], ..Default::default() };
        let mut done = 0;
        for _ in 0..(BREW_TIME + 5) {
            done += step(&mut b) as i32;
        }
        assert_eq!(done, 1);
        assert_eq!(super::super::effects::potion_of(b.items[0].as_ref().unwrap()).as_deref(), Some("awkward"));
        assert_eq!(b.items[3].as_ref().unwrap().count, 1);
        assert_eq!(b.fuel, 19);
        assert!(b.items[4].is_none());
        // Sugar -> swiftness; gunpowder -> splash.
        let s = mix(b.items[0].as_ref().unwrap(), "sugar").unwrap();
        assert_eq!(super::super::effects::potion_of(&s).as_deref(), Some("swiftness"));
        let splash = mix(&s, "gunpowder").unwrap();
        assert_eq!(splash.name(), "splash_potion");
        assert!(mix(&s, "nether_wart").is_none());
    }
}

//! Status effects (potions, golden apples, milk, spoiled food) and potions:
//! which effects each potion gives, applying/removing/syncing effects and their per-tick effect.

use super::{Player, PlayerState, Server};
use crate::item::ItemStack;
use crate::protocol::{id, packet, Nbt, Writer};
use std::sync::Arc;

/// 1.20.4 `minecraft:mob_effect` registry (the index is the network id).
pub const NAMES: [&str; 33] = [
    "speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea",
    "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger",
    "weakness", "poison", "wither", "health_boost", "absorption", "saturation", "glowing", "levitation", "luck", "unluck",
    "slow_falling", "conduit_power", "dolphins_grace", "bad_omen", "hero_of_the_village", "darkness",
];

/// Particle color of each effect (vanilla MobEffects).
const COLORS: [i32; 33] = [
    0x33EBFF, 0x8BAFE0, 0xD9C043, 0x4A4217, 0xFFC700, 0xF82423, 0xA9656A, 0xFDFF84, 0x551D4A, 0xCD5CAB, 0x9146F0, 0xFF9900,
    0x98DAC0, 0xF6F6F6, 0x1F1F23, 0xC2FF66, 0x587653, 0x484D48, 0x87A363, 0x736156, 0xF87D23, 0x2552A5, 0xF82423, 0x94A061,
    0xCEFFFF, 0x59C106, 0xC0A44D, 0xF3CFB9, 0x1DC2D1, 0x88A3BE, 0x0B6138, 0x44FF44, 0x292721,
];

#[derive(Clone, Debug, PartialEq)]
pub struct ActiveEffect {
    pub id: i32,
    pub amp: i32,
    /// Remaining ticks.
    pub duration: i32,
}

pub fn id(name: &str) -> Option<i32> {
    let name = name.trim_start_matches("minecraft:");
    NAMES.iter().position(|n| *n == name).map(|i| i as i32)
}

/// Particle color of an effect (by name).
pub fn color(name: &str) -> i32 {
    id(name).map_or(0x385DC6, |i| COLORS[i as usize])
}

pub fn name(id: i32) -> &'static str {
    NAMES.get(id as usize).copied().unwrap_or("speed")
}

// ---------- potions ----------

/// Effects of a potion (`minecraft:potion` registry): (effect, amplifier, duration in ticks).
pub fn potion_effects(potion: &str) -> Vec<(&'static str, i32, i32)> {
    let p = potion.trim_start_matches("minecraft:");
    let (long, strong) = (p.starts_with("long_"), p.starts_with("strong_"));
    let base = p.trim_start_matches("long_").trim_start_matches("strong_");
    let t = |normal: i32, long_t: i32, strong_t: i32| if long { long_t } else if strong { strong_t } else { normal };
    let amp = i32::from(strong);
    match base {
        "night_vision" => vec![("night_vision", 0, t(3600, 9600, 3600))],
        "invisibility" => vec![("invisibility", 0, t(3600, 9600, 3600))],
        "leaping" => vec![("jump_boost", amp, t(3600, 9600, 1800))],
        "fire_resistance" => vec![("fire_resistance", 0, t(3600, 9600, 3600))],
        "swiftness" => vec![("speed", amp, t(3600, 9600, 1800))],
        "slowness" => vec![("slowness", if strong { 3 } else { 0 }, t(1800, 4800, 400))],
        "turtle_master" => {
            let d = t(400, 800, 400);
            vec![("slowness", if strong { 5 } else { 3 }, d), ("resistance", if strong { 3 } else { 2 }, d)]
        }
        "water_breathing" => vec![("water_breathing", 0, t(3600, 9600, 3600))],
        "healing" => vec![("instant_health", amp, 1)],
        "harming" => vec![("instant_damage", amp, 1)],
        "poison" => vec![("poison", amp, t(900, 1800, 432))],
        "regeneration" => vec![("regeneration", amp, t(900, 1800, 450))],
        "strength" => vec![("strength", amp, t(3600, 9600, 1800))],
        "weakness" => vec![("weakness", 0, t(1800, 4800, 1800))],
        "luck" => vec![("luck", 0, 6000)],
        "slow_falling" => vec![("slow_falling", 0, t(1800, 4800, 1800))],
        _ => vec![],
    }
}

/// All potions with an effect or base.
#[cfg_attr(not(test), allow(dead_code))]
pub const POTIONS: &[&str] = &[
    "water", "mundane", "thick", "awkward", "night_vision", "long_night_vision", "invisibility", "long_invisibility", "leaping",
    "long_leaping", "strong_leaping", "fire_resistance", "long_fire_resistance", "swiftness", "long_swiftness", "strong_swiftness",
    "slowness", "long_slowness", "strong_slowness", "turtle_master", "long_turtle_master", "strong_turtle_master", "water_breathing",
    "long_water_breathing", "healing", "strong_healing", "harming", "strong_harming", "poison", "long_poison", "strong_poison",
    "regeneration", "long_regeneration", "strong_regeneration", "strength", "long_strength", "strong_strength", "weakness",
    "long_weakness", "luck", "slow_falling", "long_slow_falling",
];

/// Potion of an item (`potion`, `splash_potion`, `lingering_potion`, `tipped_arrow`).
pub fn potion_of(item: &ItemStack) -> Option<String> {
    let p = item.nbt.as_ref()?.get_str("Potion")?;
    Some(p.trim_start_matches("minecraft:").to_string())
}

pub fn potion_item(item: &str, potion: &str) -> Option<ItemStack> {
    let id = crate::data::get().item_id(item)?;
    let mut s = ItemStack::new(id, 1);
    s.nbt = Some(Box::new(Nbt::compound().with("Potion", Nbt::str(&format!("minecraft:{potion}")))));
    Some(s)
}

/// Effects of eating something (golden apples, pufferfish, rotten flesh...): (effect, amp, ticks, probability).
pub fn food_effects(food: &str) -> Vec<(&'static str, i32, i32, f32)> {
    match food {
        "golden_apple" => vec![("regeneration", 1, 100, 1.0), ("absorption", 0, 2400, 1.0)],
        "enchanted_golden_apple" => vec![("regeneration", 1, 400, 1.0), ("resistance", 0, 6000, 1.0), ("fire_resistance", 0, 6000, 1.0), ("absorption", 3, 2400, 1.0)],
        "pufferfish" => vec![("poison", 1, 1200, 1.0), ("hunger", 2, 300, 1.0), ("nausea", 0, 300, 1.0)],
        "rotten_flesh" => vec![("hunger", 0, 600, 0.8)],
        "spider_eye" => vec![("poison", 0, 100, 1.0)],
        "chicken" => vec![("hunger", 0, 600, 0.3)],
        "poisonous_potato" => vec![("poison", 0, 100, 0.6)],
        _ => vec![],
    }
}

// ---------- queries ----------

/// Amplifier of the effect if the player has it.
pub fn level(st: &PlayerState, name: &str) -> Option<i32> {
    let id = id(name)?;
    st.effects.iter().find(|e| e.id == id).map(|e| e.amp)
}

pub fn has(st: &PlayerState, name: &str) -> bool {
    level(st, name).is_some()
}

/// Particle color to display (0 = none).
pub fn particle_color(st: &PlayerState) -> i32 {
    st.effects.iter().find(|e| name(e.id) != "invisibility").map_or(0, |e| COLORS[e.id as usize])
}

// ---------- packets ----------

fn effect_packet(eid: i32, e: &ActiveEffect) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_EFFECT);
    // Flags: 0x02 particles, 0x04 icon.
    p.varint(eid).varint(e.id).i8(e.amp.clamp(0, 127) as i8).varint(e.duration).i8(0x06).bool(false);
    p
}

fn remove_packet(eid: i32, effect: i32) -> Vec<u8> {
    let mut p = packet(id::play::REMOVE_ENTITY_EFFECT);
    p.varint(eid).varint(effect);
    p
}

/// Speed `update_attributes`: the client applies the Speed/Slowness modifier the server
/// sends it (and the sprinting one, included so it isn't removed).
pub fn attributes_packet(eid: i32, st: &PlayerState) -> Vec<u8> {
    let uuid = |s: &str| u128::from_str_radix(&s.replace('-', ""), 16).unwrap_or(0);
    let mut mods: Vec<(u128, f64)> = Vec::new();
    if st.sprinting {
        mods.push((uuid("662A6B8D-DA3E-4C1C-8813-96EA6097278D"), 0.3));
    }
    if let Some(a) = level(st, "speed") {
        mods.push((uuid("91AEAA56-376B-4498-935B-2F7F68070635"), 0.2 * (a + 1) as f64));
    }
    if let Some(a) = level(st, "slowness") {
        mods.push((uuid("7107DE5E-7CE8-4030-940E-514C1F160890"), -0.15 * (a + 1) as f64));
    }
    let mut p = packet(id::play::ENTITY_UPDATE_ATTRIBUTES);
    p.varint(eid).varint(1).string("minecraft:generic.movement_speed").f64(0.1).varint(mods.len() as i32);
    for (u, amount) in mods {
        // Operation 2: multiply the total.
        p.uuid(u).f64(amount).i8(2);
    }
    p
}

/// Resends all effects (on join, respawn or dimension change).
pub fn resend(player: &Player) {
    let st = player.state.lock().unwrap();
    for e in &st.effects {
        player.send(effect_packet(player.eid, e));
    }
    player.send(attributes_packet(player.eid, &st));
    player.send(absorption_packet(player.eid, st.absorption));
}

fn absorption_packet(eid: i32, absorption: f32) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(eid).u8(15).varint(3).f32(absorption).u8(0xff);
    p
}

// ---------- applying ----------

/// Gives an effect to the player (instant ones are applied immediately). `potency` scales the
/// duration and healing of thrown potions according to distance.
pub fn add(server: &Arc<Server>, player: &Arc<Player>, name: &str, amp: i32, duration: i32, potency: f64) {
    let Some(eid) = id(name) else { return };
    match name {
        "instant_health" => {
            let heal = ((4 << amp.clamp(0, 10)) as f64 * potency) as f32;
            let mut st = player.state.lock().unwrap();
            if !st.dead {
                st.health = (st.health + heal).min(20.0);
                player.send(super::health::health_packet(st.health, st.food, st.saturation));
            }
            return;
        }
        "instant_damage" => {
            let dmg = ((6 << amp.clamp(0, 10)) as f64 * potency) as f32;
            player.state.lock().unwrap().hurt_cooldown = 0;
            super::health::damage(server, player, dmg, "magic", None);
            return;
        }
        _ => {}
    }
    let duration = ((duration as f64 * potency) as i32).max(1);
    let (packets, meta_changed) = {
        let mut st = player.state.lock().unwrap();
        if st.dead {
            return;
        }
        let before_color = particle_color(&st);
        let before_invisible = has(&st, "invisibility");
        let new = ActiveEffect { id: eid, amp, duration };
        let applied = match st.effects.iter_mut().find(|e| e.id == eid) {
            Some(e) if amp > e.amp || (amp == e.amp && duration > e.duration) => {
                *e = new.clone();
                true
            }
            Some(_) => false,
            None => {
                st.effects.push(new.clone());
                true
            }
        };
        if !applied {
            return;
        }
        let mut packets = vec![effect_packet(player.eid, &new)];
        if name == "absorption" {
            st.absorption = st.absorption.max(4.0 * (amp + 1) as f32);
            packets.push(absorption_packet(player.eid, st.absorption));
        }
        if matches!(name, "speed" | "slowness") {
            packets.push(attributes_packet(player.eid, &st));
        }
        (packets, before_color != particle_color(&st) || before_invisible != has(&st, "invisibility"))
    };
    for p in packets {
        server.broadcast_tracked(player.eid, &p, true);
    }
    if meta_changed {
        server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
    }
}

/// Removes all effects (milk). Returns whether there were any.
pub fn clear(server: &Arc<Server>, player: &Arc<Player>) -> bool {
    let removed: Vec<i32> = {
        let mut st = player.state.lock().unwrap();
        st.absorption = 0.0;
        std::mem::take(&mut st.effects).into_iter().map(|e| e.id).collect()
    };
    finish_removal(server, player, &removed);
    !removed.is_empty()
}

/// Removes a specific effect (honey removes poison).
pub fn remove(server: &Arc<Server>, player: &Arc<Player>, name: &str) {
    let Some(eid) = id(name) else { return };
    let had = {
        let mut st = player.state.lock().unwrap();
        let n = st.effects.len();
        st.effects.retain(|e| e.id != eid);
        n != st.effects.len()
    };
    if had {
        finish_removal(server, player, &[eid]);
    }
}

fn finish_removal(server: &Arc<Server>, player: &Arc<Player>, removed: &[i32]) {
    if removed.is_empty() {
        return;
    }
    for &e in removed {
        server.broadcast_tracked(player.eid, &remove_packet(player.eid, e), true);
        if name(e) == "absorption" {
            player.state.lock().unwrap().absorption = 0.0;
            server.broadcast_tracked(player.eid, &absorption_packet(player.eid, 0.0), true);
        }
    }
    let attrs = attributes_packet(player.eid, &player.state.lock().unwrap());
    player.send(attrs);
    server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
}

/// Applies the effects of a potion (drunk or thrown).
pub fn apply_potion(server: &Arc<Server>, player: &Arc<Player>, potion: &str, potency: f64) {
    for (name, amp, dur) in potion_effects(potion) {
        add(server, player, name, amp, dur, potency);
    }
}

/// Per-player effect tick: duration, regeneration, poison, wither and hunger.
pub fn tick(server: &Arc<Server>, player: &Arc<Player>) {
    let mut hurt: Vec<(f32, &'static str)> = Vec::new();
    let mut expired = Vec::new();
    {
        let mut st = player.state.lock().unwrap();
        if st.dead || st.effects.is_empty() {
            return;
        }
        let mut heal = 0.0;
        let mut exhaustion = 0.0;
        for e in st.effects.iter_mut() {
            let every = |base: i32| {
                let k = base >> e.amp.clamp(0, 31);
                k <= 0 || e.duration % k == 0
            };
            match name(e.id) {
                "regeneration" if every(50) => heal += 1.0,
                "poison" if every(25) => hurt.push((1.0, "magic")),
                "wither" if every(40) => hurt.push((1.0, "wither")),
                "hunger" => exhaustion += 0.005 * (e.amp + 1) as f32,
                _ => {}
            }
            e.duration -= 1;
            if e.duration <= 0 {
                expired.push(e.id);
            }
        }
        st.effects.retain(|e| e.duration > 0);
        if heal > 0.0 && st.health < 20.0 {
            st.health = (st.health + heal).min(20.0);
            player.send(super::health::health_packet(st.health, st.food, st.saturation));
        }
        st.exhaustion += exhaustion;
        // Poison doesn't kill (leaves half a heart).
        if st.health <= 1.0 {
            hurt.retain(|h| h.1 != "magic");
        }
    }
    for (amount, kind) in hurt {
        player.state.lock().unwrap().hurt_cooldown = 0;
        super::health::damage(server, player, amount, kind, None);
    }
    finish_removal(server, player, &expired);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potions() {
        assert_eq!(potion_effects("strong_swiftness"), vec![("speed", 1, 1800)]);
        assert_eq!(potion_effects("minecraft:long_fire_resistance"), vec![("fire_resistance", 0, 9600)]);
        assert_eq!(potion_effects("turtle_master").len(), 2);
        assert!(potion_effects("awkward").is_empty());
        assert_eq!(id("speed"), Some(0));
        assert_eq!(id("darkness"), Some(32));
        for p in POTIONS {
            for (e, _, _) in potion_effects(p) {
                assert!(id(e).is_some(), "{e}");
            }
        }
    }
}

//! Horses, donkeys, mules and llamas: taming by riding them (they buck until they trust you),
//! saddles (to steer a horse, donkey or mule: the client drives it, like a boat), horse armor,
//! chests on donkeys, mules and llamas, carpets on llamas, their inventory window, feeding,
//! breeding (a horse and a donkey have a mule) and llamas spitting.

use super::inventory::WindowKind;
use super::mobs::{self, Mob, MobKind};
use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::{ItemStack, Slot};
use crate::protocol::{id, packet, Writer};
use std::sync::Arc;

pub fn is_horse(kind: MobKind) -> bool {
    matches!(kind, MobKind::Horse | MobKind::Donkey | MobKind::Mule | MobKind::Llama | MobKind::Camel)
}

/// Can carry a chest.
fn chested(kind: MobKind) -> bool {
    matches!(kind, MobKind::Donkey | MobKind::Mule | MobKind::Llama)
}

/// Slots of the chest it carries.
fn chest_slots(m: &Mob) -> usize {
    if m.kind == MobKind::Llama { 3 * m.strength.clamp(1, 5) as usize } else { 15 }
}

/// Protection of horse armor.
pub fn armor_points(item: &str) -> f32 {
    match item {
        "leather_horse_armor" => 3.0,
        "iron_horse_armor" => 5.0,
        "golden_horse_armor" => 7.0,
        "diamond_horse_armor" => 11.0,
        _ => 0.0,
    }
}

/// Carpet colour shown on a llama (dye id), -1 without one.
fn carpet_color(item: &Slot) -> i32 {
    const COLORS: [&str; 16] = ["white", "orange", "magenta", "light_blue", "yellow", "lime", "pink", "gray", "light_gray", "cyan", "purple", "blue", "brown", "green", "red", "black"];
    item.as_ref().and_then(|i| i.name().strip_suffix("_carpet")).and_then(|c| COLORS.iter().position(|x| *x == c)).map_or(-1, |i| i as i32)
}

/// A random new horse (or donkey, mule, llama): looks, speed, jump and, for llamas, strength.
pub fn init(m: &mut Mob) {
    let r = || entities::random() as f64;
    match m.kind {
        MobKind::Horse => {
            m.variant = (r() * 7.0) as i32 | ((r() * 5.0) as i32) << 8;
            m.speed_attr = (0.45 + r() * 0.3 + r() * 0.3 + r() * 0.3) * 0.25;
            m.jump_attr = 0.4 + r() * 0.2 + r() * 0.2 + r() * 0.2;
        }
        MobKind::Llama => {
            m.variant = (r() * 4.0) as i32;
            m.strength = 1 + (r() * 3.0) as i32 + i32::from(r() < 0.04) * 2;
            m.speed_attr = 0.175;
            m.jump_attr = 0.5;
        }
        MobKind::Camel => {
            m.speed_attr = 0.09;
            m.jump_attr = 0.42;
        }
        _ => {
            m.speed_attr = 0.175;
            m.jump_attr = 0.5;
        }
    }
}

/// Metadata of these mobs (from index 17).
pub fn metadata(m: &Mob, p: &mut Vec<u8>) {
    // 17: tamed, saddled, bred, eating, rearing.
    let flags = if m.owner.is_some() { 0x02 } else { 0 } | if m.saddle { 0x04 } else { 0 } | if m.stand > 0 { 0x20 } else { 0 };
    p.u8(17).varint(0).i8(flags);
    match m.kind {
        MobKind::Horse => {
            p.u8(18).varint(1).varint(m.variant);
        }
        MobKind::Donkey | MobKind::Mule => {
            p.u8(18).varint(8).bool(m.chest.is_some());
        }
        MobKind::Llama => {
            p.u8(18).varint(8).bool(m.chest.is_some());
            p.u8(19).varint(1).varint(m.strength);
            p.u8(20).varint(1).varint(carpet_color(&m.armor));
            p.u8(21).varint(1).varint(m.variant);
        }
        _ => {}
    }
}

/// Speed and jump strength, which the client needs to drive it.
pub fn attributes(m: &Mob) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_UPDATE_ATTRIBUTES);
    p.varint(m.eid).varint(2);
    p.string("minecraft:generic.movement_speed").f64(m.speed_attr).varint(0);
    p.string("minecraft:horse.jump_strength").f64(m.jump_attr).varint(0);
    p
}

pub fn spawn_extras(m: &Mob, out: &mut Vec<Vec<u8>>) {
    out.push(attributes(m));
    if m.kind == MobKind::Horse && m.armor.is_some() {
        out.push(mobs::equipment_stack(m.eid, 4, m.armor.clone()));
    }
    if m.rider.is_some() {
        out.push(passengers(m.eid, m.rider));
    }
}

pub fn passengers(eid: i32, rider: Option<i32>) -> Vec<u8> {
    let mut p = packet(id::play::SET_PASSENGERS);
    p.varint(eid);
    match rider {
        Some(r) => p.varint(1).varint(r),
        None => p.varint(0),
    };
    p
}

/// Driven by its rider (tamed and saddled; llamas cannot be steered).
pub fn controlled(m: &Mob) -> bool {
    m.rider.is_some() && m.owner.is_some() && m.saddle && m.kind != MobKind::Llama
}

/// Seat height for the rider.
pub fn seat(m: &Mob) -> f64 {
    m.size().1 * 0.75 - 0.35
}

/// Every tick while somebody rides it untamed: it bucks until it trusts the rider.
pub fn ridden_tick(m: &mut Mob, actions: &mut Vec<mobs::Action>) {
    m.stand = m.stand.saturating_sub(1);
    let Some(rider) = m.rider else { return };
    if m.owner.is_some() {
        return;
    }
    m.burst -= 1;
    if m.burst > 0 {
        return;
    }
    if entities::random() * 100.0 < m.temper as f32 {
        actions.push(mobs::Action::Tame(m.eid, rider));
    } else {
        // Throws the rider off.
        m.temper = (m.temper + 5).min(100);
        m.stand = 20;
        actions.push(mobs::Action::Buck(m.eid, rider));
        actions.push(mobs::Action::Sound(m.eid, format!("entity.{}.angry", m.kind.name()), sound::NEUTRAL));
    }
}

// ---------- interaction ----------

fn give_back(player: &Player, item: ItemStack) -> Option<ItemStack> {
    let mut st = player.state.lock().unwrap();
    let left = st.inv.add(item).0;
    let p = st.inv.window_items();
    player.send(p);
    left
}

fn consume(player: &Player, hand: i32) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 {
        return;
    }
    let held = st.held_slot;
    let slot = st.inv.consume_held(hand, held, 1, None);
    st.sync_slot(player, slot);
}

/// Right click on a horse-like mob. Returns whether it was one.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32, held: Option<&ItemStack>) -> bool {
    let sneaking = player.state.lock().unwrap().sneaking;
    let name = held.map(|h| h.name()).unwrap_or("");
    enum Do {
        Nothing,
        Consume(Vec<Vec<u8>>, Option<ItemStack>),
        Mount,
        Inventory,
    }
    let what = {
        let mut map = server.mobs.lock().unwrap();
        let Some(m) = map.get_mut(&eid).filter(|m| is_horse(m.kind) && m.death_ticks.is_none()) else { return false };
        let tamed = m.owner.is_some();
        let adult = m.grow >= 0;
        if tamed && sneaking && adult {
            Do::Inventory
        } else if name == "saddle" && tamed && adult && !m.saddle && m.kind != MobKind::Llama {
            m.saddle = true;
            Do::Consume(vec![mobs::metadata(m)], None)
        } else if m.kind == MobKind::Horse && tamed && adult && armor_points(name) > 0.0 {
            let old = m.armor.replace(held.unwrap().with_count(1));
            Do::Consume(vec![mobs::equipment_stack(m.eid, 4, m.armor.clone())], old)
        } else if m.kind == MobKind::Llama && tamed && adult && name.ends_with("_carpet") {
            let old = m.armor.replace(held.unwrap().with_count(1));
            Do::Consume(vec![mobs::metadata(m)], old)
        } else if name == "chest" && chested(m.kind) && tamed && adult && m.chest.is_none() {
            m.chest = Some(vec![None; chest_slots(m)]);
            Do::Consume(vec![mobs::metadata(m)], None)
        } else if let Some(heal) = food_value(m.kind, name) {
            // Food heals and makes it trust you; golden food (hay for llamas) makes tamed adults fall in love.
            let love = tamed && adult && m.grow == 0 && m.love == 0 && breeding_food(m.kind, name);
            if m.health >= m.max_health() && !love && m.temper >= 100 && adult {
                Do::Nothing
            } else {
                m.health = (m.health + heal).min(m.max_health());
                m.temper = (m.temper + 3).min(100);
                if m.grow < 0 {
                    m.grow += -m.grow / 10;
                }
                if love {
                    m.love = 600;
                    m.love_by = Some(player.eid);
                }
                Do::Consume(vec![super::animals::status(m.eid, 18), mobs::metadata(m)], None)
            }
        } else if adult && m.rider.is_none() && !sneaking {
            Do::Mount
        } else {
            Do::Nothing
        }
    };
    match what {
        Do::Nothing => false,
        Do::Consume(packets, old) => {
            consume(player, hand);
            for p in packets {
                server.broadcast_tracked(eid, &p, true);
            }
            if let Some(o) = old {
                if let Some(l) = give_back(player, o) {
                    entities::throw_from(server, player, l);
                }
            }
            let snd = match name {
                "saddle" => "entity.horse.saddle",
                n if n.ends_with("_horse_armor") => "entity.horse.armor",
                n if n.ends_with("_carpet") => "entity.llama.swag",
                "chest" => "entity.donkey.chest",
                _ => "entity.horse.eat",
            };
            sound::entity(server, eid, snd, sound::NEUTRAL, 1.0, 1.0);
            true
        }
        Do::Mount => {
            mount(server, player, eid);
            true
        }
        Do::Inventory => {
            open_inventory(server, player, eid);
            true
        }
    }
}

fn food_value(kind: MobKind, item: &str) -> Option<f32> {
    match (kind, item) {
        (MobKind::Llama, "wheat") => Some(2.0),
        (MobKind::Llama, "hay_block") => Some(10.0),
        (MobKind::Llama, _) => None,
        (MobKind::Camel, "cactus") => Some(2.0),
        (MobKind::Camel, _) => None,
        (_, "sugar") => Some(1.0),
        (_, "wheat") => Some(2.0),
        (_, "apple") => Some(3.0),
        (_, "golden_carrot") => Some(4.0),
        (_, "golden_apple" | "enchanted_golden_apple") => Some(10.0),
        (_, "hay_block") => Some(20.0),
        _ => None,
    }
}

fn breeding_food(kind: MobKind, item: &str) -> bool {
    match kind {
        MobKind::Llama => item == "hay_block",
        MobKind::Camel => item == "cactus",
        MobKind::Mule => false,
        _ => matches!(item, "golden_carrot" | "golden_apple" | "enchanted_golden_apple"),
    }
}

/// Is `item` what this kind breeds with (also used for following).
pub fn is_food(kind: MobKind, item: &str) -> bool {
    breeding_food(kind, item)
}

fn mount(server: &Arc<Server>, player: &Arc<Player>, eid: i32) {
    let packet_out = {
        let mut map = server.mobs.lock().unwrap();
        let Some(m) = map.get_mut(&eid) else { return };
        m.rider = Some(player.eid);
        m.burst = 20 + (entities::random() * 40.0) as i32;
        m.path.clear();
        m.wander = None;
        passengers(eid, m.rider)
    };
    {
        let mut st = player.state.lock().unwrap();
        st.vehicle = Some(eid);
        st.sleeping = None;
    }
    server.broadcast_tracked(eid, &packet_out, false);
    player.send(packet_out);
}

/// The rider gets off (sneaking, bucked, the mount died).
pub fn unmount(server: &Arc<Server>, player: &Arc<Player>, eid: i32, teleport: bool) {
    let out = {
        let mut map = server.mobs.lock().unwrap();
        map.get_mut(&eid).filter(|m| m.rider == Some(player.eid)).map(|m| {
            m.rider = None;
            (m.pos, m.size().1)
        })
    };
    {
        let mut st = player.state.lock().unwrap();
        if st.vehicle == Some(eid) {
            st.vehicle = None;
        }
    }
    let p = passengers(eid, None);
    server.broadcast_tracked(eid, &p, false);
    player.send(p);
    if let (Some((pos, _)), true) = (out, teleport) {
        let a = entities::random() as f64 * std::f64::consts::TAU;
        super::play::teleport(server, player, pos[0] + a.cos() * 1.2, pos[1] + 0.1, pos[2] + a.sin() * 1.2);
    }
}

/// The client driving its horse reports where it is.
pub fn client_moved(server: &Arc<Server>, player: &Arc<Player>, eid: i32, pos: [f64; 3], yaw: f32) -> bool {
    let seat = {
        let mut map = server.mobs.lock().unwrap();
        let Some(m) = map.get_mut(&eid).filter(|m| m.rider == Some(player.eid)) else { return false };
        if !controlled(m) || mobs::dist2(&m.pos, &pos) > 100.0 {
            return true;
        }
        m.pos = pos;
        m.yaw = yaw;
        m.vel = [0.0; 3];
        m.fall = 0.0;
        seat(m)
    };
    follow(player, pos, seat);
    true
}

/// The rider goes where its mount goes.
pub fn follow(player: &Player, pos: [f64; 3], seat: f64) {
    let mut st = player.state.lock().unwrap();
    let p = [pos[0], pos[1] + seat, pos[2]];
    (st.x, st.y, st.z) = (p[0], p[1], p[2]);
    st.fall_distance = 0.0;
    st.ac.last_valid = p;
    st.ac.last_ground = p;
}

// ---------- inventory ----------

pub fn open_inventory(server: &Arc<Server>, player: &Arc<Player>, eid: i32) {
    let slots = {
        let map = server.mobs.lock().unwrap();
        let Some(m) = map.get(&eid).filter(|m| is_horse(m.kind) && m.owner.is_some()) else { return };
        let saddle = m.saddle.then(|| data::get().item_id("saddle").map(|i| ItemStack::new(i, 1))).flatten();
        let mut s = vec![saddle, m.armor.clone()];
        if let Some(c) = &m.chest {
            s.extend(c.iter().cloned());
        }
        s
    };
    let mut st = player.state.lock().unwrap();
    for p in st.inv.open_horse(eid, slots) {
        player.send(p);
    }
}

/// After a click in a horse's window: its saddle, armor and chest follow the window.
pub fn sync_window(server: &Arc<Server>, player: &Arc<Player>) {
    let Some((eid, slots)) = ({
        let st = player.state.lock().unwrap();
        st.inv.window.as_ref().filter(|w| w.kind == WindowKind::Horse).map(|w| (w.station.merchant, w.slots.clone()))
    }) else {
        return;
    };
    let packets = {
        let mut map = server.mobs.lock().unwrap();
        let Some(m) = map.get_mut(&eid) else { return };
        m.saddle = slots.first().is_some_and(|s| s.is_some()) && m.kind != MobKind::Llama;
        m.armor = slots.get(1).cloned().flatten();
        if let Some(c) = &mut m.chest {
            for (i, s) in c.iter_mut().enumerate() {
                *s = slots.get(2 + i).cloned().flatten();
            }
        }
        let mut out = vec![mobs::metadata(m)];
        if m.kind == MobKind::Horse {
            out.push(mobs::equipment_stack(eid, 4, m.armor.clone()));
        }
        out
    };
    for p in packets {
        server.broadcast_tracked(eid, &p, true);
    }
}

/// What it drops when it dies: saddle, armor, chest and its contents.
pub fn belongings(m: &Mob) -> Vec<ItemStack> {
    let reg = data::get();
    let mut out = Vec::new();
    if m.saddle {
        out.extend(reg.item_id("saddle").map(|i| ItemStack::new(i, 1)));
    }
    out.extend(m.armor.clone());
    if let Some(c) = &m.chest {
        out.extend(reg.item_id("chest").map(|i| ItemStack::new(i, 1)));
        out.extend(c.iter().flatten().cloned());
    }
    out
}

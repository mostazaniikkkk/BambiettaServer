//! Ranged weapons and the player's shield: bow (charging, Power, Punch, Flame, Infinity),
//! crossbow (loading, Quick Charge, Multishot, Piercing), trident (throwing, Loyalty,
//! Riptide) and shield (blocks what comes from the front, an axe disables it).

use super::projectiles::{look, ArrowData, ProjectileKind};
use super::{health, sound, Player, PlayerState, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, Nbt, Writer};
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UseKind {
    Bow,
    Crossbow,
    Trident,
    Shield,
}

/// Item the player is using (drawing the bow, loading the crossbow, with the shield
/// raised...).
#[derive(Clone, Debug)]
pub struct Using {
    pub hand: i32,
    pub start: i64,
    pub kind: UseKind,
}

fn now(server: &Server) -> i64 {
    server.age.load(Ordering::Relaxed)
}

fn is_arrow(name: &str) -> bool {
    matches!(name, "arrow" | "tipped_arrow" | "spectral_arrow")
}

/// Ammo like vanilla: offhand first, then the hotbar and the inventory.
fn find_ammo(st: &PlayerState) -> Option<usize> {
    let order = std::iter::once(super::inventory::OFFHAND).chain(super::inventory::HOTBAR..super::inventory::HOTBAR + 9).chain(9..36);
    order.into_iter().find(|&i| st.inv.slots[i].as_ref().is_some_and(|s| is_arrow(s.name())))
}

fn arrow_item(name: &str) -> ItemStack {
    ItemStack::new(data::get().item_id(name).unwrap_or(0), 1)
}

/// Crossbow charge time (25 ticks, -5 per Quick Charge level).
fn crossbow_charge(item: &ItemStack) -> i64 {
    (25 - 5 * item.enchantment("quick_charge") as i64).max(0)
}

fn charged(item: &ItemStack) -> bool {
    item.nbt.as_ref().and_then(|n| n.get_i64("Charged")).unwrap_or(0) != 0
}

fn held_slot(st: &PlayerState, hand: i32) -> usize {
    if hand == 1 { super::inventory::OFFHAND } else { super::inventory::HOTBAR + st.held_slot }
}

/// `use_item` with a ranged weapon or a shield. Returns whether it was one.
pub fn start(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let t = now(server);
    let kind = {
        let mut st = player.state.lock().unwrap();
        let Some(item) = st.inv.held(hand, st.held_slot).clone() else { return false };
        let creative = st.game_mode == 1;
        let kind = match item.name() {
            "bow" => UseKind::Bow,
            "crossbow" => UseKind::Crossbow,
            "trident" => UseKind::Trident,
            "shield" => UseKind::Shield,
            _ => return false,
        };
        if st.dead {
            return true;
        }
        match kind {
            UseKind::Crossbow if charged(&item) => {
                drop(st);
                shoot_crossbow(server, player, hand);
                return true;
            }
            UseKind::Bow | UseKind::Crossbow if !creative && find_ammo(&st).is_none() => return true,
            // A trident about to break is not thrown.
            UseKind::Trident if item.max_durability() > 0 && item.damage() >= item.max_durability() - 1 => return true,
            UseKind::Shield if st.shield_disabled_until > t => return true,
            _ => {}
        }
        st.using = Some(Using { hand, start: t, kind });
        st.eating = None;
        kind
    };
    server.broadcast_tracked(player.eid, &super::play::metadata(player), false);
    if kind == UseKind::Crossbow {
        sound::at(server, player.dim(), pos(player), "item.crossbow.loading_start", sound::PLAYER, 0.5, 1.0);
    }
    true
}

fn pos(player: &Player) -> [f64; 3] {
    let st = player.state.lock().unwrap();
    [st.x, st.y + 1.0, st.z]
}

/// Releasing the use button (`block_dig` packet 5): fires the bow, loads the crossbow or
/// throws the trident; lowers the shield.
pub fn release(server: &Arc<Server>, player: &Arc<Player>) {
    let Some(u) = player.state.lock().unwrap().using.take() else { return };
    let ticks = now(server) - u.start;
    match u.kind {
        UseKind::Bow => shoot_bow(server, player, u.hand, ticks),
        UseKind::Crossbow => load_crossbow(server, player, u.hand, ticks),
        UseKind::Trident => throw_trident(server, player, u.hand, ticks),
        UseKind::Shield => {}
    }
    server.broadcast_tracked(player.eid, &super::play::metadata(player), false);
}

/// Switching the held item cancels the use.
pub fn cancel(server: &Arc<Server>, player: &Arc<Player>) {
    if player.state.lock().unwrap().using.take().is_some() {
        server.broadcast_tracked(player.eid, &super::play::metadata(player), false);
    }
}

/// Bow power based on how far it was drawn (BowItem.getPowerForTime).
pub fn bow_power(ticks: i64) -> f64 {
    let f = ticks as f64 / 20.0;
    ((f * f + f * 2.0) / 3.0).min(1.0)
}

/// Takes an arrow from the ammo. Returns the arrow and whether it can be picked up (0/1/2).
fn take_ammo(player: &Player, st: &mut PlayerState, infinity: bool) -> Option<(ItemStack, u8)> {
    let creative = st.game_mode == 1;
    let Some(slot) = find_ammo(st) else {
        // In creative it fires even without arrows.
        return creative.then(|| (arrow_item("arrow"), 2));
    };
    let item = st.inv.slots[slot].clone()?.with_count(1);
    let free = creative || (infinity && item.name() == "arrow");
    if !free {
        if let Some(s) = &mut st.inv.slots[slot] {
            s.count -= 1;
            if s.count <= 0 {
                st.inv.slots[slot] = None;
            }
        }
        st.sync_slot(player, slot);
    }
    Some((item, if free { 2 } else { 1 }))
}

fn shoot_bow(server: &Arc<Server>, player: &Arc<Player>, hand: i32, ticks: i64) {
    let power = bow_power(ticks);
    if power < 0.1 {
        return;
    }
    let (data, eye, dir) = {
        let mut st = player.state.lock().unwrap();
        let Some(bow) = st.inv.held(hand, st.held_slot).clone().filter(|b| b.name() == "bow") else { return };
        let Some((arrow, pickup)) = take_ammo(player, &mut st, bow.enchantment("infinity") > 0) else { return };
        let mut d = ArrowData::new(arrow);
        d.pickup = pickup;
        d.crit = power >= 1.0;
        let p = bow.enchantment("power");
        if p > 0 {
            d.damage += 0.5 * p as f64 + 0.5;
        }
        d.knock = bow.enchantment("punch");
        d.flame = bow.enchantment("flame") > 0;
        (d, st.eye(), look(st.yaw, st.pitch))
    };
    let speed = power * 3.0;
    let vel = [dir[0] * speed, dir[1] * speed, dir[2] * speed];
    super::projectiles::shoot_with(server, player.dim(), ProjectileKind::Arrow, player.eid, [eye[0], eye[1] - 0.1, eye[2]], vel, Some(data));
    let pitch = 1.0 / (super::entities::random() * 0.4 + 1.2) + power as f32 * 0.5;
    sound::at(server, player.dim(), eye, "entity.arrow.shoot", sound::PLAYER, 1.0, pitch);
    wear(server, player, hand, 1);
}

/// Wears down the item in one hand.
fn wear(server: &Server, player: &Player, hand: i32, amount: i32) {
    if hand == 0 {
        health::wear_held(server, player, amount);
        return;
    }
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 {
        return;
    }
    let slot = held_slot(&st, hand);
    if let Some(i) = &mut st.inv.slots[slot] {
        if i.wear(amount, &mut super::entities::random) {
            st.inv.slots[slot] = None;
        }
    }
    st.sync_slot(player, slot);
}

fn load_crossbow(server: &Arc<Server>, player: &Arc<Player>, hand: i32, ticks: i64) {
    let loaded = {
        let mut st = player.state.lock().unwrap();
        let slot = held_slot(&st, hand);
        let Some(cb) = st.inv.slots[slot].clone().filter(|c| c.name() == "crossbow" && !charged(c)) else { return };
        if ticks < crossbow_charge(&cb) {
            return;
        }
        let Some((arrow, pickup)) = take_ammo(player, &mut st, false) else { return };
        // Multishot: three arrows for one.
        let n = if cb.enchantment("multishot") > 0 { 3 } else { 1 };
        let list = (0..n)
            .map(|i| {
                Nbt::compound()
                    .with("id", Nbt::str(&format!("minecraft:{}", arrow.name())))
                    .with("Count", Nbt::Byte(1))
                    .with("bambietta_pickup", Nbt::Byte(if i == 0 { pickup as i8 } else { 2 }))
            })
            .collect();
        let mut cb = cb;
        let tag = cb.nbt.get_or_insert_with(|| Box::new(Nbt::compound()));
        tag.set("Charged", Nbt::Byte(1));
        tag.set("ChargedProjectiles", Nbt::List(list));
        st.inv.slots[slot] = Some(cb);
        st.sync_slot(player, slot);
        true
    };
    if loaded {
        sound::at(server, player.dim(), pos(player), "item.crossbow.loading_end", sound::PLAYER, 1.0, 1.0);
    }
}

fn shoot_crossbow(server: &Arc<Server>, player: &Arc<Player>, hand: i32) {
    let (shots, eye, yaw, pitch, pierce) = {
        let mut st = player.state.lock().unwrap();
        let slot = held_slot(&st, hand);
        let Some(mut cb) = st.inv.slots[slot].clone() else { return };
        let list: Vec<(String, u8)> = cb
            .nbt
            .as_ref()
            .and_then(|n| n.get("ChargedProjectiles"))
            .map(Nbt::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|p| Some((p.get_str("id")?.trim_start_matches("minecraft:").to_string(), p.get_i64("bambietta_pickup").unwrap_or(1) as u8)))
            .collect();
        if let Some(tag) = cb.nbt.as_mut() {
            tag.remove("Charged");
            tag.remove("ChargedProjectiles");
            if tag.is_empty_compound() {
                cb.nbt = None;
            }
        }
        let pierce = cb.enchantment("piercing");
        st.inv.slots[slot] = Some(cb);
        st.sync_slot(player, slot);
        (list, st.eye(), st.yaw, st.pitch, pierce)
    };
    // Multishot: fanned out at ±10°.
    let offsets = [0.0f32, -10.0, 10.0];
    for (i, (name, pickup)) in shots.iter().enumerate() {
        if !is_arrow(name) {
            continue;
        }
        let dir = look(yaw + offsets[i.min(2)], pitch);
        let vel = [dir[0] * 3.15, dir[1] * 3.15, dir[2] * 3.15];
        let mut d = ArrowData::new(arrow_item(name));
        d.crit = true;
        d.pickup = *pickup;
        d.pierce = pierce;
        super::projectiles::shoot_with(server, player.dim(), ProjectileKind::Arrow, player.eid, [eye[0], eye[1] - 0.1, eye[2]], vel, Some(d));
    }
    if !shots.is_empty() {
        sound::at(server, player.dim(), eye, "item.crossbow.shoot", sound::PLAYER, 1.0, 1.0);
        wear(server, player, hand, if shots.len() > 1 { 3 } else { 1 });
    }
}

fn throw_trident(server: &Arc<Server>, player: &Arc<Player>, hand: i32, ticks: i64) {
    if ticks < 10 {
        return;
    }
    let dim = player.dim();
    let (item, eye, dir, riptide, wet, creative) = {
        let st = player.state.lock().unwrap();
        let Some(t) = st.inv.held(hand, st.held_slot).clone().filter(|t| t.name() == "trident") else { return };
        let feet = crate::protocol::BlockPos::new(st.x.floor() as i32, st.y.floor() as i32, st.z.floor() as i32);
        let head = crate::protocol::BlockPos::new(feet.x, feet.y + 1, feet.z);
        let wet = {
            let mut w = server.world(dim).lock().unwrap();
            let reg = data::get();
            [feet, head].iter().any(|p| {
                let s = w.get_block(*p);
                let b = reg.block_of_state(s);
                b.name == "water" || b.get(s, "waterlogged") == Some("true")
            })
        };
        (t.clone(), st.eye(), look(st.yaw, st.pitch), t.enchantment("riptide"), wet, st.game_mode == 1)
    };
    tracing::debug!("{}: tridente (propulsión {riptide}, en el agua: {wet})", player.name);
    if riptide > 0 {
        // Riptide: only in water; launches the player in the look direction.
        if !wet {
            return;
        }
        let k = 3.0 * (1.0 + riptide as f64) / 4.0;
        let v = [dir[0] * k, dir[1] * k, dir[2] * k];
        player.state.lock().unwrap().ac.grace = 60;
        let mut p = packet(id::play::ENTITY_VELOCITY);
        p.varint(player.eid);
        for x in v {
            p.i16((x.clamp(-3.9, 3.9) * 8000.0) as i16);
        }
        player.send(p);
        let snd = match riptide {
            1 => "item.trident.riptide_1",
            2 => "item.trident.riptide_2",
            _ => "item.trident.riptide_3",
        };
        sound::at(server, dim, eye, snd, sound::PLAYER, 1.0, 1.0);
        wear(server, player, hand, 1);
        return;
    }
    wear(server, player, hand, 1);
    // The thrown trident carries its wear; in survival it leaves the inventory.
    let thrown = {
        let mut st = player.state.lock().unwrap();
        let slot = held_slot(&st, hand);
        let current = st.inv.slots[slot].clone().unwrap_or(item);
        if !creative {
            st.inv.slots[slot] = None;
            st.sync_slot(player, slot);
        }
        current
    };
    let mut d = ArrowData::new(thrown.with_count(1));
    d.loyalty = thrown.enchantment("loyalty");
    d.pickup = if creative { 2 } else { 1 };
    let vel = [dir[0] * 2.5, dir[1] * 2.5, dir[2] * 2.5];
    super::projectiles::shoot_with(server, dim, ProjectileKind::Trident, player.eid, [eye[0], eye[1] - 0.1, eye[2]], vel, Some(d));
    sound::at(server, dim, eye, "item.trident.throw", sound::PLAYER, 1.0, 1.0);
    server.broadcast_tracked(player.eid, &super::play::equipment(player), false);
}

// ---------- shield ----------

/// Is the shield raised (for 5 ticks, like vanilla) toward `src`?
pub fn blocking_towards(st: &PlayerState, now: i64, src: [f64; 3]) -> bool {
    if !st.using.as_ref().is_some_and(|u| u.kind == UseKind::Shield && now - u.start >= 5) {
        return false;
    }
    let to = [src[0] - st.x, src[2] - st.z];
    let len = (to[0] * to[0] + to[1] * to[1]).sqrt();
    if len < 1e-6 {
        return false;
    }
    let l = look(st.yaw, 0.0);
    (to[0] * l[0] + to[1] * l[2]) / len > 0.0
}

pub fn blocks_from(server: &Server, player: &Player, src: [f64; 3]) -> bool {
    blocking_towards(&player.state.lock().unwrap(), now(server), src)
}

/// An axe hit disables the shield for 5 s.
pub fn disable_shield(server: &Server, player: &Player) {
    {
        let mut st = player.state.lock().unwrap();
        st.using = None;
        st.shield_disabled_until = now(server) + 100;
    }
    if let Some(id) = data::get().item_id("shield") {
        let mut c = packet(id::play::SET_COOLDOWN);
        c.varint(id).varint(100);
        player.send(c);
    }
    let mut p = packet(id::play::ENTITY_STATUS);
    p.i32(player.eid).i8(30);
    server.broadcast_tracked(player.eid, &p, true);
    server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bow_power_curve() {
        assert_eq!(bow_power(0), 0.0);
        assert!((bow_power(10) - (0.25 + 1.0) / 3.0).abs() < 1e-9);
        assert_eq!(bow_power(20), 1.0);
        assert_eq!(bow_power(60), 1.0);
    }

    #[test]
    fn shield_faces_attacker() {
        let mut st = PlayerState::new(0.0, 64.0, 0.0, 0, 8);
        st.yaw = 0.0; // facing south (+z)
        st.using = Some(Using { hand: 1, start: 0, kind: UseKind::Shield });
        assert!(blocking_towards(&st, 10, [0.0, 64.0, 5.0]));
        assert!(!blocking_towards(&st, 10, [0.0, 64.0, -5.0]));
        assert!(!blocking_towards(&st, 3, [0.0, 64.0, 5.0]), "hacen falta 5 ticks");
    }
}

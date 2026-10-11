//! Experience: levels with the vanilla formula, the bar, orbs and Mending.

use super::{Player, PlayerState};
use crate::protocol::{id, packet, Writer};

/// Points needed to go from level `level` to the next.
pub fn xp_to_next(level: i32) -> i32 {
    match level {
        l if l >= 30 => 112 + (l - 30) * 9,
        l if l >= 15 => 37 + (l - 15) * 5,
        l => 7 + l * 2,
    }
}

/// Total points represented by having `level` levels and `progress` of the bar.
pub fn total_for(level: i32, progress: f32) -> i32 {
    (0..level).map(xp_to_next).sum::<i32>() + (progress * xp_to_next(level) as f32).round() as i32
}

pub fn packet_for(st: &PlayerState) -> Vec<u8> {
    let mut p = packet(id::play::EXPERIENCE);
    p.f32(st.xp_progress).varint(st.xp_level).varint(st.xp_total);
    p
}

/// Adds (or subtracts) experience points and notifies the client.
pub fn add_points(player: &Player, st: &mut PlayerState, points: i32) {
    let total = (total_for(st.xp_level, st.xp_progress) + points).max(0);
    st.xp_total = (st.xp_total + points.max(0)).max(0);
    let (mut level, mut left) = (0, total);
    while left >= xp_to_next(level) {
        left -= xp_to_next(level);
        level += 1;
    }
    st.xp_level = level;
    st.xp_progress = left as f32 / xp_to_next(level) as f32;
    player.send(packet_for(st));
}

/// Spends whole levels (enchanting table, anvil...).
pub fn spend_levels(player: &Player, st: &mut PlayerState, levels: i32) {
    st.xp_level = (st.xp_level - levels).max(0);
    if st.xp_level == 0 && levels > 0 {
        st.xp_progress = st.xp_progress.min(0.99);
    }
    player.send(packet_for(st));
}

/// Picks up an orb: first repairs gear with Mending (2 durability per point).
pub fn pickup_orb(player: &Player, value: i32) {
    let mut st = player.state.lock().unwrap();
    let mut xp = value;
    let held = super::inventory::HOTBAR + st.held_slot;
    let candidates: Vec<usize> = [held, super::inventory::OFFHAND, 5, 6, 7, 8]
        .into_iter()
        .filter(|&i| st.inv.slots[i].as_ref().is_some_and(|s| s.enchantment("mending") > 0 && s.damage() > 0))
        .collect();
    if let Some(&slot) = candidates.get((super::entities::random() * candidates.len() as f32) as usize) {
        if let Some(item) = &mut st.inv.slots[slot] {
            let repair = (xp * 2).min(item.damage());
            let new_damage = item.damage() - repair;
            if let Some(tag) = item.nbt.as_deref_mut() {
                tag.set("Damage", crate::protocol::Nbt::Int(new_damage));
            }
            xp -= repair / 2;
        }
        st.sync_slot(player, slot);
    }
    if xp > 0 {
        add_points(player, &mut st, xp);
    }
}

/// Experience lost on death (vanilla: 7 per level, maximum 100).
pub fn death_drop(st: &PlayerState) -> i32 {
    (st.xp_level * 7).min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(xp_to_next(0), 7);
        assert_eq!(xp_to_next(15), 37);
        assert_eq!(xp_to_next(30), 112);
        // Vanilla: level 30 = 1395 points.
        assert_eq!(total_for(30, 0.0), 1395);
    }
}

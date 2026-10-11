//! Little things you do with animals by hand: taming parrots with seeds (a cookie kills
//! them), making a tamed parrot sit, parrots riding on their owner's shoulder (they jump
//! off when the owner falls, swims, sleeps or gets hurt), scooping fish, tadpoles and
//! axolotls up with a water bucket, feeding tadpoles and axolotls, and repairing iron
//! golems with iron ingots.

use super::mobs::{self, MobKind};
use super::{entities, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::Nbt;
use std::sync::Arc;

fn take_held(player: &Player, hand: i32, rem: Option<ItemStack>) {
    let mut st = player.state.lock().unwrap();
    if st.game_mode == 1 && rem.is_none() {
        return;
    }
    let held = st.held_slot;
    st.inv.consume_held(hand, held, 1, rem);
    let p = st.inv.window_items();
    player.send(p);
}

/// Right click on one of these mobs. Returns whether something happened.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, kind: MobKind, hand: i32, held: Option<&ItemStack>) -> bool {
    let name = held.map(|h| h.name()).unwrap_or("");
    match (kind, name) {
        (MobKind::IronGolem, "iron_ingot") => {
            let healed = server.mobs.lock().unwrap().get_mut(&eid).filter(|m| m.health < m.max_health()).map(|m| m.health = (m.health + 25.0).min(m.max_health())).is_some();
            if healed {
                take_held(player, hand, None);
                sound::entity(server, eid, "entity.iron_golem.repair", sound::NEUTRAL, 1.0, 1.0);
            }
            healed
        }
        // Into the bucket.
        (k, "water_bucket") if k.fish() || matches!(k, MobKind::Axolotl | MobKind::Tadpole) => {
            let Some(m) = server.mobs.lock().unwrap().remove(&eid) else { return false };
            let Some(bucket) = super::wildlife::bucket_of(&m) else { return false };
            take_held(player, hand, Some(bucket));
            let snd = if k == MobKind::Axolotl { "item.bucket.fill_axolotl" } else { "item.bucket.fill_fish" };
            sound::at(server, m.dim, m.pos, snd, sound::NEUTRAL, 1.0, 1.0);
            true
        }
        (MobKind::Tadpole, "slime_ball") => {
            if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
                m.grow += -m.grow / 10;
            }
            take_held(player, hand, None);
            true
        }
        // Axolotls breed with a bucket of tropical fish (the water stays in the bucket).
        (MobKind::Axolotl, "tropical_fish_bucket") => {
            let fed = {
                let mut map = server.mobs.lock().unwrap();
                map.get_mut(&eid).filter(|m| m.grow == 0 && m.love == 0).map(|m| {
                    m.love = 600;
                    m.love_by = Some(player.eid);
                })
            };
            if fed.is_some() {
                take_held(player, hand, data::get().item_id("water_bucket").map(|i| ItemStack::new(i, 1)));
                server.broadcast_tracked(eid, &super::animals::status(eid, 18), true);
            }
            fed.is_some()
        }
        (MobKind::Parrot, "cookie") => {
            take_held(player, hand, None);
            mobs::hurt(server, eid, 1000.0, "magic", Some(player.eid), None, 0);
            true
        }
        (MobKind::Parrot, n) => {
            let seeds = matches!(n, "wheat_seeds" | "melon_seeds" | "pumpkin_seeds" | "beetroot_seeds" | "torchflower_seeds" | "pitcher_pod");
            let (packets, ate, tamed) = {
                let mut map = server.mobs.lock().unwrap();
                let Some(m) = map.get_mut(&eid) else { return false };
                if m.owner.is_none() {
                    if !seeds {
                        return false;
                    }
                    let tamed = entities::random() < 1.0 / 3.0;
                    if tamed {
                        m.owner = Some(player.uuid);
                        m.sitting = true;
                    }
                    (vec![super::animals::status(eid, if tamed { 7 } else { 6 }), mobs::metadata(m)], true, tamed)
                } else if m.owner == Some(player.uuid) {
                    m.sitting = !m.sitting;
                    m.wander = None;
                    (vec![mobs::metadata(m)], false, false)
                } else {
                    return false;
                }
            };
            if ate {
                take_held(player, hand, None);
            }
            for p in packets {
                server.broadcast_tracked(eid, &p, true);
            }
            if tamed {
                super::advancements::grant(server, player, "husbandry/tame_an_animal");
            }
            true
        }
        _ => false,
    }
}

// ---------- parrots on the shoulder ----------

/// What the player carries on the left shoulder (player metadata 19).
pub fn shoulder_nbt(variant: Option<i32>) -> Nbt {
    match variant {
        Some(v) => Nbt::compound().with("id", Nbt::str("minecraft:parrot")).with("Variant", Nbt::Int(v)),
        None => Nbt::compound(),
    }
}

/// The parrot jumps onto its owner's shoulder (the entity goes, the player carries it).
pub fn perch(server: &Arc<Server>, player: &Arc<Player>, eid: i32) {
    let variant = {
        let mut map = server.mobs.lock().unwrap();
        match map.get(&eid) {
            Some(m) if m.owner == Some(player.uuid) && !m.sitting => {
                let v = m.variant;
                map.remove(&eid);
                v
            }
            _ => return,
        }
    };
    {
        let mut st = player.state.lock().unwrap();
        if st.shoulder.is_some() {
            return;
        }
        st.shoulder = Some(variant);
    }
    server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
}

/// The parrot jumps off (the player fell, swam, slept or was hurt).
pub fn unperch(server: &Arc<Server>, player: &Arc<Player>) {
    let (variant, pos) = {
        let mut st = player.state.lock().unwrap();
        let Some(v) = st.shoulder.take() else { return };
        (v, [st.x, st.y + 1.2, st.z])
    };
    let eid = mobs::spawn(server, player.dim(), MobKind::Parrot, pos);
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.variant = variant;
        m.owner = Some(player.uuid);
        m.attack_cooldown = 200;
    }
    server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
}

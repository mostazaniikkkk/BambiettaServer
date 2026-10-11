//! Persistent player data in `<world>/playerdata/<uuid>.json`.

use super::{format_uuid, Player, PlayerState, Server};
use crate::item::ItemStack;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;

#[derive(Serialize, Deserialize)]
pub struct PlayerData {
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub game_mode: u8,
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub held_slot: usize,
    pub inventory: Vec<SavedItem>,
    #[serde(default)]
    pub xp_level: i32,
    #[serde(default)]
    pub xp_progress: f32,
    #[serde(default)]
    pub xp_total: i32,
    #[serde(default)]
    pub xp_seed: i32,
    #[serde(default)]
    pub spawn_point: Option<[i32; 3]>,
    #[serde(default)]
    pub dimension: Option<String>,
    /// Active effects: (name, amplifier, remaining ticks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<(String, i32, i32)>,
    #[serde(default)]
    pub absorption: f32,
    /// Ticks since the player last slept.
    #[serde(default)]
    pub rest: i64,
    /// Parrot on the shoulder (variant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder: Option<i32>,
    /// Advancements made: (id, Unix ms).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advancements: Vec<(String, i64)>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedItem {
    pub slot: usize,
    pub item: String,
    pub count: i8,
    /// Network NBT in hexadecimal (enchantments, name...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbt: Option<String>,
}

fn path(server: &Server, uuid: u128) -> PathBuf {
    server.world(crate::world::Dimension::Overworld).lock().unwrap().dir().join("playerdata").join(format!("{}.json", format_uuid(uuid)))
}

pub fn load(server: &Server, uuid: u128) -> Option<PlayerData> {
    let text = std::fs::read_to_string(path(server, uuid)).ok()?;
    serde_json::from_str(&text).map_err(|e| warn!("playerdata de {} inválido: {e}", format_uuid(uuid))).ok()
}

/// Applies the saved data to the freshly created state. Returns the dimension the player was in.
pub fn apply(data: PlayerData, st: &mut PlayerState) -> crate::world::Dimension {
    let g = crate::data::get();
    [st.x, st.y, st.z] = data.pos;
    (st.yaw, st.pitch) = (data.yaw, data.pitch);
    st.game_mode = data.game_mode.min(3);
    st.health = if data.health > 0.0 { data.health.min(20.0) } else { 20.0 };
    (st.food, st.saturation, st.exhaustion) = (data.food.clamp(0, 20), data.saturation, data.exhaustion);
    st.held_slot = data.held_slot.min(8);
    (st.xp_level, st.xp_progress, st.xp_total) = (data.xp_level.max(0), data.xp_progress.clamp(0.0, 1.0), data.xp_total.max(0));
    st.spawn_point = data.spawn_point.map(|p| crate::protocol::BlockPos::new(p[0], p[1], p[2]));
    if data.xp_seed != 0 {
        st.xp_seed = data.xp_seed;
    }
    for s in data.inventory {
        let Some(id) = g.item_id(&s.item) else { continue };
        if s.slot < 46 && s.count > 0 {
            let nbt = s.nbt.and_then(|h| (0..h.len()).step_by(2).map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect::<Option<Vec<u8>>>());
            let nbt = nbt.and_then(|b| crate::protocol::Nbt::from_network(&b)).map(Box::new);
            st.inv.slots[s.slot] = Some(ItemStack { id, count: s.count, nbt });
        }
    }
    st.effects = data
        .effects
        .iter()
        .filter_map(|(n, amp, dur)| Some(super::effects::ActiveEffect { id: super::effects::id(n)?, amp: *amp, duration: *dur }))
        .filter(|e| e.duration > 0)
        .collect();
    st.absorption = data.absorption.clamp(0.0, 16.0);
    st.advancements = data.advancements.into_iter().collect();
    st.rest = data.rest;
    st.shoulder = data.shoulder;
    st.ac = super::anticheat::MoveState::new(data.pos);
    crate::world::Dimension::ALL.into_iter().find(|d| Some(d.name()) == data.dimension.as_deref()).unwrap_or(crate::world::Dimension::Overworld)
}

pub fn save(server: &Server, player: &Arc<Player>) {
    let data = {
        let st = player.state.lock().unwrap();
        PlayerData {
            pos: [st.x, st.y, st.z],
            yaw: st.yaw,
            pitch: st.pitch,
            game_mode: st.game_mode,
            health: if st.dead { 20.0 } else { st.health },
            food: st.food,
            saturation: st.saturation,
            exhaustion: st.exhaustion,
            held_slot: st.held_slot,
            xp_level: st.xp_level,
            xp_progress: st.xp_progress,
            xp_total: st.xp_total,
            xp_seed: st.xp_seed,
            spawn_point: st.spawn_point.map(|p| [p.x, p.y, p.z]),
            dimension: Some(player.dim().name().to_string()),
            effects: st.effects.iter().map(|e| (super::effects::name(e.id).to_string(), e.amp, e.duration)).collect(),
            absorption: st.absorption,
            advancements: st.advancements.iter().map(|(k, v)| (k.clone(), *v)).collect(),
            rest: st.rest,
            shoulder: st.shoulder,
            inventory: st
                .inv
                .slots
                .iter()
                .enumerate()
                .filter(|(i, _)| *i > 4)
                .filter_map(|(slot, s)| {
                    let s = s.as_ref()?;
                    Some(SavedItem {
                        slot,
                        item: s.name().to_string(),
                        count: s.count,
                        nbt: s.nbt.as_ref().map(|n| n.to_network().iter().map(|b| format!("{b:02x}")).collect()),
                    })
                })
                .collect(),
        }
    };
    let p = path(server, player.uuid);
    let res = std::fs::create_dir_all(p.parent().unwrap()).and_then(|_| std::fs::write(&p, serde_json::to_string_pretty(&data).unwrap()));
    if let Err(e) = res {
        warn!("no se pudo guardar {}: {e}", player.name);
    }
}

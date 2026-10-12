//! Small items: the spyglass, goat horns (eight instruments, heard far away, with a
//! cooldown), lodestone compasses (bound to a lodestone; they spin when it is gone) and the
//! recovery compass (it points where the player last died, which the client is told on
//! joining and respawning).

use super::{sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{BlockPos, Nbt, Writer};
use crate::world::Dimension;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

/// Goat horn instruments, in the order of their sounds (`item.goat_horn.sound.N`).
pub const HORNS: [&str; 8] = ["ponder", "sing", "seek", "feel", "admire", "call", "yearn", "dream"];

/// Ticks before a horn can be blown again (vanilla: its sound's length, 7 seconds).
const HORN_COOLDOWN: i64 = 140;

/// When each player may blow a horn again.
static HORN_READY: Mutex<Option<HashMap<i32, i64>>> = Mutex::new(None);

/// A goat horn with instrument `i`, as `name{snbt}` (for mob drops).
pub fn horn_spec(i: usize) -> String {
    format!("goat_horn{{instrument:\"minecraft:{}_goat_horn\"}}", HORNS[i % HORNS.len()])
}

/// Instrument index of a horn (ponder when it has none).
fn instrument(item: &ItemStack) -> usize {
    let tag = item.nbt.as_ref().and_then(|n| n.get_str("instrument")).unwrap_or("");
    let name = tag.trim_start_matches("minecraft:").trim_end_matches("_goat_horn");
    HORNS.iter().position(|h| *h == name).unwrap_or(0)
}

/// `use_item` with a spyglass or a goat horn. Returns whether it was handled.
pub fn use_item(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (item, pos) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), [st.x, st.y + 1.5, st.z])
    };
    let Some(item) = item else { return false };
    match item.name() {
        // The zoom is the client's; others hear it.
        "spyglass" => {
            sound::entity(server, player.eid, "item.spyglass.use", sound::PLAYER, 1.0, 1.0);
            false
        }
        "goat_horn" => {
            let now = server.age.load(Ordering::Relaxed);
            {
                let mut ready = HORN_READY.lock().unwrap();
                let map = ready.get_or_insert_with(HashMap::new);
                if map.get(&player.eid).is_some_and(|&t| t > now) {
                    return true;
                }
                map.insert(player.eid, now + HORN_COOLDOWN);
                map.retain(|_, t| *t > now);
            }
            // Volume 16: heard up to 256 blocks away.
            sound::at(server, player.dim(), pos, &format!("item.goat_horn.sound.{}", instrument(&item)), RECORDS, 16.0, 1.0);
            let mut c = crate::protocol::packet(crate::protocol::id::play::SET_COOLDOWN);
            c.varint(item.id).varint(HORN_COOLDOWN as i32);
            player.send(c);
            true
        }
        _ => false,
    }
}

/// Sound category of instruments (`SoundSource.RECORDS`).
const RECORDS: i32 = 2;

// ---------- compasses ----------

fn lodestone_tag(dim: Dimension, pos: BlockPos) -> Nbt {
    Nbt::compound()
        .with("LodestonePos", Nbt::compound().with("X", Nbt::Int(pos.x)).with("Y", Nbt::Int(pos.y)).with("Z", Nbt::Int(pos.z)))
        .with("LodestoneDimension", Nbt::str(dim.name()))
        .with("LodestoneTracked", Nbt::Byte(1))
}

/// A compass used on a lodestone becomes bound to it (one from a stack is split off).
pub fn lodestone(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, hand: i32) {
    let dim = player.dim();
    let Some(id) = data::get().item_id("compass") else { return };
    let bound = ItemStack { id, count: 1, nbt: Some(Box::new(lodestone_tag(dim, pos))) };
    {
        let mut st = player.state.lock().unwrap();
        let held = st.held_slot;
        let single = st.inv.held(hand, held).as_ref().is_some_and(|i| i.count == 1);
        if st.game_mode == 1 && !single {
            // Creative keeps the plain compass.
            st.inv.add(bound);
        } else {
            st.inv.consume_held(hand, held, 1, Some(bound));
        }
        let p = st.inv.window_items();
        player.send(p);
    }
    sound::at(server, dim, [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5], "item.lodestone_compass.lock", sound::PLAYER, 1.0, 1.0);
}

/// The lodestone a compass is bound to, if any.
fn bound_to(item: &ItemStack) -> Option<(String, BlockPos)> {
    let tag = item.nbt.as_ref()?;
    let p = tag.get("LodestonePos")?;
    let c = |k: &str| p.get(k).and_then(Nbt::as_i64).map(|v| v as i32);
    Some((tag.get_str("LodestoneDimension")?.to_string(), BlockPos::new(c("X")?, c("Y")?, c("Z")?)))
}

/// Every second: compasses whose lodestone was broken lose it (and spin), like vanilla.
pub fn tick(server: &Arc<Server>) {
    if server.age.load(Ordering::Relaxed) % 20 != 0 {
        return;
    }
    let reg = data::get();
    for player in server.online() {
        let dim = player.dim();
        let bound: Vec<(usize, BlockPos)> = {
            let st = player.state.lock().unwrap();
            st.inv
                .slots
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    let s = s.as_ref().filter(|s| s.name() == "compass")?;
                    let (d, p) = bound_to(s)?;
                    (d == dim.name()).then_some((i, p))
                })
                .collect()
        };
        if bound.is_empty() {
            continue;
        }
        let gone: Vec<usize> = {
            let world = server.world(dim).lock().unwrap();
            bound.into_iter().filter(|(_, p)| world.peek_block(*p).is_some_and(|s| reg.block_of_state(s).name != "lodestone")).map(|(i, _)| i).collect()
        };
        if gone.is_empty() {
            continue;
        }
        let mut st = player.state.lock().unwrap();
        for i in gone {
            if let Some(it) = st.inv.slots[i].as_mut() {
                let tag = it.tag_mut();
                tag.remove("LodestonePos");
                tag.remove("LodestoneDimension");
            }
            st.sync_slot(&player, i);
        }
    }
}

/// The optional death location of `login` and `respawn`.
pub fn death_location(p: &mut Vec<u8>, last: Option<(Dimension, BlockPos)>) {
    match last {
        Some((dim, pos)) => {
            p.bool(true).string(dim.name()).position(pos);
        }
        None => {
            p.bool(false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horn_instruments() {
        let id = data::get().item_id("goat_horn").unwrap();
        let spec = horn_spec(5);
        let (_, t) = spec.split_once('{').unwrap();
        let tag = crate::protocol::nbt::parse_snbt(&format!("{{{t}")).unwrap();
        let item = ItemStack { id, count: 1, nbt: Some(Box::new(tag)) };
        assert_eq!(instrument(&item), 5);
        assert_eq!(instrument(&ItemStack::new(id, 1)), 0);
    }

    #[test]
    fn lodestone_binding() {
        let id = data::get().item_id("compass").unwrap();
        let item = ItemStack { id, count: 1, nbt: Some(Box::new(lodestone_tag(Dimension::Nether, BlockPos::new(3, 70, -9)))) };
        assert_eq!(bound_to(&item), Some(("minecraft:the_nether".into(), BlockPos::new(3, 70, -9))));
    }
}

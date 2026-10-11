//! Signs (writing on the front and back, dyes, glow ink, waxing) and books
//! (book and quill, signing and reading written books).

use super::{Player, Server};
use crate::data::{self, Block};
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Nbt, Writer};
use crate::world::blockentity::{BlockEntity, Sign};
use std::sync::Arc;

fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| *c != '§' && !c.is_control()).take(max).collect()
}

/// `block_entity_data` so clients see the new text.
fn update_packet(pos: BlockPos, block: &str, sign: &Sign) -> Vec<u8> {
    let mut p = packet(id::play::TILE_ENTITY_DATA);
    p.position(pos).varint(crate::world::blockentity::type_id(block).unwrap_or(7));
    sign.to_nbt().write_network(&mut p);
    p
}

/// Normal of the sign's front face (where the front text faces).
fn front_normal(block: &Block, state: u16) -> (f64, f64) {
    if let Some(r) = block.get(state, "rotation").and_then(|r| r.parse::<f64>().ok()) {
        let a = (r * 22.5).to_radians();
        return (-a.sin(), a.cos());
    }
    match block.get(state, "facing") {
        Some("north") => (0.0, -1.0),
        Some("south") => (0.0, 1.0),
        Some("west") => (-1.0, 0.0),
        _ => (1.0, 0.0),
    }
}

/// Opens the sign editor at `pos` (when placing it or on right click).
pub fn open_editor(server: &Server, player: &Player, pos: BlockPos, front: bool) {
    let dim = player.dim();
    let (sign, name) = {
        let mut world = server.world(dim).lock().unwrap();
        let name = data::get().block_of_state(world.get_block(pos)).name.clone();
        let sign = match world.block_entity(pos) {
            Some(BlockEntity::Sign(s)) => s.clone(),
            _ => {
                let s = Sign::default();
                world.set_block_entity(pos, BlockEntity::Sign(s.clone()));
                s
            }
        };
        (sign, name)
    };
    // The client must have the current text before opening the editor.
    player.send(update_packet(pos, &name, &sign));
    player.state.lock().unwrap().editing_sign = Some(pos);
    let mut p = packet(id::play::OPEN_SIGN_ENTITY);
    p.position(pos).bool(front);
    player.send(p);
}

/// Right click on a sign: dyes, glow ink, wax or editing the text.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, block: &Block, state: u16, hand: i32) -> bool {
    if !block.name.ends_with("_sign") {
        return false;
    }
    let dim = player.dim();
    let (held, px, pz, creative) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.x, st.z, st.game_mode == 1)
    };
    let (nx, nz) = front_normal(block, state);
    let front = (px - (pos.x as f64 + 0.5)) * nx + (pz - (pos.z as f64 + 0.5)) * nz >= 0.0;
    let mut sign = match server.world(dim).lock().unwrap().block_entity(pos) {
        Some(BlockEntity::Sign(s)) => s.clone(),
        _ => Sign::default(),
    };
    let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
    if sign.waxed {
        super::sound::at(server, dim, center, "block.sign.waxed_interact_fail", super::sound::BLOCK, 1.0, 1.0);
        return true;
    }
    let side = if front { &mut sign.front } else { &mut sign.back };
    let used = match held.as_ref().map(|i| i.name()) {
        Some(n) if super::stations::dye_color(n).is_some() => {
            let color = n.trim_end_matches("_dye").to_string();
            (side.color != color).then(|| {
                side.color = color;
                "item.dye.use"
            })
        }
        Some("glow_ink_sac") => (!side.glowing).then(|| {
            side.glowing = true;
            "item.glow_ink_sac.use"
        }),
        Some("ink_sac") => side.glowing.then(|| {
            side.glowing = false;
            "item.ink_sac.use"
        }),
        Some("honeycomb") => {
            sign.waxed = true;
            Some("item.honeycomb.wax_on")
        }
        _ => None,
    };
    match used {
        Some(sound) => {
            server.world(dim).lock().unwrap().set_block_entity(pos, BlockEntity::Sign(sign.clone()));
            server.broadcast_dim(dim, &update_packet(pos, &block.name, &sign), None);
            super::sound::at(server, dim, center, sound, super::sound::BLOCK, 1.0, 1.0);
            if !creative {
                let mut st = player.state.lock().unwrap();
                let h = st.held_slot;
                let slot = st.inv.consume_held(hand, h, 1, None);
                st.sync_slot(player, slot);
            }
        }
        None => open_editor(server, player, pos, front),
    }
    true
}

/// `update_sign` packet: the text written in the editor.
pub fn update(server: &Arc<Server>, player: &Arc<Player>, pos: BlockPos, front: bool, lines: [String; 4]) {
    let dim = player.dim();
    {
        let mut st = player.state.lock().unwrap();
        if st.editing_sign != Some(pos) || super::anticheat::reach2(&st, pos) > 8.0 * 8.0 {
            return;
        }
        st.editing_sign = None;
    }
    let (sign, name) = {
        let mut world = server.world(dim).lock().unwrap();
        let name = data::get().block_of_state(world.get_block(pos)).name.clone();
        if !name.ends_with("_sign") {
            return;
        }
        let mut sign = match world.block_entity(pos) {
            Some(BlockEntity::Sign(s)) => s.clone(),
            _ => Sign::default(),
        };
        if sign.waxed {
            return;
        }
        let side = if front { &mut sign.front } else { &mut sign.back };
        for (i, l) in lines.iter().enumerate() {
            side.lines[i] = clean(l, 384);
        }
        world.set_block_entity(pos, BlockEntity::Sign(sign.clone()));
        (sign, name)
    };
    server.broadcast_dim(dim, &update_packet(pos, &name, &sign), None);
}

/// `edit_book` packet: save the book and quill pages or sign it.
pub fn edit_book(player: &Arc<Player>, slot: i32, pages: Vec<String>, title: Option<String>) {
    let reg = data::get();
    let idx = match slot {
        0..=8 => super::inventory::HOTBAR + slot as usize,
        40 => super::inventory::OFFHAND,
        _ => return,
    };
    let mut st = player.state.lock().unwrap();
    let Some(book) = st.inv.slots[idx].clone().filter(|b| b.name() == "writable_book") else { return };
    let pages: Vec<String> = pages.iter().take(100).map(|p| clean(p, 1024)).collect();
    let new = match title.map(|t| clean(&t, 32)).filter(|t| !t.trim().is_empty()) {
        Some(title) => {
            let Some(id) = reg.item_id("written_book") else { return };
            let mut w = ItemStack::new(id, 1);
            let json: Vec<Nbt> = pages.iter().map(|p| Nbt::str(&serde_json::json!({ "text": p }).to_string())).collect();
            w.nbt = Some(Box::new(
                Nbt::compound()
                    .with("pages", Nbt::List(json))
                    .with("author", Nbt::str(&player.name))
                    .with("title", Nbt::str(&title))
                    .with("generation", Nbt::Int(0))
                    .with("resolved", Nbt::Byte(1)),
            ));
            w
        }
        None => {
            let mut b = book.clone();
            let list: Vec<Nbt> = pages.iter().map(|p| Nbt::str(p)).collect();
            let tag = b.nbt.get_or_insert_with(|| Box::new(Nbt::compound()));
            tag.set("pages", Nbt::List(list));
            b
        }
    };
    st.inv.slots[idx] = Some(new);
    st.sync_slot(player, idx);
}

/// Using a written book: the client opens it when the server asks it to.
pub fn read_book(player: &Arc<Player>, hand: i32) -> bool {
    let is_book = {
        let st = player.state.lock().unwrap();
        st.inv.held(hand, st.held_slot).as_ref().is_some_and(|i| i.name() == "written_book")
    };
    if is_book {
        let mut p = packet(id::play::OPEN_BOOK);
        p.varint(hand);
        player.send(p);
    }
    is_book
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_nbt_roundtrip() {
        let mut s = Sign::default();
        s.front.lines[0] = "Hola".into();
        s.back.color = "red".into();
        s.back.glowing = true;
        s.waxed = true;
        let n = s.to_nbt();
        assert_eq!(n.get("front_text").unwrap().get("messages").unwrap().as_list()[0].as_str(), Some(r#"{"text":"Hola"}"#));
        assert_eq!(Sign::from_nbt(&n), s);
        let b = data::get().block("oak_sign").unwrap();
        let (x, z) = front_normal(b, b.with(b.default_state, "rotation", "0"));
        assert!(x.abs() < 1e-9 && (z - 1.0).abs() < 1e-9);
    }
}

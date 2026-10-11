//! Advancements: the vanilla tabs (Minecraft, Nether, The End, Adventure, Husbandry) with a
//! selection of their advancements, the client's own titles and icons, the toast when one is
//! made, the chat announcement (`announceAdvancements`) and saving per player.
//!
//! What grants them: items in the inventory (checked every second), and events reported by
//! the rest of the server through `grant` (changing dimension, killing, sleeping, trading,
//! enchanting, planting, breeding, taming, eating, blocking with a shield...).

use super::{Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, Nbt, Writer};
use std::sync::Arc;

pub struct Adv {
    pub id: &'static str,
    parent: Option<&'static str>,
    icon: &'static str,
    /// 0 task, 1 challenge, 2 goal.
    frame: i32,
    x: f32,
    y: f32,
}

const fn adv(id: &'static str, parent: Option<&'static str>, icon: &'static str, frame: i32, x: f32, y: f32) -> Adv {
    Adv { id, parent, icon, frame, x, y }
}

pub const ALL: &[Adv] = &[
    adv("story/root", None, "grass_block", 0, 0.0, 0.0),
    adv("story/mine_stone", Some("story/root"), "wooden_pickaxe", 0, 1.0, 0.0),
    adv("story/upgrade_tools", Some("story/mine_stone"), "stone_pickaxe", 0, 2.0, 0.0),
    adv("story/smelt_iron", Some("story/upgrade_tools"), "iron_ingot", 0, 3.0, 0.0),
    adv("story/obtain_armor", Some("story/smelt_iron"), "iron_chestplate", 0, 4.0, -1.0),
    adv("story/lava_bucket", Some("story/smelt_iron"), "lava_bucket", 0, 4.0, 0.0),
    adv("story/iron_tools", Some("story/smelt_iron"), "iron_pickaxe", 0, 4.0, 1.0),
    adv("story/deflect_arrow", Some("story/obtain_armor"), "shield", 0, 5.0, -1.0),
    adv("story/form_obsidian", Some("story/lava_bucket"), "obsidian", 0, 5.0, 0.0),
    adv("story/mine_diamond", Some("story/iron_tools"), "diamond", 0, 5.0, 1.0),
    adv("story/enter_the_nether", Some("story/form_obsidian"), "flint_and_steel", 0, 6.0, 0.0),
    adv("story/shiny_gear", Some("story/mine_diamond"), "diamond_chestplate", 0, 6.0, 1.0),
    adv("story/enchant_item", Some("story/mine_diamond"), "enchanted_book", 0, 6.0, 2.0),
    adv("story/enter_the_end", Some("story/enter_the_nether"), "end_stone", 0, 7.0, 0.0),
    adv("nether/root", None, "red_nether_bricks", 0, 0.0, 0.0),
    adv("nether/find_fortress", Some("nether/root"), "nether_bricks", 0, 1.0, 0.0),
    adv("nether/obtain_blaze_rod", Some("nether/find_fortress"), "blaze_rod", 0, 2.0, 0.0),
    adv("end/root", None, "end_stone", 0, 0.0, 0.0),
    adv("end/kill_dragon", Some("end/root"), "dragon_head", 2, 1.0, 0.0),
    adv("adventure/root", None, "map", 0, 0.0, 0.0),
    adv("adventure/kill_a_mob", Some("adventure/root"), "iron_sword", 0, 1.0, 0.0),
    adv("adventure/sleep_in_bed", Some("adventure/root"), "red_bed", 0, 1.0, 1.0),
    adv("adventure/trade", Some("adventure/root"), "emerald", 0, 1.0, -1.0),
    adv("husbandry/root", None, "hay_block", 0, 0.0, 0.0),
    adv("husbandry/plant_seed", Some("husbandry/root"), "wheat", 0, 1.0, 0.0),
    adv("husbandry/breed_an_animal", Some("husbandry/root"), "wheat", 0, 1.0, 1.0),
    adv("husbandry/tame_an_animal", Some("husbandry/root"), "lead", 0, 1.0, 2.0),
    adv("husbandry/fishy_business", Some("husbandry/root"), "fishing_rod", 0, 1.0, -1.0),
];

fn background(id: &str) -> Option<&'static str> {
    Some(match id {
        "story/root" => "minecraft:textures/gui/advancements/backgrounds/stone.png",
        "nether/root" => "minecraft:textures/gui/advancements/backgrounds/nether.png",
        "end/root" => "minecraft:textures/gui/advancements/backgrounds/end.png",
        "adventure/root" => "minecraft:textures/gui/advancements/backgrounds/adventure.png",
        "husbandry/root" => "minecraft:textures/gui/advancements/backgrounds/husbandry.png",
        _ => return None,
    })
}

/// Items whose possession grants an advancement.
fn by_item(item: &str) -> Option<&'static str> {
    Some(match item {
        "crafting_table" => "story/root",
        "cobblestone" | "cobbled_deepslate" | "blackstone" => "story/mine_stone",
        "stone_pickaxe" => "story/upgrade_tools",
        "iron_ingot" => "story/smelt_iron",
        "iron_helmet" | "iron_chestplate" | "iron_leggings" | "iron_boots" => "story/obtain_armor",
        "lava_bucket" => "story/lava_bucket",
        "iron_pickaxe" => "story/iron_tools",
        "obsidian" => "story/form_obsidian",
        "diamond" => "story/mine_diamond",
        "diamond_helmet" | "diamond_chestplate" | "diamond_leggings" | "diamond_boots" => "story/shiny_gear",
        "blaze_rod" => "nether/obtain_blaze_rod",
        "cod" | "salmon" | "tropical_fish" | "pufferfish" => "husbandry/fishy_business",
        _ => return None,
    })
}

fn key(id: &str) -> String {
    format!("advancements.{}", id.replace('/', "."))
}

fn tr(k: &str) -> Nbt {
    Nbt::Compound(vec![("translate".into(), Nbt::String(k.into()))])
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The whole tree with the player's progress (on joining).
pub fn send_all(player: &Player) {
    let done = player.state.lock().unwrap().advancements.clone();
    let reg = data::get();
    let mut p = packet(id::play::ADVANCEMENTS);
    p.bool(true).varint(ALL.len() as i32);
    for a in ALL {
        p.string(&format!("minecraft:{}", a.id));
        match a.parent {
            Some(parent) => {
                p.bool(true).string(&format!("minecraft:{parent}"));
            }
            None => {
                p.bool(false);
            }
        }
        p.bool(true);
        tr(&format!("{}.title", key(a.id))).write_network(&mut p);
        tr(&format!("{}.description", key(a.id))).write_network(&mut p);
        crate::item::write_slot(&mut p, &reg.item_id(a.icon).map(|i| ItemStack::new(i, 1)));
        p.varint(a.frame);
        let bg = background(a.id);
        // 0x01 background, 0x02 show the toast.
        p.i32(0x02 | i32::from(bg.is_some()));
        if let Some(bg) = bg {
            p.string(bg);
        }
        p.f32(a.x).f32(a.y);
        p.varint(1).varint(1).string("done");
        p.bool(false);
    }
    p.varint(0);
    p.varint(ALL.len() as i32);
    for a in ALL {
        p.string(&format!("minecraft:{}", a.id)).varint(1).string("done");
        match done.get(a.id) {
            Some(&t) => {
                p.bool(true).i64(t);
            }
            None => {
                p.bool(false);
            }
        }
    }
    player.send(p);
}

/// Grants an advancement (if the player does not have it yet): toast and announcement.
pub fn grant(server: &Server, player: &Player, id: &str) {
    let Some(a) = ALL.iter().find(|a| a.id == id) else { return };
    let t = now_ms();
    {
        let mut st = player.state.lock().unwrap();
        if st.advancements.contains_key(a.id) {
            return;
        }
        st.advancements.insert(a.id.to_string(), t);
    }
    let mut p = packet(id::play::ADVANCEMENTS);
    p.bool(false).varint(0).varint(0).varint(1);
    p.string(&format!("minecraft:{}", a.id)).varint(1).string("done").bool(true).i64(t);
    player.send(p);
    if server.rule("announceAdvancements") {
        // "<player> has made the advancement [<title>]" in the client's language.
        let title = Nbt::Compound(vec![
            ("translate".into(), Nbt::String(format!("{}.title", key(a.id)))),
            ("color".into(), Nbt::String(if a.frame == 1 { "dark_purple".into() } else { "green".into() })),
        ]);
        let msg = Nbt::Compound(vec![
            ("translate".into(), Nbt::String(if a.frame == 2 { "chat.type.advancement.goal".into() } else { "chat.type.advancement.task".into() })),
            ("with".into(), Nbt::List(vec![crate::protocol::text(&player.name), title])),
        ]);
        let mut chat = packet(id::play::SYSTEM_CHAT);
        msg.write_network(&mut chat);
        chat.bool(false);
        server.broadcast(&chat, None);
    }
}

/// Every second: advancements for what the players carry, and for where they are.
pub fn tick(server: &Arc<Server>) {
    for p in server.online() {
        let ids: Vec<&'static str> = {
            let st = p.state.lock().unwrap();
            st.inv.slots.iter().flatten().filter_map(|s| by_item(s.name())).filter(|id| !st.advancements.contains_key(*id)).collect()
        };
        for id in ids {
            grant(server, &p, id);
        }
        if p.dim() == crate::world::Dimension::Nether && !p.state.lock().unwrap().advancements.contains_key("nether/find_fortress") {
            let pos = {
                let st = p.state.lock().unwrap();
                [st.x, st.y, st.z]
            };
            let inside = server.world(crate::world::Dimension::Nether).lock().unwrap().generator().in_structure("fortress", pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32);
            if inside {
                grant(server, &p, "nether/find_fortress");
            }
        }
    }
}

/// Arriving in a dimension.
pub fn entered(server: &Server, player: &Player, dim: crate::world::Dimension) {
    match dim {
        crate::world::Dimension::Nether => {
            grant(server, player, "story/enter_the_nether");
            grant(server, player, "nether/root");
        }
        crate::world::Dimension::End => {
            grant(server, player, "story/enter_the_end");
            grant(server, player, "end/root");
        }
        _ => {}
    }
}

//! Health, hunger, damage, death, respawning and eating.

use super::anticheat::environment;
use super::{entities, Player, Server};
use crate::gameplay;
use crate::item::ItemStack;
use crate::protocol::{id, packet, text, Writer};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub fn health_packet(health: f32, food: i32, saturation: f32) -> Vec<u8> {
    let mut p = packet(id::play::UPDATE_HEALTH);
    p.f32(health).varint(food).f32(saturation);
    p
}

pub(super) fn send_health(player: &Player) {
    let p = {
        let s = player.state.lock().unwrap();
        health_packet(s.health, s.food, s.saturation)
    };
    player.send(p);
}

/// Who causes the damage (player or mob).
#[derive(Clone)]
pub struct Attacker {
    pub eid: i32,
    pub name: String,
    /// Where the hit comes from (for the shield).
    pub pos: Option<[f64; 3]>,
}

impl Attacker {
    pub fn player(p: &Player) -> Self {
        Self { eid: p.eid, name: p.name.clone(), pos: None }
    }
}

/// Damage a shield can block (if it comes from the front).
fn blockable(kind: &str) -> bool {
    matches!(kind, "player_attack" | "mob_attack" | "arrow" | "trident" | "mob_projectile" | "fireball" | "explosion" | "player_explosion" | "wither_skull" | "thrown")
}

/// Entity status that plays the break animation of the item in the slot (47 main hand ... 52 feet).
fn break_status(slot: usize) -> i8 {
    match slot {
        super::inventory::OFFHAND => 48,
        5 => 49,
        6 => 50,
        7 => 51,
        8 => 52,
        _ => 47,
    }
}

/// Consumes durability of a player slot (with the state already locked).
/// Returns the break status if it broke.
fn wear_locked(player: &Player, st: &mut super::PlayerState, slot: usize, amount: i32) -> Option<i8> {
    if st.game_mode == 1 {
        return None;
    }
    let item = st.inv.slots[slot].as_mut()?;
    let broke = item.wear(amount, &mut entities::random);
    if broke {
        st.inv.slots[slot] = None;
    }
    st.sync_slot(player, slot);
    broke.then(|| break_status(slot))
}

/// Consumes durability of the main-hand item (tools and weapons).
pub fn wear_held(server: &Server, player: &Player, amount: i32) {
    let broke = {
        let mut st = player.state.lock().unwrap();
        let slot = super::inventory::HOTBAR + st.held_slot;
        wear_locked(player, &mut st, slot, amount)
    };
    if let Some(status) = broke {
        let mut p = packet(id::play::ENTITY_STATUS);
        p.i32(player.eid).i8(status);
        server.broadcast_tracked(player.eid, &p, true);
        server.broadcast_tracked(player.eid, &super::play::equipment(player), false);
    }
}

/// Fire damage (Fire Resistance negates it).
const FIRE_KINDS: &[&str] = &["in_fire", "on_fire", "lava", "hot_floor", "fireball", "unattributed_fireball"];

fn pos_of(player: &Player) -> [f64; 3] {
    let st = player.state.lock().unwrap();
    [st.x, st.y, st.z]
}

/// Sets the player on fire for `ticks`.
pub fn set_on_fire(server: &Server, player: &Player, ticks: i32) {
    let changed = {
        let mut st = player.state.lock().unwrap();
        if st.dead || !st.survival_like() {
            return;
        }
        let was = st.fire_ticks > 0;
        st.fire_ticks = st.fire_ticks.max(ticks);
        !was
    };
    if changed {
        server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
    }
}

/// Damage reduced by armor (the rest ignores it: fall, starvation, void, drowning...).
fn armor_applies(kind: &str) -> bool {
    matches!(kind, "player_attack" | "mob_attack" | "mob_attack_no_aggro" | "arrow" | "mob_projectile" | "explosion" | "player_explosion" | "generic" | "cactus" | "thorns" | "sting" | "falling_block" | "trident" | "wither_skull" | "thrown")
}

/// Reduction from armor, toughness and Protection enchantments (vanilla formulas).
/// Returns the final damage and consumes armor durability.
fn mitigate(player: &Player, st: &mut super::PlayerState, amount: f32, kind: &str) -> (f32, Vec<i8>) {
    let mut amount = amount;
    let mut broken = Vec::new();
    if armor_applies(kind) {
        let (armor, tough) = (5..=8).filter_map(|i| st.inv.slots[i].as_ref()).map(gameplay::armor_points).fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
        if armor > 0.0 {
            let reduced = (armor / 5.0).max(armor - 4.0 * amount / (tough + 2.0)).min(20.0);
            let before = amount;
            amount *= 1.0 - reduced / 25.0;
            let wear = ((before / 4.0).floor() as i32).max(1);
            for slot in 5..=8 {
                if st.inv.slots[slot].as_ref().is_some_and(|i| gameplay::armor_points(i).0 > 0.0) {
                    broken.extend(wear_locked(player, st, slot, wear));
                }
            }
        }
    }
    if matches!(kind, "out_of_world" | "generic_kill" | "starve") {
        return (amount, broken);
    }
    let mut epf = 0;
    for slot in 5..=8 {
        let Some(item) = &st.inv.slots[slot] else { continue };
        epf += item.enchantment("protection");
        epf += match kind {
            "fall" => 3 * item.enchantment("feather_falling"),
            "explosion" | "player_explosion" => 2 * item.enchantment("blast_protection"),
            "arrow" | "mob_projectile" | "trident" => 2 * item.enchantment("projectile_protection"),
            "lava" | "in_fire" | "on_fire" | "hot_floor" => 2 * item.enchantment("fire_protection"),
            _ => 0,
        };
    }
    amount *= 1.0 - epf.min(20) as f32 / 25.0;
    (amount, broken)
}

/// Applies damage to a player. Returns whether damage was dealt.
pub fn damage(server: &Arc<Server>, player: &Arc<Player>, amount: f32, kind: &str, attacker: Option<Attacker>) -> bool {
    // A hit wakes the player up (like vanilla).
    if player.state.lock().unwrap().sleeping.is_some() && amount > 0.0 {
        super::beds::wake(server, player);
    }
    // And scares the parrot off the shoulder.
    if amount > 0.0 && player.state.lock().unwrap().survival_like() {
        super::pets::unperch(server, player);
    }
    let bypass = matches!(kind, "out_of_world" | "generic_kill");
    // Damage turned off by game rules.
    let off = match kind {
        "fall" => !server.rule("fallDamage"),
        "in_fire" | "on_fire" | "lava" | "hot_floor" => !server.rule("fireDamage"),
        "drown" => !server.rule("drowningDamage"),
        _ => false,
    };
    if off {
        return false;
    }
    let (died, broken, totem) = {
        let mut st = player.state.lock().unwrap();
        if st.dead || amount <= 0.0 || (!st.survival_like() && !bypass) || st.game_mode == 3 {
            return false;
        }
        if st.hurt_cooldown > 0 && !bypass {
            return false;
        }
        // Fire Resistance: immune to fire, lava and fireballs.
        if FIRE_KINDS.contains(&kind) && super::effects::has(&st, "fire_resistance") {
            return false;
        }
        // Shield raised toward the hit: blocks it fully and takes durability damage.
        let now = server.age.load(Ordering::Relaxed);
        if let Some(src) = attacker.as_ref().and_then(|a| a.pos).filter(|_| blockable(kind)) {
            if super::ranged::blocking_towards(&st, now, src) {
                let hand = st.using.as_ref().map_or(1, |u| u.hand);
                let slot = if hand == 1 { super::inventory::OFFHAND } else { super::inventory::HOTBAR + st.held_slot };
                let broke = if amount >= 3.0 { wear_locked(player, &mut st, slot, 1 + amount.floor() as i32) } else { None };
                drop(st);
                let mut p = packet(id::play::ENTITY_STATUS);
                p.i32(player.eid).i8(broke.unwrap_or(29));
                server.broadcast_tracked(player.eid, &p, true);
                if matches!(kind, "arrow" | "trident") {
                    super::advancements::grant(server, player, "story/deflect_arrow");
                }
                if broke.is_some() {
                    player.state.lock().unwrap().using = None;
                    server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
                }
                // Melee attackers get knocked back.
                if matches!(kind, "mob_attack") {
                    if let Some(a) = &attacker {
                        super::mobs::push_away(server, a.eid, [src[0] - pos_of(player)[0], 0.0, src[2] - pos_of(player)[2]]);
                    }
                }
                return false;
            }
        }
        st.hurt_cooldown = 10;
        let (mut amount, broken) = mitigate(player, &mut st, amount, kind);
        if let Some(a) = super::effects::level(&st, "resistance").filter(|_| !bypass) {
            amount *= (1.0 - 0.2 * (a + 1) as f32).max(0.0);
        }
        // Absorption: golden hearts are consumed first.
        if st.absorption > 0.0 {
            let absorbed = amount.min(st.absorption);
            st.absorption -= absorbed;
            amount -= absorbed;
            let mut m = packet(id::play::ENTITY_METADATA);
            m.varint(player.eid).u8(15).varint(3).f32(st.absorption).u8(0xff);
            player.send(m);
        }
        st.health = (st.health - amount).max(0.0);
        st.exhaustion += 0.1;
        st.eating = None;
        // A Totem of Undying in either hand cheats death (not the void or /kill).
        let mut totem = false;
        if st.health <= 0.0 && !bypass {
            let main = super::inventory::HOTBAR + st.held_slot;
            if let Some(slot) = [main, super::inventory::OFFHAND].into_iter().find(|&s| st.inv.slots[s].as_ref().is_some_and(|i| i.name() == "totem_of_undying")) {
                st.inv.slots[slot] = None;
                st.sync_slot(player, slot);
                st.health = 1.0;
                totem = true;
            }
        }
        player.send(health_packet(st.health, st.food, st.saturation));
        (st.health <= 0.0, broken, totem)
    };
    if totem {
        super::effects::clear(server, player);
        super::effects::add(server, player, "regeneration", 1, 900, 1.0);
        super::effects::add(server, player, "absorption", 1, 100, 1.0);
        super::effects::add(server, player, "fire_resistance", 0, 800, 1.0);
        let mut p = packet(id::play::ENTITY_STATUS);
        p.i32(player.eid).i8(35);
        server.broadcast_tracked(player.eid, &p, true);
    }
    for status in &broken {
        let mut p = packet(id::play::ENTITY_STATUS);
        p.i32(player.eid).i8(*status);
        server.broadcast_tracked(player.eid, &p, true);
    }
    if !broken.is_empty() {
        server.broadcast_tracked(player.eid, &super::play::equipment(player), false);
    }
    let pos = {
        let s = player.state.lock().unwrap();
        [s.x, s.y + 1.0, s.z]
    };
    let hurt_sound = match kind {
        "fall" if amount > 4.0 => "entity.player.big_fall",
        "fall" => "entity.player.small_fall",
        _ if died => "entity.player.death",
        _ => "entity.player.hurt",
    };
    super::sound::at(server, player.dim(), pos, hurt_sound, super::sound::PLAYER, 1.0, 1.0);
    let mut p = packet(id::play::DAMAGE_EVENT);
    let cause = attacker.as_ref().map_or(0, |a| a.eid + 1);
    p.varint(player.eid).varint(gameplay::get().damage_type(kind)).varint(cause).varint(cause).bool(false);
    server.broadcast_tracked(player.eid, &p, true);
    if died {
        let msg = death_message(&player.name, kind, attacker.as_ref().map(|a| a.name.as_str()));
        die(server, player, &msg);
    }
    true
}

fn death_message(name: &str, kind: &str, killer: Option<&str>) -> String {
    match (kind, killer) {
        (_, Some(k)) => format!("{name} fue asesinado por {k}"),
        ("fall", _) => format!("{name} cayó desde muy alto"),
        ("out_of_world", _) => format!("{name} cayó al vacío"),
        ("starve", _) => format!("{name} murió de hambre"),
        ("lava", _) => format!("{name} intentó nadar en lava"),
        ("explosion" | "player_explosion", _) => format!("{name} voló por los aires"),
        ("drown", _) => format!("{name} se ahogó"),
        ("outside_border", _) => format!("{name} murió fuera del borde del mundo"),
        _ => format!("{name} murió"),
    }
}

pub fn die(server: &Arc<Server>, player: &Arc<Player>, msg: &str) {
    let (items, pos, xp) = {
        let mut st = player.state.lock().unwrap();
        if st.dead {
            return;
        }
        // keepInventory: items and experience stay with the player.
        let keep = server.rule("keepInventory");
        let xp = if keep { 0 } else { super::experience::death_drop(&st) };
        if !keep {
            (st.xp_level, st.xp_progress, st.xp_total) = (0, 0.0, 0);
        }
        player.send(super::experience::packet_for(&st));
        st.last_death = Some((player.dim(), crate::protocol::BlockPos::new(st.x.floor() as i32, st.y.floor() as i32, st.z.floor() as i32)));
        st.dead = true;
        st.health = 0.0;
        st.eating = None;
        st.fire_ticks = 0;
        st.absorption = 0.0;
        st.digging = None;
        st.fall_distance = 0.0;
        let items = if keep { Vec::new() } else { st.inv.drain() };
        let p = st.inv.window_items();
        player.send(p);
        player.send(health_packet(0.0, st.food, st.saturation));
        (items, [st.x, st.y + 0.5, st.z], xp)
    };
    entities::spawn_xp(server, player.dim(), pos, xp);
    super::vehicles::dismount(server, player, false);
    super::effects::clear(server, player);
    // Death screen for them; animation for everyone else.
    let mut p = packet(id::play::DEATH_COMBAT_EVENT);
    p.varint(player.eid);
    text(msg).write_network(&mut p);
    player.send(p);
    let mut meta = packet(id::play::ENTITY_METADATA);
    meta.varint(player.eid).u8(9).varint(3).f32(0.0).u8(0xff);
    server.broadcast_tracked(player.eid, &meta, false);
    let mut status = packet(id::play::ENTITY_STATUS);
    status.i32(player.eid).i8(3);
    server.broadcast_tracked(player.eid, &status, false);
    if server.rule("showDeathMessages") {
        server.broadcast_message(&format!("§c{msg}"));
    }
    for item in items {
        let a = entities::random() as f64 * std::f64::consts::TAU;
        let v = entities::random() as f64 * 0.5;
        entities::spawn(server, player.dim(), pos, [a.cos() * v * 0.3, 0.2, a.sin() * v * 0.3], item, 40);
    }
}

/// Respawns at spawn after pressing "Respawn".
pub async fn respawn(server: &Arc<Server>, player: &Arc<Player>) {
    let spawn = server.spawn;
    // At their respawn anchor in the Nether, next to their bed if they have one; otherwise,
    // the world spawn point.
    let anchored = if player.state.lock().unwrap().spawn_dim == crate::world::Dimension::Nether { super::household::anchor_point(server, player) } else { None };
    let dim = if anchored.is_some() { crate::world::Dimension::Nether } else { crate::world::Dimension::Overworld };
    let [x, y, z] = anchored.or_else(|| super::beds::respawn_point(server, player)).unwrap_or([spawn.x as f64 + 0.5, spawn.y as f64, spawn.z as f64 + 0.5]);
    let game_mode = {
        let mut st = player.state.lock().unwrap();
        if !st.dead {
            return;
        }
        st.dead = false;
        st.rest = 0;
        (st.x, st.y, st.z) = (x, y, z);
        st.health = 20.0;
        st.food = 20;
        st.saturation = 5.0;
        st.exhaustion = 0.0;
        st.air = 300;
        st.hurt_cooldown = 60;
        st.loaded.clear();
        st.view_epoch = st.view_epoch.wrapping_add(1);
        st.game_mode
    };
    player.set_dim(dim);
    let mut p = packet(id::play::RESPAWN);
    p.string(dim.name()).string(dim.name()).i64(server.config.generation.options.seed);
    p.i8(game_mode as i8).u8(255).bool(false).bool(dim == crate::world::Dimension::Overworld && server.config.generation.name == "flat");
    super::gadgets::death_location(&mut p, player.state.lock().unwrap().last_death);
    p.varint(0).u8(0);
    player.send(p);
    player.send(super::play::abilities(game_mode));
    send_health(player);
    super::effects::resend(player);
    super::weather::send_state(server, player);
    super::border::send_state(server, player);
    super::leads::send_knots(server, player);
    super::frames::send_all(player);
    super::decor::send_all(player);
    let (inv, xp) = {
        let mut st = player.state.lock().unwrap();
        (st.inv.window_items(), super::experience::packet_for(&st))
    };
    player.send(inv);
    player.send(xp);
    let mut p = packet(id::play::SPAWN_POSITION);
    p.position(spawn).f32(0.0);
    player.send(p);
    player.send(super::update_time(server.age.load(Ordering::Relaxed), server.day_time.load(Ordering::Relaxed)));
    let mut p = packet(id::play::GAME_STATE_CHANGE);
    p.u8(13).f32(0.0);
    player.send(p);
    super::play::update_view(server, player, true).await;
    super::play::teleport(server, player, x, y, z);

    // Others see the "new" player.
    // The respawning client cleared its entities; others need to see it again.
    player.tracked.lock().unwrap().clear();
    super::tracker::respawn_for_others(server, player.eid);
}

/// Ticks it takes to eat (or drink) an item.
fn eat_ticks(item: &ItemStack) -> i64 {
    match item.name() {
        "dried_kelp" => 16,
        "honey_bottle" => 40,
        _ => 32,
    }
}

fn drinkable(item: &ItemStack) -> bool {
    matches!(item.name(), "potion" | "milk_bucket")
}

/// Starts eating or drinking if the item is food (and the player is hungry or it is always edible),
/// a potion or milk.
pub fn start_using(server: &Server, player: &Player, hand: i32) {
    let mut st = player.state.lock().unwrap();
    let Some(item) = st.inv.held(hand, st.held_slot).clone() else { return };
    if st.dead {
        return;
    }
    if drinkable(&item) {
        st.eating = Some((hand, server.age.load(Ordering::Relaxed)));
        return;
    }
    if gameplay::get().food(item.id).is_none() {
        return;
    }
    let always = matches!(item.name(), "golden_apple" | "enchanted_golden_apple" | "chorus_fruit" | "honey_bottle" | "suspicious_stew");
    if st.food < 20 || always || st.game_mode == 1 {
        st.eating = Some((hand, server.age.load(Ordering::Relaxed)));
    }
}

/// What to do after eating/drinking, outside the player lock.
enum AfterEating {
    Potion(String),
    Milk,
    Effects(Vec<(&'static str, i32, i32)>),
    Cure(&'static str),
}

fn finish_eating(player: &Player, st: &mut super::PlayerState, hand: i32) -> Option<AfterEating> {
    let item = st.inv.held(hand, st.held_slot).clone()?;
    let after = match item.name() {
        "potion" => Some(AfterEating::Potion(super::effects::potion_of(&item).unwrap_or_else(|| "water".into()))),
        "milk_bucket" => Some(AfterEating::Milk),
        "honey_bottle" => Some(AfterEating::Cure("poison")),
        n => {
            let list: Vec<_> = super::effects::food_effects(n).into_iter().filter(|e| entities::random() < e.3).map(|e| (e.0, e.1, e.2)).collect();
            (!list.is_empty()).then_some(AfterEating::Effects(list))
        }
    };
    if let Some((food, sat)) = gameplay::get().food(item.id) {
        st.food = (st.food + food).min(20);
        st.saturation = (st.saturation + sat).min(st.food as f32);
    } else if !drinkable(&item) {
        return None;
    }
    if st.game_mode != 1 {
        let rem = match item.name() {
            "mushroom_stew" | "rabbit_stew" | "beetroot_soup" | "suspicious_stew" => "bowl",
            "honey_bottle" | "potion" => "glass_bottle",
            "milk_bucket" => "bucket",
            _ => "",
        };
        let rem = crate::data::get().item_id(rem).map(|id| ItemStack::new(id, 1));
        let held = st.held_slot;
        let slot = st.inv.consume_held(hand, held, 1, rem);
        st.sync_slot(player, slot);
    }
    let mut p = packet(id::play::ENTITY_STATUS);
    p.i32(player.eid).i8(9);
    player.send(p);
    player.send(health_packet(st.health, st.food, st.saturation));
    after
}

/// Survival tick for a player (20 per second).
pub fn tick(server: &Arc<Server>, player: &Arc<Player>, age: i64) {
    super::portals::tick_player(server, player);
    super::gateways::check(server, player);
    super::blockmech::freeze_player(server, player, age);
    super::effects::tick(server, player);
    let mut leftovers = Vec::new();
    let mut drop_parrot = false;
    let mut ate = false;
    let mut after = None;
    let (pos, sneaking, survival) = {
        let mut st = player.state.lock().unwrap();
        if st.dead || st.game_mode == 3 {
            return;
        }
        st.hurt_cooldown = st.hurt_cooldown.saturating_sub(1);
        if st.sleeping.is_none() {
            st.rest += 1;
        }
        // A parrot on the shoulder jumps off when the player falls or sleeps.
        if st.shoulder.is_some() && (st.fall_distance > 0.5 || st.sleeping.is_some()) {
            drop_parrot = true;
        }
        // The crafting table closes if the player walks away (like vanilla's stillValid).
        if let Some(w) = &st.inv.window {
            let d2 = (w.pos.x as f64 + 0.5 - st.x).powi(2) + (w.pos.y as f64 + 0.5 - st.y).powi(2) + (w.pos.z as f64 + 0.5 - st.z).powi(2);
            if d2 > 64.0 {
                let mut p = packet(id::play::CLOSE_WINDOW);
                p.u8(w.id);
                player.send(p);
                leftovers = st.inv.close();
                let p = st.inv.window_items();
                player.send(p);
            }
        }
        if let Some((hand, start)) = st.eating {
            let ready = st.inv.held(hand, st.held_slot).as_ref().map(eat_ticks);
            match ready {
                Some(t) if age - start >= t => {
                    st.eating = None;
                    after = finish_eating(player, &mut st, hand);
                    ate = true;
                }
                None => st.eating = None,
                _ => {}
            }
        }
        ([st.x, st.y, st.z], st.sneaking, st.survival_like())
    };
    if drop_parrot {
        super::pets::unperch(server, player);
    }
    for item in leftovers {
        entities::throw_from(server, player, item);
    }
    if ate {
        super::advancements::grant(server, player, "husbandry/root");
    }
    match after {
        Some(AfterEating::Potion(p)) => super::effects::apply_potion(server, player, &p, 1.0),
        Some(AfterEating::Milk) => {
            super::effects::clear(server, player);
        }
        Some(AfterEating::Effects(list)) => {
            for (n, a, d) in list {
                super::effects::add(server, player, n, a, d, 1.0);
            }
        }
        Some(AfterEating::Cure(n)) => super::effects::remove(server, player, n),
        None => {}
    }

    // Void (also affects creative, as in vanilla).
    let dim = player.dim();
    if pos[1] < dim.min_y() as f64 - 64.0 && age % 10 == 0 {
        damage(server, player, 4.0, "out_of_world", None);
    }
    if !survival {
        return;
    }
    let (env, feet) = {
        let mut world = server.world(dim).lock().unwrap();
        let env = environment(&mut world, pos, sneaking, if sneaking { 1.27 } else { 1.62 });
        let feet = crate::data::get().block_of_state(world.get_block(crate::protocol::BlockPos::new(pos[0].floor() as i32, (pos[1] + 0.1).floor() as i32, pos[2].floor() as i32))).name.clone();
        (env, feet)
    };
    if env.in_lava {
        set_on_fire(server, player, 300);
        damage(server, player, 4.0, "lava", None);
    }
    // Fire: inside a fire block it burns and ignites; while burning, 1 damage per second.
    if feet == "fire" || feet == "soul_fire" {
        set_on_fire(server, player, 160);
        damage(server, player, if feet == "soul_fire" { 2.0 } else { 1.0 }, "in_fire", None);
    }
    let (burning, extinguished) = {
        let mut st = player.state.lock().unwrap();
        if st.fire_ticks > 0 {
            if env.in_water {
                st.fire_ticks = 0;
                (false, true)
            } else {
                st.fire_ticks -= 1;
                (st.fire_ticks % 20 == 0 && !env.in_lava, st.fire_ticks == 0)
            }
        } else {
            (false, false)
        }
    };
    if burning {
        damage(server, player, 1.0, "on_fire", None);
    }
    if extinguished {
        server.broadcast_tracked(player.eid, &super::play::metadata(player), true);
    }

    let difficulty = server.config.difficulty;
    let mut hurt = None;
    {
        let mut st = player.state.lock().unwrap();
        if st.dead {
            return;
        }
        // Air underwater.
        let old_air = st.air;
        if env.eyes_in_water && !super::effects::has(&st, "water_breathing") {
            st.air -= 1;
            if st.air <= -20 {
                st.air = 0;
                hurt = Some((2.0, "drown"));
            }
        } else {
            st.air = (st.air + 4).min(300);
        }
        if st.air != old_air {
            let mut p = packet(id::play::ENTITY_METADATA);
            p.varint(player.eid).u8(1).varint(1).varint(st.air.max(0)).u8(0xff);
            player.send(p);
        }

        // Hunger (vanilla formula).
        let before = (st.health, st.food, st.saturation);
        if st.exhaustion > 4.0 {
            st.exhaustion -= 4.0;
            if st.saturation > 0.0 {
                st.saturation = (st.saturation - 1.0).max(0.0);
            } else if difficulty > 0 {
                st.food = (st.food - 1).max(0);
            }
        }
        let regen = server.rule("naturalRegeneration");
        if !regen {
            st.regen_timer = 0;
        } else if st.saturation > 0.0 && st.food >= 20 && st.health < 20.0 {
            st.regen_timer += 1;
            if st.regen_timer >= 10 {
                let amount = st.saturation.min(6.0);
                st.health = (st.health + amount / 6.0).min(20.0);
                st.exhaustion += amount;
                st.regen_timer = 0;
            }
        } else if st.food >= 18 && st.health < 20.0 {
            st.regen_timer += 1;
            if st.regen_timer >= 80 {
                st.health = (st.health + 1.0).min(20.0);
                st.exhaustion += 6.0;
                st.regen_timer = 0;
            }
        } else if st.food <= 0 {
            st.regen_timer += 1;
            if st.regen_timer >= 80 {
                if st.health > 10.0 || difficulty >= 3 || (st.health > 1.0 && difficulty == 2) {
                    hurt = hurt.or(Some((1.0, "starve")));
                }
                st.regen_timer = 0;
            }
        } else {
            st.regen_timer = 0;
        }
        if before != (st.health, st.food, st.saturation) {
            player.send(health_packet(st.health, st.food, st.saturation));
        }
    }
    if let Some((amount, kind)) = hurt {
        damage(server, player, amount, kind, None);
    }
}

/// Melee hit damage based on the weapon.
pub fn attack_damage(item: Option<&ItemStack>) -> f32 {
    let Some(name) = item.map(|i| i.name()) else { return 1.0 };
    let tier = |n: &str| match n.split('_').next().unwrap_or("") {
        "wooden" | "golden" => 0.0,
        "stone" => 1.0,
        "iron" => 2.0,
        "diamond" => 3.0,
        "netherite" => 4.0,
        _ => 0.0,
    };
    if name.ends_with("_sword") {
        4.0 + tier(name)
    } else if name.ends_with("_axe") {
        match name {
            "wooden_axe" | "golden_axe" => 7.0,
            "stone_axe" | "iron_axe" | "diamond_axe" => 9.0,
            _ => 10.0,
        }
    } else if name.ends_with("_pickaxe") {
        2.0 + tier(name)
    } else if name.ends_with("_shovel") {
        2.5 + tier(name)
    } else if name == "trident" {
        9.0
    } else {
        1.0
    }
}

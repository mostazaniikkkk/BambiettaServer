//! Projectiles: arrows (from skeletons, bows and crossbows, tipped and spectral),
//! tridents, blaze and ghast fireballs, dragon fireballs and what players throw
//! (splash and lingering potions, ender pearls and bottles o' enchanting).
//! They hit players and mobs; arrows and tridents get stuck and can be picked up.

use super::health::Attacker;
use super::{effects, entities, explosion, health, mobs, sound, Player, Server};
use crate::data;
use crate::item::ItemStack;
use crate::protocol::{id, packet, Writer};
use crate::world::physics::{move_aabb, Aabb};
use crate::world::Dimension;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectileKind {
    Arrow,
    SmallFireball,
    Fireball,
    DragonFireball,
    /// Splash or lingering potion (the item carries the potion).
    Potion(ItemStack),
    EnderPearl,
    XpBottle,
    Trident,
    LlamaSpit,
    Snowball,
    Egg,
    /// Homes in on its target; Levitation.
    ShulkerBullet,
    WitherSkull,
}

impl ProjectileKind {
    fn entity(&self) -> &'static str {
        match self {
            ProjectileKind::Arrow => "arrow",
            ProjectileKind::SmallFireball => "small_fireball",
            ProjectileKind::Fireball => "fireball",
            ProjectileKind::DragonFireball => "dragon_fireball",
            ProjectileKind::Potion(_) => "potion",
            ProjectileKind::EnderPearl => "ender_pearl",
            ProjectileKind::XpBottle => "experience_bottle",
            ProjectileKind::Trident => "trident",
            ProjectileKind::LlamaSpit => "llama_spit",
            ProjectileKind::Snowball => "snowball",
            ProjectileKind::Egg => "egg",
            ProjectileKind::ShulkerBullet => "shulker_bullet",
            ProjectileKind::WitherSkull => "wither_skull",
        }
    }

    /// (gravity, drag) per tick.
    fn physics(&self) -> (f64, f64) {
        match self {
            ProjectileKind::Arrow | ProjectileKind::Trident => (0.05, 0.99),
            ProjectileKind::SmallFireball | ProjectileKind::Fireball | ProjectileKind::DragonFireball | ProjectileKind::WitherSkull | ProjectileKind::ShulkerBullet => (0.0, 1.0),
            ProjectileKind::XpBottle => (0.07, 0.99),
            _ => (0.03, 0.99),
        }
    }

    fn item(&self) -> Option<ItemStack> {
        let reg = data::get();
        match self {
            ProjectileKind::Potion(i) => Some(i.clone()),
            ProjectileKind::EnderPearl => reg.item_id("ender_pearl").map(|i| ItemStack::new(i, 1)),
            ProjectileKind::XpBottle => reg.item_id("experience_bottle").map(|i| ItemStack::new(i, 1)),
            ProjectileKind::SmallFireball | ProjectileKind::Fireball => reg.item_id("fire_charge").map(|i| ItemStack::new(i, 1)),
            ProjectileKind::Snowball => reg.item_id("snowball").map(|i| ItemStack::new(i, 1)),
            ProjectileKind::Egg => reg.item_id("egg").map(|i| ItemStack::new(i, 1)),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Projectile {
    pub eid: i32,
    pub dim: Dimension,
    pub kind: ProjectileKind,
    /// Who launched it (player or mob).
    pub shooter: i32,
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    age: u32,
    stuck: bool,
    /// Arrows and tridents launched by players (damage, enchantments, pickup).
    pub arrow: Option<Box<ArrowData>>,
    /// What it homes in on (shulker bullets).
    pub target: Option<i32>,
}

/// Data for a player's arrow or trident.
#[derive(Clone, Debug)]
pub struct ArrowData {
    /// Base damage (2 for arrows; Power adds 0.5 per level + 0.5).
    pub damage: f64,
    pub crit: bool,
    /// Punch level.
    pub knock: i32,
    /// Fire (Flame on the bow).
    pub flame: bool,
    /// 0 cannot be picked up, 1 can be picked up, 2 creative only.
    pub pickup: u8,
    /// What is returned on pickup (the specific arrow, or the trident with its damage).
    pub item: ItemStack,
    /// Entities it can still pass through (Piercing).
    pub pierce: i32,
    pub hit: Vec<i32>,
    /// Loyalty level (tridents).
    pub loyalty: i32,
    pub returning: bool,
    pub dealt: bool,
}

impl ArrowData {
    pub fn new(item: ItemStack) -> Self {
        Self { damage: 2.0, crit: false, knock: 0, flame: false, pickup: 1, item, pierce: 0, hit: vec![], loyalty: 0, returning: false, dealt: false }
    }
}

pub fn positions(server: &Server) -> Vec<(i32, [f64; 3], Dimension)> {
    server.projectiles.lock().unwrap().iter().map(|a| (a.eid, a.pos, a.dim)).collect()
}

fn spawn_packets(a: &Projectile) -> Vec<Vec<u8>> {
    let spectral = a.arrow.as_ref().is_some_and(|d| d.item.name() == "spectral_arrow");
    let entity = if spectral { "spectral_arrow" } else { a.kind.entity() };
    let mut p = packet(id::play::SPAWN_ENTITY);
    p.varint(a.eid).uuid(((a.shooter as u128) << 64) | a.eid as u128).varint(data::get().entity(entity));
    p.f64(a.pos[0]).f64(a.pos[1]).f64(a.pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(a.shooter);
    for v in a.vel {
        p.i16((v.clamp(-3.9, 3.9) * 8000.0) as i16);
    }
    let mut out = vec![p];
    if let Some(item) = a.kind.item() {
        let mut m = packet(id::play::ENTITY_METADATA);
        m.varint(a.eid).u8(8).varint(7);
        crate::item::write_slot(&mut m, &Some(item));
        m.u8(0xff);
        out.push(m);
    }
    if let Some(d) = &a.arrow {
        // 8: flags (0x01 critical), 9: piercing; tipped arrow: 10 color; trident: 10 loyalty, 11 glint.
        let mut m = packet(id::play::ENTITY_METADATA);
        m.varint(a.eid).u8(8).varint(0).i8(if d.crit { 0x01 } else { 0 });
        m.u8(9).varint(0).i8(d.pierce.clamp(0, 127) as i8);
        if a.kind == ProjectileKind::Trident {
            m.u8(10).varint(0).i8(d.loyalty.clamp(0, 127) as i8);
            m.u8(11).varint(8).bool(d.item.is_enchanted());
        } else if !spectral {
            let color = effects::potion_of(&d.item).and_then(|p| effects::potion_effects(&p).first().map(|e| effects::color(e.0))).unwrap_or(-1);
            m.u8(10).varint(1).varint(color);
        }
        m.u8(0xff);
        out.push(m);
    }
    out
}

pub fn spawn_packets_for(server: &Server, eids: &[i32]) -> HashMap<i32, Vec<Vec<u8>>> {
    if eids.is_empty() {
        return HashMap::new();
    }
    let list = server.projectiles.lock().unwrap();
    list.iter().filter(|a| eids.contains(&a.eid)).map(|a| (a.eid, spawn_packets(a))).collect()
}

pub fn shoot(server: &Server, dim: Dimension, kind: ProjectileKind, shooter: i32, from: [f64; 3], vel: [f64; 3]) -> i32 {
    shoot_with(server, dim, kind, shooter, from, vel, None)
}

/// A player's arrow or trident.
pub fn shoot_with(server: &Server, dim: Dimension, kind: ProjectileKind, shooter: i32, from: [f64; 3], vel: [f64; 3], arrow: Option<ArrowData>) -> i32 {
    let a = Projectile { eid: server.next_entity_id(), dim, kind, shooter, pos: from, vel, age: 0, stuck: false, arrow: arrow.map(Box::new), target: None };
    let eid = a.eid;
    server.projectiles.lock().unwrap().push(a);
    eid
}

/// A projectile that homes in on `target` (a shulker bullet).
pub fn shoot_at(server: &Server, dim: Dimension, kind: ProjectileKind, shooter: i32, from: [f64; 3], vel: [f64; 3], target: i32) -> i32 {
    let eid = shoot(server, dim, kind, shooter, from, vel);
    if let Some(p) = server.projectiles.lock().unwrap().iter_mut().find(|p| p.eid == eid) {
        p.target = Some(target);
    }
    eid
}

/// Look direction.
pub fn look(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (yaw.to_radians() as f64, pitch.to_radians() as f64);
    [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()]
}

/// The player throws what they hold in hand (potions, pearls, bottles). Returns whether it was throwable.
pub fn throw_held(server: &Arc<Server>, player: &Arc<Player>, hand: i32) -> bool {
    let (item, eye, yaw, pitch, creative) = {
        let st = player.state.lock().unwrap();
        (st.inv.held(hand, st.held_slot).clone(), st.eye(), st.yaw, st.pitch, st.game_mode == 1)
    };
    let Some(item) = item else { return false };
    let (kind, speed, pitch_offset, snd) = match item.name() {
        "splash_potion" | "lingering_potion" => (ProjectileKind::Potion(item.with_count(1)), 0.5, -20.0, "entity.splash_potion.throw"),
        "ender_pearl" => (ProjectileKind::EnderPearl, 1.5, 0.0, "entity.ender_pearl.throw"),
        "experience_bottle" => (ProjectileKind::XpBottle, 0.7, -20.0, "entity.experience_bottle.throw"),
        "snowball" => (ProjectileKind::Snowball, 1.5, 0.0, "entity.snowball.throw"),
        "egg" => (ProjectileKind::Egg, 1.5, 0.0, "entity.egg.throw"),
        _ => return false,
    };
    {
        let mut st = player.state.lock().unwrap();
        if kind == ProjectileKind::EnderPearl {
            let now = server.age.load(std::sync::atomic::Ordering::Relaxed);
            if st.pearl_cooldown > now {
                return true;
            }
            st.pearl_cooldown = now + 20;
            let mut c = packet(id::play::SET_COOLDOWN);
            c.varint(item.id).varint(20);
            player.send(c);
        }
        if !creative {
            let h = st.held_slot;
            let slot = st.inv.consume_held(hand, h, 1, None);
            st.sync_slot(player, slot);
        }
    }
    let d = look(yaw, pitch + pitch_offset);
    let vel = [d[0] * speed, d[1] * speed, d[2] * speed];
    shoot(server, player.dim(), kind, player.eid, [eye[0], eye[1] - 0.1, eye[2]], vel);
    sound::at(server, player.dim(), eye, snd, sound::PLAYER, 0.5, 0.4);
    true
}

/// Does segment a->b intersect the box?
pub fn segment_hits(a: [f64; 3], b: [f64; 3], bx: &Aabb) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 1e-12 {
            if a[i] < bx.min[i] || a[i] > bx.max[i] {
                return false;
            }
        } else {
            let (mut ta, mut tb) = ((bx.min[i] - a[i]) / d, (bx.max[i] - a[i]) / d);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// What it hit.
enum Hit {
    Player(Arc<Player>),
    Mob(i32),
    Block,
    /// Only the stuck sound.
    Sound(&'static str),
}

/// A player hits a projectile: ghast fireballs get deflected back.
pub fn deflect(server: &Arc<Server>, player: &Arc<Player>, eid: i32) -> bool {
    let (yaw, pitch) = {
        let st = player.state.lock().unwrap();
        (st.yaw, st.pitch)
    };
    let mut list = server.projectiles.lock().unwrap();
    let Some(a) = list.iter_mut().find(|a| a.eid == eid) else { return false };
    if a.kind == ProjectileKind::Fireball {
        let d = look(yaw, pitch);
        let speed = (a.vel.iter().map(|v| v * v).sum::<f64>()).sqrt().max(0.5);
        a.vel = [d[0] * speed, d[1] * speed, d[2] * speed];
        a.shooter = player.eid;
    }
    true
}

pub fn tick(server: &Arc<Server>) {
    struct Seen {
        player: Arc<Player>,
        dim: Dimension,
        pos: [f64; 3],
        hittable: bool,
        creative: bool,
    }
    let players: Vec<Seen> = server
        .online()
        .into_iter()
        .map(|p| {
            let (pos, hittable, creative) = {
                let s = p.state.lock().unwrap();
                ([s.x, s.y, s.z], !s.dead && s.game_mode != 3, s.game_mode == 1)
            };
            Seen { dim: p.dim(), player: p, pos, hittable, creative }
        })
        .collect();
    let mob_boxes = mobs::hit_boxes(server);
    let mut impacts: Vec<(Projectile, Hit, [f64; 3])> = Vec::new();
    // Arrows and tridents picked up: (projectile, player, whether the item is given).
    let mut pickups: Vec<(Projectile, Arc<Player>, bool)> = Vec::new();
    let mut packets = Vec::new();
    {
        let mut list = server.projectiles.lock().unwrap();
        if list.is_empty() {
            return;
        }
        let mut worlds: Vec<Option<std::sync::MutexGuard<'_, crate::world::World>>> = Dimension::ALL.iter().map(|_| None).collect();
        list.retain_mut(|a| {
            let world = worlds[a.dim.index()].get_or_insert_with(|| server.world(a.dim).lock().unwrap());
            a.age += 1;
            let by_player = a.arrow.is_some();
            if (a.age > 600 && !a.stuck) || (a.stuck && a.age > if by_player { 1200 } else { 100 }) {
                return false;
            }
            // Loyalty trident returning to its owner (passes through blocks).
            if let Some(d) = a.arrow.as_mut().filter(|d| d.loyalty > 0 && (d.dealt || (a.stuck && a.age > 4))) {
                d.returning = true;
            }
            if a.arrow.as_ref().is_some_and(|d| d.returning) {
                let owner = players.iter().find(|s| s.player.eid == a.shooter && s.dim == a.dim && s.hittable);
                let Some(o) = owner else {
                    a.arrow.as_mut().unwrap().returning = false;
                    a.arrow.as_mut().unwrap().loyalty = 0;
                    return true;
                };
                let target = [o.pos[0], o.pos[1] + 1.2, o.pos[2]];
                let to = [target[0] - a.pos[0], target[1] - a.pos[1], target[2] - a.pos[2]];
                let dist = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
                if dist < 1.6 {
                    pickups.push((a.clone(), o.player.clone(), !o.creative));
                    return false;
                }
                let k = 0.05 * a.arrow.as_ref().unwrap().loyalty as f64;
                for i in 0..3 {
                    a.vel[i] = a.vel[i] * 0.95 + to[i] / dist * k * 4.0;
                    a.pos[i] += a.vel[i];
                }
                a.stuck = false;
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(a.eid).f64(a.pos[0]).f64(a.pos[1]).f64(a.pos[2]).u8(0).u8(0).bool(false);
                packets.push((a.eid, p));
                return true;
            }
            if a.stuck {
                // Pickup: the player walks over the stuck arrow.
                if let Some(d) = &a.arrow {
                    if let Some(s) = players.iter().find(|s| {
                        s.hittable && s.dim == a.dim && (s.pos[0] - a.pos[0]).abs() < 1.3 && (s.pos[2] - a.pos[2]).abs() < 1.3 && a.pos[1] - s.pos[1] > -1.0 && a.pos[1] - s.pos[1] < 2.3
                    }) {
                        match d.pickup {
                            1 => {
                                pickups.push((a.clone(), s.player.clone(), !s.creative));
                                return false;
                            }
                            2 if s.creative => {
                                pickups.push((a.clone(), s.player.clone(), false));
                                return false;
                            }
                            _ => {}
                        }
                    }
                }
                return true;
            }
            // A shulker bullet steers towards its target.
            if let Some(t) = a.target {
                let at = players
                    .iter()
                    .find(|s| s.player.eid == t && s.dim == a.dim && s.hittable)
                    .map(|s| [s.pos[0], s.pos[1] + 0.9, s.pos[2]])
                    .or_else(|| mob_boxes.iter().find(|(e, (d, _))| *e == t && *d == a.dim).map(|(_, (_, b))| [(b.min[0] + b.max[0]) / 2.0, (b.min[1] + b.max[1]) / 2.0, (b.min[2] + b.max[2]) / 2.0]));
                match at {
                    Some(at) => {
                        let to = [at[0] - a.pos[0], at[1] - a.pos[1], at[2] - a.pos[2]];
                        let l = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt().max(1e-6);
                        for i in 0..3 {
                            a.vel[i] = a.vel[i] * 0.8 + to[i] / l * 0.15 * 0.2 * 5.0 * 0.2;
                        }
                    }
                    None if a.age > 200 => return false,
                    None => {}
                }
            }
            let next = [a.pos[0] + a.vel[0], a.pos[1] + a.vel[1], a.pos[2] + a.vel[2]];
            // Does it hit a player or a mob? (not the shooter during the first ticks)
            let skip_shooter = a.age < 5;
            let already = |eid: i32| a.arrow.as_ref().is_some_and(|d| d.hit.contains(&eid) || d.dealt);
            let hit = players
                .iter()
                .find(|s| {
                    s.hittable
                        && s.dim == a.dim
                        && !(skip_shooter && s.player.eid == a.shooter)
                        && !already(s.player.eid)
                        && segment_hits(a.pos, next, &Aabb::entity(s.pos[0], s.pos[1], s.pos[2], 0.6, 1.8).grow(0.3, 0.3, 0.3))
                })
                .map(|s| (s.player.eid, Hit::Player(s.player.clone())))
                .or_else(|| {
                    mob_boxes
                        .iter()
                        .find(|(eid, (dim, bx))| *dim == a.dim && *eid != a.shooter && !already(*eid) && segment_hits(a.pos, next, &bx.grow(0.3, 0.3, 0.3)))
                        .map(|(eid, _)| (*eid, Hit::Mob(*eid)))
                });
            if let Some((eid, h)) = hit {
                impacts.push((a.clone(), h, a.pos));
                match a.arrow.as_mut() {
                    // Piercing: keeps going.
                    Some(d) if d.pierce > 0 && a.kind == ProjectileKind::Arrow => {
                        d.pierce -= 1;
                        d.hit.push(eid);
                    }
                    // The trident bounces off and falls.
                    Some(d) if a.kind == ProjectileKind::Trident => {
                        d.dealt = true;
                        a.vel = [a.vel[0] * -0.01, -0.1, a.vel[2] * -0.01];
                        return true;
                    }
                    _ => return false,
                }
            }
            // Does it hit a block?
            let aabb = Aabb::entity(a.pos[0], a.pos[1], a.pos[2], 0.1, 0.1);
            let d = move_aabb(world, &aabb, a.vel);
            if (0..3).any(|i| (d[i] - a.vel[i]).abs() > 1e-6) {
                for i in 0..3 {
                    a.pos[i] += d[i];
                }
                if matches!(a.kind, ProjectileKind::Arrow | ProjectileKind::Trident) {
                    a.stuck = true;
                    a.age = 0;
                    let snd = if a.kind == ProjectileKind::Trident { "item.trident.hit_ground" } else { "entity.arrow.hit" };
                    impacts.push((Projectile { kind: ProjectileKind::XpBottle, ..a.clone() }, Hit::Sound(snd), a.pos));
                } else {
                    impacts.push((a.clone(), Hit::Block, a.pos));
                    return false;
                }
            } else {
                a.pos = next;
                let (g, drag) = a.kind.physics();
                for v in &mut a.vel {
                    *v *= drag;
                }
                a.vel[1] -= g;
            }
            if a.age % 3 == 0 || a.stuck {
                let mut p = packet(id::play::ENTITY_TELEPORT);
                p.varint(a.eid).f64(a.pos[0]).f64(a.pos[1]).f64(a.pos[2]).u8(0).u8(0).bool(false);
                packets.push((a.eid, p));
            }
            true
        });
    }
    for (eid, p) in packets {
        server.broadcast_tracked(eid, &p, true);
    }
    for (a, hit, at) in impacts {
        if let Hit::Sound(s) = hit {
            sound::at(server, a.dim, at, s, sound::NEUTRAL, 1.0, 1.1);
            continue;
        }
        // Piercing arrows and tridents keep existing.
        let alive = server.projectiles.lock().unwrap().iter().any(|p| p.eid == a.eid);
        if !alive {
            destroy(server, a.eid);
        }
        impact(server, &a, hit, at);
    }
    for (a, player, give) in pickups {
        let mut c = packet(id::play::COLLECT);
        c.varint(a.eid).varint(player.eid).varint(1);
        server.broadcast_tracked(player.eid, &c, true);
        destroy(server, a.eid);
        if give {
            if let Some(d) = &a.arrow {
                let rest = {
                    let mut st = player.state.lock().unwrap();
                    let (rest, changed) = st.inv.add(d.item.with_count(1));
                    for s in changed {
                        st.sync_slot(&player, s);
                    }
                    rest
                };
                if let Some(r) = rest {
                    entities::throw_from(server, &player, r);
                }
            }
        }
        let snd = if a.kind == ProjectileKind::Trident { "item.trident.return" } else { "entity.item.pickup" };
        sound::at(server, a.dim, a.pos, snd, sound::PLAYER, 0.2, 1.4);
    }
}

fn destroy(server: &Server, eid: i32) {
    let mut d = packet(id::play::ENTITY_DESTROY);
    d.varint(1).varint(eid);
    for p in server.online() {
        if p.tracked.lock().unwrap().remove(&eid) {
            p.send(d.clone());
        }
    }
}

fn shooter_attacker(server: &Server, shooter: i32) -> Attacker {
    if let Some(p) = server.players.read().unwrap().get(&shooter) {
        return Attacker::player(p);
    }
    Attacker { eid: shooter, name: mobs::display_name(server, shooter).unwrap_or_else(|| "?".into()), pos: None }
}

fn impact(server: &Arc<Server>, a: &Projectile, hit: Hit, at: [f64; 3]) {
    let mut by = shooter_attacker(server, a.shooter);
    // The shield blocks what comes from the front: the position is the projectile's.
    by.pos = Some([at[0] - a.vel[0], at[1] - a.vel[1], at[2] - a.vel[2]]);
    let by_player = server.players.read().unwrap().contains_key(&a.shooter);
    let speed = (a.vel.iter().map(|v| v * v).sum::<f64>()).sqrt();
    let hurt = |amount: f32, kind: &str, fire: bool| match &hit {
        Hit::Player(p) => {
            let dmg = if by_player { amount } else { mobs::difficulty_damage(server, amount) };
            let ok = health::damage(server, p, dmg, kind, Some(by.clone()));
            if ok && fire {
                health::set_on_fire(server, p, 100);
            }
            if ok && matches!(a.kind, ProjectileKind::Arrow | ProjectileKind::Trident) {
                let len = (a.vel[0] * a.vel[0] + a.vel[2] * a.vel[2]).sqrt().max(1e-6);
                // Bow Punch: 0.6 per level on top of the normal knockback.
                let k = 0.3 + 0.6 * a.arrow.as_ref().map_or(0, |d| d.knock) as f64;
                let mut v = packet(id::play::ENTITY_VELOCITY);
                v.varint(p.eid).i16((a.vel[0] / len * k * 8000.0) as i16).i16((0.2 * 8000.0) as i16).i16((a.vel[2] / len * k * 8000.0) as i16);
                p.state.lock().unwrap().ac.grace = 20;
                server.broadcast_tracked(p.eid, &v, true);
            }
        }
        Hit::Mob(eid) => {
            let k = 1.0 + 3.0 * a.arrow.as_ref().map_or(0, |d| d.knock) as f64;
            let knock = matches!(a.kind, ProjectileKind::Arrow | ProjectileKind::Trident).then(|| [a.vel[0] * 0.2 * k, 0.3, a.vel[2] * 0.2 * k]);
            mobs::hurt(server, *eid, amount, kind, Some(a.shooter), knock, i32::from(fire));
        }
        Hit::Block | Hit::Sound(_) => {}
    };
    match &a.kind {
        ProjectileKind::Arrow => match &a.arrow {
            Some(d) => {
                // Like vanilla: damage = speed x base damage, and crits add a random bonus.
                let mut dmg = (speed * d.damage).ceil();
                if d.crit {
                    dmg += (entities::random() as f64 * (dmg / 2.0 + 2.0)).floor();
                }
                hurt(dmg as f32, "arrow", d.flame);
                if let Hit::Player(p) = &hit {
                    if let Some(potion) = effects::potion_of(&d.item) {
                        // Tipped arrow: potion effects last 1/8.
                        for (n, amp, dur) in effects::potion_effects(&potion) {
                            effects::add(server, p, n, amp, (dur / 8).max(1), 1.0);
                        }
                    }
                    if d.item.name() == "spectral_arrow" {
                        effects::add(server, p, "glowing", 0, 200, 1.0);
                    }
                }
                sound::at(server, a.dim, at, "entity.arrow.hit", sound::NEUTRAL, 1.0, 1.1);
            }
            None => hurt((speed * 2.0).ceil() as f32, "arrow", false),
        },
        ProjectileKind::LlamaSpit => hurt(1.0, "mob_projectile", false),
        ProjectileKind::Snowball => {
            // Harmless, except to blazes.
            let blaze = matches!(&hit, Hit::Mob(e) if mobs::kind_of(server, *e) == Some(mobs::MobKind::Blaze));
            if !matches!(hit, Hit::Block | Hit::Sound(_)) {
                hurt(if blaze { 3.0 } else { 0.01 }, "thrown", false);
            }
        }
        ProjectileKind::Egg => {
            if !matches!(hit, Hit::Sound(_)) {
                if !matches!(hit, Hit::Block) {
                    hurt(0.01, "thrown", false);
                }
                // One in eight hatches a chick (and now and then four).
                if entities::random() < 0.125 {
                    let n = if entities::random() < 1.0 / 32.0 { 4 } else { 1 };
                    for _ in 0..n {
                        let eid = mobs::spawn(server, a.dim, mobs::MobKind::Chicken, at);
                        if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
                            m.grow = super::animals::BABY;
                        }
                    }
                }
            }
        }
        ProjectileKind::ShulkerBullet => {
            hurt(4.0, "mob_projectile", false);
            if let Hit::Player(p) = &hit {
                effects::add(server, p, "levitation", 0, 200, 1.0);
            }
            sound::at(server, a.dim, at, "entity.shulker_bullet.hit", sound::HOSTILE, 1.0, 1.0);
        }
        ProjectileKind::WitherSkull => {
            hurt(8.0, "wither_skull", false);
            if let Hit::Player(p) = &hit {
                let secs = if server.config.difficulty >= 3 { 40 } else { 10 };
                effects::add(server, p, "wither", 1, secs * 20, 1.0);
            }
            explosion::explode_ex(server, a.dim, at, 1.0, Some(by.clone()), false);
        }
        ProjectileKind::Trident => {
            hurt(8.0, "trident", false);
            sound::at(server, a.dim, at, "item.trident.hit", sound::PLAYER, 1.0, 1.0);
        }
        ProjectileKind::SmallFireball => {
            hurt(5.0, "fireball", true);
            if matches!(hit, Hit::Block) {
                super::fire::ignite(server, a.dim, crate::protocol::BlockPos::new((at[0] - a.vel[0] * 0.1).floor() as i32, (at[1] - a.vel[1] * 0.1).floor() as i32, (at[2] - a.vel[2] * 0.1).floor() as i32));
            }
        }
        ProjectileKind::Fireball => {
            // A fireball deflected by a player kills the ghast in one hit (like vanilla).
            match &hit {
                Hit::Mob(eid) if by_player && mobs::kind_of(server, *eid) == Some(mobs::MobKind::Ghast) => {
                    mobs::hurt(server, *eid, 1000.0, "player_attack", Some(a.shooter), None, 0);
                }
                _ => hurt(6.0, "fireball", false),
            }
            explosion::explode_ex(server, a.dim, at, 1.0, Some(by.clone()), true);
        }
        ProjectileKind::DragonFireball => {
            hurt(6.0, "dragon_breath", false);
            // The breath cloud: damages nearby players.
            for p in server.online().into_iter().filter(|p| p.dim() == a.dim) {
                let pos = {
                    let s = p.state.lock().unwrap();
                    [s.x, s.y, s.z]
                };
                if (0..3).map(|i| (pos[i] - at[i]).powi(2)).sum::<f64>() < 9.0 {
                    effects::add(server, &p, "instant_damage", 0, 1, 1.0);
                }
            }
            sound::at(server, a.dim, at, "entity.dragon_fireball.explode", sound::HOSTILE, 1.0, 1.0);
        }
        ProjectileKind::Potion(item) => splash(server, a, item, at),
        ProjectileKind::EnderPearl => {
            if let Some(p) = server.players.read().unwrap().get(&a.shooter).cloned() {
                if p.dim() == a.dim && !p.state.lock().unwrap().dead {
                    super::vehicles::dismount(server, &p, false);
                    super::play::teleport(server, &p, at[0], at[1], at[2]);
                    p.state.lock().unwrap().hurt_cooldown = 0;
                    health::damage(server, &p, 5.0, "fall", None);
                    if entities::random() < 0.05 {
                        mobs::spawn(server, a.dim, mobs::MobKind::Endermite, at);
                    }
                }
            }
            sound::at(server, a.dim, at, "entity.ender_pearl.throw", sound::PLAYER, 1.0, 1.0);
        }
        ProjectileKind::XpBottle => {
            world_event(server, a.dim, at, 2002, 0x385DC6);
            let xp = 3 + (entities::random() * 5.0) as i32 + (entities::random() * 5.0) as i32;
            entities::spawn_xp(server, a.dim, at, xp);
        }
    }
}

fn world_event(server: &Server, dim: Dimension, at: [f64; 3], event: i32, data: i32) {
    let mut p = packet(id::play::WORLD_EVENT);
    p.i32(event).position(crate::protocol::BlockPos::new(at[0].floor() as i32, at[1].floor() as i32, at[2].floor() as i32)).i32(data).bool(false);
    server.broadcast_dim(dim, &p, None);
}

/// Splash potion: effects for anyone nearby, stronger the closer they are.
fn splash(server: &Arc<Server>, a: &Projectile, item: &ItemStack, at: [f64; 3]) {
    let potion = effects::potion_of(item).unwrap_or_else(|| "water".into());
    let list = effects::potion_effects(&potion);
    let color = list.first().map_or(0x385DC6, |(n, _, _)| effects::color(n));
    let instant = list.iter().any(|(n, _, _)| n.starts_with("instant_"));
    world_event(server, a.dim, at, if instant { 2007 } else { 2002 }, color);
    let lingering = item.name() == "lingering_potion";
    for p in server.online().into_iter().filter(|p| p.dim() == a.dim) {
        let pos = {
            let s = p.state.lock().unwrap();
            [s.x, s.y + 0.9, s.z]
        };
        let d2: f64 = (0..3).map(|i| (pos[i] - at[i]).powi(2)).sum();
        if d2 < 16.0 {
            let potency = (1.0 - d2.sqrt() / 4.0) * if lingering { 0.25 } else { 1.0 };
            effects::apply_potion(server, &p, &potion, potency.max(0.1));
        }
    }
    // Instant health and damage also apply to mobs (inverted for undead).
    for (eid, (dim, bx)) in mobs::hit_boxes(server) {
        if dim != a.dim {
            continue;
        }
        let c = [(bx.min[0] + bx.max[0]) / 2.0, (bx.min[1] + bx.max[1]) / 2.0, (bx.min[2] + bx.max[2]) / 2.0];
        let d2: f64 = (0..3).map(|i| (c[i] - at[i]).powi(2)).sum();
        if d2 >= 16.0 {
            continue;
        }
        let potency = 1.0 - d2.sqrt() / 4.0;
        for (name, amp, dur) in &list {
            let amount = ((if *name == "instant_damage" { 6 } else { 4 } << amp) as f64 * potency) as f32;
            match *name {
                "instant_damage" => mobs::magic(server, eid, amount, false, a.shooter),
                "instant_health" => mobs::magic(server, eid, amount, true, a.shooter),
                "weakness" => mobs::weaken(server, eid, (*dur as f64 * potency) as u32),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_and_kinds() {
        let bx = Aabb::entity(0.0, 0.0, 0.0, 0.6, 1.8);
        assert!(segment_hits([-1.0, 1.0, 0.0], [1.0, 1.0, 0.0], &bx));
        assert!(!segment_hits([-1.0, 3.0, 0.0], [1.0, 3.0, 0.0], &bx));
        for k in [ProjectileKind::Arrow, ProjectileKind::SmallFireball, ProjectileKind::Fireball, ProjectileKind::DragonFireball, ProjectileKind::EnderPearl, ProjectileKind::XpBottle] {
            assert!(data::get().entity(k.entity()) > 0, "{:?}", k);
        }
    }
}

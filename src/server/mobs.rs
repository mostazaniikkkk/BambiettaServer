//! Mobs: animals, Overworld monsters, Nether mobs (piglins, zombified piglins, blazes, ghasts,
//! magma cubes, wither skeletons, hoglins, striders) and End mobs (endermen, endermites, the
//! dragon and its crystals), and villagers. Simple AI, physics with collisions, natural
//! spawning, combat, loot, sounds and projectiles.

use super::health::Attacker;
use super::projectiles::ProjectileKind;
use super::{effects, entities, explosion, health, sound, Player, Server};
use crate::data;
use crate::gameplay;
use crate::item::ItemStack;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::physics::{move_aabb, Aabb};
use crate::world::{Dimension, World};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Maximum monsters and animals near each player.
const HOSTILE_CAP: usize = 20;
const PASSIVE_CAP: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum MobKind {
    Cow,
    Pig,
    Sheep,
    Chicken,
    Zombie,
    Skeleton,
    Creeper,
    Spider,
    Villager,
    ZombifiedPiglin,
    Piglin,
    Blaze,
    Ghast,
    MagmaCube,
    WitherSkeleton,
    Hoglin,
    Strider,
    Enderman,
    Endermite,
    EnderDragon,
    EndCrystal,
}

pub const ALL: [MobKind; 21] = [
    MobKind::Cow,
    MobKind::Pig,
    MobKind::Sheep,
    MobKind::Chicken,
    MobKind::Zombie,
    MobKind::Skeleton,
    MobKind::Creeper,
    MobKind::Spider,
    MobKind::Villager,
    MobKind::ZombifiedPiglin,
    MobKind::Piglin,
    MobKind::Blaze,
    MobKind::Ghast,
    MobKind::MagmaCube,
    MobKind::WitherSkeleton,
    MobKind::Hoglin,
    MobKind::Strider,
    MobKind::Enderman,
    MobKind::Endermite,
    MobKind::EnderDragon,
    MobKind::EndCrystal,
];

impl MobKind {
    pub fn name(self) -> &'static str {
        match self {
            MobKind::Cow => "cow",
            MobKind::Pig => "pig",
            MobKind::Sheep => "sheep",
            MobKind::Chicken => "chicken",
            MobKind::Zombie => "zombie",
            MobKind::Skeleton => "skeleton",
            MobKind::Creeper => "creeper",
            MobKind::Spider => "spider",
            MobKind::Villager => "villager",
            MobKind::ZombifiedPiglin => "zombified_piglin",
            MobKind::Piglin => "piglin",
            MobKind::Blaze => "blaze",
            MobKind::Ghast => "ghast",
            MobKind::MagmaCube => "magma_cube",
            MobKind::WitherSkeleton => "wither_skeleton",
            MobKind::Hoglin => "hoglin",
            MobKind::Strider => "strider",
            MobKind::Enderman => "enderman",
            MobKind::Endermite => "endermite",
            MobKind::EnderDragon => "ender_dragon",
            MobKind::EndCrystal => "end_crystal",
        }
    }

    pub fn from_name(n: &str) -> Option<Self> {
        ALL.into_iter().find(|k| k.name() == n)
    }

    pub fn display(self) -> &'static str {
        match self {
            MobKind::Cow => "Vaca",
            MobKind::Pig => "Cerdo",
            MobKind::Sheep => "Oveja",
            MobKind::Chicken => "Gallina",
            MobKind::Zombie => "Zombi",
            MobKind::Skeleton => "Esqueleto",
            MobKind::Creeper => "Creeper",
            MobKind::Spider => "Araña",
            MobKind::Villager => "Aldeano",
            MobKind::ZombifiedPiglin => "Piglin zombificado",
            MobKind::Piglin => "Piglin",
            MobKind::Blaze => "Blaze",
            MobKind::Ghast => "Ghast",
            MobKind::MagmaCube => "Cubo de magma",
            MobKind::WitherSkeleton => "Esqueleto wither",
            MobKind::Hoglin => "Hoglin",
            MobKind::Strider => "Lacayo",
            MobKind::Enderman => "Enderman",
            MobKind::Endermite => "Endermite",
            MobKind::EnderDragon => "Dragón del End",
            MobKind::EndCrystal => "Cristal del End",
        }
    }

    /// Monster: despawns far from players and counts towards the monster cap.
    pub fn hostile(self) -> bool {
        matches!(
            self,
            MobKind::Zombie
                | MobKind::Skeleton
                | MobKind::Creeper
                | MobKind::Spider
                | MobKind::ZombifiedPiglin
                | MobKind::Piglin
                | MobKind::Blaze
                | MobKind::Ghast
                | MobKind::MagmaCube
                | MobKind::WitherSkeleton
                | MobKind::Hoglin
                | MobKind::Enderman
                | MobKind::Endermite
        )
    }

    /// Attacks players unprovoked.
    fn aggressive(self) -> bool {
        self.hostile() && !matches!(self, MobKind::ZombifiedPiglin | MobKind::Enderman)
    }

    fn base_size(self) -> (f64, f64) {
        match self {
            MobKind::Cow => (0.9, 1.4),
            MobKind::Pig => (0.9, 0.9),
            MobKind::Sheep => (0.9, 1.3),
            MobKind::Chicken => (0.4, 0.7),
            MobKind::Zombie | MobKind::Villager | MobKind::ZombifiedPiglin | MobKind::Piglin => (0.6, 1.95),
            MobKind::Skeleton => (0.6, 1.99),
            MobKind::Creeper => (0.6, 1.7),
            MobKind::Spider => (1.4, 0.9),
            MobKind::Blaze => (0.6, 1.8),
            MobKind::Ghast => (4.0, 4.0),
            MobKind::MagmaCube => (0.52, 0.52),
            MobKind::WitherSkeleton => (0.7, 2.4),
            MobKind::Hoglin => (1.3965, 1.4),
            MobKind::Strider => (0.9, 1.7),
            MobKind::Enderman => (0.6, 2.9),
            MobKind::Endermite => (0.4, 0.3),
            MobKind::EnderDragon => (16.0, 8.0),
            MobKind::EndCrystal => (2.0, 2.0),
        }
    }

    fn base_health(self) -> f32 {
        match self {
            MobKind::Cow | MobKind::Pig | MobKind::Ghast => 10.0,
            MobKind::Sheep | MobKind::Endermite => 8.0,
            MobKind::Chicken => 4.0,
            MobKind::Spider | MobKind::Piglin => 16.0,
            MobKind::Hoglin | MobKind::Enderman => 40.0,
            MobKind::EnderDragon => 200.0,
            MobKind::EndCrystal => 1.0,
            _ => 20.0,
        }
    }

    /// Walking speed (blocks per tick).
    fn speed(self) -> f64 {
        match self {
            MobKind::Chicken | MobKind::Blaze => 0.06,
            MobKind::Spider => 0.11,
            MobKind::Zombie | MobKind::Creeper | MobKind::ZombifiedPiglin | MobKind::Piglin | MobKind::Endermite => 0.08,
            MobKind::Skeleton | MobKind::WitherSkeleton | MobKind::Hoglin => 0.09,
            MobKind::Enderman => 0.1,
            MobKind::Villager | MobKind::Strider | MobKind::Ghast => 0.05,
            _ => 0.07,
        }
    }

    pub fn undead(self) -> bool {
        matches!(self, MobKind::Zombie | MobKind::Skeleton | MobKind::ZombifiedPiglin | MobKind::WitherSkeleton)
    }

    pub fn fire_immune(self) -> bool {
        matches!(
            self,
            MobKind::Blaze | MobKind::Ghast | MobKind::MagmaCube | MobKind::WitherSkeleton | MobKind::ZombifiedPiglin | MobKind::Strider | MobKind::EnderDragon | MobKind::EndCrystal
        )
    }

    /// No gravity.
    fn flies(self) -> bool {
        matches!(self, MobKind::Blaze | MobKind::Ghast | MobKind::EnderDragon | MobKind::EndCrystal)
    }

    /// Living entity (the End crystal isn't one: its metadata is different).
    fn living(self) -> bool {
        self != MobKind::EndCrystal
    }

    fn sound_category(self) -> i32 {
        if self.hostile() || self == MobKind::EnderDragon { sound::HOSTILE } else { sound::NEUTRAL }
    }

    fn ambient(self) -> Option<String> {
        match self {
            MobKind::Creeper | MobKind::MagmaCube | MobKind::EndCrystal => None,
            k => Some(format!("entity.{}.ambient", k.name())),
        }
    }

    /// Base melee damage.
    fn melee(self) -> f32 {
        match self {
            MobKind::Zombie => 3.0,
            MobKind::Spider | MobKind::Endermite => 2.0,
            MobKind::ZombifiedPiglin | MobKind::Piglin => 5.0,
            MobKind::WitherSkeleton => 8.0,
            MobKind::Hoglin => 6.0,
            MobKind::Enderman => 7.0,
            MobKind::Blaze => 6.0,
            _ => 2.0,
        }
    }
}

#[derive(Clone)]
pub struct Mob {
    pub eid: i32,
    pub dim: Dimension,
    pub uuid: u128,
    pub kind: MobKind,
    pub pos: [f64; 3],
    pub vel: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub health: f32,
    pub hurt_time: u32,
    pub knockback_ticks: u32,
    pub death_ticks: Option<u32>,
    pub wander: Option<[f64; 3]>,
    pub panic: u32,
    pub target: Option<i32>,
    pub attack_cooldown: u32,
    pub fuse: Option<u32>,
    pub fire: u32,
    pub fall: f64,
    pub age: u32,
    pub sheared: bool,
    pub sent: [f64; 3],
    pub sent_yaw: f32,
    /// Magma cube size (1, 2 or 4).
    pub size: i32,
    /// Anger ticks of neutral mobs (zombified piglins, endermen).
    pub angry: u32,
    /// Shots left in the blaze's burst; ghast charge.
    pub burst: i32,
    pub charging: bool,
    /// Ticks spent admiring gold (piglins).
    pub admire: u32,
    /// Ticks standing still while trading (villagers).
    pub busy: u32,
    pub villager: Option<Box<super::villagers::VillagerData>>,
    pub dragon: Option<Box<super::dragon::DragonState>>,
}

impl Mob {
    pub fn size(&self) -> (f64, f64) {
        let (w, h) = self.kind.base_size();
        if self.kind == MobKind::MagmaCube {
            (w * self.size as f64 * 2.0 * 0.5 + 0.0, h * self.size as f64 * 2.0 * 0.5)
        } else {
            (w, h)
        }
    }

    pub fn max_health(&self) -> f32 {
        if self.kind == MobKind::MagmaCube { (self.size * self.size) as f32 } else { self.kind.base_health() }
    }

    fn aabb(&self) -> Aabb {
        let (w, h) = self.size();
        Aabb::entity(self.pos[0], self.pos[1], self.pos[2], w, h)
    }

    pub fn center(&self) -> [f64; 3] {
        [self.pos[0], self.pos[1] + self.size().1 / 2.0, self.pos[2]]
    }

}

// ---------- packets ----------

fn equipment(eid: i32, slot: u8, item: &str) -> Vec<u8> {
    let mut e = packet(id::play::ENTITY_EQUIPMENT);
    e.varint(eid).u8(slot);
    crate::item::write_slot(&mut e, &data::get().item_id(item).map(|b| ItemStack::new(b, 1)));
    e
}

fn spawn_packets(m: &Mob) -> Vec<Vec<u8>> {
    let reg = data::get();
    let mut p = packet(id::play::SPAWN_ENTITY);
    // The dragon faces the opposite way from other entities.
    let yaw = if m.kind == MobKind::EnderDragon { m.yaw + 180.0 } else { m.yaw };
    p.varint(m.eid).uuid(m.uuid).varint(reg.entity(m.kind.name()));
    p.f64(m.pos[0]).f64(m.pos[1]).f64(m.pos[2]).angle(m.pitch).angle(yaw).angle(yaw).varint(0);
    p.i16(0).i16(0).i16(0);
    let mut out = vec![p, metadata(m)];
    match m.kind {
        MobKind::Skeleton => out.push(equipment(m.eid, 0, "bow")),
        MobKind::WitherSkeleton => out.push(equipment(m.eid, 0, "stone_sword")),
        MobKind::Piglin | MobKind::ZombifiedPiglin => out.push(equipment(m.eid, 0, "golden_sword")),
        _ => {}
    }
    if m.admire > 0 {
        out.push(equipment(m.eid, 1, "gold_ingot"));
    }
    out
}

pub fn metadata(m: &Mob) -> Vec<u8> {
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(m.eid);
    // 0: flags (on fire).
    p.u8(0).varint(0).i8(if m.fire > 0 && !m.kind.fire_immune() { 0x01 } else { 0 });
    if m.kind.living() {
        // 9: health, 15: mob flags (aggressive = arms raised).
        p.u8(9).varint(3).f32(m.health.max(0.0));
        p.u8(15).varint(0).i8(if m.target.is_some() { 0x04 } else { 0 });
    }
    match m.kind {
        MobKind::Creeper => {
            p.u8(16).varint(1).varint(if m.fuse.is_some() { 1 } else { -1 });
        }
        MobKind::Sheep => {
            p.u8(17).varint(0).i8(if m.sheared { 0x10 } else { 0 });
        }
        MobKind::Villager => {
            if let Some(v) = &m.villager {
                p.u8(18).varint(18).varint(v.vtype).varint(super::villagers::profession_id(&v.profession)).varint(v.level);
            }
        }
        MobKind::Blaze => {
            p.u8(16).varint(0).i8(if m.burst > 0 { 0x01 } else { 0 });
        }
        MobKind::Ghast => {
            p.u8(16).varint(8).bool(m.charging);
        }
        MobKind::MagmaCube => {
            p.u8(16).varint(1).varint(m.size);
        }
        MobKind::Enderman => {
            p.u8(17).varint(8).bool(m.angry > 0);
        }
        MobKind::EnderDragon => {
            p.u8(16).varint(1).varint(m.dragon.as_ref().map_or(0, |d| d.phase));
        }
        MobKind::EndCrystal => {
            p.u8(9).varint(8).bool(true);
        }
        _ => {}
    }
    p.u8(0xff);
    p
}

/// Spawn packets for the requested mobs (for per-player tracking).
pub fn spawn_packets_for(server: &Server, eids: &[i32]) -> std::collections::HashMap<i32, Vec<Vec<u8>>> {
    if eids.is_empty() {
        return Default::default();
    }
    let mobs = server.mobs.lock().unwrap();
    eids.iter().filter_map(|e| mobs.get(e).filter(|m| m.death_ticks.is_none() || m.kind == MobKind::EnderDragon).map(|m| (*e, spawn_packets(m)))).collect()
}

/// Hitboxes of living mobs (for projectiles).
pub fn hit_boxes(server: &Server) -> Vec<(i32, (Dimension, Aabb))> {
    server
        .mobs
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.death_ticks.is_none())
        .map(|m| {
            let b = if m.kind == MobKind::EnderDragon { Aabb::entity(m.pos[0], m.pos[1] - 1.0, m.pos[2], 8.0, 5.0) } else { m.aabb() };
            (m.eid, (m.dim, b))
        })
        .collect()
}

pub fn display_name(server: &Server, eid: i32) -> Option<String> {
    server.mobs.lock().unwrap().get(&eid).map(|m| m.kind.display().to_string())
}

pub fn spawn(server: &Server, dim: Dimension, kind: MobKind, pos: [f64; 3]) -> i32 {
    let r = || entities::random();
    let mut m = Mob {
        eid: server.next_entity_id(),
        dim,
        uuid: ((r().to_bits() as u128) << 96) | ((r().to_bits() as u128) << 64) | ((r().to_bits() as u128) << 32) | r().to_bits() as u128,
        kind,
        pos,
        vel: [0.0; 3],
        yaw: r() * 360.0,
        pitch: 0.0,
        on_ground: false,
        health: kind.base_health(),
        hurt_time: 0,
        knockback_ticks: 0,
        death_ticks: None,
        wander: None,
        panic: 0,
        target: None,
        attack_cooldown: 0,
        fuse: None,
        fire: 0,
        fall: 0.0,
        age: 0,
        sheared: false,
        sent: pos,
        sent_yaw: 0.0,
        size: 1,
        angry: 0,
        burst: 0,
        charging: false,
        admire: 0,
        busy: 0,
        villager: None,
        dragon: None,
    };
    match kind {
        MobKind::MagmaCube => {
            m.size = [1, 2, 4][(r() * 3.0) as usize % 3];
            m.health = m.max_health();
        }
        MobKind::Villager => m.villager = Some(Box::new(super::villagers::VillagerData::random())),
        MobKind::EnderDragon => {
            m.dragon = Some(Box::default());
            // The client numbers the dragon's 8 parts with the following ids.
            for _ in 0..8 {
                server.next_entity_id();
            }
        }
        MobKind::EndCrystal => m.yaw = 0.0,
        _ => {}
    }
    let eid = m.eid;
    server.mobs.lock().unwrap().insert(eid, m);
    eid
}

/// Changes the size of a freshly created magma cube.
pub fn set_size(server: &Server, eid: i32, size: i32) {
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.size = size.max(1);
        m.health = m.max_health();
    }
}

// ---------- persistence ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedMob {
    kind: String,
    #[serde(default)]
    dim: Option<String>,
    pos: [f64; 3],
    yaw: f32,
    health: f32,
    #[serde(default)]
    sheared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    size: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    villager: Option<super::villagers::SavedVillager>,
}

fn mobs_file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("mobs.json")
}

/// Saves living mobs to `<world>/mobs.json`.
pub fn save(server: &Server) {
    let saved: Vec<SavedMob> = server
        .mobs
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.death_ticks.is_none())
        .map(|m| SavedMob {
            kind: m.kind.name().into(),
            dim: Some(m.dim.name().into()),
            pos: m.pos,
            yaw: m.yaw,
            health: m.health,
            sheared: m.sheared,
            size: (m.kind == MobKind::MagmaCube).then_some(m.size),
            villager: m.villager.as_ref().map(|v| v.save()),
        })
        .collect();
    let path = mobs_file(server);
    if let Err(e) = std::fs::write(&path, serde_json::to_string(&saved).unwrap()) {
        tracing::warn!("no se pudieron guardar los mobs: {e}");
    }
}

/// Loads saved mobs at startup.
pub fn load(server: &Server) {
    let Ok(text) = std::fs::read_to_string(mobs_file(server)) else { return };
    let Ok(saved) = serde_json::from_str::<Vec<SavedMob>>(&text) else {
        tracing::warn!("mobs.json inválido; se ignora");
        return;
    };
    let n = saved.len();
    for s in saved {
        let Some(kind) = MobKind::from_name(&s.kind) else { continue };
        let dim = Dimension::ALL.into_iter().find(|d| Some(d.name()) == s.dim.as_deref()).unwrap_or(Dimension::Overworld);
        let eid = spawn(server, dim, kind, s.pos);
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
            m.yaw = s.yaw;
            if let Some(size) = s.size {
                m.size = size.max(1);
            }
            m.health = s.health.clamp(0.5, m.max_health());
            m.sheared = s.sheared;
            if let Some(v) = s.villager {
                m.villager = Some(Box::new(super::villagers::VillagerData::load(v)));
            }
        }
    }
    tracing::info!("{n} mobs cargados");
}

// ---------- tick ----------

/// Player as seen by the AI.
#[derive(Clone)]
pub struct Seen {
    pub player: Arc<Player>,
    pub dim: Dimension,
    pub pos: [f64; 3],
    pub eye: [f64; 3],
    pub look: [f64; 3],
    pub targetable: bool,
    pub gold_armor: bool,
    pub pumpkin: bool,
}

/// Effects applied outside the mobs/world locks.
pub enum Action {
    Hurt(Arc<Player>, f32, &'static str, Attacker),
    Explode(Dimension, [f64; 3], f32, Attacker, bool),
    Shoot(Dimension, ProjectileKind, i32, [f64; 3], [f64; 3]),
    Died(Mob, Option<i32>),
    Sound(i32, String, i32),
    Effect(Arc<Player>, &'static str, i32, i32),
    Knock(Arc<Player>, [f64; 3]),
    Barter(Dimension, [f64; 3]),
    Meta(Vec<u8>, i32),
    Xp(Dimension, [f64; 3], i32),
}

pub fn difficulty_damage(server: &Server, base: f32) -> f32 {
    match server.config.difficulty {
        0 => 0.0,
        1 => (base / 2.0 + 1.0).min(base),
        3 => base * 1.5,
        _ => base,
    }
}

fn is_day(server: &Server) -> bool {
    let t = server.day_time.load(Ordering::Relaxed).rem_euclid(24000);
    !(12542..23460).contains(&t)
}

/// Does it see the sky? (nothing blocking light above).
fn sky_exposed(world: &mut World, p: [f64; 3]) -> bool {
    let reg = data::get();
    let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
    (p[1].floor() as i32 + 1..320).all(|y| reg.light(world.get_block(BlockPos::new(x, y, z))).0 == 0)
}

fn block_name(world: &mut World, p: [f64; 3], dy: f64) -> &'static str {
    data::get().block_of_state(world.get_block(BlockPos::new(p[0].floor() as i32, (p[1] + dy).floor() as i32, p[2].floor() as i32))).name.as_str()
}

fn in_water(world: &mut World, p: [f64; 3]) -> bool {
    block_name(world, p, 0.4) == "water"
}

fn in_lava(world: &mut World, p: [f64; 3]) -> bool {
    block_name(world, p, 0.2) == "lava"
}

fn seen_players(server: &Server) -> Vec<Seen> {
    server
        .online()
        .into_iter()
        .map(|p| {
            let s = p.state.lock().unwrap();
            let (yaw, pitch) = (s.yaw.to_radians() as f64, s.pitch.to_radians() as f64);
            let gold = (5..=8).any(|i| s.inv.slots[i].as_ref().is_some_and(|it| it.name().starts_with("golden_")));
            let pumpkin = s.inv.slots[5].as_ref().is_some_and(|it| it.name() == "carved_pumpkin");
            let seen = Seen {
                dim: p.dim(),
                player: p.clone(),
                pos: [s.x, s.y, s.z],
                eye: s.eye(),
                look: [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()],
                targetable: !s.dead && s.survival_like(),
                gold_armor: gold,
                pumpkin,
            };
            drop(s);
            seen
        })
        .collect()
}

pub fn tick(server: &Arc<Server>, age: i64) {
    let seen = seen_players(server);
    if age % 20 == 0 {
        natural_spawn(server, &seen, age);
    }
    super::dragon::tick_fight(server, &seen, age);

    let mut actions = Vec::new();
    let mut packets: Vec<(i32, Vec<u8>)> = Vec::new();
    let day = is_day(server);
    let peaceful = server.config.difficulty == 0;
    {
        let mut mobs = server.mobs.lock().unwrap();
        if mobs.is_empty() {
            return;
        }
        let crystals: Vec<(i32, Dimension, [f64; 3])> = mobs.values().filter(|m| m.kind == MobKind::EndCrystal && m.death_ticks.is_none()).map(|m| (m.eid, m.dim, m.pos)).collect();
        let mut remove = Vec::new();
        for dim in Dimension::ALL {
            if !mobs.values().any(|m| m.dim == dim) {
                continue;
            }
            let mut world = server.world(dim).lock().unwrap();
            let seen: Vec<Seen> = seen.iter().filter(|s| s.dim == dim).cloned().collect();
            for m in mobs.values_mut().filter(|m| m.dim == dim) {
                m.age += 1;
                if let Some(t) = m.death_ticks {
                    let t = t + 1;
                    m.death_ticks = Some(t);
                    let end = if m.kind == MobKind::EnderDragon { 200 } else { 20 };
                    if m.kind == MobKind::EnderDragon {
                        super::dragon::dying_tick(m, t, &mut actions);
                    }
                    if t >= end {
                        remove.push(m.eid);
                        if m.kind == MobKind::EnderDragon {
                            actions.push(Action::Died(m.clone(), None));
                        }
                    }
                    continue;
                }
                if m.kind == MobKind::EndCrystal {
                    continue;
                }
                let nearest = seen.iter().map(|s| dist2(&s.pos, &m.pos)).fold(f64::MAX, f64::min);
                // Despawn: monsters far from everyone (or on peaceful).
                if m.kind.hostile() && (peaceful || nearest > 128.0 * 128.0 || (nearest > 32.0 * 32.0 && entities::random() < 1.0 / 800.0)) {
                    remove.push(m.eid);
                    continue;
                }
                if nearest > 160.0 * 160.0 && m.kind != MobKind::EnderDragon {
                    continue; // too far: frozen
                }
                let look = |m: &Mob| (m.target.is_some(), m.fire > 0, m.fuse.is_some(), m.angry > 0, m.burst > 0, m.charging, m.dragon.as_ref().map(|d| d.phase));
                let before = look(m);
                let health_before = m.health;
                if m.kind == MobKind::EnderDragon {
                    super::dragon::ai(server, &mut world, m, &seen, &crystals, &mut actions);
                } else {
                    ai(server, &mut world, m, &seen, day, &mut actions);
                    physics(&mut world, m, &mut actions);
                }
                // Arms raised, on fire, about to explode...: new metadata.
                if look(m) != before || (m.kind == MobKind::EnderDragon && m.health != health_before) {
                    packets.push((m.eid, metadata(m)));
                }
                if m.health <= 0.0 && m.death_ticks.is_none() {
                    m.death_ticks = Some(0);
                    if m.kind == MobKind::EnderDragon {
                        packets.push((m.eid, metadata(m)));
                    } else {
                        actions.push(Action::Died(m.clone(), m.target));
                    }
                    continue;
                }
                // Ambient sound every now and then.
                if entities::random() < 1.0 / 240.0 {
                    if let Some(s) = m.kind.ambient() {
                        actions.push(Action::Sound(m.eid, s, m.kind.sound_category()));
                    }
                }
                // Position and rotation for clients.
                let moved = (0..3).any(|i| (m.pos[i] - m.sent[i]).abs() > 0.01);
                if moved || (m.yaw - m.sent_yaw).abs() > 2.0 {
                    let yaw = if m.kind == MobKind::EnderDragon { m.yaw + 180.0 } else { m.yaw };
                    let mut p = packet(id::play::ENTITY_TELEPORT);
                    p.varint(m.eid).f64(m.pos[0]).f64(m.pos[1]).f64(m.pos[2]).angle(yaw).angle(m.pitch).bool(m.on_ground);
                    packets.push((m.eid, p));
                    let mut h = packet(id::play::ENTITY_HEAD_ROTATION);
                    h.varint(m.eid).angle(yaw);
                    packets.push((m.eid, h));
                    m.sent = m.pos;
                    m.sent_yaw = m.yaw;
                }
            }
        }
        for eid in remove {
            mobs.remove(&eid);
        }
    }
    for (eid, p) in packets {
        server.broadcast_tracked(eid, &p, true);
    }
    run_actions(server, actions);
}

pub fn run_actions(server: &Arc<Server>, actions: Vec<Action>) {
    for a in actions {
        match a {
            Action::Hurt(p, amount, kind, by) => {
                health::damage(server, &p, amount, kind, Some(by));
            }
            Action::Explode(dim, pos, power, by, fire) => explosion::explode_ex(server, dim, pos, power, Some(by), fire),
            Action::Shoot(dim, kind, shooter, from, vel) => {
                let snd = match kind {
                    ProjectileKind::Arrow => "entity.skeleton.shoot",
                    ProjectileKind::SmallFireball => "entity.blaze.shoot",
                    ProjectileKind::Fireball => "entity.ghast.shoot",
                    _ => "entity.ender_dragon.shoot",
                };
                super::projectiles::shoot(server, dim, kind, shooter, from, vel);
                sound::entity(server, shooter, snd, sound::HOSTILE, 1.0, 1.0);
            }
            Action::Died(m, killer) => on_death(server, &m, killer),
            Action::Sound(eid, name, cat) => sound::entity(server, eid, &name, cat, 1.0, 0.9 + entities::random() * 0.2),
            Action::Effect(p, name, amp, dur) => effects::add(server, &p, name, amp, dur, 1.0),
            Action::Knock(p, v) => {
                p.state.lock().unwrap().ac.grace = 30;
                let mut pk = packet(id::play::ENTITY_VELOCITY);
                pk.varint(p.eid);
                for x in v {
                    pk.i16((x.clamp(-3.9, 3.9) * 8000.0) as i16);
                }
                server.broadcast_tracked(p.eid, &pk, true);
            }
            Action::Barter(dim, pos) => barter(server, dim, pos),
            Action::Meta(p, eid) => server.broadcast_tracked(eid, &p, true),
            Action::Xp(dim, pos, n) => entities::spawn_xp(server, dim, pos, n),
        }
    }
}

pub fn dist2(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum()
}

fn normalize(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-9);
    [v[0] / l, v[1] / l, v[2] / l]
}

/// Random enderman teleport (up to 32 blocks), onto solid ground with room.
fn teleport_random(world: &mut World, m: &mut Mob, around: [f64; 3], range: f64) -> bool {
    for _ in 0..16 {
        let x = around[0] + (entities::random() as f64 - 0.5) * 2.0 * range;
        let z = around[2] + (entities::random() as f64 - 0.5) * 2.0 * range;
        let y0 = around[1] as i32 + ((entities::random() as f64 - 0.5) * range) as i32;
        if let Some(spot) = spawn_spot(world, x.floor() as i32, z.floor() as i32, y0, 3) {
            if block_name(world, spot, -0.5) == "water" || block_name(world, spot, 0.0) == "water" {
                continue;
            }
            m.pos = spot;
            m.vel = [0.0; 3];
            m.sent = [f64::MAX; 3];
            return true;
        }
    }
    false
}

fn ai(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], day: bool, actions: &mut Vec<Action>) {
    m.attack_cooldown = m.attack_cooldown.saturating_sub(1);
    m.hurt_time = m.hurt_time.saturating_sub(1);
    m.panic = m.panic.saturating_sub(1);
    m.busy = m.busy.saturating_sub(1);
    let kind = m.kind;

    // Zombies and skeletons burn in sunlight.
    if matches!(kind, MobKind::Zombie | MobKind::Skeleton) && day && m.dim == Dimension::Overworld && m.age % 20 == 0 {
        if sky_exposed(world, m.pos) && !in_water(world, m.pos) {
            if m.fire == 0 {
                m.fire = 1;
            }
        } else {
            m.fire = 0;
        }
    }
    // Fire and lava.
    if !kind.fire_immune() {
        let feet = block_name(world, m.pos, 0.1);
        if feet == "fire" || feet == "soul_fire" || in_lava(world, m.pos) {
            m.fire = m.fire.max(1);
            if in_lava(world, m.pos) && m.age % 10 == 0 {
                m.health -= 4.0;
                m.hurt_time = 10;
            }
        }
        if in_water(world, m.pos) {
            m.fire = 0;
        }
    }
    if m.fire > 0 && !kind.fire_immune() {
        m.fire += 1;
        if m.fire % 20 == 0 {
            m.health -= 1.0;
            m.hurt_time = 10;
        }
        if (!day || !matches!(kind, MobKind::Zombie | MobKind::Skeleton)) && m.fire > 160 {
            m.fire = 0;
        }
    }
    // Water damages blazes and scares endermen away.
    if kind == MobKind::Blaze && in_water(world, m.pos) && m.age % 10 == 0 {
        m.health -= 1.0;
        m.hurt_time = 10;
    }
    if kind == MobKind::Enderman && (in_water(world, m.pos) || block_name(world, m.pos, 1.0) == "water") && teleport_random(world, m, m.pos, 32.0) {
        actions.push(Action::Sound(m.eid, "entity.enderman.teleport".into(), sound::HOSTILE));
    }

    // Piglin admiring gold: stands still, then drops the barter item.
    if m.admire > 0 {
        m.admire -= 1;
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        if m.admire == 0 {
            actions.push(Action::Barter(m.dim, m.center()));
            let mut e = packet(id::play::ENTITY_EQUIPMENT);
            e.varint(m.eid).u8(1);
            crate::item::write_slot(&mut e, &None);
            actions.push(Action::Meta(e, m.eid));
        }
        return;
    }
    // Villager trading: stands still looking at the player.
    if m.busy > 0 {
        m.vel[0] = 0.0;
        m.vel[2] = 0.0;
        return;
    }

    // Neutrals: staring at an enderman makes it angry.
    if kind == MobKind::Enderman && m.angry == 0 {
        let eye = [m.pos[0], m.pos[1] + 2.55, m.pos[2]];
        for s in seen.iter().filter(|s| s.targetable && !s.pumpkin) {
            let d = [eye[0] - s.eye[0], eye[1] - s.eye[1], eye[2] - s.eye[2]];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            if dist > 64.0 || dist < 1e-3 {
                continue;
            }
            let n = normalize(d);
            let dot = n[0] * s.look[0] + n[1] * s.look[1] + n[2] * s.look[2];
            if dot > 1.0 - 0.025 / dist {
                m.angry = 400 + (entities::random() * 400.0) as u32;
                m.target = Some(s.player.eid);
                actions.push(Action::Sound(m.eid, "entity.enderman.stare".into(), sound::HOSTILE));
                break;
            }
        }
    }
    if m.angry > 0 {
        m.angry -= 1;
        if m.angry == 0 {
            m.target = None;
        }
    }

    let mut goal: Option<([f64; 3], f64)> = None;
    let target: Option<&Seen> = if kind.aggressive() {
        let range = if kind == MobKind::Ghast { 64.0 } else if kind == MobKind::Blaze { 48.0 } else { 16.0 };
        let t = seen
            .iter()
            .filter(|s| s.targetable && dist2(&s.pos, &m.pos) < range * range && !(kind == MobKind::Piglin && s.gold_armor))
            .filter(|s| m.target.is_none() || m.target == Some(s.player.eid) || dist2(&s.pos, &m.pos) < 8.0 * 8.0)
            .min_by(|a, b| dist2(&a.pos, &m.pos).total_cmp(&dist2(&b.pos, &m.pos)));
        // A provoked piglin chases even while holding gold.
        let provoked = m.target.and_then(|t| seen.iter().find(|s| s.player.eid == t && s.targetable && dist2(&s.pos, &m.pos) < 32.0 * 32.0));
        t.or(if m.angry > 0 { provoked } else { None })
    } else if m.angry > 0 {
        m.target.and_then(|t| seen.iter().find(|s| s.player.eid == t && s.targetable && dist2(&s.pos, &m.pos) < 48.0 * 48.0))
    } else {
        None
    };
    m.target = target.map(|t| t.player.eid);
    if m.target.is_none() && m.angry > 0 {
        m.angry = 0;
    }

    if let Some(t) = target {
        let d = dist2(&t.pos, &m.pos).sqrt();
        let dy = (t.pos[1] - m.pos[1]).abs();
        let by = Attacker { eid: m.eid, name: kind.display().to_string(), pos: Some(m.pos) };
        face(m, t.pos);
        let reach = 1.6 + m.size().0 / 2.0;
        match kind {
            MobKind::Zombie | MobKind::Spider | MobKind::ZombifiedPiglin | MobKind::Piglin | MobKind::WitherSkeleton | MobKind::Hoglin | MobKind::Enderman | MobKind::Endermite => {
                let speed = if kind == MobKind::Enderman { 0.15 } else { kind.speed() * 1.25 };
                goal = Some((t.pos, speed));
                if d < reach && dy < 2.5 && m.attack_cooldown == 0 {
                    m.attack_cooldown = 20;
                    actions.push(Action::Hurt(t.player.clone(), difficulty_damage(server, kind.melee()), "mob_attack", by.clone()));
                    if kind == MobKind::WitherSkeleton {
                        actions.push(Action::Effect(t.player.clone(), "wither", 0, 200));
                    }
                    if kind == MobKind::Hoglin {
                        let dir = normalize([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]);
                        actions.push(Action::Knock(t.player.clone(), [dir[0] * 0.6, 0.6, dir[2] * 0.6]));
                    }
                }
                // An angry enderman far away closes in with a jump.
                if kind == MobKind::Enderman && d > 12.0 && entities::random() < 1.0 / 40.0 && teleport_random(world, m, t.pos, 8.0) {
                    actions.push(Action::Sound(m.eid, "entity.enderman.teleport".into(), sound::HOSTILE));
                }
            }
            MobKind::Skeleton => {
                if d > 10.0 {
                    goal = Some((t.pos, kind.speed()));
                }
                if d < 15.0 && m.attack_cooldown == 0 {
                    m.attack_cooldown = if server.config.difficulty >= 3 { 20 } else { 40 };
                    let from = [m.pos[0], m.pos[1] + 1.5, m.pos[2]];
                    let to = [t.pos[0], t.pos[1] + 1.1, t.pos[2]];
                    let dir = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
                    let horiz = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
                    let speed = 1.6;
                    let len = (dir[0] * dir[0] + (dir[1] + horiz * 0.2).powi(2) + dir[2] * dir[2]).sqrt().max(1e-6);
                    let spread = 0.05 * (4 - server.config.difficulty.min(3) as i32) as f64;
                    let vel = [
                        dir[0] / len * speed + (entities::random() as f64 - 0.5) * spread,
                        (dir[1] + horiz * 0.2) / len * speed,
                        dir[2] / len * speed + (entities::random() as f64 - 0.5) * spread,
                    ];
                    actions.push(Action::Shoot(m.dim, ProjectileKind::Arrow, m.eid, from, vel));
                }
            }
            MobKind::Creeper => {
                if d < 3.0 && m.fuse.is_none() {
                    m.fuse = Some(0);
                    actions.push(Action::Sound(m.eid, "entity.creeper.primed".into(), sound::HOSTILE));
                }
                if m.fuse.is_none() {
                    goal = Some((t.pos, kind.speed()));
                }
            }
            MobKind::Blaze => {
                // Hovers slightly above the player's height and fires bursts of 3.
                let hover = [t.pos[0], t.pos[1] + 2.0, t.pos[2]];
                if d > 8.0 {
                    goal = Some((hover, kind.speed()));
                }
                m.vel[1] += ((hover[1] - m.pos[1]) * 0.02).clamp(-0.04, 0.04);
                if d < 2.0 && m.attack_cooldown == 0 && dy < 2.0 {
                    m.attack_cooldown = 20;
                    actions.push(Action::Hurt(t.player.clone(), difficulty_damage(server, kind.melee()), "mob_attack", by));
                } else if d < 48.0 {
                    if m.burst == 0 && m.attack_cooldown == 0 {
                        m.burst = 3;
                        m.attack_cooldown = 60;
                    }
                    if m.burst > 0 && m.attack_cooldown % 6 == 0 && m.attack_cooldown <= 40 {
                        m.burst -= 1;
                        if m.burst == 0 {
                            m.attack_cooldown = 100;
                        }
                        let from = [m.pos[0], m.pos[1] + 1.4, m.pos[2]];
                        let spread = (d.sqrt() * 0.5) * 0.03;
                        let dir = normalize([
                            t.pos[0] - from[0] + (entities::random() as f64 - 0.5) * spread * 10.0,
                            t.pos[1] + 0.9 - from[1],
                            t.pos[2] - from[2] + (entities::random() as f64 - 0.5) * spread * 10.0,
                        ]);
                        actions.push(Action::Shoot(m.dim, ProjectileKind::SmallFireball, m.eid, from, [dir[0] * 0.9, dir[1] * 0.9, dir[2] * 0.9]));
                    }
                }
            }
            MobKind::Ghast => {
                // Charges for 20 ticks (warning at 10) and shoots a fireball.
                if m.attack_cooldown == 0 && d < 64.0 {
                    m.burst += 1;
                    if m.burst == 10 {
                        m.charging = true;
                        actions.push(Action::Sound(m.eid, "entity.ghast.warn".into(), sound::HOSTILE));
                    }
                    if m.burst >= 20 {
                        let from = [m.pos[0], m.pos[1] + 2.0, m.pos[2]];
                        let dir = normalize([t.pos[0] - from[0], t.pos[1] + 0.9 - from[1], t.pos[2] - from[2]]);
                        let from = [from[0] + dir[0] * 2.5, from[1], from[2] + dir[2] * 2.5];
                        actions.push(Action::Shoot(m.dim, ProjectileKind::Fireball, m.eid, from, [dir[0] * 0.8, dir[1] * 0.8, dir[2] * 0.8]));
                        m.burst = 0;
                        m.charging = false;
                        m.attack_cooldown = 40;
                    }
                }
            }
            MobKind::MagmaCube => {
                if d < reach && dy < 2.0 && m.attack_cooldown == 0 {
                    m.attack_cooldown = 20;
                    actions.push(Action::Hurt(t.player.clone(), difficulty_damage(server, (m.size + 2) as f32), "mob_attack", by));
                }
            }
            _ => {}
        }
        if let Some(f) = &mut m.fuse {
            let close = dist2(&t.pos, &m.pos) < 7.0 * 7.0;
            if !close {
                m.fuse = None;
            } else {
                *f += 1;
                if *f >= 30 {
                    m.health = 0.0;
                    m.death_ticks = Some(20); // disappears without a death animation
                    actions.push(Action::Explode(m.dim, m.center(), 3.0, Attacker { eid: m.eid, name: "Creeper".into(), pos: Some(m.center()) }, false));
                    return;
                }
            }
        }
    } else {
        m.fuse = None;
        if m.charging || m.burst != 0 {
            m.charging = false;
            m.burst = 0;
        }
    }

    // Magma cube: moves by hopping.
    if kind == MobKind::MagmaCube {
        if m.on_ground && m.attack_cooldown % 4 == 0 {
            let jump_delay = if target.is_some() { 10 } else { 30 };
            if m.age % jump_delay == 0 {
                let dir = match target {
                    Some(t) => normalize([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]),
                    None => {
                        let a = entities::random() as f64 * std::f64::consts::TAU;
                        [a.cos(), 0.0, a.sin()]
                    }
                };
                m.yaw = (-dir[0].atan2(dir[2])).to_degrees() as f32;
                m.vel = [dir[0] * 0.15 * m.size as f64, 0.42 + 0.1 * m.size as f64, dir[2] * 0.15 * m.size as f64];
                m.knockback_ticks = 1;
            }
        }
        return;
    }

    // Ghast without a target (or with one): flies slowly to random points.
    if kind == MobKind::Ghast {
        let reached = m.wander.is_some_and(|w| dist2(&w, &m.pos) < 4.0);
        if m.wander.is_none() || reached || entities::random() < 1.0 / 200.0 {
            m.wander = Some([
                m.pos[0] + (entities::random() as f64 - 0.5) * 32.0,
                (m.pos[1] + (entities::random() as f64 - 0.5) * 16.0).clamp(m.dim.min_y() as f64 + 8.0, m.dim.min_y() as f64 + 110.0),
                m.pos[2] + (entities::random() as f64 - 0.5) * 32.0,
            ]);
        }
        if let Some(w) = m.wander {
            let dir = normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
            for i in 0..3 {
                m.vel[i] += dir[i] * 0.01;
            }
            if target.is_none() {
                face(m, w);
            }
        }
        return;
    }

    // Animals, villagers and monsters without a target: wander or flee.
    if goal.is_none() {
        let speed = if m.panic > 0 { kind.speed() * 2.0 } else { kind.speed() };
        let reached = m.wander.is_some_and(|w| (w[0] - m.pos[0]).powi(2) + (w[2] - m.pos[2]).powi(2) < 1.0);
        if reached || (m.panic > 0 && m.panic % 30 == 0) {
            m.wander = None;
        }
        if m.wander.is_none() && (m.panic > 0 || entities::random() < 1.0 / 120.0) {
            let a = entities::random() as f64 * std::f64::consts::TAU;
            let r = 3.0 + entities::random() as f64 * 7.0;
            m.wander = Some([m.pos[0] + a.cos() * r, m.pos[1], m.pos[2] + a.sin() * r]);
        }
        if let Some(w) = m.wander {
            face(m, w);
            goal = Some((w, speed));
        }
        if kind == MobKind::Blaze {
            // Without a target it descends slowly.
            m.vel[1] -= 0.005;
        }
    }

    // Horizontal movement towards the target (except during knockback from a hit).
    if m.knockback_ticks > 0 {
        m.knockback_ticks -= 1;
    } else if let Some((g, speed)) = goal {
        let (dx, dz) = (g[0] - m.pos[0], g[2] - m.pos[2]);
        let len = (dx * dx + dz * dz).sqrt();
        if len > 0.1 {
            m.vel[0] = dx / len * speed;
            m.vel[2] = dz / len * speed;
        }
    } else {
        m.vel[0] *= 0.5;
        m.vel[2] *= 0.5;
    }
}

fn face(m: &mut Mob, target: [f64; 3]) {
    let (dx, dz) = (target[0] - m.pos[0], target[2] - m.pos[2]);
    if dx.abs() + dz.abs() > 1e-3 {
        m.yaw = (-dx.atan2(dz)).to_degrees() as f32;
    }
}

fn physics(world: &mut World, m: &mut Mob, actions: &mut Vec<Action>) {
    let wet = in_water(world, m.pos);
    let lava = in_lava(world, m.pos);
    if m.kind.flies() {
        for v in &mut m.vel {
            *v *= 0.91;
        }
        m.fall = 0.0;
    } else if wet || (lava && m.kind == MobKind::Strider) {
        // They float towards the surface (the strider, in lava).
        m.vel[1] = (m.vel[1] + 0.04).min(0.1);
        m.fall = 0.0;
    } else if lava {
        m.vel[1] = (m.vel[1] - 0.02).max(-0.1);
        m.fall = 0.0;
    } else {
        m.vel[1] -= 0.08;
    }
    let want = m.vel;
    let d = move_aabb(world, &m.aabb(), m.vel);
    for i in 0..3 {
        m.pos[i] += d[i];
    }
    let landed = want[1] < 0.0 && d[1] > want[1] + 1e-9;
    let blocked = (d[0] - want[0]).abs() > 1e-6 || (d[2] - want[2]).abs() > 1e-6;
    if (d[1] - want[1]).abs() > 1e-9 {
        m.vel[1] = 0.0;
    }
    if !m.kind.flies() {
        m.vel[1] *= 0.98;
    }
    // Mob fall damage.
    if landed {
        if m.fall > 3.0 && !wet && !matches!(m.kind, MobKind::MagmaCube) {
            m.health -= (m.fall - 3.0).ceil() as f32;
            m.hurt_time = 10;
            actions.push(Action::Sound(m.eid, "entity.generic.big_fall".into(), m.kind.sound_category()));
        }
        m.fall = 0.0;
    } else if d[1] < 0.0 {
        m.fall -= d[1];
    }
    m.on_ground = landed;
    if landed && m.kind == MobKind::MagmaCube {
        m.vel[0] *= 0.3;
        m.vel[2] *= 0.3;
    }
    // Step up one block; spiders climb walls.
    if blocked && !m.kind.flies() {
        if m.kind == MobKind::Spider {
            m.vel[1] = 0.2;
        } else if m.on_ground {
            m.vel[1] = 0.42;
        }
    }
    if m.knockback_ticks > 0 && m.kind != MobKind::MagmaCube {
        let f = if m.on_ground { 0.6 } else { 0.91 };
        m.vel[0] *= f;
        m.vel[2] *= f;
    }
}

// ---------- combat ----------

/// A player hits a mob (or a dragon part).
pub fn attacked_by_player(server: &Arc<Server>, player: &Arc<Player>, eid: i32) {
    // The dragon's parts have the 8 ids following its own; the head is the first.
    let (target, part) = {
        let mobs = server.mobs.lock().unwrap();
        match mobs.get(&eid) {
            Some(m) => (Some((m.eid, m.center(), m.kind)), 0),
            None => match mobs.values().find(|m| m.kind == MobKind::EnderDragon && eid > m.eid && eid <= m.eid + 8) {
                Some(m) => (Some((m.eid, m.center(), m.kind)), eid - m.eid),
                None => (None, 0),
            },
        }
    };
    let Some((eid, center, kind)) = target else {
        if !super::vehicles::attacked(server, player, eid) {
            super::projectiles::deflect(server, player, eid);
        }
        return;
    };
    if server.mobs.lock().unwrap().get(&eid).is_some_and(|m| m.death_ticks.is_some()) {
        return;
    }
    let eye = player.state.lock().unwrap().eye();
    let max = if kind == MobKind::EnderDragon { 14.0 } else { 6.0 };
    if dist2(&eye, &center) > max * max {
        return;
    }
    let Some(sw) = super::play::swing(server, player) else { return };
    let mut extra = 0.0;
    let mut fire = 0;
    if let Some(w) = &sw.weapon {
        fire = w.enchantment("fire_aspect");
        if kind.undead() {
            extra += 2.5 * w.enchantment("smite") as f32;
        }
        if matches!(kind, MobKind::Spider | MobKind::Endermite) {
            extra += 2.5 * w.enchantment("bane_of_arthropods") as f32;
        }
    }
    let mut dmg = sw.damage + extra;
    // Only the head takes full damage.
    if kind == MobKind::EnderDragon && part != 1 {
        dmg = dmg / 4.0 + dmg.min(1.0);
    }
    let (s, c) = (sw.yaw.to_radians() as f64).sin_cos();
    let knock = (kind != MobKind::EnderDragon && kind != MobKind::EndCrystal).then_some([-s * sw.knockback, 0.4, c * sw.knockback]);
    hurt(server, eid, dmg, "player_attack", Some(player.eid), knock, fire);
}

/// Pushes a mob in direction `dir` (shield blocking a hit).
pub fn push_away(server: &Server, eid: i32, dir: [f64; 3]) {
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        let l = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt().max(1e-6);
        m.vel = [dir[0] / l * 0.5, 0.3, dir[2] / l * 0.5];
        m.knockback_ticks = 10;
    }
}

/// Pulls a mob towards `to` (fishing rod).
pub fn pull(server: &Server, eid: i32, to: [f64; 3]) {
    if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
        m.vel = [(to[0] - m.pos[0]) * 0.1, (to[1] - m.pos[1]) * 0.1 + 0.2, (to[2] - m.pos[2]) * 0.1];
        m.knockback_ticks = 10;
    }
}

pub fn kind_of(server: &Server, eid: i32) -> Option<MobKind> {
    server.mobs.lock().unwrap().get(&eid).map(|m| m.kind)
}

/// Magic healing/damage from potions (inverted for undead).
pub fn magic(server: &Arc<Server>, eid: i32, amount: f32, heal: bool, by: i32) {
    let undead = server.mobs.lock().unwrap().get(&eid).is_some_and(|m| m.kind.undead());
    if heal != undead {
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
            m.health = (m.health + amount).min(m.max_health());
        }
    } else {
        hurt(server, eid, amount, "magic", Some(by), None, 0);
    }
}

/// Damages a mob (hits, arrows, explosions). `by` = causing entity.
pub fn hurt(server: &Arc<Server>, eid: i32, amount: f32, kind: &str, by: Option<i32>, knock: Option<[f64; 3]>, fire_aspect: i32) {
    let by_player = by.filter(|b| server.players.read().unwrap().contains_key(b));
    let mut alert: Option<(Dimension, [f64; 3], i32)> = None;
    let mut crystal: Option<(Dimension, [f64; 3])> = None;
    let mut teleported = false;
    let died = {
        let mut mobs = server.mobs.lock().unwrap();
        let Some(m) = mobs.get_mut(&eid) else { return };
        if m.death_ticks.is_some() || (m.hurt_time > 0 && kind == "player_attack") {
            return;
        }
        if m.kind.fire_immune() && matches!(kind, "in_fire" | "on_fire" | "lava" | "fireball") {
            return;
        }
        // The End crystal explodes on any hit.
        if m.kind == MobKind::EndCrystal {
            m.death_ticks = Some(20);
            m.health = 0.0;
            crystal = Some((m.dim, m.pos));
            None
        } else if m.kind == MobKind::EnderDragon && matches!(kind, "explosion" | "in_fire" | "on_fire") && by_player.is_none() {
            return;
        } else if m.kind == MobKind::Enderman && matches!(kind, "arrow" | "fireball" | "magic") {
            // Endermen dodge projectiles.
            let mut world = server.world(m.dim).lock().unwrap();
            teleported = teleport_random(&mut world, m, m.pos, 32.0);
            None
        } else {
            m.health -= amount;
            m.hurt_time = 10;
            if let Some(k) = knock {
                m.vel = k;
                m.knockback_ticks = 10;
            }
            if fire_aspect > 0 && !m.kind.fire_immune() {
                m.fire = m.fire.max(1);
            }
            match m.kind {
                MobKind::ZombifiedPiglin | MobKind::Enderman | MobKind::Piglin => {
                    if let Some(p) = by_player {
                        m.angry = 400 + (entities::random() * 400.0) as u32;
                        m.target = Some(p);
                        if m.kind == MobKind::ZombifiedPiglin {
                            alert = Some((m.dim, m.pos, p));
                        }
                    }
                }
                k if k.hostile() => {
                    if let Some(p) = by {
                        m.target = Some(p);
                    }
                }
                MobKind::EnderDragon => {}
                _ => {
                    m.panic = 100;
                    m.wander = None;
                }
            }
            let mut p = packet(id::play::DAMAGE_EVENT);
            let cause = by.map_or(0, |e| e + 1);
            p.varint(eid).varint(gameplay::get().damage_type(kind)).varint(cause).varint(cause).bool(false);
            server.broadcast_tracked(eid, &p, true);
            sound::entity(server, eid, &format!("entity.{}.hurt", m.kind.name()), m.kind.sound_category(), 1.0, 1.0);
            if m.kind == MobKind::EnderDragon {
                server.broadcast_tracked(eid, &metadata(m), true);
                if m.health <= 0.0 {
                    m.health = 0.0;
                    m.death_ticks = Some(0);
                    if let Some(d) = &mut m.dragon {
                        d.phase = super::dragon::phase::DYING;
                        d.killer = by_player;
                    }
                    server.broadcast_tracked(eid, &metadata(m), true);
                    sound::entity(server, eid, "entity.ender_dragon.death", sound::HOSTILE, 5.0, 1.0);
                }
                None
            } else if m.health <= 0.0 {
                m.death_ticks = Some(0);
                Some(m.clone())
            } else {
                None
            }
        }
    };
    if teleported {
        sound::entity(server, eid, "entity.enderman.teleport", sound::HOSTILE, 1.0, 1.0);
    }
    if let Some((dim, pos)) = crystal {
        let mut d = packet(id::play::ENTITY_DESTROY);
        d.varint(1).varint(eid);
        server.broadcast_tracked(eid, &d, true);
        explosion::explode(server, dim, [pos[0], pos[1], pos[2]], 6.0, by.map(|e| Attacker { eid: e, name: "Cristal del End".into(), pos: Some(pos) }));
    }
    // Nearby zombified piglins join the fight.
    if let Some((dim, pos, p)) = alert {
        let mut mobs = server.mobs.lock().unwrap();
        for o in mobs.values_mut().filter(|o| o.kind == MobKind::ZombifiedPiglin && o.dim == dim && o.death_ticks.is_none() && dist2(&o.pos, &pos) < 20.0 * 20.0) {
            o.angry = o.angry.max(400 + (entities::random() * 400.0) as u32);
            o.target = Some(p);
        }
    }
    if let Some(m) = died {
        on_death(server, &m, by);
    }
}

fn on_death(server: &Arc<Server>, m: &Mob, killer: Option<i32>) {
    if m.kind == MobKind::EnderDragon {
        super::dragon::died(server, m);
        return;
    }
    let mut status = packet(id::play::ENTITY_STATUS);
    status.i32(m.eid).i8(3);
    server.broadcast_tracked(m.eid, &status, true);
    let death = format!("entity.{}.death", m.kind.name());
    sound::entity(server, m.eid, &death, m.kind.sound_category(), 1.0, 1.0);
    let player = killer.and_then(|k| server.players.read().unwrap().get(&k).cloned());
    let looting = player
        .as_ref()
        .map(|p| {
            let st = p.state.lock().unwrap();
            st.inv.held(0, st.held_slot).as_ref().map_or(0, |w| w.enchantment("looting"))
        })
        .unwrap_or(0);
    let mut drops = gameplay::get().mob_drops(m.kind.name(), player.is_some(), looting, &mut entities::random);
    if m.kind == MobKind::MagmaCube {
        // There are no frogs: only magma cream, and only from large cubes.
        let cream = data::get().item_id("magma_cream");
        drops.retain(|d| Some(d.0) == cream && m.size > 1);
    }
    // Unsheared sheep drop wool.
    if m.kind == MobKind::Sheep && !m.sheared {
        if let Some(w) = data::get().item_id("white_wool") {
            drops.push((w, 1));
        }
    }
    // Whatever dies while burning drops cooked meat.
    for (item, count) in drops {
        let mut stack = ItemStack::new(item, count);
        if m.fire > 0 {
            if let Some((cooked, _)) = gameplay::smelt(stack.name()).filter(|_| gameplay::get().food(item).is_some()) {
                stack = ItemStack::new(data::get().item_id(&cooked).unwrap_or(item), count);
            }
        }
        entities::spawn(server, m.dim, m.center(), [(entities::random() as f64 - 0.5) * 0.2, 0.2, (entities::random() as f64 - 0.5) * 0.2], stack, 10);
    }
    // A magma cube splits into 2-4 smaller ones.
    if m.kind == MobKind::MagmaCube && m.size > 1 {
        for _ in 0..2 + (entities::random() * 3.0) as i32 {
            let off = [(entities::random() as f64 - 0.5) * m.size as f64 * 0.5, 0.5, (entities::random() as f64 - 0.5) * m.size as f64 * 0.5];
            let eid = spawn(server, m.dim, MobKind::MagmaCube, [m.pos[0] + off[0], m.pos[1] + off[1], m.pos[2] + off[2]]);
            set_size(server, eid, m.size / 2);
        }
    }
    if player.is_some() {
        let xp = match m.kind {
            MobKind::Villager => 0,
            MobKind::Blaze => 10,
            MobKind::MagmaCube => m.size,
            MobKind::Endermite => 3,
            k if k.hostile() => 5,
            _ => 1 + (entities::random() * 3.0) as i32,
        };
        if xp > 0 {
            entities::spawn_xp(server, m.dim, m.center(), xp);
        }
    }
}

/// Piglin barter: drops an item from the vanilla table.
fn barter(server: &Arc<Server>, dim: Dimension, pos: [f64; 3]) {
    let table = &super::stations::get().barter;
    let total: i32 = table.iter().map(|e| e.1).sum();
    let mut r = (entities::random() * total as f32) as i32;
    let Some(e) = table.iter().find(|e| {
        r -= e.1;
        r < 0
    }) else {
        return;
    };
    let reg = data::get();
    let count = e.2 + (entities::random() * (e.3 - e.2 + 1) as f32) as i32;
    let mut item = match &e.4 {
        Some(potion) => effects::potion_item(&e.0, potion),
        None => reg.item_id(&e.0).map(|i| ItemStack::new(i, 1)),
    };
    if let (Some(it), Some(ench)) = (item.as_mut(), &e.5) {
        let lvl = 1 + (entities::random() * 3.0) as i32;
        if it.name() == "book" {
            if let Some(b) = reg.item_id("enchanted_book") {
                *it = ItemStack::new(b, 1);
            }
        }
        it.enchant(ench, lvl);
    }
    if let Some(it) = item {
        let n = count.clamp(1, it.max_stack() as i32) as i8;
        tracing::debug!("trueque de piglin: {} x{n}", it.name());
        entities::spawn(server, dim, pos, [(entities::random() as f64 - 0.5) * 0.2, 0.25, (entities::random() as f64 - 0.5) * 0.2], it.with_count(n), 20);
    }
}

/// Right click on a mob: milk cows, shear sheep, trade, give gold to a piglin.
pub fn interact(server: &Arc<Server>, player: &Arc<Player>, eid: i32, hand: i32) {
    if super::vehicles::interact(server, player, eid) {
        return;
    }
    let reg = data::get();
    let Some(kind) = server.mobs.lock().unwrap().get(&eid).filter(|m| m.death_ticks.is_none()).map(|m| m.kind) else { return };
    let held = {
        let st = player.state.lock().unwrap();
        st.inv.held(hand, st.held_slot).clone()
    };
    if kind == MobKind::Villager {
        super::villagers::open_trades(server, player, eid);
        return;
    }
    let Some(held) = held else { return };
    match (kind, held.name()) {
        (MobKind::Cow, "bucket") => {
            let milk = reg.item_id("milk_bucket").map(|m| ItemStack::new(m, 1));
            let mut st = player.state.lock().unwrap();
            if st.game_mode != 1 {
                let h = st.held_slot;
                st.inv.consume_held(hand, h, 1, milk);
                let p = st.inv.window_items();
                player.send(p);
            }
            drop(st);
            sound::entity(server, eid, "entity.cow.milk", sound::PLAYER, 1.0, 1.0);
        }
        (MobKind::Sheep, "shears") => {
            let pos = {
                let mut mobs = server.mobs.lock().unwrap();
                let Some(m) = mobs.get_mut(&eid).filter(|m| !m.sheared) else { return };
                m.sheared = true;
                server.broadcast_tracked(m.eid, &metadata(m), true);
                m.center()
            };
            if let Some(w) = reg.item_id("white_wool") {
                let n = 1 + (entities::random() * 3.0) as i8;
                entities::spawn(server, player.dim(), pos, [0.0, 0.2, 0.0], ItemStack::new(w, n), 10);
            }
            sound::entity(server, eid, "entity.sheep.shear", sound::PLAYER, 1.0, 1.0);
            health::wear_held(server, player, 1);
        }
        (MobKind::Piglin, "gold_ingot") => {
            {
                let mut mobs = server.mobs.lock().unwrap();
                let Some(m) = mobs.get_mut(&eid).filter(|m| m.admire == 0 && m.angry == 0) else { return };
                m.admire = 120;
                m.target = None;
                server.broadcast_tracked(m.eid, &equipment(m.eid, 1, "gold_ingot"), true);
            }
            let mut st = player.state.lock().unwrap();
            if st.game_mode != 1 {
                let h = st.held_slot;
                let slot = st.inv.consume_held(hand, h, 1, None);
                st.sync_slot(player, slot);
            }
            drop(st);
            sound::entity(server, eid, "entity.piglin.admiring_item", sound::HOSTILE, 1.0, 1.0);
        }
        _ => {}
    }
}

// ---------- natural spawning ----------

/// Ground (solid block with `room` air above it) below `y` in column (x, z).
fn spawn_spot(world: &mut World, x: i32, z: i32, from_y: i32, room: i32) -> Option<[f64; 3]> {
    let reg = data::get();
    let g = gameplay::get();
    let dim = world.dim;
    for y in (from_y - 24..from_y + 16).rev() {
        if !dim.in_bounds(y) || !dim.in_bounds(y + room) {
            continue;
        }
        let floor = world.get_block(BlockPos::new(x, y, z));
        let fb = reg.block_of_state(floor);
        let solid = g.shape(floor).len() == 1 && g.shape(floor)[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0] && !fb.name.ends_with("leaves") && !fb.name.contains("glass") && fb.name != "bedrock";
        if solid && (1..=room).all(|dy| reg.is_air(world.get_block(BlockPos::new(x, y + dy, z)))) {
            return Some([x as f64 + 0.5, (y + 1) as f64, z as f64 + 0.5]);
        }
    }
    None
}

/// Lava surface with air above (for striders).
fn lava_spot(world: &mut World, x: i32, z: i32, from_y: i32) -> Option<[f64; 3]> {
    let reg = data::get();
    for y in (from_y - 24..from_y + 16).rev() {
        if reg.block_of_state(world.get_block(BlockPos::new(x, y, z))).name == "lava" && reg.is_air(world.get_block(BlockPos::new(x, y + 1, z))) && reg.is_air(world.get_block(BlockPos::new(x, y + 2, z))) {
            return Some([x as f64 + 0.5, y as f64 + 0.9, z as f64 + 0.5]);
        }
    }
    None
}

fn pick<T: Copy>(table: &[(T, u32)]) -> T {
    let total: u32 = table.iter().map(|t| t.1).sum();
    let mut r = (entities::random() * total as f32) as u32;
    for &(t, w) in table {
        if r < w {
            return t;
        }
        r -= w;
    }
    table[0].0
}

fn natural_spawn(server: &Arc<Server>, seen: &[Seen], age: i64) {
    let night = !is_day(server);
    let counts: Vec<(usize, usize)> = {
        let mobs = server.mobs.lock().unwrap();
        seen.iter()
            .map(|s| {
                let near = mobs.values().filter(|m| m.dim == s.dim && dist2(&m.pos, &s.pos) < 128.0 * 128.0);
                let (h, p): (Vec<_>, Vec<_>) = near.partition(|m| m.kind.hostile());
                (h.len(), p.iter().filter(|m| m.kind != MobKind::Villager && m.kind != MobKind::EndCrystal).count())
            })
            .collect()
    };
    let monsters = server.config.spawn_monsters && server.config.difficulty > 0;
    let mut to_spawn: Vec<(Dimension, MobKind, [f64; 3])> = Vec::new();
    for (s, &(hostile, passive)) in seen.iter().zip(&counts) {
        let mut world = server.world(s.dim).lock().unwrap();
        let a = entities::random() as f64 * std::f64::consts::TAU;
        let r = 24.0 + entities::random() as f64 * 40.0;
        let (x, z) = ((s.pos[0] + a.cos() * r).floor() as i32, (s.pos[2] + a.sin() * r).floor() as i32);
        match s.dim {
            Dimension::Overworld => {
                let Some(spot) = spawn_spot(&mut world, x, z, s.pos[1] as i32, 2) else { continue };
                let below = data::get().block_of_state(world.get_block(BlockPos::new(x, spot[1] as i32 - 1, z))).name.clone();
                // Monsters: at night, under open sky.
                if monsters && night && hostile < HOSTILE_CAP && sky_exposed(&mut world, spot) {
                    let kind = pick(&[(MobKind::Zombie, 95), (MobKind::Skeleton, 100), (MobKind::Creeper, 100), (MobKind::Spider, 100), (MobKind::Enderman, 10)]);
                    if kind != MobKind::Enderman || spawn_spot(&mut world, x, z, spot[1] as i32, 3).is_some() {
                        to_spawn.push((s.dim, kind, spot));
                    }
                }
                // Animals: on grass, every so often, in small groups.
                if server.config.spawn_animals && age % 200 == 0 && passive < PASSIVE_CAP && below == "grass_block" {
                    let kind = [MobKind::Cow, MobKind::Pig, MobKind::Sheep, MobKind::Chicken][(entities::random() * 4.0) as usize % 4];
                    for i in 0..2 + (entities::random() * 2.0) as i32 {
                        to_spawn.push((s.dim, kind, [spot[0] + i as f64 * 0.7, spot[1], spot[2]]));
                    }
                }
            }
            Dimension::Nether if monsters && hostile < HOSTILE_CAP => {
                // Striders on lava; the rest depending on the ground.
                if entities::random() < 0.15 {
                    if let Some(spot) = lava_spot(&mut world, x, z, s.pos[1] as i32) {
                        to_spawn.push((s.dim, MobKind::Strider, spot));
                        continue;
                    }
                }
                let Some(spot) = spawn_spot(&mut world, x, z, s.pos[1] as i32, 2) else { continue };
                let below = data::get().block_of_state(world.get_block(BlockPos::new(x, spot[1] as i32 - 1, z))).name.clone();
                let kind = if below.starts_with("soul_s") {
                    pick(&[(MobKind::Skeleton, 20), (MobKind::Ghast, 50), (MobKind::Blaze, 10), (MobKind::Enderman, 1)])
                } else {
                    // Without fortresses, blazes and wither skeletons spawn (sparsely) across the whole Nether.
                    pick(&[
                        (MobKind::ZombifiedPiglin, 100),
                        (MobKind::Piglin, 15),
                        (MobKind::MagmaCube, 4),
                        (MobKind::Ghast, 30),
                        (MobKind::Hoglin, 9),
                        (MobKind::Blaze, 8),
                        (MobKind::WitherSkeleton, 4),
                        (MobKind::Enderman, 1),
                    ])
                };
                // The ghast needs a lot of room and spawns floating.
                let spot = if kind == MobKind::Ghast {
                    let air = (1..=6).all(|dy| data::get().is_air(world.get_block(BlockPos::new(x, spot[1] as i32 + dy, z))));
                    if !air {
                        continue;
                    }
                    [spot[0], spot[1] + 2.0, spot[2]]
                } else {
                    spot
                };
                let group = if kind == MobKind::ZombifiedPiglin { 2 + (entities::random() * 3.0) as i32 } else { 1 };
                for i in 0..group {
                    to_spawn.push((s.dim, kind, [spot[0] + i as f64 * 0.8, spot[1], spot[2]]));
                }
            }
            Dimension::End if monsters && hostile < HOSTILE_CAP => {
                let Some(spot) = spawn_spot(&mut world, x, z, s.pos[1] as i32, 3) else { continue };
                to_spawn.push((s.dim, MobKind::Enderman, spot));
            }
            _ => {}
        }
    }
    for (dim, kind, pos) in to_spawn {
        spawn(server, dim, kind, pos);
    }
    super::villagers::populate_villages(server, seen);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_exist_in_registry() {
        for k in ALL {
            assert!(data::get().entity(k.name()) > 0, "{}", k.name());
            assert!(MobKind::from_name(k.name()) == Some(k));
        }
    }

    #[test]
    fn weighted_pick_covers_table() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            seen.insert(pick(&[(1, 10), (2, 1), (3, 5)]));
        }
        assert_eq!(seen.len(), 3);
    }
}

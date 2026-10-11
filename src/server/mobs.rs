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
use std::sync::atomic::{AtomicU32, Ordering};

/// Path searches left in this tick (they are the expensive part of the AI; the rest wait a tick).
static PATH_BUDGET: AtomicU32 = AtomicU32::new(0);
const PATHS_PER_TICK: u32 = 2;
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
    Pillager,
    Wolf,
}

pub const ALL: [MobKind; 23] = [
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
    MobKind::Pillager,
    MobKind::Wolf,
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
            MobKind::Pillager => "pillager",
            MobKind::Wolf => "wolf",
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
            MobKind::Pillager => "Saqueador",
            MobKind::Wolf => "Lobo",
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
                | MobKind::Pillager
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
            MobKind::Zombie | MobKind::Villager | MobKind::ZombifiedPiglin | MobKind::Piglin | MobKind::Pillager => (0.6, 1.95),
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
            MobKind::Wolf => (0.6, 0.85),
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
            MobKind::Pillager => 24.0,
            MobKind::Wolf => 8.0,
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
            MobKind::Pillager => 0.12,
            MobKind::Wolf => 0.1,
            MobKind::Villager | MobKind::Strider | MobKind::Ghast => 0.05,
            _ => 0.07,
        }
    }

    pub fn walk_speed(self) -> f64 {
        self.speed()
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
            MobKind::Wolf => 4.0,
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
    /// Walking around an obstacle it cannot jump: horizontal direction and ticks left.
    pub detour: Option<([f64; 2], u32)>,
    /// Cells left on the way to the target (A*), the target cell it was computed for and
    /// ticks until it may be recomputed.
    pub path: std::collections::VecDeque<BlockPos>,
    pub path_goal: Option<BlockPos>,
    pub path_wait: u32,
    /// Closest it has got to the next waypoint, and ticks without getting closer.
    pub path_best: f64,
    pub path_stuck: u32,
    /// Skeleton: ticks drawing the bow; side it circles around the player (+1/-1).
    pub drawing: u32,
    pub strafe: f64,
    /// Undead: whether it is under the open sky (refreshed every few ticks) and the shaded cell
    /// it is heading to before the sun comes up.
    pub exposed: bool,
    pub shade: Option<BlockPos>,
    /// Pillager patrol it belongs to (0: none) and whether it is its captain.
    pub patrol: u32,
    pub captain: bool,
    /// Creeper struck by lightning (bigger explosion).
    pub charged: bool,
    /// Skeleton on hard: where the player was last tick, how fast it is closing in, and the
    /// cover / peek spots it is using.
    pub last_target: Option<[f64; 3]>,
    pub approach: f64,
    pub cover: Option<(BlockPos, BlockPos)>,
    /// Animals: age like vanilla's `Age` (negative: baby; positive: breeding cooldown), ticks
    /// in love and the player who fed it.
    pub grow: i32,
    pub love: u32,
    pub love_by: Option<i32>,
    /// Tamed wolf: owner's UUID, sitting, and the mob it is attacking for its owner.
    pub owner: Option<u128>,
    pub sitting: bool,
    pub prey: Option<i32>,
}

impl Mob {
    pub fn size(&self) -> (f64, f64) {
        let (w, h) = self.kind.base_size();
        if self.grow < 0 {
            return (w * 0.5, h * 0.5);
        }
        if self.kind == MobKind::MagmaCube {
            (w * self.size as f64 * 2.0 * 0.5 + 0.0, h * self.size as f64 * 2.0 * 0.5)
        } else {
            (w, h)
        }
    }

    pub fn max_health(&self) -> f32 {
        match self.kind {
            MobKind::MagmaCube => (self.size * self.size) as f32,
            MobKind::Wolf if self.owner.is_some() => 20.0,
            k => k.base_health(),
        }
    }

    fn aabb(&self) -> Aabb {
        let (w, h) = self.size();
        Aabb::entity(self.pos[0], self.pos[1], self.pos[2], w, h)
    }

    /// Drawing the bow or loading the crossbow (arm raised).
    fn charging(&self) -> bool {
        self.drawing > 0 && (self.kind != MobKind::Pillager || self.drawing < 25)
    }

    pub fn center(&self) -> [f64; 3] {
        [self.pos[0], self.pos[1] + self.size().1 / 2.0, self.pos[2]]
    }

}

// ---------- packets ----------

fn equipment(eid: i32, slot: u8, item: &str) -> Vec<u8> {
    equipment_stack(eid, slot, data::get().item_id(item).map(|b| ItemStack::new(b, 1)))
}

pub(super) fn equipment_stack(eid: i32, slot: u8, stack: Option<ItemStack>) -> Vec<u8> {
    let mut e = packet(id::play::ENTITY_EQUIPMENT);
    e.varint(eid).u8(slot);
    crate::item::write_slot(&mut e, &stack);
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
        MobKind::Pillager => {
            out.push(equipment(m.eid, 0, "crossbow"));
            if m.captain {
                out.push(equipment_stack(m.eid, 5, super::pillagers::ominous_banner()));
            }
        }
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
        // 8: hand states (drawing the bow).
        p.u8(8).varint(0).i8(if m.charging() { 0x01 } else { 0 });
        // 16: baby (animals).
        if super::animals::breeds(m.kind) {
            p.u8(16).varint(8).bool(m.grow < 0);
        }
    }
    match m.kind {
        MobKind::Creeper => {
            p.u8(16).varint(1).varint(if m.fuse.is_some() { 1 } else { -1 });
            // 17: charged.
            p.u8(17).varint(8).bool(m.charged);
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
        MobKind::Pillager => {
            // 17: loading the crossbow.
            p.u8(17).varint(8).bool(m.charging());
        }
        MobKind::Wolf => {
            // 17: sitting / tamed, 18: owner, 20: collar colour (red), 21: anger.
            p.u8(17).varint(0).i8(if m.sitting { 0x01 } else { 0 } | if m.owner.is_some() { 0x04 } else { 0 });
            p.u8(18).varint(13);
            match m.owner {
                Some(u) => {
                    p.bool(true).uuid(u);
                }
                None => {
                    p.bool(false);
                }
            }
            p.u8(20).varint(1).varint(14);
            p.u8(21).varint(1).varint(if m.angry > 0 { m.angry as i32 } else { 0 });
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
        detour: None,
        path: Default::default(),
        path_goal: None,
        path_wait: 0,
        path_best: f64::MAX,
        path_stuck: 0,
        drawing: 0,
        strafe: 1.0,
        exposed: false,
        shade: None,
        patrol: 0,
        captain: false,
        charged: false,
        last_target: None,
        approach: 0.0,
        cover: None,
        grow: 0,
        love: 0,
        love_by: None,
        owner: None,
        sitting: false,
        prey: None,
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
    #[serde(default, skip_serializing_if = "is_zero")]
    patrol: u32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    captain: bool,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    grow: i32,
    /// Tamed wolf's owner (UUID in hex).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    sitting: bool,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

fn is_zero(v: &u32) -> bool {
    *v == 0
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
            patrol: m.patrol,
            captain: m.captain,
            grow: m.grow,
            owner: m.owner.map(|u| format!("{u:032x}")),
            sitting: m.sitting,
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
            m.patrol = s.patrol;
            m.captain = s.captain;
            m.grow = s.grow;
            m.owner = s.owner.as_deref().and_then(|u| u128::from_str_radix(u, 16).ok());
            m.sitting = s.sitting;
            if m.owner.is_some() {
                m.health = s.health.clamp(0.5, m.max_health());
            }
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
    /// Holds a bow or crossbow; carries a shield; has the shield raised.
    pub archer: bool,
    pub shield: bool,
    pub shield_up: bool,
    /// Items in the main and off hand.
    pub hands: [&'static str; 2],
    /// Alive and not a spectator (creative players count: animals follow them).
    pub present: bool,
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
    /// Two animals have a baby: (dimension, kind, where, parents, player who fed them, owner).
    Breed(Dimension, MobKind, [f64; 3], (i32, i32), Option<i32>, Option<u128>),
    /// A mob bites another mob: (target, damage, attacker).
    HurtMob(i32, f32, i32),
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
            let holds = |f: &dyn Fn(&str) -> bool| (0..2).any(|h| s.inv.held(h, s.held_slot).as_ref().is_some_and(|it| f(it.name())));
            let archer = holds(&|n| n == "bow" || n == "crossbow");
            let shield = holds(&|n| n == "shield");
            let shield_up = s.using.as_ref().is_some_and(|u| matches!(u.kind, super::ranged::UseKind::Shield));
            let hands = [0, 1].map(|h| s.inv.held(h, s.held_slot).as_ref().map_or("", |it| it.name()));
            let seen = Seen {
                dim: p.dim(),
                player: p.clone(),
                pos: [s.x, s.y, s.z],
                eye: s.eye(),
                look: [-yaw.sin() * pitch.cos(), -pitch.sin(), yaw.cos() * pitch.cos()],
                targetable: !s.dead && s.survival_like(),
                gold_armor: gold,
                pumpkin,
                archer,
                shield,
                shield_up,
                hands,
                present: !s.dead && s.game_mode != 3,
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
    if age % 1200 == 600 {
        super::pillagers::natural_patrols(server, &seen);
    }
    super::dragon::tick_fight(server, &seen, age);

    PATH_BUDGET.store(PATHS_PER_TICK, Ordering::Relaxed);
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
            let groups = super::pillagers::groups(mobs.values().filter(|m| m.dim == dim));
            let others = super::animals::snapshot(mobs.values().filter(|m| m.dim == dim));
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
                let look = |m: &Mob| (m.target.is_some(), m.fire > 0, m.fuse.is_some(), m.angry > 0, m.burst > 0, m.charging, m.dragon.as_ref().map(|d| d.phase), m.charging());
                let before = look(m);
                let health_before = m.health;
                if m.kind == MobKind::EnderDragon {
                    super::dragon::ai(server, &mut world, m, &seen, &crystals, &mut actions);
                } else {
                    ai(server, &mut world, m, &seen, &groups, &others, day, &mut actions);
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
                // Whoever hurts a player gets its wolves on it.
                if !server.players.read().unwrap().contains_key(&by.eid) {
                    super::animals::sic(server, p.uuid, by.eid);
                }
                health::damage(server, &p, amount, kind, Some(by));
            }
            Action::Breed(dim, kind, at, parents, by, owner) => super::animals::breed(server, dim, kind, at, parents, by, owner),
            Action::HurtMob(target, amount, by) => hurt(server, target, amount, "mob_attack", Some(by), None, 0),
            Action::Explode(dim, pos, power, by, fire) => explosion::explode_ex(server, dim, pos, power, Some(by), fire),
            Action::Shoot(dim, kind, shooter, from, vel) => {
                let crossbow = kind == ProjectileKind::Arrow && kind_of(server, shooter) == Some(MobKind::Pillager);
                let snd = match kind {
                    ProjectileKind::Arrow if crossbow => "item.crossbow.shoot",
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

pub(super) fn normalize(v: [f64; 3]) -> [f64; 3] {
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

#[allow(clippy::too_many_arguments)]
fn ai(server: &Server, world: &mut World, m: &mut Mob, seen: &[Seen], groups: &std::collections::HashMap<u32, super::pillagers::Group>, others: &[super::animals::Other], day: bool, actions: &mut Vec<Action>) {
    let group = groups.get(&m.patrol).filter(|_| m.patrol != 0);
    m.attack_cooldown = m.attack_cooldown.saturating_sub(1);
    m.hurt_time = m.hurt_time.saturating_sub(1);
    m.panic = m.panic.saturating_sub(1);
    m.busy = m.busy.saturating_sub(1);
    let kind = m.kind;

    // Rain puts out burning mobs.
    if m.fire > 0 && m.age % 10 == 0 && super::weather::rain_at(server, world, m.dim, m.pos) {
        m.fire = 0;
    }
    // Zombies and skeletons burn in sunlight (not while it rains on them).
    if matches!(kind, MobKind::Zombie | MobKind::Skeleton) && day && m.dim == Dimension::Overworld && m.age % 20 == 0 {
        if sky_exposed(world, m.pos) && !in_water(world, m.pos) && !super::weather::rain_at(server, world, m.dim, m.pos) {
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
        // Zombies notice players from further away (vanilla: 35 blocks).
        let range = match kind {
            MobKind::Ghast => 64.0,
            MobKind::Blaze => 48.0,
            MobKind::Zombie => 32.0,
            _ => 16.0,
        };
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
    // A patrol fights together: whoever spots the player warns the others.
    let target = if kind == MobKind::Pillager { target.or_else(|| super::pillagers::shared_target(m, group, seen)) } else { target };
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
            MobKind::Zombie | MobKind::Spider | MobKind::ZombifiedPiglin | MobKind::Piglin | MobKind::WitherSkeleton | MobKind::Hoglin | MobKind::Enderman | MobKind::Endermite | MobKind::Wolf => {
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
                // Shoots only what it sees; otherwise (or too far) it walks to the player.
                // In range it keeps its distance and circles around while drawing the bow.
                // On hard it also backs off, flanks shields and takes cover from archers
                // (see `skeletons`).
                let eye = [m.pos[0], m.pos[1] + 1.74, m.pos[2]];
                let sees = d < 16.0 && line_of_sight(world, eye, t.eye);
                let hard = server.config.difficulty >= 3;
                // Full draw: 20 ticks on hard, 30 otherwise.
                let draw = if hard { 20 } else { 30 };
                let tactic = if hard && d < 20.0 { super::skeletons::tactics(world, m, t, d, sees, m.drawing >= draw) } else { None };
                if let Some(g) = tactic {
                    goal = Some(g.unwrap_or((m.pos, 0.0)));
                } else if !sees || d > 12.0 {
                    goal = Some((t.pos, kind.speed() * 1.25));
                } else {
                    if entities::random() < 0.05 {
                        m.strafe = -m.strafe;
                    }
                    let to = normalize([t.pos[0] - m.pos[0], 0.0, t.pos[2] - m.pos[2]]);
                    let back = if d < 6.0 { -0.7 } else if d > 10.0 { 0.5 } else { 0.0 };
                    let dir = [to[0] * back - to[2] * m.strafe, 0.0, to[2] * back + to[0] * m.strafe];
                    goal = Some(([m.pos[0] + dir[0] * 2.0, m.pos[1], m.pos[2] + dir[2] * 2.0], kind.speed() * 0.6));
                }
                // In cover it draws the bow without seeing the player, and holds it drawn.
                let hidden = hard && m.cover.is_some();
                if !sees && !hidden {
                    m.drawing = 0;
                } else if m.drawing == 0 && m.attack_cooldown == 0 {
                    m.drawing = 1;
                } else if m.drawing > 0 && m.drawing < draw {
                    m.drawing += 1;
                }
                if sees && m.drawing >= draw {
                    face(m, t.pos);
                }
                // Never into a raised shield.
                let shielded = t.shield_up && super::skeletons::shield_covers(t, m.pos);
                if m.drawing >= draw && sees && !shielded {
                    m.drawing = 0;
                    m.attack_cooldown = if server.config.difficulty >= 3 { 10 } else { 25 };
                    let from = [m.pos[0], m.pos[1] + 1.5, m.pos[2]];
                    // Vanilla aims at a third of the target's height; the arc compensation does the rest.
                    let to = [t.pos[0], t.pos[1] + 0.6, t.pos[2]];
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
            MobKind::Pillager => {
                goal = super::pillagers::combat(server, world, m, t, group, actions);
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
                    actions.push(Action::Explode(m.dim, m.center(), if m.charged { 6.0 } else { 3.0 }, Attacker { eid: m.eid, name: "Creeper".into(), pos: Some(m.center()) }, false));
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

    // Walking towards a player: along an A* path, so they go around walls and up steps
    // instead of pushing in a straight line.
    if let (Some(t), Some((g, speed))) = (target, goal) {
        if g == t.pos && !kind.flies() {
            goal = Some((follow_path(world, m, t.pos), speed));
        }
    } else if kind != MobKind::Pillager {
        m.path.clear();
        m.path_goal = None;
    }

    // Undead that burn: shortly before sunrise and during the day they go for the shade (under
    // trees, roofs, caves) and do not leave it; from there they still attack whoever comes close
    // and skeletons keep shooting.
    if matches!(kind, MobKind::Zombie | MobKind::Skeleton) && m.dim == Dimension::Overworld && sun_soon(server) && !in_water(world, m.pos) {
        if m.age % 10 == 0 || m.shade.is_some() {
            m.exposed = sky_exposed(world, m.pos);
        }
        if m.exposed {
            let stale = m.shade.is_none_or(|c| sky_exposed(world, [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5]));
            if stale && m.age % 20 == m.eid.unsigned_abs() % 20 {
                m.shade = find_shade(world, m);
            }
            if let Some(c) = m.shade {
                let dest = [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5];
                let next = follow_path(world, m, dest);
                goal = Some((next, kind.speed() * 1.4));
            }
        } else {
            m.shade = None;
            let into_sun = goal.is_some_and(|(g, _)| {
                let (dx, dz) = (g[0] - m.pos[0], g[2] - m.pos[2]);
                let len = (dx * dx + dz * dz).sqrt().max(1e-6);
                sky_exposed(world, [m.pos[0] + dx / len * 0.8, g[1].max(m.pos[1]), m.pos[2] + dz / len * 0.8])
            });
            if into_sun || goal.is_none() {
                m.vel[0] = 0.0;
                m.vel[2] = 0.0;
                return;
            }
        }
    } else {
        m.shade = None;
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

    // Patrol on the move (or waiting in formation).
    if kind == MobKind::Pillager && target.is_none() {
        goal = super::pillagers::patrol(world, m, group);
    }
    // Animals: love, babies growing up, following food; tamed wolves.
    if super::animals::breeds(kind) {
        let g = super::animals::behave(world, m, seen, others, actions);
        if goal.is_none() {
            goal = g;
        }
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
    } else if let (Some((_, speed)), Some((dir, left))) = (goal, m.detour) {
        // Going around a wall: walk along it for a while, then try the goal again.
        m.vel[0] = dir[0] * speed;
        m.vel[2] = dir[1] * speed;
        face(m, [m.pos[0] + dir[0], m.pos[1], m.pos[2] + dir[1]]);
        m.detour = (left > 1).then_some((dir, left - 1));
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

/// Nothing solid between two points (sampled every quarter block).
pub(super) fn line_of_sight(world: &mut World, from: [f64; 3], to: [f64; 3]) -> bool {
    let g = crate::gameplay::get();
    let d = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let steps = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() * 4.0).ceil().max(1.0) as i32;
    let mut last = None;
    for i in 1..steps {
        let f = i as f64 / steps as f64;
        let p = [from[0] + d[0] * f, from[1] + d[1] * f, from[2] + d[2] * f];
        let c = cell(p);
        if last == Some(c) {
            continue;
        }
        last = Some(c);
        let (lx, ly, lz) = (p[0] - c.x as f64, p[1] - c.y as f64, p[2] - c.z as f64);
        let state = world.get_block(c);
        if g.shape(state).iter().any(|s| lx >= s[0] && lx <= s[3] && ly >= s[1] && ly <= s[4] && lz >= s[2] && lz <= s[5]) {
            return false;
        }
    }
    true
}

/// Day, or less than a minute before sunrise.
fn sun_soon(server: &Server) -> bool {
    let t = server.day_time.load(Ordering::Relaxed).rem_euclid(24000);
    !(12542..22300).contains(&t)
}

/// Closest shaded cell it can stand in, among random ones within 12 blocks.
fn find_shade(world: &mut World, m: &Mob) -> Option<BlockPos> {
    let (w, h) = m.size();
    let base = cell(m.pos);
    let mut best: Option<(f64, BlockPos)> = None;
    for _ in 0..24 {
        let (dx, dz) = (((entities::random() - 0.5) * 24.0) as i32, ((entities::random() - 0.5) * 24.0) as i32);
        for dy in [0, 1, -1, 2, -2, 3, -3, 4, -4] {
            let c = BlockPos::new(base.x + dx, base.y + dy, base.z + dz);
            if !crate::world::pathfind::standable(world, c, w, h) {
                continue;
            }
            let p = [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5];
            if !sky_exposed(world, p) {
                let d = dist2(&p, &m.pos);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, c));
                }
            }
            break;
        }
    }
    best.map(|b| b.1)
}

fn cell(p: [f64; 3]) -> BlockPos {
    BlockPos::new(p[0].floor() as i32, (p[1] + 0.01).floor() as i32, p[2].floor() as i32)
}

/// Next point to walk to on the way to `target`, (re)computing the path when needed.
pub(super) fn follow_path(world: &mut World, m: &mut Mob, target: [f64; 3]) -> [f64; 3] {
    let goal = cell(target);
    m.path_wait = m.path_wait.saturating_sub(1);
    // Close and at the same height: straight at it.
    if dist2(&m.pos, &target) < 2.0 * 2.0 && (target[1] - m.pos[1]).abs() < 1.0 {
        m.path.clear();
        return target;
    }
    // Waypoints already reached (center of the cell, at its height or above).
    while let Some(&c) = m.path.front() {
        let (dx, dz) = (c.x as f64 + 0.5 - m.pos[0], c.z as f64 + 0.5 - m.pos[2]);
        if dx * dx + dz * dz < 0.4 * 0.4 && m.pos[1] > c.y as f64 - 0.5 {
            m.path.pop_front();
            m.path_best = f64::MAX;
        } else {
            break;
        }
    }
    // Shortcut: on flat ground, head straight for a later waypoint if the way there is clear
    // (instead of zig-zagging from cell centre to cell centre).
    if m.age % 5 == 0 {
        let (w, h) = m.size();
        let here = cell(m.pos);
        while m.path.len() > 1 {
            let next = m.path[1];
            if next.y != here.y || !straight_walk(world, m.pos, next, w, h) {
                break;
            }
            m.path.pop_front();
            m.path_best = f64::MAX;
        }
    }
    // Stuck: no progress towards the next waypoint for 2 s, so plan again.
    if let Some(c) = m.path.front() {
        let d = dist2(&m.pos, &[c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5]);
        if d + 0.01 < m.path_best {
            m.path_best = d;
            m.path_stuck = 0;
        } else {
            m.path_stuck += 1;
            if m.path_stuck > 40 {
                m.path.clear();
                m.path_wait = 0;
                m.path_stuck = 0;
                m.path_best = f64::MAX;
            }
        }
    }
    let lost = m.path.front().is_some_and(|c| dist2(&m.pos, &[c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5]) > 3.0 * 3.0);
    let moved = m.path_goal.is_none_or(|g| (g.x - goal.x).abs() + (g.y - goal.y).abs() + (g.z - goal.z).abs() > 2);
    if m.path_wait == 0 && (m.path.is_empty() || lost || moved) && PATH_BUDGET.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |b| b.checked_sub(1)).is_ok() {
        let (w, h) = m.size();
        m.path = crate::world::pathfind::find(world, cell(m.pos), goal, w, h, 800).unwrap_or_default().into();
        m.path_goal = Some(goal);
        m.path_best = f64::MAX;
        // Unreachable target: do not search again on every tick.
        m.path_wait = if m.path.is_empty() { 40 } else { 10 };
    }
    match m.path.front() {
        Some(c) => {
            m.detour = None;
            let p = [c.x as f64 + 0.5, c.y as f64, c.z as f64 + 0.5];
            // Ladder or vine: the next step is above, so climb.
            if p[1] > m.pos[1] + 0.5 && crate::world::pathfind::climbable(block_name(world, m.pos, 0.2)) {
                m.vel[1] = 0.2;
            }
            face(m, p);
            p
        }
        None => target,
    }
}

/// Whether a mob can walk in a straight line from `from` to the cell `to` (same height):
/// every cell crossed is standable.
fn straight_walk(world: &mut World, from: [f64; 3], to: BlockPos, w: f64, h: f64) -> bool {
    let (tx, tz) = (to.x as f64 + 0.5, to.z as f64 + 0.5);
    let (dx, dz) = (tx - from[0], tz - from[2]);
    let steps = (dx.hypot(dz) * 3.0).ceil().max(1.0) as i32;
    let mut last = None;
    for i in 1..=steps {
        let f = i as f64 / steps as f64;
        // The mob's half-width each side of the line.
        for side in [-0.3, 0.3] {
            let (nx, nz) = (-dz, dx);
            let len = nx.hypot(nz).max(1e-6);
            let c = BlockPos::new((from[0] + dx * f + nx / len * side).floor() as i32, to.y, (from[2] + dz * f + nz / len * side).floor() as i32);
            if last == Some(c) {
                continue;
            }
            last = Some(c);
            if !crate::world::pathfind::standable(world, c, w, h) {
                return false;
            }
        }
    }
    true
}

pub(super) fn face(m: &mut Mob, target: [f64; 3]) {
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
    // Step up one block; spiders climb walls. Anything taller (a wall, a fence) cannot be
    // jumped: without this check the mob would hop against it forever, so it goes around it.
    if blocked && !m.kind.flies() {
        if m.kind == MobKind::Spider {
            m.vel[1] = 0.2;
        } else if m.on_ground {
            if can_jump_over(world, m, want) {
                // Gravity is applied before moving, so 0.5 gives vanilla's first 0.42 and a
                // peak of ~1.25 blocks (with 0.42 it stayed at 0.85 and never cleared a block).
                m.vel[1] = 0.5;
            } else if m.detour.is_none() {
                m.wander = None;
                m.detour = Some((along_wall(want, d), 20 + (entities::random() * 20.0) as u32));
            }
        }
    }
    if m.knockback_ticks > 0 && m.kind != MobKind::MagmaCube {
        let f = if m.on_ground { 0.6 } else { 0.91 };
        m.vel[0] *= f;
        m.vel[2] *= f;
    }
}

/// Whether, raised a jump's height (1.1 blocks: a block yes, a fence or a wall no), the mob could
/// keep moving horizontally.
fn can_jump_over(world: &mut World, m: &Mob, want: [f64; 3]) -> bool {
    let a = m.aabb();
    if move_aabb(world, &a, [0.0, 1.1, 0.0])[1] < 1.1 - 1e-6 {
        return false;
    }
    let h = move_aabb(world, &a.offset([0.0, 1.1, 0.0]), [want[0], 0.0, want[2]]);
    let low = move_aabb(world, &a, [want[0], 0.0, want[2]]);
    h[0].hypot(h[2]) > low[0].hypot(low[2]) + 1e-3
}

/// Direction along the obstacle: keeps the free axis; in a corner, goes back along one of them.
fn along_wall(want: [f64; 3], moved: [f64; 3]) -> [f64; 2] {
    let side = || if entities::random() < 0.5 { 1.0 } else { -1.0 };
    let sign = |v: f64| if v.abs() > 1e-3 { v.signum() } else { side() };
    let bx = (moved[0] - want[0]).abs() > 1e-6;
    let bz = (moved[2] - want[2]).abs() > 1e-6;
    match (bx, bz) {
        (true, false) => [0.0, sign(want[2])],
        (false, true) => [sign(want[0]), 0.0],
        _ if entities::random() < 0.5 => [-sign(want[0]), 0.0],
        _ => [0.0, -sign(want[2])],
    }
}

// ---------- combat ----------

/// A player hits a mob (or a dragon part).
/// A lightning bolt charges a creeper.
pub fn charge(server: &Server, eid: i32) {
    let meta = {
        let mut mobs = server.mobs.lock().unwrap();
        let Some(m) = mobs.get_mut(&eid) else { return };
        m.charged = true;
        metadata(m)
    };
    server.broadcast_tracked(eid, &meta, true);
}

/// Removes a mob without a death (it turns into something else).
pub fn remove(server: &Server, eid: i32) {
    server.mobs.lock().unwrap().remove(&eid);
}

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
    // The player's wolves join in.
    super::animals::sic(server, player.uuid, eid);
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
                MobKind::ZombifiedPiglin | MobKind::Enderman | MobKind::Piglin | MobKind::Pillager | MobKind::Wolf => {
                    // A tamed wolf does not turn on anyone; a wild one calls its pack.
                    if let Some(p) = by_player.filter(|_| m.owner.is_none()) {
                        m.angry = 400 + (entities::random() * 400.0) as u32;
                        m.target = Some(p);
                        if matches!(m.kind, MobKind::ZombifiedPiglin | MobKind::Wolf) {
                            alert = Some((m.dim, m.pos, p));
                        }
                    }
                    m.sitting = false;
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
        let pack = mobs.get(&eid).map_or(MobKind::ZombifiedPiglin, |m| m.kind);
        for o in mobs.values_mut().filter(|o| o.kind == pack && o.owner.is_none() && o.dim == dim && o.death_ticks.is_none() && dist2(&o.pos, &pos) < 20.0 * 20.0) {
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
    // Babies drop nothing.
    if m.grow < 0 {
        drops.clear();
    }
    if m.kind == MobKind::MagmaCube {
        // There are no frogs: only magma cream, and only from large cubes.
        let cream = data::get().item_id("magma_cream");
        drops.retain(|d| Some(d.0) == cream && m.size > 1);
    }
    // The patrol captain drops its banner and whoever killed it gets Bad Omen; the crossbow
    // sometimes.
    if m.kind == MobKind::Pillager {
        if m.captain {
            if let Some(b) = super::pillagers::ominous_banner() {
                entities::spawn(server, m.dim, m.center(), [0.0, 0.2, 0.0], b, 10);
            }
            if let Some(p) = &player {
                effects::add(server, p, "bad_omen", 0, 120000, 1.0);
            }
        }
        if player.is_some() && entities::random() < 0.085 + 0.01 * looting as f32 {
            if let Some(c) = data::get().item_id("crossbow") {
                drops.push((c, 1));
            }
        }
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
    if player.is_some() && m.grow >= 0 {
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
    // Feeding, breeding, taming and wolves sitting down.
    if super::animals::interact(server, player, eid, hand, held.as_ref().map(|h| h.name())) {
        return;
    }
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
/// Ground with open sky above, for a patrol.
pub(super) fn open_sky_spot(world: &mut World, x: i32, z: i32, from_y: i32) -> Option<[f64; 3]> {
    let spot = spawn_spot(world, x, z, from_y, 2)?;
    let below = data::get().block_of_state(world.get_block(BlockPos::new(x, spot[1] as i32 - 1, z))).name.as_str();
    (sky_exposed(world, spot) && !matches!(below, "water" | "lava" | "ice")).then_some(spot)
}

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
    // Thunderstorms darken the sky enough for monsters to spawn.
    let night = !is_day(server) || server.weather.lock().unwrap().is_thundering();
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
                    let biome = world.generator().biome_name(x, z);
                    let wolves = (biome.contains("forest") || biome.contains("taiga") || biome == "grove") && entities::random() < 0.25;
                    let kind = if wolves { MobKind::Wolf } else { [MobKind::Cow, MobKind::Pig, MobKind::Sheep, MobKind::Chicken][(entities::random() * 4.0) as usize % 4] };
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

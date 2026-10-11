//! Raids. A player with Bad Omen who walks into a village sets off a raid: waves of
//! raiders (pillagers, vindicators, witches, evokers and ravagers, more of them each wave and
//! with a stronger omen) come from outside the village, go for the villagers and fight
//! whoever defends them. A boss bar shows the raiders left; between waves the horn sounds.
//! Defeating every wave gives the defenders Hero of the Village; if the villagers all die,
//! the raid is lost and the raiders celebrate.
//!
//! Raids in progress are saved in `raids.json` (their raiders, with the mobs) and go on
//! after a restart.
//!
//! Also the illagers that only come with raids: evokers (fang attacks, summoning vexes,
//! keeping their distance), vexes (fly through walls, live a short while) and ravagers
//! (heavy hitters that trample leaves and crops in their way).

use super::mobs::{self, Action, Mob, MobKind, Seen};
use super::{effects, entities, sound, Server};
use crate::data;
use crate::protocol::{id, packet, text, BlockPos, Nbt, Writer};
use crate::world::{Dimension, World};
use std::sync::{Arc, Mutex};

/// Raiders of each kind per wave (vanilla Raid.RaiderType): index = wave number.
const WAVES: [(MobKind, [i32; 9]); 5] = [
    (MobKind::Vindicator, [0, 0, 2, 0, 1, 4, 2, 5, 5]),
    (MobKind::Evoker, [0, 0, 0, 0, 1, 1, 0, 2, 2]),
    (MobKind::Pillager, [0, 4, 3, 3, 4, 4, 4, 2, 2]),
    (MobKind::Witch, [0, 0, 0, 0, 3, 0, 0, 1, 1]),
    (MobKind::Ravager, [0, 0, 0, 1, 0, 1, 0, 2, 2]),
];

/// Ticks between waves (vanilla: 300).
const PAUSE: i64 = 300;
/// How far from the village the bar is shown and heroes are counted.
const RANGE: f64 = 96.0;
/// A raid that goes on for this long is called off (40 minutes).
const TIMEOUT: i64 = 48000;

#[derive(PartialEq, Clone, Copy, Debug)]
enum Phase {
    /// Next wave after this tick.
    Waiting(i64),
    Fighting,
    /// Over (won or lost) until this tick, when the bar goes away.
    Over(i64, bool),
}

struct Raid {
    id: u32,
    center: [f64; 3],
    level: i32,
    wave: i32,
    waves: i32,
    raiders: Vec<i32>,
    wave_health: f32,
    phase: Phase,
    started: i64,
    bar: u128,
    viewers: Vec<i32>,
    heroes: Vec<u128>,
}

static RAIDS: Mutex<Vec<Raid>> = Mutex::new(Vec::new());

pub fn is_raider(kind: MobKind) -> bool {
    matches!(kind, MobKind::Pillager | MobKind::Vindicator | MobKind::Witch | MobKind::Evoker | MobKind::Ravager)
}

fn waves_for(difficulty: u8) -> i32 {
    match difficulty {
        0 | 1 => 3,
        2 => 5,
        _ => 7,
    }
}

fn dist_flat(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

// ---------- boss bar ----------

fn title(r: &Raid) -> Nbt {
    let raid = |key: &str| Nbt::compound().with("translate", Nbt::str(key));
    match r.phase {
        Phase::Over(_, true) => raid("event.minecraft.raid").with("extra", Nbt::List(vec![text(" - "), raid("event.minecraft.raid.victory")])),
        Phase::Over(_, false) => raid("event.minecraft.raid").with("extra", Nbt::List(vec![text(" - "), raid("event.minecraft.raid.defeat")])),
        Phase::Fighting if !r.raiders.is_empty() => raid("event.minecraft.raid").with(
            "extra",
            Nbt::List(vec![text(" - "), raid("event.minecraft.raid.raiders_remaining").with("with", Nbt::List(vec![text(&r.raiders.len().to_string())]))]),
        ),
        _ => raid("event.minecraft.raid"),
    }
}

fn bar_add(r: &Raid, progress: f32) -> Vec<u8> {
    let mut p = packet(id::play::BOSS_BAR);
    p.uuid(r.bar).varint(0);
    title(r).write_network(&mut p);
    // Red, 10 notches, no flags.
    p.f32(progress.clamp(0.0, 1.0)).varint(2).varint(2).u8(0);
    p
}

fn bar_update(r: &Raid, progress: f32) -> Vec<Vec<u8>> {
    let mut h = packet(id::play::BOSS_BAR);
    h.uuid(r.bar).varint(2).f32(progress.clamp(0.0, 1.0));
    let mut t = packet(id::play::BOSS_BAR);
    t.uuid(r.bar).varint(3);
    title(r).write_network(&mut t);
    vec![h, t]
}

fn bar_remove(r: &Raid) -> Vec<u8> {
    let mut p = packet(id::play::BOSS_BAR);
    p.uuid(r.bar).varint(1);
    p
}

// ---------- the raid ----------

/// Every second: Bad Omen in a village starts a raid; raids advance.
pub fn tick(server: &Arc<Server>, seen: &[Seen], age: i64) {
    if age % 20 == 0 {
        trigger(server, seen, age);
    }
    if age % 10 == 0 {
        advance(server, seen, age);
    }
}

fn trigger(server: &Arc<Server>, seen: &[Seen], age: i64) {
    if server.config.difficulty == 0 {
        return;
    }
    for s in seen.iter().filter(|s| s.dim == Dimension::Overworld && s.present) {
        let omen = {
            let st = s.player.state.lock().unwrap();
            effects::level(&st, "bad_omen")
        };
        let Some(amp) = omen else { continue };
        let center = {
            let world = server.world(Dimension::Overworld).lock().unwrap();
            world.generator().village_centers_near(s.pos[0] as i32, s.pos[2] as i32, 40).first().copied()
        };
        let Some((cx, cz)) = center else { continue };
        // A village needs villagers.
        let c = [cx as f64 + 0.5, s.pos[1], cz as f64 + 0.5];
        let villagers = server.mobs.lock().unwrap().values().any(|m| m.kind == MobKind::Villager && m.dim == Dimension::Overworld && dist_flat(&m.pos, &c) < RANGE);
        if !villagers {
            continue;
        }
        effects::remove(server, &s.player, "bad_omen");
        let mut raids = RAIDS.lock().unwrap();
        if let Some(r) = raids.iter_mut().find(|r| dist_flat(&r.center, &c) < RANGE && !matches!(r.phase, Phase::Over(..))) {
            // A stronger omen makes the raid bigger.
            r.level = (r.level + amp + 1).min(5);
            r.waves = waves_for(server.config.difficulty) + i32::from(r.level > 1);
            continue;
        }
        let level = (amp + 1).min(5);
        let id = 1 + (entities::random() * 1e9) as u32;
        raids.push(Raid {
            id,
            center: c,
            level,
            wave: 0,
            waves: waves_for(server.config.difficulty) + i32::from(level > 1),
            raiders: vec![],
            wave_health: 1.0,
            phase: Phase::Waiting(age + PAUSE),
            started: age,
            bar: 0x5241_4944_0000_0000_0000_0000_0000_0000 | id as u128,
            viewers: vec![],
            heroes: vec![s.player.uuid],
        });
        tracing::info!("incursión en la aldea ({cx}, {cz}), presagio {level}, por {}", s.player.name);
    }
}

fn advance(server: &Arc<Server>, seen: &[Seen], age: i64) {
    let mut spawns: Vec<(u32, [f64; 3], i32, i32)> = Vec::new();
    let mut packets: Vec<(i32, Vec<u8>)> = Vec::new();
    let mut horns: Vec<[f64; 3]> = Vec::new();
    let mut rewards: Vec<(u128, i32)> = Vec::new();
    let mut celebrate: Vec<i32> = Vec::new();
    {
        let mut raids = RAIDS.lock().unwrap();
        let mobs_map = server.mobs.lock().unwrap();
        for r in raids.iter_mut() {
            // Raiders still alive (also those loaded after a restart), and villagers left.
            r.raiders = mobs_map.values().filter(|m| m.raid == r.id && m.death_ticks.is_none()).map(|m| m.eid).collect();
            let villagers = mobs_map.values().any(|m| m.kind == MobKind::Villager && m.death_ticks.is_none() && dist_flat(&m.pos, &r.center) < RANGE);
            let left: f32 = r.raiders.iter().filter_map(|e| mobs_map.get(e)).map(|m| m.health.max(0.0)).sum();
            let progress = match r.phase {
                Phase::Waiting(at) => 1.0 - (at - age) as f32 / PAUSE as f32,
                Phase::Fighting => left / r.wave_health.max(1.0),
                Phase::Over(..) => 0.0,
            };
            // Who is around sees the bar (and counts as a defender).
            let near: Vec<&Seen> = seen.iter().filter(|s| s.dim == Dimension::Overworld && dist_flat(&s.pos, &r.center) < RANGE).collect();
            for s in &near {
                if !r.viewers.contains(&s.player.eid) {
                    r.viewers.push(s.player.eid);
                    packets.push((s.player.eid, bar_add(r, progress)));
                }
                if !r.heroes.contains(&s.player.uuid) {
                    r.heroes.push(s.player.uuid);
                }
            }
            let gone: Vec<i32> = r.viewers.iter().copied().filter(|v| !near.iter().any(|s| s.player.eid == *v)).collect();
            for v in gone {
                packets.push((v, bar_remove(r)));
                r.viewers.retain(|x| *x != v);
            }
            match r.phase {
                Phase::Waiting(_) | Phase::Fighting if !villagers || age - r.started > TIMEOUT => {
                    r.phase = Phase::Over(age + 600, false);
                    celebrate.extend(r.raiders.iter().copied());
                }
                Phase::Waiting(at) if at <= age => {
                    r.wave += 1;
                    r.phase = Phase::Fighting;
                    spawns.push((r.id, r.center, r.wave, r.level));
                    horns.push(r.center);
                }
                Phase::Fighting if r.raiders.is_empty() && r.wave_health > 1.0 => {
                    if r.wave >= r.waves {
                        r.phase = Phase::Over(age + 600, true);
                        for h in &r.heroes {
                            rewards.push((*h, r.level - 1));
                        }
                    } else {
                        r.phase = Phase::Waiting(age + PAUSE);
                        r.wave_health = 1.0;
                    }
                }
                _ => {}
            }
            for v in &r.viewers {
                for p in bar_update(r, progress) {
                    packets.push((*v, p));
                }
            }
            if let Phase::Over(until, _) = r.phase {
                if until <= age {
                    for v in &r.viewers {
                        packets.push((*v, bar_remove(r)));
                    }
                    r.viewers.clear();
                }
            }
        }
        raids.retain(|r| !matches!(r.phase, Phase::Over(until, _) if until <= age));
    }
    for (eid, p) in packets {
        if let Some(pl) = server.players.read().unwrap().get(&eid) {
            pl.send(p);
        }
    }
    for c in horns {
        for p in server.online().into_iter().filter(|p| p.dim() == Dimension::Overworld) {
            let pos = {
                let st = p.state.lock().unwrap();
                [st.x, st.y, st.z]
            };
            if dist_flat(&pos, &c) < RANGE * 2.0 {
                sound::at(server, Dimension::Overworld, [pos[0] + (c[0] - pos[0]).signum() * 8.0, pos[1], pos[2] + (c[2] - pos[2]).signum() * 8.0], "event.raid.horn", sound::NEUTRAL, 64.0, 1.0);
            }
        }
    }
    for (raid, center, wave, level) in spawns {
        spawn_wave(server, raid, center, wave, level);
    }
    for (uuid, amp) in rewards {
        if let Some(p) = server.online().into_iter().find(|p| p.uuid == uuid) {
            effects::add(server, &p, "hero_of_the_village", amp.max(0), 48000, 1.0);
            super::advancements::grant(server, &p, "adventure/root");
        }
    }
    if !celebrate.is_empty() {
        let mut map = server.mobs.lock().unwrap();
        for e in celebrate {
            if let Some(m) = map.get_mut(&e) {
                m.flags |= CELEBRATING;
                server.broadcast_tracked(e, &mobs::metadata(m), true);
            }
        }
    }
}

// ---------- saving ----------

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedRaid {
    id: u32,
    center: [f64; 3],
    level: i32,
    wave: i32,
    waves: i32,
    wave_health: f32,
    /// Ticks until the next wave (none: fighting one).
    next_wave: Option<i64>,
    /// Ticks the raid has lasted.
    elapsed: i64,
    heroes: Vec<String>,
}

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("raids.json")
}

/// Saves the raids in progress.
pub fn save(server: &Server) {
    let age = server.age.load(std::sync::atomic::Ordering::Relaxed);
    let saved: Vec<SavedRaid> = RAIDS
        .lock()
        .unwrap()
        .iter()
        .filter_map(|r| {
            let next_wave = match r.phase {
                Phase::Waiting(at) => Some((at - age).max(0)),
                Phase::Fighting => None,
                Phase::Over(..) => return None,
            };
            Some(SavedRaid {
                id: r.id,
                center: r.center,
                level: r.level,
                wave: r.wave,
                waves: r.waves,
                wave_health: r.wave_health,
                next_wave,
                elapsed: age - r.started,
                heroes: r.heroes.iter().map(|u| format!("{u:032x}")).collect(),
            })
        })
        .collect();
    let path = file(server);
    if saved.is_empty() {
        let _ = std::fs::remove_file(path);
    } else if let Err(e) = std::fs::write(&path, serde_json::to_string(&saved).unwrap_or_default()) {
        tracing::warn!("no se pudieron guardar las incursiones: {e}");
    }
}

/// Loads the raids that were going on (after the mobs: their raiders head back to the village).
pub fn load(server: &Server) {
    let Ok(text) = std::fs::read_to_string(file(server)) else { return };
    let Ok(saved) = serde_json::from_str::<Vec<SavedRaid>>(&text) else {
        tracing::warn!("raids.json inválido; se ignora");
        return;
    };
    let age = server.age.load(std::sync::atomic::Ordering::Relaxed);
    let mut map = server.mobs.lock().unwrap();
    let mut raids = RAIDS.lock().unwrap();
    for s in saved {
        for m in map.values_mut().filter(|m| m.raid == s.id) {
            m.anchor = Some(s.center);
        }
        raids.push(Raid {
            id: s.id,
            center: s.center,
            level: s.level,
            wave: s.wave,
            waves: s.waves,
            raiders: vec![],
            wave_health: s.wave_health,
            phase: match s.next_wave {
                Some(t) => Phase::Waiting(age + t),
                None => Phase::Fighting,
            },
            started: age - s.elapsed,
            bar: 0x5241_4944_0000_0000_0000_0000_0000_0000 | s.id as u128,
            viewers: vec![],
            heroes: s.heroes.iter().filter_map(|h| u128::from_str_radix(h, 16).ok()).collect(),
        });
    }
    tracing::info!("{} incursiones en curso", raids.len());
}

/// Raider flag: cheering after a won raid.
pub const CELEBRATING: u32 = 0x100;

fn spawn_wave(server: &Arc<Server>, raid: u32, center: [f64; 3], wave: i32, level: i32) {
    let difficulty = server.config.difficulty;
    // From a spot on the ground outside the village.
    let mut spot = None;
    for _ in 0..20 {
        let a = entities::random() as f64 * std::f64::consts::TAU;
        let (x, z) = ((center[0] + a.cos() * 32.0).floor() as i32, (center[2] + a.sin() * 32.0).floor() as i32);
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        let top = world.generator().surface(x, z);
        if let Some(s) = mobs::open_sky_spot(&mut world, x, z, top + 4) {
            spot = Some(s);
            break;
        }
    }
    // No room outside: they come out at the village itself.
    let spot = spot.unwrap_or_else(|| {
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        let (x, z) = (center[0].floor() as i32, center[2].floor() as i32);
        let top = world.generator().surface(x, z);
        mobs::spawn_spot_at(&mut world, x, z, top + 4, 2).unwrap_or([center[0], top as f64 + 1.0, center[2]])
    });
    let idx = (wave as usize).min(8);
    let mut eids = Vec::new();
    for (kind, counts) in WAVES {
        let mut n = counts[idx];
        // Extra pillagers and vindicators on the harder settings and with stronger omens.
        if matches!(kind, MobKind::Pillager | MobKind::Vindicator) && difficulty >= 2 {
            n += (entities::random() * (difficulty as f32 - 1.0 + (level - 1) as f32 * 0.5)) as i32;
        }
        for _ in 0..n {
            let at = [spot[0] + (entities::random() as f64 - 0.5) * 3.0, spot[1], spot[2] + (entities::random() as f64 - 0.5) * 3.0];
            let eid = mobs::spawn(server, Dimension::Overworld, kind, at);
            if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
                m.raid = raid;
                m.anchor = Some(center);
            }
            eids.push(eid);
        }
    }
    // The first pillager of the first wave carries the banner.
    if wave == 1 {
        if let Some(&first) = eids.iter().find(|e| mobs::kind_of(server, **e) == Some(MobKind::Pillager)) {
            super::pillagers::make_captain(server, first, 0);
        }
    }
    let health: f32 = {
        let map = server.mobs.lock().unwrap();
        eids.iter().filter_map(|e| map.get(e)).map(|m| m.health).sum()
    };
    let mut raids = RAIDS.lock().unwrap();
    if let Some(r) = raids.iter_mut().find(|r| r.id == raid) {
        r.raiders = eids;
        r.wave_health = health.max(2.0);
    }
}

/// A raider with no player to fight: off to the village, and after its villagers.
pub fn raider_goal(world: &mut World, m: &mut Mob, others: &[super::animals::Other], actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    if m.flags & CELEBRATING != 0 {
        if m.on_ground && entities::random() < 0.05 {
            m.vel[1] = 0.4;
        }
        return Some((m.pos, 0.0));
    }
    let center = m.anchor?;
    let villager = others
        .iter()
        .filter(|o| o.kind == MobKind::Villager && mobs::dist2(&o.pos, &m.pos) < 32.0 * 32.0)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)));
    let speed = m.kind.walk_speed();
    let Some(v) = villager else {
        return (dist_flat(&center, &m.pos) > 8.0).then(|| (mobs::follow_path(world, m, center), speed));
    };
    mobs::face(m, v.pos);
    let d = mobs::dist2(&v.pos, &m.pos).sqrt();
    match m.kind {
        // Pillagers shoot, witches throw potions, from where they stand.
        MobKind::Pillager | MobKind::Witch if d < 12.0 => {
            if m.attack_cooldown == 0 {
                m.attack_cooldown = 50;
                let from = [m.pos[0], m.pos[1] + 1.5, m.pos[2]];
                let to = [v.pos[0] - from[0], v.pos[1] + 1.0 - from[1], v.pos[2] - from[2]];
                let horiz = (to[0] * to[0] + to[2] * to[2]).sqrt();
                if m.kind == MobKind::Pillager {
                    let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
                    actions.push(Action::Shoot(m.dim, super::projectiles::ProjectileKind::Arrow, m.eid, from, [dir[0] * 1.6, dir[1] * 1.6, dir[2] * 1.6]));
                } else if let Some(item) = effects::potion_item("splash_potion", "harming") {
                    let dir = mobs::normalize([to[0], to[1] + horiz * 0.2, to[2]]);
                    actions.push(Action::Shoot(m.dim, super::projectiles::ProjectileKind::Potion(item), m.eid, from, [dir[0] * 0.75, dir[1] * 0.75, dir[2] * 0.75]));
                }
            }
            Some((m.pos, 0.0))
        }
        MobKind::Evoker if d < 10.0 => {
            if m.attack_cooldown == 0 {
                m.attack_cooldown = 100;
                fang_line(m, v.pos, actions);
            }
            Some((m.pos, 0.0))
        }
        _ => {
            if d < 1.6 + m.size().0 / 2.0 && m.attack_cooldown == 0 {
                m.attack_cooldown = 20;
                actions.push(Action::HurtMob(v.eid, m.kind.melee_damage(), m.eid, None));
            }
            Some((mobs::follow_path(world, m, v.pos), speed * 1.2))
        }
    }
}

/// Villagers run from raiders (and vexes) close by.
pub fn villager_flee(world: &mut World, m: &mut Mob, others: &[super::animals::Other]) -> Option<([f64; 3], f64)> {
    let threat = others
        .iter()
        .filter(|o| (is_raider(o.kind) || o.kind == MobKind::Vex || o.kind.zombie_like()) && mobs::dist2(&o.pos, &m.pos) < 10.0 * 10.0)
        .min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))?;
    let away = mobs::normalize([m.pos[0] - threat.pos[0], 0.0, m.pos[2] - threat.pos[2]]);
    let dest = [m.pos[0] + away[0] * 8.0, m.pos[1], m.pos[2] + away[2] * 8.0];
    Some((mobs::follow_path(world, m, dest), m.kind.walk_speed() * 1.6))
}

// ---------- evokers, fangs and vexes ----------

/// Spell being cast (metadata): 1 summon vexes, 2 fangs.
pub const SPELL_VEX: u8 = 1;
pub const SPELL_FANGS: u8 = 2;

fn fang_line(m: &mut Mob, to: [f64; 3], actions: &mut Vec<Action>) {
    m.spell = SPELL_FANGS;
    m.drink = 20;
    let dir = mobs::normalize([to[0] - m.pos[0], 0.0, to[2] - m.pos[2]]);
    let fangs: Vec<([f64; 3], i32)> = (0..16).map(|i| {
        let d = 1.25 * (i + 1) as f64;
        ([m.pos[0] + dir[0] * d, m.pos[1], m.pos[2] + dir[2] * d], i)
    }).collect();
    actions.push(Action::Fangs(m.dim, m.eid, fangs));
    actions.push(Action::Sound(m.eid, "entity.evoker.prepare_attack".into(), sound::HOSTILE));
}

/// An evoker fighting a player: keeps its distance, bites with fangs (a ring around itself
/// when the player is close, a line otherwise) and summons vexes.
pub fn evoker_attack(world: &mut World, m: &mut Mob, t: &Seen, d: f64, actions: &mut Vec<Action>) -> Option<([f64; 3], f64)> {
    let eye = [m.pos[0], m.pos[1] + 1.6, m.pos[2]];
    let sees = mobs::line_of_sight(world, eye, t.eye);
    if m.drink > 0 {
        // Casting: stands still with its arms up.
        m.drink -= 1;
        if m.drink == 0 {
            m.spell = 0;
        }
        return Some((m.pos, 0.0));
    }
    if sees && m.burst <= 0 && d < 16.0 {
        // Three vexes every 17 s.
        m.burst = 340;
        m.spell = SPELL_VEX;
        m.drink = 20;
        actions.push(Action::SummonVexes(m.dim, m.eid, m.pos, t.player.eid));
        actions.push(Action::Sound(m.eid, "entity.evoker.prepare_summon".into(), sound::HOSTILE));
        return Some((m.pos, 0.0));
    }
    m.burst -= 1;
    if sees && m.attack_cooldown == 0 && d < 16.0 {
        m.attack_cooldown = 100;
        if d < 3.0 {
            m.spell = SPELL_FANGS;
            m.drink = 20;
            let mut fangs = Vec::new();
            for i in 0..5 {
                let a = i as f64 * std::f64::consts::TAU / 5.0;
                fangs.push(([m.pos[0] + a.cos() * 1.5, m.pos[1], m.pos[2] + a.sin() * 1.5], 0));
            }
            for i in 0..8 {
                let a = i as f64 * std::f64::consts::TAU / 8.0 + 1.2566;
                fangs.push(([m.pos[0] + a.cos() * 2.5, m.pos[1], m.pos[2] + a.sin() * 2.5], 3));
            }
            actions.push(Action::Fangs(m.dim, m.eid, fangs));
            actions.push(Action::Sound(m.eid, "entity.evoker.prepare_attack".into(), sound::HOSTILE));
        } else {
            fang_line(m, t.pos, actions);
        }
        return Some((m.pos, 0.0));
    }
    // Keeps 8 blocks away.
    if d < 8.0 {
        let away = mobs::normalize([m.pos[0] - t.pos[0], 0.0, m.pos[2] - t.pos[2]]);
        let dest = [m.pos[0] + away[0] * 6.0, m.pos[1], m.pos[2] + away[2] * 6.0];
        return Some((mobs::follow_path(world, m, dest), m.kind.walk_speed() * 1.2));
    }
    if !sees || d > 14.0 {
        return Some((mobs::follow_path(world, m, t.pos), m.kind.walk_speed()));
    }
    Some((m.pos, 0.0))
}

/// Evoker fangs: they appear, snap after a short delay and vanish.
struct Fang {
    eid: i32,
    dim: Dimension,
    pos: [f64; 3],
    owner: i32,
    delay: i32,
    age: i32,
}

static FANGS: Mutex<Vec<Fang>> = Mutex::new(Vec::new());

pub fn spawn_fangs(server: &Arc<Server>, dim: Dimension, owner: i32, list: Vec<([f64; 3], i32)>) {
    let reg = data::get();
    let mut out = Vec::new();
    for (pos, delay) in list {
        // On the ground: from a little above, down to the first solid block.
        let ground = {
            let mut world = server.world(dim).lock().unwrap();
            let g = crate::gameplay::get();
            (0..6).map(|dy| pos[1].floor() as i32 + 2 - dy).find(|&y| {
                let below = world.get_block(BlockPos::new(pos[0].floor() as i32, y - 1, pos[2].floor() as i32));
                let here = world.get_block(BlockPos::new(pos[0].floor() as i32, y, pos[2].floor() as i32));
                !g.shape(below).is_empty() && g.shape(here).is_empty()
            })
        };
        let Some(y) = ground else { continue };
        let f = Fang { eid: server.next_entity_id(), dim, pos: [pos[0], y as f64, pos[2]], owner, delay: delay * 2 + 6, age: 0 };
        let mut p = packet(id::play::SPAWN_ENTITY);
        let uuid = ((entities::random().to_bits() as u128) << 96) | f.eid as u128;
        p.varint(f.eid).uuid(uuid).varint(reg.entity("evoker_fangs"));
        p.f64(f.pos[0]).f64(f.pos[1]).f64(f.pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(0);
        p.i16(0).i16(0).i16(0);
        server.broadcast_dim(dim, &p, None);
        out.push(f);
    }
    FANGS.lock().unwrap().extend(out);
}

pub fn tick_fangs(server: &Arc<Server>) {
    let mut bites = Vec::new();
    let mut gone = Vec::new();
    {
        let mut fangs = FANGS.lock().unwrap();
        if fangs.is_empty() {
            return;
        }
        for f in fangs.iter_mut() {
            f.age += 1;
            if f.age == f.delay {
                bites.push((f.eid, f.dim, f.pos, f.owner));
            }
            if f.age >= f.delay + 20 {
                gone.push((f.eid, f.dim));
            }
        }
        fangs.retain(|f| f.age < f.delay + 20);
    }
    for (eid, dim, pos, owner) in bites {
        let mut st = packet(id::play::ENTITY_STATUS);
        st.i32(eid).i8(4);
        server.broadcast_dim(dim, &st, None);
        sound::at(server, dim, pos, "entity.evoker_fangs.attack", sound::HOSTILE, 1.0, 1.0);
        let by = super::health::Attacker { eid: owner, name: "Invocador".into(), pos: Some(pos) };
        for p in server.online().into_iter().filter(|p| p.dim() == dim) {
            let at = {
                let s = p.state.lock().unwrap();
                [s.x, s.y, s.z]
            };
            if dist_flat(&at, &pos) < 0.9 && (at[1] - pos[1]).abs() < 1.5 {
                super::health::damage(server, &p, mobs::difficulty_damage(server, 6.0), "magic", Some(by.clone()));
            }
        }
        let hit: Vec<i32> = {
            let map = server.mobs.lock().unwrap();
            map.values().filter(|m| m.dim == dim && m.eid != owner && !is_raider(m.kind) && m.kind != MobKind::Vex && dist_flat(&m.pos, &pos) < 0.9 && (m.pos[1] - pos[1]).abs() < 1.5).map(|m| m.eid).collect()
        };
        for e in hit {
            mobs::hurt(server, e, 6.0, "magic", Some(owner), None, 0);
        }
    }
    for (eid, dim) in gone {
        let mut d = packet(id::play::ENTITY_DESTROY);
        d.varint(1).varint(eid);
        server.broadcast_dim(dim, &d, None);
    }
}

/// Three vexes appear around the evoker, after its target.
pub fn summon_vexes(server: &Arc<Server>, dim: Dimension, owner: i32, at: [f64; 3], target: i32) {
    let (raid, count) = {
        let map = server.mobs.lock().unwrap();
        let raid = map.get(&owner).map_or(0, |m| m.raid);
        (raid, map.values().filter(|m| m.kind == MobKind::Vex && m.summoner == Some(owner)).count())
    };
    if count >= 8 {
        return;
    }
    for _ in 0..3 {
        let p = [at[0] + (entities::random() as f64 - 0.5) * 2.0, at[1] + 1.0, at[2] + (entities::random() as f64 - 0.5) * 2.0];
        let eid = mobs::spawn(server, dim, MobKind::Vex, p);
        if let Some(m) = server.mobs.lock().unwrap().get_mut(&eid) {
            m.summoner = Some(owner);
            m.target = Some(target);
            m.raid = raid;
            m.life = 20 * (30 + (entities::random() * 90.0) as u32);
        }
    }
}

/// A vex: flies straight through walls at its target, charges, and wastes away after a while.
pub fn vex(server: &Server, m: &mut Mob, seen: &[Seen], actions: &mut Vec<Action>) {
    if m.life > 0 {
        m.life -= 1;
        if m.life == 0 {
            m.life = 1;
            if m.age % 20 == 0 {
                m.health -= 1.0;
                m.hurt_time = 10;
            }
        }
    }
    let target = m.target.and_then(|e| seen.iter().find(|s| s.player.eid == e && s.targetable)).or_else(|| {
        seen.iter().filter(|s| s.targetable && mobs::dist2(&s.pos, &m.pos) < 16.0 * 16.0).min_by(|a, b| mobs::dist2(&a.pos, &m.pos).total_cmp(&mobs::dist2(&b.pos, &m.pos)))
    });
    m.target = target.map(|t| t.player.eid);
    let Some(t) = target else {
        // Wanders about.
        if m.wander.is_none_or(|w| mobs::dist2(&w, &m.pos) < 1.0) {
            m.wander = Some([m.pos[0] + (entities::random() as f64 - 0.5) * 8.0, m.pos[1] + (entities::random() as f64 - 0.5) * 4.0, m.pos[2] + (entities::random() as f64 - 0.5) * 8.0]);
        }
        let w = m.wander.unwrap();
        let dir = mobs::normalize([w[0] - m.pos[0], w[1] - m.pos[1], w[2] - m.pos[2]]);
        m.vel = [dir[0] * 0.1, dir[1] * 0.1, dir[2] * 0.1];
        m.charging = false;
        return;
    };
    let aim = [t.pos[0], t.pos[1] + 1.0, t.pos[2]];
    let d = mobs::dist2(&aim, &m.center()).sqrt();
    m.charging = d < 9.0;
    let speed = if m.charging { 0.3 } else { 0.15 };
    let dir = mobs::normalize([aim[0] - m.pos[0], aim[1] - m.pos[1], aim[2] - m.pos[2]]);
    m.vel = [dir[0] * speed, dir[1] * speed, dir[2] * speed];
    mobs::face(m, t.pos);
    if d < 1.3 && m.attack_cooldown == 0 {
        m.attack_cooldown = 20;
        let by = super::health::Attacker { eid: m.eid, name: m.kind.display().to_string(), pos: Some(m.pos) };
        actions.push(Action::Hurt(t.player.clone(), mobs::difficulty_damage(server, 9.0), "mob_attack", by));
    }
}

/// A ravager tramples the leaves and crops in its way.
pub fn trample(world: &mut World, m: &Mob, actions: &mut Vec<Action>) {
    let reg = data::get();
    let (w, h) = m.size();
    let ahead = [m.pos[0] + m.vel[0] * 4.0, m.pos[1], m.pos[2] + m.vel[2] * 4.0];
    for dy in 0..h.ceil() as i32 {
        for dx in [-w / 2.0, 0.0, w / 2.0] {
            for dz in [-w / 2.0, 0.0, w / 2.0] {
                let p = BlockPos::new((ahead[0] + dx).floor() as i32, ahead[1].floor() as i32 + dy, (ahead[2] + dz).floor() as i32);
                let s = world.get_block(p);
                let n = reg.block_of_state(s).name.as_str();
                if n.ends_with("_leaves") || matches!(n, "wheat" | "carrots" | "potatoes" | "beetroots" | "short_grass" | "tall_grass" | "fern") {
                    actions.push(Action::Break(m.dim, p, s));
                }
            }
        }
    }
}

/// Bad Omen: each captain killed makes it one level stronger (up to V).
pub fn omen_level(player: &super::Player) -> i32 {
    let st = player.state.lock().unwrap();
    effects::level(&st, "bad_omen").map_or(0, |l| (l + 1).min(4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waves_grow() {
        let size = |w: usize| WAVES.iter().map(|(_, c)| c[w]).sum::<i32>();
        assert_eq!(size(1), 4);
        assert!(size(7) > size(3));
        assert_eq!(waves_for(1), 3);
        assert_eq!(waves_for(3), 7);
    }
}

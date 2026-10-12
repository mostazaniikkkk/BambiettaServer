//! Global server state and periodic tasks.

pub mod admin;
pub mod beacons;
pub mod blockmech;
pub mod falling;
pub mod frames;
pub mod decor;
pub mod gadgets;
pub mod household;
pub mod gateways;
pub mod sites;
pub mod leads;
pub mod creatures;
pub mod guardians;
pub mod warden;
pub mod wither;
pub mod raids;
pub mod golems;
pub mod horses;
pub mod pets;
pub mod wildlife;
pub mod monsters;
pub mod hoppers;
pub mod mechanisms;
pub mod auth;
pub mod advancements;
pub mod animals;
pub mod anticheat;
pub mod beds;
pub mod bees;
pub mod blocks;
pub mod border;
pub mod brewing;
pub mod commands;
pub mod containers;
pub mod doors;
pub mod dragon;
pub mod effects;
pub mod enchanting;
pub mod entities;
pub mod experience;
pub mod explosion;
pub mod eyes;
pub mod farming;
pub mod fire;
pub mod fishing;
pub mod fluids;
pub mod gamerules;
pub mod health;
pub mod inventory;
pub mod maps;
pub mod mobs;
pub mod pillagers;
pub mod play;
pub mod playerdata;
pub mod portals;
pub mod projectiles;
pub mod query;
pub mod ranged;
pub mod redstone;
pub mod session;
pub mod signs;
pub mod skeletons;
pub mod spawners;
pub mod sound;
pub mod stations;
pub mod tracker;
pub mod vehicles;
pub mod villagers;
pub mod weather;

use crate::config::Config;
use crate::net::{Out, Tx};
use crate::protocol::{id, packet, text, BlockPos, Writer};
use crate::world::{Dimension, World};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI32, AtomicI64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::sync::Notify;
use tracing::{error, info};

pub struct Server {
    pub config: Config,
    /// Overworld, Nether and End (index = `Dimension::index`).
    worlds: [Mutex<World>; 3],
    pub players: RwLock<HashMap<i32, Arc<Player>>>,
    next_eid: AtomicI32,
    pub age: AtomicI64,
    pub day_time: AtomicI64,
    pub spawn: BlockPos,
    pub items: Mutex<HashMap<i32, entities::ItemEntity>>,
    pub orbs: Mutex<HashMap<i32, entities::XpOrb>>,
    pub fluids: Mutex<fluids::Fluids>,
    pub mobs: Mutex<HashMap<i32, mobs::Mob>>,
    pub projectiles: Mutex<Vec<projectiles::Projectile>>,
    pub redstone: Mutex<redstone::Redstone>,
    pub fire: Mutex<fire::Fire>,
    pub vehicles: Mutex<HashMap<i32, vehicles::Vehicle>>,
    pub bobbers: Mutex<HashMap<i32, fishing::Bobber>>,
    pub dragon: Mutex<dragon::Fight>,
    pub weather: Mutex<weather::Weather>,
    pub rules: Mutex<gamerules::GameRules>,
    pub admin: Mutex<admin::Admin>,
    pub border: Mutex<border::Border>,
    /// Already populated villages (loaded from `villages.json` on first use).
    pub villages: Mutex<Option<HashSet<(i32, i32)>>>,
    /// One save at a time (region files are written outside the world lock).
    save_lock: Mutex<()>,
}

pub struct Player {
    pub eid: i32,
    pub uuid: u128,
    pub name: String,
    pub transport: &'static str,
    /// Address it connected from.
    pub peer: std::net::SocketAddr,
    /// Profile properties from Mojang (the skin), empty in offline mode.
    pub properties: Vec<auth::Property>,
    pub tx: Tx,
    /// Notified to close the session (kick, shutdown).
    pub closed: Notify,
    pub state: Mutex<PlayerState>,
    /// Current dimension (atomic so it can be read without locking the state).
    pub dim_index: AtomicU8,
    /// Entities this client currently has (distance-based tracking).
    pub tracked: Mutex<HashSet<i32>>,
}

pub struct PlayerState {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub game_mode: u8,
    pub view_distance: i32,
    pub center: (i32, i32),
    pub loaded: HashSet<(i32, i32)>,
    pub held_slot: usize,
    pub inv: inventory::Inventory,
    pub sneaking: bool,
    pub sprinting: bool,
    pub ping: i32,
    // Survival
    pub health: f32,
    pub food: i32,
    pub saturation: f32,
    pub exhaustion: f32,
    pub regen_timer: u32,
    pub air: i32,
    pub fall_distance: f64,
    pub hurt_cooldown: u32,
    pub dead: bool,
    /// Eating: (hand, start tick).
    pub eating: Option<(i32, i64)>,
    /// Digging: (block, start tick).
    pub digging: Option<(BlockPos, i64)>,
    pub ac: anticheat::MoveState,
    // Experience
    pub xp_level: i32,
    pub xp_progress: f32,
    pub xp_total: i32,
    /// Enchanting table seed (changes after enchanting).
    pub xp_seed: i32,
    pub chat_times: Vec<std::time::Instant>,
    /// Bed to respawn at (head).
    pub spawn_point: Option<BlockPos>,
    /// Where it last died (the recovery compass points there).
    pub last_death: Option<(crate::world::Dimension, BlockPos)>,
    /// Dimension of `spawn_point`: a bed in the Overworld or a respawn anchor in the Nether.
    pub spawn_dim: crate::world::Dimension,
    /// The ender chest's contents (the same in every ender chest).
    pub ender: Vec<crate::item::Slot>,
    /// Sleeping in this bed.
    pub sleeping: Option<BlockPos>,
    /// Ticks since the player last slept (or died): phantoms come after three days.
    pub rest: i64,
    /// Parrot riding on the left shoulder (its variant).
    pub shoulder: Option<i32>,
    /// Ticks spent freezing in powder snow.
    pub frozen: u32,
    pub sleep_ticks: u32,
    pub portal_ticks: u32,
    pub portal_cooldown: u32,
    /// Active status effects.
    pub effects: Vec<effects::ActiveEffect>,
    /// Extra hearts (Absorption).
    pub absorption: f32,
    /// Advancements made: id -> when (Unix ms).
    pub advancements: HashMap<String, i64>,
    /// Remaining burning ticks.
    pub fire_ticks: i32,
    /// Vehicle being ridden.
    pub vehicle: Option<i32>,
    /// Sign whose editor is open.
    pub editing_sign: Option<BlockPos>,
    /// Maps already sent in full.
    pub maps_seen: HashSet<i32>,
    /// Tick until which another pearl cannot be thrown.
    pub pearl_cooldown: i64,
    /// Changes on dimension change or respawn: chunks being prepared for the
    /// previous view are no longer sent.
    pub view_epoch: u32,
    /// Bow drawn, crossbow charging, trident or shield raised.
    pub using: Option<ranged::Using>,
    /// Tick until which the shield is disabled (axe hit).
    pub shield_disabled_until: i64,
    /// Cast fishing bobber.
    pub bobber: Option<i32>,
}

impl PlayerState {
    pub fn new(x: f64, y: f64, z: f64, game_mode: u8, view_distance: i32) -> Self {
        Self {
            x,
            y,
            z,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            game_mode,
            view_distance,
            center: ((x.floor() as i32) >> 4, (z.floor() as i32) >> 4),
            loaded: HashSet::new(),
            held_slot: 0,
            inv: inventory::Inventory::default(),
            sneaking: false,
            sprinting: false,
            ping: 0,
            health: 20.0,
            food: 20,
            saturation: 5.0,
            exhaustion: 0.0,
            regen_timer: 0,
            air: 300,
            fall_distance: 0.0,
            hurt_cooldown: 0,
            dead: false,
            eating: None,
            digging: None,
            ac: anticheat::MoveState::new([x, y, z]),
            xp_level: 0,
            xp_progress: 0.0,
            xp_total: 0,
            xp_seed: (entities::random() * i32::MAX as f32) as i32,
            chat_times: Vec::new(),
            spawn_point: None,
            last_death: None,
            spawn_dim: crate::world::Dimension::Overworld,
            ender: vec![None; 27],
            rest: 0,
            shoulder: None,
            frozen: 0,
            sleeping: None,
            sleep_ticks: 0,
            portal_ticks: 0,
            portal_cooldown: 0,
            effects: Vec::new(),
            absorption: 0.0,
            advancements: HashMap::new(),
            fire_ticks: 0,
            vehicle: None,
            editing_sign: None,
            maps_seen: HashSet::new(),
            pearl_cooldown: 0,
            view_epoch: 0,
            using: None,
            shield_disabled_until: 0,
            bobber: None,
        }
    }

    pub fn survival_like(&self) -> bool {
        self.game_mode == 0 || self.game_mode == 2
    }

    pub fn eye(&self) -> [f64; 3] {
        [self.x, self.y + if self.sneaking { 1.27 } else { 1.62 }, self.z]
    }

    /// Sends the client the `set_slot` for one of the player's slots.
    pub fn sync_slot(&mut self, player: &Player, slot: usize) {
        if let Some(p) = self.inv.slot_packet(slot) {
            player.send(p);
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
struct LevelData {
    age: i64,
    day_time: i64,
}

impl Server {
    pub fn new(config: Config) -> Result<Arc<Self>> {
        let mut overworld = World::new(&config, Dimension::Overworld)?;
        overworld.note_legacy();
        // level.dat (Anvil, also a vanilla world's) or the old level.json.
        let anvil = crate::world::anvil::read_level(overworld.dir());
        let level: LevelData = match &anvil {
            Some(l) => LevelData { age: l.time, day_time: l.day_time },
            None => std::fs::read_to_string(overworld.dir().join("level.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(LevelData { age: 0, day_time: 1000 }),
        };
        let dragon_killed = anvil.as_ref().is_some_and(|l| l.dragon_killed);
        let w = anvil.as_ref().map_or((false, 0, false, 0, 0), |l| l.weather);
        let weather = weather::Weather::new(w.0, w.1, w.2, w.3, w.4);
        let rules = gamerules::GameRules::new(&anvil.as_ref().map_or(vec![], |l| l.rules.clone()));
        let admin = admin::Admin::load(config.white_list);
        let world_border = match anvil.as_ref().and_then(|l| l.border.clone()) {
            Some(b) => border::Border {
                center: b.center,
                size: b.size,
                target: b.target,
                lerp_ticks: b.lerp_ms / 50,
                damage_per_block: b.damage_per_block,
                safe_zone: b.safe_zone,
                warning_blocks: b.warning_blocks,
                warning_time: b.warning_time,
            },
            None => border::Border::default(),
        };
        let spawn = match anvil.and_then(|l| l.spawn) {
            Some(s) => s,
            None => overworld.spawn_point(),
        };
        info!("punto de aparición: {} {} {}", spawn.x, spawn.y, spawn.z);
        let nether = World::new(&config, Dimension::Nether)?;
        let end = World::new(&config, Dimension::End)?;
        Ok(Arc::new(Self {
            config,
            worlds: [Mutex::new(overworld), Mutex::new(nether), Mutex::new(end)],
            players: RwLock::new(HashMap::new()),
            next_eid: AtomicI32::new(1),
            age: AtomicI64::new(level.age),
            day_time: AtomicI64::new(level.day_time),
            spawn,
            items: Mutex::new(HashMap::new()),
            orbs: Mutex::new(HashMap::new()),
            fluids: Mutex::new(fluids::Fluids::default()),
            mobs: Mutex::new(HashMap::new()),
            projectiles: Mutex::new(Vec::new()),
            redstone: Mutex::new(redstone::Redstone::default()),
            fire: Mutex::new(fire::Fire::default()),
            vehicles: Mutex::new(HashMap::new()),
            bobbers: Mutex::new(HashMap::new()),
            dragon: Mutex::new(dragon::Fight::new(dragon_killed)),
            weather: Mutex::new(weather),
            rules: Mutex::new(rules),
            admin: Mutex::new(admin),
            border: Mutex::new(world_border),
            villages: Mutex::new(None),
            save_lock: Mutex::new(()),
        }))
    }

    pub fn world(&self, dim: Dimension) -> &Mutex<World> {
        &self.worlds[dim.index()]
    }

    /// Sends a packet to players in `dim` (block changes, explosions...).
    pub fn broadcast_dim(&self, dim: Dimension, body: &[u8], except: Option<i32>) {
        for p in self.players.read().unwrap().values() {
            if Some(p.eid) != except && p.dim() == dim {
                p.send(body.to_vec());
            }
        }
    }

    pub fn next_entity_id(&self) -> i32 {
        self.next_eid.fetch_add(1, Ordering::Relaxed)
    }

    pub fn online(&self) -> Vec<Arc<Player>> {
        let mut v: Vec<_> = self.players.read().unwrap().values().cloned().collect();
        v.sort_by_key(|p| p.eid);
        v
    }

    pub fn find_player(&self, name: &str) -> Option<Arc<Player>> {
        self.players.read().unwrap().values().find(|p| p.name.eq_ignore_ascii_case(name)).cloned()
    }

    /// Sends a packet to everyone (optionally except `except`).
    pub fn broadcast(&self, body: &[u8], except: Option<i32>) {
        for p in self.players.read().unwrap().values() {
            if Some(p.eid) != except {
                p.send(body.to_vec());
            }
        }
    }

    /// Sends a packet about entity `eid` only to those tracking it (and to itself if it is a
    /// player and `include_self`).
    pub fn broadcast_tracked(&self, eid: i32, body: &[u8], include_self: bool) {
        for p in self.players.read().unwrap().values() {
            if p.eid == eid {
                if include_self {
                    p.send(body.to_vec());
                }
            } else if p.tracked.lock().unwrap().contains(&eid) {
                p.send(body.to_vec());
            }
        }
    }

    pub fn broadcast_message(&self, msg: &str) {
        info!("[chat] {}", strip_codes(msg));
        self.broadcast(&system_chat(msg), None);
    }

    pub fn save(&self) -> Result<()> {
        let _guard = self.save_lock.lock().unwrap();
        for p in self.online() {
            playerdata::save(self, &p);
        }
        mobs::save(self);
        raids::save(self);
        frames::save(self);
        decor::save(self);
        vehicles::save(self);
        maps::save();
        // Player views before locking the world (lock order: player -> world).
        let views = self.views();
        let (age, day_time) = (self.age.load(Ordering::Relaxed), self.day_time.load(Ordering::Relaxed));
        let weather = self.weather.lock().unwrap().saved();
        let rules = self.rules.lock().unwrap().saved();
        let level_border = {
            let b = self.border.lock().unwrap();
            crate::world::anvil::LevelBorder { center: b.center, size: b.size, target: b.target, lerp_ms: b.lerp_ticks * 50, safe_zone: b.safe_zone, damage_per_block: b.damage_per_block, warning_blocks: b.warning_blocks, warning_time: b.warning_time }
        };
        let dragon = {
            let f = self.dragon.lock().unwrap();
            (f.killed, f.previously_killed)
        };
        let mut total = 0;
        for dim in Dimension::ALL {
            // Serialized with the world locked, but written to disk without holding the lock: the
            // game does not freeze while hundreds of chunks are saved.
            let (dir, dirty) = self.world(dim).lock().unwrap().take_dirty();
            let keys: Vec<(i32, i32)> = dirty.iter().map(|d| d.0).collect();
            let written = if dirty.is_empty() {
                Ok(())
            } else {
                let nbt = dirty.into_iter().map(|((x, z), c)| ((x, z), crate::world::anvil::chunk_to_nbt(&c, x, z))).collect();
                crate::world::anvil::write_chunks(&dir, nbt)
            };
            let mut world = self.world(dim).lock().unwrap();
            if let Err(e) = written {
                world.mark_dirty(&keys);
                return Err(e);
            }
            total += keys.len();
            if dim == Dimension::Overworld {
                crate::world::anvil::write_level(
                    world.dir(),
                    "Bambietta",
                    self.config.generation.options.seed,
                    self.spawn,
                    age,
                    day_time,
                    self.config.game_mode,
                    self.config.difficulty,
                    dragon,
                    weather,
                    &rules,
                    &level_border,
                )?;
            }
            // Free chunks far from every player in that dimension.
            world.unload_unused(|x, z| views.iter().any(|&(d, (cx, cz), r)| d == dim && (x - cx).abs() <= r && (z - cz).abs() <= r));
        }
        if total > 0 {
            let (loaded, bytes) = Dimension::ALL.iter().fold((0, 0), |(n, b), d| {
                let w = self.world(*d).lock().unwrap();
                (n + w.loaded_chunks(), b + w.memory())
            });
            info!("mundo guardado ({total} chunks, {loaded} en memoria, ~{} MB)", bytes / (1 << 20));
        }
        Ok(())
    }

    /// What each player sees: (dimension, center chunk, radius with a margin of 2).
    fn views(&self) -> Vec<(Dimension, (i32, i32), i32)> {
        self.online()
            .iter()
            .map(|p| {
                let s = p.state.lock().unwrap();
                (p.dim(), s.center, s.view_distance + 2)
            })
            .collect()
    }

    /// Every 5 s: unload chunks nobody sees (modified ones wait for the
    /// save) and drop caches of the surplus ones.
    fn unload_far_chunks(&self) {
        let views = self.views();
        for dim in Dimension::ALL {
            let near = |x: i32, z: i32| views.iter().any(|&(d, (cx, cz), r)| d == dim && (x - cx).abs() <= r && (z - cz).abs() <= r);
            let mut world = self.world(dim).lock().unwrap();
            world.unload_unused(near);
            world.drop_caches_outside(near);
        }
    }

    /// Tick loop (20 TPS): world time.
    pub async fn tick_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_millis(50));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let age = self.age.fetch_add(1, Ordering::Relaxed) + 1;
            let time = if self.rule("doDaylightCycle") { self.day_time.fetch_add(1, Ordering::Relaxed) + 1 } else { self.day_time.load(Ordering::Relaxed) };
            if age % 20 == 0 {
                self.broadcast(&update_time(age, time), None);
            }
            // Each part is timed: if a tick exceeds 100 ms, report which part was slow.
            let start = std::time::Instant::now();
            let mut parts: Vec<(&str, u128)> = Vec::new();
            let mut lap = std::time::Instant::now();
            let mut mark = |name: &'static str| {
                parts.push((name, lap.elapsed().as_millis()));
                lap = std::time::Instant::now();
            };
            entities::tick(&self, age);
            beds::tick(&self);
            if age % 2 == 0 {
                tracker::tick(&self);
            }
            mark("entidades");
            containers::tick_furnaces(&self);
            hoppers::tick(&self);
            fluids::tick(&self, age);
            mark("fluidos");
            falling::tick(&self);
            beacons::tick(&self, age);
            gadgets::tick(&self);
            decor::tick(&self);
            if age % 10 == 3 {
                blockmech::footsteps(&self);
            }
            redstone::tick(&self, age);
            fire::tick(&self, age);
            weather::tick(&self, age);
            border::tick(&self, age);
            farming::tick(&self);
            if age % 20 == 5 {
                advancements::tick(&self);
            }
            spawners::tick(&self);
            eyes::tick(&self);
            mark("redstone/fuego/clima");
            mobs::tick(&self, age);
            projectiles::tick(&self);
            fishing::tick(&self);
            vehicles::tick(&self);
            mark("mobs");
            maps::tick(&self, age);
            if age % 20 == 0 {
                villagers::restock(&self);
            }
            if age % 100 == 50 {
                self.unload_far_chunks();
            }
            mark("mapas");
            for p in self.online() {
                health::tick(&self, &p, age);
            }
            mark("jugadores");
            let total = start.elapsed().as_millis();
            if total > 100 {
                let detail: Vec<String> = parts.iter().filter(|p| p.1 > 5).map(|(n, ms)| format!("{n} {ms} ms")).collect();
                tracing::warn!("tick lento: {total} ms ({})", detail.join(", "));
            }
        }
    }

    pub async fn autosave_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.tick().await;
        loop {
            interval.tick().await;
            let s = self.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || s.save()).await.unwrap() {
                error!("error al guardar: {e:#}");
            }
        }
    }

    pub fn shutdown(&self) {
        for p in self.online() {
            p.kick("§cServidor apagado");
        }
        if let Err(e) = self.save() {
            error!("error al guardar: {e:#}");
        }
    }
}

impl Player {
    pub fn dim(&self) -> Dimension {
        Dimension::ALL[self.dim_index.load(Ordering::Relaxed) as usize]
    }

    pub fn set_dim(&self, dim: Dimension) {
        self.dim_index.store(dim.index() as u8, Ordering::Relaxed);
    }

    pub fn send(&self, body: Vec<u8>) {
        let _ = self.tx.send(Out::Packet(body));
    }

    pub fn message(&self, msg: &str) {
        self.send(system_chat(msg));
    }

    pub fn peer_ip(&self) -> String {
        self.peer.ip().to_string()
    }

    pub fn kick(&self, reason: &str) {
        let mut p = packet(id::play::KICK_DISCONNECT);
        text(reason).write_network(&mut p);
        self.send(p);
        let _ = self.tx.send(Out::Close);
        self.closed.notify_one();
    }

    pub fn is_op(&self, server: &Server) -> bool {
        server.config.everybody_op || server.admin.lock().unwrap().is_op(&format_uuid(self.uuid))
    }
}

pub fn system_chat(msg: &str) -> Vec<u8> {
    let mut p = packet(id::play::SYSTEM_CHAT);
    text(msg).write_network(&mut p);
    p.bool(false);
    p
}

pub fn update_time(age: i64, time: i64) -> Vec<u8> {
    let mut p = packet(id::play::UPDATE_TIME);
    p.i64(age).i64(time);
    p
}

pub fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

/// Offline UUID like vanilla: MD5("OfflinePlayer:<name>") with version 3.
pub fn offline_uuid(name: &str) -> u128 {
    use md5::{Digest, Md5};
    let mut h: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    u128::from_be_bytes(h)
}

pub fn format_uuid(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

#[cfg(test)]
mod tests {
    #[test]
    fn offline_uuid_matches_vanilla() {
        // Known value: offline player "Notch".
        assert_eq!(super::format_uuid(super::offline_uuid("Notch")), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }
}

impl Server {
    /// A boolean game rule.
    pub fn rule(&self, name: &str) -> bool {
        self.rules.lock().unwrap().bool(name)
    }

    /// A numeric game rule.
    pub fn rule_int(&self, name: &str) -> i32 {
        self.rules.lock().unwrap().int(name)
    }
}

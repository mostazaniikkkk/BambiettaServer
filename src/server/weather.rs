//! Weather in the Overworld, like vanilla: rain and thunderstorms that come and go on their own
//! (saved in `level.dat`), lightning during storms (damage, fire, charged creepers, pigs into
//! zombified piglins), rain putting out fires and burning entities, and snow piling up and
//! water freezing in cold biomes.

use super::{blocks, entities, health, mobs, sound, Player, Server};
use crate::data;
use crate::protocol::{id, packet, BlockPos, Writer};
use crate::world::{Dimension, World};
use std::sync::Arc;

/// Game events of the "change game state" packet.
const END_RAIN: u8 = 1;
const BEGIN_RAIN: u8 = 2;
const RAIN_LEVEL: u8 = 7;
const THUNDER_LEVEL: u8 = 8;
/// Weather sound category.
const WEATHER: i32 = 3;

pub struct Weather {
    pub raining: bool,
    pub thundering: bool,
    /// Ticks until rain (or thunder) toggles.
    pub rain_time: i32,
    pub thunder_time: i32,
    /// Ticks of forced clear weather left (`/weather clear`).
    pub clear_time: i32,
    rain_level: f32,
    thunder_level: f32,
    /// Lightning bolts on screen: (eid, ticks left).
    bolts: Vec<(i32, u32)>,
}

fn between(lo: i32, hi: i32) -> i32 {
    lo + (entities::random() * (hi - lo) as f32) as i32
}

impl Weather {
    pub fn new(raining: bool, rain_time: i32, thundering: bool, thunder_time: i32, clear_time: i32) -> Self {
        Self {
            raining,
            thundering,
            rain_time: if rain_time > 0 { rain_time } else { between(12000, 180000) },
            thunder_time: if thunder_time > 0 { thunder_time } else { between(12000, 180000) },
            clear_time,
            rain_level: if raining { 1.0 } else { 0.0 },
            thunder_level: if raining && thundering { 1.0 } else { 0.0 },
            bolts: Vec::new(),
        }
    }

    /// Raining (as the client sees it).
    pub fn is_raining(&self) -> bool {
        self.rain_level > 0.2
    }

    /// A thunderstorm: lightning can strike.
    pub fn is_thundering(&self) -> bool {
        self.rain_level * self.thunder_level > 0.9
    }

    /// `/weather`: clear, rain or thunder for `ticks` (or a random time).
    pub fn set(&mut self, kind: &str, ticks: Option<i32>) {
        match kind {
            "clear" => {
                self.clear_time = ticks.unwrap_or(between(12000, 180000));
                self.raining = false;
                self.thundering = false;
            }
            "rain" => {
                self.clear_time = 0;
                self.raining = true;
                self.thundering = false;
                self.rain_time = ticks.unwrap_or(between(12000, 24000));
            }
            _ => {
                self.clear_time = 0;
                self.raining = true;
                self.thundering = true;
                let t = ticks.unwrap_or(between(3600, 15600));
                self.rain_time = t;
                self.thunder_time = t;
            }
        }
    }

    /// Sleeping through the night also ends the rain.
    pub fn clear_after_sleep(&mut self) {
        self.raining = false;
        self.thundering = false;
        self.rain_time = between(12000, 180000);
        self.thunder_time = between(12000, 180000);
    }

    /// For `level.dat`: (raining, rainTime, thundering, thunderTime, clearWeatherTime).
    pub fn saved(&self) -> (bool, i32, bool, i32, i32) {
        (self.raining, self.rain_time, self.thundering, self.thunder_time, self.clear_time)
    }
}

fn game_event(event: u8, value: f32) -> Vec<u8> {
    let mut p = packet(id::play::GAME_STATE_CHANGE);
    p.u8(event).f32(value);
    p
}

/// The weather a player should see on joining or arriving in a dimension.
pub fn send_state(server: &Server, player: &Player) {
    let w = server.weather.lock().unwrap();
    if player.dim() == Dimension::Overworld && w.is_raining() {
        player.send(game_event(BEGIN_RAIN, 0.0));
        player.send(game_event(RAIN_LEVEL, w.rain_level));
        player.send(game_event(THUNDER_LEVEL, w.thunder_level));
    } else {
        player.send(game_event(END_RAIN, 0.0));
        player.send(game_event(RAIN_LEVEL, 0.0));
        player.send(game_event(THUNDER_LEVEL, 0.0));
    }
}

/// Snow instead of rain in cold biomes; nothing in dry ones.
enum Precipitation {
    None,
    Rain,
    Snow,
}

fn precipitation(world: &World, x: i32, z: i32) -> Precipitation {
    let b = world.generator().biome_name(x, z);
    if b.contains("snowy") || b.contains("frozen") || matches!(b, "ice_spikes" | "grove" | "jagged_peaks" | "frozen_peaks") {
        Precipitation::Snow
    } else if b.contains("desert") || b.contains("savanna") || b.contains("badlands") {
        Precipitation::None
    } else {
        Precipitation::Rain
    }
}

/// Is it raining on this spot? (Overworld, raining, open sky above and a biome where it rains.)
pub fn rain_at(server: &Server, world: &mut World, dim: Dimension, p: [f64; 3]) -> bool {
    if dim != Dimension::Overworld || !server.weather.lock().unwrap().is_raining() {
        return false;
    }
    let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
    if !matches!(precipitation(world, x, z), Precipitation::Rain) {
        return false;
    }
    // Nothing above it but what it stands in (grass, the fire block itself).
    world.top_y(x, z) <= p[1].floor() as i32
}

/// Every tick.
pub fn tick(server: &Arc<Server>, age: i64) {
    let (events, thunder) = {
        let mut w = server.weather.lock().unwrap();
        let was_raining = w.is_raining();
        if !server.rule("doWeatherCycle") {
            // Frozen weather: the timers do not run.
        } else if w.clear_time > 0 {
            w.clear_time -= 1;
            w.raining = false;
            w.thundering = false;
        } else {
            w.thunder_time -= 1;
            if w.thunder_time <= 0 {
                w.thundering = !w.thundering;
                w.thunder_time = if w.thundering { between(3600, 15600) } else { between(12000, 180000) };
            }
            w.rain_time -= 1;
            if w.rain_time <= 0 {
                w.raining = !w.raining;
                w.rain_time = if w.raining { between(12000, 24000) } else { between(12000, 180000) };
            }
        }
        let (old_rain, old_thunder) = (w.rain_level, w.thunder_level);
        let step = |v: f32, up: bool| if up { (v + 0.01).min(1.0) } else { (v - 0.01).max(0.0) };
        w.rain_level = step(w.rain_level, w.raining);
        w.thunder_level = step(w.thunder_level, w.thundering);
        let mut events = Vec::new();
        if was_raining != w.is_raining() {
            events.push(game_event(if w.is_raining() { BEGIN_RAIN } else { END_RAIN }, 0.0));
        }
        if w.rain_level != old_rain {
            events.push(game_event(RAIN_LEVEL, w.rain_level));
        }
        if w.thunder_level != old_thunder {
            events.push(game_event(THUNDER_LEVEL, w.thunder_level));
        }
        // Bolts disappear after a second.
        let mut gone = Vec::new();
        w.bolts.retain_mut(|b| {
            b.1 = b.1.saturating_sub(1);
            if b.1 == 0 {
                gone.push(b.0);
            }
            b.1 > 0
        });
        for eid in gone {
            let mut d = packet(id::play::ENTITY_DESTROY);
            d.varint(1).varint(eid);
            events.push(d);
        }
        (events, w.is_thundering())
    };
    for e in &events {
        server.broadcast_dim(Dimension::Overworld, e, None);
    }

    let players: Vec<Arc<Player>> = server.online().into_iter().filter(|p| p.dim() == Dimension::Overworld).collect();
    if players.is_empty() {
        return;
    }
    // Lightning near each player now and then during a storm.
    if thunder {
        for p in &players {
            if entities::random() < 1.0 / 400.0 {
                let (x, z) = {
                    let st = p.state.lock().unwrap();
                    (st.x, st.z)
                };
                let a = entities::random() as f64 * std::f64::consts::TAU;
                let r = entities::random() as f64 * 48.0;
                strike(server, (x + a.cos() * r).floor() as i32, (z + a.sin() * r).floor() as i32);
            }
        }
    }
    // Rain puts out burning players.
    if age % 10 == 0 {
        for p in &players {
            let (pos, burning) = {
                let st = p.state.lock().unwrap();
                ([st.x, st.y, st.z], st.fire_ticks > 0)
            };
            if !burning {
                continue;
            }
            let wet = rain_at(server, &mut server.world(Dimension::Overworld).lock().unwrap(), Dimension::Overworld, pos);
            if wet {
                p.state.lock().unwrap().fire_ticks = 0;
                server.broadcast_tracked(p.eid, &super::play::metadata(p), true);
            }
        }
    }
    precipitate(server, &players);
}

/// Snow piles up and water freezes on random columns around the players (cold biomes).
fn precipitate(server: &Server, players: &[Arc<Player>]) {
    let snowing = server.weather.lock().unwrap().is_raining();
    let reg = data::get();
    let (water, ice, snow) = (reg.state("water"), reg.state("ice"), reg.state("snow"));
    let mut changes = Vec::new();
    // Positions first: the lock order is player -> world, never the other way round.
    let around: Vec<(i32, i32)> = players
        .iter()
        .map(|p| {
            let st = p.state.lock().unwrap();
            (st.x as i32, st.z as i32)
        })
        .collect();
    {
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        for (px, pz) in around {
            for _ in 0..4 {
                let x = px + ((entities::random() - 0.5) * 128.0) as i32;
                let z = pz + ((entities::random() - 0.5) * 128.0) as i32;
                if !world.is_loaded(x >> 4, z >> 4) || !matches!(precipitation(&world, x, z), Precipitation::Snow) {
                    continue;
                }
                let y = world.top_y(x, z);
                let top = BlockPos::new(x, y, z);
                let state = world.get_block(top);
                if state == water {
                    // Freezes from the edges inwards (like vanilla: needs a non-water neighbour).
                    let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| world.get_block(BlockPos::new(x + dx, y, z + dz)) != water);
                    if edge {
                        world.set_block(top, ice);
                        changes.push((top, ice));
                    }
                } else if snowing {
                    let above = BlockPos::new(x, y + 1, z);
                    let name = reg.block_of_state(state).name.as_str();
                    let full = crate::gameplay::get().shape(state).iter().any(|s| s[4] >= 1.0 && s[0] <= 0.0 && s[3] >= 1.0);
                    if reg.is_air(world.get_block(above)) && full && !matches!(name, "ice" | "packed_ice" | "blue_ice" | "barrier") {
                        world.set_block(above, snow);
                        changes.push((above, snow));
                    }
                }
            }
        }
    }
    for (p, s) in changes {
        server.broadcast_dim(Dimension::Overworld, &blocks::block_change(p, s), None);
    }
}

/// A lightning bolt at the top of column (x, z).
pub fn strike(server: &Arc<Server>, x: i32, z: i32) {
    let reg = data::get();
    let y = {
        let mut world = server.world(Dimension::Overworld).lock().unwrap();
        if !world.is_loaded(x >> 4, z >> 4) {
            return;
        }
        world.top_y(x, z) + 1
    };
    let pos = [x as f64 + 0.5, y as f64, z as f64 + 0.5];
    let eid = server.next_entity_id();
    let mut p = packet(id::play::SPAWN_ENTITY);
    let r = || entities::random().to_bits() as u128;
    p.varint(eid).uuid((r() << 96) | (r() << 64) | (r() << 32) | r()).varint(reg.entity("lightning_bolt"));
    p.f64(pos[0]).f64(pos[1]).f64(pos[2]).angle(0.0).angle(0.0).angle(0.0).varint(0);
    p.i16(0).i16(0).i16(0);
    server.broadcast_dim(Dimension::Overworld, &p, None);
    server.weather.lock().unwrap().bolts.push((eid, 20));
    sound::at(server, Dimension::Overworld, pos, "entity.lightning_bolt.thunder", WEATHER, 10000.0, 0.8 + entities::random() * 0.2);
    sound::at(server, Dimension::Overworld, pos, "entity.lightning_bolt.impact", WEATHER, 2.0, 0.5 + entities::random() * 0.2);
    // Sets fire where it lands (normal and hard, like vanilla).
    if server.config.difficulty >= 2 {
        super::fire::ignite(server, Dimension::Overworld, BlockPos::new(x, y, z));
    }
    // Players and mobs within 3 blocks: 5 damage and on fire.
    for pl in server.online().into_iter().filter(|p| p.dim() == Dimension::Overworld) {
        let near = {
            let st = pl.state.lock().unwrap();
            mobs::dist2(&[st.x, st.y, st.z], &pos) < 3.0 * 3.0
        };
        if near {
            health::damage(server, &pl, 5.0, "lightning_bolt", None);
            health::set_on_fire(server, &pl, 160);
        }
    }
    let hit: Vec<(i32, mobs::MobKind, [f64; 3])> = server
        .mobs
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.dim == Dimension::Overworld && m.death_ticks.is_none() && mobs::dist2(&m.pos, &pos) < 3.0 * 3.0)
        .map(|m| (m.eid, m.kind, m.pos))
        .collect();
    for (eid, kind, at) in hit {
        match kind {
            // Lightning charges creepers and turns pigs into zombified piglins.
            mobs::MobKind::Creeper => mobs::charge(server, eid),
            mobs::MobKind::Pig => {
                mobs::remove(server, eid);
                mobs::spawn(server, Dimension::Overworld, mobs::MobKind::ZombifiedPiglin, at);
                continue;
            }
            _ => {}
        }
        mobs::hurt(server, eid, 5.0, "lightning_bolt", None, None, 1);
    }
}

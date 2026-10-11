//! Play state: joining the world, validated movement, chunks, chat, inventory, combat
//! and player entities.

use super::anticheat::{self, Verdict};
use super::blocks::{self, PlaceRequest};
use super::session::Login;
use super::{commands, entities, health, playerdata, Player, PlayerState, Server};
use crate::data;
use crate::item::{read_slot, write_slot, Slot};
use crate::net::Conn;
use crate::protocol::{id, packet, text, Reader, Writer};
use anyhow::Result;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::{debug, info, warn};

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
static TELEPORT_ID: AtomicI32 = AtomicI32::new(1);

pub async fn run(server: Arc<Server>, mut conn: Conn, login: Login) -> Result<()> {
    let spawn = server.spawn;
    let mut state = PlayerState::new(spawn.x as f64 + 0.5, spawn.y as f64, spawn.z as f64 + 0.5, server.config.game_mode.min(3), login.view_distance);
    let mut dim = crate::world::Dimension::Overworld;
    if let Some(data) = playerdata::load(&server, login.uuid) {
        dim = playerdata::apply(data, &mut state);
    }
    let player = Arc::new(Player {
        eid: server.next_entity_id(),
        uuid: login.uuid,
        name: login.name,
        transport: conn.transport,
        tx: conn.tx.clone(),
        closed: Notify::new(),
        state: Mutex::new(state),
        dim_index: std::sync::atomic::AtomicU8::new(dim.index() as u8),
        tracked: Mutex::new(Default::default()),
    });
    if server.players.read().unwrap().values().any(|p| p.name.eq_ignore_ascii_case(&player.name)) {
        player.kick("§cYa hay un jugador conectado con ese nombre");
        return Ok(());
    }
    info!("{} entró ({} desde {}, eid {})", player.name, conn.transport, conn.peer, player.eid);

    if !join(&server, &player).await {
        player.kick("§cYa hay un jugador conectado con ese nombre");
        return Ok(());
    }
    conn.reader.max_per_second = server.config.max_packets_per_second;
    let result = play_loop(&server, &player, &mut conn).await;
    if let Err(e) = &result {
        if e.downcast_ref::<crate::net::RateLimited>().is_some() {
            warn!("{} expulsado: demasiados paquetes por segundo", player.name);
            player.kick("§cDemasiados paquetes");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    // On leave: whatever was in the crafting grid or on the cursor goes back to the inventory before saving.
    super::vehicles::dismount(&server, &player, false);
    let leftovers = player.state.lock().unwrap().inv.close();
    for item in leftovers {
        entities::throw_from(&server, &player, item);
    }
    server.players.write().unwrap().remove(&player.eid);
    playerdata::save(&server, &player);
    let mut rm = packet(id::play::PLAYER_REMOVE);
    rm.varint(1).uuid(player.uuid);
    server.broadcast(&rm, None);
    let mut destroy = packet(id::play::ENTITY_DESTROY);
    destroy.varint(1).varint(player.eid);
    server.broadcast(&destroy, None);
    server.broadcast_message(&format!("§e{} salió del juego", player.name));
    let _ = conn.tx.send(crate::net::Out::Close);
    if let Err(e) = &result {
        debug!("{}: {e:#}", player.name);
    }
    result
}

/// Sends the join sequence. Returns false if someone with the same name got in meanwhile.
async fn join(server: &Arc<Server>, player: &Arc<Player>) -> bool {
    let cfg = &server.config;
    let (game_mode, vd, x, y, z, yaw, pitch, held) = {
        let s = player.state.lock().unwrap();
        (s.game_mode, s.view_distance, s.x, s.y, s.z, s.yaw, s.pitch, s.held_slot)
    };

    let mut p = packet(id::play::LOGIN);
    p.i32(player.eid).bool(false);
    p.varint(3).string("minecraft:overworld").string("minecraft:the_nether").string("minecraft:the_end");
    p.varint(cfg.max_players as i32).varint(vd).varint(vd);
    p.bool(false).bool(true).bool(false);
    let dim = player.dim();
    p.string(dim.name()).string(dim.name());
    p.i64(cfg.generation.options.seed).u8(game_mode).i8(-1);
    p.bool(false).bool(dim == crate::world::Dimension::Overworld && matches!(cfg.generation.name.as_str(), "flat" | "superflat"));
    p.bool(false).varint(0);
    player.send(p);
    // Only now does it join the player list: before the login packet, a broadcast (a block
    // change, a mob) would reach a client that is not in the game yet.
    {
        let mut players = server.players.write().unwrap();
        if players.values().any(|p| p.name.eq_ignore_ascii_case(&player.name)) {
            return false;
        }
        players.insert(player.eid, player.clone());
    }

    let mut p = packet(id::play::DIFFICULTY);
    p.u8(cfg.difficulty.min(3)).bool(false);
    player.send(p);
    player.send(abilities(game_mode));
    let mut p = packet(id::play::HELD_ITEM_SLOT);
    p.i8(held as i8);
    player.send(p);
    player.send(commands::declare_commands());
    player.send(super::stations::declare_recipes());
    {
        let mut s = player.state.lock().unwrap();
        player.send(health::health_packet(s.health, s.food, s.saturation));
        let inv = s.inv.window_items();
        player.send(inv);
    }

    let mut p = packet(id::play::PLAYERLIST_HEADER);
    text(&cfg.player_list_text.header).write_network(&mut p);
    text(&cfg.player_list_text.footer).write_network(&mut p);
    player.send(p);

    // Player list: everyone for the newcomer, the newcomer for everyone.
    let others: Vec<Arc<Player>> = server.online().into_iter().filter(|o| o.eid != player.eid).collect();
    let mut all = others.clone();
    all.push(player.clone());
    player.send(player_info_add(&all));
    server.broadcast(&player_info_add(std::slice::from_ref(player)), Some(player.eid));

    let mut p = packet(id::play::SPAWN_POSITION);
    p.position(server.spawn).f32(0.0);
    player.send(p);
    player.send(super::update_time(server.age.load(Ordering::Relaxed), server.day_time.load(Ordering::Relaxed)));

    // 1.20.3+: without this event the client stays stuck on "Loading terrain".
    let mut p = packet(id::play::GAME_STATE_CHANGE);
    p.u8(13).f32(0.0);
    player.send(p);
    // Like vanilla, chunks are sent on the tick after login: some clients
    // (e.g. mineflayer) initialize the world asynchronously when they receive `login`.
    // Like vanilla: position first, then chunks (which go out on the following ticks).
    send_position(player, x, y, z, yaw, pitch);
    tokio::time::sleep(Duration::from_millis(50)).await;
    update_view(server, player, true).await;

    // Other players' entities for the newcomer, and the newcomer for the others.
    // Entities (players, mobs, items) are sent by distance-based tracking.
    {
        let st = player.state.lock().unwrap();
        player.send(super::experience::packet_for(&st));
    }
    super::effects::resend(player);
    super::weather::send_state(server, player);
    server.broadcast_message(&format!("§e{} entró al juego", player.name));
    player.message("§dBienvenido a Bambietta§7 · usa §e/help");
    true
}

async fn play_loop(server: &Arc<Server>, player: &Arc<Player>, conn: &mut Conn) -> Result<()> {
    let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
    let mut pending: Option<(i64, Instant)> = None;
    let mut last_ack = Instant::now();
    let timeout = Duration::from_millis(server.config.kick_timeout);
    loop {
        tokio::select! {
            body = conn.reader.next() => {
                let Some(body) = body? else { return Ok(()) };
                if let Err(e) = handle_packet(server, player, &body, &mut pending, &mut last_ack).await {
                    warn!("{}: paquete inválido: {e:#}", player.name);
                }
            }
            _ = keep_alive.tick() => {
                if last_ack.elapsed() > timeout {
                    player.kick("§cTiempo de espera agotado");
                    return Ok(());
                }
                let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
                let mut p = packet(id::play::KEEP_ALIVE);
                p.i64(id);
                player.send(p);
                pending = Some((id, Instant::now()));
            }
            _ = player.closed.notified() => return Ok(()),
        }
    }
}

async fn handle_packet(
    server: &Arc<Server>,
    player: &Arc<Player>,
    body: &[u8],
    pending: &mut Option<(i64, Instant)>,
    last_ack: &mut Instant,
) -> Result<()> {
    use id::play_in::*;
    let mut r = Reader::new(body);
    match r.varint()? {
        KEEP_ALIVE => {
            let id = r.i64()?;
            if let Some((expected, sent)) = *pending {
                if id == expected {
                    let ping = sent.elapsed().as_millis() as i32;
                    player.state.lock().unwrap().ping = ping;
                    *pending = None;
                    let mut p = packet(id::play::PLAYER_INFO);
                    p.u8(0x10).varint(1).uuid(player.uuid).varint(ping);
                    server.broadcast(&p, None);
                }
            }
            *last_ack = Instant::now();
        }
        PING_REQUEST => {
            let mut p = packet(id::play::PING_RESPONSE);
            p.i64(r.i64()?);
            player.send(p);
        }
        TELEPORT_CONFIRM => {
            let tp = r.varint()?;
            {
                let mut st = player.state.lock().unwrap();
                if st.ac.pending_teleport == Some(tp) {
                    st.ac.pending_teleport = None;
                }
            }
            update_view(server, player, false).await;
        }
        POSITION => {
            let (x, y, z) = (r.f64()?, r.f64()?, r.f64()?);
            moved(server, player, Some([x, y, z]), None, r.bool()?).await;
        }
        POSITION_LOOK => {
            let (x, y, z) = (r.f64()?, r.f64()?, r.f64()?);
            let (yaw, pitch) = (r.f32()?, r.f32()?);
            moved(server, player, Some([x, y, z]), Some((yaw, pitch)), r.bool()?).await;
        }
        LOOK => {
            let (yaw, pitch) = (r.f32()?, r.f32()?);
            moved(server, player, None, Some((yaw, pitch)), r.bool()?).await;
        }
        FLYING => {}
        CHAT_MESSAGE => {
            let msg = r.string(256)?;
            if !chat_allowed(server, player) {
                return Ok(());
            }
            let msg: String = msg.chars().filter(|c| *c != '§' && !c.is_control()).collect();
            if !msg.trim().is_empty() {
                server.broadcast_message(&format!("§7<§f{}§7>§r {msg}", player.name));
            }
        }
        CHAT_COMMAND => {
            let cmd = r.string(256)?;
            if chat_allowed(server, player) {
                commands::execute(server, player, &cmd);
            }
        }
        CLIENT_COMMAND => {
            if r.varint()? == 0 {
                health::respawn(server, player).await;
            }
        }
        SETTINGS => {
            let _locale = r.string(16)?;
            let vd = (r.i8()? as i32).clamp(2, server.config.view_distance);
            let changed = {
                let mut s = player.state.lock().unwrap();
                std::mem::replace(&mut s.view_distance, vd) != vd
            };
            if changed {
                update_view(server, player, true).await;
            }
        }
        BLOCK_DIG => {
            let status = r.varint()?;
            let pos = r.position()?;
            let face = r.i8()?;
            let seq = r.varint()?;
            // Put out fire with a punch (it sits on the face of the clicked block).
            if status == 0 && blocks::extinguish(server, player, pos, face as i32) {
                blocks::ack(player, seq);
                return Ok(());
            }
            blocks::handle_dig(server, player, status, pos);
            if matches!(status, 0..=2) {
                blocks::ack(player, seq);
            }
        }
        BLOCK_PLACE => {
            let hand = r.varint()?;
            let location = r.position()?;
            let face = r.varint()?;
            let (_cx, cursor_y, _cz) = (r.f32()?, r.f32()?, r.f32()?);
            let _inside = r.bool()?;
            let seq = r.varint()?;
            blocks::place(server, player, PlaceRequest { hand, location, face, cursor_y });
            blocks::ack(player, seq);
        }
        USE_ITEM => {
            let hand = r.varint()?;
            let seq = r.varint()?;
            let used = super::fluids::use_bucket(server, player, hand)
                || super::signs::read_book(player, hand)
                || super::maps::use_empty_map(server, player, hand)
                || super::vehicles::use_boat(server, player, hand)
                || super::eyes::throw(server, player, hand)
                || super::projectiles::throw_held(server, player, hand)
                || super::fishing::use_rod(server, player, hand)
                || super::ranged::start(server, player, hand);
            if !used {
                health::start_using(server, player, hand);
            }
            blocks::ack(player, seq);
        }
        EDIT_BOOK => {
            let slot = r.varint()?;
            let n = r.varint()?.clamp(0, 100);
            let mut pages = Vec::with_capacity(n as usize);
            for _ in 0..n {
                pages.push(r.string(8192)?);
            }
            let title = if r.bool()? { Some(r.string(128)?) } else { None };
            super::signs::edit_book(player, slot, pages, title);
        }
        UPDATE_SIGN => {
            let pos = r.position()?;
            let front = r.bool()?;
            let lines = [r.string(384)?, r.string(384)?, r.string(384)?, r.string(384)?];
            super::signs::update(server, player, pos, front, lines);
        }
        NAME_ITEM => {
            let name = r.string(32767)?;
            super::stations::rename(player, &name);
        }
        SELECT_TRADE => {
            let slot = r.varint()?;
            super::villagers::select(player, slot);
        }
        VEHICLE_MOVE => {
            let pos = [r.f64()?, r.f64()?, r.f64()?];
            let yaw = r.f32()?;
            super::vehicles::client_moved(server, player, pos, yaw);
        }
        STEER_BOAT => {
            let (left, right) = (r.bool()?, r.bool()?);
            super::vehicles::paddles(server, player, left, right);
        }
        STEER_VEHICLE => {
            let _sideways = r.f32()?;
            let forward = r.f32()?;
            let flags = r.u8()?;
            super::vehicles::steer(server, player, forward, flags);
        }
        USE_ENTITY => {
            let target = r.varint()?;
            match r.varint()? {
                1 => attack(server, player, target),
                0 => super::mobs::interact(server, player, target, r.varint()?),
                _ => {
                    let _ = (r.f32()?, r.f32()?, r.f32()?);
                    super::mobs::interact(server, player, target, r.varint()?);
                }
            }
        }
        ENCHANT_ITEM => {
            let _window = r.i8()?;
            let button = r.i8()?;
            let kind = player.state.lock().unwrap().inv.window_kind();
            match kind {
                Some(super::inventory::WindowKind::Enchanting) if (0..3).contains(&button) => super::enchanting::enchant(server, player, button as usize),
                Some(_) => super::stations::button(player, button as i32),
                None => {}
            }
        }
        WINDOW_CLICK => window_click(server, player, &mut r)?,
        CLOSE_WINDOW => {
            let _window = r.u8()?;
            let (leftovers, closed, merchant) = {
                let mut st = player.state.lock().unwrap();
                let closed = st.inv.window.as_ref().map(|w| w.pos);
                let merchant = st.inv.window.as_ref().filter(|w| w.kind == super::inventory::WindowKind::Merchant).map(|w| w.station.merchant);
                let l = st.inv.close();
                let p = st.inv.window_items();
                player.send(p);
                (l, closed, merchant)
            };
            for item in leftovers {
                entities::throw_from(server, player, item);
            }
            // The villager starts moving again when trading is closed.
            if let Some(e) = merchant {
                if let Some(m) = server.mobs.lock().unwrap().get_mut(&e) {
                    m.busy = 0;
                }
            }
            if let Some(pos) = closed {
                super::containers::update_lid(server, player.dim(), pos);
            }
        }
        HELD_ITEM_SLOT => {
            let slot = r.i16()?;
            if (0..9).contains(&slot) {
                {
                    let mut st = player.state.lock().unwrap();
                    st.held_slot = slot as usize;
                    st.eating = None;
                    st.digging = None;
                }
                super::ranged::cancel(server, player);
                server.broadcast_tracked(player.eid, &equipment(player), false);
            }
        }
        SET_CREATIVE_SLOT => {
            let slot = r.i16()?;
            let item = read_slot(&mut r)?;
            let held = {
                let mut s = player.state.lock().unwrap();
                if s.game_mode != 1 {
                    return Ok(());
                }
                // -1: drop from the creative inventory.
                if slot == -1 {
                    drop(s);
                    if let Some(i) = item {
                        entities::throw_from(server, player, i);
                    }
                    return Ok(());
                }
                if !(0..46).contains(&slot) {
                    return Ok(());
                }
                s.inv.slots[slot as usize] = item.clone();
                // Acknowledgement (like vanilla): the client expects the set_slot back.
                let mut p = packet(id::play::SET_SLOT);
                p.i8(0).varint(s.inv.state_id).i16(slot);
                write_slot(&mut p, &item);
                player.send(p);
                slot as usize == super::inventory::HOTBAR + s.held_slot || slot as usize == super::inventory::OFFHAND || (5..=8).contains(&slot)
            };
            if held {
                server.broadcast_tracked(player.eid, &equipment(player), false);
            }
        }
        ARM_ANIMATION => {
            let hand = r.varint()?;
            let mut p = packet(id::play::ANIMATION);
            p.varint(player.eid).u8(if hand == 1 { 3 } else { 0 });
            server.broadcast_tracked(player.eid, &p, false);
        }
        ENTITY_ACTION => {
            let _eid = r.varint()?;
            let action = r.varint()?;
            {
                let mut s = player.state.lock().unwrap();
                if action == 2 {
                    drop(s);
                    super::beds::wake(server, player);
                    return Ok(());
                }
                match action {
                    0 => s.sneaking = true,
                    1 => s.sneaking = false,
                    3 => s.sprinting = true,
                    4 => s.sprinting = false,
                    _ => return Ok(()),
                }
            }
            server.broadcast_tracked(player.eid, &metadata(player), false);
        }
        other => debug!("{}: paquete 0x{other:02x} ignorado", player.name),
    }
    Ok(())
}

/// Chat anti-spam (like vanilla, ops are exempt): more than 10 messages in 5 s are
/// ignored with a warning; more than 25 gets the player kicked.
fn chat_allowed(server: &Server, player: &Arc<Player>) -> bool {
    if player.is_op(server) {
        return true;
    }
    let n = {
        let mut st = player.state.lock().unwrap();
        let now = Instant::now();
        st.chat_times.retain(|t| now.duration_since(*t) < Duration::from_secs(5));
        st.chat_times.push(now);
        st.chat_times.len()
    };
    if n > 25 {
        player.kick("§cSpam en el chat");
        return false;
    }
    if n > 10 {
        player.message("§cMás despacio, por favor");
        return false;
    }
    true
}

fn window_click(server: &Arc<Server>, player: &Arc<Player>, r: &mut Reader) -> Result<()> {
    let window = r.u8()?;
    let state_id = r.varint()?;
    let slot = r.i16()?;
    let button = r.i8()?;
    let mode = r.varint()?;
    let n = r.varint()?.clamp(0, 128);
    let mut changed: Vec<(i16, Slot)> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        changed.push((r.i16()?, read_slot(r)?));
    }
    let cursor = read_slot(r)?;

    let (drops, armor_changed, smelted, taken) = {
        let mut st = player.state.lock().unwrap();
        if st.dead || window != st.inv.window_id() {
            let p = st.inv.window_items();
            player.send(p);
            return Ok(());
        }
        let creative = st.game_mode == 1;
        (st.inv.levels, st.inv.creative) = (st.xp_level, creative);
        // Like vanilla: if the client had a stale state, it is sent the full window.
        let stale = state_id != st.inv.state_id;
        let armor_before: Vec<Slot> = st.inv.slots[5..=8].to_vec();
        let before = st.inv.snapshot();
        let out = st.inv.click(creative, slot, button, mode);
        if !stale && st.inv.matches_prediction(&changed, &cursor) {
            // Like vanilla: what the client predicted correctly is not resent; what changed and it
            // did not predict is (crafting and workstation results, shift-clicks...).
            let after = st.inv.snapshot();
            let same = |a: &Slot, b: &Slot| match (a, b) {
                (None, None) => true,
                (Some(x), Some(y)) => x.id == y.id && x.count == y.count && x.nbt == y.nbt,
                _ => false,
            };
            let mut unpredicted: Vec<i16> = (0..after.len())
                .filter(|&i| !same(&before.get(i).cloned().flatten(), &after[i]))
                .map(|i| i as i16)
                .filter(|i| !changed.iter().any(|(s, item)| s == i && same(item, &after[*i as usize])))
                .collect();
            // Results the client assumed empty and the server filled again.
            for (s, item) in &changed {
                if after.get(*s as usize).is_some_and(|a| !same(item, a)) && !unpredicted.contains(s) {
                    unpredicted.push(*s);
                }
            }
            // Like vanilla, with a crafting grid the result is always sent
            // (some clients expect it even if it did not change).
            if matches!(st.inv.window_kind(), None | Some(super::inventory::WindowKind::Crafting)) && !unpredicted.contains(&0) {
                unpredicted.push(0);
            }
            for i in unpredicted {
                let p = st.inv.window_slot_packet(i);
                player.send(p);
            }
        } else {
            if !stale {
            let show = |s: &Slot| s.as_ref().map_or("-".to_string(), |i| format!("{}x{}", i.name(), i.count));
            debug!(
                "{}: inventario desincronizado (modo {mode}, botón {button}, slot {slot}): cliente {:?} cursor {}; servidor cursor {}, slots {:?}",
                player.name,
                changed.iter().map(|(s, i)| format!("{s}={}", show(i))).collect::<Vec<_>>(),
                show(&cursor),
                show(&st.inv.cursor),
                changed.iter().map(|(s, _)| format!("{s}={}", show(&st.inv.slot_at(*s)))).collect::<Vec<_>>()
            );
            }
            let p = st.inv.window_items();
            player.send(p);
        }
        (out.drops, st.inv.slots[5..=8] != armor_before[..], out.smelted_taken, out.taken)
    };
    for item in drops {
        entities::throw_from(server, player, item);
    }
    super::containers::after_click(server, player, smelted);
    super::stations::after_click(server, player, taken);
    super::enchanting::refresh(server, player);
    if armor_changed {
        server.broadcast_tracked(player.eid, &equipment(player), false);
    }
    Ok(())
}

async fn moved(server: &Arc<Server>, player: &Arc<Player>, pos: Option<[f64; 3]>, look: Option<(f32, f32)>, _claimed_ground: bool) {
    let mut fall_damage = 0.0f32;
    let mut landed = 0.0f64;
    let mut setback = None;
    let (x, y, z, yaw, pitch, on_ground) = {
        let mut st = player.state.lock().unwrap();
        if let Some((yaw, pitch)) = look {
            (st.yaw, st.pitch) = (yaw, pitch);
        }
        // Until it confirms a server teleport, its positions are ignored.
        if st.ac.pending_teleport.is_none() && !st.dead {
            if let Some(new) = pos {
                if !new.iter().all(|v| v.is_finite()) || new[0].abs() > 3.0e7 || new[2].abs() > 3.0e7 {
                    return;
                }
                let verdict = {
                    let mut world = server.world(player.dim()).lock().unwrap();
                    anticheat::check(&mut world, &mut st, new, server.config.anti_cheat)
                };
                match verdict {
                    Verdict::Setback(back, why) => {
                        debug!("{}: corrección de posición ({why})", player.name);
                        setback = Some(back);
                    }
                    Verdict::Accept(env) => {
                        let dy = new[1] - st.y;
                        let horizontal = ((new[0] - st.x).powi(2) + (new[2] - st.z).powi(2)).sqrt();
                        // Hunger: sprinting, swimming and jumping cause exhaustion.
                        if st.survival_like() {
                            if env.in_water {
                                st.exhaustion += 0.01 * horizontal as f32;
                            } else if st.sprinting {
                                st.exhaustion += 0.1 * horizontal as f32;
                            }
                            if st.on_ground && !env.ground && dy > 0.0 {
                                st.exhaustion += if st.sprinting { 0.2 } else { 0.05 };
                            }
                        }
                        // Fall damage with the ground computed on the server.
                        if env.in_water || env.climbing || !st.survival_like() {
                            st.fall_distance = 0.0;
                        } else if env.ground {
                            if st.fall_distance > 0.5 {
                                debug!("{}: aterriza tras caer {:.2} bloques (factor {})", player.name, st.fall_distance, env.fall_factor);
                            }
                            if st.fall_distance > 3.0 {
                                fall_damage = ((st.fall_distance - 3.0).ceil() * env.fall_factor) as f32;
                            }
                            landed = st.fall_distance;
                            st.fall_distance = 0.0;
                        } else if dy < 0.0 {
                            st.fall_distance -= dy;
                        }
                        [st.x, st.y, st.z] = new;
                        st.on_ground = env.ground;
                    }
                }
            }
        }
        (st.x, st.y, st.z, st.yaw, st.pitch, st.on_ground)
    };
    if let Some(back) = setback {
        teleport(server, player, back[0], back[1], back[2]);
        return;
    }
    if fall_damage > 0.0 {
        health::damage(server, player, fall_damage, "fall", None);
    }
    // Jumping or falling onto farmland tramples it.
    if landed > 0.5 {
        super::farming::trample(server, player, landed, [x, y, z]);
    }
    let mut p = packet(id::play::ENTITY_TELEPORT);
    p.varint(player.eid).f64(x).f64(y).f64(z).angle(yaw).angle(pitch).bool(on_ground);
    server.broadcast_tracked(player.eid, &p, false);
    if look.is_some() {
        let mut p = packet(id::play::ENTITY_HEAD_ROTATION);
        p.varint(player.eid).angle(yaw);
        server.broadcast_tracked(player.eid, &p, false);
    }
    if pos.is_some() {
        update_view(server, player, false).await;
    }
}

/// Data for the hit a player is about to deal.
pub struct Swing {
    pub yaw: f32,
    pub damage: f32,
    /// Knockback strength (0.4 base + 0.5 per Knockback level/sprint).
    pub knockback: f64,
    pub weapon: Option<crate::item::ItemStack>,
}

/// Prepares the hit: weapon damage with Sharpness and crits, knockback and weapon wear.
pub fn swing(server: &Arc<Server>, attacker: &Arc<Player>) -> Option<Swing> {
    let s = {
        let mut st = attacker.state.lock().unwrap();
        if st.dead || st.game_mode == 3 {
            return None;
        }
        st.exhaustion += 0.1;
        let weapon = st.inv.held(0, st.held_slot).clone();
        let crit = !st.on_ground && st.fall_distance > 0.0 && !st.sprinting;
        let sharp = weapon.as_ref().map_or(0, |w| w.enchantment("sharpness"));
        let mut damage = health::attack_damage(weapon.as_ref()) + if sharp > 0 { 0.5 * sharp as f32 + 0.5 } else { 0.0 };
        // Strength and Weakness.
        if let Some(a) = super::effects::level(&st, "strength") {
            damage += 3.0 * (a + 1) as f32;
        }
        if let Some(a) = super::effects::level(&st, "weakness") {
            damage = (damage - 4.0 * (a + 1) as f32).max(0.0);
        }
        if crit {
            damage *= 1.5;
        }
        let kb = weapon.as_ref().map_or(0, |w| w.enchantment("knockback")) + i32::from(st.sprinting);
        Swing { yaw: st.yaw, damage, knockback: 0.4 + 0.5 * kb as f64, weapon }
    };
    if let Some(w) = &s.weapon {
        let n = w.name();
        let wear = if n.ends_with("_sword") || n == "trident" { 1 } else if n.ends_with("_axe") || n.ends_with("_pickaxe") || n.ends_with("_shovel") || n.ends_with("_hoe") { 2 } else { 0 };
        if wear > 0 {
            health::wear_held(server, attacker, wear);
        }
    }
    Some(s)
}

/// Melee hit against another player or a mob.
fn attack(server: &Arc<Server>, attacker: &Arc<Player>, target_eid: i32) {
    if target_eid == attacker.eid {
        return;
    }
    let Some(target) = server.players.read().unwrap().get(&target_eid).cloned() else {
        super::mobs::attacked_by_player(server, attacker, target_eid);
        return;
    };
    let target_pos = {
        let st = target.state.lock().unwrap();
        [st.x, st.y + 0.9, st.z]
    };
    let eye = attacker.state.lock().unwrap().eye();
    let dist2: f64 = (0..3).map(|i| (eye[i] - target_pos[i]).powi(2)).sum();
    if dist2 > 6.0 * 6.0 {
        return;
    }
    let Some(sw) = swing(server, attacker) else { return };
    // An axe against a raised shield disables it for 5 s (the hit is still blocked).
    let axe = sw.weapon.as_ref().is_some_and(|w| w.name().ends_with("_axe"));
    let from = {
        let st = attacker.state.lock().unwrap();
        [st.x, st.y, st.z]
    };
    if axe && super::ranged::blocks_from(server, &target, from) {
        super::ranged::disable_shield(server, &target);
    }
    let by = health::Attacker { pos: Some(from), ..health::Attacker::player(attacker) };
    if health::damage(server, &target, sw.damage, "player_attack", Some(by)) {
        // Knockback in the direction the attacker is facing.
        let k = sw.knockback;
        let (s, c) = (sw.yaw.to_radians() as f64).sin_cos();
        let vel = [-s * k, 0.4, c * k];
        target.state.lock().unwrap().ac.grace = 30;
        let mut p = packet(id::play::ENTITY_VELOCITY);
        p.varint(target.eid);
        for v in vel {
            p.i16((v * 8000.0) as i16);
        }
        server.broadcast_tracked(target.eid, &p, true);
    }
}

/// Sends/unloads chunks around the player. With `force` it recomputes even if the chunk did not change.
pub async fn update_view(server: &Arc<Server>, player: &Arc<Player>, force: bool) {
    let (to_load, epoch) = {
        let mut s = player.state.lock().unwrap();
        let center = ((s.x.floor() as i32) >> 4, (s.z.floor() as i32) >> 4);
        if center == s.center && !force && !s.loaded.is_empty() {
            return;
        }
        let center_changed = center != s.center || s.loaded.is_empty();
        s.center = center;
        let vd = s.view_distance;
        let in_view = |(x, z): (i32, i32)| (x - center.0).abs() <= vd && (z - center.1).abs() <= vd;

        let unload: Vec<_> = s.loaded.iter().copied().filter(|&c| !in_view(c)).collect();
        for c in &unload {
            s.loaded.remove(c);
            let mut p = packet(id::play::UNLOAD_CHUNK);
            p.i32(c.1).i32(c.0);
            player.send(p);
        }
        let mut to_load: Vec<(i32, i32)> = (-vd..=vd)
            .flat_map(|dx| (-vd..=vd).map(move |dz| (center.0 + dx, center.1 + dz)))
            .filter(|c| !s.loaded.contains(c))
            .collect();
        to_load.sort_by_key(|&(x, z)| (x - center.0).pow(2) + (z - center.1).pow(2));
        s.loaded.extend(to_load.iter().copied());
        if center_changed {
            let mut p = packet(id::play::UPDATE_VIEW_POSITION);
            p.varint(center.0).varint(center.1);
            player.send(p);
        }
        (to_load, s.view_epoch)
    };
    // They are generated on a blocking thread and sent in batches.
    let dim = player.dim();
    for batch in to_load.chunks(32) {
        let srv = server.clone();
        let batch = batch.to_vec();
        let packets = tokio::task::spawn_blocking(move || {
            // Loading and generating are the slow part: done without the world lock (and on several
            // threads), so the game tick does not wait for them. The packet also needs the
            // neighbors' border light, so they are produced too.
            let mut need: Vec<(i32, i32)> = batch.iter().flat_map(|&(x, z)| [(x, z), (x + 1, z), (x - 1, z), (x, z + 1), (x, z - 1)]).collect();
            need.sort_unstable();
            need.dedup();
            let (source, missing) = {
                let w = srv.world(dim).lock().unwrap();
                (w.source(), w.missing(&need))
            };
            let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8);
            let per = missing.len().div_ceil(threads).max(1);
            let produced: Vec<((i32, i32), crate::world::chunk::Chunk)> = std::thread::scope(|s| {
                let handles: Vec<_> = missing.chunks(per).map(|part| s.spawn(|| part.iter().map(|&(x, z)| ((x, z), source.produce(x, z))).collect::<Vec<_>>())).collect();
                handles.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
            });
            {
                let mut w = srv.world(dim).lock().unwrap();
                for ((x, z), c) in produced {
                    w.insert(x, z, c);
                }
            }
            crate::world::World::packets_outside_lock(srv.world(dim), &batch)
        })
        .await
        .unwrap();
        // If the player changed dimension in the meantime, these chunks are useless (and the client would
        // read them with the new dimension's height). It is checked and sent with the state
        // locked so it cannot interleave with the `respawn` packet.
        let st = player.state.lock().unwrap();
        if st.view_epoch != epoch {
            return;
        }
        player.send(packet(id::play::CHUNK_BATCH_START));
        for p in &packets {
            player.send(p.to_vec());
        }
        let mut p = packet(id::play::CHUNK_BATCH_FINISHED);
        p.varint(packets.len() as i32);
        player.send(p);
        drop(st);
    }
}

fn send_position(player: &Player, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
    let tp = TELEPORT_ID.fetch_add(1, Ordering::Relaxed);
    player.state.lock().unwrap().ac.reset([x, y, z], tp);
    let mut p = packet(id::play::POSITION);
    p.f64(x).f64(y).f64(z).f32(yaw).f32(pitch).i8(0).varint(tp);
    player.send(p);
}

pub fn teleport(server: &Arc<Server>, player: &Arc<Player>, x: f64, y: f64, z: f64) {
    super::vehicles::dismount(server, player, false);
    let (yaw, pitch) = {
        let mut s = player.state.lock().unwrap();
        (s.x, s.y, s.z) = (x, y, z);
        s.fall_distance = 0.0;
        (s.yaw, s.pitch)
    };
    send_position(player, x, y, z, yaw, pitch);
    let mut p = packet(id::play::ENTITY_TELEPORT);
    p.varint(player.eid).f64(x).f64(y).f64(z).angle(yaw).angle(pitch).bool(false);
    server.broadcast_tracked(player.eid, &p, false);
    // Destination chunks are sent when the client confirms the teleport.
}

pub fn set_game_mode(server: &Arc<Server>, player: &Arc<Player>, mode: u8) {
    {
        let mut st = player.state.lock().unwrap();
        st.game_mode = mode;
        st.fall_distance = 0.0;
        let pos = [st.x, st.y, st.z];
        st.ac.last_valid = pos;
        st.ac.last_ground = pos;
    }
    let mut p = packet(id::play::GAME_STATE_CHANGE);
    p.u8(3).f32(mode as f32);
    player.send(p);
    player.send(abilities(mode));
    let mut p = packet(id::play::PLAYER_INFO);
    p.u8(0x04).varint(1).uuid(player.uuid).varint(mode as i32);
    server.broadcast(&p, None);
}

pub fn abilities(game_mode: u8) -> Vec<u8> {
    let flags = match game_mode {
        1 => 0x01 | 0x04 | 0x08,
        3 => 0x01 | 0x02 | 0x04,
        _ => 0,
    };
    let mut p = packet(id::play::ABILITIES);
    p.i8(flags).f32(0.05).f32(0.1);
    p
}

/// player_info with add_player + game_mode + listed + latency.
fn player_info_add(players: &[Arc<Player>]) -> Vec<u8> {
    let mut p = packet(id::play::PLAYER_INFO);
    p.u8(0x01 | 0x04 | 0x08 | 0x10).varint(players.len() as i32);
    for pl in players {
        let s = pl.state.lock().unwrap();
        p.uuid(pl.uuid).string(&pl.name).varint(0);
        p.varint(s.game_mode as i32).bool(true).varint(s.ping);
    }
    p
}

/// Packets so others can see `player`.
pub fn spawn_packets(player: &Player) -> Vec<Vec<u8>> {
    let (x, y, z, yaw, pitch) = {
        let s = player.state.lock().unwrap();
        (s.x, s.y, s.z, s.yaw, s.pitch)
    };
    let mut spawn = packet(id::play::SPAWN_ENTITY);
    spawn.varint(player.eid).uuid(player.uuid).varint(data::get().player_entity);
    spawn.f64(x).f64(y).f64(z).angle(pitch).angle(yaw).angle(yaw).varint(0);
    spawn.i16(0).i16(0).i16(0);
    let mut head = packet(id::play::ENTITY_HEAD_ROTATION);
    head.varint(player.eid).angle(yaw);
    vec![spawn, head, metadata(player), equipment(player)]
}

/// Metadata: flags (on fire/sneaking/sprinting/invisible), pose, effect particles and
/// skin layers.
pub fn metadata(player: &Player) -> Vec<u8> {
    let (sneaking, sprinting, burning, invisible, color, hand_flags) = {
        let s = player.state.lock().unwrap();
        let burning = s.fire_ticks > 0 && !super::effects::has(&s, "fire_resistance");
        // Hand in use (drawn bow, shield, eating): 0x01 active, 0x02 offhand.
        let using = s.using.as_ref().map(|u| u.hand).or(s.eating.map(|e| e.0));
        let hand_flags = using.map_or(0, |h| 0x01 | if h == 1 { 0x02 } else { 0 });
        (s.sneaking, s.sprinting, burning, super::effects::has(&s, "invisibility"), super::effects::particle_color(&s), hand_flags)
    };
    let mut p = packet(id::play::ENTITY_METADATA);
    p.varint(player.eid);
    let flags = (if burning { 0x01 } else { 0 }) | (if sneaking { 0x02 } else { 0 }) | (if sprinting { 0x08 } else { 0 }) | (if invisible { 0x20 } else { 0 });
    p.u8(0).varint(0).i8(flags);
    p.u8(8).varint(0).i8(hand_flags);
    p.u8(10).varint(1).varint(color);
    p.u8(6).varint(20).varint(if sneaking { 5 } else { 0 });
    p.u8(17).varint(0).u8(0x7f);
    p.u8(0xff);
    p
}

/// Visible equipment: hands and armor.
pub fn equipment(player: &Player) -> Vec<u8> {
    let items: Vec<(u8, Slot)> = {
        let s = player.state.lock().unwrap();
        let hot = super::inventory::HOTBAR + s.held_slot;
        // Equipment slots: 0 main hand, 1 offhand, 2 boots, 3 leggings, 4 chestplate, 5 helmet.
        vec![
            (0, s.inv.slots[hot].clone()),
            (1, s.inv.slots[super::inventory::OFFHAND].clone()),
            (2, s.inv.slots[8].clone()),
            (3, s.inv.slots[7].clone()),
            (4, s.inv.slots[6].clone()),
            (5, s.inv.slots[5].clone()),
        ]
    };
    let mut p = packet(id::play::ENTITY_EQUIPMENT);
    p.varint(player.eid);
    let last = items.len() - 1;
    for (i, (slot, item)) in items.iter().enumerate() {
        p.u8(if i < last { slot | 0x80 } else { *slot });
        write_slot(&mut p, item);
    }
    p
}

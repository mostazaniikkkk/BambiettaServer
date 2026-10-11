//! Chat commands (equivalent to commands.js and flying-squid's command plugins).

use super::{update_time, Player, Server};
use crate::protocol::{id, packet, Writer};
use std::sync::atomic::Ordering;
use std::sync::Arc;

struct Command {
    name: &'static str,
    usage: &'static str,
    help: &'static str,
    op: bool,
    run: fn(&Arc<Server>, &Arc<Player>, &str) -> Result<(), String>,
}

const COMMANDS: &[Command] = &[
    Command { name: "help", usage: "/help", help: "Lista de comandos", op: false, run: help },
    Command { name: "list", usage: "/list", help: "Jugadores conectados", op: false, run: list },
    Command { name: "msg", usage: "/msg <jugador> <mensaje>", help: "Mensaje privado", op: false, run: msg },
    Command { name: "tell", usage: "/tell <jugador> <mensaje>", help: "Mensaje privado", op: false, run: msg },
    Command { name: "me", usage: "/me <acción>", help: "Acción en tercera persona", op: false, run: me },
    Command { name: "spawn", usage: "/spawn", help: "Volver al punto de aparición", op: false, run: spawn },
    Command { name: "ping", usage: "/ping", help: "Tu latencia", op: false, run: ping },
    Command { name: "gamemode", usage: "/gamemode <modo> [jugador]", help: "Cambiar modo de juego", op: true, run: gamemode },
    Command { name: "gm", usage: "/gm <0-3> [jugador]", help: "Alias de /gamemode", op: true, run: gamemode },
    Command { name: "tp", usage: "/tp <x y z> | <jugador> | <jugador> <destino>", help: "Teletransportar", op: true, run: tp },
    Command { name: "weather", usage: "/weather <clear|rain|thunder> [segundos]", help: "Cambiar el clima", op: true, run: weather },
    Command { name: "time", usage: "/time set <day|noon|night|midnight|ticks> | /time add <ticks>", help: "Cambiar la hora", op: true, run: time },
    Command { name: "say", usage: "/say <mensaje>", help: "Anuncio global", op: true, run: say },
    Command { name: "kick", usage: "/kick <jugador> [motivo]", help: "Expulsar a un jugador", op: true, run: kick },
    Command { name: "give", usage: "/give <ítem>[{nbt}] [cantidad] [jugador]", help: "Dar ítems (con NBT: pociones, libros...)", op: true, run: give },
    Command { name: "effect", usage: "/effect <efecto> [segundos] [nivel] [jugador] | /effect clear [jugador]", help: "Dar o quitar efectos", op: true, run: effect },
    Command { name: "clear", usage: "/clear [jugador]", help: "Vaciar inventario", op: true, run: clear },
    Command { name: "kill", usage: "/kill [jugador]", help: "Matar a un jugador", op: true, run: kill },
    Command { name: "heal", usage: "/heal [jugador]", help: "Restaurar vida y hambre", op: true, run: heal },
    Command { name: "food", usage: "/food <0-20> [jugador]", help: "Fijar el hambre (QA)", op: true, run: food },
    Command { name: "enchant", usage: "/enchant <encantamiento> [nivel] [jugador]", help: "Encantar el ítem de la mano", op: true, run: enchant },
    Command { name: "xp", usage: "/xp <cantidad>[L] [jugador]", help: "Dar experiencia (L = niveles)", op: true, run: xp },
    Command { name: "summon", usage: "/summon <mob|barco|minecart> [profesión|tamaño|patrulla|capitan]", help: "Invocar un mob o un vehículo", op: true, run: summon },
    Command { name: "killmobs", usage: "/killmobs [tipo|all] [radio]", help: "Matar mobs cercanos (QA)", op: true, run: killmobs },
    Command { name: "setblock", usage: "/setblock <x y z> <bloque[prop=valor,...]>", help: "Poner un bloque", op: true, run: setblock },
    Command { name: "fill", usage: "/fill <x1 y1 z1> <x2 y2 z2> <bloque>", help: "Rellenar una zona (máx. 32768)", op: true, run: fill },
    Command { name: "dimension", usage: "/dimension <overworld|nether|end>", help: "Ir a otra dimensión", op: true, run: dimension },
    Command { name: "where", usage: "/where", help: "Dimensión, posición y bioma", op: false, run: where_am_i },
    Command { name: "save-all", usage: "/save-all", help: "Guardar el mundo", op: true, run: save_all },
];

/// `declare_commands` packet: each command is a literal with an optional
/// `greedy_string` argument, so the client recognizes them and does not flag an error.
pub fn declare_commands() -> Vec<u8> {
    let mut p = packet(id::play::DECLARE_COMMANDS);
    let n = COMMANDS.len() as i32;
    p.varint(1 + 2 * n);
    // Root
    p.u8(0x00).varint(n);
    for i in 0..n {
        p.varint(1 + 2 * i);
    }
    for (i, c) in COMMANDS.iter().enumerate() {
        // Executable literal with one child
        p.u8(0x01 | 0x04).varint(1).varint(2 + 2 * i as i32).string(c.name);
        // Executable argument: brigadier:string (5) in greedy mode (2)
        p.u8(0x02 | 0x04).varint(0).string("args").varint(5).varint(2);
    }
    p.varint(0);
    p
}

pub fn execute(server: &Arc<Server>, player: &Arc<Player>, line: &str) {
    let line = line.trim_start_matches('/');
    let (name, args) = line.split_once(' ').map_or((line, ""), |(a, b)| (a, b.trim()));
    tracing::info!("{} ejecutó /{line}", player.name);
    let Some(cmd) = COMMANDS.iter().find(|c| c.name.eq_ignore_ascii_case(name)) else {
        player.message(&format!("§cComando desconocido: /{name}. Usa /help"));
        return;
    };
    if cmd.op && !player.is_op(server) {
        player.message("§cNo tienes permiso para usar este comando");
        return;
    }
    if let Err(e) = (cmd.run)(server, player, args) {
        player.message(&format!("§c{}", if e.is_empty() { format!("Uso: {}", cmd.usage) } else { e }));
    }
}

fn help(_: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    p.message("§6--- Comandos de Bambietta ---");
    for c in COMMANDS {
        p.message(&format!("§e{}§7 - {}", c.usage, c.help));
    }
    Ok(())
}

fn list(s: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    let players = s.online();
    let names: Vec<String> = players.iter().map(|x| format!("{} §8({})§r", x.name, x.transport)).collect();
    p.message(&format!("§e{}/{} conectados: §r{}", players.len(), s.config.max_players, names.join(", ")));
    Ok(())
}

fn target(s: &Server, name: &str) -> Result<Arc<Player>, String> {
    s.find_player(name).ok_or_else(|| format!("Jugador no encontrado: {name}"))
}

fn msg(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let (to, text) = args.split_once(' ').ok_or_default()?;
    let to = target(s, to)?;
    to.message(&format!("§7§o{} te susurra: {text}", p.name));
    p.message(&format!("§7§oSusurras a {}: {text}", to.name));
    Ok(())
}

fn me(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    if args.is_empty() {
        return Err(String::new());
    }
    s.broadcast_message(&format!("* {} {args}", p.name));
    Ok(())
}

fn spawn(s: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    super::play::teleport(s, p, s.spawn.x as f64 + 0.5, s.spawn.y as f64, s.spawn.z as f64 + 0.5);
    Ok(())
}

fn ping(_: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    let ms = p.state.lock().unwrap().ping;
    p.message(&format!("§aPing: {ms} ms ({})", p.transport));
    Ok(())
}

fn gamemode(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let mode = match it.next().ok_or_default()?.to_ascii_lowercase().as_str() {
        "0" | "s" | "survival" | "supervivencia" => 0,
        "1" | "c" | "creative" | "creativo" => 1,
        "2" | "a" | "adventure" | "aventura" => 2,
        "3" | "sp" | "spectator" | "espectador" => 3,
        m => return Err(format!("Modo desconocido: {m}")),
    };
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    super::play::set_game_mode(s, &who, mode);
    let name = ["supervivencia", "creativo", "aventura", "espectador"][mode as usize];
    who.message(&format!("§aModo de juego: {name}"));
    if who.eid != p.eid {
        p.message(&format!("§a{} ahora está en modo {name}", who.name));
    }
    Ok(())
}

fn tp(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let pos_of = |pl: &Arc<Player>| {
        let st = pl.state.lock().unwrap();
        (st.x, st.y, st.z)
    };
    let coord = |v: &str, cur: f64| -> Result<f64, String> {
        match v.strip_prefix('~') {
            Some("") => Ok(cur),
            Some(rel) => rel.parse::<f64>().map(|d| cur + d).map_err(|_| format!("Coordenada inválida: {v}")),
            None => v.parse::<f64>().map_err(|_| format!("Coordenada inválida: {v}")),
        }
    };
    let (who, dest) = match parts.as_slice() {
        [x, y, z] => {
            let (cx, cy, cz) = pos_of(p);
            (p.clone(), (coord(x, cx)?, coord(y, cy)?, coord(z, cz)?))
        }
        [who, x, y, z] => {
            let w = target(s, who)?;
            let (cx, cy, cz) = pos_of(&w);
            (w, (coord(x, cx)?, coord(y, cy)?, coord(z, cz)?))
        }
        [to] => (p.clone(), pos_of(&target(s, to)?)),
        [who, to] => (target(s, who)?, pos_of(&target(s, to)?)),
        _ => return Err(String::new()),
    };
    super::play::teleport(s, &who, dest.0, dest.1, dest.2);
    p.message(&format!("§aTeletransportado {} a {:.1} {:.1} {:.1}", who.name, dest.0, dest.1, dest.2));
    Ok(())
}

fn weather(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let kind = it.next().ok_or_default()?;
    if !matches!(kind, "clear" | "rain" | "thunder") {
        return Err(format!("Clima desconocido: {kind}"));
    }
    let ticks = it.next().map(|v| v.parse::<i32>().map(|s| s * 20).map_err(|_| format!("Duración inválida: {v}"))).transpose()?;
    s.weather.lock().unwrap().set(kind, ticks);
    let name = match kind {
        "clear" => "despejado",
        "rain" => "lluvia",
        _ => "tormenta",
    };
    p.message(&format!("§aClima: {name}"));
    Ok(())
}

fn time(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let action = it.next().ok_or_default()?;
    let value = it.next().ok_or_default()?;
    let ticks = match value {
        "day" => 1000,
        "noon" => 6000,
        "night" => 13000,
        "midnight" => 18000,
        v => v.parse::<i64>().map_err(|_| format!("Valor inválido: {v}"))?,
    };
    let new = match action {
        "set" => {
            let cur = s.day_time.load(Ordering::Relaxed);
            cur - cur.rem_euclid(24000) + ticks
        }
        "add" => s.day_time.load(Ordering::Relaxed) + ticks,
        _ => return Err(String::new()),
    };
    s.day_time.store(new, Ordering::Relaxed);
    s.broadcast(&update_time(s.age.load(Ordering::Relaxed), new), None);
    p.message(&format!("§aHora: {}", new.rem_euclid(24000)));
    Ok(())
}

fn say(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    if args.is_empty() {
        return Err(String::new());
    }
    s.broadcast_message(&format!("§d[{}] {args}", p.name));
    Ok(())
}

fn kick(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let (who, reason) = args.split_once(' ').unwrap_or((args, "Expulsado del servidor"));
    if who.is_empty() {
        return Err(String::new());
    }
    let w = target(s, who)?;
    w.kick(&format!("§c{reason}"));
    p.message(&format!("§aExpulsado {}", w.name));
    Ok(())
}

/// Splits `name{snbt} rest` (the SNBT may contain spaces).
fn split_nbt(args: &str) -> Result<(&str, Option<crate::protocol::Nbt>, &str), String> {
    let first_space = args.find(char::is_whitespace).unwrap_or(args.len());
    let Some(brace) = args.find('{').filter(|&b| b < first_space) else {
        return Ok((&args[..first_space], None, &args[first_space..]));
    };
    let bytes = args.as_bytes();
    let (mut depth, mut quote, mut end) = (0i32, None::<u8>, None);
    let mut i = brace;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(_) if c == b'\\' => i += 1,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'{' || c == b'[' => depth += 1,
            None if c == b'}' || c == b']' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            None => {}
        }
        i += 1;
    }
    let end = end.ok_or("NBT sin cerrar")?;
    let nbt = crate::protocol::nbt::parse_snbt(&args[brace..=end]).map_err(|e| format!("NBT inválido: {e}"))?;
    Ok((&args[..brace], Some(nbt), &args[end + 1..]))
}

fn give(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let (name, nbt, rest) = split_nbt(args.trim())?;
    let name = name.trim_start_matches("minecraft:");
    let mut it = rest.split_whitespace();
    let id = crate::data::get().item_id(name).filter(|&i| i > 0).ok_or_else(|| format!("Ítem desconocido: {name}"))?;
    let count: i32 = match it.next() {
        Some(n) => n.parse().map_err(|_| format!("Cantidad inválida: {n}"))?,
        None => 1,
    };
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    let mut item = crate::item::ItemStack::new(id, 1);
    item.nbt = nbt.filter(|n| !n.is_empty_compound()).map(Box::new);
    let max = item.max_stack() as i32;
    let mut left = count.clamp(1, 64 * 36);
    let mut thrown = Vec::new();
    {
        let mut st = who.state.lock().unwrap();
        while left > 0 {
            let n = left.min(max);
            left -= n;
            let (rest, changed) = st.inv.add(item.with_count(n as i8));
            for slot in changed {
                st.sync_slot(&who, slot);
            }
            thrown.extend(rest);
        }
    }
    for i in thrown {
        super::entities::throw_from(s, &who, i);
    }
    p.message(&format!("§aDado {count} x {} a {}", item.name(), who.name));
    Ok(())
}

fn clear(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let who = if args.is_empty() { p.clone() } else { target(s, args)? };
    {
        let mut st = who.state.lock().unwrap();
        st.inv.drain();
        let pk = st.inv.window_items();
        who.send(pk);
    }
    s.broadcast_tracked(who.eid, &super::play::equipment(&who), false);
    p.message(&format!("§aInventario de {} vaciado", who.name));
    Ok(())
}

fn kill(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let who = if args.is_empty() { p.clone() } else { target(s, args)? };
    super::health::die(s, &who, &format!("{} murió", who.name));
    Ok(())
}

fn heal(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let who = if args.is_empty() { p.clone() } else { target(s, args)? };
    {
        let mut st = who.state.lock().unwrap();
        if st.dead {
            return Err(format!("{} está muerto", who.name));
        }
        (st.health, st.food, st.saturation, st.exhaustion) = (20.0, 20, 5.0, 0.0);
        who.send(super::health::health_packet(20.0, 20, 5.0));
    }
    p.message(&format!("§a{} curado", who.name));
    Ok(())
}

fn food(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let v: i32 = it.next().ok_or_default()?.parse().map_err(|_| "Valor inválido".to_string())?;
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    let mut st = who.state.lock().unwrap();
    st.food = v.clamp(0, 20);
    st.saturation = st.saturation.min(st.food as f32);
    who.send(super::health::health_packet(st.health, st.food, st.saturation));
    Ok(())
}

fn enchant(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let name = it.next().ok_or_default()?.trim_start_matches("minecraft:").to_string();
    let e = crate::gameplay::get().enchantment(&name).ok_or_else(|| format!("Encantamiento desconocido: {name}"))?;
    let level: i32 = match it.next() {
        Some(l) => l.parse().map_err(|_| format!("Nivel inválido: {l}"))?,
        None => 1,
    };
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    {
        let mut st = who.state.lock().unwrap();
        let slot = super::inventory::HOTBAR + st.held_slot;
        let Some(item) = st.inv.slots[slot].as_mut() else { return Err(format!("{} no tiene nada en la mano", who.name)) };
        item.enchant(&e.name, level.clamp(1, 255));
        st.sync_slot(&who, slot);
    }
    s.broadcast_tracked(who.eid, &super::play::equipment(&who), false);
    p.message(&format!("§aEncantado con {} {level}", e.name));
    Ok(())
}

fn xp(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let amount = it.next().ok_or_default()?;
    let (n, levels) = match amount.strip_suffix(['L', 'l']) {
        Some(n) => (n, true),
        None => (amount, false),
    };
    let n: i32 = n.parse().map_err(|_| format!("Cantidad inválida: {amount}"))?;
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    let mut st = who.state.lock().unwrap();
    if levels {
        st.xp_level = (st.xp_level + n).max(0);
        who.send(super::experience::packet_for(&st));
    } else {
        super::experience::add_points(&who, &mut st, n);
    }
    p.message(&format!("§a{} ahora tiene nivel {}", who.name, st.xp_level));
    Ok(())
}

fn summon(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let name = it.next().ok_or_default()?.trim_start_matches("minecraft:");
    let extra = it.next();
    let (pos, yaw) = {
        let st = p.state.lock().unwrap();
        let (sy, cy) = (st.yaw.to_radians() as f64).sin_cos();
        ([st.x - sy * 2.0, st.y, st.z + cy * 2.0], st.yaw)
    };
    let vehicle = match name {
        "boat" => Some(super::vehicles::VehicleKind::Boat(0, false)),
        "chest_boat" => Some(super::vehicles::VehicleKind::Boat(0, true)),
        n => super::vehicles::VehicleKind::from_item(n),
    };
    if name == "lightning_bolt" {
        if p.dim() != crate::world::Dimension::Overworld {
            return Err("Los rayos solo caen en el supramundo".into());
        }
        super::weather::strike(s, pos[0].floor() as i32, pos[2].floor() as i32);
        return Ok(());
    }
    if let Some(v) = vehicle {
        let eid = super::vehicles::spawn(s, p.dim(), v, pos, yaw);
        p.message(&format!("§a{name} invocado (eid {eid})"));
        return Ok(());
    }
    let kind = super::mobs::MobKind::from_name(name).ok_or_else(|| format!("Mob desconocido: {name}"))?;
    if kind == super::mobs::MobKind::Pillager && matches!(extra, Some("patrulla" | "patrol")) {
        let eids = super::pillagers::spawn_patrol(s, p.dim(), pos, 4);
        p.message(&format!("§aPatrulla de saqueadores invocada (capitán eid {})", eids[0]));
        return Ok(());
    }
    let eid = super::mobs::spawn(s, p.dim(), kind, pos);
    match (kind, extra) {
        (super::mobs::MobKind::Pillager, Some("capitan" | "capitán" | "captain")) => super::pillagers::make_captain(s, eid, 1 + (super::entities::random() * 1e9) as u32),
        (super::mobs::MobKind::MagmaCube, Some(n)) => super::mobs::set_size(s, eid, n.parse().map_err(|_| format!("Tamaño inválido: {n}"))?),
        (super::mobs::MobKind::Villager, Some(prof)) => {
            if let Some(m) = s.mobs.lock().unwrap().get_mut(&eid) {
                m.villager = Some(Box::new(super::villagers::VillagerData::with_profession(prof)));
            }
        }
        _ => {}
    }
    p.message(&format!("§a{} invocado (eid {eid})", kind.display()));
    Ok(())
}

fn killmobs(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let kind = it.next().unwrap_or("all").trim_start_matches("minecraft:");
    let filter = if kind == "all" { None } else { Some(super::mobs::MobKind::from_name(kind).ok_or_else(|| format!("Mob desconocido: {kind}"))?) };
    let r: f64 = it.next().map(|v| v.parse().map_err(|_| format!("Radio inválido: {v}"))).transpose()?.unwrap_or(32.0);
    let pos = {
        let st = p.state.lock().unwrap();
        [st.x, st.y, st.z]
    };
    let dim = p.dim();
    let targets: Vec<i32> = s
        .mobs
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.dim == dim && m.death_ticks.is_none() && filter.is_none_or(|k| m.kind == k) && super::mobs::dist2(&m.pos, &pos) < r * r)
        .map(|m| m.eid)
        .collect();
    for eid in &targets {
        super::mobs::hurt(s, *eid, 100_000.0, "player_attack", Some(p.eid), None, 0);
    }
    p.message(&format!("§a{} mobs eliminados", targets.len()));
    Ok(())
}

fn effect(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let mut it = args.split_whitespace();
    let first = it.next().ok_or_default()?.trim_start_matches("minecraft:");
    if first == "clear" {
        let who = match it.next() {
            Some(n) => target(s, n)?,
            None => p.clone(),
        };
        super::effects::clear(s, &who);
        p.message(&format!("§aEfectos de {} quitados", who.name));
        return Ok(());
    }
    super::effects::id(first).ok_or_else(|| format!("Efecto desconocido: {first}"))?;
    let secs: i32 = it.next().map(|v| v.parse().map_err(|_| format!("Segundos inválidos: {v}"))).transpose()?.unwrap_or(30);
    let level: i32 = it.next().map(|v| v.parse().map_err(|_| format!("Nivel inválido: {v}"))).transpose()?.unwrap_or(1);
    let who = match it.next() {
        Some(n) => target(s, n)?,
        None => p.clone(),
    };
    let name: &'static str = super::effects::NAMES.iter().find(|n| **n == first).copied().unwrap_or("speed");
    super::effects::add(s, &who, name, (level - 1).clamp(0, 255), secs.max(1) * 20, 1.0);
    p.message(&format!("§a{} {} durante {secs} s a {}", name, level, who.name));
    Ok(())
}

/// Absolute or relative (`~`, `~3`) block coordinate, relative to `cur`.
fn block_coord(v: &str, cur: f64) -> Result<i32, String> {
    let r = match v.strip_prefix('~') {
        Some("") => cur,
        Some(rel) => cur + rel.parse::<f64>().map_err(|_| format!("Coordenada inválida: {v}"))?,
        None => v.parse::<f64>().map_err(|_| format!("Coordenada inválida: {v}"))?,
    };
    Ok(r.floor() as i32)
}

fn parse_pos(p: &Player, parts: &[&str]) -> Result<crate::protocol::BlockPos, String> {
    let (x, y, z) = {
        let st = p.state.lock().unwrap();
        (st.x, st.y, st.z)
    };
    Ok(crate::protocol::BlockPos::new(block_coord(parts[0], x)?, block_coord(parts[1], y)?, block_coord(parts[2], z)?))
}

fn place_blocks(s: &Arc<Server>, dim: crate::world::Dimension, a: crate::protocol::BlockPos, b: crate::protocol::BlockPos, name: &str) -> Result<usize, String> {
    // Vanilla syntax: name[prop=value,...]
    let (name, props) = match name.split_once('[') {
        Some((n, rest)) => (n, rest.trim_end_matches(']')),
        None => (name, ""),
    };
    let block = crate::data::get().block(name).ok_or_else(|| format!("Bloque desconocido: {name}"))?;
    let mut state = block.default_state;
    for kv in props.split(',').filter(|kv| !kv.is_empty()) {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("Propiedad inválida: {kv}"))?;
        if !block.has(k.trim()) {
            return Err(format!("{name} no tiene la propiedad {k}"));
        }
        state = block.with(state, k.trim(), v.trim());
    }
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y).max(dim.min_y()), a.y.max(b.y).min(dim.min_y() + dim.height() - 1));
    let (z0, z1) = (a.z.min(b.z), a.z.max(b.z));
    let n = (x1 - x0 + 1) as i64 * (y1 - y0 + 1).max(0) as i64 * (z1 - z0 + 1) as i64;
    if n > 32768 {
        return Err(format!("Demasiados bloques ({n} > 32768)"));
    }
    let mut changed = Vec::new();
    {
        let mut world = s.world(dim).lock().unwrap();
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    let p = crate::protocol::BlockPos::new(x, y, z);
                    world.take_block_entity(p);
                    if world.set_block(p, state) != state {
                        changed.push(p);
                    }
                    if let Some(be) = crate::world::blockentity::BlockEntity::for_block(name) {
                        world.set_block_entity(p, be);
                    }
                }
            }
        }
    }
    for p in &changed {
        s.broadcast_dim(dim, &super::blocks::block_change(*p, state), None);
        super::fluids::neighbor_changed(s, dim, *p);
        super::redstone::changed(s, dim, *p);
        if name == "fire" {
            super::fire::schedule(s, dim, *p);
        }
    }
    Ok(changed.len())
}

fn setblock(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let [_, _, _, block] = parts[..] else { return Err(String::new()) };
    let pos = parse_pos(p, &parts[..3])?;
    place_blocks(s, p.dim(), pos, pos, block.trim_start_matches("minecraft:"))?;
    Ok(())
}

fn fill(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let [_, _, _, _, _, _, block] = parts[..] else { return Err(String::new()) };
    let (a, b) = (parse_pos(p, &parts[..3])?, parse_pos(p, &parts[3..6])?);
    let n = place_blocks(s, p.dim(), a, b, block.trim_start_matches("minecraft:"))?;
    p.message(&format!("§a{n} bloques cambiados"));
    Ok(())
}

fn dimension(s: &Arc<Server>, p: &Arc<Player>, args: &str) -> Result<(), String> {
    use crate::world::Dimension;
    let dim = match args.trim().trim_start_matches("minecraft:") {
        "overworld" | "supramundo" => Dimension::Overworld,
        "nether" | "the_nether" => Dimension::Nether,
        "end" | "the_end" => Dimension::End,
        _ => return Err(String::new()),
    };
    // A safe spot: the spawn, a platform in the Nether, or the End one.
    let pos = match dim {
        Dimension::Overworld => [s.spawn.x as f64 + 0.5, s.spawn.y as f64, s.spawn.z as f64 + 0.5],
        Dimension::Nether => {
            let (obsidian, reg) = (crate::data::get().state("obsidian"), crate::data::get());
            let mut changes = Vec::new();
            {
                let mut w = s.world(dim).lock().unwrap();
                for dx in -2..=2 {
                    for dz in -2..=2 {
                        changes.push((crate::protocol::BlockPos::new(dx, 69, dz), obsidian));
                        for h in 70..73 {
                            changes.push((crate::protocol::BlockPos::new(dx, h, dz), 0));
                        }
                    }
                }
                for &(q, st) in &changes {
                    w.set_block(q, st);
                }
            }
            let _ = reg;
            for (q, st) in changes {
                s.broadcast_dim(dim, &super::blocks::block_change(q, st), None);
            }
            [0.5, 70.0, 0.5]
        }
        Dimension::End => [100.5, 49.0, 0.5],
    };
    let (srv, pl) = (s.clone(), p.clone());
    tokio::spawn(async move {
        if dim == Dimension::End {
            // Reuse portal arrival (creates the platform).
            let reg = crate::data::get();
            let mut changes = Vec::new();
            {
                let mut w = srv.world(dim).lock().unwrap();
                for dx in -2..=2 {
                    for dz in -2..=2 {
                        changes.push((crate::protocol::BlockPos::new(100 + dx, 48, dz), reg.state("obsidian")));
                        for h in 49..52 {
                            changes.push((crate::protocol::BlockPos::new(100 + dx, h, dz), 0));
                        }
                    }
                }
                for &(q, st) in &changes {
                    w.set_block(q, st);
                }
            }
        }
        super::portals::change_dimension(&srv, &pl, dim, pos).await;
    });
    Ok(())
}

fn where_am_i(s: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    let (x, y, z) = {
        let st = p.state.lock().unwrap();
        (st.x, st.y, st.z)
    };
    let dim = p.dim();
    let biome = s.world(dim).lock().unwrap().generator().biome_name(x.floor() as i32, z.floor() as i32);
    p.message(&format!("§e{} §7{:.1} {:.1} {:.1} §ebioma §7{biome}", dim.name(), x, y, z));
    Ok(())
}

fn save_all(s: &Arc<Server>, p: &Arc<Player>, _: &str) -> Result<(), String> {
    s.save().map_err(|e| format!("Error al guardar: {e:#}"))?;
    p.message("§aMundo guardado");
    Ok(())
}

trait OrDefault<T> {
    fn ok_or_default(self) -> Result<T, String>;
}

impl<T> OrDefault<T> for Option<T> {
    /// Empty error = show the command usage.
    fn ok_or_default(self) -> Result<T, String> {
        self.ok_or_else(String::new)
    }
}

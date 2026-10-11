//! Handshake, status (server list), login (offline, or online through `auth`) and the
//! configuration phase.

use super::{offline_uuid, Server};
use crate::data::REGISTRY_CODEC;
use crate::net::{Conn, Out};
use crate::protocol::{id, packet, text_json, Reader, Writer, PROTOCOL_VERSION, VERSION_NAME};
use anyhow::{bail, Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Maximum time to complete handshake/login/configuration.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn handle(server: Arc<Server>, mut conn: Conn) -> Result<()> {
    let body = next(&mut conn).await?.context("cerrada antes del handshake")?;
    let mut r = Reader::new(&body);
    if r.varint()? != id::handshake::SET_PROTOCOL {
        bail!("se esperaba handshake");
    }
    let protocol = r.varint()?;
    let _address = r.string(255)?;
    let _port = r.u16()?;
    match r.varint()? {
        1 => status(&server, conn).await,
        2 => {
            let Some(login) = tokio::time::timeout(LOGIN_TIMEOUT, login(&server, &mut conn, protocol)).await.context("login: tiempo agotado")?? else {
                return Ok(());
            };
            super::play::run(server, conn, login).await
        }
        s => bail!("estado siguiente inválido {s}"),
    }
}

async fn next(conn: &mut Conn) -> Result<Option<Vec<u8>>> {
    tokio::time::timeout(LOGIN_TIMEOUT, conn.reader.next()).await.context("tiempo agotado")?
}

async fn status(server: &Server, mut conn: Conn) -> Result<()> {
    while let Some(body) = next(&mut conn).await? {
        let mut r = Reader::new(&body);
        match r.varint()? {
            id::status::REQUEST => {
                let players = server.online();
                let json = serde_json::json!({
                    "version": { "name": VERSION_NAME, "protocol": PROTOCOL_VERSION },
                    "players": {
                        "max": server.config.max_players,
                        "online": players.len(),
                        "sample": players.iter().take(12).map(|p| serde_json::json!({
                            "name": p.name, "id": super::format_uuid(p.uuid)
                        })).collect::<Vec<_>>(),
                    },
                    "description": { "text": server.config.motd },
                    "enforcesSecureChat": false,
                    "previewsChat": false,
                });
                let mut p = packet(id::status::SERVER_INFO);
                p.string(&json.to_string());
                conn.send(p);
            }
            id::status::PING => {
                let mut p = packet(id::status::PONG);
                p.i64(r.i64()?);
                conn.send(p);
                let _ = conn.tx.send(Out::Close);
                break;
            }
            _ => {}
        }
    }
    Ok(())
}

pub struct Login {
    pub name: String,
    pub uuid: u128,
    pub properties: Vec<super::auth::Property>,
    pub view_distance: i32,
}

fn login_disconnect(conn: &Conn, reason: &str) {
    let mut p = packet(id::login::DISCONNECT);
    p.string(&text_json(reason));
    conn.send(p);
    let _ = conn.tx.send(Out::Close);
}

async fn login(server: &Server, conn: &mut Conn, protocol: i32) -> Result<Option<Login>> {
    let body = next(conn).await?.context("cerrada durante login")?;
    let mut r = Reader::new(&body);
    if r.varint()? != id::login::LOGIN_START {
        bail!("se esperaba login_start");
    }
    let mut name = r.string(16)?;
    if protocol != PROTOCOL_VERSION {
        login_disconnect(conn, &format!("§cEste servidor usa Minecraft {VERSION_NAME} (protocolo {PROTOCOL_VERSION})"));
        info!("{name} rechazado: protocolo {protocol} ({})", conn.peer);
        return Ok(None);
    }
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        login_disconnect(conn, "§cNombre de usuario inválido");
        return Ok(None);
    }
    // Online mode: encryption and the Mojang account.
    let mut uuid = offline_uuid(&name);
    let mut properties = Vec::new();
    if server.config.online_mode && (conn.transport == "tcp" || server.config.online_mode_ws) {
        match super::auth::authenticate(conn, &name).await {
            Ok(Some(profile)) => {
                name = profile.name;
                uuid = profile.uuid;
                properties = profile.properties;
            }
            Ok(None) => {
                info!("{name} rechazado ({}): Mojang no confirma la sesión", conn.peer);
                login_disconnect(conn, "§cNo se pudo verificar tu cuenta de Minecraft.
§7Inicia sesión en el launcher oficial y vuelve a intentarlo.");
                return Ok(None);
            }
            Err(e) => {
                info!("{name}: falló la autenticación ({}): {e:#}", conn.peer);
                login_disconnect(conn, "§cNo se pudo verificar tu cuenta con los servidores de Mojang.
§7Inténtalo de nuevo en un momento.");
                return Ok(None);
            }
        }
    }
    // Bans and whitelist.
    let refused = server.admin.lock().unwrap().refuse(&name, &super::format_uuid(uuid), &conn.peer.ip().to_string());
    if let Some(reason) = refused {
        info!("{name} rechazado ({}): {}", conn.peer, reason.replace('\n', " "));
        login_disconnect(conn, &reason);
        return Ok(None);
    }
    if server.find_player(&name).is_some() {
        login_disconnect(conn, "§cYa hay un jugador conectado con ese nombre");
        return Ok(None);
    }
    if server.players.read().unwrap().len() >= server.config.max_players {
        login_disconnect(conn, "§cEl servidor está lleno");
        return Ok(None);
    }

    let threshold = match conn.transport {
        "tcp" => server.config.compression_threshold,
        _ => server.config.ws_compression_threshold.unwrap_or(server.config.compression_threshold),
    };
    if threshold >= 0 {
        let mut p = packet(id::login::COMPRESS);
        p.varint(threshold);
        conn.send(p);
        let _ = conn.tx.send(Out::Compression(threshold));
        conn.reader.compression = Some(threshold);
    }

    let mut p = packet(id::login::SUCCESS);
    p.uuid(uuid).string(&name);
    write_properties(&mut p, &properties);
    conn.send(p);

    // Wait for login_acknowledged (plugin responses are ignored).
    loop {
        let body = next(conn).await?.context("cerrada durante login")?;
        if Reader::new(&body).varint()? == id::login::LOGIN_ACKNOWLEDGED {
            break;
        }
    }

    // Configuration: registries, feature flags and server brand.
    let mut p = packet(id::config::REGISTRY_DATA);
    p.extend_from_slice(REGISTRY_CODEC);
    conn.send(p);
    let mut p = packet(id::config::FEATURE_FLAGS);
    p.varint(1).string("minecraft:vanilla");
    conn.send(p);
    let mut p = packet(id::config::TAGS);
    p.extend_from_slice(crate::data::tags_payload());
    conn.send(p);
    let mut p = packet(id::config::CUSTOM_PAYLOAD);
    p.string("minecraft:brand").string("Bambietta");
    conn.send(p);
    conn.send(packet(id::config::FINISH_CONFIGURATION));

    let mut view_distance = server.config.view_distance;
    loop {
        let body = next(conn).await?.context("cerrada durante configuración")?;
        let mut r = Reader::new(&body);
        match r.varint()? {
            id::config::C_FINISH_CONFIGURATION => break,
            id::config::C_SETTINGS => {
                let _locale = r.string(16)?;
                view_distance = (r.i8()? as i32).clamp(2, server.config.view_distance);
            }
            other => debug!("{name}: paquete de configuración 0x{other:02x} ignorado"),
        }
    }
    Ok(Some(Login { name, uuid, properties, view_distance }))
}

/// Profile properties as in login_success and player_info.
pub fn write_properties(p: &mut Vec<u8>, properties: &[super::auth::Property]) {
    p.varint(properties.len() as i32);
    for prop in properties {
        p.string(&prop.name).string(&prop.value);
        match &prop.signature {
            Some(s) => {
                p.bool(true).string(s);
            }
            None => {
                p.bool(false);
            }
        }
    }
}

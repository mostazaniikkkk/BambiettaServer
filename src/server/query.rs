//! Query protocol (GameSpy4) over UDP, compatible with vanilla's `enable-query`.

use super::{strip_codes, Server};
use crate::protocol::VERSION_NAME;
use anyhow::Result;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tracing::info;

pub async fn serve(server: Arc<Server>, bind: SocketAddr) -> Result<()> {
    let socket = UdpSocket::bind(bind).await?;
    info!("query UDP en {bind}");
    let mut challenges: HashMap<IpAddr, (i32, Instant)> = HashMap::new();
    let mut buf = [0u8; 1500];
    loop {
        let (n, peer) = socket.recv_from(&mut buf).await?;
        let req = &buf[..n];
        if n < 7 || req[0] != 0xfe || req[1] != 0xfd {
            continue;
        }
        let kind = req[2];
        let session = &req[3..7];
        let mut out = vec![kind];
        out.extend_from_slice(session);
        match kind {
            // Handshake: returns a challenge token.
            9 => {
                challenges.retain(|_, (_, t)| t.elapsed() < Duration::from_secs(30));
                let token = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as i32 ^ peer.port() as i32) & 0x7fff_ffff;
                challenges.insert(peer.ip(), (token, Instant::now()));
                out.extend_from_slice(token.to_string().as_bytes());
                out.push(0);
            }
            // Basic (11 bytes) or full (15 bytes) stats.
            0 if n >= 11 => {
                let token = i32::from_be_bytes(req[7..11].try_into().unwrap());
                if challenges.get(&peer.ip()).map(|c| c.0) != Some(token) {
                    continue;
                }
                let motd = strip_codes(server.config.motd.lines().next().unwrap_or(""));
                let players = server.online();
                let port = server.config.port;
                if n >= 15 {
                    out.extend_from_slice(b"splitnum\0\x80\0");
                    let kv = [
                        ("hostname", motd),
                        ("gametype", "SMP".into()),
                        ("game_id", "MINECRAFT".into()),
                        ("version", VERSION_NAME.into()),
                        ("plugins", "Bambietta".into()),
                        ("map", server.config.world_folder.clone()),
                        ("numplayers", players.len().to_string()),
                        ("maxplayers", server.config.max_players.to_string()),
                        ("hostport", port.to_string()),
                        ("hostip", server.config.host.clone()),
                    ];
                    for (k, v) in kv {
                        out.extend_from_slice(k.as_bytes());
                        out.push(0);
                        out.extend_from_slice(v.as_bytes());
                        out.push(0);
                    }
                    out.extend_from_slice(b"\0\x01player_\0\0");
                    for p in &players {
                        out.extend_from_slice(p.name.as_bytes());
                        out.push(0);
                    }
                    out.push(0);
                } else {
                    for v in [motd, "SMP".into(), server.config.world_folder.clone(), players.len().to_string(), server.config.max_players.to_string()] {
                        out.extend_from_slice(v.as_bytes());
                        out.push(0);
                    }
                    out.extend_from_slice(&port.to_le_bytes());
                    out.extend_from_slice(server.config.host.as_bytes());
                    out.push(0);
                }
            }
            _ => continue,
        }
        let _ = socket.send_to(&out, peer).await;
    }
}

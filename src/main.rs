//! Bambietta Server: a Minecraft Java 1.20.4 server in Rust, reachable over TCP and WebSocket.
//! A port of the ideas from PrismarineJS/flying-squid.

mod config;
mod data;
mod gameplay;
mod item;
mod net;
mod protocol;
mod server;
mod world;

use anyhow::Result;
use net::listener::{self, Mode};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let path = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("config.json"));
    let config = config::Config::load(&path)?;
    let _ = server::admin::CONFIG_PATH.set(path.clone());
    if config.online_mode {
        tokio::task::spawn_blocking(server::auth::init).await??;
        info!("modo online: las cuentas se verifican con Mojang{}", if config.online_mode_ws { "" } else { " (los clientes WebSocket entran en modo offline)" });
    }
    info!("Bambietta Server · Minecraft {} (protocolo {})", protocol::VERSION_NAME, protocol::PROTOCOL_VERSION);

    let ip: std::net::IpAddr = config.host.parse()?;
    let srv = server::Server::new(config.clone())?;
    server::mobs::load(&srv);
    server::raids::load(&srv);
    server::vehicles::load(&srv);
    server::maps::load(&std::path::PathBuf::from(&config.world_folder));

    tokio::spawn(srv.clone().tick_loop());
    tokio::spawn(srv.clone().autosave_loop());

    let tls = net::tls::acceptor(&config)?;
    let main_addr = SocketAddr::new(ip, config.port);
    let mut tasks = vec![tokio::spawn(listener::serve(srv.clone(), main_addr, Mode::Auto, tls.clone()))];
    if let Some(p) = config.ws_port {
        tasks.push(tokio::spawn(listener::serve(srv.clone(), SocketAddr::new(ip, p), Mode::WebSocket, tls)));
    }
    if config.enable_query {
        let addr = SocketAddr::new(ip, config.query_port.unwrap_or(config.port));
        tasks.push(tokio::spawn(server::query::serve(srv.clone(), addr)));
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => info!("apagando..."),
        r = futures_util::future::select_all(tasks) => {
            match r.0 {
                Ok(Err(e)) => error!("listener caído: {e:#}"),
                Err(e) => error!("listener caído: {e}"),
                Ok(Ok(())) => {}
            }
        }
    }
    let s = srv.clone();
    tokio::task::spawn_blocking(move || s.shutdown()).await?;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    Ok(())
}

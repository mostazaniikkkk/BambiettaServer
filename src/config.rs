//! Configuration (`config.json`), with flying-squid-style keys.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub motd: String,
    pub host: String,
    /// Main port: accepts vanilla TCP and WebSocket at the same time.
    pub port: u16,
    /// Additional WebSocket-only port (optional).
    #[serde(rename = "ws-port")]
    pub ws_port: Option<u16>,
    /// Allowed WebSocket origins (empty = any).
    #[serde(rename = "ws-allowed-origins")]
    pub ws_allowed_origins: Vec<String>,
    #[serde(rename = "max-players")]
    pub max_players: usize,
    #[serde(rename = "online-mode")]
    pub online_mode: bool,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    #[serde(rename = "gameMode")]
    pub game_mode: u8,
    pub difficulty: u8,
    #[serde(rename = "worldFolder")]
    pub world_folder: String,
    pub generation: Generation,
    #[serde(rename = "view-distance")]
    pub view_distance: i32,
    #[serde(rename = "kickTimeout")]
    pub kick_timeout: u64,
    /// zlib compression threshold (-1 disables it).
    #[serde(rename = "compression-threshold")]
    pub compression_threshold: i32,
    /// Threshold specific to WebSocket clients (falls back to the general one if missing).
    #[serde(rename = "ws-compression-threshold")]
    pub ws_compression_threshold: Option<i32>,
    #[serde(rename = "player-list-text")]
    pub player_list_text: PlayerListText,
    #[serde(rename = "everybody-op")]
    pub everybody_op: bool,
    /// Query protocol (GameSpy4) over UDP, like vanilla's `enable-query`.
    #[serde(rename = "enable-query")]
    pub enable_query: bool,
    #[serde(rename = "query-port")]
    pub query_port: Option<u16>,
    /// Movement validation (speed, flight, noclip).
    #[serde(rename = "anti-cheat")]
    pub anti_cheat: bool,
    /// Natural spawning of monsters (at night) and animals.
    #[serde(rename = "spawn-monsters")]
    pub spawn_monsters: bool,
    #[serde(rename = "spawn-animals")]
    pub spawn_animals: bool,
    /// PEM certificate and key for `wss://` (same port).
    #[serde(rename = "tls-cert")]
    pub tls_cert: Option<String>,
    #[serde(rename = "tls-key")]
    pub tls_key: Option<String>,
    /// Generates a self-signed certificate (development only) if there is no tls-cert/tls-key.
    #[serde(rename = "tls-self-signed")]
    pub tls_self_signed: bool,
    /// Extra names for the self-signed certificate (domain or LAN IP).
    #[serde(rename = "tls-hostnames")]
    pub tls_hostnames: Vec<String>,
    /// Simultaneous connections per IP (0 = unlimited; localhost doesn't count).
    #[serde(rename = "max-connections-per-ip")]
    pub max_connections_per_ip: usize,
    /// New connections per IP every 10 s (0 = unlimited).
    #[serde(rename = "connections-per-ip-per-10s")]
    pub connections_per_ip_per_10s: usize,
    /// Packets per second from a client before kicking it (0 = unlimited).
    #[serde(rename = "max-packets-per-second")]
    pub max_packets_per_second: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Generation {
    /// `noise` (hills, water and trees) or `flat` (superflat).
    pub name: String,
    pub options: GenerationOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GenerationOptions {
    pub seed: i64,
    /// Layers for `flat`, from bottom to top: [block, thickness].
    pub layers: Vec<(String, i32)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerListText {
    pub header: String,
    pub footer: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            motd: "§dBambietta Server§r\n§7Minecraft 1.20.4 · TCP + WebSocket".into(),
            host: "0.0.0.0".into(),
            port: 25565,
            ws_port: None,
            ws_allowed_origins: vec![],
            max_players: 20,
            online_mode: false,
            game_mode: 1,
            difficulty: 1,
            world_folder: "world".into(),
            generation: Generation::default(),
            view_distance: 8,
            kick_timeout: 30_000,
            compression_threshold: 256,
            ws_compression_threshold: None,
            player_list_text: PlayerListText::default(),
            everybody_op: true,
            enable_query: true,
            query_port: None,
            anti_cheat: true,
            spawn_monsters: true,
            spawn_animals: true,
            tls_cert: None,
            tls_key: None,
            tls_self_signed: false,
            tls_hostnames: vec![],
            max_connections_per_ip: 5,
            connections_per_ip_per_10s: 15,
            max_packets_per_second: 500,
        }
    }
}

impl Default for Generation {
    fn default() -> Self {
        Self { name: "noise".into(), options: GenerationOptions::default() }
    }
}

impl Default for GenerationOptions {
    fn default() -> Self {
        Self {
            seed: 20_200_420,
            layers: vec![("bedrock".into(), 1), ("dirt".into(), 2), ("grass_block".into(), 1)],
        }
    }
}

impl Default for PlayerListText {
    fn default() -> Self {
        Self { header: "§dBambietta Server".into(), footer: "§7TCP + WebSocket · 1.20.4".into() }
    }
}

impl Config {
    /// Loads `path`; if it doesn't exist, creates it with the default values.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let cfg = Config::default();
            std::fs::write(path, serde_json::to_string_pretty(&cfg)?)?;
            return Ok(cfg);
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).with_context(|| format!("config inválida: {}", path.display()))
    }
}

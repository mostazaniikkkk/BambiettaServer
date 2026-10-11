//! Minecraft Java 1.20.4 protocol (protocol version 765).

// Complete tables and primitives even though the server does not use all of them yet.
#![allow(dead_code)]

pub mod buf;
pub mod nbt;

pub use buf::{packet, BlockPos, Reader, Writer};
pub use nbt::Nbt;

pub const PROTOCOL_VERSION: i32 = 765;
pub const VERSION_NAME: &str = "1.20.4";

/// Text component as NBT (1.20.3+). Accepts legacy formatting codes with `§`.
pub fn text(s: &str) -> Nbt {
    Nbt::Compound(vec![("text".into(), Nbt::String(s.into()))])
}

/// Text component as JSON (status and login states).
pub fn text_json(s: &str) -> String {
    serde_json::json!({ "text": s }).to_string()
}

/// Packet IDs (protocol 765). Names according to minecraft-data.
pub mod id {
    pub mod handshake {
        pub const SET_PROTOCOL: i32 = 0x00;
    }
    pub mod status {
        // server -> client
        pub const SERVER_INFO: i32 = 0x00;
        pub const PONG: i32 = 0x01;
        // client -> server
        pub const REQUEST: i32 = 0x00;
        pub const PING: i32 = 0x01;
    }
    pub mod login {
        pub const DISCONNECT: i32 = 0x00;
        pub const SUCCESS: i32 = 0x02;
        pub const COMPRESS: i32 = 0x03;

        pub const LOGIN_START: i32 = 0x00;
        pub const LOGIN_ACKNOWLEDGED: i32 = 0x03;
    }
    pub mod config {
        pub const CUSTOM_PAYLOAD: i32 = 0x00;
        pub const DISCONNECT: i32 = 0x01;
        pub const FINISH_CONFIGURATION: i32 = 0x02;
        pub const KEEP_ALIVE: i32 = 0x03;
        pub const REGISTRY_DATA: i32 = 0x05;
        pub const FEATURE_FLAGS: i32 = 0x08;
        pub const TAGS: i32 = 0x09;

        pub const C_SETTINGS: i32 = 0x00;
        pub const C_FINISH_CONFIGURATION: i32 = 0x02;
    }
    /// Server -> client (play).
    pub mod play {
        pub const SPAWN_ENTITY: i32 = 0x01;
        pub const SPAWN_XP_ORB: i32 = 0x02;
        pub const ANIMATION: i32 = 0x03;
        pub const ACK_DIGGING: i32 = 0x05;
        pub const BLOCK_ACTION: i32 = 0x08;
        pub const BLOCK_CHANGE: i32 = 0x09;
        pub const DIFFICULTY: i32 = 0x0b;
        pub const CHUNK_BATCH_FINISHED: i32 = 0x0c;
        pub const CHUNK_BATCH_START: i32 = 0x0d;
        pub const DECLARE_COMMANDS: i32 = 0x11;
        pub const CLOSE_WINDOW: i32 = 0x12;
        pub const WINDOW_ITEMS: i32 = 0x13;
        pub const WINDOW_PROPERTY: i32 = 0x14;
        pub const SET_SLOT: i32 = 0x15;
        pub const DAMAGE_EVENT: i32 = 0x19;
        pub const ENTITY_STATUS: i32 = 0x1d;
        pub const CUSTOM_PAYLOAD: i32 = 0x18;
        pub const KICK_DISCONNECT: i32 = 0x1b;
        pub const EXPLOSION: i32 = 0x1e;
        pub const UNLOAD_CHUNK: i32 = 0x1f;
        pub const GAME_STATE_CHANGE: i32 = 0x20;
        pub const KEEP_ALIVE: i32 = 0x24;
        pub const MAP_CHUNK: i32 = 0x25;
        pub const WORLD_EVENT: i32 = 0x26;
        pub const LOGIN: i32 = 0x29;
        pub const ENTITY_LOOK: i32 = 0x2e;
        pub const OPEN_WINDOW: i32 = 0x31;
        pub const PING_RESPONSE: i32 = 0x34;
        pub const ABILITIES: i32 = 0x36;
        pub const DEATH_COMBAT_EVENT: i32 = 0x3a;
        pub const PLAYER_REMOVE: i32 = 0x3b;
        pub const PLAYER_INFO: i32 = 0x3c;
        pub const POSITION: i32 = 0x3e;
        pub const ENTITY_DESTROY: i32 = 0x40;
        pub const RESPAWN: i32 = 0x45;
        pub const ENTITY_HEAD_ROTATION: i32 = 0x46;
        pub const HELD_ITEM_SLOT: i32 = 0x51;
        pub const UPDATE_VIEW_POSITION: i32 = 0x52;
        pub const SPAWN_POSITION: i32 = 0x54;
        pub const ENTITY_METADATA: i32 = 0x56;
        pub const ENTITY_VELOCITY: i32 = 0x58;
        pub const ENTITY_EQUIPMENT: i32 = 0x59;
        pub const EXPERIENCE: i32 = 0x5a;
        pub const UPDATE_HEALTH: i32 = 0x5b;
        pub const UPDATE_TIME: i32 = 0x62;
        pub const ENTITY_SOUND_EFFECT: i32 = 0x65;
        pub const SOUND_EFFECT: i32 = 0x66;
        pub const SYSTEM_CHAT: i32 = 0x69;
        pub const PLAYERLIST_HEADER: i32 = 0x6a;
        pub const COLLECT: i32 = 0x6c;
        pub const ENTITY_TELEPORT: i32 = 0x6d;
        pub const TILE_ENTITY_DATA: i32 = 0x07;
        pub const BOSS_BAR: i32 = 0x0a;
        pub const HURT_ANIMATION: i32 = 0x22;
        pub const WORLD_PARTICLES: i32 = 0x27;
        pub const MAP: i32 = 0x2a;
        pub const TRADE_LIST: i32 = 0x2b;
        pub const VEHICLE_MOVE: i32 = 0x2f;
        pub const OPEN_BOOK: i32 = 0x30;
        pub const OPEN_SIGN_ENTITY: i32 = 0x32;
        pub const REMOVE_ENTITY_EFFECT: i32 = 0x41;
        pub const SET_PASSENGERS: i32 = 0x5d;
        pub const ENTITY_UPDATE_ATTRIBUTES: i32 = 0x71;
        pub const ENTITY_EFFECT: i32 = 0x72;
        pub const DECLARE_RECIPES: i32 = 0x73;
        pub const SET_COOLDOWN: i32 = 0x16;
    }
    /// Client -> server (play).
    pub mod play_in {
        pub const TELEPORT_CONFIRM: i32 = 0x00;
        pub const CHAT_COMMAND: i32 = 0x04;
        pub const CHAT_MESSAGE: i32 = 0x05;
        pub const CLIENT_COMMAND: i32 = 0x08;
        pub const SETTINGS: i32 = 0x09;
        pub const ENCHANT_ITEM: i32 = 0x0c;
        pub const WINDOW_CLICK: i32 = 0x0d;
        pub const CLOSE_WINDOW: i32 = 0x0e;
        pub const USE_ENTITY: i32 = 0x13;
        pub const KEEP_ALIVE: i32 = 0x15;
        pub const POSITION: i32 = 0x17;
        pub const POSITION_LOOK: i32 = 0x18;
        pub const LOOK: i32 = 0x19;
        pub const FLYING: i32 = 0x1a;
        pub const PING_REQUEST: i32 = 0x1e;
        pub const ABILITIES: i32 = 0x20;
        pub const BLOCK_DIG: i32 = 0x21;
        pub const ENTITY_ACTION: i32 = 0x22;
        pub const HELD_ITEM_SLOT: i32 = 0x2c;
        pub const SET_CREATIVE_SLOT: i32 = 0x2f;
        pub const ARM_ANIMATION: i32 = 0x33;
        pub const BLOCK_PLACE: i32 = 0x35;
        pub const USE_ITEM: i32 = 0x36;
        pub const EDIT_BOOK: i32 = 0x11;
        pub const VEHICLE_MOVE: i32 = 0x1b;
        pub const STEER_BOAT: i32 = 0x1c;
        pub const STEER_VEHICLE: i32 = 0x23;
        pub const NAME_ITEM: i32 = 0x27;
        pub const SELECT_TRADE: i32 = 0x2a;
        pub const UPDATE_SIGN: i32 = 0x32;
    }
}

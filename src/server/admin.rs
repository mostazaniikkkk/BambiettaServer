//! Whitelist, bans (players and IPs) and operators, in vanilla's files next to the server:
//! `whitelist.json`, `banned-players.json`, `banned-ips.json` and `ops.json` (same format,
//! so they can be copied from or to a vanilla server). Whether the whitelist is on is the
//! `white-list` key of the config file, which `/whitelist on|off` updates.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Path of the config file (to save `white-list`).
pub static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Named {
    pub uuid: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerBan {
    pub uuid: String,
    pub name: String,
    pub created: String,
    pub source: String,
    pub expires: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IpBan {
    pub ip: String,
    pub created: String,
    pub source: String,
    pub expires: String,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Op {
    pub uuid: String,
    pub name: String,
    pub level: i32,
    #[serde(rename = "bypassesPlayerLimit")]
    pub bypasses_player_limit: bool,
}

#[derive(Default)]
pub struct Admin {
    pub whitelist_on: bool,
    pub whitelist: Vec<Named>,
    pub bans: Vec<PlayerBan>,
    pub ip_bans: Vec<IpBan>,
    pub ops: Vec<Op>,
}

fn load<T: for<'a> Deserialize<'a>>(file: &str) -> Vec<T> {
    std::fs::read_to_string(file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn save<T: Serialize>(file: &str, v: &[T]) {
    if let Err(e) = std::fs::write(file, serde_json::to_string_pretty(v).unwrap_or_else(|_| "[]".into())) {
        tracing::warn!("no se pudo guardar {file}: {e}");
    }
}

/// Date in vanilla's format ("2024-01-31 18:05:00 +0000").
fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} +0000", rem / 3600, rem % 3600 / 60, rem % 60)
}

impl Admin {
    pub fn load(whitelist_on: bool) -> Self {
        Self { whitelist_on, whitelist: load("whitelist.json"), bans: load("banned-players.json"), ip_bans: load("banned-ips.json"), ops: load("ops.json") }
    }

    fn save_all(&self) {
        save("whitelist.json", &self.whitelist);
        save("banned-players.json", &self.bans);
        save("banned-ips.json", &self.ip_bans);
        save("ops.json", &self.ops);
    }

    /// Why this player may not join, if it may not.
    pub fn refuse(&self, name: &str, uuid: &str, ip: &str) -> Option<String> {
        if let Some(b) = self.bans.iter().find(|b| b.uuid == uuid || b.name.eq_ignore_ascii_case(name)) {
            return Some(format!("§cEstás baneado de este servidor.\n§7{}", b.reason));
        }
        if let Some(b) = self.ip_bans.iter().find(|b| b.ip == ip) {
            return Some(format!("§cTu IP está baneada de este servidor.\n§7{}", b.reason));
        }
        if self.whitelist_on && !self.is_op(uuid) && !self.whitelist.iter().any(|w| w.uuid == uuid || w.name.eq_ignore_ascii_case(name)) {
            return Some("§cNo estás en la lista blanca de este servidor.".into());
        }
        None
    }

    pub fn is_op(&self, uuid: &str) -> bool {
        self.ops.iter().any(|o| o.uuid == uuid)
    }

    pub fn ban(&mut self, uuid: String, name: String, source: &str, reason: &str) {
        self.bans.retain(|b| b.uuid != uuid);
        self.bans.push(PlayerBan { uuid, name, created: now(), source: source.into(), expires: "forever".into(), reason: reason.into() });
        self.save_all();
    }

    pub fn pardon(&mut self, name: &str) -> bool {
        let before = self.bans.len();
        self.bans.retain(|b| !b.name.eq_ignore_ascii_case(name));
        self.save_all();
        self.bans.len() != before
    }

    pub fn ban_ip(&mut self, ip: String, source: &str, reason: &str) {
        self.ip_bans.retain(|b| b.ip != ip);
        self.ip_bans.push(IpBan { ip, created: now(), source: source.into(), expires: "forever".into(), reason: reason.into() });
        self.save_all();
    }

    pub fn pardon_ip(&mut self, ip: &str) -> bool {
        let before = self.ip_bans.len();
        self.ip_bans.retain(|b| b.ip != ip);
        self.save_all();
        self.ip_bans.len() != before
    }

    pub fn whitelist_add(&mut self, uuid: String, name: String) -> bool {
        if self.whitelist.iter().any(|w| w.uuid == uuid) {
            return false;
        }
        self.whitelist.push(Named { uuid, name });
        self.save_all();
        true
    }

    pub fn whitelist_remove(&mut self, name: &str) -> bool {
        let before = self.whitelist.len();
        self.whitelist.retain(|w| !w.name.eq_ignore_ascii_case(name));
        self.save_all();
        self.whitelist.len() != before
    }

    pub fn op(&mut self, uuid: String, name: String) -> bool {
        if self.is_op(&uuid) {
            return false;
        }
        self.ops.push(Op { uuid, name, level: 4, bypasses_player_limit: false });
        self.save_all();
        true
    }

    pub fn deop(&mut self, name: &str) -> bool {
        let before = self.ops.len();
        self.ops.retain(|o| !o.name.eq_ignore_ascii_case(name));
        self.save_all();
        self.ops.len() != before
    }

    /// `/whitelist on|off`: remembered in the config file (`white-list`).
    pub fn set_whitelist(&mut self, on: bool) {
        self.whitelist_on = on;
        let Some(path) = CONFIG_PATH.get() else { return };
        if let Err(e) = patch_config(path, "white-list", serde_json::Value::Bool(on)) {
            tracing::warn!("no se pudo guardar white-list en {}: {e}", path.display());
        }
    }
}

/// Sets one key of the config file, keeping the rest.
fn patch_config(path: &Path, key: &str, value: serde_json::Value) -> std::io::Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| "{}".into());
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
    if let Some(o) = v.as_object_mut() {
        o.insert(key.to_string(), value);
    }
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap_or(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_banned_and_unlisted() {
        let mut a = Admin::default();
        assert!(a.refuse("Steve", "u1", "1.2.3.4").is_none());
        a.bans.push(PlayerBan { uuid: "u1".into(), name: "Steve".into(), created: now(), source: "t".into(), expires: "forever".into(), reason: "griefing".into() });
        assert!(a.refuse("steve", "x", "1.2.3.4").unwrap().contains("griefing"));
        a.ip_bans.push(IpBan { ip: "9.9.9.9".into(), created: now(), source: "t".into(), expires: "forever".into(), reason: "r".into() });
        assert!(a.refuse("Alex", "u2", "9.9.9.9").is_some());
        a.whitelist_on = true;
        assert!(a.refuse("Alex", "u2", "1.1.1.1").is_some());
        a.whitelist.push(Named { uuid: "u2".into(), name: "Alex".into() });
        assert!(a.refuse("Alex", "u2", "1.1.1.1").is_none());
        assert_eq!(now().len(), 25);
    }
}

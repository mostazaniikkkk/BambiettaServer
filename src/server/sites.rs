//! Life in structures: the first time a player comes close to an End city, an ocean
//! monument, a bastion or some ocean ruins, the mobs that live there appear (shulkers, elder
//! guardians, piglins and brutes, drowned...) and its item frames are hung (the End ship's
//! elytra). Remembered in `sites.json`, so it only happens once.

use super::mobs::{self, MobKind, Seen};
use super::Server;
use crate::world::Dimension;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

type Key = (u8, i32, i32, i32);

static DONE: Mutex<Option<HashSet<Key>>> = Mutex::new(None);

fn file(server: &Server) -> std::path::PathBuf {
    server.world(Dimension::Overworld).lock().unwrap().dir().join("sites.json")
}

fn done(server: &Server) -> std::sync::MutexGuard<'static, Option<HashSet<Key>>> {
    let mut g = DONE.lock().unwrap();
    if g.is_none() {
        let list: Vec<Key> = std::fs::read_to_string(file(server)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        *g = Some(list.into_iter().collect());
    }
    g
}

/// Every five seconds: structures near players get their inhabitants.
pub fn populate(server: &Arc<Server>, seen: &[Seen]) {
    let mut todo = Vec::new();
    for s in seen {
        let sites = server.world(s.dim).lock().unwrap().generator().sites_near(s.pos[0] as i32, s.pos[2] as i32);
        for site in sites {
            if site.mobs.is_empty() && site.frames.is_empty() {
                continue;
            }
            if !site.contains(s.pos[0] as i32, s.pos[1] as i32, s.pos[2] as i32, 48) {
                continue;
            }
            let (a, b, c) = site.id();
            let key = (s.dim.index() as u8, a, b, c);
            if done(server).as_mut().unwrap().insert(key) {
                todo.push((s.dim, site));
            }
        }
    }
    if todo.is_empty() {
        return;
    }
    let list: Vec<Key> = done(server).as_ref().unwrap().iter().copied().collect();
    let _ = std::fs::write(file(server), serde_json::to_string(&list).unwrap_or_default());
    for (dim, site) in todo {
        for &(pos, name) in &site.mobs {
            if let Some(kind) = MobKind::from_name(name) {
                mobs::spawn(server, dim, kind, pos);
            }
        }
        for &(pos, facing, item) in &site.frames {
            let stack = crate::data::get().item_id(item).map(|i| crate::item::ItemStack::new(i, 1));
            super::frames::add(server, dim, pos, facing, stack);
        }
        tracing::debug!("{} poblado en {:?}", site.kind, site.id());
    }
}

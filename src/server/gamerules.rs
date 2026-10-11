//! Game rules (`/gamerule`), saved in `level.dat` like vanilla (`GameRules`, as strings).
//! The ones the server honours:
//! keepInventory, doDaylightCycle, doWeatherCycle, doMobSpawning, doPatrolSpawning,
//! doFireTick, mobGriefing, naturalRegeneration, fallDamage, fireDamage, drowningDamage,
//! showDeathMessages, announceAdvancements, randomTickSpeed, playersSleepingPercentage.

use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Default_ {
    Bool(bool),
    Int(i32),
}

const RULES: &[(&str, Default_)] = &[
    ("keepInventory", Default_::Bool(false)),
    ("doDaylightCycle", Default_::Bool(true)),
    ("doWeatherCycle", Default_::Bool(true)),
    ("doMobSpawning", Default_::Bool(true)),
    ("doPatrolSpawning", Default_::Bool(true)),
    ("doFireTick", Default_::Bool(true)),
    ("mobGriefing", Default_::Bool(true)),
    ("naturalRegeneration", Default_::Bool(true)),
    ("fallDamage", Default_::Bool(true)),
    ("fireDamage", Default_::Bool(true)),
    ("drowningDamage", Default_::Bool(true)),
    ("showDeathMessages", Default_::Bool(true)),
    ("announceAdvancements", Default_::Bool(true)),
    ("randomTickSpeed", Default_::Int(3)),
    ("playersSleepingPercentage", Default_::Int(100)),
];

pub struct GameRules {
    values: BTreeMap<String, String>,
}

impl GameRules {
    /// From `level.dat` (unknown or invalid values keep their default).
    pub fn new(saved: &[(String, String)]) -> Self {
        let mut r = Self { values: BTreeMap::new() };
        for (k, v) in saved {
            let _ = r.set(k, v);
        }
        r
    }

    pub fn names() -> impl Iterator<Item = &'static str> {
        RULES.iter().map(|r| r.0)
    }

    fn default_of(name: &str) -> Option<Default_> {
        RULES.iter().find(|r| r.0 == name).map(|r| r.1)
    }

    pub fn bool(&self, name: &str) -> bool {
        match (self.values.get(name), Self::default_of(name)) {
            (Some(v), _) => v == "true",
            (None, Some(Default_::Bool(b))) => b,
            _ => false,
        }
    }

    pub fn int(&self, name: &str) -> i32 {
        match (self.values.get(name).and_then(|v| v.parse().ok()), Self::default_of(name)) {
            (Some(v), _) => v,
            (None, Some(Default_::Int(i))) => i,
            _ => 0,
        }
    }

    /// The value as text (for `/gamerule <name>`).
    pub fn get(&self, name: &str) -> Option<String> {
        match Self::default_of(name)? {
            Default_::Bool(_) => Some(self.bool(name).to_string()),
            Default_::Int(_) => Some(self.int(name).to_string()),
        }
    }

    /// Sets a rule, checking the value's type.
    pub fn set(&mut self, name: &str, value: &str) -> Result<(), String> {
        match Self::default_of(name).ok_or_else(|| format!("Regla desconocida: {name}"))? {
            Default_::Bool(_) if value == "true" || value == "false" => {}
            Default_::Int(_) if value.parse::<i32>().is_ok_and(|v| v >= 0) => {}
            _ => return Err(format!("Valor inválido para {name}: {value}")),
        }
        self.values.insert(name.to_string(), value.to_string());
        Ok(())
    }

    /// All the rules, for `level.dat`.
    pub fn saved(&self) -> Vec<(String, String)> {
        RULES.iter().filter_map(|r| self.get(r.0).map(|v| (r.0.to_string(), v))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_types() {
        let mut r = GameRules::new(&[("keepInventory".into(), "true".into()), ("randomTickSpeed".into(), "nope".into())]);
        assert!(r.bool("keepInventory"));
        assert_eq!(r.int("randomTickSpeed"), 3, "valor inválido: queda el de fábrica");
        assert!(r.set("mobGriefing", "3").is_err());
        assert!(r.set("noSuchRule", "true").is_err());
        r.set("randomTickSpeed", "10").unwrap();
        assert_eq!(r.int("randomTickSpeed"), 10);
        assert!(r.saved().iter().any(|(k, v)| k == "randomTickSpeed" && v == "10"));
    }
}

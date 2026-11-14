// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/arcadenames.js (lookup), as RustyShelf has it.
//! Arcade and Neo Geo games are zips named by their ROM set ("sf2.zip"). FinalBurn Neo's game list says what each set
//! is and which sets are BIOS files; the Electron app keeps the names from it in `data/arcade-names.json`, read here.
//! Without that file the set name is shown.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

pub const SYSTEMS: [&str; 2] = ["arcade", "neogeo"];

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Names {
    /// set name (lower case) → the game's full name
    names: HashMap<String, String>,
    #[serde(default)]
    bios: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Set {
    Bios,
    Game(String),
}

impl Names {
    /// None when the Electron app hasn't fetched the list yet.
    pub fn load(data_dir: &Path) -> Option<Names> {
        let text = std::fs::read_to_string(data_dir.join("arcade-names.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn lookup(&self, set: &str) -> Option<Set> {
        let k = set.to_lowercase();
        if self.bios.contains(&k) {
            return Some(Set::Bios);
        }
        self.names.get(&k).map(|t| Set::Game(t.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let n: Names = serde_json::from_str(r#"{"v":1,"at":0,"names":{"sf2":"Street Fighter II"},"bios":["neogeo"]}"#).unwrap();
        assert_eq!(n.lookup("SF2"), Some(Set::Game("Street Fighter II".into())));
        assert_eq!(n.lookup("neogeo"), Some(Set::Bios));
        assert_eq!(n.lookup("nope"), None);
    }
}

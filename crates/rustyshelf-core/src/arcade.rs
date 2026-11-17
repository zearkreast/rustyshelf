// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/arcadenames.js, as RustyShelf has it.
//! Arcade and Neo Geo games are zips named by their ROM set ("sf2.zip"). FinalBurn Neo's game list (its ClrMame Pro
//! XML, published in the FBNeo repository) says what each set is and which sets are BIOS files. It's fetched once and
//! only the names are kept, in `data/arcade-names.json`, a file the Electron app shares. Without it the set name is
//! shown.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use regex::Regex;
use serde::Deserialize;
use std::sync::LazyLock;

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
    /// None when the list hasn't been fetched yet.
    pub fn load(data_dir: &Path) -> Option<Names> {
        crate::files::read_json_as(&data_dir.join("arcade-names.json"))
    }

    pub fn lookup(&self, set: &str) -> Option<Set> {
        let k = set.to_lowercase();
        if self.bios.contains(&k) {
            return Some(Set::Bios);
        }
        self.names.get(&k).map(|t| Set::Game(t.clone()))
    }
}

fn un_xml(s: &str) -> String {
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new("&(?i:(amp|lt|gt|quot|apos)|#([0-9]+)|#x([0-9a-f]+));").unwrap());
    let code = |n: Option<u32>| n.and_then(char::from_u32).map(String::from);
    ENTITY
        .replace_all(s, |m: &regex::Captures| {
            let named = m.get(1).map(|e| match e.as_str().to_lowercase().as_str() {
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "quot" => "\"",
                _ => "'",
            });
            let number = m.get(2).map(|d| d.as_str().parse().ok()).or(m.get(3).map(|h| u32::from_str_radix(h.as_str(), 16).ok()));
            named.map(String::from).or_else(|| code(number.flatten())).unwrap_or_else(|| m[0].to_string())
        })
        .into_owned()
}

/// Each set with its game's name, in the list's order, and the BIOS sets.
pub type Parsed = (Vec<(String, String)>, Vec<String>);

/// The names in FinalBurn Neo's list and its BIOS sets. None when it holds no games (not the list).
pub fn parse_dat(text: &str) -> Option<Parsed> {
    static GAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<game(?-u:\b)([^>]*)>").unwrap());
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?-u:\b)name="([^"]+)""#).unwrap());
    static BIOS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?-u:\b)isbios="yes""#).unwrap());
    static DESCRIPTION: LazyLock<Regex> = LazyLock::new(|| Regex::new("<description>([^<]*)</description>").unwrap());
    let (mut names, mut bios): (Vec<(String, String)>, Vec<String>) = (Vec::new(), Vec::new());
    let mut at: HashMap<String, usize> = HashMap::new();
    let mut cur: Option<String> = None;
    let mut found = 0;
    for l in text.split('\n') {
        if let Some(g) = GAME.captures(l) {
            cur = NAME.captures(&g[1]).map(|n| n[1].to_lowercase());
            if let Some(set) = cur.take_if(|_| BIOS.is_match(&g[1])) {
                bios.push(set);
            }
        }
        // a game's description may follow on the same line
        if let (Some(set), Some(d)) = (&cur, DESCRIPTION.captures(l)) {
            let title = crate::js::trim(&un_xml(&d[1])).to_string();
            match at.get(set) {
                Some(i) => names[*i].1 = title,
                None => {
                    at.insert(set.clone(), names.len());
                    names.push((set.clone(), title));
                }
            }
            cur = None;
            found += 1;
        }
    }
    (found > 100).then_some((names, bios))
}

const URL_DAT: &str = "https://raw.githubusercontent.com/libretro/FBNeo/master/dats/FinalBurn%20Neo%20(ClrMame%20Pro%20XML%2C%20Arcade%20only).dat";

/// Fetches the names once; true when a new table was saved.
pub fn fetch(data_dir: &Path) -> Result<bool, String> {
    if Names::load(data_dir).is_some() {
        return Ok(false);
    }
    let dat = crate::net::get_bytes(URL_DAT, std::time::Duration::from_secs(60), 64 << 20).map_err(|e| format!("arcade list: {e}"))?.ok_or("arcade list: HTTP 404")?;
    let (names, bios) = parse_dat(&String::from_utf8_lossy(&dat)).ok_or("arcade list: no games in it")?;
    let at = crate::now_ms();
    let table = serde_json::json!({ "v": 1, "at": at, "names": names.into_iter().map(|(k, v)| (k, v.into())).collect::<serde_json::Map<String, serde_json::Value>>(), "bios": bios });
    let file = data_dir.join("arcade-names.json");
    crate::files::write_atomic(&file, table.to_string()).map_err(|e| e.to_string())?;
    Ok(true)
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

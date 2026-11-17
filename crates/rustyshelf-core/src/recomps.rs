// Based on ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf by
// sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source): ported from recompDetect in its main.js and
// detectConsole in its main/covers.js.
//! Recomps and ports put on their console's shelf by themselves (Settings → Custom games → Put recomps on their
//! console's shelf): Quiver's games and the ones added by hand with no shelf chosen, when LaunchBox's database has a
//! game of exactly that name on one platform only, a console's. Only an index already built is read.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::covers::{self, LB_PC};
use crate::lbindex::norm;
use crate::pcgames::PcGame;
use crate::systems::systems;
use crate::{boxart, js};

/// A title nothing was found for is looked up again after this long (ms).
const MISS_MS: f64 = 30.0 * 86_400_000.0;
const FILE: &str = "recomp-detect.json";

/// A game the lookup may move: Quiver's or one added by hand, with no shelf chosen, and not a port.
pub fn candidate(g: &PcGame) -> bool {
    matches!(g.source, "custom" | "quiver") && g.shelf.is_none() && !g.port && g.port_of.is_empty()
}

/// What a game's answer is kept under in `data/recomp-detect.json`.
pub fn key(id: &str, title: &str) -> String {
    format!("{id}|{title}")
}

/// The title a game is looked up and kept under: its own, not its rename, as in Zak's fork's file.
pub fn title(g: &PcGame) -> &str {
    g.file_title.as_deref().unwrap_or(&g.title)
}

fn read(data: &Path) -> Map<String, Value> {
    match std::fs::read(data.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// Whether LaunchBox's name is the title, or the title with an edition note off.
fn exact(title: &str, name: &str) -> bool {
    let n = norm(name);
    covers::lookup_names(title).iter().any(|t| covers::match_title(&[n.as_str()], t, &|_| 0, true).is_some())
}

/// The console found for each game, by `key`. An answer that names another game than its title is left out: Zak's
/// fork writes some from a recomp's project name.
pub fn found(data: &Path) -> HashMap<String, String> {
    read(data)
        .into_iter()
        .filter_map(|(k, v)| {
            let sys = js::str_of(v.get("sys")).filter(|s| !s.is_empty())?.to_string();
            exact(k.split_once('|')?.1, js::str_of(v.get("name")).unwrap_or("")).then_some((k, sys))
        })
        .collect()
}

/// The LaunchBox platforms a name is counted on, each with its console when that isn't a standalone one.
fn platforms() -> Vec<(&'static str, Option<&'static str>)> {
    let mut out: Vec<(&'static str, Option<&'static str>)> = Vec::new();
    for c in &systems().consoles {
        if let Some(p) = covers::launchbox_platform(&c.id).filter(|p| !out.iter().any(|o| o.0 == *p)) {
            out.push((p, (!c.standalone).then_some(c.id.as_str())));
        }
    }
    for p in covers::pc_box_platforms().iter().map(|p| p.1.as_str()).chain([LB_PC]) {
        if !out.iter().any(|o| o.0 == p) {
            out.push((p, None));
        }
    }
    out
}

/// Looks up the games (id, title) not answered yet, and again a month after nothing was found for one. Returns
/// whether one was found for a console of `shelves`.
pub fn detect(data: &Path, cache_dir: &Path, games: &[(String, String)], shelves: &HashSet<&str>, now: u64) -> bool {
    let mut memo = read(data);
    let due: Vec<&(String, String)> = games
        .iter()
        .filter(|(id, title)| match memo.get(&key(id, title)) {
            Some(v) => js::str_of(v.get("sys")).is_none_or(str::is_empty) && now as f64 - js::number(v.get("at")) >= MISS_MS,
            None => true,
        })
        .collect();
    if due.is_empty() {
        return false;
    }
    let Some(mut idx) = boxart::ready_index(cache_dir) else { return false };
    let names: Vec<Vec<String>> = due.iter().map(|(_, title)| covers::lookup_names(title)).collect();
    // every platform each game is on: (the console, LaunchBox's name)
    let mut hits: Vec<Vec<(Option<&str>, String)>> = vec![Vec::new(); due.len()];
    for (plat, sys) in platforms() {
        if !idx.has(plat) {
            continue;
        }
        let Some(shard) = idx.platform(plat) else { continue };
        let list: Vec<&str> = shard.games.iter().map(|g| g.n.as_str()).collect();
        let art = |i: usize| shard.images.get(&shard.games[i].id).map_or(0, |l| l.iter().filter(|im| im.r#type.starts_with("Box - ") || im.r#type.starts_with("Cart - ") || im.r#type == "Disc").count());
        for (i, looked) in names.iter().enumerate() {
            if let Some(at) = looked.iter().find_map(|t| covers::match_title(&list, t, &art, true)) {
                hits[i].push((sys, shard.games[at].name.clone()));
            }
        }
    }
    let mut placed = false;
    for ((id, title), hit) in due.into_iter().zip(hits) {
        let answer = match hit.as_slice() {
            [(Some(sys), name)] => {
                placed |= shelves.contains(sys);
                json!({ "sys": sys, "name": name })
            }
            _ => json!({ "at": now }),
        };
        memo.insert(key(id, title), answer);
    }
    let mut out = Vec::new();
    let tmp = data.join(format!("{FILE}.tmp"));
    if Value::Object(memo).serialize(&mut serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(b" "))).is_ok() && std::fs::write(&tmp, out).is_ok() {
        let _ = std::fs::rename(&tmp, data.join(FILE));
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lb(sys: &str) -> &'static str {
        covers::launchbox_platform(sys).unwrap()
    }

    fn cache_with(dir: &Path) {
        let game = |id: &str, name: &str| json!({ "id": id, "name": name, "n": norm(name) });
        let games = json!({
            lb("n64"): [game("1", "Zeta Run"), game("2", "Omega Quest"), game("3", "Native Thing"), game("6", "Bare Quest")],
            lb("gbc"): [game("4", "Omega Quest")],
            lb("snes"): [game("7", "Bare Quest")],
            LB_PC: [game("5", "Native Thing")],
        });
        let plats: Vec<&str> = games.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(plats.len(), 4);
        std::fs::write(dir.join("launchbox-index.json"), json!({ "v": 8, "at": 1, "games": games, "images": {} }).to_string()).unwrap();
    }

    fn g(title: &str) -> (String, String) {
        (format!("custom-{}", crate::pcgames::slug(title)), title.to_string())
    }

    #[test]
    fn a_game_on_one_console_only_is_placed() {
        let t = tempfile::tempdir().unwrap();
        let (data, cache) = (t.path().join("data"), t.path().join("cache"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        let games = [g("Zeta Run"), g("Omega Quest"), g("Native Thing"), g("Zeta Run - Definitive Edition"), g("kart")];
        let shelves = HashSet::from(["snes", "gbc"]);
        assert!(!detect(&data, &cache, &games, &shelves, 1_000), "no index yet");
        assert!(!data.join(FILE).exists());
        std::fs::write(data.join(FILE), json!({ "custom-kart|kart": { "sys": "sw2", "name": "Mario Kart World" } }).to_string()).unwrap();
        cache_with(&cache);
        assert!(!detect(&data, &cache, &games, &shelves, 1_000), "found for the N64, which has no shelf");
        let memo: Value = serde_json::from_slice(&std::fs::read(data.join(FILE)).unwrap()).unwrap();
        assert_eq!(
            memo,
            json!({
                "custom-kart|kart": { "sys": "sw2", "name": "Mario Kart World" },
                "custom-zeta-run|Zeta Run": { "sys": "n64", "name": "Zeta Run" },
                "custom-omega-quest|Omega Quest": { "at": 1_000 },
                "custom-native-thing|Native Thing": { "at": 1_000 },
                "custom-zeta-run-definitive-edition|Zeta Run - Definitive Edition": { "sys": "n64", "name": "Zeta Run" },
            })
        );
        assert_eq!(found(&data), HashMap::from([("custom-zeta-run|Zeta Run".to_string(), "n64".to_string()), ("custom-zeta-run-definitive-edition|Zeta Run - Definitive Edition".to_string(), "n64".to_string())]));
        // a miss waits a month; an answer stays
        assert!(!detect(&data, &cache, &[g("Bare Quest")], &shelves, 2_000), "on two consoles");
        assert!(!detect(&data, &cache, &games, &HashSet::from(["n64"]), 1_000 + 29 * 86_400_000));
        std::fs::write(cache.join("launchbox-index.json"), json!({ "v": 8, "at": 2, "games": { lb("gbc"): [{ "id": "8", "name": "Native Thing", "n": "native thing" }] }, "images": {} }).to_string()).unwrap();
        assert!(detect(&data, &cache, &games, &HashSet::from(["gbc"]), 1_000 + 31 * 86_400_000));
        assert_eq!(found(&data).get("custom-native-thing|Native Thing").map(String::as_str), Some("gbc"));
    }

    #[test]
    fn only_games_with_no_shelf_that_aren_t_ports() {
        let pc = |source: &'static str, shelf: Option<&str>, port: bool, port_of: &str| PcGame { source, shelf: shelf.map(String::from), port, port_of: port_of.into(), ..PcGame::default() };
        assert!(candidate(&pc("custom", None, false, "")) && candidate(&pc("quiver", None, false, "")));
        assert!(!candidate(&pc("quiver", None, true, "n64")), "Quiver's port");
        assert!(!candidate(&pc("custom", Some("ports"), true, "")) && !candidate(&pc("custom", Some("snes"), false, "")) && !candidate(&pc("custom", None, false, "n64")));
        assert!(!candidate(&pc("folder", None, false, "")) && !candidate(&pc("epic", None, false, "")));
    }
}

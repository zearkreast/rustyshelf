// Based on ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf by
// sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source): ported from pcBack in its main/covers.js and
// pcBacksSoon in its main.js.
//! The back of a PC game's box from LaunchBox's PC games (its Windows platform), for the games with none; a port or
//! recomp's from its console's release. Only an index already built is read: nothing here downloads the database.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::boxart::{self, Index};
use crate::covers::{self, LB_PC};
use crate::lbindex::ShardFile;
use crate::scanner::{Game, Source};
use crate::{js, pccovers};

/// The games one background run looks up at most.
pub const RUN_MAX: usize = 30;
/// A title LaunchBox had nothing for is looked up again after this long.
const MISS_MS: f64 = 30.0 * 86_400_000.0;

/// A PC game whose back is wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub id: String,
    pub title: String,
    /// a store's game: its title is the game's own name, so close calls are taken
    pub store: bool,
    /// LaunchBox's platform it's looked up on: Windows, or a port's console
    pub plat: &'static str,
}

impl Want {
    pub fn of(g: &Game) -> Option<Want> {
        let (store, plat) = match &g.source {
            Source::Steam { .. } => (true, LB_PC),
            // a port or recomp is named after the console game it's made from
            Source::Pc(p) if !p.port_of.is_empty() => (true, covers::launchbox_platform(&p.port_of)?),
            Source::Pc(p) => (!p.port && matches!(p.source, "epic" | "gog" | "ubisoft" | "ea" | "xbox" | "folder" | "hoyoplay"), LB_PC),
            _ => return None,
        };
        (!g.id.is_empty() && !g.title.is_empty()).then(|| Want { id: g.id.clone(), title: g.title.clone(), store, plat })
    }
}

pub enum Got {
    /// the picture, and the name of LaunchBox's game it's from
    Back(PathBuf, String),
    Miss,
    /// no index with LaunchBox's PC games
    NotReady,
}

/// LaunchBox's Windows game a title names, and its backs' addresses, the best first.
pub fn find(shard: &ShardFile, title: &str, store: bool, region: &str) -> Option<(String, Vec<String>)> {
    let backs = |i: usize| shard.images.get(&shard.games[i].id).map_or(0, |l| l.iter().filter(|im| im.r#type.starts_with("Box - Back")).count());
    let names: Vec<&str> = shard.games.iter().map(|g| g.n.as_str()).collect();
    let hit = if store { covers::match_name(&names, title, &backs) } else { covers::match_title(&names, title, &backs, true) };
    let g = &shard.games[hit.filter(|i| backs(*i) > 0)?];
    let imgs = shard.images.get(&g.id)?;
    let urls = covers::pick_images(imgs, "Box - Back", region).into_iter().chain(covers::pick_images(imgs, "Box - Back - Reconstructed", region));
    Some((g.name.clone(), urls.map(|im| format!("{}{}", covers::LB_IMG, js::encode_uri(&im.file))).collect()))
}

/// One game's back looked up and downloaded with `get`.
fn look(shard: &ShardFile, want: &Want, region: &str, get: &dyn Fn(&Want, &str) -> Option<PathBuf>) -> Got {
    let Some((name, urls)) = find(shard, &want.title, want.store, region) else { return Got::Miss };
    urls.iter().find_map(|u| get(want, u)).map_or(Got::Miss, |f| Got::Back(f, name))
}

fn index(cache_dir: &Path) -> Option<Index> {
    boxart::ready_index(cache_dir)
}

/// One game's back, asked for by hand: LaunchBox's goes on the case, over the player's own.
pub fn fetch(data: &Path, cache_dir: &Path, want: &Want, region: &str) -> Got {
    let Some(mut idx) = index(cache_dir).filter(|i| i.has(want.plat)) else { return Got::NotReady };
    let Some(shard) = idx.platform(want.plat) else { return Got::NotReady };
    let got = look(shard, want, region, &|w, u| pccovers::fetch_auto_back(data, &w.id, u));
    if matches!(got, Got::Back(..)) {
        pccovers::prefer_auto_back(data, &want.id);
    }
    got
}

/// Backs for these games, behind the shelf: at most RUN_MAX looked up, none for a game the player has a back for or
/// chose "Made-up back" for, and a title with none found waits 30 days (`data/pc-covers/back-miss.json`, as Zak's
/// fork keeps it). Returns how many came in.
pub fn run(data: &Path, cache_dir: &Path, wants: &[Want], region: &str, now: u64) -> usize {
    let Some(mut idx) = index(cache_dir) else { return 0 };
    run_with(data, &mut idx, wants, region, now, &|w, u| pccovers::fetch_auto_back(data, &w.id, u))
}

fn run_with(data: &Path, idx: &mut Index, wants: &[Want], region: &str, now: u64, get: &dyn Fn(&Want, &str) -> Option<PathBuf>) -> usize {
    let file = data.join("pc-covers").join("back-miss.json");
    let mut miss = crate::files::read_object(&file);
    let (mut tried, mut got, mut missed) = (0, 0, false);
    for w in wants {
        if tried >= RUN_MAX {
            break;
        }
        if pccovers::back(data, &w.id).is_some() || pccovers::auto_back(data, &w.id).is_some() || pccovers::made_up_back(data, &w.id) {
            continue;
        }
        let key = format!("{}|{}", w.id, w.title);
        if (now as f64 - js::number(miss.get(&key))) < MISS_MS {
            continue;
        }
        if !idx.has(w.plat) {
            continue;
        }
        let Some(shard) = idx.platform(w.plat) else { continue };
        tried += 1;
        match look(shard, w, region, get) {
            Got::Back(..) => got += 1,
            _ => {
                miss.insert(key, Value::from(now));
                missed = true;
            }
        }
    }
    if missed {
        let tmp = file.with_extension("json.tmp");
        if file.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(&tmp, Value::Object(miss).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    got
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cache_with(dir: &Path, plats: &[&str]) {
        let games: serde_json::Map<String, Value> = plats
            .iter()
            .map(|p| {
                let list = if *p == "Nintendo 64" {
                    json!([{ "id": "4", "name": "The Legend of Zelda: Ocarina of Time", "n": "the legend of zelda ocarina of time" }])
                } else if *p == LB_PC {
                    json!([{ "id": "1", "name": "Zeta Run", "n": "zeta run" }, { "id": "2", "name": "Omega Quest: Deluxe", "n": "omega quest deluxe" }, { "id": "3", "name": "Bare", "n": "bare" }])
                } else {
                    json!([])
                };
                (p.to_string(), list)
            })
            .collect();
        let images = json!({
            "1": [{ "type": "Box - Back", "region": "Europe", "file": "1-eu.jpg" }, { "type": "Box - Back", "region": "North America", "file": "1 us.jpg" }, { "type": "Box - Front", "region": "", "file": "1-f.jpg" }],
            "2": [{ "type": "Box - Back - Reconstructed", "region": "", "file": "2-r.png" }],
            "3": [{ "type": "Box - Front", "region": "", "file": "3-f.jpg" }],
            "4": [{ "type": "Box - Back", "region": "North America", "file": "4-us.jpg" }],
        });
        std::fs::write(dir.join("launchbox-index.json"), json!({ "v": 8, "at": 1, "games": games, "images": images }).to_string()).unwrap();
    }

    fn want(id: &str, title: &str, store: bool) -> Want {
        Want { id: id.into(), title: title.into(), store, plat: LB_PC }
    }

    #[test]
    fn only_windows_games_with_a_back() {
        let t = tempfile::tempdir().unwrap();
        cache_with(t.path(), &["Nintendo GameCube", LB_PC]);
        let mut idx = index(t.path()).unwrap();
        let shard = idx.platform(LB_PC).unwrap();
        let urls = |title: &str, store: bool, region: &str| find(shard, title, store, region).map(|(name, urls)| (name, urls.iter().map(|u| u.trim_start_matches(covers::LB_IMG).to_string()).collect::<Vec<_>>()));
        assert_eq!(urls("Zeta Run", false, "auto"), Some(("Zeta Run".into(), vec!["1%20us.jpg".into(), "1-eu.jpg".into()])));
        assert_eq!(urls("Zeta Run", false, "eur").unwrap().1, ["1-eu.jpg", "1%20us.jpg"]);
        // close calls for a store's game only
        assert_eq!(urls("Omega Quest Deluxe Edition", true, "auto").map(|u| u.1), Some(vec!["2-r.png".to_string()]));
        assert_eq!(urls("Omega Quest Deluxe Edition", false, "auto"), None);
        assert_eq!(urls("Bare", true, "auto"), None, "no back");
        let t = tempfile::tempdir().unwrap();
        cache_with(t.path(), &["Nintendo 64"]);
        let mut idx = index(t.path()).unwrap();
        assert!(!idx.has(LB_PC));
        // a recomp: its console's release
        let port = Game { id: "custom-oot".into(), title: "The Legend of Zelda Ocarina of Time".into(), source: Source::Pc(Box::new(crate::pcgames::PcGame { id: "custom-oot".into(), source: "custom", port: true, port_of: "n64".into(), ..Default::default() })), ..Game::default() };
        let w = Want::of(&port).unwrap();
        assert_eq!(w.plat, "Nintendo 64");
        let got = find(idx.platform(w.plat).unwrap(), &w.title, w.store, "auto").map(|(n, u)| (n, u.len()));
        assert_eq!(got, Some(("The Legend of Zelda: Ocarina of Time".into(), 1)));
        assert!(matches!(fetch(t.path(), t.path(), &want("steam-1", "Zeta Run", true), "auto"), Got::NotReady), "no Windows games in this index");
    }

    #[test]
    fn a_run_skips_backs_already_there_and_waits_after_a_miss() {
        let t = tempfile::tempdir().unwrap();
        let (data, cache) = (t.path().join("data"), t.path().join("cache"));
        std::fs::create_dir_all(&cache).unwrap();
        cache_with(&cache, &[LB_PC]);
        let mut idx = index(&cache).unwrap();
        let asked = std::sync::Mutex::new(Vec::new());
        let get = |w: &Want, u: &str| {
            asked.lock().unwrap().push(w.id.clone());
            (!u.contains("eu")).then(|| PathBuf::from(u))
        };
        let wants = [want("steam-1", "Zeta Run", true), want("custom-a", "Nothing Like It", false), want("custom-b", "Zeta Run", false), want("custom-c", "Zeta Run", false)];
        std::fs::create_dir_all(data.join("pc-covers")).unwrap();
        std::fs::write(data.join("pc-covers/custom-b.back.png"), b"x").unwrap();
        pccovers::reset_back(&data, "custom-c").unwrap();
        assert_eq!(run_with(&data, &mut idx, &wants, "auto", 1_000, &get), 1);
        assert_eq!(*asked.lock().unwrap(), ["steam-1"]);
        let memo: Value = serde_json::from_slice(&std::fs::read(data.join("pc-covers/back-miss.json")).unwrap()).unwrap();
        assert_eq!(memo, json!({ "custom-a|Nothing Like It": 1_000 }));
        asked.lock().unwrap().clear();
        assert_eq!(run_with(&data, &mut idx, &wants[1..2], "auto", 1_000 + 29 * 86_400_000, &get), 0);
        assert_eq!(run_with(&data, &mut idx, &wants[1..2], "auto", 1_000 + 31 * 86_400_000, &get), 0);
        assert_eq!(*asked.lock().unwrap(), Vec::<String>::new(), "a miss in the index downloads nothing");
        let many: Vec<Want> = (0..RUN_MAX + 5).map(|i| want(&format!("custom-{i}"), "Unknown", false)).collect();
        run_with(&data, &mut idx, &many, "auto", 1_000, &get);
        let memo: Value = serde_json::from_slice(&std::fs::read(data.join("pc-covers/back-miss.json")).unwrap()).unwrap();
        assert_eq!(memo.as_object().unwrap().len(), 1 + RUN_MAX);
    }

    #[test]
    fn a_miss_is_kept_with_no_pc_covers_folder_yet() {
        let t = tempfile::tempdir().unwrap();
        let (data, cache) = (t.path().join("data"), t.path().join("cache"));
        std::fs::create_dir_all(&cache).unwrap();
        cache_with(&cache, &[LB_PC]);
        let mut idx = index(&cache).unwrap();
        assert_eq!(run_with(&data, &mut idx, &[want("custom-a", "Nothing Like It", false)], "auto", 1_000, &|_, _| None), 0);
        let memo: Value = serde_json::from_slice(&std::fs::read(data.join("pc-covers/back-miss.json")).unwrap()).unwrap();
        assert_eq!(memo, json!({ "custom-a|Nothing Like It": 1_000 }));
    }

    #[test]
    fn which_games_are_a_store_s() {
        let pc = |source: &'static str, port: bool| Game { id: "x".into(), title: "X".into(), source: Source::Pc(Box::new(crate::pcgames::PcGame { source, port, ..Default::default() })), ..Game::default() };
        assert_eq!(Want::of(&pc("epic", false)).map(|w| w.store), Some(true));
        assert_eq!(Want::of(&pc("folder", true)).map(|w| w.store), Some(false), "a port from the ports folder");
        assert_eq!(Want::of(&pc("custom", false)).map(|w| w.store), Some(false));
        assert_eq!(Want::of(&Game { id: "x".into(), title: "X".into(), ..Game::default() }), None);
    }
}

// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the pc-categories
// branch's main/pccats.js.
//! Settings → Collections → Import: Steam's collections and Playnite's categories as shelves of your own. Both are
//! only read, never changed. A Steam collection is `user-collections.<id>` in
//! `<Steam>/userdata/<account>/config/cloudstorage/cloud-storage-namespace-1.json` (its games by app); a Playnite
//! category is in its library's `categories.db`, its games in `games.db` (`CategoryIds`). Steam's dynamic
//! collections (filters, no list of games) and its Hidden one are left out; its Favorites become favourites.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::js;

/// A PC game of the shelf, for matching: its id, its Steam app when it's a Steam game, its title.
pub struct Have<'a> {
    pub id: &'a str,
    pub appid: Option<&'a str>,
    pub title: &'a str,
}

/// One shelf to make: its id (`st-…` or `pn-…`, so importing again updates it), its name and its games' ids.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub ids: Vec<String>,
}

fn norm(t: &str) -> String {
    t.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect()
}

fn slug(s: &str) -> String {
    let s: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    let s = s.trim_matches('-').chars().take(32).collect::<String>();
    if s.is_empty() { "cat".into() } else { s }
}

fn name_of(s: &str) -> String {
    let n = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect::<String>();
    if n.is_empty() { "Collection".into() } else { n }
}

/// Steam's collection files, the newest first (each signed-in account has its own).
pub fn steam_files(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name() != "0")
        .map(|e| e.path().join("config").join("cloudstorage").join("cloud-storage-namespace-1.json"))
        .filter_map(|f| std::fs::metadata(&f).and_then(|m| m.modified()).ok().map(|t| (t, f)))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, f)| f).collect()
}

/// Steam's collections in one of its files (`steam_files`), matched to the shelf's games by app: the shelves to make,
/// and the games of its Favorites.
pub fn steam(file_text: &str, games: &[Have]) -> (Vec<Group>, Vec<String>) {
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(file_text) else { return (Vec::new(), Vec::new()) };
    let mut by_app: HashMap<&str, Vec<&str>> = HashMap::new();
    for g in games {
        if let Some(a) = g.appid {
            by_app.entry(a).or_default().push(g.id);
        }
    }
    let (mut groups, mut favorites) = (Vec::new(), Vec::new());
    for item in &items {
        let (Some(key), Some(entry)) = (item[0].as_str(), item.get(1)) else { continue };
        if !key.starts_with("user-collections.") || js::truthy(entry.get("is_deleted")) {
            continue;
        }
        let val = match &entry["value"] {
            Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or_default(),
            v => v.clone(),
        };
        let (Some(id), Some(name)) = (val["id"].as_str(), val["name"].as_str()) else { continue };
        if id == "hidden" {
            continue;
        }
        // a dynamic collection is a filter, not a list of games
        let filters = val["filterSpec"]["filterGroups"].as_array().is_some_and(|g| g.iter().any(|fg| fg["rgOptions"].as_array().is_some_and(|o| !o.is_empty())));
        if filters && id != "favorite" {
            continue;
        }
        let removed: Vec<String> = val["removed"].as_array().into_iter().flatten().map(|x| js::string(Some(x))).collect();
        let mut ids: Vec<String> = Vec::new();
        for a in val["added"].as_array().into_iter().flatten().map(|x| js::string(Some(x))).filter(|a| !removed.contains(a)) {
            for gid in by_app.get(a.as_str()).into_iter().flatten() {
                if !ids.iter().any(|x| x == gid) {
                    ids.push(gid.to_string());
                }
            }
        }
        if id == "favorite" {
            favorites.extend(ids);
        } else {
            groups.push(Group { id: format!("st-{}", slug(id)), name: name_of(name), ids });
        }
    }
    (groups, favorites)
}

/// Playnite's categories in a library `addgames::read` gave, matched to the shelf's games: a game not installed from
/// Playnite by its id, a Steam game by its app, the rest by title.
pub fn playnite(lib: &Value, games: &[Have]) -> Vec<Group> {
    let text = |v: Option<&Value>| if js::truthy(v) { js::string(v) } else { String::new() };
    let (mut by_app, mut by_title): (HashMap<&str, Vec<&str>>, HashMap<String, Vec<&str>>) = (HashMap::new(), HashMap::new());
    for g in games {
        if let Some(a) = g.appid {
            by_app.entry(a).or_default().push(g.id);
        }
        by_title.entry(norm(g.title)).or_default().push(g.id);
    }
    let own: HashMap<String, &str> = games.iter().filter_map(|g| g.id.strip_prefix("own-").map(|k| (k.to_lowercase(), g.id))).collect();
    let mut cats: Vec<(String, String, Vec<String>)> = lib["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = text([c.get("Id"), c.get("_id")].into_iter().find(|x| js::truthy(*x)).flatten()).to_lowercase();
            let name = text(c.get("Name"));
            (!id.is_empty() && !name.trim().is_empty()).then(|| (id, name_of(&name), Vec::new()))
        })
        .collect();
    for g in lib["games"].as_array().into_iter().flatten() {
        if js::truthy(g.get("Hidden")) {
            continue;
        }
        let Some(cids) = g["CategoryIds"].as_array().filter(|c| !c.is_empty()) else { continue };
        let pid = text([g.get("Id"), g.get("_id")].into_iter().find(|x| js::truthy(*x)).flatten()).to_lowercase();
        let game_id = text(g.get("GameId"));
        let mut matched: Vec<&str> = own.get(&pid).copied().into_iter().collect();
        if game_id.chars().all(|c| c.is_ascii_digit()) && !game_id.is_empty() {
            matched.extend(by_app.get(game_id.as_str()).into_iter().flatten().copied());
        }
        if matched.is_empty() {
            matched.extend(by_title.get(&norm(&text(g.get("Name")))).into_iter().flatten().copied());
        }
        for cid in cids.iter().map(|c| js::string(Some(c)).to_lowercase()) {
            if let Some(cat) = cats.iter_mut().find(|c| c.0 == cid) {
                for m in &matched {
                    if !cat.2.iter().any(|x| x == m) {
                        cat.2.push(m.to_string());
                    }
                }
            }
        }
    }
    cats.into_iter().filter(|c| !c.2.is_empty()).map(|(id, name, ids)| Group { id: format!("pn-{}", slug(&id)), name, ids }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn games() -> Vec<Have<'static>> {
        vec![
            Have { id: "steam-620", appid: Some("620"), title: "Portal 2" },
            Have { id: "steam-400", appid: Some("400"), title: "Portal" },
            Have { id: "gog-zeta", appid: None, title: "Zeta Run" },
            Have { id: "own-11111111-1111-1111-1111-111111111111", appid: None, title: "Alpha Quest" },
        ]
    }

    #[test]
    fn steam_s_own_collections_become_shelves_and_its_favorites_favourites() {
        let col = |id: &str, name: &str, added: Value, extra: Value| {
            let mut v = json!({ "id": id, "name": name, "added": added, "removed": [] });
            if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(e.clone());
            }
            json!([format!("user-collections.{id}"), { "key": format!("user-collections.{id}"), "value": v.to_string() }])
        };
        let file = json!([
            col("uc-puzzle", "Puzzle  games", json!([620, 400, 999]), json!({})),
            col("favorite", "Favorites", json!([400]), json!({})),
            col("hidden", "Hidden", json!([620]), json!({})),
            col("uc-dyn", "Installed", json!([]), json!({ "filterSpec": { "filterGroups": [{ "rgOptions": [] }, { "rgOptions": [1] }] } })),
            ["user-collections.gone", { "key": "user-collections.gone", "is_deleted": true }],
            ["something-else", { "value": "{}" }],
        ])
        .to_string();
        let (groups, favs) = steam(&file, &games());
        assert_eq!(groups, [Group { id: "st-uc-puzzle".into(), name: "Puzzle games".into(), ids: vec!["steam-620".into(), "steam-400".into()] }]);
        assert_eq!(favs, ["steam-400"]);
    }

    #[test]
    fn playnite_s_categories_match_by_its_own_id_steam_app_and_title() {
        let lib = json!({
            "categories": [{ "Id": "AAAA", "Name": "Couch" }, { "Id": "bbbb", "Name": "Empty" }],
            "games": [
                { "Id": "11111111-1111-1111-1111-111111111111", "Name": "Alpha Quest", "CategoryIds": ["aaaa"] },
                { "Id": "x", "Name": "Portal 2", "GameId": "620", "CategoryIds": ["AAAA"] },
                { "Id": "y", "Name": "Zeta: Run", "CategoryIds": ["aaaa"] },
                { "Id": "z", "Name": "Portal", "CategoryIds": ["aaaa"], "Hidden": true },
            ]
        });
        let got = playnite(&lib, &games());
        assert_eq!(got, [Group { id: "pn-aaaa".into(), name: "Couch".into(), ids: vec!["own-11111111-1111-1111-1111-111111111111".into(), "steam-620".into(), "gog-zeta".into()] }]);
    }
}

// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the
// to-install-shelf branch's owned() in main/addgames.js and ownedRefresh() in main.js.
//! Games you own in a store but haven't installed (Settings → PC → Games you haven't installed), from Playnite:
//! its store libraries (Steam, Epic, GOG, EA, Ubisoft, Xbox, Battle.net …) list every game the account owns. They
//! join the PC shelf greyed, and Play installs one: Steam's own install dialog for a Steam game
//! (`steam://install/<appid>`), Playnite for the rest (`playnite://playnite/start/<game>` runs that store's
//! installer). Windows only.
//!
//! Reading Playnite's library takes PowerShell a few seconds, so the list is kept in `data/playnite-owned.json`
//! and read again only when Playnite's `games.db` has changed since.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::addgames::Playnite;
use crate::js;
use crate::pcgames::PcGame;

/// Playnite's store plugins by id; a store not listed is "other" and installs through Playnite all the same.
const STORES: [(&str, &str); 9] = [
    ("cb91dfc9-b977-43bf-8e70-55f46e410fab", "steam"),
    ("00000002-dbd1-46c6-b5d0-b1ba559d10e4", "epic"),
    ("aebe8b7c-6dc3-4a66-af31-e7375c6b5e9e", "gog"),
    ("85dd7072-2f20-4e76-a007-41035e390724", "ea"),
    ("c2f038e5-8b92-4877-91f1-da9094155fc5", "ubisoft"),
    ("7e4fbb5e-2ae3-48d4-8ba0-6b30e7a4e287", "xbox"),
    ("e3c26a3d-d695-4cb7-a769-5ff7612c7edd", "battlenet"),
    ("402674cd-4af6-4886-b6ec-0e695bfa0688", "amazon"),
    ("88409022-088a-4de8-805a-fdbac291f00a", "rockstar"),
];

/// The settings' switches each store's installed games go by: a store switched off there has none here either.
const STORE_SWITCH: [(&str, &str); 5] = [("epic", "epic"), ("gog", "gog"), ("ubisoft", "ubisoft"), ("ea", "ea"), ("xbox", "xbox")];

const FILE: &str = "playnite-owned.json";

/// One game owned and not installed.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Owned {
    /// Playnite's id for it
    pub key: String,
    pub title: String,
    /// one of `STORES`' names, or "other"
    pub store: String,
    /// the link that installs it
    pub uri: String,
}

/// Its store's name as the shelf writes it.
pub fn store_name(store: &str) -> &'static str {
    match store {
        "steam" => "Steam",
        "epic" => "Epic Games",
        "gog" => "GOG",
        "ea" => "EA",
        "ubisoft" => "Ubisoft",
        "xbox" => "Xbox",
        "battlenet" => "Battle.net",
        "amazon" => "Amazon",
        "rockstar" => "Rockstar",
        _ => "Playnite",
    }
}

/// Only the two kinds of link these games are given are ever opened.
pub fn install_link(uri: &str) -> bool {
    static OK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(steam://install/\d{1,10}|playnite://playnite/start/[0-9a-f-]{36})$").unwrap());
    OK.is_match(uri)
}

/// The Steam app a Steam game's link installs.
pub fn steam_appid(uri: &str) -> Option<&str> {
    uri.strip_prefix("steam://install/")
}

/// The owned games in a library `addgames::read` gave, each with its cover in Playnite's files when it has one.
/// Hidden games, installed ones and games added to Playnite by hand (nothing to install) are left out.
pub fn owned(lib: &Value, found: &Playnite) -> Vec<(Owned, Option<PathBuf>)> {
    const EMPTY: &str = "00000000-0000-0000-0000-000000000000";
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[0-9a-f-]{36}$").unwrap());
    static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{1,10}$").unwrap());
    let text = |v: Option<&Value>| if js::truthy(v) { js::string(v) } else { String::new() };
    let mut out = Vec::new();
    for g in lib.get("games").and_then(Value::as_array).into_iter().flatten() {
        if !js::truthy(g.get("Name")) || js::truthy(g.get("Hidden")) || js::truthy(g.get("IsInstalled")) {
            continue;
        }
        let plugin = text(g.get("PluginId")).to_lowercase();
        if plugin.is_empty() || plugin == EMPTY {
            continue;
        }
        let key = text([g.get("Id"), g.get("_id")].into_iter().find(|x| js::truthy(*x)).flatten());
        if !ID.is_match(&key) {
            continue;
        }
        let store = STORES.iter().find(|(id, _)| *id == plugin).map_or("other", |(_, s)| s);
        let game_id = text(g.get("GameId"));
        let uri = if store == "steam" && APP.is_match(&game_id) { format!("steam://install/{game_id}") } else { format!("playnite://playnite/start/{key}") };
        let cover = picture(found, &text(g.get("CoverImage")));
        out.push((Owned { key, title: js::slice(&text(g.get("Name")), 120).to_string(), store: store.into(), uri }, cover));
    }
    out.sort_by(|a, b| crate::collate::compare_exact(&a.0.title, &b.0.title));
    out
}

/// A picture of the library's own (not one on the web), when it's there.
fn picture(found: &Playnite, v: &str) -> Option<PathBuf> {
    let low = v.to_lowercase();
    if v.is_empty() || low.starts_with("http:") || low.starts_with("https:") {
        return None;
    }
    let file = if crate::addgames::win_absolute(v) { PathBuf::from(v) } else { v.split(['\\', '/']).fold(found.library.join("files"), |p, part| p.join(part)) };
    file.is_file().then_some(file)
}

/// The list kept from the last read, and the `games.db` time it was read at.
pub fn cached(data: &Path) -> (Vec<Owned>, f64) {
    let Some(v) = crate::files::read_json(&data.join(FILE)) else { return (Vec::new(), 0.0) };
    let s = |g: &Value, k: &str| g[k].as_str().unwrap_or("").to_string();
    let games = v["games"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| Owned { key: s(g, "key"), title: s(g, "title"), store: s(g, "store"), uri: s(g, "uri") })
        .filter(|g| !g.key.is_empty() && !g.title.is_empty() && install_link(&g.uri))
        .collect();
    (games, js::number(v.get("dbAt")))
}

/// Reads Playnite's library again when its `games.db` changed since the last read: the list is kept, each game's
/// cover copied once into the automatic covers. True when the list changed (the shelf is read again then).
pub fn refresh(data: &Path, found: &Playnite) -> Result<bool, String> {
    let at = std::fs::metadata(found.library.join("games.db")).and_then(|m| m.modified()).map_err(|e| e.to_string())?;
    let at = at.duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    let (before, read_at) = cached(data);
    if read_at == at {
        return Ok(false);
    }
    let lib = crate::addgames::read(found)?;
    let games = owned(&lib, found);
    for (g, cover) in &games {
        let id = id_of(&g.key);
        if let Some(cover) = cover.as_ref().filter(|_| crate::pccovers::auto_cover(data, &id).is_none()) {
            keep_cover(data, &id, cover);
        }
    }
    let list: Vec<Owned> = games.into_iter().map(|(g, _)| g).collect();
    let file = json!({ "v": 1, "dbAt": at, "games": list.iter().map(|g| json!({ "key": g.key, "title": g.title, "store": g.store, "uri": g.uri })).collect::<Vec<_>>() });
    crate::files::write_atomic(&data.join(FILE), file.to_string()).map_err(|e| e.to_string())?;
    Ok(list != before)
}

fn keep_cover(data: &Path, id: &str, from: &Path) {
    let ext = from.extension().map_or(String::new(), |e| e.to_string_lossy().to_lowercase().replace("jpeg", "jpg"));
    if !["jpg", "png", "webp"].contains(&ext.as_str()) {
        return;
    }
    let dir = data.join("pc-covers-auto");
    let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(from, dir.join(format!("{}.{ext}", crate::pccovers::safe_id(id)))));
}

/// The shelf's id for an owned game.
pub fn id_of(key: &str) -> String {
    format!("own-{key}")
}

/// The owned games for the PC shelf, from the kept list: only stores switched on in Settings → PC, and none that's
/// already on a shelf (the same Steam app, or the same title).
pub fn games(data: &Path, pc: Option<&Value>, steam_on: bool, have_titles: &[String], have_apps: &[String]) -> Vec<PcGame> {
    let norm = |t: &str| t.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect::<String>();
    let titles: std::collections::HashSet<String> = have_titles.iter().map(|t| norm(t)).collect();
    let store_on = |s: &str| match s {
        "steam" => steam_on,
        _ => STORE_SWITCH.iter().find(|(st, _)| *st == s).is_none_or(|(_, k)| crate::pcgames::wanted(pc, k)),
    };
    cached(data)
        .0
        .into_iter()
        .filter(|g| store_on(&g.store) && !titles.contains(&norm(&g.title)) && !steam_appid(&g.uri).is_some_and(|a| have_apps.iter().any(|h| h == a)))
        .map(|g| PcGame { id: id_of(&g.key), source: "playnite", title: g.title, uri: g.uri, store: g.store, installed: Some(false), ..PcGame::default() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(dir: &Path) -> Playnite {
        Playnite { app: dir.join("app"), library: dir.join("library") }
    }

    #[test]
    fn owned_games_get_the_link_that_installs_them() {
        let dir = std::env::temp_dir().join(format!("rs-owned-{}", std::process::id()));
        let lib = json!({ "games": [
            { "Id": "11111111-1111-1111-1111-111111111111", "Name": "Zeta Run", "PluginId": "cb91dfc9-b977-43bf-8e70-55f46e410fab", "GameId": "620" },
            { "Id": "22222222-2222-2222-2222-222222222222", "Name": "Alpha Quest", "PluginId": "00000002-dbd1-46c6-b5d0-b1ba559d10e4", "GameId": "abc" },
            { "Id": "33333333-3333-3333-3333-333333333333", "Name": "Installed", "PluginId": "cb91dfc9-b977-43bf-8e70-55f46e410fab", "IsInstalled": true },
            { "Id": "44444444-4444-4444-4444-444444444444", "Name": "By hand", "PluginId": "00000000-0000-0000-0000-000000000000" },
            { "Id": "55555555-5555-5555-5555-555555555555", "Name": "Hidden", "PluginId": "aebe8b7c-6dc3-4a66-af31-e7375c6b5e9e", "Hidden": true },
            { "Id": "66666666-6666-6666-6666-666666666666", "Name": "Odd Store", "PluginId": "99999999-0000-0000-0000-000000000000" },
        ] });
        let got: Vec<(String, String, String)> = owned(&lib, &found(&dir)).into_iter().map(|(g, _)| (g.title, g.store, g.uri)).collect();
        assert_eq!(
            got,
            [
                ("Alpha Quest".to_string(), "epic".to_string(), "playnite://playnite/start/22222222-2222-2222-2222-222222222222".to_string()),
                ("Odd Store".into(), "other".into(), "playnite://playnite/start/66666666-6666-6666-6666-666666666666".into()),
                ("Zeta Run".into(), "steam".into(), "steam://install/620".into()),
            ]
        );
        assert!(got.iter().all(|g| install_link(&g.2)));
        assert!(!install_link("steam://run/620") && !install_link("playnite://playnite/start/x; calc") && !install_link("https://example.invalid"));
    }

    #[test]
    fn a_game_already_on_a_shelf_or_of_a_store_switched_off_is_left_out() {
        let data = std::env::temp_dir().join(format!("rs-owned-list-{}", std::process::id()));
        std::fs::create_dir_all(&data).unwrap();
        let list = json!({ "v": 1, "dbAt": 5.0, "games": [
            { "key": "1", "title": "Zeta Run", "store": "steam", "uri": "steam://install/620" },
            { "key": "2", "title": "Alpha Quest", "store": "epic", "uri": "playnite://playnite/start/22222222-2222-2222-2222-222222222222" },
            { "key": "3", "title": "Beta: The Game", "store": "gog", "uri": "playnite://playnite/start/33333333-3333-3333-3333-333333333333" },
            { "key": "4", "title": "Gamma", "store": "steam", "uri": "steam://install/700" },
        ] });
        std::fs::write(data.join(FILE), list.to_string()).unwrap();
        let pc = json!({ "epic": false });
        let got: Vec<String> = games(&data, Some(&pc), true, &["beta the game".into()], &["700".into()]).into_iter().map(|g| g.title).collect();
        assert_eq!(got, ["Zeta Run"]);
        let g = &games(&data, None, true, &[], &[])[0];
        assert_eq!((g.id.as_str(), g.source, g.installed, g.store.as_str()), ("own-1", "playnite", Some(false), "steam"));
        assert!(games(&data, None, false, &[], &[]).iter().all(|g| g.store != "steam"));
        let _ = std::fs::remove_dir_all(&data);
    }
}

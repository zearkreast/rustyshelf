// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// shelf's sorting and filtering in Boxshelf's renderer/app.js, as RustyShelf has it.
//! How a shelf's games are shown: which are left out (hidden, or not what the filter asks for) and in which order.
//! The choices are the settings' `ui.sort`, `ui.filter`, `ui.favorites`, `ui.hidden` and `ui.customSort`, shared with
//! the Electron app.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::collate::compare_exact;
use crate::history::{History, Played};
use crate::library::{Shelf, ShelfKind};
use crate::scanner::{Game, Source};

/// The orders a shelf can be in, with the name each has in the menu (renderer/app.js SORTS).
pub const SORTS: [(&str, &str); 7] =
    [("az", "A–Z"), ("year", "Release year"), ("recent", "Last played"), ("most", "Most played"), ("rating", "Rating"), ("genre", "Genre"), ("custom", "Custom")];

/// The filters every library has (renderer/app.js FILTERS); `filters` adds the ones made from the library itself.
const FILTERS: [(&str, &str); 4] = [("all", "All games"), ("fav", "Favorites"), ("played", "Played"), ("unplayed", "Not played yet")];

/// The player's choices for the shelves.
#[derive(Debug, Clone, Default)]
pub struct View {
    pub sort: String,
    pub filter: String,
    pub favorites: HashSet<String>,
    pub hidden: HashSet<String>,
    /// a shelf's id → its games' ids in the order the player put them
    pub custom: HashMap<String, Vec<String>>,
}

impl View {
    /// From the settings' `ui` section.
    pub fn from_ui(ui: Option<&Value>) -> View {
        let get = |k: &str| ui.and_then(|u| u.get(k));
        let ids = |v: Option<&Value>| -> Vec<String> { v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default() };
        View {
            sort: get("sort").and_then(Value::as_str).unwrap_or("az").to_string(),
            filter: get("filter").and_then(Value::as_str).unwrap_or("all").to_string(),
            favorites: ids(get("favorites")).into_iter().collect(),
            hidden: ids(get("hidden")).into_iter().collect(),
            custom: get("customSort").and_then(Value::as_object).map(|o| o.iter().map(|(k, v)| (k.clone(), ids(Some(v)))).collect()).unwrap_or_default(),
        }
    }
}

/// A game's plays: RustyShelf's own count, else what Steam keeps for its games.
pub fn played(g: &Game, history: &History) -> Option<Played> {
    history.get(&g.id).or(match g.source {
        Source::Steam { minutes, last_played, .. } if minutes > 0 || last_played > 0 => Some(Played { count: u64::from(minutes > 0), ms: minutes * 60_000, last: last_played * 1000 }),
        _ => None,
    })
}

/// The filters this library can use: the usual ones, then "Installed" when some games aren't, 2+ players, co-op,
/// each decade it has, and its twelve most common genres. Each is its id and its name.
pub fn filters(games: &[&Game], current: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = FILTERS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect();
    if games.iter().any(|g| g.not_installed) || current == "installed" {
        out.push(("installed".into(), "Installed".into()));
    }
    let infos: Vec<&crate::info::Info> = games.iter().filter_map(|g| g.info.as_ref()).collect();
    if infos.iter().any(|i| i.max_players().unwrap_or(0) >= 2) {
        out.push(("multi".into(), "2+ players".into()));
    }
    if infos.iter().any(|i| i.is_coop()) {
        out.push(("coop".into(), "Co-op".into()));
    }
    let mut decades: Vec<u32> = infos.iter().filter_map(|i| i.year()).map(|y| y / 10 * 10).collect();
    decades.sort_unstable();
    decades.dedup();
    out.extend(decades.into_iter().map(|d| (format!("dec:{d}"), format!("{d}s"))));
    let mut count: Vec<(&str, usize)> = Vec::new();
    for g in infos.iter().flat_map(|i| i.genres()) {
        match count.iter_mut().find(|c| c.0 == g) {
            Some(c) => c.1 += 1,
            None => count.push((g, 1)),
        }
    }
    count.sort_by_key(|c| std::cmp::Reverse(c.1));
    let mut top: Vec<&str> = count.into_iter().take(12).map(|c| c.0).collect();
    top.sort_by(|a, b| compare_exact(a, b));
    out.extend(top.into_iter().map(|g| (format!("genre:{g}"), g.to_string())));
    out
}

/// Whether a filter keeps a game.
fn keeps(filter: &str, g: &Game, view: &View, history: &History) -> bool {
    let info = g.info.as_ref();
    let was_played = || played(g, history).is_some_and(|p| p.count > 0);
    match filter {
        "installed" => !g.not_installed,
        "fav" => view.favorites.contains(&g.id),
        "played" => was_played(),
        "unplayed" => !was_played(),
        "multi" => info.and_then(|i| i.max_players()).unwrap_or(0) >= 2,
        "coop" => info.is_some_and(|i| i.is_coop()),
        f => match (f.strip_prefix("dec:").and_then(|d| d.parse::<u32>().ok()), f.strip_prefix("genre:")) {
            (Some(d), _) => info.and_then(|i| i.year()).is_some_and(|y| y >= d && y < d + 10),
            (_, Some(genre)) => info.is_some_and(|i| i.genres().contains(&genre)),
            _ => true,
        },
    }
}

/// The games of a shelf to show, as their places in `games`: the hidden ones and the ones the filter leaves out
/// gone, the rest in the order chosen. A game the order knows nothing of (no year, never played) goes last, and
/// games that tie stay as the shelf has them.
pub fn arrange(shelf_id: &str, games: &[Game], view: &View, history: &History) -> Vec<usize> {
    let mut shown: Vec<usize> = (0..games.len()).filter(|i| !view.hidden.contains(&games[*i].id) && keeps(&view.filter, &games[*i], view, history)).collect();
    // an order by a number each game may not have
    let mut by = |key: &dyn Fn(&Game) -> Option<f64>, descending: bool| {
        shown.sort_by(|a, b| match (key(&games[*a]), key(&games[*b])) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => if descending { y.partial_cmp(&x) } else { x.partial_cmp(&y) }.unwrap_or(Ordering::Equal),
        })
    };
    let info = |g: &Game| g.info.clone().unwrap_or_default();
    let plays = |g: &Game| played(g, history);
    match view.sort.as_str() {
        "year" => by(&|g| info(g).year().map(f64::from), false),
        "recent" => by(&|g| plays(g).map(|p| p.last as f64).filter(|l| *l != 0.0), true),
        "most" => by(&|g| plays(g).map(|p| p.ms as f64).filter(|m| *m != 0.0), true),
        "rating" => by(&|g| info(g).score(), true),
        "genre" => shown.sort_by(|a, b| match (games[*a].info.as_ref().and_then(|i| i.genre()), games[*b].info.as_ref().and_then(|i| i.genre())) {
            (x, y) if x == y => Ordering::Equal,
            (None, _) => Ordering::Greater,
            (_, None) => Ordering::Less,
            (Some(x), Some(y)) => compare_exact(x, y),
        }),
        "custom" => {
            let place: HashMap<&str, usize> = view.custom.get(shelf_id).map(|l| l.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect()).unwrap_or_default();
            shown.sort_by_key(|i| place.get(games[*i].id.as_str()).copied().unwrap_or(999_999));
        }
        _ => {}
    }
    shown
}

/// The genres the games have, for Add a tag shelf: each with how many games, the most first (then by name), at most 60.
pub fn genre_counts(shelves: &[Shelf]) -> Vec<(String, usize)> {
    let (mut seen, mut counts) = (HashSet::new(), HashMap::<String, usize>::new());
    for g in shelves.iter().filter(|s| !matches!(s.kind, ShelfKind::Mixed)).flat_map(|s| &s.games) {
        if !matches!(g.source, Source::Tile(_)) && seen.insert(g.id.as_str()) {
            for x in g.info.as_ref().map(|i| i.genres()).unwrap_or_default() {
                let k = x.split_whitespace().collect::<Vec<_>>().join(" ");
                if !k.is_empty() {
                    *counts.entry(k).or_default() += 1;
                }
            }
        }
    }
    let mut out: Vec<(String, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| compare_exact(&a.0, &b.0)));
    out.truncate(60);
    out
}

fn tag_key(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

static TAG_SPLIT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r" *[/,&] *").unwrap());

/// The shelves that mix the others' games, which come before them: Home (the twelve games played last, the latest
/// first, then the favourites by title), All (every game by title, when the settings ask for it) and your
/// collections (each one's games in the order they were added). They keep their own order whatever the shelves'
/// order and filter are; hidden games aren't on them. Home is there once it has a game, a collection too.
// From sassiestrhihno's 1.82.0-beta.14 source (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the
// automatic shelves and the shelves that fill themselves are ported from renderer/app.js (autoBuilt, autoRuleGames).
/// The automatic shelves, as `ui.autoShelves` switches them on: each its key, name and words under it.
pub const AUTO_SHELVES: [(&str, &str, &str); 5] = [("recent", "Recently played", "Your last games"), ("most", "Most played", "By play time"), ("playing", "Playing", "Your status"), ("finished", "Finished", "Your status"), ("never", "Never played", "Not started yet")];

/// The shelves made of games from every console: Home, All, the automatic ones switched on and your collections.
/// `status` is each game's Status from Details (`games.<id>.status`), by id.
pub fn mixed_shelves(shelves: &[Shelf], ui: Option<&Value>, history: &History) -> Vec<Shelf> {
    mixed_shelves_with(shelves, ui, history, &|_| String::new())
}

pub fn mixed_shelves_with(shelves: &[Shelf], ui: Option<&Value>, history: &History, status: &dyn Fn(&str) -> String) -> Vec<Shelf> {
    let view = View::from_ui(ui);
    let all: Vec<(&Game, &str)> = shelves.iter().filter(|s| !matches!(s.kind, ShelfKind::Mixed)).flat_map(|s| s.games.iter().map(move |g| (g, s.id.as_str()))).filter(|(g, _)| !view.hidden.contains(&g.id)).collect();
    let mixed = |id: String, name: String, games: Vec<(&Game, &str)>| Shelf { id, name, kind: ShelfKind::Mixed, from: games.iter().map(|(_, s)| s.to_string()).collect(), games: games.into_iter().map(|(g, _)| g.clone()).collect() };
    let mut out = Vec::new();

    let last = |g: &Game| history.get(&g.id).filter(|p| p.count > 0).map(|p| p.last);
    let mut home: Vec<(&Game, &str)> = all.iter().copied().filter(|(g, _)| last(g).is_some()).collect();
    home.sort_by_key(|(g, _)| std::cmp::Reverse(last(g)));
    home.truncate(12);
    let recent: HashSet<&str> = home.iter().map(|(g, _)| g.id.as_str()).collect();
    let mut favorites: Vec<(&Game, &str)> = all.iter().copied().filter(|(g, _)| view.favorites.contains(&g.id) && !recent.contains(g.id.as_str())).collect();
    favorites.sort_by(|a, b| compare_exact(&a.0.title, &b.0.title));
    home.extend(favorites);
    if !home.is_empty() {
        out.push(mixed("home".into(), "Home".into(), home));
    }

    if ui.and_then(|u| u.get("allShelf")) == Some(&Value::Bool(true)) {
        let mut seen = HashSet::new();
        let mut every: Vec<(&Game, &str)> = all.iter().copied().filter(|(g, _)| seen.insert(g.id.as_str())).collect();
        every.sort_by(|a, b| compare_exact(&a.0.title, &b.0.title));
        if !every.is_empty() {
            out.push(mixed("all".into(), "All".into(), every));
        }
    }

    // (a game that's on two shelves under one id is taken from the later one)
    let by_id: HashMap<&str, (&Game, &str)> = all.iter().map(|x| (x.0.id.as_str(), *x)).collect();
    let mut seen = HashSet::new();
    let mut by_title: Vec<(&Game, &str)> = all.iter().copied().filter(|(g, _)| seen.insert(g.id.as_str())).collect();
    by_title.sort_by(|a, b| compare_exact(&a.0.title, &b.0.title));
    // the automatic shelves switched on, after Home
    let autos = ui.and_then(|u| u.get("autoShelves"));
    let auto_on = |k: &str| autos.and_then(|a| a.get(k)) == Some(&Value::Bool(true));
    let mut unique: Vec<(&Game, &str)> = by_id.values().copied().collect();
    unique.sort_by(|a, b| crate::collate::compare(&a.0.title, &b.0.title).then_with(|| a.0.id.cmp(&b.0.id)));
    let plays = |g: &Game| played(g, history);
    let (last, ms, count) = (|g: &Game| plays(g).map_or(0, |p| p.last), |g: &Game| plays(g).map_or(0, |p| p.ms), |g: &Game| plays(g).map_or(0, |p| p.count));
    for (k, name, _) in AUTO_SHELVES {
        if !auto_on(k) {
            continue;
        }
        let mut games: Vec<(&Game, &str)> = match k {
            "recent" => unique.iter().copied().filter(|(g, _)| count(g) > 0 && last(g) > 0).collect(),
            "most" => unique.iter().copied().filter(|(g, _)| ms(g) > 0).collect(),
            "playing" | "finished" => unique.iter().copied().filter(|(g, _)| status(&g.id) == k).collect(),
            _ => unique.iter().copied().filter(|(g, _)| count(g) == 0).collect(),
        };
        match k {
            "recent" => {
                games.sort_by_key(|(g, _)| std::cmp::Reverse(last(g)));
                games.truncate(30);
            }
            "most" => {
                games.sort_by(|a, b| ms(b.0).cmp(&ms(a.0)).then(count(b.0).cmp(&count(a.0))));
                games.truncate(30);
            }
            "playing" => games.sort_by_key(|(g, _)| std::cmp::Reverse(last(g))),
            "never" => games.truncate(60),
            _ => {}
        }
        if !games.is_empty() {
            out.push(mixed(format!("auto-{k}"), name.into(), games));
        }
    }
    // By genre: a shelf for each genre with 4 games or more, the 8 biggest, each A to Z (up to 100)
    if auto_on("genre") {
        let mut by_genre: Vec<(String, Vec<(&Game, &str)>)> = Vec::new();
        for x in &unique {
            for g in x.0.info.as_ref().map(|i| i.genres()).unwrap_or_default() {
                match by_genre.iter_mut().find(|e| e.0 == g) {
                    Some(e) => e.1.push(*x),
                    None => by_genre.push((g.to_string(), vec![*x])),
                }
            }
        }
        by_genre.retain(|e| e.1.len() >= 4);
        by_genre.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| crate::collate::compare(&a.0, &b.0)));
        let mut used = HashSet::new();
        for (name, mut games) in by_genre.into_iter().take(8) {
            let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '-' }).collect::<String>().split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
            let mut id = format!("auto-genre-{}", if slug.is_empty() { "x" } else { &slug });
            while !used.insert(id.clone()) {
                id.push('-');
            }
            games.truncate(100);
            out.push(mixed(id, name, games));
        }
    }
    for c in ui.and_then(|u| u.get("collections")).and_then(Value::as_array).into_iter().flatten() {
        let (Some(id), Some(ids)) = (c.get("id").and_then(Value::as_str).filter(|i| !i.is_empty()), c.get("ids").and_then(Value::as_array)) else { continue };
        let name = c.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or("Collection");
        // a shelf that fills itself: every game of a Status, genre or console, or the ones never played (up to 300)
        if let Some(kind) = c.get("rule").and_then(|r| r.get("kind")).and_then(Value::as_str) {
            let v = c["rule"].get("value").and_then(Value::as_str).unwrap_or("");
            let games: Vec<(&Game, &str)> = unique
                .iter()
                .copied()
                .filter(|(g, from)| match kind {
                    "status" => status(&g.id) == v,
                    "never" => count(g) == 0,
                    "genre" => g.info.as_ref().is_some_and(|i| i.genres().contains(&v)),
                    "console" => *from == v,
                    _ => false,
                })
                .take(300)
                .collect();
            if !games.is_empty() {
                out.push(mixed(format!("col-{id}"), name.to_string(), games));
            }
            continue;
        }
        let mut games: Vec<(&Game, &str)> = ids.iter().filter_map(|i| by_id.get(i.as_str()?).copied()).collect();
        // a tag shelf: after the games added by hand, every game whose genres include the tag ("Racing" finds
        // "Racing / Driving" too), by title
        if let Some(tag) = c.get("tag").and_then(Value::as_str).map(tag_key).filter(|t| !t.is_empty()) {
            let have: HashSet<String> = games.iter().map(|(g, _)| g.id.clone()).collect();
            let fits = |g: &Game| g.info.as_ref().is_some_and(|i| i.genres().iter().any(|x| { let k = tag_key(x); k == tag || TAG_SPLIT.split(&k).any(|p| p == tag) }));
            games.extend(by_title.iter().copied().filter(|(g, _)| !have.contains(&g.id) && fits(g)));
        }
        if games.is_empty() {
            continue;
        }
        out.push(mixed(format!("col-{id}"), name.to_string(), games));
    }
    out
}

/// The heading a game sits under in the order chosen ("M", "1998", "This week", "★ 4+"); None for a custom order.
pub fn group_of(g: &Game, sort: &str, history: &History, now_ms: u64) -> Option<String> {
    let info = g.info.clone().unwrap_or_default();
    let p = played(g, history);
    Some(match sort {
        "az" => g.title.chars().next().map(|c| c.to_ascii_uppercase()).filter(char::is_ascii_uppercase).map_or("#".to_string(), String::from),
        "year" => info.year().map_or("Undated".to_string(), |y| y.to_string()),
        "genre" => info.genre().unwrap_or("Other").to_string(),
        "recent" => match p.map(|p| p.last).filter(|l| *l != 0).map(|l| now_ms.saturating_sub(l) as f64 / 86_400_000.0) {
            None => "Never",
            Some(d) if d < 7.0 => "This week",
            Some(d) if d < 31.0 => "This month",
            Some(d) if d < 365.0 => "This year",
            Some(_) => "Earlier",
        }
        .to_string(),
        "most" => match p.map(|p| p.ms as f64 / 3_600_000.0).filter(|h| *h != 0.0) {
            None => "Never",
            Some(h) if h >= 20.0 => "20 h +",
            Some(h) if h >= 5.0 => "5–20 h",
            Some(h) if h >= 1.0 => "1–5 h",
            Some(_) => "Under 1 h",
        }
        .to_string(),
        "rating" => match info.score() {
            None => "Unrated",
            Some(r) if r >= 4.5 => "★ 4.5+",
            Some(r) if r >= 4.0 => "★ 4+",
            Some(r) if r >= 3.0 => "★ 3+",
            Some(_) => "★ < 3",
        }
        .to_string(),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::Info;

    fn game(id: &str, title: &str, year: Option<&str>, genres: Option<&str>, rating: Option<f64>) -> Game {
        Game { id: id.into(), title: title.into(), info: Some(Info { year: year.map(String::from), genres: genres.map(String::from), rating, players: Some("1-4".into()), ..Info::default() }), ..Game::default() }
    }

    #[test]
    fn a_tag_shelf_gathers_a_genre_from_every_console() {
        let history = History::load(tempfile::tempdir().unwrap().path());
        let shelf = |id: &str, games: Vec<Game>| Shelf { id: id.into(), name: id.into(), kind: ShelfKind::Pc, games, from: Vec::new() };
        let g = |id: &str, title: &str, genres: &str| game(id, title, None, Some(genres), None);
        let shelves = vec![
            shelf("gc", vec![g("a", "Zeta Run", "Racing / Driving"), g("b", "Omega Quest", "Role-Playing"), g("h", "Tau Racers", "Racing")]),
            shelf("pc", vec![g("c", "Alpha Tale", "Sports;  racing "), g("d", "Phi", "Racingish"), g("e", "Delta", "Action, Racing")]),
        ];
        let ui = serde_json::json!({ "hidden": ["h"], "collections": [{ "id": "t", "name": "Racing", "ids": ["b", "a"], "tag": "Racing" }, { "id": "u", "name": "Nothing", "ids": [], "tag": "Puzzle" }] });
        let mixed = mixed_shelves(&shelves, Some(&ui), &history);
        let ids: Vec<(&str, Vec<&str>)> = mixed.iter().map(|s| (s.id.as_str(), s.games.iter().map(|g| g.id.as_str()).collect())).collect();
        // the ones added by hand first, then the rest by title; hidden games and lookalike genres aren't on it
        assert_eq!(ids, [("col-t", vec!["b", "a", "c", "e"])]);
    }

    #[test]
    fn home_all_and_collections() {
        let tmp = tempfile::tempdir().unwrap();
        // b played last, then d, then a; c started and taken back (no count)
        std::fs::write(tmp.path().join("history.json"), r#"{"a":{"count":2,"ms":5,"last":100},"b":{"count":1,"ms":0,"last":900},"d":{"count":1,"ms":9,"last":500},"c":{"count":0,"ms":0,"last":950}}"#).unwrap();
        let history = History::load(tmp.path());
        let shelf = |id: &str, games: Vec<Game>| Shelf { id: id.into(), name: id.to_uppercase(), kind: ShelfKind::Pc, games, from: Vec::new() };
        let plain = |id: &str, title: &str| game(id, title, None, None, None);
        let shelves = vec![shelf("gc", vec![plain("a", "Alpha"), plain("b", "beta"), plain("h", "Hidden")]), shelf("pc", vec![plain("c", "Gamma"), plain("d", "Delta"), plain("e", "Épsilon"), plain("f", "Zeta")])];
        let ui = serde_json::json!({ "favorites": ["f", "e", "d"], "hidden": ["h"], "allShelf": true,
            "collections": [{ "id": "c1", "name": "Couch", "ids": ["f", "gone", "a"] }, { "id": "c2", "name": "Empty", "ids": ["gone"] }, { "id": "", "name": "No id", "ids": ["a"] }, { "id": "c3", "ids": ["b"] }] });
        let mixed = mixed_shelves(&shelves, Some(&ui), &history);
        let shown: Vec<(&str, &str, Vec<&str>, Vec<&str>)> = mixed.iter().map(|s| (s.id.as_str(), s.name.as_str(), s.games.iter().map(|g| g.id.as_str()).collect(), s.from.iter().map(String::as_str).collect())).collect();
        assert_eq!(
            shown,
            [
                // played last first, then the favourites that aren't there yet, by title
                ("home", "Home", vec!["b", "d", "a", "e", "f"], vec!["gc", "pc", "gc", "pc", "pc"]),
                ("all", "All", vec!["a", "b", "d", "e", "c", "f"], vec!["gc", "gc", "pc", "pc", "pc", "pc"]),
                ("col-c1", "Couch", vec!["f", "a"], vec!["pc", "gc"]),
                ("col-c3", "Collection", vec!["b"], vec!["gc"]),
            ]
        );
        assert!(mixed.iter().all(|s| matches!(s.kind, ShelfKind::Mixed)));
        // nothing played, no favourite, no All shelf asked for: no shelf of the kind
        assert!(mixed_shelves(&shelves, Some(&serde_json::json!({})), &History::default()).is_empty());
    }

    #[test]
    fn automatic_shelves_and_shelves_that_fill_themselves() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("history.json"), r#"{"a":{"count":2,"ms":50,"last":100},"b":{"count":1,"ms":90,"last":900},"c":{"count":1,"ms":0,"last":500}}"#).unwrap();
        let history = History::load(tmp.path());
        let shelf = |id: &str, games: Vec<Game>| Shelf { id: id.into(), name: id.into(), kind: ShelfKind::Pc, games, from: Vec::new() };
        let g = |id: &str, title: &str, genres: &str| game(id, title, None, Some(genres), None);
        let shelves = vec![
            shelf("gc", vec![g("a", "Alpha", "Racing"), g("b", "Beta", "Racing"), g("c", "Gamma", "Racing; Puzzle")]),
            shelf("pc", vec![g("d", "Delta", "Racing"), g("e", "Epsilon", "Puzzle")]),
        ];
        let ui = serde_json::json!({ "autoShelves": { "recent": true, "most": true, "playing": true, "never": true, "genre": true, "finished": false },
            "collections": [{ "id": "s", "name": "Done", "ids": ["e"], "rule": { "kind": "status", "value": "finished" } }, { "id": "k", "name": "GC", "ids": [], "rule": { "kind": "console", "value": "gc" } }] });
        let status = |id: &str| match id { "a" => "playing".to_string(), "e" => "finished".into(), _ => String::new() };
        let mixed = mixed_shelves_with(&shelves, Some(&ui), &history, &status);
        let ids: Vec<(&str, Vec<&str>)> = mixed.iter().filter(|s| s.id != "home").map(|s| (s.id.as_str(), s.games.iter().map(|g| g.id.as_str()).collect())).collect();
        assert_eq!(
            ids,
            [
                ("auto-recent", vec!["b", "c", "a"]),
                ("auto-most", vec!["b", "a"]),
                ("auto-playing", vec!["a"]),
                ("auto-never", vec!["d", "e"]),
                // only genres with four games or more
                ("auto-genre-racing", vec!["a", "b", "d", "c"]),
                // a rule's games, not the ones picked by hand
                ("col-s", vec!["e"]),
                ("col-k", vec!["a", "b", "c"]),
            ]
        );
    }

    #[test]
    fn orders_and_filters() {
        let games = vec![
            game("a", "Alpha", Some("1999"), Some("Racing; Sports"), Some(3.5)),
            game("b", "Beta", None, None, None),
            game("c", "Gamma", Some("1987"), Some("Action"), Some(4.6)),
            game("d", "Delta", Some("1999"), Some("Action"), None),
        ];
        let history = History::default();
        let mut view = View { sort: "year".into(), filter: "all".into(), ..View::default() };
        let titles = |v: &View| arrange("gc", &games, v, &history).into_iter().map(|i| games[i].title.as_str()).collect::<Vec<_>>();
        assert_eq!(titles(&view), ["Gamma", "Alpha", "Delta", "Beta"]);
        view.sort = "rating".into();
        assert_eq!(titles(&view), ["Gamma", "Alpha", "Beta", "Delta"]);
        view.sort = "genre".into();
        assert_eq!(titles(&view), ["Gamma", "Delta", "Alpha", "Beta"]);
        view.sort = "custom".into();
        view.custom.insert("gc".into(), vec!["d".into(), "b".into()]);
        assert_eq!(titles(&view), ["Delta", "Beta", "Alpha", "Gamma"]);
        view.sort = "az".into();
        view.hidden.insert("a".into());
        view.filter = "dec:1990".into();
        assert_eq!(titles(&view), ["Delta"]);
        view.filter = "genre:Action".into();
        assert_eq!(titles(&view), ["Gamma", "Delta"]);
        view.filter = "fav".into();
        view.favorites.insert("b".into());
        assert_eq!(titles(&view), ["Beta"]);
        view.filter = "played".into();
        assert!(titles(&view).is_empty());

        let all: Vec<&Game> = games.iter().collect();
        let ids: Vec<String> = filters(&all, "all").into_iter().map(|f| f.0).collect();
        assert_eq!(ids, ["all", "fav", "played", "unplayed", "multi", "dec:1980", "dec:1990", "genre:Action", "genre:Racing", "genre:Sports"]);
        assert_eq!(group_of(&games[2], "rating", &history, 0).as_deref(), Some("★ 4.5+"));
        assert_eq!(group_of(&games[1], "year", &history, 0).as_deref(), Some("Undated"));
        assert_eq!(group_of(&games[0], "az", &history, 0).as_deref(), Some("A"));
    }
}

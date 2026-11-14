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

// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the pc-quick-edit
// branch's Edit the PC shelf (renderer/settings.js pcEditRows), for every shelf here.
//! Settings → Games → Edit a shelf: every game of a shelf in one list, each on the shelf, a favourite or hidden, a
//! step left or right away. It changes at once, through the same lists as H on the shelf and Details → Hide.

use super::*;

/// The most games one page lists; typing a few letters narrows a longer shelf.
const MOST: usize = 300;

const STATES: [(&str, &str); 3] = [("show", "On the shelf"), ("fav", "♥ Favorite"), ("hide", "Hidden")];

impl Settings {
    /// A game's place now: "show", "fav" or "hide" (hidden wins: a hidden favourite is nowhere to be seen).
    fn edit_state(&self, id: &str) -> &'static str {
        if self.hidden().iter().any(|x| x == id) {
            "hide"
        } else if self.list(&["ui", "favorites"]).iter().any(|x| x == id) {
            "fav"
        } else {
            "show"
        }
    }

    /// Puts a game on the shelf, among the favourites or hides it.
    pub(super) fn set_edit_state(&mut self, id: &str, state: &str) {
        for (list, wanted) in [("favorites", state == "fav"), ("hidden", state == "hide")] {
            let mut ids = self.list(&["ui", list]);
            let had = ids.iter().any(|x| x == id);
            if had != wanted {
                ids.retain(|x| x != id);
                if wanted {
                    ids.push(id.to_string());
                }
                self.set(&["ui", list], json!(ids));
            }
        }
    }

    pub(super) fn edit_shelves_page(&self) -> Page {
        let rows: Vec<Row> = self
            .shelf_games
            .iter()
            .map(|(id, name, games)| {
                let (fav, hid) = (games.iter().filter(|g| self.edit_state(g) == "fav").count(), games.iter().filter(|g| self.edit_state(g) == "hide").count());
                let id = id.clone();
                Row {
                    label: name.clone(),
                    value: format!("{} game{} · {fav} ♥ · {hid} hidden", games.len(), plural_s(games.len())),
                    hint: format!("Every game of {name} in one list: on the shelf, a favourite or hidden."),
                    press: Some(Box::new(move |s: &mut Settings| {
                        s.shelf_q.clear();
                        s.go(Screen::EditShelf(id.clone()));
                    })),
                    step: None,
                }
            })
            .collect();
        let lead = if rows.is_empty() { "No shelf has games yet." } else { "Pick a shelf, then move each game left or right: on the shelf, ♥ favorite or hidden." };
        Page { title: "Edit a shelf".into(), lead: lead.into(), rows }
    }

    pub(super) fn edit_shelf_page(&self, shelf: &str) -> Page {
        let Some((_, name, games)) = self.shelf_games.iter().find(|s| s.0 == shelf) else {
            return Page { title: "Edit a shelf".into(), lead: String::new(), rows: vec![Row { label: "This shelf isn't there any more".into(), ..Row::default() }] };
        };
        const SEARCH: &str = "Type part of a game's name, then Enter: only the games that match are listed.";
        let mut rows = vec![Row {
            label: "Find a game".into(),
            value: if self.shelf_q.is_empty() { "Type to find one".into() } else { self.shelf_q.clone() },
            hint: SEARCH.into(),
            press: Some(Box::new(|s: &mut Settings| {
                let now = s.shelf_q.clone();
                s.edit("Find a game", SEARCH, now, Box::new(|s: &mut Settings, q: String| s.shelf_q = q));
            })),
            step: None,
        }];
        let q = self.shelf_q.trim().to_lowercase();
        let mut list: Vec<(&String, &String)> = games.iter().filter_map(|g| self.titles.get(g).map(|t| (g, t))).filter(|(_, t)| q.is_empty() || t.to_lowercase().contains(&q)).collect();
        list.sort_by(|a, b| rustyshelf_core::collate::compare(a.1, b.1));
        if list.is_empty() {
            rows.push(Row { label: if q.is_empty() { "No games on this shelf".into() } else { format!("No game matches “{}”", self.shelf_q.trim()) }, ..Row::default() });
        } else if list.len() > MOST {
            rows.push(Row { label: format!("The first {MOST} of {}", list.len()), hint: "Find a game narrows the list.".into(), ..Row::default() });
        }
        for (id, title) in list.into_iter().take(MOST) {
            let now = STATES.iter().position(|s| s.0 == self.edit_state(id)).unwrap_or(0);
            let id = id.clone();
            rows.push(Row::turning(title.clone(), STATES[now].1, "Left / right: on the shelf, a favorite (♥, also on the Favorites shelf) or hidden from every shelf. It changes at once.", move |s: &mut Settings, d: i32| {
                let next = STATES[(now as i32 + d).rem_euclid(STATES.len() as i32) as usize].0;
                s.set_edit_state(&id, next);
            }));
        }
        Page { title: name.clone(), lead: "Each game: on the shelf, ♥ favorite or hidden. Left / right changes it at once.".into(), rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_moves_between_the_shelf_the_favourites_and_hidden() {
        let mut s = Settings { wizard: false, ..Settings::default() };
        s.set_edit_state("pc:zeta", "fav");
        assert_eq!((s.edit_state("pc:zeta"), s.list(&["ui", "favorites"])), ("fav", vec!["pc:zeta".to_string()]));
        s.set_edit_state("pc:zeta", "hide");
        assert_eq!((s.edit_state("pc:zeta"), s.list(&["ui", "favorites"]).len(), s.hidden()), ("hide", 0, vec!["pc:zeta".to_string()]));
        s.set_edit_state("pc:zeta", "show");
        assert!(s.edit_state("pc:zeta") == "show" && s.hidden().is_empty());
    }

    #[test]
    fn a_shelf_lists_its_games_a_to_z_and_narrows_to_what_is_typed() {
        let mut s = Settings { wizard: false, ..Settings::default() };
        s.shelf_games = vec![("pc".into(), "PC".into(), vec!["b".into(), "a".into(), "c".into()])];
        s.titles = [("a", "Alpha"), ("b", "Beta"), ("c", "Zeta Alpha")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let labels = |s: &Settings| s.edit_shelf_page("pc").rows.iter().skip(1).map(|r| r.label.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&s), ["Alpha", "Beta", "Zeta Alpha"]);
        s.shelf_q = "alpha".into();
        assert_eq!(labels(&s), ["Alpha", "Zeta Alpha"]);
        assert_eq!(s.edit_shelves_page().rows[0].value, "3 games · 0 ♥ · 0 hidden");
    }
}

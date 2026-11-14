// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the pages and their wording follow
// Collections in Boxshelf's renderer/settings.js, as RustyShelf has them.
//! Your collections: shelves of your own, each a name and the games put on it (`ui.collections` in the settings:
//! `[{ id, name, ids }]`). Details → Collections puts the chosen game on them; Settings → Collections renames and
//! deletes them.

use super::*;

/// The most collections there can be, and the longest name one takes.
const MOST: usize = 50;
const LONGEST: usize = 40;

/// A name as it's kept: spaces evened out, no longer than the longest.
fn named(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(LONGEST).collect()
}

impl Settings {
    fn collections(&self) -> Vec<Value> {
        at(&self.cfg, &["ui", "collections"]).and_then(Value::as_array).cloned().unwrap_or_default()
    }

    /// Puts a game on the collection of this name, making the collection if there's none.
    fn collect(&mut self, name: &str, game: &str, title: &str) {
        let name = named(name);
        if name.is_empty() {
            return;
        }
        let mut list = self.collections();
        let same = |c: &Value| c.get("name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase() == name.to_lowercase());
        match list.iter().position(same) {
            Some(i) => {
                if let Some(ids) = list[i].get_mut("ids").and_then(Value::as_array_mut).filter(|ids| !ids.iter().any(|i| i == game)) {
                    ids.push(json!(game));
                }
            }
            None if list.len() >= MOST => {
                self.say(format!("That's the most collections there can be ({MOST})."));
                return;
            }
            None => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                // the id the Electron app gives one: "c" and the time in base 36
                list.push(json!({ "id": format!("c{}", custom::base36(now)), "name": name, "ids": [game] }));
            }
        }
        self.set(&["ui", "collections"], json!(list));
        self.say(format!("{title} is on {name}"));
    }

    /// Changes one collection, by its id.
    fn change_collection(&mut self, id: &str, change: impl Fn(&mut Value)) {
        let mut list = self.collections();
        if let Some(c) = list.iter_mut().find(|c| c["id"] == id) {
            change(c);
        }
        self.set(&["ui", "collections"], json!(list));
    }

    /// Details → Collections: a switch per collection for the chosen game, and a new one by name.
    pub(super) fn game_collections_page(&self) -> Page {
        let Some(c) = &self.chosen else { return Page { title: "Collections".into(), lead: String::new(), rows: Vec::new() } };
        let (game, title) = (c.game.id.clone(), c.game.title.clone());
        let mut rows = Vec::new();
        for col in self.collections() {
            let (Some(id), Some(name)) = (col["id"].as_str().map(String::from), col["name"].as_str().map(String::from)) else { continue };
            let on = col["ids"].as_array().is_some_and(|ids| ids.iter().any(|i| *i == game));
            let flip = {
                let game = game.clone();
                move |s: &mut Settings| {
                    s.change_collection(&id, |c| {
                        if let Some(ids) = c.get_mut("ids").and_then(Value::as_array_mut) {
                            match ids.iter().position(|i| *i == game) {
                                Some(at) => {
                                    ids.remove(at);
                                }
                                None => ids.push(json!(game)),
                            }
                        }
                    })
                }
            };
            let flip2 = flip.clone();
            rows.push(Row { label: name, value: on_off(on), hint: format!("On: {title} is on this shelf."), press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| flip2(s))) });
        }
        const NEW: &str = "A name for a new shelf of your own. Enter makes it, with this game on it.";
        let (game, title2) = (game.clone(), title.clone());
        rows.push(Row {
            label: "New collection".into(),
            value: "Couch co-op, Racing, Finished…".into(),
            hint: NEW.into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let (game, title) = (game.clone(), title2.clone());
                s.edit("New collection", NEW, String::new(), Box::new(move |s: &mut Settings, name: String| s.collect(&name, &game, &title)));
            })),
            step: None,
        });
        Page { title: "Collections".into(), lead: format!("Shelves of your own, right after Home. {title} can be on as many as you like."), rows }
    }

    /// Settings → Collections: each one's name (Enter renames it) and Delete (pressed twice).
    pub(super) fn collections_page(&self) -> Page {
        let mut rows = Vec::new();
        for col in self.collections() {
            let (Some(id), Some(name)) = (col["id"].as_str().map(String::from), col["name"].as_str().map(String::from)) else { continue };
            let n = col["ids"].as_array().map_or(0, Vec::len);
            let (id2, name2, name3) = (id.clone(), name.clone(), name.clone());
            rows.push(Row {
                label: name.clone(),
                value: format!("{n} game{}", if n == 1 { "" } else { "s" }),
                hint: "The shelf's name. Enter renames it.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    let id = id.clone();
                    s.edit(
                        "Name",
                        "The collection's name.",
                        name2.clone(),
                        Box::new(move |s: &mut Settings, text: String| {
                            let text = named(&text);
                            if !text.is_empty() {
                                s.change_collection(&id, |c| c["name"] = json!(text));
                            }
                        }),
                    );
                })),
                step: None,
            });
            rows.push(Row {
                label: format!("Delete {name3}"),
                hint: "Takes the shelf away. Its games stay where they are.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if !s.twice(&format!("collection:{id2}"), "Press again to delete it") {
                        return;
                    }
                    let left: Vec<Value> = s.collections().into_iter().filter(|c| c["id"] != id2.as_str()).collect();
                    s.set(&["ui", "collections"], json!(left));
                })),
                ..Row::default()
            });
        }
        if rows.is_empty() {
            rows.push(Row { label: "No collections yet".into(), hint: "Open a game's Details and choose Collections to make one.".into(), ..Row::default() });
        }
        Page { title: "Collections".into(), lead: "Shelves of your own, right after Home.".into(), rows }
    }
}

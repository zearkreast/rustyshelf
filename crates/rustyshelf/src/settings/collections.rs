// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the pages and their wording follow
// Collections in Boxshelf's renderer/settings.js, as RustyShelf has them.
//! Your collections: shelves of your own, each a name and the games put on it (`ui.collections` in the settings:
//! `[{ id, name, ids, tag?, rule? }]`; a tag shelf also gathers every game with that genre). Details → Add to… puts the
//! chosen game on them; Settings → Collections makes them, and each one's page searches for games to put on it, renames
//! and deletes it.

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
        for col in self.collections().into_iter().filter(|c| c.get("rule").is_none()) {
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
        let (game2, title2) = (game.clone(), title.clone());
        rows.push(Row {
            label: "New collection".into(),
            value: "Couch co-op, Racing, Finished…".into(),
            hint: NEW.into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let (game, title) = (game2.clone(), title2.clone());
                s.edit("New collection", NEW, String::new(), Box::new(move |s: &mut Settings, name: String| s.collect(&name, &game, &title)));
            })),
            step: None,
        });
        if !super::kid::kid_mode_on(self.cfg.get("ui")) {
            let adds = self.cfg.get("ui").and_then(|u| u.get("shelfAdds")).cloned().unwrap_or(Value::Null);
            let targets = self.console_shelves.iter().cloned().chain([("pc".to_string(), "PC".to_string()), ("custom".to_string(), "Custom".to_string())]);
            for (to, name) in targets {
                if to == c.console {
                    rows.push(Row { label: format!("{name} · its own"), value: on_off(true), hint: format!("{name} is its own shelf: it always shows there"), ..Row::default() });
                    continue;
                }
                let on = adds.get(&to).and_then(Value::as_array).is_some_and(|l| l.iter().any(|i| *i == game));
                let flip = {
                    let (game, title, to, name) = (game.clone(), title.clone(), to.clone(), name.clone());
                    move |s: &mut Settings| s.add_to(&to, &name, &game, &title)
                };
                let flip2 = flip.clone();
                rows.push(Row { label: name.clone(), value: on_off(on), hint: format!("On: {title} shows on the {name} shelf too, and still starts as it does on its own"), press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| flip2(s))) });
            }
        }
        Page { title: "Add to…".into(), lead: format!("Shelves of your own, right after Home, and other consoles' shelves. {title} can be on as many as you like."), rows }
    }

    /// Add to…: the game shown on another console's shelf, PC or Custom (`ui.shelfAdds`), or taken off it.
    fn add_to(&mut self, to: &str, name: &str, game: &str, title: &str) {
        let mut list = self.list(&["ui", "shelfAdds", to]);
        let on = !list.iter().any(|g| g == game);
        if on {
            if list.len() >= 2000 {
                return self.say(format!("{name} can't take more games"));
            }
            list.push(game.to_string());
        } else {
            list.retain(|g| g != game);
        }
        let mut adds = self.cfg.get("ui").and_then(|u| u.get("shelfAdds")).and_then(Value::as_object).cloned().unwrap_or_default();
        if list.is_empty() {
            adds.remove(to);
        } else {
            adds.insert(to.to_string(), json!(list));
        }
        self.set(&["ui", "shelfAdds"], Value::Object(adds));
        self.say(if on { format!("{title} added to {name}") } else { format!("{title} taken off {name}") });
    }

    /// Settings → Collections: the automatic shelves, Make a new shelf, each of yours (opening its own page), tag shelves.
    pub(super) fn collections_page(&self) -> Page {
        let mut rows = Vec::new();
        // (beta.14) the automatic shelves, each off until switched on, after Home
        let hints = [
            ("recent", "The games you played last, newest first. Up to 30."),
            ("most", "The games you played longest, most first. Up to 30."),
            ("playing", "Every game whose Status is Playing (in its Details)."),
            ("finished", "Every game whose Status is Finished (in its Details)."),
            ("never", "Games you haven't started yet, A to Z. Up to 60."),
            ("genre", "A shelf for each genre with 4 games or more, the 8 biggest."),
        ];
        for (k, hint) in hints {
            let name = rustyshelf_core::shelfview::AUTO_SHELVES.iter().find(|a| a.0 == k).map_or("By genre", |a| a.1);
            let on = at(&self.cfg, &["ui", "autoShelves", k]).and_then(Value::as_bool).unwrap_or(false);
            let flip = move |s: &mut Settings| s.set(&["ui", "autoShelves", k], json!(!on));
            rows.push(Row { label: name.into(), value: on_off(on), hint: format!("{hint} It shows after Home."), press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| flip(s))) });
        }
        const NEW: &str = "Give it a name and press Enter; then search your games and switch them on.";
        rows.push(Row {
            label: "Make a new shelf".into(),
            value: "Couch co-op, Racing, Finished…".into(),
            hint: NEW.into(),
            press: Some(Box::new(|s: &mut Settings| s.edit("Make a new shelf", NEW, String::new(), Box::new(|s: &mut Settings, name: String| s.new_shelf(&name))))),
            step: None,
        });
        for col in self.collections() {
            let (Some(id), Some(name)) = (col["id"].as_str().map(String::from), col["name"].as_str().map(String::from)) else { continue };
            let n = col["ids"].as_array().map_or(0, Vec::len);
            rows.push(Row {
                label: name.clone(),
                value: if col.get("rule").is_some() { "Automatic".into() } else { format!("{n} game{}", if n == 1 { "" } else { "s" }) },
                hint: format!("Open {name}: search for games to add, take games off, rename or delete it."),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.shelf_q.clear();
                    s.go(Screen::MyShelf(id.clone()));
                })),
                step: None,
            });
        }
        if self.collections().is_empty() {
            rows.push(Row { label: "No shelves of your own yet".into(), hint: "Make one above, or choose Add to… in any game's Details".into(), ..Row::default() });
        }
        // a tag shelf: every game with that genre, from every console, plus any added to it by hand
        let has = |g: &str| self.collections().iter().any(|c| c.get("tag").and_then(Value::as_str).is_some_and(|t| t.to_lowercase() == g.to_lowercase()));
        if self.genres.is_empty() {
            rows.push(Row { label: "No genres yet".into(), hint: "They come with your games' details (box art downloads fetch them).".into(), ..Row::default() });
        }
        for (g, n) in &self.genres {
            let (g2, on) = (g.clone(), has(g));
            rows.push(Row {
                label: format!("Add a tag shelf: {g}"),
                value: format!("{n} game{}{}", if *n == 1 { "" } else { "s" }, if on { " · on your shelves" } else { "" }),
                hint: format!("A shelf of every {g} game, from every console. You can still add other games to it from their Details (Collections)."),
                press: Some(Box::new(move |s: &mut Settings| s.add_tag_shelf(&g2))),
                step: None,
            });
        }
        Page { title: "Collections".into(), lead: "Shelves of your own, right after Home.".into(), rows }
    }

    /// Makes a collection of this name (or finds the one there is) and opens it.
    fn new_shelf(&mut self, name: &str) {
        let name = named(name);
        if name.is_empty() {
            return;
        }
        let mut list = self.collections();
        let same = |c: &Value| c.get("name").and_then(Value::as_str).is_some_and(|n| n.to_lowercase() == name.to_lowercase());
        let id = match list.iter().find(|c| same(c)) {
            Some(c) => c["id"].as_str().unwrap_or("").to_string(),
            None if list.len() >= MOST => return self.say(format!("That's the most collections there can be ({MOST}).")),
            None => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                let id = format!("c{}", custom::base36(now));
                list.push(json!({ "id": id, "name": name, "ids": [] }));
                self.set(&["ui", "collections"], json!(list));
                id
            }
        };
        self.shelf_q.clear();
        self.go(Screen::MyShelf(id));
    }

    /// One of your collections: what fills it; Search games and the games that match, each with a switch; the games on
    /// it; its name; Delete, pressed twice.
    pub(super) fn my_shelf_page(&self, id: &str) -> Page {
        let Some(col) = self.collections().into_iter().find(|c| c["id"] == id) else {
            return Page { title: "Collections".into(), lead: String::new(), rows: vec![Row { label: "This shelf was deleted".into(), ..Row::default() }] };
        };
        let name = col["name"].as_str().unwrap_or("Collection").to_string();
        let ids: Vec<String> = col["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        let mut rows = Vec::new();
        let rule = rule_of(&col);
        let choices = self.rule_choices(&rule);
        let at_now = choices.iter().position(|c| c.0 == rule).unwrap_or(0);
        let id4 = id.to_string();
        let turn = move |s: &mut Settings, d: i32| {
            let (v, _) = &choices[(at_now as i32 + d).rem_euclid(choices.len() as i32) as usize];
            let v = v.clone();
            s.change_collection(&id4, |c| match v.as_str() {
                "off" => {
                    if let Some(o) = c.as_object_mut() {
                        o.remove("rule");
                    }
                }
                "never" => c["rule"] = json!({ "kind": "never", "value": "" }),
                _ => {
                    let (kind, value) = v.split_once(':').unwrap_or((&v, ""));
                    c["rule"] = json!({ "kind": kind, "value": value });
                }
            });
        };
        rows.push(Row {
            label: "Fill it automatically".into(),
            value: self.rule_name(&rule),
            hint: "Off: the games you pick. Or the shelf fills itself, with every game of a Status, genre or console, or the ones you never played.".into(),
            press: Some(Box::new({
                let turn = turn.clone();
                move |s: &mut Settings| turn(s, 1)
            })),
            step: Some(Box::new(turn)),
        });
        let toggle = |game: &str, title: &str| {
            let on = ids.iter().any(|i| i == game);
            let (cid, game) = (id.to_string(), game.to_string());
            let flip = move |s: &mut Settings| {
                let game = game.clone();
                s.change_collection(&cid, move |c| {
                    if let Some(ids) = c.get_mut("ids").and_then(Value::as_array_mut) {
                        match ids.iter().position(|i| *i == game) {
                            Some(at) => {
                                ids.remove(at);
                            }
                            None => ids.push(json!(game)),
                        }
                    }
                })
            };
            let flip2 = flip.clone();
            Row { label: title.to_string(), value: on_off(on), hint: format!("On: {title} is on {name}."), press: Some(Box::new(flip)), step: Some(Box::new(move |s: &mut Settings, _| flip2(s))) }
        };
        if rule == "off" {
            const SEARCH: &str = "Type part of a game's name, then Enter: the games that match show below, each with a switch to put it on this shelf.";
            rows.push(Row {
                label: "Search games".into(),
                value: if self.shelf_q.is_empty() { "Zelda, Mario Kart, Crash…".into() } else { self.shelf_q.clone() },
                hint: SEARCH.into(),
                press: Some(Box::new(|s: &mut Settings| {
                    let now = s.shelf_q.clone();
                    s.edit("Search games", SEARCH, now, Box::new(|s: &mut Settings, q: String| s.shelf_q = q));
                })),
                step: None,
            });
            let q = self.shelf_q.trim().to_lowercase();
            if !q.is_empty() {
                let mut hits: Vec<(&String, &String, usize)> = self.titles.iter().filter_map(|(gid, t)| t.to_lowercase().find(&q).map(|at| (gid, t, at))).collect();
                hits.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| rustyshelf_core::collate::compare(a.1, b.1)));
                let label = if hits.is_empty() { format!("No game matches “{}”", self.shelf_q.trim()) } else { format!("Games matching “{}”", self.shelf_q.trim()) };
                rows.push(Row { label, value: if hits.len() > 40 { format!("first 40 of {}", hits.len()) } else { String::new() }, ..Row::default() });
                rows.extend(hits.iter().take(40).map(|(gid, t, _)| toggle(gid, t)));
            }
            let on: Vec<(&String, &String)> = ids.iter().filter_map(|g| self.titles.get(g).map(|t| (g, t))).collect();
            rows.push(Row { label: "On this shelf".into(), value: format!("{} game{}", on.len(), if on.len() == 1 { "" } else { "s" }), ..Row::default() });
            if on.is_empty() {
                rows.push(Row { label: "No games yet".into(), hint: "Search above and switch a game on to add it".into(), ..Row::default() });
            }
            rows.extend(on.iter().map(|(g, t)| toggle(g, t)));
        } else {
            rows.push(Row { label: format!("Fills itself: {}", self.rule_name(&rule)), hint: "Games join and leave by themselves. Off brings back the ones you picked.".into(), ..Row::default() });
        }
        let (id2, id3, name2) = (id.to_string(), id.to_string(), name.clone());
        rows.push(Row {
            label: "Rename".into(),
            value: name.clone(),
            hint: "The shelf's name, as it shows beside Home. Enter renames it.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let id = id2.clone();
                s.edit(
                    "Rename",
                    "The shelf's name, as it shows beside Home. Enter renames it.",
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
        let name3 = name.clone();
        rows.push(Row {
            label: "Delete this shelf".into(),
            hint: "Removes the shelf (its games stay everywhere else). Press twice.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                if !s.twice(&format!("collection:{id3}"), "Press again to delete it") {
                    return;
                }
                let left: Vec<Value> = s.collections().into_iter().filter(|c| c["id"] != id3.as_str()).collect();
                s.set(&["ui", "collections"], json!(left));
                s.back();
                s.say(format!("{name3} deleted"));
            })),
            ..Row::default()
        });
        Page { title: name, lead: "Search your games and switch them on to put them on this shelf. It shows beside Home once it has a game on it.".into(), rows }
    }

    /// The choices for a shelf that fills itself: its value ("off", "never", "status:finished", "genre:Racing",
    /// "console:gc") and its name; the one it has now is always among them.
    fn rule_choices(&self, now: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = [("off", "Off (you pick)"), ("status:playing", "Status: Playing"), ("status:finished", "Status: Finished"), ("status:notstarted", "Status: Not started"), ("status:abandoned", "Status: Abandoned"), ("never", "Never played")]
            .iter()
            .map(|(v, n)| (v.to_string(), n.to_string()))
            .collect();
        out.extend(self.console_shelves.iter().map(|(id, name)| (format!("console:{id}"), format!("Console: {name}"))));
        out.extend(self.genres.iter().map(|(g, _)| (format!("genre:{g}"), format!("Genre: {g}"))));
        if !out.iter().any(|c| c.0 == now) {
            out.push((now.to_string(), self.rule_name(now)));
        }
        out
    }

    fn rule_name(&self, v: &str) -> String {
        match v.split_once(':') {
            _ if v == "off" => "Off".into(),
            _ if v == "never" => "Never played".into(),
            Some(("status", x)) => format!("Status: {}", match x { "playing" => "Playing", "finished" => "Finished", "notstarted" => "Not started", "abandoned" => "Abandoned", other => other }),
            Some(("genre", x)) => format!("Genre: {x}"),
            Some(("console", x)) => format!("Console: {}", self.console_shelves.iter().find(|c| c.0 == x).map_or(x, |c| c.1.as_str())),
            _ => v.into(),
        }
    }

    fn add_tag_shelf(&mut self, genre: &str) {
        let mut list = self.collections();
        if list.iter().any(|c| c.get("tag").and_then(Value::as_str).is_some_and(|t| t.to_lowercase() == genre.to_lowercase())) {
            self.say(format!("There's a {genre} shelf already"));
            return;
        }
        if list.len() >= MOST {
            self.say(format!("That's the most collections there can be ({MOST})."));
            return;
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let name: String = genre.chars().take(LONGEST).collect();
        list.push(json!({ "id": format!("t{}", custom::base36(now)), "name": name, "ids": [], "tag": name }));
        self.set(&["ui", "collections"], json!(list));
        self.say(format!("{genre} shelf added"));
    }
}

/// A shelf's rule as one value: "off", "never", "status:finished", "genre:Racing", "console:gc".
fn rule_of(c: &Value) -> String {
    match c.get("rule").and_then(|r| r.get("kind")).and_then(Value::as_str) {
        None => "off".into(),
        Some("never") => "never".into(),
        Some(kind) => format!("{kind}:{}", c["rule"].get("value").and_then(Value::as_str).unwrap_or("")),
    }
}

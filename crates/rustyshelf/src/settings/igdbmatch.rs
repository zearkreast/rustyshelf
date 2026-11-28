// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the
// pc-details-media branch's IGDB match picker and manual details editor (renderer/settings.js, main/igdb.js search).
//! Details → IGDB match: which IGDB game a game's description, genres, developer and dates come from, picked from a
//! search when the automatic one is wrong (kept in `igdb-info`, never looked up again by itself). Details → Edit
//! details: the description, developer, publisher, genres and year typed by hand (`games.<id>.info`), over every
//! source.

use rustyshelf_core::igdb::{self, Igdb};

use super::*;

#[derive(Default)]
pub(super) struct IgdbMatchState {
    term: String,
    /// what was looked up last, and what came of it
    q: String,
    found: Option<Result<Vec<Value>, String>>,
    looking: Option<Later<Result<Vec<Value>, String>>>,
}

/// The fields Edit details has: its label, its key in `games.<id>.info`, and what it's for.
const FIELDS: [(&str, &str, &str); 5] = [
    ("Description", "overview", "What the game is about, as Details shows it."),
    ("Developer", "dev", "Who made it. Several are separated by commas."),
    ("Publisher", "pub", "Who published it."),
    ("Genres", "genres", "Separated by semicolons: Action; Adventure."),
    ("Year", "year", "The year it came out, four digits."),
];

impl Settings {
    /// The chosen game's IGDB platform and its key in `igdb-info`; None for a game IGDB isn't asked about.
    fn igdb_key(&self) -> Option<(String, String)> {
        let c = self.chosen.as_ref()?;
        let sys = match &c.game.source {
            Source::Rom => c.console.clone(),
            Source::Pc(_) | Source::Steam { .. } => "pc".to_string(),
            _ => return None,
        };
        let key = rustyshelf_core::gamekey::game_key(&sys, &c.game.file, &c.game.title);
        Some((sys, key))
    }

    /// Details' rows for where its info comes from: IGDB match and Edit details.
    pub(super) fn info_rows(&self) -> Vec<Row> {
        let Some((_, key)) = self.igdb_key() else { return Vec::new() };
        let picked = igdb::Store::load(&paths::data_dir(&self.app_root)).picked_name(&key).map(String::from);
        let edited = self.chosen.as_ref().and_then(|c| self.cfg["games"][&c.game.id]["info"].as_object()).is_some_and(|o| !o.is_empty());
        vec![
            Row {
                label: "IGDB match".into(),
                value: picked.unwrap_or_else(|| "Automatic".into()),
                hint: "Which IGDB game the description, genres, developer and dates come from: search and pick another when they're wrong.".into(),
                press: Some(Box::new(|s: &mut Settings| s.open_igdb_match())),
                step: None,
            },
            Row {
                label: "Edit details".into(),
                value: if edited { "Edited".into() } else { String::new() },
                hint: "Type the description, developer, publisher, genres or year yourself; what you type wins over every source.".into(),
                press: Some(Box::new(|s: &mut Settings| s.go(Screen::EditDetails))),
                step: None,
            },
        ]
    }

    fn open_igdb_match(&mut self) {
        let Some(title) = self.chosen.as_ref().map(|c| c.game.title.clone()) else { return };
        self.igdb_match = IgdbMatchState::default();
        self.go(Screen::IgdbMatch);
        self.igdb_look(title);
    }

    fn igdb_look(&mut self, term: String) {
        let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
        self.igdb_match.term = term.clone();
        let Some((sys, _)) = self.igdb_key().filter(|_| !term.is_empty()) else { return };
        let data = paths::data_dir(&self.app_root);
        let q: String = term.chars().take(120).collect();
        let looking = Later::spawn(move || match Igdb::from_settings(&data) {
            Some(c) if c.ready() => c.search(&sys, &q).map_err(|e| e.why),
            _ => Err("Add your IGDB keys in Settings → Accounts first.".into()),
        });
        (self.igdb_match.q, self.igdb_match.found, self.igdb_match.looking, self.redraw) = (term, None, Some(looking), true);
    }

    pub(super) fn poll_igdb_match(&mut self) {
        if let Some(found) = self.igdb_match.looking.as_ref().and_then(Later::poll) {
            (self.igdb_match.found, self.igdb_match.looking, self.redraw) = (Some(found), None, true);
        }
    }

    /// Keeps `hit` as the game's match (None: automatic again), shows it in Details at once and reads the shelf again.
    fn use_igdb_match(&mut self, hit: Option<&Value>) {
        let Some((_, key)) = self.igdb_key() else { return };
        let data = paths::data_dir(&self.app_root);
        let mut store = igdb::Store::load(&data);
        match hit {
            Some(h) => store.pick(&key, h, rustyshelf_core::js::date_now()),
            None => store.forget(&key),
        }
        if let Err(e) = store.save() {
            return self.say(format!("It couldn't be kept ({e})."));
        }
        if let (Some(c), Some(i)) = (self.chosen.as_mut(), hit.and_then(igdb::info_of)) {
            igdb::over(c.game.info.get_or_insert_with(Default::default), &i);
        }
        crate::loading::STALE.store(true, std::sync::atomic::Ordering::Relaxed);
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        self.note = match hit.and_then(|h| h["name"].as_str()) {
            Some(name) => format!("{title}'s details come from \"{name}\" now."),
            None => format!("{title}'s details are looked up by themselves again."),
        };
        self.back();
    }

    pub(super) fn igdb_match_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let picked = self.igdb_key().and_then(|(_, key)| igdb::Store::load(&paths::data_dir(&self.app_root)).picked_name(&key).map(String::from));
        const SEARCH: &str = "A game's name to look up on IGDB. Enter searches.";
        let mut rows = vec![Row {
            label: "Search for".into(),
            value: self.igdb_match.term.clone(),
            hint: SEARCH.into(),
            press: Some(Box::new(|s: &mut Settings| {
                let had = s.igdb_match.term.clone();
                s.edit("Search for", SEARCH, had, Box::new(|s: &mut Settings, typed: String| s.igdb_look(typed)));
            })),
            step: None,
        }];
        if let Some(now) = &picked {
            rows.push(Row {
                label: "Back to automatic".into(),
                value: "Automatic".into(),
                hint: format!("Now: \"{now}\". Automatic looks this game up by its own name again."),
                press: Some(Box::new(|s: &mut Settings| s.use_igdb_match(None))),
                step: None,
            });
        }
        let q = &self.igdb_match.q;
        match &self.igdb_match.found {
            None if self.igdb_match.looking.is_some() => rows.push(Row { label: "Looking…".into(), ..Row::default() }),
            Some(Err(why)) => rows.push(Row { label: "IGDB couldn't be asked".into(), hint: why.clone(), ..Row::default() }),
            Some(Ok(list)) if list.is_empty() => rows.push(Row { label: format!("Nothing found for \"{q}\""), hint: "Try fewer words, or another spelling.".into(), ..Row::default() }),
            Some(Ok(list)) => {
                rows.push(Row { label: format!("— Found ({}) —", list.len()), ..Row::default() });
                for h in list {
                    let name = h["name"].as_str().unwrap_or("").to_string();
                    let info = igdb::info_of(h).unwrap_or_default();
                    let year = info["year"].as_u64().map(|y| y.to_string());
                    let platforms: Vec<&str> = h["platforms"].as_array().into_iter().flatten().filter_map(|p| p["name"].as_str()).take(3).collect();
                    let value = [year.unwrap_or_default(), platforms.join(", ")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                    let about = info["overview"].as_str().map(|o| o.chars().take(240).collect::<String>()).unwrap_or_default();
                    let hit = h.clone();
                    rows.push(Row {
                        label: if picked.as_deref() == Some(name.as_str()) { format!("{name} (in use)") } else { name },
                        value,
                        hint: if about.is_empty() { format!("Its details become {title}'s.") } else { about },
                        press: Some(Box::new(move |s: &mut Settings| s.use_igdb_match(Some(&hit)))),
                        step: None,
                    });
                }
            }
            None => {}
        }
        Page { title: "IGDB match".into(), lead: format!("Which IGDB game {title}'s description, genres, developer and dates come from. It's remembered for this game."), rows }
    }

    /// Sets (or, empty, clears) one of the game's own details, and shows it in Details at once.
    fn set_own_info(&mut self, key: &'static str, value: String) {
        let Some(id) = self.chosen.as_ref().map(|c| c.game.id.clone()) else { return };
        let value = value.trim().to_string();
        if key == "year" && !value.is_empty() && !(value.len() == 4 && value.chars().all(|c| c.is_ascii_digit())) {
            return self.say("A year is four digits: 1998.".into());
        }
        let mut own = self.cfg["games"][&id]["info"].as_object().cloned().unwrap_or_default();
        if value.is_empty() {
            own.shift_remove(key);
        } else {
            own.insert(key.into(), json!(value));
        }
        let own = Value::Object(own);
        self.set(&["games", &id, "info"], own.clone());
        if let Some(c) = self.chosen.as_mut() {
            igdb::over(c.game.info.get_or_insert_with(Default::default), &own);
        }
        crate::loading::STALE.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub(super) fn edit_details_page(&self) -> Page {
        let Some(c) = self.chosen.as_ref() else { return Page { title: "Edit details".into(), lead: String::new(), rows: Vec::new() } };
        let own = self.cfg["games"][&c.game.id]["info"].as_object().cloned().unwrap_or_default();
        let info = c.game.info.clone().unwrap_or_default();
        let shown = |k: &str| match k {
            "overview" => info.overview.clone(),
            "dev" => info.dev.clone(),
            "pub" => info.publisher.clone(),
            "genres" => info.genres.clone(),
            _ => info.year.clone().or_else(|| info.release.as_ref().map(|r| r.chars().take(4).collect())),
        };
        let mut rows: Vec<Row> = FIELDS
            .iter()
            .map(|&(label, key, hint)| {
                let now = shown(key).unwrap_or_default();
                let mine = own.contains_key(key);
                let short: String = if now.chars().count() > 60 { format!("{}…", now.chars().take(59).collect::<String>()) } else { now.clone() };
                Row {
                    label: label.into(),
                    value: if now.is_empty() { "—".into() } else if mine { format!("{short} (yours)") } else { short },
                    hint: format!("{hint} Empty puts back what the sources say."),
                    press: Some(Box::new(move |s: &mut Settings| {
                        let had = now.clone();
                        s.edit(label, hint, had, Box::new(move |s: &mut Settings, typed: String| s.set_own_info(key, typed)));
                    })),
                    step: None,
                }
            })
            .collect();
        if !own.is_empty() {
            let id = c.game.id.clone();
            rows.push(Row {
                label: "Put back what the sources say".into(),
                hint: "Forgets everything typed here for this game.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    // (set, so the change is saved and the shelf read again, then taken out, as Details' Status does)
                    s.set(&["games", &id, "info"], Value::Null);
                    if let Some(o) = s.cfg.get_mut("games").and_then(|g| g.get_mut(&id)).and_then(Value::as_object_mut) {
                        o.shift_remove("info");
                    }
                    crate::loading::STALE.store(true, std::sync::atomic::Ordering::Relaxed);
                    s.note = "The details come from the sources again once the shelf is read again.".into();
                })),
                ..Row::default()
            });
        }
        Page { title: "Edit details".into(), lead: format!("{}'s details as you want them. What you type wins over IGDB, LaunchBox and the stores.", c.game.title), rows }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_typed_by_hand_are_kept_with_the_game_and_win() {
        let mut s = Settings { wizard: false, ..Settings::default() };
        let game = Game { id: "pc:zeta".into(), title: "Zeta".into(), source: Source::Pc(Box::default()), info: Some(rustyshelf_core::info::Info { dev: Some("Old".into()), ..Default::default() }), ..Game::default() };
        s.chosen = Some(Chosen { console: "pc".into(), emulator: String::new(), game, plays: None });
        s.set_own_info("dev", "  New Studio ".into());
        assert_eq!(s.cfg["games"]["pc:zeta"]["info"]["dev"], "New Studio");
        assert_eq!(s.chosen.as_ref().unwrap().game.info.as_ref().unwrap().dev.as_deref(), Some("New Studio"));
        s.set_own_info("year", "98".into());
        assert!(s.cfg["games"]["pc:zeta"]["info"].get("year").is_none());
        s.set_own_info("dev", String::new());
        assert!(s.cfg["games"]["pc:zeta"]["info"].get("dev").is_none());
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): Details → Art match
// of Boxshelf's renderer/settings.js and main.js (art:matchSearch, art:matchSet), as RustyShelf has it.
//! Details → Art match: which game in the box art databases a console game's art comes from, for a ROM hack, a
//! translation or a mod whose file name or serial isn't the original game's. Kept with the game
//! (`games.<id>.artName`).

use rustyshelf_core::art::ArtLibrary;
use rustyshelf_core::boxart::{self, ArtMatch};
use rustyshelf_core::covers;

use super::*;

#[derive(Default)]
pub(super) struct ArtMatchState {
    term: String,
    /// what was looked up last, and what came of it (the matches, and whether the LaunchBox index is built)
    q: String,
    found: Option<(Vec<ArtMatch>, bool)>,
    looking: Option<Mutex<Receiver<(Vec<ArtMatch>, bool)>>>,
}

impl Settings {
    fn art_match_now(&self) -> String {
        self.chosen.as_ref().and_then(|c| covers::art_name(&self.cfg, &c.game.id)).unwrap_or("").to_string()
    }

    /// The Details row, for a console game.
    pub(super) fn art_match_row(&self) -> Row {
        let now = self.art_match_now();
        Row {
            label: "Art match".into(),
            value: if now.is_empty() { "Automatic".into() } else { now },
            hint: "Which game in the box art databases this one's art comes from: for a ROM hack, a translation or a mod, search its original name.".into(),
            press: Some(Box::new(|s: &mut Settings| s.open_art_match())),
            step: None,
        }
    }

    fn open_art_match(&mut self) {
        let Some(title) = self.chosen.as_ref().map(|c| c.game.title.clone()) else { return };
        let now = self.art_match_now();
        self.art_match = ArtMatchState::default();
        self.go(Screen::ArtMatch);
        self.art_match_look(if now.is_empty() { title } else { now });
    }

    fn art_match_look(&mut self, term: String) {
        let term = term.split_whitespace().collect::<Vec<_>>().join(" ");
        self.art_match.term = term.clone();
        let Some(sys) = self.chosen.as_ref().map(|c| c.console.clone()).filter(|_| !term.is_empty()) else { return };
        let cache = paths::boxart_cache(&paths::data_dir(&self.app_root));
        let q: String = term.chars().take(120).collect();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(boxart::art_matches(&cache, &sys, &q));
        });
        (self.art_match.q, self.art_match.found, self.art_match.looking, self.redraw) = (term, None, Some(Mutex::new(rx)), true);
    }

    pub(super) fn poll_art_match(&mut self) {
        if let Some(found) = self.art_match.looking.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
            (self.art_match.found, self.art_match.looking, self.redraw) = (Some(found), None, true);
        }
    }

    /// Keeps `name` as the game's art match ("" looks it up under its own name again), fetches its art again behind
    /// the shelf (parts picked in the Art Studio stay) and closes Settings.
    fn use_art_match(&mut self, name: &str) {
        let Some((sys, game)) = self.chosen.as_ref().map(|c| (c.console.clone(), c.game.clone())) else { return };
        let mut games = self.cfg.get("games").filter(|g| g.is_object()).cloned().unwrap_or_else(|| json!({}));
        let mut entry = games.get(&game.id).and_then(Value::as_object).cloned().unwrap_or_default();
        if name.is_empty() {
            entry.shift_remove("artName");
        } else {
            entry.insert("artName".into(), json!(name));
        }
        if let Some(map) = games.as_object_mut() {
            if entry.is_empty() {
                map.shift_remove(&game.id);
            } else {
                map.insert(game.id.clone(), Value::Object(entry));
            }
        }
        self.set(&["games"], games);
        let (cfg, root, title) = (self.cfg.clone(), self.app_root.clone(), game.title.clone());
        std::thread::spawn(move || {
            let (Some(config), Some(console)) = (Config::from_value(&cfg), systems().console(&sys)) else { return };
            let sc = config.system(console);
            let art = ArtLibrary::from_config(&config, &root);
            let cache = paths::boxart_cache(&paths::data_dir(&root));
            let run = boxart::Run { art: &art, app_root: &root, cache_dir: &cache, kinds: &["redo-front", "redo-back", "redo-spine", "redo-disc"], region: config.art_region(), hd: config.art_hd(), explicit: true, cfg: &cfg, index_ready_only: true };
            let report = boxart::download(&run, &[boxart::Wanted { console, config: &sc, games: vec![&game] }], &|_, _, _| {}, &std::sync::atomic::AtomicBool::new(false));
            if report.systems.iter().any(|r| r.front + r.back + r.spine + r.disc > 0) {
                crate::loading::STALE.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        self.toast = Some(if name.is_empty() { format!("{title}: art looked up under its own name again") } else { format!("{title}: its art comes from \"{name}\" now") });
        self.shut();
    }

    pub(super) fn art_match_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let now = self.art_match_now();
        const SEARCH: &str = "A game's name to look up in the box art databases (LaunchBox, libretro). Enter searches.";
        let mut rows = vec![Row {
            label: "Search for".into(),
            value: self.art_match.term.clone(),
            hint: SEARCH.into(),
            press: Some(Box::new(|s: &mut Settings| {
                let had = s.art_match.term.clone();
                s.edit("Search for", SEARCH, had, Box::new(|s: &mut Settings, typed: String| s.art_match_look(typed)));
            })),
            step: None,
        }];
        if !now.is_empty() {
            rows.push(Row {
                label: "Back to automatic".into(),
                value: "Automatic".into(),
                hint: format!("Now: \"{now}\". Automatic looks this game's art up under its own name again."),
                press: Some(Box::new(|s: &mut Settings| s.use_art_match(""))),
                step: None,
            });
        }
        let q = &self.art_match.q;
        match &self.art_match.found {
            None if self.art_match.looking.is_some() => rows.push(Row { label: "Looking…".into(), ..Row::default() }),
            Some((list, ready)) if list.is_empty() => rows.push(Row {
                label: format!("Nothing found for \"{q}\""),
                hint: if *ready { "Try fewer words, or another spelling.".into() } else { "The LaunchBox box art database isn't downloaded yet: Box art builds it the first time box art is fetched.".into() },
                ..Row::default()
            }),
            Some((list, _)) => {
                rows.push(Row { label: format!("— Found ({}) —", list.len()), ..Row::default() });
                for m in list {
                    let what = if m.launchbox { format!("LaunchBox · {} picture{}", m.pictures, plural_s(m.pictures)) } else { "libretro · front cover".into() };
                    let name = m.name.clone();
                    rows.push(Row {
                        label: m.name.clone(),
                        value: if now == m.name { "In use" } else { "Use" }.into(),
                        hint: format!("{what}. Its box art becomes {title}'s, fetched again now (art you picked in the Art Studio stays)."),
                        press: Some(Box::new(move |s: &mut Settings| s.use_art_match(&name))),
                        step: None,
                    });
                }
            }
            None => {}
        }
        Page {
            title: "Art match".into(),
            lead: format!("Which game {title}'s box art comes from. For a ROM hack, a translation or a mod whose file name or serial isn't the original game's: search its original name and pick it. It's remembered for this game."),
            rows,
        }
    }
}

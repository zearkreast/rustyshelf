//! Details → Find a manual: the manuals archive.org and GamesDatabase.org have for the chosen game, listed to
//! pick from; the one picked is saved as pages beside the game's box art, where the reader finds them.

use std::sync::Arc;

use rustyshelf_core::art::{art_base, ArtLibrary};
use rustyshelf_core::manuals::{self, Candidate, Got, Online, Rejected, Stores};
use rustyshelf_core::{manualcoll, paths, pccovers};

use super::*;

/// The search for the chosen game's manual, and the download of the one picked.
#[derive(Default)]
pub(super) struct ManualSearch {
    /// the game it's for
    game: String,
    asking: Option<Mutex<Receiver<Result<Vec<Candidate>, String>>>>,
    found: Option<Result<Vec<Candidate>, String>>,
    /// the pages down and their number, and the download's end
    getting: Option<(Arc<Mutex<(usize, usize)>>, Mutex<Receiver<Result<Got, String>>>)>,
    /// a manual being set aside as another game's, and what came of it
    rejecting: Option<Mutex<Receiver<Result<Rejected, String>>>>,
    /// each console's choice of manuals and the games whose manual is kept as it is, as the data folder has
    /// them: read when a page first asks
    kept: std::sync::OnceLock<(std::collections::HashMap<String, String>, std::collections::HashSet<String>)>,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

impl Settings {
    /// Where the chosen game's manual goes, and the name its art has: a PC game's on a console's shelf is kept with
    /// its art, under its id.
    fn manual_place(&self) -> Option<(PathBuf, String)> {
        let c = self.chosen.as_ref()?;
        let console = systems().console(&c.console)?;
        match c.game.source {
            Source::Rom => {
                let config = Config::from_value(&self.cfg)?;
                Some((ArtLibrary::from_config(&config, &self.app_root).manual_dir_for(&config.system(console), &c.game.file, &self.app_root), art_base(&c.game.file)))
            }
            Source::Pc(_) => Some((paths::data_dir(&self.app_root).join("pc-covers"), pccovers::safe_id(&c.game.id))),
            _ => None,
        }
    }

    /// Whether Details offers the search: a console's game, or a PC game on a console's shelf, with no manual yet.
    pub(super) fn can_find_manual(&self) -> bool {
        self.manual_place().is_some() && self.chosen.as_ref().is_some_and(|c| c.game.art.manual.is_empty() && c.game.art.manual_pdf.is_none())
    }

    pub(super) fn find_manual(&mut self) {
        let Some(c) = &self.chosen else { return };
        let (sys, title, id) = (c.console.clone(), c.game.title.clone(), c.game.id.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(manuals::search(&Online, &sys, &title).map_err(|e| e.words()));
        });
        self.manual_search = ManualSearch { game: id, asking: Some(Mutex::new(rx)), found: None, getting: None, ..Default::default() };
        self.go(Screen::Manuals);
    }

    fn get_manual(&mut self, id: String) {
        let Some((dir, base)) = self.manual_place() else { return };
        if self.manual_search.getting.is_some() {
            return;
        }
        let progress = Arc::new(Mutex::new((0, 0)));
        let (said, (tx, rx)) = (progress.clone(), channel());
        std::thread::spawn(move || {
            let _ = tx.send(manuals::download(&Online, &id, &dir, &base, &|done, of| *said.lock().unwrap() = (done, of)));
        });
        self.manual_search.getting = Some((progress, Mutex::new(rx)));
        self.redraw = true;
    }

    /// The chosen game as the manual stores know it, when it's a console's game or a PC game on a console's shelf.
    pub(super) fn manual_game(&self) -> Option<manualcoll::Game> {
        let c = self.chosen.as_ref()?;
        let (dir, base) = self.manual_place()?;
        let system = systems().console(&c.console)?.name.clone();
        Some(manualcoll::Game { sys: c.console.clone(), system, title: c.game.title.clone(), title_id: c.game.title_id.clone(), file: c.game.file.to_string_lossy().into_owned(), region: c.game.region.clone(), dir, base })
    }

    fn manual_stores(&self) -> &(std::collections::HashMap<String, String>, std::collections::HashSet<String>) {
        self.manual_search.kept.get_or_init(|| {
            let stores = Stores::load(&paths::data_dir(&self.app_root));
            (systems().consoles.iter().map(|c| (c.id.clone(), stores.pref(&c.id))).collect(), stores.locked_keys())
        })
    }

    /// The row of a console's page that says which manuals its games get.
    pub(super) fn manual_pref_row(&self, console: &str) -> Row {
        let now = self.manual_stores().0.get(console).cloned().unwrap_or_else(|| "best".into());
        let at = Stores::PREFS.iter().position(|p| p.0 == now).unwrap_or(0);
        let id = console.to_string();
        let turn = move |s: &mut Settings, d: i32| {
            let next = Stores::PREFS[(at as i32 + d).rem_euclid(Stores::PREFS.len() as i32) as usize].0;
            if let Err(why) = Stores::load(&paths::data_dir(&s.app_root)).set_pref(&id, next) {
                s.note = why;
            }
            s.manual_search.kept = Default::default();
            s.redraw = true;
        };
        let again = turn.clone();
        Row {
            label: "Manuals".into(),
            value: Stores::PREFS[at].1.into(),
            hint: "Which manual the automatic search picks for this console's games: the sharpest complete English one, or one in another language or from another region.".into(),
            press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
            step: Some(Box::new(again)),
        }
    }

    /// Details' rows for a manual that's there: keep it as it is, or say it's another game's.
    pub(super) fn manual_rows(&self) -> Vec<Row> {
        let Some(g) = self.manual_game() else { return Vec::new() };
        if self.chosen.as_ref().is_some_and(|c| c.game.art.manual.is_empty() && c.game.art.manual_pdf.is_none()) {
            return Vec::new();
        }
        let (key, key2) = (g.key(), g.key());
        let locked = self.manual_stores().1.contains(&key);
        let flip = move |s: &mut Settings| {
            Stores::load(&paths::data_dir(&s.app_root)).set_lock(&key, !locked, true, now_ms());
            s.manual_search.kept = Default::default();
            s.redraw = true;
        };
        let flip2 = flip.clone();
        vec![
            Row {
                label: "Keep this manual".into(),
                value: on_off(locked),
                hint: "On: the automatic search, the scan collections and a new choice for the console leave this game's manual as it is.".into(),
                press: Some(Box::new(flip)),
                step: Some(Box::new(move |s: &mut Settings, _| flip2(s))),
            },
            Row {
                label: if self.manual_search.rejecting.is_some() { "Looking for another manual…" } else { "Wrong manual" }.into(),
                hint: "These pages are another game's: they're moved aside (to .manual-originals beside them, nothing is deleted), never brought again, and the next best manual takes their place.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if s.manual_search.rejecting.is_some() || !s.twice(&format!("wrong-{key2}"), "Enter again to set this manual aside as another game's.") {
                        return;
                    }
                    let Some(g) = s.manual_game() else { return };
                    let data = paths::data_dir(&s.app_root);
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(manuals::reject_manual(&Online, &data, &g, &now_ms));
                    });
                    s.manual_search.rejecting = Some(Mutex::new(rx));
                    s.redraw = true;
                })),
                ..Row::default()
            },
        ]
    }

    /// The chosen game's manual as its folder has it now: Details shows it at once.
    pub(super) fn reread_manual(&mut self) {
        let Some((dir, base)) = self.manual_place() else { return };
        let files = manualcoll::manual_files(&dir, &base);
        let Some(c) = self.chosen.as_mut() else { return };
        let mut pages: Vec<String> = files.iter().filter(|f| !f.to_lowercase().ends_with(".pdf")).cloned().collect();
        pages.sort_by(|a, b| rustyshelf_core::collate::compare(a, b));
        c.game.art.manual = pages.iter().map(|f| dir.join(f)).collect();
        c.game.art.manual_pdf = files.iter().find(|f| f.to_lowercase().ends_with(".pdf")).map(|f| dir.join(f));
    }

    /// What the search and the download came back with since the last frame.
    pub(super) fn poll_manuals(&mut self) {
        if let Some(done) = self.manual_search.rejecting.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            self.manual_search.rejecting = None;
            self.reread_manual();
            self.changed = true;
            self.say(match done {
                Ok(Rejected { got, from: Some(from), pdf, .. }) if got > 0 => format!("Set aside. Another manual took its place: {} from {from}.", if pdf { "a PDF".to_string() } else { format!("{got} pages") }),
                Ok(Rejected { offline: true, .. }) => "Set aside. The search for another couldn't be made; it's tried again on a later start.".into(),
                Ok(_) => "Set aside. No other manual was found for this game.".into(),
                Err(why) => why,
            });
        }
        if let Some(found) = self.manual_search.asking.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            (self.manual_search.asking, self.manual_search.found, self.redraw) = (None, Some(found), true);
        }
        let Some((_, done)) = &self.manual_search.getting else { return };
        let ended = done.lock().unwrap().try_recv();
        let on_page = self.stack.last().is_some_and(|p| p.0 == Screen::Manuals);
        let got = match ended {
            Ok(got) => got,
            Err(TryRecvError::Disconnected) => Err("the download stopped".into()),
            Err(TryRecvError::Empty) => {
                // the page follows the pages coming down
                self.redraw |= on_page;
                return;
            }
        };
        self.manual_search.getting = None;
        let place = self.manual_place();
        let same = self.chosen.as_ref().is_some_and(|c| c.game.id == self.manual_search.game);
        match got {
            Ok(Got::Pages(n)) if n > 0 => {
                // the shelf reads the pages when Settings closes; Details has them at once
                if let (Some((dir, base)), Some(c)) = (place, self.chosen.as_mut().filter(|_| same)) {
                    let prefix = format!("{base}-manual-").to_lowercase();
                    let mut pages: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|f| f.to_string_lossy().to_lowercase().starts_with(&prefix)) && !p.to_string_lossy().ends_with(".part")).collect();
                    pages.sort();
                    c.game.art.manual = pages;
                }
                self.changed = true;
                if on_page {
                    self.back();
                }
                self.say(format!("Manual saved: {n} pages. Read the manual opens it."));
            }
            Ok(Got::Pdf(pdf)) => {
                if let Some(c) = self.chosen.as_mut().filter(|_| same) {
                    c.game.art.manual_pdf = Some(pdf);
                }
                self.changed = true;
                if on_page {
                    self.back();
                }
                self.say("Manual saved as a PDF. Open the manual shows it in this PC's PDF reader.".into());
            }
            Ok(Got::Pages(_)) => self.say("That upload has no pages to show. Try another one.".into()),
            Err(why) => self.say(format!("The manual couldn't be downloaded ({why}). Try again, or another one.")),
        }
    }

    pub(super) fn manuals_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let search = &self.manual_search;
        let mut rows = Vec::new();
        let lead = match (&search.getting, &search.found) {
            (Some((progress, _)), _) => match *progress.lock().unwrap() {
                (_, 0) => "Downloading…".to_string(),
                (done, of) => format!("Downloading… page {done} of {of}"),
            },
            (None, None) => "Asking archive.org and GamesDatabase.org…".into(),
            (None, Some(Err(why))) => format!("The search couldn't be made ({why}). Try again in a while."),
            (None, Some(Ok(found))) if found.is_empty() => "Nothing found for this game.".into(),
            (None, Some(Ok(found))) => {
                for c in found {
                    let id = c.id.clone();
                    let pages = if c.pages > 0 { format!(" · {} pages", c.pages) } else { String::new() };
                    rows.push(Row {
                        label: c.title.clone(),
                        value: format!("{}{pages} · {}", c.language_name(), c.source),
                        hint: format!("Saves it as this game's manual, beside its box art.{}", if c.score >= manuals::AUTO_MIN { "" } else { " A loose match: it may be another game's, or not a manual." }),
                        press: Some(Box::new(move |s: &mut Settings| s.get_manual(id.clone()))),
                        step: None,
                    });
                }
                format!("{} found, the best match first.", found.len())
            }
        };
        Page { title: format!("Manuals · {title}"), lead, rows }
    }
}

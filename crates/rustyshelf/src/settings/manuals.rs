//! Details → Find a manual: the manuals archive.org and GamesDatabase.org have for the chosen game, listed to
//! pick from; the one picked is saved as pages beside the game's box art, where the reader finds them.

use std::sync::Arc;

use rustyshelf_core::art::{art_base, ArtLibrary};
use rustyshelf_core::manuals::{self, Candidate, Got, Online};

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
}

impl Settings {
    /// Where the chosen game's manual goes, and the name its art has.
    fn manual_place(&self) -> Option<(PathBuf, String)> {
        let c = self.chosen.as_ref()?;
        let config = Config::from_value(&self.cfg)?;
        let console = systems().console(&c.console)?;
        Some((ArtLibrary::from_config(&config, &self.app_root).manual_dir_for(&config.system(console), &c.game.file, &self.app_root), art_base(&c.game.file)))
    }

    /// Whether Details offers the search: a console's game with no manual yet.
    pub(super) fn can_find_manual(&self) -> bool {
        self.chosen.as_ref().is_some_and(|c| c.game.source == Source::Rom && c.game.art.manual.is_empty() && c.game.art.manual_pdf.is_none() && systems().console(&c.console).is_some())
    }

    pub(super) fn find_manual(&mut self) {
        let Some(c) = &self.chosen else { return };
        let (sys, title, id) = (c.console.clone(), c.game.title.clone(), c.game.id.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(manuals::search(&Online, &sys, &title).map_err(|e| e.words()));
        });
        self.manual_search = ManualSearch { game: id, asking: Some(Mutex::new(rx)), found: None, getting: None };
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

    /// What the search and the download came back with since the last frame.
    pub(super) fn poll_manuals(&mut self) {
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

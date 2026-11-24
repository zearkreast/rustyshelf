//! Details → Manual Studio: every manual found for the chosen game (the one in use, earlier ones set aside, the
//! console's collections, GamesDatabase, archive.org), listed as each is checked, the best for the console's choice
//! first; Enter puts one in place, and the manual before can be put back.

use rustyshelf_core::manuals::Online;
use rustyshelf_core::manualstudio::{self as ms, Cand, Db, Remote};
use rustyshelf_core::paths;

use super::*;

/// What the search sends as it goes: a manual that can be had, or the end (and whether the web couldn't be reached).
enum Found {
    One(Box<(Cand, Remote)>),
    Done(bool),
}

/// The Manual Studio's list for the chosen game, and a manual being put in place.
#[derive(Default)]
pub(super) struct MStudio {
    list: Vec<(Cand, Option<Remote>)>,
    searching: Option<Mutex<Receiver<Found>>>,
    offline: bool,
    pref: String,
    can_undo: bool,
    /// what's being done, and how it ends: the pages put in and whether only the first were kept
    working: Option<(String, Later<Result<(usize, bool), String>>)>,
}

fn lang_name(l: &str) -> &str {
    match l {
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "it" => "Italian",
        "ja" => "Japanese",
        "pt" => "Portuguese",
        "nl" => "Dutch",
        "" => "Language unknown",
        other => other,
    }
}

impl Settings {
    /// Details' row that opens the Manual Studio, for a console's game.
    pub(super) fn manual_studio_row(&self) -> Option<Row> {
        self.manual_game()?;
        Some(Row {
            label: "Manual Studio".into(),
            hint: "Every manual found for this game side by side, with its language, pages and scan quality, to choose from; the one it replaces is kept (its name is sent to archive.org and GamesDatabase.org)".into(),
            press: Some(Box::new(|s: &mut Settings| s.open_manual_studio())),
            ..Row::default()
        })
    }

    fn open_manual_studio(&mut self) {
        let Some(g) = self.manual_game() else { return };
        let data = paths::data_dir(&self.app_root);
        let db = Db::load(&data);
        let local: Vec<Cand> = ms::current_cand(&data, &db, &g).into_iter().chain(ms::earlier_cands(&g)).collect();
        let skip: Vec<String> = local.iter().filter_map(|c| c.src.clone()).collect();
        let (tx, rx) = channel();
        let game = g.clone();
        std::thread::spawn(move || {
            let (found, offline) = ms::search(&Online, &data, &game, &skip, rustyshelf_core::js::date_now());
            // three checked at a time
            let tx = Mutex::new(tx);
            rustyshelf_core::pool::each(&found, 3, |_, (c, r)| {
                let mut c = c.clone();
                !(ms::verify(&Online, &data, &mut c, r) && tx.lock().unwrap().send(Found::One(Box::new((c, r.clone())))).is_err())
            });
            let _ = tx.lock().unwrap().send(Found::Done(offline));
        });
        self.mstudio = MStudio { list: local.into_iter().map(|c| (c, None)).collect(), searching: Some(Mutex::new(rx)), pref: db.pref(&g.sys), can_undo: db.can_undo(&g.key()), ..Default::default() };
        self.go(Screen::ManualStudio);
    }

    fn manual_studio_work(&mut self, what: &str, work: impl FnOnce(&std::path::Path, &rustyshelf_core::manualcoll::Game) -> Result<(usize, bool), String> + Send + 'static) {
        let Some(g) = self.manual_game().filter(|_| self.mstudio.working.is_none()) else { return };
        let data = paths::data_dir(&self.app_root);
        self.mstudio.working = Some((what.to_string(), Later::spawn(move || work(&data, &g))));
        self.redraw = true;
    }

    fn use_manual(&mut self, at: usize) {
        let Some((c, r)) = self.mstudio.list.get(at).cloned() else { return };
        if c.in_use {
            self.say("That's the manual in use.".into());
            return;
        }
        self.manual_studio_work("Downloading and putting it in place…", move |data, g| ms::apply(&Online, data, g, &c, r.as_ref(), rustyshelf_core::js::date_now()));
    }

    /// Your own PDF as the chosen game's manual (from the file picker).
    pub(super) fn own_manual(&mut self, file: &std::path::Path) {
        let file = file.to_path_buf();
        self.manual_studio_work("Turning your PDF into pages…", move |data, g| ms::own(data, g, &file, rustyshelf_core::js::date_now()));
    }

    /// What the search and a manual being put in came to since the last frame.
    pub(super) fn poll_manual_studio(&mut self) {
        let mut ended = None;
        if let Some(rx) = &self.mstudio.searching {
            while let Ok(f) = rx.lock().unwrap().try_recv() {
                match f {
                    Found::One(x) => self.mstudio.list.push((x.0, Some(x.1))),
                    Found::Done(offline) => ended = Some(offline),
                }
                self.redraw = true;
            }
        }
        if let Some(offline) = ended {
            (self.mstudio.searching, self.mstudio.offline) = (None, offline);
        }
        let Some(done) = self.mstudio.working.as_ref().and_then(|w| w.1.poll()) else { return };
        self.mstudio.working = None;
        match done {
            Ok((n, cut)) => {
                self.reread_manual();
                self.changed = true;
                let note = if cut { " (only its first pages are kept)" } else { "" };
                self.say(format!("Manual in place: {n} pages{note}. The one it replaced is kept under Earlier."));
                if self.stack.last().is_some_and(|p| p.0 == Screen::ManualStudio) {
                    self.back();
                    self.open_manual_studio();
                }
            }
            Err(why) => self.say(why),
        }
    }

    pub(super) fn manual_studio_page(&self) -> Page {
        let st = &self.mstudio;
        let title = format!("Manual Studio · {}", self.chosen.as_ref().map_or("", |c| c.game.title.as_str()));
        let cands: Vec<Cand> = st.list.iter().map(|x| x.0.clone()).collect();
        let expect = ms::expect_of(&cands);
        // the one in use first, then the best for the console's choice
        let mut order: Vec<usize> = (0..st.list.len()).collect();
        order.sort_by(|a, b| {
            let (x, y) = (&st.list[*a].0, &st.list[*b].0);
            y.in_use.cmp(&x.in_use).then(ms::rank(y, &st.pref, &expect).total_cmp(&ms::rank(x, &st.pref, &expect)))
        });
        let mut rows: Vec<Row> = order
            .into_iter()
            .map(|i| {
                let c = &st.list[i].0;
                let (_, quality) = ms::quality(c.w, c.bpp);
                let region = match c.region.as_str() {
                    "usa" => "USA",
                    "eur" => "Europe",
                    "jpn" => "Japan",
                    _ => "",
                };
                let pages = c.pages.map_or(String::new(), |n| format!("{n} pages"));
                let value = [if c.in_use { "In use" } else { "" }, lang_name(&c.lang), region, &pages, &quality].into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" · ");
                let note = ms::complete_text(c, &expect);
                let fits = if ms::fits(c, &st.pref) { "" } else { " Not what this console's manuals are set to." };
                Row {
                    label: c.label.clone(),
                    value,
                    hint: if c.in_use { format!("The manual in use.{}", if note.is_empty() { String::new() } else { format!(" {note}.") }) } else { format!("Enter puts this one in place; the one it replaces is kept under Earlier.{}{fits}", if note.is_empty() { String::new() } else { format!(" {note}.") }) },
                    press: Some(Box::new(move |s: &mut Settings| s.use_manual(i))),
                    step: None,
                }
            })
            .collect();
        if st.working.is_none() {
            if st.can_undo {
                rows.push(Row {
                    label: "Undo".into(),
                    hint: "Puts the manual before back; the one in use goes under Earlier.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.manual_studio_work("Putting the manual before back…", |data, g| ms::undo(data, g).map(|()| (ms::pages_in(&g.dir, &g.base).len(), false))))),
                    ..Row::default()
                });
            }
            rows.push(Row {
                label: "Your own PDF…".into(),
                hint: "Choose a PDF of this game's manual: it's copied and turned into pages, the file stays where it is.".into(),
                press: Some(Box::new(|s: &mut Settings| s.pick(Pick::OwnManual, "Your own manual (PDF)", false))),
                ..Row::default()
            });
        }
        let lead = match (&st.working, &st.searching) {
            (Some((what, _)), _) => what.clone(),
            (None, Some(_)) => format!("Looking on archive.org, GamesDatabase.org and the console's collections… {} so far.", st.list.len()),
            (None, None) if st.offline && st.list.iter().all(|x| x.1.is_none()) => "The sites couldn't be reached: only the manuals on this computer are listed.".into(),
            (None, None) => format!("{} manuals, the best for this console's choice first.", st.list.len()),
        };
        Page { title, lead, rows }
    }
}

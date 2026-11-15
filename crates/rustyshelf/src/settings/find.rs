//! Find a game: a title typed, the matches under it as it's typed, and the shelf taken to the one chosen.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use rustyshelf_core::find::{self, Entry};

use super::*;

/// The search while Find a game is open: what's typed, and every game on the shelves.
#[derive(Default)]
pub(super) struct Finding {
    typed: String,
    games: Vec<Entry>,
    /// each game's shelf, by its id
    shelves: Vec<String>,
}

/// The longest title that's searched for.
const LONGEST: usize = 60;

impl Settings {
    /// Asks for Find a game, from the shelf.
    pub fn open_find(&mut self) {
        self.wanted = Some(Screen::Find);
    }

    /// Reads every game of the library for the search: each once, on its own shelf.
    pub(super) fn find_from(&mut self, library: &rustyshelf_core::library::Library, plays: &rustyshelf_core::history::History) {
        let mut found = Finding::default();
        let mut seen = std::collections::HashSet::new();
        let kid = super::kid_hidden(self.cfg.get("ui"));
        for shelf in library.shelves.iter().filter(|s| !kid.contains(&s.id)) {
            for g in shelf.games.iter().filter(|g| seen.insert(g.id.clone())) {
                let last = shelfview::played(g, plays).filter(|p| p.count > 0).map_or(0, |p| p.last);
                found.games.push(Entry::new(&g.id, &g.title, &shelf.name, last));
                found.shelves.push(shelf.id.clone());
            }
        }
        self.finding = found;
    }

    pub(super) fn find_page(&self) -> Page {
        let f = &self.finding;
        let (found, total) = find::search(&f.games, &f.typed);
        let typed = f.typed.clone();
        let mut rows = vec![Row {
            label: "Title".into(),
            value: if f.typed.is_empty() { "Type a title".into() } else { format!("{}_", f.typed) },
            hint: "Type on the keyboard, or press A for the keys on screen. Up and down choose a game, Enter goes to it.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                s.edit("Title", "The game's title, or a part of it.", typed.clone(), Box::new(|s: &mut Settings, v: String| s.find_set(v)));
            })),
            step: None,
        }];
        for i in found.iter().copied() {
            let (shelf, id) = (f.shelves[i].clone(), f.games[i].id.clone());
            rows.push(Row {
                label: f.games[i].title.clone(),
                value: f.games[i].shelf.clone(),
                hint: String::new(),
                press: Some(Box::new(move |s: &mut Settings| {
                    s.go_to = Some((shelf.clone(), id.clone()));
                    s.stack.truncate(1);
                    s.back();
                })),
                step: None,
            });
        }
        let all = f.games.len();
        let lead = match (f.typed.trim().is_empty(), total) {
            (true, 0) => format!("{all} game{} on your shelves.", if all == 1 { "" } else { "s" }),
            (true, _) => format!("{all} game{} on your shelves. Played last:", if all == 1 { "" } else { "s" }),
            (false, 0) => "No game with that in its title.".into(),
            (false, n) if n > found.len() => format!("The first {} of {n} games found.", found.len()),
            (false, n) => format!("{n} game{} found.", if n == 1 { "" } else { "s" }),
        };
        Page { title: "Find a game".into(), lead, rows }
    }

    /// What's typed becomes `typed`, the best match under the cursor.
    fn find_set(&mut self, typed: String) {
        self.finding.typed = typed.chars().take(LONGEST).collect();
        let any = !find::search(&self.finding.games, &self.finding.typed).0.is_empty();
        if let Some(page) = self.stack.last_mut() {
            page.1 = usize::from(any);
        }
        self.redraw = true;
    }

    /// The keys typed this frame go into the title; true when one did, so the page doesn't take it as its own
    /// (Space for choosing, Backspace for going back).
    pub(super) fn find_type(&mut self, typed: &[KeyboardInput], ctrl: bool, shift: bool) -> bool {
        let mut title = self.finding.typed.clone();
        let mut took = false;
        for k in typed.iter().filter(|k| k.state == ButtonState::Pressed) {
            match &k.logical_key {
                Key::Backspace if !title.is_empty() => {
                    title.pop();
                    took = true;
                }
                Key::Enter | Key::Escape | Key::Backspace | Key::Tab => {}
                _ if super::text::is_paste(k, ctrl, shift) => {
                    let text = crate::osk::clipboard().unwrap_or_default();
                    took |= !text.is_empty();
                    title.push_str(&text);
                }
                _ if ctrl => {}
                _ => {
                    let letters: String = k.text.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control()).collect();
                    took |= !letters.is_empty();
                    title.push_str(&letters);
                }
            }
        }
        if took {
            self.find_set(title);
        }
        took
    }
}

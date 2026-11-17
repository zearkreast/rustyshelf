//! RetroAchievements on the shelf: each game's progress beside its facts, and what was unlocked when a game is
//! come back from.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::i18n::plural_s;
use rustyshelf_core::ra;
use rustyshelf_core::scanner::Source;
use serde_json::Value;

use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

enum Answer {
    Shelf(String, Result<Value, String>),
    Unlocked(Result<Vec<Value>, String>),
}

/// What's known of the account's progress: by shelf, each game's by its title.
#[derive(Resource, Default)]
pub struct Cheevos {
    shelves: HashMap<String, Value>,
    asking: Option<Mutex<Receiver<Answer>>>,
    account: (String, String),
    checked: f32,
    /// a game running: when it started; and when it ended, until what it unlocked has been asked for
    started: Option<f32>,
    ended: Option<(f32, f32)>,
}

impl Cheevos {
    /// A game's progress as its line under the title says it: "★ 12/30", or "★ Mastered".
    pub fn mark(&self, shelf: &str, title: &str) -> Option<String> {
        let p = self.shelves.get(shelf)?.get(title)?;
        Some(if p["mastered"] == "mastered" { "★ Mastered".into() } else { format!("★ {}/{}", p["earned"], p["total"]) })
    }
}

pub fn run(
    time: Res<Time>,
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    settings: Res<crate::settings::Settings>,
    playing: Res<crate::play::Playing>,
    mut toast: ResMut<crate::hud::Toast>,
    mut c: ResMut<Cheevos>,
) {
    let now = time.elapsed_secs();
    // (the key is kept sealed: opened once a second, not every frame)
    if now - c.checked >= 1.0 {
        c.checked = now;
        let account = settings.ra_account();
        if account != c.account {
            c.account = account;
            c.shelves.clear();
        }
    }
    if let Some(answer) = c.asking.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        c.asking = None;
        match answer {
            // (a shelf that couldn't be read is left empty: asked for again only with another account or start)
            Answer::Shelf(shelf, got) => {
                c.shelves.insert(shelf, got.unwrap_or(Value::Null));
            }
            Answer::Unlocked(Ok(list)) if !list.is_empty() => {
                let names: Vec<&str> = list.iter().filter_map(|a| a["title"].as_str()).take(3).collect();
                toast.show(format!("{} achievement{} unlocked: {}{}", list.len(), plural_s(list.len()), names.join(", "), if list.len() > names.len() { "…" } else { "" }), now, 8.0);
                // the shelves' marks are out of date now
                c.shelves.clear();
            }
            Answer::Unlocked(_) => {}
        }
    }
    // a game come back from: a few seconds later, what it unlocked
    match (playing.title.is_some(), c.started) {
        (true, None) => c.started = Some(now),
        (false, Some(since)) => {
            c.started = None;
            c.ended = Some((now, now - since));
        }
        _ => {}
    }
    if c.account.1.is_empty() || c.asking.is_some() {
        return;
    }
    let account = c.account.clone();
    let ask = |work: Box<dyn FnOnce(&ra::Ra) -> Answer + Send>| {
        let (site, (user, key)) = (settings.ra(), account.clone());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            site.set_credentials(&user, &key);
            let _ = tx.send(work(&site));
        });
        Mutex::new(rx)
    };
    if let Some((at, played)) = c.ended.filter(|e| now - e.0 >= 4.0) {
        c.ended = None;
        let minutes = f64::from((now - at + played) / 60.0);
        c.asking = Some(ask(Box::new(move |site| Answer::Unlocked(site.recent(minutes)))));
        return;
    }
    let LibraryState::Ready(l) = &lib.0 else { return };
    let Some(shelf) = l.shelves.get(cursor.shelf).filter(|s| ra::console_id(&s.id).is_some() && !c.shelves.contains_key(&s.id)) else { return };
    let (id, titles): (String, Vec<String>) = (shelf.id.clone(), shelf.games.iter().filter(|g| !matches!(g.source, Source::Tile(_))).map(|g| g.title.clone()).collect());
    c.asking = Some(ask(Box::new(move |site| { let got = site.shelf_progress(&id, &titles); Answer::Shelf(id, got) })));
}

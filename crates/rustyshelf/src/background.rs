// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): main.js's
// moreShotsBg and igdbInfoBg, as RustyShelf has them.
//! After a scan, one console game at a time, only while no game runs and nothing has been pressed for a while: more
//! screenshots for a game short of them (with box art fetching on; IGDB's alone without it), and IGDB's info for a
//! game whose details have gaps. What's found is kept on disk; the next scan shows it.

use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use rustyshelf_core::library::ShelfKind;
use rustyshelf_core::moreshots::{self, Source};
use rustyshelf_core::scanner::Source as From;
use rustyshelf_core::{igdb, paths, screenscraper};

use crate::loading::Base;
use crate::play::Playing;
use crate::settings::Settings;

/// seconds without a press before a game is looked up, and between two games
const QUIET: f32 = 8.0;
const GAP: f32 = 1.5;
/// ScreenScraper lookups a pass may make (the account's daily quota is shared with the box art)
const SS_LIMIT: usize = 150;

struct Item {
    sys: String,
    file: std::path::PathBuf,
    title: String,
    region: String,
    info: rustyshelf_core::info::Info,
}

/// What the pass works with: the sites (one client each, for their limits and tokens) and what's set up.
struct Pass {
    ss: Option<Arc<screenscraper::ScreenScraper>>,
    igdb: Option<Arc<igdb::Igdb>>,
    /// box art fetching is on: every source; off, IGDB's alone
    auto: bool,
    shots: bool,
    art_region: String,
}

#[derive(Resource, Default)]
pub struct Background {
    queue: Vec<Item>,
    pass: Option<Arc<Pass>>,
    working: Option<Mutex<Receiver<bool>>>,
    ss_used: usize,
    next_at: f32,
    looked: usize,
}

/// Starts a pass when a scan has come in; looks one game up at a time while it's quiet.
pub fn run(time: Res<Time>, base: Res<Base>, settings: Res<Settings>, playing: Res<Playing>, keys: Res<ButtonInput<KeyCode>>, pads: Query<&Gamepad>, mut bg: ResMut<Background>, mut quiet_since: Local<f32>) {
    let now = time.elapsed_secs();
    if keys.get_just_pressed().next().is_some() || pads.iter().any(|p| p.get_just_pressed().next().is_some()) {
        *quiet_since = now;
    }
    if base.is_changed() {
        if let Some(lib) = &base.0 {
            start(&mut bg, lib, &settings);
        }
    }
    if let Some(rx) = &bg.working {
        match rx.lock().ok().and_then(|r| r.try_recv().ok()) {
            Some(asked_ss) => {
                bg.working = None;
                bg.next_at = now + GAP;
                bg.ss_used += usize::from(asked_ss);
            }
            None => return,
        }
    }
    if bg.queue.is_empty() || now < bg.next_at || now - *quiet_since < QUIET || playing.title.is_some() || settings.open {
        if bg.queue.is_empty() && bg.looked > 0 {
            info!("More screenshots and game info: looked for {} games", std::mem::take(&mut bg.looked));
        }
        return;
    }
    let (Some(item), Some(pass)) = (bg.queue.pop(), bg.pass.clone()) else { return };
    let ss_left = bg.ss_used < SS_LIMIT;
    let (tx, rx) = channel();
    bg.working = Some(Mutex::new(rx));
    bg.looked += 1;
    std::thread::spawn(move || {
        let _ = tx.send(look(&pass, &item, ss_left));
    });
}

fn start(bg: &mut Background, lib: &rustyshelf_core::library::Library, settings: &Settings) {
    let data = paths::data_dir(&paths::app_root());
    let (auto, shots) = (settings.auto_covers(), settings.more_shots());
    let ig = igdb::Igdb::from_settings(&data).map(Arc::new);
    bg.queue.clear();
    if !(auto && shots) && ig.is_none() {
        return;
    }
    let ss = if auto { screenscraper::ScreenScraper::from_settings(&data).map(Arc::new) } else { None };
    bg.pass = Some(Arc::new(Pass { ss, igdb: ig, auto, shots, art_region: settings.art_region() }));
    for shelf in lib.shelves.iter().filter(|s| matches!(s.kind, ShelfKind::Console { .. })) {
        for g in shelf.games.iter().filter(|g| g.source == From::Rom && !g.file.as_os_str().is_empty()) {
            let info = g.info.clone().unwrap_or_default();
            bg.queue.push(Item { sys: shelf.id.clone(), file: g.file.clone(), title: g.title.clone(), region: g.region.clone(), info });
        }
    }
    // taken from the end: the shelves' own order first
    bg.queue.reverse();
}

/// One game: its screenshots topped up, then IGDB's info for its gaps. Whether ScreenScraper was asked.
fn look(pass: &Pass, item: &Item, ss_left: bool) -> bool {
    let data = paths::data_dir(&paths::app_root());
    let cache = paths::boxart_cache(&data);
    let lr = moreshots::Libretro { cache_dir: &cache, region: &pass.art_region };
    let ss = pass.ss.as_deref().filter(|_| pass.auto && ss_left).map(moreshots::Ss);
    let ig = pass.igdb.as_deref().map(|client| moreshots::Igdb { client, data_dir: &data });
    let mut sources: Vec<&dyn Source> = Vec::new();
    if pass.auto {
        sources.extend(ss.as_ref().map(|s| s as &dyn Source));
        sources.push(&lr);
    }
    sources.extend(ig.as_ref().map(|s| s as &dyn Source));
    let mut asked_ss = false;
    if pass.shots {
        let mut index = moreshots::Index::load(&data);
        if index.need(&item.sys, &item.file, &item.title, &item.info.shots, &sources) {
            asked_ss = ss.is_some();
            let game = moreshots::Game { file: &item.file, title: &item.title, region: &item.region };
            moreshots::top_up(&mut index, &item.sys, &game, &item.info.shots, &sources, &|_| None);
        }
    }
    if let Some(client) = pass.igdb.as_deref().filter(|c| !c.is_refused() && igdb::gaps(Some(&item.info))) {
        let mut info = item.info.clone();
        igdb::lookup_info(Some(client), &data, &item.sys, &item.file, &item.title, &mut info);
    }
    asked_ss
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the PC cover
// picker of Boxshelf's renderer/app.js (pcCoverOpen) and renderer/style.css (.coverpick), as RustyShelf has it.
//! Choosing a Steam, store or added game's cover: a card over the shelf with a grid of covers — Steam's own, the
//! Steam store's games of that name, SteamGridDB's posters (with a key) — and tiles to search under another name,
//! use a picture of your own, or go back to the automatic cover. The arrows move through the grid, Enter / A uses a
//! tile, Esc / B closes. The covers are looked up, their thumbnails fetched and a chosen one downloaded off the
//! frame.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::pccovers::{self, CoverOption};

use rustyshelf_core::library::Library;
use rustyshelf_core::scanner::{Game, Source};

use crate::hud::MonaSans;
use crate::loading::to_image;
use crate::motion::{self, Ease, OUT};

/// The game a picker is asked for: its id, a Steam game's appid, its title, and a title typed in to search under.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    pub id: String,
    pub appid: Option<String>,
    pub title: String,
    pub term: String,
}

/// A cover tool asked for from Settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Job {
    /// every PC game's cover, to pick one to change
    Gallery,
    /// a picker for each custom game without a cover
    OneByOne,
    /// SteamGridDB's top cover for each custom game without one
    Best,
    /// those covers taken away again
    Undo,
}

/// What Settings → Custom games → Covers shows; `missing` is None until the library is read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub missing: Option<usize>,
    pub undo: usize,
    pub busy: bool,
}

/// tiles to a row
const COLS: usize = 7;
/// the thumbnails' height, in pixels
const THUMB_H: u32 = 300;

enum Act {
    Search,
    Own,
    Reset,
    SteamGrid,
    Skip,
    Cover(Box<CoverOption>),
    /// a game of the gallery: its picker
    Game(Ask),
}

struct Item {
    act: Act,
    label: String,
    sub: String,
    icon: &'static str,
    thumb: Option<Handle<Image>>,
    /// a game of the gallery with no cover at all
    made: bool,
}

type Decoded = Option<(u32, u32, Vec<u8>)>;

/// What a worker sends back: the covers found (with their thumbnails decoded), or a cover put in place.
type Found = (pccovers::Options, Vec<Decoded>);

struct Open {
    ask: Ask,
    title: String,
    /// the line under it: the game, or the gallery's count
    game: String,
    gallery: bool,
    items: Vec<Item>,
    at: usize,
    note: String,
    finding: Option<Mutex<Receiver<Found>>>,
    /// the gallery's covers, decoded
    fronts: Option<Mutex<Receiver<Vec<Decoded>>>>,
    using: Option<Mutex<Receiver<Result<PathBuf, String>>>>,
    built: bool,
    since: f32,
}

/// The custom games, one picker after another.
struct Batch {
    list: Vec<(String, String)>,
    i: usize,
    done: usize,
}

/// How a picker was left.
enum After {
    Back,
    /// the game is settled: a cover used (`counted`), or left as it was
    Settled { counted: bool },
}

enum Best {
    NoKey,
    /// the covers that came in (game id, file) and how many games were looked up
    Done(Vec<(String, PathBuf)>, usize),
}

#[derive(Resource, Default)]
pub struct CoverPick {
    open: Option<Open>,
    /// a cover changed: the library is read again (as soon as it isn't being read already)
    reload: bool,
    /// the grid's scroll, eased to keep the lit tile in view
    scroll: Ease<f32>,
    batch: Option<Batch>,
    /// a picker opened from the gallery: the gallery comes back when it closes
    gallery_back: bool,
    /// the gallery's coming back once the library has the new cover (since when)
    gallery_wait: Option<f32>,
    after: Option<After>,
    best: Option<Mutex<Receiver<Best>>>,
    /// the games "best for each" gave a cover, for Undo
    undo: Vec<String>,
    missing: Option<usize>,
    recount: bool,
}

impl CoverPick {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

#[derive(Component)]
pub struct CoverRoot;

#[derive(Component)]
pub struct CoverGrid;

#[derive(Component)]
pub struct LitTile;

/// The card, coming in.
#[derive(Component)]
pub struct CardIn;

/// seconds: the card coming in (style.css .buildpick, .bp-card)
const IN: f32 = 0.3;

fn decode(file: &std::path::Path) -> Decoded {
    let img = image::ImageReader::open(file).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.height() > THUMB_H { img.resize(u32::MAX, THUMB_H, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// Pictures decoded a few at a time.
fn decode_all(files: &[Option<PathBuf>]) -> Vec<Decoded> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let got: Vec<Mutex<Decoded>> = files.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(f) = files.get(i) else { return };
                let d = f.as_deref().and_then(decode);
                if let Ok(mut g) = got[i].lock() {
                    *g = d;
                }
            });
        }
    });
    got.into_iter().map(|g| g.into_inner().ok().flatten()).collect()
}

fn opening(ask: Ask, title: String, game: String, gallery: bool, note: &str, now: f32) -> Open {
    Open { ask, title, game, gallery, items: Vec::new(), at: 0, note: note.into(), finding: None, fronts: None, using: None, built: false, since: now }
}

fn start(ask: Ask, batch: Option<&Batch>, data: PathBuf, now: f32) -> Open {
    let (tx, rx) = channel();
    let job = ask.clone();
    std::thread::spawn(move || {
        let sg = rustyshelf_core::steamgrid::SteamGrid::from_settings(&data);
        let region = rustyshelf_core::config::Config::load(&data).map(|c| c.art_region().to_string()).unwrap_or_default();
        let found = pccovers::options(&data, &job.id, job.appid.as_deref(), if job.term.is_empty() { &job.title } else { &job.term }, !job.term.is_empty(), &region, sg.as_ref());
        let t: Vec<Option<PathBuf>> = found.list.iter().map(|c| Some(c.thumb.clone())).collect();
        let t = decode_all(&t);
        let _ = tx.send((found, t));
    });
    let title = match batch {
        Some(b) => format!("Choose a cover · {} of {}", b.i + 1, b.list.len()),
        None => "Choose a cover".into(),
    };
    let game = ask.title.clone();
    Open { finding: Some(Mutex::new(rx)), ..opening(ask, title, game, false, "Looking for covers…", now) }
}

/// The name each store goes by (app.js PC_SRC).
fn store_name(g: &Game) -> &'static str {
    match &g.source {
        Source::Pc(p) => match p.source {
            "epic" => "Epic Games",
            "folder" => "PC games folder",
            "quiver" => "Quiver",
            "custom" => "Custom",
            "gog" => "GOG",
            "ubisoft" => "Ubisoft",
            "ea" => "EA",
            "xbox" => "Xbox",
            "hoyoplay" => "HoYoPlay",
            _ => "PC",
        },
        _ => "Steam",
    }
}

/// Where a PC game's cover comes from, as the gallery says it, and its front.
fn cover_of(data: &std::path::Path, g: &Game) -> (&'static str, Option<PathBuf>) {
    let chosen = pccovers::cover(data, &g.id);
    let front = chosen.clone().or_else(|| g.art.front.clone().filter(|f| f.is_file()));
    let from = if g.box_plat.is_some() {
        "Its console box"
    } else if chosen.is_some() {
        "Your pick"
    } else if pccovers::auto_cover(data, &g.id).is_some() {
        if matches!(&g.source, Source::Pc(p) if p.source == "epic") { "Epic's box art" } else { "The store's box art" }
    } else if matches!(&g.source, Source::Pc(p) if !p.exe.is_empty() && pccovers::folder_cover(&p.exe).is_some()) {
        "A picture in its folder"
    } else if front.is_some() {
        "Steam's art"
    } else {
        "Made-up case"
    };
    (from, front)
}

fn ask_for(g: &Game) -> Ask {
    let appid = match &g.source {
        Source::Steam { appid, .. } => Some(appid.clone()),
        _ => None,
    };
    Ask { id: g.id.clone(), appid, title: g.title.clone(), term: String::new() }
}

/// Every PC game's cover as the shelf shows it, and where it comes from (app.js pcGallery).
fn gallery(lib: &Library, data: &std::path::Path, now: f32) -> Option<Open> {
    let mut games: Vec<&Game> = lib.shelves.iter().filter(|s| matches!(s.id.as_str(), "pc" | "custom" | "ports")).flat_map(|s| &s.games).filter(|g| matches!(g.source, Source::Steam { .. } | Source::Pc(_))).collect();
    if games.is_empty() {
        return None;
    }
    games.sort_by_cached_key(|g| g.title.to_lowercase());
    let mut fronts = Vec::new();
    let items: Vec<Item> = games
        .iter()
        .map(|g| {
            let (from, front) = cover_of(data, g);
            let made = front.is_none();
            fronts.push(front);
            Item { act: Act::Game(ask_for(g)), label: g.title.clone(), sub: format!("{} · {from}", store_name(g)), icon: "", thumb: None, made }
        })
        .collect();
    let made = items.iter().filter(|i| i.made).count();
    let n = items.len();
    let count = format!("{n} game{}{}", if n == 1 { "" } else { "s" }, if made > 0 { format!(" · {made} with a made-up case") } else { String::new() });
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(decode_all(&fronts));
    });
    let mut open = opening(Ask::default(), "Box art · PC games".into(), count, true, "Each game's cover as the shelf shows it, and where it comes from. Pick one to change it.", now);
    (open.items, open.fronts, open.built) = (items, Some(Mutex::new(rx)), false);
    Some(open)
}

/// The custom games (added by hand, or Quiver's) without a cover chosen: they never get one by themselves.
fn missing(lib: &Library, data: &std::path::Path) -> Vec<(String, String)> {
    let mut seen = std::collections::HashSet::new();
    lib.shelves
        .iter()
        .flat_map(|s| &s.games)
        .filter(|g| matches!(&g.source, Source::Pc(p) if matches!(p.source, "custom" | "quiver")))
        .filter(|g| seen.insert(g.id.clone()) && pccovers::cover(data, &g.id).is_none())
        .map(|g| (g.id.clone(), g.title.clone()))
        .collect()
}

/// What the note says once the covers are in (app.js pcCoverOpen).
fn note_for(found: &pccovers::Options, term: &str) -> String {
    let n = found.list.iter().filter(|c| c.source == "steamgriddb").count();
    let ns = found.list.iter().filter(|c| matches!(c.source, "steamstore" | "steam")).count();
    let s = |k: usize| if k == 1 { "" } else { "s" };
    if n > 0 {
        format!("{n} cover{} from SteamGridDB{}{}.", s(n), if term.is_empty() { String::new() } else { format!(" for \"{term}\"") }, if ns > 0 { ", and Steam's own" } else { "" })
    } else if !term.is_empty() {
        if ns > 0 {
            format!("Steam's cover{} for \"{term}\".{}", s(ns), if found.steamgriddb { " SteamGridDB has none." } else { "" })
        } else {
            format!("Nothing found for \"{term}\". Try another name.")
        }
    } else if found.steamgriddb {
        if ns > 0 { "Steam's own cover. SteamGridDB has none for this game." } else { "SteamGridDB has no covers for this game. Search by name, or use a picture of yours." }.into()
    } else if ns > 0 {
        "Steam's own cover. For dozens more, add a free SteamGridDB key.".into()
    } else {
        "No cover under this name on Steam. Search by name, use a picture of yours, or add a free SteamGridDB key for dozens more.".into()
    }
}

fn covers_count(n: usize) -> String {
    format!("New covers for {n} game{}", if n == 1 { "" } else { "s" })
}

/// A line for the player: on Settings' page while it's open, else over the shelf.
fn say(settings: &mut crate::settings::Settings, toast: &mut crate::hud::Toast, text: String, now: f32) {
    if settings.open {
        settings.say(text);
    } else {
        (toast.text, toast.until) = (text, now + 5.0);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut pick: ResMut<CoverPick>,
    mut settings: ResMut<crate::settings::Settings>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut toast: ResMut<crate::hud::Toast>,
    mut covers: ResMut<crate::shelf::Covers>,
    mut images: ResMut<Assets<Image>>,
    (mona, face): (Res<MonaSans>, Res<crate::hud::Face>),
    roots: Query<Entity, With<CoverRoot>>,
    (mut refresh, base): (ResMut<crate::loading::Refresh>, Res<crate::loading::Base>),
) {
    let now = time.elapsed_secs();
    let root = rustyshelf_core::paths::app_root();
    let data = rustyshelf_core::paths::data_dir(&root);
    let pick = &mut *pick;
    if pick.reload && crate::loading::read_again(&mut refresh, &root) {
        (pick.reload, pick.recount) = (false, true);
    }
    let lib = base.0.as_deref();
    if base.is_changed() || pick.recount {
        pick.recount = false;
        pick.missing = lib.map(|l| missing(l, &data).len());
    }
    settings.set_cover_state(State { missing: pick.missing, undo: pick.undo.len(), busy: pick.best.is_some() });
    if let Some(done) = pick.best.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        pick.best = None;
        sounds.play("toast", 1);
        let text = match done {
            Best::NoKey => "Add a free SteamGridDB key first: Settings → Accounts & services".to_string(),
            Best::Done(got, _) if got.is_empty() => "SteamGridDB had no covers for these games".into(),
            Best::Done(got, of) => {
                for (_, f) in &got {
                    covers.forget(f);
                }
                pick.undo = got.into_iter().map(|g| g.0).collect();
                pick.reload = true;
                format!("New covers for {} of {of} game{}. Don't like them? Settings → Custom games → Undo", pick.undo.len(), if of == 1 { "" } else { "s" })
            }
        };
        say(&mut settings, &mut toast, text, now);
    }
    if let Some(job) = settings.cover_job.take() {
        let list = lib.map(|l| missing(l, &data)).unwrap_or_default();
        match job {
            Job::Gallery => match lib.and_then(|l| gallery(l, &data, now)) {
                Some(g) => {
                    sounds.play("flip", 1);
                    pick.open = Some(g);
                    pick.scroll.snap(0.0, now);
                }
                None => say(&mut settings, &mut toast, "No PC games on the shelves yet".into(), now),
            },
            _ if matches!(job, Job::OneByOne | Job::Best) && list.is_empty() => say(&mut settings, &mut toast, "Every custom game has a cover already".into(), now),
            Job::OneByOne => {
                let b = Batch { list, i: 0, done: 0 };
                let ask = Ask { id: b.list[0].0.clone(), title: b.list[0].1.clone(), ..Ask::default() };
                sounds.play("flip", 1);
                pick.open = Some(start(ask, Some(&b), data.clone(), now));
                pick.scroll.snap(0.0, now);
                pick.batch = Some(b);
            }
            Job::Best if pick.best.is_none() => {
                let n = list.len();
                say(&mut settings, &mut toast, format!("Finding covers for {n} game{}…", if n == 1 { "" } else { "s" }), now);
                let (tx, rx) = channel();
                let data = data.clone();
                std::thread::spawn(move || {
                    let Some(sg) = rustyshelf_core::steamgrid::SteamGrid::from_settings(&data) else {
                        let _ = tx.send(Best::NoKey);
                        return;
                    };
                    let got = list.iter().filter_map(|(id, title)| Some((id.clone(), pccovers::use_best(&data, id, title, &sg)?))).collect();
                    let _ = tx.send(Best::Done(got, list.len()));
                });
                pick.best = Some(Mutex::new(rx));
            }
            Job::Best => {}
            Job::Undo => {
                let gone = std::mem::take(&mut pick.undo);
                for id in &gone {
                    if let Some(f) = pccovers::cover(&data, id) {
                        covers.forget(&f);
                    }
                    pccovers::clear(&data, id);
                }
                if !gone.is_empty() {
                    sounds.play("toast", 1);
                    say(&mut settings, &mut toast, format!("{} cover{} put back", gone.len(), if gone.len() == 1 { "" } else { "s" }), now);
                    pick.reload = true;
                }
            }
        }
    }
    // a picker left: on to the next custom game, or back to the gallery
    if let Some(after) = pick.after.take() {
        if let Some(mut b) = pick.batch.take() {
            match after {
                After::Back => {
                    if b.done > 0 {
                        say(&mut settings, &mut toast, covers_count(b.done), now);
                    }
                }
                After::Settled { counted } => {
                    (b.i, b.done) = (b.i + 1, b.done + usize::from(counted));
                    match b.list.get(b.i) {
                        Some((id, title)) => {
                            let ask = Ask { id: id.clone(), title: title.clone(), ..Ask::default() };
                            pick.open = Some(start(ask, Some(&b), data.clone(), now));
                            pick.scroll.snap(0.0, now);
                            pick.batch = Some(b);
                        }
                        None => say(&mut settings, &mut toast, if b.done > 0 { covers_count(b.done) } else { "No covers changed".into() }, now),
                    }
                }
            }
        } else if std::mem::take(&mut pick.gallery_back) {
            pick.gallery_wait = Some(now);
        }
    }
    // the gallery again, with the new cover once the library has it (a long read behind the shelf isn't waited for)
    if pick.gallery_wait.is_some_and(|t| pick.open.is_none() && !pick.reload && (!refresh.busy() || now - t > 2.0)) {
        pick.gallery_wait = None;
        if let Some(g) = lib.and_then(|l| gallery(l, &data, now)) {
            pick.open = Some(g);
            pick.scroll.snap(0.0, now);
        }
    }
    if let Some(ask) = settings.cover_asked.take() {
        sounds.play("flip", 1);
        pick.open = Some(start(ask, None, data.clone(), now));
        pick.scroll.snap(0.0, now);
    }
    let Some(open) = pick.open.as_mut() else {
        for r in &roots {
            commands.entity(r).despawn();
        }
        return;
    };
    settings.keys_taken = true;
    let in_batch = pick.batch.as_ref().map(|b| b.i + 1 < b.list.len());
    // the covers found
    if let Some(found) = open.finding.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        let (found, decoded) = found;
        open.finding = None;
        open.note = note_for(&found, &open.ask.term);
        let tile = |act, label: &str, sub: String, icon| Item { act, label: label.into(), sub, icon, thumb: None, made: false };
        let mut items = Vec::new();
        match in_batch {
            Some(more) => items.push(tile(Act::Skip, "Skip this game", if more { "On to the next" } else { "The last one" }.into(), "→")),
            None => items.push(tile(Act::Search, "Search by name…", if open.ask.term.is_empty() { "Another name to look up".into() } else { format!("Now: {}", open.ask.term) }, "Aa")),
        }
        // (Android picks files through its own screens, which this doesn't reach yet)
        if cfg!(not(target_os = "android")) {
            items.push(tile(Act::Own, "A picture of yours", "JPG, PNG or WebP".into(), "+"));
        }
        if found.chosen {
            items.push(tile(Act::Reset, "Automatic cover", "Go back to it".into(), "↺"));
        }
        let first_cover = items.len() + usize::from(found.list.first().is_some_and(|c| c.source == "yours"));
        for (c, d) in found.list.into_iter().zip(decoded) {
            let thumb = d.map(|(w, h, data)| images.add(to_image(w, h, data)));
            items.push(Item { label: c.label.clone(), sub: c.sub.clone(), icon: "", thumb, act: Act::Cover(Box::new(c)), made: false });
        }
        if !found.steamgriddb && in_batch.is_none() {
            items.push(tile(Act::SteamGrid, "Add SteamGridDB", "Dozens more covers · free key".into(), "+"));
        }
        open.at = if first_cover < items.len() && matches!(items[first_cover].act, Act::Cover(_)) { first_cover } else { 0 };
        open.items = items;
        open.built = false;
    }
    // the gallery's covers
    if let Some(decoded) = open.fronts.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        open.fronts = None;
        for (it, d) in open.items.iter_mut().zip(decoded) {
            it.thumb = d.map(|(w, h, data)| images.add(to_image(w, h, data)));
        }
        open.built = false;
    }
    // a cover put in place
    if let Some(done) = open.using.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        open.using = None;
        match done {
            Ok(path) => {
                covers.forget(&path);
                sounds.play("toast", 1);
                if pick.batch.is_none() {
                    (toast.text, toast.until) = (format!("New cover for {}", open.ask.title), now + 4.0);
                }
                (pick.reload, pick.open, pick.after) = (true, None, Some(After::Settled { counted: true }));
                return;
            }
            // (the file dialog closed without a picture)
            Err(e) if e.is_empty() => open.built = false,
            Err(e) => {
                sounds.play("error", 1);
                (open.note, open.built) = (e, false);
            }
        }
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let (ok, back) = settings.confirm();
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(back) {
        sounds.play("back", 1);
        if open.gallery {
            pick.gallery_back = false;
        } else {
            pick.after = Some(After::Back);
        }
        pick.open = None;
        return;
    }
    let n = open.items.len();
    let dx = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
    let dy = (keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32;
    if (dx != 0 || dy != 0) && n > 0 && open.using.is_none() {
        let to = (open.at as i32 + dx + dy * COLS as i32).clamp(0, n as i32 - 1) as usize;
        if to != open.at {
            open.at = to;
            open.built = false;
            sounds.play("tick", dx + dy);
        }
    }
    if (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(ok)) && open.using.is_none() && open.finding.is_none() {
        if let Some(item) = open.items.get(open.at) {
            let ask = open.ask.clone();
            match &item.act {
                Act::Game(g) => {
                    sounds.play("open", 1);
                    let g = g.clone();
                    pick.gallery_back = true;
                    pick.open = Some(start(g, None, data.clone(), now));
                    pick.scroll.snap(0.0, now);
                    return;
                }
                Act::Skip => {
                    sounds.play("tick", 1);
                    (pick.open, pick.after) = (None, Some(After::Settled { counted: false }));
                    return;
                }
                Act::Search => {
                    sounds.play("panel", 1);
                    settings.search_cover(ask);
                    (pick.open, pick.gallery_back) = (None, false);
                    return;
                }
                Act::SteamGrid => {
                    sounds.play("panel", 1);
                    settings.open_account();
                    (pick.open, pick.gallery_back) = (None, false);
                    return;
                }
                Act::Own => {
                    // (the file dialog waits for the player: off the frame, as a cover being downloaded is)
                    let (tx, rx) = channel();
                    let (id, data) = (ask.id.clone(), data.clone());
                    std::thread::spawn(move || {
                        let got = match crate::dialog::pick_file("Choose a cover", false) {
                            Some(file) => pccovers::own(&data, &id, &file),
                            None => Err(String::new()),
                        };
                        let _ = tx.send(got);
                    });
                    open.using = Some(Mutex::new(rx));
                }
                Act::Reset => {
                    let gone = pccovers::cover(&data, &ask.id);
                    pccovers::clear(&data, &ask.id);
                    if let Some(g) = gone {
                        covers.forget(&g);
                    }
                    sounds.play("toast", 1);
                    if pick.batch.is_none() {
                        (toast.text, toast.until) = (format!("{} is back on its automatic cover", ask.title), now + 4.0);
                    }
                    (pick.reload, pick.open, pick.after) = (true, None, Some(After::Settled { counted: false }));
                    return;
                }
                // the cover chosen before: kept as it is
                Act::Cover(c) if c.source == "yours" => {
                    (pick.open, pick.after) = (None, Some(After::Settled { counted: false }));
                    return;
                }
                Act::Cover(c) => {
                    let (tx, rx) = channel();
                    let (c, id, data) = ((**c).clone(), ask.id.clone(), data.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(pccovers::choose(&data, &id, &c));
                    });
                    open.using = Some(Mutex::new(rx));
                    (open.note, open.built) = ("Downloading the cover…".into(), false);
                }
            }
        }
    }
    if open.built {
        return;
    }
    open.built = true;
    for r in &roots {
        commands.entity(r).despawn();
    }
    let stop = pick.batch.is_some();
    build(&mut commands, open, stop, &mona, &face, &settings, now);
}

fn build(commands: &mut Commands, open: &Open, stop: bool, mona: &MonaSans, face: &crate::hud::Face, settings: &crate::settings::Settings, now: f32) {
    let lang = settings.language();
    let t = |s: &str| rustyshelf_core::i18n::tr(&lang, s);
    let text = |s: String, weight: u32, size: f32, color: Color| (Text::new(s), mona.font(weight, size), TextColor(color));
    let ink = Color::srgb(0.94, 0.95, 0.98);
    let dim = Color::srgba(0.94, 0.95, 0.98, 0.6);
    let shown = motion::progress(now, open.since, IN, OUT);
    commands
        .spawn((
            CoverRoot,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(30),
        ))
        .with_children(|r| {
            r.spawn((
                Node { width: px(980), max_width: percent(94), max_height: percent(84), flex_direction: FlexDirection::Column, padding: UiRect::new(px(18), px(18), px(20), px(14)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(20)), ..default() },
                BackgroundColor(Color::srgba(14.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.94)),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
                BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.55), x_offset: px(0), y_offset: px(30), spread_radius: px(0), blur_radius: px(80) }]),
                UiTransform { translation: Val2::px(0.0, 10.0 * (1.0 - shown)), scale: Vec2::splat(0.98 + 0.02 * shown), ..default() },
                CardIn,
            ))
            .with_children(|card| {
                card.spawn((Node { padding: UiRect::horizontal(px(8)), ..default() }, text(t(&open.title).to_uppercase(), 600, 12.0, dim)));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(4), px(8)), ..default() }, text(open.game.clone(), 600, 20.0, ink)));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(2), px(10)), ..default() }, text(t(&open.note), 400, 13.0, dim)));
                card.spawn((
                    CoverGrid,
                    Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::flex(COLS as u16, 1.0),
                        column_gap: px(12),
                        row_gap: px(12),
                        padding: UiRect::new(px(8), px(8), px(6), px(6)),
                        overflow: Overflow::scroll_y(),
                        min_height: px(0),
                        ..default()
                    },
                ))
                .with_children(|grid| {
                    for (i, it) in open.items.iter().enumerate() {
                        let lit = i == open.at;
                        let mut tile = grid.spawn((
                            Node { flex_direction: FlexDirection::Column, row_gap: px(6), padding: UiRect::all(px(6)), border_radius: BorderRadius::all(px(14)), min_width: px(0), ..default() },
                            BackgroundColor(if lit { Color::srgba(1.0, 1.0, 1.0, 0.92) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) }),
                        ));
                        if lit {
                            tile.insert((LitTile, UiTransform::from_translation(Val2::px(0.0, -2.0)), BoxShadow(vec![ShadowStyle { color: Color::srgba(0.47, 0.55, 1.0, 0.4), x_offset: px(0), y_offset: px(0), spread_radius: px(2), blur_radius: px(18) }])));
                        }
                        tile.with_children(|tile| {
                            let mut im = tile.spawn((
                                Node { width: percent(100), aspect_ratio: Some(2.0 / 3.0), border_radius: BorderRadius::all(px(9)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() },
                                BackgroundColor(Color::srgb(0.043, 0.051, 0.086)),
                            ));
                            match &it.thumb {
                                Some(h) => {
                                    im.with_children(|im| {
                                        im.spawn((Node { width: percent(100), height: percent(100), ..default() }, ImageNode::new(h.clone())));
                                    });
                                }
                                None => {
                                    if matches!(it.act, Act::SteamGrid) {
                                        im.insert((BorderColor::all(Color::srgba(0.42, 0.55, 1.0, 0.75)), BackgroundColor(Color::srgba(0.42, 0.55, 1.0, 0.1))));
                                    }
                                    // (the tiles' signs aren't in Mona Sans)
                                    im.with_children(|im| {
                                        im.spawn((Text::new(it.icon), TextFont::from_font_size(bevy::text::FontSize::Px(34.0)).with_font(face.0.clone()), TextColor(if lit { Color::srgb(0.24, 0.26, 0.33) } else { dim })));
                                    });
                                }
                            }
                            let dark = Color::srgb(0.063, 0.071, 0.102);
                            tile.spawn(text(t(&it.label), 600, 12.0, if lit { dark } else { ink }));
                            tile.spawn(text(t(&it.sub), 400, 11.0, if lit { Color::srgb(0.23, 0.25, 0.32) } else { dim }));
                        });
                    }
                });
                let (a, b) = (rustyshelf_core::pads::button_word(true, settings.glyphs(), settings.swap()), rustyshelf_core::pads::button_word(false, settings.glyphs(), settings.swap()));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(10), px(0)), ..default() }, text(format!("{a} {} · {b} {}", t(if open.gallery { "change" } else { "use" }), t(if stop { "stop" } else { "back" })), 400, 11.0, dim)));
            });
        });
}

/// Once laid out: the grid scrolled to keep the lit tile in view.
pub fn scroll(time: Res<Time>, mut pick: ResMut<CoverPick>, mut grid: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<CoverGrid>>, lit: Query<(&ComputedNode, &UiGlobalTransform), With<LitTile>>) {
    let now = time.elapsed_secs();
    let (Ok((g, gt, mut at)), Ok((l, lt))) = (grid.single_mut(), lit.single()) else { return };
    let inv = g.inverse_scale_factor;
    let top = ((lt.translation.y - l.size.y / 2.0) - (gt.translation.y - g.size.y / 2.0) + g.scroll_position.y) * inv;
    let (h, view) = (l.size.y * inv, g.size.y * inv);
    let mut to = pick.scroll.target();
    if top - 12.0 < to {
        to = top - 12.0;
    }
    if top + h + 12.0 > to + view {
        to = top + h + 12.0 - view;
    }
    pick.scroll.go(to.max(0.0), now, 0.22, OUT);
    let y = pick.scroll.now(now, 0.22, OUT);
    if at.0.y != y {
        at.0.y = y;
    }
}

/// The card coming in: sliding up and growing to its size, and fading in with what's behind it dimmed.
pub fn animate(time: Res<Time>, pick: Res<CoverPick>, mut card: Query<&mut UiTransform, With<CardIn>>, roots: Query<Entity, With<CoverRoot>>, children: Query<&Children>, mut colours: motion::FadeColours, mut commands: Commands, mut faded: Local<bool>) {
    let Some(open) = pick.open.as_ref() else {
        *faded = false;
        return;
    };
    let now = time.elapsed_secs();
    let k = motion::progress(now, open.since, IN, OUT).min(1.0);
    for mut t in &mut card {
        t.translation = Val2::px(0.0, 10.0 * (1.0 - k));
        t.scale = Vec2::splat(0.98 + 0.02 * k);
    }
    if k < 1.0 || !*faded {
        *faded = k >= 1.0;
        for r in &roots {
            motion::fade_tree(r, k, &|_| 1.0, &children, &mut colours, &mut commands);
        }
    }
}

/// Settings → PC games → Console box art: the PC games' console boxes not made yet, made behind the shelf one game at
/// a time and never while a game runs; the library is read again when some came in.
#[derive(Resource, Default)]
pub struct BoxMaker {
    work: Option<Mutex<Receiver<usize>>>,
    playing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// the library changed while a run was going
    again: bool,
}

pub fn make_boxes(mut maker: ResMut<BoxMaker>, base: Res<crate::loading::Base>, playing: Res<crate::play::Playing>) {
    let maker = &mut *maker;
    let busy = playing.title.is_some();
    maker.playing.store(busy, std::sync::atomic::Ordering::Relaxed);
    if let Some(made) = maker.work.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        maker.work = None;
        if made > 0 {
            crate::loading::STALE.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    maker.again |= base.is_changed();
    if !maker.again || maker.work.is_some() || busy {
        return;
    }
    maker.again = false;
    let Some(lib) = base.0.as_deref().filter(|l| l.config.pc_console_box()) else { return };
    let games: Vec<(String, String)> = lib
        .shelves
        .iter()
        .filter(|s| matches!(s.id.as_str(), "pc" | "custom" | "ports"))
        .flat_map(|s| &s.games)
        .filter(|g| matches!(g.source, Source::Steam { .. } | Source::Pc(_)))
        .map(|g| (g.id.clone(), g.title.clone()))
        .collect();
    let (data, region, playing) = (rustyshelf_core::paths::data_dir(&lib.app_root), lib.config.art_region().to_string(), maker.playing.clone());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let games: Vec<(&str, &str)> = games.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let _ = tx.send(pccovers::make_console_boxes(&data, &games, &region, &|| !playing.load(std::sync::atomic::Ordering::Relaxed)));
    });
    maker.work = Some(Mutex::new(rx));
}

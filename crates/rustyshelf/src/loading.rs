//! Work that would stall a frame runs on threads of its own: reading the games folders, decoding box art.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rustyshelf_core::config::LoadError;
use rustyshelf_core::library::{Library, ShelfKind};
use rustyshelf_core::scanner::{Game, Source, Tile};
use rustyshelf_core::looks::Look;

use crate::threads::Later;

/// Box art is shown at most this tall: plenty for a box on a TV, and a few hundred covers stay within GPU memory.
const ART_HEIGHT: u32 = 512;

pub enum LibraryState {
    Loading(Later<Result<Library, LoadError>>),
    Ready(Library),
    Failed(String),
}

/// The shelves as they're shown: the games the filter keeps, in the order chosen.
#[derive(Resource)]
pub struct LibraryRes(pub LibraryState);

/// Every game the library holds, as it was read: what the shown shelves are made from.
#[derive(Resource, Default)]
pub struct Base(pub Option<std::sync::Arc<Library>>);

/// Makes the shown shelves again on a worker, as doing it copies the whole library. An answer to an older ask, or
/// to a library since read again, is dropped.
#[derive(Resource)]
pub struct Reshower {
    ask: Sender<(u64, std::sync::Arc<Library>, rustyshelf_core::history::History)>,
    done: Mutex<Receiver<(u64, Library)>>,
    asked: u64,
}

impl Default for Reshower {
    fn default() -> Self {
        let (ask, done) = crate::threads::worker(true, |job: (u64, std::sync::Arc<Library>, rustyshelf_core::history::History)| Some((job.0, shown(&job.1, &job.2))));
        Reshower { ask, done, asked: 0 }
    }
}

impl Reshower {
    /// Asks for the shelves to be shown again from `base`.
    pub fn ask(&mut self, base: &std::sync::Arc<Library>, history: &rustyshelf_core::history::History) {
        self.asked += 1;
        let _ = self.ask.send((self.asked, base.clone(), history.clone()));
    }

    /// Drops any answer in flight: the library it came from has been replaced.
    pub fn forget(&mut self) {
        self.asked += 1;
    }
}

/// Puts the worker's newest shelves in place.
pub fn take_reshown(mut lib: ResMut<LibraryRes>, mut cursor: ResMut<crate::shelf::Cursor>, re: Res<Reshower>) {
    let Some((n, fresh)) = re.done.lock().ok().and_then(|d| d.try_iter().last()) else { return };
    if n != re.asked {
        return;
    }
    let _span = crate::frames::span("reshown");
    put_shown(&mut lib, fresh, &mut cursor);
}

/// The library with each shelf's hidden games and the ones the filter leaves out gone, the rest in the order chosen
/// (the settings' `ui.sort`, `ui.filter`, `ui.favorites`, `ui.hidden`).
pub fn shown(base: &Library, history: &rustyshelf_core::history::History) -> Library {
    use rustyshelf_core::shelfview::{arrange, View};
    let view = View::from_ui(base.config.raw().get("ui"));
    let mut lib = base.clone();
    let ui = base.config.raw().get("ui");
    let hidden: Vec<&str> = ui.and_then(|u| u.get("hidden")).and_then(|h| h.as_array()).map(|h| h.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let kid = crate::settings::kid_hidden(ui);
    rustyshelf_core::shelfview::add_into(&mut lib.shelves, ui, &kid);
    for shelf in lib.shelves.iter_mut() {
        let all = if shelf.from.is_empty() { shelf.games.len() } else { shelf.from.iter().filter(|f| **f == shelf.id).count() };
        let order = arrange(&shelf.id, &shelf.games, &view, history);
        if !shelf.from.is_empty() {
            shelf.from = order.iter().map(|i| shelf.from[*i].clone()).collect();
        }
        shelf.games = order.into_iter().map(|i| shelf.games[i].clone()).collect();
        // the shelf's own cases among its games (app.js prepare()): Surprise me, Add games and the emulator's in
        // front, the Custom shelf's Add games last
        let tile = |id: String, title: String, tile: Tile| Game { id, title, source: Source::Tile(tile), ..Game::default() };
        let mut front = Vec::new();
        let (console, pc) = (matches!(shelf.kind, ShelfKind::Console { .. }), matches!(shelf.kind, ShelfKind::Pc));
        let on = |key: &str, off: &serde_json::Value| ui.and_then(|u| u.get(key)) != Some(off);
        if (console || pc) && shelf.games.len() > 1 && !hidden.contains(&format!("rnd-{}", shelf.id).as_str()) && on("surpriseMe", &"off".into()) {
            front.push(tile(format!("rnd-{}", shelf.id), "Surprise me".into(), Tile::Surprise));
        }
        if console && all == 0 {
            front.push(tile(format!("setup-{}", shelf.id), "Add games".into(), Tile::AddGames));
        }
        if let ShelfKind::Console { console, config } = &shelf.kind {
            let emulator = if config.emulator.is_empty() && console.retro { base.config.retroarch() } else { config.emulator.clone() };
            let there = !emulator.is_empty() && rustyshelf_core::paths::is_file(&rustyshelf_core::launch::emu_path(&emulator, &base.app_root));
            if there && !hidden.contains(&format!("emu-{}", shelf.id).as_str()) && on("emuCase", &false.into()) {
                let mut case = tile(format!("emu-{}", shelf.id), format!("Open {}", crate::settings::emulator_of(shelf)), Tile::Emulator);
                case.file = rustyshelf_core::launch::emu_path(&emulator, &base.app_root);
                case.art.front = crate::settings::emu_cover(&base.app_root, &shelf.id);
                front.push(case);
            }
        }
        if !shelf.from.is_empty() {
            shelf.from.splice(0..0, vec![shelf.id.clone(); front.len()]);
        }
        shelf.games.splice(0..0, front);
        if shelf.id == "custom" {
            shelf.games.push(tile("setup-custom".into(), "Add games".into(), Tile::AddCustom));
            if !shelf.from.is_empty() {
                shelf.from.push(shelf.id.clone());
            }
        }
    }
    // Kid mode: only the shelves ticked for kids
    lib.shelves.retain(|s| !kid.contains(&s.id));
    // a console's shelf with nothing to show (its games all hidden) isn't shown
    lib.shelves.retain(|s| !s.games.is_empty());
    // your own order of the shelves (ui.shelfOrder); the ones it doesn't name keep theirs, after the rest
    let order: Vec<&str> = ui.and_then(|u| u.get("shelfOrder")).and_then(|o| o.as_array()).map(|o| o.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    if !order.is_empty() {
        let place: std::collections::HashMap<String, usize> = lib.shelves.iter().enumerate().map(|(i, s)| (s.id.clone(), order.iter().position(|o| *o == s.id).unwrap_or(1000 + i))).collect();
        lib.shelves.sort_by_key(|s| place[&s.id]);
    }
    // Home, All and the collections come first, in their own order
    let games = base.config.raw().get("games");
    let status = |id: &str| games.and_then(|g| g.get(id)).and_then(|g| g.get("status")).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let mut shelves = rustyshelf_core::shelfview::mixed_shelves_with(&base.shelves, base.config.raw().get("ui"), history, &status);
    if !kid.is_empty() {
        for s in shelves.iter_mut() {
            let keep: Vec<bool> = s.from.iter().map(|f| !kid.contains(f)).collect();
            let mut at = keep.iter();
            s.games.retain(|_| *at.next().unwrap_or(&true));
            let mut at = keep.iter();
            s.from.retain(|_| *at.next().unwrap_or(&true));
        }
        shelves.retain(|s| !s.games.is_empty());
    }
    shelves.append(&mut lib.shelves);
    lib.shelves = shelves;
    lib
}

/// The shelves were asked to be shown again: a game was played (Home changes with it).
#[derive(Resource, Default)]
pub struct Reshow(pub bool);

/// Puts shelves shown again in place, the cursor staying on its shelf and its game where they're still shown.
fn put_shown(lib: &mut LibraryRes, fresh: Library, cursor: &mut crate::shelf::Cursor) {
    let keep = match &lib.0 {
        LibraryState::Ready(l) => l.shelves.get(cursor.shelf).map(|s| (s.id.clone(), s.games.get(cursor.game).map(|g| g.id.clone()))),
        _ => None,
    };
    if let Some((shelf, game)) = keep {
        cursor.shelf = fresh.shelves.iter().position(|s| s.id == shelf).unwrap_or(cursor.shelf.min(fresh.shelves.len().saturating_sub(1)));
        let games = fresh.shelves.get(cursor.shelf).map_or(&[][..], |s| s.games.as_slice());
        cursor.game = game.and_then(|id| games.iter().position(|g| g.id == id)).unwrap_or(cursor.game.min(games.len().saturating_sub(1)));
    }
    cursor.built = None;
    lib.0 = LibraryState::Ready(fresh);
}

/// The library being read again behind the shelf, after its lists or sizes changed.
#[derive(Resource, Default)]
pub struct Refresh(Option<Later<Option<Library>>>);

/// Set from any thread when what's on disk changed behind the shelf: the library is read again.
pub static STALE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads the library again once something set `STALE`.
pub fn reread(mut refresh: ResMut<Refresh>) {
    if STALE.load(std::sync::atomic::Ordering::Relaxed) && read_again(&mut refresh, &rustyshelf_core::paths::app_root()) {
        STALE.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Refresh {
    /// The library is being read again.
    pub fn busy(&self) -> bool {
        self.0.is_some()
    }
}

/// A games folder that was out of reach is looked at again every minute; back, the library is read again behind the
/// shelf.
pub fn recheck_folders(time: Res<Time>, mut next: Local<f32>, base: Res<Base>, mut refresh: ResMut<Refresh>) {
    if *next == 0.0 {
        *next = 60.0;
    }
    if time.elapsed_secs() < *next || refresh.0.is_some() {
        return;
    }
    *next = time.elapsed_secs() + 60.0;
    let Some(root) = base.0.as_ref().map(|l| l.app_root.clone()).filter(|_| rustyshelf_core::library::folders_gone()) else { return };
    read_again_if(&mut refresh, root, rustyshelf_core::library::folders_back);
}

/// Settings → PC → Games you haven't installed: Playnite's library is looked at once a minute while the shelf is
/// idle, and read again (on a thread: it takes PowerShell a few seconds) when it has changed; a change reads the
/// shelf again.
pub fn owned_watch(time: Res<Time>, mut next: Local<f32>, mut busy: Local<Option<Later<bool>>>, mut was_on: Local<bool>, settings: Res<crate::settings::Settings>, playing: Res<crate::play::Playing>) {
    if let Some(job) = busy.as_ref() {
        match job.try_recv() {
            Ok(changed) => {
                *busy = None;
                if changed {
                    STALE.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => *busy = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        return;
    }
    if *next == 0.0 {
        *next = 10.0;
    }
    // switched on just now: looked at straight away
    let on = settings.flag_owned();
    if on && !*was_on && time.elapsed_secs() > 10.0 {
        *next = 0.0;
    }
    *was_on = on;
    if !cfg!(windows) || time.elapsed_secs() < *next || playing.title.is_some() || !on {
        return;
    }
    *next = time.elapsed_secs() + 60.0;
    *busy = Some(Later::spawn(|| {
        let Some(found) = rustyshelf_core::addgames::find(None, &rustyshelf_core::paths::env_var) else { return false };
        let data = rustyshelf_core::paths::data_dir(&rustyshelf_core::paths::app_root());
        rustyshelf_core::notinstalled::refresh(&data, &found).unwrap_or_else(|e| {
            warn!("Games you haven't installed: {e}");
            false
        })
    }));
}

/// Something outside the shelf changed what's on it (a RomM game downloaded, an Epic install finished): it's read
/// again behind the shelf.
pub static READ_AGAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Reads the library again behind the shelf; false while it's already being read.
pub fn read_again(refresh: &mut Refresh, root: &std::path::Path) -> bool {
    if refresh.0.is_some() {
        return false;
    }
    read_again_if(refresh, root.to_path_buf(), || true);
    true
}

/// Reads the library again behind the shelf when `first`, run on the same thread, says to.
fn read_again_if(refresh: &mut Refresh, root: std::path::PathBuf, first: impl FnOnce() -> bool + Send + 'static) {
    refresh.0 = Some(Later::spawn(move || if first() { Library::load_from(&root).ok() } else { None }));
}

pub fn start_library() -> LibraryRes {
    LibraryRes(LibraryState::Loading(Later::spawn(Library::load)))
}

pub fn poll_library(
    mut lib: ResMut<LibraryRes>,
    mut base: ResMut<Base>,
    plays: Res<crate::play::Plays>,
    mut refresh: ResMut<Refresh>,
    mut again: ResMut<Reshow>,
    mut cursor: ResMut<crate::shelf::Cursor>,
    mut re: ResMut<Reshower>,
) {
    if std::mem::take(&mut again.0) {
        if let (Some(b), LibraryState::Ready(_)) = (&base.0, &lib.0) {
            re.ask(b, &plays.0);
        }
    }
    if let LibraryState::Loading(loading) = &lib.0 {
        let got = loading.try_recv();
        match got {
            Ok(Ok(l)) => {
                // what the shelves want from outside comes in behind them; a library that shows more is read again
                let seen = l.clone();
                read_again_if(&mut refresh, l.app_root.clone(), move || rustyshelf_core::library::upkeep(&seen));
                let shelves = shown(&l, &plays.0);
                let open_into = l.config.raw().get("ui").and_then(|u| u.get("openInto")).and_then(|v| v.as_str()).unwrap_or("last").to_string();
                crate::place::open_into(&open_into, &rustyshelf_core::paths::data_dir(&l.app_root), &shelves, &mut cursor);
                lib.0 = LibraryState::Ready(shelves);
                base.0 = Some(std::sync::Arc::new(l));
                re.forget();
            }
            Ok(Err(e)) => lib.0 = LibraryState::Failed(e.to_string()),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => lib.0 = LibraryState::Failed("reading the games stopped".into()),
        }
        return;
    }
    let fresh = refresh.0.as_ref().map(Later::try_recv);
    match fresh {
        Some(Ok(Some(fresh))) => {
            let l = shown(&fresh, &plays.0);
            base.0 = Some(std::sync::Arc::new(fresh));
            re.forget();
            // the same shelf and game stay under the cursor where they still exist
            let at = |lib: &Library| lib.shelves.get(cursor.shelf).map(|s| (s.id.clone(), s.games.get(cursor.game).map(|g| g.id.clone())));
            let was = match &lib.0 {
                LibraryState::Ready(old) => at(old),
                _ => None,
            };
            if let Some((shelf, game)) = was {
                cursor.shelf = l.shelves.iter().position(|s| s.id == shelf).unwrap_or(0);
                cursor.game = l.shelves.get(cursor.shelf).and_then(|s| s.games.iter().position(|g| Some(&g.id) == game.as_ref())).unwrap_or(0);
            }
            cursor.built = None;
            refresh.0 = None;
            lib.0 = LibraryState::Ready(l);
        }
        Some(Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected)) => refresh.0 = None,
        _ => {}
    }
}

pub struct Decoded {
    pub path: PathBuf,
    /// None: the file couldn't be read or isn't a picture
    pub image: Option<(u32, u32, Vec<u8>)>,
    /// a cover's colour and its few-pixel swatch for the backdrop, worked out here rather than on the frame
    pub hue: Option<f32>,
    pub swatch: Option<std::sync::Arc<Vec<u8>>>,
}

/// A picture for a case: one on disk, or one drawn for a game that has none.
#[derive(Clone)]
pub enum Art {
    File(PathBuf),
    /// `seed`: the number it's drawn from, None for its title's own
    Front { look: Box<Look>, title: String, region: String, scene: Option<&'static str>, hue: Option<f32>, seed: Option<u32> },
    /// the cover of one of the shelf's own cases
    Tile { look: Box<Look>, tile: Tile },
    /// an "Open <emulator>" case: the emulator's own icon on its cover, else `drawn`
    Emulator { look: Box<Look>, name: String, exe: PathBuf, drawn: Box<Art> },
    /// a picture in greys, this bright and with this much contrast: a game that isn't installed
    Grey(Box<Art>, f32, f32),
    /// `shots`: the game's screenshots (files or addresses), for the four pictures
    Back { look: Box<Look>, title: String, facts: Vec<(String, String)>, blurb: String, shots: Vec<String> },
    /// a spine painted `width` × `height` pixels, two to a pixel of the shelf: `edge` is the case's border there
    Spine { look: Box<Look>, title: String, label: String, width: u32, height: u32, edge: f32 },
}

impl Art {
    /// What the picture goes by once it's made: a file's own path, or all that a drawn one is drawn from.
    pub fn key(&self) -> PathBuf {
        match self {
            Art::File(p) => p.clone(),
            Art::Front { look, title, region, hue, seed, .. } => PathBuf::from(format!("drawn:{}|f|{title}|{region}|{hue:?}|{seed:?}", look.id)),
            Art::Tile { look, tile } => PathBuf::from(format!("drawn:{}|t|{tile:?}", look.id)),
            Art::Emulator { look, name, exe, .. } => {
                let at = exe.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                PathBuf::from(format!("drawn:{}|e|{name}|{}|{at}", look.id, exe.display()))
            }
            Art::Grey(of, bright, contrast) => PathBuf::from(format!("grey:{bright}:{contrast}:{}", of.key().display())),
            Art::Back { look, title, facts, blurb, shots } => {
                let said: String = facts.iter().flat_map(|(n, v)| [n.as_str(), v.as_str()]).chain([blurb.as_str()]).chain(shots.iter().map(String::as_str)).collect();
                PathBuf::from(format!("drawn:{}|b|{title}|{}", look.id, rustyshelf_core::drawn::hash(&said)))
            }
            Art::Spine { look, title, label, width, height, edge } => PathBuf::from(format!("drawn:{}|s|{title}|{label}|{width}x{height}|{edge}", look.id)),
        }
    }

    fn make(&self) -> Option<(u32, u32, Vec<u8>)> {
        let drawn = |p: rustyshelf_core::drawn::Picture| Some((p.width, p.height, p.rgba));
        match self {
            Art::File(p) => decode(p, ART_HEIGHT),
            Art::Front { look, title, region, scene, hue, seed } => drawn(rustyshelf_core::drawn::front_from(look, title, region, *scene, *hue, seed.unwrap_or_else(|| rustyshelf_core::drawn::hash(title)))),
            Art::Tile { look, tile: Tile::Surprise } => drawn(rustyshelf_core::drawn::surprise(look)),
            Art::Tile { look, .. } => drawn(rustyshelf_core::drawn::add_games(look)),
            // (a tiny icon would look blurred that large: the drawn cover instead)
            Art::Emulator { look, name, exe, drawn: other } => match rustyshelf_core::exeicon::largest(exe).filter(|i| i.0.min(i.1) >= 96) {
                Some(icon) => drawn(rustyshelf_core::drawn::emulator(look, name, &icon)),
                None => other.make(),
            },
            // CSS's grayscale(1) brightness(b) contrast(c)
            Art::Grey(of, bright, contrast) => of.make().map(|(w, h, mut rgba)| {
                for px in rgba.as_chunks_mut::<4>().0 {
                    let grey = 0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2]);
                    let v = ((grey * bright / 255.0 - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
                    px[..3].fill((v * 255.0).round() as u8);
                }
                (w, h, rgba)
            }),
            Art::Back { look, title, facts, blurb, shots } => {
                let data = rustyshelf_core::paths::data_dir(&rustyshelf_core::paths::app_root());
                let pictures: Vec<image::RgbaImage> = shots
                    .iter()
                    .filter_map(|s| if s.starts_with("https://") { rustyshelf_core::info::shot_file(&data, s) } else { Some(std::path::PathBuf::from(s)) })
                    .filter_map(|f| image::ImageReader::open(f).ok()?.with_guessed_format().ok()?.decode().ok())
                    .map(|i| i.resize(480, 480, image::imageops::FilterType::Triangle).into_rgba8())
                    .take(4)
                    .collect();
                drawn(rustyshelf_core::drawn::back_with(look, title, facts, blurb, &pictures))
            }
            Art::Spine { look, title, label, width, height, edge } => drawn(rustyshelf_core::drawn::spine(look, title, label, *width, *height, *edge, 2.0)),
        }
    }
}

#[derive(Resource)]
pub struct ArtLoader {
    tx: Sender<Art>,
    rx: Mutex<Receiver<Decoded>>,
}

impl ArtLoader {
    pub fn start() -> ArtLoader {
        let (req_tx, req_rx) = channel::<Art>();
        let (done_tx, done_rx) = channel();
        let req_rx = std::sync::Arc::new(Mutex::new(req_rx));
        // a third of the cores, so art never crowds out the frame
        for _ in 0..thread::available_parallelism().map_or(2, |n| (n.get() / 3).clamp(2, 4)) {
            let (req_rx, done_tx) = (req_rx.clone(), done_tx.clone());
            thread::spawn(move || loop {
                let Ok(art) = req_rx.lock().unwrap().recv() else { return };
                let image = art.make();
                // a spine's or a back's colours are never asked for
                let cover = !matches!(art, Art::Back { .. } | Art::Spine { .. });
                let (hue, swatch) = match &image {
                    Some((w, h, data)) if cover => (crate::shelf::hue_of(*w, *h, data), Some(std::sync::Arc::new(rustyshelf_core::themes::swatch(*w, *h, data)))),
                    _ => (None, None),
                };
                if done_tx.send(Decoded { path: art.key(), image, hue, swatch }).is_err() {
                    return;
                }
            });
        }
        ArtLoader { tx: req_tx, rx: Mutex::new(done_rx) }
    }

    pub fn request(&self, art: Art) {
        let _ = self.tx.send(art);
    }

    pub fn finished(&self) -> Vec<Decoded> {
        self.rx.lock().unwrap().try_iter().collect()
    }
}

/// A picture shrunk to at most `tallest` pixels tall, as (width, height, RGBA).
pub fn decode(path: &std::path::Path, tallest: u32) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.height() > tallest { img.resize(u32::MAX, tallest, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

pub fn to_image(w: u32, h: u32, data: Vec<u8>) -> Image {
    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

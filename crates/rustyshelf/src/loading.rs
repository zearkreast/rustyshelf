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

/// Box art is shown at most this tall: plenty for a box on a TV, and a few hundred covers stay within GPU memory.
const ART_HEIGHT: u32 = 512;

pub enum LibraryState {
    Loading(Mutex<Receiver<Result<Library, LoadError>>>),
    Ready(Library),
    Failed(String),
}

/// The shelves as they're shown: the games the filter keeps, in the order chosen.
#[derive(Resource)]
pub struct LibraryRes(pub LibraryState);

/// Every game the library holds, as it was read: what the shown shelves are made from.
#[derive(Resource, Default)]
pub struct Base(pub Option<Library>);

/// The library with each shelf's hidden games and the ones the filter leaves out gone, the rest in the order chosen
/// (the settings' `ui.sort`, `ui.filter`, `ui.favorites`, `ui.hidden`).
pub fn shown(base: &Library, history: &rustyshelf_core::history::History) -> Library {
    use rustyshelf_core::shelfview::{arrange, View};
    let view = View::from_ui(base.config.raw().get("ui"));
    let mut lib = base.clone();
    let ui = base.config.raw().get("ui");
    let hidden: Vec<&str> = ui.and_then(|u| u.get("hidden")).and_then(|h| h.as_array()).map(|h| h.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    for shelf in lib.shelves.iter_mut() {
        let all = shelf.games.len();
        let order = arrange(&shelf.id, &shelf.games, &view, history);
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
                front.push(tile(format!("emu-{}", shelf.id), format!("Open {}", crate::settings::emulator_of(shelf)), Tile::Emulator));
            }
        }
        shelf.games.splice(0..0, front);
        if shelf.id == "custom" {
            shelf.games.push(tile("setup-custom".into(), "Add games".into(), Tile::AddCustom));
        }
    }
    // a console's shelf with nothing to show (its games all hidden) isn't shown
    lib.shelves.retain(|s| !s.games.is_empty());
    // your own order of the shelves (ui.shelfOrder); the ones it doesn't name keep theirs, after the rest
    let order: Vec<&str> = ui.and_then(|u| u.get("shelfOrder")).and_then(|o| o.as_array()).map(|o| o.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    if !order.is_empty() {
        let place: std::collections::HashMap<String, usize> = lib.shelves.iter().enumerate().map(|(i, s)| (s.id.clone(), order.iter().position(|o| *o == s.id).unwrap_or(1000 + i))).collect();
        lib.shelves.sort_by_key(|s| place[&s.id]);
    }
    // Home, All and the collections come first, in their own order
    let mut shelves = rustyshelf_core::shelfview::mixed_shelves(&base.shelves, base.config.raw().get("ui"), history);
    shelves.append(&mut lib.shelves);
    lib.shelves = shelves;
    lib
}

/// The shelves were asked to be shown again: a game was played (Home changes with it).
#[derive(Resource, Default)]
pub struct Reshow(pub bool);

/// Shows the shelves again from the library as it was read, the cursor staying on its shelf and its game where
/// they're still shown.
pub fn reshow(lib: &mut LibraryRes, base: &Library, history: &rustyshelf_core::history::History, cursor: &mut crate::shelf::Cursor) {
    let keep = match &lib.0 {
        LibraryState::Ready(l) => l.shelves.get(cursor.shelf).map(|s| (s.id.clone(), s.games.get(cursor.game).map(|g| g.id.clone()))),
        _ => None,
    };
    let fresh = shown(base, history);
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
pub struct Refresh(Option<Mutex<Receiver<Library>>>);

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
    let (tx, rx) = channel();
    thread::spawn(move || {
        if rustyshelf_core::library::folders_back() {
            if let Ok(fresh) = Library::load_from(&root) {
                let _ = tx.send(fresh);
            }
        }
    });
    refresh.0 = Some(Mutex::new(rx));
}

/// Reads the library again behind the shelf; false while it's already being read.
pub fn read_again(refresh: &mut Refresh, root: &std::path::Path) -> bool {
    if refresh.0.is_some() {
        return false;
    }
    let (tx, rx) = channel();
    let root = root.to_path_buf();
    thread::spawn(move || {
        if let Ok(fresh) = Library::load_from(&root) {
            let _ = tx.send(fresh);
        }
    });
    refresh.0 = Some(Mutex::new(rx));
    true
}

pub fn start_library() -> LibraryRes {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let _ = tx.send(Library::load());
    });
    LibraryRes(LibraryState::Loading(Mutex::new(rx)))
}

pub fn poll_library(
    mut lib: ResMut<LibraryRes>,
    mut base: ResMut<Base>,
    plays: Res<crate::play::Plays>,
    mut refresh: ResMut<Refresh>,
    mut again: ResMut<Reshow>,
    mut cursor: ResMut<crate::shelf::Cursor>,
) {
    if std::mem::take(&mut again.0) {
        if let (Some(b), LibraryState::Ready(_)) = (&base.0, &lib.0) {
            reshow(&mut lib, b, &plays.0, &mut cursor);
        }
    }
    if let LibraryState::Loading(rx) = &lib.0 {
        let got = rx.lock().unwrap().try_recv();
        match got {
            Ok(Ok(l)) => {
                // what the shelves want from outside comes in behind them; a library that shows more is read again
                let (tx, rx) = channel();
                let seen = l.clone();
                thread::spawn(move || {
                    if rustyshelf_core::library::upkeep(&seen) {
                        if let Ok(fresh) = Library::load_from(&seen.app_root) {
                            let _ = tx.send(fresh);
                        }
                    }
                });
                refresh.0 = Some(Mutex::new(rx));
                lib.0 = LibraryState::Ready(shown(&l, &plays.0));
                base.0 = Some(l);
            }
            Ok(Err(e)) => lib.0 = LibraryState::Failed(e.to_string()),
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => lib.0 = LibraryState::Failed("reading the games stopped".into()),
        }
        return;
    }
    let fresh = refresh.0.as_ref().map(|rx| rx.lock().unwrap().try_recv());
    match fresh {
        Some(Ok(fresh)) => {
            let l = shown(&fresh, &plays.0);
            base.0 = Some(fresh);
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
        Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => refresh.0 = None,
        _ => {}
    }
}

pub struct Decoded {
    pub path: PathBuf,
    /// None: the file couldn't be read or isn't a picture
    pub image: Option<(u32, u32, Vec<u8>)>,
}

/// A picture for a case: one on disk, or one drawn for a game that has none.
#[derive(Clone)]
pub enum Art {
    File(PathBuf),
    /// `seed`: the number it's drawn from, None for its title's own
    Front { look: Box<Look>, title: String, region: String, scene: Option<&'static str>, hue: Option<f32>, seed: Option<u32> },
    /// the cover of one of the shelf's own cases
    Tile { look: Box<Look>, tile: Tile },
    /// a picture in greys, this bright and with this much contrast: a game that isn't installed
    Grey(Box<Art>, f32, f32),
    Back { look: Box<Look>, title: String, facts: Vec<(String, String)>, blurb: String },
    /// a spine painted `width` × `height` pixels, two to a pixel of the shelf: `edge` is the case's border there
    Spine { look: Box<Look>, title: String, label: String, width: u32, height: u32, edge: f32 },
    /// a museum row's name plate, and its section labels
    Plaque { name: String, lines: Vec<String> },
    Tabs { labels: Vec<(String, u32)>, tall: u32 },
    /// an arcade cabinet's marquee
    Marquee { title: String },
}

impl Art {
    /// What the picture goes by once it's made: a file's own path, or all that a drawn one is drawn from.
    pub fn key(&self) -> PathBuf {
        match self {
            Art::File(p) => p.clone(),
            Art::Front { look, title, region, hue, seed, .. } => PathBuf::from(format!("drawn:{}|f|{title}|{region}|{hue:?}|{seed:?}", look.id)),
            Art::Tile { look, tile } => PathBuf::from(format!("drawn:{}|t|{tile:?}", look.id)),
            Art::Grey(of, bright, contrast) => PathBuf::from(format!("grey:{bright}:{contrast}:{}", of.key().display())),
            Art::Back { look, title, facts, blurb } => {
                let said: String = facts.iter().flat_map(|(n, v)| [n.as_str(), v.as_str()]).chain([blurb.as_str()]).collect();
                PathBuf::from(format!("drawn:{}|b|{title}|{}", look.id, rustyshelf_core::drawn::hash(&said)))
            }
            Art::Spine { look, title, label, width, height, edge } => PathBuf::from(format!("drawn:{}|s|{title}|{label}|{width}x{height}|{edge}", look.id)),
            Art::Plaque { name, lines } => PathBuf::from(format!("drawn:plaque|{name}|{}", lines.join("|"))),
            Art::Tabs { labels, tall } => PathBuf::from(format!("drawn:tabs|{tall}|{}", labels.iter().map(|(l, w)| format!("{l}:{w}")).collect::<Vec<_>>().join("|"))),
            Art::Marquee { title } => PathBuf::from(format!("drawn:marquee|{title}")),
        }
    }

    fn make(&self) -> Option<(u32, u32, Vec<u8>)> {
        let drawn = |p: rustyshelf_core::drawn::Picture| Some((p.width, p.height, p.rgba));
        match self {
            Art::File(p) => decode(p),
            Art::Front { look, title, region, scene, hue, seed } => drawn(rustyshelf_core::drawn::front_from(look, title, region, *scene, *hue, seed.unwrap_or_else(|| rustyshelf_core::drawn::hash(title)))),
            Art::Tile { look, tile: Tile::Surprise } => drawn(rustyshelf_core::drawn::surprise(look)),
            Art::Tile { look, .. } => drawn(rustyshelf_core::drawn::add_games(look)),
            // CSS's grayscale(1) brightness(b) contrast(c)
            Art::Grey(of, bright, contrast) => of.make().map(|(w, h, mut rgba)| {
                for px in rgba.as_chunks_mut::<4>().0 {
                    let grey = 0.2126 * f32::from(px[0]) + 0.7152 * f32::from(px[1]) + 0.0722 * f32::from(px[2]);
                    let v = ((grey * bright / 255.0 - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
                    px[..3].fill((v * 255.0).round() as u8);
                }
                (w, h, rgba)
            }),
            Art::Back { look, title, facts, blurb } => drawn(rustyshelf_core::drawn::back(look, title, facts, blurb)),
            Art::Spine { look, title, label, width, height, edge } => drawn(rustyshelf_core::drawn::spine(look, title, label, *width, *height, *edge, 2.0)),
            Art::Plaque { name, lines } => drawn(rustyshelf_core::drawn::plaque(name, lines)),
            Art::Tabs { labels, tall } => drawn(rustyshelf_core::drawn::tabs(labels, *tall)),
            Art::Marquee { title } => drawn(rustyshelf_core::drawn::marquee(title)),
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
        for _ in 0..thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 6)) {
            let (req_rx, done_tx) = (req_rx.clone(), done_tx.clone());
            thread::spawn(move || loop {
                let Ok(art) = req_rx.lock().unwrap().recv() else { return };
                let image = art.make();
                if done_tx.send(Decoded { path: art.key(), image }).is_err() {
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

fn decode(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.height() > ART_HEIGHT { img.resize(u32::MAX, ART_HEIGHT, image::imageops::FilterType::Triangle) } else { img };
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

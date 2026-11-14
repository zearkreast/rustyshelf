//! Work that would stall a frame runs on threads of its own: reading the games folders, decoding box art.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rustyshelf_core::config::LoadError;
use rustyshelf_core::library::Library;
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
    for shelf in lib.shelves.iter_mut() {
        let order = arrange(&shelf.id, &shelf.games, &view, history);
        shelf.games = order.into_iter().map(|i| shelf.games[i].clone()).collect();
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
pub enum Art {
    File(PathBuf),
    Front { look: Box<Look>, title: String, region: String, scene: Option<&'static str>, hue: Option<f32> },
    Back { look: Box<Look>, title: String, facts: Vec<(String, String)>, blurb: String },
}

impl Art {
    /// What the picture goes by once it's made: a file's own path, or all that a drawn one is drawn from.
    pub fn key(&self) -> PathBuf {
        match self {
            Art::File(p) => p.clone(),
            Art::Front { look, title, region, hue, .. } => PathBuf::from(format!("drawn:{}|f|{title}|{region}|{hue:?}", look.id)),
            Art::Back { look, title, facts, blurb } => {
                let said: String = facts.iter().flat_map(|(n, v)| [n.as_str(), v.as_str()]).chain([blurb.as_str()]).collect();
                PathBuf::from(format!("drawn:{}|b|{title}|{}", look.id, rustyshelf_core::drawn::hash(&said)))
            }
        }
    }

    fn make(&self) -> Option<(u32, u32, Vec<u8>)> {
        let drawn = |p: rustyshelf_core::drawn::Picture| Some((p.width, p.height, p.rgba));
        match self {
            Art::File(p) => decode(p),
            Art::Front { look, title, region, scene, hue } => drawn(rustyshelf_core::drawn::front(look, title, region, *scene, *hue)),
            Art::Back { look, title, facts, blurb } => drawn(rustyshelf_core::drawn::back(look, title, facts, blurb)),
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

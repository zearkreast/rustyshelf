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

/// Box art is shown at most this tall: plenty for a box on a TV, and a few hundred covers stay within GPU memory.
const ART_HEIGHT: u32 = 512;

pub enum LibraryState {
    Loading(Mutex<Receiver<Result<Library, LoadError>>>),
    Ready(Library),
    Failed(String),
}

#[derive(Resource)]
pub struct LibraryRes(pub LibraryState);

pub fn start_library() -> LibraryRes {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let _ = tx.send(Library::load());
    });
    LibraryRes(LibraryState::Loading(Mutex::new(rx)))
}

pub fn poll_library(mut lib: ResMut<LibraryRes>) {
    let LibraryState::Loading(rx) = &lib.0 else { return };
    let got = rx.lock().unwrap().try_recv();
    match got {
        Ok(Ok(l)) => lib.0 = LibraryState::Ready(l),
        Ok(Err(e)) => lib.0 = LibraryState::Failed(e.to_string()),
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(std::sync::mpsc::TryRecvError::Disconnected) => lib.0 = LibraryState::Failed("reading the games stopped".into()),
    }
}

pub struct Decoded {
    pub path: PathBuf,
    /// None: the file couldn't be read or isn't a picture
    pub image: Option<(u32, u32, Vec<u8>)>,
}

#[derive(Resource)]
pub struct ArtLoader {
    tx: Sender<PathBuf>,
    rx: Mutex<Receiver<Decoded>>,
}

impl ArtLoader {
    pub fn start() -> ArtLoader {
        let (req_tx, req_rx) = channel::<PathBuf>();
        let (done_tx, done_rx) = channel();
        let req_rx = std::sync::Arc::new(Mutex::new(req_rx));
        for _ in 0..thread::available_parallelism().map_or(2, |n| n.get().clamp(2, 6)) {
            let (req_rx, done_tx) = (req_rx.clone(), done_tx.clone());
            thread::spawn(move || loop {
                let Ok(path) = req_rx.lock().unwrap().recv() else { return };
                let image = decode(&path);
                if done_tx.send(Decoded { path, image }).is_err() {
                    return;
                }
            });
        }
        ArtLoader { tx: req_tx, rx: Mutex::new(done_rx) }
    }

    pub fn request(&self, path: PathBuf) {
        let _ = self.tx.send(path);
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

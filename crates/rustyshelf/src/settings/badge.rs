//! The highlighted achievement's badge, or game's icon, beside the Achievements and Trophies lists: fetched off the
//! frame after a moment on the row, and kept for the run.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::*;

type Picture = (u32, u32, Vec<u8>);

const DWELL: f32 = 0.25;
/// px on screen
pub(super) const SIZE: f32 = 180.0;

#[derive(Default)]
pub(super) struct Badge {
    shown: Option<(String, Handle<Image>)>,
    lit: (String, f32),
    at: Option<usize>,
    reading: Option<(String, Mutex<Receiver<Option<Picture>>>)>,
    got: HashMap<String, Option<Picture>>,
}

impl Settings {
    fn badge_url(&self) -> Option<String> {
        let (page, pics) = match self.stack.last()?.0 {
            Screen::Achievements => self.achievements_listing(),
            Screen::Trophies => self.trophies_listing(),
            _ => return None,
        };
        let at = self.stack.last().map_or(0, |p| p.1).min(page.rows.len().saturating_sub(1));
        pics.get(at).cloned().flatten()
    }

    pub(super) fn poll_badge(&mut self, now: f32) {
        if let Some(pic) = self.badge.reading.as_ref().and_then(|(_, rx)| rx.lock().unwrap().try_recv().ok()) {
            let (url, _) = self.badge.reading.take().expect("a badge being read");
            if self.badge.got.len() > 400 {
                self.badge.got.clear();
            }
            self.badge.got.insert(url, pic);
            self.redraw = true;
        }
        if self.badge.reading.is_some() || !self.stack.last().is_some_and(|p| matches!(p.0, Screen::Achievements | Screen::Trophies)) {
            return;
        }
        // the page is only built again when something on it changed
        let at = self.stack.last().map_or(0, |p| p.1);
        if self.redraw || self.badge.at != Some(at) {
            self.badge.at = Some(at);
            let url = self.badge_url().unwrap_or_default();
            if self.badge.lit.0 != url {
                self.badge.lit = (url, now);
            }
        }
        let url = self.badge.lit.0.clone();
        if url.is_empty() || self.badge.got.contains_key(&url) || now - self.badge.lit.1 < DWELL {
            return;
        }
        let (tx, rx) = channel();
        let ask = url.clone();
        std::thread::spawn(move || {
            let pic = rustyshelf_core::net::get_bytes(&ask, std::time::Duration::from_secs(15), 4 << 20).ok().flatten().and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
                let rgba = img.into_rgba8();
                (rgba.width(), rgba.height(), rgba.into_raw())
            });
            let _ = tx.send(pic);
        });
        self.badge.reading = Some((url, Mutex::new(rx)));
    }

    /// The picture for the highlighted row, once it's here.
    pub(super) fn badge_preview(&mut self, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        let Some(url) = self.badge_url() else {
            if let Some((_, old)) = self.badge.shown.take() {
                images.remove(&old);
            }
            return None;
        };
        if self.badge.shown.as_ref().is_some_and(|s| s.0 == url) {
            return self.badge.shown.as_ref().map(|s| s.1.clone());
        }
        if let Some((_, old)) = self.badge.shown.take() {
            images.remove(&old);
        }
        let (w, h, data) = self.badge.got.get(&url)?.clone()?;
        let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
        let handle = images.add(image);
        self.badge.shown = Some((url, handle.clone()));
        Some(handle)
    }
}

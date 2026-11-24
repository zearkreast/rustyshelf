//! The pictures beside the rows of the achievement and trophy lists: badges and game icons, from the web or from
//! RPCS3's folders, fetched off the frame for the rows round the chosen one, kept for the run and on disk
//! (`rustyshelf_core::achpics`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::Sender;

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::*;

type Picture = (u32, u32, Vec<u8>);

/// the "How to get it" row's picture, drawn here: a red rounded box with a white play mark
pub(super) const YOUTUBE: &str = "drawn:youtube";
/// at most this many fetched at once
const AT_ONCE: usize = 6;
/// rows each side of the chosen one whose pictures are fetched
const AROUND: usize = 14;
/// px of the stored picture's longer side
const SIZE: u32 = 128;

enum Pic {
    Loading,
    Raw(Picture),
    Ready(Handle<Image>),
    Failed,
}

pub(super) struct Badge {
    got: HashMap<String, Pic>,
    wanted: Vec<String>,
    busy: usize,
    tx: Sender<(String, Option<Picture>)>,
    rx: Mutex<Receiver<(String, Option<Picture>)>>,
    /// a picture came in since the last redraw, and when the last one was drawn
    fresh: bool,
    drawn: f32,
}

impl Default for Badge {
    fn default() -> Self {
        let (tx, rx) = channel();
        Badge { got: HashMap::new(), wanted: Vec::new(), busy: 0, tx, rx: Mutex::new(rx), fresh: false, drawn: 0.0 }
    }
}

fn youtube() -> Picture {
    let n = 64u32;
    let mut px = vec![0u8; (n * n * 4) as usize];
    let (w, h, r) = (64.0f32, 46.0f32, 12.0f32);
    let (x0, y0) = ((n as f32 - w) / 2.0, (n as f32 - h) / 2.0);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let dx = (x0 + r - fx).max(fx - (x0 + w - r)).max(0.0);
            let dy = (y0 + r - fy).max(fy - (y0 + h - r)).max(0.0);
            let inside = fx >= x0 && fx <= x0 + w && fy >= y0 && fy <= y0 + h && dx * dx + dy * dy <= r * r;
            if !inside {
                continue;
            }
            let (tx, ty) = (fx - 25.0, fy - 32.0);
            let play = (0.0..=17.0).contains(&tx) && ty.abs() <= 10.0 * (1.0 - tx / 17.0);
            let i = ((y * n + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(if play { &[255, 255, 255, 255] } else { &[230, 33, 23, 255] });
        }
    }
    (n, n, px)
}

fn load(data: &Path, src: &str) -> Option<Picture> {
    let bytes = if src.starts_with("https://") || src.starts_with("http://") {
        rustyshelf_core::achpics::get(data, src)?
    } else {
        std::fs::read(src).ok()?
    };
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width().max(img.height()) > SIZE { img.resize(SIZE, SIZE, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.into_rgba8();
    Some((rgba.width(), rgba.height(), rgba.into_raw()))
}

impl Settings {
    /// The chosen page's rows' pictures, when it's a page that has them.
    fn page_pics(&self) -> Option<Vec<Option<String>>> {
        Some(match self.stack.last()?.0 {
            Screen::Achievements => self.achievements_listing().1,
            Screen::Trophies => self.trophies_listing().1,
            Screen::TrophyGame => self.trophy_game_listing().1,
            Screen::AchievementOne => self.achievement_listing().1,
            _ => return None,
        })
    }

    pub(super) fn poll_badge(&mut self, now: f32) {
        let came: Vec<(String, Option<Picture>)> = self.badge.rx.lock().unwrap().try_iter().collect();
        for (url, pic) in came {
            self.badge.busy = self.badge.busy.saturating_sub(1);
            self.badge.got.insert(url, pic.map_or(Pic::Failed, Pic::Raw));
            self.badge.fresh = true;
        }
        // pictures that came in are drawn in batches, five times a second at most
        if self.badge.fresh && now - self.badge.drawn >= 0.2 {
            (self.badge.fresh, self.badge.drawn, self.redraw) = (false, now, true);
        }
        if self.badge.got.len() > 800 {
            self.badge.got.retain(|_, p| matches!(p, Pic::Loading));
        }
        while self.badge.busy < AT_ONCE {
            let Some(url) = self.badge.wanted.iter().find(|u| !self.badge.got.contains_key(*u)).cloned() else { break };
            if url == YOUTUBE {
                self.badge.got.insert(url, Pic::Raw(youtube()));
                self.badge.fresh = true;
                continue;
            }
            self.badge.got.insert(url.clone(), Pic::Loading);
            self.badge.busy += 1;
            let (tx, data) = (self.badge.tx.clone(), paths::data_dir(&self.app_root));
            std::thread::spawn(move || {
                let pic = load(&data, &url);
                let _ = tx.send((url, pic));
            });
        }
    }

    /// Each row's picture for the list: None for a row without one, Some(None) while it's coming.
    pub(super) fn row_pics(&mut self, images: &mut Assets<Image>) -> Vec<Option<Option<Handle<Image>>>> {
        let Some(pics) = self.page_pics() else {
            self.badge.wanted.clear();
            return Vec::new();
        };
        // what's fetched follows the page as drawn: the rows round the chosen one, the nearest first
        let at = self.stack.last().map_or(0, |p| p.1);
        let (lo, hi) = (at.saturating_sub(AROUND), (at + AROUND + 1).min(pics.len()));
        let mut near: Vec<(usize, String)> = pics.iter().enumerate().take(hi).skip(lo).filter_map(|(i, p)| Some((i.abs_diff(at), p.clone()?))).collect();
        near.sort_by_key(|n| n.0);
        self.badge.wanted = near.into_iter().map(|n| n.1).collect();
        pics.into_iter()
            .map(|p| {
                let url = p?;
                let slot = self.badge.got.get_mut(&url);
                Some(match slot {
                    Some(Pic::Ready(h)) => Some(h.clone()),
                    Some(slot @ Pic::Raw(_)) => {
                        let Pic::Raw((w, h, data)) = std::mem::replace(slot, Pic::Failed) else { unreachable!() };
                        let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
                        let handle = images.add(image);
                        *slot = Pic::Ready(handle.clone());
                        Some(handle)
                    }
                    _ => None,
                })
            })
            .collect()
    }
}

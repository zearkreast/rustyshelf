//! Details → Box art: every version of each part of the chosen game's box the sources have, the best for the
//! console's style first, with the highlighted one shown beside the list; Enter puts one in use.

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rustyshelf_core::art::{art_base, ArtLibrary};
use rustyshelf_core::artstudio::{self as studio, Cand, Ctx, GameArt, Sources, Studio};
use rustyshelf_core::{boxart, covers, gametdb, paths};

use super::*;

/// a version is downloaded for its preview once it has been highlighted this long (seconds)
const DWELL: f32 = 0.6;
/// the preview is shown this tall at most (pixels)
const PREVIEW: u32 = 720;

type Picture = (u32, u32, Vec<u8>);

/// What came of putting a picture in use: the game with its pictures as they are now, and whether anything moved.
type Placed = Result<(GameArt, bool), String>;

#[derive(Default)]
pub(super) struct ArtPick {
    game: Option<GameArt>,
    part: &'static str,
    /// the versions shown, the best first
    list: Vec<Cand>,
    listing: Option<Mutex<Receiver<Vec<Cand>>>>,
    /// a picture being put in use (or the last one put back), and the lock to keep when it has worked
    placing: Option<(Option<String>, Mutex<Receiver<Placed>>)>,
    /// the highlighted version, since when, and its picture: asked for, decoded, shown
    lit: (String, f32),
    reading: Option<(String, Mutex<Receiver<Option<Picture>>>)>,
    ready: Option<(String, Option<Picture>)>,
    shown: Option<(String, Handle<Image>)>,
    /// previews that couldn't be had
    failed: Vec<String>,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

fn part_name(part: &str) -> &'static str {
    match part {
        "back" => "Back",
        "spine" => "Spine",
        "disc" => "Disc or cartridge",
        _ => "Front",
    }
}

fn source_name(source: &str) -> &str {
    match source {
        "launchbox" => "LaunchBox",
        "libretro" => "libretro",
        "gametdb" => "GameTDB",
        "current" => "In use",
        "earlier" => "Replaced earlier",
        other => other,
    }
}

impl Settings {
    /// The chosen game as the Art Studio works on it, when it's a console's game.
    fn art_game(&self) -> Option<GameArt> {
        let c = self.chosen.as_ref().filter(|c| c.game.source == Source::Rom)?;
        let config = Config::from_value(&self.cfg)?;
        let console = systems().console(&c.console)?;
        let art = &c.game.art;
        Some(GameArt {
            sys: c.console.clone(),
            title: c.game.title.clone(),
            file: c.game.file.clone(),
            region: c.game.region.clone(),
            dir: ArtLibrary::from_config(&config, &self.app_root).dir_for(&config.system(console), &c.game.file, &self.app_root),
            base: art_base(&c.game.file),
            current: [art.front.clone(), art.back.clone().filter(|_| !art.made_back), art.spine.clone().filter(|_| !art.made_spine), art.disc.clone()],
        })
    }

    /// The row of a console's page that says which version of a box its games get.
    pub(super) fn art_style_row(&self, console: &str) -> Row {
        // (a style's preset and, for a region's, the region)
        const STYLES: [(&str, &str, &str); 6] = [("authentic", "", "Authentic: your copy's own box"), ("hd", "", "HD: the sharpest picture"), ("region", "usa", "USA boxes"), ("region", "eur", "European boxes"), ("region", "jpn", "Japanese boxes"), ("special", "", "Special editions")];
        let now = studio::style_for(&self.cfg, console);
        let at = STYLES.iter().position(|s| now["preset"] == s.0 && (s.1.is_empty() || now["region"] == s.1));
        let id = console.to_string();
        let turn = move |s: &mut Settings, d: i32| {
            let (preset, region, _) = STYLES[(at.map_or(0, |a| a as i32 + d)).rem_euclid(STYLES.len() as i32) as usize];
            let mut style = studio::style_for(&s.cfg, &id);
            style["preset"] = json!(preset);
            if !region.is_empty() {
                style["region"] = json!(region);
            }
            let base = rustyshelf_core::settings::default_art_style(s.cfg.get("ui"));
            s.set(&["art", "styles", &id], rustyshelf_core::settings::clean_art_style(&style, &base));
        };
        let again = turn.clone();
        Row {
            label: "Box art style".into(),
            value: at.map_or("Set in the Electron app", |a| STYLES[a].2).into(),
            hint: "Which version of a box the Art Studio puts first for this console's games (Details → Box art). The automatic art check still takes the first picture it finds.".into(),
            press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
            step: Some(Box::new(again)),
        }
    }

    pub(super) fn can_pick_art(&self) -> bool {
        self.art_game().is_some()
    }

    /// Opens a part's versions: the lists kept on this computer are read off the frame.
    fn open_art_part(&mut self, part: &'static str) {
        let Some(g) = self.art_game() else { return };
        let (data, style) = (paths::data_dir(&self.app_root), studio::style_for(&self.cfg, &g.sys));
        let (tx, rx) = channel();
        let game = g.clone();
        std::thread::spawn(move || {
            let cache = data.join("boxart-cache");
            let mut index = boxart::ready_index(&cache);
            let shard = covers::launchbox_platform(&game.sys).and_then(|plat| index.as_mut()?.platform(plat));
            let libretro = if part == "front" { boxart::libretro_names(&cache, &game.sys) } else { None };
            let tdb = if gametdb::has(&game.sys) && part != "disc" { gametdb::index(&cache, &game.sys) } else { None };
            let region = style["region"].as_str().unwrap_or("usa");
            let names = covers::lookup_names(&game.title);
            let wiki = |part: &str| rustyshelf_core::wikiart::find(&cache, &game.sys, &names, part, "usa", now_ms(), &rustyshelf_core::wikiart::ask);
            let from = Sources { launchbox: shard, libretro: libretro.as_ref(), gametdb: tdb.as_ref(), disc_id: covers::disc_id(&game.file), region, wiki: Some(&wiki) };
            let list = Studio::new(&data).list_for(&game, part, studio::candidates(&game.sys, &game.title, &game.file, part, &from), now_ms());
            // the best for the console's style first
            let shown: Vec<Cand> = studio::shown(&list).cloned().collect();
            let ctx = Ctx { style: &style, game_region: &game.region, sharp: None };
            let _ = tx.send(studio::rank_list(&shown, &studio::look_for(&style, part), &ctx).into_iter().cloned().collect());
        });
        self.art_pick = ArtPick { game: Some(g), part, listing: Some(Mutex::new(rx)), ..Default::default() };
        self.go(Screen::ArtPart);
    }

    /// Puts a version in use (`key`), or the picture before the one in use back (None).
    fn place_art(&mut self, key: Option<String>) {
        let chosen = key.as_ref().and_then(|k| self.art_pick.list.iter().find(|c| c.key == *k)).cloned();
        self.place_with(key, move |st, g, part| match &chosen {
            Some(c) => st.apply(g, part, c, now_ms()),
            None => st.undo(g, part, now_ms()).map(|()| true),
        });
    }

    /// Puts a picture of the player's own in use for the part that's open; `cut`: the spine cut from a picture
    /// of the whole box.
    pub(super) fn own_art(&mut self, file: &Path, cut: bool) {
        let file = file.to_path_buf();
        self.place_with(Some("own".into()), move |st, g, part| st.own(g, part, &file, cut, now_ms()).map(|()| true));
    }

    /// Changes the open part's picture off the frame; `key` is what the part is then locked as.
    fn place_with(&mut self, key: Option<String>, work: impl FnOnce(&Studio, &mut GameArt, &str) -> Result<bool, String> + Send + 'static) {
        let Some(mut g) = self.art_pick.game.clone().filter(|_| self.art_pick.placing.is_none()) else { return };
        let (part, data) = (self.art_pick.part, paths::data_dir(&self.app_root));
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let done = work(&Studio::new(&data), &mut g, part);
            let _ = tx.send(done.map(|moved| (g, moved)));
        });
        self.art_pick.placing = Some((key, Mutex::new(rx)));
        self.redraw = true;
    }

    /// What the lists, the previews and a picture being placed came to since the last frame.
    pub(super) fn poll_art(&mut self, now: f32) {
        let on_page = self.stack.last().is_some_and(|p| p.0 == Screen::ArtPart);
        if let Some(list) = self.art_pick.listing.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            (self.art_pick.listing, self.art_pick.list, self.redraw) = (None, list, true);
        }
        if let Some(done) = self.art_pick.placing.as_ref().and_then(|p| p.1.lock().unwrap().try_recv().ok()) {
            let (key, _) = self.art_pick.placing.take().expect("a picture being placed");
            let part = self.art_pick.part;
            match done {
                Ok((g, moved)) => {
                    // kept as chosen by hand: the automatic art check leaves this part alone
                    let mut lock = json!({ "look": "exact", "at": now_ms() });
                    if let Some(key) = &key {
                        lock["key"] = json!(key);
                    }
                    self.set(&["art", "locks", &studio::lock_key(&g.sys, &g.file), part], lock);
                    if let Some(c) = self.chosen.as_mut() {
                        let now = g.current[studio::PARTS.iter().position(|p| *p == part).unwrap_or(0)].clone();
                        match part {
                            "back" => (c.game.art.back, c.game.art.made_back) = (now, false),
                            "spine" => (c.game.art.spine, c.game.art.made_spine) = (now, false),
                            "disc" => c.game.art.disc = now,
                            _ => c.game.art.front = now,
                        }
                    }
                    self.say(match (moved, key.is_some()) {
                        (_, true) if key.as_deref() == Some("own") => "Your picture is in use now, and stays until you change it.".into(),
                        (false, _) => "That's the picture in use already.".into(),
                        (true, true) => "In use now. The picture it replaced is kept in the replaced folder beside it; the shelf shows the new one when Settings closes.".into(),
                        (true, false) => "The earlier picture is back.".into(),
                    });
                    if on_page {
                        self.back();
                        self.open_art_part(part);
                    }
                }
                Err(why) => self.say(why),
            }
        }
        if !on_page {
            return;
        }
        // the highlighted version's picture: at once from this computer, from the web after a moment on it
        let at = self.stack.last().map_or(0, |p| p.1);
        let Some(c) = self.art_pick.list.get(at).cloned() else { return };
        if self.art_pick.lit.0 != c.key {
            self.art_pick.lit = (c.key.clone(), now);
        }
        if let Some((key, picture)) = self.art_pick.reading.as_ref().and_then(|(key, rx)| Some((key.clone(), rx.lock().unwrap().try_recv().ok()?))) {
            self.art_pick.reading = None;
            if picture.is_none() {
                self.art_pick.failed.push(key.clone());
            }
            (self.art_pick.ready, self.redraw) = (Some((key, picture)), true);
        }
        let have = self.art_pick.shown.as_ref().is_some_and(|s| s.0 == c.key) || self.art_pick.ready.as_ref().is_some_and(|r| r.0 == c.key) || self.art_pick.failed.contains(&c.key);
        if have || self.art_pick.reading.is_some() {
            return;
        }
        let Some(g) = &self.art_pick.game else { return };
        let (data, sys) = (paths::data_dir(&self.app_root), g.sys.clone());
        let local = c.file.clone().or_else(|| Some(Studio::new(&data).cache_path(&c)).filter(|f| f.is_file()));
        if local.is_none() && now - self.art_pick.lit.1 < DWELL {
            return;
        }
        let (tx, rx) = channel();
        let key = c.key.clone();
        std::thread::spawn(move || {
            let file = local.or_else(|| Studio::new(&data).fetch_cand(&c, &sys, now_ms()).ok().flatten());
            let picture = file.and_then(|f| image::ImageReader::open(f).ok()?.with_guessed_format().ok()?.decode().ok()).map(|img| {
                let img = if img.height() > PREVIEW { img.resize(u32::MAX, PREVIEW, image::imageops::FilterType::Triangle) } else { img };
                let rgba = img.into_rgba8();
                (rgba.width(), rgba.height(), rgba.into_raw())
            });
            let _ = tx.send(picture);
        });
        self.art_pick.reading = Some((key, Mutex::new(rx)));
    }

    /// The picture to show beside the list: the highlighted version's, once it's there.
    pub(super) fn art_preview(&mut self, images: &mut Assets<Image>) -> Option<Handle<Image>> {
        if !self.stack.last().is_some_and(|p| p.0 == Screen::ArtPart) {
            if let Some((_, old)) = self.art_pick.shown.take() {
                images.remove(&old);
            }
            return None;
        }
        if let Some((key, picture)) = self.art_pick.ready.take() {
            if let Some((_, old)) = self.art_pick.shown.take() {
                images.remove(&old);
            }
            if let Some((w, h, data)) = picture {
                let image = Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
                self.art_pick.shown = Some((key, images.add(image)));
            }
        }
        let at = self.stack.last().map_or(0, |p| p.1);
        let lit = self.art_pick.list.get(at)?;
        self.art_pick.shown.as_ref().filter(|s| s.0 == lit.key).map(|s| s.1.clone())
    }

    pub(super) fn art_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let Some(g) = self.art_game() else { return Page { title, lead: String::new(), rows: Vec::new() } };
        let rows = studio::PARTS
            .into_iter()
            .zip(&g.current)
            .map(|(part, now)| Row {
                label: part_name(part).into(),
                value: now.as_ref().map_or("None yet".into(), |f| short(&f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                hint: "Every version of this part the box art sources have, to choose from".into(),
                press: Some(Box::new(move |s: &mut Settings| s.open_art_part(part))),
                step: None,
            })
            .collect();
        Page { title: format!("Box art · {title}"), lead: "A part chosen here stays as chosen: the automatic art check leaves it alone.".into(), rows }
    }

    pub(super) fn art_part_page(&self) -> Page {
        let pick = &self.art_pick;
        let title = format!("{} · {}", part_name(pick.part), pick.game.as_ref().map_or("", |g| g.title.as_str()));
        let mut rows: Vec<Row> = pick
            .list
            .iter()
            .map(|c| {
                let key = c.key.clone();
                let size = match (c.w, c.h) {
                    (Some(w), Some(h)) => format!(" · {w}×{h}"),
                    _ => String::new(),
                };
                Row {
                    label: if c.label.is_empty() { source_name(&c.source).to_string() } else { c.label.clone() },
                    value: format!("{}{size}{}", source_name(&c.source), if c.in_use && c.source != "current" { " · in use" } else { "" }),
                    hint: if c.in_use { "The picture in use.".into() } else { "Enter puts this one in use. The picture it replaces is kept in the replaced folder beside it.".into() },
                    press: Some(Box::new(move |s: &mut Settings| s.place_art(Some(key.clone())))),
                    step: None,
                }
            })
            .collect();
        let lead = if pick.placing.is_some() {
            "Putting it in place…".to_string()
        } else if pick.listing.is_some() {
            "Looking through the box art lists…".into()
        } else if rows.is_empty() {
            "No source has this part for this game. The box art lists come with the first box art download (Settings → Box art).".into()
        } else {
            format!("{} to choose from, the best for this console's style first.", rows.len())
        };
        if pick.listing.is_none() {
            let what = match pick.part {
                "back" => "back cover",
                "spine" => "spine",
                "disc" => "disc",
                _ => "front cover",
            };
            rows.push(Row {
                label: "A picture of yours…".into(),
                hint: format!("Choose a PNG, JPEG or WebP picture for the {what}. It's copied in; the file stays where it is."),
                press: Some(Box::new(move |s: &mut Settings| s.pick(Pick::OwnArt(false), &format!("Your own {what}"), false))),
                ..Row::default()
            });
            if pick.part == "spine" {
                rows.push(Row {
                    label: "Cut the spine from a picture of the whole box…".into(),
                    hint: "Choose a picture of the box opened flat (back, spine and front side by side): the spine is cut out of it.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.pick(Pick::OwnArt(true), "A picture of the whole box to cut the spine from", false))),
                    ..Row::default()
                });
            }
            rows.push(Row { label: "Put the earlier picture back".into(), hint: "The picture this part had before the one in use.".into(), press: Some(Box::new(|s: &mut Settings| s.place_art(None))), ..Row::default() });
        }
        Page { title, lead, rows }
    }
}

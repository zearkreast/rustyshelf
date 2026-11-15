//! Details → Box art: every version of each part of the chosen game's box the sources have, the best for the
//! console's style first, with the highlighted one shown beside the list; Enter puts one in use.

use bevy::asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rustyshelf_core::art::{art_base, ArtLibrary};
use rustyshelf_core::artjob::{self, Job, Remote, Set, SetDone};
use rustyshelf_core::artstudio::{self as studio, Cand, Ctx, GameArt, Studio};
use rustyshelf_core::paths;
use std::sync::atomic::Ordering;

use super::*;

/// a version is downloaded for its preview once it has been highlighted this long (seconds)
const DWELL: f32 = 0.6;
/// the preview is shown this tall at most (pixels)
const PREVIEW: u32 = 720;

type Picture = (u32, u32, Vec<u8>);

/// What came of putting a picture in use: the game with its pictures as they are now, and whether anything moved.
type Placed = Result<(GameArt, bool), String>;

/// What came of putting a set in (Some) or taking it out again (None): the game with its pictures as they are now.
type SetPlaced = Result<(GameArt, Option<(Set, SetDone)>), String>;

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
    /// the chosen game's complete sets, while they're gathered, and one being put in
    sets: Vec<Set>,
    sets_listing: Option<Mutex<Receiver<Vec<Set>>>>,
    set_placing: Option<Mutex<Receiver<SetPlaced>>>,
    /// the last set put in, to undo it: the game's lock key, what it changed, and its locks before
    last_set: Option<(String, SetDone, Value)>,
    /// the consoles being restyled, and how each run ends
    restyles: Vec<Restyle>,
    /// per console, every style its shelf may still partly show (a restyle stopped leaves some games in the one
    /// before)
    stale: std::collections::HashMap<String, Vec<Value>>,
}

struct Restyle {
    sys: String,
    job: std::sync::Arc<Job>,
    ended: Mutex<Receiver<String>>,
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
        // (a style's preset, its region, and whether it's the HD sets)
        const STYLES: [(&str, &str, bool, &str); 10] = [
            ("authentic", "", false, "Authentic: your copy's own box"),
            ("hd", "", false, "HD: the sharpest picture"),
            ("region", "usa", false, "USA boxes"),
            ("region", "eur", false, "European boxes"),
            ("region", "jpn", false, "Japanese boxes"),
            ("special", "", false, "Special editions"),
            ("sets", "usa", false, "Complete sets · USA"),
            ("sets", "eur", false, "Complete sets · Europe"),
            ("sets", "jpn", false, "Complete sets · Japan"),
            ("sets", "", true, "Complete sets · HD"),
        ];
        let now = studio::style_for(&self.cfg, console);
        let hd_sets = now["setsHd"] == true;
        let at = STYLES.iter().position(|s| now["preset"] == s.0 && (s.1.is_empty() || now["region"] == s.1) && (s.0 != "sets" || hd_sets == s.2));
        let id = console.to_string();
        let turn = move |s: &mut Settings, d: i32| {
            let (preset, region, hd, _) = STYLES[(at.map_or(0, |a| a as i32 + d)).rem_euclid(STYLES.len() as i32) as usize];
            let mut style = studio::style_for(&s.cfg, &id);
            style["preset"] = json!(preset);
            if !region.is_empty() {
                style["region"] = json!(region);
            }
            if preset == "sets" {
                style["setsHd"] = json!(hd);
            } else if let Some(o) = style.as_object_mut() {
                o.remove("setsHd");
            }
            let base = rustyshelf_core::settings::default_art_style(s.cfg.get("ui"));
            let before = studio::style_for(&s.cfg, &id);
            s.set(&["art", "styles", &id], rustyshelf_core::settings::clean_art_style(&style, &base));
            s.restyle(&id, before);
        };
        let again = turn.clone();
        let running = self.art_pick.restyles.iter().find(|r| r.sys == console).map(|r| (r.job.done.load(Ordering::Relaxed), r.job.total.load(Ordering::Relaxed)));
        let value = at.map_or("Set in the Electron app", |a| STYLES[a].3).to_string();
        Row {
            label: "Box art style".into(),
            value: match running {
                Some((done, total)) if total > 0 => format!("{value} · restyling {done} of {total}"),
                Some(_) => format!("{value} · restyling"),
                None => value,
            },
            hint: "Which version of a box this console's games get. Changing it picks their pictures again in the background (the old ones go to the replaced folder); a part you chose by hand gives way to it, a game you locked doesn't.".into(),
            press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
            step: Some(Box::new(again)),
        }
    }

    /// The console's pictures picked again for its style, now `before` has given way to it: what the player picked
    /// by hand for its games goes, and a run already going on it stops first.
    fn restyle(&mut self, sys: &str, before: Value) {
        let after = studio::style_for(&self.cfg, sys);
        let stale = self.art_pick.stale.entry(sys.to_string()).or_default();
        stale.push(before);
        let mut seen = std::collections::HashSet::new();
        stale.retain(|s| seen.insert(artjob::sig_key(s)));
        let bases: Vec<Value> = stale.iter().rev().take(6).rev().cloned().collect();
        *stale = bases.clone();
        let earlier: Vec<std::sync::Arc<Job>> = self.art_pick.restyles.iter().filter(|r| r.sys == sys).map(|r| r.job.clone()).collect();
        for j in &earlier {
            j.cancel.store(true, Ordering::Relaxed);
        }
        let cfg = self.cfg.clone();
        let mut cleared = self.cfg.clone();
        artjob::clear_picks(&mut cleared, sys);
        if cleared["art"]["locks"] != self.cfg["art"]["locks"] {
            self.set(&["art", "locks"], cleared["art"]["locks"].clone());
        }
        let (job, (tx, rx)) = (std::sync::Arc::new(Job::default()), channel());
        let (app_root, id, mine) = (self.app_root.clone(), sys.to_string(), job.clone());
        std::thread::spawn(move || {
            // (the run it replaces stops after the parts in flight: never two writing one game's pictures)
            while earlier.iter().any(|j| std::sync::Arc::strong_count(j) > 2) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            drop(earlier);
            let _ = tx.send(artjob::restyle(&app_root, &cfg, &id, &bases, &after, now_ms(), &mine));
        });
        self.art_pick.restyles.push(Restyle { sys: sys.to_string(), job, ended: Mutex::new(rx) });
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
            let region = style["region"].as_str().unwrap_or("usa");
            let remote = Remote::new(&data, &game.sys).cands(&game, part, region, true, now_ms());
            let list = Studio::new(&data).list_for(&game, part, remote, now_ms());
            // the best for the console's style first
            let shown: Vec<Cand> = studio::shown(&list).cloned().collect();
            let ctx = Ctx { style: &style, game_region: &game.region, sharp: None };
            let _ = tx.send(studio::rank_list(&shown, &studio::look_for(&style, part), &ctx).into_iter().cloned().collect());
        });
        self.art_pick = ArtPick { game: Some(g), part, listing: Some(Mutex::new(rx)), last_set: self.art_pick.last_set.take(), restyles: std::mem::take(&mut self.art_pick.restyles), stale: std::mem::take(&mut self.art_pick.stale), ..Default::default() };
        self.go(Screen::ArtPart);
    }

    /// Opens the chosen game's complete sets: the four parts' lists are read off the frame.
    fn open_art_sets(&mut self) {
        let Some(g) = self.art_game() else { return };
        let (data, style) = (paths::data_dir(&self.app_root), studio::style_for(&self.cfg, &g.sys));
        let (tx, rx) = channel();
        let game = g.clone();
        std::thread::spawn(move || {
            let (st, mut remote) = (Studio::new(&data), Remote::new(&data, &game.sys));
            let region = style["region"].as_str().unwrap_or("usa");
            let lists = studio::PARTS.map(|part| st.list_for(&game, part, remote.cands(&game, part, region, true, now_ms()), now_ms()));
            let _ = tx.send(artjob::build_sets(&style, &game.region, &lists));
        });
        let last = self.art_pick.last_set.take().filter(|l| l.0 == studio::lock_key(&g.sys, &g.file));
        self.art_pick = ArtPick { game: Some(g), sets_listing: Some(Mutex::new(rx)), last_set: last, restyles: std::mem::take(&mut self.art_pick.restyles), stale: std::mem::take(&mut self.art_pick.stale), ..Default::default() };
        self.go(Screen::ArtSets);
    }

    /// Puts a set in (Some, by its place in the list) or the last one out again (None), off the frame.
    fn place_set(&mut self, at: Option<usize>) {
        let Some(mut g) = self.art_pick.game.clone().filter(|_| self.art_pick.set_placing.is_none()) else { return };
        let data = paths::data_dir(&self.app_root);
        let work: Box<dyn FnOnce(&Studio, &mut GameArt) -> SetPlaced + Send> = match at {
            Some(i) => {
                let Some(set) = self.art_pick.sets.get(i).cloned() else { return };
                Box::new(move |st, g| st.apply_set(g, &set, now_ms()).map(|done| (g.clone(), Some((set, done)))))
            }
            None => {
                let Some((_, done, _)) = self.art_pick.last_set.clone() else { return };
                Box::new(move |st, g| st.revert_parts(g, &done, now_ms()).map(|()| (g.clone(), None)))
            }
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work(&Studio::new(&data), &mut g));
        });
        self.art_pick.set_placing = Some(Mutex::new(rx));
        self.redraw = true;
    }

    /// The chosen game's pictures as `g` has them now.
    fn art_now(&mut self, g: &GameArt) {
        if let Some(c) = self.chosen.as_mut() {
            let [front, back, spine, disc] = g.current.clone();
            (c.game.art.front, c.game.art.back, c.game.art.spine, c.game.art.disc) = (front, back, spine, disc);
            (c.game.art.made_back, c.game.art.made_spine) = (false, false);
        }
    }

    pub(super) fn art_sets_page(&self) -> Page {
        let pick = &self.art_pick;
        let title = format!("Complete sets · {}", pick.game.as_ref().map_or("", |g| g.title.as_str()));
        let mut rows: Vec<Row> = pick
            .sets
            .iter()
            .enumerate()
            .map(|(i, set)| Row {
                label: set.label.clone(),
                value: if set.same { "Back and spine from one scan".into() } else { String::new() },
                hint: format!("{}. Enter puts all four in; the pictures they replace are kept in the replaced folder.", studio::PARTS.iter().zip(&set.parts).map(|(p, c)| format!("{}: {}", part_name(p), if c.label.is_empty() { source_name(&c.source) } else { &c.label })).collect::<Vec<_>>().join(" · ")),
                press: Some(Box::new(move |s: &mut Settings| s.place_set(Some(i)))),
                step: None,
            })
            .collect();
        if pick.last_set.is_some() && pick.set_placing.is_none() {
            rows.push(Row { label: "Undo the set".into(), hint: "The four parts as they were before the set went in.".into(), press: Some(Box::new(|s: &mut Settings| s.place_set(None))), ..Row::default() });
        }
        let lead = if pick.set_placing.is_some() {
            "Downloading the set and putting it in…".to_string()
        } else if pick.sets_listing.is_some() {
            "Looking through the box art lists…".into()
        } else if pick.sets.is_empty() {
            "No source has a front, back, spine and disc that go together for this game.".into()
        } else {
            "A front, back, spine and disc from one region and source, so the case reads as one box.".into()
        };
        Page { title, lead, rows }
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
        let mut ended = Vec::new();
        for (i, r) in self.art_pick.restyles.iter().enumerate() {
            match r.ended.lock().unwrap().try_recv() {
                Ok(msg) => ended.push((i, Some(msg))),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => ended.push((i, None)),
                Err(_) => {}
            }
        }
        if !self.art_pick.restyles.is_empty() && self.stack.last().is_some_and(|p| matches!(&p.0, Screen::Console(_))) {
            self.redraw = true;
        }
        for (i, msg) in ended.into_iter().rev() {
            let r = self.art_pick.restyles.remove(i);
            let cancelled = r.job.cancel.load(Ordering::Relaxed);
            if !cancelled && !self.art_pick.restyles.iter().any(|x| x.sys == r.sys) {
                // the shelf is all in this style now
                self.art_pick.stale.remove(&r.sys);
            }
            if let Some(msg) = msg.filter(|_| !cancelled) {
                let name = rustyshelf_core::systems::systems().console(&r.sys).map_or(r.sys.clone(), |c| c.name.clone());
                self.say(format!("{name}: {msg}."));
                self.changed = true;
            }
        }
        if let Some(sets) = self.art_pick.sets_listing.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            (self.art_pick.sets_listing, self.art_pick.sets, self.redraw) = (None, sets, true);
        }
        if let Some(done) = self.art_pick.set_placing.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            self.art_pick.set_placing = None;
            match done {
                Ok((g, Some((set, done)))) => {
                    let key = studio::lock_key(&g.sys, &g.file);
                    let before = self.cfg["art"]["locks"][key.as_str()].clone();
                    for (p, c) in studio::PARTS.iter().zip(&set.parts) {
                        self.set(&["art", "locks", &key, p], json!({ "look": "exact", "key": c.key, "at": now_ms() }));
                    }
                    self.art_now(&g);
                    self.say(if done.changed.is_empty() { "That set is in use already.".into() } else { format!("{} is in use now; the shelf shows it when Settings closes.", set.label) });
                    self.art_pick.last_set = Some((key, done, before));
                    self.art_pick.game = Some(g);
                }
                Ok((g, None)) => {
                    // the parts' picks as they were; a game lock set since stays
                    if let Some((key, _, before)) = self.art_pick.last_set.take() {
                        for p in studio::PARTS {
                            match before.get(p) {
                                Some(was) => self.set(&["art", "locks", &key, p], was.clone()),
                                None => {
                                    if let Some(e) = self.cfg.get_mut("art").and_then(|a| a.get_mut("locks")).and_then(|l| l.get_mut(&key)).and_then(Value::as_object_mut) {
                                        e.remove(p);
                                    }
                                }
                            }
                        }
                        self.save();
                    }
                    self.art_now(&g);
                    self.say("The parts are as they were before the set.".into());
                    self.art_pick.game = Some(g);
                }
                Err(why) => self.say(why),
            }
            self.redraw = true;
        }
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
            .collect::<Vec<Row>>();
        let mut rows = rows;
        rows.push(Row { label: "Complete sets".into(), hint: "A front, back, spine and disc that belong together, from one region and source".into(), press: Some(Box::new(|s: &mut Settings| s.open_art_sets())), ..Row::default() });
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

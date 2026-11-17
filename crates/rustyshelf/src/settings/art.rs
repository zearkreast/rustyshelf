//! Details → Box art: every version of each part of the chosen game's box the sources have, the best for the
//! console's style first, with the highlighted one shown beside the list; Enter puts one in use.

use rustyshelf_core::art::{art_base, ArtLibrary};
use rustyshelf_core::artjob::{self, Ask, Job, Remote, Set, SetDone};
use rustyshelf_core::artstudio::{self as studio, Cand, Ctx, GameArt, Studio};
use rustyshelf_core::{js, looks, paths, pccovers};
use std::sync::atomic::Ordering;

use super::*;

/// how long the Art Studio waits for SteamGridDB and ScreenScraper before showing the versions it has
const LATE_WAIT: std::time::Duration = std::time::Duration::from_millis(1200);
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
    /// the lists as they come: the quick ones, then with the slow sources' versions (false: the last)
    listing: Option<Mutex<Receiver<(Vec<Cand>, bool)>>>,
    /// a slow source is still answering (the versions so far are shown)
    pending: bool,
    /// the sources not set up that could add versions of this part
    missing: Vec<&'static str>,
    /// LaunchBox's art database being downloaded: how far it is, and how it ends
    lb_get: Option<(std::sync::Arc<Mutex<String>>, Mutex<Receiver<Result<(), String>>>)>,
    /// the accounts as they were when one of them was opened from here, to look again on coming back
    acct_at: Option<String>,
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
    /// when a restyle's progress was last drawn
    shown_at: f32,
    /// per console, every style its shelf may still partly show (a restyle stopped leaves some games in the one
    /// before)
    stale: std::collections::HashMap<String, Vec<Value>>,
    /// the versions shown, by region, source, type and edition ("" for all): kept for the session, across games
    filter: [String; 4],
    /// an unlocked game's parts picked again for the shelf style: the game as it is then, and the pictures changed
    repick: Option<Mutex<Receiver<(GameArt, usize)>>>,
}

/// The versions' filters (settings.js FLT_DIMS): a name, and its values.
const FILTERS: [(&str, &[(&str, &str)]); 4] = [
    ("Region", &[("usa", "USA"), ("eur", "Europe"), ("jpn", "Japan")]),
    ("Source", &[("launchbox", "LaunchBox"), ("gametdb", "GameTDB"), ("screenscraper", "ScreenScraper"), ("steamgriddb", "SteamGridDB"), ("libretro", "libretro")]),
    ("Type", &[("scan", "Scans"), ("clean", "Clean"), ("hd", "HD")]),
    ("Edition", &[("original", "Original"), ("reprint", "Reprint"), ("special", "Special")]),
];

fn dim(c: &Cand, d: usize) -> &str {
    match d {
        0 => &c.region,
        1 => &c.source,
        2 => &c.kind,
        _ => &c.edition,
    }
}

/// A version found by a source (the picture in use and earlier ones always show).
fn found(c: &Cand) -> bool {
    c.source != "current" && c.source != "earlier"
}

fn passes(filter: &[String; 4], c: &Cand) -> bool {
    !found(c) || filter.iter().enumerate().all(|(d, f)| f.is_empty() || dim(c, d) == f)
}

struct Restyle {
    sys: String,
    job: std::sync::Arc<Job>,
    ended: Mutex<Receiver<String>>,
}

/// A part's name; the disc is named for what the console's games come on (a cartridge, a UMD, a floppy disk).
fn part_name(part: &str, sys: &str) -> &'static str {
    match part {
        "back" => "Back",
        "spine" => "Spine",
        "disc" => looks::media_word(sys),
        _ => "Front",
    }
}

fn source_name(source: &str) -> &str {
    match source {
        "launchbox" => "LaunchBox",
        "libretro" => "libretro",
        "gametdb" => "GameTDB",
        "steamgriddb" => "SteamGridDB",
        "screenscraper" => "ScreenScraper",
        "mariowiki" => "Super Mario Wiki",
        "current" => "In use",
        "earlier" => "Replaced earlier",
        other => other,
    }
}

impl Settings {
    /// The chosen game as the Art Studio works on it, when it's a console's game or a PC game on a console's shelf.
    /// The folder the chosen game's art (or its manual, `manual`) goes in and the name its files start with: a PC
    /// game's are kept in `pc-covers` under its id.
    pub(super) fn art_place(&self, manual: bool) -> Option<(PathBuf, String)> {
        let c = self.chosen.as_ref()?;
        let console = systems().console(&c.console)?;
        match c.game.source {
            Source::Rom => {
                let config = Config::from_value(&self.cfg)?;
                let lib = ArtLibrary::from_config(&config, &self.app_root);
                let sys = config.system(console);
                let dir = if manual { lib.manual_dir_for(&sys, &c.game.file, &self.app_root) } else { lib.dir_for(&sys, &c.game.file, &self.app_root) };
                Some((dir, art_base(&c.game.file)))
            }
            Source::Pc(_) => Some((paths::data_dir(&self.app_root).join("pc-covers"), pccovers::safe_id(&c.game.id))),
            _ => None,
        }
    }

    fn art_game(&self) -> Option<GameArt> {
        let (dir, base) = self.art_place(false)?;
        let c = self.chosen.as_ref()?;
        let art = &c.game.art;
        let current = [art.front.clone(), art.back.clone().filter(|_| !art.made_back), art.spine.clone().filter(|_| !art.made_spine), art.disc.clone()];
        let title = rustyshelf_core::covers::art_name(&self.cfg, &c.game.id).unwrap_or(&c.game.title).to_string();
        let pc = matches!(c.game.source, Source::Pc(_));
        Some(GameArt {
            sys: c.console.clone(),
            // the sources go by a PC game's title, not its program's name or contents
            file: if pc { PathBuf::from(title.replace(['/', '\\'], " ")) } else { c.game.file.clone() },
            title,
            region: if pc { String::new() } else { c.game.region.clone() },
            dir,
            base,
            current,
            pc,
        })
    }

    /// Keeps a part chosen here as it is: the automatic art check leaves it alone (it never looks at a PC game's).
    fn lock_part(&mut self, g: &GameArt, part: &str, lock: Value) {
        if g.pc {
            self.changed = true;
        } else {
            self.set(&["art", "locks", &studio::lock_key(&g.sys, &g.file), part], lock);
        }
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
            let _ = tx.send(artjob::restyle(&app_root, &cfg, &id, &bases, &after, rustyshelf_core::js::date_now(), &mine));
        });
        self.art_pick.restyles.push(Restyle { sys: sys.to_string(), job, ended: Mutex::new(rx) });
    }

    pub(super) fn can_pick_art(&self) -> bool {
        self.art_game().is_some()
    }

    /// Opens a part's versions: the lists are read off the frame.
    fn open_art_part(&mut self, part: &'static str) {
        let Some(g) = self.art_game() else { return };
        self.art_pick = ArtPick { game: Some(g), part, last_set: self.art_pick.last_set.take(), restyles: std::mem::take(&mut self.art_pick.restyles), stale: std::mem::take(&mut self.art_pick.stale), ..Default::default() };
        self.list_art_part();
        self.go(Screen::ArtPart);
    }

    /// Looks for the part's versions again. The lists kept on this computer answer first; SteamGridDB and
    /// ScreenScraper join if they answer within LATE_WAIT, else their versions are added when they come.
    fn list_art_part(&mut self) {
        let Some(g) = self.art_game() else { return };
        let part = self.art_pick.part;
        let (data, style) = (paths::data_dir(&self.app_root), studio::style_for(&self.cfg, &g.sys));
        let (tx, rx) = channel();
        let game = g.clone();
        let mut remote = Remote::new(&data, &g.sys);
        self.art_pick.missing = remote.missing(part);
        std::thread::spawn(move || {
            let (ask, region) = (Ask { wiki: true, posters: true, ss_cache_only: false }, style["region"].as_str().unwrap_or("usa").to_string());
            let slow = remote.take_slow();
            let (stx, srx) = channel();
            let late = game.clone();
            std::thread::spawn(move || {
                let _ = stx.send(slow.cands(&late, part, ask));
            });
            let quick = remote.quick(&game, part, &region, ask, rustyshelf_core::js::date_now());
            let ranked = |remote: Vec<Cand>| -> Vec<Cand> {
                let list = Studio::new(&data).list_for(&game, part, remote, rustyshelf_core::js::date_now());
                // the best for the console's style first
                let shown: Vec<Cand> = studio::shown(&list).cloned().collect();
                let ctx = Ctx { style: &style, game_region: &game.region, sharp: None };
                studio::rank_list(&shown, &studio::look_for(&style, part), &ctx).into_iter().cloned().collect()
            };
            let slow = match srx.recv_timeout(LATE_WAIT) {
                Ok(slow) => slow,
                Err(_) => {
                    if tx.send((ranked(quick.clone()), true)).is_err() {
                        return;
                    }
                    srx.recv().unwrap_or_default()
                }
            };
            let _ = tx.send((ranked(quick.into_iter().chain(slow).collect()), false));
        });
        (self.art_pick.listing, self.art_pick.pending) = (Some(Mutex::new(rx)), false);
        self.redraw = true;
    }

    /// The accounts the Art Studio's sources use, as one string, to tell whether one changed.
    fn art_accounts(&self) -> String {
        ["steamgriddb", "ssUser", "ssPass", "ssDevId", "ssDevPass"].map(|k| self.api_key(k)).join("\u{1}")
    }

    /// Accounts & services, opened from a source's row: on coming back, the part is looked through again if an
    /// account changed.
    fn art_account(&mut self) {
        self.art_pick.acct_at = Some(self.art_accounts());
        self.go(Screen::Accounts);
    }

    /// LaunchBox's art database, downloaded and read from its row; then the part is looked through again.
    fn get_launchbox(&mut self) {
        if self.art_pick.lb_get.is_some() {
            return;
        }
        if self.pc_backs.get.is_some() {
            self.say("LaunchBox's database is downloading already".into());
            return;
        }
        let cache = paths::boxart_cache(&paths::data_dir(&self.app_root));
        let (tx, rx) = channel();
        let stage = std::sync::Arc::new(Mutex::new("Starting the download…".to_string()));
        let at = stage.clone();
        std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&cache);
            let _ = tx.send(rustyshelf_core::boxart::launchbox_index(&cache, &mut |s| *at.lock().unwrap() = s.to_string()).map(|_| ()));
        });
        self.art_pick.lb_get = Some((stage, Mutex::new(rx)));
        self.say("LaunchBox art: downloading…".into());
    }

    pub(super) fn launchbox_downloading(&self) -> bool {
        self.art_pick.lb_get.is_some()
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
            let ask = Ask { wiki: true, posters: false, ss_cache_only: false };
            let lists = studio::PARTS.map(|part| st.list_for(&game, part, remote.cands(&game, part, region, ask, rustyshelf_core::js::date_now()), rustyshelf_core::js::date_now()));
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
                Box::new(move |st, g| st.apply_set(g, &set, rustyshelf_core::js::date_now()).map(|done| (g.clone(), Some((set, done)))))
            }
            None => {
                let Some((_, done, _)) = self.art_pick.last_set.clone() else { return };
                Box::new(move |st, g| st.revert_parts(g, &done, rustyshelf_core::js::date_now()).map(|()| (g.clone(), None)))
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
        let title = format!("Complete sets · {}", self.chosen.as_ref().map_or("", |c| c.game.title.as_str()));
        let sys = pick.game.as_ref().map_or("", |g| g.sys.as_str());
        let mut rows: Vec<Row> = pick
            .sets
            .iter()
            .enumerate()
            .map(|(i, set)| Row {
                label: set.label.clone(),
                value: if set.same { "Back and spine from one scan".into() } else { String::new() },
                hint: format!("{}. Enter puts all four in; the pictures they replace are kept in the replaced folder.", studio::PARTS.iter().zip(&set.parts).map(|(p, c)| format!("{}: {}", part_name(p, sys), if c.label.is_empty() { source_name(&c.source) } else { &c.label })).collect::<Vec<_>>().join(" · ")),
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
            Some(c) => st.apply(g, part, c, rustyshelf_core::js::date_now()),
            None => st.undo(g, part, rustyshelf_core::js::date_now()).map(|()| true),
        });
    }

    /// Puts a picture of the player's own in use for the part that's open; `cut`: the spine cut from a picture
    /// of the whole box.
    pub(super) fn own_art(&mut self, file: &Path, cut: bool) {
        let file = file.to_path_buf();
        self.place_with(Some("own".into()), move |st, g, part| st.own(g, part, &file, cut, rustyshelf_core::js::date_now()).map(|()| true));
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
        // the progress shows four times a second, not every frame
        if !self.art_pick.restyles.is_empty() && self.stack.last().is_some_and(|p| matches!(&p.0, Screen::Console(_))) && now - self.art_pick.shown_at >= 0.25 {
            (self.redraw, self.art_pick.shown_at) = (true, now);
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
        if let Some((g, changed)) = self.art_pick.repick.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            self.art_pick.repick = None;
            if let Some(c) = self.chosen.as_mut() {
                let [front, back, spine, disc] = g.current.clone();
                (c.game.art.front, c.game.art.disc) = (front, disc);
                if back.is_some() {
                    (c.game.art.back, c.game.art.made_back) = (back, false);
                }
                if spine.is_some() {
                    (c.game.art.spine, c.game.art.made_spine) = (spine, false);
                }
            }
            let title = self.chosen.as_ref().map_or(g.title.clone(), |c| c.game.title.clone());
            self.say(match changed {
                0 => format!("{title}: nothing changed"),
                n => format!("{title}: {n} picture{} changed", plural_s(n)),
            });
            self.changed |= changed > 0;
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
                        self.lock_part(&g, p, json!({ "look": "exact", "key": c.key, "at": rustyshelf_core::js::date_now() }));
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
        if let Some((list, more)) = self.art_pick.listing.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
            // the cursor stays on the version it was on
            let lit = self.stack.last().filter(|_| on_page).and_then(|p| self.art_versions().get(p.1).map(|c| c.key.clone()));
            (self.art_pick.list, self.art_pick.pending, self.redraw) = (list, more, true);
            if !more {
                self.art_pick.listing = None;
            }
            if let (Some(key), Some(top)) = (lit, self.stack.last_mut()) {
                if let Some(i) = self.art_pick.list.iter().filter(|c| passes(&self.art_pick.filter, c)).position(|c| c.key == key) {
                    top.1 = i;
                }
            }
        }
        if on_page {
            if let Some(was) = self.art_pick.acct_at.take() {
                if was != self.art_accounts() {
                    self.list_art_part();
                }
            }
        }
        if let Some((stage, rx)) = &self.art_pick.lb_get {
            let said = format!("LaunchBox art: {}", stage.lock().unwrap());
            let ended = rx.lock().unwrap().try_recv().ok();
            if self.note != said && ended.is_none() {
                self.say(said);
            }
            if let Some(ended) = ended {
                self.art_pick.lb_get = None;
                match ended {
                    Ok(()) => {
                        self.say("LaunchBox art is ready: looking for versions again".into());
                        if on_page {
                            self.list_art_part();
                        }
                    }
                    Err(why) => self.say(format!("LaunchBox art didn't download: {why}")),
                }
            }
        }
        if let Some(done) = self.art_pick.placing.as_ref().and_then(|p| p.1.lock().unwrap().try_recv().ok()) {
            let (key, _) = self.art_pick.placing.take().expect("a picture being placed");
            let part = self.art_pick.part;
            match done {
                Ok((g, moved)) => {
                    let to_style = key.as_deref() == Some("shelf");
                    // kept as chosen by hand: the automatic art check leaves this part alone
                    let mut lock = json!({ "look": "exact", "at": rustyshelf_core::js::date_now() });
                    if let Some(key) = &key {
                        lock["key"] = json!(key);
                    }
                    if !to_style {
                        self.lock_part(&g, part, lock);
                    }
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
                        _ if to_style => format!("{} follows the shelf style again.", part_name(part, &g.sys)),
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
        let Some(c) = self.art_versions().get(at).map(|c| (*c).clone()) else { return };
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
            let file = local.or_else(|| Studio::new(&data).fetch_cand(&c, &sys, rustyshelf_core::js::date_now()).ok().flatten());
            let picture = file.and_then(|f| crate::loading::decode(&f, PREVIEW));
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
                self.art_pick.shown = Some((key, images.add(crate::loading::to_image(w, h, data))));
            }
        }
        let at = self.stack.last().map_or(0, |p| p.1);
        let lit = self.art_versions().get(at)?.key.clone();
        self.art_pick.shown.as_ref().filter(|s| s.0 == lit).map(|s| s.1.clone())
    }

    pub(super) fn art_page(&self) -> Page {
        let title = self.chosen.as_ref().map_or(String::new(), |c| c.game.title.clone());
        let Some(g) = self.art_game() else { return Page { title, lead: String::new(), rows: Vec::new() } };
        let locks = &self.cfg["art"]["locks"][studio::lock_key(&g.sys, &g.file).as_str()];
        let rows = studio::PARTS
            .into_iter()
            .zip(&g.current)
            .map(|(part, now)| Row {
                label: part_name(part, &g.sys).into(),
                value: format!(
                    "{}{}",
                    now.as_ref().map_or("None yet".into(), |f| short(&f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())),
                    if js::truthy(locks.get(part)) { " · your pick" } else { "" }
                ),
                hint: "Every version of this part the box art sources have, to choose from".into(),
                press: Some(Box::new(move |s: &mut Settings| s.open_art_part(part))),
                step: None,
            })
            .collect::<Vec<Row>>();
        let mut rows = rows;
        rows.push(Row { label: "Complete sets".into(), hint: "A front, back, spine and disc that belong together, from one region and source".into(), press: Some(Box::new(|s: &mut Settings| s.open_art_sets())), ..Row::default() });
        let frozen = self.art_frozen(&g);
        rows.push(Row {
            label: "Lock this game".into(),
            value: if self.art_pick.repick.is_some() { "One moment…".into() } else { on_off(frozen) },
            hint: if frozen { "Every part stays as it is. Shelf styles and auto-fetch skip it." } else { "Off: a style change replaces picks too." }.into(),
            press: Some(Box::new(|s: &mut Settings| s.toggle_game_lock())),
            step: Some(Box::new(|s: &mut Settings, _| s.toggle_game_lock())),
        });
        Page { title: format!("Box art · {title}"), lead: "A part chosen here stays as chosen: the automatic art check leaves it alone.".into(), rows }
    }

    /// The open part's versions the filters let through.
    fn art_versions(&self) -> Vec<&Cand> {
        self.art_pick.list.iter().filter(|c| passes(&self.art_pick.filter, c)).collect()
    }

    /// A filter for each way the versions differ (two values or more), and any filter that's on.
    fn filter_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (d, (label, names)) in FILTERS.iter().enumerate() {
            let have: std::collections::HashSet<&str> = self.art_pick.list.iter().filter(|c| found(c)).map(|c| dim(c, d)).collect();
            let on = self.art_pick.filter[d].clone();
            let vals: Vec<&str> = names.iter().map(|v| v.0).filter(|v| have.contains(v) || *v == on).collect();
            if on.is_empty() && (vals.is_empty() || have.len() < 2) {
                continue;
            }
            let mut ring = vec![String::new()];
            ring.extend(vals.iter().map(|v| v.to_string()));
            let at = ring.iter().position(|v| *v == on).unwrap_or(0);
            let turn = move |s: &mut Settings, step: i32| {
                s.art_pick.filter[d] = ring[(at as i32 + step).rem_euclid(ring.len() as i32) as usize].clone();
                // the cursor stays on this filter as the versions above it come and go
                let row = s.page().rows.iter().position(|r| r.label == *label);
                if let (Some(row), Some(top)) = (row, s.stack.last_mut()) {
                    top.1 = row;
                }
                s.redraw = true;
            };
            let turn2 = turn.clone();
            rows.push(Row {
                label: label.to_string(),
                value: names.iter().find(|v| v.0 == on).map_or("All", |v| v.1).into(),
                hint: format!("Shows only the versions of one {}: ← → choose; All shows every one.", label.to_lowercase()),
                press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                step: Some(Box::new(turn2)),
            });
        }
        rows
    }

    /// The open part stops being the player's pick: the console's shelf style picks it again.
    fn art_to_style(&mut self) {
        let Some(g) = self.art_pick.game.clone().filter(|_| self.art_pick.placing.is_none()) else { return };
        let part = self.art_pick.part;
        let key = studio::lock_key(&g.sys, &g.file);
        let mut e = self.cfg["art"]["locks"][key.as_str()].clone();
        if let Some(o) = e.as_object_mut() {
            o.shift_remove(part);
        }
        self.set(&["art", "locks", &key], e);
        // the player handed this part to the style: a picture kept from before is the style's to replace now
        rustyshelf_core::provenance::adopt(&g.current[studio::PARTS.iter().position(|p| *p == part).unwrap_or(0)].iter().cloned().collect::<Vec<_>>());
        let style = studio::style_for(&self.cfg, &g.sys);
        let look = studio::look_for(&style, part);
        let data = paths::data_dir(&self.app_root);
        self.place_with(Some("shelf".into()), move |st, g, part| artjob::pick_by_look(st, &data, g, part, &look, &style));
    }

    fn art_frozen(&self, g: &GameArt) -> bool {
        js::truthy(self.cfg["art"]["locks"][studio::lock_key(&g.sys, &g.file).as_str()].get("all"))
    }

    /// Locks the chosen game as it is, or puts it back on the shelf style: unlocking picks again only the parts
    /// that followed the shelf and whose look the style changed since, or that are missing (settings.js toggleLock,
    /// artstudio.js lockGame).
    fn toggle_game_lock(&mut self) {
        let Some(g) = self.art_game().filter(|_| self.art_pick.repick.is_none()) else { return };
        let title = self.chosen.as_ref().map_or(g.title.clone(), |c| c.game.title.clone());
        let key = studio::lock_key(&g.sys, &g.file);
        let style = studio::style_for(&self.cfg, &g.sys);
        let mut e = self.cfg["art"]["locks"][key.as_str()].as_object().cloned().unwrap_or_default();
        if !js::truthy(e.get("all")) {
            let sig: serde_json::Map<String, Value> = studio::PARTS.iter().filter(|p| !js::truthy(e.get(**p))).map(|p| (p.to_string(), json!(artjob::look_sig(&style, p, &g.region)))).collect();
            e.insert("all".into(), json!({ "at": rustyshelf_core::js::date_now(), "sig": sig }));
            self.set(&["art", "locks", &key], Value::Object(e));
            self.say(format!("{title}: locked as it is"));
            return;
        }
        let sig = e.get("all").map_or(Value::Null, |a| a["sig"].clone());
        let kept = studio::PARTS.iter().filter(|p| js::truthy(e.get(**p))).count();
        let todo: Vec<&'static str> = studio::PARTS
            .iter()
            .enumerate()
            .filter(|(i, p)| !js::truthy(e.get(**p)) && (g.current[*i].as_ref().is_none_or(|f| !f.is_file()) || sig[**p].as_str().is_some_and(|s| s != artjob::look_sig(&style, p, &g.region))))
            .map(|(_, p)| *p)
            .collect();
        e.shift_remove("all");
        self.set(&["art", "locks", &key], Value::Object(e));
        let own = if kept > 0 { " · your picks stay until the shelf style changes" } else { "" };
        self.say(match todo.len() {
            0 if kept > 0 => format!("Unlocked{own}"),
            0 => "Unlocked · already matches the shelf style".into(),
            n => format!("Unlocked · re-picking {n} part{} for the shelf style{own}", plural_s(n)),
        });
        if todo.is_empty() {
            return;
        }
        let data = paths::data_dir(&self.app_root);
        let (tx, rx) = channel();
        let mut g = g;
        std::thread::spawn(move || {
            let st = Studio::new(&data);
            let changed = todo.iter().filter(|p| artjob::pick_by_look(&st, &data, &mut g, p, &studio::look_for(&style, p), &style).unwrap_or(false)).count();
            let _ = tx.send((g, changed));
        });
        self.art_pick.repick = Some(Mutex::new(rx));
    }

    pub(super) fn art_part_page(&self) -> Page {
        let pick = &self.art_pick;
        let sys = pick.game.as_ref().map_or("", |g| g.sys.as_str());
        let title = format!("{} · {}", part_name(pick.part, sys), self.chosen.as_ref().map_or("", |c| c.game.title.as_str()));
        let versions = self.art_versions();
        let mut rows: Vec<Row> = versions
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
        let n = rows.len();
        let filtered = pick.filter.iter().any(|f| !f.is_empty());
        let lead = if pick.placing.is_some() {
            "Putting it in place…".to_string()
        } else if pick.pending {
            if n == 0 { "Still looking…".to_string() } else { format!("{n} so far · still looking…") }
        } else if pick.listing.is_some() {
            "Looking through the box art lists…".into()
        } else if rows.is_empty() && !pick.missing.is_empty() {
            "No source set up here has this part for this game. Each source below can add versions.".into()
        } else if rows.is_empty() {
            "No source has this part for this game. The box art lists come with the first box art download (Settings → Box art).".into()
        } else if filtered {
            format!("{n} of {} versions · filtered", pick.list.len())
        } else {
            format!("{n} to choose from, the best for this console's style first.")
        };
        rows.extend(self.filter_rows());
        if let Some(g) = &pick.game {
            if js::truthy(self.cfg["art"]["locks"][studio::lock_key(&g.sys, &g.file).as_str()].get(pick.part)) {
                rows.push(Row {
                    label: "Back to the shelf style".into(),
                    hint: "This part stops being your pick and is picked again by the console's shelf style.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.art_to_style())),
                    ..Row::default()
                });
            }
        }
        for src in &pick.missing {
            rows.push(match *src {
                "LaunchBox" => Row {
                    label: "Get LaunchBox art".into(),
                    value: if pick.lb_get.is_some() { "Downloading…".into() } else { "About 110 MB".into() },
                    hint: "Fronts, backs, full boxes and discs for most consoles, from LaunchBox's free database. A one-time download of about 110 MB; then this game's versions show here.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.get_launchbox())),
                    step: None,
                },
                "ScreenScraper" => Row {
                    label: "Connect ScreenScraper".into(),
                    value: "Free account".into(),
                    hint: "Fronts, backs, spines, discs and full boxes for most consoles. Sign in with your free ScreenScraper account in Accounts, then you're back here.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.art_account())),
                    step: None,
                },
                _ => Row {
                    label: "Add SteamGridDB".into(),
                    value: "Free key".into(),
                    hint: "Front covers made by fans, many in HD. Add a free SteamGridDB key in Accounts, then you're back here.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.art_account())),
                    step: None,
                },
            });
        }
        if n == 0 && pick.listing.is_none() && !pick.missing.contains(&"LaunchBox") {
            rows.push(Row { label: "Try again".into(), hint: "Look for versions again.".into(), press: Some(Box::new(|s: &mut Settings| s.list_art_part())), ..Row::default() });
        }
        if pick.listing.is_none() || pick.pending {
            let what = match pick.part {
                "back" => "back cover".to_string(),
                "spine" => "spine".into(),
                "disc" => looks::media_word_lower(sys),
                _ => "front cover".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pc_game_on_a_console_s_shelf_has_the_art_studio_and_manuals() {
        let r = std::env::temp_dir().join(format!("rustyshelf-pcart-{}", std::process::id()));
        let mut s = Settings { wizard: false, stack: vec![(Screen::Details, 0)], app_root: r.clone(), ..Settings::default() };
        let pc = rustyshelf_core::pcgames::PcGame { id: "custom-zeta".into(), source: "custom", ..Default::default() };
        let game = Game { id: "custom-zeta".into(), title: "Zeta: Run/Again".into(), file: PathBuf::from("/games/zeta.exe"), source: Source::Pc(Box::new(pc)), ..Game::default() };
        s.chosen = Some(Chosen { console: "custom".into(), emulator: String::new(), game, plays: None });
        assert!(s.art_game().is_none() && !s.can_find_manual() && s.manual_game().is_none());
        s.chosen.as_mut().unwrap().console = "n64".into();
        let g = s.art_game().unwrap();
        let dir = paths::data_dir(&r).join("pc-covers");
        assert_eq!((g.sys.as_str(), g.file, &g.dir, g.base.as_str(), g.pc), ("n64", PathBuf::from("Zeta: Run Again"), &dir, "custom-zeta", true));
        assert!(s.can_find_manual());
        let m = s.manual_game().unwrap();
        assert_eq!((m.dir, m.base.as_str(), m.file.as_str()), (dir, "custom-zeta", "/games/zeta.exe"));
    }
}

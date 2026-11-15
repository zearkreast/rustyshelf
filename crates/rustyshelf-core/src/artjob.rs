// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/artstudio.js (pickBest and the restyle job), as RustyShelf has it.
//! A console's pictures picked again for its new Art Studio style: every game's parts whose look changed, were
//! picked by hand before, or are missing get the best version the new look ranks first.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde_json::Value;

use crate::art::{art_base, ArtLibrary};
use crate::artstudio::{self as studio, Cand, Ctx, GameArt, Sources, Studio, PARTS};
use crate::config::Config;
use crate::scanner::{self, ScanData};
use crate::{boxart, covers, gametdb, js, paths, systems};

/// What a part's look gives, as a string: two styles with the same one for a game give it the same picture.
pub fn look_sig(style: &Value, part: &str, game_region: &str) -> String {
    let l = studio::look_for(style, part);
    let sets = if style["preset"] == "sets" { "set:" } else { "" };
    let region = || studio::region_for_look(style, "authentic", game_region);
    match l.as_str() {
        "authentic" => format!("{sets}{l}|{}{}", region(), if js::truthy(style.get("sharper")) { "+" } else { "" }),
        "special" | "hd" => format!("{sets}{l}|{}", region()),
        _ => format!("{sets}{l}"),
    }
}

/// A style as one string, to tell two apart.
pub fn sig_key(style: &Value) -> String {
    serde_json::json!([style["preset"], style["region"], style["parts"], js::truthy(style.get("sharper")), js::truthy(style.get("regionFirst"))]).to_string()
}

/// The parts of a game a restyle picks again: one picked by hand (its lock goes), one whose look differs from what
/// any style it may still be in gives, and one it doesn't have.
pub fn parts_to_redo(bases: &[Value], after: &Value, lock: &Value, has: [bool; 4], game_region: &str) -> Vec<&'static str> {
    PARTS
        .iter()
        .enumerate()
        .filter(|(i, p)| js::truthy(lock.get(**p)) || bases.iter().any(|b| look_sig(b, p, game_region) != look_sig(after, p, game_region)) || !has[*i])
        .map(|(_, p)| *p)
        .collect()
}

/// The sources' lists for one console's games, read once; SteamGridDB and ScreenScraper with the user's own key
/// and account only.
pub struct Remote {
    cache: PathBuf,
    sys: String,
    index: Option<boxart::Index>,
    libretro: Option<covers::LibretroNames>,
    tdb: Option<serde_json::Map<String, Value>>,
    sg: Option<crate::steamgrid::SteamGrid>,
    ss: Option<crate::screenscraper::ScreenScraper>,
}

/// Which of the slower sources a list asks.
#[derive(Debug, Clone, Copy)]
pub struct Ask {
    /// the Super Mario Wiki (Switch 2)
    pub wiki: bool,
    /// SteamGridDB's posters for the front (only HD may pick one)
    pub posters: bool,
    /// ScreenScraper from its cache only: a whole console's restyle never spends the account's daily lookups
    pub ss_cache_only: bool,
}

impl Remote {
    pub fn new(data_dir: &Path, sys: &str) -> Remote {
        let cache = data_dir.join("boxart-cache");
        Remote {
            index: boxart::ready_index(&cache),
            libretro: boxart::libretro_names(&cache, sys),
            tdb: if gametdb::has(sys) { gametdb::index(&cache, sys) } else { None },
            sg: crate::steamgrid::SteamGrid::from_settings(data_dir),
            ss: crate::screenscraper::ScreenScraper::from_settings(data_dir),
            sys: sys.into(),
            cache,
        }
    }

    /// Every version the sources have of a part of this game.
    pub fn cands(&mut self, g: &GameArt, part: &str, region: &str, ask: Ask, now: f64) -> Vec<Cand> {
        let mut out = self.listed(g, part, region, ask.wiki, now);
        if let (Some(sg), true, "front") = (&self.sg, ask.posters, part) {
            let names = covers::lookup_names(&g.title);
            for p in sg.posters(&crate::steamgrid::Game { appid: "", title: &g.title, names: &names }) {
                let label = ["SteamGridDB", p.style_name(), &if p.author.is_empty() { String::new() } else { format!("by {}", p.author) }].into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" · ");
                let hd = p.w.max(p.h) >= 900;
                out.push(Cand { url: p.url, ext: p.ext.into(), source: "steamgriddb".into(), kind_of: "grid".into(), edition: "original".into(), kind: if hd { "hd" } else { "clean" }.into(), w: Some(p.w), h: Some(p.h), rank: p.rank as f64, label, ..Cand::default() });
            }
        }
        if let Some(ss) = &self.ss {
            let file = g.file.to_string_lossy();
            for c in ss.candidates(&g.sys, &crate::screenscraper::Rom { file: &file, title: &g.title, size_bytes: 0 }, part, ask.ss_cache_only, true) {
                out.push(Cand { url: c.url, ext: c.ext, cut: c.cut.map(String::from), src: c.src, source: "screenscraper".into(), kind_of: c.kind, region: c.region.into(), edition: "original".into(), kind: "scan".into(), label: c.label, ..Cand::default() });
            }
        }
        out
    }

    fn listed(&mut self, g: &GameArt, part: &str, region: &str, wiki: bool, now: f64) -> Vec<Cand> {
        let shard = covers::launchbox_platform(&self.sys).and_then(|plat| self.index.as_mut()?.platform(plat));
        let names = covers::lookup_names(&g.title);
        let ask = |p: &str| crate::wikiart::find(&self.cache, &g.sys, &names, p, "usa", now, &crate::wikiart::ask);
        let from = Sources {
            launchbox: shard,
            libretro: self.libretro.as_ref().filter(|_| part == "front"),
            gametdb: self.tdb.as_ref().filter(|_| part != "disc"),
            disc_id: covers::disc_id(&g.file),
            region,
            wiki: if wiki { Some(&ask) } else { None },
        };
        studio::candidates(&g.sys, &g.title, &g.file, part, &from)
    }
}

impl Studio {
    /// The best version a look can have of a part, downloaded: the candidate and its file. Only HD may take a
    /// SteamGridDB poster (the other looks must be a real release's box); for HD the likeliest few are downloaded
    /// first and ranked again on their real sizes. Also how many downloads the web didn't answer.
    pub fn pick_best(&self, g: &GameArt, list: &[Cand], look: &str, style: &Value, now: f64, stop: &dyn Fn() -> bool) -> (Option<(Cand, PathBuf)>, usize) {
        let mut net_err = 0;
        let best = self.pick(g, list, look, style, now, stop, &mut net_err);
        (best, net_err)
    }

    #[allow(clippy::too_many_arguments)]
    fn pick(&self, g: &GameArt, list: &[Cand], look: &str, style: &Value, now: f64, stop: &dyn Fn() -> bool, net_err: &mut usize) -> Option<(Cand, PathBuf)> {
        let ctx = Ctx { style, game_region: &g.region, sharp: Some(look == "hd" || (js::truthy(style.get("sharper")) && look == "authentic")) };
        let usable: Vec<Cand> = list.iter().filter(|c| c.source != "earlier" && !(c.dup.is_some() && c.source == "current") && (look == "hd" || c.source != "steamgriddb")).cloned().collect();
        let mut order: Vec<Cand> = studio::rank_list(&usable, look, &ctx).into_iter().cloned().collect();
        let mut failed: Vec<String> = Vec::new();
        if look == "hd" {
            // (SteamGridDB says its posters' sizes)
            let probe: Vec<usize> = order.iter().enumerate().filter(|(_, c)| c.file.is_none() && !c.cached && c.source != "steamgriddb").map(|(i, _)| i).take(4).collect();
            for i in probe {
                if stop() {
                    return None;
                }
                match self.fetch_cand(&order[i], &g.sys, now) {
                    Ok(Some(f)) => {
                        let size = boxart::size_of(&f);
                        let c = &mut order[i];
                        (c.w, c.h, c.cached) = (size.map(|z| z.0), size.map(|z| z.1), true);
                    }
                    Ok(None) => failed.push(order[i].key.clone()),
                    Err(_) => *net_err += 1,
                }
            }
            order.retain(|c| !failed.contains(&c.key));
            order = studio::rank_list(&order, look, &ctx).into_iter().cloned().collect();
        }
        for c in order {
            if stop() {
                return None;
            }
            if c.source == "current" {
                return c.file.clone().map(|f| (c, f));
            }
            match self.fetch_cand(&c, &g.sys, now) {
                Ok(Some(f)) => return Some((c, f)),
                Ok(None) => {}
                Err(_) => *net_err += 1,
            }
        }
        None
    }
}

/// A restyle's progress, read by the screen while it runs.
#[derive(Default)]
pub struct Job {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

/// How a restyle ended, as the screen says it.
fn summary(changed: usize, total: usize, not_found: usize, failed: usize, net_err: usize, locked: usize, offline: bool) -> String {
    if offline {
        return "Couldn't reach the art sites. Check the internet connection and try again.".into();
    }
    let s = |n: usize| if n == 1 { "" } else { "s" };
    let extra: Vec<String> = [(not_found, format!("{not_found} not found")), (failed, format!("{failed} failed")), (net_err, format!("{net_err} connection error{}", s(net_err)))].into_iter().filter(|x| x.0 > 0).map(|x| x.1).collect();
    let mut msg = if changed > 0 {
        format!("{changed} picture{} changed", s(changed))
    } else if total == 0 {
        "Nothing to change".into()
    } else if !extra.is_empty() {
        "Nothing changed".into()
    } else {
        "Nothing needed changing".into()
    };
    if !extra.is_empty() {
        msg += &format!(" · {}", extra.join(" · "));
    }
    if locked > 0 {
        msg += &format!(" · {locked} locked game{} skipped", s(locked));
    }
    msg
}

/// Whether a part was locked after `t0` (a pick made while the restyle runs wins over it), or the whole game is.
fn locked_now(data_dir: &Path, key: &str, part: &str, t0: f64) -> bool {
    let cfg: Value = std::fs::read(data_dir.join("config.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let e = &cfg["art"]["locks"][key];
    js::truthy(e.get("all")) || e.get(part).is_some_and(|l| js::number(l.get("at")) > t0)
}

/// Picks a console's pictures again for its new style `after`. `cfg`: the settings as they were when the style
/// changed (with the hand-made picks the change clears); `bases`: every style the shelf may still partly show.
/// Returns what to tell the player. A picture is replaced, never deleted: the old one goes to `replaced`.
pub fn restyle(app_root: &Path, cfg: &Value, sys: &str, bases: &[Value], after: &Value, t0: f64, job: &Job) -> String {
    let data_dir = paths::data_dir(app_root);
    let (Some(config), Some(console)) = (Config::from_value(cfg), systems::systems().console(sys)) else { return "Nothing to change".into() };
    let sc = config.system(console);
    let lib = ArtLibrary::from_config(&config, app_root);
    let games = if sc.rom_dirs.is_empty() { Vec::new() } else { scanner::scan(console, &sc, &lib.dirs_for(&sc, app_root), &ScanData::load(&data_dir)) };
    let mut locked = 0;
    // Complete sets: every unlocked game gets its set (a locked one counts as skipped)
    let sets_mode = after["preset"] == "sets";
    let mut work: Vec<(GameArt, Vec<&'static str>)> = Vec::new();
    for game in games.iter().filter(|g| g.source == crate::scanner::Source::Rom) {
        let key = studio::lock_key(sys, &game.file);
        let lock = &cfg["art"]["locks"][key.as_str()];
        if js::truthy(lock.get("all")) {
            locked += 1;
            continue;
        }
        let art = &game.art;
        let current = [art.front.clone(), art.back.clone().filter(|_| !art.made_back), art.spine.clone().filter(|_| !art.made_spine), art.disc.clone()];
        let parts = if sets_mode { PARTS.to_vec() } else { parts_to_redo(bases, after, lock, current.clone().map(|c| c.is_some()), &game.region) };
        if !parts.is_empty() {
            let g = GameArt { sys: sys.into(), title: game.title.clone(), file: game.file.clone(), region: game.region.clone(), dir: lib.dir_for(&sc, &game.file, app_root), base: art_base(&game.file), current };
            work.push((g, parts));
        }
    }
    job.total.store(work.len(), Ordering::Relaxed);
    let remote = Mutex::new(Remote::new(&data_dir, sys));
    let st = Studio::new(&data_dir);
    let next = AtomicUsize::new(0);
    let (changed, not_found, failed, net_err, ok_net) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0));
    let (sets_full, sets_partial, sets_skipped) = (AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(locked));
    let offline = AtomicBool::new(false);
    let stop = || job.cancel.load(Ordering::Relaxed) || offline.load(Ordering::Relaxed);
    let now = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    // two games at a time
    std::thread::scope(|s| {
        for _ in 0..work.len().min(2) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= work.len() || stop() {
                    break;
                }
                // nothing reachable: stop instead of trying every game
                if net_err.load(Ordering::Relaxed) >= 6 && ok_net.load(Ordering::Relaxed) == 0 {
                    offline.store(true, Ordering::Relaxed);
                    break;
                }
                let (mut g, parts) = (work[i].0.clone(), &work[i].1);
                let key = studio::lock_key(sys, &g.file);
                if sets_mode {
                    // a pick made while the job runs keeps the game as it is
                    if PARTS.iter().any(|p| locked_now(&data_dir, &key, p, t0)) {
                        sets_skipped.fetch_add(1, Ordering::Relaxed);
                        job.done.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let region = after["region"].as_str().unwrap_or("usa");
                    let ask = Ask { wiki: true, posters: false, ss_cache_only: true };
                    let lists = PARTS.map(|p| st.list_for(&g, p, remote.lock().unwrap().cands(&g, p, region, ask, now()), now()));
                    let want_hd = js::truthy(after.get("setsHd"));
                    let set = build_sets(after, &g.region, &lists).into_iter().find(|x| if want_hd { x.hd } else { x.region == region && !x.hd });
                    if let Some(Ok(done)) = set.map(|set| st.apply_set(&mut g, &set, now())) {
                        changed.fetch_add(done.changed.len(), Ordering::Relaxed);
                        ok_net.fetch_add(1, Ordering::Relaxed);
                        sets_full.fetch_add(1, Ordering::Relaxed);
                        job.done.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // no complete set: the style's look for each part, as a normal restyle
                    sets_partial.fetch_add(1, Ordering::Relaxed);
                }
                for part in parts {
                    if stop() || locked_now(&data_dir, &key, part, t0) {
                        continue;
                    }
                    let look = studio::look_for(after, part);
                    let region = studio::region_for_look(after, &look, &g.region);
                    let ask = Ask { wiki: true, posters: look == "hd", ss_cache_only: true };
                    let list = st.list_for(&g, part, remote.lock().unwrap().cands(&g, part, &region, ask, now()), now());
                    let (best, errs) = st.pick_best(&g, &list, &look, after, now(), &stop);
                    net_err.fetch_add(errs, Ordering::Relaxed);
                    let Some((c, _)) = best else {
                        not_found.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    ok_net.fetch_add(1, Ordering::Relaxed);
                    if stop() || locked_now(&data_dir, &key, part, t0) {
                        continue;
                    }
                    match st.apply(&mut g, part, &c, now()) {
                        Ok(true) => {
                            changed.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(false) => {}
                        Err(_) => {
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                job.done.fetch_add(1, Ordering::Relaxed);
            });
        }
    });
    if job.cancel.load(Ordering::Relaxed) && !offline.load(Ordering::Relaxed) {
        return format!("Stopped · {} picture{} changed", changed.load(Ordering::Relaxed), if changed.load(Ordering::Relaxed) == 1 { "" } else { "s" });
    }
    if sets_mode && !offline.load(Ordering::Relaxed) {
        let s = |n: usize| if n == 1 { "" } else { "s" };
        let full = sets_full.into_inner();
        return format!("{full} full set{} · {} partly · {} skipped (locked)", s(full), sets_partial.into_inner(), sets_skipped.into_inner());
    }
    summary(changed.into_inner(), work.len(), not_found.into_inner(), failed.into_inner(), net_err.into_inner(), locked, offline.into_inner())
}

/// The settings with the hand-made picks of a console's games cleared (a style change replaces them; a locked game
/// keeps everything).
pub fn clear_picks(cfg: &mut Value, sys: &str) {
    let pre = format!("{sys}|");
    let Some(locks) = cfg.get_mut("art").and_then(|a| a.get_mut("locks")).and_then(Value::as_object_mut) else { return };
    for (_, e) in locks.iter_mut().filter(|(k, _)| k.starts_with(&pre)) {
        if js::truthy(e.get("all")) {
            continue;
        }
        if let Some(e) = e.as_object_mut() {
            for p in PARTS {
                e.shift_remove(p);
            }
        }
    }
    locks.retain(|_, e| e.as_object().is_none_or(|o| !o.is_empty()));
}

// ---------- complete sets: a front, back, spine and disc that belong together ----------

/// One set: every part from one region and source family (or the best of each source for a region, or the HD set).
#[derive(Debug, Clone, PartialEq)]
pub struct Set {
    pub id: String,
    pub label: String,
    pub region: String,
    /// "launchbox", "gametdb", "screenscraper" or "mixed"
    pub source: String,
    pub hd: bool,
    /// the back and the spine cut from the same full scan: they read as one box
    pub same: bool,
    pub parts: [Cand; 4],
}

/// A real release's picture that can be had: not the one in use, an earlier one, a SteamGridDB poster, fan art,
/// or a guess not yet seen.
fn set_usable(c: &Cand) -> bool {
    !matches!(c.source.as_str(), "current" | "earlier" | "steamgriddb") && c.dup.is_none() && !c.fan && !(c.guess && !c.cached)
}

fn region_ok(c: &Cand, r: &str, strict: bool) -> bool {
    c.region == r || (!strict && (c.region.is_empty() || c.region == "world"))
}

/// The complete sets the four parts' lists make, sources' own families first and back-and-spine-from-one-scan first
/// among them; the region of the game's own copy first.
pub fn build_sets(style: &Value, game_region: &str, lists: &[Vec<Cand>; 4]) -> Vec<Set> {
    const SOURCES: [(&str, &str); 3] = [("launchbox", "LaunchBox scans"), ("gametdb", "GameTDB"), ("screenscraper", "ScreenScraper scans")];
    let ctx = Ctx { style, game_region, sharp: Some(true) };
    let mut out: Vec<Set> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |id: String, label: String, region: &str, source: &str, hd: bool, look: &str, pred: &dyn Fn(&Cand, &str) -> bool| {
        let mut parts: Vec<Cand> = Vec::new();
        for (i, p) in PARTS.iter().enumerate() {
            let mut pool: Vec<Cand> = lists[i].iter().filter(|c| set_usable(c) && pred(c, p)).cloned().collect();
            // the spine from the same full scan as the back (one wrap, cut twice), when there is one
            if let (&"spine", Some(back)) = (p, parts.get(1).filter(|b| b.cut.is_some())) {
                let same: Vec<Cand> = pool.iter().filter(|c| c.cut.is_some() && c.url == back.url).cloned().collect();
                if !same.is_empty() {
                    pool = same;
                }
            }
            let Some(best) = studio::rank_list(&pool, look, &ctx).first().map(|c| (*c).clone()) else { return };
            parts.push(best);
        }
        // (the HD set stays even when a region set has the same four: it's what the HD shelf style asks for)
        if !seen.insert(parts.iter().map(|c| c.key.as_str()).collect::<Vec<_>>().join("|")) && !hd {
            return;
        }
        let same = parts[1].cut.is_some() && parts[2].cut.is_some() && parts[1].url == parts[2].url;
        let parts: [Cand; 4] = parts.try_into().expect("four parts");
        out.push(Set { id, label: format!("{label} · complete"), region: region.into(), source: source.into(), hd, same, parts });
    };
    let own = studio::game_region(game_region);
    let mut regions = ["usa", "eur", "jpn"];
    regions.sort_by_key(|r| *r != own);
    for r in regions {
        let name = match r {
            "usa" => "USA",
            "eur" => "Europe",
            _ => "Japan",
        };
        let look = format!("region:{r}");
        for (src, src_name) in SOURCES {
            add(format!("{r}-{src}"), format!("{name} · {src_name}"), r, src, false, &look, &|c: &Cand, p: &str| c.source == src && region_ok(c, r, p != "disc"));
        }
        add(format!("{r}-mixed"), format!("{name} · best of each source"), r, "mixed", false, &look, &|c: &Cand, p: &str| region_ok(c, r, p != "disc"));
    }
    add("hd".into(), "HD".into(), "", "mixed", true, "hd", &|_: &Cand, _: &str| true);
    out.sort_by_key(|s| (s.source == "mixed", !s.same));
    out
}

/// What a set put in, to undo it: the parts it changed and which of them had a picture before.
#[derive(Debug, Clone, Default)]
pub struct SetDone {
    pub changed: Vec<&'static str>,
    pub had: [bool; 4],
}

impl Studio {
    /// Puts a whole set in use: every picture downloaded first (nothing changes if one can't be had), then each
    /// part (old pictures to `replaced`); a part that fails to go in puts the ones already in back.
    pub fn apply_set(&self, g: &mut GameArt, set: &Set, now: f64) -> Result<SetDone, String> {
        let mut srcs = Vec::new();
        let mut miss = Vec::new();
        for (i, p) in PARTS.iter().enumerate() {
            match self.fetch_cand(&set.parts[i], &g.sys, now) {
                Ok(Some(f)) => srcs.push(f),
                _ => miss.push(*p),
            }
        }
        if !miss.is_empty() {
            return Err(format!("The {} of that set couldn't be downloaded. Nothing was changed.", miss.join(" and ")));
        }
        let had = g.current.clone().map(|c| c.is_some_and(|f| f.is_file()));
        let mut done = SetDone { changed: Vec::new(), had };
        for (i, p) in PARTS.iter().enumerate() {
            match self.apply(g, p, &set.parts[i], now) {
                Ok(true) => done.changed.push(p),
                Ok(false) => {}
                Err(e) => {
                    let _ = self.revert_parts(g, &done, now);
                    return Err(format!("That set couldn't be put in ({e}). Nothing was changed."));
                }
            }
        }
        Ok(done)
    }

    /// The parts a set changed, as they were before it: the newest picture in `replaced` back for a part that had
    /// one; for a part that had none, the set's picture moves to `replaced`.
    pub fn revert_parts(&self, g: &mut GameArt, done: &SetDone, now: f64) -> Result<(), String> {
        for p in &done.changed {
            let i = PARTS.iter().position(|x| x == p).unwrap_or(0);
            if done.had[i] {
                self.undo(g, p, now)?;
                continue;
            }
            let Some(cur) = g.current[i].clone().filter(|f| f.is_file()) else { continue };
            let dir = cur.parent().map_or_else(|| g.dir.clone(), Path::to_path_buf).join("replaced");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let ext = cur.extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
            let stamp = boxart::art_stamp();
            let mut to = dir.join(format!("{}{} ({stamp}){ext}", g.base, studio::suffix(p)));
            let mut n = 2;
            // never over an earlier one
            while to.symlink_metadata().is_ok() {
                to = dir.join(format!("{}{} ({stamp}-{n}){ext}", g.base, studio::suffix(p)));
                n += 1;
            }
            std::fs::rename(&cur, &to).map_err(|e| e.to_string())?;
            g.current[i] = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn style(v: Value) -> Value {
        crate::settings::clean_art_style(&v, &crate::settings::default_art_style(None))
    }

    #[test]
    fn a_part_is_picked_again_when_its_look_changes_or_it_was_picked_by_hand() {
        let (authentic, hd, eur) = (style(json!({ "preset": "authentic" })), style(json!({ "preset": "hd" })), style(json!({ "preset": "region", "region": "eur" })));
        assert_eq!(look_sig(&authentic, "front", "USA"), "authentic|usa+");
        assert_eq!(look_sig(&authentic, "front", "Japan"), "authentic|jpn+");
        assert_eq!(look_sig(&hd, "back", ""), "hd|usa");
        assert_eq!(look_sig(&eur, "spine", "USA"), "region:eur");
        assert_eq!(look_sig(&style(json!({ "preset": "sets", "region": "jpn" })), "front", "USA"), "set:region:jpn");
        assert_eq!(look_sig(&style(json!({ "preset": "special" })), "disc", "Europe"), "special|eur");
        let all = [true; 4];
        assert!(parts_to_redo(std::slice::from_ref(&authentic), &authentic, &Value::Null, all, "USA").is_empty());
        assert_eq!(parts_to_redo(std::slice::from_ref(&authentic), &authentic, &json!({ "back": { "look": "exact" } }), [true, true, false, true], "USA"), ["back", "spine"]);
        assert_eq!(parts_to_redo(std::slice::from_ref(&authentic), &hd, &Value::Null, all, "USA"), PARTS);
        // a style the shelf may still partly show counts too
        assert_eq!(parts_to_redo(&[eur, hd.clone()], &hd, &Value::Null, all, "USA").len(), 4);
        assert_ne!(sig_key(&authentic), sig_key(&hd));
    }

    #[test]
    fn a_style_change_clears_the_picks_but_not_a_locked_game() {
        let mut cfg = json!({ "art": { "locks": { "gc|/a.iso": { "front": { "look": "exact" } }, "gc|/b.iso": { "all": true, "front": { "look": "exact" } }, "gc|/c.iso": { "back": { "look": "hd" }, "note": 1 }, "wii|/d.iso": { "front": { "look": "exact" } } } } });
        clear_picks(&mut cfg, "gc");
        assert_eq!(cfg["art"]["locks"], json!({ "gc|/b.iso": { "all": true, "front": { "look": "exact" } }, "gc|/c.iso": { "note": 1 }, "wii|/d.iso": { "front": { "look": "exact" } } }));
    }

    #[test]
    fn a_set_goes_in_whole_and_comes_out_again() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, art, src) = (tmp.path().join("data"), tmp.path().join("covers"), tmp.path().join("src"));
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&art).unwrap();
        let pic = |name: &str, shade: u8| {
            let f = src.join(name);
            image::RgbImage::from_pixel(60, 80, image::Rgb([shade, 0, 0])).save(&f).unwrap();
            f
        };
        let old_front = art.join("Zeta Run (USA).png");
        std::fs::copy(pic("old.png", 9), &old_front).unwrap();
        let cand = |f: PathBuf| Cand { key: f.to_string_lossy().into(), source: "launchbox".into(), ext: ".png".into(), file: Some(f), ..Cand::default() };
        let set = Set { id: "usa-launchbox".into(), label: "USA".into(), region: "usa".into(), source: "launchbox".into(), hd: false, same: false, parts: [cand(pic("f.png", 1)), cand(pic("b.png", 2)), cand(pic("s.png", 3)), cand(pic("d.png", 4))] };
        let mut g = GameArt { sys: "gc".into(), title: "Zeta Run".into(), file: tmp.path().join("Zeta Run (USA).iso"), region: "USA".into(), dir: art.clone(), base: "Zeta Run (USA)".into(), current: [Some(old_front.clone()), None, None, None] };
        let st = Studio::new(&data);
        let done = st.apply_set(&mut g, &set, 1.0).unwrap();
        assert_eq!((done.changed.len(), done.had), (4, [true, false, false, false]));
        assert!(g.current.iter().all(|c| c.as_ref().is_some_and(|f| f.is_file())));
        st.revert_parts(&mut g, &done, 2.0).unwrap();
        assert_eq!(std::fs::read(&old_front).unwrap(), std::fs::read(src.join("old.png")).unwrap());
        assert!(g.current[1..].iter().all(Option::is_none));
        // nothing is lost: the set's pictures are in replaced
        assert_eq!(std::fs::read_dir(art.join("replaced")).unwrap().count(), 4);
        let missing = Set { parts: [cand(src.join("gone.png")), set.parts[1].clone(), set.parts[2].clone(), set.parts[3].clone()], ..set };
        assert!(st.apply_set(&mut g, &missing, 3.0).unwrap_err().starts_with("The front of that set"));
    }

    #[test]
    fn a_restyle_says_what_it_did() {
        assert_eq!(summary(3, 5, 1, 0, 2, 1, false), "3 pictures changed · 1 not found · 2 connection errors · 1 locked game skipped");
        assert_eq!(summary(0, 0, 0, 0, 0, 0, false), "Nothing to change");
        assert_eq!(summary(0, 4, 0, 0, 0, 0, false), "Nothing needed changing");
        assert_eq!(summary(0, 4, 4, 0, 0, 0, false), "Nothing changed · 4 not found");
        assert!(summary(1, 1, 0, 0, 0, 0, true).starts_with("Couldn't reach"));
    }
}

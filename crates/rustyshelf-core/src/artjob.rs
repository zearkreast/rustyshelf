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

/// The sources' lists for one console's games, read once.
pub struct Remote {
    cache: PathBuf,
    sys: String,
    index: Option<boxart::Index>,
    libretro: Option<covers::LibretroNames>,
    tdb: Option<serde_json::Map<String, Value>>,
}

impl Remote {
    pub fn new(data_dir: &Path, sys: &str) -> Remote {
        let cache = data_dir.join("boxart-cache");
        Remote { index: boxart::ready_index(&cache), libretro: boxart::libretro_names(&cache, sys), tdb: if gametdb::has(sys) { gametdb::index(&cache, sys) } else { None }, sys: sys.into(), cache }
    }

    /// Every version the sources have of a part of this game (the Super Mario Wiki asked when `wiki`).
    pub fn cands(&mut self, g: &GameArt, part: &str, region: &str, wiki: bool, now: f64) -> Vec<Cand> {
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
            let probe: Vec<usize> = order.iter().enumerate().filter(|(_, c)| c.file.is_none() && !c.cached).map(|(i, _)| i).take(4).collect();
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
        let parts = parts_to_redo(bases, after, lock, current.clone().map(|c| c.is_some()), &game.region);
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
                for part in parts {
                    if stop() || locked_now(&data_dir, &key, part, t0) {
                        continue;
                    }
                    let look = studio::look_for(after, part);
                    let region = studio::region_for_look(after, &look, &g.region);
                    let list = st.list_for(&g, part, remote.lock().unwrap().cands(&g, part, &region, true, now()), now());
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
        assert_eq!(parts_to_redo(&[authentic.clone()], &hd, &Value::Null, all, "USA"), PARTS);
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
    fn a_restyle_says_what_it_did() {
        assert_eq!(summary(3, 5, 1, 0, 2, 1, false), "3 pictures changed · 1 not found · 2 connection errors · 1 locked game skipped");
        assert_eq!(summary(0, 0, 0, 0, 0, 0, false), "Nothing to change");
        assert_eq!(summary(0, 4, 0, 0, 0, 0, false), "Nothing needed changing");
        assert_eq!(summary(0, 4, 4, 0, 0, 0, false), "Nothing changed · 4 not found");
        assert!(summary(1, 1, 0, 0, 0, 0, true).starts_with("Couldn't reach"));
    }
}

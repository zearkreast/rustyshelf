// Ported from MicaelTek's boxshelf-source (https://github.com/MicaelTek/boxshelf-source, GPL-3.0), the pc-bulk-covers
// branch's main/pcbulk.js.
//! Settings → PC → Covers: every PC game's best front cover at once. For each game the candidates (Steam's cover on
//! this PC, Steam's store cover, SteamGridDB's posters with a key) are ranked without fetching a thumbnail, and the
//! best one that downloads becomes its chosen cover. A few games at a time; it can be stopped. The covers it
//! replaced are kept in `data/pc-covers/.undo` until the next run, so Undo puts them back.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::pccovers;

/// One game to find a cover for.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Wanted {
    pub id: String,
    /// a Steam game's app
    pub appid: Option<String>,
    pub title: String,
    /// it has a front cover already (from any source)
    pub has_front: bool,
}

/// How far a run is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    /// the game being looked at
    pub title: String,
}

const WORKERS: usize = 4;

fn undo_dir(data: &Path) -> PathBuf {
    data.join("pc-covers").join(".undo")
}

fn norm(t: &str) -> String {
    t.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect()
}

/// A candidate's rank (higher is better) by where it comes from, its place in its source's list and its name: Steam's
/// own cover first, a SteamGridDB poster close behind (a blurred or logo-less one well behind), another game of a
/// similar name on Steam's store last.
pub fn score(source: &str, idx: usize, label: &str, exact_name: bool) -> f64 {
    match source {
        "steamstore" if exact_name => 98.0,
        "steamstore" => 52.0,
        "steamgriddb" => {
            let low = label.to_lowercase();
            let mut s = 97.0 - idx as f64 * 0.35;
            if ["blur", "no logo", "no_logo", "textless"].iter().any(|w| low.contains(w)) {
                s -= 12.0;
            }
            if ["official", "alternate", "material"].iter().any(|w| low.contains(w)) {
                s += 1.5;
            }
            s
        }
        "steamown" => 96.0,
        "steam" => 92.0,
        _ => 60.0 - idx as f64 * 0.3,
    }
}

/// A game's candidates, best first: each a rank and the addresses (or the file) to take it from.
fn candidates(data: &Path, g: &Wanted, sg: Option<&crate::steamgrid::SteamGrid>) -> Vec<(f64, Vec<String>)> {
    let mut out: Vec<(f64, Vec<String>)> = Vec::new();
    if let Some(a) = &g.appid {
        let local = data.join("steam-art").join(format!("{a}_cover.jpg"));
        if local.is_file() {
            out.push((score("steam", 0, "", false), vec![local.display().to_string()]));
        }
        out.push((score("steamown", 0, "", false), [crate::steamart::asset(a, "library_600x900_2x.jpg"), crate::steamart::asset(a, "library_600x900.jpg")].concat()));
    } else {
        let want = norm(&g.title);
        for (a, name) in crate::pcgames::steam_search(&g.title, true).into_iter().take(3) {
            let exact = !want.is_empty() && (name.is_empty() || norm(&name) == want);
            out.push((score("steamstore", 0, "", exact), [crate::steamart::asset(&a, "library_600x900_2x.jpg"), crate::steamart::asset(&a, "library_600x900.jpg")].concat()));
        }
    }
    if let Some(sg) = sg.filter(|s| s.enabled()) {
        let names = [g.title.clone()];
        let posters = sg.posters(&crate::steamgrid::Game { appid: g.appid.as_deref().unwrap_or(""), title: &g.title, names: &names });
        for (i, p) in posters.iter().take(12).enumerate() {
            out.push((score("steamgriddb", i, &format!("{} {}", p.style, p.style_name()), false), vec![p.url.clone()]));
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out
}

/// The best candidate that comes, made the game's chosen cover; the one it replaces kept for Undo first.
/// Some(true) when it changed, Some(false) when there was nothing to take, None when it failed.
fn one(data: &Path, g: &Wanted, sg: Option<&crate::steamgrid::SteamGrid>, kept: &Mutex<Vec<Value>>) -> Option<bool> {
    let cands = candidates(data, g, sg);
    if cands.is_empty() {
        return Some(false);
    }
    let got = cands.iter().find_map(|(_, from)| {
        from.iter().find_map(|f| if f.starts_with("https://") { pccovers::download(f).map(|b| (b, f.clone())) } else { std::fs::read(f).ok().map(|b| (b, f.clone())) })
    });
    let Some((bytes, from)) = got else { return None };
    let before = pccovers::cover(data, &g.id);
    if before.as_ref().and_then(|b| std::fs::read(b).ok()).is_some_and(|b| b == bytes) {
        return Some(false);
    }
    let bak = before.as_ref().and_then(|b| {
        let dir = undo_dir(data);
        let ext = b.extension().map_or("jpg".to_string(), |e| e.to_string_lossy().into_owned());
        let to = dir.join(format!("{}.{ext}", pccovers::safe_id(&g.id)));
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::copy(b, &to)).ok().map(|_| to)
    });
    pccovers::keep(data, &g.id, &bytes, &from).ok()?;
    kept.lock().unwrap().push(json!({ "id": g.id, "bak": bak.map(|b| b.display().to_string()) }));
    Some(true)
}

/// Finds the best cover for each game (`all`: every one; else only those without a front cover), `WORKERS` at a time,
/// until `stop` is set. `seen` hears the progress after each game. What it replaced can be put back with `undo`.
pub fn run(data: &Path, games: &[Wanted], all: bool, sg: Option<&crate::steamgrid::SteamGrid>, stop: &AtomicBool, seen: &(dyn Fn(&Progress) + Sync)) -> Progress {
    let list: Vec<&Wanted> = games.iter().filter(|g| all || (!g.has_front && pccovers::cover(data, &g.id).is_none())).collect();
    let p = Mutex::new(Progress { total: list.len(), ..Progress::default() });
    seen(&p.lock().unwrap());
    if list.is_empty() {
        return p.into_inner().unwrap();
    }
    let _ = std::fs::remove_dir_all(undo_dir(data));
    let kept = Mutex::new(Vec::new());
    crate::pool::each(&list, WORKERS, |_, g| {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        p.lock().unwrap().title = g.title.clone();
        let r = one(data, g, sg, &kept);
        let mut now = p.lock().unwrap();
        match r {
            Some(true) => now.updated += 1,
            Some(false) => now.skipped += 1,
            None => now.failed += 1,
        }
        now.done += 1;
        seen(&now);
        true
    });
    let kept = kept.into_inner().unwrap();
    if !kept.is_empty() {
        let _ = std::fs::create_dir_all(undo_dir(data)).and_then(|()| crate::files::write_atomic(&undo_dir(data).join("meta.json"), json!({ "at": crate::now_ms(), "entries": kept }).to_string()));
    }
    let mut out = p.into_inner().unwrap();
    out.title.clear();
    out
}

/// How many covers the last run replaced, that Undo can put back.
pub fn can_undo(data: &Path) -> usize {
    crate::files::read_json(&undo_dir(data).join("meta.json")).and_then(|m| m["entries"].as_array().map(Vec::len)).unwrap_or(0)
}

/// Puts back what the last run replaced: each game's cover from before, or none (its automatic one) where it had
/// none. How many games it put back.
pub fn undo(data: &Path) -> usize {
    let Some(meta) = crate::files::read_json(&undo_dir(data).join("meta.json")) else { return 0 };
    let mut n = 0;
    for e in meta["entries"].as_array().into_iter().flatten() {
        let Some(id) = e["id"].as_str() else { continue };
        pccovers::clear(data, id);
        if let Some(bak) = e["bak"].as_str().map(PathBuf::from).filter(|b| b.is_file()) {
            if pccovers::own(data, id, &bak).is_err() {
                continue;
            }
        }
        n += 1;
    }
    let _ = std::fs::remove_dir_all(undo_dir(data));
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_s_own_cover_ranks_first_and_a_blurred_poster_last() {
        assert!(score("steamstore", 0, "", true) > score("steamgriddb", 0, "", false));
        assert!(score("steamgriddb", 0, "", false) > score("steamown", 0, "", false));
        assert!(score("steamgriddb", 0, "", false) > score("steamgriddb", 5, "", false));
        assert!(score("steamgriddb", 0, "blurred Blurred", false) < score("steam", 0, "", false));
        assert!(score("steamstore", 0, "", false) < score("steamgriddb", 20, "", false));
    }

    #[test]
    fn undo_puts_back_the_cover_a_run_replaced() {
        let data = std::env::temp_dir().join(format!("rs-pcbulk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let before = data.join("before.png");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(&before, [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
        pccovers::own(&data, "custom-zeta", &before).unwrap();
        // a run replaced it (as `one` does): the old one kept, the new one chosen
        let bak = undo_dir(&data).join("custom-zeta.png");
        std::fs::create_dir_all(undo_dir(&data)).unwrap();
        std::fs::copy(pccovers::cover(&data, "custom-zeta").unwrap(), &bak).unwrap();
        pccovers::keep(&data, "custom-zeta", &[0xFF, 0xD8, 0xFF, 9], "x.jpg").unwrap();
        crate::files::write_atomic(&undo_dir(&data).join("meta.json"), json!({ "entries": [{ "id": "custom-zeta", "bak": bak.display().to_string() }, { "id": "custom-new", "bak": null }] }).to_string()).unwrap();
        assert_eq!(can_undo(&data), 2);
        assert_eq!(undo(&data), 2);
        let now = pccovers::cover(&data, "custom-zeta").unwrap();
        assert_eq!(std::fs::read(now).unwrap(), [0x89, b'P', b'N', b'G', 1, 2, 3]);
        assert_eq!(can_undo(&data), 0);
        let _ = std::fs::remove_dir_all(&data);
    }
}

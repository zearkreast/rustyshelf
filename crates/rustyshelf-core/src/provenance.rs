// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/artstudio.js (its provenance part), as RustyShelf has it.
//! Which box art pictures RustyShelf wrote: `data/art-provenance.json`, shared with the Electron app.
//! `{ v, at, files: { "<path lower-case>": [origin, bytes, mtime] }, base: { "<path>": [bytes, mtime] } }`: `files` is
//! every picture placed by either app, `base` the pictures that were already there at the first look (`at`), whose
//! origin nothing on disk tells. A picture in neither was put there by hand. A record matches a file only while its
//! size and date are the same: one the user overwrote by hand is theirs again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

// (main/artstudio.js PROV_V: keep in sync)
const V: u64 = 1;
const ORIGINS: [&str; 7] = ["auto", "studio", "own", "restored", "adopted", "copied", "poster"];

struct State {
    file: Option<PathBuf>,
    loaded: Option<Map<String, Value>>,
    saving: bool,
    pruned: u64,
}

static STATE: Mutex<State> = Mutex::new(State { file: None, loaded: None, saving: false, pruned: 0 });

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The record is kept in this data folder from now on.
pub fn init(data_dir: &Path) {
    let file = data_dir.join("art-provenance.json");
    let mut s = lock();
    if s.file.as_ref() != Some(&file) {
        s.file = Some(file);
        s.loaded = None;
    }
}

fn read(file: Option<&Path>) -> Map<String, Value> {
    let fresh = || json!({ "v": V, "at": 0, "files": {}, "base": {} }).as_object().cloned().unwrap_or_default();
    let Some(j) = file.and_then(|f| std::fs::read(f).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { return fresh() };
    if j["v"].as_u64() != Some(V) || !j["files"].is_object() || !j["base"].is_object() {
        return fresh();
    }
    let at = crate::js::number(j.get("at"));
    json!({ "v": V, "at": if at.is_nan() { json!(0) } else { crate::js::num(at) }, "files": j["files"], "base": j["base"] }).as_object().cloned().unwrap_or_default()
}

fn with<R>(f: impl FnOnce(&mut Map<String, Value>) -> R) -> R {
    let mut s = lock();
    if s.loaded.is_none() {
        s.loaded = Some(read(s.file.as_deref()));
    }
    f(s.loaded.as_mut().expect("loaded"))
}

fn key(file: &Path) -> String {
    std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()).to_string_lossy().to_lowercase()
}

/// A file's size and date (ms), as the record keeps them.
fn stat_of(file: &Path) -> Option<(u64, f64)> {
    let m = std::fs::metadata(file).ok().filter(std::fs::Metadata::is_file)?;
    let at = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64() * 1000.0;
    Some((m.len(), at.round()))
}

/// FAT and network drives keep 2 s dates.
fn same_stat(bytes: &Value, at: &Value, st: (u64, f64)) -> bool {
    bytes.as_u64() == Some(st.0) && at.as_f64().is_some_and(|a| (a - st.1).abs() <= 2000.0)
}

/// Saved a moment later: a burst of pictures is one write.
fn save_soon() {
    let mut s = lock();
    if s.saving || s.file.is_none() {
        return;
    }
    s.saving = true;
    drop(s);
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(1500));
        flush();
    });
}

/// Writes the record now.
pub fn flush() {
    let mut s = lock();
    s.saving = false;
    let (Some(file), pruned) = (s.file.clone(), s.pruned) else { return };
    let Some(p) = s.loaded.as_mut() else { return };
    let n = p["files"].as_object().map_or(0, Map::len) + p["base"].as_object().map_or(0, Map::len);
    // pictures long gone (moved to replaced, games removed): a check of every file, so at most every 6 h
    let prune = n > 80_000 && now_ms().saturating_sub(pruned) > 6 * 3_600_000;
    if prune {
        for m in ["files", "base"] {
            if let Some(m) = p.get_mut(m).and_then(Value::as_object_mut) {
                m.retain(|k, _| Path::new(k).is_file());
            }
        }
    }
    let text = Value::Object(p.clone()).to_string();
    if prune {
        s.pruned = now_ms();
    }
    drop(s);
    let tmp = PathBuf::from(format!("{}.tmp", file.display()));
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &file);
    }
}

/// This picture is on record with this origin ("auto" for one RustyShelf downloaded, "own" for the user's pick…).
pub fn record(file: &Path, origin: &str) {
    let Some(st) = stat_of(file) else { return };
    let origin = if ORIGINS.contains(&origin) { origin } else { "auto" };
    let k = key(file);
    with(|p| {
        if let Some(files) = p.get_mut("files").and_then(Value::as_object_mut) {
            files.insert(k.clone(), json!([origin, st.0, crate::js::num(st.1)]));
        }
        if let Some(base) = p.get_mut("base").and_then(Value::as_object_mut) {
            base.shift_remove(&k);
        }
    });
    save_soon();
}

/// "rustyshelf" (RustyShelf wrote it, or the user let a style manage it), "own" (a picture the user chose through
/// RustyShelf), "legacy" (there at the first look, origin unknown), "hand" (put there by hand since), or None (no file).
pub fn origin_of(file: &Path) -> Option<&'static str> {
    let st = stat_of(file)?;
    let k = key(file);
    Some(with(|p| {
        if let Some(r) = p["files"].get(&k).filter(|r| same_stat(&r[1], &r[2], st)) {
            return if r[0] == "own" { "own" } else { "rustyshelf" };
        }
        if p["base"].get(&k).is_some_and(|b| same_stat(&b[0], &b[1], st)) {
            "legacy"
        } else {
            "hand"
        }
    }))
}

/// May automatic work (an HD upgrade) replace this file? Only one RustyShelf wrote; `explicit` (a download the user
/// started, which says it replaces low-res art) also pictures from before the first look.
pub fn may_replace(file: &Path, explicit: bool) -> bool {
    let o = origin_of(file);
    o == Some("rustyshelf") || (explicit && o == Some("legacy"))
}

/// A front taken from SteamGridDB because no box scan existed: the next check replaces it with a real scan.
pub fn is_poster(file: &Path) -> bool {
    let Some(st) = stat_of(file) else { return false };
    let k = key(file);
    with(|p| p["files"].get(&k).is_some_and(|r| r[0] == "poster" && same_stat(&r[1], &r[2], st)))
}

/// The art library's copy of a picture beside the games is what its original was.
pub fn copied(src: &Path, dest: &Path) {
    match origin_of(src) {
        Some("own") => record(dest, "own"),
        Some("rustyshelf") => record(dest, "copied"),
        _ => {}
    }
}

/// The user let the shelf style manage these: from now on they're RustyShelf's. How many changed.
pub fn adopt(files: &[PathBuf]) -> usize {
    let mut n = 0;
    for f in files {
        match origin_of(f) {
            None | Some("rustyshelf") => {}
            Some(o) => {
                record(f, if o == "own" { "own" } else { "adopted" });
                n += 1;
            }
        }
    }
    n
}

/// Whether the first look was made.
pub fn looked() -> bool {
    with(|p| crate::js::number(p.get("at")) > 0.0)
}

/// The first look: every picture on a shelf goes into `base`, except the ones with the very bytes of a picture the
/// Art Studio downloaded into `cand_dir` (those are RustyShelf's for sure). Returns (legacy, RustyShelf's).
pub fn first_look(art: &[PathBuf], cand_dir: &Path, now: u64) -> (usize, usize) {
    if looked() {
        return (0, 0);
    }
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    for e in std::fs::read_dir(cand_dir).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if let Some(st) = stat_of(&e.path()) {
            by_size.entry(st.0).or_default().push(e.path());
        }
    }
    let (mut legacy, mut mine) = (0, 0);
    let mut found: Vec<(String, (u64, f64), bool)> = Vec::new();
    for f in art {
        let k = key(f);
        if with(|p| p["files"].get(&k).is_some()) || found.iter().any(|x| x.0 == k) {
            continue;
        }
        let Some(st) = stat_of(f) else { continue };
        let same = by_size.get(&st.0).is_some_and(|twins| std::fs::read(f).is_ok_and(|b| twins.iter().any(|t| std::fs::read(t).is_ok_and(|c| c == b))));
        if same {
            mine += 1;
        } else {
            legacy += 1;
        }
        found.push((k, st, same));
    }
    with(|p| {
        for (k, st, same) in found {
            let (m, v) = if same { ("files", json!(["studio", st.0, crate::js::num(st.1)])) } else { ("base", json!([st.0, crate::js::num(st.1)])) };
            if let Some(m) = p.get_mut(m).and_then(Value::as_object_mut) {
                m.insert(k, v);
            }
        }
        p.insert("at".into(), json!(now));
    });
    flush();
    (legacy, mine)
}

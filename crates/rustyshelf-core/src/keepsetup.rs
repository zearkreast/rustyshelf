// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/keepsetup.js (the safety copy, Undo, changes put in at the next start, and bringing a setup over).
//! Keeping your setup. The first start of a new version copies your setup first (`data\backups\<version>\`, the
//! newest three kept) and checks it afterwards: a missing or damaged config.json is put back at once. Settings →
//! System → Undo puts the copy back at the next start. A new RustyShelf folder can bring the setup over from the one
//! used before (`installs.json` in the user's settings folder says where each RustyShelf has run from). Changes for
//! the next start wait in `data\.pending\` (data\… and app\…), with ready.json written last so a half-made one is
//! never put in. Box art beside your games is never touched.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::files::write_atomic;

/// What's made by you: in every safety copy, and brought over.
pub const STATE_FILES: [&str; 10] = ["config.json", "history.json", "art-provenance.json", "manual-studio.json", "manual-auto.json", "manual-rejects.json", "emu-setup.json", "romm-local.json", "romm-device.json", "companion.json"];
/// Your sound packs' and custom themes' own files: in the safety copy.
const DEF_DIRS: [(&str, &str); 2] = [("sounds", "pack.json"), ("Custom themes", "theme.json")];
/// Brought over whole from another folder.
const BRING_APP_DIRS: [&str; 3] = ["sounds", "Custom themes", "themes"];
const BRING_DATA_DIRS: [&str; 1] = ["pc-covers"];
/// The downloads that can come along too ("Also copy downloaded extras").
const EXTRA_DATA: [&str; 40] = [
    "thumbs", "previews", "more-shots", "boxart-cache", "art-cand", "manual-cand", "steam-art", "stremio-art", "pc-cover-cache", "commercials", "trophy-packs", "custom-theme-cache", "wiiu-manuals", "ra", "icons", "ach-icons",
    "logo-find", "romm-covers", "library", "wiimote-cache", "tools", "steam", "cache", "library-cache.json", "cover-checks.json", "cover-backs.json", "steam-info.json", "x360-titles.json", "sgdb-cache.json",
    "ss-cache.json", "arcade-names.json", "commercials.json", "manual-collections.json", "logo-find.json", "art-missing.json", "sw2-builds.json", "pc-xbox.json", "pc-steam.json", "romm-library.json", "igdb-info",
];
const EXTRA_APP: [&str; 1] = ["Music"];
const KEEP: usize = 3;
const MAX_JSON: u64 = 16 << 20;

fn read_json(f: &Path) -> Option<Value> {
    std::fs::metadata(f).ok().filter(|m| m.is_file() && m.len() <= MAX_JSON)?;
    crate::files::read_json(f)
}

fn same_path(a: &Path, b: &Path) -> bool {
    let (x, y) = (std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf()), std::path::absolute(b).unwrap_or_else(|_| b.to_path_buf()));
    if cfg!(windows) { x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase() } else { x == y }
}

/// A version as a folder's name.
pub fn safe_ver(v: &str) -> String {
    let s: String = v.chars().map(|c| if c.is_ascii_alphanumeric() || "._+-".contains(c) { c } else { '_' }).take(64).collect();
    if s.is_empty() { "unknown".into() } else { s }
}

/// The sound packs' and custom themes' own files: their path with '/' and where they are.
fn def_files(app_root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for (dir, file) in DEF_DIRS {
        let Ok(rd) = std::fs::read_dir(app_root.join(dir)) else { continue };
        let mut names: Vec<String> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for n in names {
            let f = app_root.join(dir).join(&n).join(file);
            if f.is_file() {
                out.push((format!("{dir}/{n}/{file}"), f));
            }
        }
    }
    out
}

// ---------- the safety copy ----------

/// Copies your setup into `data\backups\<version>\` (`version`: the one it's from); the newest three are kept.
/// How many files.
pub fn safety_copy(data_dir: &Path, app_root: &Path, version: &str, now: f64) -> std::io::Result<usize> {
    let dir = data_dir.join("backups").join(safe_ver(version));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let mut files = Vec::new();
    let mut copy = |src: &Path, rel: String| -> std::io::Result<()> {
        let to = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::copy(src, &to)?;
        files.push(json!({ "p": rel, "size": std::fs::metadata(&to).map_or(0, |m| m.len()), "json": read_json(&to).is_some() }));
        Ok(())
    };
    for f in STATE_FILES {
        let src = data_dir.join(f);
        if src.is_file() {
            copy(&src, format!("data/{f}"))?;
        }
    }
    for (rel, src) in def_files(app_root) {
        copy(&src, format!("app/{rel}"))?;
    }
    let n = files.len();
    write_atomic(&dir.join("manifest.json"), crate::js::stringify_indented(&json!({ "app": "RustyShelf", "kind": "safety", "version": version, "at": now, "files": files }), 1))?;
    for old in safety_list(data_dir).into_iter().skip(KEEP) {
        let _ = std::fs::remove_dir_all(&old.0);
    }
    Ok(n)
}

/// The safety copies, the newest first: their folder and manifest.
pub fn safety_list(data_dir: &Path) -> Vec<(PathBuf, Value)> {
    let Ok(rd) = std::fs::read_dir(data_dir.join("backups")) else { return Vec::new() };
    let mut out: Vec<(PathBuf, Value)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let m = read_json(&e.path().join("manifest.json"))?;
            (matches!(m["app"].as_str(), Some("RustyShelf" | "Boxshelf")) && m["kind"] == "safety" && m["files"].is_array()).then(|| (e.path(), m))
        })
        .collect();
    out.sort_by(|a, b| b.1["at"].as_f64().unwrap_or(0.0).total_cmp(&a.1["at"].as_f64().unwrap_or(0.0)));
    out
}

/// After a new version's first start: is everything from the newest safety copy still there and readable? A
/// missing or damaged config.json is put back at once; anything else is only listed. Each copy is checked once.
/// The version it was from, what isn't as it was, and whether config.json was put back.
pub fn check_after_update(data_dir: &Path, app_root: &Path, version: &str) -> Option<(String, Vec<String>, bool)> {
    let (dir, mut m) = safety_list(data_dir).into_iter().next()?;
    let from = m["version"].as_str().unwrap_or("").to_string();
    if from == version || m["checked"] == version {
        return None;
    }
    let (mut problems, mut fixed) = (Vec::new(), false);
    for f in m["files"].as_array().cloned().unwrap_or_default() {
        let p = f["p"].as_str().unwrap_or("");
        let Some((place, rest)) = p.split_once('/') else { continue };
        let root = if place == "app" { app_root } else { data_dir };
        let (cur, copy) = (root.join(rest), dir.join(place).join(rest));
        if cur.is_file() && (f["json"] != true || read_json(&cur).is_some()) {
            continue;
        }
        if p == "data/config.json" && read_json(&copy).is_some() {
            if cur.is_file() {
                let _ = std::fs::copy(&cur, cur.with_extension("json.before-repair"));
            }
            if std::fs::copy(&copy, &cur).is_ok() {
                fixed = true;
                continue;
            }
        }
        problems.push(p.to_string());
    }
    m["checked"] = json!(version);
    let _ = write_atomic(&dir.join("manifest.json"), crate::js::stringify_indented(&m, 1));
    Some((from, problems, fixed))
}

// ---------- changes put in at the next start ----------

fn pending_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(".pending")
}

fn pending_begin(data_dir: &Path) -> std::io::Result<PathBuf> {
    let d = pending_dir(data_dir);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn pending_commit(data_dir: &Path, meta: Value) -> std::io::Result<()> {
    let mut meta = meta;
    meta["at"] = json!(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64));
    write_atomic(&pending_dir(data_dir).join("ready.json"), meta.to_string())
}

fn move_tree(from: &Path, to: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(from) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let (s, d) = (e.path(), to.join(e.file_name()));
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            n += move_tree(&s, &d);
        } else if kind.is_file() && !d.is_dir() {
            let _ = std::fs::create_dir_all(to);
            if std::fs::rename(&s, &d).is_ok() || std::fs::copy(&s, &d).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// At start, before anything reads its files: a waiting change put in. Its ready.json, and how many files.
pub fn apply_pending(data_dir: &Path, app_root: &Path) -> Option<(Value, usize)> {
    let d = pending_dir(data_dir);
    if !d.is_dir() {
        return None;
    }
    let meta = read_json(&d.join("ready.json")).filter(Value::is_object);
    let Some(meta) = meta else {
        let _ = std::fs::remove_dir_all(&d);
        return None;
    };
    let n = move_tree(&d.join("data"), data_dir) + move_tree(&d.join("app"), app_root);
    let _ = std::fs::remove_dir_all(&d);
    Some((meta, n))
}

/// Undo the last update's settings changes: the newest safety copy, put back at the next start. Its version.
pub fn stage_undo(data_dir: &Path) -> Result<String, String> {
    let (dir, m) = safety_list(data_dir).into_iter().next().ok_or("There is no safety copy to go back to.")?;
    let d = pending_begin(data_dir).map_err(|e| e.to_string())?;
    for f in m["files"].as_array().cloned().unwrap_or_default() {
        let p = f["p"].as_str().unwrap_or("");
        let parts: Vec<&str> = p.split('/').collect();
        if !matches!(parts.first(), Some(&"data" | &"app")) || parts.iter().any(|x| x.is_empty() || *x == "..") {
            continue;
        }
        let src = parts.iter().fold(dir.clone(), |a, x| a.join(x));
        if src.is_file() {
            let to = parts.iter().fold(d.clone(), |a, x| a.join(x));
            let _ = std::fs::create_dir_all(to.parent().unwrap_or(&d));
            std::fs::copy(&src, &to).map_err(|e| e.to_string())?;
        }
    }
    let version = m["version"].as_str().unwrap_or("").to_string();
    pending_commit(data_dir, json!({ "kind": "undo", "from": version })).map_err(|e| e.to_string())?;
    Ok(version)
}

// ---------- installs.json: where each RustyShelf has run from ----------

/// One RustyShelf that has run on this PC.
#[derive(Debug, Clone, PartialEq)]
pub struct Install {
    pub data_dir: PathBuf,
    pub app_root: PathBuf,
    pub version: String,
    pub last_run: f64,
}

/// `installs.json` in the user's settings folder (Windows %APPDATA%\RustyShelf, else ~/.config/rustyshelf).
pub fn registry_file(var: crate::emucfg::Var) -> Option<PathBuf> {
    if cfg!(windows) {
        var("APPDATA").map(|a| PathBuf::from(a).join("RustyShelf").join("installs.json"))
    } else {
        var("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(PathBuf::from).or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config"))).map(|c| c.join("rustyshelf").join("installs.json"))
    }
}

pub fn read_registry(file: &Path) -> Vec<Install> {
    read_json(file).and_then(|j| j["installs"].as_array().cloned()).unwrap_or_default().iter().filter_map(|x| {
        let data_dir = PathBuf::from(x["dataDir"].as_str().filter(|d| d.len() < 1024)?);
        if !data_dir.is_absolute() {
            return None;
        }
        let app_root = x["appRoot"].as_str().map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| data_dir.parent().map_or_else(|| data_dir.clone(), Path::to_path_buf));
        Some(Install { data_dir, app_root, version: x["version"].as_str().unwrap_or("").chars().take(64).collect(), last_run: x["lastRun"].as_f64().unwrap_or(0.0) })
    }).collect()
}

/// This RustyShelf has run: it goes first, at most eight kept.
pub fn note_run(file: &Path, me: &Install) -> std::io::Result<()> {
    let mut list: Vec<Install> = read_registry(file).into_iter().filter(|x| !same_path(&x.data_dir, &me.data_dir)).collect();
    list.insert(0, me.clone());
    list.sort_by(|a, b| b.last_run.total_cmp(&a.last_run));
    list.truncate(8);
    let rows: Vec<Value> = list.iter().map(|x| json!({ "dataDir": x.data_dir.to_string_lossy(), "appRoot": x.app_root.to_string_lossy(), "version": x.version, "lastRun": x.last_run })).collect();
    write_atomic(file, crate::js::stringify_indented(&json!({ "installs": rows }), 1))
}

fn setup_done(data_dir: &Path) -> bool {
    read_json(&data_dir.join("config.json")).is_some_and(|c| c["setupDone"] == true && c["ui"].is_object() && c["systems"].is_object())
}

/// Another RustyShelf's setup this one could start from: the most recently run one whose data folder still has a
/// finished setup.
pub fn find_other(file: &Path, data_dir: &Path) -> Option<Install> {
    let mut list = read_registry(file);
    list.sort_by(|a, b| b.last_run.total_cmp(&a.last_run));
    list.into_iter().find(|x| !same_path(&x.data_dir, data_dir) && setup_done(&x.data_dir))
}

/// A RustyShelf folder (or its data folder) with a finished setup in it.
pub fn setup_at(dir: &Path) -> Option<Install> {
    if !dir.is_absolute() {
        return None;
    }
    let mut tries = vec![(dir.join("data"), dir.to_path_buf())];
    if dir.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("data")) {
        tries.insert(0, (dir.to_path_buf(), dir.parent()?.to_path_buf()));
    }
    tries.into_iter().find_map(|(data_dir, app_root)| {
        let f = data_dir.join("config.json");
        let c = read_json(&f).filter(|c| c["setupDone"] == true && c["ui"].is_object() && c["systems"].is_object())?;
        let last_run = std::fs::metadata(&f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_millis() as f64);
        Some(Install { version: c["ui"]["lastSeenVersion"].as_str().unwrap_or("").chars().take(64).collect(), data_dir, app_root, last_run })
    })
}

/// The newest finished setup in a folder inside one of `roots` (the folder this RustyShelf is in, Downloads, the
/// Desktop…), or one level further down in a folder named after it.
pub fn find_nearby(data_dir: &Path, roots: &[PathBuf]) -> Option<Install> {
    let mut best: Option<Install> = None;
    let kids = |d: &Path, max: usize| -> Vec<(String, PathBuf)> {
        std::fs::read_dir(d).map(|rd| rd.flatten().filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.')).take(max).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect()).unwrap_or_default()
    };
    let mut consider = |d: &Path| {
        if let Some(x) = setup_at(d).filter(|x| !same_path(&x.data_dir, data_dir)) {
            if best.as_ref().is_none_or(|b| x.last_run > b.last_run) {
                best = Some(x);
            }
        }
    };
    for r in roots.iter().filter(|r| r.is_absolute()) {
        for (name, d) in kids(r, 400) {
            consider(&d);
            let lower = name.to_lowercase();
            if lower.contains("rustyshelf") || lower.contains("boxshelf") {
                for (_, d2) in kids(&d, 40) {
                    consider(&d2);
                }
            }
        }
    }
    best
}

// ---------- bringing a setup over ----------

fn walk(root: &Path, rel: &str, out: &mut Vec<(PathBuf, String)>, cap: &mut usize) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        if *cap == 0 {
            return;
        }
        let (p, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            walk(&p, &format!("{rel}/{name}"), out, cap);
        } else if kind.is_file() {
            *cap -= 1;
            out.push((p, format!("{rel}/{name}")));
        }
    }
}

/// What's brought over: each file and its '/' path under `.pending`.
pub fn bring_list(from: &Install, to: &Install, extras: bool) -> Vec<(PathBuf, String)> {
    let (mut out, mut cap, same_app) = (Vec::new(), 400_000usize, same_path(&from.app_root, &to.app_root));
    let file = |src: PathBuf, rel: String, out: &mut Vec<(PathBuf, String)>| {
        if src.is_file() {
            out.push((src, rel));
        }
    };
    for f in STATE_FILES {
        file(from.data_dir.join(f), format!("data/{f}"), &mut out);
    }
    for d in BRING_DATA_DIRS {
        walk(&from.data_dir.join(d), &format!("data/{d}"), &mut out, &mut cap);
    }
    if !same_app {
        for d in BRING_APP_DIRS {
            walk(&from.app_root.join(d), &format!("app/{d}"), &mut out, &mut cap);
        }
    }
    if extras {
        for x in EXTRA_DATA {
            let p = from.data_dir.join(x);
            if p.is_dir() {
                walk(&p, &format!("data/{x}"), &mut out, &mut cap);
            } else {
                file(p, format!("data/{x}"), &mut out);
            }
        }
        if !same_app {
            for d in EXTRA_APP {
                walk(&from.app_root.join(d), &format!("app/{d}"), &mut out, &mut cap);
            }
        }
    }
    out
}

/// Whether that setup's settings hold sealed keys (on Windows they open only with that folder's "Local State").
fn has_sealed_keys(data_dir: &Path) -> bool {
    read_json(&data_dir.join("config.json")).is_some_and(|c| c["keysEnc"].as_object().is_some_and(|k| k.values().any(|v| crate::js::truthy(Some(v)))) || crate::js::truthy(c["ra"].get("keyEnc")))
}

/// Copies another setup into `.pending`, put in at the next start. Paths written as {app} in its settings point at
/// its own folder then (its emulators and games stay where they are). How many files, and their bytes.
pub fn bring_stage(from: &Install, to: &Install, extras: bool, progress: &dyn Fn(u64, u64)) -> Result<(usize, u64), String> {
    let list = bring_list(from, to, extras);
    let total: u64 = list.iter().filter_map(|(f, _)| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    let d = pending_begin(&to.data_dir).map_err(|e| e.to_string())?;
    let staged = (|| -> Result<u64, String> {
        let mut done = 0u64;
        for (src, rel) in &list {
            let dst = rel.split('/').fold(d.clone(), |a, x| a.join(x));
            std::fs::create_dir_all(dst.parent().unwrap_or(&d)).map_err(|e| e.to_string())?;
            if rel == "data/config.json" {
                let mut t = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
                let c: Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
                if c["setupDone"] != true {
                    return Err("That setup isn't finished.".into());
                }
                if !same_path(&from.app_root, &to.app_root) {
                    let abs = std::path::absolute(&from.app_root).unwrap_or_else(|_| from.app_root.clone());
                    let quoted = serde_json::to_string(&abs.to_string_lossy()).unwrap_or_default();
                    static APP: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)\{app\}").unwrap());
                    t = APP.replace_all(&t, regex::NoExpand(&quoted[1..quoted.len() - 1])).into_owned();
                }
                std::fs::write(&dst, t).map_err(|e| e.to_string())?;
            } else {
                std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
            }
            done += std::fs::metadata(src).map_or(0, |m| m.len());
            progress(done, total);
        }
        if cfg!(windows) && has_sealed_keys(&from.data_dir) && from.data_dir.join("Local State").is_file() {
            std::fs::copy(from.data_dir.join("Local State"), d.join("data").join("Local State")).map_err(|e| e.to_string())?;
        }
        Ok(done)
    })();
    match staged {
        Ok(bytes) => {
            pending_commit(&to.data_dir, json!({ "kind": "bring", "from": from.version, "fromDir": from.data_dir.to_string_lossy(), "extras": extras })).map_err(|e| e.to_string())?;
            Ok((list.len(), bytes))
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&d);
            Err(e)
        }
    }
}

// ---------- at start ----------

static NOTE: Mutex<Option<String>> = Mutex::new(None);

/// What the start found to say (a change put in, config.json put back), once.
pub fn take_note() -> Option<String> {
    NOTE.lock().unwrap().take()
}

/// At start, before the settings are read: a waiting change put in; at a new version's first start a safety copy
/// of the setup (from the version before), then checked; and this RustyShelf noted in installs.json.
pub fn at_start(app_root: &Path, version: &str, var: crate::emucfg::Var) {
    let data_dir = crate::paths::data_dir(app_root);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    let mut said = Vec::new();
    if let Some((meta, _)) = apply_pending(&data_dir, app_root) {
        said.push(match meta["kind"].as_str() {
            Some("bring") => "Your setup was brought over from your other RustyShelf".to_string(),
            Some("undo") => format!("Your settings are back as they were before {}", meta["from"].as_str().unwrap_or("the update")),
            _ => "A waiting change was put in".to_string(),
        });
    }
    let registry = registry_file(var);
    let before = registry.as_ref().and_then(|f| read_registry(f).into_iter().find(|x| same_path(&x.data_dir, &data_dir))).map(|x| x.version);
    if setup_done(&data_dir) && before.as_deref() != Some(version) {
        let _ = safety_copy(&data_dir, app_root, before.as_deref().unwrap_or("unknown"), now);
    }
    if let Some((_, problems, fixed)) = check_after_update(&data_dir, app_root, version) {
        if fixed {
            said.push("Your settings file was damaged: it was put back from the copy made before this version".into());
        } else if !problems.is_empty() {
            said.push(format!("Not as before this version: {}. Settings → System can put the copy back", problems.join(", ")));
        }
    }
    if let Some(f) = registry {
        let _ = note_run(&f, &Install { data_dir: data_dir.clone(), app_root: app_root.to_path_buf(), version: version.into(), last_run: now });
    }
    if !said.is_empty() {
        *NOTE.lock().unwrap() = Some(said.join(" · "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(dir: &Path, theme: &str) {
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data").join("config.json"), json!({ "setupDone": true, "ui": { "theme": theme }, "systems": { "ps2": { "emulator": "{app}\\\\PCSX2\\\\pcsx2.exe" } } }).to_string()).unwrap();
        std::fs::write(dir.join("data").join("history.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("sounds").join("mine")).unwrap();
        std::fs::write(dir.join("sounds").join("mine").join("pack.json"), "{\"name\":\"Mine\"}").unwrap();
    }

    #[test]
    fn a_new_version_copies_the_setup_first_and_can_put_it_back() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, home) = (tmp.path().join("RustyShelf"), tmp.path().join("home"));
        setup(&app, "nebula");
        let var = |k: &str| match k {
            "APPDATA" | "XDG_CONFIG_HOME" => Some(home.to_string_lossy().into_owned()),
            _ => None,
        };
        at_start(&app, "0.4.0", &var);
        let first = safety_list(&app.join("data"));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1["version"], "unknown");
        assert_eq!(first[0].1["files"].as_array().unwrap().len(), 3);
        // the same version again: no new copy
        at_start(&app, "0.4.0", &var);
        assert_eq!(safety_list(&app.join("data")).len(), 1);
        // the next version: a copy of 0.4.0's setup; its config.json damaged meanwhile is put back
        let before = safety_copy(&app.join("data"), &app, "0.4.0", 9e12).unwrap();
        assert_eq!(before, 3);
        std::fs::write(app.join("data").join("config.json"), "{ not json").unwrap();
        let checked = check_after_update(&app.join("data"), &app, "0.5.0").unwrap();
        assert!(checked.2 && checked.1.is_empty());
        assert!(read_json(&app.join("data").join("config.json")).is_some());
        assert!(check_after_update(&app.join("data"), &app, "0.5.0").is_none());
        // Undo: the copy waits in .pending and goes in at the next start
        std::fs::write(app.join("data").join("config.json"), json!({ "setupDone": true, "ui": { "theme": "storm" }, "systems": {} }).to_string()).unwrap();
        assert_eq!(stage_undo(&app.join("data")).unwrap(), "0.4.0");
        let (meta, n) = apply_pending(&app.join("data"), &app).unwrap();
        assert_eq!((meta["kind"].as_str(), n), (Some("undo"), 3));
        assert_eq!(read_json(&app.join("data").join("config.json")).unwrap()["ui"]["theme"], "nebula");
        // a half-made change is dropped
        std::fs::create_dir_all(app.join("data").join(".pending").join("data")).unwrap();
        assert!(apply_pending(&app.join("data"), &app).is_none() && !app.join("data").join(".pending").exists());
    }

    #[test]
    fn a_new_folder_brings_the_setup_over_from_the_old_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (old, new, home) = (tmp.path().join("RustyShelf-0.4"), tmp.path().join("RustyShelf-0.5"), tmp.path().join("home"));
        setup(&old, "dusk");
        std::fs::create_dir_all(old.join("data").join("boxart-cache")).unwrap();
        std::fs::write(old.join("data").join("boxart-cache").join("x.json"), "{}").unwrap();
        std::fs::create_dir_all(new.join("data")).unwrap();
        let var = |k: &str| matches!(k, "APPDATA" | "XDG_CONFIG_HOME").then(|| home.to_string_lossy().into_owned());
        at_start(&old, "0.4.0", &var);
        at_start(&new, "0.5.0", &var);
        let reg = registry_file(&var).unwrap();
        let from = find_other(&reg, &new.join("data")).unwrap();
        assert!(same_path(&from.app_root, &old));
        assert!(find_nearby(&new.join("data"), &[tmp.path().to_path_buf()]).is_some_and(|x| same_path(&x.data_dir, &old.join("data"))));
        let to = Install { data_dir: new.join("data"), app_root: new.clone(), version: "0.5.0".into(), last_run: 0.0 };
        assert!(bring_list(&from, &to, false).iter().all(|(_, rel)| !rel.starts_with("data/boxart-cache")));
        assert!(bring_list(&from, &to, true).iter().any(|(_, rel)| rel == "data/boxart-cache/x.json"));
        let (n, _) = bring_stage(&from, &to, false, &|_, _| {}).unwrap();
        assert_eq!(n, 3);
        apply_pending(&new.join("data"), &new).unwrap();
        let cfg = std::fs::read_to_string(new.join("data").join("config.json")).unwrap();
        // {app} in the old settings points at the old folder now
        assert!(!cfg.contains("{app}") && cfg.contains("RustyShelf-0.4"));
        assert!(new.join("sounds").join("mine").join("pack.json").is_file());
    }
}

// Ported from RustyShelf's main/dataexport.js (GPL-3.0).
//! Export everything and Import everything (Settings → System): a collection's art and manuals take days to
//! download, so they can be carried whole to another PC or a second install.
//!
//! An export is one folder, "RustyShelf export <date>", holding `data/` (the data folder without the Electron
//! app's engine caches, logs and sign-ins), `app/<name>/` (the app folder's Library, themes, Custom themes,
//! Guides, Magazines and Songs, and an art library kept elsewhere as `app/Library-elsewhere`),
//! `rom-art/<console>/<n>/` (art and manuals kept next to the games) and `manifest.json` (what's where, and
//! where it came from). Exporting into a folder that holds an export updates it: only new and changed files
//! are copied, and nothing is ever deleted from it. An import copies an export back: nothing is deleted, files
//! of the same name are replaced, and the settings there were are kept beside the new ones.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::js;

/// What's left out of the data folder, by name at its top level: the Electron app's engine caches, logs,
/// sign-ins and locks.
// (main/dataexport.js SKIP, keep in sync)
static SKIP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?i)(cache|code cache|gpucache|dawngraphitecache|dawnwebgpucache|grshadercache|shadercache|blob_storage|crashpad|logs|session storage|shared dictionary|sharedstorage|dictionaries|dips|trust tokens.*|cookies.*|network persistent state|transportsecurity|preferences|local state|devtoolsactiveport|padwatch|legendary|companion\.json|.*\.log|.*\.tmp|singleton.*|lockfile|.*\.lock|lock)$")
        .unwrap()
});
const APP_DIRS: [&str; 6] = ["Library", "themes", "Custom themes", "Guides", "Magazines", "Songs"];

fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

fn lower(p: &Path) -> String {
    js::normalize(&p.to_string_lossy()).to_string_lossy().to_lowercase()
}

fn inside(p: &Path, root: &Path) -> bool {
    let (a, b) = (lower(p), lower(root));
    a == b || a.starts_with(&format!("{b}{}", std::path::MAIN_SEPARATOR))
}

/// The time as an export's folder is named after it ("2026-11-14 21-40", by UTC), and as its manifest has it.
fn stamps() -> (String, String) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, s) = (rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60);
    (format!("{y:04}-{m:02}-{d:02} {h:02}-{min:02}"), format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}.{:03}Z", rest % 1000))
}

/// A file to copy: where it is, its path under what's being copied ("" for a single file), its size and when it
/// was changed (ms).
struct Found {
    src: PathBuf,
    rel: PathBuf,
    size: u64,
    changed: f64,
}

fn changed_ms(m: &fs::Metadata) -> f64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Every file under `src`, without what `skip_top` names at its top level.
fn list(src: &Path, skip_top: bool) -> Vec<Found> {
    fn walk(dir: &Path, rel: &Path, top: bool, skip_top: bool, out: &mut Vec<Found>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let (name, path) = (e.file_name(), e.path());
            if top && skip_top && SKIP.is_match(&name.to_string_lossy()) {
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else { continue };
            if meta.is_dir() {
                walk(&path, &rel.join(&name), false, skip_top, out);
            } else if meta.is_file() {
                out.push(Found { src: path, rel: rel.join(&name), size: meta.len(), changed: changed_ms(&meta) });
            }
        }
    }
    let mut out = Vec::new();
    match fs::metadata(src) {
        Ok(m) if m.is_file() => out.push(Found { src: src.to_path_buf(), rel: PathBuf::new(), size: m.len(), changed: changed_ms(&m) }),
        Ok(m) if m.is_dir() => walk(src, Path::new(""), true, skip_top, &mut out),
        _ => {}
    }
    out
}

fn size_of(dir: &Path) -> (u64, u64) {
    let files = list(dir, false);
    (files.iter().map(|f| f.size).sum(), files.len() as u64)
}

/// Asked of a running export or import, between two files.
#[derive(Default)]
pub struct Control {
    pub paused: AtomicBool,
    pub cancelled: AtomicBool,
}

/// How far an export or import is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Progress {
    /// "export" or "import"
    pub what: &'static str,
    pub bytes: u64,
    pub of: u64,
    pub files: u64,
    pub of_files: u64,
    /// the part being copied
    pub label: String,
    /// the files really copied (an update leaves the unchanged ones)
    pub copied: u64,
    pub paused: bool,
    /// seconds left, once that can be told
    pub eta: Option<u64>,
}

/// What an export or import ended with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Copied {
    pub copied: u64,
    pub copied_bytes: u64,
    pub unchanged: u64,
}

struct Job<'a> {
    what: &'static str,
    control: &'a Control,
    on_progress: &'a mut dyn FnMut(&Progress),
    /// a file already there with the same size and time is left as it is
    same: bool,
    started: Instant,
    paused: Duration,
    said: Option<Instant>,
    now: Progress,
    copied_bytes: u64,
}

impl Job<'_> {
    fn say(&mut self, force: bool) {
        if !force && self.said.is_some_and(|at| at.elapsed() < Duration::from_millis(250)) {
            return;
        }
        self.said = Some(Instant::now());
        let spent = (self.started.elapsed() - self.paused).as_secs_f64();
        let rate = if spent > 2.0 && self.now.bytes > 0 { self.now.bytes as f64 / spent } else { 0.0 };
        self.now.paused = self.control.paused.load(Ordering::Relaxed);
        self.now.eta = (rate > 0.0).then(|| js::round(self.now.of.saturating_sub(self.now.bytes) as f64 / rate) as u64);
        (self.on_progress)(&self.now);
    }

    /// Waits while paused; an error once cancelled.
    fn gate(&mut self) -> Result<(), String> {
        let (paused, cancelled) = (|c: &Control| c.paused.load(Ordering::Relaxed), |c: &Control| c.cancelled.load(Ordering::Relaxed));
        if paused(self.control) && !cancelled(self.control) {
            let since = Instant::now();
            self.say(true);
            while paused(self.control) && !cancelled(self.control) {
                std::thread::sleep(Duration::from_millis(150));
            }
            self.paused += since.elapsed();
            self.say(true);
        }
        if cancelled(self.control) {
            return Err(format!("The {} was cancelled.", self.what));
        }
        Ok(())
    }

    fn one(&mut self, f: &Found, dst: &Path) -> Result<(), String> {
        self.gate()?;
        let there = if self.same { fs::metadata(dst).ok().filter(|d| d.is_file() && d.len() == f.size && (changed_ms(d) - f.changed).abs() < 2000.0) } else { None };
        if there.is_none() {
            if let Some(dir) = dst.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            match fs::copy(&f.src, dst) {
                Ok(_) => {
                    // the file's own time is kept: the next export into the same folder compares by it
                    if let Ok(file) = fs::File::options().write(true).open(dst) {
                        let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs_f64(f.changed / 1000.0));
                    }
                    self.now.copied += 1;
                    self.copied_bytes += f.size;
                }
                // a file in use, or one that may not be read: left out, the rest goes on
                Err(e) if matches!(e.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ResourceBusy | std::io::ErrorKind::ExecutableFileBusy) => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        self.now.bytes += f.size;
        self.now.files += 1;
        self.say(false);
        Ok(())
    }

    fn run(&mut self, label: &str, files: &[Found], to: &Path) -> Result<(), String> {
        self.now.label = label.to_string();
        self.say(true);
        for f in files {
            self.one(f, &if f.rel.as_os_str().is_empty() { to.to_path_buf() } else { to.join(&f.rel) })?;
        }
        Ok(())
    }

    fn end(mut self) -> Copied {
        self.say(true);
        Copied { copied: self.now.copied, copied_bytes: self.copied_bytes, unchanged: self.now.files - self.now.copied }
    }
}

fn job<'a>(what: &'static str, control: &'a Control, on_progress: &'a mut dyn FnMut(&Progress), same: bool) -> Job<'a> {
    Job { what, control, on_progress, same, started: Instant::now(), paused: Duration::ZERO, said: None, now: Progress { what, ..Progress::default() }, copied_bytes: 0 }
}

/// The install an export is made from. `art_dirs`: every folder a console's art may be in, by its id and its
/// settings (`ArtLibrary::dirs_for`).
pub struct Install<'a> {
    pub app_root: &'a Path,
    pub data_dir: &'a Path,
    /// the settings: its `systems` are looked through
    pub cfg: &'a Value,
    pub art_dirs: &'a dyn Fn(&str, &Value) -> Vec<PathBuf>,
    /// the art library, when there is one
    pub art_lib: Option<&'a Path>,
}

/// Art kept next to a console's games: the console, its place among that console's folders, and the folder.
#[derive(Debug, Clone, PartialEq)]
pub struct RomArt {
    pub console: String,
    pub n: usize,
    pub path: PathBuf,
}

/// The art (and the manuals folders) kept next to each console's games, without what's already inside the app
/// folder or the art library.
pub fn rom_art(i: &Install) -> Vec<RomArt> {
    let mut out: Vec<RomArt> = Vec::new();
    let Some(systems) = i.cfg.get("systems").and_then(Value::as_object) else { return out };
    for (id, sc) in js::entries(systems) {
        let manuals = sc.get("romDirs").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(|r| Path::new(r).join("manuals"));
        let mut n = 0;
        for d in (i.art_dirs)(id, sc).into_iter().chain(manuals) {
            if d.as_os_str().is_empty() || !is_dir(&d) || inside(&d, i.app_root) || i.art_lib.is_some_and(|lib| inside(&d, lib)) || out.iter().any(|o| lower(&o.path) == lower(&d)) {
                continue;
            }
            out.push(RomArt { console: id.clone(), n, path: d });
            n += 1;
        }
    }
    out
}

struct Part {
    label: String,
    src: PathBuf,
    /// where it goes, under the export
    to: PathBuf,
    skip_top: bool,
    /// its name in the manifest's parts
    part: Option<String>,
    art_lib: bool,
    art: Option<RomArt>,
}

fn parts(i: &Install) -> Vec<Part> {
    let part = |label: &str, src: PathBuf, to: PathBuf, name: Option<String>| Part { label: label.to_string(), src, to, skip_top: false, part: name, art_lib: false, art: None };
    let mut out = vec![Part { skip_top: true, ..part("Settings and downloads", i.data_dir.to_path_buf(), "data".into(), Some("data".into())) }];
    for name in APP_DIRS {
        let p = i.app_root.join(name);
        if is_dir(&p) {
            out.push(part(name, p, Path::new("app").join(name), Some(format!("app/{name}"))));
        }
    }
    if let Some(lib) = i.art_lib.filter(|lib| is_dir(lib) && !inside(lib, i.app_root)) {
        out.push(Part { art_lib: true, ..part("Art library", lib.to_path_buf(), Path::new("app").join("Library-elsewhere"), Some("app/Library-elsewhere".into())) });
    }
    for a in rom_art(i) {
        out.push(Part { art: Some(a.clone()), ..part(&format!("{} art next to the games", a.console), a.path.clone(), Path::new("rom-art").join(&a.console).join(a.n.to_string()), None) });
    }
    out
}

/// How big an export of this install would be: its bytes and its files.
pub fn export_size(i: &Install) -> (u64, u64) {
    let files: Vec<Found> = parts(i).iter().flat_map(|p| list(&p.src, p.skip_top)).collect();
    (files.iter().map(|f| f.size).sum(), files.len() as u64)
}

fn manifest_of(dir: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(dir.join("manifest.json")).ok()?).ok()
}

fn is_export(dir: &Path) -> bool {
    manifest_of(dir).is_some_and(|m| m["app"].as_str().is_some_and(|a| a.contains("RustyShelf") || a.contains("(T)rustyShelf")))
}

/// The earlier export to update: `dest` itself, else the newest "RustyShelf export …" folder in it.
fn earlier(dest: &Path) -> Option<PathBuf> {
    if is_export(dest) {
        return Some(dest.to_path_buf());
    }
    let mut found: Vec<PathBuf> = fs::read_dir(dest)
        .ok()?
        .flatten()
        .filter(|e| { let n = e.file_name().to_string_lossy().into_owned(); n.starts_with("RustyShelf export ") || n.starts_with("(T)rustyShelf export ") })
        .map(|e| e.path())
        .filter(|p| is_dir(p) && is_export(p))
        .collect();
    found.sort();
    found.pop()
}

/// What an export came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Exported {
    pub dir: PathBuf,
    /// the whole export's size and files, its manifest too
    pub bytes: u64,
    pub files: u64,
    pub parts: usize,
    /// an earlier export was brought up to date
    pub updated: bool,
    pub copied: Copied,
}

/// Exports the install into `dest`, or brings the export already there up to date. `on_step(i, of, label)` as
/// each part begins.
pub fn export_all(i: &Install, dest: &Path, version: &str, control: &Control, on_step: &mut dyn FnMut(usize, usize, &str), on_progress: &mut dyn FnMut(&Progress)) -> Result<Exported, String> {
    if dest.as_os_str().is_empty() || !is_dir(dest) {
        return Err("Choose a folder to export to.".into());
    }
    let (named, at) = stamps();
    let prev = earlier(dest);
    let dir = prev.clone().unwrap_or_else(|| dest.join(format!("RustyShelf export {named}")));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut manifest = Map::new();
    manifest.insert("app".into(), json!("RustyShelf"));
    manifest.insert("version".into(), json!(version));
    manifest.insert("at".into(), json!(at));
    manifest.insert("from".into(), json!({ "appRoot": i.app_root.to_string_lossy(), "dataDir": i.data_dir.to_string_lossy() }));
    let (mut named_parts, mut arts) = (Vec::new(), Vec::new());
    let first = prev.as_ref().and_then(|_| manifest_of(&dir)).and_then(|m| [&m["first"], &m["at"]].into_iter().find(|v| js::truthy(Some(v))).cloned());
    let mut j = job("export", control, on_progress, prev.is_some());
    let steps: Vec<(Part, Vec<Found>)> = parts(i).into_iter().map(|p| { let files = list(&p.src, p.skip_top); (p, files) }).collect();
    j.now.of = steps.iter().flat_map(|s| &s.1).map(|f| f.size).sum();
    j.now.of_files = steps.iter().map(|s| s.1.len() as u64).sum();
    let mut art_lib = None;
    for (n, (p, files)) in steps.iter().enumerate() {
        on_step(n + 1, steps.len(), &p.label);
        j.run(&p.label, files, &dir.join(&p.to))?;
        named_parts.extend(p.part.clone());
        if p.art_lib {
            art_lib = i.art_lib;
        }
        if let Some(a) = &p.art {
            arts.push(json!({ "console": a.console, "n": a.n, "path": a.path.to_string_lossy(), "rel": p.to.to_string_lossy() }));
        }
    }
    let n_parts = named_parts.len() + arts.len();
    manifest.insert("parts".into(), json!(named_parts));
    manifest.insert("romArt".into(), json!(arts));
    if let Some(first) = first {
        manifest.insert("first".into(), first);
    }
    if let Some(lib) = art_lib {
        manifest.insert("artLib".into(), json!(lib.to_string_lossy()));
    }
    fs::write(dir.join("manifest.json"), js::stringify_indented(&Value::Object(manifest), 2)).map_err(|e| e.to_string())?;
    let (bytes, files) = size_of(&dir);
    Ok(Exported { dir, bytes, files, parts: n_parts, updated: prev.is_some(), copied: j.end() })
}

/// What an import came to.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub parts: usize,
    /// art that had nowhere to go here: its console and the folder it came from
    pub skipped: Vec<String>,
    /// when the export was made
    pub from: String,
    pub copied: Copied,
}

/// Where an import goes: the install's folders, each console's art folders (as `Install::art_dirs`), and the
/// settings as they are here before the import.
pub struct Target<'a> {
    pub app_root: &'a Path,
    pub data_dir: &'a Path,
    pub art_dirs: &'a dyn Fn(&str, &Value) -> Vec<PathBuf>,
    pub art_lib: Option<&'a Path>,
    pub cfg: &'a Value,
}

/// Copies an export back. Art from next to the games goes to the folder it came from when this PC has it, else
/// to the same console's folder in the same place on its list.
pub fn import_all(src: &Path, t: &Target, control: &Control, on_step: &mut dyn FnMut(usize, usize, &str), on_progress: &mut dyn FnMut(&Progress)) -> Result<Imported, String> {
    let manifest = manifest_of(src).ok_or("That folder isn't a RustyShelf export (no manifest.json in it).")?;
    if !matches!(manifest["app"].as_str(), Some("RustyShelf" | "(T)rustyShelf" | "Boxshelf")) {
        return Err("That folder isn't a RustyShelf export.".into());
    }
    // what to copy: its label, where from, where to (None: nowhere here), and whether it's the data folder
    let mut steps: Vec<(String, PathBuf, Option<PathBuf>, bool)> = Vec::new();
    if is_dir(&src.join("data")) {
        steps.push(("Settings and downloads".into(), src.join("data"), Some(t.data_dir.to_path_buf()), true));
    }
    for name in APP_DIRS {
        let p = src.join("app").join(name);
        if is_dir(&p) {
            steps.push((name.to_string(), p, Some(t.app_root.join(name)), false));
        }
    }
    let elsewhere = src.join("app").join("Library-elsewhere");
    if is_dir(&elsewhere) {
        let to = t.art_lib.filter(|lib| !inside(lib, t.app_root)).map_or_else(|| t.app_root.join("Library"), Path::to_path_buf);
        steps.push(("Art library".into(), elsewhere, Some(to), false));
    }
    let mut skipped = Vec::new();
    for a in manifest["romArt"].as_array().into_iter().flatten() {
        let (console, from) = (js::string(a.get("console")), src.join(a["rel"].as_str().unwrap_or("")));
        let was = PathBuf::from(a["path"].as_str().unwrap_or(""));
        let parent_there = |p: &Path| p.parent().is_some_and(is_dir);
        let to = if parent_there(&was) {
            Some(was.clone())
        } else {
            let own: Vec<PathBuf> = t.cfg.get("systems").and_then(|s| s.get(&console)).map_or(Vec::new(), |sc| (t.art_dirs)(&console, sc)).into_iter().filter(|d| !d.as_os_str().is_empty() && !inside(d, t.app_root) && !t.art_lib.is_some_and(|lib| inside(d, lib))).collect();
            let n = js::number(a.get("n"));
            own.get(if n.is_finite() && n >= 0.0 { n as usize } else { 0 }).or(own.first()).filter(|d| parent_there(d)).cloned()
        };
        if is_dir(&from) && to.is_none() {
            skipped.push(format!("{console}: {}", was.display()));
        }
        steps.push((format!("{console} art next to the games"), from, to, false));
    }
    let mut j = job("import", control, on_progress, false);
    j.now.of = size_of(src).0;
    let count = steps.len();
    for (n, (label, from, to, data)) in steps.into_iter().enumerate() {
        on_step(n + 1, count, &label);
        let Some(to) = to.filter(|_| is_dir(&from)) else { continue };
        if data {
            let now = t.data_dir.join("config.json");
            if now.is_file() {
                let _ = fs::copy(&now, t.data_dir.join(format!("config-before-import-{}.json", stamps().0)));
            }
        }
        j.run("", &list(&from, data), &to)?;
    }
    Ok(Imported { parts: count, skipped, from: js::string(manifest.get("at")), copied: j.end() })
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/detect.js, as RustyShelf has it.
//! Finding what's already on the computer: each console's emulator and games folders, RetroArch, Steam and the media
//! apps.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use crate::js;
use crate::paths::{self, is_dir};
use crate::scanner::read_dir_sorted;
use crate::systems::{systems, Console, Systems};
use crate::vita3k;

/// An environment variable by name.
type Lookup<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// The files under a folder by lower-case name: where each is, and how early it was found (breadth first).
type Listing = Rc<HashMap<String, (String, usize)>>;

/// Each folder searched is listed once, a few folders deep, and every console and app looks its program names up in
/// that list: the first match breadth first, without walking the Desktop and Downloads again for each console.
struct Index {
    max_depth: u32,
    /// a folder picked by hand is looked through deeper, but never more than this many folders
    max_dirs: usize,
    cache: RefCell<HashMap<String, Listing>>,
    /// the common places are looked through deeper: a portable copy sits further down than an installed one
    /// (Downloads\Emulators\Dolphin\dolphin-2412-x64\Dolphin-x64\Dolphin.exe)
    deep: std::cell::OnceCell<Box<Index>>,
}

impl Index {
    fn new(max_depth: u32, max_dirs: usize) -> Index {
        Index { max_depth, max_dirs, cache: RefCell::new(HashMap::new()), deep: std::cell::OnceCell::new() }
    }

    fn deeper(&self) -> &Index {
        self.deep.get_or_init(|| Box::new(Index::new(4, 6000)))
    }

    fn of(&self, dir: &str) -> Listing {
        static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\$recycle\.bin|system volume information|windows|node_modules)$").unwrap());
        let key = dir.to_lowercase();
        if let Some(m) = self.cache.borrow().get(&key) {
            return m.clone();
        }
        let mut m: HashMap<String, (String, usize)> = HashMap::new();
        let mut queue = std::collections::VecDeque::from([(dir.to_string(), 0u32)]);
        let mut dirs = 0;
        while let Some((d, depth)) = queue.pop_front().filter(|_| dirs < self.max_dirs) {
            dirs += 1;
            for e in read_dir_sorted(Path::new(&d)) {
                let (Ok(t), name) = (e.file_type(), e.file_name().to_string_lossy().into_owned()) else { continue };
                let full = js::join(&[&d, &name]);
                if t.is_file() {
                    let n = m.len();
                    m.entry(name.to_lowercase()).or_insert((full, n));
                } else if t.is_dir() && depth < self.max_depth && !name.starts_with('.') && !SKIP.is_match(&name) {
                    queue.push_back((full, depth + 1));
                }
            }
        }
        let m = Rc::new(m);
        self.cache.borrow_mut().insert(key, m.clone());
        m
    }
}

/// A folder named for emulators or games at the top of every drive (D:\Emulators, E:\Emulation …), where a
/// downloaded portable emulator is often unzipped (Windows; found once). Keep in sync with main/detect.js driveRoots.
fn drive_roots() -> &'static [String] {
    static TOPS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    TOPS.get_or_init(|| {
        static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(emulators?|emulation|emus?|games|retro|portable)$").unwrap());
        if !cfg!(windows) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for c in b'C'..=b'Z' {
            let d = format!("{}:\\", c as char);
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            out.extend(rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| NAME.is_match(n)).map(|n| format!("{d}{n}")));
        }
        out
    })
}

/// BizHawk covers many consoles: in a folder with another emulator for the console too, that one wins.
fn late(name: &str) -> bool {
    name.eq_ignore_ascii_case("emuhawk.exe")
}

/// The program in this folder with one of these names, the one found first; `re:` starts a pattern (Linux's
/// versioned AppImage names).
fn find_in(index: &Index, dir: &str, names: &[String]) -> Option<String> {
    let n_late = names.iter().filter(|n| late(n)).count();
    if n_late > 0 && n_late < names.len() {
        let (last, first): (Vec<String>, Vec<String>) = names.iter().cloned().partition(|n| late(n));
        return find_in(index, dir, &first).or_else(|| find_in(index, dir, &last));
    }
    let m = index.of(dir);
    let mut hits: Vec<&(String, usize)> = Vec::new();
    for n in names {
        if let Some(pattern) = n.strip_prefix("re:") {
            if let Ok(re) = Regex::new(&format!("(?i){pattern}")) {
                hits.extend(m.iter().filter(|(k, _)| re.is_match(k)).map(|(_, h)| h));
            }
        } else if let Some(h) = m.get(&n.to_lowercase()) {
            hits.push(h);
        }
    }
    hits.into_iter().min_by_key(|h| h.1).map(|h| h.0.clone())
}

/// What one console has on this computer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub emulator: Option<String>,
    /// every copy of its emulators found, at most 8, for Settings to offer
    pub emulators: Vec<String>,
    pub rom_dirs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detected {
    /// by console id, in the consoles' order
    pub systems: Vec<(String, Found)>,
    /// by app id
    pub apps: Vec<(String, Option<String>)>,
    pub steam: Option<String>,
    pub retroarch: String,
}

pub struct Detector<'a> {
    systems: &'a Systems,
    app_root: PathBuf,
    var: Box<Lookup<'a>>,
    /// SD cards and USB drives (Linux), each searched like the home folder
    media: Vec<String>,
}

/// Linux: `/run/media/<user>/<card>` (SteamOS, most desktops), `/run/media/<card>` (older SteamOS),
/// `/media/<user>/<card>` (Ubuntu); each card, and its Emulation and retrodeck folders.
fn media_roots() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if !cfg!(target_os = "linux") {
        return out;
    }
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).unwrap_or_default();
    for base in [format!("/run/media/{user}"), "/run/media".to_string(), format!("/media/{user}")] {
        for e in read_dir_sorted(Path::new(&base)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == user {
                continue;
            }
            let d = js::join(&[&base, &name]);
            for p in [d.clone(), js::join(&[&d, "Emulation"]), js::join(&[&d, "retrodeck"])] {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
    }
    out
}

impl<'a> Detector<'a> {
    pub fn new(app_root: &Path) -> Detector<'static> {
        Detector { systems: systems(), app_root: app_root.to_path_buf(), var: Box::new(paths::env_var), media: media_roots() }
    }

    /// With these consoles, variables and removable drives instead of this computer's.
    pub fn with(systems: &'a Systems, app_root: &Path, var: impl Fn(&str) -> Option<String> + 'a, media: Vec<String>) -> Detector<'a> {
        Detector { systems, app_root: app_root.to_path_buf(), var: Box::new(var), media }
    }

    fn expand(&self, p: &str) -> String {
        paths::expand_with(p, &self.app_root, &*self.var)
    }

    fn search_roots(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in self.systems.search_roots.iter().map(|r| self.expand(r)).chain(drive_roots().iter().cloned()).chain(self.media.iter().cloned()) {
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    /// The program in the emulator's usual folders, else a portable copy in a common place
    /// (Desktop\Emulators\Dolphin-x64\Dolphin.exe), looked through deeper.
    fn find_exe(&self, folders: &[String], names: &[String], index: &Index) -> Option<String> {
        folders.iter().map(|f| self.expand(f)).filter(|d| is_dir(Path::new(d))).find_map(|d| find_in(index, &d, names)).or_else(|| self.search_roots().into_iter().filter(|d| is_dir(Path::new(d))).find_map(|d| find_in(index.deeper(), &d, names)))
    }

    /// Every copy of a console's emulators found, in the order they're found.
    fn find_all_exe(&self, folders: &[String], names: &[String], index: &Index) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let roots = self.search_roots();
        for dir in folders.iter().map(|f| self.expand(f)).chain(roots.iter().cloned()) {
            if out.len() >= 8 || !is_dir(Path::new(&dir)) {
                continue;
            }
            let at = if roots.contains(&dir) { index.deeper() } else { index };
            for n in names {
                if let Some(h) = find_in(at, &dir, std::slice::from_ref(n)).filter(|h| !out.iter().any(|x| x.to_lowercase() == h.to_lowercase())) {
                    out.push(h);
                }
            }
        }
        out.truncate(8);
        out
    }

    /// `<root>\ROMS\<system>` style folders.
    fn find_rom_dirs(&self, c: &Console) -> Vec<String> {
        let dirs_in = |d: &str| -> Vec<String> {
            read_dir_sorted(Path::new(d)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect()
        };
        let mut found: Vec<String> = Vec::new();
        for root in std::iter::once(self.app_root.to_string_lossy().into_owned()).chain(self.search_roots()).filter(|r| is_dir(Path::new(r))) {
            let holders = dirs_in(&root).into_iter().filter(|n| self.systems.rom_root_names.contains(&n.to_lowercase())).map(|n| js::join(&[&root, &n]));
            for holder in std::iter::once(root.clone()).chain(holders) {
                for n in dirs_in(&holder).into_iter().filter(|n| c.rom_folder_names.contains(&n.to_lowercase())) {
                    let d = js::join(&[&holder, &n]);
                    if !found.contains(&d) {
                        found.push(d);
                    }
                }
            }
        }
        found
    }

    fn console(&self, c: &Console, index: &Index) -> Found {
        let emulator = self.find_exe(&c.emulator_folders, &c.exe_names, index);
        let emulators = if emulator.is_some() { self.find_all_exe(&c.emulator_folders, &c.exe_names, index) } else { Vec::new() };
        let mut rom_dirs = self.find_rom_dirs(c);
        // games an emulator keeps in its own folders: Vita3K's installed games
        if c.id == "psvita" {
            for d in vita3k::app_dirs(emulator.as_deref().unwrap_or(""), &*self.var) {
                if !rom_dirs.contains(&d) {
                    rom_dirs.push(d);
                }
            }
        }
        Found { emulator, emulators, rom_dirs }
    }

    pub fn detect_all(&self) -> Detected {
        let index = Index::new(2, usize::MAX);
        let mut out = Detected::default();
        for c in &self.systems.consoles {
            let f = self.console(c, &index);
            if let Some(emu) = f.emulator.as_ref().filter(|e| out.retroarch.is_empty() && c.retro && is_retroarch(e)) {
                out.retroarch = emu.clone();
            }
            out.systems.push((c.id.clone(), f));
        }
        out.apps = self.systems.apps.iter().map(|a| (a.id.clone(), self.find_exe(&a.folders, &a.exe_names, &index))).collect();
        out.steam = crate::steam::find_steam().map(|p| p.to_string_lossy().into_owned());
        out
    }

    /// As `detect_all`, and the drives chosen looked through as well (Haseosama's fork): what they have is added to
    /// each console's finds, its emulator only where none was found in the usual places.
    pub fn detect_with(&self, drives: &[String]) -> Detected {
        let mut out = self.detect_all();
        let roots: Vec<PathBuf> = drives.iter().filter(|d| is_drive_root(d)).map(|d| PathBuf::from(format!("{}\\", d.trim_end_matches(['\\', '/'])))).collect();
        for (id, e) in self.scan_roots(&roots) {
            let Some((_, s)) = out.systems.iter_mut().find(|(i, _)| *i == id) else { continue };
            if let Some(emu) = &e.emulator {
                if !s.emulators.iter().any(|x| x.eq_ignore_ascii_case(emu)) {
                    s.emulators.push(emu.clone());
                }
                if s.emulator.is_none() {
                    s.emulator = Some(emu.clone());
                }
            }
            for d in e.rom_dirs {
                if !s.rom_dirs.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                    s.rom_dirs.push(d);
                }
            }
        }
        out
    }

    /// These folders looked through five folders deep, nearest the top first: emulators by their program's name, and
    /// games folders by a console's folder name, only somewhere a folder above says roms / games / emulation (a "psx"
    /// folder anywhere else is someone's photos). Each stops after 400,000 folders or three minutes. Keep in sync
    /// with main/detect.js scanDrives.
    pub fn scan_roots(&self, roots: &[PathBuf]) -> Vec<(String, Found)> {
        const SKIP: [&str; 12] = ["windows", "$recycle.bin", "system volume information", "node_modules", "appdata", "programdata", "winsxs", "recovery", "perflogs", "$windows.~bt", "msocache", ".git"];
        const ROMISH: [&str; 8] = ["rom", "game", "iso", "emul", "retro", "batocera", "recalbox", "jeux"];
        let mut names: HashMap<String, Vec<String>> = HashMap::new();
        let mut patterns: Vec<(Regex, String)> = Vec::new();
        let mut rom_names: HashMap<String, Vec<String>> = HashMap::new();
        for c in &self.systems.consoles {
            for n in &c.exe_names {
                match n.strip_prefix("re:") {
                    Some(p) => {
                        if let Ok(re) = Regex::new(&format!("(?i){p}")) {
                            patterns.push((re, c.id.clone()));
                        }
                    }
                    None => names.entry(n.to_lowercase()).or_default().push(c.id.clone()),
                }
            }
            for n in &c.rom_folder_names {
                rom_names.entry(n.to_lowercase()).or_default().push(c.id.clone());
            }
        }
        let mut found: Vec<(String, Found)> = Vec::new();
        fn get<'f>(found: &'f mut Vec<(String, Found)>, id: &str) -> &'f mut Found {
            let k = found.iter().position(|(i, _)| i == id).unwrap_or_else(|| {
                found.push((id.to_string(), Found::default()));
                found.len() - 1
            });
            &mut found[k].1
        }
        let mut emulator_hits: Vec<(String, String)> = Vec::new();
        let mut rom_hits: Vec<(String, String)> = Vec::new();
        for root in roots.iter().filter(|r| is_dir(r)) {
            let started = std::time::Instant::now();
            let mut queue: std::collections::VecDeque<(PathBuf, usize, bool)> = [(root.clone(), 0, false)].into();
            let mut seen = 0usize;
            while let Some((d, depth, romish)) = queue.pop_front() {
                seen += 1;
                if seen > 400_000 || started.elapsed().as_secs() >= 180 {
                    break;
                }
                for ent in read_dir_sorted(&d) {
                    let Ok(kind) = ent.file_type() else { continue };
                    let full = ent.path();
                    let low = ent.file_name().to_string_lossy().to_lowercase();
                    if kind.is_file() {
                        for id in names.get(&low).into_iter().flatten() {
                            emulator_hits.push((id.clone(), full.to_string_lossy().into_owned()));
                        }
                        if low.ends_with(".exe") {
                            for (_, id) in patterns.iter().filter(|(re, _)| re.is_match(&low)) {
                                emulator_hits.push((id.clone(), full.to_string_lossy().into_owned()));
                            }
                        }
                    } else if kind.is_dir() && !SKIP.contains(&low.as_str()) && !low.starts_with('$') {
                        if let Some(ids) = rom_names.get(&low).filter(|_| romish) {
                            for id in ids {
                                rom_hits.push((id.clone(), full.to_string_lossy().into_owned()));
                            }
                            continue;
                        }
                        if depth < 5 {
                            queue.push_back((full, depth + 1, romish || ROMISH.iter().any(|w| low.contains(w))));
                        }
                    }
                }
            }
        }
        for (id, exe) in emulator_hits {
            get(&mut found, &id).emulator.get_or_insert(exe);
        }
        for (id, dir) in rom_hits {
            let f = get(&mut found, &id);
            if !f.rom_dirs.contains(&dir) {
                f.rom_dirs.push(dir);
            }
        }
        found
    }

    /// (Haseosama's fork) A folder with one folder of games per console (RetroBat's roms: gc, wii, ps2 …), up to three
    /// folders down, by console id. A console's folder isn't looked into. Keep in sync with main/detect.js romFoldersIn.
    pub fn rom_folders_in(&self, dir: &str) -> Vec<(String, Vec<String>)> {
        static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\$recycle\.bin|system volume information|windows|node_modules)$").unwrap());
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        if dir.is_empty() || !is_dir(Path::new(dir)) {
            return out;
        }
        let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
        for c in &self.systems.consoles {
            for n in &c.rom_folder_names {
                by_name.entry(n.to_lowercase()).or_default().push(c.id.clone());
            }
        }
        let mut queue: std::collections::VecDeque<(PathBuf, usize)> = [(PathBuf::from(dir), 0)].into();
        let mut seen = 0;
        while let Some((d, depth)) = queue.pop_front() {
            seen += 1;
            if seen > 4000 {
                break;
            }
            for ent in read_dir_sorted(&d) {
                let name = ent.file_name().to_string_lossy().into_owned();
                if !ent.file_type().is_ok_and(|t| t.is_dir()) || name.starts_with('.') {
                    continue;
                }
                let full = ent.path().to_string_lossy().into_owned();
                if let Some(ids) = by_name.get(&name.to_lowercase()) {
                    for id in ids {
                        let k = out.iter().position(|(i, _)| i == id).unwrap_or_else(|| {
                            out.push((id.clone(), Vec::new()));
                            out.len() - 1
                        });
                        if !out[k].1.contains(&full) {
                            out[k].1.push(full.clone());
                        }
                    }
                    continue;
                }
                if depth < 2 && !SKIP.is_match(&name) {
                    queue.push_back((ent.path(), depth + 1));
                }
            }
        }
        out
    }

    /// One console's emulator and games folders (a console added in an update, found once as setup would have).
    pub fn detect_system(&self, id: &str) -> Option<Found> {
        Some(self.console(self.systems.console(id)?, &Index::new(2, usize::MAX)))
    }

    /// Portable emulators kept anywhere: the emulator programs in a folder the player picks (four folders deep), by
    /// console id, and RetroArch if it's among them.
    pub fn emulators_in(&self, dir: &str) -> (Vec<(String, String)>, String) {
        let (mut found, mut retroarch) = (Vec::new(), String::new());
        if dir.is_empty() || !is_dir(Path::new(dir)) {
            return (found, retroarch);
        }
        let index = Index::new(4, 4000);
        for c in self.systems.consoles.iter().filter(|c| !c.standalone && !c.exe_names.is_empty()) {
            if let Some(hit) = find_in(&index, dir, &c.exe_names) {
                if retroarch.is_empty() && c.retro && is_retroarch(&hit) {
                    retroarch = hit.clone();
                }
                found.push((c.id.clone(), hit));
            }
        }
        (found, retroarch)
    }
}

fn is_drive_root(d: &str) -> bool {
    let b = d.trim_end_matches(['\\', '/']).as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// A drive the drive search can look through (Windows): its letter, its root, and whether Windows is on it.
#[derive(Debug, Clone, PartialEq)]
pub struct Drive {
    pub letter: char,
    pub path: String,
    pub system: bool,
}

/// The drives there are (Windows; none elsewhere).
pub fn list_drives() -> Vec<Drive> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()).trim_end_matches(':').to_uppercase();
    (b'A'..=b'Z')
        .map(char::from)
        .filter(|l| std::fs::read_dir(format!("{l}:\\")).is_ok())
        .map(|l| Drive { letter: l, path: format!("{l}:\\"), system: l.to_string() == sys })
        .collect()
}

fn is_retroarch(exe: &str) -> bool {
    js::basename(exe).to_lowercase().contains("retroarch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_is_searched_deep_and_games_folders_only_under_a_games_folder() {
        let root = std::env::temp_dir().join(format!("rs-drives-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let d = Detector::new(&root);
        let gc = d.systems.console("gc").unwrap();
        let exe = gc.exe_names.iter().find(|n| !n.starts_with("re:")).unwrap().clone();
        for dir in ["Stuff/Emus/a/dolphin", "My Games/roms/gamecube", "Photos/gc", "Windows/gc", "a/b/c/d/e/f/roms/gc"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("Stuff/Emus/a/dolphin").join(&exe), b"").unwrap();
        let found = d.scan_roots(std::slice::from_ref(&root));
        let f = &found.iter().find(|(id, _)| id == "gc").unwrap().1;
        assert_eq!(f.emulator.as_deref(), Some(root.join("Stuff/Emus/a/dolphin").join(&exe).to_str().unwrap()));
        // a "gc" folder of photos, one under Windows and one six folders down are left alone
        assert_eq!(f.rom_dirs, vec![root.join("My Games/roms/gamecube").to_string_lossy().into_owned()]);
        assert!(is_drive_root("D:") && is_drive_root("d:\\") && !is_drive_root("D:\\Games") && !is_drive_root("/mnt"));
        // a RetroBat-style roms folder: each console's folder, its own subfolders not looked into
        std::fs::create_dir_all(root.join("RetroBat/roms/snes/snes")).unwrap();
        let per = d.rom_folders_in(root.join("RetroBat").to_str().unwrap());
        let get = |id: &str| per.iter().find(|(i, _)| i == id).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(get("snes"), vec![root.join("RetroBat/roms/snes").to_string_lossy().into_owned()]);
        assert!(get("gc").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}

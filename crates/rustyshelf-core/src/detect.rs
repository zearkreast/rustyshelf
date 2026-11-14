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
}

impl Index {
    fn new(max_depth: u32, max_dirs: usize) -> Index {
        Index { max_depth, max_dirs, cache: RefCell::new(HashMap::new()) }
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
        self.systems.search_roots.iter().map(|r| self.expand(r)).chain(self.media.iter().cloned()).collect()
    }

    /// The program in the emulator's usual folders, else a portable copy in a common place
    /// (Desktop\Emulators\Dolphin-x64\Dolphin.exe).
    fn find_exe(&self, folders: &[String], names: &[String], index: &Index) -> Option<String> {
        folders.iter().map(|f| self.expand(f)).chain(self.search_roots()).filter(|d| is_dir(Path::new(d))).find_map(|d| find_in(index, &d, names))
    }

    /// Every copy of a console's emulators found, in the order they're found.
    fn find_all_exe(&self, folders: &[String], names: &[String], index: &Index) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for dir in folders.iter().map(|f| self.expand(f)).chain(self.search_roots()) {
            if out.len() >= 8 || !is_dir(Path::new(&dir)) {
                continue;
            }
            for n in names {
                if let Some(h) = find_in(index, &dir, std::slice::from_ref(n)).filter(|h| !out.iter().any(|x| x.to_lowercase() == h.to_lowercase())) {
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

fn is_retroarch(exe: &str) -> bool {
    js::basename(exe).to_lowercase().contains("retroarch")
}

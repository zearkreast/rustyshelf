// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/paths.js (artBase, artDirsFor, the art library) and main/scanner.js (findArt), as RustyShelf has them.
//! Where a game's box art is: `<name>.png` for the front, `<name>-back`, `-spine`, `-disc`, manual pages
//! `<name>-manual-01.jpg` …, in the art folders of its console.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::config::{Config, SystemConfig};
use crate::paths::{expand, is_file};
use crate::{collate, sfo};

pub const ART_EXT: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

static UNSAFE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*]+"#).unwrap());
static GENERIC_DISC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(dvd|disc|game|image|iso|default|xbox ?360)\s*[0-9]*\.(iso|xex|zar)$").unwrap());

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Art {
    pub front: Option<PathBuf>,
    pub back: Option<PathBuf>,
    /// the back was made by RustyShelf (no scan of a real one yet)
    pub made_back: bool,
    pub spine: Option<PathBuf>,
    pub made_spine: bool,
    pub disc: Option<PathBuf>,
    /// manual pages, in page order
    pub manual: Vec<PathBuf>,
    pub manual_pdf: Option<PathBuf>,
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn parent_name(p: &Path) -> String {
    p.parent().map(file_name).unwrap_or_default()
}

/// node's path.resolve: absolute, with `.` and `..` worked out and no trailing slash.
pub fn resolve(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// A path as a case-blind key: resolved, lowercased.
pub fn path_key(p: &Path) -> String {
    resolve(p).to_string_lossy().to_lowercase()
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

fn console_folder_names() -> &'static std::collections::HashSet<String> {
    static NAMES: LazyLock<std::collections::HashSet<String>> = LazyLock::new(|| {
        let mut s: std::collections::HashSet<String> =
            ["roms", "games", "game", "ps4 games"].into_iter().map(String::from).collect();
        for c in &crate::systems::systems().consoles {
            s.extend(c.rom_folder_names.iter().map(|n| n.to_lowercase()));
        }
        s
    });
    &NAMES
}

/// The name a game's art files use: the file name without its extension, or for a game that's a folder (PS3 / PS4
/// eboot.bin, Wii U title.tmd, an Xbox 360 "DVD.iso") the folder's name.
pub fn art_base(file: &Path) -> String {
    let b = file_name(file);
    let low = b.to_lowercase();
    let parent = file.parent().unwrap_or(Path::new(""));
    if low == "eboot.bin" && file_name(parent).eq_ignore_ascii_case("usrdir") {
        let mut d = parent.parent().unwrap_or(Path::new(""));
        if file_name(d).eq_ignore_ascii_case("ps3_game") {
            d = d.parent().unwrap_or(Path::new(""));
        }
        return UNSAFE_NAME.replace_all(&file_name(d), "_").into_owned();
    }
    if low == "eboot.bin" || low == "title.tmd" || GENERIC_DISC.is_match(&b) {
        let dir = parent_name(file);
        // a game unpacked loose into the console's own games folder: that folder's name would pass to whatever game is
        // put there next, so the game's id names its art
        if low == "eboot.bin" && console_folder_names().contains(dir.trim().to_lowercase().as_str()) {
            if let Some(id) = sfo::read(&parent.join("sce_sys").join("param.sfo"))
                .and_then(|s| sfo::text(&s, "TITLE_ID").map(str::to_string))
                .filter(|id| !id.is_empty())
            {
                return id;
            }
        }
        return UNSAFE_NAME.replace_all(&dir, "_").into_owned();
    }
    // unpacked Switch 2 games (every one's file is "main") are named by the game's folder
    if low == "main" && parent_name(file).eq_ignore_ascii_case("exefs") {
        return UNSAFE_NAME.replace_all(&file_name(&crate::nx2::game_root(file)), "_").into_owned();
    }
    match b.rfind('.') {
        Some(i) if i + 1 < b.len() => b[..i].to_string(),
        _ => b,
    }
}

/// Settings → Box art → the art library: one folder holding every console's art, a folder for each games folder.
#[derive(Debug, Clone, Default)]
pub struct ArtLibrary {
    root: Option<PathBuf>,
    /// every games folder that has a folder in the library, resolved and lower case
    roots: Vec<String>,
}

fn lib_name(r: &Path) -> String {
    let n = file_name(r);
    let n = if n.is_empty() { "Games".to_string() } else { n };
    n.replace(['<', '>', ':', '"', '|', '?', '*'], "_")
}

impl ArtLibrary {
    pub fn from_config(cfg: &Config, app_root: &Path) -> ArtLibrary {
        let want = cfg.art_library();
        let root = (!want.is_empty()).then(|| resolve(Path::new(&expand(&want, app_root))));
        let mut roots: Vec<String> = Vec::new();
        for r in cfg.library_roots() {
            let r = path_key(Path::new(&r));
            if !roots.contains(&r) {
                roots.push(r);
            }
        }
        ArtLibrary { root, roots }
    }

    /// The library's own folder, when there is one.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn plain(&self, lib: &Path, games: &Path) -> PathBuf {
        lib.join(lib_name(&resolve(games)))
    }

    /// Two games folders of the same name ("D:\Games", "E:\Games") would share a library folder.
    fn clashes(&self, games: &Path) -> bool {
        let r = resolve(games);
        let (me, name) = (r.to_string_lossy().to_lowercase(), lib_name(&r).to_lowercase());
        self.roots.iter().any(|o| *o != me && lib_name(Path::new(o)).to_lowercase() == name)
    }

    /// The library's folder for one games folder.
    pub fn folder(&self, games: &Path) -> Option<PathBuf> {
        let lib = self.root.as_deref()?;
        if !self.clashes(games) {
            return Some(self.plain(lib, games));
        }
        let r = resolve(games);
        static SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"|?*\\/]+"#).unwrap());
        let parent = r.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let place = SEP.replace_all(&parent, " ").trim().to_string();
        let place = if place.is_empty() { "root".to_string() } else { place };
        Some(lib.join(format!("{} ({place})", lib_name(&r))))
    }

    /// Where a game's art goes: the console's own art folder, else with an art library the games folder's `covers`
    /// folder in it, else `<games folder>\covers`.
    pub fn dir_for(&self, sc: &SystemConfig, file: &Path, app_root: &Path) -> PathBuf {
        if !sc.art_dir.is_empty() {
            return PathBuf::from(expand(&sc.art_dir, app_root));
        }
        let inside = |d: &&String| {
            let (f, d) = (path_key(file), path_key(Path::new(d)));
            f.starts_with(&if d.ends_with(std::path::MAIN_SEPARATOR) { d } else { format!("{d}{}", std::path::MAIN_SEPARATOR) })
        };
        let root = sc.rom_dirs.iter().find(inside).map(PathBuf::from).unwrap_or_else(|| file.parent().unwrap_or(Path::new("")).to_path_buf());
        self.folder(&root).unwrap_or(root).join("covers")
    }

    /// Where a game's manual goes: as its art, but with an art library the games folder's `manuals` folder in it.
    pub fn manual_dir_for(&self, sc: &SystemConfig, file: &Path, app_root: &Path) -> PathBuf {
        let covers = self.dir_for(sc, file, app_root);
        if sc.art_dir.is_empty() && self.root.is_some() { covers.with_file_name("manuals") } else { covers }
    }

    /// The folders a console's art is looked for in, first match wins.
    pub fn dirs_for(&self, sc: &SystemConfig, app_root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if !sc.art_dir.is_empty() {
            out.push(PathBuf::from(expand(&sc.art_dir, app_root)));
        }
        if let Some(lib) = self.root.as_deref() {
            for d in &sc.rom_dirs {
                let d = Path::new(d);
                let f = self.folder(d).expect("library set");
                out.push(f.join("covers"));
                out.push(f.join("manuals"));
                if self.clashes(d) {
                    let p = self.plain(lib, d);
                    out.push(p.join("covers"));
                    out.push(p.join("manuals"));
                }
            }
        }
        for d in &sc.rom_dirs {
            let d = Path::new(d);
            out.push(d.join("covers"));
            out.push(d.join("media").join("covers"));
            out.push(d.join("boxart"));
        }
        out
    }
}

/// One listing per art folder for a whole scan (lower-case name → the name on disk).
#[derive(Default)]
pub struct ArtIndex {
    listings: HashMap<PathBuf, Option<HashMap<String, String>>>,
    manuals: HashMap<PathBuf, Option<HashMap<String, Vec<PathBuf>>>>,
}

impl ArtIndex {
    fn listing(&mut self, dir: &Path) -> Option<&HashMap<String, String>> {
        self.listings
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let rd = std::fs::read_dir(dir).ok()?;
                Some(
                    rd.filter_map(Result::ok)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .map(|n| (n.to_lowercase(), n))
                        .collect(),
                )
            })
            .as_ref()
    }

    fn manual_index(&mut self, dir: &Path) -> Option<&HashMap<String, Vec<PathBuf>>> {
        if !self.manuals.contains_key(dir) {
            let idx = self.listing(dir).map(|l| {
                let mut idx: HashMap<String, Vec<(String, String)>> = HashMap::new();
                for (low, real) in l {
                    let Some(i) = low.rfind("-manual-").filter(|&i| i > 0) else { continue };
                    let ext = low.rsplit_once('.').map_or("", |(_, e)| e);
                    if !ART_EXT.contains(&ext) {
                        continue;
                    }
                    idx.entry(low[..i].to_string()).or_default().push((low.clone(), real.clone()));
                }
                idx.into_iter()
                    .map(|(k, mut pages)| {
                        pages.sort_by(|a, b| collate::compare(&a.0, &b.0));
                        (k, pages.into_iter().map(|(_, real)| dir.join(real)).collect())
                    })
                    .collect()
            });
            self.manuals.insert(dir.to_path_buf(), idx);
        }
        self.manuals.get(dir).and_then(Option::as_ref)
    }

    fn look(&mut self, dirs: &[PathBuf], names: &[String], suffix: &str) -> Option<PathBuf> {
        for dir in dirs {
            let Some(l) = self.listing(dir) else { continue };
            for n in names {
                for e in ART_EXT {
                    if let Some(real) = l.get(&format!("{n}{suffix}.{e}").to_lowercase()) {
                        return Some(dir.join(real));
                    }
                }
            }
        }
        None
    }

    /// A game's art, looked for under its file's name, then under its title.
    pub fn find(&mut self, file: &Path, title: &str, dirs: &[PathBuf]) -> Art {
        let mut names = vec![art_base(file)];
        if names[0] != title {
            names.push(title.to_string());
        }
        let mut manual = Vec::new();
        'dirs: for dir in dirs {
            let Some(idx) = self.manual_index(dir) else { continue };
            for n in &names {
                if let Some(pages) = idx.get(&n.to_lowercase()) {
                    manual = pages.clone();
                    break 'dirs;
                }
            }
        }
        let mut manual_pdf = None;
        if manual.is_empty() {
            'pdf: for dir in dirs {
                let Some(l) = self.listing(dir) else { continue };
                for n in &names {
                    if let Some(real) = l.get(&format!("{n}-manual.pdf").to_lowercase()) {
                        manual_pdf = Some(dir.join(real));
                        break 'pdf;
                    }
                }
            }
        }
        let back = self.look(dirs, &names, "-back");
        let made_back = if back.is_none() { self.look(dirs, &names, "-backmade") } else { None };
        let spine = self.look(dirs, &names, "-spine");
        let made_spine = if spine.is_none() { self.look(dirs, &names, "-spinemade") } else { None };
        Art {
            front: self.look(dirs, &names, ""),
            made_back: made_back.is_some(),
            back: back.or(made_back),
            made_spine: made_spine.is_some(),
            spine: spine.or(made_spine),
            disc: self.look(dirs, &names, "-disc"),
            manual,
            manual_pdf,
        }
    }
}

/// A file that's there, for art a game folder carries itself (icon0.png).
pub fn existing(p: PathBuf) -> Option<PathBuf> {
    is_file(&p).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_names() {
        assert_eq!(art_base(Path::new("/g/Metroid Prime (USA).rvz")), "Metroid Prime (USA)");
        assert_eq!(art_base(Path::new("/g/Some Game/PS3_GAME/USRDIR/EBOOT.BIN")), "Some Game");
        assert_eq!(art_base(Path::new("/g/Halo 3/DVD1.iso")), "Halo 3");
        assert_eq!(art_base(Path::new("/g/noext")), "noext");
        assert_eq!(art_base(Path::new("/g/Mario Kart World/base/exefs/main")), "Mario Kart World");
    }

    #[test]
    fn library_folders() {
        let cfg = Config::parse(
            r#"{"ui":{"artLibrary":"/lib"},"systems":{"gc":{"romDirs":["/d/Games"]},"wii":{"romDirs":["/e/Games"]},"psx":{"romDirs":["/e/psx"]}}}"#,
        )
        .unwrap();
        let lib = ArtLibrary::from_config(&cfg, Path::new("/app"));
        assert_eq!(lib.folder(Path::new("/e/psx")).unwrap(), Path::new("/lib/psx"));
        assert_eq!(lib.folder(Path::new("/d/Games")).unwrap(), Path::new("/lib/Games (d)"));
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/scanner.js, as RustyShelf has it.
//! Walks a console's games folders and makes one shelf entry per game: disc 2 joins disc 1, two dumps of a game are
//! one box (the fuller one), updates and DLC stay off the shelf.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use regex::Regex;

use crate::arcade::{self, Set};
use crate::art::{resolve, Art, ArtIndex};
use crate::config::SystemConfig;
use crate::gamelist::Gamelists;
use crate::systems::Console;
use crate::titles::{clean_title, CleanTitle};
use crate::x360::{self, Package};
use crate::{collate, kyty, nx2, pkg, sfo};

// "[UPD]", "(DLC)", or the word after a separator ("Game - Update v1.0.1", "Game_DLC"); not anywhere ("DLC Quest")
static SKIP_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[\[(](upd|update|dlc)[\])]|[\s._-](update|dlc)([^a-z0-9]|$)").unwrap());
// a game's own data, or packages that aren't games: never walked into
static SKIP_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(dlc|updates?|patches|mods|sce_sys|sce_module|romfs|exefs)$").unwrap());
static TM: LazyLock<Regex> = LazyLock::new(|| Regex::new("[™®]").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static PS_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)([A-Z]{4}[0-9]{5})(?-u:\b)").unwrap());
static PS_ID_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{4}[0-9]{5}").unwrap());
static PS3_REGION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[BN][CLP]([A-Z])([A-Z])").unwrap());
static WIIU_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)(0005000[0-9A-F])([0-9A-F]{8})(?-u:\b)").unwrap());
static WIIU_KIND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)0005000([0-9A-F])[0-9A-F]{8}(?-u:\b)").unwrap());
static UPDATE_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?-u:\b)(update|dlc)(?-u:\b)").unwrap());
static HEX_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[0-9A-F]{16}$").unwrap());
static DATA_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.data$").unwrap());
// what an Xbox 360 game's disc is often called, which says nothing of the game
static GENERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(dvd|disc|game|image|iso|default|xbox ?360)\s*([0-9]*)$").unwrap());
static DISC_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\(disc [0-9]+\)$").unwrap());
// BIOS sets that sit beside the arcade games, hidden even without the arcade name list
const ARCADE_BIOS: [&str; 26] = [
    "neogeo", "pgm", "skns", "stvbios", "decocass", "cpzn1", "cpzn2", "taitofx1", "konamigx", "nss", "naomi", "awbios", "hod2bios",
    "f355bios", "airlbios", "megaplay", "megatech", "playch10", "qsound", "neocdz", "isgsm", "nmk004", "ym2608", "bubsys", "cchip", "sp-cpu",
];

/// Where a game comes from, which decides how it's started.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Source {
    /// a file in a console's games folder, started in its emulator
    #[default]
    Rom,
    /// started through Steam
    Steam {
        appid: String,
        minutes: u64,
        /// seconds since 1970
        last_played: u64,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Game {
    /// `<console id>:<file>`, as the Electron app has it (history and favourites use it)
    pub id: String,
    pub title: String,
    pub region: String,
    pub disc: u32,
    pub file: PathBuf,
    pub ext: String,
    pub size_bytes: u64,
    /// when the file arrived (ms since 1970)
    pub added: u64,
    /// every disc of the game
    pub discs: Vec<PathBuf>,
    pub title_id: Option<String>,
    pub art: Art,
    /// a picture for behind the game: the game folder's own (PS3 PIC1.PNG) or a gamelist.xml's
    pub hero: Option<PathBuf>,
    /// a PS4 .pkg that has to be extracted before it can be played
    pub not_installed: bool,
    /// how well the game runs in Xenia, from the Xbox 360 title list
    pub compat: Option<String>,
    pub source: Source,
}

/// What a scan reads from the data folder besides the settings: lists the Electron app downloads, and sizes it measured.
#[derive(Debug, Clone, Default)]
pub struct ScanData {
    pub x360: x360::TitleList,
    pub arcade: Option<arcade::Names>,
    /// `data/dir-sizes.json`: a folder (resolved) → its size in bytes, as the Electron app last measured it
    pub dir_sizes: HashMap<String, u64>,
}

impl ScanData {
    pub fn load(data_dir: &Path) -> ScanData {
        let sizes: HashMap<String, serde_json::Value> =
            fs::read_to_string(data_dir.join("dir-sizes.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        ScanData {
            x360: x360::TitleList::load(data_dir),
            arcade: arcade::Names::load(data_dir),
            dir_sizes: sizes.into_iter().filter_map(|(k, v)| Some((k, v.get("bytes")?.as_f64()? as u64))).collect(),
        }
    }

    // TODO: measure a folder the Electron app hasn't (an unpacked Switch 2 game shows no size until it has)
    fn dir_size(&self, dir: &Path) -> u64 {
        if !dir.is_absolute() || fs::metadata(dir).is_err() {
            return 0;
        }
        self.dir_sizes.get(&resolve(dir).to_string_lossy().into_owned()).copied().unwrap_or(0)
    }
}

struct Found {
    game: Game,
    art_title: String,
    icon: Option<PathBuf>,
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// node's path.extname without the dot: "" for ".hidden" and "name."
fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

fn is_file(p: &Path) -> bool {
    p.metadata().map(|m| m.is_file()).unwrap_or(false)
}

/// A folder's entries in the order node's readdir gives them: by name on Unix (libuv sorts them), as stored on Windows.
/// Which disc comes first and which of two equal dumps stays depend on it.
pub(crate) fn read_dir_sorted(dir: &Path) -> Vec<fs::DirEntry> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    #[allow(unused_mut)]
    let mut ents: Vec<_> = rd.filter_map(Result::ok).collect();
    #[cfg(unix)]
    ents.sort_by_key(|e| e.file_name());
    ents
}

fn walk(dir: &Path, exts: &[String], depth: u32, out: &mut Vec<PathBuf>, game_file: Option<&str>) {
    let ents = read_dir_sorted(dir);
    let dir_name = name_of(dir);
    // a PS4 / PS5 game's folder: its eboot.bin is the game, and its data folders hold no other
    if game_file == Some("eboot.bin") && !dir_name.eq_ignore_ascii_case("usrdir") {
        let eboot = ents.iter().find(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false) && e.file_name().to_string_lossy().eq_ignore_ascii_case("eboot.bin")
        });
        if let Some(e) = eboot {
            let sce = dir.join("sce_sys");
            if is_file(&sce.join("param.sfo")) || is_file(&sce.join("param.json")) {
                out.push(e.path());
                return;
            }
        }
    }
    let stfs = exts.iter().any(|e| e == "stfs");
    // an extracted Xbox 360 game's folder: its subfolders are the game's data, never another game
    let xex_here = game_file.is_none()
        && exts.iter().any(|e| e == "xex")
        && ents.iter().any(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false) && e.file_name().to_string_lossy().eq_ignore_ascii_case("default.xex")
        });
    for ent in ents {
        let name = ent.file_name().to_string_lossy().into_owned();
        let full = ent.path();
        let Ok(ft) = ent.file_type() else { continue };
        // a link to a folder or game elsewhere is followed (the depth limit stops a loop)
        let (is_dir, is_file) = if ft.is_symlink() {
            match fs::metadata(&full) {
                Ok(m) => (m.is_dir(), m.is_file()),
                Err(_) => continue,
            }
        } else {
            (ft.is_dir(), ft.is_file())
        };
        if is_dir {
            if game_file.is_some() && dir_name.eq_ignore_ascii_case("usrdir") {
                continue;
            }
            // a Games on Demand package's data folder: "<hash>.data"
            if xex_here || (stfs && DATA_DIR.is_match(&name)) {
                continue;
            }
            if depth > 0 && !name.starts_with('.') && !SKIP_DIR.is_match(&name) {
                walk(&full, exts, depth - 1, out, game_file);
            }
            continue;
        }
        if !is_file {
            continue;
        }
        if let Some(gf) = game_file {
            if name.to_lowercase() == gf {
                out.push(full);
            }
            continue;
        }
        let ext = ext_of(&name);
        if exts.contains(&ext) && !SKIP_NAME.is_match(&name) {
            out.push(full);
        } else if ext.is_empty() && stfs && x360::is_stfs_candidate(&full) {
            // a Games on Demand / Arcade package: its header decides later whether it's a game
            out.push(full);
        }
    }
}

fn clean(s: &str) -> String {
    TM.replace_all(&SPACES.replace_all(s, " "), "").trim().to_string()
}

/// A PS3 game's region from its id: BLUS / NPUB… USA, BLES / NPEB… Europe, BLJM / NPJB… Japan …
pub fn ps3_region(id: &str) -> &'static str {
    let Some(m) = PS3_REGION.captures(id) else { return "" };
    match format!("{}{}", &m[1], &m[2]).to_uppercase().as_str() {
        "US" | "UB" | "UA" => "USA",
        "ES" | "EB" | "EA" => "Europe",
        "JS" | "JM" | "JB" | "JA" | "JJ" => "Japan",
        "AS" | "HA" | "HB" => "Asia",
        "KS" | "KB" => "Korea",
        _ => "",
    }
}

struct FolderGame {
    title: String,
    title_id: Option<String>,
    region: String,
    icon: Option<PathBuf>,
    hero: Option<PathBuf>,
    /// an unpacked Switch 2 game's folder, whose size is the game's
    folder: Option<PathBuf>,
}

/// PS3: EBOOT.BIN in USRDIR; PARAM.SFO, ICON0.PNG and PIC1.PNG one level up (PS3_GAME for discs, the NPxx00000 folder
/// for digital games). The game's name folder is above PS3_GAME. None: installed game data or a patch.
fn ps3_game(file: &Path) -> Option<FolderGame> {
    let usr = file.parent()?;
    if !name_of(usr).eq_ignore_ascii_case("usrdir") {
        return None;
    }
    let base = usr.parent()?;
    let sfo = sfo::read(&base.join("PARAM.SFO"));
    let cat = sfo.as_ref().and_then(|s| sfo::text(s, "CATEGORY")).unwrap_or("");
    if ["GD", "2P", "AT"].iter().any(|c| c.eq_ignore_ascii_case(cat)) {
        return None;
    }
    let is_disc = name_of(base).eq_ignore_ascii_case("ps3_game");
    let folder = if is_disc { base.parent()? } else { base };
    let mut title = sfo.as_ref().and_then(|s| sfo::text(s, "TITLE")).map(clean).unwrap_or_default();
    if title.is_empty() {
        title = TM.replace_all(&name_of(folder), "").trim().to_string();
    }
    let title_id = sfo
        .as_ref()
        .and_then(|s| sfo::text(s, "TITLE_ID"))
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| PS_ID.captures(&format!("{} {}", name_of(folder), name_of(base))).map(|c| c[1].to_string()));
    let region = ps3_region(title_id.as_deref().unwrap_or("")).to_string();
    Some(FolderGame {
        title,
        region,
        title_id,
        icon: crate::art::existing(base.join("ICON0.PNG")),
        hero: crate::art::existing(base.join("PIC1.PNG")),
        folder: None,
    })
}

/// A PS4 / Vita / PS5 game folder: the title from param.sfo (a PS5 game's param.json), else the folder's name
/// ("CUSA00207": the folder above, unless that's the games folder itself).
fn folder_game(file: &Path, rom_dirs: &[String]) -> FolderGame {
    let dir = file.parent().unwrap_or(Path::new(""));
    let sfo = sfo::read(&dir.join("sce_sys").join("param.sfo"));
    let (sfo_title, title_id) = match &sfo {
        Some(s) => (sfo::text(s, "TITLE").map(String::from), sfo::text(s, "TITLE_ID").map(String::from)),
        None => match kyty::read_param(dir) {
            Some(p) => (Some(p.title), Some(p.title_id)),
            None => (None, None),
        },
    };
    let mut title = sfo_title.map(|t| SPACES.replace_all(&t, " ").trim().to_string()).unwrap_or_default();
    let folder = name_of(dir);
    let parent = dir.parent().unwrap_or(Path::new(""));
    if title.is_empty() {
        let parent_is_root = rom_dirs.iter().any(|d| resolve(Path::new(d)) == resolve(parent));
        title = if PS_ID_START.is_match(&folder) && !parent_is_root && !name_of(parent).is_empty() { name_of(parent) } else { folder };
    }
    let sce = dir.join("sce_sys");
    // PS5 games often have only pic0
    let pic = crate::art::existing(sce.join("pic1.png")).or_else(|| crate::art::existing(sce.join("pic0.png")));
    FolderGame {
        title: TM.replace_all(&title, "").trim().to_string(),
        title_id: title_id.filter(|t| !t.is_empty()),
        region: String::new(),
        icon: crate::art::existing(sce.join("icon0.png")),
        hero: pic,
        folder: None,
    }
}

/// An extracted Wii U game: <game>/code/<name>.rpx, its title in <game>/meta/meta.xml (longname_en, else shortname_en).
fn wiiu_folder(file: &Path) -> Option<(PathBuf, String)> {
    let code = file.parent()?;
    if !name_of(code).eq_ignore_ascii_case("code") {
        return None;
    }
    let root = code.parent()?.to_path_buf();
    let xml = fs::read_to_string(root.join("meta").join("meta.xml")).unwrap_or_default();
    let tag = |t: &str| {
        let re = Regex::new(&format!(r"(?i)<{t}[^>]*>([^<]*)</{t}>")).ok()?;
        let raw = re.captures(&xml)?[1].to_string();
        static ENT: LazyLock<Regex> = LazyLock::new(|| Regex::new("&#([0-9]+);").unwrap());
        let s = ENT.replace_all(&raw, |c: &regex::Captures| match c[1].parse::<u32>() {
            Ok(10 | 13) => " ".to_string(),
            Ok(n) => char::from_u32(n).map(String::from).unwrap_or_default(),
            Err(_) => String::new(),
        });
        let s = s.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"");
        Some(SPACES.replace_all(&s, " ").trim().to_string()).filter(|s| !s.is_empty())
    };
    let title = tag("longname_en").or_else(|| tag("shortname_en")).unwrap_or_default();
    Some((root, title))
}

fn with_ext(p: &Path, ext: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

fn added_ms(m: &fs::Metadata) -> u64 {
    let t = m.created().or_else(|_| m.modified()).ok();
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

fn walk_files(console: &Console, sc: &SystemConfig) -> Vec<PathBuf> {
    let deep = console.game_file.is_some() || console.deep || console.id == "x360";
    let depth = if deep { 6 } else { 4 };
    let game_file = console.game_file.as_deref();
    // "stfs": Games on Demand / Arcade packages, which have no extension
    let mut exts = console.extensions.clone();
    if console.id == "x360" {
        exts.push("stfs".into());
    }
    let mut files = Vec::new();
    for dir in &sc.rom_dirs {
        walk(Path::new(dir), &exts, depth, &mut files, game_file);
    }
    if !console.also_files.is_empty() {
        for dir in &sc.rom_dirs {
            walk(Path::new(dir), &console.also_files, 4, &mut files, None);
        }
    }
    if console.id == "sw2" {
        let packed = std::mem::take(&mut files);
        for dir in &sc.rom_dirs {
            nx2::find(Path::new(dir), 3, &mut files);
        }
        let sep = std::path::MAIN_SEPARATOR_STR;
        let roots: Vec<String> = files.iter().map(|f| nx2::game_root(f).to_string_lossy().to_lowercase() + sep).collect();
        // a .nsp / .xci / .nup kept inside an unpacked game's folder is that game again
        for f in packed {
            let low = f.to_string_lossy().to_lowercase();
            if !roots.iter().any(|r| low.starts_with(r.as_str())) {
                files.push(f);
            }
        }
    }
    files
}

/// PS4 .pkg packages: on the shelf as "Not installed" unless the installed copy is.
fn add_packages(console: &Console, sc: &SystemConfig, by_title: &mut HashMap<String, Found>, order: &mut Vec<String>) {
    let mut pkgs = Vec::new();
    for dir in &sc.rom_dirs {
        walk(Path::new(dir), &["pkg".to_string()], 4, &mut pkgs, None);
    }
    let installed: HashSet<String> = by_title.values().filter_map(|f| f.game.title_id.clone()).collect();
    for f in pkgs {
        let sfo = pkg::info(&f).unwrap_or_default();
        let cat = match sfo.get("CATEGORY") {
            Some(sfo::SfoValue::Text(t)) if !t.is_empty() => t.to_lowercase(),
            Some(sfo::SfoValue::Int(n)) if *n != 0 => n.to_string(),
            _ => "gd".into(),
        };
        // patches and add-ons aren't games
        if cat != "gd" {
            continue;
        }
        let id = sfo::text(&sfo, "TITLE_ID")
            .filter(|t| !t.is_empty())
            .map(String::from)
            .or_else(|| pkg::NAME_ID.find(&name_of(&f)).map(|m| m.as_str().to_uppercase()))
            .unwrap_or_default();
        if !id.is_empty() && installed.contains(&id) {
            continue;
        }
        let from_sfo = sfo::text(&sfo, "TITLE").map(|t| TM.replace_all(&SPACES.replace_all(t, " "), "").trim().to_string());
        let title = [from_sfo.unwrap_or_default(), pkg::title_from_name(&f), name_of(&f)].into_iter().find(|t| !t.is_empty()).unwrap_or_default();
        let key = format!("{}|pkg", title.to_lowercase());
        if by_title.contains_key(&key) {
            continue;
        }
        order.push(key.clone());
        by_title.insert(
            key,
            Found {
                art_title: title.clone(),
                icon: None,
                game: Game {
                    id: format!("{}:{}", console.id, f.display()),
                    title,
                    region: "Not installed".into(),
                    disc: 0,
                    ext: "pkg".into(),
                    size_bytes: fs::metadata(&f).map_or(0, |m| m.len()),
                    added: 0,
                    discs: vec![f.clone()],
                    file: f,
                    title_id: Some(id).filter(|i| !i.is_empty()),
                    art: Art::default(),
                    hero: None,
                    not_installed: true,
                    compat: None,
                    source: Source::Rom,
                },
            },
        );
    }
}

/// Xbox 360: a disc 2+ (a bonus or content disc, with its own title id) joins its disc 1, found by the disc 1's file
/// name, else in the same game folder. On its own it says which disc it is, so it isn't taken for the game.
// TODO: games with no usable file name ("DVD1.iso") share the `None` name, as they share `undefined` in the Electron
// app: a disc 2 of one joins the last of them, not the disc 1 in its own folder. Fix both apps together.
fn join_x360_discs(sc: &SystemConfig, by_title: &mut HashMap<String, Found>, order: &[String], by_name: &HashMap<String, String>) {
    let later = |f: &Found| f.game.disc > 1;
    let mut firsts: HashMap<Option<String>, String> = HashMap::new();
    for k in order {
        if by_title.get(k).is_some_and(|f| !later(f)) {
            firsts.insert(by_name.get(k).cloned(), k.clone());
        }
    }
    let roots: Vec<String> = sc.rom_dirs.iter().map(|d| resolve(Path::new(d)).to_string_lossy().to_lowercase()).collect();
    let sep = std::path::MAIN_SEPARATOR_STR;
    for k in order {
        let Some(g) = by_title.get(k).filter(|g| later(g)) else { continue };
        let mut host = firsts.get(&by_name.get(k).cloned()).cloned();
        if host.is_none() {
            let dir = g.game.file.parent().map(|d| resolve(d).to_string_lossy().to_lowercase()).unwrap_or_default();
            if !roots.contains(&dir) {
                let inside = dir + sep;
                host = order
                    .iter()
                    .filter(|h| *h != k)
                    .find(|h| {
                        by_title.get(*h).is_some_and(|h| !later(h) && resolve(&h.game.file).to_string_lossy().to_lowercase().starts_with(&inside))
                    })
                    .cloned();
            }
        }
        match host {
            Some(h) => {
                let g = by_title.remove(k).unwrap();
                by_title.get_mut(&h).unwrap().game.discs.push(g.game.file);
            }
            None => {
                let g = by_title.get_mut(k).unwrap();
                if !DISC_TAG.is_match(&g.game.title) {
                    g.game.title = format!("{} (Disc {})", g.game.title, g.game.disc);
                }
            }
        }
    }
}

/// One console's games, sorted by title, with their art.
pub fn scan(console: &Console, sc: &SystemConfig, art_dirs: &[PathBuf], data: &ScanData) -> Vec<Game> {
    let files = walk_files(console, sc);
    let switch = console.id == "sw" || console.id == "sw2";
    let (is_x360, is_sw2) = (console.id == "x360", console.id == "sw2");
    let ct = |f: &Path| clean_title(&name_of(f), switch);
    let is_root = |d: &Path| sc.rom_dirs.iter().any(|r| resolve(Path::new(r)) == resolve(d));
    // an extracted Xbox 360 game: default.xex is the game, the other .xex beside it are helpers
    let xex_dirs: HashSet<PathBuf> = files
        .iter()
        .filter(|f| name_of(f).eq_ignore_ascii_case("default.xex"))
        .filter_map(|f| f.parent().map(Path::to_path_buf))
        .collect();

    let mut by_title: HashMap<String, Found> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut disc_of: HashMap<PathBuf, (u32, u64)> = HashMap::new();
    // Xbox 360: the title of each game's file name, to find a later disc's disc 1
    let mut by_name: HashMap<String, String> = HashMap::new();

    for f in files {
        let fname = name_of(&f);
        let is_xex = fname.to_lowercase().ends_with(".xex");
        let is_default = is_xex && fname.eq_ignore_ascii_case("default.xex");
        if is_xex && !is_default && f.parent().is_some_and(|p| xex_dirs.contains(p)) {
            continue;
        }
        let as_folder = console.game_file.as_deref().is_some_and(|g| fname.to_lowercase() == g);
        let mut fg = None;
        if as_folder && console.id == "ps3" {
            match ps3_game(&f) {
                Some(g) => fg = Some(g),
                None => continue,
            }
        }
        if is_sw2 {
            let n = if nx2::is_unpacked(&f) { nx2::info(&f) } else { nx2::file_info(&f) };
            fg = Some(FolderGame {
                title: n.title,
                title_id: Some(n.title_id).filter(|t| !t.is_empty()),
                region: String::new(),
                icon: None,
                hero: None,
                folder: Some(n.folder),
            });
        }
        // an Xbox 360 package is a game only if its header says so (not a save, DLC or update)
        let stfs: Option<Package> = if is_x360 && x360::no_extname(&f) {
            match x360::stfs_info(&f) {
                Some(p) => Some(p),
                None => continue,
            }
        } else {
            None
        };
        let mut info: CleanTitle = if let Some(p) = &stfs {
            let title = if p.title.is_empty() { format!("Xbox 360 game {}", p.title_id) } else { p.title.clone() };
            CleanTitle { title, ..Default::default() }
        } else if as_folder || fg.is_some() {
            let g = fg.get_or_insert_with(|| folder_game(&f, &sc.rom_dirs));
            CleanTitle { title: g.title.clone(), region: g.region.clone(), ..Default::default() }
        } else if is_default {
            ct(&with_ext(f.parent().unwrap_or(Path::new("")), ".xex"))
        } else {
            ct(&f)
        };
        if arcade::SYSTEMS.contains(&console.id.as_str()) {
            let set = fname.rsplit_once('.').map_or(fname.as_str(), |(s, _)| s).to_lowercase();
            match data.arcade.as_ref().and_then(|a| a.lookup(&set)) {
                Some(Set::Bios) => continue,
                None if ARCADE_BIOS.contains(&set.as_str()) => continue,
                // "A / B": two names for one game, the first is the shelf's; versions of a game ("(World 910522)",
                // "(USA 910206)") are one box, the fuller zip kept
                Some(Set::Game(t)) if !t.is_empty() => {
                    let first = t.split(" / ").next().unwrap_or("").replace(['\\', '/'], "-");
                    info = clean_title(&format!("{first}.zip"), switch);
                    info.region = String::new();
                }
                _ => {}
            }
        }
        let mut nus_bytes = 0;
        if console.id == "wiiu" && fname.to_lowercase().ends_with(".tmd") {
            // a NUS folder ("Game [0005000010xxxxxx]/title.tmd + *.app") is the game; updates (0005000E…) and DLC
            // (0005000C…) aren't
            if !fname.eq_ignore_ascii_case("title.tmd") {
                continue;
            }
            let dir = f.parent().unwrap_or(Path::new(""));
            let dir_name = name_of(dir);
            let id = WIIU_ID.captures(&dir_name);
            if id.as_ref().is_some_and(|c| c[1].to_uppercase() == "0005000C" || c[1].to_uppercase() == "0005000E") {
                continue;
            }
            info = ct(&with_ext(dir, ".tmd"));
            if info.title.is_empty() || HEX_ID.is_match(&info.title) {
                info.title = format!("Wii U game {}", id.map_or(dir_name.clone(), |c| c[0].to_string()));
            }
            nus_bytes = fs::read_dir(dir)
                .map(|rd| rd.filter_map(Result::ok).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
                .unwrap_or(0);
        }
        if console.id == "wiiu" && fname.to_lowercase().ends_with(".rpx") {
            let Some((root, title)) = wiiu_folder(&f) else { continue };
            info = ct(&with_ext(&root, ".rpx"));
            if !title.is_empty() {
                info.title = TM.replace_all(&title, "").trim().to_string();
            }
        }
        let mut x = None;
        if is_x360 {
            // "DVD.iso" / "DVD1.iso" / "game.iso" inside a folder: the folder is the name (a digit in the file name is the
            // disc number); then the title id inside the image beats any file name
            if stfs.is_none() {
                if let Some(gen) = GENERIC.captures(&info.title.clone()) {
                    let n: u32 = gen[2].parse().unwrap_or(0);
                    let dir = f.parent().unwrap_or(Path::new(""));
                    if !is_root(dir) {
                        let ext = fname.rfind('.').filter(|&i| i > 0).map_or("", |i| &fname[i..]);
                        info = ct(&with_ext(dir, ext));
                    }
                    if info.disc == 0 && n > 0 {
                        info.disc = n;
                    }
                }
            }
            // FFxxxxxx ids are generic (bonus and downloadable-content discs share FFED2000): never trusted for a title
            x = x360::lookup(&f, stfs.as_ref(), &data.x360).filter(|l| !l.title_id.to_uppercase().starts_with("FF"));
            if let Some(l) = &x {
                if let Some(t) = &l.title {
                    info.title = TM.replace_all(t, "").trim().to_string();
                } else if GENERIC.is_match(&info.title) {
                    info.title = format!("Xbox 360 game {}", l.title_id);
                }
            }
        }
        if console.id == "wiiu" {
            let update = WIIU_KIND.captures(&f.to_string_lossy()).is_some_and(|c| c[1].eq_ignore_ascii_case("c") || c[1].eq_ignore_ascii_case("e"));
            info.extra = update || UPDATE_WORD.is_match(&info.title);
        }
        if info.extra {
            continue;
        }
        let (mut size, mut added) = (nus_bytes, 0);
        if let Ok(m) = fs::metadata(&f) {
            added = added_ms(&m);
            if fg.is_none() && size == 0 {
                size = m.len();
            }
        }
        if let Some(dir) = fg.as_ref().and_then(|g| g.folder.as_deref()).filter(|_| size == 0 && is_sw2) {
            size = data.dir_size(dir);
        }
        let key = match &x {
            Some(l) => format!("x360:{}", l.title_id),
            None => format!("{}|{}", info.title.to_lowercase(), info.region),
        };
        if is_x360 && stfs.is_none() {
            let nt = ct(&f).title;
            if !GENERIC.is_match(&nt) {
                by_name.insert(key.clone(), nt.to_lowercase());
            }
        }
        let n = info.disc.max(1);
        disc_of.insert(f.clone(), (n, size));
        let mut discs = vec![f.clone()];
        if let Some(prev) = by_title.get_mut(&key) {
            let pn = prev.game.disc.max(1);
            let better = n < pn || (n == pn && size > prev.game.size_bytes);
            // two dumps of the same disc (an .iso and an .rvz of disc 1) are one disc: the fuller one stays
            if info.disc > 0 {
                let ds = &mut prev.game.discs;
                match ds.iter().position(|d| disc_of.get(d).map_or(1, |x| x.0) == info.disc) {
                    None => ds.push(f.clone()),
                    Some(i) if size > disc_of.get(&ds[i]).map_or(0, |x| x.1) => ds[i] = f.clone(),
                    Some(_) => {}
                }
            }
            if !better {
                continue;
            }
            discs = prev.game.discs.clone();
        } else {
            order.push(key.clone());
        }
        let ext = ext_of(&fname);
        let (icon, hero, folder_id) = match fg {
            Some(g) => (g.icon, g.hero, g.title_id),
            None => (None, None, None),
        };
        let (title_id, compat) = match x {
            Some(l) => (Some(l.title_id), l.compat),
            None => (folder_id, None),
        };
        by_title.insert(
            key,
            Found {
                art_title: info.title.clone(),
                icon,
                game: Game {
                    id: format!("{}:{}", console.id, f.display()),
                    title: info.title,
                    region: info.region,
                    disc: info.disc,
                    file: f,
                    ext,
                    size_bytes: size,
                    added,
                    discs,
                    title_id,
                    art: Art::default(),
                    hero,
                    not_installed: false,
                    compat,
                    source: Source::Rom,
                },
            },
        );
    }
    if console.id == "ps4" {
        add_packages(console, sc, &mut by_title, &mut order);
    }
    if is_x360 {
        join_x360_discs(sc, &mut by_title, &order, &by_name);
    }

    let mut index = ArtIndex::default();
    let mut lists = Gamelists::default();
    let mut games: Vec<Game> = order
        .into_iter()
        .filter_map(|k| by_title.remove(&k))
        .map(|mut f| {
            let mut art = index.find(&f.game.file, &f.art_title, art_dirs);
            if let Some(icon) = f.icon.take() {
                if art.front.is_none() {
                    art.front = Some(icon.clone());
                }
                if art.disc.is_none() {
                    art.disc = Some(icon);
                }
            }
            // art of RustyShelf's own beats a gamelist.xml's
            if let Some(gl) = lists.lookup(&f.game.file, &sc.rom_dirs) {
                let name = SPACES.replace_all(&TM.replace_all(&gl.name, ""), " ").trim().to_string();
                if !name.is_empty() {
                    f.game.title = name;
                }
                art.front = art.front.or(gl.front);
                art.back = art.back.or(gl.back);
                art.disc = art.disc.or(gl.disc);
                art.manual_pdf = art.manual_pdf.or(gl.manual_pdf);
                f.game.hero = f.game.hero.or(gl.hero);
            }
            f.game.art = art;
            f.game
        })
        .collect();
    games.sort_by(|a, b| collate::compare(&a.title, &b.title));
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_names() {
        assert!(SKIP_NAME.is_match("Game [UPD].nsp"));
        assert!(SKIP_NAME.is_match("Game - Update v1.nsp"));
        assert!(SKIP_NAME.is_match("Game_DLC.nsp"));
        assert!(!SKIP_NAME.is_match("DLC Quest.iso"));
        assert!(!SKIP_NAME.is_match("Game - Updated.iso"));
    }

    #[test]
    fn extensions() {
        assert_eq!(ext_of("a.ISO"), "iso");
        assert_eq!(ext_of(".hidden"), "");
        assert_eq!(ext_of("name."), "");
    }

    #[test]
    fn ps3_regions() {
        assert_eq!(ps3_region("BLUS30001"), "USA");
        assert_eq!(ps3_region("NPEB00001"), "Europe");
        assert_eq!(ps3_region("XXXX"), "");
    }
}

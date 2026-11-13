// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/scanner.js, as RustyShelf has it.
//! Walks a console's games folders and makes one shelf entry per game: disc 2 joins disc 1, two dumps of a game are
//! one box (the fuller one), updates and DLC stay off the shelf.
//!
//! Not ported yet, so those games are missing or named by their file: the Xbox 360 (packages, title list), Switch 2
//! (unpacked folders), PS4 .pkg packages, PS5 param.json and the arcade name list.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::UNIX_EPOCH;

use regex::Regex;

use crate::art::{resolve, Art, ArtIndex};
use crate::config::SystemConfig;
use crate::systems::Console;
use crate::titles::{clean_title, CleanTitle};
use crate::{collate, sfo};

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
// BIOS sets that sit beside the arcade games
const ARCADE_BIOS: [&str; 26] = [
    "neogeo", "pgm", "skns", "stvbios", "decocass", "cpzn1", "cpzn2", "taitofx1", "konamigx", "nss", "naomi", "awbios", "hod2bios",
    "f355bios", "airlbios", "megaplay", "megatech", "playch10", "qsound", "neocdz", "isgsm", "nmk004", "ym2608", "bubsys", "cchip", "sp-cpu",
];

#[derive(Debug, Clone, PartialEq)]
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
    /// a hero picture the game's folder carries (PS3 PIC1.PNG)
    pub hero: Option<PathBuf>,
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

fn walk(dir: &Path, exts: &[String], depth: u32, out: &mut Vec<PathBuf>, game_file: Option<&str>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let ents: Vec<_> = rd.filter_map(Result::ok).collect();
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
        if exts.contains(&ext_of(&name)) && !SKIP_NAME.is_match(&name) {
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
        title = clean(&name_of(folder));
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
    })
}

/// A PS4 / Vita / PS5 game folder: the title from param.sfo, else the folder's name ("CUSA00207": the folder above,
/// unless that's the games folder itself).
fn folder_game(file: &Path, rom_dirs: &[String]) -> FolderGame {
    let dir = file.parent().unwrap_or(Path::new(""));
    // TODO: a PS5 game's sce_sys/param.json (main/kyty.js) when there's no param.sfo
    let sfo = sfo::read(&dir.join("sce_sys").join("param.sfo"));
    let mut title = sfo.as_ref().and_then(|s| sfo::text(s, "TITLE")).map(|t| SPACES.replace_all(t, " ").trim().to_string()).unwrap_or_default();
    let folder = name_of(dir);
    let parent = dir.parent().unwrap_or(Path::new(""));
    if title.is_empty() {
        let parent_is_root = rom_dirs.iter().any(|d| resolve(Path::new(d)) == resolve(parent));
        title = if PS_ID_START.is_match(&folder) && !parent_is_root && !name_of(parent).is_empty() { name_of(parent) } else { folder };
    }
    let sce = dir.join("sce_sys");
    let pic = crate::art::existing(sce.join("pic1.png")).or_else(|| crate::art::existing(sce.join("pic0.png")));
    FolderGame {
        title: TM.replace_all(&title, "").trim().to_string(),
        title_id: sfo.as_ref().and_then(|s| sfo::text(s, "TITLE_ID")).map(String::from),
        region: String::new(),
        icon: crate::art::existing(sce.join("icon0.png")),
        hero: pic,
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
    let mut files = Vec::new();
    for dir in &sc.rom_dirs {
        walk(Path::new(dir), &console.extensions, depth, &mut files, game_file);
    }
    if !console.also_files.is_empty() {
        for dir in &sc.rom_dirs {
            walk(Path::new(dir), &console.also_files, 4, &mut files, None);
        }
    }
    files
}

/// One console's games, sorted by title, with their art.
pub fn scan(console: &Console, sc: &SystemConfig, art_dirs: &[PathBuf]) -> Vec<Game> {
    let files = walk_files(console, sc);
    let switch = console.id == "sw" || console.id == "sw2";
    let ct = |f: &Path| clean_title(&name_of(f), switch);
    // an extracted Xbox 360 game: default.xex is the game, the other .xex beside it are helpers
    let xex_dirs: HashSet<PathBuf> = files
        .iter()
        .filter(|f| name_of(f).eq_ignore_ascii_case("default.xex"))
        .filter_map(|f| f.parent().map(Path::to_path_buf))
        .collect();

    let mut by_title: HashMap<String, Found> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut disc_of: HashMap<PathBuf, (u32, u64)> = HashMap::new();

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
        let mut info: CleanTitle = if as_folder || fg.is_some() {
            let g = fg.get_or_insert_with(|| folder_game(&f, &sc.rom_dirs));
            CleanTitle { title: g.title.clone(), region: g.region.clone(), ..Default::default() }
        } else if is_default {
            ct(&with_ext(f.parent().unwrap_or(Path::new("")), ".xex"))
        } else {
            ct(&f)
        };
        if console.id == "arcade" || console.id == "neogeo" {
            let set = fname.rsplit_once('.').map_or(fname.as_str(), |(s, _)| s).to_lowercase();
            if ARCADE_BIOS.contains(&set.as_str()) {
                continue;
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
        let key = format!("{}|{}", info.title.to_lowercase(), info.region);
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
        let (icon, hero, title_id) = match fg {
            Some(g) => (g.icon, g.hero, g.title_id),
            None => (None, None, None),
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
                },
            },
        );
    }

    let mut index = ArtIndex::default();
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

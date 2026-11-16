// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's android-app/web-extras/android-systems.js and android-api.js (the scan), as RustyShelf has them.
//! Android: the consoles the Android app shows, and their games read through the Storage Access Framework. A games
//! folder is one the player picked in Android's own folder picker: `romDirs` keeps the path it stands for
//! (/storage/emulated/0/ROMs/gc) and `data/folders.json` the tree address the app may read
//! ({ "<path>": "content://…/tree/…" }), as the Android app keeps them. The listing itself comes from the app (JNI).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::config::{Config, SystemConfig};
use crate::scanner::Game;
use crate::systems::Console;
use crate::titles::clean_title;

/// One file in a picked folder, as Android's DocumentsContract lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocFile {
    pub name: String,
    /// its content:// address
    pub uri: String,
    /// "primary:ROMs/gc/Game.rvz", "1A2B-3C4D:ROMs/Game.iso"
    pub doc_id: String,
    pub size: u64,
    /// last modified (ms)
    pub mtime: u64,
}

/// Lists the files with one of `exts` under a picked folder (`tree`), `depth` folders down.
pub type Lister = fn(tree: &str, exts: &[String], depth: u32) -> Result<Vec<DocFile>, String>;

static LISTER: OnceLock<Lister> = OnceLock::new();

/// The app hands in its folder listing (Android); the library reads picked folders through it from then on.
pub fn set_lister(f: Lister) {
    let _ = LISTER.set(f);
}

pub fn lister() -> Option<Lister> {
    LISTER.get().copied()
}

/// A Console's place on Android: its emulator apps (EMULATORS' ids, the first the usual one), RetroArch cores, and the
/// files that are games there.
pub struct System {
    pub id: &'static str,
    pub emus: &'static [&'static str],
    pub cores: &'static [&'static str],
    pub extensions: &'static [&'static str],
}

const fn sys(id: &'static str, emus: &'static [&'static str], cores: &'static [&'static str], extensions: &'static [&'static str]) -> System {
    System { id, emus, cores, extensions }
}

// keep in sync with android-app/web-extras/android-systems.js SYSTEMS
pub const SYSTEMS: &[System] = &[
    sys("gc", &["dolphin"], &[], &["rvz", "iso", "gcm", "gcz", "ciso", "wia", "dol", "elf", "m3u"]),
    sys("wii", &["dolphin"], &[], &["rvz", "iso", "wbfs", "wia", "gcz", "ciso", "wad", "m3u"]),
    sys("wiiu", &["cemu"], &[], &["wua", "wud", "wux", "rpx", "wuhb", "elf"]),
    sys("sw", &["eden"], &[], &["nsp", "xci", "nro", "nso"]),
    sys("n3ds", &["azahar"], &["citra"], &["3ds", "cci", "cxi", "3dsx", "app", "z3dsx", "zcci", "zcxi"]),
    sys("nds", &["melonds", "drastic", "retroarch"], &["melondsds", "melonds", "desmume"], &["nds", "zip", "7z"]),
    sys("ps2", &["nethersx2", "armsx2", "play"], &[], &["iso", "chd", "bin", "cso", "zso", "gz", "m3u", "elf"]),
    sys("psp", &["ppsspp", "retroarch"], &["ppsspp"], &["iso", "cso", "chd", "pbp", "elf", "prx"]),
    sys("psx", &["duckstation", "retroarch"], &["swanstation", "pcsx_rearmed", "mednafen_psx_hw"], &["chd", "cue", "m3u", "pbp", "iso", "img", "ccd", "mds", "ecm", "exe"]),
    sys("n64", &["m64plus", "retroarch"], &["mupen64plus_next_gles3", "parallel_n64"], &["z64", "n64", "v64", "zip", "7z"]),
    sys("dreamcast", &["redream", "flycast", "retroarch"], &["flycast"], &["chd", "gdi", "cdi", "cue", "m3u"]),
    sys("saturn", &["yaba", "saturnemu", "retroarch"], &["mednafen_saturn", "yabasanshiro"], &["chd", "cue", "m3u", "iso", "ccd", "mds"]),
    sys("megadrive", &["retroarch", "mdemu"], &["genesis_plus_gx", "picodrive"], &["md", "gen", "bin", "smd", "32x", "zip", "7z"]),
    sys("mastersystem", &["retroarch", "mdemu", "mastergear"], &["genesis_plus_gx", "gearsystem"], &["sms", "bin", "zip", "7z"]),
    sys("gamegear", &["retroarch", "mastergear"], &["genesis_plus_gx", "gearsystem"], &["gg", "zip", "7z"]),
    sys("snes", &["retroarch", "snes9x"], &["snes9x", "bsnes"], &["sfc", "smc", "bs", "fig", "swc", "zip", "7z"]),
    sys("nes", &["retroarch", "nesemu"], &["mesen", "nestopia", "fceumm"], &["nes", "fds", "unf", "unif", "zip", "7z"]),
    sys("gba", &["retroarch", "myboy", "pizzagba", "gbaemu"], &["mgba", "vbam", "gpsp"], &["gba", "agb", "zip", "7z"]),
    sys("gbc", &["retroarch", "myoldboy", "pizzagbc", "gbcemu"], &["gambatte", "sameboy", "mgba"], &["gbc", "cgb", "zip", "7z"]),
    sys("gb", &["retroarch", "myoldboy", "pizzagbc", "gbcemu"], &["gambatte", "sameboy", "mgba"], &["gb", "dmg", "sgb", "zip", "7z"]),
    sys("pcengine", &["retroarch", "pceemu"], &["mednafen_pce_fast", "mednafen_pce"], &["pce", "sgx", "chd", "cue", "zip", "7z"]),
    sys("neogeo", &["retroarch", "neoemu", "mame4d"], &["fbneo", "geolith"], &["zip", "7z", "neo"]),
    sys("arcade", &["retroarch", "mame4d"], &["fbneo", "mame2003_plus", "mamearcade"], &["zip", "7z"]),
    sys("segacd", &["retroarch", "mdemu"], &["genesis_plus_gx", "picodrive"], &["cue", "chd", "iso", "m3u"]),
    sys("sega32x", &["retroarch"], &["picodrive"], &["32x", "zip", "7z"]),
    sys("atari2600", &["retroarch"], &["stella", "stella2014"], &["a26", "bin", "zip", "7z"]),
    sys("atarilynx", &["retroarch"], &["handy", "mednafen_lynx"], &["lnx", "zip", "7z"]),
    sys("wonderswan", &["retroarch"], &["mednafen_wswan"], &["ws", "wsc", "zip", "7z"]),
    sys("ngp", &["retroarch"], &["mednafen_ngp"], &["ngp", "zip", "7z"]),
    sys("ngpc", &["retroarch"], &["mednafen_ngp"], &["ngc", "ngpc", "npc", "zip", "7z"]),
    sys("dos", &["retroarch"], &["dosbox_pure"], &["zip", "dosz"]),
];

/// Multi-file games: the tracks are listed by the .cue / .gdi / .m3u that points at them, never as boxes of their own.
const COMPANION: &[(&str, &[&str])] = &[
    ("cue", &["bin", "img", "wav", "ape"]),
    ("gdi", &["bin", "raw"]),
    ("m3u", &["cue", "chd", "iso", "gdi", "cdi", "bin", "img", "pbp"]),
    ("ccd", &["img", "sub"]),
    ("mds", &["mdf"]),
];

pub fn system(id: &str) -> Option<&'static System> {
    SYSTEMS.iter().find(|s| s.id == id)
}

/// The path a document id stands for: "primary:ROMs/gc" is internal storage, "1A2B-3C4D:ROMs" an SD card.
pub fn id_to_path(id: &str) -> String {
    let Some((vol, rest)) = id.split_once(':') else { return id.to_string() };
    let root = if vol == "primary" { "/storage/emulated/0".to_string() } else { format!("/storage/{vol}") };
    if rest.is_empty() { root } else { format!("{root}/{rest}") }
}

/// A picked folder's tree address → the path it stands for.
pub fn tree_path(uri: &str) -> Option<String> {
    let id = uri.split("/tree/").nth(1)?.split('/').next()?;
    Some(id_to_path(&crate::js::decode_uri_component(id)?))
}

fn folders_file(data_dir: &Path) -> PathBuf {
    data_dir.join("folders.json")
}

/// The picked folders: path → tree address.
pub fn folders(data_dir: &Path) -> Map<String, Value> {
    std::fs::read(folders_file(data_dir)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// Keeps a picked folder (its path, for `romDirs`, is answered).
pub fn add_folder(data_dir: &Path, uri: &str) -> Option<String> {
    let path = tree_path(uri)?;
    let mut all = folders(data_dir);
    all.insert(path.clone(), Value::String(uri.to_string()));
    let _ = std::fs::create_dir_all(data_dir);
    std::fs::write(folders_file(data_dir), Value::Object(all).to_string()).ok()?;
    Some(path)
}

/// shared/titles.js skipFile (keep in sync)
static SKIP_FILE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)\b(update|dlc)\b|\[(upd|dlc)\]").unwrap());

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

/// A console's games from its picked folders (android-api.js scanSystem): one box per game, a .cue's tracks under it,
/// discs together, updates and DLC left out, the names given in Details kept.
pub fn scan(console: &Console, sc: &SystemConfig, cfg: &Config, data_dir: &Path) -> Vec<Game> {
    let (Some(list), Some(s)) = (lister(), system(&console.id)) else { return Vec::new() };
    let trees = folders(data_dir);
    let mut exts: Vec<String> = s.extensions.iter().map(|e| e.to_string()).collect();
    for (c, _) in COMPANION {
        if !exts.iter().any(|e| e == c) {
            exts.push(c.to_string());
        }
    }
    let mut files: Vec<(DocFile, String, String)> = Vec::new();
    for dir in &sc.rom_dirs {
        let Some(uri) = trees.get(dir).and_then(Value::as_str) else { continue };
        if let Ok(found) = list(uri, &exts, 4) {
            files.extend(found.into_iter().map(|f| {
                let (path, ext) = (id_to_path(&f.doc_id), ext_of(&f.name));
                (f, path, ext)
            }));
        }
    }
    games_of(&console.id, s, files, cfg)
}

fn games_of(id: &str, s: &System, files: Vec<(DocFile, String, String)>, cfg: &Config) -> Vec<Game> {
    let mut by_dir: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (_, path, _)) in files.iter().enumerate() {
        by_dir.entry(path.rsplit_once('/').map_or("", |(d, _)| d)).or_default().push(i);
    }
    let mut hidden: HashSet<usize> = HashSet::new();
    static DISC: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)\s*\(dis[ck] \d+\)").unwrap());
    for list in by_dir.values() {
        for &f in list {
            let Some((_, owned)) = COMPANION.iter().find(|(c, _)| *c == files[f].2) else { continue };
            let stem = files[f].0.name.rsplit_once('.').map_or(files[f].0.name.as_str(), |(s, _)| s).to_lowercase();
            let prefix: String = DISC.replace_all(&stem, "").chars().take(12).collect();
            for &g in list {
                if g != f && owned.contains(&files[g].2.as_str()) && (files[f].2 != "m3u" || files[g].0.name.to_lowercase().starts_with(&prefix)) {
                    hidden.insert(g);
                }
            }
        }
    }
    let mut games: Vec<Game> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for (i, (f, path, ext)) in files.iter().enumerate() {
        if hidden.contains(&i) || !s.extensions.contains(&ext.as_str()) || SKIP_FILE.is_match(&f.name) {
            continue;
        }
        let info = clean_title(&f.name, id == "sw");
        if info.extra {
            continue;
        }
        let key = format!("{}|{}", info.title.to_lowercase(), info.region);
        if let Some(&k) = at.get(&key) {
            let prev = &mut games[k];
            prev.discs.push(PathBuf::from(path));
            if info.disc.max(1) >= prev.disc.max(1) {
                continue;
            }
            let discs = std::mem::take(&mut prev.discs);
            games[k] = game(id, f, path, ext, &info, discs);
            continue;
        }
        at.insert(key, games.len());
        games.push(game(id, f, path, ext, &info, vec![PathBuf::from(path)]));
    }
    for g in &mut games {
        if let Some(own) = cfg.game_title(&g.id).filter(|t| *t != g.title) {
            g.title = own.to_string();
        }
    }
    games.sort_by(|a, b| crate::collate::compare(&a.title, &b.title));
    games
}

fn game(id: &str, f: &DocFile, path: &str, ext: &str, info: &crate::titles::CleanTitle, discs: Vec<PathBuf>) -> Game {
    Game {
        id: format!("{id}:{path}"),
        title: info.title.clone(),
        region: info.region.clone(),
        disc: info.disc,
        file: PathBuf::from(path),
        ext: ext.to_string(),
        size_bytes: f.size,
        added: f.mtime,
        discs,
        doc: Some(f.uri.clone()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, size: u64) -> (DocFile, String, String) {
        let name = id.rsplit('/').next().unwrap().to_string();
        let f = DocFile { name: name.clone(), uri: format!("content://x/document/{id}"), doc_id: id.into(), size, mtime: 1 };
        (f, id_to_path(id), ext_of(&name))
    }

    #[test]
    fn paths() {
        assert_eq!(id_to_path("primary:ROMs/gc"), "/storage/emulated/0/ROMs/gc");
        assert_eq!(id_to_path("1A2B-3C4D:"), "/storage/1A2B-3C4D");
        assert_eq!(tree_path("content://com.android.externalstorage.documents/tree/primary%3AROMs%2Fpsx").as_deref(), Some("/storage/emulated/0/ROMs/psx"));
    }

    #[test]
    fn tracks_discs_and_updates() {
        let cfg = Config::from_value(&serde_json::json!({"games": {"psx:/storage/emulated/0/ROMs/psx/Other.chd": {"title": "Renamed"}}})).unwrap();
        let files = vec![
            doc("primary:ROMs/psx/Game (USA) (Disc 1).cue", 1),
            doc("primary:ROMs/psx/Game (USA) (Disc 1).bin", 500),
            doc("primary:ROMs/psx/Game (USA) (Disc 2).cue", 1),
            doc("primary:ROMs/psx/Game (USA) (Disc 2).bin", 500),
            doc("primary:ROMs/psx/Other.chd", 300),
            doc("primary:ROMs/psx/Other Update.chd", 3),
        ];
        let g = games_of("psx", system("psx").unwrap(), files, &cfg);
        assert_eq!(g.len(), 2, "{g:#?}");
        let game = g.iter().find(|x| x.title == "Game").unwrap();
        assert_eq!(game.discs.len(), 2);
        assert!(game.file.to_string_lossy().ends_with("(Disc 1).cue"));
        assert_eq!(game.doc.as_deref(), Some("content://x/document/primary:ROMs/psx/Game (USA) (Disc 1).cue"));
        assert!(g.iter().any(|x| x.title == "Renamed" && x.id == "psx:/storage/emulated/0/ROMs/psx/Other.chd"));
    }

    #[test]
    fn every_system_is_a_console() {
        for s in SYSTEMS {
            assert!(crate::systems::systems().console(s.id).is_some(), "{}", s.id);
        }
    }
}

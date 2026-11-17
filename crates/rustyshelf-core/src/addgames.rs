// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/addgames.js, as RustyShelf has it (folders of categories, and why a folder gave no game).
//! Add games: many PC games at once (Settings → Custom games).
//!
//! `scan_folder`: a folder of games (D:\Games), one game per folder in it, its main program picked (the one named
//! like its folder, else the biggest), helpers (uninstallers, redistributables, crash reporters) left out. A
//! folder of categories ("Games\Racing\Blur") is stepped into.
//!
//! Playnite (experimental: built from Playnite's documented library layout, not tried against every Playnite
//! version): `find` looks for it, `read` copies its library's database files to a temporary folder (Playnite may
//! be running, and its files are never touched) and reads the copies with Playnite's own LiteDB.dll through
//! PowerShell, `plan` turns the games into what RustyShelf adds: PC games for the Custom shelf, emulated games
//! as games folders for their console, with counts of what's left out.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::pccovers::folder_cover;
use crate::scanner::read_dir_sorted;
use crate::{collate, js};

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

// (main/addgames.js HELPER, SKIP_DIR, GENERIC, keep in sync)
static HELPER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^(?i)(unins.*|uninstall.*|setup.*|install.*|vc_?redist.*|dxsetup.*|dotnet.*|crash.*|.*crashhandler.*|.*crashreport.*|.*updater.*|update\.exe|unitycrashhandler.*|ue4prereq.*|ueprereq.*|easyanticheat.*|beservice.*|.*_launcher_setup.*|7z.*|cmd\.exe|notification_helper.*|cefsharp.*|.*webhelper.*|quicksfv.*|dxwebsetup.*|oalinst.*|physx.*)$")
});
static SKIP_DIR: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(redist|_commonredist|commonredist|directx|vcredist|support|tools?|mods?|saves?|config|logs?|__installer|_redist|prereqs?|dotnet|easyanticheat|battleye)$"));
static GENERIC: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(bin|binaries|win32|win64|x64|x86|game|release|retail|shipping|windows(noeditor)?|app|exe)$"));

/// A name as it's compared: its letters and digits, lower case; a program's without its extension.
fn key(s: &str, folder: bool) -> String {
    static EXT: LazyLock<Regex> = LazyLock::new(|| re(r"\.[a-z0-9]{2,4}$"));
    let low = s.to_lowercase();
    let low = if folder { low } else { EXT.replace(&low, "").into_owned() };
    low.chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// A folder's or a program's name as a title: separators as spaces, version numbers and release tags off.
pub fn clean(s: &str) -> String {
    static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| re(r"[-_.]+"));
    static BRACKETED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\s*[(\[](gog|repack|www\.[^)\]]*|v?[0-9][0-9.]*[a-z]?)[)\]]\s*"));
    static TAGS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?-u:\b)(v?[0-9]+(\.[0-9]+)+[a-z]?|build\s*[0-9]+|repack|gog)(?-u:\b)"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
    let t = SEPARATORS.replace_all(s, " ");
    let t = BRACKETED.replace_all(&t, " ");
    let t = TAGS.replace_all(&t, " ");
    js::trim(&SPACES.replace_all(&t, " ")).to_string()
}

fn is_exe(name: &str) -> bool {
    let low = name.to_lowercase();
    low.ends_with(".exe") || (cfg!(target_os = "linux") && [".sh", ".x86_64", ".bin", ".x86"].iter().any(|e| low.ends_with(e)))
}

fn program(name: &str) -> bool {
    is_exe(name) && !HELPER.is_match(name)
}

/// A folder's files and folders by name, in name order.
fn listing(dir: &Path) -> Vec<(String, PathBuf, bool)> {
    read_dir_sorted(dir).into_iter().filter_map(|e| { let meta = fs::metadata(e.path()).ok()?; Some((e.file_name().to_string_lossy().into_owned(), e.path(), meta.is_dir())) }).collect()
}

/// The main program of one game's folder, looked for `depth` folders down, and up to three others: named like
/// the folder wins, then the biggest.
pub fn main_exe(dir: &Path, depth: u32) -> Option<(PathBuf, Vec<PathBuf>)> {
    struct Program {
        path: PathBuf,
        size: u64,
        key: String,
        deep: u32,
    }
    fn walk(dir: &Path, left: u32, depth: u32, found: &mut Vec<Program>) {
        for (name, path, is_dir) in listing(dir) {
            if !is_dir && program(&name) {
                let Ok(meta) = fs::metadata(&path) else { continue };
                found.push(Program { size: meta.len(), key: key(&name, false), deep: depth - left, path });
            } else if is_dir && left > 0 && !SKIP_DIR.is_match(&name) {
                walk(&path, left - 1, depth, found);
            }
        }
    }
    let want = key(&dir.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()), true);
    let mut found = Vec::new();
    walk(dir, depth, depth, &mut found);
    let score = |p: &Program| {
        let like = if p.key.is_empty() || want.is_empty() { 0.0 } else if p.key == want { 3.0 } else if want.contains(&p.key) || p.key.contains(&want) { 2.0 } else { 0.0 };
        like * 1e12 + p.size as f64 - f64::from(p.deep) * 1e6
    };
    found.sort_by(|a, b| score(b).total_cmp(&score(a)));
    let mut all = found.into_iter().map(|p| p.path);
    Some((all.next()?, all.take(3).collect()))
}

/// A game's name from its program: the nearest folder that isn't a generic one ("bin", "x64" …), else the
/// program's own name. `root`: the folder being scanned, which names no game.
pub fn title_for(exe: &Path, root: Option<&Path>) -> String {
    let same = |a: &Path, b: &Path| js::normalize(&a.to_string_lossy()) == js::normalize(&b.to_string_lossy());
    let name = |p: &Path| p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
    let mut d = exe.parent().unwrap_or(Path::new("")).to_path_buf();
    while let Some(up) = d.parent().filter(|up| GENERIC.is_match(&name(&d)) && !up.as_os_str().is_empty() && !root.is_some_and(|r| same(up, r))) {
        d = up.to_path_buf();
    }
    let from_folder = clean(&name(&d));
    let own = if !from_folder.is_empty() && !root.is_some_and(|r| same(&d, r)) { from_folder } else { clean(&exe.file_stem().map_or(String::new(), |n| n.to_string_lossy().into_owned())) };
    if own.is_empty() { "Game".into() } else { own }
}

/// A game found in a scanned folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub folder: PathBuf,
    pub title: String,
    pub exe: PathBuf,
    /// other programs in its folder, should the one picked not be the game
    pub others: Vec<PathBuf>,
    /// the category folders it sits under, from the scanned folder
    pub group: Option<String>,
    /// a picture found in its folder
    pub cover: Option<PathBuf>,
}

/// What a scan of a folder of games gave.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    pub games: Vec<Found>,
    /// the folders that gave no game: the folder, its name and why
    pub skipped: Vec<(PathBuf, String, &'static str)>,
    pub error: Option<String>,
}

fn has_program(dir: &Path) -> bool {
    listing(dir).iter().any(|(name, _, is_dir)| !is_dir && program(name))
}

/// Whether a folder has a program of its own: at its top, or in a bin / x64 kind of folder.
fn looks_like_game(dir: &Path) -> bool {
    has_program(dir) || listing(dir).iter().any(|(name, path, is_dir)| *is_dir && GENERIC.is_match(name) && has_program(path))
}

/// A folder is a category when it has no program of its own and at least two of its folders hold a game (or are
/// categories themselves).
fn is_category(dir: &Path, deeper: u32) -> bool {
    if has_program(dir) {
        return false;
    }
    let mut n = 0;
    for (name, path, is_dir) in listing(dir) {
        if !is_dir || GENERIC.is_match(&name) || SKIP_DIR.is_match(&name) || name.starts_with('.') {
            continue;
        }
        // a game whose program sits a level down ("RPG\Some Game\Game\game.exe") counts too
        if looks_like_game(&path) || main_exe(&path, 1).is_some() || (deeper > 0 && is_category(&path, deeper - 1)) {
            n += 1;
            if n >= 2 {
                return true;
            }
        }
    }
    false
}

fn why_not(dir: &Path) -> &'static str {
    static INSTALLER: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(setup|install).*\.exe$"));
    static PACKED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(zip|rar|7z|iso|vhdx?|img)$"));
    let files: Vec<String> = listing(dir).into_iter().filter(|e| !e.2).map(|e| e.0).collect();
    if files.iter().any(|n| INSTALLER.is_match(n)) {
        "only an installer"
    } else if files.iter().any(|n| PACKED.is_match(n)) {
        "an archive or disc image, not installed"
    } else {
        "no program found"
    }
}

/// Scans a folder of games: one game for each folder in it (categories stepped into, three levels at most), and
/// one for each program right in it. At most `limit` games.
pub fn scan_folder(root: &Path, limit: usize) -> Scan {
    fn walk(root: &Path, dir: &Path, level: u32, limit: usize, out: &mut Scan) {
        for (name, path, is_dir) in listing(dir) {
            if out.games.len() >= limit {
                return;
            }
            if !is_dir || SKIP_DIR.is_match(&name) || name.starts_with('.') {
                continue;
            }
            if level < 3 && is_category(&path, 2) {
                walk(root, &path, level + 1, limit, out);
                continue;
            }
            let cleaned = clean(&name);
            match main_exe(&path, 3) {
                Some((exe, others)) => {
                    let cover = folder_cover(&path.to_string_lossy()).or_else(|| folder_cover(&exe.to_string_lossy()));
                    let title = if cleaned.is_empty() { title_for(&exe, Some(dir)) } else { cleaned };
                    let group = (level > 0).then(|| dir.strip_prefix(root).unwrap_or(dir).to_string_lossy().into_owned());
                    out.games.push(Found { folder: path, title, exe, others, group, cover });
                }
                None if out.skipped.len() < 500 => out.skipped.push((path.clone(), if cleaned.is_empty() { name } else { cleaned }, why_not(&path))),
                None => {}
            }
        }
    }
    if !root.is_dir() {
        return Scan { error: Some("That folder isn't there".into()), ..Scan::default() };
    }
    let mut out = Scan::default();
    walk(root, root, 0, limit, &mut out);
    for (name, path, is_dir) in listing(root) {
        if !is_dir && program(&name) {
            let cover = folder_cover(&path.to_string_lossy());
            out.games.push(Found { folder: root.to_path_buf(), title: clean(&js::stem(&name)), exe: path, others: Vec::new(), group: None, cover });
        }
    }
    out.games.sort_by(|a, b| collate::compare_exact(&a.title, &b.title));
    out.skipped.sort_by(|a, b| collate::compare_exact(&a.1, &b.1));
    out
}

// ---------- Playnite ----------

/// Where Playnite is: the folder with its programs (and LiteDB.dll), and the one with its library.
#[derive(Debug, Clone, PartialEq)]
pub struct Playnite {
    pub app: PathBuf,
    pub library: PathBuf,
}

/// Looks for Playnite: in `extra` (a folder the player chose), where it installs itself, and in a few usual
/// places; its library beside it (portable) or in the user's own data.
pub fn find(extra: Option<&Path>, var: crate::emucfg::Var) -> Option<Playnite> {
    let home = crate::emucfg::home(var);
    let data = |name: &str, under: &[&str]| crate::emucfg::get(var, name).map_or_else(|| PathBuf::from(&home).join(under.iter().collect::<PathBuf>()), PathBuf::from);
    let (local, roaming) = (data("LOCALAPPDATA", &["AppData", "Local"]), data("APPDATA", &["AppData", "Roaming"]));
    let try_one = |app: &Path, library: PathBuf| (app.join("LiteDB.dll").is_file() && library.join("games.db").is_file()).then(|| Playnite { app: app.to_path_buf(), library });
    let dirs = [extra.map(Path::to_path_buf), Some(local.join("Playnite")), Some(PathBuf::from("C:\\Program Files\\Playnite")), Some(PathBuf::from("C:\\Playnite")), Some(PathBuf::from("D:\\Playnite")), Some(PathBuf::from(&home).join("Playnite"))];
    dirs.into_iter().flatten().filter(|d| !d.as_os_str().is_empty()).find_map(|d| try_one(&d, d.join("library")).or_else(|| try_one(&d, roaming.join("Playnite").join("library"))))
}

/// LiteDB's JSON with its wrapped values (`$guid`, `$date`, `$numberLong`, `$oid`) as plain ones.
fn unwrap(v: Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.into_iter().map(unwrap).collect()),
        Value::Object(o) => {
            if let Some(plain) = ["$guid", "$date"].iter().find_map(|k| o.get(*k).filter(|x| js::truthy(Some(x)))) {
                return plain.clone();
            }
            if let Some(n) = o.get("$numberLong").filter(|x| js::truthy(Some(x))) {
                return js::num(js::number(Some(n)));
            }
            if let Some(id) = o.get("$oid").filter(|x| js::truthy(Some(x))) {
                return id.clone();
            }
            Value::Object(o.into_iter().map(|(k, x)| (k, unwrap(x))).collect())
        }
        other => other,
    }
}

/// The library's documents from what the reading script printed: a line each, "<database>|<collection>|<JSON>".
pub fn documents(printed: &str) -> Value {
    let (mut games, mut platforms, mut emulators) = (Vec::new(), Vec::new(), Vec::new());
    for line in printed.lines() {
        let mut parts = line.splitn(3, '|');
        let (Some(db), Some(collection), Some(doc)) = (parts.next(), parts.next(), parts.next()) else { continue };
        if db.is_empty() || collection.is_empty() {
            continue;
        }
        let Ok(doc) = serde_json::from_str::<Value>(doc.trim_end_matches('\r')) else { continue };
        match db.to_lowercase().as_str() {
            "games" => games.push(unwrap(doc)),
            "platforms" => platforms.push(unwrap(doc)),
            "emulators" => emulators.push(unwrap(doc)),
            _ => {}
        }
    }
    json!({ "games": games, "platforms": platforms, "emulators": emulators })
}

/// Reads Playnite's library: its games, platforms and emulators. Windows only.
pub fn read(found: &Playnite) -> Result<Value, String> {
    if !cfg!(windows) {
        return Err("Playnite runs on Windows".into());
    }
    // (main/addgames.js PS, keep in sync) the folders are passed in the environment, never put into the script
    const SCRIPT: &str = "\n$ErrorActionPreference = 'Stop'\nAdd-Type -Path (Join-Path $env:BS_PN_APP 'LiteDB.dll')\nforeach ($f in (Get-ChildItem -LiteralPath $env:BS_PN_COPY -Filter '*.db')) {\n  $db = New-Object LiteDB.LiteDatabase (\"Filename=\" + $f.FullName)\n  try { foreach ($c in $db.GetCollectionNames()) { foreach ($d in $db.GetCollection($c).FindAll()) { [Console]::Out.WriteLine($f.BaseName + '|' + $c + '|' + [LiteDB.JsonSerializer]::Serialize($d)) } } }\n  finally { $db.Dispose() }\n}";
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let copy = std::env::temp_dir().join(format!("bs-playnite-{}-{stamp}", std::process::id()));
    let copied = fs::create_dir_all(&copy).and_then(|()| {
        for name in ["games.db", "platforms.db", "emulators.db"] {
            let file = found.library.join(name);
            if file.is_file() {
                fs::copy(&file, copy.join(name))?;
            }
        }
        Ok(())
    });
    if let Err(e) = copied {
        let _ = fs::remove_dir_all(&copy);
        return Err(format!("Playnite's library couldn't be copied ({e})"));
    }
    let mut command = std::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT]).env("BS_PN_APP", &found.app).env("BS_PN_COPY", &copy);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(NO_WINDOW);
    }
    let out = command.output();
    let _ = fs::remove_dir_all(&copy);
    match out {
        Ok(o) if o.status.success() => Ok(documents(&String::from_utf8_lossy(&o.stdout))),
        _ => Err("Playnite's library couldn't be read with this Playnite version".into()),
    }
}

/// Playnite's platform ids (SpecificationId) as RustyShelf's consoles.
// (main/addgames.js SPEC and BY_NAME, keep in sync)
fn by_spec(id: &str) -> Option<&'static str> {
    Some(match id {
        "nintendo_gamecube" => "gc",
        "nintendo_wii" => "wii",
        "nintendo_wiiu" => "wiiu",
        "nintendo_switch" => "sw",
        "sony_psp" => "psp",
        "sony_psvita" | "sony_ps_vita" | "sony_playstation_vita" => "psvita",
        "sony_playstation" => "psx",
        "sony_playstation2" => "ps2",
        "sony_playstation3" => "ps3",
        "sony_playstation4" => "ps4",
        "sony_playstation5" => "ps5",
        "xbox360" => "x360",
        "nintendo_3ds" => "n3ds",
        "nintendo_nes" => "nes",
        "nintendo_famicom_disk_system" => "fds",
        "nintendo_super_nes" => "snes",
        "nintendo_64" => "n64",
        "nintendo_gameboy" => "gb",
        "nintendo_gameboycolor" => "gbc",
        "nintendo_gameboyadvance" => "gba",
        "nintendo_ds" => "nds",
        "sega_genesis" => "megadrive",
        "sega_mastersystem" => "mastersystem",
        "sega_gamegear" => "gamegear",
        "sega_dreamcast" => "dreamcast",
        "sega_saturn" => "saturn",
        "nec_turbografx_16" | "nec_pcengine" => "pcengine",
        "snk_neogeo_aes" | "snk_neogeo" => "neogeo",
        "sega_cd" => "segacd",
        "sega_32x" => "sega32x",
        "atari_2600" => "atari2600",
        "atari_lynx" => "atarilynx",
        "bandai_wonderswan" | "bandai_wonderswan_color" => "wonderswan",
        "snk_neogeo_pocket" | "snk_neo_geo_pocket" => "ngp",
        "snk_neogeo_pocket_color" | "snk_neo_geo_pocket_color" => "ngpc",
        "tiger_game_com" | "tiger_gamecom" => "gamecom",
        "pc_dos" => "dos",
        "pc_windows" => "pc",
        "arcade" => "arcade",
        _ => return None,
    })
}

/// A platform without an id of Playnite's, by its name: the first that fits.
fn by_name(name: &str) -> Option<&'static str> {
    static NAMES: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
        let b = r"(?-u:\b)";
        [
            ("gamecube".to_string(), "gc"),
            ("wii ?u".into(), "wiiu"),
            (format!("{b}wii{b}"), "wii"),
            ("switch".into(), "sw"),
            (format!("{b}ps ?vita{b}|playstation vita|vita3k"), "psvita"),
            (format!("{b}psp{b}|portable"), "psp"),
            ("playstation 5|ps5".into(), "ps5"),
            ("playstation 4|ps4".into(), "ps4"),
            ("playstation 3|ps3".into(), "ps3"),
            ("playstation 2|ps2".into(), "ps2"),
            ("playstation|psx|ps1".into(), "psx"),
            ("xbox 360".into(), "x360"),
            ("3ds".into(), "n3ds"),
            (format!("{b}ds{b}"), "nds"),
            ("game ?boy advance|gba".into(), "gba"),
            ("game ?boy color|gbc".into(), "gbc"),
            ("game ?boy".into(), "gb"),
            ("super n|snes|super famicom".into(), "snes"),
            ("nintendo 64|n64".into(), "n64"),
            (format!("famicom disk system|{b}fds{b}"), "fds"),
            (format!("{b}nes{b}|famicom|nintendo entertainment"), "nes"),
            ("genesis|mega ?drive".into(), "megadrive"),
            ("master system".into(), "mastersystem"),
            ("game ?gear".into(), "gamegear"),
            ("dreamcast".into(), "dreamcast"),
            ("saturn".into(), "saturn"),
            ("turbografx|pc engine".into(), "pcengine"),
            ("neo ?-?geo ?pocket colou?r|ngpc".into(), "ngpc"),
            (format!("neo ?-?geo ?pocket|{b}ngp{b}"), "ngp"),
            ("neo ?geo".into(), "neogeo"),
            (r"game\.?com".into(), "gamecom"),
            ("sega cd|mega cd".into(), "segacd"),
            ("32x".into(), "sega32x"),
            ("2600".into(), "atari2600"),
            ("lynx".into(), "atarilynx"),
            ("wonderswan".into(), "wonderswan"),
            ("dos".into(), "dos"),
            ("arcade|mame".into(), "arcade"),
            (format!("windows|{b}pc{b}"), "pc"),
        ]
        .into_iter()
        .map(|(pattern, id)| (re(&format!("(?i){pattern}")), id))
        .collect()
    });
    NAMES.iter().find(|(pattern, _)| pattern.is_match(name)).map(|(_, id)| *id)
}

// Playnite's paths are Windows ones, read as such on any system

fn win_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    matches!(b.first(), Some(b'\\' | b'/')) || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/'))
}

/// A drive ("C:") and the rest of a Windows path.
fn win_split(p: &str) -> (&str, &str) {
    let b = p.as_bytes();
    if b.len() > 1 && b[0].is_ascii_alphabetic() && b[1] == b':' { p.split_at(2) } else { ("", p) }
}

/// node's `path.win32.basename`.
fn win_basename(p: &str) -> String {
    win_split(p).1.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

/// node's `path.win32.dirname`.
fn win_dirname(p: &str) -> String {
    let (drive, rest) = win_split(p);
    let rooted = rest.starts_with(['\\', '/']);
    let body = rest.trim_end_matches(['\\', '/']);
    match body.rfind(['\\', '/']) {
        Some(0) => format!("{drive}{}", &rest[..1]),
        Some(i) => format!("{drive}{}", body[..i].trim_end_matches(['\\', '/'])),
        None if rooted => format!("{drive}{}", &rest[..1]),
        None if drive.is_empty() => ".".into(),
        None => drive.to_string(),
    }
}

/// node's `path.win32.join` of two parts.
fn win_join(a: &str, b: &str) -> String {
    let joined = [a, b].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\\");
    let (drive, rest) = win_split(&joined);
    let rooted = rest.starts_with(['\\', '/']);
    let mut parts: Vec<&str> = Vec::new();
    for seg in rest.split(['\\', '/']) {
        match seg {
            "" | "." => {}
            ".." if parts.last().is_some_and(|l| *l != "..") => {
                parts.pop();
            }
            ".." if rooted => {}
            seg => parts.push(seg),
        }
    }
    let out = format!("{drive}{}{}", if rooted { "\\" } else { "" }, parts.join("\\"));
    if out.is_empty() { ".".into() } else { out }
}

/// What a Playnite library comes to here.
///
/// `pc`: the games with a program to start, for the Custom shelf: `{key, title, exe, args, cover, bg, favorite,
/// there}`. `consoles`: by console, its games folders, how many games, and the folder of its emulator in
/// Playnite. `skipped`: how many were left out and why: `store` (games of stores RustyShelf reads itself),
/// `hidden`, `link` (started by a link), `nothing` (nothing to start, or a console RustyShelf hasn't).
pub fn plan(lib: &Value, found: &Playnite) -> Value {
    const EMPTY: &str = "00000000-0000-0000-0000-000000000000";
    static INSTALL_DIR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\{InstallDir\}"));
    static PLAYNITE_DIR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\{PlayniteDir\}"));
    static INSTALL_NAME: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\{InstallDirName\}"));
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let text = |v: Option<&Value>| if js::truthy(v) { js::string(v) } else { String::new() };
    let id_of = |v: &Value| text([v.get("Id"), v.get("_id")].into_iter().find(|x| js::truthy(*x)).flatten());
    let (games, platforms, emulators) = (list(lib.get("games")), list(lib.get("platforms")), list(lib.get("emulators")));
    let console_of = |g: &Value| {
        list(g.get("PlatformIds")).iter().find_map(|id| {
            let p = platforms.iter().rev().find(|p| id_of(p).to_lowercase() == js::string(Some(id)).to_lowercase())?;
            p["SpecificationId"].as_str().and_then(by_spec).or_else(|| by_name(&text(p.get("Name"))))
        })
    };
    let vars = |s: Option<&Value>, g: &Value| {
        let dir = text(g.get("InstallDirectory"));
        let t = INSTALL_DIR.replace_all(&text(s), regex::NoExpand(&dir)).into_owned();
        let t = PLAYNITE_DIR.replace_all(&t, regex::NoExpand(&found.app.to_string_lossy())).into_owned();
        INSTALL_NAME.replace_all(&t, regex::NoExpand(&win_basename(&dir))).replace('/', "\\")
    };
    // a picture of the library's own (not one on the web), when it's there
    let media = |v: Option<&Value>| -> Value {
        let v = text(v);
        if v.is_empty() || v.to_lowercase().starts_with("http:") || v.to_lowercase().starts_with("https:") {
            return Value::Null;
        }
        let file = if win_absolute(&v) { PathBuf::from(&v) } else { v.split(['\\', '/']).fold(found.library.join("files"), |p, part| p.join(part)) };
        if file.is_file() { json!(file.to_string_lossy()) } else { Value::Null }
    };
    let mut pc: Vec<Value> = Vec::new();
    let mut consoles: Map<String, Value> = Map::new();
    let (mut store_n, mut hidden, mut link_n, mut nothing) = (0, 0, 0, 0);
    for g in &games {
        if !js::truthy(Some(g)) || !js::truthy(g.get("Name")) {
            continue;
        }
        if js::truthy(g.get("Hidden")) {
            hidden += 1;
            continue;
        }
        let store = js::truthy(g.get("PluginId")) && js::string(g.get("PluginId")).to_lowercase() != EMPTY;
        let actions = list(g.get("GameActions"));
        let play: Vec<&Value> = actions.iter().filter(|a| js::truthy(Some(a)) && (js::truthy(a.get("IsPlayAction")) || actions.len() == 1)).collect();
        let of_type = |n: u64, name: &str| play.iter().find(|a| a["Type"].as_f64() == Some(n as f64) || a["Type"].as_str() == Some(name)).copied();
        let (file, emu, link) = (of_type(0, "File"), of_type(2, "Emulator"), of_type(1, "URL"));
        let sys = console_of(g);
        let roms = list(g.get("Roms"));
        if emu.is_some() || (sys.is_some_and(|s| s != "pc") && !roms.is_empty()) {
            let path = roms.first().map(|r| vars(r.get("Path"), g)).filter(|p| !p.is_empty());
            let (Some(path), Some(sys)) = (path, sys.filter(|s| *s != "pc")) else {
                nothing += 1;
                continue;
            };
            let c = consoles.entry(sys).or_insert_with(|| json!({ "dirs": [], "count": 0, "emulatorDir": "" }));
            c["count"] = json!(c["count"].as_u64().unwrap_or(0) + 1);
            let dir = win_dirname(&path);
            let dirs = c["dirs"].as_array_mut().expect("a list");
            if !dirs.iter().any(|d| d.as_str().is_some_and(|d| d.to_lowercase() == dir.to_lowercase())) {
                dirs.push(json!(dir));
            }
            if c["emulatorDir"] == "" {
                if let Some(id) = emu.and_then(|e| e.get("EmulatorId")).filter(|id| js::truthy(Some(id))) {
                    let own = emulators.iter().find(|e| id_of(e).to_lowercase() == js::string(Some(id)).to_lowercase());
                    if let Some(e) = own.filter(|e| js::truthy(e.get("InstallDir"))) {
                        c["emulatorDir"] = json!(vars(e.get("InstallDir"), g));
                    }
                }
            }
            continue;
        }
        if let Some(file) = file.filter(|f| js::truthy(f.get("Path"))) {
            // Steam, GOG Galaxy, Epic …: RustyShelf reads those stores itself
            if store {
                store_n += 1;
                continue;
            }
            let exe = vars(file.get("Path"), g);
            let abs = if win_absolute(&exe) { exe } else { win_join(&text(g.get("InstallDirectory")), &exe) };
            let key = text([g.get("Id"), g.get("_id"), g.get("Name")].into_iter().find(|x| js::truthy(*x)).flatten());
            pc.push(json!({ "key": key, "title": js::slice(&js::string(g.get("Name")), 120), "exe": abs, "args": js::slice(&vars(file.get("Arguments"), g), 1000), "cover": media(g.get("CoverImage")), "bg": media(g.get("BackgroundImage")),
                "favorite": js::truthy(g.get("Favorite")), "there": Path::new(&abs).is_file() }));
            continue;
        }
        if link.is_some() || store {
            if store {
                store_n += 1;
            } else {
                link_n += 1;
            }
            continue;
        }
        nothing += 1;
    }
    pc.sort_by(|a, b| collate::compare_exact(a["title"].as_str().unwrap_or(""), b["title"].as_str().unwrap_or("")));
    json!({ "pc": pc, "consoles": consoles, "skipped": { "store": store_n, "hidden": hidden, "link": link_n, "nothing": nothing }, "total": games.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_are_read_as_node_reads_them() {
        assert_eq!(win_dirname("C:\\Games\\Zeta Run\\zeta.iso"), "C:\\Games\\Zeta Run");
        assert_eq!(win_dirname("C:\\zeta.iso"), "C:\\");
        assert_eq!(win_dirname("zeta.iso"), ".");
        assert_eq!(win_dirname("D:/roms/gc/zeta.iso"), "D:/roms/gc");
        assert_eq!(win_dirname("\\zeta.iso"), "\\");
        assert_eq!(win_basename("C:\\Games\\Zeta Run\\"), "Zeta Run");
        assert_eq!(win_basename("C:\\"), "");
        assert_eq!(win_basename(""), "");
        assert_eq!(win_join("C:\\Games\\Zeta Run\\", "bin\\..\\zeta.exe"), "C:\\Games\\Zeta Run\\zeta.exe");
        assert_eq!(win_join("", "zeta.exe"), "zeta.exe");
        assert!(win_absolute("C:\\x") && win_absolute("\\x") && win_absolute("c:/x") && !win_absolute("C:x") && !win_absolute("x\\y"));
    }
}

#[cfg(test)]
mod reading {
    use super::*;

    #[test]
    fn the_scripts_lines_become_the_librarys_documents() {
        let printed = "games|Game|{\"_id\":{\"$guid\":\"g1\"},\"Name\":\"Zeta Run\",\"Added\":{\"$date\":\"2026-01-01T00:00:00Z\"},\"Playtime\":{\"$numberLong\":\"120\"},\"PlatformIds\":[{\"$guid\":\"p1\"}]}\r\nplatforms|Platform|{\"_id\":{\"$oid\":\"p1\"},\"Name\":\"Made Up | Console\"}\nemulators|Emulator|not json\nother|x|{}\nno bars here\n|x|{}\n";
        let lib = documents(printed);
        assert_eq!(lib["games"], json!([{ "_id": "g1", "Name": "Zeta Run", "Added": "2026-01-01T00:00:00Z", "Playtime": 120, "PlatformIds": ["p1"] }]));
        assert_eq!(lib["platforms"], json!([{ "_id": "p1", "Name": "Made Up | Console" }]));
        assert_eq!(lib["emulators"], json!([]));
    }
}

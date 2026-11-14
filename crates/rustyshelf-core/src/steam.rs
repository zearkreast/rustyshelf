// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/steam.js (scanSteamAsync) and main/detect.js (findSteam), as RustyShelf has them.
//! Installed Steam games, their playtime and cover art, straight from the Steam folder.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;

use crate::art::resolve;
use crate::paths::{expand, is_dir, is_file};
use crate::scanner::read_dir_sorted;
use crate::vdf::{self, Vdf};

/// How long a library's drive gets to answer before it's left out (asleep, unplugged, a network share gone).
const WAIT: Duration = Duration::from_secs(8);

// Steam installs helpers beside the games (Proton, runtimes, redistributables, SteamVR …): left out unless Settings says
// otherwise. Soundtracks, dedicated servers, SDKs and benchmarks are never games.
static NEVER_GAMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)soundtrack|dedicated server|(?-u:\b)sdk(?-u:\b)|benchmark").unwrap());
static TOOL_NAMES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)redistributable|steamworks common|^proton(?-u:\b)|proton (experimental|hotfix|easyanticheat|battleye)|steam linux runtime|^steamvr(?-u:\b)|steam audio|directx|visual c\+\+|vc\+\+ ?redist|vcredist|\.net framework|physx|steam controller config|steamworks shared|openvr|compatibility tool").unwrap()
});
const TOOL_IDS: &[&str] = &[
    "228980", // Steamworks Common Redistributables
    "1070560", "1391110", "1628350", // Steam Linux Runtime (scout / soldier / sniper)
    "858280", "961940", "1054830", "1113280", "1245040", "1420170", "1580130", "1887720", "2348590", "2805730", "1493710", "2180100",
    "2230260", // Proton 3.7 … 9.0, Experimental, Hotfix, Next
    "1161040", "1826330", // Proton BattlEye / EasyAntiCheat runtimes
    "250820", "323910", // SteamVR, SteamVR Performance Test
    "241100", // Steam Controller Configs
];

pub fn is_tool(appid: &str, name: &str) -> bool {
    TOOL_IDS.contains(&appid) || TOOL_NAMES.is_match(name)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SteamGame {
    pub appid: String,
    pub title: String,
    pub size_bytes: u64,
    /// its folder in steamapps/common
    pub dir: Option<PathBuf>,
    pub minutes: u64,
    /// seconds since 1970
    pub last_played: u64,
    pub front: Option<PathBuf>,
    pub hero: Option<PathBuf>,
}

/// Runs `f` on a thread of its own, giving up on it after `wait`.
fn within<T: Send + 'static>(wait: Duration, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(wait).ok()
}

fn read_vdf(file: &Path) -> Option<Vdf> {
    let b = std::fs::read(file).ok()?;
    Some(vdf::parse(&String::from_utf8_lossy(&b)))
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The folder that holds steamapps, from the folder the player gave: Steam's own, or by mistake steam.exe, its
/// steamapps folder or another library ("D:\SteamLibrary").
pub fn steam_root(given: &str) -> Option<PathBuf> {
    static EXE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(exe|sh)$").unwrap());
    static STEAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[\\/]steam$").unwrap());
    let t = given.trim();
    let t = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(t);
    let t = t.trim_end_matches(['\\', '/']);
    if t.is_empty() {
        return None;
    }
    let mut s = PathBuf::from(t);
    if EXE.is_match(t) || (STEAM.is_match(t) && !is_dir(&s)) {
        s = s.parent().map(Path::to_path_buf).unwrap_or_default();
    }
    if name_of(&s).eq_ignore_ascii_case("steamapps") && is_dir(&s) {
        return s.parent().map(Path::to_path_buf);
    }
    is_dir(&s.join("steamapps")).then_some(s)
}

/// `steam_root`, giving up (None) when the folder's drive doesn't answer in time.
pub fn steam_root_within(given: &str) -> Option<PathBuf> {
    let g = given.to_string();
    within(WAIT, move || steam_root(&g)).flatten()
}

/// Steam's own folder (the list of every library, playtimes and its cover cache), not just a library on another drive.
pub fn is_main_steam(p: &Path) -> bool {
    is_dir(&p.join("userdata")) || is_dir(&p.join("appcache"))
}

/// `is_main_steam`, false when the folder's drive doesn't answer in time.
pub fn is_main_steam_within(p: &Path) -> bool {
    let p = p.to_path_buf();
    within(WAIT, move || is_main_steam(&p)).unwrap_or(false)
}

/// Steam's folder on this computer, when it's in the usual place.
pub fn find_steam() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        for p in ["%HOME%/.steam/steam", "%HOME%/.local/share/Steam", "%HOME%/.var/app/com.valvesoftware.Steam/.local/share/Steam"] {
            let p = PathBuf::from(expand(p, Path::new("")));
            if is_dir(&p.join("steamapps")) {
                return Some(std::fs::canonicalize(&p).unwrap_or(p));
            }
        }
        return None;
    }
    let reg = if cfg!(windows) { registry_steam_path() } else { None };
    let mut options: Vec<String> = reg.into_iter().map(|r| r.replace('/', "\\")).collect();
    options.extend(["%ProgramFiles(x86)%\\Steam", "%ProgramFiles%\\Steam"].map(|p| expand(p, Path::new(""))));
    options.extend(["C:\\Steam", "D:\\Steam"].map(String::from));
    options.into_iter().map(PathBuf::from).find(|p| is_file(&p.join("steam.exe")))
}

fn registry_steam_path() -> Option<String> {
    let mut cmd = std::process::Command::new("reg");
    cmd.args(["query", "HKCU\\Software\\Valve\\Steam", "/v", "SteamPath"]);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)SteamPath\s+REG_\w+\s+(.+)").unwrap());
    LINE.captures(&String::from_utf8_lossy(&out.stdout)).map(|c| c[1].trim().to_string())
}

/// Libraries on a drive that didn't answer this session: not asked again (each still holds a waiting thread).
static SILENT: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

fn lib_key(p: &Path) -> String {
    resolve(p).to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
}

fn answers(p: &Path) -> bool {
    let k = lib_key(p);
    if SILENT.lock().unwrap().contains(&k) {
        return false;
    }
    let q = p.to_path_buf();
    match within(WAIT, move || is_dir(&q)) {
        Some(d) => d,
        None => {
            SILENT.lock().unwrap().insert(k);
            false
        }
    }
}

/// Playtime (minutes) and last played (seconds) per app, from the most recently used account.
fn user_stats(steam: &Path) -> Vec<(String, u64, u64)> {
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for e in read_dir_sorted(&steam.join("userdata")) {
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let f = e.path().join("config").join("localconfig.vdf");
        if let Ok(t) = std::fs::metadata(&f).and_then(|m| m.modified()) {
            if best.as_ref().is_none_or(|(_, b)| t > *b) {
                best = Some((f, t));
            }
        }
    }
    let Some(data) = best.and_then(|(f, _)| read_vdf(&f)) else { return Vec::new() };
    let Some(apps) = data.get(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) else { return Vec::new() };
    let whole = |n: f64| if n.is_finite() && n > 0.0 { n as u64 } else { 0 };
    apps.entries().into_iter().map(|(id, a)| (id.to_string(), whole(a.num(&["Playtime"])), whole(a.num(&["LastPlayed"])))).collect()
}

/// Steam's covers: librarycache/<id>_library_600x900.jpg (older), or librarycache/<id>/…/library_600x900.jpg (newer).
fn find_art(steam: &Path, appid: &str, names: &[String]) -> Option<PathBuf> {
    let cache = steam.join("appcache").join("librarycache");
    if let Some(f) = names.iter().map(|n| cache.join(n)).find(|f| is_file(f)) {
        return Some(f);
    }
    let bare: Vec<String> = names.iter().map(|n| n.replacen(&format!("{appid}_"), "", 1).to_lowercase()).collect();
    let mut stack = vec![(cache.join(appid), 0)];
    while let Some((d, depth)) = stack.pop() {
        for e in read_dir_sorted(&d) {
            let Ok(t) = e.file_type() else { continue };
            if t.is_file() && bare.contains(&e.file_name().to_string_lossy().to_lowercase()) {
                return Some(e.path());
            }
            if t.is_dir() && depth < 2 {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    None
}

/// The installed games of every Steam library. `given`: Settings → PC games → the Steam folder; `main`: Steam's own
/// folder when the one given is a library on another drive (it lists every library, and has the playtimes and covers).
pub fn scan(given: &str, main: Option<&str>, hide_tools: bool) -> Vec<SteamGame> {
    let Some(given) = steam_root_within(given) else { return Vec::new() };
    let main = main.and_then(steam_root_within);
    let steam = match main {
        Some(m) if !is_main_steam(&given) => m,
        _ => given.clone(),
    };
    let stats = user_stats(&steam);
    let mut libs: Vec<PathBuf> = Vec::new();
    let add = |libs: &mut Vec<PathBuf>, p: PathBuf| {
        if !libs.iter().any(|l| lib_key(l) == lib_key(&p)) {
            libs.push(p);
        }
    };
    add(&mut libs, steam.clone());
    if let Some(lf) = read_vdf(&steam.join("steamapps").join("libraryfolders.vdf")) {
        if let Some(root) = lf.get(&["libraryfolders"]) {
            for (_, v) in root.entries() {
                let Some(p) = v.as_str().or_else(|| v.text(&["path"])).filter(|p| !p.is_empty()) else { continue };
                let p = PathBuf::from(p.replace("\\\\", "\\"));
                if libs.iter().any(|l| lib_key(l) == lib_key(&p)) {
                    continue;
                }
                if answers(&p) {
                    add(&mut libs, p);
                }
            }
        }
    }
    if steam != given {
        add(&mut libs, given);
    }
    static MANIFEST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^appmanifest_[0-9]+\.acf$").unwrap());
    let (mut games, mut seen) = (Vec::new(), HashSet::new());
    for lib in libs {
        let sa = lib.join("steamapps");
        let q = sa.clone();
        let Some(ents) = within(WAIT, move || read_dir_sorted(&q)) else {
            SILENT.lock().unwrap().insert(lib_key(&lib));
            continue;
        };
        for e in ents {
            let name = e.file_name().to_string_lossy().into_owned();
            if !MANIFEST.is_match(&name) {
                continue;
            }
            let Some(m) = read_vdf(&sa.join(&name)) else { continue };
            let Some(st) = m.get(&["AppState"]) else { continue };
            let appid = st.text(&["appid"]).unwrap_or("").to_string();
            let title = st.text(&["name"]).unwrap_or("").to_string();
            if appid.is_empty() || NEVER_GAMES.is_match(&title) || (hide_tools && is_tool(&appid, &title)) {
                continue;
            }
            // a game whose update is waiting, paused or running drops the "fully installed" flag: it's on disk and plays
            let flags = vdf::bits(st.num(&["StateFlags"]));
            let dir = st.text(&["installdir"]).filter(|d| !d.is_empty()).map(|d| lib.join("steamapps").join("common").join(d));
            let installed = if flags & 4 != 0 {
                true
            } else if flags & 1 != 0 {
                false
            } else {
                st.num(&["SizeOnDisk"]) > 0.0 && dir.as_deref().is_some_and(is_dir)
            };
            if !installed || !seen.insert(appid.clone()) {
                continue;
            }
            let (minutes, last) = stats.iter().find(|s| s.0 == appid).map_or((0, 0), |s| (s.1, s.2));
            let size = st.num(&["SizeOnDisk"]);
            let own_last = st.num(&["LastPlayed"]);
            games.push(SteamGame {
                front: find_art(&steam, &appid, &[format!("{appid}_library_600x900_2x.jpg"), format!("{appid}_library_600x900.jpg")]),
                hero: find_art(&steam, &appid, &[format!("{appid}_library_hero.jpg")]),
                appid,
                title,
                size_bytes: if size.is_finite() && size > 0.0 { size as u64 } else { 0 },
                dir,
                minutes,
                last_played: if last > 0 { last } else if own_last.is_finite() && own_last > 0.0 { own_last as u64 } else { 0 },
            });
        }
    }
    games.sort_by(|a, b| crate::collate::compare_plain(&a.title, &b.title));
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    }

    fn manifest(lib: &Path, id: &str, name: &str, flags: u32, dir: &str) {
        write(
            &lib.join(format!("steamapps/appmanifest_{id}.acf")),
            &format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"installdir\"\t\t\"{dir}\"\n\t\"SizeOnDisk\"\t\t\"1000\"\n\t\"LastPlayed\"\t\t\"5\"\n}}\n"),
        );
    }

    #[test]
    fn libraries_and_games() {
        let t = tempfile::tempdir().unwrap();
        let (steam, other) = (t.path().join("Steam"), t.path().join("Library"));
        write(
            &steam.join("steamapps/libraryfolders.vdf"),
            &format!("\"libraryfolders\" {{ \"0\" {{ \"path\" \"{}\" }} \"1\" {{ \"path\" \"{}\" }} \"2\" {{ \"path\" \"{}\" }} }}", steam.display(), other.display(), t.path().join("gone").display()),
        );
        manifest(&steam, "10", "Zeta Run", 4, "Zeta");
        manifest(&steam, "228980", "Steamworks Common Redistributables", 4, "Common");
        manifest(&steam, "20", "Alpha Soundtrack", 4, "AlphaOST");
        manifest(&steam, "30", "Uninstalled", 1, "Un");
        manifest(&other, "40", "beta quest", 1026, "Beta");
        std::fs::create_dir_all(other.join("steamapps/common/Beta")).unwrap();
        manifest(&other, "10", "Zeta Run", 4, "Zeta");
        write(&steam.join("appcache/librarycache/40/abc/library_600x900.jpg"), "");
        write(&steam.join("appcache/librarycache/10_library_600x900_2x.jpg"), "");
        write(
            &steam.join("userdata/123/config/localconfig.vdf"),
            "\"UserLocalConfigStore\" { \"Software\" { \"Valve\" { \"Steam\" { \"apps\" { \"10\" { \"Playtime\" \"125\" \"LastPlayed\" \"99\" } } } } } }",
        );
        // given as a library on another drive: Steam's own folder lists every library
        let games = scan(&other.to_string_lossy(), Some(&steam.to_string_lossy()), true);
        let got: Vec<(&str, u64, u64)> = games.iter().map(|g| (g.title.as_str(), g.minutes, g.last_played)).collect();
        assert_eq!(got, [("beta quest", 0, 5), ("Zeta Run", 125, 99)]);
        assert_eq!(games[0].front, Some(steam.join("appcache/librarycache/40/abc/library_600x900.jpg")));
        assert_eq!(games[1].front, Some(steam.join("appcache/librarycache/10_library_600x900_2x.jpg")));
        assert_eq!(games[1].dir, Some(steam.join("steamapps/common/Zeta")));
        assert_eq!(scan(&steam.to_string_lossy(), None, false).len(), 3);
        assert_eq!(steam_root(&format!("\"{}\"", steam.join("steamapps").display())), Some(steam.clone()));
        assert_eq!(steam_root(&steam.join("steam.sh").to_string_lossy()), Some(steam));
        assert_eq!(steam_root(""), None);
    }
}

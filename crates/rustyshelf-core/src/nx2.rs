// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/nx2.js, as RustyShelf has it.
//! Switch 2 games. Most come unpacked: a folder with exefs/main (+ main.npdm) and romfs, or the same split as base/ and
//! updates/<version>/ (each update a full copy), or just <version>/ with no base. The shelf's file for such a game is
//! its base exefs/main (or its only version's), so favourites and history stay put when an update is added; the newest
//! version's is the one started. The emulator builds (Eglantine and its forks) are listed for the choice at Play.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::paths::is_file;

static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(exefs|romfs|updates?|dlc|mods|patches|base)$").unwrap());
static ID_IN_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[((?:0100|0400)[0-9A-F]{12})\]").unwrap());
static EXEFS_MAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(^|[\\/])exefs[\\/]main$").unwrap());

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn dirs(d: &Path) -> Vec<String> {
    crate::scanner::read_dir_sorted(d)
        .into_iter()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn is_version(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

fn version_number(n: &str) -> f64 {
    n.parse().unwrap_or(f64::INFINITY)
}

fn main_of(d: &Path) -> Option<PathBuf> {
    let f = d.join("exefs").join("main");
    (is_file(&f) && is_file(&d.join("exefs").join("main.npdm"))).then_some(f)
}

/// Whether a shelf file is an unpacked game's exefs/main.
pub fn is_unpacked(file: &Path) -> bool {
    EXEFS_MAIN.is_match(&file.to_string_lossy())
}

/// The unpacked games under a games folder: each one's base exefs/main, or its lowest version's.
pub fn find(root: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    for n in dirs(root) {
        if n.starts_with('.') {
            continue;
        }
        let d = root.join(&n);
        if let Some(m) = main_of(&d).or_else(|| main_of(&d.join("base"))) {
            out.push(m);
            continue;
        }
        let mut vers: Vec<String> = dirs(&d).into_iter().filter(|v| is_version(v)).collect();
        vers.sort_by(|a, b| version_number(a).total_cmp(&version_number(b)));
        if let Some(m) = vers.iter().find_map(|v| main_of(&d.join(v))) {
            out.push(m);
        } else if depth > 0 && !SKIP.is_match(&n) {
            find(&d, depth - 1, out);
        }
    }
}

/// The game's folder for a shelf file: …/Game/exefs/main, …/Game/base/exefs/main, …/Game/131072/exefs/main or
/// …/Game/updates/131072/exefs/main → …/Game.
pub fn game_root(file: &Path) -> PathBuf {
    let up = |p: &Path| p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut d = up(&up(file));
    if name_of(&d).eq_ignore_ascii_case("base") {
        return up(&d);
    }
    if is_version(&name_of(&d)) {
        d = up(&d);
        let n = name_of(&d).to_lowercase();
        if n == "update" || n == "updates" {
            d = up(&d);
        }
    }
    d
}

/// The file to start: the newest full version's exefs/main (updates/<version> or <version> beside base).
pub fn boot_file(file: &Path) -> PathBuf {
    if !is_unpacked(file) {
        return file.to_path_buf();
    }
    let root = game_root(file);
    let mut list: Vec<(f64, PathBuf)> = Vec::new();
    for d in [root.join("updates"), root.clone()] {
        for n in dirs(&d) {
            if is_version(&n) {
                list.push((version_number(&n), d.join(&n)));
            }
        }
    }
    list.sort_by(|a, b| b.0.total_cmp(&a.0));
    list.iter().find_map(|(_, d)| main_of(d)).unwrap_or_else(|| file.to_path_buf())
}

/// The program id in main.npdm's ACI0 block, e.g. 0400C3F00006E000; empty when there's none.
pub fn title_id(file: &Path) -> String {
    let read = || -> Option<String> {
        let b = fs::read(file.parent()?.join("main.npdm")).ok()?;
        if b.get(..4)? != b"META" {
            return None;
        }
        let aci = u32::from_le_bytes(b.get(0x70..0x74)?.try_into().ok()?) as usize;
        if b.get(aci..aci + 4)? != b"ACI0" {
            return None;
        }
        let id = u64::from_le_bytes(b.get(aci + 0x10..aci + 0x18)?.try_into().ok()?);
        Some(format!("{id:016X}"))
    };
    read().unwrap_or_default()
}

fn id_in_name(s: &str) -> String {
    ID_IN_NAME.captures(s).map(|c| c[1].to_uppercase()).unwrap_or_default()
}

/// Unpacked games carry no name (it's in the control data, not dumped): known title ids first, so a folder called
/// "SR ROM" still shows the real title.
fn known(id: &str) -> Option<&'static str> {
    Some(match id {
        "0400C3F00006E000" => "Mario Kart World",
        "0400380020056000" => "Splatoon Raiders",
        "040074A01BF12000" => "Donkey Kong Bananza",
        "04007EF01F8AE000" => "Fire Emblem: Fortune's Weave",
        "0400D4201C6EA000" => "Mario Tennis Fever",
        "04008E001F098000" => "Nintendo Switch Sports Resort",
        "0400A3A01EE4C000" => "Pokémon Pokopia",
        "0100F43008C44000" => "Pokémon Legends: Z-A - Nintendo Switch 2 Edition",
        // Switch 2 Editions keep their Switch game's id: the folder or file name carries it
        "010015100B514000" => "Super Mario Bros. Wonder – Nintendo Switch 2 Edition",
        _ => return None,
    })
}

/// Known games without a readable id (a .nup carries none in its name), by their tidied name.
fn known_name(tidied: &str) -> Option<&'static str> {
    match tidied.to_lowercase().as_str() {
        "hyrule warriors age of imprisonment" => Some("Hyrule Warriors: Age of Imprisonment"),
        _ => None,
    }
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(TAGS, r"\[[^\]]*\]|\([^)]*\)");
re!(DOT_VERSION, r"(?i)[_.]v?[0-9]+(\.[0-9]+)+(?-u:\b)");
re!(SEPARATORS, r"[._]+");
re!(CAMEL, r"([a-z])([A-Z])");
re!(SWITCH2, r"(?i)(?-u:\b)Switch2(?-u:\b)");
re!(DUMP_WORDS, r"(?i)(?-u:\b)(base|rom|unpacked|extracted|exefs|romfs|nsw2?)(?-u:\b)");
re!(VERSION, r"(?i)(?-u:\b)v[0-9]+(\.[0-9]+)*(?-u:\b)");
re!(SPACES, r"\s+");
re!(ENDS, r"^[\s-]+|[\s-]+$");

/// A readable name from a folder or file name: dump words go ("base", "[BASE]", "ROM", version and id tags) and
/// joined words split ("MarioTennisFever_v1.1.1" → "Mario Tennis Fever").
pub fn tidy(n: &str) -> String {
    let n = TAGS.replace_all(n, " ");
    let n = DOT_VERSION.replace_all(&n, " ");
    let n = SEPARATORS.replace_all(&n, " ");
    let n = CAMEL.replace_all(&n, "$1 $2");
    let n = SWITCH2.replace_all(&n, "Switch 2");
    let n = DUMP_WORDS.replace_all(&n, " ");
    let n = VERSION.replace_all(&n, " ");
    let n = SPACES.replace_all(&n, " ");
    let n = ENDS.replace_all(&n, "").trim().to_string();
    known_name(&n).map(String::from).unwrap_or(n)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub title: String,
    /// empty when unknown
    pub title_id: String,
    /// the game's folder (an unpacked game) or the folder the file is in
    pub folder: PathBuf,
}

/// An unpacked game, from its exefs/main.
pub fn info(file: &Path) -> Info {
    let root = game_root(file);
    let id = title_id(file);
    let folder = name_of(&root);
    let title = known(&id)
        .or_else(|| known(&id_in_name(&folder)))
        .map(String::from)
        .unwrap_or_else(|| Some(tidy(&folder)).filter(|t| !t.is_empty()).unwrap_or(folder));
    Info { title, title_id: id, folder: root }
}

/// A packed game (.nsp / .xci / .nup): its id from the name when there is one, a known title, else the tidied name.
pub fn file_info(file: &Path) -> Info {
    let name = name_of(file);
    let base = match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[..i],
        _ => &name,
    };
    let id = id_in_name(base);
    let title = known(&id).map(String::from).unwrap_or_else(|| Some(tidy(base)).filter(|t| !t.is_empty()).unwrap_or(base.to_string()));
    Info { title, title_id: id, folder: file.parent().map(Path::to_path_buf).unwrap_or_default() }
}

// ---------- the emulator builds ----------

/// A Switch 2 emulator build (Eglantine or a fork), for the "which build?" choice at Play.
#[derive(Debug, Clone, PartialEq)]
pub struct Build {
    pub exe: PathBuf,
    /// the folder it's kept under, when it sits deeper than one folder ("Mario tennis")
    pub group: String,
    /// the emulator and its build ("Lichii · K code AI")
    pub name: String,
    pub label: String,
    /// the program's modified time (ms)
    pub date: f64,
    /// whether it opens .nup games
    pub nup: bool,
}

/// A path as the Electron app compares them (`path.resolve(p).toLowerCase()`).
pub fn low(p: &Path) -> String {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    crate::js::normalize(&p.to_string_lossy()).to_string_lossy().to_lowercase()
}

fn mtime_ms(m: &fs::Metadata) -> f64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| (d.as_nanos() as f64 / 1e6).round())
}

/// Does a file hold this text, as plain bytes or in UTF-16 (Windows and Qt programs keep their strings that way)? Read
/// in 4 MB pieces: a build is 100+ MB.
fn file_has(f: &Path, text: &str) -> bool {
    use std::io::Read;
    let wide: Vec<u8> = text.bytes().flat_map(|b| [b, 0]).collect();
    let hex = |b: &[u8]| b.iter().map(|c| format!("\\x{c:02x}")).collect::<String>();
    let Ok(re) = regex::bytes::Regex::new(&format!("(?-u){}|{}", hex(text.as_bytes()), hex(&wide))) else { return false };
    let Ok(mut fh) = fs::File::open(f) else { return false };
    const CH: usize = 4 << 20;
    let over = wide.len();
    let mut buf = vec![0u8; CH + over];
    let mut keep = 0;
    loop {
        let n = match fh.read(&mut buf[keep..keep + CH]) {
            Ok(0) | Err(_) => return false,
            Ok(n) => keep + n,
        };
        if re.is_match(&buf[..n]) {
            return true;
        }
        let k = (over - 1).min(n);
        buf.copy_within(n - k..n, 0);
        keep = k;
    }
}

static NOT_OWN_DLL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(qt\d|vcruntime|msvcp|concrt|vccorlib|api-ms-|ucrtbase|sdl\d|vulkan|d3d|dxcompiler|dxil|libcrypto|libssl|libcurl|av(codec|format|util|filter|device)|sw(scale|resample)|postproc|opengl|libgcc|libstdc|libwinpthread|zlib|openal|discord|steam_api|libusb|libEGL|libGLES)").unwrap()
});

/// The emulator's own libraries beside its program (a build can keep its file dialog in one), not the runtimes every
/// build carries; a dozen at most.
fn own_dlls(exe: &Path) -> Vec<PathBuf> {
    if !exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        return Vec::new();
    }
    let Some(dir) = exe.parent() else { return Vec::new() };
    crate::scanner::read_dir_sorted(dir)
        .into_iter()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().ends_with(".dll") && !NOT_OWN_DLL.is_match(n))
        .take(12)
        .map(|n| dir.join(n))
        .collect()
}

/// An AppImage: its runtime is an ELF with "AI" at byte 8, or the name says so.
fn is_app_image(exe: &Path) -> bool {
    use std::io::Read;
    if exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("appimage")) {
        return true;
    }
    let mut b = [0u8; 11];
    fs::File::open(exe).and_then(|mut f| f.read_exact(&mut b)).is_ok() && b[0] == 0x7f && &b[1..4] == b"ELF" && &b[8..10] == b"AI"
}

fn files_under(d: &Path, out: &mut Vec<PathBuf>, max: usize) {
    for e in crate::scanner::read_dir_sorted(d) {
        if out.len() >= max {
            break;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => files_under(&e.path(), out, max),
            Ok(t) if t.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

/// An AppImage's program is inside a compressed SquashFS, so its text never shows in the file itself: its own
/// --appimage-extract unpacks the programs (usr/bin, else everything) into a folder of our own, without starting the
/// emulator. None when it couldn't be looked into (an old type 1 AppImage, no room in the temp folder).
fn app_image_has(exe: &Path, text: &str) -> Option<bool> {
    let dir = std::env::temp_dir().join(format!("rustyshelf-build-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    fs::create_dir_all(&dir).ok()?;
    let unpack = |args: &[&str]| {
        let child = std::process::Command::new(exe)
            .args(args)
            .current_dir(&dir)
            .env_remove("APPIMAGE_EXTRACT_AND_RUN")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return };
        let until = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while matches!(child.try_wait(), Ok(None)) {
            if std::time::Instant::now() > until {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    let root = dir.join("squashfs-root");
    unpack(&["--appimage-extract", "usr/bin/*"]);
    let mut files = Vec::new();
    files_under(&root, &mut files, 400);
    if files.is_empty() {
        unpack(&["--appimage-extract"]);
        files_under(&root, &mut files, 4000);
    }
    let found = (!files.is_empty()).then(|| files.iter().any(|f| file_has(f, text)));
    let _ = fs::remove_dir_all(&dir);
    found
}

/// Each build's .nup answer by exe, date and size, kept in `data/sw2-builds.json`: reading every build's program
/// again made the first "which build?" lag.
static MEMO: LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, serde_json::Map<String, serde_json::Value>>>> = LazyLock::new(Default::default);

/// Whether a build opens .nup games: the newer builds name the format in their file dialog.
fn opens_nup(exe: &Path, m: &fs::Metadata, memo_file: &Path) -> bool {
    // (v3: the key of answers that looked into AppImages, UTF-16 text and the DLLs beside the program)
    let key = format!("v3|{}|{}|{}", exe.to_string_lossy().to_lowercase(), mtime_ms(m), m.len());
    let known = MEMO
        .lock()
        .unwrap()
        .entry(memo_file.to_path_buf())
        .or_insert_with(|| {
            let read = fs::read(memo_file).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            read.and_then(|v| v.as_object().cloned()).unwrap_or_default().into_iter().filter(|(_, v)| v.is_boolean()).collect()
        })
        .get(&key)
        .and_then(serde_json::Value::as_bool);
    if let Some(v) = known {
        return v;
    }
    let mut yes = if is_app_image(exe) { app_image_has(exe, ".nup") } else { Some(file_has(exe, ".nup")) };
    if yes == Some(false) && own_dlls(exe).iter().any(|d| file_has(d, ".nup")) {
        yes = Some(true);
    }
    // couldn't look: the emulator decides rather than the game being turned away
    let Some(yes) = yes else { return true };
    let mut g = MEMO.lock().unwrap();
    if let Some(map) = g.get_mut(memo_file) {
        map.insert(key, yes.into());
        let n = map.len();
        let kept: serde_json::Map<_, _> = map.iter().skip(n.saturating_sub(200)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let _ = fs::write(memo_file, serde_json::Value::Object(kept).to_string());
    }
    yes
}

static HELPER_EXE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(eden-cli|vc_?redist.*|.*updater.*|.*update\.exe$|unins.*|uninstall.*|crashpad.*|qtwebengineprocess)(\.exe)?$").unwrap());
static NOT_SW2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(eden|citron|yuzu|suyu|sudachi|ryujinx|ryubing|dolphin|cemu|pcsx2|rpcs3|shadps4|ppsspp|xenia|azahar|lime3ds|citra|duckstation|retroarch|melonds|mgba|flycast|rmg|xemu|rustyshelf)").unwrap()
});
static NOT_BUILD_DIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(user|licenses?|plugins|platforms|styles|imageformats|cache|log|sdmc|nand|load|shader|generic|iconengines|networkinformation|tls|translations|romfs|exefs|base|updates?|dlc)$").unwrap()
});
static GROUP_WORDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(forks?|builds?)\b").unwrap());

/// A build folder's program: eglantine.exe, or (forks that rename it: lichii.exe, orchid.exe…) the biggest program
/// beside eden-cli.exe that isn't a helper.
fn build_exe(dir: &Path, files: &[String]) -> Option<PathBuf> {
    if let Some(e) = files.iter().find(|n| n.eq_ignore_ascii_case("eglantine.exe")) {
        return Some(dir.join(e));
    }
    if !files.iter().any(|n| n.eq_ignore_ascii_case("eden-cli.exe")) {
        return None;
    }
    let mut best: Option<(u64, PathBuf)> = None;
    for n in files {
        if !n.to_lowercase().ends_with(".exe") || HELPER_EXE.is_match(n) {
            continue;
        }
        let p = dir.join(n);
        let Ok(s) = fs::metadata(&p).map(|m| m.len()) else { continue };
        if best.as_ref().is_none_or(|(b, _)| s > *b) {
            best = Some((s, p));
        }
    }
    best.map(|(_, p)| p)
}

fn exes_under(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let ents = crate::scanner::read_dir_sorted(dir);
    let files: Vec<String> = ents.iter().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    out.extend(build_exe(dir, &files));
    // (Linux) a build is an AppImage, often several side by side; the other consoles' emulators kept there are left out
    if !cfg!(windows) {
        out.extend(files.iter().filter(|n| n.to_lowercase().ends_with(".appimage") && !NOT_SW2.is_match(n)).map(|n| dir.join(n)));
    }
    for e in &ents {
        let n = e.file_name().to_string_lossy().into_owned();
        if depth > 0 && e.file_type().is_ok_and(|t| t.is_dir()) && !NOT_BUILD_DIR.is_match(&n) {
            exes_under(&e.path(), depth - 1, out);
        }
    }
}

/// Several at once: the first "which build?" after new builds were added took one AppImage at a time.
fn with_nup(found: Vec<(Build, fs::Metadata)>, memo_file: &Path) -> Vec<Build> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::Mutex<Option<Build>>> = found.iter().map(|_| Default::default()).collect();
    std::thread::scope(|s| {
        for _ in 0..found.len().min(3) {
            s.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((b, m)) = found.get(k) else { break };
                *out[k].lock().unwrap() = Some(Build { nup: opens_nup(&b.exe, m, memo_file), ..b.clone() });
            });
        }
    });
    out.into_iter().filter_map(|b| b.into_inner().unwrap()).collect()
}

fn builds_now(roots: &[PathBuf], memo_file: &Path) -> Vec<Build> {
    let mut seen = std::collections::HashSet::new();
    let mut found = Vec::new();
    for r in roots {
        let mut exes = Vec::new();
        exes_under(r, 4, &mut exes);
        for exe in exes {
            if !seen.insert(exe.to_string_lossy().to_lowercase()) {
                continue;
            }
            let Ok(m) = fs::metadata(&exe) else { continue };
            let rel: Vec<String> = exe.parent().and_then(|d| d.strip_prefix(r).ok()).map(|d| d.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            let group = if rel.len() > 1 { GROUP_WORDS.replace_all(&rel[0], "").split_whitespace().collect::<Vec<_>>().join(" ") } else { String::new() };
            let name = crate::emuname::label(&exe.to_string_lossy());
            let label = if !group.is_empty() && !name.to_lowercase().contains(&group.to_lowercase()) { format!("{group} · {name}") } else { name.clone() };
            found.push((Build { date: mtime_ms(&m), exe, group, name, label, nup: false }, m));
        }
    }
    let mut list = with_nup(found, memo_file);
    list.sort_by(|a, b| crate::collate::compare(&a.label, &b.label).then(b.date.total_cmp(&a.date)));
    list
}

/// Every build in the Switch 2 emulator folders (four folders deep). Kept for 30 s: Play asks for the list, then the
/// launch asks again.
pub fn builds(roots: &[PathBuf], memo_file: &Path) -> Vec<Build> {
    type Kept = Option<(String, std::time::Instant, Vec<Build>)>;
    static CACHE: LazyLock<std::sync::Mutex<Kept>> = LazyLock::new(Default::default);
    let key = roots.iter().map(|r| r.to_string_lossy().to_lowercase()).collect::<Vec<_>>().join("|");
    if let Some((k, at, list)) = CACHE.lock().unwrap().as_ref() {
        if *k == key && at.elapsed().as_secs() < 30 {
            return list.clone();
        }
    }
    let list = builds_now(roots, memo_file);
    *CACHE.lock().unwrap() = Some((key, std::time::Instant::now(), list.clone()));
    list
}

/// One build outside the build folders (a game's own choice, the shelf's, one added in Settings), described like the
/// others. `hint`: what it is ("Shelf").
pub fn build_of(exe: &Path, hint: &str, memo_file: &Path) -> Option<Build> {
    let m = fs::metadata(exe).ok()?;
    let name = crate::emuname::label(&exe.to_string_lossy());
    let label = if hint.is_empty() { name.clone() } else { format!("{name} ({hint})") };
    Some(Build { exe: exe.to_path_buf(), group: String::new(), label, date: mtime_ms(&m), nup: opens_nup(exe, &m, memo_file), name })
}

/// Settings → Switch 2's lists of paths (`buildDirs`, `buildExes`, `buildHidden`): strings, each once, twenty at most.
pub fn added(cfg: &crate::config::Config, key: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let list = cfg.raw().get("systems").and_then(|s| s.get("sw2")).and_then(|s| s.get(key)).and_then(serde_json::Value::as_array);
    for v in list.into_iter().flatten().filter_map(serde_json::Value::as_str) {
        if !v.is_empty() && v.encode_utf16().count() <= 1024 && !out.iter().any(|o| o == v) {
            out.push(v.to_string());
        }
    }
    out.truncate(20);
    out
}

/// The shelf's build, expanded; empty when none is set.
pub fn shelf_build(cfg: &crate::config::Config, app_root: &Path) -> PathBuf {
    let e = cfg.raw().get("systems").and_then(|s| s.get("sw2")).and_then(|s| s.get("emulator")).and_then(serde_json::Value::as_str).unwrap_or("");
    if e.trim().is_empty() { PathBuf::new() } else { crate::launch::emu_path(e, app_root) }
}

/// The folders builds are looked for in: `Switch 2 emu` beside RustyShelf, the ones added in Settings, and the top
/// folder above the shelf's build that looks like a builds folder ("Switch 2 emu" above "Selvet Forks/…/eglantine.exe"),
/// six levels up at most. The widest come first, so builds are named from the top.
pub fn roots(cfg: &crate::config::Config, app_root: &Path) -> Vec<PathBuf> {
    static BUILDS_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)switch ?2|eglantine|forks?").unwrap());
    let mut r = vec![app_root.join("Switch 2 emu")];
    r.extend(added(cfg, "buildDirs").into_iter().map(PathBuf::from));
    let e = shelf_build(cfg, app_root);
    if !e.as_os_str().is_empty() {
        let (mut d, mut top) = (e.parent().map(Path::to_path_buf).unwrap_or_default(), None);
        for _ in 0..6 {
            let Some(up) = d.parent().filter(|u| *u != d).map(Path::to_path_buf) else { break };
            if BUILDS_DIR.is_match(&name_of(&d)) {
                top = Some(d.clone());
            }
            d = up;
        }
        r.extend(top);
    }
    let mut u: Vec<PathBuf> = Vec::new();
    for p in r {
        if !u.iter().any(|q| low(q) == low(&p)) {
            u.push(p);
        }
    }
    u.sort_by_key(|p| p.as_os_str().len());
    u
}

/// The builds to choose from for a game (`file`; empty for Settings' list): the build folders' (less the ones removed
/// in Settings), plus the game's own choice, the shelf's build and the ones added in Settings when they sit elsewhere.
pub fn list_for(cfg: &crate::config::Config, app_root: &Path, file: &str) -> Vec<Build> {
    let memo = crate::paths::data_dir(app_root).join("sw2-builds.json");
    let hidden: Vec<String> = added(cfg, "buildHidden").iter().map(|h| low(Path::new(h))).collect();
    let mut list: Vec<Build> = builds(&roots(cfg, app_root), &memo).into_iter().filter(|b| !hidden.contains(&low(&b.exe))).collect();
    let own = if file.is_empty() { None } else { cfg.game_emulator(&format!("sw2:{file}")).map(|e| PathBuf::from(crate::paths::expand(e, app_root))) };
    let mut more: Vec<(PathBuf, &str)> = own.into_iter().map(|e| (e, "This game's")).collect();
    more.push((shelf_build(cfg, app_root), "Shelf"));
    more.extend(added(cfg, "buildExes").into_iter().map(|x| (PathBuf::from(x), "Added")));
    for (exe, hint) in more {
        if exe.as_os_str().is_empty() || hidden.contains(&low(&exe)) || list.iter().any(|b| low(&b.exe) == low(&exe)) || !is_file(&exe) {
            continue;
        }
        list.extend(build_of(&exe, hint, &memo));
    }
    list
}

/// What Play offers for a game: the builds, whether it's a .nup game, and the build to start on (the one it was last
/// started in, its own choice or the shelf's; for a .nup game one seen to open .nup first, else the newest).
pub struct Offer {
    pub builds: Vec<Build>,
    pub nup: bool,
    pub first: Option<PathBuf>,
}

pub fn offer(cfg: &crate::config::Config, app_root: &Path, file: &str) -> Offer {
    let builds = list_for(cfg, app_root, file);
    let nup = file.to_lowercase().ends_with(".nup");
    let ok = |b: &&Build| !nup || b.nup;
    let last = cfg.raw().get("ui").and_then(|u| u.get("sw2Last")).and_then(|l| l.get(format!("sw2:{file}"))).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let own = cfg.game_emulator(&format!("sw2:{file}")).map(|e| crate::paths::expand(e, app_root)).unwrap_or_default();
    let want: Vec<String> = [PathBuf::from(last), PathBuf::from(own), shelf_build(cfg, app_root)].iter().filter(|p| !p.as_os_str().is_empty()).map(|p| low(p)).collect();
    let find = |w: &String, need: bool| builds.iter().find(|b| low(&b.exe) == *w && (!need || ok(b)));
    let newest = || builds.iter().filter(ok).max_by(|a, b| a.date.total_cmp(&b.date));
    let first = want.iter().find_map(|w| find(w, true)).or_else(newest).or_else(|| want.iter().find_map(|w| find(w, false))).map(|b| b.exe.clone());
    Offer { builds, nup, first }
}

/// The build chosen at Play, checked: it's one of the listed builds, and opens a .nup game.
pub fn chosen(cfg: &crate::config::Config, app_root: &Path, file: &str, exe: &Path) -> Result<Build, String> {
    let Some(b) = list_for(cfg, app_root, file).into_iter().find(|b| low(&b.exe) == low(exe)) else {
        return Err(format!("That Switch 2 build isn't in the emulator folders any more ({}).", exe.display()));
    };
    if file.to_lowercase().ends_with(".nup") && !b.nup {
        return Err(format!("{} can't open .nup games. Choose a newer build.", b.label));
    }
    Ok(b)
}

/// The newest build, when the shelf's build is gone (forks get swapped often).
pub fn replacement(cfg: &crate::config::Config, app_root: &Path) -> Option<Build> {
    let e = shelf_build(cfg, app_root);
    if e.as_os_str().is_empty() || is_file(&e) {
        return None;
    }
    builds(&roots(cfg, app_root), &crate::paths::data_dir(app_root).join("sw2-builds.json")).into_iter().max_by(|a, b| a.date.total_cmp(&b.date))
}

/// main.npdm with a program id, for tests.
#[cfg(test)]
pub fn build_npdm(id: u64) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    b[..4].copy_from_slice(b"META");
    b[0x70..0x74].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"ACI0");
    b[0x90..0x98].copy_from_slice(&id.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpacked(dir: &Path, id: Option<u64>) {
        fs::create_dir_all(dir.join("exefs")).unwrap();
        fs::write(dir.join("exefs/main"), b"x").unwrap();
        fs::write(dir.join("exefs/main.npdm"), id.map(build_npdm).unwrap_or_default()).unwrap();
    }

    #[test]
    fn names() {
        assert_eq!(tidy("MarioTennisFever_v1.1.1"), "Mario Tennis Fever");
        assert_eq!(tidy("NintendoSwitch2_WelcomeTour [BASE]"), "Nintendo Switch 2 Welcome Tour");
        assert_eq!(tidy("Hyrule.Warriors.Age.of.Imprisonment.ROM"), "Hyrule Warriors: Age of Imprisonment");
        assert_eq!(file_info(Path::new("/g/x [0400C3F00006E000].nsp")).title, "Mario Kart World");
    }

    #[test]
    fn versions() {
        let t = tempfile::tempdir().unwrap();
        let g = t.path().join("SR ROM");
        unpacked(&g.join("base"), Some(0x0400_C3F0_0006_E000));
        unpacked(&g.join("updates/131072"), None);
        unpacked(&g.join("updates/65536"), None);
        let mut found = Vec::new();
        find(t.path(), 3, &mut found);
        assert_eq!(found, vec![g.join("base/exefs/main")]);
        assert_eq!(game_root(&found[0]), g);
        assert_eq!(boot_file(&found[0]), g.join("updates/131072/exefs/main"));
        let i = info(&found[0]);
        assert_eq!((i.title.as_str(), i.title_id.as_str()), ("Mario Kart World", "0400C3F00006E000"));

        let v = t.path().join("Group/Other Game");
        unpacked(&v.join("200"), None);
        unpacked(&v.join("30"), None);
        let mut found = Vec::new();
        find(&t.path().join("Group"), 3, &mut found);
        assert_eq!(found, vec![v.join("30/exefs/main")]);
        assert_eq!(info(&found[0]).title, "Other Game");
    }
    fn write(p: &Path, bytes: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn cfg(v: serde_json::Value) -> crate::config::Config {
        crate::config::Config::from_value(&v).unwrap()
    }

    #[test]
    fn build_programs() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Switch 2 emu");
        write(&root.join("Mario tennis forks/Lichii/eden-cli.exe"), b"cli");
        write(&root.join("Mario tennis forks/Lichii/lichii.exe"), &[0u8; 64]);
        write(&root.join("Mario tennis forks/Lichii/updater.exe"), &[0u8; 200]);
        write(&root.join("Plain/eglantine.exe"), b"File (*.nsp *.xci)");
        // UTF-16, as Qt programs keep their strings
        let wide: Vec<u8> = "Games (*.nup)".bytes().flat_map(|b| [b, 0]).collect();
        write(&root.join("Wide/eglantine.exe"), &wide);
        write(&root.join("Dll/eglantine.exe"), b"x");
        write(&root.join("Dll/eglantine-ui.dll"), b"Open .nup");
        write(&root.join("Dll/Qt6Core.dll"), b"");
        write(&root.join("Plain/romfs/eglantine.exe"), b"game data, not a build");
        let memo = t.path().join("sw2-builds.json");
        let list = builds(std::slice::from_ref(&root), &memo);
        let by = |p: &str| list.iter().find(|b| b.exe == root.join(p)).unwrap_or_else(|| panic!("{p} not listed: {list:?}"));
        assert_eq!(list.len(), 4);
        let l = by("Mario tennis forks/Lichii/lichii.exe");
        assert_eq!(l.group, "Mario tennis");
        assert!(l.label.starts_with("Mario tennis · ") || l.label == l.name, "{}", l.label);
        assert!(!l.nup);
        assert!(!by("Plain/eglantine.exe").nup);
        assert!(by("Wide/eglantine.exe").nup);
        assert!(by("Dll/eglantine.exe").nup);
        let kept: serde_json::Value = serde_json::from_slice(&fs::read(&memo).unwrap()).unwrap();
        assert_eq!(kept.as_object().unwrap().len(), 4);
    }

    #[test]
    fn choosing() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path();
        let found = app.join("Switch 2 emu/Old/eglantine.exe");
        write(&found, b"no");
        let elsewhere = app.join("Elsewhere/orchid/eglantine.exe");
        write(&elsewhere, b"*.nup");
        let gone = app.join("Switch 2 emu/Gone/eglantine.exe");
        write(&gone, b"x");
        let (f, e, g) = (found.to_string_lossy(), elsewhere.to_string_lossy(), gone.to_string_lossy());
        let c = cfg(serde_json::json!({"systems": {"sw2": {"emulator": f, "buildExes": [e], "buildHidden": [g]}}}));
        let list = list_for(&c, app, "");
        assert_eq!(list.iter().map(|b| b.exe.clone()).collect::<Vec<_>>(), vec![found.clone(), elsewhere.clone()]);
        assert!(list[1].label.ends_with("(Added)"));
        // a .nup game starts on a build that opens .nup, even over the shelf's
        let o = offer(&c, app, "/games/x.nup");
        assert!(o.nup);
        assert_eq!(o.first.as_deref(), Some(elsewhere.as_path()));
        assert_eq!(offer(&c, app, "/games/x.nsp").first.as_deref(), Some(found.as_path()));
        // the build a game was last started in comes first
        let c = cfg(serde_json::json!({"systems": {"sw2": {"emulator": f, "buildExes": [e]}}, "ui": {"sw2Last": {"sw2:/games/x.nsp": e}}}));
        assert_eq!(offer(&c, app, "/games/x.nsp").first.as_deref(), Some(elsewhere.as_path()));
        assert!(chosen(&c, app, "/games/x.nup", &found).unwrap_err().contains("can't open .nup games"));
        assert!(chosen(&c, app, "/games/x.nup", &elsewhere).is_ok());
        assert!(chosen(&c, app, "/games/x.nsp", &app.join("nowhere.exe")).is_err());
    }

    #[test]
    fn build_folders() {
        let t = tempfile::tempdir().unwrap();
        let app = t.path().join("RustyShelf");
        let shelf = t.path().join("Emus/Switch 2/Selvet Forks/v3/eglantine.exe");
        write(&shelf, b"x");
        let c = cfg(serde_json::json!({"systems": {"sw2": {"emulator": shelf.to_string_lossy(), "buildDirs": ["/more", "/more"]}}}));
        let r = roots(&c, &app);
        let mut want = vec![PathBuf::from("/more"), t.path().join("Emus/Switch 2"), app.join("Switch 2 emu")];
        want.sort_by_key(|p| p.as_os_str().len());
        assert_eq!(r, want);
        assert!(replacement(&c, &app).is_none());
        let c = cfg(serde_json::json!({"systems": {"sw2": {"emulator": t.path().join("Emus/Switch 2/gone/eglantine.exe").to_string_lossy()}}}));
        assert_eq!(replacement(&c, &app).map(|b| b.exe), Some(shelf));
    }
}

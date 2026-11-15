// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/pcgames.js, as RustyShelf has it.
//! PC games beyond Steam: the Epic Games Launcher's installs, GOG Galaxy, Ubisoft Connect, the EA app, Xbox / Game
//! Pass and HoYoPlay (on the PC shelf), the apps Quiver Launcher installed and the programs added by hand in Settings
//! (on the Custom shelf). Everything is read only, the way each store leaves it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::art::resolve;
use crate::js;
use crate::paths::{is_dir, is_file};
use crate::scanner::read_dir_sorted;
use crate::systems::systems;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PcGame {
    pub id: String,
    /// "epic", "gog", "ubisoft", "ea", "xbox", "hoyoplay", "folder", "quiver" or "custom"
    pub source: &'static str,
    pub title: String,
    /// the game's own program; empty when only its store can start it
    pub exe: String,
    pub args: String,
    /// its store's launch link (Epic, Ubisoft Connect, the EA app)
    pub uri: String,
    /// a Windows app's address in shell:AppsFolder, `<package family>!<app id>` (Xbox, a Store app added by hand)
    pub shell_app: String,
    pub dir: String,
    /// a picture's https address (Quiver's)
    pub icon: String,
    pub tags: Vec<String>,
    /// the shelf a game added by hand sits on instead of Custom: "pc", "ports" or a console's id
    pub shelf: Option<String>,
    /// a port or recompilation (the Ports & Recomps shelf), and the console it's a port of when that's known
    pub port: bool,
    pub port_of: String,
    /// Details → Installation, set by hand
    pub installed: Option<bool>,
    /// a PC games folder's game: made for this system (not a Windows program run through Wine)
    pub native: bool,
    /// a PC games folder's game: every program it could start with (its path inside the folder, whether it's made
    /// for this system), the best first
    pub programs: Vec<(String, bool)>,
}

fn read_json(f: &Path) -> Option<Value> {
    let t = String::from_utf8_lossy(&std::fs::read(f).ok()?).into_owned();
    serde_json::from_str(t.strip_prefix('\u{feff}').unwrap_or(&t)).ok()
}

pub fn slug(s: &str) -> String {
    static RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new("[^a-z0-9]+").unwrap());
    let l = s.to_lowercase();
    let t = RUN.replace_all(&l, "-");
    let t = t.strip_prefix('-').unwrap_or(&t);
    let t = t.strip_suffix('-').unwrap_or(t);
    let t = js::slice(t, 60);
    if t.is_empty() {
        "game".into()
    } else {
        t.to_string()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn https(v: Option<&Value>) -> String {
    js::str_of(v).filter(|u| u.starts_with("https://")).unwrap_or("").to_string()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).map(|a| a.iter().map(|x| js::string(Some(x))).collect()).unwrap_or_default()
}

// ---------- Epic Games ----------

pub fn epic_dir() -> PathBuf {
    let pd = std::env::var("ProgramData").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "C:\\ProgramData".into());
    PathBuf::from(js::join(&[&pd, "Epic", "EpicGamesLauncher", "Data", "Manifests"]))
}

/// The games in the Epic Games Launcher's manifests (`*.item`), started through the launcher's own link.
pub fn epic_games(dir: &Path) -> Vec<PcGame> {
    static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.item$").unwrap());
    static ENGINE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^UE_").unwrap());
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    let files = read_dir_sorted(dir).into_iter().filter(|e| ITEM.is_match(&e.file_name().to_string_lossy()));
    for f in files.take(2000) {
        let Some(m) = read_json(&f.path()).filter(|m| m.is_object() || m.is_array()) else { continue };
        let (app, title) = (js::text(m.get("AppName")), js::trim(&js::text(m.get("DisplayName"))).to_string());
        // half-installed, or the Unreal Engine
        if app.is_empty() || title.is_empty() || m.get("bIsIncompleteInstall") == Some(&Value::Bool(true)) || ENGINE.is_match(&app) {
            continue;
        }
        // plugins, engine parts, tools
        if m.get("AppCategories").and_then(Value::as_array).is_some_and(|c| !c.iter().any(|x| js::string(Some(x)) == "games")) {
            continue;
        }
        let loc = m.get("InstallLocation");
        if js::truthy(loc) && !js::str_of(loc).is_some_and(|l| is_dir(Path::new(l))) {
            continue;
        }
        let id = format!("epic-{}", slug(&app));
        if !seen.insert(id.clone()) {
            continue;
        }
        let (ns, item) = (js::text(m.get("CatalogNamespace")), js::text(m.get("CatalogItemId")));
        let which = if !ns.is_empty() && !item.is_empty() { format!("{ns}:{item}:{app}") } else { app };
        let dir = js::filled(loc).unwrap_or("").to_string();
        let exe = match (js::filled(loc), m.get("LaunchExecutable")) {
            (Some(l), e) if js::truthy(e) => js::join(&[l, &js::string(e)]),
            _ => String::new(),
        };
        out.push(PcGame {
            id,
            source: "epic",
            title,
            uri: format!("com.epicgames.launcher://apps/{}?action=launch&silent=true", js::encode_uri_component(&which)),
            dir,
            exe,
            ..PcGame::default()
        });
    }
    out.sort_by(|a, b| crate::collate::compare_exact(&a.title, &b.title));
    out
}

// ---------- GOG Galaxy, Ubisoft Connect, the EA app, Xbox / Game Pass, HoYoPlay ----------

/// A registry key's values by name (lower case); a REG_BINARY value's bytes as text too, under `<name>#text`.
pub type RegKey = HashMap<String, String>;

/// What `reg query <key> /s` printed: each full key and its values, in the order printed.
pub fn parse_reg(out: &str) -> Vec<(String, RegKey)> {
    static VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ {4}(.+?) {4}(REG_\w+)(?: {4}(.*))?$").unwrap());
    static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?i:[0-9a-f]{2})+$").unwrap());
    let mut m: Vec<(String, RegKey)> = Vec::new();
    let mut cur: Option<usize> = None;
    for line in out.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("HKEY_") {
            let key = js::trim(line).to_string();
            cur = Some(match m.iter().position(|(k, _)| *k == key) {
                Some(i) => {
                    m[i].1 = RegKey::new();
                    i
                }
                None => {
                    m.push((key, RegKey::new()));
                    m.len() - 1
                }
            });
            continue;
        }
        let (Some(v), Some(i)) = (VALUE.captures(line), cur) else { continue };
        let (k, data) = (v[1].to_lowercase(), js::trim(v.get(3).map_or("", |d| d.as_str())).to_string());
        if &v[2] == "REG_BINARY" && HEX.is_match(&data) {
            let bytes: Vec<u8> = (0..data.len() / 2).filter_map(|i| u8::from_str_radix(&data[i * 2..i * 2 + 2], 16).ok()).collect();
            m[i].1.insert(format!("{k}#text"), String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string());
        }
        m[i].1.insert(k, data);
    }
    m
}

/// A program's output, as far as it came within the time given; empty when it couldn't be started.
fn run(cmd: &str, args: &[&str], wait: Duration) -> String {
    use std::io::Read;
    let mut c = std::process::Command::new(cmd);
    c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
    let Ok(mut child) = c.spawn() else { return String::new() };
    let Some(mut pipe) = child.stdout.take() else { return String::new() };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = pipe.read_to_end(&mut b);
        let _ = tx.send(b);
    });
    let out = match rx.recv_timeout(wait) {
        Ok(b) => b,
        Err(_) => {
            let _ = child.kill();
            rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
        }
    };
    let _ = child.wait();
    String::from_utf8_lossy(&out).into_owned()
}

fn reg_tree(key: &str) -> Vec<(String, RegKey)> {
    if cfg!(windows) {
        parse_reg(&run("reg", &["query", key, "/s"], Duration::from_secs(20)))
    } else {
        Vec::new()
    }
}

fn val<'a>(v: &'a RegKey, k: &str) -> &'a str {
    v.get(k).map_or("", String::as_str)
}

fn unxml(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn no_end_slash(s: &str) -> &str {
    s.trim_end_matches(['\\', '/'])
}

/// A command line's program and the rest: `"C:\a b\x.exe" -y` → the program, `-y`.
fn split_cmd(c: &str) -> (String, String) {
    static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*"([^"]+)"\s*(.*)$"#).unwrap());
    static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\S+)\s*(.*)$").unwrap());
    match QUOTED.captures(c).or_else(|| BARE.captures(c)) {
        Some(m) => (m[1].to_string(), m[2].to_string()),
        None => (String::new(), String::new()),
    }
}

/// GOG's games from `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games`: started by their own program.
pub fn gog_from_tree(tree: &[(String, RegKey)]) -> Vec<PcGame> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\\Games\\([0-9]+)$").unwrap());
    let mut out = Vec::new();
    for (k, v) in tree {
        // dependsOn: a DLC of another game
        let Some(id) = ID.captures(k).filter(|_| val(v, "dependson").is_empty()) else { continue };
        let title = js::trim(if val(v, "gamename").is_empty() { val(v, "gametitle") } else { val(v, "gamename") });
        let dir = val(v, "path");
        if title.is_empty() || dir.is_empty() || !is_dir(Path::new(dir)) {
            continue;
        }
        let (mut exe, mut args) = split_cmd(val(v, "launchcommand"));
        if exe.is_empty() || !is_file(Path::new(&exe)) {
            exe = if is_file(Path::new(val(v, "exe"))) { val(v, "exe").to_string() } else { main_exe(dir).unwrap_or_default() };
            args = val(v, "launchparam").to_string();
        }
        if exe.is_empty() {
            continue;
        }
        out.push(PcGame { id: format!("gog-{}", &id[1]), source: "gog", title: title.to_string(), exe, args: js::slice(&args, 1000).to_string(), ..PcGame::default() });
    }
    out
}

pub fn gog_games() -> Vec<PcGame> {
    gog_from_tree(&reg_tree("HKLM\\SOFTWARE\\WOW6432Node\\GOG.com\\Games"))
}

struct Hoyo {
    code: &'static str,
    title: &'static str,
    exes: &'static [&'static str],
}

/// HoYoPlay's games by the start of their GameBiz code (main/pcgames.js HOYO, keep in sync).
const HOYO: &[Hoyo] = &[
    Hoyo { code: "hk4e", title: "Genshin Impact", exes: &["GenshinImpact.exe", "YuanShen.exe"] },
    Hoyo { code: "hkrpg", title: "Honkai: Star Rail", exes: &["StarRail.exe"] },
    Hoyo { code: "nap", title: "Zenless Zone Zero", exes: &["ZenlessZoneZero.exe"] },
    Hoyo { code: "bh3", title: "Honkai Impact 3rd", exes: &["BH3.exe"] },
];

/// The game's program: in its folder, else in a folder just inside it.
fn find_exe(dir: &str, names: &[&str]) -> Option<String> {
    if let Some(f) = names.iter().map(|n| js::join(&[dir, n])).find(|f| is_file(Path::new(f))) {
        return Some(f);
    }
    for e in read_dir_sorted(Path::new(dir)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
        let sub = e.file_name().to_string_lossy().into_owned();
        if let Some(f) = names.iter().map(|n| js::join(&[dir, &sub, n])).find(|f| is_file(Path::new(f))) {
            return Some(f);
        }
    }
    None
}

/// HoYoPlay's games from `HKCU\Software\Cognosphere\HYP`: started by their own program.
pub fn hoyo_from_tree(tree: &[(String, RegKey)]) -> Vec<PcGame> {
    let text = |v: &RegKey, k: &str| js::trim(v.get(&format!("{k}#text")).map_or(val(v, k), String::as_str)).to_string();
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for (_, v) in tree {
        let (biz, dir) = (text(v, "gamebiz"), no_end_slash(&text(v, "gameinstallpath")).to_string());
        if biz.is_empty() || dir.is_empty() {
            continue;
        }
        let Some(def) = HOYO.iter().find(|d| biz.to_lowercase().starts_with(d.code)).filter(|_| is_dir(Path::new(&dir))) else { continue };
        let Some(exe) = find_exe(&dir, def.exes) else { continue };
        let id = format!("hoyo-{}", slug(&biz));
        if !seen.insert(id.clone()) {
            continue;
        }
        let china = if biz.to_lowercase().ends_with("_cn") { " (China)" } else { "" };
        out.push(PcGame { id, source: "hoyoplay", title: format!("{}{china}", def.title), exe, dir, ..PcGame::default() });
    }
    out
}

pub fn hoyo_games() -> Vec<PcGame> {
    hoyo_from_tree(&reg_tree("HKCU\\Software\\Cognosphere\\HYP"))
}

/// Ubisoft Connect's games from `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs`: started through Connect's link.
pub fn ubisoft_from_tree(tree: &[(String, RegKey)]) -> Vec<PcGame> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\\Installs\\([0-9]+)$").unwrap());
    let mut out = Vec::new();
    for (k, v) in tree {
        let dir = no_end_slash(val(v, "installdir")).replace('/', "\\");
        let Some(id) = ID.captures(k).filter(|_| !dir.is_empty() && is_dir(Path::new(&dir))) else { continue };
        out.push(PcGame {
            id: format!("ubisoft-{}", &id[1]),
            source: "ubisoft",
            title: js::basename(&dir),
            uri: format!("uplay://launch/{}/0", &id[1]),
            exe: main_exe(&dir).unwrap_or_default(),
            dir,
            ..PcGame::default()
        });
    }
    out
}

pub fn ubisoft_games() -> Vec<PcGame> {
    ubisoft_from_tree(&reg_tree("HKLM\\SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs"))
}

fn installer_data(dir: &str) -> String {
    js::join(&[dir, "__Installer", "installerdata.xml"])
}

pub fn ea_games() -> Vec<PcGame> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut dirs: Vec<String> = Vec::new();
    let mut add = |d: &str| {
        let r = resolve(Path::new(d)).to_string_lossy().into_owned();
        if is_file(Path::new(&installer_data(d))) && !dirs.contains(&r) {
            dirs.push(r);
        }
    };
    for key in ["HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall", "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"] {
        for (_, v) in reg_tree(key) {
            let d = val(&v, "installlocation");
            let d = d.strip_prefix('"').unwrap_or(d);
            let d = no_end_slash(d.strip_suffix('"').unwrap_or(d));
            if !d.is_empty() {
                add(d);
            }
        }
    }
    for pf in ["ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|k| std::env::var(k).ok()).filter(|p| !p.is_empty()) {
        let ea = js::join(&[&pf, "EA Games"]);
        for e in read_dir_sorted(Path::new(&ea)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            add(&js::join(&[&ea, &e.file_name().to_string_lossy()]));
        }
    }
    ea_from_dirs(&dirs)
}

/// The games in these EA game folders (each with `__Installer\installerdata.xml`): started through the EA app's link.
pub fn ea_from_dirs(dirs: &[String]) -> Vec<PcGame> {
    static CONTENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<contentID>\s*([^<\s]+)\s*</contentID>").unwrap());
    static ENGLISH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<gameTitle\s+locale="en_US"\s*>([^<]+)<"#).unwrap());
    static ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<gameTitle(?:\s[^>]*)?>([^<]*)</gameTitle>").unwrap());
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    for d in dirs {
        let Ok(x) = std::fs::read(installer_data(d)).map(|b| String::from_utf8_lossy(&b).into_owned()) else { continue };
        let ids: Vec<&str> = CONTENT.captures_iter(&x).take(20).map(|m| m.get(1).unwrap().as_str()).collect();
        if ids.is_empty() {
            continue;
        }
        let named = match ENGLISH.captures(&x) {
            Some(m) => unxml(&m[1]),
            None => unxml(ANY.captures(&x).map_or("", |m| js::trim(m.get(1).unwrap().as_str()))),
        };
        let title = if named.is_empty() { js::basename(d) } else { named };
        let id = format!("ea-{}", slug(ids[0]));
        if !seen.insert(id.clone()) {
            continue;
        }
        out.push(PcGame {
            id,
            source: "ea",
            title: js::trim(&title).to_string(),
            uri: format!("origin2://game/launch?offerIds={}&autoDownload=1", js::encode_uri_component(&ids.join(","))),
            exe: main_exe(d).unwrap_or_default(),
            dir: d.clone(),
            ..PcGame::default()
        });
    }
    out
}

/// A game in an Xbox games folder, from its `Content\MicrosoftGame.config`.
#[derive(Debug, Clone, PartialEq)]
pub struct XboxFound {
    /// its package's name
    pub name: String,
    pub publisher: String,
    pub app_id: String,
    pub title: String,
    pub dir: String,
}

/// Each drive's Xbox games folder: the one `<drive>:\.GamingRoot` names ("RGBX", 4 bytes, then the folder in UTF-16),
/// usually `XboxGames`.
pub fn xbox_roots() -> Vec<String> {
    let mut roots = Vec::new();
    for l in 'C'..='Z' {
        let drive = format!("{l}:\\");
        let rel = match std::fs::read(format!("{drive}.GamingRoot")) {
            Ok(b) if b.starts_with(b"RGBX") => {
                let units: Vec<u16> = b.get(8..).unwrap_or(&[]).as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|u| *u != 0).collect();
                Some(String::from_utf16_lossy(&units))
            }
            Ok(_) => None,
            Err(_) if l != 'C' => continue,
            Err(_) => None,
        };
        let dir = match rel.filter(|r| !r.is_empty()) {
            Some(r) if js::is_absolute(&r) => r,
            Some(r) => js::join(&[&drive, &r]),
            None => js::join(&[&drive, "XboxGames"]),
        };
        if is_dir(Path::new(&dir)) {
            roots.push(dir);
        }
    }
    roots
}

/// A MicrosoftGame.config as text: UTF-8, or UTF-16 with its byte-order mark.
fn config_text(f: &Path) -> Option<String> {
    let b = std::fs::read(f).ok()?;
    let utf16 = |b: &[u8], be: bool| {
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| if be { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) }).collect();
        String::from_utf16_lossy(&units)
    };
    Some(match b.as_slice() {
        [0xff, 0xfe, rest @ ..] => utf16(rest, false),
        [0xfe, 0xff, rest @ ..] => utf16(rest, true),
        _ => {
            let t = String::from_utf8_lossy(&b);
            t.strip_prefix('\u{feff}').unwrap_or(&t).to_string()
        }
    })
}

/// The games in these Xbox games folders.
pub fn xbox_scan(roots: &[String]) -> Vec<XboxFound> {
    static IDENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<Identity\s[^>]*>").unwrap());
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?-u:\b)Name="([^"]+)""#).unwrap());
    static PUBLISHER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?-u:\b)Publisher="([^"]+)""#).unwrap());
    static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<Executable\s[^>]*(?-u:\b)Id="([^"]+)""#).unwrap());
    static SHOWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<ShellVisuals\s[^>]*(?-u:\b)DefaultDisplayName="([^"]+)""#).unwrap());
    let mut found = Vec::new();
    for root in roots {
        for e in read_dir_sorted(Path::new(root)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            let folder = e.file_name().to_string_lossy().into_owned();
            let dir = js::join(&[root, &folder]);
            // GameSave and other parts of the Xbox app have none
            let Some(x) = config_text(Path::new(&js::join(&[&dir, "Content", "MicrosoftGame.config"]))) else { continue };
            let tag = IDENTITY.find(&x).map_or("", |m| m.as_str());
            let Some(name) = NAME.captures(tag).map(|m| m[1].to_string()) else { continue };
            let title = Some(unxml(SHOWN.captures(&x).map_or("", |m| m.get(1).unwrap().as_str()))).filter(|t| !t.is_empty() && !t.to_lowercase().starts_with("ms-resource:"));
            found.push(XboxFound {
                name,
                publisher: unxml(PUBLISHER.captures(tag).map_or("", |m| m.get(1).unwrap().as_str())),
                app_id: APP.captures(&x).map_or("Game".into(), |m| m[1].to_string()),
                title: title.unwrap_or(folder),
                dir,
            });
        }
    }
    found
}

/// A package publisher's id, the end of its package family name: the first 8 bytes of the SHA-256 of the publisher
/// (UTF-16), in Windows' own base 32.
pub fn publisher_id(publisher: &str) -> String {
    const ALPHABET: &[u8] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let bytes: Vec<u8> = publisher.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let h = Sha256::digest(&bytes);
    // 64 bits and a zero bit, five at a time
    let mut bits: u128 = 0;
    for b in &h[..8] {
        bits = (bits << 8) | u128::from(*b);
    }
    bits <<= 1;
    (0..13).map(|i| ALPHABET[((bits >> (60 - i * 5)) & 31) as usize] as char).collect()
}

/// A Windows app's address in shell:AppsFolder (`<package family>!<app id>`).
pub fn is_shell_app(a: &str) -> bool {
    static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+_[A-Za-z0-9]+![A-Za-z0-9_.-]+$").unwrap());
    APP.is_match(a)
}

/// A game Windows said isn't installed is asked about again after a day.
const XBOX_RECHECK: u64 = 24 * 3600 * 1000;

/// Older Store games Windows keeps in its protected WindowsApps folder, which no Xbox games folder shows: asked about
/// by name like the others (main/pcgames.js STORE_GAMES, keep in sync).
fn store_games() -> Vec<XboxFound> {
    vec![XboxFound { name: "Microsoft.MinecraftUWP".into(), publisher: String::new(), app_id: "App".into(), title: "Minecraft for Windows".into(), dir: String::new() }]
}

/// Asks Windows for these packages' families: `name|family` lines, then `TS-END`.
fn ask_families(names: &[String]) -> String {
    let list = names.iter().map(|n| format!("'{}'", n.replace('\'', "''"))).collect::<Vec<_>>().join(",");
    // one name per Get-AppxPackage: its -Name takes a single name
    let script = format!("foreach ($n in @({list})) {{ Get-AppxPackage -Name $n | ForEach-Object {{ $_.Name + '|' + $_.PackageFamilyName }} }}; 'TS-END'");
    run("powershell.exe", &["-NoProfile", "-NonInteractive", "-Command", &script], Duration::from_secs(30))
}

/// The Xbox / Game Pass games: started as the Windows apps they are. Each package's family is asked for once and kept
/// in `data/pc-xbox.json` (`{ "<package name>": "<family>" }`, "" for one that isn't installed), a file the Electron
/// app shares.
pub fn xbox_games_with(data_dir: &Path, scanned: Vec<XboxFound>, ask: impl FnOnce(&[String]) -> String, now: u64) -> Vec<PcGame> {
    static END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^TS-END\s*$").unwrap());
    let extra: Vec<XboxFound> = store_games().into_iter().filter(|s| !scanned.iter().any(|f| f.name == s.name)).collect();
    let found: Vec<XboxFound> = scanned.into_iter().chain(extra).collect();
    let file = data_dir.join("pc-xbox.json");
    let mut fam = match read_json(&file) {
        Some(Value::Object(m)) if m.get("__v").and_then(Value::as_f64) == Some(2.0) => m,
        _ => Map::from_iter([("__v".to_string(), Value::from(2))]),
    };
    let asked = fam.get("__askedAt").and_then(Value::as_f64).unwrap_or(0.0);
    let stale = now as f64 - asked > XBOX_RECHECK as f64;
    let mut missing: Vec<String> = Vec::new();
    for f in &found {
        let unknown = match fam.get(&f.name) {
            None => true,
            Some(Value::String(s)) => s.is_empty() && stale,
            Some(_) => false,
        };
        if unknown {
            missing.push(f.name.clone());
        }
    }
    let mut guessed: HashSet<String> = HashSet::new();
    if !missing.is_empty() {
        let out = ask(&missing);
        if END.is_match(&out) {
            for n in &missing {
                fam.insert(n.clone(), Value::from(""));
            }
            for line in out.split('\n') {
                let mut parts = js::trim(line).split('|');
                if let (Some(n), Some(f)) = (parts.next().filter(|n| !n.is_empty()), parts.next().filter(|f| !f.is_empty())) {
                    fam.insert(n.to_string(), Value::from(f));
                }
            }
            fam.insert("__askedAt".into(), Value::from(now));
            let _ = std::fs::create_dir_all(data_dir);
            let _ = std::fs::write(&file, Value::Object(fam.clone()).to_string());
        } else {
            // no answer: this time each game's family is worked out from its publisher, nothing remembered
            guessed = missing.into_iter().collect();
        }
    }
    let family = |f: &XboxFound| {
        if guessed.contains(&f.name) {
            (!f.publisher.is_empty()).then(|| format!("{}_{}", f.name, publisher_id(&f.publisher)))
        } else {
            fam.get(&f.name).filter(|v| js::truthy(Some(v))).map(|v| js::string(Some(v)))
        }
    };
    found
        .iter()
        .filter_map(|f| {
            let fam = family(f)?;
            Some(PcGame { id: format!("xbox-{}", slug(&f.name)), source: "xbox", title: js::trim(&f.title).to_string(), shell_app: format!("{fam}!{}", f.app_id), dir: f.dir.clone(), ..PcGame::default() })
        })
        .collect()
}

pub fn xbox_games(data_dir: &Path) -> Vec<PcGame> {
    if !cfg!(windows) {
        return Vec::new();
    }
    xbox_games_with(data_dir, xbox_scan(&xbox_roots()), ask_families, now_ms())
}

/// Settings → PC games: which stores are on (each on unless switched off).
pub fn wanted(pc: Option<&Value>, store: &str) -> bool {
    pc.and_then(|p| p.get(store)) != Some(&Value::Bool(false))
}

/// GOG, Ubisoft Connect, the EA app, Xbox and HoYoPlay at once, by title.
pub fn store_games_of(pc: Option<&Value>, data_dir: &Path) -> Vec<PcGame> {
    let on = |k: &str| wanted(pc, k);
    let mut out: Vec<PcGame> = std::thread::scope(|s| {
        let jobs = [
            on("gog").then(|| s.spawn(gog_games)),
            on("ubisoft").then(|| s.spawn(ubisoft_games)),
            on("ea").then(|| s.spawn(ea_games)),
            on("xbox").then(|| s.spawn(|| xbox_games(data_dir))),
            on("hoyoplay").then(|| s.spawn(hoyo_games)),
        ];
        jobs.into_iter().flatten().flat_map(|j| j.join().unwrap_or_default()).collect()
    });
    out.sort_by(|a, b| crate::collate::compare_exact(&a.title, &b.title));
    out
}

// ---------- the program in a game's folder ----------

/// The biggest .exe that isn't a helper (crash reporters, installers, redistributables, uninstallers, updaters), two
/// folders deep at most.
pub fn main_exe(dir: &str) -> Option<String> {
    static EXE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.exe$").unwrap());
    static HELPER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)^(unins.*|uninstall.*|setup.*|install.*|vc_?redist.*|dxsetup.*|dotnet.*|crash.*|.*crashhandler.*|.*updater.*|update\.exe|unitycrashhandler.*|ue4prereq.*|easyanticheat.*|.*_launcher_setup.*|7z.*|cmd\.exe|notification_helper.*)$").unwrap()
    });
    // _Redist: installers; bin_original: a modded game's untouched copy
    static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(_?redist|_commonredist|directx|vcredist|support|tools?|mods?|saves?|config|logs?|.*_original|backups?)$").unwrap());
    fn walk(d: &Path, left: u32, best: &mut Option<(PathBuf, u64)>) {
        for e in read_dir_sorted(d) {
            let (Ok(t), n) = (e.file_type(), e.file_name().to_string_lossy().into_owned()) else { continue };
            if t.is_file() && EXE.is_match(&n) && !HELPER.is_match(&n) {
                let Ok(size) = e.path().metadata().map(|m| m.len()) else { continue };
                if best.as_ref().is_none_or(|b| size > b.1) {
                    *best = Some((e.path(), size));
                }
            } else if t.is_dir() && left > 0 && !SKIP.is_match(&n) {
                walk(&e.path(), left - 1, best);
            }
        }
    }
    let mut best = None;
    walk(&js::normalize(dir), 2, &mut best);
    best.map(|b| b.0.to_string_lossy().into_owned())
}

/// A title from a program's name: "SoH-Ship-of-Harkinian.exe" → "SoH Ship of Harkinian".
pub fn title_from_exe(exe: &str) -> String {
    static MARKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[_\-.]+").unwrap());
    let t = MARKS.replace_all(&js::stem(exe), " ").into_owned();
    let t = spaces(&t);
    if t.is_empty() {
        "Game".into()
    } else {
        t
    }
}

fn spaces(s: &str) -> String {
    static RUN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    js::trim(&RUN.replace_all(s, " ")).to_string()
}

// ---------- Quiver Launcher ----------

/// A Quiver app before it gets its id.
#[derive(Debug, Clone, PartialEq)]
struct QuiverApp {
    key: String,
    title: String,
    exe: String,
    icon: String,
    tags: Vec<String>,
    port: (bool, String),
}

/// A console's id from a name Quiver's folders use for it ("N64", "GameCube", "PS1" …).
fn console_of_name(name: &str) -> String {
    let k = |s: &str| s.to_lowercase().chars().filter(char::is_ascii_alphanumeric).collect::<String>();
    let want = k(name);
    if want.is_empty() {
        return String::new();
    }
    systems()
        .consoles
        .iter()
        .find(|c| std::iter::once(c.id.as_str()).chain(std::iter::once(c.name.as_str())).chain(c.rom_folder_names.iter().map(String::as_str)).any(|n| k(n) == want))
        .map_or(String::new(), |c| c.id.clone())
}

/// A Quiver app is a port when Quiver 4 keeps it under Ports\<console>, or its catalog tags say port / recomp / decomp.
fn port_of_quiver(group: &[&str], tags: &[String]) -> (bool, String) {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(port|ports|recomp|recompilation|decomp|decompilation)\b").unwrap());
    if group.first().is_some_and(|g| g.eq_ignore_ascii_case("ports")) {
        return (true, group.get(1).map_or(String::new(), |c| console_of_name(c)));
    }
    (tags.iter().any(|t| TAG.is_match(t)), String::new())
}

fn quiver_std() -> Vec<String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let (a, l) = (env("APPDATA"), env("LOCALAPPDATA"));
    let mut out = Vec::new();
    let mut add = |base: &Option<String>, parts: &[&str]| {
        if let Some(b) = base {
            out.push(js::join(&[&[b.as_str()], parts].concat()));
        }
    };
    add(&a, &["com.quiverlauncher.app"]);
    add(&l, &["com.quiverlauncher.app"]);
    add(&a, &["QuiverLauncher"]);
    add(&l, &["QuiverLauncher"]);
    add(&l, &["QuiverLauncher", "current"]);
    out
}

fn has(root: &str, file: &str) -> bool {
    is_file(Path::new(&js::join(&[root, file])))
}

/// A Quiver folder holds apps.json (Quiver 3) or installs.json (Quiver 4).
fn is_quiver(d: &str) -> bool {
    has(d, "apps.json") || has(d, "installs.json")
}

/// The Quiver folders: the one chosen in Settings (a portable copy can be anywhere), then where it usually is.
pub fn quiver_roots(chosen: &str) -> Vec<String> {
    let mut r = Vec::new();
    if !chosen.is_empty() {
        r.push(chosen.to_string());
        r.push(js::dirname(chosen));
    }
    r.extend(quiver_std());
    let mut out: Vec<String> = Vec::new();
    for p in r.iter().map(|p| resolve(Path::new(p)).to_string_lossy().into_owned()) {
        if !out.contains(&p) && is_quiver(&p) {
            out.push(p);
        }
    }
    out
}

fn json_in(root: &str, file: &str) -> Option<Value> {
    read_json(Path::new(&js::join(&[root, file])))
}

fn apps_path(settings: Option<&Value>, root: &str) -> String {
    match js::str_of(settings.and_then(|s| s.get("AppsPath"))).map(js::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => js::join(&[root, "Apps"]),
    }
}

/// The name the player gave an app in Quiver 3 (`UserAppDisplayNames`), by its repository, folder or name.
fn own_name<'a>(names: Option<&'a Value>, keys: &[Option<&Value>]) -> Option<&'a Value> {
    let names = names.filter(|n| n.is_object())?;
    keys.iter().map(|k| names.get(js::string(*k))).find(|v| js::truthy(*v)).flatten()
}

fn quiver3(root: &str) -> Vec<QuiverApp> {
    let (j, s) = (json_in(root, "apps.json"), json_in(root, "settings.json"));
    let apps_dir = apps_path(s.as_ref(), root);
    let names = s.as_ref().and_then(|s| s.get("UserAppDisplayNames"));
    let mut out = Vec::new();
    for a in j.as_ref().and_then(|j| j.get("apps")).and_then(Value::as_array).into_iter().flatten().take(2000) {
        if !(a.is_object() || a.is_array()) {
            continue;
        }
        let folder = js::str_of(a.get("folderName")).unwrap_or("");
        let dir = match js::filled(a.get("installPath")) {
            Some(p) => p.to_string(),
            None if !folder.is_empty() => js::join(&[&apps_dir, folder]),
            None => continue,
        };
        // listed but not installed
        if !is_dir(Path::new(&dir)) {
            continue;
        }
        let Some(exe) = main_exe(&dir) else { continue };
        let folder_v = Value::from(folder);
        let own = own_name(names, &[a.get("repository"), Some(&folder_v), a.get("name")]);
        let key = js::first(&[a.get("repository")]).map_or(folder.to_string(), |r| js::string(Some(r)));
        let title = match js::first(&[own, a.get("name"), a.get("project")]) {
            Some(t) => js::string(Some(t)),
            None => folder.to_string(),
        };
        let tags = strings(a.get("tags"));
        out.push(QuiverApp { key, title: js::trim(&title).to_string(), exe, icon: https(a.get("appIconUrl")), port: port_of_quiver(&[], &tags), tags });
    }
    out
}

/// Quiver 4's catalog.json: the last catalog it saw, each app by its id.
fn quiver_catalog(root: &str) -> Vec<(String, Value)> {
    let c = json_in(root, "catalog.json");
    let mut out: Map<String, Value> = Map::new();
    for e in js::items(c.as_ref()).into_iter().take(20000) {
        if let (true, Some(id), Some(name)) = (e.is_object(), js::str_of(e.get("id")), js::str_of(e.get("name"))) {
            if !js::trim(name).is_empty() {
                out.insert(id.to_string(), e.clone());
            }
        }
    }
    js::entries(&out).into_iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn cat_icon(e: Option<&Value>) -> String {
    let art = e.and_then(|e| e.get("libraryArt")).filter(|a| a.is_object());
    https(js::first(&[art.and_then(|a| a.get("capsule")), e.and_then(|e| e.get("artwork"))]))
}

fn tags20(v: Option<&Value>) -> Vec<String> {
    strings(v).into_iter().take(20).collect()
}

/// Quiver 4's library items: a list, a list under `items`, or an object of them.
fn library_items(lib: Option<&Value>) -> Vec<&Value> {
    match lib {
        Some(Value::Array(_)) => js::items(lib),
        Some(l) => match l.get("items") {
            Some(i) if i.is_array() => js::items(Some(i)),
            _ => js::items(lib),
        },
        None => Vec::new(),
    }
}

/// `it.custom && it.custom.name`, as JS reads it.
fn sub<'a>(it: Option<&'a Value>, a: &str, b: &str) -> Option<&'a Value> {
    it.and_then(|i| i.get(a)).filter(|v| js::truthy(Some(v))).and_then(|v| v.get(b))
}

fn quiver4(root: &str) -> Vec<QuiverApp> {
    static SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());
    static EXE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.exe$").unwrap());
    let (inst, lib) = (json_in(root, "installs.json"), json_in(root, "library.json"));
    let apps_dir = quiver_apps_dir(root);
    let mut by_id: HashMap<String, &Value> = HashMap::new();
    for x in library_items(lib.as_ref()) {
        if js::truthy(Some(x)) && js::truthy(x.get("id")) {
            by_id.insert(js::string(x.get("id")), x);
        }
    }
    let cat: HashMap<String, Value> = quiver_catalog(root).into_iter().collect();
    let entries: Vec<(String, &Value)> = match &inst {
        Some(Value::Array(a)) => a.iter().map(|x| (js::string(js::first(&[x.get("id")]).or(x.get("app"))), x)).collect(),
        Some(Value::Object(o)) => js::entries(o).into_iter().map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for (id, i) in entries.into_iter().take(2000) {
        if !(i.is_object() || i.is_array()) {
            continue;
        }
        // an app Quiver 4 groups by console sits in <apps folder>/<group>/<folder> (group: "Ports/N64", "Emulators")
        let grp: Vec<&str> = js::filled(i.get("group")).map(|g| SEP.split(g).filter(|x| !x.is_empty() && *x != ".." && *x != ".").collect()).unwrap_or_default();
        let dir = match (js::filled(i.get("dir")), js::filled(i.get("folder"))) {
            (Some(d), _) => d.to_string(),
            (None, Some(f)) => js::join(&[&[apps_dir.as_str()], grp.as_slice(), &[f]].concat()),
            _ => String::new(),
        };
        let inside = |p: &str| if js::is_absolute(p) { p.to_string() } else { js::join(&[&dir, p]) };
        let mut exe = js::filled(i.get("program")).map(inside).unwrap_or_default();
        if exe.is_empty() {
            if let Some(first) = i.get("executables").and_then(Value::as_array).and_then(|a| a.first()) {
                exe = inside(&js::string(Some(first)));
            }
        }
        if exe.is_empty() || !is_file(Path::new(&exe)) {
            exe = if !dir.is_empty() && is_dir(Path::new(&dir)) { main_exe(&dir).unwrap_or_default() } else { String::new() };
        }
        if !EXE.is_match(&exe) {
            continue;
        }
        let (it, ce) = (by_id.get(&id).copied(), cat.get(&id));
        let ov = it.and_then(|i| i.get("overrides")).filter(|o| js::truthy(Some(o)));
        let named = js::first(&[
            ov.and_then(|o| o.get("name")),
            sub(it, "custom", "name"),
            sub(it, "local", "name"),
            ce.and_then(|c| c.get("name")),
            it.and_then(|i| i.get("slug")),
            ce.and_then(|c| c.get("slug")),
            i.get("folder"),
        ]);
        let title = named.map_or_else(|| js::basename(&js::dirname(&exe)), |t| js::string(Some(t)));
        let cover = https(ov.and_then(|o| o.get("cover")));
        let tags = tags20(ce.and_then(|c| c.get("tags")));
        out.push(QuiverApp { key: id, title: js::trim(&title).to_string(), exe, icon: if cover.is_empty() { cat_icon(ce) } else { cover }, port: port_of_quiver(&grp, &tags), tags });
    }
    out
}

/// Quiver's apps folder: Quiver 3's AppsPath (else `<root>\Apps`), Quiver 4's apps-folder.json (else `<root>\apps`).
fn quiver_apps_dir(root: &str) -> String {
    if has(root, "installs.json") {
        let af = json_in(root, "apps-folder.json");
        return match js::filled(af.as_ref().and_then(|a| a.get("path"))) {
            Some(p) => p.to_string(),
            None => js::join(&[root, "apps"]),
        };
    }
    apps_path(json_in(root, "settings.json").as_ref(), root)
}

/// What a name comes to for matching: its letters and digits.
fn q_key(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// The same with a version on the end left off: "Starship-v1.0.2" → "starship".
fn q_bare(s: &str) -> String {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[\s._-]*v?[0-9]+(\.[0-9]+)+[a-z]?$").unwrap());
    q_key(&VERSION.replace(s, ""))
}

#[derive(Debug, Clone)]
struct Named {
    title: String,
    icon: String,
    tags: Vec<String>,
}

/// What Quiver's own lists call an app, by its folder, project, repository and name: for an app whose folder those
/// lists don't point at.
fn quiver_names(root: &str) -> HashMap<String, Named> {
    let mut m: HashMap<String, Named> = HashMap::new();
    let mut put = |k: Option<&Value>, v: &Named| {
        let Some(k) = js::str_of(k) else { return };
        for n in [q_key(k), q_bare(k)] {
            if n.len() >= 3 && !m.contains_key(&n) {
                m.insert(n, v.clone());
            }
        }
    };
    let last = |s: &str, seps: &[char]| Value::from(s.rsplit(seps).next().unwrap_or(""));
    let s = json_in(root, "settings.json");
    let own = s.as_ref().and_then(|s| s.get("UserAppDisplayNames"));
    let j = json_in(root, "apps.json");
    for a in j.as_ref().and_then(|j| j.get("apps")).and_then(Value::as_array).into_iter().flatten().take(5000) {
        if !(a.is_object() || a.is_array()) {
            continue;
        }
        let repo = js::str_of(a.get("repository")).unwrap_or("");
        let repo_v = Value::from(repo);
        let named = js::first(&[own_name(own, &[Some(&repo_v), a.get("folderName"), a.get("name")]), a.get("name"), a.get("project")]);
        let title = js::trim(&js::text(named)).to_string();
        if title.is_empty() {
            continue;
        }
        let v = Named { title, icon: https(a.get("appIconUrl")), tags: strings(a.get("tags")) };
        let repo_end = last(repo, &['/']);
        for k in [a.get("folderName"), a.get("project"), Some(&repo_end), a.get("name")] {
            put(k, &v);
        }
    }
    for (id, e) in quiver_catalog(root) {
        let v = Named { title: js::trim(js::str_of(e.get("name")).unwrap_or("")).to_string(), icon: cat_icon(Some(&e)), tags: tags20(e.get("tags")) };
        let id_end = last(&id, &[':', '/']);
        for k in [e.get("slug"), Some(&id_end), e.get("name")] {
            put(k, &v);
        }
    }
    let lib = json_in(root, "library.json");
    for it in library_items(lib.as_ref()).into_iter().take(5000) {
        if !(it.is_object() || it.is_array()) {
            continue;
        }
        let ov = it.get("overrides").filter(|o| js::truthy(Some(o)));
        let named = js::first(&[ov.and_then(|o| o.get("name")), sub(Some(it), "custom", "name"), sub(Some(it), "local", "name"), it.get("name")]);
        let title = js::trim(&js::text(named)).to_string();
        if title.is_empty() {
            continue;
        }
        let v = Named { title: title.clone(), icon: https(ov.and_then(|o| o.get("cover"))), tags: Vec::new() };
        let repo_end = Value::from(js::str_of(it.get("repository")).map_or("", |r| r.rsplit('/').next().unwrap_or("")));
        let title_v = Value::from(title);
        for k in [it.get("folder"), it.get("slug"), sub(Some(it), "local", "folder"), Some(&repo_end), Some(&title_v)] {
            put(k, &v);
        }
    }
    m
}

/// The app folders in Quiver's apps folder. Quiver 4 groups new apps by kind and console (Emulators, Tools, Games,
/// Ports\<console>): those folders hold apps, they aren't apps.
fn quiver_app_dirs(apps_dir: &str, grouped: bool) -> Vec<String> {
    static NOT_APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\.|_)|^(temp|tmp|cache|downloads?|logs?|backups?|updates?)$").unwrap());
    static GROUP: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^(emulators|tools|games)$").unwrap());
    static PORTS: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^ports$").unwrap());
    let kids = |d: &str| -> Vec<String> {
        read_dir_sorted(Path::new(d))
            .into_iter()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !NOT_APP.is_match(n))
            .map(|n| js::join(&[d, &n]))
            .collect()
    };
    let mut out = Vec::new();
    for d in kids(apps_dir).into_iter().take(2000) {
        let n = js::basename(&d);
        if grouped && PORTS.is_match(&n) {
            for c in kids(&d).into_iter().take(200) {
                out.extend(kids(&c).into_iter().take(2000));
            }
        } else if grouped && GROUP.is_match(&n) {
            out.extend(kids(&d).into_iter().take(2000));
        } else {
            out.push(d);
        }
    }
    out.truncate(5000);
    out
}

/// The apps in Quiver's apps folder that its list doesn't show: every folder there with a program in it, less the
/// ones the list already gave.
fn quiver_loose(apps_dir: &str, listed: &[String], names: &HashMap<String, Named>, grouped: bool) -> Vec<QuiverApp> {
    static MARKS: LazyLock<Regex> = LazyLock::new(|| Regex::new("[_.]+").unwrap());
    let inside: Vec<String> = listed.iter().map(|e| resolve(Path::new(e)).to_string_lossy().to_lowercase()).collect();
    let mut out = Vec::new();
    for d in quiver_app_dirs(apps_dir, grouped) {
        let name = js::basename(&d);
        let dir = resolve(Path::new(&d)).to_string_lossy().into_owned();
        let pre = format!("{}{}", dir.to_lowercase(), std::path::MAIN_SEPARATOR);
        if inside.iter().any(|x| x.starts_with(&pre)) {
            continue;
        }
        let Some(exe) = main_exe(&dir) else { continue };
        let hit = names.get(&q_key(&name)).or_else(|| names.get(&q_bare(&name)));
        let title = match hit {
            Some(h) => h.title.clone(),
            None => Some(spaces(&MARKS.replace_all(&name, " "))).filter(|t| !t.is_empty()).unwrap_or_else(|| title_from_exe(&exe)),
        };
        // the id stays the folder's: a game already on the shelf keeps its settings
        let tags = hit.map_or(Vec::new(), |h| h.tags.clone());
        let rel = Path::new(&dir).parent().and_then(|p| p.strip_prefix(resolve(Path::new(apps_dir))).ok()).map(|p| p.iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>()).unwrap_or_default();
        let grp: Vec<&str> = rel.iter().map(String::as_str).collect();
        out.push(QuiverApp { key: name, title, exe, icon: hit.map_or(String::new(), |h| h.icon.clone()), port: port_of_quiver(&grp, &tags), tags });
    }
    out
}

/// The apps Quiver Launcher installed (recomps, ports): Quiver 3, portable or installed, and Quiver 4.
pub fn quiver_games(chosen: &str) -> Vec<PcGame> {
    quiver_games_in(quiver_roots(chosen))
}

/// The apps of these Quiver folders.
pub fn quiver_games_in(mut roots: Vec<String>) -> Vec<PcGame> {
    // moved from Quiver 3 to Quiver 4, both folders are read (Settings on the old one, Quiver 4 in AppData using the same
    // Apps folder): every root's own list first, Quiver 4's ahead of Quiver 3's, then the apps folders' loose apps with
    // the names from every list, so an app only Quiver 4 knows keeps Quiver 4's name
    roots.sort_by_key(|r| !has(r, "installs.json"));
    let (mut out, mut seen) = (Vec::new(), HashSet::new());
    let mut names: HashMap<String, Named> = HashMap::new();
    let mut lists = Vec::new();
    for root in &roots {
        lists.push(if has(root, "installs.json") { quiver4(root) } else { quiver3(root) });
        for (k, v) in quiver_names(root) {
            names.entry(k).or_insert(v);
        }
    }
    let mut add = |g: QuiverApp, out: &mut Vec<PcGame>| {
        let id = format!("quiver-{}", slug(if g.key.is_empty() { &g.title } else { &g.key }));
        let exe = g.exe.to_lowercase();
        if seen.contains(&id) || seen.contains(&exe) {
            return;
        }
        seen.insert(id.clone());
        seen.insert(exe);
        out.push(PcGame { id, source: "quiver", title: g.title, exe: g.exe, icon: g.icon, tags: g.tags, port: g.port.0, port_of: g.port.1, ..PcGame::default() });
    };
    for list in lists {
        for g in list {
            add(g, &mut out);
        }
    }
    for root in &roots {
        let listed: Vec<String> = out.iter().map(|g| g.exe.clone()).collect();
        for g in quiver_loose(&quiver_apps_dir(root), &listed, &names, has(root, "installs.json")) {
            add(g, &mut out);
        }
    }
    out
}

// ---------- games added by hand (pc.custom in the settings: [{ id, title, exe, args, shelf, portOf }]) ----------

const CUSTOM_MAX: usize = 2000;

fn custom_app(c: &Value) -> bool {
    js::str_of(c.get("shellApp")).is_some_and(is_shell_app)
}

/// The programs added by hand whose program is still there, and the Microsoft Store apps added by hand.
pub fn custom_games(list: Option<&Value>) -> Vec<PcGame> {
    let l: Vec<&Value> = list
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| js::truthy(Some(c)) && (js::filled(c.get("exe")).is_some() || custom_app(c)))
        .take(CUSTOM_MAX * 2)
        .collect();
    // 32 at a time: a thousand checks on a network drive that dropped would hold the shelf up
    let ok: Vec<bool> = l
        .chunks(32)
        .flat_map(|part| {
            std::thread::scope(|s| {
                let jobs: Vec<_> = part
                    .iter()
                    .map(|c| {
                        s.spawn(move || match js::filled(c.get("exe")) {
                            Some(e) => is_file(Path::new(e)),
                            None => custom_app(c),
                        })
                    })
                    .collect();
                jobs.into_iter().map(|j| j.join().unwrap_or(false)).collect::<Vec<_>>()
            })
        })
        .collect();
    let there: Vec<&Value> = l.into_iter().zip(ok).filter(|(_, ok)| *ok).map(|(c, _)| c).take(CUSTOM_MAX).collect();
    custom_from(&there)
}

pub fn custom_from(list: &[&Value]) -> Vec<PcGame> {
    list.iter()
        .map(|c| {
            let exe = js::filled(c.get("exe")).unwrap_or("");
            let app = if exe.is_empty() && custom_app(c) { js::str_of(c.get("shellApp")).unwrap_or("") } else { "" };
            let app_text = js::text(c.get("shellApp"));
            let named = js::first(&[c.get("id"), c.get("title")]).map_or_else(|| js::basename(if exe.is_empty() { &app_text } else { exe }), |v| js::string(Some(v)));
            let title = match js::first(&[c.get("title")]) {
                Some(t) => js::string(Some(t)),
                None => Some(js::basename(exe)).map(|n| n.strip_suffix(".exe").filter(|s| !s.is_empty()).map(String::from).unwrap_or(n)).filter(|n| !n.is_empty()).unwrap_or_else(|| "Game".into()),
            };
            let shelf = js::str_of(c.get("shelf")).filter(|s| *s == "pc" || *s == "ports" || systems().console(s).is_some());
            let port_of = js::str_of(c.get("portOf")).filter(|s| systems().console(s).is_some()).unwrap_or("");
            PcGame {
                id: format!("custom-{}", slug(&named)),
                source: "custom",
                title: js::slice(js::trim(&title), 120).to_string(),
                exe: exe.to_string(),
                shell_app: app.to_string(),
                args: js::slice(js::str_of(c.get("args")).unwrap_or(""), 1000).to_string(),
                shelf: shelf.map(String::from),
                port: shelf == Some("ports"),
                port_of: port_of.to_string(),
                ..PcGame::default()
            }
        })
        .collect()
}

/// The Steam app a store game's title was matched to, for its art: `data/pc-steam.json`
/// (`{ "<lower title>": { id: 1234 | 0, at } }`), filled in by `match_steam`.
pub fn steam_ids(data_dir: &Path) -> HashMap<String, String> {
    let Some(Value::Object(m)) = read_json(&data_dir.join("pc-steam.json")) else { return HashMap::new() };
    m.iter().filter(|(_, e)| js::truthy(e.get("id"))).map(|(k, e)| (k.clone(), js::string(e.get("id")))).collect()
}

/// A title as it's compared with a Steam app's name: marks off, "&" as "and", letters and digits only.
fn norm(s: &str) -> String {
    static OTHER: LazyLock<Regex> = LazyLock::new(|| Regex::new("[^a-z0-9]+").unwrap());
    let l = s.to_lowercase().replace(['™', '®', '©'], "").replace('&', "and");
    js::trim(&OTHER.replace_all(&l, " ")).to_string()
}

/// The Steam app with this name among the store's answers: the very same name, else one that goes on after it or
/// stops before its end.
fn steam_hit(items: &[Value], title: &str) -> Option<f64> {
    let want = norm(title);
    let name = |x: &Value| norm(&js::text(x.get("name")));
    let hit = items.iter().find(|x| name(x) == want).or_else(|| items.iter().find(|x| name(x).starts_with(&format!("{want} ")) || want.starts_with(&format!("{} ", name(x)))))?;
    Some(js::number(hit.get("id")))
}

/// Asks Steam's store for the titles not matched yet (at most 40 a run, a pause between), and keeps the answers in
/// `data/pc-steam.json`; a miss is tried again after a week. Returns how many were matched.
pub fn match_steam(data_dir: &Path, titles: &[String]) -> u32 {
    const WEEK: f64 = 7.0 * 86_400_000.0;
    let file = data_dir.join("pc-steam.json");
    let mut map = match read_json(&file) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let (mut matched, mut asked, mut seen) = (0, 0, HashSet::new());
    for t in titles.iter().filter(|t| !t.is_empty() && seen.insert(t.as_str())) {
        let k = t.to_lowercase();
        if map.get(&k).is_some_and(|e| js::truthy(e.get("id")) || (now_ms() as f64 - js::number(e.get("at"))) < WEEK) {
            continue;
        }
        asked += 1;
        if asked > 40 {
            break;
        }
        let url = format!("https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US", js::encode_uri_component(t));
        // offline or rate-limited: tried again next time
        let Ok(r) = crate::net::get_json(&url, Duration::from_secs(12)) else { break };
        let hit = steam_hit(r.get("items").and_then(Value::as_array).map_or(&[][..], Vec::as_slice), t);
        map.insert(k, serde_json::json!({ "id": js::num(hit.unwrap_or(0.0)), "at": now_ms() }));
        matched += u32::from(hit.is_some());
        std::thread::sleep(Duration::from_millis(400));
    }
    if asked > 0 {
        let tmp = data_dir.join("pc-steam.json.tmp");
        if std::fs::create_dir_all(data_dir).and_then(|()| std::fs::write(&tmp, Value::Object(map).to_string())).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(slug("  Zeta: Run! "), "zeta-run");
        assert_eq!(slug("!!!"), "game");
        assert_eq!(title_from_exe("C:/x/Zeta-Run_of.Games.exe"), "Zeta Run of Games");
        assert_eq!((q_key("Star-Ship 2"), q_bare("Starship-v1.0.2"), q_bare("Zeta 64")), ("starship2".into(), "starship".into(), "zeta64".into()));
        assert!(is_shell_app("Made.Up_8abcdefgh12345!App") && !is_shell_app("C:\\x.exe"));
        let items = serde_json::json!([{ "id": 5, "name": "Zeta Run 2" }, { "id": 7, "name": "Zeta® Run & Gun™" }, { "id": 9, "name": "Zeta" }]);
        let hit = |t: &str| steam_hit(items.as_array().unwrap(), t);
        assert_eq!((hit("zeta run and gun"), hit("Zeta Run"), hit("Zeta: Origins"), hit("Omega")), (Some(7.0), Some(5.0), Some(9.0), None));
        assert_eq!(split_cmd("\"C:\\a b\\x.exe\" -y 1"), ("C:\\a b\\x.exe".into(), "-y 1".into()));
    }

    #[test]
    fn registry() {
        let t = parse_reg("\r\nHKEY_X\\Games\\12\r\n    gameName    REG_SZ    Zeta\r\n    Empty    REG_SZ\r\n    Raw    REG_BINARY    5A6500\r\n\r\nHKEY_X\\Games\r\n");
        assert_eq!(t.len(), 2);
        let v = &t[0].1;
        assert_eq!((val(v, "gamename"), val(v, "empty"), val(v, "raw"), val(v, "raw#text")), ("Zeta", "", "5A6500", "Ze"));
    }
}

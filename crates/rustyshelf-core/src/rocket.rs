// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/rocket.js (beta.12), as RustyShelf has it.
//! (Windows) RocketLauncher, the launcher HyperSpin uses: found on this PC, its systems and their ROM folders read from
//! its own settings, the command line built for one game, and the first error of a run read back from its log.
//! Keep in sync with main/rocket.js.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::config::Config;

/// HyperSpin's usual system names for the consoles (then matched against RocketLauncher's own folder names, which
/// win: a setup can name a system its own way). Several names: the first is the usual one.
pub const NAMES: &[(&str, &[&str])] = &[
    ("nes", &["Nintendo Entertainment System"]),
    ("snes", &["Super Nintendo Entertainment System"]),
    ("n64", &["Nintendo 64"]),
    ("gb", &["Nintendo Game Boy"]),
    ("gbc", &["Nintendo Game Boy Color"]),
    ("gba", &["Nintendo Game Boy Advance"]),
    ("nds", &["Nintendo DS"]),
    ("n3ds", &["Nintendo 3DS"]),
    ("gc", &["Nintendo GameCube"]),
    ("wii", &["Nintendo Wii"]),
    ("wiiu", &["Nintendo Wii U"]),
    ("sw", &["Nintendo Switch"]),
    ("psx", &["Sony PlayStation"]),
    ("ps2", &["Sony PlayStation 2"]),
    ("ps3", &["Sony PlayStation 3"]),
    ("psp", &["Sony PSP"]),
    ("megadrive", &["Sega Genesis", "Sega Mega Drive"]),
    ("mastersystem", &["Sega Master System", "Sega Mark III"]),
    ("gamegear", &["Sega Game Gear"]),
    ("segacd", &["Sega CD", "Sega Mega-CD"]),
    ("sega32x", &["Sega 32X"]),
    ("saturn", &["Sega Saturn"]),
    ("dreamcast", &["Sega Dreamcast"]),
    ("pcengine", &["NEC TurboGrafx-16", "NEC PC Engine"]),
    ("neogeo", &["SNK Neo Geo AES", "SNK Neo Geo"]),
    ("arcade", &["MAME"]),
    ("atari2600", &["Atari 2600"]),
    ("atarilynx", &["Atari Lynx"]),
    ("wonderswan", &["Bandai WonderSwan", "Bandai WonderSwan Color"]),
    ("x360", &["Microsoft Xbox 360"]),
];

fn names_of(console: &str) -> &'static [&'static str] {
    NAMES.iter().find(|(id, _)| *id == console).map_or(&[], |(_, n)| n)
}

fn key(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_lowercase()
}

/// An .ini as section → key → value (keys as written; values trimmed). RocketLauncher's files can be UTF-16.
pub fn read_ini(file: &Path) -> Option<HashMap<String, HashMap<String, String>>> {
    let b = std::fs::read(file).ok()?;
    let t = if b.starts_with(&[0xff, 0xfe]) {
        String::from_utf16_lossy(&b[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
    } else {
        String::from_utf8_lossy(&b).trim_start_matches('\u{feff}').to_string()
    };
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut sec = String::new();
    for raw in t.lines() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with(';') || l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') && l.len() > 2 {
            sec = l[1..l.len() - 1].trim().to_string();
            out.entry(sec.clone()).or_default();
            continue;
        }
        if let Some(i) = l.find('=').filter(|&i| i > 0) {
            out.entry(sec.clone()).or_default().insert(l[..i].trim().to_string(), l[i + 1..].trim().to_string());
        }
    }
    Some(out)
}

fn val<'a>(ini: Option<&'a HashMap<String, HashMap<String, String>>>, section: &str, name: &str) -> &'a str {
    let Some(sec) = ini.and_then(|i| i.iter().find(|(k, _)| k.eq_ignore_ascii_case(section)).map(|(_, v)| v)) else { return "" };
    sec.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or("", |(_, v)| v.as_str())
}

/// RocketLauncher.exe on this PC: the usual HyperSpin places on each drive, and HyperSpin's own setting for it.
pub fn find() -> Option<PathBuf> {
    let mut tries = Vec::new();
    for c in b'C'..=b'Z' {
        let d = PathBuf::from(format!("{}:\\", c as char));
        if !d.exists() {
            continue;
        }
        // HyperSpin's Settings\Settings.ini → [Main] Hyperlaunch_Path (RocketLauncher's folder, often relative to HyperSpin's)
        let hs = d.join("HyperSpin");
        let ini = read_ini(&hs.join("Settings").join("Settings.ini"));
        let p = val(ini.as_ref(), "Main", "Hyperlaunch_Path");
        if !p.is_empty() {
            tries.insert(0, hs.join(p.replace(['\\', '/'], std::path::MAIN_SEPARATOR_STR)).join("RocketLauncher.exe"));
        }
        for sub in [&["HyperSpin", "RocketLauncher"][..], &["RocketLauncher"], &["Games", "HyperSpin", "RocketLauncher"], &["Emulation", "RocketLauncher"]] {
            tries.push(sub.iter().fold(d.clone(), |p, s| p.join(s)).join("RocketLauncher.exe"));
        }
    }
    tries.into_iter().find(|f| f.is_file())
}

/// One of RocketLauncher's systems: its folder name under Settings\, its ROM folders and default emulator (none: not
/// set up in RocketLauncherUI yet).
#[derive(Debug, Clone, PartialEq)]
pub struct System {
    pub name: String,
    pub rom_paths: Vec<PathBuf>,
    pub ready: bool,
}

/// `path` with `..` and `.` taken out, relative to `base` when it isn't absolute (as Node's path.resolve).
fn resolve(base: &Path, p: &str) -> PathBuf {
    let joined = base.join(p.replace(['\\', '/'], std::path::MAIN_SEPARATOR_STR));
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

static CACHE: Mutex<Option<(PathBuf, Instant, Vec<System>)>> = Mutex::new(None);

/// Its systems: one folder each under Settings\ (Global left out), sorted by name.
pub fn systems(exe: &Path, fresh: bool) -> Vec<System> {
    if !fresh {
        if let Some((e, at, list)) = CACHE.lock().unwrap().as_ref() {
            if e == exe && at.elapsed() < Duration::from_secs(60) {
                return list.clone();
            }
        }
    }
    let dir = exe.parent().unwrap_or(Path::new(""));
    let mut list: Vec<System> = crate::scanner::read_dir_sorted(&dir.join("Settings"))
        .into_iter()
        .map(|e| e.path())
        .filter(|e| e.is_dir() && !e.file_name().is_some_and(|n| n.eq_ignore_ascii_case("global")))
        .map(|e| {
            let ini = read_ini(&e.join("Emulators.ini"));
            let rom_paths = val(ini.as_ref(), "ROMS", "Rom_Path").split('|').map(str::trim).filter(|s| !s.is_empty()).map(|p| resolve(dir, p)).collect();
            System { name: e.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), rom_paths, ready: !val(ini.as_ref(), "ROMS", "Default_Emulator").is_empty() }
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    *CACHE.lock().unwrap() = Some((exe.to_path_buf(), Instant::now(), list.clone()));
    list
}

/// The system name for a console: RocketLauncher's own folder that matches one of the usual names, else the usual name.
pub fn name_for(console: &str, list: &[System]) -> String {
    let want: Vec<String> = names_of(console).iter().map(|n| key(n)).collect();
    list.iter().find(|s| want.contains(&key(&s.name))).map_or_else(|| names_of(console).first().copied().unwrap_or("").to_string(), |s| s.name.clone())
}

/// The command line for one game, and what's known of its system.
#[derive(Debug, PartialEq)]
pub struct Args {
    pub args: Vec<String>,
    pub in_roms: bool,
    /// whether the system has an emulator set; None when RocketLauncher has no such system
    pub ready: Option<bool>,
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// -s system, -r the game: its name without the extension when it sits in one of the system's ROM folders (what
/// RocketLauncher's 7z extraction and MultiGame need), else its full path; -p the front end's plugin when one is set.
/// Never -f: RocketLauncher would hide, move or restore that window (RustyShelf's).
pub fn args_for(system: &str, file: &Path, list: &[System], plugin: &str) -> Args {
    let sys = list.iter().find(|s| key(&s.name) == key(system));
    let dir = file.parent().unwrap_or(Path::new(""));
    let in_roms = sys.is_some_and(|s| s.rom_paths.iter().any(|p| same_dir(p, dir)));
    let r = if in_roms { file.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { file.to_string_lossy().into_owned() };
    let mut args = vec!["-s".to_string(), system.to_string(), "-r".to_string(), r];
    if !plugin.is_empty() {
        args.extend(["-p".to_string(), plugin.to_string()]);
    }
    Args { args, in_roms, ready: sys.map(|s| s.ready) }
}

/// The first error RocketLauncher wrote during the run started at `since` (it always exits "normally", so its log is
/// the only way to know).
pub fn last_error(exe: &Path, since: SystemTime) -> String {
    let f = exe.parent().unwrap_or(Path::new("")).join("RocketLauncher.log");
    let Ok(meta) = f.metadata() else { return String::new() };
    if meta.modified().is_ok_and(|m| m + Duration::from_secs(2) < since) {
        return String::new();
    }
    let Ok(b) = std::fs::read(&f) else { return String::new() };
    let tail = &b[b.len().saturating_sub(128 * 1024)..];
    let t = String::from_utf8_lossy(tail);
    let t = t.rfind("Main - Raw CLI received").map_or(&t[..], |i| &t[i..]);
    let error = |l: &&str| {
        let parts: Vec<&str> = l.split('|').collect();
        parts.len() > 2 && parts[1..parts.len() - 1].iter().any(|p| p.trim().eq_ignore_ascii_case("error"))
    };
    t.lines().find(error).map(|l| l.split('|').skip(3).collect::<Vec<_>>().join("|").trim().chars().take(200).collect()).unwrap_or_default()
}

/// RocketLauncher's settings: its program, the mode (off, all, some) and the front end it's told.
pub fn settings(cfg: &Config) -> (String, String, String) {
    let r = cfg.raw().get("rocket");
    let s = |k: &str| r.and_then(|r| r.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
    (s("exe"), s("mode"), s("frontend"))
}

/// A console's own choice (systems.<id>.viaRocket): Some(true) or Some(false), None when it's never chosen.
pub fn console_choice(cfg: &Config, console: &str) -> Option<bool> {
    cfg.raw().get("systems").and_then(|s| s.get(console)).and_then(|s| s.get("viaRocket")).and_then(Value::as_bool)
}

/// Is this console started through RocketLauncher? (Windows, found, and the mode or the console's own choice)
pub fn use_for(console: &str, cfg: &Config) -> bool {
    let (exe, mode, _) = settings(cfg);
    if !cfg!(windows) || exe.is_empty() || mode.is_empty() || mode == "off" {
        return false;
    }
    match console_choice(cfg, console) {
        Some(false) => false,
        Some(true) => true,
        None => mode == "all",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_names_and_command_lines() {
        let tmp = std::env::temp_dir().join(format!("rs-rocket-{}", std::process::id()));
        let rl = tmp.join("HyperSpin").join("RocketLauncher");
        let (roms, other) = (tmp.join("HyperSpin").join("Roms").join("Nintendo GameCube"), tmp.join("Elsewhere"));
        for d in [&roms, &other, &rl.join("Settings").join("Global")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let exe = rl.join("RocketLauncher.exe");
        std::fs::write(&exe, "stand-in").unwrap();
        let sys = |name: &str, text: String, utf16: bool| {
            let d = rl.join("Settings").join(name);
            std::fs::create_dir_all(&d).unwrap();
            let bytes = if utf16 { [0xff, 0xfe].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect() } else { text.into_bytes() };
            std::fs::write(d.join("Emulators.ini"), bytes).unwrap();
        };
        sys("Nintendo GameCube", format!("[ROMS]\r\nRom_Path=..\\Roms\\Nintendo GameCube|{}\r\nDefault_Emulator=Dolphin\r\n", other.display()), false);
        sys("Nintendo Entertainment System", "[ROMS]\r\nRom_Path=..\\Roms\\NES\r\nDefault_Emulator=\r\n".into(), false);
        sys("Sega Mega Drive", "[ROMS]\r\nRom_Path=..\\Roms\\Mega Drive\r\nDefault_Emulator=RetroArch\r\n".into(), true);
        let list = systems(&exe, true);
        assert_eq!(list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Nintendo Entertainment System", "Nintendo GameCube", "Sega Mega Drive"]);
        let gc = &list[1];
        assert_eq!(gc.rom_paths, vec![roms.clone(), other.clone()]);
        assert_eq!((list[0].ready, list[2].ready), (false, true));
        assert_eq!((name_for("gc", &list).as_str(), name_for("megadrive", &list).as_str(), name_for("ps2", &list).as_str()), ("Nintendo GameCube", "Sega Mega Drive", "Sony PlayStation 2"));
        let inside = args_for("Nintendo GameCube", &roms.join("Beta Kart (Europe).iso"), &list, "HyperSpin");
        assert_eq!(inside, Args { args: ["-s", "Nintendo GameCube", "-r", "Beta Kart (Europe)", "-p", "HyperSpin"].map(String::from).to_vec(), in_roms: true, ready: Some(true) });
        let away = tmp.join("x").join("Game.iso");
        let outside = args_for("Nintendo GameCube", &away, &list, "");
        assert_eq!((outside.args[3].as_str(), outside.args.len(), outside.in_roms), (away.to_str().unwrap(), 4, false));
        assert_eq!(args_for("Nintendo Entertainment System", &away, &list, "").ready, Some(false));
        assert_eq!(args_for("Nothing", &away, &list, "").ready, None);
        let log = rl.join("RocketLauncher.log");
        std::fs::write(&log, "old | ERROR | x | an old run's error\nMain - Raw CLI received\n12:00 | INFO | a | fine\n12:01 | ERROR | b | Module not found\n").unwrap();
        assert_eq!(last_error(&exe, SystemTime::now()), "Module not found");
        std::fs::write(&log, "Main - Raw CLI received\n12:00 | INFO | a | fine\n").unwrap();
        assert_eq!(last_error(&exe, SystemTime::now()), "");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/raemu.js, as RustyShelf has it (with DuckStation, RetroArch and Flycast, for any console).
//! Per console: will RetroAchievements work in its emulator? Reads each emulator's own settings (its portable
//! folder first, then the usual places). Never returns a token.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::emucfg::{self, Var};
use crate::js;

/// How a console's emulator stands with RetroAchievements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub system: String,
    pub emulator: &'static str,
    /// "ok", "off", "signed-out", "other-user", "unknown", "no-emulator" or "unsupported"
    pub state: &'static str,
    /// who the emulator is signed in as
    pub user: String,
    pub detail: String,
}

type Found = (&'static str, String, String);

/// An INI file's sections and keys, all in lower case; what comes before any section is under "".
pub fn parse_ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    out.insert(String::new(), HashMap::new());
    for raw in text.split('\n') {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')).filter(|n| !n.is_empty()) {
            section = name.trim().to_lowercase();
            out.entry(section.clone()).or_default();
            continue;
        }
        if let Some(i) = line.find('=').filter(|i| *i >= 1) {
            out.entry(section.clone()).or_default().insert(line[..i].trim().to_lowercase(), line[i + 1..].trim().to_string());
        }
    }
    out
}

/// retroarch.cfg's `key = "value"` lines, the keys in lower case.
pub fn parse_cfg(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim_start();
        let value = value.strip_prefix('"').unwrap_or(value);
        // nothing after a closing quote but spaces, and no other quote
        let value = match value.find('"') {
            Some(i) if value[i + 1..].trim().is_empty() => &value[..i],
            Some(_) => continue,
            None => value,
        };
        out.insert(key.to_lowercase(), value.to_string());
    }
    out
}

fn truthy(v: Option<&String>) -> bool {
    matches!(v.map(|s| s.trim().to_lowercase()).as_deref(), Some("true" | "1" | "yes" | "on"))
}

fn exists(p: &str) -> bool {
    Path::new(p).exists()
}

fn read(p: &str) -> String {
    crate::files::read_utf8(Path::new(p)).unwrap_or_default()
}

fn first_existing(list: impl IntoIterator<Item = String>) -> Option<String> {
    list.into_iter().find(|p| !p.is_empty() && exists(p))
}

fn section(file: &str, name: &str) -> HashMap<String, String> {
    parse_ini(&read(file)).remove(name).unwrap_or_default()
}

fn value(a: &HashMap<String, String>, key: &str) -> String {
    a.get(key).cloned().unwrap_or_default()
}

fn hardcore(on: bool) -> String {
    if on { "Hardcore mode on".into() } else { String::new() }
}

fn home_of(var: Var) -> String {
    emucfg::get(var, "USERPROFILE").or_else(|| emucfg::get(var, "HOME")).unwrap_or_default()
}

fn local_of(var: Var) -> String {
    emucfg::get(var, "LOCALAPPDATA").unwrap_or_else(|| js::join(&[&home_of(var), "AppData", "Local"]))
}

fn roaming_of(var: Var) -> String {
    emucfg::get(var, "APPDATA").unwrap_or_else(|| js::join(&[&home_of(var), "AppData", "Roaming"]))
}

fn check_dolphin(exe: &str, var: Var) -> Found {
    let Some(f) = first_existing(emucfg::dolphin_user_dirs(exe, var).iter().map(|r| js::join(&[r, "Config", "RetroAchievements.ini"]))) else {
        return ("unknown", String::new(), "Dolphin settings not found (run Dolphin once)".into());
    };
    let a = section(&f, "achievements");
    if !truthy(a.get("enabled")) {
        return ("off", value(&a, "username"), "Achievements are turned off in Dolphin".into());
    }
    if value(&a, "apitoken").is_empty() {
        return ("signed-out", value(&a, "username"), "Not signed in to RetroAchievements in Dolphin".into());
    }
    ("ok", value(&a, "username"), hardcore(truthy(a.get("hardcoreenabled"))))
}

fn check_ppsspp(exe: &str, var: Var) -> Found {
    let dir = js::dirname(exe);
    let mut places = vec![js::join(&[&dir, "memstick"])];
    let installed = read(&js::join(&[&dir, "installed.txt"]));
    if !installed.trim().is_empty() {
        places.push(installed.split('\n').next().unwrap_or("").trim().to_string());
    }
    places.extend(emucfg::docs_dirs(var).iter().map(|d| js::join(&[d, "PPSSPP"])));
    let Some(sys) = places.iter().map(|m| js::join(&[m, "PSP", "SYSTEM"])).find(|sys| exists(&js::join(&[sys, "ppsspp.ini"]))) else {
        return ("unknown", String::new(), "PPSSPP settings not found (run PPSSPP once)".into());
    };
    let a = section(&js::join(&[&sys, "ppsspp.ini"]), "achievements");
    let user = value(&a, "achievementsusername");
    if !truthy(a.get("achievementsenable")) {
        return ("off", user, "Achievements are turned off in PPSSPP".into());
    }
    if !exists(&js::join(&[&sys, "ppsspp_retroachievements.dat"])) {
        return ("signed-out", user, "Not signed in to RetroAchievements in PPSSPP".into());
    }
    ("ok", user, hardcore(truthy(a.get("achievementschallengemode"))))
}

/// An emulator whose settings are one INI section: `[name] enabled / username / token`, and a key for hardcore.
fn check_ini(file: Option<String>, emulator: &str, name: &str, hard: &str) -> Found {
    let Some(f) = file else { return ("unknown", String::new(), format!("{emulator} settings not found (run {emulator} once)")) };
    let a = section(&f, name);
    let user = value(&a, "username");
    if !truthy(a.get("enabled")) {
        return ("off", user, format!("Achievements are turned off in {emulator}"));
    }
    // PCSX2 2.x keeps the token in secrets.ini beside its settings
    if value(&a, "token").is_empty() && value(&section(&js::join(&[&js::dirname(&f), "secrets.ini"]), name), "token").is_empty() {
        return ("signed-out", user, format!("Not signed in to RetroAchievements in {emulator}"));
    }
    ("ok", user, hardcore(truthy(a.get(hard))))
}

fn check_duck(exe: &str, var: Var) -> Found {
    let (dir, h) = (js::dirname(exe), home_of(var));
    let mut places = Vec::new();
    if exists(&js::join(&[&dir, "portable.txt"])) {
        places.push(js::join(&[&dir, "settings.ini"]));
    }
    places.push(js::join(&[&local_of(var), "DuckStation", "settings.ini"]));
    places.extend(emucfg::docs_dirs(var).iter().map(|d| js::join(&[d, "DuckStation", "settings.ini"])));
    places.push(js::join(&[&h, ".local", "share", "duckstation", "settings.ini"]));
    places.push(js::join(&[&h, ".var", "app", "org.duckstation.DuckStation", "data", "duckstation", "settings.ini"]));
    check_ini(first_existing(places), "DuckStation", "cheevos", "challengemode")
}

fn check_retroarch(exe: &str, var: Var) -> Found {
    let h = home_of(var);
    let places = [
        js::join(&[&js::dirname(exe), "retroarch.cfg"]),
        js::join(&[&roaming_of(var), "RetroArch", "retroarch.cfg"]),
        js::join(&[&h, ".config", "retroarch", "retroarch.cfg"]),
        js::join(&[&h, ".var", "app", "org.libretro.RetroArch", "config", "retroarch", "retroarch.cfg"]),
    ];
    let Some(f) = first_existing(places) else { return ("unknown", String::new(), "RetroArch settings not found (run RetroArch once)".into()) };
    let c = parse_cfg(&read(&f));
    let user = value(&c, "cheevos_username");
    if !truthy(c.get("cheevos_enable")) {
        return ("off", user, "Achievements are turned off in RetroArch".into());
    }
    if user.is_empty() || (value(&c, "cheevos_token").is_empty() && value(&c, "cheevos_password").is_empty()) {
        return ("signed-out", user, "Not signed in to RetroAchievements in RetroArch".into());
    }
    ("ok", user, hardcore(truthy(c.get("cheevos_hardcore_mode_enable"))))
}

fn check_flycast(exe: &str, var: Var) -> Found {
    let h = home_of(var);
    let places = [
        js::join(&[&js::dirname(exe), "emu.cfg"]),
        js::join(&[&roaming_of(var), "flycast", "emu.cfg"]),
        js::join(&[&h, ".config", "flycast", "emu.cfg"]),
        js::join(&[&h, ".var", "app", "org.flycast.Flycast", "config", "flycast", "emu.cfg"]),
    ];
    check_ini(first_existing(places), "Flycast", "achievements", "hardcoremode")
}

/// Which of the emulators that can be read a program is, by its name (a portable copy keeps its usual name).
pub fn kind_of(exe: &str) -> Option<&'static str> {
    let name = js::basename(exe).to_lowercase();
    [("dolphin", "Dolphin"), ("ppsspp", "PPSSPP"), ("pcsx2", "PCSX2"), ("duckstation", "DuckStation")].iter().find(|(part, _)| name.contains(part)).map(|k| k.1).or_else(|| {
        if name.starts_with("retroarch") {
            Some("RetroArch")
        } else if name.contains("flycast") {
            Some("Flycast")
        } else {
            None
        }
    })
}

/// Every console's standing: the four always listed (GameCube, Wii, PSP, PS2), then every other console
/// RetroAchievements covers whose emulator can be read. `cfg`: the settings.
pub fn check(cfg: &Value, var: Var) -> Vec<Row> {
    let account = cfg["ra"]["user"].as_str().unwrap_or("").trim().to_string();
    let exe_of = |id: &str| cfg["systems"][id]["emulator"].as_str().unwrap_or("").trim().to_string();
    let mut rows: Vec<(String, &'static str)> = [("gc", "Dolphin"), ("wii", "Dolphin"), ("psp", "PPSSPP"), ("ps2", "PCSX2")].iter().map(|(id, usual)| (id.to_string(), kind_of(&exe_of(id)).unwrap_or(usual))).collect();
    for id in cfg["systems"].as_object().map(|s| s.keys().cloned().collect::<Vec<_>>()).unwrap_or_default() {
        if let Some(kind) = kind_of(&exe_of(&id)).filter(|_| !rows.iter().any(|r| r.0 == id) && crate::ra::console_id(&id).is_some()) {
            rows.push((id, kind));
        }
    }
    // two consoles often share one emulator: it's read once
    let mut read_already: HashMap<String, Found> = HashMap::new();
    rows.into_iter()
        .map(|(system, emulator)| {
            let row = |state: &'static str, user: String, detail: String| Row { system: system.clone(), emulator, state, user, detail };
            if crate::ra::console_id(&system).is_none() {
                return row("unsupported", String::new(), if system == "wii" { "Few Wii games have achievement sets yet".into() } else { "RetroAchievements does not cover this console".into() });
            }
            let exe = exe_of(&system);
            if exe.is_empty() {
                return row("no-emulator", String::new(), format!("No {emulator} set up for this console"));
            }
            let (state, user, detail) = read_already
                .entry(format!("{emulator}|{}", js::normalize(&exe).display()))
                .or_insert_with(|| match emulator {
                    "Dolphin" => check_dolphin(&exe, var),
                    "PPSSPP" => check_ppsspp(&exe, var),
                    "PCSX2" => check_ini(first_existing(emucfg::pcsx2_inis(&exe, var, false)), "PCSX2", "achievements", "challengemode"),
                    "DuckStation" => check_duck(&exe, var),
                    "RetroArch" => check_retroarch(&exe, var),
                    _ => check_flycast(&exe, var),
                })
                .clone();
            if state == "ok" && !account.is_empty() && !user.is_empty() && user.to_lowercase() != account.to_lowercase() {
                let detail = format!("{emulator} is signed in as {user}, not {account}");
                return row("other-user", user, detail);
            }
            row(state, user, detail)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcsx2_token_in_secrets() {
        let dir = std::env::temp_dir().join(format!("rustyshelf-raemu-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("PCSX2.ini");
        std::fs::write(&ini, "[Achievements]\nEnabled = true\nUsername = someone\nChallengeMode = false\n").unwrap();
        let f = || check_ini(Some(ini.to_string_lossy().into_owned()), "PCSX2", "achievements", "challengemode").0;
        assert_eq!(f(), "signed-out");
        std::fs::write(dir.join("secrets.ini"), "[Achievements]\nToken = abc\n").unwrap();
        assert_eq!(f(), "ok");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

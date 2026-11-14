// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/emusetup.js, as RustyShelf has it.
//! "Set up for me": an emulator set up the way the shelf expects, one switch per kind of change.
//!
//! - controller: the first pad mapped, only while the emulator is still on keyboard-only controls
//! - hotkeys: save / load / pause / screenshot on the keys the Home menu presses
//! - bios: one shared BIOS folder (`RustyShelf\BIOS`), only while the emulator's own has nothing in it
//! - graphics: a resolution preset, fast / balanced / pretty
//! - quit: no "are you sure?" question when the game is closed
//! - resume (RetroArch, off unless chosen): save on quit, carry on where you left off next time
//!
//! Every file is backed up once before its first change (the backups Settings → Restore emulator settings puts
//! back), written whole through a temporary file, and listed in `data/emu-setup.json` so Undo puts exactly those
//! back.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::art::resolve;
use crate::emucfg::{self, Var};
use crate::js;
use crate::paths::{is_dir, is_file};

/// Windows for now: the emulators keep their settings elsewhere on Linux.
pub const ON: bool = cfg!(windows);

pub const GROUPS: &[&str] = &["controller", "hotkeys", "bios", "graphics", "quit", "resume"];

/// A group's name in Settings.
pub fn group_name(group: &str) -> &'static str {
    match group {
        "controller" => "Controller",
        "hotkeys" => "Home menu hotkeys",
        "bios" => "BIOS folder",
        "graphics" => "Graphics",
        "quit" => "No quit questions",
        "resume" => "Resume where you left off",
        _ => "",
    }
}

/// Every group is on unless switched off, but resume.
fn default_on(group: &str) -> bool {
    group != "resume"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    RetroArch,
    Dolphin,
    DuckStation,
    Pcsx2,
    Ppsspp,
}

impl Kind {
    /// As the Electron app writes it in `data/emu-setup.json`.
    pub fn id(self) -> &'static str {
        match self {
            Kind::RetroArch => "retroarch",
            Kind::Dolphin => "dolphin",
            Kind::DuckStation => "duckstation",
            Kind::Pcsx2 => "pcsx2",
            Kind::Ppsspp => "ppsspp",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::RetroArch => "RetroArch",
            Kind::Dolphin => "Dolphin",
            Kind::DuckStation => "DuckStation",
            Kind::Pcsx2 => "PCSX2",
            Kind::Ppsspp => "PPSSPP",
        }
    }
}

/// The emulators this can set up, by their program's name.
pub fn kind_of(exe: &str) -> Option<Kind> {
    let n = js::basename(exe).to_lowercase();
    if n.starts_with("retroarch") {
        return Some(Kind::RetroArch);
    }
    [("dolphin", Kind::Dolphin), ("duckstation", Kind::DuckStation), ("pcsx2", Kind::Pcsx2), ("ppsspp", Kind::Ppsspp)].into_iter().find(|(word, _)| n.contains(word)).map(|(_, k)| k)
}

/// One change to a settings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub group: &'static str,
    pub file: String,
    /// RetroArch's `key = "value"` lines without sections, not an ini
    pub cfg: bool,
    pub section: String,
    pub key: String,
    pub value: String,
}

/// Every change setting an emulator up would make, and the groups left alone with why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub ok: bool,
    /// why it can't be set up (`ok` false)
    pub why: String,
    pub changes: Vec<Change>,
    pub skipped: Vec<(&'static str, &'static str)>,
    pub groups: Vec<&'static str>,
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// "fast", "balanced" or "pretty"; anything else is balanced
    pub graphics: String,
    /// the BIOS folder to point at; empty for the shared one
    pub bios_dir: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub ok: bool,
    pub why: String,
    pub applied: Vec<&'static str>,
    pub skipped: Vec<(&'static str, &'static str)>,
    pub files: Vec<String>,
}

/// What Settings shows for one emulator.
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub kind: Kind,
    /// its settings file is there (RetroArch reads one RustyShelf starts for it)
    pub ready: bool,
    pub file: Option<String>,
    /// its settings file is older than two days and RustyShelf hasn't set it up: someone has, so the first run
    /// leaves it alone
    pub used: bool,
    /// when it was set up (ms since 1970) and which groups
    pub done: Option<(u64, Vec<String>)>,
    pub groups: Vec<&'static str>,
    pub skipped: Vec<(&'static str, &'static str)>,
    pub bios_dir: Option<String>,
}

pub struct Setup<'a> {
    pub data_dir: PathBuf,
    pub app_root: PathBuf,
    pub var: Var<'a>,
}

fn first_file(list: impl IntoIterator<Item = String>) -> Option<String> {
    list.into_iter().find(|f| is_file(Path::new(f)))
}

fn read_text(f: &str) -> String {
    let t = std::fs::read(f).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    t.strip_prefix('\u{feff}').map(String::from).unwrap_or(t)
}

static SECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new("^\\s*\\[([^\n\r\u{2028}\u{2029}]+)\\]\\s*$").unwrap());
static NL: LazyLock<Regex> = LazyLock::new(|| Regex::new("\r?\n").unwrap());

/// A section's keys and values, each key once (its first place, its last value).
fn ini_section(text: &str, section: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut sec = String::new();
    for l in NL.split(text) {
        if let Some(m) = SECTION.captures(l) {
            sec = m[1].to_string();
            continue;
        }
        if sec != section {
            continue;
        }
        if let Some((k, v)) = l.split_once('=').filter(|(k, _)| !k.is_empty()) {
            let (k, v) = (js::trim(k).to_string(), js::trim(v).to_string());
            match out.iter_mut().find(|e| e.0 == k) {
                Some(e) => e.1 = v,
                None => out.push((k, v)),
            }
        }
    }
    out
}

/// A key's value in a section: the first line that sets it.
fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut sec = String::new();
    for l in NL.split(text) {
        if let Some(m) = SECTION.captures(l) {
            sec = m[1].to_string();
            continue;
        }
        if sec != section {
            continue;
        }
        if let Some((_, v)) = l.split_once('=').filter(|(k, _)| !k.is_empty() && js::trim(k) == key) {
            return Some(js::trim(v).to_string());
        }
    }
    None
}

/// A key's value in RetroArch's retroarch.cfg.
fn cfg_get(text: &str, key: &str) -> Option<String> {
    Regex::new(&format!(r#"(?m)^\s*{}\s*=\s*"([^"]*)""#, regex::escape(key))).ok()?.captures(text).map(|m| m[1].to_string())
}

fn has_bios_files(d: &str) -> bool {
    static BIOS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(bin|rom|zip|img|bios)$").unwrap());
    std::fs::read_dir(d).is_ok_and(|rd| rd.flatten().any(|e| BIOS.is_match(&e.file_name().to_string_lossy())))
}

/// The pad's inputs as SDL names them, in the order DuckStation and PCSX2 list a pad's buttons.
const SDL: &[(&str, &str)] = &[
    ("Up", "DPadUp"),
    ("Right", "DPadRight"),
    ("Down", "DPadDown"),
    ("Left", "DPadLeft"),
    ("Triangle", "Y"),
    ("Circle", "B"),
    ("Cross", "A"),
    ("Square", "X"),
    ("Select", "Back"),
    ("Start", "Start"),
    ("L1", "LeftShoulder"),
    ("R1", "RightShoulder"),
    ("L2", "+LeftTrigger"),
    ("R2", "+RightTrigger"),
    ("L3", "LeftStick"),
    ("R3", "RightStick"),
    ("LUp", "-LeftY"),
    ("LDown", "+LeftY"),
    ("LLeft", "-LeftX"),
    ("LRight", "+LeftX"),
    ("RUp", "-RightY"),
    ("RDown", "+RightY"),
    ("RLeft", "-RightX"),
    ("RRight", "+RightX"),
    ("LargeMotor", "LargeMotor"),
    ("SmallMotor", "SmallMotor"),
];

/// No "Analog" button: the pad's Guide button is the Home menu's.
fn sdl_pad(kind: &str) -> Vec<(String, String)> {
    std::iter::once(("Type".to_string(), kind.to_string())).chain(SDL.iter().map(|(k, v)| (k.to_string(), format!("SDL-0/{v}")))).collect()
}

fn pad_is_set(section: &[(String, String)]) -> bool {
    static DEVICE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)SDL-|XInput-|DInput-").unwrap());
    section.iter().any(|(k, v)| k != "Type" && DEVICE.is_match(v))
}

/// A GameCube pad on the first Xbox-style controller, as Dolphin names its inputs (main/emusetup.js DOLPHIN_PAD).
const DOLPHIN_PAD: &[(&str, &str)] = &[
    ("Device", "XInput/0/Gamepad"),
    ("Buttons/A", "`Button A`"),
    ("Buttons/B", "`Button B`"),
    ("Buttons/X", "`Button X`"),
    ("Buttons/Y", "`Button Y`"),
    ("Buttons/Z", "`Shoulder R`"),
    ("Buttons/Start", "`Start`"),
    ("Main Stick/Up", "`Left Y+`"),
    ("Main Stick/Down", "`Left Y-`"),
    ("Main Stick/Left", "`Left X-`"),
    ("Main Stick/Right", "`Left X+`"),
    ("C-Stick/Up", "`Right Y+`"),
    ("C-Stick/Down", "`Right Y-`"),
    ("C-Stick/Left", "`Right X-`"),
    ("C-Stick/Right", "`Right X+`"),
    ("Triggers/L", "`Trigger L`"),
    ("Triggers/R", "`Trigger R`"),
    ("Triggers/L-Analog", "`Trigger L`"),
    ("Triggers/R-Analog", "`Trigger R`"),
    ("D-Pad/Up", "`Pad N`"),
    ("D-Pad/Down", "`Pad S`"),
    ("D-Pad/Left", "`Pad W`"),
    ("D-Pad/Right", "`Pad E`"),
    ("Rumble/Motor", "`Motor L` | `Motor R`"),
];

/// The internal resolution each preset sets: fast, balanced, pretty.
fn resolution(kind: Kind, preset: &str) -> &'static str {
    let (fast, balanced, pretty) = match kind {
        Kind::DuckStation | Kind::Ppsspp => ("1", "2", "4"),
        _ => ("1", "2", "3"),
    };
    match preset {
        "fast" => fast,
        "pretty" => pretty,
        _ => balanced,
    }
}

fn pairs(want: &[(&str, &str)]) -> Vec<(String, String)> {
    want.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// RetroArch's retroarch.cfg: `key = "value"` lines, no sections. As safe as the ini writer: a file that can't be
/// read, or isn't UTF-8 text, is left alone. True if the file changed.
pub fn set_cfg(f: &str, want: &[(String, String)]) -> std::io::Result<bool> {
    static KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([A-Za-z0-9_]+)\s*=").unwrap());
    let buf = match std::fs::read(f) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Ok(false),
    };
    let src = match &buf {
        Some(b) if b.contains(&0) => return Ok(false),
        Some(b) => match std::str::from_utf8(b) {
            Ok(t) => t.to_string(),
            Err(_) => return Ok(false),
        },
        None => String::new(),
    };
    let nl = if src.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = if src.is_empty() { Vec::new() } else { NL.split(&src).map(String::from).collect() };
    let (mut seen, mut changed) = (Vec::new(), false);
    for line in lines.iter_mut() {
        let Some(k) = KEY.captures(line).map(|m| m[1].to_string()) else { continue };
        let Some((_, v)) = want.iter().rev().find(|(w, _)| *w == k) else { continue };
        let set = format!("{k} = \"{v}\"");
        seen.push(k);
        if *line != set {
            *line = set;
            changed = true;
        }
    }
    for (k, v) in want.iter().filter(|(k, _)| !seen.contains(k)) {
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.push(format!("{k} = \"{v}\""));
        changed = true;
    }
    if !changed {
        return Ok(false);
    }
    if buf.is_some() {
        let bak = emucfg::bak_of(f);
        if Path::new(&bak).symlink_metadata().is_err() {
            std::fs::copy(f, &bak)?;
        }
    }
    if let Some(d) = Path::new(f).parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = format!("{f}.tmp");
    std::fs::write(&tmp, format!("{}{nl}", lines.join(nl))).and_then(|()| std::fs::rename(&tmp, f)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(true)
}

impl Setup<'_> {
    fn app_data(&self) -> String {
        emucfg::get(self.var, "APPDATA").unwrap_or_else(|| js::join(&[&emucfg::home(self.var), "AppData", "Roaming"]))
    }

    /// A fresh RetroArch makes its settings beside its program.
    fn ra_cfg(&self, exe: &str) -> String {
        let own = js::join(&[&js::dirname(exe), "retroarch.cfg"]);
        let roaming = js::join(&[&self.app_data(), "RetroArch", "retroarch.cfg"]);
        if !is_file(Path::new(&own)) && is_file(Path::new(&roaming)) {
            roaming
        } else {
            own
        }
    }

    fn dolphin_dir(&self, exe: &str) -> Option<String> {
        emucfg::dolphin_user_dirs(exe, self.var).into_iter().find(|d| is_dir(Path::new(&js::join(&[d, "Config"]))))
    }

    fn duck_ini(&self, exe: &str) -> Option<String> {
        let dir = js::dirname(exe);
        let local = emucfg::get(self.var, "LOCALAPPDATA").unwrap_or_else(|| js::join(&[&emucfg::home(self.var), "AppData", "Local"]));
        let portable = is_file(Path::new(&js::join(&[&dir, "portable.txt"]))).then(|| js::join(&[&dir, "settings.ini"]));
        let docs = emucfg::docs_dirs(self.var).into_iter().map(|d| js::join(&[&d, "DuckStation", "settings.ini"]));
        first_file(portable.into_iter().chain([js::join(&[&local, "DuckStation", "settings.ini"])]).chain(docs))
    }

    fn pcsx2_ini(&self, exe: &str) -> Option<String> {
        first_file(emucfg::pcsx2_inis(exe, self.var, false))
    }

    fn ppsspp_ini(&self, exe: &str) -> Option<String> {
        let docs = emucfg::docs_dirs(self.var).into_iter().map(|d| js::join(&[&d, "PPSSPP", "PSP", "SYSTEM", "ppsspp.ini"]));
        first_file(std::iter::once(js::join(&[&js::dirname(exe), "memstick", "PSP", "SYSTEM", "ppsspp.ini"])).chain(docs))
    }

    /// The settings file this emulator has; None when it hasn't been opened yet, so there's nothing to set up
    /// (RetroArch reads a file RustyShelf starts for it).
    pub fn main_file(&self, kind: Kind, exe: &str) -> Option<String> {
        match kind {
            Kind::RetroArch => Some(self.ra_cfg(exe)),
            Kind::Dolphin => self.dolphin_dir(exe).map(|d| js::join(&[&d, "Config", "Dolphin.ini"])),
            Kind::DuckStation => self.duck_ini(exe),
            Kind::Pcsx2 => self.pcsx2_ini(exe),
            Kind::Ppsspp => self.ppsspp_ini(exe),
        }
    }

    /// `RustyShelf\BIOS`: one BIOS folder for every emulator.
    pub fn shared_bios_dir(&self) -> String {
        let root = if self.app_root.as_os_str().is_empty() { self.data_dir.parent().unwrap_or(Path::new(".")).to_path_buf() } else { self.app_root.clone() };
        js::join(&[&root.to_string_lossy(), "BIOS"])
    }

    /// The folder an emulator reads its BIOS files from right now.
    pub fn bios_dir_of(&self, kind: Kind, exe: &str) -> Option<String> {
        static OWN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:[\\/]?").unwrap());
        let dir = js::dirname(exe);
        let under = |base: String, v: String| if js::is_absolute(&v) { v } else { js::join(&[&base, &v]) };
        let or_bios = |v: Option<String>| v.filter(|v| !v.is_empty()).unwrap_or_else(|| "bios".into());
        match kind {
            Kind::RetroArch => Some(match cfg_get(&read_text(&self.ra_cfg(exe)), "system_directory").filter(|v| !v.is_empty() && v != "default") {
                // ":" is RetroArch's own folder
                Some(v) => OWN.replace(&v, regex::NoExpand(&format!("{dir}{}", std::path::MAIN_SEPARATOR))).into_owned(),
                None => js::join(&[&dir, "system"]),
            }),
            Kind::DuckStation => {
                let f = self.duck_ini(exe)?;
                Some(under(js::dirname(&f), or_bios(ini_get(&read_text(&f), "BIOS", "SearchDirectory"))))
            }
            Kind::Pcsx2 => {
                let f = self.pcsx2_ini(exe)?;
                Some(under(js::dirname(&js::dirname(&f)), or_bios(ini_get(&read_text(&f), "Folders", "Bios"))))
            }
            _ => None,
        }
    }

    /// Every change setting this emulator up would make.
    pub fn plan(&self, kind: Kind, exe: &str, opts: &Options) -> Plan {
        static PS2_BIOS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(scph|ps2|bios).*\.bin$").unwrap());
        let mut p = Plan::default();
        let add = |p: &mut Plan, group: &'static str, file: &str, cfg: bool, section: &str, want: Vec<(String, String)>| {
            p.changes.extend(want.into_iter().map(|(key, value)| Change { group, file: file.to_string(), cfg, section: section.to_string(), key, value }));
        };
        let bios = if opts.bios_dir.is_empty() { self.shared_bios_dir() } else { opts.bios_dir.clone() };
        let bios_free = |d: Option<String>| d.filter(|d| !d.is_empty()).is_none_or(|d| !has_bios_files(&d));
        let not_opened = |name: &str| Plan { why: format!("open {name} once first"), ..Plan::default() };
        const MAPPED: (&str, &str) = ("controller", "a controller is already mapped");
        match kind {
            Kind::RetroArch => {
                let f = self.ra_cfg(exe);
                add(&mut p, "quit", &f, true, "", pairs(&[("confirm_quit", "false"), ("quit_press_twice", "false")]));
                add(&mut p, "hotkeys", &f, true, "", pairs(&[("input_save_state", "f2"), ("input_load_state", "f4"), ("input_screenshot", "f8")]));
                add(&mut p, "controller", &f, true, "", pairs(&[("input_autodetect_enable", "true")]));
                if bios_free(self.bios_dir_of(kind, exe)) {
                    add(&mut p, "bios", &f, true, "", vec![("system_directory".into(), bios)]);
                } else {
                    p.skipped.push(("bios", "its system folder already has BIOS files"));
                }
                add(&mut p, "resume", &f, true, "", pairs(&[("savestate_auto_save", "true"), ("savestate_auto_load", "true")]));
            }
            Kind::Dolphin => {
                let Some(d) = self.dolphin_dir(exe) else { return not_opened("Dolphin") };
                let pad = js::join(&[&d, "Config", "GCPadNew.ini"]);
                let device = ini_section(&read_text(&pad), "GCPad1").into_iter().find(|(k, _)| k == "Device").map(|e| e.1).unwrap_or_default();
                if device.is_empty() || device.to_lowercase().contains("keyboard") {
                    add(&mut p, "controller", &pad, false, "GCPad1", pairs(DOLPHIN_PAD));
                    add(&mut p, "controller", &js::join(&[&d, "Config", "Dolphin.ini"]), false, "Core", pairs(&[("SIDevice0", "6")]));
                } else {
                    p.skipped.push(MAPPED);
                }
                add(&mut p, "graphics", &js::join(&[&d, "Config", "GFX.ini"]), false, "Settings", pairs(&[("InternalResolution", resolution(kind, &opts.graphics))]));
            }
            Kind::DuckStation => {
                let Some(f) = self.duck_ini(exe) else { return not_opened("DuckStation") };
                let t = read_text(&f);
                add(&mut p, "quit", &f, false, "Main", pairs(&[("ConfirmPowerOff", "false")]));
                let keys = [("TogglePause", "Keyboard/Space"), ("SaveSelectedSaveState", "Keyboard/F2"), ("LoadSelectedSaveState", "Keyboard/F1"), ("Screenshot", "Keyboard/F10")];
                add(&mut p, "hotkeys", &f, false, "Hotkeys", pairs(&keys));
                if !pad_is_set(&ini_section(&t, "Pad1")) {
                    add(&mut p, "controller", &f, false, "Pad1", sdl_pad("AnalogController"));
                } else {
                    p.skipped.push(MAPPED);
                }
                if bios_free(self.bios_dir_of(kind, exe)) {
                    add(&mut p, "bios", &f, false, "BIOS", vec![("SearchDirectory".into(), bios)]);
                } else {
                    p.skipped.push(("bios", "its BIOS folder already has files"));
                }
                add(&mut p, "graphics", &f, false, "GPU", pairs(&[("ResolutionScale", resolution(kind, &opts.graphics))]));
            }
            Kind::Pcsx2 => {
                let Some(f) = self.pcsx2_ini(exe) else { return not_opened("PCSX2") };
                let t = read_text(&f);
                add(&mut p, "quit", &f, false, "UI", pairs(&[("ConfirmShutdown", "false")]));
                let keys = [("TogglePause", "Keyboard/Space"), ("SaveStateToSlot", "Keyboard/F1"), ("LoadStateFromSlot", "Keyboard/F3"), ("Screenshot", "Keyboard/F8")];
                add(&mut p, "hotkeys", &f, false, "Hotkeys", pairs(&keys));
                if !pad_is_set(&ini_section(&t, "Pad1")) {
                    add(&mut p, "controller", &f, false, "Pad1", sdl_pad("DualShock2"));
                } else {
                    p.skipped.push(MAPPED);
                }
                // PCSX2 needs one BIOS file chosen, not only a folder: only when the shared folder has a PS2 BIOS
                let ps2 = crate::scanner::read_dir_sorted(Path::new(&bios)).into_iter().map(|e| e.file_name().to_string_lossy().into_owned()).find(|n| PS2_BIOS.is_match(n));
                match ps2 {
                    _ if !bios_free(self.bios_dir_of(kind, exe)) => p.skipped.push(("bios", "its BIOS folder already has files")),
                    None => p.skipped.push(("bios", "no PS2 BIOS in the BIOS folder yet")),
                    Some(ps2) => {
                        add(&mut p, "bios", &f, false, "Folders", vec![("Bios".into(), bios)]);
                        add(&mut p, "bios", &f, false, "Filenames", vec![("BIOS".into(), ps2)]);
                    }
                }
                add(&mut p, "graphics", &f, false, "EmuCore/GS", pairs(&[("upscale_multiplier", resolution(kind, &opts.graphics))]));
            }
            Kind::Ppsspp => {
                let Some(f) = self.ppsspp_ini(exe) else { return not_opened("PPSSPP") };
                add(&mut p, "graphics", &f, false, "Graphics", pairs(&[("InternalResolution", resolution(kind, &opts.graphics))]));
            }
        }
        p.ok = true;
        for c in &p.changes {
            if !p.groups.contains(&c.group) {
                p.groups.push(c.group);
            }
        }
        p
    }

    fn manifest_file(&self) -> PathBuf {
        self.data_dir.join("emu-setup.json")
    }

    /// What was set up: `{ "<kind>|<program>": { kind, exe, files, created, groups, at } }`.
    fn manifest(&self) -> Map<String, Value> {
        match std::fs::read(self.manifest_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        }
    }

    fn save_manifest(&self, m: Map<String, Value>) {
        let f = self.manifest_file();
        let tmp = f.with_extension("json.tmp");
        if std::fs::write(&tmp, js::stringify_indented(&Value::Object(m), 1)).is_ok() {
            let _ = std::fs::rename(&tmp, &f);
        }
    }

    fn key_of(kind: Kind, exe: &str) -> String {
        format!("{}|{}", kind.id(), resolve(Path::new(exe)).to_string_lossy().to_lowercase())
    }

    /// Applies the groups that are switched on. `on`: a group and whether it's on; one that isn't listed has its
    /// default.
    pub fn apply(&self, exe: &str, on: &[(&str, bool)], opts: &Options, now: u64) -> Applied {
        let fail = |why: String| Applied { why, ..Applied::default() };
        let Some(kind) = kind_of(exe) else { return fail("not one RustyShelf can set up yet".into()) };
        if !is_file(Path::new(exe)) {
            return fail("the emulator isn't there any more".into());
        }
        let p = self.plan(kind, exe, opts);
        if !p.ok {
            return fail(p.why);
        }
        let want = |g: &str| on.iter().rev().find(|(k, _)| *k == g).map_or(default_on(g), |(_, v)| *v);
        let used: Vec<&Change> = p.changes.iter().filter(|c| want(c.group)).collect();
        if used.iter().any(|c| c.group == "bios") {
            let _ = std::fs::create_dir_all(if opts.bios_dir.is_empty() { self.shared_bios_dir() } else { opts.bios_dir.clone() });
        }
        // each file and section once, with every key it gets
        let mut by_file: Vec<(&Change, Vec<(String, String)>)> = Vec::new();
        for c in &used {
            let i = match by_file.iter().position(|(f, _)| f.file == c.file && f.section == c.section) {
                Some(i) => i,
                None => {
                    by_file.push((c, Vec::new()));
                    by_file.len() - 1
                }
            };
            match by_file[i].1.iter_mut().find(|(k, _)| *k == c.key) {
                Some(e) => e.1 = c.value.clone(),
                None => by_file[i].1.push((c.key.clone(), c.value.clone())),
            }
        }
        let mut m = self.manifest();
        let key = Self::key_of(kind, exe);
        let mut rec = m.get(&key).filter(|r| r.is_object()).cloned().unwrap_or_else(|| json!({ "kind": kind.id(), "exe": exe, "files": [], "created": [], "groups": [] }));
        let listed = |rec: &Value, k: &str| -> Vec<String> { rec.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default() };
        let (mut files, mut created) = (listed(&rec, "files"), listed(&rec, "created"));
        let mut done = Vec::new();
        for (c, w) in &by_file {
            let existed = is_file(Path::new(&c.file));
            let refs: Vec<(&str, &str)> = w.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let changed = if c.cfg { set_cfg(&c.file, w) } else { emucfg::set_ini(&c.file, &c.section, &refs) };
            match changed {
                Ok(true) => {
                    if !files.contains(&c.file) {
                        files.push(c.file.clone());
                    }
                    if !existed && !created.contains(&c.file) {
                        created.push(c.file.clone());
                    }
                    done.push(c.file.clone());
                }
                Ok(false) => {}
                Err(e) => return fail(format!("couldn't write {} ({e})", js::basename(&c.file))),
            }
        }
        let mut groups: Vec<&'static str> = Vec::new();
        for c in &used {
            if !groups.contains(&c.group) {
                groups.push(c.group);
            }
        }
        let mut all = listed(&rec, "groups");
        all.extend(groups.iter().filter(|g| !all.contains(&g.to_string())).map(|g| g.to_string()).collect::<Vec<_>>());
        rec["files"] = json!(files);
        rec["created"] = json!(created);
        rec["groups"] = json!(all);
        rec["at"] = json!(now);
        m.insert(key, rec);
        self.save_manifest(m);
        Applied { ok: true, why: String::new(), applied: groups, skipped: p.skipped.into_iter().filter(|(g, _)| want(g)).collect(), files: done }
    }

    /// Puts back every file setting this emulator up changed (its copy from before), and removes the files it
    /// started from nothing. Returns the files put back.
    pub fn undo(&self, exe: &str) -> Vec<String> {
        let Some(kind) = kind_of(exe) else { return Vec::new() };
        let mut m = self.manifest();
        let key = Self::key_of(kind, exe);
        let Some(rec) = m.get(&key).filter(|r| js::truthy(Some(r))).cloned() else { return Vec::new() };
        let listed = |k: &str| -> Vec<String> { rec.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default() };
        let created = listed("created");
        let mut restored = Vec::new();
        for f in listed("files") {
            if created.contains(&f) {
                if !is_file(Path::new(&f)) || std::fs::remove_file(&f).is_ok() {
                    restored.push(f);
                }
                continue;
            }
            let bak = emucfg::bak_of(&f);
            if !is_file(Path::new(&bak)) {
                continue;
            }
            let tmp = format!("{f}.tmp");
            if std::fs::copy(&bak, &tmp).and_then(|_| std::fs::rename(&tmp, &f)).and_then(|()| std::fs::remove_file(&bak)).is_ok() {
                restored.push(f);
            }
        }
        m.shift_remove(&key);
        self.save_manifest(m);
        restored
    }

    /// What Settings shows for one emulator; None for one this can't set up.
    pub fn status(&self, exe: &str, now: u64) -> Option<Status> {
        const TWO_DAYS: u64 = 2 * 86_400_000;
        let kind = kind_of(exe)?;
        let file = self.main_file(kind, exe);
        let rec = self.manifest().get(&Self::key_of(kind, exe)).filter(|r| js::truthy(Some(r))).cloned();
        let modified = file.as_ref().and_then(|f| Path::new(f).metadata().ok()).filter(|m| m.is_file()).and_then(|m| m.modified().ok());
        let age = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|t| now.saturating_sub(t.as_millis() as u64));
        let p = self.plan(kind, exe, &Options::default());
        let done = rec.as_ref().map(|r| {
            let groups = r.get("groups").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default();
            (js::number(r.get("at")) as u64, groups)
        });
        Some(Status {
            kind,
            ready: kind == Kind::RetroArch || file.is_some(),
            used: rec.is_none() && age.is_some_and(|a| a > TWO_DAYS),
            file,
            done,
            groups: p.groups,
            skipped: p.skipped,
            bios_dir: self.bios_dir_of(kind, exe),
        })
    }
}

/// What the first run's "Set up my emulators for me" did, by emulator name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Auto {
    pub done: Vec<&'static str>,
    /// no settings file yet: set up at their next launch (`pending`)
    pub later: Vec<&'static str>,
    /// used before: left as they are
    pub left: Vec<&'static str>,
    /// the settings' `emuSetup.pending`, with the emulators that wait added
    pub pending: Vec<String>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    resolve(a).to_string_lossy().to_lowercase() == resolve(b).to_string_lossy().to_lowercase()
}

impl Setup<'_> {
    /// Sets up each emulator once: `emulators` as the settings have them (the consoles switched on), `resolved` turns
    /// one into its path. One that hasn't run yet waits for its settings file; one used before is left alone.
    pub fn auto(&self, emulators: &[String], resolved: &dyn Fn(&str) -> PathBuf, pending: &[String], resume: bool, opts: &Options, now: u64) -> Auto {
        let mut out = Auto { pending: pending.to_vec(), ..Auto::default() };
        let mut seen: Vec<String> = Vec::new();
        for emu in emulators.iter().filter(|e| !e.is_empty()) {
            let path = resolved(emu);
            let key = resolve(&path).to_string_lossy().to_lowercase();
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            let exe = path.to_string_lossy().into_owned();
            let Some(st) = self.status(&exe, now).filter(|st| st.done.is_none()) else { continue };
            let name = st.kind.name();
            if st.used {
                out.left.push(name);
            } else if !st.ready {
                if !out.pending.iter().any(|p| same_path(&resolved(p), &path)) {
                    out.pending.push(emu.clone());
                }
                out.later.push(name);
            } else if self.apply(&exe, &[("resume", resume)], opts, now).ok {
                out.done.push(name);
            }
        }
        out
    }

    /// An emulator that waited for its settings file is set up as it's started, if the file is there now. Returns
    /// the waiting list without it, or None when nothing changed.
    pub fn at_launch(&self, exe: &Path, resolved: &dyn Fn(&str) -> PathBuf, pending: &[String], resume: bool, opts: &Options, now: u64) -> Option<Vec<String>> {
        if !pending.iter().any(|p| same_path(&resolved(p), exe)) {
            return None;
        }
        let text = exe.to_string_lossy();
        if !self.status(&text, now).is_some_and(|st| st.ready) {
            return None;
        }
        self.apply(&text, &[("resume", resume)], opts, now);
        Some(pending.iter().filter(|p| !same_path(&resolved(p), exe)).cloned().collect())
    }
}

/// The graphics preset for this computer from its graphics card's vendor: NVIDIA, AMD, then Intel and the rest.
pub fn preset_for(vendor_id: Option<u32>) -> &'static str {
    match vendor_id {
        Some(0x10de) => "pretty",
        Some(0x1002) => "balanced",
        _ => "fast",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_sets_up_each_emulator_once() {
        let tmp = tempfile::tempdir().unwrap();
        let at = |p: &str| tmp.path().join(p).to_string_lossy().into_owned();
        let write = |p: &str, text: &str| {
            let f = tmp.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        };
        // a Dolphin that has run, a DuckStation that hasn't, a PCSX2 in use for days
        for f in ["a/Dolphin.exe", "a/portable.txt", "b/duckstation.exe", "b/portable.txt", "c/pcsx2.exe", "c/portable.ini"] {
            write(f, "");
        }
        std::fs::create_dir_all(tmp.path().join("app/data")).unwrap();
        write("a/User/Config/Dolphin.ini", "[Core]\n");
        write("c/inis/PCSX2.ini", "[UI]\n");
        let now = now_for_tests();
        let old = std::time::UNIX_EPOCH + std::time::Duration::from_millis(now - 3 * 86_400_000);
        std::fs::File::options().write(true).open(tmp.path().join("c/inis/PCSX2.ini")).unwrap().set_modified(old).unwrap();

        let home = at("home");
        let var = move |k: &str| matches!(k, "HOME" | "USERPROFILE").then(|| home.clone());
        let setup = Setup { data_dir: tmp.path().join("app/data"), app_root: tmp.path().join("app"), var: &var };
        let resolved = |e: &str| PathBuf::from(e);
        let emulators = vec![at("a/Dolphin.exe"), String::new(), at("b/duckstation.exe"), at("a/Dolphin.exe"), at("c/pcsx2.exe")];
        let opts = Options { graphics: "fast".into(), bios_dir: String::new() };
        let r = setup.auto(&emulators, &resolved, &[], false, &opts, now);
        assert_eq!((r.done, r.later, r.left), (vec!["Dolphin"], vec!["DuckStation"], vec!["PCSX2"]));
        assert_eq!(r.pending, vec![at("b/duckstation.exe")]);
        // done once: a second run leaves it, and the one that waits isn't listed twice
        let again = setup.auto(&emulators, &resolved, &r.pending, false, &opts, now);
        assert_eq!((again.done, again.later, again.pending.len()), (Vec::<&str>::new(), vec!["DuckStation"], 1));

        let duck = PathBuf::from(at("b/duckstation.exe"));
        assert_eq!(setup.at_launch(&duck, &resolved, &r.pending, false, &opts, now), None);
        assert_eq!(setup.at_launch(Path::new(&at("a/Dolphin.exe")), &resolved, &r.pending, false, &opts, now), None);
        write("b/settings.ini", "[Main]\n");
        assert_eq!(setup.at_launch(&duck, &resolved, &r.pending, false, &opts, now), Some(Vec::new()));
        assert!(setup.status(&duck.to_string_lossy(), now).unwrap().done.is_some());
    }

    fn now_for_tests() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
    }
}

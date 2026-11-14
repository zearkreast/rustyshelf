//! The first run and Settings: pages of rows, moved through with the keys or a pad. What's chosen is saved to
//! `data/config.json` at once, by the rules the Electron app's settings go through.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::text::FontSize;
use rustyshelf_core::config::Config;
use rustyshelf_core::detect::{Detected, Detector};
use rustyshelf_core::systems::{systems, Console};
use rustyshelf_core::{emucfg, emuname, paths};
use serde_json::{json, Value};

use crate::hud::Face;
use crate::loading::{self, LibraryRes};
use crate::shelf::Cursor;

const STEEL: Color = Color::srgb(0.87, 0.89, 0.92);
const RUST: Color = Color::srgb(0.83, 0.32, 0.17);
const DIM: Color = Color::srgb(0.55, 0.58, 0.62);
const PANEL: Color = Color::srgb(0.05, 0.05, 0.07);
const CHOSEN: Color = Color::srgba(0.83, 0.32, 0.17, 0.28);

/// The rows a page shows at once; a longer page scrolls with the cursor.
const WINDOW: usize = 11;

#[derive(Clone, PartialEq)]
enum Screen {
    Welcome,
    Root,
    Consoles,
    Console(String),
    /// the emulator of a console, or RetroArch for every retro console ("")
    Emulator(String),
    Folders(String),
    Pc,
    BoxArt,
    Display,
    Controller,
    System,
    Done,
}

type Press = Box<dyn Fn(&mut Settings) + Send + Sync>;
type Step = Box<dyn Fn(&mut Settings, i32) + Send + Sync>;

#[derive(Default)]
struct Row {
    label: String,
    value: String,
    /// what the row does, shown under the page while it's chosen
    hint: String,
    press: Option<Press>,
    /// left / right
    step: Option<Step>,
}

struct Page {
    title: String,
    lead: String,
    rows: Vec<Row>,
}

/// What a file dialog's answer is for.
#[derive(Clone)]
enum Pick {
    Emulator(String),
    Folder(String),
    Steam,
    GamesDir,
    ArtLibrary,
}

#[derive(Resource)]
pub struct Settings {
    pub open: bool,
    /// closed by a key this frame: the shelf doesn't take that key too
    pub just_closed: bool,
    /// the first run: the pages come one after another, and can't be left before the last
    wizard: bool,
    cfg: Value,
    app_root: PathBuf,
    /// the pages gone through, each with its cursor
    stack: Vec<(Screen, usize)>,
    found: Option<Detected>,
    looking: Option<Mutex<Receiver<Detected>>>,
    picking: Option<(Pick, Mutex<Receiver<Option<PathBuf>>>)>,
    redraw: bool,
    /// something was saved since Settings opened: the shelves are read again when it closes
    changed: bool,
    note: String,
    repeat: (i32, f32),
}

impl Default for Settings {
    fn default() -> Self {
        let app_root = paths::app_root();
        let cfg = Config::load(&paths::data_dir(&app_root)).unwrap_or_default();
        let wizard = !cfg.setup_done();
        Settings {
            open: wizard,
            just_closed: false,
            wizard,
            cfg: Value::Object(cfg.raw().clone()),
            app_root,
            stack: vec![(if wizard { Screen::Welcome } else { Screen::Root }, 0)],
            found: None,
            looking: None,
            picking: None,
            redraw: true,
            changed: false,
            note: String::new(),
            repeat: (0, 0.0),
        }
    }
}

fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(v, |v, k| v.get(k))
}

fn put(v: &mut Value, path: &[&str], x: Value) {
    let mut cur = v;
    for k in &path[..path.len() - 1] {
        if !cur.get(k).is_some_and(Value::is_object) {
            cur[*k] = json!({});
        }
        cur = &mut cur[*k];
    }
    cur[path[path.len() - 1]] = x;
}

fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_string()
}

/// A path short enough for a row: its end, which says the most.
fn short(p: &str) -> String {
    let n = p.chars().count();
    if n <= 58 {
        p.to_string()
    } else {
        format!("…{}", p.chars().skip(n - 57).collect::<String>())
    }
}

fn is_retroarch(exe: &str) -> bool {
    exe.to_lowercase().contains("retroarch")
}

impl Settings {
    fn flag(&self, path: &[&str], default: bool) -> bool {
        at(&self.cfg, path).and_then(Value::as_bool).unwrap_or(default)
    }

    fn text(&self, path: &[&str]) -> String {
        at(&self.cfg, path).and_then(Value::as_str).unwrap_or("").to_string()
    }

    fn list(&self, path: &[&str]) -> Vec<String> {
        at(&self.cfg, path).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default()
    }

    /// Saves the settings as they are now.
    fn save(&mut self) {
        let Some(cfg) = Config::from_value(&self.cfg) else { return };
        self.cfg = Value::Object(cfg.raw().clone());
        if let Err(e) = cfg.save(&paths::data_dir(&self.app_root)) {
            self.note = format!("The settings couldn't be saved ({e}).");
        }
        self.changed = true;
        self.redraw = true;
    }

    fn set(&mut self, path: &[&str], x: Value) {
        put(&mut self.cfg, path, x);
        self.save();
    }

    fn toggle(&mut self, path: &[&str], default: bool) {
        let on = self.flag(path, default);
        self.set(path, json!(!on));
    }

    fn go(&mut self, screen: Screen) {
        self.stack.push((screen, 0));
        self.note.clear();
        self.redraw = true;
    }

    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        } else if !self.wizard {
            self.open = false;
        }
        self.note.clear();
        self.redraw = true;
    }

    /// Looks for emulators, games folders, Steam and the media apps, off the screen's thread.
    fn look(&mut self) {
        if self.looking.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let root = self.app_root.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Detector::new(&root).detect_all());
        });
        self.looking = Some(Mutex::new(rx));
        self.redraw = true;
    }

    /// What was found goes into the settings that are still empty; nothing chosen by hand is changed.
    fn apply_found(&mut self, d: &Detected) {
        for (id, f) in &d.systems {
            if self.text(&["systems", id, "emulator"]).is_empty() {
                if let Some(e) = &f.emulator {
                    put(&mut self.cfg, &["systems", id, "emulator"], json!(e));
                }
            }
            if self.list(&["systems", id, "romDirs"]).is_empty() && !f.rom_dirs.is_empty() {
                put(&mut self.cfg, &["systems", id, "romDirs"], json!(f.rom_dirs));
            }
        }
        if self.text(&["retroarch"]).is_empty() && !d.retroarch.is_empty() {
            put(&mut self.cfg, &["retroarch"], json!(d.retroarch));
        }
        if let Some(steam) = d.steam.as_ref().filter(|_| self.text(&["steam", "path"]).is_empty()) {
            put(&mut self.cfg, &["steam", "path"], json!(steam));
        }
        for (id, path) in &d.apps {
            if let Some(p) = path.as_ref().filter(|_| self.text(&["apps", id, "path"]).is_empty()) {
                put(&mut self.cfg, &["apps", id, "path"], json!(p));
            }
        }
        // the first run: what was found comes switched on, the rest off. RetroArch alone switches nothing on: it
        // runs every retro console
        if self.wizard {
            for c in &systems().consoles {
                let found = !self.list(&["systems", &c.id, "romDirs"]).is_empty() || Some(self.text(&["systems", &c.id, "emulator"])).is_some_and(|e| !e.is_empty() && !is_retroarch(&e));
                put(&mut self.cfg, &["systems", &c.id, "enabled"], json!(found));
            }
            let steam = !self.text(&["steam", "path"]).is_empty();
            put(&mut self.cfg, &["steam", "enabled"], json!(steam));
            for a in &systems().apps {
                let there = !self.text(&["apps", &a.id, "path"]).is_empty();
                put(&mut self.cfg, &["apps", &a.id, "enabled"], json!(there));
            }
        }
        self.save();
    }

    /// Asks for a program or a folder with the system's own dialog, off the screen's thread.
    fn pick(&mut self, what: Pick, title: &str, folder: bool) {
        if self.picking.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let title = title.to_string();
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title(&title);
            let _ = tx.send(if folder { dialog.pick_folder() } else { dialog.pick_file() });
        });
        self.picking = Some((what, Mutex::new(rx)));
    }

    fn picked(&mut self, what: Pick, path: &Path) {
        let p = path.to_string_lossy().into_owned();
        match what {
            Pick::Emulator(id) if id.is_empty() => self.set(&["retroarch"], json!(p)),
            Pick::Emulator(id) => {
                self.set(&["systems", &id, "emulator"], json!(p));
                self.back();
            }
            Pick::Folder(id) => {
                let mut dirs = self.list(&["systems", &id, "romDirs"]);
                if !dirs.contains(&p) {
                    dirs.push(p);
                    self.set(&["systems", &id, "romDirs"], json!(dirs));
                }
            }
            Pick::Steam => self.set(&["steam", "path"], json!(p)),
            Pick::GamesDir => self.set(&["pc", "gamesDir"], json!(p)),
            Pick::ArtLibrary => self.set(&["ui", "artLibrary"], json!(p)),
        }
    }

    /// What a console has: its emulator found, its games folders.
    fn status(&self, c: &Console) -> String {
        let (emu, n) = (self.text(&["systems", &c.id, "emulator"]), self.list(&["systems", &c.id, "romDirs"]).len());
        let name = if c.emulator_name.is_empty() { "Emulator" } else { &c.emulator_name };
        let folders = format!("{n} game folder{}", if n == 1 { "" } else { "s" });
        match (emu.is_empty(), n) {
            (false, 0) => format!("{name} found"),
            (false, _) => format!("{name} found · {folders}"),
            (true, 0) if c.retro && !self.text(&["retroarch"]).is_empty() => "RetroArch can run it".to_string(),
            (true, 0) => format!("{name} not found"),
            (true, _) => format!("Games found · {name} not found yet"),
        }
    }

    fn console(&self, id: &str) -> Option<&'static Console> {
        systems().console(id)
    }

    fn page(&self) -> Page {
        let toggle = |label: &str, path: &'static [&'static str], default: bool, hint: &str| -> Row {
            Row {
                label: label.to_string(),
                value: on_off(self.flag(path, default)),
                hint: hint.to_string(),
                press: Some(Box::new(move |s: &mut Settings| s.toggle(path, default))),
                step: Some(Box::new(move |s: &mut Settings, _| s.toggle(path, default))),
            }
        };
        let open = |label: &str, value: String, hint: &str, to: Screen| -> Row {
            Row { label: label.to_string(), value, hint: hint.to_string(), press: Some(Box::new(move |s: &mut Settings| s.go(to.clone()))), step: None }
        };
        let next = |to: Screen| -> Row { Row { label: "Continue".into(), press: Some(Box::new(move |s: &mut Settings| s.go(to.clone()))), ..Row::default() } };
        let (screen, _) = self.stack.last().expect("a page");
        match screen {
            Screen::Welcome => Page {
                title: "Welcome to RustyShelf".into(),
                lead: "Let's find your games. RustyShelf looks for your emulators, games, Steam library and streaming apps. Next you pick which consoles you play, and it's ready in a minute.".into(),
                rows: vec![
                    Row {
                        label: if self.looking.is_some() { "Looking…" } else { "Find my emulators and games" }.into(),
                        press: Some(Box::new(|s: &mut Settings| s.look())),
                        ..Row::default()
                    },
                    Row { label: "I'll pick them myself".into(), press: Some(Box::new(|s: &mut Settings| s.go(Screen::Consoles))), ..Row::default() },
                ],
            },
            Screen::Root => Page {
                title: "Settings".into(),
                lead: String::new(),
                rows: vec![
                    open("Consoles", String::new(), "Each console's emulator and games folders", Screen::Consoles),
                    open("PC games and apps", String::new(), "Steam, the other stores, the PC games folder, the streaming apps", Screen::Pc),
                    open("Box art", String::new(), "Downloads, the region, the art library", Screen::BoxArt),
                    open("Display", String::new(), "", Screen::Display),
                    open("Controller", String::new(), "The quit combo", Screen::Controller),
                    open("System", String::new(), "Search again, emulator setup", Screen::System),
                    Row { label: "Back to the shelf".into(), press: Some(Box::new(|s: &mut Settings| s.back())), ..Row::default() },
                ],
            },
            Screen::Consoles => {
                let mut rows = vec![open(
                    "RetroArch",
                    Some(self.text(&["retroarch"])).filter(|e| !e.is_empty()).map_or("Not set".into(), |e| short(&e)),
                    "The program every retro console without an emulator of its own runs in",
                    Screen::Emulator(String::new()),
                )];
                // on the first run the consoles with something found come first
                let mut consoles: Vec<&Console> = systems().consoles.iter().collect();
                if self.wizard {
                    consoles.sort_by_key(|c| !self.flag(&["systems", &c.id, "enabled"], true));
                }
                for c in consoles {
                    let id = c.id.clone();
                    let (id2, id3) = (id.clone(), id.clone());
                    rows.push(Row {
                        label: c.name.clone(),
                        value: format!("{}  ·  {}", self.status(c), on_off(self.flag(&["systems", &c.id, "enabled"], true))),
                        hint: "◀ ▶ gives it a shelf or takes it away  ·  Enter opens it".into(),
                        press: Some(Box::new(move |s: &mut Settings| s.go(Screen::Console(id2.clone())))),
                        step: Some(Box::new(move |s: &mut Settings, _| {
                            let on = s.flag(&["systems", &id3, "enabled"], true);
                            s.set(&["systems", &id, "enabled"], json!(!on));
                        })),
                    });
                }
                if self.wizard {
                    rows.push(next(Screen::Pc));
                }
                Page { title: "Your consoles".into(), lead: "A console that's on has a shelf. Open one to choose its emulator and its games folders.".into(), rows }
            }
            Screen::Console(id) => {
                let Some(c) = self.console(id) else { return Page { title: id.clone(), lead: String::new(), rows: Vec::new() } };
                let emu = self.text(&["systems", id, "emulator"]);
                let n = self.list(&["systems", id, "romDirs"]).len();
                let (a, b, e, f) = (id.clone(), id.clone(), id.clone(), id.clone());
                let mut rows = vec![
                    Row {
                        label: "Shelf".into(),
                        value: on_off(self.flag(&["systems", id, "enabled"], true)),
                        hint: "Off: this console has no shelf".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let on = s.flag(&["systems", &a, "enabled"], true);
                            s.set(&["systems", &a, "enabled"], json!(!on));
                        })),
                        step: Some(Box::new(move |s: &mut Settings, _| {
                            let on = s.flag(&["systems", &b, "enabled"], true);
                            s.set(&["systems", &b, "enabled"], json!(!on));
                        })),
                    },
                    open("Emulator", if emu.is_empty() { "Not set".into() } else { emuname::label(&emu) }, &short(&emu), Screen::Emulator(e)),
                    open("Games folders", format!("{n} folder{}", if n == 1 { "" } else { "s" }), "", Screen::Folders(f)),
                    Row { label: "Launch options".into(), value: short(&self.text(&["systems", id, "gameArgs"])), hint: "{rom} is the game".into(), ..Row::default() },
                ];
                if id == "gc" {
                    let g = id.clone();
                    let g2 = g.clone();
                    rows.push(Row {
                        label: "GameCube startup".into(),
                        value: on_off(self.flag(&["systems", id, "useIpl"], false)),
                        hint: "Plays the console's own startup before a game (needs an IPL.bin in Dolphin's folder)".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let on = s.flag(&["systems", &g, "useIpl"], false);
                            s.set(&["systems", &g, "useIpl"], json!(!on));
                        })),
                        step: Some(Box::new(move |s: &mut Settings, _| {
                            let on = s.flag(&["systems", &g2, "useIpl"], false);
                            s.set(&["systems", &g2, "useIpl"], json!(!on));
                        })),
                    });
                }
                Page { title: c.name.clone(), lead: self.status(c), rows }
            }
            Screen::Emulator(id) => {
                let path: Vec<String> = if id.is_empty() { vec!["retroarch".into()] } else { vec!["systems".into(), id.clone(), "emulator".into()] };
                let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                let now = self.text(&refs);
                let found: Vec<String> = match &self.found {
                    Some(d) if id.is_empty() => Some(d.retroarch.clone()).filter(|r| !r.is_empty()).into_iter().collect(),
                    Some(d) => d.systems.iter().find(|(i, _)| i == id).map(|(_, f)| f.emulators.clone()).unwrap_or_default(),
                    None => Vec::new(),
                };
                let mut rows: Vec<Row> = Vec::new();
                let choose = |exe: String, path: Vec<String>| -> Press {
                    Box::new(move |s: &mut Settings| {
                        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                        s.set(&refs, json!(exe));
                        s.back();
                    })
                };
                for exe in found.iter().chain(Some(&now).filter(|n| !n.is_empty() && !found.contains(n))) {
                    rows.push(Row {
                        label: emuname::label(exe),
                        value: if *exe == now { "In use".into() } else { String::new() },
                        hint: short(exe),
                        press: Some(choose(exe.clone(), path.clone())),
                        step: None,
                    });
                }
                let what = Pick::Emulator(id.clone());
                rows.push(Row {
                    label: "Choose the program…".into(),
                    hint: "Opens a file dialog".into(),
                    press: Some(Box::new(move |s: &mut Settings| s.pick(what.clone(), "Choose the emulator's program", false))),
                    ..Row::default()
                });
                if !now.is_empty() {
                    rows.push(Row { label: "None".into(), hint: "Forgets the program".into(), press: Some(choose(String::new(), path)), ..Row::default() });
                }
                let name = if id.is_empty() { "RetroArch".to_string() } else { self.console(id).map_or(id.clone(), |c| format!("{} emulator", c.name)) };
                Page { title: name, lead: if found.is_empty() && self.found.is_none() { "Search again in System to list the ones on this computer.".into() } else { String::new() }, rows }
            }
            Screen::Folders(id) => {
                let dirs = self.list(&["systems", id, "romDirs"]);
                let mut rows: Vec<Row> = Vec::new();
                for d in &dirs {
                    let (id, d2) = (id.clone(), d.clone());
                    rows.push(Row {
                        label: short(d),
                        value: "Remove".into(),
                        hint: "Enter takes this folder off the shelf (nothing is deleted)".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let left: Vec<String> = s.list(&["systems", &id, "romDirs"]).into_iter().filter(|x| *x != d2).collect();
                            s.set(&["systems", &id, "romDirs"], json!(left));
                        })),
                        step: None,
                    });
                }
                let found = self.found.as_ref().and_then(|f| f.systems.iter().find(|(i, _)| i == id)).map(|(_, f)| f.rom_dirs.clone()).unwrap_or_default();
                for d in found.into_iter().filter(|d| !dirs.contains(d)) {
                    let (id, d2) = (id.clone(), d.clone());
                    rows.push(Row {
                        label: short(&d),
                        value: "Add".into(),
                        hint: "Found on this computer".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let mut dirs = s.list(&["systems", &id, "romDirs"]);
                            dirs.push(d2.clone());
                            s.set(&["systems", &id, "romDirs"], json!(dirs));
                        })),
                        step: None,
                    });
                }
                let what = Pick::Folder(id.clone());
                rows.push(Row {
                    label: "Add a folder…".into(),
                    hint: "Opens a folder dialog".into(),
                    press: Some(Box::new(move |s: &mut Settings| s.pick(what.clone(), "Choose a games folder", true))),
                    ..Row::default()
                });
                Page { title: self.console(id).map_or(id.clone(), |c| format!("{} games folders", c.name)), lead: String::new(), rows }
            }
            Screen::Pc => {
                let steam = self.text(&["steam", "path"]);
                let games = self.text(&["pc", "gamesDir"]);
                let mut rows = vec![
                    toggle("Steam", &["steam", "enabled"], true, "Your installed Steam games on the PC shelf"),
                    Row {
                        label: "Steam folder".into(),
                        value: if steam.is_empty() { "Found by itself".into() } else { short(&steam) },
                        hint: "Enter chooses the folder; RustyShelf looks in the usual places when none is set".into(),
                        press: Some(Box::new(|s: &mut Settings| s.pick(Pick::Steam, "Choose Steam's folder", true))),
                        step: None,
                    },
                    toggle("Steam's tools", &["steam", "hideTools"], true, "On: Proton, runtimes and the like stay off the shelf"),
                    toggle("Epic Games", &["pc", "epic"], true, ""),
                    toggle("GOG Galaxy", &["pc", "gog"], true, ""),
                    toggle("Ubisoft Connect", &["pc", "ubisoft"], true, ""),
                    toggle("EA app", &["pc", "ea"], true, ""),
                    toggle("Xbox / Game Pass", &["pc", "xbox"], true, ""),
                    toggle("HoYoPlay", &["pc", "hoyoplay"], true, ""),
                    toggle("Quiver Launcher", &["pc", "quiver"], true, "Its apps go on the Custom shelf"),
                    Row {
                        label: "PC games folder".into(),
                        value: if games.is_empty() { "None".into() } else { short(&games) },
                        hint: "Every folder in it is a game  ·  Enter chooses it, ◀ forgets it".into(),
                        press: Some(Box::new(|s: &mut Settings| s.pick(Pick::GamesDir, "Choose your PC games folder", true))),
                        step: Some(Box::new(|s: &mut Settings, d| {
                            if d < 0 {
                                s.set(&["pc", "gamesDir"], json!(""));
                            }
                        })),
                    },
                    toggle("Console box art", &["pc", "consoleBox"], false, "A PC game in the box of its console release, where there is one"),
                ];
                for a in &systems().apps {
                    let (id, id2) = (a.id.clone(), a.id.clone());
                    let there = !self.text(&["apps", &a.id, "path"]).is_empty();
                    rows.push(Row {
                        label: a.name.clone(),
                        value: if there { on_off(self.flag(&["apps", &a.id, "enabled"], true)) } else { "Not found".into() },
                        hint: "On the Media shelf".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let on = s.flag(&["apps", &id, "enabled"], true);
                            s.set(&["apps", &id, "enabled"], json!(!on));
                        })),
                        step: Some(Box::new(move |s: &mut Settings, _| {
                            let on = s.flag(&["apps", &id2, "enabled"], true);
                            s.set(&["apps", &id2, "enabled"], json!(!on));
                        })),
                    });
                }
                if self.wizard {
                    rows.push(next(Screen::BoxArt));
                }
                Page { title: "PC games and apps".into(), lead: "The PC shelf holds your Steam games and the other stores'. A store that isn't installed simply adds nothing.".into(), rows }
            }
            Screen::BoxArt => {
                let lib = self.text(&["ui", "artLibrary"]);
                const REGIONS: [(&str, &str); 4] = [("auto", "Each game's own"), ("usa", "USA"), ("eur", "Europe"), ("jpn", "Japan")];
                let region = self.text(&["ui", "artRegion"]);
                let at = REGIONS.iter().position(|r| r.0 == region).unwrap_or(0);
                let turn = move |s: &mut Settings, d: i32| {
                    let n = (at as i32 + d).rem_euclid(REGIONS.len() as i32) as usize;
                    s.set(&["ui", "artRegion"], json!(REGIONS[n].0));
                };
                let mut rows = vec![
                    toggle("Download box art", &["ui", "autoCovers"], false, "Covers come from the libretro thumbnail library and the LaunchBox Games Database; game names are sent to them"),
                    Row { label: "Region".into(), value: REGIONS[at].1.into(), hint: "Which region's box comes first".into(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(turn)) },
                    Row {
                        label: "Art library".into(),
                        value: if lib.is_empty() { "Beside the games".into() } else { short(&lib) },
                        hint: "One folder for every console's art  ·  Enter chooses it, ◀ goes back to beside the games".into(),
                        press: Some(Box::new(|s: &mut Settings| s.pick(Pick::ArtLibrary, "Choose the art library's folder", true))),
                        step: Some(Box::new(|s: &mut Settings, d| {
                            if d < 0 {
                                s.set(&["ui", "artLibrary"], json!(""));
                            }
                        })),
                    },
                ];
                if self.wizard {
                    rows.push(next(Screen::Done));
                }
                Page { title: "Box art".into(), lead: "RustyShelf can fetch the covers your games don't have yet.".into(), rows }
            }
            Screen::Display => {
                Page { title: "Display".into(), lead: String::new(), rows: vec![toggle("Start in fullscreen", &["ui", "startFullscreen"], true, "From the next start")] }
            }
            Screen::Controller => {
                const COMBOS: [(&str, &str); 4] = [("l3+r3", "L3 + R3"), ("lb+rb+start", "LB + RB + Start"), ("back+b", "Back + B"), ("start+back", "Start + Back")];
                let keys = self.text(&["ui", "quitComboKeys"]);
                let at = COMBOS.iter().position(|c| c.0 == keys).unwrap_or(0);
                let turn = move |s: &mut Settings, d: i32| {
                    let n = (at as i32 + d).rem_euclid(COMBOS.len() as i32) as usize;
                    s.set(&["ui", "quitComboKeys"], json!(COMBOS[n].0));
                };
                Page {
                    title: "Controller".into(),
                    lead: String::new(),
                    rows: vec![
                        toggle("Quit combo", &["ui", "quitCombo"], true, "Held on the pad, it closes the game and brings the shelf back"),
                        Row { label: "Buttons".into(), value: COMBOS[at].1.into(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(turn)), ..Row::default() },
                    ],
                }
            }
            Screen::System => Page {
                title: "System".into(),
                lead: format!("RustyShelf {}  ·  {}", env!("CARGO_PKG_VERSION"), short(&self.app_root.to_string_lossy())),
                rows: vec![
                    Row {
                        label: if self.looking.is_some() { "Looking…" } else { "Find emulators and games again" }.into(),
                        hint: "Fills in what isn't set yet; nothing you chose is changed".into(),
                        press: Some(Box::new(|s: &mut Settings| s.look())),
                        ..Row::default()
                    },
                    Row {
                        label: "Restore emulator settings".into(),
                        hint: "Puts back the emulators' own settings files from before RustyShelf changed them".into(),
                        press: Some(Box::new(|s: &mut Settings| {
                            let emus: Vec<(String, String)> = systems().consoles.iter().map(|c| (c.id.clone(), paths::expand(&s.text(&["systems", &c.id, "emulator"]), &s.app_root))).collect();
                            let n = emucfg::restore_backups(&emus, &paths::env_var).len();
                            s.note = if n == 0 { "Nothing to put back.".into() } else { format!("{n} settings file{} put back.", if n == 1 { "" } else { "s" }) };
                            s.redraw = true;
                        })),
                        ..Row::default()
                    },
                ],
            },
            Screen::Done => {
                let on = systems().consoles.iter().filter(|c| self.flag(&["systems", &c.id, "enabled"], true) && !self.list(&["systems", &c.id, "romDirs"]).is_empty()).count();
                Page {
                    title: "You're set".into(),
                    lead: format!("{on} console{} with games. Everything here can be changed later in Settings (F1, or Start on the pad).", if on == 1 { "" } else { "s" }),
                    rows: vec![Row {
                        label: "Open RustyShelf".into(),
                        press: Some(Box::new(|s: &mut Settings| {
                            s.set(&["setupDone"], json!(true));
                            s.wizard = false;
                            s.open = false;
                            s.stack = vec![(Screen::Root, 0)];
                        })),
                        ..Row::default()
                    }],
                }
            }
        }
    }
}

#[derive(Component)]
pub struct SettingsUi;

/// The keys and the pad while Settings is open; F1 or Start opens it from the shelf.
pub fn input(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    time: Res<Time>,
    mut s: ResMut<Settings>,
    mut lib: ResMut<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    playing: Res<crate::play::Playing>,
) {
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    if !s.open {
        if s.just_closed {
            s.just_closed = false;
        }
        if playing.title.is_none() && (keys.just_pressed(KeyCode::F1) || pad(GamepadButton::Start)) {
            s.open = true;
            s.changed = false;
            s.stack = vec![(Screen::Root, 0)];
            s.redraw = true;
        }
        return;
    }

    // what came back from a search or a dialog
    let found = s.looking.as_ref().map(|rx| rx.lock().unwrap().try_recv());
    match found {
        Some(Ok(d)) => {
            s.looking = None;
            s.apply_found(&d);
            s.found = Some(d);
            if s.stack.last().is_some_and(|p| p.0 == Screen::Welcome) {
                s.go(Screen::Consoles);
            }
            s.redraw = true;
        }
        Some(Err(TryRecvError::Disconnected)) => {
            s.looking = None;
            s.redraw = true;
        }
        _ => {}
    }
    let picked = s.picking.as_ref().map(|(_, rx)| rx.lock().unwrap().try_recv());
    match picked {
        Some(Ok(path)) => {
            let (what, _) = s.picking.take().expect("a dialog");
            if let Some(p) = path {
                s.picked(what, &p);
            }
            s.redraw = true;
        }
        Some(Err(TryRecvError::Disconnected)) => s.picking = None,
        _ => {}
    }
    // a dialog is up: the page waits for it
    if s.picking.is_some() {
        return;
    }

    let page = s.page();
    let n = page.rows.len();
    let now = time.elapsed_secs();
    let held = {
        let k = keys.pressed(KeyCode::ArrowDown) as i32 - keys.pressed(KeyCode::ArrowUp) as i32;
        let p = pads.iter().map(|p| { let v = -(p.left_stick().y + p.dpad().y); if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 } }).find(|&d| d != 0).unwrap_or(0);
        if k != 0 { k } else { p }
    };
    let mut move_by = 0;
    if held != s.repeat.0 {
        s.repeat = (held, now + 0.35);
        move_by = held;
    } else if held != 0 && now >= s.repeat.1 {
        s.repeat.1 = now + 0.08;
        move_by = held;
    }
    let jump = [(KeyCode::Home, -(n as i32)), (KeyCode::End, n as i32), (KeyCode::PageUp, -(WINDOW as i32)), (KeyCode::PageDown, WINDOW as i32)];
    if let Some((_, by)) = jump.iter().find(|(k, _)| keys.just_pressed(*k)) {
        move_by = *by;
    }
    if move_by != 0 && n > 0 {
        let at = &mut s.stack.last_mut().expect("a page").1;
        *at = (*at as i32 + move_by).clamp(0, n as i32 - 1) as usize;
        s.redraw = true;
    }
    let at = s.stack.last().map_or(0, |p| p.1).min(n.saturating_sub(1));
    let side = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
    if side != 0 {
        if let Some(step) = page.rows.get(at).and_then(|r| r.step.as_ref()) {
            step(&mut s, side);
        }
    } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(GamepadButton::South) {
        if let Some(press) = page.rows.get(at).and_then(|r| r.press.as_ref()) {
            press(&mut s);
        }
    } else if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::East) || (!s.wizard && (keys.just_pressed(KeyCode::F1) || pad(GamepadButton::Start))) {
        s.back();
    }

    if !s.open {
        s.just_closed = true;
    }
    // closed with something changed: the shelves are read again
    if !s.open && s.changed {
        s.changed = false;
        *lib = loading::start_library();
        *cursor = Cursor::default();
    }
}

/// Draws the page again when it changed.
pub fn draw(mut commands: Commands, mut s: ResMut<Settings>, face: Option<Res<Face>>, old: Query<Entity, With<SettingsUi>>) {
    if !s.redraw {
        return;
    }
    let Some(face) = face else { return };
    s.redraw = false;
    for e in &old {
        commands.entity(e).despawn();
    }
    if !s.open {
        return;
    }
    let page = s.page();
    let n = page.rows.len();
    let at = s.stack.last().map_or(0, |p| p.1).min(n.saturating_sub(1));
    let first = if n <= WINDOW { 0 } else { at.saturating_sub(WINDOW / 2).min(n - WINDOW) };
    let font = |size: f32| TextFont::from_font_size(FontSize::VMin(size)).with_font(face.0.clone());
    let hint = if s.note.is_empty() { page.rows.get(at).map(|r| r.hint.clone()).unwrap_or_default() } else { s.note.clone() };
    let keys = if s.wizard { "▲ ▼  Move      ◀ ▶  Change      Enter · A  Choose      Esc · B  Back" } else { "▲ ▼  Move      ◀ ▶  Change      Enter · A  Choose      Esc · B  Back      F1 · Start  Close" };
    commands
        .spawn((
            SettingsUi,
            Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, padding: UiRect::axes(vmin(8.0), vmin(6.0)), ..default() },
            BackgroundColor(PANEL),
            GlobalZIndex(10),
        ))
        .with_children(|p| {
            p.spawn((Text::new(page.title.clone()), font(4.4), TextColor(STEEL)));
            p.spawn((Node { margin: UiRect::vertical(vmin(1.2)), max_width: percent(80), ..default() }, Text::new(page.lead.clone()), font(2.1), TextColor(DIM)));
            if first > 0 {
                p.spawn((Text::new("▲"), font(1.6), TextColor(DIM)));
            }
            for (i, row) in page.rows.iter().enumerate().skip(first).take(WINDOW) {
                let chosen = i == at;
                p.spawn((
                    Node { width: percent(100), flex_direction: FlexDirection::Row, justify_content: JustifyContent::SpaceBetween, padding: UiRect::axes(vmin(1.6), vmin(0.9)), margin: UiRect::vertical(vmin(0.15)), ..default() },
                    BackgroundColor(if chosen { CHOSEN } else { Color::NONE }),
                ))
                .with_children(|r| {
                    r.spawn((Text::new(row.label.clone()), font(2.5), TextColor(if chosen { STEEL } else { DIM })));
                    r.spawn((Text::new(row.value.clone()), font(2.2), TextColor(if chosen { RUST } else { DIM })));
                });
            }
            if first + WINDOW < n {
                p.spawn((Text::new("▼"), font(1.6), TextColor(DIM)));
            }
            p.spawn(Node { flex_grow: 1.0, ..default() });
            p.spawn((Text::new(hint), font(2.0), TextColor(STEEL)));
            p.spawn((Node { margin: UiRect::top(vmin(1.5)), ..default() }, Text::new(keys), font(1.8), TextColor(DIM)));
        });
}

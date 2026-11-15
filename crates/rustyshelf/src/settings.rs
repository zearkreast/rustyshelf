// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the pages and their wording
// follow Boxshelf's renderer/settings.js and renderer/setup.js, as RustyShelf has them.
//! The first run and Settings: pages of rows, moved through with the keys or a pad. What's chosen is saved to
//! `data/config.json` at once, by the rules the Electron app's settings go through.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::text::FontSize;
use rustyshelf_core::config::Config;
use rustyshelf_core::detect::{Detected, Detector};
use rustyshelf_core::history::Played;
use rustyshelf_core::homemenu::Glyphs;
use rustyshelf_core::library::ShelfKind;
use rustyshelf_core::scanner::{Game, Source};
use rustyshelf_core::shelfview::{self, SORTS};
use rustyshelf_core::systems::{systems, Console};
use rustyshelf_core::{emucfg, emuname, paths};
use serde_json::{json, Value};

use crate::hud::Face;
use crate::loading::{self, Base, LibraryRes, LibraryState};
use crate::shelf::Cursor;

mod accounts;
mod addgames;
mod art;
mod collections;
mod custom;
mod find;
mod kid;
pub use kid::kid_hidden;
pub use notes::notes;
mod manuals;
mod moving;
mod notes;
mod mstudio;
mod linuxpc;
mod phone;
mod romm;
pub mod setup;
mod storage;
mod text;
mod trophies;
mod update;
mod wiimote;


const STEEL: Color = Color::srgb(0.87, 0.89, 0.92);
const RUST: Color = Color::srgb(0.83, 0.32, 0.17);
const DIM: Color = Color::srgb(0.55, 0.58, 0.62);
const PANEL: Color = Color::srgb(0.05, 0.05, 0.07);
const CHOSEN: Color = Color::srgba(0.83, 0.32, 0.17, 0.28);
const KEY: Color = Color::srgb(0.13, 0.13, 0.17);

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

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
    /// a console's emulator on its own, its launch options and startup oddities
    Advanced(String),
    /// Set up for me, for a console's emulator
    EmuSetup(String),
    Pc,
    Custom,
    /// one game added by hand, by its place in the list
    CustomGame(usize),
    /// the games found to add, to be ticked; and what a Playnite library comes to
    AddGames,
    Playnite,
    /// the manuals found for the chosen game
    Manuals,
    ManualStudio,
    BoxArt,
    Display,
    Sound,
    Controller,
    System,
    Done,
    /// the shelf's menu: its order, what's shown, the chosen game
    Shelf,
    Details,
    Hidden,
    /// the chosen game's collections, and all of them
    GameCollections,
    Collections,
    /// one of your collections, by its id: search for games to put on it
    MyShelf(String),
    Find,
    Accounts,
    Achievements,
    /// RetroAchievements and Steam achievements, the whole account
    Trophies,
    Phone,
    Romm,
    /// (Linux) the Epic account's games, and the downloads
    EpicLibrary,
    EpicDownloads,
    WhatsNew,
    /// the room the PC games take, by drive
    Storage,
    /// which drives a search looks through as well as the usual places (Windows)
    Drives,
    /// the chosen game's box art, part by part; and one part's versions
    Art,
    ArtPart,
    ArtSets,
    /// Settings → System → Kid mode
    Kid,
    /// Settings → Sounds → Sound per console
    SoundConsoles,
    /// Settings → Sounds → Custom sounds
    SoundPacks,
}

/// The game the shelf's menu or Details was opened on.
struct Chosen {
    /// its own console's shelf, and the emulator that shelf names
    console: String,
    emulator: String,
    game: Game,
    plays: Option<Played>,
}

type Press = Box<dyn Fn(&mut Settings) + Send + Sync>;

/// Another RustyShelf's finished setup: the one that ran last (installs.json), else one in a folder near this one,
/// in Downloads, on the Desktop or in Documents.
fn other_setup(app_root: &Path) -> Option<rustyshelf_core::keepsetup::Install> {
    use rustyshelf_core::keepsetup;
    let data = paths::data_dir(app_root);
    keepsetup::registry_file(&paths::env_var).and_then(|f| keepsetup::find_other(&f, &data)).or_else(|| {
        let home = PathBuf::from(paths::env_var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default());
        let roots: Vec<PathBuf> = app_root.parent().map(Path::to_path_buf).into_iter().chain(["Downloads", "Desktop", "Documents"].map(|d| home.join(d))).collect();
        keepsetup::find_nearby(&data, &roots)
    })
}

/// Starts RustyShelf again (a change waiting in data\.pending goes in as it starts).
fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn();
    }
    std::process::exit(0);
}

/// Settings' tabs, by what you came to do.
const TABS: [&str; 8] = ["Library", "Look", "Sound", "Box art & media", "Controls", "Shelf", "Accounts", "System"];

/// The interface's voices, by `ui.soundTheme` and name.
const VOICES: [(&str, &str); 6] = [("aero", "Aero"), ("melody", "Melody"), ("musicbox", "Music box"), ("disc", "Disc-era"), ("handheld", "Handheld"), ("living", "Living room")];

/// The languages there are, each by its own name (never translated).
const LANGUAGES: [(&str, &str); 4] = [("en", "English"), ("tr", "Türkçe"), ("fr", "Français"), ("pt", "Português (Brasil)")];
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
    PortsDir,
    ArtLibrary,
    ArtDir(String),
    CustomGame,
    ExportTo,
    ImportFrom,
    ScanGames,
    /// a folder of emulators downloaded and unzipped by hand: every console missing one gets the one found there
    EmulatorsIn,
    Playnite,
    ThemePicture,
    /// a picture of the player's own for a part of a game's box (true: the spine is cut from it)
    OwnArt(bool),
    /// a PDF of the player's own as the chosen game's manual
    OwnManual,
    /// RocketLauncher.exe, when it wasn't found
    Rocket,
    /// a folder with a folder of games per console (RetroBat's roms)
    RomFoldersIn,
    /// your own sounds: a folder, a sound file or a ZIP of them
    SoundPack,
}

#[derive(Resource)]
pub struct Settings {
    pub open: bool,
    /// the Kid mode PIN was given since Settings opened
    kid_open: bool,
    /// Settings' tab (TABS)
    tab: usize,
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
    /// the drives a search can look through, and whether each is ticked
    drives: Vec<(rustyshelf_core::detect::Drive, bool)>,
    picking: Option<(Pick, Mutex<Receiver<Option<PathBuf>>>)>,
    redraw: bool,
    /// something was saved since Settings opened: the shelves are read again when it closes
    changed: bool,
    /// only what's shown changed (the order, the filter, a favourite, a hidden game): the shelves are arranged again,
    /// not read again
    rearranged: bool,
    /// Details' Play was chosen: the shelf starts the game under its cursor
    pub play: bool,
    chosen: Option<Chosen>,
    /// the filters this library can use, and every game's title by its id (for the hidden ones)
    filters: Vec<(String, String)>,
    titles: std::collections::HashMap<String, String>,
    /// what's typed in a collection's Search games
    shelf_q: String,
    note: String,
    repeat: (i32, f32),
    editing: Option<text::Editing>,
    /// what a first press asked to be pressed again for (removing a game, undoing a setup)
    asked: String,
    setup: setup::SetupState,
    /// the graphics card's vendor, for the graphics preset "For this PC"
    pub gpu_vendor: Option<u32>,
    /// a console whose emulator a page asked to open on its own
    open_emulator: Option<String>,
    /// the page the shelf asked to be opened at
    wanted: Option<Screen>,
    /// the Home menu or the manual reader is up: the keys and the pads are its own
    pub keys_taken: bool,
    /// the prompts of the controller in use
    pub pad_auto: Glyphs,
    /// a manual to be opened in the reader: its pages
    pub manual: Option<Vec<PathBuf>>,
    /// `manual` is a game's screenshots (their addresses), shown one at a time
    pub gallery: bool,
    /// the phone companion, and its pairing code as it was last drawn: what it says, and its picture
    pub phone: Option<std::sync::Arc<rustyshelf_core::companion::Companion>>,
    qr: Option<(String, Handle<Image>)>,
    moves: moving::Moves,
    update: update::Update,
    adding: addgames::Adding,
    manual_search: manuals::ManualSearch,
    mstudio: mstudio::MStudio,
    trophies: trophies::Trophies,
    /// the first run's console list shows every console, not just the ones found
    every_console: bool,
    /// the genres the games have and how many of each, for Add a tag shelf (read when Settings opens)
    genres: Vec<(String, usize)>,
    /// the console shelves there are, by id and name (Sound per console)
    console_shelves: Vec<(String, String)>,
    /// your own sounds, read when a sound page opens
    packs: Vec<rustyshelf_core::soundpacks::Pack>,
    storage: storage::Storage,
    art_pick: art::ArtPick,
    finding: find::Finding,
    ra: std::sync::Arc<rustyshelf_core::ra::Ra>,
    ra_answer: Option<Mutex<Receiver<accounts::RaAnswer>>>,
    /// IGDB's info for the chosen game, asked when Details opened with gaps in it: the game's id and its info filled
    igdb_answer: Option<Mutex<Receiver<(String, rustyshelf_core::info::Info)>>>,
    /// the achievements last asked for: the game's id, and what came back
    achievements: Option<(String, Result<Option<Value>, String>)>,
    /// the achievements' order: 0 the set's (story first), 1 rarity, 2 points
    ach_sort: usize,
    /// the key the Electron app seals API keys with on this computer
    seal_key: Option<[u8; 32]>,
    /// the custom themes found, by id and name; and whether they're to be looked for again
    pub custom_themes: Vec<(String, String)>,
    pub reload_themes: bool,
    /// the game Find a game was sent to: its shelf and its id
    go_to: Option<(String, String)>,
    /// something to say on the shelf once Settings has closed
    toast: Option<String>,
    /// another RustyShelf's finished setup this new one could start from (the first run only)
    other_setup: Option<rustyshelf_core::keepsetup::Install>,
    /// a setup being brought over: how far it is, and how it ends
    bringing: Option<(std::sync::Arc<Mutex<String>>, Mutex<Receiver<Result<(), String>>>)>,
    /// the Discord switch or id changed since they were last handed over
    pub discord_changed: bool,
    /// the same for the sound settings
    pub sound_changed: bool,
    /// cues to play: a cue's name, and which way a move went
    pub cues: Vec<(&'static str, i32)>,
    /// the Songs folder, or the Music folder, was asked to be read again
    pub reload_songs: bool,
    pub reload_music: bool,
    romm: romm::RommUi,
    epic: linuxpc::EpicUi,
}

/// Settings → Sounds.
pub struct SoundPrefs {
    pub on: bool,
    pub volume: f32,
    pub voice: String,
    pub boot: String,
    pub boot_volume: f32,
    /// the menu music's track ("off", "own" or a track's name), and its level
    pub music: String,
    pub music_level: f32,
    /// with the Melody voice, the built-in songs play along with your own
    pub built_in_songs: bool,
    /// the consoles given a voice of their own (`ui.soundByShelf`), by shelf id
    pub by_shelf: std::collections::HashMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        let app_root = paths::app_root();
        let cfg = Config::load(&paths::data_dir(&app_root)).unwrap_or_default();
        let wizard = !cfg.setup_done();
        let other = if wizard { other_setup(&app_root) } else { None };
        Settings {
            open: wizard,
            just_closed: false,
            wizard,
            cfg: Value::Object(cfg.raw().clone()),
            app_root,
            stack: vec![(if wizard { Screen::Welcome } else { Screen::Root }, 0)],
            found: None,
            looking: None,
            drives: Vec::new(),
            picking: None,
            redraw: true,
            changed: false,
            rearranged: false,
            play: false,
            chosen: None,
            filters: Vec::new(),
            titles: std::collections::HashMap::new(),
            shelf_q: String::new(),
            note: String::new(),
            repeat: (0, 0.0),
            editing: None,
            asked: String::new(),
            setup: setup::SetupState::default(),
            gpu_vendor: None,
            open_emulator: None,
            wanted: None,
            keys_taken: false,
            pad_auto: Glyphs::Xbox,
            manual: None,
            gallery: false,
            phone: None,
            qr: None,
            moves: Default::default(),
            update: Default::default(),
            adding: Default::default(),
            manual_search: Default::default(),
            mstudio: Default::default(),
            trophies: Default::default(),
            every_console: false,
            genres: Vec::new(),
            console_shelves: Vec::new(),
            packs: Vec::new(),
            storage: Default::default(),
            art_pick: Default::default(),
            kid_open: false,
            tab: 0,
            finding: Default::default(),
            ra: std::sync::Arc::new(rustyshelf_core::ra::Ra::new(&paths::data_dir(&paths::app_root()))),
            ra_answer: None,
            igdb_answer: None,
            achievements: None,
            ach_sort: 0,
            seal_key: rustyshelf_core::seal::key(&paths::env_var),
            custom_themes: Vec::new(),
            reload_themes: false,
            go_to: None,
            toast: rustyshelf_core::keepsetup::take_note(),
            other_setup: other,
            bringing: None,
            discord_changed: true,
            sound_changed: true,
            cues: Vec::new(),
            reload_songs: false,
            reload_music: false,
            romm: Default::default(),
            epic: Default::default(),
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

    /// Says something under the page that's open.
    pub fn say(&mut self, text: String) {
        self.note = text;
        self.redraw = true;
    }

    pub fn sound(&self) -> SoundPrefs {
        let number = |path: &[&str], unset: f32| at(&self.cfg, path).and_then(Value::as_f64).map_or(unset, |v| v as f32).clamp(0.0, 1.0);
        SoundPrefs {
            on: self.flag(&["ui", "sound"], true),
            volume: number(&["ui", "volume"], 0.7),
            voice: self.text(&["ui", "soundTheme"]),
            boot: Some(self.text(&["ui", "bootSound"])).filter(|b| !b.is_empty()).unwrap_or_else(|| "chime".into()),
            boot_volume: number(&["ui", "bootVolume"], 1.0),
            music: Some(self.text(&["ui", "music"])).filter(|m| !m.is_empty()).unwrap_or_else(|| "off".into()),
            music_level: number(&["ui", "musicLevel"], 0.4),
            built_in_songs: self.flag(&["ui", "builtInSongs"], true),
            by_shelf: self.cfg["ui"]["soundByShelf"].as_object().map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str().filter(|v| !v.is_empty())?.to_string()))).collect()).unwrap_or_default(),
        }
    }

    /// The sounds there are to choose from: the built-in voices, then your custom sounds; by id and name.
    fn sound_list(&self) -> Vec<(String, String)> {
        VOICES.iter().map(|(id, name)| (id.to_string(), name.to_string())).chain(self.packs.iter().map(|p| (p.sound_id(), p.name.clone()))).collect()
    }

    /// A PC game's window mode (Details → Window): "", "borderless" or "windowed".
    pub fn window_mode(&self, id: &str) -> &'static str {
        let mode = self.text(&["games", id, "window"]);
        rustyshelf_core::windowmode::MODES.into_iter().find(|m| *m == mode).unwrap_or("")
    }

    /// The clock in the corner: whether the day is shown with the time, and whether the time counts 24 hours;
    /// None when it's off.
    pub fn clock(&self) -> Option<(bool, bool)> {
        let mode = self.text(&["ui", "clock"]);
        (mode != "off").then(|| (mode != "time", self.flag(&["ui", "clock24"], false)))
    }

    /// Asks for a console's page (its emulator and games folders), from its shelf's "Add games" case.
    pub fn open_console(&mut self, id: &str) {
        self.wanted = Some(Screen::Console(id.to_string()));
    }

    /// Whose letters the buttons carry on screen: the ones chosen, else those of the controller in use.
    pub fn glyphs(&self) -> Glyphs {
        match self.text(&["ui", "buttons"]).as_str() {
            "ps" => Glyphs::Ps,
            "nintendo" => Glyphs::Nintendo,
            "xbox" => Glyphs::Xbox,
            _ => self.pad_auto,
        }
    }

    /// Whether the right face button confirms and the bottom one goes back: chosen, else as a Switch pad has it.
    pub fn swap(&self) -> bool {
        let confirm = self.text(&["ui", "confirmButton"]);
        confirm == "b" || (confirm != "a" && self.glyphs() == Glyphs::Nintendo)
    }

    /// The pad's button that confirms, and the one that goes back.
    pub fn confirm(&self) -> (GamepadButton, GamepadButton) {
        if self.swap() { (GamepadButton::East, GamepadButton::South) } else { (GamepadButton::South, GamepadButton::East) }
    }

    /// A button of the hint bars as the controller in use prints it (`pads::cap`'s keys).
    pub fn cap(&self, key: &'static str) -> &'static str {
        rustyshelf_core::pads::cap(key, self.glyphs(), self.swap())
    }

    /// The Home menu: whether it's on, whether it's the centred card, whether the right face button confirms,
    /// and whose letters the buttons carry.
    /// Attract mode: whether it's on, the idle minutes before it starts, each box's stay in seconds, and whether it
    /// wanders to other shelves; then the idle minutes before the screen dims (0: never).
    pub fn idle(&self) -> (bool, f64, f32, bool, f64) {
        let mins = at(&self.cfg, &["ui", "attractMinutes"]).and_then(Value::as_f64).filter(|m| *m >= 1.0).unwrap_or(3.0);
        // keep in sync with renderer/app.js ATTRACT_MS
        let stay = match self.text(&["ui", "attractPace"]).as_str() {
            "slow" => 7.0,
            "fast" => 2.5,
            _ => 4.2,
        };
        let dim = at(&self.cfg, &["ui", "dimMinutes"]).and_then(Value::as_f64).unwrap_or(0.0);
        (self.flag(&["ui", "attract"], true), mins, stay, self.flag(&["ui", "attractWander"], true), dim)
    }

    pub fn home_menu(&self) -> (bool, bool, bool, Glyphs) {
        (self.flag(&["ui", "homeMenu"], true), self.text(&["ui", "homeLayout"]) == "center", self.swap(), self.glyphs())
    }

    /// The games taken off their shelves, by id.
    pub fn hidden(&self) -> Vec<String> {
        self.list(&["ui", "hidden"])
    }

    /// Whether what's typed now goes into a text: one being typed, or Find a game's title.
    pub fn takes_text(&self) -> bool {
        self.open && (self.editing.is_some() || self.stack.last().is_some_and(|p| p.0 == Screen::Find))
    }

    /// Closes Settings from outside, as backing out of every page would; false when it can't be left now (the
    /// first run's pages, a file dialog that's up).
    pub fn shut(&mut self) -> bool {
        if self.wizard || self.picking.is_some() {
            return false;
        }
        (self.editing, self.open, self.just_closed, self.redraw) = (None, false, true, true);
        self.stack.truncate(1);
        true
    }

    /// Asks for Settings' first page, from the Home menu.
    pub fn open_root(&mut self) {
        self.wanted = Some(Screen::Root);
    }

    /// Asks for the Custom games page, from the Custom shelf's "Add games" case.
    pub fn open_custom(&mut self) {
        self.wanted = Some(Screen::Custom);
    }

    /// Asks for a console's emulator to be opened on its own, from its shelf's "Open" case.
    pub fn open_emulator(&mut self, id: &str) {
        self.open_emulator = Some(id.to_string());
    }

    /// Puts a game among the favourites or takes it out, from the shelf; whether it's one now.
    pub fn toggle_favorite(&mut self, id: &str) -> bool {
        self.flip("favorites", id);
        self.list(&["ui", "favorites"]).iter().any(|x| x == id)
    }

    /// The shelf's colour theme.
    pub fn theme(&self) -> String {
        let saved = self.text(&["ui", "theme"]);
        if saved.starts_with("custom-") {
            saved
        } else {
            rustyshelf_core::themes::id_of(&saved).to_string()
        }
    }

    /// How much larger than usual the cases stand: Settings' size for every shelf, by the shelf's own.
    pub fn box_scale(&self, shelf: &str) -> f32 {
        let number = |path: &[&str]| at(&self.cfg, path).and_then(Value::as_f64).filter(|v| *v > 0.0).map_or(1.0, |v| v as f32);
        number(&["ui", "boxScale"]) * number(&["ui", "shelfScale", shelf])
    }

    /// Whether what's played is shown on Discord, and the application id it's shown under.
    pub fn discord(&self) -> (bool, String) {
        (self.flag(&["ui", "discordPresence"], false), self.text(&["ui", "discordAppId"]))
    }

    fn save(&mut self) {
        let Some(cfg) = Config::from_value(&self.cfg) else { return };
        self.cfg = Value::Object(cfg.raw().clone());
        if let Err(e) = cfg.save(&paths::data_dir(&self.app_root)) {
            self.note = format!("The settings couldn't be saved ({e}).");
        }
        self.redraw = true;
    }

    fn set(&mut self, path: &[&str], x: Value) {
        put(&mut self.cfg, path, x);
        // what's shown of the shelves, not what's on them
        if matches!(path, ["ui", "sort" | "filter" | "favorites" | "hidden" | "customSort" | "allShelf" | "collections" | "shelfAdds"] | ["ui", "autoShelves", ..] | ["games", _, "status"]) {
            self.rearranged = true;
        } else if matches!(path, ["ui", "discordPresence" | "discordAppId"]) {
            self.discord_changed = true;
        } else if matches!(path, ["ui", "music" | "musicLevel" | "theme" | "clock" | "clock24"]) || path[0] == "ra" {
            self.redraw = true;
            // the music and the shelf read these as they go
        } else if matches!(path, ["ui", "builtInSongs"]) {
            self.sound_changed = true;
        } else if let ["ui", key @ ("sound" | "volume" | "soundTheme" | "soundByShelf" | "bootSound" | "bootVolume")] = path {
            // heard at once: the boot for its own settings, a chime for the rest
            self.sound_changed = true;
            self.cues.push((if key.starts_with("boot") { "boot" } else { "open" }, 1));
        } else if path[0] != "emuSetup" && !matches!(path, ["games", _, "window"]) {
            self.changed = true;
        }
        self.save();
    }

    /// Adds an id to one of the settings' lists of game ids, or takes it out.
    fn flip(&mut self, list: &'static str, id: &str) {
        let mut ids = self.list(&["ui", list]);
        match ids.iter().position(|x| x == id) {
            Some(i) => {
                ids.remove(i);
            }
            None => ids.push(id.to_string()),
        }
        self.set(&["ui", list], json!(ids));
    }

    fn toggle(&mut self, path: &[&str], default: bool) {
        let on = self.flag(path, default);
        self.set(path, json!(!on));
    }

    fn go(&mut self, screen: Screen) {
        if let Screen::EmuSetup(id) = &screen {
            self.load_setup(id);
        }
        if matches!(screen, Screen::Sound | Screen::SoundConsoles | Screen::SoundPacks | Screen::Root) {
            self.packs = rustyshelf_core::soundpacks::list(&self.app_root);
        }
        if screen == Screen::Storage {
            self.load_storage(false);
        }
        if screen == Screen::Details {
            self.load_achievements(false);
            self.load_igdb();
        }
        self.stack.push((screen, 0));
        self.cues.push(("open", 1));
        self.note.clear();
        self.asked.clear();
        self.redraw = true;
    }

    /// True on the second press in a row of something that asks twice; the first says so.
    fn twice(&mut self, key: &str, ask: &str) -> bool {
        if self.asked == key {
            self.asked.clear();
            return true;
        }
        self.asked = key.to_string();
        self.note = ask.to_string();
        self.redraw = true;
        false
    }

    /// A row that switches a setting on and off.
    fn switch(&self, label: &str, path: &[&str], default: bool, hint: &str) -> Row {
        let (a, b): (Vec<String>, Vec<String>) = (path.iter().map(|p| p.to_string()).collect(), path.iter().map(|p| p.to_string()).collect());
        let flip = move |s: &mut Settings, p: &[String]| s.toggle(&p.iter().map(String::as_str).collect::<Vec<_>>(), default);
        Row {
            label: label.to_string(),
            value: on_off(self.flag(path, default)),
            hint: hint.to_string(),
            press: Some(Box::new(move |s: &mut Settings| flip(s, &a))),
            step: Some(Box::new(move |s: &mut Settings, _| flip(s, &b))),
        }
    }

    /// A row whose text is typed.
    /// RocketLauncher is found and on for every console or the chosen ones.
    fn rocket_on(&self) -> bool {
        cfg!(windows) && !self.text(&["rocket", "exe"]).is_empty() && !matches!(self.text(&["rocket", "mode"]).as_str(), "" | "off")
    }

    /// Settings → Consoles: RocketLauncher, HyperSpin's launcher: found (or chosen), then on for every console or the
    /// consoles chosen on their own pages, and the front end it's told.
    fn rocket_rows(&self) -> Vec<Row> {
        let exe = self.text(&["rocket", "exe"]);
        let mut rows = vec![Row {
            label: "RocketLauncher".into(),
            value: if exe.is_empty() { "Find".into() } else { format!("Found  ·  {}", short(Path::new(&exe).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default().as_str())) },
            hint: "Starts your games through RocketLauncher, the launcher HyperSpin uses: its fade screens, bezels, pause menu and the emulator you picked in RocketLauncherUI. RustyShelf waits while you play, counts the time and comes back when RocketLauncher closes. Off unless you turn it on below; your emulators stay set up.".into(),
            press: Some(Box::new(|s: &mut Settings| match rustyshelf_core::rocket::find() {
                Some(f) => {
                    let f = f.to_string_lossy().into_owned();
                    s.set(&["rocket", "exe"], json!(f));
                    s.say(format!("RocketLauncher found in {}", short(&f)));
                }
                None => s.pick(Pick::Rocket, "Choose RocketLauncher.exe", false),
            })),
            step: None,
        }];
        if exe.is_empty() {
            return rows;
        }
        let pick = |label: &str, path: &'static [&'static str], list: &'static [(&'static str, &'static str)], hint: &str| -> Row {
            let now = self.text(path);
            let at = list.iter().position(|x| x.0 == now).unwrap_or(0);
            let turn = move |s: &mut Settings, d: i32| s.set(path, json!(list[(at as i32 + d).rem_euclid(list.len() as i32) as usize].0));
            let again = turn;
            Row { label: label.into(), value: list[at].1.into(), hint: hint.into(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(again)) }
        };
        rows.push(pick(
            "Start games through RocketLauncher",
            &["rocket", "mode"],
            &[("off", "Off"), ("all", "Every console"), ("some", "Only the consoles I choose")],
            "Every console: each console RocketLauncher has a system for starts through it, the rest keep their emulator. Only the consoles I choose: pick on each console's page. Off: RustyShelf starts your emulators itself.",
        ));
        if self.rocket_on() {
            rows.push(pick(
                "Front end RocketLauncher is told",
                &["rocket", "frontend"],
                &[("", "Its default"), ("HyperSpin", "HyperSpin"), ("LaunchBox", "LaunchBox"), ("Playnite", "Playnite"), ("RocketLauncherUI", "RocketLauncherUI")],
                "Whose settings and media RocketLauncher uses for its fade and pause screens. Its default: the front end set in RocketLauncherUI.",
            ));
        }
        rows
    }

    /// A console's page with RocketLauncher on: whether it starts through it, the name RocketLauncher knows it by, and
    /// whether its games folder is one RocketLauncher uses (else it's sent the game's full path).
    fn rocket_console_rows(&self, id: &str, emulator: &str) -> Vec<Row> {
        let all = self.text(&["rocket", "mode"]) == "all";
        let via = at(&self.cfg, &["systems", id, "viaRocket"]).and_then(Value::as_bool).unwrap_or(all);
        let (a, b) = (id.to_string(), id.to_string());
        let mut rows = vec![Row {
            label: "Start games with".into(),
            value: if via { "RocketLauncher".into() } else { "Emulator".into() },
            hint: format!("RocketLauncher: Play hands the game to RocketLauncher, which brings its fade screen, bezel and pause menu and starts the emulator set up for this system in RocketLauncherUI. Emulator: RustyShelf starts {emulator} itself, as before."),
            press: Some(Box::new(move |s: &mut Settings| s.set(&["systems", &a, "viaRocket"], json!(!via)))),
            step: Some(Box::new(move |s: &mut Settings, _| s.set(&["systems", &b, "viaRocket"], json!(!via)))),
        }];
        if !via {
            return rows;
        }
        let exe = PathBuf::from(self.text(&["rocket", "exe"]));
        let list = rustyshelf_core::rocket::systems(&exe, false);
        let mut name_row = self.typed("RocketLauncher system name", &["systems", id, "rocketSystem"], "The system's name in RocketLauncher, read from its own settings. Change it if yours is named differently.");
        let own = self.text(&["systems", id, "rocketSystem"]);
        let name = if own.trim().is_empty() { rustyshelf_core::rocket::name_for(id, &list) } else { own.trim().to_string() };
        if own.trim().is_empty() {
            name_row.value = name.clone();
        }
        rows.push(name_row);
        let sys = list.iter().find(|s| s.name.eq_ignore_ascii_case(&name));
        let dirs = self.list(&["systems", id, "romDirs"]);
        let matches = sys.is_some_and(|s| dirs.iter().any(|d| s.rom_paths.iter().any(|p| p.to_string_lossy().trim_end_matches(['\\', '/']).eq_ignore_ascii_case(d.trim_end_matches(['\\', '/'])))));
        rows.push(Row {
            label: "Games folder for RocketLauncher".into(),
            value: match sys {
                None => "RocketLauncher has no system by this name".into(),
                Some(s) if !s.ready => "No emulator set for it in RocketLauncherUI".into(),
                Some(_) if matches => "Matches".into(),
                Some(_) => "Not one RocketLauncher uses · it's sent the full path".into(),
            },
            hint: "RocketLauncher finds a game in its own ROM folders for this system. When this console's games folder is one of them, the game goes by its name (its 7z and multi-disc handling work); otherwise it's sent the game's full path.".into(),
            ..Row::default()
        });
        rows
    }

    fn typed(&self, label: &'static str, path: &[&str], hint: &'static str) -> Row {
        let now = self.text(path);
        let owned: Vec<String> = path.iter().map(|p| p.to_string()).collect();
        let value = now.clone();
        Row {
            label: label.to_string(),
            value: if now.is_empty() { "(none)".into() } else { now },
            hint: hint.to_string(),
            press: Some(Box::new(move |s: &mut Settings| {
                let path = owned.clone();
                s.edit(label, hint, value.clone(), Box::new(move |s: &mut Settings, v: String| s.set(&path.iter().map(String::as_str).collect::<Vec<_>>(), json!(v))));
            })),
            step: None,
        }
    }

    /// The language the screen is in, each by its own name (◀ ▶ or Enter changes it).
    fn language_row(&self) -> Row {
        let now = self.text(&["ui", "language"]);
        let at = LANGUAGES.iter().position(|x| x.0 == now).unwrap_or(0);
        let turn = move |s: &mut Settings, d: i32| s.set(&["ui", "language"], json!(LANGUAGES[(at as i32 + d).rem_euclid(LANGUAGES.len() as i32) as usize].0));
        Row {
            label: "Language".into(),
            value: LANGUAGES[at].1.into(),
            hint: "The language the screen is shown in. Português (Brasil) is in the Electron app only for now: here it shows English.".into(),
            press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
            step: Some(Box::new(turn)),
        }
    }

    fn back(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        } else if !self.wizard {
            self.open = false;
        }
        self.cues.push(("back", 1));
        self.note.clear();
        self.asked.clear();
        self.redraw = true;
    }

    /// Asks which drives to look through as well (Windows, when there are drives), then looks.
    fn look(&mut self) {
        if self.looking.is_some() {
            return;
        }
        let drives = rustyshelf_core::detect::list_drives();
        if drives.is_empty() {
            return self.look_in(Vec::new());
        }
        self.drives = drives.into_iter().map(|d| { let ticked = !d.system; (d, ticked) }).collect();
        self.go(Screen::Drives);
    }

    /// Looks for emulators, games folders, Steam and the media apps, off the screen's thread; `drives` are searched
    /// deep as well.
    fn look_in(&mut self, drives: Vec<String>) {
        if self.looking.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let root = self.app_root.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Detector::new(&root).detect_with(&drives));
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
        self.changed = true;
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
            // RUSTYSHELF_PICK answers instead of the dialog: for trying these pages where there's no desktop
            if let Some(answer) = std::env::var_os("RUSTYSHELF_PICK") {
                let _ = tx.send(Some(PathBuf::from(answer)));
                return;
            }
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
            Pick::EmulatorsIn => {
                let (found, retroarch) = Detector::new(&self.app_root).emulators_in(&p);
                let mut set = 0;
                for (id, exe) in found {
                    if self.text(&["systems", &id, "emulator"]).is_empty() {
                        self.set(&["systems", &id, "emulator"], json!(exe));
                        self.set(&["systems", &id, "enabled"], json!(true));
                        set += 1;
                    }
                }
                if !retroarch.is_empty() && self.text(&["retroarch"]).is_empty() {
                    self.set(&["retroarch"], json!(retroarch));
                }
                self.say(if set == 0 { "No emulator RustyShelf knows was found in that folder".into() } else { format!("Found {set} emulator{} there", if set == 1 { "" } else { "s" }) });
            }
            Pick::SoundPack => {
                match rustyshelf_core::soundpacks::import_from(&self.app_root, &[path.to_path_buf()], None) {
                    Ok(pack) => {
                        let left = if pack.unsupported.is_empty() { String::new() } else { format!(" · {} file{} couldn't be used", pack.unsupported.len(), if pack.unsupported.len() == 1 { "" } else { "s" }) };
                        self.say(format!("Added \"{}\" to the sound list{left}", pack.name));
                    }
                    Err(why) => self.say(why),
                }
                (self.packs, self.sound_changed) = (rustyshelf_core::soundpacks::list(&self.app_root), true);
            }
            Pick::RomFoldersIn => {
                let found = Detector::new(&self.app_root).rom_folders_in(&p);
                let mut added = 0;
                for (id, dirs) in &found {
                    let mut have = self.list(&["systems", id, "romDirs"]);
                    for d in dirs {
                        if !have.iter().any(|x| x.eq_ignore_ascii_case(d)) {
                            have.push(d.clone());
                            added += 1;
                        }
                    }
                    self.set(&["systems", id, "romDirs"], json!(have));
                }
                self.say(if added > 0 {
                    format!("Added {added} games folder{}", if added == 1 { "" } else { "s" })
                } else if !found.is_empty() {
                    "Those folders were already added".into()
                } else {
                    format!("No console's folder found in {}", short(&p))
                });
            }
            Pick::Rocket if path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("RocketLauncher.exe")) => {
                self.set(&["rocket", "exe"], json!(p));
                self.say(format!("RocketLauncher found in {}", short(&p)));
            }
            Pick::Rocket => self.say("That isn't RocketLauncher.exe (not RocketLauncherUI.exe)".into()),
            Pick::Steam => self.set(&["steam", "path"], json!(p)),
            Pick::GamesDir => self.set(&["pc", "gamesDir"], json!(p)),
            Pick::PortsDir => self.set(&["pc", "portsDir"], json!(p)),
            Pick::ArtLibrary => self.set(&["ui", "artLibrary"], json!(p)),
            Pick::ArtDir(id) => self.set(&["systems", &id, "artDir"], json!(p)),
            Pick::CustomGame => self.add_custom(&p),
            Pick::ExportTo => self.export_to(path),
            Pick::ImportFrom => self.import_from(path),
            Pick::ScanGames => self.scan_games(path),
            Pick::Playnite => self.read_playnite(Some(path)),
            Pick::OwnArt(cut) => self.own_art(path, cut),
            Pick::OwnManual => self.own_manual(path),
            Pick::ThemePicture => match rustyshelf_core::customthemes::create(&rustyshelf_core::customthemes::root(&self.app_root), path, "") {
                Ok(id) => {
                    self.set(&["ui", "theme"], json!(id));
                    self.reload_themes = true;
                    self.say("Your picture is a theme now, and it's on. Its folder in Custom themes has a theme.json to change its name and colours.".into());
                }
                Err(why) => self.say(why),
            },
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
        let screen = &self.stack.last().expect("a page").0;
        if let Some(locked) = self.kid_lock_page(screen) {
            return locked;
        }
        let mut page = self.screen_page();
        if self.kid_locked() && *screen == Screen::Details {
            page.rows.retain(|r| !matches!(r.label.as_str(), "Hide from the shelf" | "Box art" | "Collections" | "Manual Studio"));
        }
        page
    }

    fn screen_page(&self) -> Page {
        self.page_of(&self.stack.last().expect("a page").0)
    }

    /// Settings' first page, by what you came to do: the tab's rows, gathered from the pages they were on.
    fn tab_page(&self) -> Page {
        let open = |label: &str, value: String, hint: &str, to: Screen| -> Row {
            Row { label: label.to_string(), value, hint: hint.to_string(), press: Some(Box::new(move |s: &mut Settings| s.go(to.clone()))), step: None }
        };
        let rows_of = |screen: Screen| self.page_of(&screen).rows;
        let (lead, mut rows) = match TABS[self.tab % TABS.len()] {
            "Library" => {
                let mut rows = self.attention_rows();
                rows.extend([
                    open("Consoles", String::new(), "Each console's emulator and games folders", Screen::Consoles),
                    open("PC games and apps", String::new(), "Steam, the other stores, the PC games folder, the streaming apps", Screen::Pc),
                    open("Custom games", String::new(), "Recomps, PC ports and any other program, added by hand", Screen::Custom),
                    open("Collections", String::new(), "Shelves you make yourself, mixing games from any console: make one, name it, search for games to put on it.", Screen::Collections),
                    open("Hidden games", format!("{}", self.list(&["ui", "hidden"]).len()), "Games taken off their shelf; they can be shown again here", Screen::Hidden),
                ]);
                ("Your consoles, their emulators and your games: where RustyShelf finds them and what goes on the shelves.", rows)
            }
            "Look" => ("How RustyShelf looks and moves: theme, size, clock and effects.", rows_of(Screen::Display)),
            "Sound" => ("Interface sounds, menu music and songs.", rows_of(Screen::Sound)),
            "Box art & media" => ("Covers and manuals: where they come from and when they download.", rows_of(Screen::BoxArt)),
            "Controls" => {
                let mut rows = rows_of(Screen::Controller);
                rows.push(open("Phone companion", on_off(self.phone.as_ref().is_some_and(|p| p.state().enabled)), "A phone on your home network as a remote, a keyboard and a second look at your games", Screen::Phone));
                ("Your controller, and how to leave a game.", rows)
            }
            "Shelf" => ("What the shelf shows.", vec![open("Shelf", String::new(), "The order of every shelf, and which games they show", Screen::Shelf)]),
            "Accounts" => {
                let mut rows = vec![Row { label: "Trophies".into(), hint: "Your RetroAchievements and Steam achievements: every game, the last month's unlocks and how rare they are".into(), press: Some(Box::new(|s: &mut Settings| s.open_trophies(false))), ..Row::default() }];
                rows.extend(rows_of(Screen::Accounts));
                ("Sign-ins and keys that add art, achievements and features.", rows)
            }
            _ => ("Updates, your settings themselves, and Kid mode.", rows_of(Screen::System)),
        };
        rows.push(Row { label: "Back to the shelf".into(), press: Some(Box::new(|s: &mut Settings| s.back())), ..Row::default() });
        Page { title: "Settings".into(), lead: lead.into(), rows }
    }

    /// Library's Needs attention: what's keeping a console's games off the shelf, each with its fix; and IGDB, a
    /// suggestion when there are console games and it isn't connected.
    fn attention_rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for c in systems().consoles.iter().filter(|c| self.flag(&["systems", &c.id, "enabled"], true) && self.cfg["systems"].get(&c.id).is_some()) {
            let (emulator, folders) = (self.text(&["systems", &c.id, "emulator"]), self.list(&["systems", &c.id, "romDirs"]));
            let retro = c.retro && !self.text(&["retroarch"]).is_empty();
            let (label, hint) = if emulator.is_empty() && !retro {
                (format!("{} has no emulator", c.name), "Choose the emulator that plays it")
            } else if folders.is_empty() {
                (format!("{} has no games folder", c.name), "Add the folder its games are in")
            } else {
                continue;
            };
            let id = c.id.clone();
            rows.push(Row { label, value: "Fix".into(), hint: format!("{hint}."), press: Some(Box::new(move |s: &mut Settings| s.go(Screen::Console(id.clone())))), step: None });
        }
        if self.api_key("igdbId").is_empty() && !self.console_shelves.is_empty() {
            rows.push(Row { label: "Connect IGDB for game info".into(), value: "Set up".into(), hint: "Free with a Twitch developer account: the description, genres and dates LaunchBox is missing.".into(), press: Some(Box::new(|s: &mut Settings| s.go(Screen::Accounts))), step: None });
        }
        if !rows.is_empty() {
            rows.insert(0, Row { label: format!("— Needs attention · {} —", rows.len()), ..Row::default() });
        }
        rows
    }

    /// Brings the other RustyShelf's setup over (with its downloads too, `extras`), then starts again to put it in.
    fn bring_over(&mut self, extras: bool) {
        let Some(from) = self.other_setup.clone().filter(|_| self.bringing.is_none()) else { return };
        let to = rustyshelf_core::keepsetup::Install { data_dir: paths::data_dir(&self.app_root), app_root: self.app_root.clone(), version: env!("CARGO_PKG_VERSION").into(), last_run: 0.0 };
        let (tx, rx) = channel();
        let stage = std::sync::Arc::new(Mutex::new("Starting…".to_string()));
        let at = stage.clone();
        std::thread::spawn(move || {
            let done = rustyshelf_core::keepsetup::bring_stage(&from, &to, extras, &|done, total| *at.lock().unwrap() = format!("{}%", done * 100 / total.max(1)));
            let _ = tx.send(done.map(|_| ()));
        });
        self.bringing = Some((stage, Mutex::new(rx)));
        self.redraw = true;
    }

    /// A setup being brought over, since the last frame.
    fn poll_bringing(&mut self) {
        let Some((stage, rx)) = &self.bringing else { return };
        let said = format!("Bringing your setup over… {}", stage.lock().unwrap());
        let ended = rx.lock().unwrap().try_recv().ok();
        if self.note != said && ended.is_none() {
            self.say(said);
        }
        match ended {
            Some(Ok(())) => restart(),
            Some(Err(why)) => {
                self.bringing = None;
                self.say(format!("Your setup couldn't be brought over: {why}"));
            }
            None => {}
        }
    }

    /// Settings' tabs, one step on (1) or back (-1).
    fn tab_step(&mut self, by: i32) {
        self.tab = (self.tab as i32 + by).rem_euclid(TABS.len() as i32) as usize;
        if TABS[self.tab] == "Sound" {
            self.packs = rustyshelf_core::soundpacks::list(&self.app_root);
        }
        if let Some(top) = self.stack.last_mut() {
            top.1 = 0;
        }
        self.cues.push(("tick", by.signum()));
        self.note.clear();
        self.redraw = true;
    }

    fn page_of(&self, screen: &Screen) -> Page {
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
                    self.language_row(),
                ]
                .into_iter()
                .chain(self.other_setup.iter().flat_map(|o| {
                    let place = short(&o.app_root.to_string_lossy());
                    [false, true].map(|extras| Row {
                        label: if self.bringing.is_some() { "Bringing your setup over…".into() } else if extras { "…and its downloaded extras too".into() } else { "Bring my setup over".into() },
                        value: if extras { String::new() } else { o.version.clone() },
                        hint: if extras { "The same, with the box art, screenshots and other downloads it keeps, so nothing is downloaded again. It can take a while.".into() } else { format!("Your settings, history, shelves, sounds and themes from the RustyShelf in {place}. Its emulators and games stay where they are. RustyShelf starts again to put them in.") },
                        press: Some(Box::new(move |s: &mut Settings| s.bring_over(extras))),
                        step: None,
                    })
                }))
                .collect(),
            },
            Screen::Drives => {
                let mut rows: Vec<Row> = self
                    .drives
                    .iter()
                    .enumerate()
                    .map(|(i, (d, on))| {
                        let flip: Press = Box::new(move |s: &mut Settings| {
                            if let Some(x) = s.drives.get_mut(i) {
                                x.1 = !x.1;
                            }
                            s.redraw = true;
                        });
                        Row {
                            label: format!("Drive {}:", d.letter),
                            value: on_off(*on),
                            hint: if d.system { "Windows drive".into() } else { String::new() },
                            press: Some(flip),
                            step: Some(Box::new(move |s: &mut Settings, _| {
                                if let Some(x) = s.drives.get_mut(i) {
                                    x.1 = !x.1;
                                }
                                s.redraw = true;
                            })),
                        }
                    })
                    .collect();
                rows.push(Row {
                    label: "Search these drives".into(),
                    press: Some(Box::new(|s: &mut Settings| {
                        let chosen = s.drives.iter().filter(|(_, on)| *on).map(|(d, _)| d.path.clone()).collect();
                        s.back();
                        s.look_in(chosen);
                    })),
                    ..Row::default()
                });
                rows.push(Row {
                    label: "Usual places only".into(),
                    press: Some(Box::new(|s: &mut Settings| {
                        s.back();
                        s.look_in(Vec::new());
                    })),
                    ..Row::default()
                });
                Page {
                    title: "Which drives should RustyShelf search?".into(),
                    lead: "The usual places (the RustyShelf folder, the Desktop, Program Files …) are always looked through. The drives ticked here are searched too, for emulators and games folders: a big drive can take a few minutes.".into(),
                    rows,
                }
            }
            Screen::Root => self.tab_page(),
            Screen::Consoles => {
                let retroarch = self.text(&["retroarch"]);
                // the first run shows what was found (and RetroArch only when it's there); every other console is a row away
                let mut rows = if self.wizard && retroarch.is_empty() {
                    Vec::new()
                } else {
                    vec![open("RetroArch", if retroarch.is_empty() { "Not set".into() } else { short(&retroarch) }, "The program every retro console without an emulator of its own runs in", Screen::Emulator(String::new()))]
                };
                let mut consoles: Vec<&Console> = systems().consoles.iter().collect();
                let mut hidden = 0;
                let found_any = consoles.iter().any(|c| self.flag(&["systems", &c.id, "enabled"], true));
                if self.wizard {
                    consoles.sort_by_key(|c| !self.flag(&["systems", &c.id, "enabled"], true));
                    if !self.every_console && found_any {
                        let before = consoles.len();
                        consoles.retain(|c| self.flag(&["systems", &c.id, "enabled"], true));
                        hidden = before - consoles.len();
                    }
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
                rows.push(Row {
                    label: "Emulators somewhere else?".into(),
                    value: "Choose their folder".into(),
                    hint: "If you downloaded your emulators and unzipped them yourself (to Downloads, another drive, a USB stick), choose the folder they're in: every console missing one gets the one found there.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.pick(Pick::EmulatorsIn, "Choose the folder your emulators are in", true))),
                    ..Row::default()
                });
                rows.push(Row {
                    label: "Find games folders in a folder".into(),
                    value: "Choose".into(),
                    hint: "A folder with a folder of games for each console, named after it (like RetroBat's roms folder: gc, wii, ps2, psx…): each console's folder is added to its games folders.".into(),
                    press: Some(Box::new(|s: &mut Settings| s.pick(Pick::RomFoldersIn, "Choose the folder with a folder of games per console", true))),
                    ..Row::default()
                });
                if cfg!(windows) && !self.wizard {
                    rows.extend(self.rocket_rows());
                }
                if hidden > 0 {
                    rows.push(Row {
                        label: format!("More consoles ({hidden})"),
                        hint: "Every console RustyShelf can run; ◀ ▶ on one gives it a shelf.".into(),
                        press: Some(Box::new(|s: &mut Settings| {
                            s.every_console = true;
                            s.redraw = true;
                        })),
                        ..Row::default()
                    });
                }
                if self.wizard {
                    rows.push(next(Screen::Pc));
                }
                let lead = if self.wizard && !found_any {
                    "Step 1 of 3 · Nothing was found on this PC yet. ◀ ▶ turns on the consoles you play; Enter opens one to choose its emulator and games folder."
                } else if self.wizard {
                    "Step 1 of 3 · The consoles that are on get a shelf: these were found on your PC. ◀ ▶ turns one on or off; Enter opens it to choose its emulator and games folders."
                } else {
                    "A console that's on has a shelf. Open one to choose its emulator and its games folders."
                };
                Page { title: "Your consoles".into(), lead: lead.into(), rows }
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
                ];
                if self.rocket_on() {
                    rows.splice(1..1, self.rocket_console_rows(id, &c.emulator_name));
                }
                let runs = self.emulator_of(id);
                if c.retro && c.ra_cores.len() > 1 && is_retroarch(&runs) {
                    let usual = c.ra_core.clone().unwrap_or_default();
                    let chosen = Some(self.text(&["systems", id, "raCore"])).filter(|x| c.ra_cores.iter().any(|(k, _)| k == x)).unwrap_or(usual.clone());
                    let at = c.ra_cores.iter().position(|(k, _)| *k == chosen).unwrap_or(0);
                    let cid = id.clone();
                    let turn = move |s: &mut Settings, d: i32| {
                        let n = (at as i32 + d).rem_euclid(c.ra_cores.len() as i32) as usize;
                        s.set(&["systems", &cid, "raCore"], json!(c.ra_cores[n].0));
                    };
                    let turn2 = turn.clone();
                    rows.push(Row {
                        label: "RetroArch core".into(),
                        value: if chosen == usual { format!("{} (usual)", c.ra_cores[at].1) } else { c.ra_cores[at].1.clone() },
                        hint: format!("The core RetroArch runs {} games in. A core that isn't there yet is downloaded the first time you play. Save states made in one core don't load in another; in-game saves usually carry over.", c.name),
                        press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                        step: Some(Box::new(turn2)),
                    });
                }
                let twin = match id.as_str() {
                    "wii" => Some(("gc", "GameCube")),
                    "gc" => Some(("wii", "Wii")),
                    _ => None,
                };
                if let Some((other, name)) = twin.filter(|(o, _)| emu.is_empty() && !self.text(&["systems", o, "emulator"]).is_empty()) {
                    let cid = id.clone();
                    rows.push(Row {
                        label: format!("Use {name}'s Dolphin"),
                        hint: "The same Dolphin runs both.".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let dolphin = s.text(&["systems", other, "emulator"]);
                            s.set(&["systems", &cid, "emulator"], json!(dolphin));
                        })),
                        ..Row::default()
                    });
                }
                let name = if emu.is_empty() { c.emulator_name.clone() } else { emuname::label(&emu) };
                if self.can_set_up(id) {
                    rows.push(open(&format!("Set up {name} for me"), String::new(), &format!("Controller, the Home menu's hotkeys, one BIOS folder and a graphics preset for {name}: backed up first, and Undo puts it back."), Screen::EmuSetup(id.clone())));
                }
                rows.push(open("Emulator & advanced", String::new(), &format!("Open {name} on its own, launch options{}.", if id == "gc" { " and the real GameCube startup" } else { "" }), Screen::Advanced(id.clone())));
                // (the Wii U reads its games' own manuals)
                if id != "wiiu" {
                    rows.push(self.manual_pref_row(id));
                }
                rows.push(self.art_style_row(id));
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
            Screen::Advanced(id) => {
                let Some(c) = self.console(id) else { return Page { title: id.clone(), lead: String::new(), rows: Vec::new() } };
                let runs = self.emulator_of(id);
                let name = if runs.is_empty() { c.emulator_name.clone() } else { emuname::label(&runs) };
                let mut rows = Vec::new();
                if !runs.is_empty() {
                    let cid = id.clone();
                    rows.push(Row {
                        label: "Open the emulator on its own".into(),
                        hint: "Starts the emulator without a game so you can change its graphics or controls.".into(),
                        press: Some(Box::new(move |s: &mut Settings| s.open_emulator = Some(cid.clone()))),
                        ..Row::default()
                    });
                }
                rows.push(self.typed("Emulator options", &["systems", id, "emulatorArgs"], "What RustyShelf passes when it opens the emulator on its own."));
                if id == "gc" {
                    let on = self.flag(&["systems", id, "useIpl"], false);
                    let exe = rustyshelf_core::launch::emu_path(&runs, &self.app_root).to_string_lossy().into_owned();
                    rows.push(self.switch(
                        "Play the real GameCube startup",
                        &["systems", id, "useIpl"],
                        false,
                        "Boots through the console's own startup animation. Needs your GameCube's system file (IPL.bin) in Dolphin's GC\\USA folder (EUR or JAP for those regions).",
                    ));
                    rows.push(Row {
                        label: "Add your IPL.bin".into(),
                        value: "Open folder".into(),
                        hint: "Opens Dolphin's GC\\USA folder: put the IPL.bin dumped from your GameCube there (EUR or JAP folders for those regions).".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let opened = rustyshelf_core::dolphin::ensure_ipl_folder(&exe, "USA", &paths::env_var).map_err(|e| e.to_string()).and_then(|d| rustyshelf_core::launch::open_folder(Path::new(&d)).map_err(|e| e.to_string()));
                            if let Err(e) = opened {
                                s.note = e;
                                s.redraw = true;
                            }
                        })),
                        step: None,
                    });
                    if on {
                        rows.push(self.switch("Skip Dolphin's warning pop-ups", &["systems", id, "hideDolphinWarnings"], false, "Hides Dolphin's warnings (like an unrecognised IPL.bin) that would stop the game from starting."));
                    }
                }
                if id == "ps5" {
                    rows.push(self.switch(
                        "Start games through Kyty's launcher",
                        &["systems", id, "viaLauncher"],
                        false,
                        "Off: Play starts the game straight away, with the settings you chose in Kyty's launcher. On: Play opens Kyty's launcher and you start the game there.",
                    ));
                }
                rows.push(self.typed("Game launch options", &["systems", id, "gameArgs"], "What RustyShelf passes to the emulator when you play. {rom} is the game file."));
                let cid = id.clone();
                rows.push(Row {
                    label: "Restore default launch options".into(),
                    hint: format!("Default: {}", if c.game_args.is_empty() { "(none)" } else { &c.game_args }),
                    press: Some(Box::new(move |s: &mut Settings| {
                        put(&mut s.cfg, &["systems", &cid, "emulatorArgs"], json!(c.emulator_args));
                        s.set(&["systems", &cid, "gameArgs"], json!(c.game_args));
                        s.note = "Launch options back to the defaults".into();
                    })),
                    ..Row::default()
                });
                let art = self.text(&["systems", id, "artDir"]);
                let (what, cid) = (Pick::ArtDir(id.clone()), id.clone());
                rows.push(Row {
                    label: "Own art folder".into(),
                    value: if art.is_empty() { "(uses the art library)".into() } else { short(&art) },
                    hint: "A folder with this console's covers, if you keep them separately. Name images after the game file; add -back for the back cover.  ·  Enter chooses it, ◀ forgets it".into(),
                    press: Some(Box::new(move |s: &mut Settings| s.pick(what.clone(), "Choose a cover art folder", true))),
                    step: Some(Box::new(move |s: &mut Settings, d| {
                        if d < 0 {
                            s.set(&["systems", &cid, "artDir"], json!(""));
                        }
                    })),
                });
                Page { title: "Emulator & advanced".into(), lead: format!("{name}'s own tools, how RustyShelf starts it, and the rarely-needed settings for {}.", c.name), rows }
            }
            Screen::EmuSetup(id) => self.setup_page(id),
            Screen::GameCollections => self.game_collections_page(),
            Screen::Collections => self.collections_page(),
            Screen::MyShelf(id) => self.my_shelf_page(id),
            Screen::Custom => self.custom_page(),
            Screen::CustomGame(i) => self.custom_game_page(*i),
            Screen::AddGames => self.add_games_page(),
            Screen::Manuals => self.manuals_page(),
            Screen::ManualStudio => self.manual_studio_page(),
            Screen::Trophies => self.trophies_page(),
            Screen::Playnite => self.playnite_page(),
            Screen::Pc => {
                let steam = self.text(&["steam", "path"]);
                let games = self.text(&["pc", "gamesDir"]);
                let ports = self.text(&["pc", "portsDir"]);
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
                    toggle("Quiver Launcher", &["pc", "quiver"], true, "Its apps go on the Custom shelf, its ports and recompilations on Ports & Recomps"),
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
                    Row {
                        label: "Ports & recomps folder".into(),
                        value: if ports.is_empty() { "None".into() } else { short(&ports) },
                        hint: "Every folder in it is a PC port or recompilation, on the Ports & Recomps shelf; a folder named after a console (N64, GameCube…) holds that console's, in its case  ·  Enter chooses it, ◀ forgets it".into(),
                        press: Some(Box::new(|s: &mut Settings| s.pick(Pick::PortsDir, "Choose your ports and recompilations folder", true))),
                        step: Some(Box::new(|s: &mut Settings, d| {
                            if d < 0 {
                                s.set(&["pc", "portsDir"], json!(""));
                            }
                        })),
                    },
                    toggle("Console box art", &["pc", "consoleBox"], false, "A PC game in the box of its console release, where there is one"),
                    open("Storage", String::new(), "Every installed PC game with its size, by drive, and how much room each drive has", Screen::Storage),
                ];
                if cfg!(target_os = "linux") {
                    rows.extend(self.linux_pc_rows());
                }
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
                let lead = if self.wizard { "Step 2 of 3 · The PC shelf holds your Steam games and the other stores'. A store that isn't installed simply adds nothing." } else { "The PC shelf holds your Steam games and the other stores'. A store that isn't installed simply adds nothing." };
                Page { title: "PC games and apps".into(), lead: lead.into(), rows }
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
                    toggle("Sharpest scans first", &["ui", "artHD"], true, "Takes the sharpest version there is and swaps out low-res pictures RustyShelf downloaded; one you put there yourself is never replaced"),
                    toggle("Fetch manuals for new games", &["ui", "autoManuals"], false, "Finds a game's manual on archive.org and GamesDatabase.org and saves its pages beside the box art, to read from Details; game names are sent to them"),
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
                // the first run asks just the one question; the rest shows once downloads are on
                if self.wizard && !self.flag(&["ui", "autoCovers"], false) {
                    rows.truncate(1);
                }
                if self.wizard {
                    rows.push(next(Screen::Done));
                }
                let lead = if self.wizard { "Step 3 of 3 · RustyShelf can fetch the covers your games don't have yet. Your game titles are sent to find them; nothing else about you or your PC." } else { "RustyShelf can fetch the covers your games don't have yet." };
                Page { title: "Box art".into(), lead: lead.into(), rows }
            }
            Screen::Display => {
                // one of a list, by its saved name: Enter and right go on to the next, left back
                let choice = |label: &str, path: &'static [&'static str], list: Vec<(String, String)>, now: &str, hint: &str| -> Row {
                    let at = list.iter().position(|x| x.0 == now).unwrap_or(0);
                    let value = list[at].1.clone();
                    let turn = move |s: &mut Settings, d: i32| s.set(path, json!(list[(at as i32 + d).rem_euclid(list.len() as i32) as usize].0));
                    let again = turn.clone();
                    Row { label: label.to_string(), value, hint: hint.to_string(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(again)) }
                };
                let named = |list: &[(&str, &str)]| list.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect::<Vec<_>>();
                let clock = self.text(&["ui", "clock"]);
                let mut rows = vec![
                    self.language_row(),
                    choice(
                        "Theme",
                        &["ui", "theme"],
                        rustyshelf_core::themes::themes().into_iter().map(|t| (t.id, t.name)).chain(self.custom_themes.iter().cloned()).collect(),
                        &self.theme(),
                        "The colour and mood behind your shelves. Close Settings to see it on your shelf. Your own pictures are themes too: each folder in Custom themes, beside RustyShelf, is one.",
                    ),
                    Row {
                        label: "Custom themes folder".into(),
                        value: "Open".into(),
                        hint: "A folder with a picture named background (.jpg, .png, .webp or .svg) is a theme; the note in the folder says the rest.".into(),
                        press: Some(Box::new(|s: &mut Settings| {
                            let dir = rustyshelf_core::customthemes::root(&s.app_root);
                            rustyshelf_core::customthemes::ensure_examples(&dir);
                            if let Err(e) = rustyshelf_core::launch::open_folder(&dir) {
                                s.say(e.to_string());
                            }
                        })),
                        step: None,
                    },
                    Row {
                        label: "Make a theme from a picture".into(),
                        hint: "Choose a JPG, PNG, WebP or SVG picture: it becomes the background of a new theme in the Custom themes folder, with colours to match.".into(),
                        press: Some(Box::new(|s: &mut Settings| s.pick(Pick::ThemePicture, "Choose a picture for your theme", false))),
                        ..Row::default()
                    },
                    Row { label: "Find themes again".into(), hint: "Looks in the Custom themes folder again.".into(), press: Some(Box::new(|s: &mut Settings| s.reload_themes = true)), ..Row::default() },
                    choice(
                        "Clock",
                        &["ui", "clock"],
                        named(&[("full", "Day, date and time"), ("time", "Time only"), ("off", "Hidden")]),
                        if clock == "time" || clock == "off" { &clock } else { "full" },
                        "What shows in the top corner of the shelf.",
                    ),
                    toggle("24-hour clock", &["ui", "clock24"], false, "Shows 18:30 instead of 6:30 PM."),
                    toggle("Start in fullscreen", &["ui", "startFullscreen"], true, "From the next start"),
                    toggle("All games shelf", &["ui", "allShelf"], false, "A shelf with your complete collection in alphabetical order."),
                    toggle("Attract mode", &["ui", "attract"], true, "After a few idle minutes the shelf drifts through your boxes on its own, with the HUD faded, like a store display."),
                ];
                // one of a list of numbers: Enter and right go on to the next, left back
                let minutes = |label: &str, path: &'static [&'static str], list: &'static [(f64, &str)], unset: f64, hint: &str| -> Row {
                    let now = at(&self.cfg, path).and_then(Value::as_f64).unwrap_or(unset);
                    let i = list.iter().position(|x| x.0 == now).or_else(|| list.iter().position(|x| x.0 == unset)).unwrap_or(0);
                    let turn = move |s: &mut Settings, d: i32| s.set(path, json!(list[(i as i32 + d).rem_euclid(list.len() as i32) as usize].0));
                    let again = turn;
                    Row { label: label.to_string(), value: list[i].1.to_string(), hint: hint.to_string(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(again)) }
                };
                if self.flag(&["ui", "attract"], true) {
                    let pace = self.text(&["ui", "attractPace"]);
                    rows.extend([
                        minutes("Attract mode starts after", &["ui", "attractMinutes"], &[(2.0, "2 min"), (3.0, "3 min"), (5.0, "5 min"), (10.0, "10 min"), (20.0, "20 min")], 3.0, "How long without input before the tour begins."),
                        choice(
                            "Attract mode pace",
                            &["ui", "attractPace"],
                            named(&[("slow", "Slow"), ("normal", "Normal"), ("fast", "Fast")]),
                            if pace == "slow" || pace == "fast" { &pace } else { "normal" },
                            "How long each box stays before the shelf moves on: about 7 seconds, 4 seconds or 2½ seconds.",
                        ),
                        toggle("Attract mode visits other consoles", &["ui", "attractWander"], true, "At the end of a shelf it wanders on to the next console. Off: it stays on the shelf it started on."),
                    ]);
                }
                rows.push(minutes("Dim the screen after", &["ui", "dimMinutes"], &[(0.0, "Never"), (10.0, "10 min"), (20.0, "20 min"), (30.0, "30 min")], 0.0, "Fades the screen to black after a long idle spell, to protect OLED TVs. Any button wakes it."));
                if cfg!(windows) {
                    rows.push(toggle(
                        "Seamless play",
                        &["ui", "seamless"],
                        true,
                        "Steam, Epic, Ubisoft, EA and Xbox games are followed while they run, as a game RustyShelf starts itself is: playtime, the quit combo and the way back to the shelf. Off: their store looks after them.",
                    ));
                }
                rows.extend([
                        toggle(
                            "Show what I'm playing on Discord",
                            &["ui", "discordPresence"],
                            false,
                            "While a game runs, your Discord profile says \"Playing <title>\". It needs a Discord application id (below): make one free at discord.com/developers, and paste its id.",
                        ),
                        self.typed(
                            "Discord application ID",
                            &["ui", "discordAppId"],
                            "The number from your Discord application's page (General Information). Nothing is sent anywhere else: RustyShelf only talks to the Discord app on this computer.",
                        ),
                ]);
                Page { title: "Display".into(), lead: String::new(), rows }
            }
            Screen::Sound => {
                const TRACKS: [(&str, &str); 5] = [("drift", "Drift"), ("night", "Night"), ("glow", "Glow"), ("own", "Your music"), ("off", "Off")];
                const BOOTS: [(&str, &str); 5] = [("chime", "Chime"), ("crystal", "Crystal"), ("pulse", "Pulse"), ("horizon", "Horizon"), ("keys", "Keys")];
                let prefs = self.sound();
                let voice = rustyshelf_core::cues::theme(&prefs.voice).id;
                let percent = |v: f32| format!("{}%", (v * 100.0).round());
                // a level in steps of a twentieth
                let level = |path: &'static [&'static str], now: f32| -> (Press, Step) {
                    let turn = move |s: &mut Settings, d: i32| {
                        let v = ((now * 20.0).round() as i32 + d).clamp(0, 20) as f64 / 20.0;
                        s.set(path, json!(v));
                    };
                    (Box::new(move |s: &mut Settings| turn(s, 1)), Box::new(turn))
                };
                let pick = |path: &'static [&'static str], list: &'static [(&'static str, &'static str)], now: &str| -> (String, Press, Step) {
                    let at = list.iter().position(|x| x.0 == now).unwrap_or(0);
                    let turn = move |s: &mut Settings, d: i32| s.set(path, json!(list[(at as i32 + d).rem_euclid(list.len() as i32) as usize].0));
                    (list[at].1.to_string(), Box::new(move |s: &mut Settings| turn(s, 1)), Box::new(turn))
                };
                let (volume, volume_step) = level(&["ui", "volume"], prefs.volume);
                let (boot_volume, boot_volume_step) = level(&["ui", "bootVolume"], prefs.boot_volume);
                let sounds = self.sound_list();
                let main = sounds.iter().position(|(id, _)| *id == prefs.voice).or_else(|| sounds.iter().position(|(id, _)| id == voice)).unwrap_or(0);
                let voice_name = sounds[main].1.clone();
                let turn_voice = move |s: &mut Settings, d: i32| {
                    let list = s.sound_list();
                    let next = list[(main as i32 + d).rem_euclid(list.len() as i32) as usize].0.clone();
                    s.set(&["ui", "soundTheme"], json!(next));
                };
                let turn_voice2 = turn_voice;
                let (voice_press, voice_step): (Press, Step) = (Box::new(move |s: &mut Settings| turn_voice(s, 1)), Box::new(turn_voice2));
                let (boot_name, boot_press, boot_step) = pick(&["ui", "bootSound"], &BOOTS, &prefs.boot);
                let playing = rustyshelf_core::music::track(&prefs.music).map_or(if prefs.music == "own" { "own" } else { "off" }, |t| t.id);
                let (music_name, music_press, music_step) = pick(&["ui", "music"], &TRACKS, playing);
                let (music_level, music_level_step) = level(&["ui", "musicLevel"], prefs.music_level);
                let open_folder = |label: &str, hint: &str, dir: PathBuf| -> Row {
                    Row {
                        label: label.to_string(),
                        value: "Open".into(),
                        hint: hint.to_string(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            let _ = std::fs::create_dir_all(&dir);
                            if let Err(e) = rustyshelf_core::launch::open_folder(&dir) {
                                s.say(e.to_string());
                            }
                        })),
                        step: None,
                    }
                };
                let mut rows = vec![
                        toggle("Interface sounds", &["ui", "sound"], true, "Ticks, whooshes and chimes as you move around."),
                        Row { label: "Volume".into(), value: percent(prefs.volume), hint: "How loud RustyShelf's own sounds are. Games and apps aren't affected.".into(), press: Some(volume), step: Some(volume_step) },
                        Row {
                            label: "Voice".into(),
                            value: voice_name,
                            hint: "Aero: soft, pure and airy; each console's shelf plays in its own key. Melody: a soft piano; every move plays the next note of a song. Music box: small steel tines, warm and bright. Disc-era: glassy bells in a big room. Handheld: small chirpy blips. Living room: warm and round.".into(),
                            press: Some(voice_press),
                            step: Some(voice_step),
                        },
                        open("Sound per console", if prefs.by_shelf.is_empty() { String::new() } else { format!("{} with their own", prefs.by_shelf.len()) }, "Give a console its own sound; the rest use the one above.", Screen::SoundConsoles),
                        open("Custom sounds", if self.packs.is_empty() { String::new() } else { self.packs.len().to_string() }, "Add sound files from a game (a folder, a file or a ZIP): they join the sound list.", Screen::SoundPacks),
                ];
                if voice == "melody" {
                    rows.push(toggle("Include the built-in songs", &["ui", "builtInSongs"], true, "With Melody, RustyShelf's own songs play along with any MIDI files in your Songs folder."));
                    rows.push(open_folder("Songs folder", "Drop .mid files here (piano versions of game themes work great), then press Reload songs.", rustyshelf_core::usermusic::songs_folder(&self.app_root)));
                    rows.push(Row { label: "Reload songs".into(), hint: "Reads your Songs folder again.".into(), press: Some(Box::new(|s: &mut Settings| s.reload_songs = true)), ..Row::default() });
                }
                rows.extend([                        Row {
                            label: "Menu music".into(),
                            value: music_name,
                            hint: "Quiet music under the shelf, like a console's home screen. Drift: airy and warm. Night: darker and slower. Glow: a slow music-box lullaby. Your music: the audio files in the Music folder, shuffled. It goes quiet while a game runs.".into(),
                            press: Some(music_press),
                            step: Some(music_step),
                        },
                        Row { label: "Music volume".into(), value: percent(prefs.music_level), hint: "How loud the menu music plays, under RustyShelf's other sounds.".into(), press: Some(music_level), step: Some(music_level_step) },
                ]);
                rows.push(open_folder("Music folder", "Drop your own audio files here (mp3, ogg, wav, flac, m4a; sub-folders too), then pick Your music under Menu music and press Reload music.", rustyshelf_core::usermusic::music_folder(&self.app_root)));
                rows.push(Row { label: "Reload music".into(), hint: "Reads your Music folder again.".into(), press: Some(Box::new(|s: &mut Settings| s.reload_music = true)), ..Row::default() });
                rows.extend([                        Row { label: "Boot sound".into(), value: boot_name, hint: "The sting that plays as a console's game starts.".into(), press: Some(boot_press), step: Some(boot_step) },
                        Row {
                            label: "Boot sound volume".into(),
                            value: percent(prefs.boot_volume),
                            hint: "How loud the boot sound plays, on its own: lower it without making the menu sounds quieter. It still follows the overall Volume.".into(),
                            press: Some(boot_volume),
                            step: Some(boot_volume_step),
                        },
                ]);
                Page { title: "Sounds".into(), lead: String::new(), rows }
            }
            Screen::SoundConsoles => {
                let prefs = self.sound();
                let mut list = self.console_shelves.clone();
                let more: Vec<String> = prefs.by_shelf.keys().filter(|id| !list.iter().any(|(l, _)| l == *id)).cloned().collect();
                list.extend(more.into_iter().map(|id| (id.clone(), systems().console(&id).map_or(id, |c| c.name.clone()))));
                let rows = list
                    .into_iter()
                    .map(|(id, name)| {
                        let sounds = self.sound_list();
                        let at = prefs.by_shelf.get(&id).and_then(|v| sounds.iter().position(|x| x.0 == *v).or_else(|| sounds.iter().position(|x| x.0 == rustyshelf_core::cues::theme(v).id))).map_or(0, |i| i + 1);
                        let shown = if at == 0 { "Same as everywhere".to_string() } else { sounds[at - 1].1.clone() };
                        let turn = move |s: &mut Settings, d: i32| {
                            let list = s.sound_list();
                            let next = (at as i32 + d).rem_euclid(list.len() as i32 + 1) as usize;
                            let mut map = s.cfg["ui"]["soundByShelf"].as_object().cloned().unwrap_or_default();
                            match next {
                                0 => map.remove(&id),
                                n => map.insert(id.clone(), json!(list[n - 1].0)),
                            };
                            s.set(&["ui", "soundByShelf"], Value::Object(map));
                            s.cues.push(("open", 1));
                        };
                        let turn2 = turn.clone();
                        Row {
                            label: name.clone(),
                            value: shown,
                            hint: format!("The sound {name}'s shelf plays."),
                            press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                            step: Some(Box::new(turn2)),
                        }
                    })
                    .collect::<Vec<Row>>();
                Page { title: "Sound per console".into(), lead: "Each console can play its own sound; the rest use the one chosen in Sounds.".into(), rows }
            }
            Screen::SoundPacks => {
                let mut rows = vec![
                    Row { label: "Add from a folder".into(), value: "Choose".into(), hint: "A folder of sound files.".into(), press: Some(Box::new(|s: &mut Settings| s.pick(Pick::SoundPack, "A folder of sound files", true))), step: None },
                    Row { label: "Add from a file or a ZIP".into(), value: "Choose".into(), hint: "One sound file, or a ZIP of sound files.".into(), press: Some(Box::new(|s: &mut Settings| s.pick(Pick::SoundPack, "A sound file or a ZIP of them", false))), step: None },
                ];
                if !self.packs.is_empty() {
                    rows.push(Row { label: "— Your custom sounds —".into(), ..Row::default() });
                }
                for p in &self.packs {
                    let (id, sound) = (p.id.clone(), p.sound_id());
                    let n = p.files.len();
                    rows.push(Row {
                        label: p.name.clone(),
                        value: "Remove".into(),
                        hint: format!("{n} file{}. Removing it puts anything that used it back to Aero.", if n == 1 { "" } else { "s" }),
                        press: Some(Box::new(move |s: &mut Settings| {
                            if !s.twice(&format!("sndrm-{id}"), "Press again to remove these sounds") {
                                return;
                            }
                            if !rustyshelf_core::soundpacks::remove(&s.app_root, &id) {
                                s.say("Couldn't remove it".into());
                                return;
                            }
                            if s.text(&["ui", "soundTheme"]) == sound {
                                s.set(&["ui", "soundTheme"], json!("aero"));
                            }
                            let mut map = s.cfg["ui"]["soundByShelf"].as_object().cloned().unwrap_or_default();
                            map.retain(|_, v| *v != sound);
                            s.set(&["ui", "soundByShelf"], Value::Object(map));
                            (s.packs, s.sound_changed) = (rustyshelf_core::soundpacks::list(&s.app_root), true);
                        })),
                        step: None,
                    });
                }
                Page { title: "Custom sounds".into(), lead: "Sounds from a game: each moment of the menu plays one of the files, matched by their names; what a pack has no file for plays Aero's. Only WAV, OGG, MP3 and FLAC are used.".into(), rows }
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
                        {
                            const STYLES: [(&str, &str); 4] = [("auto", "Match my controller"), ("xbox", "Xbox (A B X Y)"), ("ps", "PlayStation (shapes)"), ("nintendo", "Nintendo (B A Y X)")];
                            let at = STYLES.iter().position(|x| x.0 == self.text(&["ui", "buttons"])).unwrap_or(0);
                            let turn = move |s: &mut Settings, d: i32| s.set(&["ui", "buttons"], json!(STYLES[(at as i32 + d).rem_euclid(4) as usize].0));
                            Row { label: "Button prompts".into(), value: STYLES[at].1.into(), hint: "Changes the buttons shown on screen.".into(), press: Some(Box::new(move |s: &mut Settings| turn(s, 1))), step: Some(Box::new(turn)) }
                        },
                        {
                            let (bottom, right) = match self.glyphs() {
                                Glyphs::Ps => ("Cross", "Circle"),
                                Glyphs::Nintendo => ("B", "A"),
                                Glyphs::Xbox => ("A", "B"),
                            };
                            let names = [("auto", "Match my controller".to_string()), ("a", format!("{bottom} (bottom)")), ("b", format!("{right} (right)"))];
                            let at = names.iter().position(|x| x.0 == self.text(&["ui", "confirmButton"])).unwrap_or(0);
                            let turn = move |s: &mut Settings, d: i32| s.set(&["ui", "confirmButton"], json!(["auto", "a", "b"][(at as i32 + d).rem_euclid(3) as usize]));
                            Row {
                                label: "Confirm button".into(),
                                value: names[at].1.clone(),
                                hint: "Which button selects. Match my controller: A on the right with a Switch pad, as on a Switch; the bottom button with Xbox and PlayStation pads.".into(),
                                press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                                step: Some(Box::new(turn)),
                            }
                        },
                        toggle("Home menu", &["ui", "homeMenu"], true, "Guide (or View held) over a running game pauses it and brings up Resume, Save and Load state, Screenshot, Power and Quit to shelf; on the shelf, Home or P opens it"),
                        {
                            let center = self.text(&["ui", "homeLayout"]) == "center";
                            let turn = move |s: &mut Settings| s.set(&["ui", "homeLayout"], json!(if center { "side" } else { "center" }));
                            Row {
                                label: "Home menu layout".into(),
                                value: if center { "Centred card" } else { "Side panel" }.into(),
                                hint: "A panel at the left edge, or a card in the middle of the screen".into(),
                                press: Some(Box::new(turn)),
                                step: Some(Box::new(move |s: &mut Settings, _| turn(s))),
                            }
                        },
                    ]
                    .into_iter()
                    .chain(self.wiimote_rows())
                    .collect(),
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
                ]
                .into_iter()
                .chain([open("Kid mode", on_off(self.kid_on()), "A PIN for Settings, and only the shelves you choose", Screen::Kid)])
                .chain(rustyshelf_core::keepsetup::safety_list(&paths::data_dir(&self.app_root)).first().map(|(_, m)| {
                    let from = m["version"].as_str().unwrap_or("").to_string();
                    Row {
                        label: "Undo the update's settings changes".into(),
                        value: from.clone(),
                        hint: "Puts back your settings, history and shelves as they were before this version first started (a copy is made each time a new version starts). RustyShelf starts again to do it.".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            if !s.twice("undo-update", &format!("Press again to go back to your settings from before {from}")) {
                                return;
                            }
                            match rustyshelf_core::keepsetup::stage_undo(&paths::data_dir(&s.app_root)) {
                                Ok(_) => restart(),
                                Err(why) => s.say(why),
                            }
                        })),
                        step: None,
                    }
                }))
                .chain(self.moving_rows())
                .chain([self.update_row()])
                .chain(self.notes_rows())
                .collect(),
            },
            Screen::Shelf => {
                let sort = self.text(&["ui", "sort"]);
                let at = SORTS.iter().position(|x| x.0 == sort).unwrap_or(0);
                let turn_sort = move |s: &mut Settings, d: i32| {
                    let n = (at as i32 + d).rem_euclid(SORTS.len() as i32) as usize;
                    s.set(&["ui", "sort"], json!(SORTS[n].0));
                };
                let filter = self.text(&["ui", "filter"]);
                let fat = self.filters.iter().position(|f| f.0 == filter).unwrap_or(0);
                let turn_filter = move |s: &mut Settings, d: i32| {
                    if s.filters.is_empty() {
                        return;
                    }
                    let n = (fat as i32 + d).rem_euclid(s.filters.len() as i32) as usize;
                    let id = s.filters[n].0.clone();
                    s.set(&["ui", "filter"], json!(id));
                };
                let mut rows = vec![
                    Row { label: "Order".into(), value: SORTS[at].1.into(), hint: "The order of every shelf".into(), press: Some(Box::new(move |s: &mut Settings| turn_sort(s, 1))), step: Some(Box::new(turn_sort)) },
                    Row {
                        label: "Show".into(),
                        value: self.filters.get(fat).map_or("All games".into(), |f| f.1.clone()),
                        hint: "Which games every shelf shows".into(),
                        press: Some(Box::new(move |s: &mut Settings| turn_filter(s, 1))),
                        step: Some(Box::new(turn_filter)),
                    },
                ];
                if let Some(c) = &self.chosen {
                    let (id, id2) = (c.game.id.clone(), c.game.id.clone());
                    let fav = self.list(&["ui", "favorites"]).contains(&c.game.id);
                    rows.push(open(&format!("Details of {}", c.game.title), String::new(), "", Screen::Details));
                    rows.push(Row {
                        label: "Favorite".into(),
                        value: on_off(fav),
                        hint: c.game.title.clone(),
                        press: Some(Box::new(move |s: &mut Settings| s.flip("favorites", &id))),
                        step: Some(Box::new(move |s: &mut Settings, _| s.flip("favorites", &id2))),
                    });
                }
                rows.push(open("Settings", String::new(), "", Screen::Root));
                Page { title: "Shelf".into(), lead: String::new(), rows }
            }
            Screen::Details => {
                let Some(c) = &self.chosen else { return Page { title: "Details".into(), lead: String::new(), rows: Vec::new() } };
                let g = &c.game;
                let (id, id2, id3) = (g.id.clone(), g.id.clone(), g.id.clone());
                let fav = self.list(&["ui", "favorites"]).contains(&g.id);
                let mut rows = vec![
                    Row {
                        label: "Play".into(),
                        press: Some(Box::new(|s: &mut Settings| {
                            s.play = true;
                            s.open = false;
                            s.redraw = true;
                        })),
                        ..Row::default()
                    },
                    Row {
                        label: "Favorite".into(),
                        value: on_off(fav),
                        press: Some(Box::new(move |s: &mut Settings| s.flip("favorites", &id))),
                        step: Some(Box::new(move |s: &mut Settings, _| s.flip("favorites", &id2))),
                        ..Row::default()
                    },
                    Row {
                        label: "Hide from the shelf".into(),
                        hint: "It can be shown again in Settings → Hidden games".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            s.flip("hidden", &id3);
                            s.open = false;
                        })),
                        ..Row::default()
                    },
                    open("Add to…", String::new(), "This game on shelves of your own, or on another console's shelf too", Screen::GameCollections),
                ];
                // (beta.14) its Status: the Playing and Finished shelves, and shelves that fill themselves, follow it
                {
                    const STATUS: [(&str, &str); 4] = [("", "Not started"), ("playing", "Playing"), ("finished", "Finished"), ("abandoned", "Abandoned")];
                    let now = self.text(&["games", &g.id, "status"]);
                    let now = if now == "notstarted" { String::new() } else { now };
                    let i = STATUS.iter().position(|x| x.0 == now).unwrap_or(0);
                    let gid = g.id.clone();
                    let turn = move |s: &mut Settings, d: i32| {
                        let v = STATUS[(i as i32 + d).rem_euclid(STATUS.len() as i32) as usize].0;
                        if v.is_empty() {
                            // (set, so the shelves are made again, then taken out: Not started is no status)
                            s.set(&["games", &gid, "status"], Value::Null);
                            if let Some(o) = s.cfg.get_mut("games").and_then(|x| x.get_mut(&gid)).and_then(Value::as_object_mut) {
                                o.remove("status");
                            }
                        } else {
                            s.set(&["games", &gid, "status"], json!(v));
                        }
                    };
                    rows.insert(2, Row { label: "Status".into(), value: STATUS[i].1.into(), hint: "Where you are with it. The Playing and Finished shelves (Settings → Collections) follow it.".into(), press: Some(Box::new({ let turn = turn.clone(); move |s: &mut Settings| turn(s, 1) })), step: Some(Box::new(turn)) });
                }
                if let Some(line) = self.achievements_line() {
                    let whose = match (&g.source, c.console.as_str()) {
                        (Source::Steam { .. }, _) => "Your Steam achievements for this game, read from Steam's own files on this PC",
                        (_, "x360") if self.own_ach_emulator("x360").is_some() => "Your Xbox 360 achievements for this game, read from Xenia's own files on this PC",
                        (_, "ps3") if self.own_ach_emulator("ps3").is_some() => "Your trophies for this game, read from RPCS3's own files on this PC",
                        _ => "Your RetroAchievements for this game",
                    };
                    rows.push(open("Achievements", line, whose, Screen::Achievements));
                }
                if !g.art.manual.is_empty() {
                    let pages = g.art.manual.clone();
                    rows.push(Row {
                        label: "Read the manual".into(),
                        value: format!("{} pages", pages.len()),
                        hint: "Opens the game's manual as a booklet".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            s.manual = Some(pages.clone());
                            s.open = false;
                            s.redraw = true;
                        })),
                        step: None,
                    });
                } else if self.can_find_manual() {
                    rows.push(Row {
                        label: "Find a manual".into(),
                        hint: "Looks for this game's manual on archive.org and GamesDatabase.org (its name is sent to them) and lists what they have to pick from".into(),
                        press: Some(Box::new(|s: &mut Settings| s.find_manual())),
                        ..Row::default()
                    });
                } else if let Some(pdf) = g.art.manual_pdf.clone() {
                    rows.push(Row {
                        label: "Open the manual".into(),
                        value: "PDF".into(),
                        hint: "Opens the game's manual in the program this PC reads PDFs with".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            if let Err(e) = rustyshelf_core::launch::open_folder(&pdf) {
                                s.say(e.to_string());
                            }
                        })),
                        step: None,
                    });
                }
                let shots: Vec<PathBuf> = g.info.as_ref().map(|i| i.shots.iter().map(PathBuf::from).collect()).unwrap_or_default();
                if !shots.is_empty() {
                    rows.push(Row {
                        label: "Screenshots".into(),
                        value: shots.len().to_string(),
                        hint: "The game's screenshots, one at a time: ◀ ▶ to look through them, + − to zoom".into(),
                        press: Some(Box::new(move |s: &mut Settings| {
                            (s.manual, s.gallery, s.open, s.redraw) = (Some(shots.clone()), true, false, true);
                        })),
                        step: None,
                    });
                }
                rows.extend(self.manual_rows());
                rows.extend(self.manual_studio_row());
                if self.can_pick_art() {
                    rows.push(open("Box art", String::new(), "Every version of this game's box the sources have: choose its front, back, spine and disc", Screen::Art));
                }
                if matches!(&g.source, Source::Pc(pc) if rustyshelf_core::windowmode::SOURCES.contains(&pc.source)) {
                    let (mode, id) = (self.window_mode(&g.id), g.id.clone());
                    let turn = move |s: &mut Settings, d: i32| {
                        let modes = rustyshelf_core::windowmode::MODES;
                        let at = modes.iter().position(|m| *m == mode).unwrap_or(0) as i32;
                        s.set(&["games", &id, "window"], json!(modes[(at + d).rem_euclid(modes.len() as i32) as usize]));
                    };
                    let turn2 = turn.clone();
                    rows.push(Row {
                        label: "Window".into(),
                        value: match mode {
                            "borderless" => "Borderless",
                            "windowed" => "Windowed",
                            _ => "The game's own",
                        }
                        .into(),
                        hint: match mode {
                            "borderless" => "Its frame comes off and it fills the screen when it starts",
                            "windowed" => "It stays a window, with the taskbar kept",
                            _ => "The game decides how its window is shown",
                        }
                        .into(),
                        press: Some(Box::new(move |s: &mut Settings| turn(s, 1))),
                        step: Some(Box::new(turn2)),
                    });
                }
                rows.extend(facts(c).into_iter().map(|(label, value)| Row { label, value, ..Row::default() }));
                let about = g.info.as_ref().and_then(|i| i.description()).unwrap_or_default();
                let about: String = if about.chars().count() > 420 { format!("{}…", about.chars().take(419).collect::<String>()) } else { about };
                Page { title: g.title.clone(), lead: about, rows }
            }
            Screen::Find => self.find_page(),
            Screen::Accounts => self.accounts_page(),
            Screen::Phone => self.phone_page(),
            Screen::Romm => self.romm_page(),
            Screen::EpicLibrary => self.epic_library_page(),
            Screen::EpicDownloads => self.epic_downloads_page(),
            Screen::WhatsNew => self.whatsnew_page(),
            Screen::Storage => self.storage_page(),
            Screen::Art => self.art_page(),
            Screen::ArtPart => self.art_part_page(),
            Screen::Kid => self.kid_page(),
            Screen::ArtSets => self.art_sets_page(),
            Screen::Achievements => self.achievements_page(),
            Screen::Hidden => {
                let hidden = self.list(&["ui", "hidden"]);
                let rows: Vec<Row> = hidden
                    .iter()
                    .map(|id| {
                        let id2 = id.clone();
                        Row {
                            label: self.titles.get(id).cloned().unwrap_or_else(|| short(id)),
                            value: "Show again".into(),
                            hint: short(id),
                            press: Some(Box::new(move |s: &mut Settings| s.flip("hidden", &id2))),
                            step: None,
                        }
                    })
                    .collect();
                Page { title: "Hidden games".into(), lead: if rows.is_empty() { "No game is hidden. Details → Hide from the shelf takes one off its shelf.".into() } else { String::new() }, rows }
            }
            Screen::Done => {
                let on = systems().consoles.iter().filter(|c| self.flag(&["systems", &c.id, "enabled"], true) && !self.list(&["systems", &c.id, "romDirs"]).is_empty()).count();
                let mut rows = Vec::new();
                let kinds = self.setup_kinds();
                if rustyshelf_core::emusetup::ON && !kinds.is_empty() {
                    rows.push(self.switch(
                        "Set up my emulators for me",
                        &["emuSetup", "auto"],
                        true,
                        &format!("{}: your controller mapped, the Home menu's Save and Load keys, one BIOS folder and graphics that suit this PC. Emulators you've used before are left as they are. Everything is backed up first, and Settings → each console → Set up for me can undo it.", kinds.join(", ")),
                    ));
                }
                rows.push(Row {
                    label: "Open RustyShelf".into(),
                    press: Some(Box::new(|s: &mut Settings| {
                        s.set(&["setupDone"], json!(true));
                        s.toast = s.auto_setup();
                        s.wizard = false;
                        s.open = false;
                        s.stack = vec![(Screen::Root, 0)];
                    })),
                    ..Row::default()
                });
                Page {
                    title: "You're set".into(),
                    lead: format!("{on} console{} with games. Everything here can be changed later in Settings (S, or View on the pad).", if on == 1 { "" } else { "s" }),
                    rows,
                }
            }
        }
    }
}

/// A date the way Details writes it: "November 21, 1999" from a date that starts YYYY-MM-DD, else the year.
fn release(date: Option<&str>, year: Option<&str>) -> String {
    let parsed = date.and_then(|d| {
        let mut parts = d.get(..10)?.split('-').map(|p| p.parse::<u32>().ok());
        let (y, m, day) = (parts.next()??, parts.next()??, parts.next()??);
        Some(format!("{} {day}, {y}", MONTHS.get(m.checked_sub(1)? as usize)?))
    });
    parsed.or(year.map(String::from)).unwrap_or_default()
}

/// When a game was last played: "Today", "Yesterday", "5 days ago", else how long ago in months or years.
fn last_played(ms: u64) -> String {
    if ms == 0 {
        return "Never".into();
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    match now.saturating_sub(ms) / 86_400_000 {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        d @ 2..=6 => format!("{d} days ago"),
        d @ 7..=59 => format!("{} weeks ago", d / 7),
        d @ 60..=729 => format!("{} months ago", d / 30),
        d => format!("{} years ago", d / 365),
    }
}

/// What Details says of a game, a name and a value a line.
fn facts(c: &Chosen) -> Vec<(String, String)> {
    facts_of(&c.game, &c.emulator, c.plays)
}

/// The emulator a shelf's games run in, by name; nothing for a shelf without one.
pub fn emulator_of(shelf: &rustyshelf_core::library::Shelf) -> String {
    match &shelf.kind {
        ShelfKind::Console { config, .. } if !config.emulator.is_empty() => emuname::label(&config.emulator),
        ShelfKind::Console { console, .. } => console.emulator_name.clone(),
        _ => String::new(),
    }
}

/// What's known of a game, a name and a value a line: `emulator` is what runs it, `plays` its plays.
pub fn facts_of(g: &Game, emulator: &str, plays: Option<Played>) -> Vec<(String, String)> {
    let i = g.info.clone().unwrap_or_default();
    let mut rows: Vec<(&str, String)> = Vec::new();
    let hours = |ms: u64| format!("{:.1} hours", ms as f64 / 3_600_000.0);
    let playtime = match plays {
        Some(p) if p.ms > 0 => hours(p.ms),
        Some(p) if p.count > 0 => format!("Started {} time{}", p.count, if p.count == 1 { "" } else { "s" }),
        _ => "Not played yet".into(),
    };
    let known = i.release.is_some() || i.year.is_some();
    if known {
        rows.push(("Released", release(i.release.as_deref(), i.year.as_deref())));
    }
    if let Some(dev) = &i.dev {
        rows.push(("Developer", dev.clone()));
    }
    if let Some(p) = i.publisher.as_ref().filter(|p| i.dev.as_ref() != Some(p)) {
        rows.push(("Publisher", p.clone()));
    }
    if let Some(genres) = &i.genres {
        rows.push(("Genre", genres.replace(';', ", ")));
    }
    match &g.source {
        Source::Rom => {
            if let Some(p) = &i.players {
                rows.push(("Players", format!("{p}{}", if i.coop.as_deref() == Some("true") { " (co-op)" } else { "" })));
            }
            if let Some(e) = &i.esrb {
                rows.push(("Rating", e.split(" - ").next().unwrap_or(e).to_string()));
            }
            if let Some(r) = i.rating.filter(|r| *r != 0.0) {
                rows.push(("Community score", format!("{r} / 5")));
            }
            rows.push(("Region", if g.region.is_empty() { "Unknown".into() } else { g.region.clone() }));
            let discs = if g.discs.len() > 1 { format!("{} discs", g.discs.len()) } else { String::new() };
            let file: Vec<String> = [g.ext.to_uppercase(), crate::hud::size(g.size_bytes), discs].into_iter().filter(|x| !x.is_empty()).collect();
            rows.push(("File", file.join(" · ")));
            rows.push(("Emulator", emulator.to_string()));
            rows.push(("Playtime", playtime));
            rows.push(("Last played", last_played(plays.map_or(0, |p| p.last))));
        }
        Source::Steam { appid, .. } => {
            if let Some(m) = i.metacritic.filter(|m| *m != 0.0) {
                rows.push(("Metacritic", format!("{m} / 100")));
            }
            rows.push(("Playtime", playtime));
            rows.push(("Last played", last_played(plays.map_or(0, |p| p.last))));
            if g.size_bytes > 0 {
                rows.push(("Install size", crate::hud::size(g.size_bytes)));
            }
            rows.push(("Steam ID", appid.clone()));
        }
        Source::Pc(pc) => {
            rows.insert(0, ("Source", crate::hud::source_name(pc).to_string()));
            if !pc.exe.is_empty() && !known {
                rows.push(("Program", short(&pc.exe)));
                if !pc.args.is_empty() {
                    rows.push(("Launch options", pc.args.clone()));
                }
            }
            rows.push(("Playtime", playtime));
        }
        Source::App { path } => {
            rows.push(("Type", "Streaming app".into()));
            rows.push(("Program", short(path)));
        }
        Source::Tile(_) => {}
    }
    rows.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
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
    mut base: ResMut<Base>,
    plays: Res<crate::play::Plays>,
    mut cursor: ResMut<Cursor>,
    mut playing: ResMut<crate::play::Playing>,
    mut typed: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut toast: ResMut<crate::hud::Toast>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
) {
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let (confirm, back) = s.confirm();
    // read every frame: a key that opened a text mustn't be typed into it a frame later
    let typed: Vec<_> = typed.read().cloned().collect();
    if s.gpu_vendor.is_none() {
        s.gpu_vendor = adapter.map(|a| a.vendor);
    }
    if s.keys_taken {
        return;
    }
    if !s.open {
        if let Some(text) = s.toast.take() {
            toast.text = text;
            toast.until = time.elapsed_secs() + 12.0;
        }
        if s.just_closed {
            s.just_closed = false;
        }
        // shut from outside with something saved: the shelves are read again
        if s.changed {
            (s.changed, s.rearranged) = (false, false);
            *lib = loading::start_library();
            *cursor = Cursor::default();
        }
        // a favourite made on the shelf
        if s.rearranged {
            s.rearranged = false;
            if let (Some(b), Some(cfg)) = (base.0.as_mut(), Config::from_value(&s.cfg)) {
                b.config = cfg;
                loading::reshow(&mut lib, b, &plays.0, &mut cursor);
            }
        }
        if let Some(failed) = start_emulator(&mut s, &mut playing, time.elapsed_secs()).and_then(Result::err) {
            toast.text = failed;
            toast.until = time.elapsed_secs() + 8.0;
        }
        let first = if playing.title.is_some() {
            None
        } else if let Some(page) = s.wanted.take() {
            Some(page)
        } else if keys.just_pressed(KeyCode::Slash) || (keys.just_pressed(KeyCode::KeyF) && (keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight))) {
            Some(Screen::Find)
        } else if keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::F1) {
            Some(Screen::Root)
        } else if keys.just_pressed(KeyCode::KeyG) || keys.just_pressed(KeyCode::Tab) || pad(GamepadButton::Start) {
            Some(Screen::Shelf)
        // (as the Electron app: View / Minus opens Details; Settings is in the shelf menu, Start)
        } else if keys.just_pressed(KeyCode::KeyI) || keys.just_pressed(KeyCode::KeyD) || pad(GamepadButton::West) || pad(GamepadButton::Select) {
            Some(Screen::Details)
        } else {
            None
        };
        let Some(first) = first else { return };
        s.chosen = None;
        if let LibraryState::Ready(l) = &lib.0 {
            if let Some(shelf) = l.shelves.get(cursor.shelf) {
                // on a mixed shelf, the game's own shelf says what runs it
                let own = shelf.from.get(cursor.game).and_then(|id| l.shelves.iter().find(|s| s.id == *id)).unwrap_or(shelf);
                let emulator = emulator_of(own);
                s.chosen = shelf.games.get(cursor.game).filter(|g| !matches!(g.source, Source::Tile(_))).map(|g| Chosen { console: own.id.clone(), emulator, game: g.clone(), plays: shelfview::played(g, &plays.0) });
            }
        }
        if let Some(b) = &base.0 {
            let all: Vec<&Game> = b.shelves.iter().flat_map(|sh| sh.games.iter()).collect();
            s.filters = shelfview::filters(&all, &s.text(&["ui", "filter"]));
            s.titles = all.iter().map(|g| (g.id.clone(), g.title.clone())).collect();
            s.note_folders(&all);
        }
        if first == Screen::Details && s.chosen.is_none() {
            return;
        }
        if first == Screen::Details {
            s.load_achievements(false);
            s.load_igdb();
        }
        if first == Screen::Find {
            if let Some(b) = &base.0 {
                s.find_from(b, &plays.0);
            }
        }
        s.genres = base.0.as_ref().map(|b| rustyshelf_core::shelfview::genre_counts(&b.shelves)).unwrap_or_default();
        s.console_shelves = base.0.as_ref().map(|b| b.shelves.iter().filter(|sh| matches!(sh.kind, ShelfKind::Console { .. }) && !sh.games.is_empty()).map(|sh| (sh.id.clone(), sh.name.clone())).collect()).unwrap_or_default();
        s.open = true;
        s.changed = false;
        s.rearranged = false;
        if matches!(first, Screen::Root | Screen::Sound) {
            s.packs = rustyshelf_core::soundpacks::list(&s.app_root);
        }
        s.stack = vec![(first, 0)];
        s.redraw = true;
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
    s.poll_core();
    s.poll_ra();
    s.poll_bringing();
    s.poll_igdb();
    s.poll_romm();
    s.poll_epic();
    s.poll_moves();
    s.poll_update();
    s.poll_adding();
    s.poll_manuals();
    s.poll_manual_studio();
    s.poll_trophies();
    s.poll_storage();
    s.poll_art(time.elapsed_secs());
    // a dialog is up, or the emulator opened from here: the page waits for it
    if s.picking.is_some() || playing.title.is_some() {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if s.editing.is_some() {
        s.type_text(&typed, &pads, ctrl, shift);
        return;
    }

    // Find a game takes what's typed as the title
    let typing = s.stack.last().is_some_and(|p| p.0 == Screen::Find) && s.find_type(&typed, ctrl, shift);
    let page = s.page();
    let n = page.rows.len();
    let now = time.elapsed_secs();
    let held = {
        let k = keys.pressed(KeyCode::ArrowDown) as i32 - keys.pressed(KeyCode::ArrowUp) as i32;
        let p = pads.iter().map(|p| { let v = -(p.left_stick().y + p.dpad().y); if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 } }).find(|&d| d != 0).unwrap_or(0);
        if k != 0 { k } else { p }
    };
    // a tap can start and end within one frame, so it's never seen as held
    let tap = (keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32;
    let mut move_by = 0;
    if tap != 0 {
        s.repeat = (tap, now + 0.35);
        move_by = tap;
    } else if held != s.repeat.0 {
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
        let was = *at;
        *at = (*at as i32 + move_by).clamp(0, n as i32 - 1) as usize;
        if *at != was {
            s.cues.push(("tick", move_by.signum()));
        }
        s.asked.clear();
        s.note.clear();
        s.redraw = true;
    }
    let at = s.stack.last().map_or(0, |p| p.1).min(n.saturating_sub(1));
    let tab_by = (keys.just_pressed(KeyCode::PageDown) || keys.just_pressed(KeyCode::BracketRight) || pad(GamepadButton::RightTrigger)) as i32 - (keys.just_pressed(KeyCode::PageUp) || keys.just_pressed(KeyCode::BracketLeft) || pad(GamepadButton::LeftTrigger)) as i32;
    if tab_by != 0 && !typing && s.stack.len() == 1 && s.stack.last().is_some_and(|p| p.0 == Screen::Root) && !s.kid_locked() {
        s.tab_step(tab_by);
        return;
    }
    let side = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
    if side != 0 {
        if let Some(step) = page.rows.get(at).and_then(|r| r.step.as_ref()) {
            step(&mut s, side);
        }
    } else if typing {
    } else if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(confirm) {
        if let Some(press) = page.rows.get(at).and_then(|r| r.press.as_ref()) {
            press(&mut s);
        }
    } else if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(back) || (!s.wizard && (keys.just_pressed(KeyCode::F1) || pad(GamepadButton::Start) || pad(GamepadButton::Select))) {
        s.back();
    }

    if let Some(opened) = start_emulator(&mut s, &mut playing, time.elapsed_secs()) {
        s.note = match opened {
            Ok(name) => format!("{name} is open. Settings is back when it closes."),
            Err(e) => e,
        };
        s.redraw = true;
    }
    if !s.open {
        (s.just_closed, s.kid_open) = (true, false);
    }
    // Find a game's choice: the shelf goes to it, on its own console's shelf
    if let (Some((shelf, id)), LibraryState::Ready(l)) = (s.go_to.take(), &lib.0) {
        match l.shelves.iter().position(|sh| sh.id == shelf && !matches!(sh.kind, rustyshelf_core::library::ShelfKind::Mixed)) {
            Some(i) => {
                let at = l.shelves[i].games.iter().position(|g| g.id == id);
                if at.is_none() {
                    toast.text = "That game isn't shown with the filter that's on. Change it in the shelf's menu (G).".into();
                    toast.until = time.elapsed_secs() + 6.0;
                }
                (cursor.shelf, cursor.land) = (i, at.is_none());
                cursor.game = at.unwrap_or(0);
                cursor.flipped = false;
            }
            None => {
                toast.text = "That game's shelf isn't shown.".into();
                toast.until = time.elapsed_secs() + 5.0;
            }
        }
    }
    // a change to what's shown only rearranges the shelves; any other reads them again once Settings closes
    if !s.open && s.changed {
        s.changed = false;
        s.rearranged = false;
        *lib = loading::start_library();
        *cursor = Cursor::default();
    } else if s.rearranged {
        s.rearranged = false;
        if let (Some(b), Some(cfg)) = (base.0.as_mut(), Config::from_value(&s.cfg)) {
            b.config = cfg;
            loading::reshow(&mut lib, b, &plays.0, &mut cursor);
        }
    }
}

/// Opens the emulator that was asked for on its own: its name when it's open, or why it isn't; None when none was
/// asked for.
fn start_emulator(s: &mut Settings, playing: &mut crate::play::Playing, now: f32) -> Option<Result<String, String>> {
    let id = s.open_emulator.take()?;
    let (emulator, args) = (s.emulator_of(&id), s.text(&["systems", &id, "emulatorArgs"]));
    let combo = Config::from_value(&s.cfg).and_then(|c| c.quit_combo());
    let opened = rustyshelf_core::launch::emulator_plan(&id, &emulator, &args, &s.app_root).and_then(|plan| crate::play::open_alone(playing, &id, emuname::label(&emulator), &plan, combo.as_deref(), now));
    Some(opened.map(|()| emuname::label(&emulator)).map_err(|e| e.to_string()))
}

/// Draws the page again when it changed.
pub fn draw(mut commands: Commands, mut s: ResMut<Settings>, face: Option<Res<Face>>, old: Query<Entity, With<SettingsUi>>, mut images: ResMut<Assets<Image>>) {
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
    if let Some(e) = &s.editing {
        e.draw(&mut commands, &face.0, [s.cap("☰"), s.cap("o"), s.cap("sq"), s.cap("tri")]);
        return;
    }
    let page = s.page();
    let tabs = s.stack.len() == 1 && s.stack.last().is_some_and(|p| p.0 == Screen::Root) && !s.kid_locked();
    let link = s.pairing_link().or_else(|| s.romm_link());
    if s.qr.as_ref().map(|q| &q.0) != link.as_ref() {
        if let Some((_, old)) = s.qr.take() {
            images.remove(&old);
        }
        s.qr = link.and_then(|l| Some((l.clone(), images.add(phone::qr_image(&l)?))));
    }
    let code = s.qr.as_ref().map(|q| q.1.clone());
    let preview = s.art_preview(&mut images);
    let n = page.rows.len();
    let at = s.stack.last().map_or(0, |p| p.1).min(n.saturating_sub(1));
    let first = if n <= WINDOW { 0 } else { at.saturating_sub(WINDOW / 2).min(n - WINDOW) };
    let font = |size: f32| TextFont::from_font_size(FontSize::VMin(size)).with_font(face.0.clone());
    let lang = s.text(&["ui", "language"]);
    let t = |text: &str| rustyshelf_core::i18n::tr(&lang, text);
    let hint = t(&if s.note.is_empty() { page.rows.get(at).map(|r| r.hint.clone()).unwrap_or_default() } else { s.note.clone() });
    let style = (s.glyphs(), s.swap());
    let (ok, back) = (rustyshelf_core::pads::button_word(true, style.0, style.1), rustyshelf_core::pads::button_word(false, style.0, style.1));
    let keys = format!(
        "▲ ▼  {}      ◀ ▶  {}      Enter · {ok}  {}      Esc · {back}  {}{}",
        t("Move"),
        t("Change"),
        t("Choose"),
        t("Back"),
        if s.wizard { String::new() } else { format!("      F1 · {}  {}", s.cap("☰"), t("Close")) }
    );
    let keys = if tabs { format!("{keys}      PgUp PgDn · {} {}  {}", s.cap("L1"), s.cap("R1"), t("Tabs")) } else { keys };
    commands
        .spawn((
            SettingsUi,
            Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, padding: UiRect::axes(vmin(8.0), vmin(6.0)), ..default() },
            BackgroundColor(PANEL),
            GlobalZIndex(10),
        ))
        .with_children(|p| {
            p.spawn((Text::new(t(&page.title)), font(4.4), TextColor(STEEL)));
            if tabs {
                p.spawn(Node { margin: UiRect::top(vmin(1.0)), column_gap: vmin(2.2), flex_wrap: FlexWrap::Wrap, ..default() }).with_children(|bar| {
                    bar.spawn((Text::new(format!("{} ", s.cap("L1"))), font(1.8), TextColor(DIM)));
                    for (i, name) in TABS.iter().enumerate() {
                        let on = i == s.tab % TABS.len();
                        bar.spawn((Text::new(t(name)), font(if on { 2.4 } else { 2.1 }), TextColor(if on { RUST } else { DIM })));
                    }
                    bar.spawn((Text::new(format!(" {}", s.cap("R1"))), font(1.8), TextColor(DIM)));
                });
            }
            p.spawn((Node { margin: UiRect::vertical(vmin(1.2)), max_width: percent(80), ..default() }, Text::new(t(&page.lead)), font(2.1), TextColor(DIM)));
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
                    r.spawn((Text::new(t(&row.label)), font(2.5), TextColor(if chosen { STEEL } else { DIM })));
                    r.spawn((Text::new(if LANGUAGES.iter().any(|l| l.1 == row.value) { row.value.clone() } else { t(&row.value) }), font(2.2), TextColor(if chosen { RUST } else { DIM })));
                });
            }
            if first + WINDOW < n {
                p.spawn((Text::new("▼"), font(1.6), TextColor(DIM)));
            }
            if let Some(preview) = preview {
                p.spawn((Node { position_type: PositionType::Absolute, right: vmin(8.0), bottom: vmin(16.0), height: vmin(44.0), ..default() }, ImageNode::new(preview)));
            }
            if let Some(code) = code {
                p.spawn((
                    Node { position_type: PositionType::Absolute, right: vmin(8.0), bottom: vmin(16.0), width: vmin(30.0), height: vmin(30.0), ..default() },
                    ImageNode { image: code, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() },
                ));
            }
            p.spawn(Node { flex_grow: 1.0, ..default() });
            p.spawn((Text::new(hint), font(2.0), TextColor(STEEL)));
            p.spawn((Node { margin: UiRect::top(vmin(1.5)), ..default() }, Text::new(keys), font(1.8), TextColor(DIM)));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_opens_on_eight_tabs_that_go_round() {
        let mut s = Settings { wizard: false, stack: vec![(Screen::Root, 0)], ..Settings::default() };
        s.cfg = json!({ "systems": { "ps2": { "enabled": true, "emulator": "", "romDirs": [] } }, "ui": {} });
        let mut seen = Vec::new();
        for _ in 0..TABS.len() {
            let page = s.page();
            assert!(page.rows.len() > 1, "{}", TABS[s.tab]);
            assert_eq!(page.rows.last().map(|r| r.label.as_str()), Some("Back to the shelf"));
            seen.push(page.lead.clone());
            s.tab_step(1);
        }
        assert_eq!(s.tab, 0);
        seen.dedup();
        assert_eq!(seen.len(), TABS.len());
        // Library starts with what needs attention: a console with no emulator
        let rows = s.page().rows;
        assert!(rows[0].label.starts_with("— Needs attention") && rows[1].label.contains("has no emulator"));
        s.tab_step(-1);
        assert_eq!(TABS[s.tab], "System");
    }
}

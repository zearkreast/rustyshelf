//! Moving along the shelf with keys or a pad, and starting the chosen game.

use std::process::Child;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::{emucfg, emuname, nx2};
use rustyshelf_core::gamewatch::{self, Event, Spec};
use rustyshelf_core::history::History;
use rustyshelf_core::launch::{self, LaunchError};
use rustyshelf_core::paths;
use rustyshelf_core::presence::Presence;
use rustyshelf_core::library::ShelfKind;
use rustyshelf_core::scanner::{Source, Tile};

use crate::hud::Toast;
use crate::padwatch::{self, Watch};
use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

/// What Kid mode says to something only a grown-up may do.
pub const KID_SAYS: &str = "Kid mode is on: a grown-up can change this in Settings";

/// The play history, shared with the Electron app.
#[derive(Resource)]
pub struct Plays(pub History);

impl Default for Plays {
    fn default() -> Self {
        Plays(History::load(&paths::data_dir(&paths::app_root())))
    }
}

/// What Discord shows of the game being played.
#[derive(Resource)]
pub struct Discord(Presence);

impl Default for Discord {
    fn default() -> Self {
        Discord(Presence::start(&paths::env_var))
    }
}

/// A console game's emulator while it runs.
struct Emulator {
    console: String,
    exe: String,
    /// when it started (seconds): one that closes right away wasn't a play
    at: f32,
    /// opened from Settings without a game: no play to count or take back
    alone: bool,
}

/// The emulator's process and its program; the emulators still waiting to be set up, when this one just was.
type Started = Result<(Child, String, Option<Vec<String>>), LaunchError>;

/// The game that's starting or running: the shelf waits for it.
#[derive(Resource, Default)]
pub struct Playing {
    pub title: Option<String>,
    id: String,
    /// still being started (a PS5 build's options are asked for first, which can take seconds)
    starting: Option<Mutex<Receiver<Started>>>,
    child: Option<Mutex<Child>>,
    console: String,
    emulator: Option<Emulator>,
    /// the quit combo, when it's on
    combo: Option<String>,
    watch: Option<Watch>,
    /// asked to close: when it's ended by force if it's still there (seconds)
    closing: Option<f32>,
    /// a streaming app: it's given less time to close
    app: bool,
    /// started through RocketLauncher: closed through its emulator, and its log read when it ends
    rocket: Option<std::time::SystemTime>,
    /// a game as Discord is told of it: a console game's title from its file and its console's name; a PC game's
    /// title and its shelf or store
    shown: Option<(String, String, Option<String>)>,
    /// `shown` is still to be told to Discord: a PC game once it's started, a store's game once it shows up
    tell: bool,
    /// a game Steam or a store started, watched until it ends
    watched: Option<Watched>,
    /// keeps a PC game's window borderless while it runs
    borderless: Option<Helper>,
    /// the game's process, once it's there
    pub pid: Option<u32>,
    /// Home was asked for on a pad while the game is in front; and the Home menu asked for the game to be closed
    pub home_pressed: bool,
    pub close_asked: bool,
    /// a RomM game being downloaded before it starts
    pub romm: Option<Downloading>,
    /// what the boot slide says while a console game is got ready (a RomM download, a 3DS install, a RetroArch core)
    pub line: Option<std::sync::Arc<Mutex<String>>>,
    /// the Switch 2 build to start in, asked for at Play
    pick: BuildPick,
}

/// "Start in which build?" at Play on the Switch 2 shelf: the builds are looked for off the screen's thread, then
/// offered on a card; the one chosen starts the game.
#[derive(Default)]
struct BuildPick {
    /// the game the builds are being looked for (its id, file and title)
    asking: Option<(String, String, String, Mutex<Receiver<nx2::Offer>>)>,
    /// on the card: the game and the builds offered (the row after them adds one)
    shown: Option<(String, String, String, Vec<std::path::PathBuf>)>,
    /// the program picked to add, for that game
    adding: Option<(String, String, String, Mutex<Receiver<Option<std::path::PathBuf>>>)>,
    /// chosen for a game: its build, or none when there were none to offer
    next: Option<(String, Option<std::path::PathBuf>)>,
    /// Play is pressed for it once the card has let go of the keys
    go: bool,
}

impl BuildPick {
    fn busy(&self) -> bool {
        self.asking.is_some() || self.adding.is_some() || self.shown.is_some()
    }

    fn ask(&mut self, cfg: rustyshelf_core::config::Config, id: String, file: String, title: String) {
        let (tx, rx) = channel();
        let f = file.clone();
        std::thread::spawn(move || drop(tx.send(nx2::offer(&cfg, &paths::app_root(), &f))));
        self.asking = Some((id, file, title, Mutex::new(rx)));
    }
}

pub struct Downloading {
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A helper program that runs beside the game and goes when the game does.
struct Helper(Child);

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A game RustyShelf didn't start itself, watched on a thread of its own.
struct Watched {
    events: Mutex<Receiver<Event>>,
    /// asks the game to close; the number is how long it has before it's ended by force (ms)
    close: Sender<u64>,
    /// its process hasn't shown up yet
    waiting: bool,
    store: &'static str,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl Watched {
    fn start(spec: Spec, store: &'static str) -> Watched {
        let (tx, events) = channel();
        let (close, asked) = channel::<u64>();
        std::thread::spawn(move || {
            let mut world = gamewatch::System;
            let mut w = gamewatch::Watch::new(spec, now_ms(), std::process::id());
            while let Some(due) = w.due() {
                let now = now_ms();
                let mut events = match asked.try_recv() {
                    Ok(force_after) => w.close(&mut world, now, force_after),
                    Err(TryRecvError::Empty) => Vec::new(),
                    // the shelf let go of it
                    Err(TryRecvError::Disconnected) => return w.stop(),
                };
                if now >= due {
                    events.extend(w.run(&mut world, now));
                }
                for e in events {
                    let _ = tx.send(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(due.saturating_sub(now).clamp(10, 100)));
            }
        });
        Watched { events: Mutex::new(events), close, waiting: true, store }
    }
}

/// How long a program has to close by itself once asked (seconds): emulators can take a while writing saves and
/// shader caches.
const CLOSE_WAIT: f32 = 8.0;
const CLOSE_WAIT_MS: u64 = 8000;
const APP_CLOSE_WAIT: f32 = 2.5;

impl Playing {
    /// The game's process is there: the quit combo and Home are listened for.
    fn running(&mut self, child: Child) {
        self.watch = Some(Watch::start(self.combo.as_deref(), child.id()));
        self.pid = Some(child.id());
        self.child = Some(Mutex::new(child));
    }

    /// The running game's id, where it has one of the shelf's.
    pub fn game_id(&self) -> &str {
        &self.id
    }

    /// The console the running game is on and its emulator's program, when it's a console's game; and whether
    /// it's a streaming app.
    pub fn running_in(&self) -> (Option<(&str, &str)>, bool) {
        (self.emulator.as_ref().map(|e| (e.console.as_str(), e.exe.as_str())), self.app)
    }
}

/// An emulator that closes by itself this soon (a missing BIOS, keys or firmware, bad launch options) wasn't a play.
const QUICK_EXIT: f32 = 5.0;

impl Playing {
    pub fn is_starting(&self) -> bool {
        self.starting.is_some() || self.watched.as_ref().is_some_and(|w| w.waiting)
    }

    /// A game its store started: watched, with the quit combo listened for while it starts.
    fn watching(&mut self, title: &str, id: &str, combo: Option<String>, spec: Spec, store: &'static str) {
        self.title = Some(title.to_string());
        self.id = id.to_string();
        self.watch = Some(Watch::start(combo.as_deref(), 0));
        self.combo = combo;
        self.watched = Some(Watched::start(spec, store));
    }
}

/// A game's cover where Discord can fetch it: LaunchBox's front cover, or its Steam store header (a screenshot for
/// store info from before the header was kept).
fn image_of(g: &rustyshelf_core::scanner::Game) -> Option<String> {
    let i = g.info.as_ref()?;
    i.front.clone().or_else(|| i.header.clone()).or_else(|| if matches!(g.source, Source::Rom) { None } else { i.shots.first().cloned() })
}

/// Starts a console's emulator without a game (Settings → Emulator & advanced); the shelf waits for it as for a game.
pub fn open_alone(playing: &mut Playing, console: &str, name: String, plan: &launch::Plan, quit_combo: Option<&str>, now: f32) -> Result<(), LaunchError> {
    let exe = plan.exe.to_string_lossy().into_owned();
    emucfg::before_launch(console, &exe, quit_combo, &paths::env_var);
    let child = launch::start(plan)?;
    *playing = Playing { title: Some(name), combo: quit_combo.map(String::from), emulator: Some(Emulator { console: console.to_string(), exe, at: now, alone: true }), ..Playing::default() };
    playing.running(child);
    Ok(())
}

/// The game the Surprise me case landed on: its shelf, and its place there.
#[derive(Resource, Default)]
pub struct Surprise {
    pub pick: Option<(String, usize)>,
}

impl Surprise {
    /// The place of the game it landed on, on the shelf `shelf`.
    pub fn on(&self, shelf: &str) -> Option<usize> {
        self.pick.as_ref().filter(|p| p.0 == shelf).map(|p| p.1)
    }
}

/// Holding a direction steps again after a moment, then quickly.
#[derive(Resource, Default)]
pub struct Repeat {
    dir: i32,
    /// when the direction went down
    since: f32,
    next: f32,
    /// when Y went down on the pad, until it's let go or has been held long enough; and the left stick's click
    y_since: Option<f32>,
    stick_since: Option<f32>,
}

/// seconds Y is held for a favourite
const HOLD: f32 = 0.45;
/// A held direction's first repeat, and how soon each next one comes as it's held longer (app.js's pad repeat), in
/// seconds at Settings → Controls → Scroll speed's Normal.
const FIRST_REPEAT: f32 = 0.32;
fn repeat_gap(held_for: f32, pace: f32) -> f32 {
    (if held_for > 1.4 * pace { 0.045 } else if held_for > 0.7 * pace { 0.075 } else { 0.11 }) * pace
}

pub fn navigate(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    time: Res<Time>,
    lib: Res<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    mut repeat: ResMut<Repeat>,
    mut playing: ResMut<Playing>,
    mut plays: ResMut<Plays>,
    mut toast: ResMut<Toast>,
    mut settings: ResMut<crate::settings::Settings>,
    mut exit: MessageWriter<AppExit>,
    mut held: Local<padwatch::Held>,
    (mut sounds, mut rumble): (ResMut<crate::sound::Sounds>, ResMut<crate::rumble::Rumble>),
    mut surprise: ResMut<Surprise>,
    mut slide: ResMut<crate::boot::Slide>,
    covers: Res<crate::shelf::Covers>,
) {
    // Details' Play starts the game under the cursor
    let asked = std::mem::take(&mut settings.play);
    // Settings has the keys while it's open, and so has the Home menu
    if settings.open || settings.keys_taken || (settings.just_closed && !asked) || playing.pick.busy() {
        return;
    }
    if let Some(d) = playing.romm.as_ref().filter(|_| keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pads.iter().any(|p| p.just_pressed(settings.confirm().1))) {
        // the part is kept for a resume
        d.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        toast.text = "Stopping the download…".into();
        toast.until = time.elapsed_secs() + 3.0;
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && !asked && playing.watched.is_none() {
        if settings.kid_on() {
            (toast.text, toast.until) = (KID_SAYS.into(), time.elapsed_secs() + 3.0);
        } else {
            exit.write(AppExit::Success);
        }
    }
    if playing.title.is_some() {
        // a combo still held from closing the game mustn't count on the shelf
        *held = padwatch::Held::default();
        // back on the starting screen before a store's game showed up: no longer waited for
        if let Some(w) = playing.watched.as_ref().filter(|w| w.waiting) {
            if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pads.iter().any(|p| p.just_pressed(settings.confirm().1)) {
                let _ = w.close.send(CLOSE_WAIT_MS);
            }
        }
        return;
    }
    let LibraryState::Ready(l) = &lib.0 else { return };
    let combo = l.config.quit_combo();
    // held on the shelf, the quit combo closes RustyShelf
    if combo.as_deref().is_some_and(|c| padwatch::held_here(&mut held, &pads, padwatch::mask(c), time.elapsed().as_millis() as u64)) {
        if settings.kid_on() {
            (toast.text, toast.until) = (KID_SAYS.into(), time.elapsed_secs() + 3.0);
        } else {
            exit.write(AppExit::Success);
        }
    }
    if l.shelves.is_empty() {
        return;
    }

    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let held_x = {
        let k = keys.pressed(KeyCode::ArrowRight) as i32 - keys.pressed(KeyCode::ArrowLeft) as i32;
        let p = pads.iter().map(|p| { let v = p.left_stick().x + p.dpad().x; if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 } }).find(|&d| d != 0).unwrap_or(0);
        if k != 0 { k } else { p }
    };
    // a tap can start and end within one frame, so it's never seen as held
    let tap = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32
        - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
    let now = time.elapsed_secs();
    let pace = settings.scroll_pace();
    let mut step = 0;
    if tap != 0 || held_x != repeat.dir {
        step = if tap != 0 { tap } else { held_x };
        (repeat.dir, repeat.since, repeat.next) = (step, now, now + FIRST_REPEAT * pace);
    } else if held_x != 0 && now >= repeat.next {
        repeat.next = now + repeat_gap(now - repeat.since, pace);
        step = held_x;
    }
    // a game asked for is the one under the cursor now: nothing moves it first
    if asked {
        step = 0;
    }

    let n_shelves = l.shelves.len();
    let shelf_step = (keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::PageDown) || pad(GamepadButton::RightTrigger2)) as i32
        - (keys.just_pressed(KeyCode::ArrowUp) || keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::PageUp) || pad(GamepadButton::LeftTrigger2)) as i32;
    if shelf_step != 0 && !asked {
        cursor.shelf = (cursor.shelf as i32 + shelf_step).rem_euclid(n_shelves as i32) as usize;
        cursor.land = true;
        sounds.play("shelf", shelf_step);
        rumble.pulse(0.0, 0.25, 60);
        return;
    }

    let shelf = &l.shelves[cursor.shelf];
    let n = shelf.games.len();
    let was = cursor.game;
    if step != 0 && n > 0 {
        cursor.game = (cursor.game as i32 + step).clamp(0, n as i32 - 1) as usize;
    }
    // to the next letter's first game, or back to this letter's first and then the one before's (app.js letter())
    let letter_step = (keys.just_pressed(KeyCode::KeyC) || pad(GamepadButton::RightTrigger)) as i32 - (keys.just_pressed(KeyCode::KeyZ) || pad(GamepadButton::LeftTrigger)) as i32;
    if letter_step != 0 && n > 0 && !asked {
        let letter = |g: &rustyshelf_core::scanner::Game| g.title.chars().next().filter(char::is_ascii_alphabetic).map_or('#', |c| c.to_ascii_uppercase());
        let starts: Vec<usize> = (0..n).filter(|&i| i == 0 || letter(&shelf.games[i]) != letter(&shelf.games[i - 1])).collect();
        if starts.len() > 1 {
            let k = starts.iter().rposition(|&st| st <= cursor.game).unwrap_or(0);
            cursor.game = if letter_step > 0 {
                starts[(k + 1) % starts.len()]
            } else if cursor.game > starts[k] {
                starts[k]
            } else {
                starts[(k + starts.len() - 1) % starts.len()]
            };
        }
    }
    // (with the Home menu on, Home opens it)
    if keys.just_pressed(KeyCode::Home) && !settings.home_menu().0 {
        cursor.game = 0;
    }
    if keys.just_pressed(KeyCode::End) {
        cursor.game = n.saturating_sub(1);
    }
    if cursor.game != was {
        cursor.flipped = false;
        sounds.play("tick", if cursor.game < was { -1 } else { 1 });
    }
    // Y on the pad: a tap turns the case, a hold makes the game a favourite
    let (y_down, y_up) = (pad(GamepadButton::North), pads.iter().any(|p| p.just_released(GamepadButton::North)));
    if y_down {
        repeat.y_since = Some(now);
    }
    let y_held = repeat.y_since.is_some_and(|since| now - since >= HOLD);
    let y_tap = y_up && repeat.y_since.take().is_some();
    if y_held {
        repeat.y_since = None;
    }
    // the chosen case turns round to show its back, and round again
    if (keys.just_pressed(KeyCode::KeyF) || y_tap) && n > 0 {
        cursor.flipped = !cursor.flipped;
        sounds.play("flip", 1);
    }
    let tile = |g: &rustyshelf_core::scanner::Game| match g.source {
        Source::Tile(t) => Some(t),
        _ => None,
    };
    if let Some(g) = shelf.games.get(cursor.game).filter(|g| tile(g).is_none() && (keys.just_pressed(KeyCode::KeyH) || y_held)) {
        let on = settings.toggle_favorite(&g.id);
        sounds.play(if on { "toast" } else { "back" }, 1);
        rumble.pulse(0.0, 0.3, 70);
        toast.text = if on { format!("♥ {} added to favorites", g.title) } else { format!("Removed {} from favorites", g.title) };
        toast.until = now + 3.0;
    }

    let go = asked || keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(settings.confirm().0);
    // the left stick's click: a tap rolls the Surprise me case, a hold opens Find a game
    let (stick_down, stick_up) = (pad(GamepadButton::LeftThumb), pads.iter().any(|p| p.just_released(GamepadButton::LeftThumb)));
    if stick_down {
        repeat.stick_since = Some(now);
    }
    if repeat.stick_since.is_some_and(|since| now - since >= HOLD) {
        repeat.stick_since = None;
        return settings.open_find();
    }
    let stick_tap = stick_up && repeat.stick_since.take().is_some();
    // R: the Surprise me case rolls, and rolls again
    let mut roll = false;
    if keys.just_pressed(KeyCode::KeyR) || stick_tap {
        match shelf.games.iter().position(|g| tile(g) == Some(Tile::Surprise)) {
            Some(i) => {
                (cursor.game, cursor.flipped, roll) = (i, false, true);
            }
            None => {
                toast.text = "Nothing to shuffle on this shelf".into();
                toast.until = now + 3.0;
            }
        }
    } else if go {
        // one of the shelf's own cases: what it's for, not a game to start
        match shelf.games.get(cursor.game).and_then(tile) {
            // a second press plays the game it landed on, from that game's own place
            Some(Tile::Surprise) => match surprise.pick.take().filter(|p| p.0 == shelf.id) {
                Some((_, i)) => cursor.game = i,
                None => roll = true,
            },
            Some(Tile::Emulator) => {
                sounds.play("go", 1);
                settings.open_emulator(&shelf.id);
                return;
            }
            Some(Tile::AddGames) => return settings.open_console(&shelf.id),
            Some(Tile::AddCustom) => return settings.open_custom(),
            None => {}
        }
    }
    if roll {
        let pool: Vec<usize> = (0..n).filter(|&i| tile(&shelf.games[i]).is_none() && !shelf.games[i].not_installed).collect();
        if !pool.is_empty() {
            let chance = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
            surprise.pick = Some((shelf.id.clone(), pool[chance % pool.len()]));
            sounds.play("flip", 1);
            rumble.pulse(0.9, 0.6, 320);
            rumble.later(0.38, 0.3, 0.6, 180);
        }
        return;
    }
    let Some(g) = shelf.games.get(cursor.game).filter(|_| go) else { return };
    sounds.play("go", 1);
    rumble.pulse(0.6, 0.3, 140);
    // a game on Home, All or a collection starts as it does on its own shelf
    let own = shelf.from.get(cursor.game).and_then(|id| l.shelves.iter().find(|s| s.id == *id)).unwrap_or(shelf);
    match (&own.kind, &g.source) {
        // Steam starts it. With Seamless play it's watched as a game RustyShelf started; without, Steam looks after
        // it and the shelf stays as it is
        (_, Source::Steam { appid, .. }) => match launch::launch_steam(appid) {
            Ok(()) if l.config.seamless() => {
                let spec = Spec { kind: "steam".into(), appid: appid.clone(), dir: g.file.to_string_lossy().into_owned(), exe: String::new(), title: g.title.clone() };
                playing.watching(&g.title, &g.id, combo, spec, "Steam");
                playing.shown = Some((g.title.clone(), "Steam".into(), image_of(g)));
            }
            Ok(()) => {
                plays.0.touch(&g.id);
                toast.text = format!("{} is starting in Steam.", g.title);
                toast.until = now + 5.0;
            }
            Err(e) => {
                toast.text = e.to_string();
                sounds.play("error", 1);
                toast.until = now + 5.0;
            }
        },
        // a PC game's launch is counted, its time isn't
        (_, Source::Pc(pc)) if cfg!(target_os = "linux") && pc.source == "epic" && pc.installed == Some(false) => {
            toast.text = settings.epic_offer(&pc.app_name, &g.title, now);
            toast.until = now + 6.0;
        }
        (_, Source::Pc(pc)) if launch::is_linux_pc(pc) => {
            let (tx, rx) = channel();
            let (game, cfg, root) = ((**pc).clone(), l.config.clone(), l.app_root.clone());
            std::thread::spawn(move || {
                let started = launch::launch_pc_with(&game, &cfg, &root).and_then(|(child, notice)| {
                    if let Some(n) = notice {
                        info!("{n}");
                    }
                    child.map(|c| (c, String::new(), None)).ok_or_else(|| LaunchError::Refused(format!("{} didn't start.", game.title)))
                });
                let _ = tx.send(started);
            });
            if pc.source == "epic" {
                toast.text = "Getting the game ready with Epic…".into();
                toast.until = now + 6.0;
            }
            playing.title = Some(g.title.clone());
            playing.id = g.id.clone();
            playing.combo = combo;
            playing.starting = Some(Mutex::new(rx));
        }
        (_, Source::Pc(pc)) => match launch::launch_pc_with(pc, &l.config, &l.app_root) {
            Ok((Some(child), notice)) => {
                if let Some(n) = notice {
                    toast.text = n;
                    toast.until = now + 8.0;
                }
                plays.0.touch(&g.id);
                playing.title = Some(g.title.clone());
                (playing.shown, playing.tell) = (Some((g.title.clone(), if own.id == "pc" { "PC".to_string() } else { own.name.clone() }, image_of(g))), true);
                playing.combo = combo;
                if settings.window_mode(&g.id) == "borderless" {
                    let dir = if pc.dir.is_empty() { rustyshelf_core::js::dirname(&pc.exe) } else { pc.dir.clone() };
                    playing.borderless = rustyshelf_core::windowmode::borderless(child.id(), &dir).map(Helper);
                }
                playing.running(child);
            }
            Ok((None, _)) if l.config.seamless() => {
                let spec = Spec { kind: pc.source.to_string(), appid: String::new(), dir: pc.dir.clone(), exe: pc.exe.clone(), title: g.title.clone() };
                playing.watching(&g.title, &g.id, combo, spec, crate::hud::source_name(pc));
                playing.shown = Some((g.title.clone(), crate::hud::source_name(pc).into(), image_of(g)));
            }
            // its store starts it and looks after it
            Ok((None, _)) => {
                plays.0.touch(&g.id);
                toast.text = format!("{} is starting in {}.", g.title, crate::hud::source_name(pc));
                toast.until = now + 5.0;
            }
            Err(e) => {
                toast.text = e.to_string();
                sounds.play("error", 1);
                toast.until = now + 6.0;
            }
        },
        // Android: the console's emulator app, handed the game as the app wants it
        #[cfg(target_os = "android")]
        (ShelfKind::Console { console, config }, Source::Rom) => {
            crate::android_saf::start(console, config, g);
            toast.text = format!("Starting {}…", g.title);
            toast.until = now + 3.0;
        }
        #[cfg(not(target_os = "android"))]
        (ShelfKind::Console { console, config }, Source::Rom) => {
            let shelf_file = g.file.to_string_lossy().into_owned();
            let build = match playing.pick.next.take() {
                Some((id, b)) if id == g.id => b,
                _ if console.id == "sw2" && settings.sw2_asks() => {
                    let cfg = settings.config_now().unwrap_or_else(|| l.config.clone());
                    return playing.pick.ask(cfg, g.id.clone(), shelf_file, g.title.clone());
                }
                _ => None,
            };
            if let Some(b) = &build {
                settings.remember_sw2_build(&shelf_file, b);
            }
            let cfg = if build.is_some() { settings.config_now().unwrap_or_else(|| l.config.clone()) } else { l.config.clone() };
            let (tx, rx) = channel();
            let (console, sc, file, id, root) = ((**console).clone(), config.clone(), g.file.clone(), g.id.clone(), l.app_root.clone());
            playing.console = console.id.clone();
            let waiting = settings.at_launch();
            let (cancel, line) = (std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)), std::sync::Arc::new(Mutex::new(String::new())));
            if g.romm.is_some_and(|r| r.cloud) {
                playing.romm = Some(Downloading { cancel: cancel.clone() });
            }
            playing.line = Some(line.clone());
            std::thread::spawn(move || {
                let data = paths::data_dir(&root);
                let seal = rustyshelf_core::seal::key(&paths::env_var);
                let file = match rustyshelf_core::romm::fetch_shelf_game(&cfg, seal.as_ref(), &data, &root, &console.id, &sc.rom_dirs, &file, &mut |t| *line.lock().unwrap() = t, Some(&cancel)) {
                    Some(Ok(got)) => got,
                    Some(Err(why)) => return drop(tx.send(Err(LaunchError::Refused(why)))),
                    None => file,
                };
                let started = launch::plan(&console, &sc, &file, &id, &cfg, &root, build.as_deref(), &mut |t: &str| *line.lock().unwrap() = t.to_string()).and_then(|p| {
                    let exe = p.exe.to_string_lossy().into_owned();
                    let still = waiting.and_then(|w| w.run(&p.exe));
                    if !launch::is_rocket(&exe) {
                        emucfg::before_launch(&console.id, &exe, cfg.quit_combo().as_deref(), &paths::env_var);
                    }
                    // Dolphin connects to the Wii remote itself: RustyShelf lets go of it first
                    let dolphin = cfg!(windows) && rustyshelf_core::wiimote::for_dolphin(&console.id, &p.exe);
                    if dolphin {
                        rustyshelf_core::wiimote::REMOTE.pause();
                    }
                    let child = launch::start(&p).inspect_err(|_| {
                        if dolphin {
                            rustyshelf_core::wiimote::REMOTE.resume();
                        }
                    })?;
                    Ok((child, exe, still))
                });
                let _ = tx.send(started);
            });
            playing.title = Some(g.title.clone());
            playing.id = g.id.clone();
            let file = g.file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            playing.shown = Some((rustyshelf_core::titles::clean_title(&file, false).title, own.name.clone(), image_of(g)));
            playing.combo = combo;
            playing.starting = Some(Mutex::new(rx));
            sounds.play_later("boot", now, 0.7);
            // the console's boot slide covers the emulator starting, unless it's switched off for this console
            if l.config.raw().get("systems").and_then(|s| s.get(&own.id)).and_then(|s| s.get("insertAnim")) != Some(&false.into()) {
                slide.start(now, &own.id, &g.title, &crate::settings::emulator_of(own), covers.of(g, &own.id));
            }
        }
        (_, Source::App { path }) => match launch::launch_app(&g.title, path, &l.app_root) {
            Ok(child) => {
                plays.0.touch(&g.id);
                playing.title = Some(g.title.clone());
                playing.combo = combo;
                playing.app = true;
                playing.running(child);
            }
            Err(e) => {
                toast.text = e.to_string();
                sounds.play("error", 1);
                toast.until = now + 6.0;
            }
        },
        (ShelfKind::Pc | ShelfKind::Apps | ShelfKind::Mixed, Source::Rom) | (_, Source::Tile(_)) => {}
    }
}

/// The Switch 2 build picker's card, and the shelf's build put right at the start when it's gone.
pub fn pick_build(mut playing: ResMut<Playing>, mut card: ResMut<crate::card::Card>, mut settings: ResMut<crate::settings::Settings>, mut toast: ResMut<Toast>, time: Res<Time>, mut gone: Local<Option<Mutex<Receiver<Option<nx2::Build>>>>>, mut looked: Local<bool>) {
    if !std::mem::replace(&mut *looked, true) {
        if let Some(cfg) = settings.config_now() {
            let (tx, rx) = channel();
            std::thread::spawn(move || drop(tx.send(nx2::replacement(&cfg, &paths::app_root()))));
            *gone = Some(Mutex::new(rx));
        }
    }
    if let Some(b) = gone.as_ref().and_then(|rx| rx.lock().unwrap().try_recv().ok()) {
        *gone = None;
        if let Some(b) = b {
            settings.replace_sw2_build(&b);
        }
    }
    let now = time.elapsed_secs();
    let pick = &mut playing.pick;
    if pick.go {
        if !settings.keys_taken {
            pick.go = false;
            settings.play = true;
        }
        return;
    }
    let offered = pick.asking.as_ref().map(|a| a.3.lock().unwrap().try_recv());
    match offered {
        None | Some(Err(TryRecvError::Empty)) => {}
        Some(Err(TryRecvError::Disconnected)) => pick.asking = None,
        Some(Ok(offer)) => {
            let (id, file, title, _) = pick.asking.take().unwrap();
            if offer.builds.is_empty() {
                // none to choose from: the shelf's emulator, as without the list
                (pick.next, pick.go) = (Some((id, None)), true);
                return;
            }
            let day = |ms: f64| chrono::DateTime::from_timestamp_millis(ms as i64).map(|d| d.with_timezone(&chrono::Local).format("%b %-d").to_string()).unwrap_or_default();
            let mut options: Vec<(String, String)> = offer.builds.iter().map(|b| (b.label.clone(), if offer.nup && !b.nup { "may not open .nup games".into() } else { day(b.date) })).collect();
            options.push(("+ Add another emulator…".into(), "eglantine.exe, lichii.exe…".into()));
            let first = offer.first.as_ref().and_then(|f| offer.builds.iter().position(|b| b.exe == *f)).unwrap_or(0);
            card.show(crate::card::Content {
                kick: "Choose a build".into(),
                title: "Start in which build?".into(),
                sub: title.clone(),
                keys: vec![("Enter".into(), "Start".into()), ("Esc".into(), "Back".into())],
                options,
                first,
                ..default()
            });
            pick.shown = Some((id, file, title, offer.builds.into_iter().map(|b| b.exe).collect()));
        }
    }
    if let Some(got) = pick.adding.as_ref().and_then(|a| a.3.lock().unwrap().try_recv().ok()) {
        let (id, file, title, _) = pick.adding.take().unwrap();
        if let Some(exe) = got {
            toast.text = match settings.add_sw2_build(&exe) {
                Ok(()) => "Emulator added".into(),
                Err(e) => e,
            };
            toast.until = now + 4.0;
        }
        if let Some(cfg) = settings.config_now() {
            playing.pick.ask(cfg, id, file, title);
        }
        return;
    }
    if pick.shown.is_none() {
        return;
    }
    let Some(o) = card.take() else { return };
    let (id, file, title, exes) = pick.shown.take().unwrap();
    match o {
        crate::card::Outcome::Picked(i) if i < exes.len() => (pick.next, pick.go) = (Some((id, Some(exes[i].clone()))), true),
        crate::card::Outcome::Picked(_) => {
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                if let Some(answer) = std::env::var_os("RUSTYSHELF_PICK") {
                    return drop(tx.send(Some(answer.into())));
                }
                let _ = tx.send(crate::dialog::pick_file("Choose a Switch 2 emulator (eglantine.exe, lichii.exe, orchid.exe…)", true));
            });
            pick.adding = Some((id, file, title, Mutex::new(rx)));
        }
        crate::card::Outcome::Back => {}
    }
}

/// The shelf takes input again once the emulator has closed, or if it didn't start.
pub fn watch_game(
    mut playing: ResMut<Playing>,
    mut plays: ResMut<Plays>,
    mut toast: ResMut<Toast>,
    mut settings: ResMut<crate::settings::Settings>,
    pads: Query<&Gamepad>,
    discord: Res<Discord>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut again: ResMut<crate::loading::Reshow>,
    time: Res<Time>,
) {
    if std::mem::take(&mut settings.discord_changed) {
        let (on, id) = settings.discord();
        discord.0.configure(on, &id);
    }
    if std::mem::take(&mut playing.tell) {
        if let Some((title, shelf, image)) = &playing.shown {
            discord.0.set_game(title, shelf, image.as_deref());
        }
    }
    let title = playing.title.clone().unwrap_or_default();
    let now = time.elapsed_secs();
    if playing.watch.as_mut().is_some_and(|w| w.home(&pads, time.elapsed().as_millis() as u64)) {
        playing.home_pressed = true;
    }
    if playing.watched.is_some() {
        let p = &mut *playing;
        let w = p.watched.as_mut().expect("a watched game");
        let (events, lost): (Vec<Event>, bool) = {
            let rx = w.events.lock().unwrap();
            let mut events = Vec::new();
            let lost = loop {
                match rx.try_recv() {
                    Ok(e) => events.push(e),
                    Err(TryRecvError::Empty) => break false,
                    Err(TryRecvError::Disconnected) => break true,
                }
            };
            (events, lost)
        };
        let mut over = lost;
        for e in events {
            match e {
                Event::Started { pid, exe, .. } => {
                    info!("Watching {title} (process {pid}, {exe})");
                    let image = p.shown.as_ref().and_then(|s| s.2.clone());
                    discord.0.set_game(&title, w.store, image.as_deref());
                    w.waiting = false;
                    plays.0.start(&p.id);
                    p.watch = Some(Watch::start(p.combo.as_deref(), pid));
                    p.pid = Some(pid);
                }
                Event::Handover { pid, .. } => {
                    p.watch = Some(Watch::start(p.combo.as_deref(), pid));
                    p.pid = Some(pid);
                }
                Event::Ended { cancelled, .. } => {
                    if !cancelled {
                        plays.0.end();
                    }
                    over = true;
                }
                Event::Timeout => {
                    toast.text = format!("{title} didn't show up. If it's still starting, {} has it now.", w.store);
                    toast.until = now + 8.0;
                    over = true;
                }
                Event::Forced(_) => warn!("{title} didn't close when asked: ended"),
            }
        }
        if over {
            discord.0.clear();
            *playing = Playing::default();
            again.0 = true;
        } else if p.closing.is_none() && (p.watch.as_mut().is_some_and(|x| x.quit(&pads, time.elapsed().as_millis() as u64)) | std::mem::take(&mut p.close_asked)) {
            info!("Closing {title}");
            let _ = w.close.send(CLOSE_WAIT_MS);
            p.closing = Some(f32::INFINITY);
            toast.text = format!("Closing {title}…");
            toast.until = now + 4.0;
        }
        return;
    }
    let started = playing.starting.as_ref().map(|rx| rx.lock().unwrap().try_recv());
    match started {
        None | Some(Err(TryRecvError::Empty)) => {}
        Some(Ok(Ok((child, exe, still)))) => {
            info!("Launched {title}");
            playing.line = None;
            if playing.romm.take().is_some() {
                crate::loading::READ_AGAIN.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            if let Some(list) = still {
                settings.set_pending(list);
            }
            plays.0.start(&playing.id);
            if let Some((title, shelf, image)) = &playing.shown {
                discord.0.set_game(title, shelf, image.as_deref());
            }
            playing.starting = None;
            playing.running(child);
            playing.rocket = launch::is_rocket(&exe).then(std::time::SystemTime::now);
            // (a Linux PC game started through Wine or Proton has no emulator)
            if !exe.is_empty() {
                playing.emulator = Some(Emulator { console: playing.console.clone(), exe, at: now, alone: playing.rocket.is_some() });
            }
        }
        Some(Ok(Err(e))) => {
            warn!("{title}: {e}");
            sounds.play("error", 1);
            toast.text = e.to_string();
            toast.until = now + 6.0;
            *playing = Playing::default();
        }
        Some(Err(TryRecvError::Disconnected)) => *playing = Playing::default(),
    }
    let pid = playing.child.as_ref().map(|c| c.lock().unwrap().id());
    if let Some(pid) = pid {
        let held = playing.watch.as_mut().is_some_and(|w| w.quit(&pads, time.elapsed().as_millis() as u64)) | std::mem::take(&mut playing.close_asked);
        match playing.closing {
            None if held => {
                info!("Closing {title}");
                if playing.rocket.is_some() {
                    launch::close_children(pid);
                } else {
                    launch::close_program(pid, false);
                }
                playing.closing = Some(now + if playing.app { APP_CLOSE_WAIT } else { CLOSE_WAIT });
                toast.text = format!("Closing {title}…");
                toast.until = now + 4.0;
            }
            // a "really quit?" question (or anything else) is holding it open
            Some(by) if now >= by => {
                warn!("{title} didn't close when asked: ended");
                launch::close_program(pid, true);
                playing.closing = Some(f32::INFINITY);
            }
            _ => {}
        }
    }
    let exited = match &playing.child {
        Some(c) => match c.lock().unwrap().try_wait() {
            Ok(None) => None,
            Ok(Some(status)) => Some(status.code()),
            Err(_) => Some(None),
        },
        None => None,
    };
    let Some(code) = exited else { return };
    discord.0.clear();
    let emulator = playing.emulator.take();
    // RocketLauncher always exits "normally": its log says whether the game failed
    let rocket_said = playing.rocket.zip(emulator.as_ref()).map(|(at, e)| rustyshelf_core::rocket::last_error(std::path::Path::new(&e.exe), at)).filter(|e| !e.is_empty());
    if let Some(err) = rocket_said {
        warn!("RocketLauncher said: {err}");
        toast.text = format!("RocketLauncher: {err}");
        toast.until = now + 10.0;
    } else if let Some(e) = emulator.as_ref().filter(|_| playing.rocket.is_none()) {
        let (console, exe) = (e.console.clone(), e.exe.clone());
        std::thread::spawn(move || emucfg::after_exit(&console, &exe, &paths::env_var));
    }
    match emulator.filter(|e| !e.alone).map(|e| (emuname::label(&e.exe), e.at)) {
        Some((emulator, at)) if now - at < QUICK_EXIT => {
            plays.0.undo();
            let code = code.filter(|c| *c != 0).map(|c| format!(" (code {c})")).unwrap_or_default();
            toast.text = format!("{emulator} closed right away{code}. Open {emulator} on its own to check its setup: a missing BIOS, keys or firmware is the usual cause.");
            toast.until = now + 12.0;
        }
        _ => plays.0.end(),
    }
    *playing = Playing::default();
    again.0 = true;
}

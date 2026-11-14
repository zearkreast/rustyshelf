//! Moving along the shelf with keys or a pad, and starting the chosen game.

use std::process::Child;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::{emucfg, emuname};
use rustyshelf_core::history::History;
use rustyshelf_core::launch::{self, LaunchError};
use rustyshelf_core::paths;
use rustyshelf_core::library::ShelfKind;
use rustyshelf_core::scanner::Source;

use crate::hud::Toast;
use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

/// The play history, shared with the Electron app.
#[derive(Resource)]
pub struct Plays(pub History);

impl Default for Plays {
    fn default() -> Self {
        Plays(History::load(&paths::data_dir(&paths::app_root())))
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
}

/// An emulator that closes by itself this soon (a missing BIOS, keys or firmware, bad launch options) wasn't a play.
const QUICK_EXIT: f32 = 5.0;

impl Playing {
    pub fn is_starting(&self) -> bool {
        self.starting.is_some()
    }
}

/// Starts a console's emulator without a game (Settings → Emulator & advanced); the shelf waits for it as for a game.
pub fn open_alone(playing: &mut Playing, console: &str, name: String, plan: &launch::Plan, quit_combo: Option<&str>, now: f32) -> Result<(), LaunchError> {
    let exe = plan.exe.to_string_lossy().into_owned();
    emucfg::before_launch(console, &exe, quit_combo, &paths::env_var);
    let child = launch::start(plan)?;
    *playing = Playing { title: Some(name), child: Some(Mutex::new(child)), emulator: Some(Emulator { console: console.to_string(), exe, at: now, alone: true }), ..Playing::default() };
    Ok(())
}

/// Holding a direction steps again after a moment, then quickly.
#[derive(Resource, Default)]
pub struct Repeat {
    dir: i32,
    next: f32,
}

const FIRST_REPEAT: f32 = 0.35;
const REPEAT: f32 = 0.08;

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
) {
    // Details' Play starts the game under the cursor
    let asked = std::mem::take(&mut settings.play);
    // Settings has the keys while it's open
    if settings.open || (settings.just_closed && !asked) {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) && !asked {
        exit.write(AppExit::Success);
    }
    if playing.title.is_some() {
        return;
    }
    let LibraryState::Ready(l) = &lib.0 else { return };
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
    let mut step = 0;
    if tap != 0 {
        repeat.dir = tap;
        repeat.next = now + FIRST_REPEAT;
        step = tap;
    } else if held_x != repeat.dir {
        repeat.dir = held_x;
        repeat.next = now + FIRST_REPEAT;
        step = held_x;
    } else if held_x != 0 && now >= repeat.next {
        repeat.next = now + REPEAT;
        step = held_x;
    }

    let n_shelves = l.shelves.len();
    let shelf_step = (keys.just_pressed(KeyCode::KeyE) || keys.just_pressed(KeyCode::PageDown) || pad(GamepadButton::RightTrigger)) as i32
        - (keys.just_pressed(KeyCode::KeyQ) || keys.just_pressed(KeyCode::PageUp) || pad(GamepadButton::LeftTrigger)) as i32;
    if shelf_step != 0 {
        cursor.shelf = (cursor.shelf as i32 + shelf_step).rem_euclid(n_shelves as i32) as usize;
        cursor.game = 0;
        return;
    }

    let shelf = &l.shelves[cursor.shelf];
    let n = shelf.games.len();
    if step != 0 && n > 0 {
        cursor.game = (cursor.game as i32 + step).clamp(0, n as i32 - 1) as usize;
    }
    if keys.just_pressed(KeyCode::Home) {
        cursor.game = 0;
    }
    if keys.just_pressed(KeyCode::End) {
        cursor.game = n.saturating_sub(1);
    }

    let go = asked || keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(GamepadButton::South);
    let Some(g) = shelf.games.get(cursor.game).filter(|_| go) else { return };
    match (&shelf.kind, &g.source) {
        // Steam starts it and looks after it; the shelf stays as it is
        (_, Source::Steam { appid, .. }) => {
            toast.text = match launch::launch_steam(appid) {
                Ok(()) => {
                    plays.0.touch(&g.id);
                    format!("{} is starting in Steam.", g.title)
                }
                Err(e) => e.to_string(),
            };
            toast.until = now + 5.0;
        }
        // a PC game's launch is counted, its time isn't
        (_, Source::Pc(pc)) => match launch::launch_pc(pc) {
            Ok(Some(child)) => {
                plays.0.touch(&g.id);
                playing.title = Some(g.title.clone());
                playing.child = Some(Mutex::new(child));
            }
            // its store starts it and looks after it
            Ok(None) => {
                plays.0.touch(&g.id);
                toast.text = format!("{} is starting in {}.", g.title, crate::hud::store_name(pc.source));
                toast.until = now + 5.0;
            }
            Err(e) => {
                toast.text = e.to_string();
                toast.until = now + 6.0;
            }
        },
        (ShelfKind::Console { console, config }, Source::Rom) => {
            let (tx, rx) = channel();
            let (console, sc, file, id, cfg, root) = ((**console).clone(), config.clone(), g.file.clone(), g.id.clone(), l.config.clone(), l.app_root.clone());
            playing.console = console.id.clone();
            let waiting = settings.at_launch();
            std::thread::spawn(move || {
                let started = launch::plan(&console, &sc, &file, &id, &cfg, &root).and_then(|p| {
                    let exe = p.exe.to_string_lossy().into_owned();
                    let still = waiting.and_then(|w| w.run(&p.exe));
                    emucfg::before_launch(&console.id, &exe, cfg.quit_combo().as_deref(), &paths::env_var);
                    Ok((launch::start(&p)?, exe, still))
                });
                let _ = tx.send(started);
            });
            playing.title = Some(g.title.clone());
            playing.id = g.id.clone();
            playing.starting = Some(Mutex::new(rx));
        }
        (_, Source::App { path }) => match launch::launch_app(&g.title, path, &l.app_root) {
            Ok(child) => {
                plays.0.touch(&g.id);
                playing.title = Some(g.title.clone());
                playing.child = Some(Mutex::new(child));
            }
            Err(e) => {
                toast.text = e.to_string();
                toast.until = now + 6.0;
            }
        },
        (ShelfKind::Pc | ShelfKind::Apps, Source::Rom) => {}
    }
}

/// The shelf takes input again once the emulator has closed, or if it didn't start.
pub fn watch_game(mut playing: ResMut<Playing>, mut plays: ResMut<Plays>, mut toast: ResMut<Toast>, mut settings: ResMut<crate::settings::Settings>, time: Res<Time>) {
    let started = playing.starting.as_ref().map(|rx| rx.lock().unwrap().try_recv());
    let title = playing.title.clone().unwrap_or_default();
    let now = time.elapsed_secs();
    match started {
        None | Some(Err(TryRecvError::Empty)) => {}
        Some(Ok(Ok((child, exe, still)))) => {
            info!("Launched {title}");
            if let Some(list) = still {
                settings.set_pending(list);
            }
            plays.0.start(&playing.id);
            playing.starting = None;
            playing.child = Some(Mutex::new(child));
            playing.emulator = Some(Emulator { console: playing.console.clone(), exe, at: now, alone: false });
        }
        Some(Ok(Err(e))) => {
            warn!("{title}: {e}");
            toast.text = e.to_string();
            toast.until = now + 6.0;
            *playing = Playing::default();
        }
        Some(Err(TryRecvError::Disconnected)) => *playing = Playing::default(),
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
    let emulator = playing.emulator.take();
    if let Some(e) = &emulator {
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
}

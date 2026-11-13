//! Moving along the shelf with keys or a pad, and starting the chosen game.

use std::process::Child;
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::launch;

use crate::hud::Toast;
use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

/// The game that's running: the shelf waits for it.
#[derive(Resource, Default)]
pub struct Playing {
    pub title: Option<String>,
    child: Option<Mutex<Child>>,
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
    mut toast: ResMut<Toast>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
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

    let go = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(GamepadButton::South);
    if let (true, Some(g)) = (go, shelf.games.get(cursor.game)) {
        match launch::launch_game(&shelf.config, &g.file, &l.app_root) {
            Ok(child) => {
                info!("Launched {} with {}", g.title, shelf.config.emulator);
                playing.title = Some(g.title.clone());
                playing.child = Some(Mutex::new(child));
            }
            Err(e) => {
                warn!("{}: {e}", g.title);
                toast.text = e.to_string();
                toast.until = now + 5.0;
            }
        }
    }
}

/// The shelf takes input again once the emulator has closed.
pub fn watch_game(mut playing: ResMut<Playing>) {
    let done = match &playing.child {
        Some(c) => !matches!(c.lock().unwrap().try_wait(), Ok(None)),
        None => false,
    };
    if done {
        playing.child = None;
        playing.title = None;
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/app.js (the shelf's drag) and android-app/web-extras/android-touch.js (swipe to go back), as
// RustyShelf has them.
//! A finger on the screen (Android, the Steam Deck, touch screens), as the keys it stands for, so every screen does
//! what it already does for them: along the shelf a sideways drag steps through the games and a tap on the chosen case
//! plays it; from a side edge inwards is Back (Esc) everywhere, and off the shelf a quick swipe to the left is too.
//! In Settings a drag up or down moves through the rows, and a tap on a card picks its lit button. A finger held
//! still on the shelf opens the quick menu (Tab).

use bevy::prelude::*;

/// Distances as parts of the window's width (android-touch.js's, in its 1440 px layout).
const EDGE: f32 = 40.0 / 1440.0;
const COMMIT: f32 = 90.0 / 1440.0;
const MOVED: f32 = 8.0 / 1440.0;
/// a step along the shelf, and between Settings' rows (of the height)
const STEP: f32 = 0.07;
const ROW: f32 = 0.06;
/// seconds a finger is held still for the quick menu
const LONG: f32 = 0.6;

#[derive(Default)]
pub struct Finger {
    id: Option<u64>,
    start: Vec2,
    last: Vec2,
    at: f32,
    acc: Vec2,
    moved: bool,
    edge: bool,
    /// a second finger came: it's no gesture of ours
    lost: bool,
    /// held long enough: the quick menu opened, and letting go does nothing more
    long: bool,
}

pub fn run(
    touches: Res<Touches>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    settings: Res<crate::settings::Settings>,
    card: Res<crate::card::Card>,
    playing: Res<crate::play::Playing>,
    time: Res<Time>,
    mut f: Local<Finger>,
    mut held: Local<Vec<KeyCode>>,
) {
    for k in held.drain(..) {
        keys.release(k);
    }
    let Ok(win) = windows.single() else { return };
    let (w, h) = (win.width().max(1.0), win.height().max(1.0));
    let now = time.elapsed_secs();
    let mut press = |k: KeyCode| {
        keys.press(k);
        held.push(k);
    };
    let shelf = !settings.open && !settings.keys_taken && !card.is_open() && playing.title.is_none();
    if touches.iter().count() > 1 {
        f.lost = true;
    }
    for t in touches.iter_just_pressed() {
        if f.id.is_none() {
            let p = t.position();
            *f = Finger { id: Some(t.id()), start: p, last: p, at: now, edge: p.x < EDGE * w || p.x > w - EDGE * w, ..default() };
        }
    }
    let Some(id) = f.id else { return };
    if let Some(t) = touches.get_pressed(id) {
        let p = t.position();
        let d = p - f.last;
        f.last = p;
        if (p - f.start).length() > MOVED * w {
            f.moved = true;
        }
        if shelf && !f.moved && !f.lost && !f.long && now - f.at >= LONG {
            f.long = true;
            press(KeyCode::Tab);
        }
        if f.lost || f.edge || !f.moved {
            return;
        }
        f.acc += d;
        if shelf {
            while f.acc.x.abs() > STEP * w {
                press(if f.acc.x > 0.0 { KeyCode::ArrowLeft } else { KeyCode::ArrowRight });
                f.acc.x -= f.acc.x.signum() * STEP * w;
            }
        } else if settings.open {
            while f.acc.y.abs() > ROW * h {
                press(if f.acc.y > 0.0 { KeyCode::ArrowUp } else { KeyCode::ArrowDown });
                f.acc.y -= f.acc.y.signum() * ROW * h;
            }
        }
        return;
    }
    // let go
    let ended = touches.iter_just_released().any(|t| t.id() == id) || touches.iter_just_canceled().any(|t| t.id() == id);
    if !ended {
        return;
    }
    let (fin, start, quick, lost, edge, moved) = (f.last, f.start, now - f.at < 0.6, f.lost || f.long, f.edge, f.moved);
    *f = Finger::default();
    if lost {
        return;
    }
    let d = fin - start;
    let inward = (start.x < w / 2.0) == (d.x > 0.0);
    if edge && inward && d.x.abs() > COMMIT * w && d.x.abs() > d.y.abs() {
        return press(KeyCode::Escape);
    }
    if !shelf && quick && d.x < -COMMIT * w && d.x.abs() > d.y.abs() * 1.5 && !card.is_open() {
        return press(KeyCode::Escape);
    }
    if !moved && quick && (card.is_open() || crate::tour::ON.load(std::sync::atomic::Ordering::Relaxed) || (shelf && (fin.x - w / 2.0).abs() < 0.15 * w)) {
        press(KeyCode::Enter);
    }
}

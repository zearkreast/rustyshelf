//! The Wii remote (Windows, through a DolphinBar): one more controller for the shelf, Settings and the Home menu, in
//! the standard layout. Held upright: A confirms, B goes back, the D-pad moves, 1 / 2 are X / Y, - / + are the
//! shoulder buttons, Home opens the Home menu. Held sideways (Settings → Controller): the D-pad turned, 2 confirms,
//! 1 goes back. Dolphin gets the remote for its games; during one, Home held about a second opens RustyShelf's Home
//! menu (a tap stays the game's own Wii HOME menu). Pointed at the screen, a hand follows the remote over the shelf:
//! A on a case chooses it, A on the chosen one turns it round, and the remote gives a short rumble as the hand moves
//! onto a case.

use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::prelude::*;
use rustyshelf_core::paths;
use rustyshelf_core::wiimote::{btn, Event, REMOTE};

use crate::play::Playing;

/// How long Home is held during a Dolphin game before RustyShelf's Home menu opens.
const HOME_HOLD: f32 = 0.9;
/// After a Dolphin game starts, the remote is listened to from then on (Dolphin has connected to it by then).
const WATCH_AFTER: f32 = 4.0;

#[derive(Resource, Default)]
pub struct Remote {
    started: bool,
    pad: Option<Entity>,
    /// the buttons the pad shows now, and the remote's own last
    shown: u16,
    restarts: u32,
    restart_at: Option<f32>,
    watch_at: Option<f32>,
    watching: u16,
    home_since: Option<f32>,
    was_playing: bool,
    /// where the remote points (0..1 of the camera's view) and whether the bar's lights are in view, as last read
    aim: (f32, f32, bool),
    /// the hand: where it is (0..1 of the screen, smoothed), whether it shows, and since when the lights were seen
    hand: (f32, f32, bool),
    seen_at: f32,
    /// A was pressed while the hand pointed at the shelf: a click, not the pad's A
    click: bool,
    hovered: Option<usize>,
    rumbled_at: f32,
}

/// The remote's buttons as the standard layout's.
fn layout(buttons: u16, sideways: bool) -> Vec<(GamepadButton, bool)> {
    let on = |b: u16| buttons & b != 0;
    let (up, down, left, right) = (on(btn::UP), on(btn::DOWN), on(btn::LEFT), on(btn::RIGHT));
    let mut out = if sideways {
        vec![
            (GamepadButton::DPadLeft, up),
            (GamepadButton::DPadRight, down),
            (GamepadButton::DPadDown, left),
            (GamepadButton::DPadUp, right),
            (GamepadButton::East, on(btn::TWO)),
            (GamepadButton::South, on(btn::ONE)),
            (GamepadButton::West, on(btn::A)),
            (GamepadButton::North, on(btn::B)),
        ]
    } else {
        vec![
            (GamepadButton::DPadUp, up),
            (GamepadButton::DPadDown, down),
            (GamepadButton::DPadLeft, left),
            (GamepadButton::DPadRight, right),
            (GamepadButton::East, on(btn::A)),
            (GamepadButton::South, on(btn::B)),
            (GamepadButton::West, on(btn::ONE)),
            (GamepadButton::North, on(btn::TWO)),
        ]
    };
    out.extend([(GamepadButton::LeftTrigger, on(btn::MINUS)), (GamepadButton::RightTrigger, on(btn::PLUS)), (GamepadButton::Mode, on(btn::HOME))]);
    out
}

/// The pad's buttons that changed, as the controller's own reports would bring them.
fn press(remote: &mut Remote, now_buttons: u16, sideways: bool, raw: &mut MessageWriter<RawGamepadEvent>, buttons: &mut MessageWriter<RawGamepadButtonChangedEvent>) {
    let Some(pad) = remote.pad else { return };
    for ((b, before), (_, after)) in layout(remote.shown, sideways).into_iter().zip(layout(now_buttons, sideways)) {
        if before != after {
            let e = RawGamepadButtonChangedEvent::new(pad, b, if after { 1.0 } else { 0.0 });
            raw.write(e.into());
            buttons.write(e);
        }
    }
    remote.shown = now_buttons;
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    mut remote: ResMut<Remote>,
    mut playing: ResMut<Playing>,
    settings: Res<crate::settings::Settings>,
    home: Res<crate::home::Home>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
) {
    if !cfg!(windows) {
        return;
    }
    let now = time.elapsed_secs();
    let (on, sideways) = settings.wiimote();
    if !on {
        if remote.started {
            REMOTE.stop();
            remote.started = false;
        }
        return;
    }
    if !remote.started || remote.restart_at.is_some_and(|at| now >= at) {
        remote.started = true;
        remote.restart_at = None;
        REMOTE.start(&paths::data_dir(&paths::app_root()).join("wiimote-cache"));
    }
    let menu_open = home.is_open();
    for e in REMOTE.events() {
        match e {
            Event::Ready => remote.restarts = 0,
            Event::Connected(true) if remote.pad.is_none() => {
                let pad = commands.spawn(bevy::input::gamepad::GamepadSettings::default()).id();
                let c = GamepadConnectionEvent::new(pad, GamepadConnection::Connected { name: "Nintendo Wii Remote".into(), vendor_id: Some(0x057e), product_id: Some(0x0306) });
                raw.write(c.clone().into());
                connections.write(c);
                remote.pad = Some(pad);
                remote.shown = 0;
                info!("Wii remote connected");
            }
            Event::Connected(false) => {
                if let Some(pad) = remote.pad.take() {
                    let c = GamepadConnectionEvent::new(pad, GamepadConnection::Disconnected);
                    raw.write(c.clone().into());
                    connections.write(c);
                    commands.entity(pad).despawn();
                }
            }
            Event::State { buttons: b, x, y, seen } => {
                // a game owns the screen: Home opens RustyShelf's Home menu over it, which the remote then drives
                if playing.title.is_some() && !menu_open && b & btn::HOME != 0 && remote.shown & btn::HOME == 0 {
                    playing.home_pressed = true;
                }
                remote.aim = (x, y, seen);
                // A clicks where the hand points: the pad doesn't get it (held, it stays the click's)
                let pointing = remote.hand.2 && !settings.open && !settings.keys_taken && playing.title.is_none() && !menu_open;
                let a_down = b & btn::A != 0;
                if a_down && remote.shown & btn::A == 0 && pointing {
                    remote.click = true;
                }
                let b = if a_down && (pointing || remote.click) && remote.shown & btn::A == 0 { b & !btn::A } else { b };
                press(&mut remote, b, sideways, &mut raw, &mut buttons);
            }
            Event::Watch(b) => {
                remote.watching = b;
                if menu_open {
                    press(&mut remote, b, sideways, &mut raw, &mut buttons);
                }
            }
            Event::Ended(why) => {
                warn!("Wii remote reader ended: {why}");
                // ended on its own (a crash, a failed compile): tried again a few times, slower each time
                if remote.restarts < 4 {
                    remote.restart_at = Some(now + 2.0 * 2f32.powi(remote.restarts as i32));
                    remote.restarts += 1;
                }
            }
            _ => {}
        }
    }
    // a Dolphin game: listened to once Dolphin has the remote; Home held opens the Home menu; given back after it
    let playing_now = playing.title.is_some();
    if REMOTE.is_paused() {
        if playing.pid.is_some() && remote.watch_at.is_none() {
            remote.watch_at = Some(now + WATCH_AFTER);
        }
        if remote.watch_at.is_some_and(|at| now >= at) {
            remote.watch_at = Some(f32::INFINITY);
            REMOTE.watch();
        }
        match (remote.watching & btn::HOME != 0, remote.home_since) {
            (true, None) => remote.home_since = Some(now),
            (true, Some(at)) if at.is_finite() && now - at >= HOME_HOLD && !menu_open => {
                info!("Wii remote: Home held, Home menu");
                playing.home_pressed = true;
                remote.home_since = Some(f32::INFINITY);
            }
            (false, _) => remote.home_since = None,
            _ => {}
        }
        if remote.was_playing && !playing_now {
            REMOTE.resume();
            (remote.watch_at, remote.watching, remote.home_since) = (None, 0, None);
        }
    }
    remote.was_playing = playing_now;
}

#[derive(Component)]
pub struct Hand;

pub fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let p = rustyshelf_core::drawn::hand(104);
    let image = images.add(crate::loading::to_image(p.width, p.height, p.rgba));
    commands.spawn((Hand, Node { position_type: PositionType::Absolute, width: px(52), height: px(52), ..default() }, ImageNode::new(image), Visibility::Hidden, GlobalZIndex(60), Pickable::IGNORE));
}

/// The hand over the shelf: a central part of the camera's view covers the screen (Settings → Controller →
/// Pointer speed), smoothed, with a little still zone. A click chooses the case it points at, or turns the chosen one.
#[allow(clippy::too_many_arguments)]
pub fn pointer(
    time: Res<Time>,
    mut remote: ResMut<Remote>,
    settings: Res<crate::settings::Settings>,
    playing: Res<Playing>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::shelf::ShelfCamera>>,
    slots: Query<(&crate::shelf::Slot, &GlobalTransform)>,
    mut cursor: ResMut<crate::shelf::Cursor>,
    mut hand: Query<(&mut Node, &mut Visibility), With<Hand>>,
    mut sounds: ResMut<crate::sound::Sounds>,
) {
    let now = time.elapsed_secs();
    let (x, y, seen) = remote.aim;
    if seen && remote.pad.is_some() {
        let g = settings.wiimote_speed();
        let (tx, ty) = (((x - 0.5) * g + 0.5).clamp(0.0, 1.0), ((y - 0.5) * g + 0.5).clamp(0.0, 1.0));
        let (hx, hy, was) = remote.hand;
        remote.hand = if !was {
            (tx, ty, true)
        } else {
            let d = ((tx - hx).powi(2) + (ty - hy).powi(2)).sqrt();
            let a = if d > 0.03 { 0.6 } else if d > 0.006 { 0.35 } else { 0.12 };
            (hx + (tx - hx) * a, hy + (ty - hy) * a, true)
        };
        remote.seen_at = now;
    } else if now - remote.seen_at > 0.25 {
        remote.hand.2 = false;
    }
    let free = !settings.open && !settings.keys_taken && playing.title.is_none();
    let shows = remote.hand.2 && free;
    let Ok((mut node, mut vis)) = hand.single_mut() else { return };
    *vis = if shows { Visibility::Inherited } else { Visibility::Hidden };
    let Ok(window) = windows.single() else { return };
    let (w, h) = (window.width(), window.height());
    let (px_, py_) = (remote.hand.0 * w, remote.hand.1 * h);
    // the fingertip on the point
    (node.left, node.top) = (px(px_ - 18.0), px(py_ - 5.0));
    let click = std::mem::take(&mut remote.click);
    if !shows {
        remote.hovered = None;
        return;
    }
    let Ok((cam, eye)) = camera.single() else { return };
    // the case under the hand: the nearest centre along the shelf, within a case's width of it
    let under = slots
        .iter()
        .filter_map(|(slot, at)| {
            let c = cam.world_to_viewport(eye, at.translation()).ok()?;
            let r = cam.world_to_viewport(eye, at.translation() + at.right() * (at.scale().x * 0.5)).ok().map_or(40.0, |e| (e.x - c.x).abs().max(20.0));
            let (dx, dy) = ((c.x - px_).abs(), (c.y - py_).abs());
            (dx <= r && dy <= r * 2.2).then_some((slot.index, dx))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    if under != remote.hovered {
        remote.hovered = under;
        if under.is_some() && now - remote.rumbled_at > 0.09 {
            remote.rumbled_at = now;
            REMOTE.rumble(22);
        }
    }
    if let Some(i) = under.filter(|_| click) {
        if i == cursor.game {
            cursor.flipped = !cursor.flipped;
            sounds.play("flip", 1);
        } else {
            sounds.play("tick", if i < cursor.game { -1 } else { 1 });
            (cursor.game, cursor.flipped) = (i, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_upright_and_sideways() {
        let pressed = |b: u16, side: bool| layout(b, side).into_iter().filter(|(_, on)| *on).map(|(b, _)| b).collect::<Vec<_>>();
        assert_eq!(pressed(btn::A | btn::UP, false), vec![GamepadButton::DPadUp, GamepadButton::East]);
        assert_eq!(pressed(btn::TWO | btn::UP, true), vec![GamepadButton::DPadLeft, GamepadButton::East]);
        assert_eq!(pressed(btn::HOME | btn::PLUS, false), vec![GamepadButton::RightTrigger, GamepadButton::Mode]);
    }
}

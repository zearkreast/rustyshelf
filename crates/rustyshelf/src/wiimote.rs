//! The Wii remote (Windows, through a DolphinBar): one more controller for the shelf, Settings and the Home menu, in
//! the standard layout. Held upright: A confirms, B goes back, the D-pad moves, 1 / 2 are X / Y, - / + are the
//! shoulder buttons, Home opens the Home menu. Held sideways (Settings → Controller): the D-pad turned, 2 confirms,
//! 1 goes back. Dolphin gets the remote for its games; during one, Home held about a second opens RustyShelf's Home
//! menu (a tap stays the game's own Wii HOME menu).

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
            Event::State { buttons: b, .. } => {
                // a game owns the screen: Home opens RustyShelf's Home menu over it, which the remote then drives
                if playing.title.is_some() && !menu_open && b & btn::HOME != 0 && remote.shown & btn::HOME == 0 {
                    playing.home_pressed = true;
                }
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

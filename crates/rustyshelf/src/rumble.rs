// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the controller's
// pulses of Boxshelf's renderer/app.js (rumble), as RustyShelf has them.
//! Short pulses on the controller as things happen on the shelf: a game starting, the Surprise me case landing, a
//! favourite, a hidden game, another shelf, a controller connected. Settings → Controls → Controller rumble turns them
//! off; attract mode never rumbles.

use std::time::Duration;

use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent, GamepadRumbleIntensity, GamepadRumbleRequest};
use bevy::prelude::*;

struct Pulse {
    after: f32,
    at: Option<f32>,
    strong: f32,
    weak: f32,
    ms: u64,
}

#[derive(Resource, Default)]
pub struct Rumble {
    due: Vec<Pulse>,
}

impl Rumble {
    /// A pulse now: the two motors' strengths, 0 to 1, for `ms` milliseconds.
    pub fn pulse(&mut self, strong: f32, weak: f32, ms: u64) {
        self.later(0.0, strong, weak, ms);
    }

    /// A pulse `after` seconds from now.
    pub fn later(&mut self, after: f32, strong: f32, weak: f32, ms: u64) {
        self.due.push(Pulse { after, at: None, strong, weak, ms });
    }
}

pub fn send(
    time: Res<Time>,
    mut settings: ResMut<crate::settings::Settings>,
    idle: Res<crate::idle::Idle>,
    pads: Query<Entity, With<Gamepad>>,
    mut connected: MessageReader<GamepadConnectionEvent>,
    mut rumble: ResMut<Rumble>,
    mut out: MessageWriter<GamepadRumbleRequest>,
) {
    for c in connected.read() {
        if matches!(c.connection, GamepadConnection::Connected { .. }) {
            rumble.pulse(0.4, 0.4, 120);
        }
    }
    for (strong, weak, ms) in std::mem::take(&mut settings.pulses) {
        rumble.pulse(strong, weak, ms);
    }
    if !settings.rumbles() || idle.attract {
        rumble.due.clear();
        return;
    }
    let now = time.elapsed_secs();
    let Some(pad) = pads.iter().next() else {
        rumble.due.clear();
        return;
    };
    rumble.due.retain_mut(|p| {
        let at = *p.at.get_or_insert(now + p.after);
        if now < at {
            return true;
        }
        out.write(GamepadRumbleRequest::Add { duration: Duration::from_millis(p.ms), intensity: GamepadRumbleIntensity { strong_motor: p.strong, weak_motor: p.weak }, gamepad: pad });
        false
    });
}

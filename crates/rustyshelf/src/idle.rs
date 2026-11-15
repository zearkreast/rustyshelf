// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/app.js attract mode and screen dimmer, as RustyShelf has them.
//! Idle time on the shelf: after a few minutes attract mode drifts through the boxes on its own with the HUD hidden,
//! and after a longer spell (ui.dimMinutes) the screen fades nearly to black. Any input wakes both; the input that
//! wakes the dimmed screen does nothing else.

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

use crate::hud::HudRoot;
use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

#[derive(Resource, Default)]
pub struct Idle {
    pub attract: bool,
    dim: bool,
    /// when there was last input, in seconds of the app's clock
    last: f32,
    /// when attract mode next moves
    next: f32,
    dir: i32,
    /// how dark the veil is now, 0 to 1
    veil: f32,
}

#[derive(Component)]
pub struct Veil;

pub fn setup(mut commands: Commands) {
    commands.spawn((
        Veil,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        BackgroundColor(Color::NONE),
        GlobalZIndex(i32::MAX - 1),
        Pickable::IGNORE,
    ));
}

/// Something between 0 and 1 that's different each call.
fn roll(time: &Time) -> f32 {
    let n = time.elapsed().as_nanos() as u64;
    ((n ^ (n >> 17)).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40) as f32 / (1u64 << 24) as f32
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    time: Res<Time>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut pads: Query<&mut Gamepad>,
    lib: Res<LibraryRes>,
    settings: Res<crate::settings::Settings>,
    playing: Res<crate::play::Playing>,
    inspect: Res<crate::shelf::Inspect>,
    mut cursor: ResMut<Cursor>,
    mut idle: ResMut<Idle>,
    mut veil: Query<&mut BackgroundColor, With<Veil>>,
    mut hud: Query<&mut Visibility, With<HudRoot>>,
) {
    let now = time.elapsed_secs();
    let moved = motion.read().map(|m| m.delta.length()).sum::<f32>() > 2.0;
    let active = moved
        // (a tap can start and end within one frame)
        || keys.get_pressed().chain(keys.get_just_released()).next().is_some()
        || mouse.get_pressed().chain(mouse.get_just_released()).next().is_some()
        || pads.iter().any(|p| p.digital().get_pressed().chain(p.digital().get_just_released()).next().is_some() || p.left_stick().length() > 0.3 || p.right_stick().length() > 0.3);
    if active {
        if idle.dim {
            keys.reset_all();
            for mut p in &mut pads {
                p.digital_mut().reset_all();
            }
            idle.dim = false;
        }
        idle.attract = false;
        idle.last = now;
    }
    let (attract_on, mins, stay, wander, dim_mins) = settings.idle();
    // a game running is never idle time on the shelf
    if playing.title.is_some() {
        idle.last = now;
        idle.attract = false;
    }
    let busy = settings.open || settings.keys_taken || inspect.on;
    let shelves = match &lib.0 {
        LibraryState::Ready(l) => l.shelves.iter().map(|s| s.games.len()).collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let here = shelves.get(cursor.shelf).copied().unwrap_or(0);
    if !attract_on {
        idle.attract = false;
    } else if !idle.attract && !idle.dim && !busy && here >= 2 && now - idle.last > mins as f32 * 60.0 {
        (idle.attract, idle.next, idle.dir) = (true, now + 2.2, 1);
    }
    if idle.attract && !idle.dim && now >= idle.next {
        if busy {
            idle.next = now + 4.0;
        } else {
            let at = cursor.game as i32 + idle.dir;
            if at < 0 || at >= here as i32 {
                idle.dir = -idle.dir;
                // the new shelf starts at its first game: walk forwards, not back into its own cases
                if wander && shelves.len() > 1 && roll(&time) < 0.6 {
                    (cursor.shelf, cursor.land, cursor.flipped, idle.dir) = ((cursor.shelf + 1) % shelves.len(), true, false, 1);
                    idle.next = now + stay + 1.0;
                    return;
                }
            }
            cursor.game = (cursor.game as i32 + idle.dir).clamp(0, here.saturating_sub(1) as i32) as usize;
            cursor.flipped = false;
            idle.next = now + stay;
        }
    }
    if dim_mins > 0.0 && !idle.dim && playing.title.is_none() && now - idle.last > dim_mins as f32 * 60.0 {
        idle.dim = true;
    }
    // in over 3 seconds, out in a third of one
    let (to, secs) = if idle.dim { (0.92, 3.0) } else { (0.0, 0.3) };
    let was = idle.veil;
    idle.veil = if to > was { (was + time.delta_secs() * to / secs).min(to) } else { (was - time.delta_secs() * 0.92 / secs).max(to) };
    if idle.veil != was || active {
        for mut v in &mut veil {
            v.0 = Color::srgba(0.0, 0.0, 0.0, idle.veil);
        }
    }
    let show = if idle.attract { Visibility::Hidden } else { Visibility::Inherited };
    for mut v in &mut hud {
        v.set_if_neq(show);
    }
}

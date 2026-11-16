//! Android: the controllers. gilrs (Bevy's gamepads) has no Android side, and winit gets a pad's buttons as unknown
//! keys and its sticks not at all, so MainActivity passes them in through JNI and here they become one Bevy Gamepad.
//! The system Back key is Esc.

use std::sync::Mutex;

use bevy::input::gamepad::{GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent, RawGamepadEvent};
use bevy::input::keyboard::NativeKeyCode;
use bevy::prelude::*;

enum Ev {
    Device(u16, u16),
    Button(i32, bool),
    /// left x, left y, right x, right y, left trigger, right trigger (Android's: y grows downwards)
    Axes([f32; 6]),
}

static QUEUE: Mutex<Vec<Ev>> = Mutex::new(Vec::new());

fn push(e: Ev) {
    if let Ok(mut q) = QUEUE.lock() {
        q.push(e);
    }
}

// keep the names in sync with MainActivity's native methods (package io.github.rustyshelf.native_)
#[no_mangle]
pub extern "system" fn Java_io_github_rustyshelf_native_1_MainActivity_nativeDevice(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, vendor: i32, product: i32) {
    push(Ev::Device(vendor as u16, product as u16));
}

#[no_mangle]
pub extern "system" fn Java_io_github_rustyshelf_native_1_MainActivity_nativeButton(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, key: i32, down: u8) {
    push(Ev::Button(key, down != 0));
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_io_github_rustyshelf_native_1_MainActivity_nativeAxes(_env: *mut std::ffi::c_void, _class: *mut std::ffi::c_void, lx: f32, ly: f32, rx: f32, ry: f32, lt: f32, rt: f32) {
    push(Ev::Axes([lx, ly, rx, ry, lt, rt]));
}

/// Android's KeyEvent codes for a pad's buttons.
fn button(key: i32) -> Option<GamepadButton> {
    Some(match key {
        96 => GamepadButton::South,
        97 => GamepadButton::East,
        99 => GamepadButton::West,
        100 => GamepadButton::North,
        102 => GamepadButton::LeftTrigger,
        103 => GamepadButton::RightTrigger,
        104 => GamepadButton::LeftTrigger2,
        105 => GamepadButton::RightTrigger2,
        106 => GamepadButton::LeftThumb,
        107 => GamepadButton::RightThumb,
        108 => GamepadButton::Start,
        109 => GamepadButton::Select,
        110 => GamepadButton::Mode,
        19 => GamepadButton::DPadUp,
        20 => GamepadButton::DPadDown,
        21 => GamepadButton::DPadLeft,
        22 => GamepadButton::DPadRight,
        // the D-pad's middle (some remotes): A
        23 => GamepadButton::South,
        _ => return None,
    })
}

#[derive(Default)]
pub struct Pad {
    entity: Option<Entity>,
    ids: (Option<u16>, Option<u16>),
    axes: [f32; 6],
}

/// Before Bevy's input systems: what MainActivity passed in, as gamepad events.
pub fn feed(
    mut commands: Commands,
    mut pad: Local<Pad>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut buttons: MessageWriter<RawGamepadButtonChangedEvent>,
    mut axes: MessageWriter<RawGamepadAxisChangedEvent>,
) {
    let evs: Vec<Ev> = QUEUE.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
    for ev in evs {
        if let Ev::Device(v, p) = ev {
            pad.ids = (Some(v), Some(p));
            continue;
        }
        let entity = match pad.entity {
            Some(e) => e,
            None => {
                let e = commands.spawn(Name::new("Android controller")).id();
                let c = GamepadConnectionEvent::new(e, GamepadConnection::Connected { name: "Android controller".into(), vendor_id: pad.ids.0, product_id: pad.ids.1 });
                raw.write(c.clone().into());
                connections.write(c);
                pad.entity = Some(e);
                e
            }
        };
        match ev {
            Ev::Button(key, down) => {
                if let Some(b) = button(key) {
                    let e = RawGamepadButtonChangedEvent::new(entity, b, if down { 1.0 } else { 0.0 });
                    raw.write(e.clone().into());
                    buttons.write(e);
                }
            }
            Ev::Axes(now) => {
                for (i, v) in now.into_iter().enumerate() {
                    if (v - pad.axes[i]).abs() < 0.01 {
                        continue;
                    }
                    pad.axes[i] = v;
                    match i {
                        0..=3 => {
                            let axis = [GamepadAxis::LeftStickX, GamepadAxis::LeftStickY, GamepadAxis::RightStickX, GamepadAxis::RightStickY][i];
                            let e = RawGamepadAxisChangedEvent::new(entity, axis, if i % 2 == 1 { -v } else { v });
                            raw.write(e.clone().into());
                            axes.write(e);
                        }
                        _ => {
                            let e = RawGamepadButtonChangedEvent::new(entity, if i == 4 { GamepadButton::LeftTrigger2 } else { GamepadButton::RightTrigger2 }, v);
                            raw.write(e.clone().into());
                            buttons.write(e);
                        }
                    }
                }
            }
            Ev::Device(..) => {}
        }
    }
}

/// After Bevy's input systems: the system Back key (a gesture, or the navigation bar's) works as Esc.
pub fn back_is_esc(mut keys: ResMut<ButtonInput<KeyCode>>, mut held: Local<bool>) {
    let back = KeyCode::Unidentified(NativeKeyCode::Android(4));
    if keys.just_pressed(back) {
        keys.press(KeyCode::Escape);
        *held = true;
    } else if *held && !keys.pressed(back) {
        keys.release(KeyCode::Escape);
        *held = false;
    }
}

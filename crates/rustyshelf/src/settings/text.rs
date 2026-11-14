//! Typing into a setting: on the keyboard, or with a pad on the on-screen keys.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;

use super::*;
use crate::osk::{self, Hit};

pub(super) type Done = Box<dyn Fn(&mut Settings, String) + Send + Sync>;

/// The longest text a setting takes.
const LONGEST: usize = 1000;

pub(super) struct Editing {
    label: String,
    hint: String,
    value: String,
    done: Done,
    /// the on-screen key under the cursor (row, column) and the layer shown
    key: (usize, usize),
    layer: usize,
    /// where the stick was last frame: a push steps once
    stick: (i32, i32),
}

impl Settings {
    /// Opens the text of a setting for typing; `done` gets what was typed when it's kept.
    pub(super) fn edit(&mut self, label: &str, hint: &str, value: String, done: Done) {
        self.editing = Some(Editing { label: label.to_string(), hint: hint.to_string(), value, done, key: (1, 0), layer: 0, stick: (0, 0) });
        self.note.clear();
        self.redraw = true;
    }

    /// The keys typed this frame and the pad's presses go to the text being typed.
    pub(super) fn type_text(&mut self, typed: &[KeyboardInput], pads: &Query<&Gamepad>) {
        let Some(mut e) = self.editing.take() else { return };
        let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
        let (mut keep, mut touched) = (None, false);
        let mut steps: Vec<(i32, i32)> = Vec::new();
        for k in typed.iter().filter(|k| k.state == ButtonState::Pressed) {
            touched = true;
            match &k.logical_key {
                Key::Enter => keep = Some(true),
                Key::Escape => keep = Some(false),
                Key::Backspace => {
                    e.value.pop();
                }
                Key::ArrowLeft => steps.push((-1, 0)),
                Key::ArrowRight => steps.push((1, 0)),
                Key::ArrowUp => steps.push((0, -1)),
                Key::ArrowDown => steps.push((0, 1)),
                _ => e.value.extend(k.text.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control())),
            }
        }
        for (b, dx, dy) in [(GamepadButton::DPadLeft, -1, 0), (GamepadButton::DPadRight, 1, 0), (GamepadButton::DPadUp, 0, -1), (GamepadButton::DPadDown, 0, 1)] {
            if pad(b) {
                steps.push((dx, dy));
            }
        }
        let side = |v: f32| if v > 0.5 { 1 } else if v < -0.5 { -1 } else { 0 };
        let stick = pads.iter().map(|p| (side(p.left_stick().x), -side(p.left_stick().y))).find(|s| *s != (0, 0)).unwrap_or((0, 0));
        if stick != e.stick {
            e.stick = stick;
            if stick != (0, 0) {
                steps.push(if stick.0 != 0 { (stick.0, 0) } else { (0, stick.1) });
            }
        }
        for (dx, dy) in &steps {
            e.key = osk::step(e.layer, e.key, *dx, *dy);
        }
        let mut hits = Vec::new();
        if pad(GamepadButton::South) {
            hits = osk::press(e.layer, e.key);
        }
        if pad(GamepadButton::West) {
            hits.push(Hit::Back);
        }
        if pad(GamepadButton::North) {
            hits.push(Hit::Type(' '));
        }
        if pad(GamepadButton::Start) {
            hits.push(Hit::Done);
        }
        if pad(GamepadButton::East) {
            hits.push(Hit::Cancel);
        }
        touched |= !steps.is_empty() || !hits.is_empty();
        for hit in hits {
            match hit {
                Hit::Type(c) => e.value.push(c),
                Hit::Back => {
                    e.value.pop();
                }
                Hit::Clear => e.value.clear(),
                Hit::Cancel => keep = Some(false),
                Hit::Done => keep = Some(true),
                Hit::Layer(l) => {
                    e.layer = l;
                    e.key = osk::step(l, e.key, 0, 0);
                }
            }
        }
        if e.value.chars().count() > LONGEST {
            e.value = e.value.chars().take(LONGEST).collect();
        }
        self.redraw |= touched;
        match keep {
            Some(true) => (e.done)(self, osk::settled(&e.value)),
            Some(false) => {}
            None => self.editing = Some(e),
        }
    }
}

impl Editing {
    pub(super) fn draw(&self, commands: &mut Commands, face: &Handle<Font>) {
        let font = |size: f32| TextFont::from_font_size(FontSize::VMin(size)).with_font(face.clone());
        let rows = osk::rows(self.layer);
        commands
            .spawn((
                SettingsUi,
                Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, padding: UiRect::axes(vmin(8.0), vmin(6.0)), ..default() },
                BackgroundColor(PANEL),
                GlobalZIndex(10),
            ))
            .with_children(|p| {
                p.spawn((Text::new(self.label.clone()), font(4.4), TextColor(STEEL)));
                p.spawn((Node { margin: UiRect::vertical(vmin(1.2)), max_width: percent(80), ..default() }, Text::new(self.hint.clone()), font(2.1), TextColor(DIM)));
                p.spawn((Node { width: percent(100), padding: UiRect::axes(vmin(1.6), vmin(1.4)), margin: UiRect::bottom(vmin(3.0)), ..default() }, BackgroundColor(CHOSEN)))
                    .with_children(|b| {
                        b.spawn((Text::new(format!("{}▏", self.value)), font(2.8), TextColor(STEEL)));
                    });
                for (r, row) in rows.iter().enumerate() {
                    p.spawn(Node { flex_direction: FlexDirection::Row, column_gap: vmin(0.6), margin: UiRect::vertical(vmin(0.3)), ..default() }).with_children(|line| {
                        for (c, key) in row.iter().enumerate() {
                            let chosen = (r, c) == self.key;
                            let lit = (key == "Shift" && self.layer == 1) || (key == "#+=" && self.layer == 2);
                            let wide = if key == "Space" { 21.0 } else if key.chars().count() > 1 { 11.0 } else { 5.4 };
                            line.spawn((
                                Node { width: vmin(wide), height: vmin(5.4), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                                BackgroundColor(if chosen { RUST } else { KEY }),
                            ))
                            .with_children(|k| {
                                k.spawn((Text::new(key.clone()), font(2.4), TextColor(if chosen || !lit { STEEL } else { RUST })));
                            });
                        }
                    });
                }
                p.spawn(Node { flex_grow: 1.0, ..default() });
                p.spawn((
                    Node { margin: UiRect::top(vmin(1.5)), ..default() },
                    Text::new("Type on the keyboard, or pick keys with the pad      Enter · Start  Done      Esc · B  Cancel      X  ⌫      Y  Space"),
                    font(1.8),
                    TextColor(DIM),
                ));
            });
    }
}

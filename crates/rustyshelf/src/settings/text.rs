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

/// Ctrl+V (⌘V), or Shift+Insert.
pub(super) fn is_paste(k: &KeyboardInput, ctrl: bool, shift: bool) -> bool {
    (ctrl && matches!(&k.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"))) || (ctrl && k.key_code == KeyCode::KeyV) || (shift && k.logical_key == Key::Insert)
}

impl Settings {
    /// Opens the text of a setting for typing; `done` gets what was typed when it's kept.
    pub(super) fn edit(&mut self, label: &str, hint: &str, value: String, done: Done) {
        self.editing = Some(Editing { label: label.to_string(), hint: hint.to_string(), value, done, key: (1, 0), layer: 0, stick: (0, 0) });
        self.note.clear();
        self.redraw = true;
    }

    /// The keys typed this frame and the pad's presses go to the text being typed.
    /// `ctrl`: Ctrl (or ⌘) is held, so Ctrl+V and Shift+Insert paste.
    pub(super) fn type_text(&mut self, typed: &[KeyboardInput], pads: &Query<&Gamepad>, ctrl: bool, shift: bool) {
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
                _ if is_paste(k, ctrl, shift) => e.value.push_str(&osk::clipboard().unwrap_or_default()),
                // Ctrl with another key is a shortcut, not a letter
                _ if ctrl => {}
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
        let (confirm, back) = self.confirm();
        if pad(confirm) {
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
        if pad(back) {
            hits.push(Hit::Cancel);
        }
        touched |= !steps.is_empty() || !hits.is_empty();
        for hit in hits {
            match hit {
                Hit::Type(c) => e.value.push(c),
                Hit::Back => {
                    e.value.pop();
                }
                Hit::Paste => e.value.push_str(&osk::clipboard().unwrap_or_default()),
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
        // (opened only for this: done or not, Settings goes)
        if keep.is_some() && std::mem::take(&mut self.edit_alone) {
            self.shut();
        }
    }
}

impl Editing {
    /// `caps`: the keys or the pad's buttons that end the typing, cancel it, take a letter back and type a space.
    pub(super) fn draw(&self, commands: &mut Commands, font: &dyn Fn(u32, f32) -> TextFont, caps: [&str; 4]) {
        let rows = osk::rows(self.layer);
        let keys = [(vec![caps[0]], "Done"), (vec![caps[1]], "Cancel"), (vec![caps[2]], "⌫"), (vec![caps[3]], "Space")];
        commands
            .spawn((
                SettingsUi,
                Node { width: percent(100), height: percent(100), position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, padding: UiRect::new(dp(120.0), dp(120.0), dp(80.0), dp(120.0)), ..default() },
                surface(),
                GlobalZIndex(10),
            ))
            .with_children(|p| {
                p.spawn(Node { width: dp(1160.0), flex_direction: FlexDirection::Column, ..default() }).with_children(|p| {
                    p.spawn((Text::new(self.label.clone()), font(800, 52.0), TextColor(S_INK)));
                    p.spawn((Node { margin: UiRect::new(px(0), px(0), dp(10.0), dp(26.0)), max_width: dp(900.0), ..default() }, Text::new(self.hint.clone()), font(400, 20.0), TextColor(S_DIM)));
                    p.spawn((
                        Node { padding: UiRect::axes(dp(22.0), dp(16.0)), margin: UiRect::bottom(dp(30.0)), border: UiRect::all(dp(2.0)), border_radius: BorderRadius::all(dp(14.0)), ..default() },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.055)),
                        BorderColor::all(S_ACC2.with_alpha(0.85)),
                        children![(Text::new(format!("{}▏", self.value)), font(500, 28.0), TextColor(S_INK))],
                    ));
                    for (r, row) in rows.iter().enumerate() {
                        p.spawn(Node { column_gap: dp(8.0), margin: UiRect::vertical(dp(4.0)), justify_content: JustifyContent::Center, ..default() }).with_children(|line| {
                            for (c, key) in row.iter().enumerate() {
                                let chosen = (r, c) == self.key;
                                let on = (key == "Shift" && self.layer == 1) || (key == "#+=" && self.layer == 2);
                                let wide = if key == "Space" { 400.0 } else if key.chars().count() > 1 { 190.0 } else { 86.0 };
                                let mut k = line.spawn((
                                    Node { width: dp(wide), height: dp(76.0), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(dp(2.0)), border_radius: BorderRadius::all(dp(14.0)), ..default() },
                                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.055)),
                                    BorderColor::all(Color::NONE),
                                ));
                                if chosen {
                                    k.insert(lit());
                                }
                                k.with_children(|k| {
                                    k.spawn((Text::new(key.clone()), font(600, 26.0), TextColor(if chosen { Color::WHITE } else if on { S_ACC2 } else { S_INK2 })));
                                });
                            }
                        });
                    }
                });
                key_bar(p, &keys, font);
            });
    }
}

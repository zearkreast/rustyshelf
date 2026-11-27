//! One card over the shelf, in Settings' look: What's new after an update, the Steam Deck's hint, and a few choices
//! one per row (the Switch 2 build to start in). ◀ ▶ moves between its buttons (▲ ▼ between its choices), Enter / A
//! picks the lit one, Esc / B closes it. While it's up the shelf and Settings leave the keys to it.

use bevy::prelude::*;

use crate::hud::MonaSans;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Content {
    /// the small line over the title ("Updated", "Tour · 3 of 10")
    pub kick: String,
    pub title: String,
    pub sub: String,
    pub items: Vec<String>,
    /// a key and what it does ("Enter", "Play")
    pub keys: Vec<(String, String)>,
    /// the first is the main one
    pub buttons: Vec<String>,
    /// choices, one per row with a line beside it, picked instead of the buttons
    pub options: Vec<(String, String)>,
    /// the choice lit first
    pub first: usize,
    /// notes in sections ("New", "Changed", "Fixed"), each item a headline and a line more: a wider card, each
    /// section under a chip of its own colour
    pub sections: Vec<(String, Vec<(String, String)>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Picked(usize),
    Back,
}

#[derive(Resource, Default)]
pub struct Card {
    shown: Option<Content>,
    lit: usize,
    outcome: Option<Outcome>,
    /// the UI wears what's shown
    built: bool,
}

impl Card {
    pub fn show(&mut self, c: Content) {
        let lit = c.first.min(c.options.len().saturating_sub(1));
        (self.shown, self.lit, self.outcome, self.built) = (Some(c), lit, None, false);
    }

    pub fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    /// How the card was left, once.
    pub fn take(&mut self) -> Option<Outcome> {
        self.outcome.take()
    }
}

#[derive(Component)]
pub struct CardRoot;

pub(crate) const PAPER: Color = Color::srgba(0.07, 0.08, 0.12, 0.97);
pub(crate) const INK: Color = Color::srgb(0.953, 0.961, 0.984);
pub(crate) const DIM: Color = Color::srgba(0.953, 0.961, 0.984, 0.62);
pub(crate) const ACCENT: Color = Color::srgb(0.48, 0.62, 1.0);

/// A notes section's colour: green for what's new, the accent for what changed, amber for what's fixed.
fn section_colour(h: &str) -> Color {
    match h {
        "New" => Color::srgb(0.36, 0.86, 0.58),
        "Fixed" => Color::srgb(1.0, 0.73, 0.32),
        "Removed" | "Gone" => Color::srgb(1.0, 0.45, 0.5),
        _ => ACCENT,
    }
}

pub fn setup(mut commands: Commands) {
    commands.spawn((
        CardRoot,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, display: Display::None, ..default() },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        GlobalZIndex(40),
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut card: ResMut<Card>,
    mut settings: ResMut<crate::settings::Settings>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mona: Res<MonaSans>,
    mut root: Query<(Entity, &mut Node), With<CardRoot>>,
) {
    let Ok((root, mut node)) = root.single_mut() else { return };
    let Some(c) = card.shown.clone() else {
        if node.display != Display::None {
            node.display = Display::None;
            commands.entity(root).despawn_children();
        }
        return;
    };
    settings.keys_taken = true;
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let rows = !c.options.is_empty();
    let n = if rows { c.options.len() } else { c.buttons.len().max(1) };
    let step = if rows {
        (keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32
    } else {
        (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32
    };
    if step != 0 && n > 1 {
        card.lit = (card.lit as i32 + step).rem_euclid(n as i32) as usize;
        card.built = false;
        sounds.play("tick", step);
    }
    let (ok, back) = settings.confirm();
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(ok) {
        (card.outcome, card.shown) = (Some(Outcome::Picked(card.lit)), None);
        sounds.play("go", 1);
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(back) {
        (card.outcome, card.shown) = (Some(Outcome::Back), None);
        sounds.play("back", 1);
        return;
    }
    if card.built {
        return;
    }
    card.built = true;
    node.display = Display::Flex;
    commands.entity(root).despawn_children();
    let lang = settings.language();
    let t = |s: &str| rustyshelf_core::i18n::tr(&lang, s);
    let text = |s: String, weight: u32, size: f32, color: Color| (Text::new(s), mona.font(weight, size), TextColor(color));
    let lit = card.lit;
    let notes = !c.sections.is_empty();
    commands.entity(root).with_children(|r| {
        r.spawn((
            Node {
                width: px(if notes { 780 } else { 560 }),
                max_width: percent(90),
                max_height: percent(88),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(if notes { 34 } else { 28 })),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(18)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PAPER),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.14)),
        ))
        .with_children(|p| {
            if notes {
                // a band of the sections' colours along the top edge
                p.spawn((
                    Node { position_type: PositionType::Absolute, left: px(0), right: px(0), top: px(0), height: px(4), ..default() },
                    BackgroundGradient(vec![LinearGradient::to_right(vec![
                        ColorStop::new(section_colour("New"), percent(0)),
                        ColorStop::new(ACCENT, percent(50)),
                        ColorStop::new(section_colour("Fixed"), percent(100)),
                    ])
                    .into()]),
                ));
            }
            if !c.kick.is_empty() {
                if notes {
                    p.spawn((Node { align_self: AlignSelf::FlexStart, padding: UiRect::axes(px(10), px(4)), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(ACCENT.with_alpha(0.18))))
                        .with_child(text(t(&c.kick).to_uppercase(), 700, 11.0, ACCENT));
                } else {
                    p.spawn(text(t(&c.kick).to_uppercase(), 600, 11.0, ACCENT));
                }
            }
            p.spawn(text(t(&c.title), 700, if notes { 30.0 } else { 24.0 }, INK));
            if !c.sub.is_empty() {
                p.spawn(text(t(&c.sub), 400, if notes { 16.0 } else { 14.0 }, DIM));
            }
            for item in &c.items {
                p.spawn(text(format!("•  {}", t(item)), 400, 14.0, INK));
            }
            if notes {
                // what doesn't fit is cut off here, above the buttons: All the notes has the rest
                p.spawn(Node { margin: UiRect::top(px(8)), flex_direction: FlexDirection::Column, row_gap: px(16), flex_shrink: 1.0, min_height: px(0), overflow: Overflow::clip(), ..default() }).with_children(|body| {
                    for (h, items) in &c.sections {
                        let colour = section_colour(h);
                        body.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(8), flex_shrink: 0.0, ..default() }).with_children(|sec| {
                            sec.spawn((Node { align_self: AlignSelf::FlexStart, padding: UiRect::axes(px(10), px(3)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(colour.with_alpha(0.16))))
                                .with_child(text(t(h).to_uppercase(), 700, 11.0, colour));
                            for (head, more) in items {
                                sec.spawn(Node { column_gap: px(12), ..default() }).with_children(|item| {
                                    item.spawn((Node { width: px(3), flex_shrink: 0.0, border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(colour.with_alpha(0.7))));
                                    item.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(2), flex_shrink: 1.0, ..default() }).with_children(|words| {
                                        words.spawn(text(t(head), 600, 15.0, INK));
                                        if !more.is_empty() {
                                            words.spawn(text(t(more), 400, 13.0, DIM));
                                        }
                                    });
                                });
                            }
                        });
                    }
                });
            }
            if rows {
                p.spawn(Node { margin: UiRect::top(px(6)), flex_direction: FlexDirection::Column, row_gap: px(4), ..default() }).with_children(|list| {
                    for (i, (label, line)) in c.options.iter().enumerate() {
                        let on = i == lit;
                        list.spawn((
                            Node { padding: UiRect::axes(px(14), px(9)), column_gap: px(12), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, border_radius: BorderRadius::all(px(10)), ..default() },
                            BackgroundColor(if on { Color::srgba(1.0, 1.0, 1.0, 0.14) } else { Color::NONE }),
                        ))
                        .with_children(|row| {
                            row.spawn(text(label.clone(), if on { 700 } else { 500 }, 15.0, INK));
                            row.spawn(text(t(line), 400, 12.0, DIM));
                        });
                    }
                });
            }
            for (k, what) in &c.keys {
                p.spawn(Node { column_gap: px(10), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                    row.spawn((Node { padding: UiRect::axes(px(8), px(3)), border_radius: BorderRadius::all(px(6)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12))))
                        .with_child(text(t(k), 700, 12.0, INK));
                    row.spawn(text(t(what), 400, 13.0, DIM));
                });
            }
            p.spawn(Node { margin: UiRect::top(px(10)), column_gap: px(10), justify_content: JustifyContent::FlexEnd, flex_shrink: 0.0, ..default() }).with_children(|row| {
                for (i, b) in c.buttons.iter().enumerate() {
                    let on = i == lit;
                    row.spawn((
                        Node { padding: UiRect::axes(px(18), px(9)), border_radius: BorderRadius::MAX, border: UiRect::all(px(1)), ..default() },
                        BackgroundColor(if on { INK } else { Color::NONE }),
                        BorderColor::all(if on { INK } else { Color::srgba(1.0, 1.0, 1.0, 0.3) }),
                    ))
                    .with_child(text(t(b), 600, 13.0, if on { PAPER } else { INK }));
                }
            });
        });
    });
}

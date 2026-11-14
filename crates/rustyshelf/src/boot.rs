// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the boot slide of
// Boxshelf's renderer/bootslide.css and renderer/app.js (runSlide), as RustyShelf has them.
//! The boot slide: when a console's game is started, a full screen in the console's own colours with its name, the
//! game's cover and title and a line filling, over the emulator starting.

use bevy::prelude::*;
use bevy::text::{FontSize, LetterSpacing};
use bevy::ui::{UiTransform, Val2};
use rustyshelf_core::looks;

use crate::hud::Lexend;
use crate::loading::to_image;
use crate::shelf::{bezier, Stage};

/// Seconds the slide stays: the line is full by then, and the emulator's window is up.
const STAYS: f32 = 4.0;
const LEAVES: f32 = 0.35;

/// The slide that's up, or was: what it's for, and since when.
#[derive(Resource, Default)]
pub struct Slide {
    since: f32,
    /// when it began to leave
    left: Option<f32>,
    on: bool,
    /// to be set out: the game's shelf, its title, what starts it, and its cover when that's loaded
    fresh: Option<(String, String, String, Option<Handle<Image>>)>,
}

impl Slide {
    /// Puts the slide up for a game of the shelf `shelf`.
    pub fn start(&mut self, now: f32, shelf: &str, title: &str, emulator: &str, cover: Option<Handle<Image>>) {
        (self.since, self.left, self.on) = (now, None, true);
        self.fresh = Some((shelf.to_string(), title.to_string(), emulator.to_string(), cover));
    }
}

/// The parts of the slide: its backdrop; the cover; the words, each with its own colour; the line that fills.
#[derive(Component)]
pub enum Part {
    Whole,
    Back,
    Cover,
    Words,
    Booting,
    Name,
    Title,
    Emulator,
    Bar,
    Fill,
}

pub fn setup(mut commands: Commands, lexend: Res<Lexend>) {
    let text = |part: Part, weight: u32, size: f32, spacing: f32| (part, Text::default(), lexend.font(weight, size), TextColor(Color::WHITE), LetterSpacing::Px(spacing * size));
    commands.spawn((
        Part::Whole,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), align_items: AlignItems::Center, justify_content: JustifyContent::Center, padding: UiRect::horizontal(vw(8)), ..default() },
        GlobalZIndex(40),
        Visibility::Hidden,
        children![
            (Part::Back, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), ..default() }, ImageNode::default().with_color(Color::NONE)),
            (Part::Cover, Node { flex_shrink: 0.0, border_radius: BorderRadius::all(px(8)), overflow: Overflow::clip(), ..default() }, ImageNode::default(), UiTransform::IDENTITY),
            (
                Part::Words,
                Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexStart, max_width: vw(44), ..default() },
                UiTransform::IDENTITY,
                children![
                    (Node { margin: UiRect::bottom(px(18)), ..default() }, text(Part::Booting, 600, 12.0, 0.32)),
                    (Part::Name, Node::default(), ImageNode::default()),
                    (Node { margin: UiRect::top(px(26)), ..default() }, text(Part::Title, 700, 40.0, -0.01)),
                    (Node { margin: UiRect::top(px(10)), ..default() }, text(Part::Emulator, 500, 14.0, 0.0)),
                    (
                        Part::Bar,
                        Node { margin: UiRect::top(px(30)), height: px(3), border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip(), ..default() },
                        BackgroundColor(Color::NONE),
                        children![(Part::Fill, Node { width: percent(0), height: percent(100), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(Color::NONE))],
                    ),
                ]
            ),
        ],
    ));
}

fn color(c: looks::Rgba) -> Color {
    Color::srgba(c[0], c[1], c[2], c[3])
}

/// Sets the slide out for the game it's for, and moves it: in, the line filling, and away once the emulator has
/// had its time or the game didn't start.
pub fn run(
    time: Res<Time>,
    stage: Res<Stage>,
    playing: Res<crate::play::Playing>,
    mut slide: ResMut<Slide>,
    mut images: ResMut<Assets<Image>>,
    mut parts: Query<(&Part, &mut Node, &mut Visibility, Option<&mut ImageNode>, Option<&mut Text>, Option<&mut TextColor>, Option<&mut TextFont>, Option<&mut BackgroundColor>, Option<&mut UiTransform>)>,
    mut inks: Local<(Color, Color, Color)>,
) {
    let now = time.elapsed_secs();
    if !slide.on {
        return;
    }
    if let Some((shelf, title, emulator, cover)) = slide.fresh.take() {
        let (look, boot, lettering) = (looks::look(&shelf), looks::boot(&shelf), looks::lettering(&shelf));
        *inks = (color(boot.ink), color(boot.dim), color(boot.acc));
        let name = looks::wordmark(&shelf).map_or(shelf.as_str(), |w| w.0).to_string();
        let (w, h) = (stage.w, stage.h);
        for (part, mut node, _, image, text, _, font, _, _) in &mut parts {
            match part {
                Part::Whole => node.column_gap = px((w * 0.05).clamp(32.0, 90.0)),
                Part::Back => {
                    // the console's backdrop, a dark band along its foot (a taskbar peeking over it mustn't flash)
                    let (pw, ph) = ((w / 4.0).round().max(2.0) as u32, (h / 4.0).round().max(2.0) as u32);
                    let mut c = rustyshelf_core::cssbg::Canvas::new(pw, ph, 0.25);
                    c.paint(&boot.bg, (0.0, 0.0, w, h), 1.0);
                    let band = if shelf == "wii" { "linear-gradient(180deg,transparent,rgba(40,50,70,.35))" } else { "linear-gradient(180deg,transparent,rgba(0,0,0,.55))" };
                    c.paint(band, (0.0, h * 0.91, w, h * 0.09), 1.0);
                    let p = c.picture();
                    if let Some(mut image) = image {
                        image.image = images.add(to_image(p.width, p.height, p.rgba));
                    }
                }
                Part::Cover => {
                    let tall = (h * 0.52).min(w * 0.6);
                    (node.height, node.width) = (px(tall), px(tall * look.ratio));
                    node.display = if cover.is_some() { Display::Flex } else { Display::None };
                    if let (Some(mut image), Some(cover)) = (image, &cover) {
                        image.image = cover.clone();
                    }
                }
                Part::Name => {
                    let p = rustyshelf_core::drawn::lettered(&name, lettering, (w * 0.062).clamp(46.0, 96.0), boot.word.as_deref().unwrap_or(&lettering.grad), false, 2.0);
                    (node.width, node.height, node.margin) = (px(p.width as f32 / 2.0), px(p.height as f32 / 2.0), UiRect::all(px(-rustyshelf_core::drawn::WORD_ROOM)));
                    if let Some(mut image) = image {
                        image.image = images.add(to_image(p.width, p.height, p.rgba));
                    }
                }
                Part::Bar => node.width = px((w * 0.3).min(320.0)),
                Part::Booting | Part::Title | Part::Emulator => {
                    if let Some(mut text) = text {
                        text.0 = match part {
                            Part::Booting => format!("Booting {name}").to_uppercase(),
                            Part::Title => title.clone(),
                            _ if emulator.is_empty() => String::new(),
                            _ => format!("Starting in {emulator}"),
                        };
                    }
                    if let (Part::Title, Some(mut font)) = (part, font) {
                        font.font_size = FontSize::Px((w * 0.026).clamp(24.0, 40.0));
                    }
                }
                Part::Words | Part::Fill => {}
            }
        }
    }
    // it leaves when its time is up, or at once when the game didn't start
    if slide.left.is_none() && (now - slide.since >= STAYS || playing.title.is_none()) {
        slide.left = Some(now);
    }
    let away = slide.left.map_or(0.0, |left| (now - left) / LEAVES);
    if away >= 1.0 {
        slide.on = false;
    }
    let age = now - slide.since;
    let leaving = 1.0 - away.clamp(0.0, 1.0);
    let whole = bezier([0.0, 0.0, 0.58, 1.0], age / 0.45) * leaving;
    let cover = bezier([0.2, 0.8, 0.2, 1.0], (age - 0.25) / 0.9);
    let words = bezier([0.2, 0.8, 0.2, 1.0], (age - 0.45) / 0.8);
    let filled = bezier([0.4, 0.1, 0.4, 1.0], (age - 0.2) / 3.8);
    // once full the line breathes
    let held = if age > 4.2 { 1.0 - 0.45 * (0.5 - 0.5 * ((age - 4.2) / 1.6 * std::f32::consts::TAU).cos()) } else { 1.0 };
    let (ink, dim, acc) = *inks;
    for (part, mut node, mut seen, image, _, text_color, _, background, transform) in &mut parts {
        match part {
            Part::Whole => {
                seen.set_if_neq(if slide.on { Visibility::Visible } else { Visibility::Hidden });
            }
            Part::Back => {
                if let Some(mut image) = image {
                    image.color = Color::srgba(1.0, 1.0, 1.0, whole);
                }
            }
            Part::Cover => {
                if let Some(mut image) = image {
                    image.color = Color::srgba(1.0, 1.0, 1.0, cover * leaving);
                }
                if let Some(mut t) = transform {
                    (t.translation, t.scale) = (Val2::px(-30.0 * (1.0 - cover), 0.0), Vec2::splat(0.94 + 0.06 * cover));
                }
            }
            Part::Words => {
                if let Some(mut t) = transform {
                    t.translation = Val2::px(24.0 * (1.0 - words), 0.0);
                }
            }
            Part::Name => {
                if let Some(mut image) = image {
                    image.color = Color::srgba(1.0, 1.0, 1.0, words * leaving);
                }
            }
            Part::Booting | Part::Emulator | Part::Title => {
                if let Some(mut c) = text_color {
                    let own = if matches!(part, Part::Title) { ink } else { dim };
                    c.0 = own.with_alpha(own.alpha() * words * leaving);
                }
            }
            Part::Bar => {
                if let Some(mut bg) = background {
                    bg.0 = ink.with_alpha(0.16 * words * leaving);
                }
            }
            Part::Fill => {
                node.width = percent(filled * 100.0);
                if let Some(mut bg) = background {
                    bg.0 = acc.with_alpha(acc.alpha() * words * leaving * held);
                }
            }
        }
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the manual reader
// of Boxshelf's renderer/app.js (openReader, showSpread, rdTurn) and renderer/inspect.css, as RustyShelf has them.
//! The manual reader: a game's manual pages (the pictures beside its box art) as an open booklet. The first
//! page is the cover on its own; after that the pages come in pairs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use bevy::prelude::*;

use crate::hud::Lexend;
use crate::loading::to_image;
use crate::settings::Settings;
use crate::shelf::Stage;
use crate::sound::Sounds;

/// The tallest a page is kept, in pixels: small print stays readable, a scan of any size fits the card's memory.
const TALLEST: u32 = 1600;
/// pages kept ready at most
const KEPT: usize = 24;

type Decoded = (PathBuf, Option<(u32, u32, Vec<u8>)>);

#[derive(Resource)]
pub struct Reader {
    pages: Vec<PathBuf>,
    /// the page on the left, or 0 for the cover on its own
    at: usize,
    open: bool,
    fresh: bool,
    /// each page's picture and its width over its height; None for one that couldn't be read
    ready: HashMap<PathBuf, Option<(Handle<Image>, f32)>>,
    asked: HashSet<PathBuf>,
    ask: Sender<PathBuf>,
    read: Mutex<Receiver<Decoded>>,
}

impl Default for Reader {
    fn default() -> Self {
        let (ask, wanted) = channel::<PathBuf>();
        let (done, read) = channel();
        std::thread::spawn(move || {
            while let Ok(file) = wanted.recv() {
                let picture = image::ImageReader::open(&file).ok().and_then(|r| r.with_guessed_format().ok()).and_then(|r| r.decode().ok()).map(|img| {
                    let img = if img.height() > TALLEST { img.resize(u32::MAX, TALLEST, image::imageops::FilterType::Triangle) } else { img };
                    let rgba = img.into_rgba8();
                    (rgba.width(), rgba.height(), rgba.into_raw())
                });
                if done.send((file, picture)).is_err() {
                    return;
                }
            }
        });
        Reader { pages: Vec::new(), at: 0, open: false, fresh: false, ready: HashMap::new(), asked: HashSet::new(), ask, read: Mutex::new(read) }
    }
}

impl Reader {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Puts the manual away, as backing out of it does.
    pub fn close(&mut self) {
        (self.open, self.fresh) = (false, true);
    }

    /// The page on the left and the one on the right, as the booklet lies open.
    fn spread(&self) -> (Option<&PathBuf>, Option<&PathBuf>) {
        if self.at == 0 { (None, self.pages.first()) } else { (self.pages.get(self.at), self.pages.get(self.at + 1)) }
    }

    fn count(&self) -> String {
        let n = self.pages.len();
        match self.at {
            0 => format!("Cover · {n} pages"),
            at if at + 1 < n => format!("Pages {}–{} of {n}", at + 1, at + 2),
            at => format!("Pages {} of {n}", at + 1),
        }
    }

    /// Turns a spread on (1) or back (-1); false at the last page.
    fn turn(&mut self, by: i32) -> bool {
        let to = if self.at == 0 { by.max(0) } else { (self.at as i32 + by * 2).max(0) };
        if to as usize >= self.pages.len() {
            return false;
        }
        (self.at, self.fresh) = (to as usize, true);
        true
    }
}

#[derive(Component)]
pub enum Part {
    Whole,
    Left,
    Right,
    Count,
    Keys,
}

pub fn setup(mut commands: Commands, lexend: Res<Lexend>, face: Res<crate::hud::Face>) {
    let page = |part: Part| {
        (
            part,
            Node { align_items: AlignItems::Center, justify_content: JustifyContent::Center, border_radius: BorderRadius::all(px(3)), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgb(0.957, 0.949, 0.925)),
            children![(Node::default(), ImageNode::default())],
        )
    };
    commands.spawn((
        Part::Whole,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() },
        // the page's black of .82, as deep as a browser's blending makes it
        BackgroundColor(Color::srgba(0.008, 0.012, 0.031, 0.977)),
        GlobalZIndex(44),
        Visibility::Hidden,
        children![
            (Node { column_gap: px(2), justify_content: JustifyContent::Center, ..default() }, children![page(Part::Left), page(Part::Right)]),
            (Part::Count, Node { margin: UiRect::top(vh(3)), ..default() }, Text::default(), lexend.font(500, 15.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8))),
            // (Lexend has no arrows: the keys are set in the font with more)
            (Part::Keys, Node { margin: UiRect::top(px(10)), ..default() }, Text::default(), TextFont::from_font_size(bevy::text::FontSize::Px(12.0)).with_font(face.0.clone()), TextColor(Color::srgb(0.588, 0.596, 0.627))),
        ],
    ));
}

/// Opens the manual Details asked for, turns its pages and lays them out.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    stage: Res<Stage>,
    mut settings: ResMut<Settings>,
    mut sounds: ResMut<Sounds>,
    mut reader: ResMut<Reader>,
    mut images: ResMut<Assets<Image>>,
    mut parts: Query<(&Part, &mut Node, &mut Visibility, Option<&mut Text>, Option<&Children>)>,
    mut pictures: Query<(&mut Node, &mut ImageNode), Without<Part>>,
) {
    let reader = &mut *reader;
    if let Some(pages) = settings.manual.take().filter(|p| !p.is_empty()) {
        (reader.pages, reader.at, reader.open, reader.fresh) = (pages, 0, true, true);
        reader.ready.clear();
        reader.asked.clear();
        sounds.play("open", 1);
    } else if reader.open {
        let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
        let by = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) || pad(GamepadButton::RightTrigger)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) || pad(GamepadButton::LeftTrigger)) as i32;
        if by != 0 {
            let turned = reader.turn(by);
            sounds.play(if turned { "page" } else { "error" }, by);
        }
        if keys.just_pressed(KeyCode::Home) && reader.at != 0 {
            (reader.at, reader.fresh) = (0, true);
            sounds.play("page", -1);
        }
        if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(settings.confirm().1) {
            (reader.open, reader.fresh) = (false, true);
            sounds.play("back", 1);
        }
        settings.keys_taken = true;
    }

    // the pages in view and the next few are read off the frame
    if reader.open {
        let ahead: Vec<PathBuf> = reader.pages.iter().skip(reader.at.saturating_sub(2)).take(7).filter(|p| !reader.asked.contains(*p)).cloned().collect();
        for file in ahead {
            reader.asked.insert(file.clone());
            let _ = reader.ask.send(file);
        }
    }
    let read: Vec<Decoded> = reader.read.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default();
    for (file, picture) in read {
        if reader.ready.len() >= KEPT {
            let (left, right) = reader.spread();
            let shown: Vec<PathBuf> = [left, right].into_iter().flatten().cloned().collect();
            if let Some(old) = reader.ready.keys().find(|k| !shown.contains(k)).cloned() {
                reader.ready.remove(&old);
                reader.asked.remove(&old);
            }
        }
        reader.ready.insert(file, picture.map(|(w, h, rgba)| (images.add(to_image(w, h, rgba)), w as f32 / h.max(1) as f32)));
        reader.fresh = true;
    }
    if !std::mem::take(&mut reader.fresh) {
        return;
    }
    let (left, right) = reader.spread();
    let (left, right) = (left.and_then(|p| reader.ready.get(p)).cloned().flatten(), right.and_then(|p| reader.ready.get(p)).cloned().flatten());
    let (tall, widest) = (stage.h * 0.76, stage.w * 0.465);
    for (part, mut node, mut seen, text, children) in &mut parts {
        match part {
            Part::Whole => {
                seen.set_if_neq(if reader.open { Visibility::Inherited } else { Visibility::Hidden });
            }
            Part::Count => {
                if let Some(mut text) = text {
                    text.0 = reader.count();
                }
            }
            Part::Keys => {
                if let Some(mut text) = text {
                    text.0 = format!("◀ ▶  Turn the page      Home  Cover      Esc · {}  Back", settings.cap("o"));
                }
            }
            Part::Left | Part::Right => {
                let page = if matches!(part, Part::Left) { &left } else { &right };
                node.display = if page.is_some() { Display::Flex } else { Display::None };
                let Some((picture, shape)) = page else { continue };
                // as tall as the booklet, never wider than its half of the screen
                let wide = (tall * shape).min(widest);
                (node.width, node.height) = (px(wide), px(tall));
                if let Some((mut inner, mut image)) = children.and_then(|c| c.first()).and_then(|c| pictures.get_mut(*c).ok()) {
                    (inner.width, inner.height) = (px(wide), px((wide / shape).min(tall)));
                    *image = ImageNode { image: picture.clone(), image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cover_is_alone_and_the_rest_come_in_pairs() {
        let mut r = Reader { pages: (1..=6).map(|n| PathBuf::from(format!("p{n}"))).collect(), ..Reader::default() };
        let names = |r: &Reader| { let (l, rt) = r.spread(); (l.map(|p| p.to_string_lossy().into_owned()), rt.map(|p| p.to_string_lossy().into_owned())) };
        assert_eq!((names(&r), r.count()), ((None, Some("p1".into())), "Cover · 6 pages".into()));
        assert!(!r.turn(-1) || r.at == 0);
        assert!(r.turn(1));
        assert_eq!((names(&r), r.count()), ((Some("p2".into()), Some("p3".into())), "Pages 2–3 of 6".into()));
        assert!(r.turn(1) && r.turn(1));
        assert_eq!((names(&r), r.count()), ((Some("p6".into()), None), "Pages 6 of 6".into()));
        // no further than the last page, and back to the cover from the first pair
        assert!(!r.turn(1));
        assert!(r.turn(-1) && r.turn(-1) && r.turn(-1));
        assert_eq!(r.at, 0);
    }
}

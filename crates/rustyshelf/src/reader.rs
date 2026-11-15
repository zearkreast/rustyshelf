// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the manual reader
// of Boxshelf's renderer/app.js (openReader, showSpread, rdTurn) and renderer/inspect.css, as RustyShelf has them.
//! The manual reader: a game's manual pages (the pictures beside its box art) as an open booklet. The first
//! page is the cover on its own; after that the pages come in pairs. On a small screen one page at a time, and a
//! zoom that steps to the page's full width and closer, with the zoomed page moved around.

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
/// how close a zoomed page's edges come to the screen's, in pixels; the room kept under it for the keys
const EDGE: f32 = 24.0;
const FOOT: f32 = 110.0;

type Decoded = (PathBuf, Option<(u32, u32, Vec<u8>)>);

#[derive(Resource)]
pub struct Reader {
    pages: Vec<PathBuf>,
    /// the page on the left, or 0 for the cover on its own
    at: usize,
    open: bool,
    fresh: bool,
    /// one page at a time (a small screen, or P / X)
    one: bool,
    /// the zoom, 1 for the whole page, and how far the zoomed page is moved, in pixels
    z: f32,
    pan: Vec2,
    /// the page each manual was left at this session, by its first page
    left_at: HashMap<PathBuf, usize>,
    /// how big the pages in view are drawn unzoomed, in pixels
    base: Vec2,
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
        Reader { pages: Vec::new(), at: 0, open: false, fresh: false, one: false, z: 1.0, pan: Vec2::ZERO, left_at: HashMap::new(), base: Vec2::ZERO, ready: HashMap::new(), asked: HashSet::new(), ask, read: Mutex::new(read) }
    }
}

impl Reader {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Puts the manual away, as backing out of it does.
    pub fn close(&mut self) {
        self.keep();
        (self.open, self.fresh, self.z, self.pan) = (false, true, 1.0, Vec2::ZERO);
    }

    fn keep(&mut self) {
        if let Some(first) = self.pages.first() {
            self.left_at.insert(first.clone(), self.at);
        }
    }

    /// The page on the left and the one on the right, as the booklet lies open.
    fn spread(&self) -> (Option<&PathBuf>, Option<&PathBuf>) {
        if self.one {
            (None, self.pages.get(self.at))
        } else if self.at == 0 {
            (None, self.pages.first())
        } else {
            (self.pages.get(self.at), self.pages.get(self.at + 1))
        }
    }

    fn count(&self) -> String {
        let n = self.pages.len();
        match self.at {
            0 => format!("Cover · {n} pages"),
            at if self.one => format!("Page {} of {n}", at + 1),
            at if at + 1 < n => format!("Pages {}–{} of {n}", at + 1, at + 2),
            at => format!("Pages {} of {n}", at + 1),
        }
    }

    /// Turns a page (one at a time) or a spread on (1) or back (-1); false at the last page.
    fn turn(&mut self, by: i32) -> bool {
        let to = if self.one { self.at as i32 + by } else if self.at == 0 { by.max(0) } else { self.at as i32 + by * 2 }.max(0);
        if to as usize >= self.pages.len() {
            return false;
        }
        (self.at, self.fresh) = (to as usize, true);
        self.keep();
        if self.z > 1.0 {
            self.top();
        }
        true
    }

    /// One page at a time or two; the page being read stays in view.
    fn pages_at_once(&mut self, one: bool) {
        if one == self.one {
            return;
        }
        self.one = one;
        if !one && self.at > 0 && self.at.is_multiple_of(2) {
            self.at -= 1;
        }
        (self.z, self.pan, self.fresh) = (1.0, Vec2::ZERO, true);
    }

    /// The zoom that makes the pages in view as wide as the screen.
    fn fit_width(&self, screen: Vec2) -> f32 {
        if self.base.x <= 0.0 {
            return 1.6;
        }
        let f = screen.x * 0.96 / self.base.x;
        if f < 1.25 { 1.6 } else { f.min(4.0) }
    }

    /// A step of zoom in (1) or out (-1): the whole page, the page's width, then closer. False when there's none.
    fn step(&mut self, by: i32, screen: Vec2) -> bool {
        let fit = self.fit_width(screen);
        let steps = [1.0, fit, (fit * 1.6).min(4.0)];
        let to = if by > 0 { steps.iter().find(|s| **s > self.z + 0.02) } else { steps.iter().rev().find(|s| **s < self.z - 0.02) };
        let Some(&to) = to else { return false };
        let was = self.z;
        self.z = to;
        if to <= 1.0 {
            self.pan = Vec2::ZERO;
        } else if was <= 1.0 {
            self.top();
        }
        self.clamp(screen);
        self.fresh = true;
        true
    }

    /// The top of the page in view, as a page is read.
    fn top(&mut self) {
        self.pan = Vec2::new(0.0, f32::MAX);
    }

    /// Keeps a zoomed page's edges within reach: never further in than EDGE from the screen's edges, its foot above
    /// the keys.
    fn clamp(&mut self, screen: Vec2) {
        if self.z <= 1.0 {
            self.pan = Vec2::ZERO;
            return;
        }
        let size = self.base * self.z;
        let room_x = ((size.x - screen.x) / 2.0 + EDGE).max(0.0);
        let room_y = ((size.y - (screen.y - FOOT)) / 2.0 + EDGE).max(0.0);
        self.pan = Vec2::new(self.pan.x.clamp(-room_x, room_x), self.pan.y.clamp(-room_y, room_y));
    }
}

#[derive(Component)]
pub enum Part {
    Whole,
    Left,
    Right,
    Count,
    Keys,
    /// the pages together: zoomed and moved as one
    Book,
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
            (Part::Book, UiTransform::default(), Node { column_gap: px(2), justify_content: JustifyContent::Center, ..default() }, children![page(Part::Left), page(Part::Right)]),
            // (over the page when it's zoomed: on a dark pill, as the Electron app's hint bar)
            (Part::Count, Node { margin: UiRect::top(vh(3)), padding: UiRect::axes(px(10), px(3)), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(0.04, 0.05, 0.09, 0.78)), Text::default(), lexend.font(500, 15.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8))),
            // (Lexend has no arrows: the keys are set in the font with more)
            (Part::Keys, Node { margin: UiRect::top(px(8)), padding: UiRect::axes(px(12), px(5)), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(0.04, 0.05, 0.09, 0.78)), Text::default(), TextFont::from_font_size(bevy::text::FontSize::Px(12.0)).with_font(face.0.clone()), TextColor(Color::srgb(0.588, 0.596, 0.627))),
        ],
    ));
}

/// Opens the manual Details asked for, turns its pages and lays them out.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run(
    time: Res<Time>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    stage: Res<Stage>,
    mut settings: ResMut<Settings>,
    mut sounds: ResMut<Sounds>,
    mut reader: ResMut<Reader>,
    mut images: ResMut<Assets<Image>>,
    mut parts: Query<(&Part, &mut Node, &mut Visibility, Option<&mut Text>, Option<&Children>, Option<&mut UiTransform>)>,
    mut pictures: Query<(&mut Node, &mut ImageNode), Without<Part>>,
) {
    let reader = &mut *reader;
    if let Some(pages) = settings.manual.take().filter(|p| !p.is_empty()) {
        let back_at = pages.first().and_then(|f| reader.left_at.get(f)).copied().filter(|&at| at < pages.len()).unwrap_or(0);
        reader.one = stage.h <= 820.0 || stage.w < 1000.0;
        let back_at = if reader.one || !back_at.is_multiple_of(2) || back_at == 0 { back_at } else { back_at - 1 };
        (reader.pages, reader.at, reader.open, reader.fresh, reader.z, reader.pan) = (pages, back_at, true, true, 1.0, Vec2::ZERO);
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
        let screen = Vec2::new(stage.w, stage.h);
        let zoom = (keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) || pad(GamepadButton::RightTrigger2)) as i32
            - (keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) || pad(GamepadButton::LeftTrigger2)) as i32;
        if zoom != 0 {
            let stepped = reader.step(zoom, screen);
            sounds.play(if stepped { "tick" } else { "error" }, zoom);
        }
        if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0) {
            (reader.z, reader.pan, reader.fresh) = (1.0, Vec2::ZERO, true);
        }
        if keys.just_pressed(KeyCode::KeyP) || pad(GamepadButton::West) {
            let one = !reader.one;
            reader.pages_at_once(one);
            sounds.play("tick", 1);
        }
        // zoomed in, the arrows up and down, the d-pad's, the mouse wheel or the right stick move around the page
        if reader.z > 1.0 {
            let dt = time.delta_secs();
            let mut by = Vec2::ZERO;
            by.y += (keys.pressed(KeyCode::ArrowUp) as i32 - keys.pressed(KeyCode::ArrowDown) as i32) as f32 * 900.0 * dt;
            for p in &pads {
                let (r, d) = (p.right_stick(), p.dpad());
                let r = Vec2::new(if r.x.abs() > 0.15 { r.x } else { 0.0 }, if r.y.abs() > 0.15 { r.y } else { 0.0 });
                by += Vec2::new(-r.x * r.x.abs(), r.y * r.y.abs() + d.y * 0.6) * 1100.0 * dt;
            }
            for w in wheel.read() {
                let px = if w.unit == bevy::input::mouse::MouseScrollUnit::Line { 60.0 } else { 1.0 };
                by += Vec2::new(w.x, w.y) * px;
            }
            if by != Vec2::ZERO {
                reader.pan += by;
                reader.clamp(screen);
                reader.fresh = true;
            }
        } else {
            wheel.clear();
        }
        if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(settings.confirm().1) {
            reader.close();
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
    let (tall, widest) = (stage.h * 0.76, stage.w * if reader.one { 0.94 } else { 0.465 });
    let wide_of = |page: &Option<(Handle<Image>, f32)>| page.as_ref().map_or(0.0, |(_, shape)| (tall * shape).min(widest));
    let shown = [&left, &right].iter().filter(|p| p.is_some()).count();
    reader.base = Vec2::new(wide_of(&left) + wide_of(&right) + if shown > 1 { 2.0 } else { 0.0 }, tall);
    if reader.pan.y == f32::MAX {
        reader.pan.y = ((reader.base.y * reader.z - (stage.h - FOOT)) / 2.0 + EDGE).max(0.0);
    }
    reader.clamp(Vec2::new(stage.w, stage.h));
    for (part, mut node, mut seen, text, children, moved) in &mut parts {
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
                    let zoom = if reader.z > 1.0 { "+ −  Zoom · move with ▲ ▼" } else { "+ −  Zoom" };
                    let pages = if reader.one { "P  Two pages" } else { "P  One page" };
                    text.0 = format!("◀ ▶  Turn the page      {zoom}      {pages}      Home  Cover      Esc · {}  Back", settings.cap("o"));
                }
            }
            Part::Book => {
                if let Some(mut t) = moved {
                    *t = UiTransform { translation: Val2::px(reader.pan.x, reader.pan.y), scale: Vec2::splat(reader.z), ..default() };
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

    #[test]
    fn one_page_at_a_time_and_zoom_steps() {
        let mut r = Reader { pages: (1..=6).map(|n| PathBuf::from(format!("p{n}"))).collect(), one: true, base: Vec2::new(500.0, 700.0), ..Reader::default() };
        assert!(r.turn(1) && r.turn(1));
        assert_eq!((r.at, r.count()), (2, "Page 3 of 6".into()));
        // to two pages: the page being read is the spread's right
        r.pages_at_once(false);
        assert_eq!((r.at, r.count()), (1, "Pages 2–3 of 6".into()));
        let screen = Vec2::new(1280.0, 720.0);
        assert!(r.step(1, screen));
        assert!((r.z - 1280.0 * 0.96 / 500.0).abs() < 1e-3);
        assert!(r.step(1, screen) && !r.step(1, screen));
        let z = 1280.0 * 0.96 / 500.0 * 1.6;
        assert!((r.z - z).abs() < 1e-3);
        r.pan = Vec2::new(1e6, -1e6);
        r.clamp(screen);
        let room = Vec2::new((500.0 * r.z - 1280.0) / 2.0 + EDGE, -((700.0 * r.z - (720.0 - FOOT)) / 2.0 + EDGE));
        assert!((r.pan - room).length() < 1e-2, "{:?} {room:?}", r.pan);
        assert!(r.step(-1, screen) && r.step(-1, screen) && !r.step(-1, screen));
        assert_eq!((r.z, r.pan), (1.0, Vec2::ZERO));
    }
}

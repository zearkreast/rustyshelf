// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): laid out as the
// shelf's top bar, title and hint bar in Boxshelf's renderer/index.html, style.css and app.js, as RustyShelf has them.
//! What's written over the shelf: the console's name, the consoles' tabs, the clock, the chosen game and the keys.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::text::{FontSize, FontSource, FontWeight, FontWidth, LetterSpacing};
use rustyshelf_core::{looks, pads};
use rustyshelf_core::scanner::{Game, Source, Tile};

use crate::loading::{to_image, LibraryRes, LibraryState};
use crate::play::Playing;
use crate::shelf::{Cursor, Stage};

const INK: Color = Color::srgb(0.953, 0.961, 0.984);
const INK_DIM: Color = Color::srgba(0.953, 0.961, 0.984, 0.62);
const GLASS: Color = Color::srgba(0.031, 0.039, 0.078, 0.46);
const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.16);
const PAPER: Color = Color::srgb(0.922, 0.922, 0.941);
const ON_PAPER: Color = Color::srgb(0.063, 0.071, 0.102);

#[cfg(test)]
mod tests {
    #[test]
    fn playtime() {
        assert_eq!(super::played(125), "2.1 h played");
        assert_eq!(super::played(45), "0.8 h played");
        assert_eq!(super::played(15), "0.3 h played");
        assert_eq!(super::played(9), "0.1 h played");
        assert_eq!(super::played(75), "1.3 h played");
        assert_eq!(super::played(57), "0.9 h played");
        assert_eq!(super::played(0), "Not played yet");
    }
}


/// The pieces of the bar whose words change.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// the line under the console's name, and the count of its games
    Tag,
    Count,
    Date,
    Time,
    Counter,
    Title,
    Meta,
    Notice,
}

/// The HUD's whole tree (attract mode hides it).
#[derive(Component)]
pub struct HudRoot;

/// The console's name, lettered; the consoles' tabs; one tab; the keys' bar; what hides while Settings is open.
#[derive(Component)]
pub struct Word;
#[derive(Component)]
pub struct Tabs;
#[derive(Component)]
pub struct Tab(usize);
#[derive(Component)]
pub struct Hints;
#[derive(Component)]
pub struct Info;
#[derive(Component)]
pub struct NoticePill;

/// A message shown for a few seconds (a game that didn't start).
#[derive(Resource, Default)]
pub struct Toast {
    pub text: String,
    pub until: f32,
}

/// The font Settings is written in.
#[derive(Resource)]
pub struct Face(pub Handle<Font>);

/// Mona Sans, the shelf's font: its Latin and Latin Extended files, at any weight.
#[derive(Resource)]
pub struct MonaSans([Handle<Font>; 2]);

impl MonaSans {
    pub fn font(&self, weight: u32, size: f32) -> TextFont {
        // small light letters on the dark shelf come out heavier here than in the browser: a weight lighter
        let weight = if size < 16.0 { weight.saturating_sub(100).max(300) } else { weight };
        let mut font = TextFont::from_font_size(FontSize::Px(size));
        font.font = FontSource::List(self.0.iter().cloned().map(FontSource::Handle).collect());
        font.weight = FontWeight(weight.clamp(200, 900) as u16);
        font
    }

    /// At a size relative to the window, with another font for the arrows and symbols Mona Sans hasn't got.
    pub fn sized(&self, weight: u32, size: FontSize, also: &Handle<Font>) -> TextFont {
        let mut font = TextFont::from_font_size(size);
        font.font = FontSource::List(self.0.iter().chain([also]).cloned().map(FontSource::Handle).collect());
        font.weight = FontWeight(weight.clamp(200, 900) as u16);
        font
    }

    /// A title, in the font's wide cut.
    pub fn wide(&self, weight: u32, size: f32) -> TextFont {
        TextFont { width: FontWidth::EXPANDED, ..self.font(weight, size) }
    }
}

/// Whether the keys shown are the pad's: the last thing pressed was on one.
#[derive(Resource, Default)]
pub struct Prompts {
    pub pad: bool,
    /// the controller last pressed
    in_use: Option<Entity>,
    /// the controllers there were when the real ones plugged in were last asked for, those, and an answer on its way
    pads: usize,
    real: Vec<pads::Real>,
    asking: Option<Mutex<Receiver<Vec<pads::Real>>>>,
}

// the default font has no ◀ ▶ or ·
const FONT: &[u8] = include_bytes!("../../img-tools/assets/DejaVuSans-Bold.ttf");

pub fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let face = fonts.add(Font::from_bytes(FONT.to_vec()));
    commands.insert_resource(Face(face.clone()));
    let mona = MonaSans(rustyshelf_core::drawn::mona_sans().map(|file| fonts.add(Font::from_bytes(file.to_vec()))));
    let text = |line: Line, weight: u32, size: f32, color: Color, spacing: f32| (line, Text::default(), mona.font(weight, size), TextColor(color), LetterSpacing::Px(spacing * size));
    commands.spawn((
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        Pickable::IGNORE,
        HudRoot,
        Visibility::Inherited,
        children![
            // the top bar: the console, the tabs, the clock
            (
                Node { position_type: PositionType::Absolute, left: px(32), top: px(26), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexStart, ..default() },
                children![
                    (Word, Node::default(), ImageNode::default().with_color(Color::NONE)),
                    (Node { margin: UiRect::top(px(7)), ..default() }, text(Line::Tag, 600, 9.0, INK_DIM, 0.38)),
                    (Node { margin: UiRect::top(px(8)), ..default() }, text(Line::Count, 500, 11.0, INK_DIM, 0.24)),
                ]
            ),
            (
                Node { position_type: PositionType::Absolute, top: px(26), width: percent(100), justify_content: JustifyContent::Center, ..default() },
                children![(
                    Tabs,
                    Node { max_width: vw(58), padding: UiRect::all(px(4)), column_gap: px(4), border: UiRect::all(px(1)), border_radius: BorderRadius::MAX, overflow: Overflow::clip_x(), ..default() },
                    BackgroundColor(GLASS),
                    BorderColor::all(LINE),
                )]
            ),
            (
                Node { position_type: PositionType::Absolute, right: px(32), top: px(26), flex_direction: FlexDirection::Column, align_items: AlignItems::FlexEnd, row_gap: px(10), ..default() },
                children![
                    (
                        Node { align_items: AlignItems::Baseline, column_gap: px(8), ..default() },
                        children![text(Line::Date, 500, 13.0, INK_DIM, 0.08), text(Line::Time, 700, 15.0, INK, 0.08)]
                    ),
                    text(Line::Counter, 500, 14.0, INK, 0.06),
                ]
            ),
            // a message, down from the top
            (
                Node { position_type: PositionType::Absolute, top: px(22), width: percent(100), justify_content: JustifyContent::Center, ..default() },
                children![(
                    NoticePill,
                    Node { padding: UiRect::axes(px(16), px(9)), border_radius: BorderRadius::MAX, display: Display::None, ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.95)),
                    children![text(Line::Notice, 600, 13.0, ON_PAPER, 0.0)],
                )]
            ),
            // the chosen game, under its case
            (
                Info,
                Node { position_type: PositionType::Absolute, width: percent(100), padding: UiRect::horizontal(px(16)), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(6), ..default() },
                children![
                    (text(Line::Title, 500, 38.0, INK, -0.005), TextLayout::justify(Justify::Center)),
                    (text(Line::Meta, 400, 13.0, INK_DIM, 0.04), TextLayout::justify(Justify::Center)),
                ]
            ),
            // the keys
            (
                Node { position_type: PositionType::Absolute, bottom: px(24), width: percent(100), justify_content: JustifyContent::Center, ..default() },
                children![(
                    Hints,
                    Node {
                        max_width: percent(96),
                        padding: UiRect::axes(px(8), px(6)),
                        column_gap: px(6),
                        row_gap: px(4),
                        flex_wrap: FlexWrap::Wrap,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(10.0 / 255.0, 12.0 / 255.0, 22.0 / 255.0, 0.78)),
                    BorderColor::all(LINE),
                )]
            ),
            // Inspect's keys, in their place
            (
                Node { position_type: PositionType::Absolute, bottom: px(32), width: percent(100), justify_content: JustifyContent::Center, ..default() },
                children![(crate::shelf::InspectKeys, Text::default(), TextFont::from_font_size(FontSize::Px(12.0)).with_font(face), TextColor(INK), Visibility::Hidden)]
            ),
        ],
    ));
    commands.insert_resource(mona);
}

pub fn size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        String::new()
    }
}

/// A Steam game's playtime as the Electron app writes it: hours to one decimal, as JS toFixed(1) rounds them.
fn played(minutes: u64) -> String {
    if minutes == 0 {
        return "Not played yet".into();
    }
    // Rust rounds the double as JS does, except on an exact half (a quarter hour, 1.25), which JS rounds up
    let h = if minutes.is_multiple_of(15) && !(minutes / 15).is_multiple_of(2) { format!("{}.{}", (minutes + 3) / 60, (minutes + 3) / 6 % 10) } else { format!("{:.1}", minutes as f64 / 60.0) };
    format!("{h} h played")
}

/// Where a PC game comes from: its store, or the folder it was found in.
pub fn source_name(pc: &rustyshelf_core::pcgames::PcGame) -> &'static str {
    if pc.port && pc.source == "folder" {
        "Ports & recomps folder"
    } else {
        store_name(pc.source)
    }
}

/// Where a PC game comes from, as the Electron app names it (renderer/app.js PC_SRC, keep in sync).
pub fn store_name(source: &str) -> &'static str {
    match source {
        "epic" => "Epic Games",
        "folder" => "PC games folder",
        "quiver" => "Quiver",
        "custom" => "Custom",
        "gog" => "GOG",
        "ubisoft" => "Ubisoft",
        "ea" => "EA",
        "xbox" => "Xbox",
        "hoyoplay" => "HoYoPlay",
        _ => "Steam",
    }
}

/// What's written under a game's title, a piece at a time (app.js metaFor). `shelf`: its shelf's name; `ok`: the
/// key that chooses, as the bar of keys names it.
fn meta(g: &Game, shelf: &str, ok: &str) -> Vec<String> {
    let parts = match &g.source {
        Source::Tile(Tile::Emulator) => vec!["Emulator".to_string(), "Open settings".into()],
        Source::Tile(Tile::AddCustom) => vec!["Add games".to_string(), format!("{ok} adds several at once, a whole folder, or your Playnite library")],
        Source::Tile(Tile::AddGames) => vec![format!("No {shelf} games yet"), format!("{ok} opens {shelf} settings to add a games folder")],
        Source::Tile(Tile::Surprise) => vec!["Shuffle".to_string(), "Lands on a random game".into()],
        Source::Steam { minutes, .. } => vec!["Steam".to_string(), played(*minutes), size(g.size_bytes)],
        // TODO: the playtime RustyShelf keeps for the games it starts itself (the Electron app's history)
        Source::Pc(pc) => vec![source_name(pc).to_string(), if g.not_installed { "Not installed".into() } else { played(0) }, size(g.size_bytes)],
        Source::App { .. } => vec!["Streaming app".to_string()],
        Source::Rom => vec![
            g.region.clone(),
            g.ext.to_uppercase(),
            size(g.size_bytes),
            if g.discs.len() > 1 { format!("{} discs", g.discs.len()) } else { String::new() },
            if !g.art.manual.is_empty() || g.art.manual_pdf.is_some() { "Manual".into() } else { String::new() },
        ],
    };
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// The keys the bar shows: for the keyboard, and for a pad.
const KEYS: [(&[&str], &str); 11] = [
    (&["Z", "C"], "Letter"),
    (&["↑", "↓"], "System"),
    (&["F"], "Flip"),
    (&["H"], "♥ Favorite"),
    (&["I"], "Details"),
    (&["Enter"], "Play"),
    (&["R"], "Shuffle"),
    (&["/"], "Find"),
    (&["G"], "Sort"),
    (&["S"], "Settings"),
    (&["Esc"], "Exit"),
];
/// (the pad's buttons by `pads::cap`'s keys)
const PAD_KEYS: [(&[&str], &str); 8] = [
    (&["L1", "R1"], "Letter"),
    (&["L2", "R2"], "System"),
    (&["tri"], "Flip · hold ♥"),
    (&["⧉"], "Details"),
    (&["x"], "Play"),
    (&["L3"], "Shuffle · hold Find"),
    (&["☰"], "Sort · Settings"),
    (&["o"], "Back"),
];

/// The date and the time as the Electron app's clock writes them in English: "Sat, Nov 14" and "5:16 PM" (or
/// "17:16").
pub fn clock(now: chrono::DateTime<chrono::Local>, day_too: bool, twenty_four: bool) -> (String, String) {
    let date = if day_too { now.format("%a, %b %-d").to_string() } else { String::new() };
    (date, if twenty_four { now.format("%H:%M").to_string() } else { now.format("%-I:%M %p").to_string() })
}

/// What's written: the console's count, the clock, the chosen game and any message.
pub fn update(
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    stage: Res<Stage>,
    playing: Res<Playing>,
    toast: Res<Toast>,
    time: Res<Time>,
    settings: Res<crate::settings::Settings>,
    prompts: Res<Prompts>,
    surprise: Res<crate::play::Surprise>,
    cheevos: Res<crate::cheevos::Cheevos>,
    mona: Res<MonaSans>,
    face: Res<Face>,
    mut lines: Query<(&mut Text, &mut TextFont, &Line)>,
    mut info: Query<(&mut Node, &mut Visibility), (With<Info>, Without<NoticePill>)>,
    mut pill: Query<&mut Node, (With<NoticePill>, Without<Info>)>,
) {
    let [mut tag, mut count, mut counter, mut title, mut note] = [const { String::new() }; 5];
    let mut under: Vec<String> = Vec::new();
    match &lib.0 {
        LibraryState::Loading(_) => note = "Reading your games…".into(),
        LibraryState::Failed(e) => note = format!("Your settings couldn't be read ({e})."),
        LibraryState::Ready(l) if l.shelves.is_empty() => {
            title = "Nothing on the shelf yet".into();
            under = vec![format!("Connect your emulators and game folders in Settings ({})", if prompts.pad { settings.cap("☰") } else { "S" })];
        }
        LibraryState::Ready(l) => {
            let shelf = &l.shelves[cursor.shelf.min(l.shelves.len() - 1)];
            // the shelf's own cases are counted among its cases, not among its games, and so are games added to it
            let mixed = matches!(shelf.kind, rustyshelf_core::library::ShelfKind::Mixed);
            let own = |i: usize| mixed || shelf.from.get(i).is_none_or(|f| *f == shelf.id);
            let (cases, n) = (shelf.games.len(), shelf.games.iter().enumerate().filter(|(i, g)| !matches!(g.source, Source::Tile(_)) && own(*i)).count());
            let pc = matches!(shelf.kind, rustyshelf_core::library::ShelfKind::Pc);
            let what = if shelf.id == "media" { if n == 1 { "app" } else { "apps" } } else if pc { "installed" } else if n == 1 { "game" } else { "games" };
            count = format!("{n} {what}").to_uppercase();
            tag = looks::wordmark(&shelf.id).map_or(String::new(), |w| w.1.to_uppercase());
            match shelf.games.get(cursor.game) {
                Some(g) => {
                    let ok = if prompts.pad { pads::button_word(true, settings.glyphs(), settings.swap()) } else { "Enter" };
                    // the Surprise me case shows the game it landed on
                    let picked = surprise.on(&shelf.id).filter(|_| g.source == Source::Tile(Tile::Surprise)).and_then(|i| shelf.games.get(i));
                    title = picked.map_or(&g.title, |p| &p.title).clone();
                    counter = format!("{} / {cases}", cursor.game + 1);
                    under = match picked {
                        Some(p) => [format!("Surprise pick · {}", shelf.name)].into_iter().chain(meta(p, &shelf.name, ok).into_iter().take(2)).chain([format!("{} rolls again", if prompts.pad { pads::pad_word("L3", settings.glyphs()) } else { "R" })]).collect(),
                        None => meta(g, &shelf.name, ok),
                    };
                    // on Home, All or a collection: the shelf it's from first
                    let own = shelf.from.get(cursor.game).and_then(|id| l.shelves.iter().find(|s| s.id == *id));
                    if let Some(own) = own {
                        under.insert(0, own.name.clone());
                    }
                    if let Some(mark) = cheevos.mark(&own.unwrap_or(shelf).id, &picked.unwrap_or(g).title) {
                        under.push(mark);
                    }
                }
                None => {
                    title = "No games match this filter".into();
                    under = vec![format!("Press {} to change the filter", if prompts.pad { pads::pad_word("menu", settings.glyphs()) } else { "G" })];
                }
            }
        }
    }
    if let Some(p) = &playing.title {
        note = if playing.is_starting() { format!("Starting {p}…") } else { format!("Playing {p}. The shelf comes back when it closes.") };
    }
    if time.elapsed_secs() < toast.until {
        note = toast.text.clone();
    }
    let (date, hour) = match settings.clock() {
        Some((day_too, twenty_four)) => clock(chrono::Local::now(), day_too, twenty_four),
        None => Default::default(),
    };
    let under = under.join("  ·  ");
    for (mut text, mut font, line) in &mut lines {
        let want = match line {
            Line::Tag => &tag,
            Line::Count => &count,
            Line::Date => &date,
            Line::Time => &hour,
            Line::Counter => &counter,
            Line::Title => &title,
            Line::Meta => &under,
            Line::Notice => &note,
        };
        if text.0 != *want {
            text.0 = want.clone();
            // Mona Sans has the Latin letters only: a line with anything else is set in the font with more
            let (weight, size) = match line {
                Line::Title => (500, 38.0),
                Line::Meta => (400, 13.0),
                Line::Notice => (600, 13.0),
                _ => (0, 0.0),
            };
            if weight > 0 {
                let latin = want.chars().all(|c| (c as u32) < 0x250 || matches!(c, '·' | '…' | '–' | '—' | '‘' | '’' | '“' | '”' | '★'));
                let shown = font.font_size;
                *font = if !latin { TextFont::default().with_font(face.0.clone()) } else if matches!(line, Line::Title) { mona.wide(weight, size) } else { mona.font(weight, size) };
                font.font_size = if matches!(line, Line::Notice) { FontSize::Px(size) } else { shown };
            }
        }
        // the title: 3% of the window's width, 22 to 44 pixels; the line under it 1%, 11 to 14
        let size = match line {
            Line::Title => Some((stage.w * 0.03).clamp(22.0, 44.0)),
            Line::Meta => Some((stage.w * 0.01).clamp(11.0, 14.0)),
            _ => None,
        };
        if let Some(size) = size.map(FontSize::Px).filter(|s| *s != font.font_size) {
            font.font_size = size;
        }
    }
    if let Ok(mut pill) = pill.single_mut() {
        let want = if note.is_empty() { Display::None } else { Display::Flex };
        if pill.display != want {
            pill.display = want;
        }
    }
    if let Ok((mut node, mut seen)) = info.single_mut() {
        let top = px(stage.h * 0.42 + stage.bh / 2.0 + stage.h * 0.08);
        if node.top != top {
            node.top = top;
        }
        seen.set_if_neq(if settings.open { Visibility::Hidden } else { Visibility::Inherited });
    }
}

/// The console's name lettered in its colours, and the consoles' tabs with the chosen one on white.
pub fn top(
    mut commands: Commands,
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    stage: Res<Stage>,
    mona: Res<MonaSans>,
    mut images: ResMut<Assets<Image>>,
    mut word: Query<(&mut ImageNode, &mut Node), With<Word>>,
    lit: Res<crate::shelf::Lit>,
    mut tabs: Query<(Entity, &mut BackgroundColor, &mut BorderColor), (With<Tabs>, Without<Tab>)>,
    mut tab: Query<(&Tab, &mut BackgroundColor, &Children)>,
    mut ink: Query<&mut TextColor>,
    mut shown: Local<(String, String)>,
) {
    let (names, id, name): (Vec<String>, String, String) = match &lib.0 {
        LibraryState::Ready(l) if !l.shelves.is_empty() => {
            let shelf = &l.shelves[cursor.shelf.min(l.shelves.len() - 1)];
            (l.shelves.iter().map(|s| s.name.clone()).collect(), shelf.id.clone(), shelf.name.clone())
        }
        _ => Default::default(),
    };
    let wanted = format!("{id}|{name}|{}", stage.w);
    if shown.0 != wanted {
        shown.0 = wanted;
        if let Ok((mut image, mut node)) = word.single_mut() {
            if id.is_empty() {
                image.color = Color::NONE;
            } else {
                let p = rustyshelf_core::drawn::wordmark(looks::wordmark(&id).map_or(name.as_str(), |w| w.0), looks::lettering(&id), stage.w, 2.0);
                // the glow's room round the letters hangs outside the bar's own lines
                (node.width, node.height, node.margin) = (px(p.width as f32 / 2.0), px(p.height as f32 / 2.0), UiRect::all(px(-rustyshelf_core::drawn::WORD_ROOM)));
                image.image = images.add(to_image(p.width, p.height, p.rgba));
                image.color = Color::WHITE;
            }
        }
    }
    let all = names.join("\u{1}");
    if shown.1 != all {
        shown.1 = all;
        if let Ok((bar, ..)) = tabs.single() {
            commands.entity(bar).despawn_related::<Children>().with_children(|b| {
                for (i, name) in names.iter().enumerate() {
                    b.spawn((Tab(i), Node { padding: UiRect::axes(px(14), px(6)), border_radius: BorderRadius::MAX, flex_shrink: 0.0, ..default() }, BackgroundColor(Color::NONE), children![(Text::new(name.clone()), mona.font(500, 12.0), TextColor(INK_DIM))]));
                }
            });
        }
    }
    // the bar takes the colour of the glow behind the shelf (setup.css: color-mix of the glow at .62 and the glass)
    if let Ok((_, mut bg, mut border)) = tabs.single_mut() {
        let glow = Color::hsl(lit.glow.rem_euclid(360.0), 0.95, 0.62).to_srgba();
        let mix = |part: f32, with: [f32; 4]| {
            let (a, b) = (part * 0.62, (1.0 - part) * with[3]);
            Color::srgba((a * glow.red + b * with[0]) / (a + b), (a * glow.green + b * with[1]) / (a + b), (a * glow.blue + b * with[2]) / (a + b), a + b)
        };
        let (want, edge) = (mix(0.3, [8.0 / 255.0, 10.0 / 255.0, 20.0 / 255.0, 0.4]), mix(0.45, [1.0, 1.0, 1.0, 0.1]));
        if bg.0 != want {
            bg.0 = want;
            *border = BorderColor::all(edge);
        }
    }
    for (t, mut bg, children) in &mut tab {
        let on = t.0 == cursor.shelf;
        let want = if on { PAPER } else { Color::NONE };
        if bg.0 != want {
            bg.0 = want;
        }
        for child in children.iter() {
            if let Ok(mut ink) = ink.get_mut(child) {
                let want = if on { ON_PAPER } else { INK_DIM };
                if ink.0 != want {
                    ink.0 = want;
                }
            }
        }
    }
}

/// The keys' bar: it follows what was pressed last, the keyboard's keys or the pad's.
/// A controller's id as a browser writes one, for `pads::pad_info`: its name, then its maker and model. A pad
/// Windows shows as an Xbox 360 one (what Steam Input and DS4Windows make of any pad), or without a maker, is
/// an XInput pad there.
fn pad_id(pad: &Gamepad, name: Option<&Name>) -> String {
    let name = name.map_or("", Name::as_str);
    match (pad.vendor_id(), pad.product_id()) {
        (Some(v), Some(p)) if (v, p) != (0x045e, 0x028e) => format!("{name} (Vendor: {v:04x} Product: {p:04x})"),
        _ if cfg!(windows) => format!("{name} (XInput)"),
        _ => name.to_string(),
    }
}

pub fn hints(
    mut commands: Commands,
    lib: Res<LibraryRes>,
    mut settings: ResMut<crate::settings::Settings>,
    mona: Res<MonaSans>,
    face: Res<Face>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad, Option<&Name>)>,
    mut prompts: ResMut<Prompts>,
    mut bar: Query<(Entity, &mut Visibility), With<Hints>>,
    mut shown: Local<Option<(bool, pads::Glyphs, bool)>>,
    inspect: Res<crate::shelf::Inspect>,
) {
    // the phone's remote isn't a controller of its own: the prompts stay those of the pad there is
    let own = |name: Option<&Name>| name.is_none_or(|n| n.as_str() != crate::phone::REMOTE);
    let used = gamepads.iter().find(|(_, p, _)| p.get_just_pressed().next().is_some());
    if keys.get_just_pressed().next().is_some() {
        prompts.pad = false;
    } else if let Some((pad, _, name)) = used {
        prompts.pad = true;
        if own(name) {
            prompts.in_use = Some(pad);
        }
    }
    // the real controllers plugged in, asked for again whenever one comes or goes
    let count = gamepads.iter().count();
    if count != prompts.pads && prompts.asking.is_none() {
        prompts.pads = count;
        let (tx, rx) = channel();
        prompts.asking = Some(Mutex::new(rx));
        std::thread::spawn(move || {
            let _ = tx.send(pads::list());
        });
    }
    if let Some(real) = prompts.asking.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        (prompts.asking, prompts.real) = (None, real);
    }
    // (before any press, the first controller there is)
    if let Some((_, pad, name)) = prompts.in_use.and_then(|e| gamepads.get(e).ok()).or_else(|| gamepads.iter().find(|(_, _, n)| own(*n))) {
        let style = pads::style_of(&pad_id(pad, name), &prompts.real);
        if settings.pad_auto != style {
            settings.pad_auto = style;
        }
    }
    let Ok((bar, mut seen)) = bar.single_mut() else { return };
    let any = matches!(&lib.0, LibraryState::Ready(l) if !l.shelves.is_empty());
    seen.set_if_neq(if settings.open || inspect.on || !any { Visibility::Hidden } else { Visibility::Inherited });
    let now = (prompts.pad, settings.glyphs(), settings.swap());
    if *shown == Some(now) {
        return;
    }
    *shown = Some(now);
    let list: Vec<(Vec<&str>, &str)> = if prompts.pad { PAD_KEYS.iter().map(|(caps, label)| (caps.iter().map(|c| settings.cap(c)).collect(), *label)).collect() } else { KEYS.iter().map(|(caps, label)| (caps.to_vec(), *label)).collect() };
    // the arrows and the heart aren't in Mona Sans
    let font = |text: &str, weight: u32, size: f32| if text.is_ascii() { mona.font(weight, size) } else { TextFont::from_font_size(FontSize::Px(size)).with_font(face.0.clone()) };
    commands.entity(bar).despawn_related::<Children>().with_children(|b| {
        for (caps, label) in &list {
            b.spawn(Node { padding: UiRect::axes(px(10), px(5)), column_gap: px(7), align_items: AlignItems::Center, ..default() }).with_children(|hint| {
                hint.spawn(Node { column_gap: px(3), ..default() }).with_children(|row| {
                    for cap in caps.iter() {
                        row.spawn((
                            Node { min_width: px(22), height: px(18), padding: UiRect::horizontal(px(5)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(5)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
                            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.55)),
                            children![(Text::new(*cap), font(cap, 600, 10.0), TextColor(INK))],
                        ));
                    }
                });
                hint.spawn((Text::new(*label), font(label, 500, 12.0), TextColor(INK)));
            });
        }
    });
}

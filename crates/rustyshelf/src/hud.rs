//! What's written over the shelf: the consoles, the chosen game, the buttons, and anything that went wrong.

use bevy::prelude::*;
use bevy::text::FontSize;
use rustyshelf_core::paths;
use rustyshelf_core::scanner::{Game, Source};

use crate::loading::{LibraryRes, LibraryState};
use crate::play::Playing;
use crate::shelf::Cursor;

const STEEL: Color = Color::srgb(0.87, 0.89, 0.92);
const RUST: Color = Color::srgb(0.83, 0.32, 0.17);
const DIM: Color = Color::srgb(0.55, 0.58, 0.62);

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

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    Tabs,
    Title,
    Details,
    Notice,
}

/// A message shown for a few seconds (a game that didn't start).
#[derive(Resource, Default)]
pub struct Toast {
    pub text: String,
    pub until: f32,
}

// the default font has no ◀ ▶ or ·
const FONT: &[u8] = include_bytes!("../../img-tools/assets/DejaVuSans-Bold.ttf");

pub fn setup(mut commands: Commands, mut fonts: ResMut<Assets<Font>>) {
    let face = fonts.add(Font::from_bytes(FONT.to_vec()));
    let font = |vmin: f32| TextFont::from_font_size(FontSize::VMin(vmin)).with_font(face.clone());
    commands.spawn((
        Node { width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, padding: UiRect::all(vmin(3.0)), ..default() },
        children![
            (
                Node { flex_direction: FlexDirection::Row, column_gap: vmin(3.0), align_items: AlignItems::Baseline, ..default() },
                children![
                    (Text::new("RUSTY"), font(3.2), TextColor(RUST), children![(TextSpan::new("SHELF"), font(3.2), TextColor(STEEL))]),
                    (Line::Tabs, Text::default(), font(2.4), TextColor(DIM)),
                ]
            ),
            (Node { flex_grow: 1.0, ..default() }),
            (Line::Notice, Text::default(), font(2.6), TextColor(STEEL)),
            (Line::Title, Text::default(), font(4.6), TextColor(STEEL)),
            (Line::Details, Text::default(), font(2.2), TextColor(DIM)),
            (
                Node { margin: UiRect::top(vmin(2.0)), ..default() },
                Text::new("◀ ▶  Game      Q / E · LB / RB  Console      Enter · A  Play      Esc  Quit"),
                font(1.9),
                TextColor(DIM),
            ),
        ],
    ));
}

fn size(bytes: u64) -> String {
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

fn details(g: &Game) -> String {
    let pc = match &g.source {
        Source::Steam { minutes, .. } => Some(("Steam", played(*minutes))),
        // TODO: the playtime RustyShelf keeps for the games it starts itself (the Electron app's history)
        Source::Pc(pc) => Some((store_name(pc.source), if g.not_installed { "Not installed".into() } else { played(0) })),
        Source::Rom => None,
    };
    if let Some((store, played)) = pc {
        let parts = [store.to_string(), played, size(g.size_bytes)];
        return parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join("  ·  ");
    }
    let mut parts = vec![g.region.clone(), g.ext.to_uppercase(), size(g.size_bytes)];
    if g.discs.len() > 1 {
        parts.push(format!("{} discs", g.discs.len()));
    }
    if !g.art.manual.is_empty() || g.art.manual_pdf.is_some() {
        parts.push("Manual".into());
    }
    parts.retain(|p| !p.is_empty());
    parts.join("  ·  ")
}

pub fn update(
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    playing: Res<Playing>,
    toast: Res<Toast>,
    time: Res<Time>,
    mut lines: Query<(&mut Text, &Line)>,
) {
    let (mut tabs, mut title, mut det, mut note) = (String::new(), String::new(), String::new(), String::new());
    match &lib.0 {
        LibraryState::Loading(_) => note = "Reading your games…".into(),
        LibraryState::Failed(e) => note = format!("Your settings couldn't be read ({e})."),
        LibraryState::Ready(l) if l.shelves.is_empty() => {
            note = format!(
                "No games found. Add your games folders in the Electron app (Settings → the console), then start this again.\nSettings: {}",
                paths::data_dir(&l.app_root).join("config.json").display()
            )
        }
        LibraryState::Ready(l) => {
            let shelf = &l.shelves[cursor.shelf.min(l.shelves.len() - 1)];
            tabs = l
                .shelves
                .iter()
                .enumerate()
                .map(|(i, s)| if i == cursor.shelf { format!("[ {} ]", s.name) } else { s.name.clone() })
                .collect::<Vec<_>>()
                .join("   ");
            if let Some(g) = shelf.games.get(cursor.game) {
                title = g.title.clone();
                det = format!("{}  ·  {} of {}", details(g), cursor.game + 1, shelf.games.len());
            }
        }
    }
    if let Some(p) = &playing.title {
        note = if playing.is_starting() { format!("Starting {p}…") } else { format!("Playing {p}. The shelf comes back when it closes.") };
    }
    if time.elapsed_secs() < toast.until {
        note = toast.text.clone();
    }
    for (mut text, line) in &mut lines {
        let want = match line {
            Line::Tabs => &tabs,
            Line::Title => &title,
            Line::Details => &det,
            Line::Notice => &note,
        };
        if text.0 != *want {
            text.0 = want.clone();
        }
    }
}

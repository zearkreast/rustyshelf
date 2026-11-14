// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the Home menu of
// Boxshelf's main/homemenu.js, renderer/home.js and renderer/home.css, as RustyShelf has them.
//! The Home menu on screen. On the shelf (Home or P, or Guide on the pad): the Home shelf, Settings, Power. For
//! a running game (Guide, or View held on its own): the game is paused with its emulator's own hotkey and
//! RustyShelf comes to the front with the menu; what's chosen goes back to the game as hotkeys.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use bevy::text::LetterSpacing;
use bevy::ui::{UiTransform, Val2};
use rustyshelf_core::homemenu::{self, Info, Keys, Menu, Out};
use rustyshelf_core::systems::systems;

use crate::hud::{Lexend, Toast};
use crate::loading::{to_image, LibraryRes, LibraryState};
use crate::padwatch;
use crate::play::Playing;
use crate::settings::Settings;
use crate::shelf::{bezier, Covers, Cursor, Stage};
use crate::sound::Sounds;

const ACCENT: Color = Color::srgb(0.616, 0.549, 1.0);
const INK: Color = Color::srgb(0.953, 0.961, 0.984);
// the page's 60% ink over the panel
const DIM: Color = Color::srgb(0.588, 0.596, 0.627);
const DARK: Color = Color::srgb(0.055, 0.063, 0.094);
const LINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.085);
const RUST: Color = Color::srgb(0.831, 0.322, 0.173);
/// seconds the panel takes to slide in or out
const SLIDE: f32 = 0.34;
/// a power action chosen over a game still runs if the game has closed within this many seconds
const POWER_WAIT: f32 = 20.0;

/// The game the menu is for.
struct Game {
    pid: u32,
    keys: Keys,
    paused: bool,
    /// when it started, by the clock (ms)
    started: f64,
    title: String,
    system: String,
    /// the PC was put to sleep from the menu: on waking the menu comes back over the paused game
    slept: bool,
}

/// What was done to the game off the frame.
enum Done {
    /// the pause key went through (or didn't) as the menu opened
    Paused(bool),
    /// an action's keys went through (or didn't)
    Sent { action: String, ok: bool, was_paused: bool },
}

#[derive(Resource, Default)]
pub struct Home {
    menu: Option<Menu>,
    /// when it opened and when it began to leave (seconds); `shown` until it has slid away
    since: f32,
    left: Option<f32>,
    shown: bool,
    /// to be set out again
    fresh: bool,
    game: Option<Game>,
    done: Option<Mutex<Receiver<Done>>>,
    /// Restart or Shut down chosen over a game: done once the game has closed. With when it was chosen.
    then: Option<(&'static str, f32)>,
    /// a power action and when it's due
    power_at: Option<(&'static str, f32)>,
    /// the clock at the last frame (ms): a jump is the PC waking
    wall: f64,
    reopen_at: Option<f32>,
    cover: Option<Handle<Image>>,
    icons: HashMap<&'static str, Handle<Image>>,
    /// the minute the panel's clock shows
    minute: u32,
}

#[derive(Component)]
pub struct Whole;
#[derive(Component)]
pub struct Dim;
#[derive(Component)]
pub struct Panel;
#[derive(Component)]
pub struct HoldBar;

// (renderer/home.js ICON, keep in sync) a 24 by 24 drawing each; `true`: filled, not stroked
const ICONS: [(&str, &str, bool); 12] = [
    ("resume", "M8 5.5v13l11-6.5z", true),
    ("save", "M12 4v10m0 0-4-4m4 4 4-4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3", false),
    ("load", "M12 15V5m0 0-4 4m4-4 4 4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3", false),
    ("shot", "M4 8.5A2.5 2.5 0 0 1 6.5 6h1.7l1.3-2h5l1.3 2h1.7A2.5 2.5 0 0 1 20 8.5v8a2.5 2.5 0 0 1-2.5 2.5h-11A2.5 2.5 0 0 1 4 16.5zM12 9.1a3.4 3.4 0 1 0 0 6.8 3.4 3.4 0 0 0 0-6.8z", false),
    ("quit", "M15 4h3a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2h-3M10 8l-4 4 4 4M6 12h10", false),
    ("power", "M12 3v8M6.3 6.8a8 8 0 1 0 11.4 0", false),
    ("shelfhome", "M4 11l8-7 8 7v8a1 1 0 0 1-1 1h-5v-6h-4v6H5a1 1 0 0 1-1-1z", false),
    (
        "settings",
        "M12 9.2a2.8 2.8 0 1 0 0 5.6 2.8 2.8 0 0 0 0-5.6zM19.4 13.5l1.6 1.2-1.8 3.1-1.9-.6a7.4 7.4 0 0 1-1.6.9L15.3 20h-3.6l-.4-1.9a7.4 7.4 0 0 1-1.6-.9l-1.9.6-1.8-3.1 1.6-1.2a7 7 0 0 1 0-1.9L6 10.4l1.8-3.1 1.9.6c.5-.4 1-.7 1.6-.9L11.7 5h3.6l.4 1.9c.6.2 1.1.5 1.6.9l1.9-.6 1.8 3.1-1.6 1.2a7 7 0 0 1 0 1.9z",
        false,
    ),
    ("sleep", "M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z", false),
    ("restart", "M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7", false),
    ("quitapp", "M6 6l12 12M18 6L6 18", false),
    ("back", "M15 5l-7 7 7 7", false),
];

fn icon_of(id: &str) -> &str {
    if id == "shutdown" { "power" } else { id }
}

pub fn setup(mut commands: Commands, mut home: ResMut<Home>, mut images: ResMut<Assets<Image>>) {
    for (id, path, filled) in ICONS {
        let paint = if filled { r##"fill="#fff""## } else { r##"fill="none" stroke="#fff" stroke-width="2" stroke-linecap="round" stroke-linejoin="round""## };
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="{path}" {paint}/></svg>"#);
        if let Some(p) = rustyshelf_core::cssbg::svg(&svg, 48, 48) {
            home.icons.insert(id, images.add(to_image(p.width, p.height, p.rgba)));
        }
    }
    // the game behind is dimmed: darker toward the left, where the panel is (the page's black of .25 to .6, as
    // deep as a browser's blending makes it)
    let dim = rustyshelf_core::cssbg::paint("radial-gradient(ellipse at 70% 50%,rgba(0,0,0,.47),rgba(0,0,0,.87))", 160, 90, 1.0);
    commands.spawn((
        Whole,
        Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() },
        GlobalZIndex(45),
        Visibility::Hidden,
        children![
            (Dim, Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, stretched(images.add(to_image(dim.width, dim.height, dim.rgba))).with_color(Color::NONE)),
            (Panel, Node { position_type: PositionType::Absolute, ..default() }, UiTransform::IDENTITY),
        ],
    ));
}

/// A picture drawn over the whole of its node, whatever its own shape.
fn stretched(image: Handle<Image>) -> ImageNode {
    ImageNode { image, image_mode: bevy::ui::widget::NodeImageMode::Stretch, ..default() }
}

fn wall_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// Opens and closes the menu, takes the keys and the pads while it's open, and does what's chosen.
#[allow(clippy::too_many_arguments)]
pub fn run(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    lib: Res<LibraryRes>,
    covers: Res<Covers>,
    mut cursor: ResMut<Cursor>,
    mut playing: ResMut<Playing>,
    mut settings: ResMut<Settings>,
    mut sounds: ResMut<Sounds>,
    mut toast: ResMut<Toast>,
    mut exit: MessageWriter<AppExit>,
    mut home: ResMut<Home>,
    mut stick: Local<(i32, i32)>,
) {
    let home = &mut *home;
    let (now, wall) = (time.elapsed_secs(), wall_ms());
    let (enabled, center, swap_ab, glyphs) = settings.home_menu();
    let LibraryState::Ready(l) = &lib.0 else { return };

    let done: Vec<Done> = home.done.as_ref().map_or(Vec::new(), |rx| rx.lock().unwrap().try_iter().collect());
    for d in done {
        let Some(game) = home.game.as_mut() else { break };
        match d {
            Done::Paused(paused) => {
                game.paused = paused;
                if let Some(menu) = home.menu.as_mut().filter(|m| m.info.in_game) {
                    (menu.info.paused, home.fresh) = (paused, true);
                }
            }
            Done::Sent { action, ok, was_paused } => {
                if ok && was_paused {
                    game.paused = false;
                }
                if let Some(text) = Some(()).filter(|()| !ok).and_then(|()| homemenu::toast_for(&action, &game.keys, was_paused, false)) {
                    (toast.text, toast.until) = (text, now + 5.0);
                }
            }
        }
    }

    match (&playing.title, playing.pid) {
        (None, _) => {
            if home.game.take().is_some() {
                if home.menu.as_ref().is_some_and(|m| m.info.in_game) {
                    (home.menu, home.left) = (None, Some(now));
                }
                home.power_at = home.then.take().filter(|t| now - t.1 < POWER_WAIT).map(|t| (t.0, now + 0.8));
            }
        }
        (Some(title), Some(pid)) => match home.game.as_mut() {
            // a launcher handed over to the game itself
            Some(game) => game.pid = pid,
            None => {
                let (emulator, app) = playing.running_in();
                let (keys, system) = match emulator {
                    Some((console, exe)) => {
                        let own = l.config.raw().get("systems").and_then(|s| s.get(console)).and_then(|s| s.get("hotkeys")).and_then(serde_json::Value::as_object);
                        (homemenu::hotkeys_for(console, exe, own), systems().console(console).map_or(console.to_string(), |c| c.name.clone()))
                    }
                    None => (Keys::default(), if app { "Media" } else { "PC" }.to_string()),
                };
                home.game = Some(Game { pid, keys, paused: false, started: wall, title: title.clone(), system, slept: false });
            }
        },
        (Some(_), None) => {}
    }
    if let Some((kind, _)) = home.power_at.filter(|p| now >= p.1) {
        home.power_at = None;
        padwatch::power(kind);
    }
    // back from a sleep chosen here: the game was left paused for the menu's Resume
    if home.game.as_ref().is_some_and(|g| g.slept) && wall - home.wall > 15_000.0 {
        home.reopen_at = Some(now + 1.5);
        if let Some(g) = home.game.as_mut() {
            g.slept = false;
        }
    }
    home.wall = wall;

    let was_open = home.menu.is_some();
    let mut outs: Vec<Out> = Vec::new();
    let ms = f64::from(now) * 1000.0;
    // (the Home key too, where RustyShelf still has the keyboard while a game runs)
    let asked = std::mem::take(&mut playing.home_pressed) | home.reopen_at.take_if(|at| now >= *at).is_some() | (!was_open && playing.title.is_some() && keys.just_pressed(KeyCode::Home));
    match &home.game {
        Some(game) if asked && was_open => outs.extend(home.menu.as_mut().and_then(|m| m.press("home", true, ms))),
        Some(game) if asked && enabled && !playing.is_starting() => {
            let info = Info {
                in_game: true,
                title: game.title.clone(),
                system: game.system.clone(),
                actions: homemenu::actions_for(&game.keys).iter().map(|a| a.to_string()).collect(),
                power: ["sleep", "restart", "shutdown"].map(String::from).to_vec(),
                can_save: game.keys.save.is_some(),
                slot: game.keys.slot,
                paused: game.paused,
                center,
                glyphs,
                swap_ab,
                ..Info::default()
            };
            home.cover = l.shelves.get(cursor.shelf).and_then(|shelf| {
                let own = shelf.from.get(cursor.game).map_or(shelf.id.as_str(), String::as_str);
                covers.of(shelf.games.get(cursor.game)?, own)
            });
            (home.menu, home.since, home.left, home.shown, home.fresh) = (Some(Menu::open(info)), now, None, true, true);
            sounds.play("panel", 0);
            // paused while it's still in front, then RustyShelf comes forward with the menu; a game already
            // paused (left so by Sleep) isn't toggled back on
            let (pid, pause, paused) = (game.pid, game.keys.pause.clone(), game.paused);
            let (tx, rx) = channel();
            home.done = Some(Mutex::new(rx));
            std::thread::spawn(move || {
                let paused = match pause {
                    Some(key) if !paused => padwatch::send_keys(pid, &[key]),
                    _ => paused,
                };
                let _ = tx.send(Done::Paused(paused));
                padwatch::front_self();
            });
        }
        None if !was_open && enabled && !settings.open && playing.title.is_none() && (keys.just_pressed(KeyCode::Home) || keys.just_pressed(KeyCode::KeyP) || pads.iter().any(|p| p.just_pressed(GamepadButton::Mode))) => {
            let others = l.shelves.iter().filter(|s| s.id != "home");
            let info = Info {
                title: "Home".into(),
                system: "RustyShelf".into(),
                subtitle: format!("{} games · {} shelves", others.clone().map(|s| s.games.len()).sum::<usize>(), others.count()),
                actions: ["shelfhome", "settings", "power"].into_iter().filter(|a| *a != "shelfhome" || l.shelves.iter().any(|s| s.id == "home")).map(String::from).collect(),
                power: ["sleep", "restart", "shutdown", "quitapp"].map(String::from).to_vec(),
                center,
                glyphs,
                swap_ab,
                ..Info::default()
            };
            (home.menu, home.since, home.left, home.shown, home.fresh, home.cover) = (Some(Menu::open(info)), now, None, true, true, None);
            sounds.play("panel", 0);
        }
        _ => {}
    }

    if let Some(menu) = home.menu.as_mut().filter(|_| was_open) {
        let mut press = |button: &str, down: bool| outs.extend(menu.press(button, down, ms));
        for (key, button) in [(KeyCode::ArrowUp, "up"), (KeyCode::ArrowDown, "down"), (KeyCode::ArrowLeft, "left"), (KeyCode::ArrowRight, "right"), (KeyCode::Escape, "cancel"), (KeyCode::Backspace, "cancel")] {
            if keys.just_pressed(key) {
                press(button, true);
            }
        }
        for key in [KeyCode::Enter, KeyCode::Space] {
            if keys.just_pressed(key) {
                press("ok", true);
            }
            if keys.just_released(key) {
                press("ok", false);
            }
        }
        // the press that opened the menu (or the pad's own echo of it) mustn't close it straight away
        let settled = now - home.since >= 0.45;
        if settled && (keys.just_pressed(KeyCode::Home) || keys.just_pressed(KeyCode::KeyP)) {
            press("home", true);
        }
        let (mut x, mut y) = (0, 0);
        for pad in &pads {
            for (b, button) in [(GamepadButton::DPadUp, "up"), (GamepadButton::DPadDown, "down"), (GamepadButton::DPadLeft, "left"), (GamepadButton::DPadRight, "right")] {
                if pad.just_pressed(b) {
                    press(button, true);
                }
            }
            for (b, button) in [(GamepadButton::South, "a"), (GamepadButton::East, "b")] {
                if pad.just_pressed(b) {
                    press(button, true);
                }
                if pad.just_released(b) {
                    press(button, false);
                }
            }
            // over a game Guide comes through the watcher, which hears it whichever window is in front
            if settled && (pad.just_pressed(GamepadButton::Start) || (home.game.is_none() && pad.just_pressed(GamepadButton::Mode))) {
                press("start", true);
            }
            let s = pad.left_stick();
            (x, y) = (if s.x.abs() > 0.6 { s.x.signum() as i32 } else { x }, if s.y.abs() > 0.6 { s.y.signum() as i32 } else { y });
        }
        if (x, y) != *stick {
            match (x, y) {
                (_, 1) => press("up", true),
                (_, -1) => press("down", true),
                (-1, _) => press("left", true),
                (1, _) => press("right", true),
                _ => {}
            }
        }
        *stick = (x, y);
        outs.extend(menu.frame(ms));
        if menu.held() > 0.0 {
            home.fresh = true;
        }
    }

    for out in outs {
        home.fresh = true;
        let (cue, action) = match out {
            Out::Cue(name, dir) => {
                sounds.play(name, dir);
                continue;
            }
            Out::Close(cue, action) => (cue, action),
        };
        sounds.play(cue, 0);
        (home.menu, home.left) = (None, Some(now));
        let Some(game) = home.game.as_mut() else {
            match action.as_str() {
                "shelfhome" => {
                    if let Some(i) = l.shelves.iter().position(|s| s.id == "home") {
                        (cursor.shelf, cursor.land) = (i, true);
                    }
                }
                "settings" => settings.open_root(),
                "sleep" | "restart" | "shutdown" => padwatch::power(match action.as_str() {
                    "sleep" => "sleep",
                    "restart" => "restart",
                    _ => "shutdown",
                }),
                "quitapp" => {
                    exit.write(AppExit::Success);
                }
                _ => {}
            }
            continue;
        };
        match action.as_str() {
            "quit" => playing.close_asked = true,
            // the game is closed properly first; the PC restarts or shuts down once it has gone
            "restart" | "shutdown" => {
                playing.close_asked = true;
                home.then = Some((if action == "restart" { "restart" } else { "shutdown" }, now));
            }
            _ => {
                // Sleep saves a state where the emulator can and leaves the game paused; anything else sends
                // its own key and unpauses
                let sleep = action == "sleep";
                let sent = if sleep { game.keys.save.iter().cloned().collect() } else { homemenu::keys_for(&action, &game.keys, game.paused) };
                if sleep {
                    (game.slept, home.power_at) = (true, Some(("sleep", now + 0.6 + if sent.is_empty() { 0.0 } else { 0.4 })));
                }
                let (pid, was_paused) = (game.pid, game.paused && !sleep);
                let (tx, rx) = channel();
                home.done = Some(Mutex::new(rx));
                std::thread::spawn(move || {
                    let ok = padwatch::send_keys(pid, &sent);
                    let _ = tx.send(Done::Sent { action, ok, was_paused });
                });
            }
        }
    }
    // (the frame it closes in too: the key that closed it isn't the shelf's)
    settings.keys_taken = was_open || home.menu.is_some();
}

/// Sets the menu out when it has changed, and slides it in and out.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn draw(
    mut commands: Commands,
    time: Res<Time>,
    stage: Res<Stage>,
    lexend: Res<Lexend>,
    face: Option<Res<crate::hud::Face>>,
    settings: Res<Settings>,
    mut home: ResMut<Home>,
    mut images: ResMut<Assets<Image>>,
    mut whole: Query<&mut Visibility, With<Whole>>,
    mut dim: Query<&mut ImageNode, With<Dim>>,
    mut panel: Query<(Entity, &mut Node, &mut UiTransform), With<Panel>>,
    mut bars: Query<&mut Node, (With<HoldBar>, Without<Panel>)>,
    mut backs: Local<Option<(Handle<Image>, Handle<Image>)>>,
) {
    let home = &mut *home;
    let Ok(mut seen) = whole.single_mut() else { return };
    if !home.shown {
        seen.set_if_neq(Visibility::Hidden);
        return;
    }
    seen.set_if_neq(Visibility::Visible);
    let now = time.elapsed_secs();
    let (panel_back, strip) = backs
        .get_or_insert_with(|| {
            let back = rustyshelf_core::cssbg::paint("linear-gradient(180deg,rgba(14,16,26,.97),rgba(8,9,15,.97))", 4, 64, 1.0);
            let strip = rustyshelf_core::cssbg::paint("linear-gradient(180deg,transparent,rgba(157,140,255,.8),transparent)", 4, 64, 1.0);
            (images.add(to_image(back.width, back.height, back.rgba)), images.add(to_image(strip.width, strip.height, strip.rgba)))
        })
        .clone();
    let Ok((entity, mut node, mut transform)) = panel.single_mut() else { return };
    let center = home.menu.as_ref().is_some_and(|m| m.info.center);
    let wide = if center { (stage.w * 0.8).min(760.0) } else { (stage.w * 0.36).clamp(380.0, 520.0) };
    // the page is laid out for a window at least this tall: in a lower one the panel is drawn smaller
    let small = (stage.h / 800.0).min(1.0);

    // the clock's minute going round sets it out again too
    let clock = settings.clock().unwrap_or((false, false));
    let (_, hour) = crate::hud::clock(chrono::Local::now(), false, clock.1);
    let minute = chrono::Timelike::minute(&chrono::Local::now());
    if minute != home.minute {
        (home.minute, home.fresh) = (minute, true);
    }
    if let Some(menu) = home.menu.as_ref().filter(|_| std::mem::take(&mut home.fresh)) {
        let (title, line) = menu.heading(home.game.as_ref().map_or(0.0, |g| g.started), wall_ms());
        // Lexend has the Latin letters only: arrows and a pad's own signs are set in the font with more
        let font = |words: &str, weight: u32, size: f32| match &face {
            Some(face) if words.chars().any(|c| c >= '\u{2190}') => TextFont::from_font_size(bevy::text::FontSize::Px(size)).with_font(face.0.clone()),
            _ => lexend.font(weight, size),
        };
        let text = |words: &str, weight: u32, size: f32, color: Color| (Text::new(words), font(words, weight, size), TextColor(color));
        let spaced = |words: &str, weight: u32, size: f32, color: Color, spacing: f32| (Text::new(words), font(words, weight, size), TextColor(color), LetterSpacing::Px(spacing * size));
        *node = if center {
            Node { position_type: PositionType::Absolute, left: px((stage.w - wide) / 2.0), top: percent(50), width: px(wide), max_height: percent(86), flex_direction: FlexDirection::Column, padding: UiRect::new(px(30), px(30), px(34), px(26)), border_radius: BorderRadius::all(px(24)), overflow: Overflow::clip(), ..default() }
        } else {
            Node { position_type: PositionType::Absolute, left: px(0), top: px(0), height: px(stage.h / small), width: px(wide), flex_direction: FlexDirection::Column, padding: UiRect::new(px(30), px(30), px(34), px(26)), ..default() }
        };
        let (rows, in_game, paused, in_power) = (menu.rows(), menu.info.in_game, menu.info.paused, menu.in_power());
        let (held, foot, system, cover) = (menu.held() as f32, menu.foot(), menu.info.system.clone(), home.cover.clone());
        let icons = &home.icons;
        commands.entity(entity).despawn_related::<Children>().with_children(|p| {
            p.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: percent(100), height: percent(100), border_radius: BorderRadius::all(px(if center { 24 } else { 0 })), ..default() }, stretched(panel_back)));
            if !center {
                p.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(0), width: px(3), height: percent(100), ..default() }, stretched(strip)));
                p.spawn((Node { position_type: PositionType::Absolute, right: px(0), top: px(0), width: px(1), height: percent(100), ..default() }, BackgroundColor(LINE)));
            }
            p.spawn(Node { justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Baseline, ..default() }).with_children(|top| {
                top.spawn(Node::default()).with_children(|brand| {
                    brand.spawn(spaced("RUSTY", 400, 12.0, RUST, 0.16));
                    brand.spawn(spaced("SHELF", 400, 12.0, DIM, 0.16));
                });
                top.spawn(spaced(&hour, 500, 15.0, INK, 0.06));
            });
            p.spawn(Node { column_gap: px(18), align_items: AlignItems::FlexEnd, margin: UiRect::new(px(0), px(0), px(26), px(if in_game { 18 } else { 22 })), ..default() }).with_children(|playing| {
                if let Some(cover) = cover {
                    playing.spawn((Node { width: px(112), height: px(112.0 / 0.71), flex_shrink: 0.0, border_radius: BorderRadius::all(px(8)), overflow: Overflow::clip(), ..default() }, ImageNode::new(cover)));
                }
                playing.spawn(Node { flex_direction: FlexDirection::Column, min_width: px(0), ..default() }).with_children(|meta| {
                    meta.spawn(spaced(&system.to_uppercase(), 600, 11.0, ACCENT, 0.18));
                    meta.spawn((Node { margin: UiRect::new(px(0), px(0), px(6), px(8)), ..default() }, text(&title, 600, 26.0, INK)));
                    meta.spawn(text(&line, 400, 13.0, DIM));
                });
            });
            if in_game {
                p.spawn((Node { margin: UiRect::bottom(px(14)), ..default() }, spaced(if paused { "Game paused" } else { " " }, 400, 12.0, DIM, 0.04)));
            }
            if in_power {
                p.spawn((Node { margin: UiRect::new(px(6), px(6), px(2), px(8)), ..default() }, spaced("POWER", 400, 11.0, DIM, 0.18)));
            }
            let grid = center && !in_power;
            let list = if grid {
                Node { display: Display::Grid, grid_template_columns: RepeatedGridTrack::flex(3, 1.0), column_gap: px(10), row_gap: px(10), flex_grow: 1.0, ..default() }
            } else {
                Node { flex_direction: FlexDirection::Column, row_gap: px(6), flex_grow: 1.0, ..default() }
            };
            p.spawn(list).with_children(|items| {
                for row in &rows {
                    if row.sep_before && !grid {
                        items.spawn((Node { height: px(1), margin: UiRect::axes(px(6), px(8)), ..default() }, BackgroundColor(LINE)));
                    }
                    let (ink, back) = match (row.selected, row.danger) {
                        (true, true) => (DARK, Color::srgb(1.0, 0.914, 0.914)),
                        (true, false) => (DARK, Color::srgba(1.0, 1.0, 1.0, 0.94)),
                        _ => (DIM, Color::NONE),
                    };
                    let shape = if grid {
                        Node { flex_direction: FlexDirection::Column, align_items: AlignItems::FlexStart, row_gap: px(20), padding: UiRect::all(px(18)), min_height: px(112), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(14)), overflow: Overflow::clip(), ..default() }
                    } else {
                        Node { align_items: AlignItems::Center, column_gap: px(16), padding: UiRect::axes(px(16), px(14)), border_radius: BorderRadius::all(px(14)), overflow: Overflow::clip(), ..default() }
                    };
                    let moved = if !row.selected { Val2::ZERO } else if grid { Val2::px(0.0, -3.0) } else { Val2::px(4.0, 0.0) };
                    items.spawn((shape, BackgroundColor(back), BorderColor::all(if grid { LINE } else { Color::NONE }), UiTransform { translation: moved, ..UiTransform::IDENTITY })).with_children(|item| {
                        if row.danger && row.selected {
                            item.spawn((HoldBar, Node { position_type: PositionType::Absolute, left: px(0), top: px(0), bottom: px(0), width: percent(held * 100.0), border: UiRect::right(px(if held > 0.0 { 3 } else { 0 })), ..default() }, BackgroundColor(Color::srgba(0.898, 0.282, 0.302, 0.55)), BorderColor::all(Color::srgb(0.898, 0.282, 0.302))));
                        }
                        if row.selected && !grid {
                            item.spawn((Node { position_type: PositionType::Absolute, left: px(0), top: px(2), bottom: px(2), width: px(4), ..default() }, BackgroundColor(ACCENT)));
                        }
                        if let Some(icon) = icons.get(icon_of(&row.id)) {
                            item.spawn((Node { width: px(24), height: px(24), flex_shrink: 0.0, ..default() }, ImageNode::new(icon.clone()).with_color(if row.selected { DARK } else { DIM.with_alpha(0.85) })));
                        }
                        item.spawn(text(&row.label, 500, 17.0, ink));
                        if !row.sub.is_empty() {
                            item.spawn((Node { margin: if grid { UiRect::default() } else { UiRect::left(auto()) }, ..default() }, text(&row.sub, 400, 12.0, ink.with_alpha(0.7))));
                        }
                    });
                }
            });
            p.spawn((Node { column_gap: px(18), padding: UiRect::top(px(16)), margin: UiRect::top(px(16)), border: UiRect::top(px(1)), ..default() }, BorderColor::all(LINE))).with_children(|prompts| {
                for (button, does) in &foot {
                    prompts.spawn(Node { align_items: AlignItems::Center, column_gap: px(6), ..default() }).with_children(|prompt| {
                        let guide = button.len() > 3;
                        prompt.spawn((Node { min_width: px(22), height: px(22), padding: UiRect::horizontal(px(if guide { 8 } else { 6 })), border_radius: BorderRadius::all(px(11)), align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.085))))
                            .with_children(|pill| {
                                pill.spawn(if guide { spaced(&button.to_uppercase(), 600, 9.5, INK, 0.06) } else { spaced(button, 600, 11.0, INK, 0.0) });
                            });
                        prompt.spawn(text(does, 400, 12.0, DIM));
                    });
                }
            });
        });
    }

    let t = match home.left {
        None => bezier([0.2, 0.9, 0.2, 1.0], (now - home.since) / SLIDE),
        Some(left) => 1.0 - bezier([0.2, 0.9, 0.2, 1.0], (now - left) / SLIDE),
    };
    if home.left.is_some_and(|left| now - left >= SLIDE) && home.menu.is_none() {
        home.shown = false;
        seen.set_if_neq(Visibility::Hidden);
        commands.entity(entity).despawn_related::<Children>();
    }
    // (a node is made smaller about its middle: moved back so its corner stays in the window's)
    let (tall, shrunk) = (stage.h / small, 1.0 - small);
    transform.scale = Vec2::splat(if center { 1.0 } else { small });
    transform.translation = if center { Val2::new(Val::Px(0.0), Val::Percent(-50.0 + 4.0 * (1.0 - t))) } else { Val2::px(-wide * shrunk / 2.0 - 1.04 * wide * small * (1.0 - t), -tall * shrunk / 2.0) };
    if let Ok(mut dim) = dim.single_mut() {
        dim.color = Color::srgba(1.0, 1.0, 1.0, t.clamp(0.0, 1.0));
    }
    if let (Some(menu), Ok(mut bar)) = (&home.menu, bars.single_mut()) {
        bar.width = percent(menu.held() as f32 * 100.0);
    }
}

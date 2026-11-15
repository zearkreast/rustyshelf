//! The phone companion on the app's side: what a paired phone asks of the web server reaches the shelf here. Its
//! remote is one more controller, so every screen reads it as it reads a pad; what it types goes where the
//! keyboard's keys go; its Play starts the game as pressing A on its case would.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy::input::keyboard::{Key, KeyboardInput, NativeKeyCode};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use rustyshelf_core::companion::{Companion, Host, Input, Running};
use rustyshelf_core::library::{Library, ShelfKind};
use rustyshelf_core::scanner::Source;
use rustyshelf_core::{paths, ra};
use serde_json::{json, Value};

use crate::hud::Toast;
use crate::loading::{Base, LibraryRes, LibraryState};
use crate::play::Playing;
use crate::settings::Settings;

/// a tap lasts long enough for a frame to see it; a button held lets go when the phone stops saying it's still
/// down (seconds)
const TAP: f32 = 0.09;
const HOLD: f32 = 1.3;
/// how long a game asked for from the phone is waited for while something else is up (seconds)
const PLAY_WAIT: f32 = 20.0;
/// the remote's name as a controller
pub const REMOTE: &str = "RustyShelf phone remote";
/// the buttons the Home menu takes
const MENU_BUTTONS: [&str; 8] = ["up", "down", "left", "right", "a", "b", "start", "home"];

/// What the web server's threads and the shelf tell each other.
#[derive(Default)]
struct Shared {
    library: Arc<Value>,
    hidden: Vec<String>,
    glyphs: (String, bool),
    home_menu: bool,
    running: Option<Running>,
    /// the RetroAchievements account
    account: (String, String),
    inputs: Vec<Input>,
    /// a game to start: its shelf, its id, its title
    play: Option<(String, String, String)>,
    quit: bool,
}

struct Bridge {
    shared: Mutex<Shared>,
    ra: Arc<ra::Ra>,
}

impl Host for Bridge {
    fn library(&self) -> Arc<Value> {
        self.shared.lock().unwrap().library.clone()
    }
    fn hidden(&self) -> Vec<String> {
        self.shared.lock().unwrap().hidden.clone()
    }
    fn glyphs(&self) -> (String, bool) {
        self.shared.lock().unwrap().glyphs.clone()
    }
    fn home_menu(&self) -> bool {
        self.shared.lock().unwrap().home_menu
    }
    fn running(&self) -> Option<Running> {
        self.shared.lock().unwrap().running.clone()
    }
    fn play(&self, shelf: &str, id: &str, title: &str) -> bool {
        self.shared.lock().unwrap().play = Some((shelf.to_string(), id.to_string(), title.to_string()));
        true
    }
    fn quit(&self) {
        self.shared.lock().unwrap().quit = true;
    }
    fn input(&self, input: Input) {
        let mut shared = self.shared.lock().unwrap();
        if shared.inputs.len() < 256 {
            shared.inputs.push(input);
        }
    }
    fn achievements(&self, shelf: &str, title: &str) -> Option<Value> {
        let (user, key) = self.shared.lock().unwrap().account.clone();
        if key.is_empty() {
            return None;
        }
        self.ra.set_credentials(&user, &key);
        self.ra.progress(shelf, title, false).ok().flatten()
    }
    fn has_achievements(&self, shelf: &str) -> bool {
        !self.shared.lock().unwrap().account.1.is_empty() && ra::console_id(shelf).is_some()
    }
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }
}

/// A button of the phone's remote that's down.
struct Held {
    since: f32,
    /// let go by itself then, unless the phone says again that it's down
    until: f32,
    /// let go then: the phone said so
    up_at: Option<f32>,
}

#[derive(Resource)]
pub struct Phone {
    companion: Arc<Companion>,
    bridge: Arc<Bridge>,
    /// the remote as a controller, once it's been used
    pad: Option<Entity>,
    held: HashMap<&'static str, Held>,
    /// presses and releases the controller hasn't been given yet
    presses: Vec<(GamepadButton, bool)>,
    /// keys and text for the keyboard's own path, next frame
    typed: Vec<Input>,
    /// a game to start, and until when it's waited for
    play: Option<((String, String, String), f32)>,
    /// the library the phone was told is out of date
    stale: bool,
    on: bool,
    next_look: f32,
    next_network: f32,
    /// the companion's state as Settings last showed it
    seen: String,
}

fn button(name: &str) -> Option<GamepadButton> {
    Some(match name {
        "a" => GamepadButton::South,
        "b" => GamepadButton::East,
        "x" => GamepadButton::West,
        "y" => GamepadButton::North,
        "lb" => GamepadButton::LeftTrigger,
        "rb" => GamepadButton::RightTrigger,
        "view" => GamepadButton::Select,
        "start" => GamepadButton::Start,
        "home" => GamepadButton::Mode,
        "up" => GamepadButton::DPadUp,
        "down" => GamepadButton::DPadDown,
        "left" => GamepadButton::DPadLeft,
        "right" => GamepadButton::DPadRight,
        _ => return None,
    })
}

/// The shelves as the phone's library is made from them: every game with the files of its pictures.
fn library_json(base: &Library) -> Value {
    let file = |p: &Option<std::path::PathBuf>| p.as_ref().map_or(Value::Null, |p| json!(p.to_string_lossy()));
    let shelves: Vec<Value> = base
        .shelves
        .iter()
        .filter(|s| !matches!(s.kind, ShelfKind::Mixed))
        .map(|s| {
            let games: Vec<Value> = s
                .games
                .iter()
                .filter_map(|g| {
                    let kind = match g.source {
                        Source::Rom => "rom",
                        Source::Steam { .. } | Source::Pc(_) => "steam",
                        Source::App { .. } => "app",
                        Source::Tile(_) => return None,
                    };
                    let art = json!({ "front": file(&g.art.front), "hero": file(&g.hero), "back": if g.art.made_back { Value::Null } else { file(&g.art.back) },
                        "manual": g.art.manual.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(), "manualPdf": file(&g.art.manual_pdf) });
                    Some(json!({ "id": g.id, "title": g.title, "kind": kind, "region": g.region, "sizeBytes": g.size_bytes, "art": art, "info": g.info }))
                })
                .collect();
            let kind = match s.kind {
                ShelfKind::Console { .. } => "console",
                ShelfKind::Apps => "apps",
                _ => "steam",
            };
            json!({ "id": s.id, "name": s.name, "kind": kind, "games": games })
        })
        .collect();
    Value::Array(shelves)
}

pub fn setup(mut commands: Commands, mut settings: ResMut<Settings>) {
    let bridge = Arc::new(Bridge { shared: Mutex::default(), ra: settings.ra() });
    let companion = Companion::new(&paths::data_dir(&paths::app_root()), bridge.clone());
    companion.begin();
    settings.phone = Some(companion.clone());
    commands.insert_resource(Phone { companion, bridge, pad: None, held: HashMap::new(), presses: Vec::new(), typed: Vec::new(), play: None, stale: true, on: false, next_look: 0.0, next_network: 30.0, seen: String::new() });
}

/// What the phone typed, as keys of the keyboard: before the keyboard's own are read, so that the keys held and
/// the keys typed say the same in a frame.
pub fn keys(mut phone: ResMut<Phone>, settings: Res<Settings>, window: Query<Entity, With<PrimaryWindow>>, mut out: MessageWriter<KeyboardInput>) {
    if phone.typed.is_empty() {
        return;
    }
    let typed = std::mem::take(&mut phone.typed);
    // (Settings closed since: Escape on the shelf would close RustyShelf)
    let (Ok(window), true) = (window.single(), settings.open) else { return };
    for input in typed {
        let keys: Vec<(KeyCode, Key, bool)> = match input {
            Input::Key("Enter") => vec![(KeyCode::Enter, Key::Enter, false)],
            Input::Key("Escape") => vec![(KeyCode::Escape, Key::Escape, false)],
            Input::Key("Backspace") => vec![(KeyCode::Backspace, Key::Backspace, false)],
            // no key of the keyboard: a letter typed must not be one of the shelf's shortcuts
            Input::Text(text) => text.chars().map(|c| (KeyCode::Unidentified(NativeKeyCode::Unidentified), Key::Character(c.to_string().as_str().into()), true)).collect(),
            _ => Vec::new(),
        };
        for (key_code, logical_key, letter) in keys {
            let text = match (&logical_key, letter) {
                (Key::Character(c), true) => Some(c.clone()),
                _ => None,
            };
            out.write(KeyboardInput { key_code, logical_key: logical_key.clone(), state: ButtonState::Pressed, text, repeat: false, window });
            out.write(KeyboardInput { key_code, logical_key, state: ButtonState::Released, text: None, repeat: false, window });
        }
    }
}

pub fn run(
    time: Res<Time>,
    mut commands: Commands,
    mut phone: ResMut<Phone>,
    base: Res<Base>,
    lib: Res<LibraryRes>,
    mut settings: ResMut<Settings>,
    mut playing: ResMut<Playing>,
    home: Res<crate::home::Home>,
    mut reader: ResMut<crate::reader::Reader>,
    mut cursor: ResMut<crate::shelf::Cursor>,
    mut toast: ResMut<Toast>,
    mut pads: Query<&mut Gamepad>,
) {
    let phone = &mut *phone;
    let now = time.elapsed_secs();
    phone.stale |= base.is_changed();
    if now >= phone.next_look {
        phone.next_look = now + 0.5;
        let state = phone.companion.state();
        phone.on = state.on;
        let seen = format!("{} {} {} {} {} {} {:?}", state.enabled, state.url, state.pin, state.error, state.note, state.locked, state.phones.iter().map(|p| &p.id).collect::<Vec<_>>());
        if seen != phone.seen {
            phone.seen = seen;
            settings.phone_changed();
        }
        // the router can hand this PC another address, or the network arrive after the app started
        if state.enabled && now >= phone.next_network {
            phone.next_network = now + 30.0;
            phone.companion.follow_network();
        }
        if state.on {
            let (enabled, _, swap, glyphs) = settings.home_menu();
            let style = match glyphs {
                rustyshelf_core::homemenu::Glyphs::Ps => "ps",
                rustyshelf_core::homemenu::Glyphs::Nintendo => "nintendo",
                _ => "xbox",
            };
            let mut shared = phone.bridge.shared.lock().unwrap();
            (shared.hidden, shared.glyphs, shared.home_menu, shared.account) = (settings.hidden(), (style.to_string(), swap), enabled, settings.ra_account());
        }
    }
    if !phone.on {
        return;
    }
    let (inputs, play, quit) = {
        let mut shared = phone.bridge.shared.lock().unwrap();
        if let Some(b) = base.0.as_ref().filter(|_| phone.stale) {
            shared.library = Arc::new(library_json(b));
            phone.stale = false;
        }
        let running = playing.title.as_ref().map(|t| (playing.game_id(), t.as_str()));
        if shared.running.as_ref().map(|r| (r.id.as_str(), r.title.as_str())) != running {
            shared.running = running.map(|(id, title)| Running { id: id.to_string(), title: title.to_string(), kind: String::new() });
        }
        (std::mem::take(&mut shared.inputs), shared.play.take(), std::mem::take(&mut shared.quit))
    };

    let (in_game, menu_open) = (playing.title.is_some(), home.is_open());
    for input in inputs {
        match input {
            Input::Button(b, down) => {
                // over a game Home comes through the game's own watcher, as the pad's does
                if b == "home" && in_game {
                    if down && (menu_open || settings.home_menu().0) {
                        playing.home_pressed = true;
                    }
                    continue;
                }
                // the game has the controller; the Home menu takes its own buttons only
                if (in_game && !menu_open) || (menu_open && !MENU_BUTTONS.contains(&b)) || (b == "home" && !menu_open && !settings.home_menu().0) {
                    continue;
                }
                let Some(pad_button) = button(b) else { continue };
                if phone.pad.is_none() {
                    phone.pad = Some(commands.spawn((Gamepad::default(), Name::new(REMOTE))).id());
                }
                if down {
                    match phone.held.get_mut(b) {
                        Some(h) => h.until = now + HOLD,
                        None => {
                            phone.held.insert(b, Held { since: now, until: now + HOLD, up_at: None });
                            phone.presses.push((pad_button, true));
                        }
                    }
                } else if let Some(h) = phone.held.get_mut(b) {
                    h.up_at.get_or_insert((h.since + TAP).max(now));
                }
            }
            Input::Key(_) | Input::Text(_) => {
                if in_game || menu_open {
                    continue;
                }
                if !settings.open || (matches!(input, Input::Text(_)) && !settings.takes_text()) {
                    (toast.text, toast.until) = ("Open a text box on the TV to type into it (Find a game, or a name in Settings)".into(), now + 5.0);
                    continue;
                }
                phone.typed.push(input);
            }
        }
    }
    let presses = &mut phone.presses;
    phone.held.retain(|b, h| {
        let up = h.up_at.is_some_and(|at| now >= at) || now > h.until;
        if up {
            presses.extend(button(b).map(|pad_button| (pad_button, false)));
        }
        !up
    });
    if let Some(mut pad) = phone.pad.and_then(|e| pads.get_mut(e).ok()) {
        for (b, down) in phone.presses.drain(..) {
            // (the d-pad is read by how far it's pressed, too)
            pad.analog_mut().set(b, if down { 1.0 } else { 0.0 });
            if down {
                pad.digital_mut().press(b);
            } else {
                pad.digital_mut().release(b);
            }
        }
    }

    if quit && in_game {
        playing.close_asked = true;
    }
    if let Some(game) = play {
        phone.play = Some((game, now + PLAY_WAIT));
    }
    // whatever is open closes first; a game still closing is waited for
    let Some(((shelf, id, title), until)) = phone.play.clone() else { return };
    if now > until {
        phone.play = None;
        (toast.text, toast.until) = ("Couldn't start the game from the phone: RustyShelf was busy".into(), now + 6.0);
        return;
    }
    if in_game || menu_open {
        return;
    }
    if reader.is_open() {
        return reader.close();
    }
    if settings.open {
        settings.shut();
        return;
    }
    let LibraryState::Ready(l) = &lib.0 else { return };
    if settings.keys_taken || settings.just_closed {
        return;
    }
    phone.play = None;
    let place = l.shelves.iter().position(|s| s.id == shelf && !matches!(s.kind, ShelfKind::Mixed)).and_then(|i| Some((i, l.shelves[i].games.iter().position(|g| g.id == id)?)));
    let Some((i, at)) = place else {
        (toast.text, toast.until) = (format!("{} isn't on the shelf right now", if title.is_empty() { "That game" } else { &title }), now + 6.0);
        return;
    };
    (cursor.shelf, cursor.game, cursor.flipped, cursor.land) = (i, at, false, false);
    settings.play = true;
}

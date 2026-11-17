// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/tour.js and tour.css, as RustyShelf has them.
//! The shelf tour: a one-minute look at the basics, the first time the shelf is up after setup, and again from
//! Settings → System → Show the tour. The real screens are opened in turn (Find a game, Details, Inspect, the quick
//! menu, the Art Studio, Settings) with everything dimmed but the part being explained, and a card beside it.
//! Enter (→, A, RB) goes on, ← (LB) goes back, Esc (B) ends it for good; it never comes back on its own. A new
//! install owes it from its first start (data/tour-pending), so closing RustyShelf right after setup brings it on the
//! next start.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use rustyshelf_core::paths;
use rustyshelf_core::scanner::Source;

use crate::card::{ACCENT, DIM, INK, PAPER};
use crate::hud::{Face, MonaSans, Prompts, Tabs};
use crate::loading::{LibraryRes, LibraryState};
use crate::play::Playing;
use crate::settings::{Settings, TourPage};
use crate::shelf::{Inspect, ShelfCamera, Slot};

/// Settings → System → Show the tour was pressed.
pub static ASKED: AtomicBool = AtomicBool::new(false);
/// The tour is up (a tap on the screen goes on).
pub static ON: AtomicBool = AtomicBool::new(false);

/// seconds: the shelf still before the tour starts on its own; the longest a step's screen takes to settle; the
/// Inspect step turning the case a side
const SETTLE: f32 = 2.5;
const SETTLE_MAX: f32 = 1.5;
const SPIN_EVERY: f32 = 2.2;
/// logical pixels: the card's width, the room round the spotlit part, the card's distance from it and the edges
const CARD_W: f32 = 400.0;
const PAD: f32 = 10.0;
const GAP: f32 = 22.0;
const MARGIN: f32 = 18.0;
const SHADE: Color = Color::srgba(0.016, 0.02, 0.04, 0.72);

/// What a step opens.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Enter {
    Shelf,
    Page(TourPage),
    Inspect,
}

/// The part a step points at.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    None,
    /// the console tabs at the top
    Tabs,
    /// the chosen case, on the shelf or held up
    Case,
    /// the quick menu at the right
    Quick,
    /// the page of Settings that's open
    Page,
    /// the whole screen (Details): nothing dimmed, the card at the lower left
    Whole,
}

/// A key and what it does: the pad's buttons (as `Settings::cap` names them, or an arrow), a word before them
/// ("Hold"), the keyboard's keys (none: nothing on the keyboard does it) or a word in their place.
struct Key {
    pad: &'static [&'static str],
    pad_word: &'static str,
    kbd: &'static [&'static str],
    kbd_word: &'static str,
    what: &'static str,
    /// what it says on a pad, when that's different
    what_pad: &'static str,
}

const fn key(pad: &'static [&'static str], kbd: &'static [&'static str], what: &'static str) -> Key {
    Key { pad, pad_word: "", kbd, kbd_word: "", what, what_pad: "" }
}

struct Step {
    k: &'static str,
    t: &'static str,
    p: &'static str,
    enter: Enter,
    target: Target,
    keys: &'static [Key],
    /// only for a console game, which has a case to hold and an Art Studio
    rom_only: bool,
}

const STEPS: [Step; 10] = [
    Step { k: "Welcome", t: "Your shelf, in a minute", p: "A quick look at where things are. Skip it any time; it won’t come back on its own.", enter: Enter::Shelf, target: Target::None, keys: &[], rom_only: false },
    Step {
        k: "Consoles",
        t: "Switch consoles",
        p: "Each console has its own shelf. The tabs show which one you’re on.",
        enter: Enter::Shelf,
        target: Target::Tabs,
        keys: &[key(&["L2", "R2"], &["↑", "↓"], "Next or previous console"), key(&["L1", "R1"], &["Z", "C"], "Jump by letter")],
        rom_only: false,
    },
    Step {
        k: "Your games",
        t: "Pick a game",
        p: "Move along the shelf with the stick, the d-pad or the arrow keys.",
        enter: Enter::Shelf,
        target: Target::Case,
        keys: &[key(&["x"], &["Enter"], "Play"), key(&["tri"], &["F"], "Flip to the back"), Key { pad: &["tri"], pad_word: "Hold", kbd: &["H"], kbd_word: "", what: "Favourite", what_pad: "Hold to favourite" }],
        rom_only: false,
    },
    Step {
        k: "Find a game",
        t: "Search every shelf",
        p: "Type a few letters of a title and pick the game from the list. The shelf goes straight to it, whichever console it’s on. It’s in the quick menu too.",
        enter: Enter::Page(TourPage::Find),
        target: Target::Page,
        keys: &[Key { pad: &["L3"], pad_word: "Hold", kbd: &["/"], kbd_word: "", what: "Find a game", what_pad: "" }],
        rom_only: false,
    },
    Step {
        k: "Details",
        t: "Everything about a game",
        p: "Play it, make it a favourite, read its manual and see its screenshots and what’s known of it. Box art opens its Art Studio.",
        enter: Enter::Page(TourPage::Details),
        target: Target::Whole,
        keys: &[key(&["sq"], &["I"], "Open Details")],
        rom_only: false,
    },
    Step {
        k: "Inspect",
        t: "Hold the case",
        p: "Turn it round, bring it nearer and read its manual.",
        enter: Enter::Inspect,
        target: Target::Case,
        keys: &[key(&["R3"], &["V"], "Inspect"), key(&["tri"], &["F"], "Other side"), key(&["L2", "R2"], &["PgUp", "PgDn"], "Nearer, further"), key(&["sq"], &["M"], "Manual")],
        rom_only: true,
    },
    Step {
        k: "Quick menu",
        t: "The quick menu",
        p: "The chosen game’s Favourite, Details, Manual, Box art and Hide, then Find a game, the volume, the order of the shelves, trophies, Settings and power.",
        enter: Enter::Page(TourPage::Quick),
        target: Target::Quick,
        keys: &[key(&["☰"], &["Esc"], "Quick menu"), Key { pad: &["☰"], pad_word: "Hold", kbd: &["G"], kbd_word: "", what: "Sort and filter", what_pad: "" }],
        rom_only: false,
    },
    Step {
        k: "Art Studio",
        t: "Choose the box art",
        p: "Box art in Details or the quick menu opens the game here. Pick a part, then a version of it. Lock this game keeps the art as you chose it.",
        enter: Enter::Page(TourPage::Art),
        target: Target::Page,
        keys: &[key(&["↑", "↓"], &["↑", "↓"], "Front, back, spine, disc"), key(&["x"], &["Enter"], "See its versions"), key(&["o"], &["Esc"], "Back")],
        rom_only: true,
    },
    Step {
        k: "Adding games",
        t: "Add more games",
        p: "Consoles: point RustyShelf at a games folder. PC games and Custom games: add anything else, one at a time or a whole folder.",
        enter: Enter::Page(TourPage::AddGames),
        target: Target::Page,
        keys: &[Key { pad: &["☰"], pad_word: "", kbd: &["S"], kbd_word: "", what: "Settings", what_pad: "Quick menu, then Settings" }],
        rom_only: false,
    },
    Step { k: "Done", t: "That’s the basics", p: "Watch this one again from Settings → System → Show the tour.", enter: Enter::Shelf, target: Target::None, keys: &[], rom_only: false },
];

#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum Phase {
    #[default]
    Off,
    /// what the last step opened is closing
    Closing,
    /// the step's screen is opening, its part measured until it stops moving
    Settling,
    /// the card is made, and placed once its size is known
    Placing,
    Shown,
}

#[derive(Resource, Default)]
pub struct Tour {
    /// the steps of this run, and the one shown
    steps: Vec<usize>,
    at: usize,
    phase: Phase,
    since: f32,
    /// the part as last measured, and for how many frames it's stayed there
    rect: Option<Rect>,
    same: u32,
    /// when the case turns a side next, and to where
    spin_at: f32,
    spin_to: f32,
    owed: Option<bool>,
    still_since: Option<f32>,
    /// the frame the tour began on (its key isn't the tour's)
    began: bool,
}

/// Where the parts are this frame, in logical pixels (measured once laid out).
#[derive(Resource, Default)]
pub struct TourRects {
    tabs: Option<Rect>,
    quick: Option<Rect>,
    page: Option<Rect>,
    case: Option<Rect>,
    card: Option<Vec2>,
    screen: Vec2,
}

#[derive(Component)]
pub struct TourRoot;

/// The four shades round the spotlit part (above, below, left, right) and the ring on it.
#[derive(Component)]
pub struct Shade(usize);

#[derive(Component)]
pub struct Ring;

#[derive(Component)]
pub struct TourCard;

fn pending_file() -> std::path::PathBuf {
    paths::data_dir(&paths::app_root()).join("tour-pending")
}

/// Where the card goes (its top left) beside the part `r`: right or left of it, else below or above (below first
/// for a wide, low part such as the tabs), else on the part's lower left; in the middle with no part, at the lower
/// left for the whole screen.
fn place(r: Option<Rect>, card: Vec2, screen: Vec2, bottom_left: bool) -> Vec2 {
    let clamp_x = |x: f32| x.min(screen.x - card.x - MARGIN).max(MARGIN);
    let clamp_y = |y: f32| y.min(screen.y - card.y - MARGIN).max(MARGIN);
    if bottom_left {
        return Vec2::new(MARGIN, clamp_y(screen.y - card.y - MARGIN));
    }
    let Some(r) = r else { return ((screen - card) / 2.0).max(Vec2::ZERO) };
    let (x, y, w, h) = (r.min.x, r.min.y, r.width(), r.height());
    let side = [
        (x + w + GAP + card.x <= screen.x - MARGIN).then(|| Vec2::new(x + w + GAP, clamp_y(y + h / 2.0 - card.y / 2.0))),
        (x - GAP - card.x >= MARGIN).then(|| Vec2::new(x - GAP - card.x, clamp_y(y + h / 2.0 - card.y / 2.0))),
    ];
    let under = [
        (y + h + GAP + card.y <= screen.y - MARGIN).then(|| Vec2::new(clamp_x(x + w / 2.0 - card.x / 2.0), y + h + GAP)),
        (y - GAP - card.y >= MARGIN).then(|| Vec2::new(clamp_x(x + w / 2.0 - card.x / 2.0), y - GAP - card.y)),
    ];
    let tries = if w > h * 3.0 { [under[0], under[1], side[0], side[1]] } else { [side[0], side[1], under[0], under[1]] };
    tries.into_iter().flatten().next().unwrap_or_else(|| Vec2::new(clamp_x(x + MARGIN), clamp_y(y + h - card.y - MARGIN)))
}

/// The steps for the chosen game: Inspect and the Art Studio only for a console game.
fn steps_for(rom: bool) -> Vec<usize> {
    (0..STEPS.len()).filter(|&i| rom || !STEPS[i].rom_only).collect()
}

/// A key's caps and what it does, on a pad or the keyboard; nothing when the keyboard has no key for it.
fn key_line(k: &Key, pad: bool, cap: &dyn Fn(&'static str) -> String) -> Option<(String, Vec<String>, String)> {
    if pad {
        let what = if k.what_pad.is_empty() { k.what } else { k.what_pad };
        return Some((k.pad_word.to_string(), k.pad.iter().map(|b| cap(b)).collect(), what.to_string()));
    }
    if k.kbd.is_empty() && k.kbd_word.is_empty() {
        return None;
    }
    Some((k.kbd_word.to_string(), k.kbd.iter().map(|s| s.to_string()).collect(), k.what.to_string()))
}

pub fn setup(mut commands: Commands) {
    commands
        .spawn((TourRoot, Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), display: Display::None, ..default() }, GlobalZIndex(30)))
        .with_children(|r| {
            for i in 0..4 {
                r.spawn((Shade(i), Node { position_type: PositionType::Absolute, ..default() }, BackgroundColor(SHADE)));
            }
            r.spawn((
                Ring,
                Node { position_type: PositionType::Absolute, border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(12)), ..default() },
                BorderColor::all(Color::srgba(0.66, 0.75, 1.0, 0.8)),
                Visibility::Hidden,
            ));
        });
}

/// Once laid out: where the tabs, the quick menu, the open page, the chosen case and the tour's card are.
#[allow(clippy::type_complexity)]
pub fn measure(
    tour: Res<Tour>,
    mut rects: ResMut<TourRects>,
    window: Query<&Window, With<PrimaryWindow>>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform, Has<Tabs>, Has<crate::settings::panels::QuickPanel>, Has<crate::settings::motion::PageBody>, Has<TourCard>)>,
    camera: Query<(&Camera, &GlobalTransform), With<ShelfCamera>>,
    slots: Query<(&Slot, &GlobalTransform)>,
    cursor: Res<crate::shelf::Cursor>,
) {
    if tour.phase == Phase::Off {
        return;
    }
    *rects = TourRects::default();
    if let Ok(w) = window.single() {
        rects.screen = Vec2::new(w.width(), w.height());
    }
    for (n, at, tabs, quick, page, card) in &nodes {
        let inv = n.inverse_scale_factor;
        let size = n.size * inv;
        if size.x < 1.0 || size.y < 1.0 {
            continue;
        }
        let r = Some(Rect::from_center_size(at.translation * inv, size));
        if tabs {
            rects.tabs = r;
        } else if quick {
            rects.quick = r;
        } else if page {
            rects.page = r;
        } else if card {
            rects.card = Some(size);
        }
    }
    let Ok((cam, eye)) = camera.single() else { return };
    if let Some((_, at)) = slots.iter().find(|(s, _)| s.index == cursor.game) {
        let mut r: Option<Rect> = None;
        // the front of the case (its depth only widens the box round it)
        for c in 0..4 {
            let corner = Vec3::new(if c & 1 == 0 { -0.5 } else { 0.5 }, if c & 2 == 0 { -0.5 } else { 0.5 }, 0.0);
            if let Ok(p) = cam.world_to_viewport(eye, at.transform_point(corner)) {
                r = Some(r.map_or(Rect::from_corners(p, p), |r| r.union_point(p)));
            }
        }
        rects.case = r;
    }
}

type RootQ<'w, 's> = Query<'w, 's, (Entity, &'static mut Node), (With<TourRoot>, Without<Shade>, Without<Ring>, Without<TourCard>)>;
type ShadeQ<'w, 's> = Query<'w, 's, (&'static Shade, &'static mut Node), (Without<TourRoot>, Without<Ring>, Without<TourCard>)>;
type RingQ<'w, 's> = Query<'w, 's, (&'static mut Node, &'static mut Visibility), (With<Ring>, Without<TourRoot>, Without<Shade>, Without<TourCard>)>;
type CardQ<'w, 's> = Query<'w, 's, (Entity, &'static mut Node, &'static mut Visibility), (With<TourCard>, Without<TourRoot>, Without<Shade>, Without<Ring>)>;

pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut tour: ResMut<Tour>,
    mut settings: ResMut<Settings>,
    mut inspect: ResMut<Inspect>,
    mut sounds: ResMut<crate::sound::Sounds>,
    (lib, cursor, rects, card, playing, prompts, ui_scale): (Res<LibraryRes>, Res<crate::shelf::Cursor>, Res<TourRects>, Res<crate::card::Card>, Res<Playing>, Res<Prompts>, Res<UiScale>),
    fonts: (Option<Res<MonaSans>>, Option<Res<Face>>),
    (mut root, mut shades, mut ring, mut cards): (RootQ, ShadeQ, RingQ, CardQ),
) {
    let (now, dt) = (time.elapsed_secs(), time.delta_secs());
    if tour.phase == Phase::Off {
        if !wants_to_start(&mut tour, &lib, &settings, &playing, &card, now) {
            return;
        }
        let LibraryState::Ready(l) = &lib.0 else { return };
        let rom = l.shelves.get(cursor.shelf).and_then(|s| s.games.get(cursor.game)).is_some_and(|g| g.source == Source::Rom);
        let game = l.shelves.get(cursor.shelf).and_then(|s| s.games.get(cursor.game)).is_some_and(|g| !matches!(g.source, Source::Tile(_)));
        // (Details needs a game, not a shelf's own tile)
        tour.steps = steps_for(rom).into_iter().filter(|&i| game || STEPS[i].enter != Enter::Page(TourPage::Details)).collect();
        tour.at = 0;
        tour.began = true;
        ON.store(true, Ordering::Relaxed);
        if let Ok((_, mut n)) = root.single_mut() {
            n.display = Display::Flex;
        }
        go_to(&mut tour, &mut settings, &mut inspect, 0, now);
    }
    settings.keys_taken = true;
    let step = &STEPS[tour.steps[tour.at]];

    // the keys: only presses, never what was down as the tour began
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let (ok, back) = settings.confirm();
    let began = std::mem::take(&mut tour.began);
    let next = !began && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::ArrowRight) || pad(ok) || pad(GamepadButton::RightTrigger));
    let prev = !began && (keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::LeftTrigger));
    let end = !began && (keys.just_pressed(KeyCode::Escape) || pad(back));
    if end {
        sounds.play("back", 1);
        finish(&mut tour, &mut settings, &mut inspect, now);
        hide(&mut root, &mut ring, &mut cards, &mut commands);
        return;
    }
    if tour.phase == Phase::Shown && (next || prev) {
        let to = tour.at as i32 + if next { 1 } else { -1 };
        if to >= tour.steps.len() as i32 {
            sounds.play("go", 1);
            finish(&mut tour, &mut settings, &mut inspect, now);
            hide(&mut root, &mut ring, &mut cards, &mut commands);
            return;
        }
        if to >= 0 {
            sounds.play("tick", if next { 1 } else { -1 });
            go_to(&mut tour, &mut settings, &mut inspect, to as usize, now);
            for (e, ..) in &cards {
                commands.entity(e).despawn();
            }
            // (the spotlight goes at once, not after the next screen is up)
            spot(&mut shades, &mut ring, None, rects.screen, ui_scale.0.max(0.01), false);
            return;
        }
    }

    let target = |t: Target| match t {
        Target::None | Target::Whole => None,
        Target::Tabs => rects.tabs,
        Target::Case => rects.case,
        Target::Quick => rects.quick,
        Target::Page => rects.page,
    };
    match tour.phase {
        Phase::Off => {}
        Phase::Closing => {
            if !settings.tour_busy() && now - tour.since > 0.2 {
                match step.enter {
                    Enter::Shelf => {}
                    Enter::Page(p) => settings.tour_page = Some(p),
                    Enter::Inspect => inspect.hold(now),
                }
                (tour.phase, tour.since, tour.rect, tour.same) = (Phase::Settling, now, None, 0);
                (tour.spin_at, tour.spin_to) = (now + SPIN_EVERY, 0.0);
            }
        }
        Phase::Settling => {
            let r = target(step.target);
            let same = match (r, tour.rect) {
                (Some(a), Some(b)) => (a.min - b.min).abs().max_element() < 1.0 && (a.max - b.max).abs().max_element() < 1.0,
                (None, None) => true,
                _ => false,
            };
            tour.same = if same { tour.same + 1 } else { 0 };
            tour.rect = r;
            let opened = match step.enter {
                Enter::Page(TourPage::Quick) => settings.tour_busy() && r.is_some(),
                Enter::Page(_) => settings.open,
                _ => true,
            };
            if (opened && tour.same >= 4 && now - tour.since > 0.15) || now - tour.since > SETTLE_MAX {
                tour.phase = Phase::Placing;
                if let (Some(mona), Some(face)) = (&fonts.0, &fonts.1) {
                    let n = tour.steps.len();
                    build_card(&mut commands, &root, step, tour.at, n, &settings, prompts.pad, mona, face);
                }
            }
        }
        Phase::Placing => {
            let Some(size) = rects.card else { return };
            let screen = rects.screen;
            let r = tour.rect.map(|r| Rect::from_corners(r.min - PAD, r.max + PAD));
            let at = place(r, size, screen, step.target == Target::Whole);
            let s = ui_scale.0.max(0.01);
            for (_, mut n, mut v) in &mut cards {
                (n.left, n.top) = (px(at.x / s), px(at.y / s));
                *v = Visibility::Inherited;
            }
            spot(&mut shades, &mut ring, if step.target == Target::Whole { Some(Rect::from_corners(Vec2::ZERO, screen)) } else { r }, screen, s, step.target != Target::Whole);
            tour.phase = Phase::Shown;
        }
        Phase::Shown => {
            // the case turns a side at a time, to show what holding it does
            if step.enter == Enter::Inspect && inspect.on && !settings.reduce_motion() {
                if now >= tour.spin_at {
                    tour.spin_to = (tour.spin_to + 90.0).rem_euclid(360.0);
                    tour.spin_at = now + SPIN_EVERY;
                }
                inspect.turn_toward(tour.spin_to, dt);
            }
        }
    }
}

/// Whether the tour should start now: asked for, or owed to a new install once the shelf is up and still.
fn wants_to_start(tour: &mut Tour, lib: &LibraryRes, settings: &Settings, playing: &Playing, card: &crate::card::Card, now: f32) -> bool {
    let owed = *tour.owed.get_or_insert_with(|| {
        if !settings.setup_done() {
            let _ = std::fs::write(pending_file(), "1");
        }
        !settings.setup_done() || pending_file().exists()
    });
    let asked = ASKED.load(Ordering::Relaxed);
    if !crate::whatsnew::quiet(lib, settings, playing, card) || settings.tour_busy() || (!asked && (!owed || !settings.setup_done() || settings.tour_done())) {
        tour.still_since = None;
        return false;
    }
    if !asked && now - *tour.still_since.get_or_insert(now) < SETTLE {
        return false;
    }
    if !matches!(lib.0, LibraryState::Ready(_)) {
        return false;
    }
    ASKED.store(false, Ordering::Relaxed);
    true
}

/// Goes to step `i`: what the step before opened closes first.
fn go_to(tour: &mut Tour, settings: &mut Settings, inspect: &mut Inspect, i: usize, now: f32) {
    let was = STEPS[tour.steps[tour.at]].enter;
    let to = STEPS[tour.steps[i]].enter;
    tour.at = i;
    // (two shelf steps in a row: nothing to close in between)
    if !(was == Enter::Shelf && to == Enter::Shelf) {
        settings.tour_close(now);
        inspect.put_down(now);
    }
    (tour.phase, tour.since) = (Phase::Closing, now);
}

/// Ends the tour for good: what it opened closes first, then it's marked as seen.
fn finish(tour: &mut Tour, settings: &mut Settings, inspect: &mut Inspect, now: f32) {
    settings.tour_close(now);
    inspect.put_down(now);
    settings.set_tour_done();
    let _ = std::fs::remove_file(pending_file());
    (tour.phase, tour.steps, tour.at) = (Phase::Off, Vec::new(), 0);
    tour.still_since = None;
    ON.store(false, Ordering::Relaxed);
    // (the key that ended it isn't the shelf's too)
    settings.just_closed = true;
}

#[allow(clippy::type_complexity)]
fn hide(
    root: &mut RootQ,
    ring: &mut RingQ,
    cards: &mut CardQ,
    commands: &mut Commands,
) {
    if let Ok((_, mut n)) = root.single_mut() {
        n.display = Display::None;
    }
    if let Ok((_, mut v)) = ring.single_mut() {
        *v = Visibility::Hidden;
    }
    for (e, ..) in cards.iter() {
        commands.entity(e).despawn();
    }
}

/// The shades round `r` and the ring on it (`ring`: drawn), in logical pixels; with no part the whole screen is
/// shaded.
#[allow(clippy::type_complexity)]
fn spot(
    shades: &mut ShadeQ,
    ring: &mut RingQ,
    r: Option<Rect>,
    screen: Vec2,
    s: f32,
    show_ring: bool,
) {
    let r = r.map(|r| r.intersect(Rect::from_corners(Vec2::ZERO, screen)));
    let boxes: [Rect; 4] = match r {
        Some(r) => [
            Rect::new(0.0, 0.0, screen.x, r.min.y),
            Rect::new(0.0, r.max.y, screen.x, screen.y),
            Rect::new(0.0, r.min.y, r.min.x, r.max.y),
            Rect::new(r.max.x, r.min.y, screen.x, r.max.y),
        ],
        None => [Rect::from_corners(Vec2::ZERO, screen), Rect::default(), Rect::default(), Rect::default()],
    };
    for (Shade(i), mut n) in shades.iter_mut() {
        let b = boxes[*i];
        (n.left, n.top, n.width, n.height) = (px(b.min.x / s), px(b.min.y / s), px(b.width().max(0.0) / s), px(b.height().max(0.0) / s));
    }
    if let Ok((mut n, mut v)) = ring.single_mut() {
        match r.filter(|_| show_ring) {
            Some(r) => {
                (n.left, n.top, n.width, n.height) = (px(r.min.x / s), px(r.min.y / s), px(r.width() / s), px(r.height() / s));
                *v = Visibility::Inherited;
            }
            None => *v = Visibility::Hidden,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_card(
    commands: &mut Commands,
    root: &RootQ,
    step: &Step,
    at: usize,
    of: usize,
    settings: &Settings,
    pad: bool,
    mona: &MonaSans,
    face: &Face,
) {
    let Ok((root, _)) = root.single() else { return };
    let lang = settings.language();
    let t = |s: &str| rustyshelf_core::i18n::tr(&lang, s);
    let text = |s: String, weight: u32, size: f32, color: Color| (Text::new(s), mona.sized(weight, bevy::text::FontSize::Px(size), &face.0), TextColor(color));
    let cap = |k: &'static str| -> String { if k.chars().all(|c| c.is_ascii_alphanumeric()) || matches!(k, "☰" | "⧉") { settings.cap(k).to_string() } else { k.to_string() } };
    let last = at + 1 == of;
    let pill = |r: &mut ChildSpawnerCommands, s: String| {
        r.spawn((Node { padding: UiRect::axes(px(8), px(3)), border_radius: BorderRadius::all(px(6)), border: UiRect::all(px(1)), ..default() }, BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.45))))
            .with_child(text(s, 700, 12.0, INK));
    };
    commands.entity(root).with_children(|r| {
        r.spawn((
            TourCard,
            Node {
                position_type: PositionType::Absolute,
                width: px(CARD_W),
                max_width: percent(92),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::new(px(26), px(26), px(24), px(20)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(18)),
                ..default()
            },
            BackgroundColor(PAPER),
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.1)),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.7), px(0), px(28), px(-18), px(70)),
            Visibility::Hidden,
        ))
        .with_children(|c| {
            c.spawn(Node { justify_content: JustifyContent::SpaceBetween, ..default() }).with_children(|k| {
                k.spawn(text(t(step.k).to_uppercase(), 600, 11.0, ACCENT));
                k.spawn(text(format!("{} / {}", at + 1, of), 500, 11.0, DIM));
            });
            c.spawn(text(t(step.t), 700, 24.0, INK));
            c.spawn(text(t(step.p), 400, 14.0, DIM));
            let lines: Vec<_> = step.keys.iter().filter_map(|k| key_line(k, pad, &cap)).collect();
            if !lines.is_empty() {
                c.spawn(Node { margin: UiRect::top(px(4)), flex_direction: FlexDirection::Column, row_gap: px(7), ..default() }).with_children(|list| {
                    for (word, caps, what) in lines {
                        list.spawn(Node { column_gap: px(8), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                            if !word.is_empty() {
                                row.spawn(text(t(&word), 400, 12.0, DIM));
                            }
                            for k in caps {
                                pill(row, t(&k));
                            }
                            row.spawn((Node { margin: UiRect::left(px(4)), ..default() }, text(t(&what), 400, 13.0, DIM)));
                        });
                    }
                });
            }
            c.spawn((Node { margin: UiRect::top(px(10)), padding: UiRect::top(px(12)), border: UiRect::top(px(1)), justify_content: JustifyContent::SpaceBetween, align_items: AlignItems::Center, ..default() }, BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.08))))
                .with_children(|foot| {
                    foot.spawn(Node { column_gap: px(3), align_items: AlignItems::Center, ..default() }).with_children(|dots| {
                        for i in 0..of {
                            let on = i == at;
                            dots.spawn((Node { width: px(if on { 14.0 } else { 4.0 }), height: px(4), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(if on { ACCENT } else { Color::srgba(1.0, 1.0, 1.0, 0.18) })));
                        }
                    });
                    foot.spawn(Node { column_gap: px(10), align_items: AlignItems::Center, ..default() }).with_children(|acts| {
                        let (skip_cap, next_cap) = if pad { (cap("o"), cap("x")) } else { ("Esc".to_string(), "Enter".to_string()) };
                        acts.spawn(Node { column_gap: px(6), align_items: AlignItems::Center, ..default() }).with_children(|b| {
                            pill(b, t(&skip_cap));
                            b.spawn((text(t(if last { "Close" } else { "Skip the tour" }), 500, 13.0, DIM), TextLayout::no_wrap()));
                        });
                        acts.spawn((Node { column_gap: px(6), align_items: AlignItems::Center, padding: UiRect::axes(px(12), px(6)), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.1)))).with_children(|b| {
                            pill(b, t(&next_cap));
                            b.spawn((text(t(if last { "Done" } else { "Next" }), 600, 13.0, INK), TextLayout::no_wrap()));
                        });
                    });
                });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = Vec2::new(1600.0, 900.0);
    const CARD: Vec2 = Vec2::new(440.0, 300.0);

    #[test]
    fn the_card_goes_beside_a_part_then_under_it() {
        // a case in the middle: on its right
        let case = Rect::new(650.0, 250.0, 950.0, 650.0);
        assert_eq!(place(Some(case), CARD, SCREEN, false), Vec2::new(950.0 + GAP, 300.0));
        // the quick menu at the right edge: on its left
        let quick = Rect::new(1140.0, 0.0, 1600.0, 900.0);
        assert_eq!(place(Some(quick), CARD, SCREEN, false).x, 1140.0 - GAP - CARD.x);
        // the tabs, wide and low: under them
        let tabs = Rect::new(400.0, 20.0, 1200.0, 70.0);
        assert_eq!(place(Some(tabs), CARD, SCREEN, false), Vec2::new(580.0, 70.0 + GAP));
    }

    #[test]
    fn the_card_sits_in_the_middle_or_the_lower_left() {
        assert_eq!(place(None, CARD, SCREEN, false), Vec2::new(580.0, 300.0));
        assert_eq!(place(None, CARD, SCREEN, true), Vec2::new(MARGIN, 900.0 - 300.0 - MARGIN));
        // a part that fills the screen: on its lower left, inside it
        let all = Rect::new(0.0, 0.0, 1600.0, 900.0);
        assert_eq!(place(Some(all), CARD, SCREEN, false), Vec2::new(MARGIN, 900.0 - 300.0 - MARGIN));
    }

    #[test]
    fn a_game_without_a_case_skips_inspect_and_the_art_studio() {
        let names = |rom| steps_for(rom).into_iter().map(|i| STEPS[i].k).collect::<Vec<_>>();
        assert!(names(true).contains(&"Inspect") && names(true).contains(&"Art Studio"));
        assert!(!names(false).contains(&"Inspect") && !names(false).contains(&"Art Studio"));
        assert_eq!(names(false).first(), Some(&"Welcome"));
        assert_eq!(names(false).last(), Some(&"Done"));
    }

    #[test]
    fn keys_follow_the_device_last_used() {
        let cap = |k: &'static str| format!("<{k}>");
        let fav = &STEPS[2].keys[2];
        assert_eq!(key_line(fav, true, &cap), Some(("Hold".into(), vec!["<tri>".into()], "Hold to favourite".into())));
        assert_eq!(key_line(fav, false, &cap), Some((String::new(), vec!["H".into()], "Favourite".into())));
        let none = Key { pad: &["R3"], pad_word: "", kbd: &[], kbd_word: "", what: "Inspect", what_pad: "" };
        assert_eq!(key_line(&none, false, &cap), None);
    }
}

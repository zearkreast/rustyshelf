// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the Sort panel
// and the quick menu of Boxshelf's renderer/app.js (openSort, qmOpen) and renderer/style.css / quickshelf.css, as
// RustyShelf has them.
//! The two panels over the shelf. The Sort panel (G, or Start held): a card at the top with the order and the
//! filter as rows of chips, applied as they change. The Quick Shelf (a Start tap, or Tab): a panel that slides in at
//! the right while the shelf steps aside, with the chosen game, its Favorite / Details / Box art / Hide tiles, and
//! rows for Find a game, the volume, sorting, trophies, updates, Settings and power.

use super::*;
use crate::motion::{self, Ease, FADE, PANEL};

/// seconds: the Sort panel fading in and its card sliding down (style.css .sortpanel, .sp-card), the quick menu
/// sliding in (quickshelf.css .qm) and its highlight gliding, and a row's chips opening
const SORT_FADE: f32 = 0.25;
const SORT_SLIDE: f32 = 0.35;
const QUICK_SLIDE: f32 = 0.55;
const PILL: f32 = 0.32;
/// cubic-bezier(.2,.9,.25,1) and (.3,.9,.25,1)
const CARD: [f32; 4] = [0.2, 0.9, 0.25, 1.0];
const GLIDE: [f32; 4] = [0.3, 0.9, 0.25, 1.0];
/// a Start held this long opens the Sort panel; one let go sooner is a tap
const START_HOLD: f32 = 0.65;

/// The game the panels are about: as it was when one opened.
#[derive(Clone)]
pub(super) struct PanelGame {
    pub game: Game,
    pub console: String,
    pub shelf_name: String,
    pub played: Option<rustyshelf_core::history::Played>,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Which {
    Sort,
    Quick,
}

/// A row of the quick menu.
#[derive(Clone, Copy, PartialEq, Debug)]
enum QRow {
    Acts,
    Find,
    Volume,
    Sort,
    Trophies,
    Updates,
    Settings,
    Power,
}

const ACTS: [&str; 4] = ["fav", "det", "art", "hide"];

#[derive(Default)]
pub struct Panels {
    open: Option<Which>,
    /// when it opened, and when it began to close (it's drawn until it's gone)
    since: f32,
    closing: Option<(Which, f32)>,
    game: Option<PanelGame>,
    /// the Sort panel's row (0: order, 1: filter)
    sort_row: usize,
    /// the quick menu: its row, its action tile, the row opened into its chips and the line and chip in them
    row: usize,
    act: usize,
    sub: Option<usize>,
    line: usize,
    chip: usize,
    /// the pill's place, as measured, eased
    pill: Ease<Vec4>,
    pill_placed: bool,
    /// the shelf stepping aside (0: in place, 1: aside)
    aside: Ease<f32>,
    pub(super) redraw: bool,
    /// the Start button went down then (a tap or a hold is told apart as it's held)
    start_down: Option<f32>,
    /// Exit RustyShelf was chosen
    quit: bool,
    /// opened on the Home shelf, which keeps its own order
    home: bool,
}

#[derive(Component)]
pub struct PanelRoot;

#[derive(Component)]
pub struct SortCard;

#[derive(Component)]
pub struct QuickPanel;

#[derive(Component)]
pub struct Pill;

#[derive(Component)]
pub struct PillRow;

impl Panels {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Exit RustyShelf was chosen in the quick menu, once.
    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit)
    }

    /// How far the shelf has stepped aside for the quick menu: 0 to 1.
    pub fn aside(&self, now: f32) -> f32 {
        self.aside.now(now, QUICK_SLIDE, PANEL)
    }
}

/// The quick menu's rows, as this platform has them.
fn quick_rows(has_game: bool) -> Vec<QRow> {
    let mut rows = Vec::new();
    if has_game {
        rows.push(QRow::Acts);
    }
    rows.extend([QRow::Find, QRow::Volume, QRow::Sort, QRow::Trophies]);
    if cfg!(not(target_os = "android")) {
        rows.push(QRow::Updates);
    }
    rows.extend([QRow::Settings, QRow::Power]);
    rows
}

const POWER: [(&str, &str); 4] = [("quit", "Exit RustyShelf"), ("sleep", "Sleep"), ("restart", "Restart PC"), ("shutdown", "Shut down")];

impl Settings {
    fn panel_lines(&self, row: QRow) -> Vec<(&'static str, Vec<(String, String)>)> {
        match row {
            QRow::Sort => vec![("Sort by", SORTS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()), ("Show", self.filters.clone())],
            QRow::Power => vec![("", POWER.iter().filter(|p| cfg!(windows) || p.0 == "quit" || cfg!(target_os = "linux")).map(|(a, b)| (a.to_string(), b.to_string())).collect())],
            _ => Vec::new(),
        }
    }

    fn panel_value(&self, line: &str) -> String {
        match line {
            "Sort by" => self.text(&["ui", "sort"]),
            "Show" => self.text(&["ui", "filter"]),
            _ => String::new(),
        }
    }

    fn panel_volume(&self) -> f64 {
        at(&self.cfg, &["ui", "volume"]).and_then(Value::as_f64).unwrap_or(0.7)
    }

    /// Opens a panel over the shelf, about the game chosen there.
    pub(super) fn open_panel(&mut self, which: Which, game: Option<PanelGame>, home: bool, now: f32) {
        self.panels.home = home;
        let had = self.panels.open;
        if had == Some(which) {
            return;
        }
        self.panels.open = Some(which);
        self.panels.since = now;
        self.panels.closing = had.map(|w| (w, now));
        self.panels.game = game;
        self.panels.sub = None;
        self.panels.pill_placed = false;
        self.panels.redraw = true;
        self.panels.sort_row = 0;
        if which == Which::Quick {
            let rows = quick_rows(self.panels.game.is_some());
            (self.panels.row, self.panels.act) = (rows.iter().position(|r| *r == QRow::Volume).unwrap_or(0), 0);
            self.panels.aside.go(1.0, now, QUICK_SLIDE, PANEL);
            self.cues.push(("panel", 1));
        } else {
            self.panels.aside.go(0.0, now, QUICK_SLIDE, PANEL);
            self.cues.push(("flip", 1));
        }
    }

    pub(super) fn close_panel(&mut self, now: f32, quiet: bool) {
        if let Some(w) = self.panels.open.take() {
            self.panels.closing = Some((w, now));
            self.panels.aside.go(0.0, now, QUICK_SLIDE, PANEL);
            self.panels.redraw = true;
            if !quiet {
                self.cues.push(("back", 1));
            }
        }
    }

    /// The Start button on the shelf: a tap opens the quick menu as it's let go, a hold opens the Sort panel.
    pub(super) fn start_button(&mut self, down: bool, up: bool, held: bool, now: f32) -> Option<Which> {
        if down {
            self.panels.start_down = Some(now);
        }
        let since = self.panels.start_down?;
        if held && now - since >= START_HOLD {
            self.panels.start_down = None;
            return Some(Which::Sort);
        }
        if up {
            self.panels.start_down = None;
            return Some(Which::Quick);
        }
        None
    }

    /// The keys while a panel is open. True when they were its.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn panel_keys(&mut self, keys: &ButtonInput<KeyCode>, pads: &Query<&Gamepad>, now: f32) -> bool {
        let Some(which) = self.panels.open else { return false };
        let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
        let (ok, back) = self.confirm();
        let dx = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
        let dy = (keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32;
        let enter = keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(ok);
        let esc = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(back);
        match which {
            Which::Sort => {
                if dy != 0 {
                    self.panels.sort_row = 1 - self.panels.sort_row;
                    self.cues.push(("tick", dy));
                    self.panels.redraw = true;
                }
                if dx != 0 {
                    let line = if self.panels.sort_row == 0 { "Sort by" } else { "Show" };
                    self.step_chip(line, dx);
                }
                let letter = keys.just_pressed(KeyCode::KeyZ) || keys.just_pressed(KeyCode::KeyC) || pad(GamepadButton::LeftTrigger) || pad(GamepadButton::RightTrigger);
                if enter || esc || letter || keys.just_pressed(KeyCode::KeyG) || pad(GamepadButton::Start) || pad(GamepadButton::Select) {
                    self.close_panel(now, false);
                }
            }
            Which::Quick => self.quick_keys(dx, dy, enter, esc, keys.just_pressed(KeyCode::Tab) || pad(GamepadButton::Start) || pad(GamepadButton::Select), now),
        }
        true
    }

    /// Steps a line of chips (the order, or the filter) to the next or the one before, and applies it.
    fn step_chip(&mut self, line: &str, d: i32) {
        let (path, list): (&'static [&'static str], Vec<String>) = match line {
            "Sort by" => (&["ui", "sort"], SORTS.iter().map(|s| s.0.to_string()).collect()),
            _ => (&["ui", "filter"], self.filters.iter().map(|f| f.0.clone()).collect()),
        };
        if list.is_empty() {
            return;
        }
        let now = self.panel_value(line);
        let at = list.iter().position(|v| *v == now).unwrap_or(0) as i32;
        let to = list[(at + d).rem_euclid(list.len() as i32) as usize].clone();
        self.set(path, json!(to));
        self.cues.push(("tick", d));
        self.panels.redraw = true;
    }

    fn quick_keys(&mut self, dx: i32, dy: i32, enter: bool, esc: bool, shut: bool, now: f32) {
        let rows = quick_rows(self.panels.game.is_some());
        let row = rows[self.panels.row.min(rows.len() - 1)];
        if shut {
            return self.close_panel(now, false);
        }
        // inside a row's chips
        if let Some(sub) = self.panels.sub {
            let lines = self.panel_lines(rows[sub]);
            if esc {
                (self.panels.sub, self.panels.redraw) = (None, true);
                self.cues.push(("back", 1));
                return;
            }
            if dy != 0 {
                let to = self.panels.line as i32 + dy;
                if to < 0 {
                    (self.panels.sub, self.panels.redraw) = (None, true);
                } else if (to as usize) < lines.len() {
                    let (line, chips) = (&lines[to as usize], &lines[to as usize].1);
                    let cur = chips.iter().position(|c| c.0 == self.panel_value(line.0)).unwrap_or(0);
                    (self.panels.line, self.panels.chip, self.panels.redraw) = (to as usize, cur, true);
                }
                self.cues.push(("tick", dy));
            }
            if dx != 0 {
                let n = lines.get(self.panels.line).map_or(0, |l| l.1.len()) as i32;
                let to = self.panels.chip as i32 + dx;
                if to < 0 {
                    (self.panels.sub, self.panels.redraw) = (None, true);
                } else if to < n {
                    (self.panels.chip, self.panels.redraw) = (to as usize, true);
                    self.cues.push(("tick", dx));
                }
            }
            if enter {
                if let Some((name, chips)) = lines.get(self.panels.line) {
                    if let Some((v, label)) = chips.get(self.panels.chip) {
                        match *name {
                            "Sort by" => self.set(&["ui", "sort"], json!(v)),
                            "Show" => self.set(&["ui", "filter"], json!(v)),
                            _ => self.power(v, label, now),
                        }
                        self.cues.push(("tick", 1));
                        self.panels.redraw = true;
                    }
                }
            }
            return;
        }
        if esc {
            return self.close_panel(now, false);
        }
        if dy != 0 {
            let to = self.panels.row as i32 + dy;
            if to >= 0 && (to as usize) < rows.len() {
                (self.panels.row, self.panels.redraw) = (to as usize, true);
                self.asked.clear();
                self.cues.push(("tick", dy));
            }
        }
        if dx != 0 {
            match row {
                QRow::Acts => {
                    let to = self.panels.act as i32 + dx;
                    if (0..self.acts().len() as i32).contains(&to) {
                        (self.panels.act, self.panels.redraw) = (to as usize, true);
                        self.cues.push(("tick", dx));
                    }
                }
                QRow::Volume => {
                    let v = ((self.panel_volume() + f64::from(dx) * 0.05) * 100.0).round() / 100.0;
                    self.set(&["ui", "volume"], json!(v.clamp(0.0, 1.0)));
                    self.panels.redraw = true;
                }
                _ if dx > 0 => self.quick_pick(row, now),
                _ => {}
            }
        }
        if enter {
            self.quick_pick(row, now);
        }
    }

    /// The tiles of the actions row: Box art only for a game whose art can be chosen.
    fn acts(&self) -> Vec<&'static str> {
        let art = self.panels.game.as_ref().is_some_and(|g| matches!(g.game.source, Source::Rom | Source::Steam { .. } | Source::Pc(_)));
        ACTS.iter().copied().filter(|a| *a != "art" || art).collect()
    }

    fn quick_pick(&mut self, row: QRow, now: f32) {
        let rows = quick_rows(self.panels.game.is_some());
        match row {
            QRow::Acts => {
                let Some(g) = self.panels.game.clone() else { return };
                match self.acts().get(self.panels.act).copied() {
                    Some("fav") => {
                        let on = self.toggle_favorite(&g.game.id);
                        self.cues.push((if on { "toast" } else { "back" }, 1));
                        self.pulses.push((0.0, 0.3, 70));
                        self.panels.redraw = true;
                    }
                    Some("det") => {
                        self.close_panel(now, true);
                        self.wanted = Some(Screen::Details);
                    }
                    Some("art") => {
                        self.close_panel(now, true);
                        match &g.game.source {
                            Source::Rom => {
                                self.wanted = Some(Screen::Details);
                                self.then_art = true;
                            }
                            Source::Steam { appid, .. } => self.cover_asked = Some(crate::coverpick::Ask { id: g.game.id.clone(), appid: Some(appid.clone()), title: g.game.title.clone(), term: String::new() }),
                            _ => self.cover_asked = Some(crate::coverpick::Ask { id: g.game.id.clone(), appid: None, title: g.game.title.clone(), term: String::new() }),
                        }
                    }
                    Some("hide") => {
                        self.flip("hidden", &g.game.id);
                        self.pulses.push((0.0, 0.25, 60));
                        self.close_panel(now, true);
                    }
                    _ => {}
                }
            }
            QRow::Find => {
                self.close_panel(now, true);
                self.open_find();
            }
            QRow::Volume => {
                let v = self.panel_volume();
                self.set(&["ui", "volume"], json!(if v > 0.0 { 0.0 } else { 0.7 }));
                self.panels.redraw = true;
            }
            QRow::Trophies => {
                self.close_panel(now, true);
                self.wanted = Some(Screen::Trophies);
            }
            QRow::Updates => {
                self.close_panel(now, true);
                self.tab = TABS.iter().position(|t| *t == "System").unwrap_or(0);
                self.wanted = Some(Screen::Root);
            }
            QRow::Settings => {
                self.close_panel(now, true);
                self.wanted = Some(Screen::Root);
            }
            QRow::Sort | QRow::Power => {
                let i = rows.iter().position(|r| *r == row).unwrap_or(0);
                if self.panels.sub == Some(i) {
                    self.panels.sub = None;
                } else {
                    let first = self.panel_lines(row).first().map_or(0, |(name, chips)| chips.iter().position(|c| c.0 == self.panel_value(name)).unwrap_or(0));
                    (self.panels.sub, self.panels.line, self.panels.chip) = (Some(i), 0, first);
                }
                self.panels.redraw = true;
                self.cues.push(("tick", 1));
            }
        }
    }

    /// A power choice: Exit at once; Sleep, Restart and Shut down after a second press.
    fn power(&mut self, kind: &str, label: &str, now: f32) {
        if kind == "quit" {
            self.close_panel(now, true);
            self.panels.quit = true;
            return;
        }
        if matches!(kind, "restart" | "shutdown") && !self.twice(&format!("power-{kind}"), &format!("Press again to {}", label.to_lowercase())) {
            self.panels.redraw = true;
            return;
        }
        self.close_panel(now, true);
        crate::padwatch::power(kind);
    }
}

/// The panels drawn, and moving: rebuilt when something in them changes; the card sliding, the menu sliding in and
/// out, the pill gliding and the whole fading, every frame.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn draw(
    mut commands: Commands,
    time: Res<Time>,
    mut s: ResMut<Settings>,
    mona: Res<crate::hud::MonaSans>,
    face: Res<crate::hud::Face>,
    covers: Res<crate::shelf::Covers>,
    roots: Query<(Entity, &PanelRoot)>,
    mut card: Query<&mut UiTransform, (With<SortCard>, Without<QuickPanel>)>,
    mut quick: Query<&mut UiTransform, (With<QuickPanel>, Without<SortCard>)>,
    mut pill: Query<(&mut Node, &mut Visibility), With<Pill>>,
    children: Query<&Children>,
    mut colours: motion::FadeColours,
) {
    let now = time.elapsed_secs();
    // closed and gone
    if let Some((w, at)) = s.panels.closing {
        let done = now - at >= if w == Which::Quick { QUICK_SLIDE } else { SORT_FADE };
        if done {
            s.panels.closing = None;
            s.panels.redraw = true;
        }
    }
    if std::mem::take(&mut s.panels.redraw) {
        for (e, _) in &roots {
            commands.entity(e).despawn();
        }
        let shown = s.panels.open.or(s.panels.closing.map(|c| c.0));
        if let Some(which) = shown {
            build(&mut commands, &s, which, &mona, &face, &covers);
        }
    }
    let (open, closing) = (s.panels.open, s.panels.closing);
    let k = |w: Which, dur: f32, curve: [f32; 4]| match (open, closing) {
        (Some(o), _) if o == w => motion::progress(now, s.panels.since, dur, curve).min(1.0),
        (_, Some((c, at))) if c == w => 1.0 - motion::progress(now, at, dur, curve).min(1.0),
        _ => 0.0,
    };
    let sort = k(Which::Sort, SORT_SLIDE, CARD);
    for mut t in &mut card {
        t.translation = Val2::px(0.0, -10.0 * (1.0 - sort));
    }
    let slide = k(Which::Quick, QUICK_SLIDE, PANEL);
    for mut t in &mut quick {
        t.translation = Val2::percent(104.0 * (1.0 - slide), 0.0);
    }
    let fade = k(Which::Sort, SORT_FADE, FADE);
    for (e, root) in &roots {
        let _ = root;
        if s.panels.closing.is_some_and(|c| c.0 == Which::Sort) || (open == Some(Which::Sort) && fade < 1.0) {
            motion::fade_tree(e, fade, &|_| 1.0, &children, &mut colours, &mut commands);
        }
    }
    let rect = s.panels.pill_placed.then(|| s.panels.pill.now(now, PILL, GLIDE));
    for (mut node, mut seen) in &mut pill {
        match rect {
            Some(r) => {
                (node.left, node.top, node.width, node.height) = (px(r.x), px(r.y), px(r.z), px(r.w));
                seen.set_if_neq(Visibility::Inherited);
            }
            None => {
                seen.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

/// Once laid out: the quick menu's lit row, for the pill to glide to.
pub fn measure(time: Res<Time>, mut s: ResMut<Settings>, list: Query<(&ComputedNode, &UiGlobalTransform), With<QuickList>>, row: Query<(&ComputedNode, &UiGlobalTransform), With<PillRow>>) {
    if s.panels.open != Some(Which::Quick) {
        return;
    }
    let now = time.elapsed_secs();
    let (Ok((l, lt)), Ok((r, rt))) = (list.single(), row.single()) else {
        s.panels.pill_placed = false;
        return;
    };
    let inv = l.inverse_scale_factor;
    let at = rt.translation - r.size / 2.0 - (lt.translation - l.size / 2.0);
    let rect = Vec4::new(at.x, at.y, r.size.x, r.size.y) * inv;
    if !s.panels.pill_placed {
        s.panels.pill.snap(rect, now);
        s.panels.pill_placed = true;
    } else {
        s.panels.pill.go(rect, now, PILL, GLIDE);
    }
}

#[derive(Component)]
pub struct QuickList;

const INK: Color = Color::srgb(0.953, 0.961, 0.984);
const DIM: Color = Color::srgba(0.953, 0.961, 0.984, 0.62);
const DARK: Color = Color::srgb(0.063, 0.071, 0.102);
const LIGHT: Color = Color::srgb(0.922, 0.922, 0.941);

fn build(commands: &mut Commands, s: &Settings, which: Which, mona: &crate::hud::MonaSans, face: &crate::hud::Face, covers: &crate::shelf::Covers) {
    let lang = s.language();
    let t = |x: &str| rustyshelf_core::i18n::tr(&lang, x);
    let text = |x: String, weight: u32, size: f32, color: Color| (Text::new(x), mona.font(weight, size), TextColor(color));
    let chip = |p: &mut ChildSpawnerCommands, label: String, cur: bool, lit: bool| {
        let mut c = p.spawn((
            Node { padding: UiRect::axes(px(14), px(7)), border: UiRect::all(px(1)), border_radius: BorderRadius::MAX, ..default() },
            BackgroundColor(if lit { LIGHT } else { Color::srgba(1.0, 1.0, 1.0, 0.06) }),
            BorderColor::all(if cur && !lit { Color::srgba(1.0, 1.0, 1.0, 0.16) } else { Color::NONE }),
        ));
        if lit {
            c.insert(BoxShadow(vec![ShadowStyle { color: Color::srgba(0.47, 0.55, 1.0, 0.45), x_offset: px(0), y_offset: px(6), spread_radius: px(-8), blur_radius: px(22) }]));
        }
        c.with_children(|c| {
            c.spawn(text(label, 500, 13.0, if lit { DARK } else if cur { INK } else { DIM }));
        });
    };
    match which {
        Which::Sort => {
            commands
                .spawn((
                    PanelRoot,
                    Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::FlexStart, padding: UiRect::top(px(96)), ..default() },
                    BackgroundGradient::from(LinearGradient::to_bottom(vec![ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.55), percent(0)), ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.15), percent(55)), ColorStop::new(Color::NONE, percent(100))])),
                    GlobalZIndex(9),
                ))
                .with_children(|r| {
                    r.spawn((
                        SortCard,
                        UiTransform::from_translation(Val2::px(0.0, -10.0)),
                        Node { width: px(880), max_width: percent(92), flex_direction: FlexDirection::Column, padding: UiRect::new(px(20), px(20), px(18), px(14)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(20)), ..default() },
                        BackgroundColor(Color::srgba(14.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.9)),
                        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
                        BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.5), x_offset: px(0), y_offset: px(30), spread_radius: px(0), blur_radius: px(80) }]),
                    ))
                    .with_children(|c| {
                        c.spawn((Node { margin: UiRect::bottom(px(12)), ..default() }, text(super::upper(&lang, &t("Arrange your shelves")), 600, 12.0, DIM), TextLayout::default()));
                        for (i, (name, chips)) in [("Sort by", SORTS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>()), ("Show", s.filters.clone())].into_iter().enumerate() {
                            let active = i == s.panels.sort_row;
                            let cur = s.panel_value(name);
                            c.spawn((Node { align_items: AlignItems::Center, column_gap: px(14), padding: UiRect::axes(px(10), px(8)), border_radius: BorderRadius::all(px(14)), ..default() }, BackgroundColor(if active { Color::srgba(1.0, 1.0, 1.0, 0.05) } else { Color::NONE })))
                                .with_children(|row| {
                                    row.spawn((Node { width: px(64), flex_shrink: 0.0, ..default() }, text(t(name), 400, 12.0, DIM)));
                                    row.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }).with_children(|opts| {
                                        for (v, label) in &chips {
                                            let on = *v == cur;
                                            chip(opts, t(label), on, on && active);
                                        }
                                    });
                                });
                        }
                        let foot = match s.panels.game.as_ref() {
                            _ if s.panels.home => t("Home keeps its own order.") + " ",
                            Some(g) => format!("{} · ", t(&format!("on {}", g.shelf_name))),
                            None => String::new(),
                        };
                        c.spawn((Node { margin: UiRect::new(px(10), px(10), px(10), px(0)), ..default() }, text(format!("{foot}{}", t("Z / C (or L1 / R1) jump between sections · applies to every shelf")), 400, 11.0, DIM)));
                    });
                });
        }
        Which::Quick => {
            let rows = quick_rows(s.panels.game.is_some());
            commands
                .spawn((PanelRoot, Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::FlexEnd, ..default() }, GlobalZIndex(9)))
                .with_children(|r| {
                    r.spawn((
                        QuickPanel,
                        UiTransform::from_translation(Val2::percent(104.0, 0.0)),
                        Node { width: px(460), max_width: percent(100), height: percent(100), flex_direction: FlexDirection::Column, padding: UiRect::new(px(24), px(24), px(34), px(0)), border: UiRect::left(px(1)), ..default() },
                        // (the glass is blurred in the Electron app; with no blur here it's nearly opaque)
                        BackgroundColor(Color::srgba(10.0 / 255.0, 12.0 / 255.0, 20.0 / 255.0, 0.96)),
                        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.1)),
                        BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.5), x_offset: px(-30), y_offset: px(0), spread_radius: px(0), blur_radius: px(80) }]),
                    ))
                    .with_children(|q| {
                        // the game
                        if let Some(g) = &s.panels.game {
                            q.spawn(Node { column_gap: px(16), align_items: AlignItems::Center, padding: UiRect::new(px(2), px(2), px(0), px(16)), ..default() }).with_children(|top| {
                                let mut cov = top.spawn((Node { width: px(50), height: px(68), flex_shrink: 0.0, border_radius: BorderRadius::all(px(5)), overflow: Overflow::clip(), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.06))));
                                if let Some(h) = covers.of(&g.game, &g.console) {
                                    cov.with_children(|c| {
                                        c.spawn((Node { width: percent(100), height: percent(100), ..default() }, ImageNode::new(h)));
                                    });
                                }
                                top.spawn(Node { flex_direction: FlexDirection::Column, min_width: px(0), row_gap: px(4), ..default() }).with_children(|tx| {
                                    tx.spawn(text(super::upper(&lang, &g.shelf_name), 500, 11.0, DIM));
                                    tx.spawn(text(g.game.title.clone(), 600, 23.0, INK));
                                    let meta = match &g.played {
                                        Some(p) if p.ms > 0 || p.last > 0 => [(p.ms > 0).then(|| format!("{} {}", played_for(p.ms), t("played"))), (p.last > 0).then(|| format!("{} {}", t("last played"), last_played(p.last)))].into_iter().flatten().collect::<Vec<_>>().join(" · "),
                                        _ => t("Not played yet"),
                                    };
                                    tx.spawn(text(meta, 400, 13.0, DIM));
                                });
                            });
                        }
                        q.spawn((QuickList, Node { flex_direction: FlexDirection::Column, row_gap: px(2), padding: UiRect::new(px(0), px(0), px(8), px(24)), ..default() })).with_children(|list| {
                            list.spawn((Pill, Node { position_type: PositionType::Absolute, border_radius: BorderRadius::all(px(25)), ..default() }, BackgroundColor(LIGHT), Visibility::Hidden));
                            for (i, row) in rows.iter().enumerate() {
                                let lit = i == s.panels.row && s.panels.sub.is_none();
                                if *row == QRow::Updates || (*row == QRow::Settings && !rows.contains(&QRow::Updates)) {
                                    list.spawn((Node { height: px(1), margin: UiRect::axes(px(14), px(10)), flex_shrink: 0.0, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08))));
                                }
                                if *row == QRow::Acts {
                                    let fav = s.panels.game.as_ref().is_some_and(|g| s.list(&["ui", "favorites"]).contains(&g.game.id));
                                    list.spawn(Node { column_gap: px(8), padding: UiRect::bottom(px(14)), ..default() }).with_children(|acts| {
                                        for (j, a) in s.acts().iter().enumerate() {
                                            let on = lit && j == s.panels.act;
                                            let (sign, label) = match *a {
                                                "fav" => ("♥", if fav { "Favorited" } else { "Favorite" }),
                                                "det" => ("≡", "Details"),
                                                "art" => ("▣", "Box art"),
                                                _ => ("⊘", "Hide"),
                                            };
                                            let mut tile = acts.spawn((
                                                Node { flex_grow: 1.0, flex_basis: px(0), min_width: px(0), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(7), padding: UiRect::new(px(0), px(0), px(12), px(10)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(16)), ..default() },
                                                BackgroundColor(if on { LIGHT } else { Color::NONE }),
                                                BorderColor::all(if on { Color::NONE } else { Color::srgba(1.0, 1.0, 1.0, 0.16) }),
                                            ));
                                            if on {
                                                tile.insert((UiTransform::from_translation(Val2::px(0.0, -2.0)), BoxShadow(vec![ShadowStyle { color: Color::srgba(0.47, 0.55, 1.0, 0.55), x_offset: px(0), y_offset: px(8), spread_radius: px(-10), blur_radius: px(30) }])));
                                            }
                                            tile.with_children(|tl| {
                                                let colour = if *a == "fav" && fav { Color::srgb(1.0, 0.36, 0.54) } else if on { DARK } else { INK };
                                                tl.spawn((Text::new(sign), TextFont::from_font_size(bevy::text::FontSize::Px(20.0)).with_font(face.0.clone()), TextColor(colour)));
                                                tl.spawn(text(t(label), 400, 12.0, if on { DARK } else { DIM }));
                                            });
                                        }
                                    });
                                    continue;
                                }
                                let (label, value, more) = match row {
                                    QRow::Find => ("Find a game", t("Search every shelf"), true),
                                    QRow::Volume => ("Volume", String::new(), false),
                                    QRow::Sort => ("Sort & filter", {
                                        let sort = SORTS.iter().find(|x| x.0 == s.panel_value("Sort by")).map_or("", |x| x.1);
                                        let filter = s.filters.iter().find(|f| f.0 == s.panel_value("Show")).filter(|f| f.0 != "all").map(|f| format!(" · {}", t(&f.1))).unwrap_or_default();
                                        format!("{}{filter}", t(sort))
                                    }, true),
                                    QRow::Trophies => ("Trophies", String::new(), true),
                                    QRow::Updates => ("Updates", t("Check for a newer version"), true),
                                    QRow::Settings => ("Settings", String::new(), true),
                                    QRow::Power => ("Power", if s.asked.is_empty() { String::new() } else { t(&s.note) }, true),
                                    QRow::Acts => unreachable!(),
                                };
                                let mut line = list.spawn(Node { height: px(50), flex_shrink: 0.0, align_items: AlignItems::Center, column_gap: px(14), padding: UiRect::horizontal(px(18)), ..default() });
                                if lit {
                                    line.insert(PillRow);
                                }
                                let ink = if lit { DARK } else { INK };
                                line.with_children(|l| {
                                    l.spawn((text(t(label), 500, 15.0, ink), TextLayout::no_wrap(), Node { flex_shrink: 0.0, ..default() }));
                                    if *row == QRow::Volume {
                                        let v = (s.panel_volume() * 100.0).round() as i32;
                                        l.spawn((Node { flex_grow: 1.0, height: px(4), border_radius: BorderRadius::all(px(2)), margin: UiRect::horizontal(px(4)), ..default() }, BackgroundColor(if lit { Color::srgba(0.063, 0.071, 0.102, 0.18) } else { Color::srgba(1.0, 1.0, 1.0, 0.18) })))
                                            .with_children(|bar| {
                                                bar.spawn((Node { width: percent(v as f32), height: percent(100), border_radius: BorderRadius::all(px(2)), ..default() }, BackgroundColor(ink)));
                                            });
                                        l.spawn((Node { min_width: px(44), justify_content: JustifyContent::FlexEnd, ..default() }, text(if v > 0 { format!("{v}%") } else { t("Muted") }, 400, 13.0, if lit { DARK } else { DIM })));
                                    } else {
                                        l.spawn((Node { margin: UiRect::left(auto()), min_width: px(0), ..default() }, text(value, 400, 13.0, if lit { Color::srgba(0.063, 0.071, 0.102, 0.62) } else { DIM }), TextLayout::no_wrap()));
                                        if more {
                                            l.spawn(text("›".into(), 400, 18.0, if lit { Color::srgba(0.063, 0.071, 0.102, 0.5) } else { Color::srgba(0.953, 0.961, 0.984, 0.38) }));
                                        }
                                    }
                                });
                                // the row opened into its chips
                                if s.panels.sub == Some(i) {
                                    list.spawn(Node { flex_direction: FlexDirection::Column, row_gap: px(12), padding: UiRect::new(px(50), px(8), px(6), px(12)), ..default() }).with_children(|sub| {
                                        for (li, (name, chips)) in s.panel_lines(*row).into_iter().enumerate() {
                                            let cur = s.panel_value(name);
                                            sub.spawn(Node { flex_direction: FlexDirection::Column, ..default() }).with_children(|ln| {
                                                if !name.is_empty() {
                                                    ln.spawn((Node { margin: UiRect::new(px(2), px(0), px(0), px(7)), ..default() }, text(super::upper(&lang, &t(name)), 500, 11.0, Color::srgba(0.953, 0.961, 0.984, 0.42))));
                                                }
                                                ln.spawn(Node { flex_wrap: FlexWrap::Wrap, column_gap: px(6), row_gap: px(6), ..default() }).with_children(|opts| {
                                                    for (ci, (v, label)) in chips.iter().enumerate() {
                                                        chip(opts, t(label), *v == cur, li == s.panels.line && ci == s.panels.chip);
                                                    }
                                                });
                                            });
                                        }
                                    });
                                }
                            }
                        });
                        q.spawn(Node { flex_grow: 1.0, ..default() });
                        // the keys
                        let style = (s.glyphs(), s.swap());
                        let (ok, back) = (rustyshelf_core::pads::button_word(true, style.0, style.1), rustyshelf_core::pads::button_word(false, style.0, style.1));
                        let mut keys = vec![(t("Move"), "↑↓".to_string())];
                        if rows.get(s.panels.row) == Some(&QRow::Volume) && s.panels.sub.is_none() {
                            keys.push((t("Volume"), "←→".into()));
                        }
                        keys.push((t(if s.panels.sub.is_some() { "Select" } else { "Open" }), format!("Enter {ok}")));
                        keys.push((t(if s.panels.sub.is_some() { "Back" } else { "Close" }), format!("Esc {back}")));
                        q.spawn((Node { align_self: AlignSelf::Center, column_gap: px(14), padding: UiRect::axes(px(14), px(8)), margin: UiRect::new(px(0), px(0), px(12), px(24)), border: UiRect::all(px(1)), border_radius: BorderRadius::MAX, ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)), BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16))))
                            .with_children(|k| {
                                for (label, caps) in keys {
                                    k.spawn((Text::new(format!("{caps}  {label}")), TextFont::from_font_size(bevy::text::FontSize::Px(12.0)).with_font(face.0.clone()), TextColor(DIM)));
                                }
                            });
                    });
                });
        }
    }
}

/// How long a game was played, as the menu says it: "2 h 5 min", "40 min".
fn played_for(ms: u64) -> String {
    let min = ms / 60_000;
    if min >= 60 {
        format!("{} h {} min", min / 60, min % 60)
    } else {
        format!("{} min", min.max(1))
    }
}

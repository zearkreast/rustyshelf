// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the PC cover
// picker of Boxshelf's renderer/app.js (pcCoverOpen) and renderer/style.css (.coverpick), as RustyShelf has it.
//! Choosing a Steam, store or added game's cover: a card over the shelf with a grid of covers — Steam's own, the
//! Steam store's games of that name, SteamGridDB's posters (with a key) — and tiles to search under another name,
//! use a picture of your own, or go back to the automatic cover. The arrows move through the grid, Enter / A uses a
//! tile, Esc / B closes. The covers are looked up, their thumbnails fetched and a chosen one downloaded off the
//! frame.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::prelude::*;
use rustyshelf_core::pccovers::{self, CoverOption};

use crate::hud::MonaSans;
use crate::loading::to_image;
use crate::motion::{self, Ease, OUT};

/// The game a picker is asked for: its id, a Steam game's appid, its title, and a title typed in to search under.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    pub id: String,
    pub appid: Option<String>,
    pub title: String,
    pub term: String,
}

/// tiles to a row
const COLS: usize = 7;
/// the thumbnails' height, in pixels
const THUMB_H: u32 = 300;

enum Act {
    Search,
    Own,
    Reset,
    SteamGrid,
    Cover(Box<CoverOption>),
}

struct Item {
    act: Act,
    label: String,
    sub: String,
    icon: &'static str,
    thumb: Option<Handle<Image>>,
}

/// What a worker sends back: the covers found (with their thumbnails decoded), or a cover put in place.
type Found = (pccovers::Options, Vec<Option<(u32, u32, Vec<u8>)>>);

struct Open {
    ask: Ask,
    items: Vec<Item>,
    at: usize,
    note: String,
    finding: Option<Mutex<Receiver<Found>>>,
    using: Option<Mutex<Receiver<Result<PathBuf, String>>>>,
    built: bool,
    since: f32,
}

#[derive(Resource, Default)]
pub struct CoverPick {
    open: Option<Open>,
    /// a cover changed: the library is read again (as soon as it isn't being read already)
    reload: bool,
    /// the grid's scroll, eased to keep the lit tile in view
    scroll: Ease<f32>,
}

#[derive(Component)]
pub struct CoverRoot;

#[derive(Component)]
pub struct CoverGrid;

#[derive(Component)]
pub struct LitTile;

/// The card, coming in.
#[derive(Component)]
pub struct CardIn;

/// seconds: the card coming in (style.css .buildpick, .bp-card)
const IN: f32 = 0.3;

fn thumbs(list: &[CoverOption]) -> Vec<Option<(u32, u32, Vec<u8>)>> {
    list.iter()
        .map(|c| {
            let img = image::ImageReader::open(&c.thumb).ok()?.with_guessed_format().ok()?.decode().ok()?;
            let img = if img.height() > THUMB_H { img.resize(u32::MAX, THUMB_H, image::imageops::FilterType::Triangle) } else { img };
            let rgba = img.into_rgba8();
            Some((rgba.width(), rgba.height(), rgba.into_raw()))
        })
        .collect()
}

fn start(ask: Ask, data: PathBuf, now: f32) -> Open {
    let (tx, rx) = channel();
    let job = ask.clone();
    std::thread::spawn(move || {
        let sg = rustyshelf_core::steamgrid::SteamGrid::from_settings(&data);
        let found = pccovers::options(&data, &job.id, job.appid.as_deref(), if job.term.is_empty() { &job.title } else { &job.term }, !job.term.is_empty(), sg.as_ref());
        let t = thumbs(&found.list);
        let _ = tx.send((found, t));
    });
    Open { ask, items: Vec::new(), at: 0, note: "Looking for covers…".into(), finding: Some(Mutex::new(rx)), using: None, built: false, since: now }
}

/// What the note says once the covers are in (app.js pcCoverOpen).
fn note_for(found: &pccovers::Options, term: &str) -> String {
    let n = found.list.iter().filter(|c| c.source == "steamgriddb").count();
    let ns = found.list.iter().filter(|c| matches!(c.source, "steamstore" | "steam")).count();
    let s = |k: usize| if k == 1 { "" } else { "s" };
    if n > 0 {
        format!("{n} cover{} from SteamGridDB{}{}.", s(n), if term.is_empty() { String::new() } else { format!(" for \"{term}\"") }, if ns > 0 { ", and Steam's own" } else { "" })
    } else if !term.is_empty() {
        if ns > 0 {
            format!("Steam's cover{} for \"{term}\".{}", s(ns), if found.steamgriddb { " SteamGridDB has none." } else { "" })
        } else {
            format!("Nothing found for \"{term}\". Try another name.")
        }
    } else if found.steamgriddb {
        if ns > 0 { "Steam's own cover. SteamGridDB has none for this game." } else { "SteamGridDB has no covers for this game. Search by name, or use a picture of yours." }.into()
    } else if ns > 0 {
        "Steam's own cover. For dozens more, add a free SteamGridDB key.".into()
    } else {
        "No cover under this name on Steam. Search by name, use a picture of yours, or add a free SteamGridDB key for dozens more.".into()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut pick: ResMut<CoverPick>,
    mut settings: ResMut<crate::settings::Settings>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut toast: ResMut<crate::hud::Toast>,
    mut covers: ResMut<crate::shelf::Covers>,
    mut images: ResMut<Assets<Image>>,
    (mona, face): (Res<MonaSans>, Res<crate::hud::Face>),
    roots: Query<Entity, With<CoverRoot>>,
    mut refresh: ResMut<crate::loading::Refresh>,
) {
    let now = time.elapsed_secs();
    let root = rustyshelf_core::paths::app_root();
    let data = rustyshelf_core::paths::data_dir(&root);
    if pick.reload && crate::loading::read_again(&mut refresh, &root) {
        pick.reload = false;
    }
    if let Some(ask) = settings.cover_asked.take() {
        sounds.play("flip", 1);
        pick.open = Some(start(ask, data.clone(), now));
        pick.scroll.snap(0.0, now);
    }
    let Some(open) = pick.open.as_mut() else {
        for r in &roots {
            commands.entity(r).despawn();
        }
        return;
    };
    settings.keys_taken = true;
    // the covers found
    if let Some(found) = open.finding.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        let (found, decoded) = found;
        open.finding = None;
        open.note = note_for(&found, &open.ask.term);
        let tile = |act, label: &str, sub: String, icon| Item { act, label: label.into(), sub, icon, thumb: None };
        let mut items = vec![tile(Act::Search, "Search by name…", if open.ask.term.is_empty() { "Another name to look up".into() } else { format!("Now: {}", open.ask.term) }, "Aa")];
        // (Android picks files through its own screens, which this doesn't reach yet)
        if cfg!(not(target_os = "android")) {
            items.push(tile(Act::Own, "A picture of yours", "JPG, PNG or WebP".into(), "+"));
        }
        if found.chosen {
            items.push(tile(Act::Reset, "Automatic cover", "Go back to it".into(), "↺"));
        }
        let first_cover = items.len() + usize::from(found.list.first().is_some_and(|c| c.source == "yours"));
        for (c, d) in found.list.into_iter().zip(decoded) {
            let thumb = d.map(|(w, h, data)| images.add(to_image(w, h, data)));
            items.push(Item { label: c.label.clone(), sub: c.sub.clone(), icon: "", thumb, act: Act::Cover(Box::new(c)) });
        }
        if !found.steamgriddb {
            items.push(tile(Act::SteamGrid, "Add SteamGridDB", "Dozens more covers · free key".into(), "+"));
        }
        open.at = if first_cover < items.len() && matches!(items[first_cover].act, Act::Cover(_)) { first_cover } else { 0 };
        open.items = items;
        open.built = false;
    }
    // a cover put in place
    if let Some(done) = open.using.as_ref().and_then(|rx| rx.lock().ok()?.try_recv().ok()) {
        open.using = None;
        match done {
            Ok(path) => {
                covers.forget(&path);
                sounds.play("toast", 1);
                (toast.text, toast.until) = (format!("New cover for {}", open.ask.title), now + 4.0);
                pick.reload = true;
                pick.open = None;
                return;
            }
            // (the file dialog closed without a picture)
            Err(e) if e.is_empty() => open.built = false,
            Err(e) => {
                sounds.play("error", 1);
                (open.note, open.built) = (e, false);
            }
        }
    }
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let (ok, back) = settings.confirm();
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(back) {
        sounds.play("back", 1);
        pick.open = None;
        return;
    }
    let n = open.items.len();
    let dx = (keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32;
    let dy = (keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32;
    if (dx != 0 || dy != 0) && n > 0 && open.using.is_none() {
        let to = (open.at as i32 + dx + dy * COLS as i32).clamp(0, n as i32 - 1) as usize;
        if to != open.at {
            open.at = to;
            open.built = false;
            sounds.play("tick", dx + dy);
        }
    }
    if (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) || pad(ok)) && open.using.is_none() && open.finding.is_none() {
        if let Some(item) = open.items.get(open.at) {
            let ask = open.ask.clone();
            match &item.act {
                Act::Search => {
                    sounds.play("panel", 1);
                    settings.search_cover(ask);
                    pick.open = None;
                    return;
                }
                Act::SteamGrid => {
                    sounds.play("panel", 1);
                    settings.open_account();
                    pick.open = None;
                    return;
                }
                Act::Own => {
                    // (the file dialog waits for the player: off the frame, as a cover being downloaded is)
                    let (tx, rx) = channel();
                    let (id, data) = (ask.id.clone(), data.clone());
                    std::thread::spawn(move || {
                        let got = match crate::dialog::pick_file("Choose a cover", false) {
                            Some(file) => pccovers::own(&data, &id, &file),
                            None => Err(String::new()),
                        };
                        let _ = tx.send(got);
                    });
                    open.using = Some(Mutex::new(rx));
                }
                Act::Reset => {
                    let gone = pccovers::cover(&data, &ask.id);
                    pccovers::clear(&data, &ask.id);
                    if let Some(g) = gone {
                        covers.forget(&g);
                    }
                    sounds.play("toast", 1);
                    (toast.text, toast.until) = (format!("{} is back on its automatic cover", ask.title), now + 4.0);
                    pick.reload = true;
                    pick.open = None;
                    return;
                }
                Act::Cover(c) => {
                    let (tx, rx) = channel();
                    let (c, id, data) = ((**c).clone(), ask.id.clone(), data.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(pccovers::choose(&data, &id, &c));
                    });
                    open.using = Some(Mutex::new(rx));
                    (open.note, open.built) = ("Downloading the cover…".into(), false);
                }
            }
        }
    }
    if open.built {
        return;
    }
    open.built = true;
    for r in &roots {
        commands.entity(r).despawn();
    }
    build(&mut commands, open, &mona, &face, &settings, now);
}

fn build(commands: &mut Commands, open: &Open, mona: &MonaSans, face: &crate::hud::Face, settings: &crate::settings::Settings, now: f32) {
    let lang = settings.language();
    let t = |s: &str| rustyshelf_core::i18n::tr(&lang, s);
    let text = |s: String, weight: u32, size: f32, color: Color| (Text::new(s), mona.font(weight, size), TextColor(color));
    let ink = Color::srgb(0.94, 0.95, 0.98);
    let dim = Color::srgba(0.94, 0.95, 0.98, 0.6);
    let shown = motion::progress(now, open.since, IN, OUT);
    commands
        .spawn((
            CoverRoot,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            GlobalZIndex(30),
        ))
        .with_children(|r| {
            r.spawn((
                Node { width: px(980), max_width: percent(94), max_height: percent(84), flex_direction: FlexDirection::Column, padding: UiRect::new(px(18), px(18), px(20), px(14)), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(20)), ..default() },
                BackgroundColor(Color::srgba(14.0 / 255.0, 16.0 / 255.0, 24.0 / 255.0, 0.94)),
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.16)),
                BoxShadow(vec![ShadowStyle { color: Color::srgba(0.0, 0.0, 0.0, 0.55), x_offset: px(0), y_offset: px(30), spread_radius: px(0), blur_radius: px(80) }]),
                UiTransform { translation: Val2::px(0.0, 10.0 * (1.0 - shown)), scale: Vec2::splat(0.98 + 0.02 * shown), ..default() },
                CardIn,
            ))
            .with_children(|card| {
                card.spawn((Node { padding: UiRect::horizontal(px(8)), ..default() }, text(t("Choose a cover").to_uppercase(), 600, 12.0, dim)));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(4), px(8)), ..default() }, text(open.ask.title.clone(), 600, 20.0, ink)));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(2), px(10)), ..default() }, text(t(&open.note), 400, 13.0, dim)));
                card.spawn((
                    CoverGrid,
                    Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::flex(COLS as u16, 1.0),
                        column_gap: px(12),
                        row_gap: px(12),
                        padding: UiRect::new(px(8), px(8), px(6), px(6)),
                        overflow: Overflow::scroll_y(),
                        min_height: px(0),
                        ..default()
                    },
                ))
                .with_children(|grid| {
                    for (i, it) in open.items.iter().enumerate() {
                        let lit = i == open.at;
                        let mut tile = grid.spawn((
                            Node { flex_direction: FlexDirection::Column, row_gap: px(6), padding: UiRect::all(px(6)), border_radius: BorderRadius::all(px(14)), min_width: px(0), ..default() },
                            BackgroundColor(if lit { Color::srgba(1.0, 1.0, 1.0, 0.92) } else { Color::srgba(1.0, 1.0, 1.0, 0.04) }),
                        ));
                        if lit {
                            tile.insert((LitTile, UiTransform::from_translation(Val2::px(0.0, -2.0)), BoxShadow(vec![ShadowStyle { color: Color::srgba(0.47, 0.55, 1.0, 0.4), x_offset: px(0), y_offset: px(0), spread_radius: px(2), blur_radius: px(18) }])));
                        }
                        tile.with_children(|tile| {
                            let mut im = tile.spawn((
                                Node { width: percent(100), aspect_ratio: Some(2.0 / 3.0), border_radius: BorderRadius::all(px(9)), justify_content: JustifyContent::Center, align_items: AlignItems::Center, overflow: Overflow::clip(), ..default() },
                                BackgroundColor(Color::srgb(0.043, 0.051, 0.086)),
                            ));
                            match &it.thumb {
                                Some(h) => {
                                    im.with_children(|im| {
                                        im.spawn((Node { width: percent(100), height: percent(100), ..default() }, ImageNode::new(h.clone())));
                                    });
                                }
                                None => {
                                    if matches!(it.act, Act::SteamGrid) {
                                        im.insert((BorderColor::all(Color::srgba(0.42, 0.55, 1.0, 0.75)), BackgroundColor(Color::srgba(0.42, 0.55, 1.0, 0.1))));
                                    }
                                    // (the tiles' signs aren't in Mona Sans)
                                    im.with_children(|im| {
                                        im.spawn((Text::new(it.icon), TextFont::from_font_size(bevy::text::FontSize::Px(34.0)).with_font(face.0.clone()), TextColor(if lit { Color::srgb(0.24, 0.26, 0.33) } else { dim })));
                                    });
                                }
                            }
                            let dark = Color::srgb(0.063, 0.071, 0.102);
                            tile.spawn(text(t(&it.label), 600, 12.0, if lit { dark } else { ink }));
                            tile.spawn(text(t(&it.sub), 400, 11.0, if lit { Color::srgb(0.23, 0.25, 0.32) } else { dim }));
                        });
                    }
                });
                let (a, b) = (rustyshelf_core::pads::button_word(true, settings.glyphs(), settings.swap()), rustyshelf_core::pads::button_word(false, settings.glyphs(), settings.swap()));
                card.spawn((Node { padding: UiRect::new(px(8), px(8), px(10), px(0)), ..default() }, text(format!("{a} {} · {b} {}", t("use"), t("back")), 400, 11.0, dim)));
            });
        });
}

/// Once laid out: the grid scrolled to keep the lit tile in view.
pub fn scroll(time: Res<Time>, mut pick: ResMut<CoverPick>, mut grid: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition), With<CoverGrid>>, lit: Query<(&ComputedNode, &UiGlobalTransform), With<LitTile>>) {
    let now = time.elapsed_secs();
    let (Ok((g, gt, mut at)), Ok((l, lt))) = (grid.single_mut(), lit.single()) else { return };
    let inv = g.inverse_scale_factor;
    let top = ((lt.translation.y - l.size.y / 2.0) - (gt.translation.y - g.size.y / 2.0) + g.scroll_position.y) * inv;
    let (h, view) = (l.size.y * inv, g.size.y * inv);
    let mut to = pick.scroll.target();
    if top - 12.0 < to {
        to = top - 12.0;
    }
    if top + h + 12.0 > to + view {
        to = top + h + 12.0 - view;
    }
    pick.scroll.go(to.max(0.0), now, 0.22, OUT);
    let y = pick.scroll.now(now, 0.22, OUT);
    if at.0.y != y {
        at.0.y = y;
    }
}

/// The card coming in: sliding up and growing to its size, and fading in with what's behind it dimmed.
pub fn animate(time: Res<Time>, pick: Res<CoverPick>, mut card: Query<&mut UiTransform, With<CardIn>>, roots: Query<Entity, With<CoverRoot>>, children: Query<&Children>, mut colours: motion::FadeColours, mut commands: Commands, mut faded: Local<bool>) {
    let Some(open) = pick.open.as_ref() else {
        *faded = false;
        return;
    };
    let now = time.elapsed_secs();
    let k = motion::progress(now, open.since, IN, OUT).min(1.0);
    for mut t in &mut card {
        t.translation = Val2::px(0.0, 10.0 * (1.0 - k));
        t.scale = Vec2::splat(0.98 + 0.02 * k);
    }
    if k < 1.0 || !*faded {
        *faded = k >= 1.0;
        for r in &roots {
            motion::fade_tree(r, k, &|_| 1.0, &children, &mut colours, &mut commands);
        }
    }
}

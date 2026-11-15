// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the museum of
// Boxshelf's renderer/museum.js (its classic wall), as RustyShelf has it.
//! The museum: the whole collection as one tall shelving wall, a shelf row per console with every game standing
//! spine out, lit from above. Up and down pick a row, left and right a case, which slides out and turns its cover to
//! you; Enter plays it, Escape goes back to the shelf. M opens it from the shelf.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::ClearColorConfig;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rustyshelf_core::library::ShelfKind;
use rustyshelf_core::looks;
use rustyshelf_core::scanner::Source;

use crate::loading::{Art, ArtLoader, LibraryRes, LibraryState};
use crate::shelf::{color, cover_of, Covers, Cursor};

/// The layer the museum is drawn on: the shelf's camera never sees it.
const LAYER: usize = 3;
/// A case is 1 tall; a row's height, the shelf's depth from the wall, a plank's thickness, the gap between cases.
const ROW: f32 = 1.34;
const DEPTH: f32 = 0.8;
const PLANK: f32 = 0.055;
const GAP: f32 = 0.012;
/// a picked case comes out by this share of its width, plus its thickness
const PULL: f32 = 0.62;
/// how long View / Minus is held on the shelf to open the museum, in seconds
const HOLD: f32 = 0.55;
/// rows around the one in view whose spines are painted (a big library's every spine would fill the graphics card)
const NEAR: usize = 2;

#[derive(Component)]
pub struct MuseumThing;

#[derive(Component)]
pub struct MuseumCamera;

#[derive(Component)]
pub struct MuseumText;

/// The text under the wall, kept when a room is rebuilt.
#[derive(Component)]
pub struct MuseumHud;

/// The card beside the pulled case.
#[derive(Component)]
pub struct MuseumCard;

/// The narrow beam that follows the chosen case.
#[derive(Component)]
pub struct CaseSpot;

/// One side of the wide view's frame: top, bottom, left, right.
#[derive(Component)]
pub struct FrameSide(u8);

/// The card's scale on the shelf.
const CARD_S: f32 = 1.16;

/// The card's parts, its one picture (each new selection is drawn over it), and how far it has faded in.
struct CardParts {
    entity: Entity,
    face: Handle<StandardMaterial>,
    board: Handle<StandardMaterial>,
    image: Handle<Image>,
    /// the case it was last asked to show, and the one its picture shows
    asked: Option<(usize, usize)>,
    shows: Option<(usize, usize)>,
    drawing: Option<std::sync::Mutex<std::sync::mpsc::Receiver<((usize, usize), rustyshelf_core::drawn::Picture)>>>,
    fade: f32,
}

/// The picture light over a row: the row.
#[derive(Component)]
pub struct RowLight(usize);

type Cam<'w, 's> = Query<'w, 's, (&'static mut Camera, &'static mut Transform, &'static Projection), (With<MuseumCamera>, Without<Case>, Without<RowLight>)>;

/// A picture light's strength for each unit of a scene's lamp, and the ambient light's for each of its.
const LAMP_UNIT: f32 = 16_250.0;
const AMB_UNIT: f32 = 1_180.0;

/// A lighting scene: the picture lamps' colour and strength, the room's ambient colour and strength, the dark beyond
/// the room, and whether each row is lit in its console's own colour.
#[derive(Clone, Copy)]
struct Scene {
    lamp: u32,
    lamp_i: f32,
    /// the narrow beam on the chosen case
    spot: u32,
    spot_i: f32,
    amb: f32,
    amb_c: u32,
    bg: u32,
    hue: bool,
}

// keep in sync with settings.rs's Museum page and renderer/museum.js LIGHTS
const SCENES: [(&str, Scene); 6] = [
    ("gallery", Scene { lamp: 0xffe0b0, lamp_i: 16.0, spot: 0xfff1dc, spot_i: 5.0, amb: 0.22, amb_c: 0xaab0c8, bg: 0x05060a, hue: false }),
    ("daylight", Scene { lamp: 0xf4f7ff, lamp_i: 13.0, spot: 0xffffff, spot_i: 3.0, amb: 0.58, amb_c: 0xe4ecff, bg: 0x2a3140, hue: false }),
    ("night", Scene { lamp: 0xffa860, lamp_i: 11.0, spot: 0xffd6a0, spot_i: 7.0, amb: 0.12, amb_c: 0x6a5a8a, bg: 0x0b0612, hue: false }),
    ("neon", Scene { lamp: 0xffffff, lamp_i: 10.0, spot: 0xffffff, spot_i: 4.0, amb: 0.1, amb_c: 0x7a50ff, bg: 0x0a0320, hue: true }),
    ("spotlit", Scene { lamp: 0xfff0d6, lamp_i: 26.0, spot: 0xffffff, spot_i: 10.0, amb: 0.09, amb_c: 0xaab0c8, bg: 0x020203, hue: false }),
    ("lightsout", Scene { lamp: 0xffd0a0, lamp_i: 0.0, spot: 0xffe8cc, spot_i: 1.4, amb: 0.03, amb_c: 0x6070a0, bg: 0x010102, hue: false }),
];

fn hex(h: u32) -> Color {
    Color::srgb_u8((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

/// The room as the Museum settings have it: the scene, brightness and warmth, and the walls', floor's and shelves'
/// finishes.
#[derive(Clone)]
struct Style {
    scene: Scene,
    bright: f32,
    warmth: f32,
    wall: String,
    floor: String,
    shelf: String,
    /// how strong the beam on the chosen case is, 0 to 2, and how much dust drifts in the lamps' light
    spot: f32,
    dust: f32,
    /// the gallery pieces: display plinths, velvet ropes, a bench, posters of your favourites
    plinths: bool,
    ropes: bool,
    bench: bool,
    posters: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style { scene: SCENES[0].1, bright: 1.0, warmth: 0.0, wall: "plaster".into(), floor: "stone".into(), shelf: "walnut".into(), spot: 1.0, dust: 1.0, plinths: false, ropes: false, bench: false, posters: false }
    }
}

impl Style {
    fn read(ui: Option<&serde_json::Value>) -> Style {
        let st = |k: &str| ui.and_then(|u| u.get("museumStyle")).and_then(|x| x.get(k));
        let text = |k: &str, d: &str| st(k).and_then(|v| v.as_str()).unwrap_or(d).to_string();
        let on = |k: &str| st(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let chosen = ui.and_then(|u| u.get("museumLight")).and_then(|v| v.as_str()).unwrap_or("gallery").to_string();
        // following the time of day: Daylight in the morning, Classic in the afternoon, Evening at night
        let scene = if st("timeOfDay").and_then(|v| v.as_bool()).unwrap_or(false) {
            use chrono::Timelike;
            match chrono::Local::now().hour() {
                6..=10 => "daylight",
                11..=18 => "gallery",
                _ => "night",
            }
            .to_string()
        } else {
            chosen
        };
        Style {
            scene: SCENES.iter().find(|s| s.0 == scene).map_or(SCENES[0].1, |s| s.1),
            bright: st("bright").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            warmth: st("warmth").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
            wall: text("wall", "plaster"),
            floor: text("floor", "stone"),
            shelf: text("shelf", "walnut"),
            spot: st("spot").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            dust: st("dust").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32,
            plinths: on("plinths"),
            ropes: on("ropes"),
            bench: on("bench"),
            posters: on("posters"),
        }
    }

    /// A picture lamp's full strength.
    fn lamp(&self) -> f32 {
        self.scene.lamp_i * LAMP_UNIT * self.bright
    }

    /// A row's lamp colour: the scene's (in Neon night the console's own), warmed or cooled.
    fn lamp_color(&self, glow: Color) -> Color {
        let mut c = hex(self.scene.lamp).to_srgba();
        if self.scene.hue {
            c = glow.to_srgba().mix(&Srgba::WHITE, 0.06);
        }
        if self.warmth > 0.0 {
            c = c.mix(&hex(0xffa45c).to_srgba(), self.warmth * 0.45);
        } else if self.warmth < 0.0 {
            c = c.mix(&hex(0xc8dcff).to_srgba(), -self.warmth * 0.55);
        }
        c.into()
    }
}

/// A case on the wall: its row, its place there, and where it stands.
#[derive(Component)]
pub struct Case {
    row: usize,
    col: usize,
    home: Vec3,
    w: f32,
    d: f32,
    h: f32,
}

struct Row {
    /// the wall it's on (0 back, 1 left, 2 right) and its place on that wall, top down
    wall: usize,
    /// the shelf its games are from, and their places on that shelf
    shelf: usize,
    name: String,
    games: Vec<usize>,
    /// each case's spine and front sheet, and its look's id
    sheets: Vec<(Handle<StandardMaterial>, Handle<StandardMaterial>)>,
    xs: Vec<f32>,
    y: f32,
    /// where the row's plaque ends, its console's middle and width, and its cases' width
    end: f32,
    console: (f32, f32),
    w: f32,
    painted: bool,
    /// how far its light has come on in the walk-in, 0 to 1, and its plaque
    lamp_on: f32,
    plaque: Handle<StandardMaterial>,
}

/// What the museum shows of a game besides its case: its plays, the section it's in for the shelf's sort, a favourite.
struct About {
    plays: u64,
    ms: u64,
    /// when it was last played, in milliseconds since 1970
    last: u64,
    group: Option<String>,
    fav: bool,
}

#[derive(Resource, Default)]
pub struct Museum {
    pub open: bool,
    camera: Option<Entity>,
    rows: Vec<Row>,
    row: usize,
    col: usize,
    /// the camera's place and where it looks
    eye: Vec3,
    look: Vec3,
    /// how far out the chosen case is, 0 to 1, and when the arrows last moved
    pull: f32,
    moved_at: f32,
    /// a held arrow repeats from this time
    repeat_at: f32,
    /// when View / Minus went down on the shelf: a tap opens Details, a hold the museum
    select_since: Option<f32>,
    shown_front: Option<(usize, usize)>,
    /// "classic" (one wall of every shelf), "manufacturer" or "genre" (rooms); the rooms, and the one in view
    grouping: String,
    plan: Option<rustyshelf_core::museum_plan::Plan>,
    room: usize,
    /// each wall's place and turn (the left and right walls face into the room)
    walls: [(Vec3, f32); 3],
    /// walking through the arcade
    walk: Option<crate::arcade::Walk>,
    /// each shelf's games that have a case, for this visit, and what's known of them by id
    kept: Vec<(usize, Vec<usize>)>,
    about: std::collections::HashMap<String, About>,
    /// how long the walk-in has run: the picture lights come on row by row while the camera drifts in to the wall
    intro: Option<f32>,
    lang: String,
    card: Option<CardParts>,
    /// stepped back to see the whole wall, a soft gold frame on the shelf in view: the frame's box on its wall (left,
    /// right, bottom, top), its wall, how far it has faded in, and its colour
    wide: bool,
    frame: [f32; 4],
    frame_wall: usize,
    frame_k: f32,
    frame_mat: Handle<StandardMaterial>,
    /// a game played from the museum, to come back to once it ends
    back_to: Option<BackTo>,
    /// the reading corner: its racks, the issue chosen there instead of a case, the rack the wide view's frame is
    /// on, the reading folder's items, and an issue's pages being made ready to read
    rack: crate::rack::Rack,
    rack_sel: Option<usize>,
    wide_unit: Option<usize>,
    reading: Vec<rustyshelf_core::reading::Item>,
    opening: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Result<Vec<std::path::PathBuf>, String>>>>,
    /// a word under the wall for a moment (an issue that couldn't be opened), and until when
    note: Option<(String, f32)>,
    /// the reader is drawn over the museum while it's open; and was open last frame (its closing key isn't the museum's)
    reader_here: bool,
    reading_was: bool,
    style: Style,
    /// the case in your hands: how it's turned (and where it's turning to), in radians; how far it has come up to you,
    /// 0 to 1; and the cases whose backs have been put on
    held: Option<Held>,
    lift: f32,
    backed: std::collections::HashSet<(usize, usize)>,
    /// each row's dust: its motes (where they are, and each one's own drift), and their look
    dust: Vec<Dust>,
    /// the surfaces drawn so far, by their finish
    surfaces: std::collections::HashMap<String, Handle<Image>>,
}

struct Dust {
    row: usize,
    mesh: Handle<Mesh>,
    look: Handle<StandardMaterial>,
    at: Vec<[f32; 3]>,
    seed: Vec<f32>,
}

#[derive(Default)]
struct Held {
    rx: f32,
    ry: f32,
    to_rx: f32,
    to_ry: f32,
}

/// Where to come back to after a game played from the museum: where you stood in the arcade, if you were walking;
/// when it was started, and whether it has been seen running.
struct BackTo {
    walk: Option<(f32, f32, f32)>,
    at: f32,
    started: bool,
}

impl Museum {
    /// Where the camera stands for the row and case in view, in the room.
    fn view(&self) -> (Vec3, Vec3) {
        if let Some(p) = self.rack_sel.and_then(|i| self.rack.pieces.get(i)) {
            let y = self.rows.get(p.row).map_or(0.0, |r| r.y);
            let (at, turn) = self.walls[0];
            let to_room = |v: Vec3| at + Quat::from_rotation_y(turn) * v;
            return (to_room(Vec3::new(p.x, y - 0.03, 2.4)), to_room(Vec3::new(p.x, y - 0.06, 0.0)));
        }
        let Some(r) = self.rows.get(self.row) else { return (Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO) };
        let x = r.xs.get(self.col).copied().unwrap_or(0.0);
        let (at, turn) = self.walls[r.wall];
        let to_room = |p: Vec3| at + Quat::from_rotation_y(turn) * p;
        (to_room(Vec3::new(x - 0.2, r.y + 0.02, 3.7)), to_room(Vec3::new(x - 0.2, r.y - 0.03, 0.0)))
    }

    /// The whole wall the row in view is on, at most 26 wide (else centred on the case in view): the wide view.
    fn overview(&self, aspect: f32) -> (Vec3, Vec3) {
        let Some(r) = self.rows.get(self.row) else { return self.view() };
        let on: Vec<&Row> = self.rows.iter().filter(|o| o.wall == r.wall).collect();
        let rack_end = if r.wall == 0 { self.rack.units.last().map_or(0.0, |u| u.x1) } else { 0.0 };
        let (l, rt) = (-0.55, on.iter().map(|o| o.end).fold(rack_end, f32::max) + 0.55);
        let h = on.len() as f32 * ROW + 1.1;
        let w = rt - l + 0.6;
        let cap = (26.0f32).max(h * aspect);
        let focus = match self.wide_unit.and_then(|u| self.rack.units.get(u)) {
            Some(u) => (u.x0 + u.x1) / 2.0,
            None => r.xs.get(self.col).copied().unwrap_or(0.0),
        };
        let x = if w > cap { focus.clamp(l - 0.3 + cap / 2.0, rt + 0.3 - cap / 2.0) } else { (l + rt) / 2.0 };
        let top = on.iter().map(|o| o.y).fold(f32::MIN, f32::max);
        let y = top - (on.len() as f32 - 1.0) * ROW / 2.0 + 0.15;
        let half = 18f32.to_radians().tan();
        let z = ((h / 2.0) / half).max((w.min(cap) / 2.0) / half / aspect.max(0.1)) + 0.25;
        let (at, turn) = self.walls[r.wall];
        let to_room = |p: Vec3| at + Quat::from_rotation_y(turn) * p;
        (to_room(Vec3::new(x, y, z)), to_room(Vec3::new(x, y, 0.0)))
    }

    /// The wide view's frame round a row, on its wall: left, right, bottom, top.
    fn frame_of(&self, i: usize) -> [f32; 4] {
        let Some(r) = self.rows.get(i) else { return [0.0; 4] };
        let wide = self.rows.iter().filter(|o| o.wall == r.wall).map(|o| o.end).fold(0.0, f32::max);
        [-0.47, wide + 0.47, r.y - 0.5 - PLANK - 0.03, r.y + ROW - 0.5 - PLANK + 0.02]
    }

    fn room_title(&self) -> Option<&str> {
        self.plan.as_ref().and_then(|p| p.rooms.get(self.room)).map(|r| r.title.as_str())
    }
}

/// Each museum shelf's games that get a case, by shelf: every game, or for a very big library a share of them
/// (museum_plan::pick: favourites and the most played first).
fn kept(lib: &rustyshelf_core::library::Library, plays: &rustyshelf_core::history::History) -> Vec<(usize, Vec<usize>)> {
    let favs: std::collections::HashSet<&str> = lib.config.raw().get("ui").and_then(|u| u.get("favorites")).and_then(|f| f.as_array()).map(|f| f.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    let shelves: Vec<(usize, Vec<usize>)> = lib
        .shelves
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s.kind, ShelfKind::Console { .. } | ShelfKind::Pc))
        .map(|(si, s)| (si, s.games.iter().enumerate().filter(|(_, g)| !matches!(g.source, Source::Tile(_))).map(|(i, _)| i).collect::<Vec<_>>()))
        .filter(|(_, g)| !g.is_empty())
        .collect();
    let n = shelves.len();
    shelves
        .into_iter()
        .map(|(si, games)| {
            let s = &lib.shelves[si];
            let fav: Vec<bool> = games.iter().map(|&i| favs.contains(s.games[i].id.as_str())).collect();
            let count: Vec<u64> = games.iter().map(|&i| plays.get(&s.games[i].id).map_or(0, |p| p.count)).collect();
            let keep = rustyshelf_core::museum_plan::pick(&fav, &count, n);
            (si, keep.into_iter().map(|k| games[k]).collect())
        })
        .collect()
}

/// The museum's games as the room plan takes them: each kept game, with its shelf as its system and its first genre.
fn entries(lib: &rustyshelf_core::library::Library, kept: &[(usize, Vec<usize>)]) -> Vec<rustyshelf_core::museum_plan::Entry> {
    kept.iter()
        .flat_map(|(si, games)| {
            let s = &lib.shelves[*si];
            games.iter().map(|&i| &s.games[i]).map(|g| rustyshelf_core::museum_plan::Entry {
                id: g.id.clone(),
                system: s.id.clone(),
                genre: g.info.as_ref().and_then(|i| i.genres.as_deref()).and_then(|x| x.split(';').next()).unwrap_or("").to_string(),
            })
        })
        .collect()
}

fn layer() -> RenderLayers {
    RenderLayers::layer(LAYER)
}

/// Builds the wall from the library: the console shelves and the PC games, each game once.
#[allow(clippy::too_many_arguments)]
fn build(
    commands: &mut Commands,
    m: &mut Museum,
    lib: &rustyshelf_core::library::Library,
    cursor: &Cursor,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    covers: &mut Covers,
    loader: &ArtLoader,
    images: &mut Assets<Image>,
) {
    m.rows.clear();
    m.dust.clear();
    (m.held, m.lift) = (None, 0.0);
    m.backed.clear();
    // what stands on the walls: (wall, the shelf, its games' places there)
    let groups: Vec<(usize, usize, Vec<usize>)> = match m.plan.as_ref().and_then(|p| p.rooms.get(m.room)) {
        Some(room) => {
            let mut at = std::collections::HashMap::new();
            for (si, games) in &m.kept {
                for &gi in games {
                    at.entry(lib.shelves[*si].games[gi].id.as_str()).or_insert((*si, gi));
                }
            }
            room.segments
                .iter()
                .filter_map(|seg| {
                    let places: Vec<(usize, usize)> = seg.game_ids.iter().filter_map(|id| at.get(id.as_str()).copied()).collect();
                    let si = places.first()?.0;
                    Some((["back", "left", "right"].iter().position(|w| *w == seg.wall).unwrap_or(0), si, places.into_iter().map(|p| p.1).collect()))
                })
                .collect()
        }
        None => m.kept.iter().map(|(si, games)| (0, *si, games.clone())).collect(),
    };
    // a shelf split over several rows in a room has its console and plaque at the end of its last one
    let parts = |si: usize| groups.iter().filter(|g| g.1 == si).count();
    let last_of = |k: usize| !groups[k + 1..].iter().any(|g| g.1 == groups[k].1);
    let room = m.style.clone();
    let style = &room;
    let mut surface = |what: &str, kind: &str| -> Option<Handle<Image>> {
        let key = format!("{what}:{kind}");
        if let Some(h) = m.surfaces.get(&key) {
            return Some(h.clone());
        }
        let pic = rustyshelf_core::drawn::museum_surface(what, kind)?;
        let mut img = crate::loading::to_image(pic.width, pic.height, pic.rgba);
        img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor { address_mode_u: bevy::image::ImageAddressMode::Repeat, address_mode_v: bevy::image::ImageAddressMode::Repeat, ..bevy::image::ImageSamplerDescriptor::linear() });
        let h = images.add(img);
        m.surfaces.insert(key, h.clone());
        Some(h)
    };
    let wood = materials.add(StandardMaterial { base_color_texture: surface("shelf", &style.shelf), uv_transform: bevy::math::Affine2::from_scale(Vec2::new(2.0, 1.0)), perceptual_roughness: 0.55, ..default() });
    let wall_tex = surface("wall", &style.wall);
    let wall_color = match style.wall.as_str() {
        _ if wall_tex.is_some() => Color::WHITE,
        "green" => hex(0x14241b),
        "burgundy" => hex(0x2a1219),
        "navy" => hex(0x101a2e),
        _ => hex(0x171a24),
    };
    // a wall's pattern keeps its size however big the wall: a tile is this many metres
    let wall_tile = match style.wall.as_str() {
        "wallpaper" => 0.75,
        _ => 2.0,
    };
    let (floor_rough, floor_metal) = match style.floor.as_str() {
        "wood" => (0.45, 0.05),
        "carpet" => (0.95, 0.0),
        "checker" => (0.28, 0.1),
        _ => (0.32, 0.25),
    };
    let floor_tile = match style.floor.as_str() {
        "wood" => 3.0,
        "carpet" => 1.0,
        "checker" => 2.0,
        _ => 4.0,
    };
    let floor_tex = surface("floor", &style.floor);
    let brass = materials.add(StandardMaterial { base_color: Color::srgb_u8(0xc9, 0xa8, 0x6a), metallic: 0.8, perceptual_roughness: 0.3, emissive: LinearRgba::rgb(0.045, 0.024, 0.005), ..default() });
    // the console, the plaque beside it and the room around them, after a row's cases
    let tail = |w: f32| 0.3 + (w * 1.7).max(1.3) + 0.12 + 1.5 + 0.3;
    let width_of = |si: usize, games: &[usize]| -> f32 {
        let s = &lib.shelves[si];
        let w = games.first().map_or(1.0, |&gi| { let look = looks::look_for(&s.id, &s.games[gi]); look.h.unwrap_or(1.0) * look.ratio });
        games.iter().map(|&gi| { let look = looks::look_for(&s.id, &s.games[gi]); (look.h.unwrap_or(1.0) * look.ratio * look.depth).max(0.045) + GAP }).sum::<f32>() + if games.len() >= 3 { 0.16 } else { 0.0 } + tail(w)
    };
    let mut wall_w = [4.0f32; 3];
    for (wall, si, games) in &groups {
        wall_w[*wall] = wall_w[*wall].max(width_of(*si, games));
    }
    let side = groups.iter().any(|g| g.0 != 0);
    // the side walls flank the back wall and run toward the room's front: the left one turned to face +x starts at
    // the front and runs back, the right one turned to face -x starts at the back
    m.walls = [(Vec3::ZERO, 0.0), (Vec3::new(-1.7, 0.0, 0.3 + wall_w[1]), std::f32::consts::FRAC_PI_2), (Vec3::new(wall_w[0] + 1.7, 0.0, 0.3), -std::f32::consts::FRAC_PI_2)];
    let parents: Vec<Entity> = m.walls.iter().map(|(at, turn)| commands.spawn((MuseumThing, Transform::from_translation(*at).with_rotation(Quat::from_rotation_y(*turn)), Visibility::Inherited)).id()).collect();
    let mut on_wall = [0usize; 3];
    let mut widest: f32 = 4.0;
    let mut seen = std::collections::HashMap::new();
    for (k, (wall, si, games)) in groups.iter().cloned().enumerate() {
        let s = &lib.shelves[si];
        let ri = m.rows.len();
        let li = on_wall[wall];
        on_wall[wall] += 1;
        let y = -(li as f32) * ROW;
        let parent = parents[wall];
        let mut x = 0.0;
        let (mut sheets, mut xs) = (Vec::new(), Vec::new());
        let style = looks::look(&s.id).clone();
        let (sw, sch) = (style.h.unwrap_or(1.0) * style.ratio, style.h.unwrap_or(1.0));
        // each run of games in one section of the shelf's sort: where it starts and ends, and its name
        let mut tabs: Vec<(f32, f32, String)> = Vec::new();
        for (col, &gi) in games.iter().enumerate() {
            let g = &s.games[gi];
            let look = looks::look_for(&s.id, g);
            let ch = look.h.unwrap_or(1.0);
            let w = ch * look.ratio;
            let d = (w * look.depth).max(0.045);
            x += d / 2.0;
            let home = Vec3::new(x, y - (1.0 - ch) / 2.0, -DEPTH / 2.0 + w / 2.0 + 0.02);
            let case = materials.add(StandardMaterial { base_color: color(look.case_color), perceptual_roughness: 0.42, ..default() });
            let spine = materials.add(StandardMaterial { base_color: color(look.case_color), perceptual_roughness: 0.36, ..default() });
            let front = materials.add(StandardMaterial { base_color: color(look.case_color), perceptual_roughness: 0.42, ..default() });
            commands
                .spawn((Case { row: ri, col, home, w, d, h: ch }, layer(), Mesh3d(meshes.add(Cuboid::new(d, ch, w))), MeshMaterial3d(case), Transform::from_translation(home), ChildOf(parent)))
                .with_children(|c| {
                    c.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(d * 0.96, ch * 0.98))), MeshMaterial3d(spine.clone()), Transform::from_xyz(0.0, 0.0, w / 2.0 + 0.001)));
                    c.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(w * 0.96, ch * 0.98))), MeshMaterial3d(front.clone()), Transform::from_xyz(d / 2.0 + 0.001, 0.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))));
                });
            if let Some(group) = m.about.get(&g.id).and_then(|a| a.group.clone()).filter(|_| games.len() >= 6) {
                match tabs.last_mut() {
                    Some(t) if t.2 == group => t.1 = x + d / 2.0,
                    _ => tabs.push((x - d / 2.0, x + d / 2.0, group)),
                }
            }
            sheets.push((spine, front));
            xs.push(x);
            x += d / 2.0 + GAP;
        }
        let top = y - 0.5;
        let front_z = (DEPTH + 0.1) / 2.0 + 0.002;
        if !tabs.is_empty() {
            section_tabs(commands, parent, &tabs, top, front_z, &m.lang, &brass, meshes, materials, covers, loader);
        }
        if games.len() >= 3 {
            let z = -DEPTH / 2.0 + sw / 2.0 + 0.02;
            commands.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(0.12, 0.02, sw * 0.9))), MeshMaterial3d(wood.clone()), Transform::from_xyz(x + 0.05, top + 0.01, z), ChildOf(parent)));
            commands.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(0.02, sch * 0.62, sw * 0.9))), MeshMaterial3d(wood.clone()), Transform::from_xyz(x + 0.005, top + sch * 0.31, z), ChildOf(parent)));
            x += 0.16;
        }
        let console_w = (sw * 1.7).max(1.3);
        if last_of(k) {
            stand_in(commands, parent, &style, Vec3::new(x + 0.3 + console_w / 2.0, top, 0.0), console_w, meshes, materials);
        }
        let total = parts(si);
        let part = seen.entry(si).and_modify(|n| *n += 1).or_insert(1usize);
        let name = if total > 1 { format!("{} · {part} of {total}", s.name) } else { s.name.clone() };
        let lit = if m.intro.is_some() { 0.0 } else { 1.0 };
        let plaque = materials.add(StandardMaterial { base_color: plaque_tint(lit), unlit: true, ..default() });
        let at = x + 0.3 + console_w + 0.12 + 0.75;
        commands.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(1.5, 0.56))), MeshMaterial3d(plaque.clone()), Transform::from_xyz(at, top + 0.3, -DEPTH / 2.0 + 0.06), ChildOf(parent)));
        let lines = plaque_lines(&games.iter().map(|&gi| &s.games[gi]).collect::<Vec<_>>(), &m.about, &m.lang);
        covers.want(Art::Plaque { name: name.clone(), lines }, (plaque.clone(), 0.0), loader, materials);
        let end = at + 0.75;
        widest = widest.max(end + 0.3);
        // the plank under the row, and the lamp over it
        commands.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(end + 0.66, PLANK, DEPTH + 0.1))), MeshMaterial3d(wood.clone()), Transform::from_xyz(end / 2.0, top - PLANK / 2.0, 0.0), ChildOf(parent)));
        if room.dust > 0.0 {
            // dust in the lamp's cone
            let mut chance = rustyshelf_core::drawn::hash(&format!("{}{ri}", s.id)) | 1;
            let mut roll = || {
                chance ^= chance << 13;
                chance ^= chance >> 17;
                chance ^= chance << 5;
                (chance % 10_000) as f32 / 10_000.0
            };
            let (lx, span) = (x / 2.0, x.min(5.0));
            let at: Vec<[f32; 3]> = (0..70).map(|_| [lx + (roll() - 0.5) * span, y - 0.5 + roll() * 1.2, -0.2 + roll() * 1.4]).collect();
            let seed: Vec<f32> = (0..70).map(|_| roll() * 100.0).collect();
            let mesh = meshes.add(Mesh::new(bevy::mesh::PrimitiveTopology::PointList, bevy::asset::RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, at.clone()));
            let look = materials.add(StandardMaterial { base_color: Color::srgba_u8(0xff, 0xe6, 0xc0, 0), unlit: true, alpha_mode: AlphaMode::Add, ..default() });
            commands.spawn((layer(), Mesh3d(mesh.clone()), MeshMaterial3d(look.clone()), Transform::default(), ChildOf(parent)));
            m.dust.push(Dust { row: ri, mesh, look, at, seed });
        }
        commands.spawn((
            ChildOf(parent),
            RowLight(ri),
            layer(),
            SpotLight { color: room.lamp_color(color(style_glow(&s.id))), intensity: 0.0, range: 7.5, outer_angle: 0.95, inner_angle: 0.5, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(x / 2.0, y + ROW - 0.62, 0.9).looking_at(Vec3::new(x / 2.0, y - 0.5, 0.0), Vec3::Y),
        ));
        m.rows.push(Row { wall, shelf: si, name, games, sheets, xs, y, end, console: (x + 0.3 + console_w / 2.0, console_w), w: sw, painted: false, lamp_on: lit, plaque });
    }
    // the Reading & media room has no game shelves: its rack's ledges are level with rows of their own
    let media = m.plan.as_ref().and_then(|p| p.rooms.get(m.room)).filter(|r| r.kind == "media").map(|r| r.title.clone());
    if let Some(title) = &media {
        let most = m.reading.iter().filter(|i| i.kind == "guide").count().max(m.reading.iter().filter(|i| i.kind != "guide").count());
        let lit = if m.intro.is_some() { 0.0 } else { 1.0 };
        let n = most.div_ceil(3).clamp(4, 12);
        for li in 0..n {
            m.rows.push(Row { wall: 0, shelf: usize::MAX, name: title.clone(), games: Vec::new(), sheets: Vec::new(), xs: Vec::new(), y: -(li as f32) * ROW, end: 0.0, console: (0.0, 0.0), w: 1.0, painted: true, lamp_on: lit, plaque: Handle::default() });
        }
        on_wall[0] = n;
    }
    (m.rack, m.rack_sel, m.wide_unit) = (crate::rack::Rack::default(), None, None);
    if m.plan.is_none() || media.is_some() {
        let ledges: Vec<(usize, f32)> = m.rows.iter().enumerate().filter(|(_, r)| r.wall == 0).map(|(i, r)| (i, r.y)).collect();
        let x0 = if media.is_some() { 0.5 } else { m.rows.iter().filter(|r| r.wall == 0).map(|r| r.end).fold(0.0, f32::max) + 0.89 };
        let items = m.reading.iter().take(rustyshelf_core::museum_plan::MEDIA_CAP).cloned().collect();
        let lamp = (style.lamp() * 180.0 / 260.0, style.lamp_color(Color::WHITE));
        m.rack = crate::rack::build(commands, parents[0], &ledges, x0, lamp, wood.clone(), items, rustyshelf_core::reading::cache_dir(&lib.app_root), &m.lang, layer(), meshes, materials, covers, loader);
        if let Some(u) = m.rack.units.last() {
            widest = widest.max(u.x1 + 0.8);
            wall_w[0] = wall_w[0].max(u.x1 + 0.8);
        }
        if media.is_some() {
            m.wide_unit = Some(0);
        }
    }
    // each wall's panel behind its rows (a room's are its own width; the classic wall runs on); one floor for the room
    let tallest = on_wall.iter().copied().max().unwrap_or(1).max(1) as f32;
    for (k, &n) in on_wall.iter().enumerate() {
        if n == 0 && k > 0 {
            continue;
        }
        let n = n.max(1) as f32;
        let w = if side { wall_w[k] + 2.0 } else { widest + 60.0 };
        let h = n * ROW + 8.0;
        let wall = materials.add(StandardMaterial { base_color: wall_color, base_color_texture: wall_tex.clone(), uv_transform: bevy::math::Affine2::from_scale(Vec2::new(w / wall_tile, h / wall_tile)), perceptual_roughness: if style.wall == "wallpaper" { 0.7 } else { 0.95 }, ..default() });
        commands.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(w, h))), MeshMaterial3d(wall), Transform::from_xyz(wall_w[k] / 2.0, -(n - 1.0) * ROW / 2.0, -DEPTH / 2.0 - 0.01), ChildOf(parents[k])));
    }
    // the dark all round the room: the shelf's own backdrop is drawn by a camera before this one, and must never show
    let dark = materials.add(StandardMaterial { base_color: hex(style.scene.bg), unlit: true, cull_mode: None, ..default() });
    commands.spawn((MuseumThing, layer(), Mesh3d(meshes.add(Sphere::new(400.0))), MeshMaterial3d(dark), Transform::from_xyz(widest / 2.0, 0.0, 0.0)));
    let bottom = -(tallest - 1.0) * ROW - 0.5 - PLANK - 0.4;
    let (fw, fd) = (widest + 60.0, 40.0 + wall_w[1].max(wall_w[2]));
    let floor = materials.add(StandardMaterial { base_color: if floor_tex.is_some() { Color::WHITE } else { Color::srgb(0.12, 0.11, 0.1) }, base_color_texture: floor_tex, uv_transform: bevy::math::Affine2::from_scale(Vec2::new(fw / floor_tile, fd / floor_tile)), perceptual_roughness: floor_rough, metallic: floor_metal, ..default() });
    commands.spawn((MuseumThing, layer(), Mesh3d(meshes.add(Plane3d::default().mesh().size(fw, fd))), MeshMaterial3d(floor), Transform::from_xyz(widest / 2.0, bottom, 10.0)));
    let back_w = m.rows.iter().filter(|r| r.wall == 0).map(|r| r.end).fold(0.0, f32::max);
    let rack_end = m.rack.units.last().map(|u| u.x1);
    let favourites: Vec<Art> = m
        .kept
        .iter()
        .flat_map(|(si, games)| games.iter().map(move |&gi| (*si, gi)))
        .filter(|&(si, gi)| m.about.get(&lib.shelves[si].games[gi].id).is_some_and(|a| a.fav))
        .take(4)
        .map(|(si, gi)| {
            let (s, g) = (&lib.shelves[si], &lib.shelves[si].games[gi]);
            cover_of(g, &s.id, &looks::look_for(&s.id, g)).0
        })
        .collect();
    let on_wall0 = on_wall[0].max(1) as f32;
    gallery(commands, parents[0], style, (back_w, rack_end, side, bottom, -(on_wall0 - 1.0) * ROW / 2.0), &favourites, meshes, materials, covers, loader);
    let image = images.add(crate::loading::to_image(1, 1, vec![255; 4]));
    let face = materials.add(StandardMaterial { base_color: Color::srgba(1.0, 1.0, 1.0, 0.0), base_color_texture: Some(image.clone()), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let board = materials.add(StandardMaterial { base_color: Color::srgba_u8(0x8a, 0x6a, 0x34, 0), metallic: 0.7, perceptual_roughness: 0.35, alpha_mode: AlphaMode::Blend, ..default() });
    let entity = commands
        .spawn((MuseumThing, MuseumCard, layer(), Transform::from_scale(Vec3::splat(CARD_S)), Visibility::Hidden))
        .with_children(|c| {
            c.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(0.9, 0.44))), MeshMaterial3d(face.clone()), Transform::from_xyz(0.0, 0.0, 0.0125)));
            c.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(0.94, 0.48, 0.024))), MeshMaterial3d(board.clone())));
        })
        .id();
    m.card = Some(CardParts { entity, face, board, image, asked: None, shows: None, drawing: None, fade: 0.0 });
    m.frame_mat = materials.add(StandardMaterial { base_color: Color::srgba_u8(0xff, 0xd9, 0x8a, 0), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    let piece = meshes.add(Cuboid::new(1.0, 1.0, 0.01));
    for side in 0..4 {
        commands.spawn((MuseumThing, FrameSide(side), layer(), Mesh3d(piece.clone()), MeshMaterial3d(m.frame_mat.clone()), Transform::default(), Visibility::Hidden));
    }
    m.frame_k = 0.0;
    commands.spawn((MuseumThing, CaseSpot, layer(), SpotLight { color: room.lamp_color(hex(room.scene.spot)), intensity: 0.0, range: 5.0, outer_angle: 17f32.to_radians(), inner_angle: 17f32.to_radians() * 0.55, shadow_maps_enabled: false, ..default() }, Transform::default()));
    // begin at the shelf and game the cursor is on
    m.row = m.rows.iter().position(|r| r.shelf == cursor.shelf).unwrap_or(0);
    m.col = m.rows.get(m.row).and_then(|r| r.games.iter().position(|&g| g == cursor.game)).unwrap_or(0);
    let (eye, look) = m.view();
    (m.eye, m.look) = (eye + Vec3::new(0.0, 0.0, 6.0), look);
    paint(m, lib, covers, loader, materials);
}

/// A row's section labels on its plank's front edge, each over a brass rail under exactly its cases. A label that would
/// run into the one before is left off; its rail still shows the split.
#[allow(clippy::too_many_arguments)]
fn section_tabs(
    commands: &mut Commands,
    parent: Entity,
    tabs: &[(f32, f32, String)],
    top: f32,
    z: f32,
    lang: &str,
    brass: &Handle<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    covers: &mut Covers,
    loader: &ArtLoader,
) {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    const LH: f32 = 0.036;
    const PXH: u32 = 80;
    let mut last_end = -1.0;
    let mut shown: Vec<(f32, f32, String)> = Vec::new();
    for (x0, x1, label) in tabs {
        let label = rustyshelf_core::i18n::tr(lang, label);
        let w = (0.03 + label.chars().count() as f32 * 0.019).clamp(0.07, 0.26).min((x1 - x0).max(0.07));
        if *x0 < last_end + 0.006 {
            continue;
        }
        last_end = x0 + w;
        shown.push((*x0, w, label));
    }
    let cells: Vec<u32> = shown.iter().map(|t| (t.1 * PXH as f32 / LH).round() as u32).collect();
    let atlas = cells.iter().copied().max().unwrap_or(32).max(32) as f32;
    let n = shown.len() as f32;
    let (mut pos, mut uv, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for (k, (x0, w, _)) in shown.iter().enumerate() {
        let (y0, y1, u1, v0, v1) = (top - 0.011 - LH, top - 0.011, cells[k] as f32 / atlas, k as f32 / n, (k + 1) as f32 / n);
        let o = pos.len() as u32;
        pos.extend([[*x0, y0, z], [x0 + w, y0, z], [x0 + w, y1, z], [*x0, y1, z]]);
        uv.extend([[0.0, v1], [u1, v1], [u1, v0], [0.0, v0]]);
        idx.extend([o, o + 1, o + 2, o, o + 2, o + 3]);
    }
    if !shown.is_empty() {
        let normals = vec![[0.0, 0.0, 1.0]; pos.len()];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx));
        let plates = materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.45, 0.3), metallic: 0.6, perceptual_roughness: 0.35, ..default() });
        commands.spawn((layer(), Mesh3d(meshes.add(mesh)), MeshMaterial3d(plates.clone()), ChildOf(parent)));
        if let Some(mut m) = materials.get_mut(&plates) {
            m.base_color = Color::WHITE;
        }
        let labels = shown.into_iter().zip(cells).map(|(t, w)| (t.2, w)).collect();
        covers.want(Art::Tabs { labels, tall: PXH }, (plates, 0.0), loader, materials);
    }
    for (x0, x1, _) in tabs {
        let (a, b) = (x0 + 0.003, x1 - 0.003);
        if b > a {
            commands.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(b - a, 0.005))), MeshMaterial3d(brass.clone()), Transform::from_xyz((a + b) / 2.0, top - 0.0055, z), ChildOf(parent)));
        }
    }
}

/// The original stand-in console at a row's end, its foot at `at`: a low slab in the shelf's colours with a lit strip,
/// and a controller in front of it.
fn stand_in(commands: &mut Commands, parent: Entity, style: &looks::Look, at: Vec3, w: f32, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let (h, d) = (0.34, DEPTH * 0.7);
    let body = materials.add(StandardMaterial { base_color: color(style.case_side), perceptual_roughness: 0.45, metallic: 0.1, clearcoat: 0.5, ..default() });
    let shell = materials.add(StandardMaterial { base_color: color(style.case_color), perceptual_roughness: 0.3, clearcoat: 0.7, ..default() });
    let pad = materials.add(StandardMaterial { base_color: color(style.case_color), perceptual_roughness: 0.6, ..default() });
    let strip = materials.add(StandardMaterial { base_color: color(style.glow), unlit: true, ..default() });
    commands.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(w, h, d))), MeshMaterial3d(body), Transform::from_translation(at + Vec3::Y * h / 2.0), ChildOf(parent))).with_children(|c| {
        c.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(w * 0.5, h * 0.12, 0.01))), MeshMaterial3d(strip), Transform::from_xyz(0.0, -h * 0.2, d / 2.0 + 0.005)));
        c.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(w * 0.9, 0.01, d * 0.9))), MeshMaterial3d(shell), Transform::from_xyz(0.0, h / 2.0, 0.0)));
        c.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(w * 0.32, h * 0.35, d * 0.5))), MeshMaterial3d(pad), Transform::from_xyz(w * 0.1, -h / 2.0 + h * 0.18, d / 2.0 + d * 0.4)));
    });
}

/// What a row's plaque says under its name: how many games and how much they've been played, the most played one,
/// and the favourites.
fn plaque_lines(games: &[&rustyshelf_core::scanner::Game], about: &std::collections::HashMap<String, About>, lang: &str) -> Vec<String> {
    let tr = |t: &str| rustyshelf_core::i18n::tr(lang, t);
    let (mut played, mut ms, mut top, mut fav) = (0, 0u64, None::<(u64, &str)>, 0);
    for g in games {
        let Some(a) = about.get(&g.id) else { continue };
        if a.plays > 0 {
            played += 1;
            ms += a.ms;
            if top.is_none_or(|t| a.ms > t.0) {
                top = Some((a.ms, g.title.as_str()));
            }
        }
        fav += usize::from(a.fav);
    }
    let n = games.len();
    let count = tr(&if n == 1 { "1 game".to_string() } else { format!("{n} games") });
    let hrs = ms as f64 / 3_600_000.0;
    let hours = if hrs >= 1.0 { format!("{hrs:.1} h played") } else if ms > 0 { format!("{} min played", (ms as f64 / 60_000.0).round()) } else { "not played yet".to_string() };
    let mut lines = vec![if played > 0 { format!("{count} · {} · {}", tr(&format!("{played} played")), tr(&hours)) } else { format!("{count} · {}", tr("nothing played yet")) }];
    if let Some((_, t)) = top.filter(|t| t.0 > 0) {
        lines.push(tr(&format!("Most played: {t}")));
    }
    if fav > 0 {
        lines.push(tr(&if fav == 1 { "1 favorite".to_string() } else { format!("{fav} favorites") }));
    }
    lines
}

/// The spines of the rows near the one in view, painted once each.
fn paint(m: &mut Museum, lib: &rustyshelf_core::library::Library, covers: &mut Covers, loader: &ArtLoader, materials: &mut Assets<StandardMaterial>) {
    let (from, to) = (m.row.saturating_sub(NEAR), (m.row + NEAR + 1).min(m.rows.len()));
    for r in &mut m.rows[from..to] {
        if std::mem::replace(&mut r.painted, true) {
            continue;
        }
        let Some(s) = lib.shelves.get(r.shelf) else { continue };
        for (k, &gi) in r.games.iter().enumerate() {
            let g = &s.games[gi];
            let look = looks::look_for(&s.id, g);
            let ch = look.h.unwrap_or(1.0);
            let d = (ch * look.ratio * look.depth).max(0.045);
            let spine = match &g.art.spine {
                Some(path) => Art::File(path.clone()),
                None => {
                    let label = look.spine_band.as_ref().map_or(String::new(), |b| b.label.clone());
                    Art::Spine { look: Box::new(look.clone()), title: g.title.clone(), label, width: ((512.0 * d / ch).round() as u32).max(4), height: 512, edge: 0.0 }
                }
            };
            if let Some(mut mat) = materials.get_mut(&r.sheets[k].0) {
                mat.base_color = Color::WHITE;
            }
            covers.want(spine, (r.sheets[k].0.clone(), 0.0), loader, materials);
        }
    }
}

pub fn setup(mut commands: Commands, mut museum: ResMut<Museum>) {
    museum.camera = Some(commands.spawn((
        MuseumCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection { fov: 36f32.to_radians(), near: 0.05, far: 900.0, ..default() }),
        Camera { order: 2, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.02, 0.03)), ..default() },
        AmbientLight { color: Color::srgb(0.8, 0.82, 1.0), brightness: 260.0, ..default() },
        layer(),
        Transform::from_xyz(0.0, 0.0, 6.0),
    )).id());
}

/// What the wall's pictures and shapes are made with.
#[derive(SystemParam)]
pub struct Making<'w> {
    covers: ResMut<'w, Covers>,
    loader: Res<'w, ArtLoader>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    mona: Res<'w, crate::hud::MonaSans>,
    images: ResMut<'w, Assets<Image>>,
}

/// The wall's moving parts: the cases, the row lights and the card.
#[derive(SystemParam)]
pub struct Wall<'w, 's> {
    cases: Query<'w, 's, (Entity, &'static Case, &'static mut Transform), (Without<MuseumCamera>, Without<RowLight>, Without<MuseumCard>)>,
    lights: Query<'w, 's, (&'static RowLight, &'static mut SpotLight), (Without<Case>, Without<MuseumCamera>)>,
    card: Query<'w, 's, (&'static mut Transform, &'static mut Visibility), (With<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>,
    frame: Query<'w, 's, (&'static FrameSide, &'static mut Transform, &'static mut Visibility), (Without<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>,
    pieces: Query<'w, 's, (&'static crate::rack::RackPiece, &'static mut Transform), (Without<FrameSide>, Without<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>,
    spot: Query<'w, 's, (&'static mut SpotLight, &'static mut Transform), (With<CaseSpot>, Without<crate::rack::RackPiece>, Without<FrameSide>, Without<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>,
}

/// What has the keys instead (a game running, Inspect, the manual reader), and the shelf's camera, which rests
/// while the museum is drawn.
#[derive(SystemParam)]
pub struct Others<'w, 's> {
    playing: Res<'w, crate::play::Playing>,
    inspect: Res<'w, crate::shelf::Inspect>,
    reader: Res<'w, crate::reader::Reader>,
    shelf_camera: Query<'w, 's, &'static mut Camera, (With<crate::shelf::ShelfCamera>, Without<MuseumCamera>)>,
    plays: Res<'w, crate::play::Plays>,
    reader_root: Query<'w, 's, (Entity, &'static crate::reader::Part)>,
}

/// Opens the museum (M on the shelf), takes the keys while it's open, and moves through it.
#[allow(clippy::too_many_arguments)]
pub fn run(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    lib: Res<LibraryRes>,
    mut museum: ResMut<Museum>,
    mut settings: ResMut<crate::settings::Settings>,
    mut others: Others,
    mut cursor: ResMut<Cursor>,
    mut sounds: ResMut<crate::sound::Sounds>,
    mut making: Making,
    mut camera: Cam,
    mut wall: Wall,
    things: Query<(Entity, Has<MuseumHud>), Or<(With<MuseumThing>, With<MuseumHud>)>>,
    mut text: Query<&mut Text, With<MuseumText>>,
) {
    let LibraryState::Ready(l) = &lib.0 else { return };
    let Making { covers, loader, meshes, materials, mona, images } = &mut making;
    let mona = &**mona;
    let (covers, loader, meshes, materials, images) = (&mut **covers, &**loader, &mut **meshes, &mut **materials, &mut **images);
    let now = time.elapsed_secs();
    let m = &mut *museum;
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    let reader_root = others.reader_root.iter().find(|(_, p)| matches!(p, crate::reader::Part::Whole)).map(|(e, _)| e);
    if m.open != m.reader_here {
        if let (Some(root), Some(cam)) = (reader_root, m.camera) {
            if m.open {
                commands.entity(root).insert(UiTargetCamera(cam));
            } else {
                commands.entity(root).remove::<UiTargetCamera>();
            }
            m.reader_here = m.open;
        }
    }
    if !m.open {
        let mut reopen = false;
        if let Some(b) = m.back_to.as_mut() {
            if others.playing.title.is_some() {
                b.started = true;
            } else if b.started {
                reopen = true;
            } else if now - b.at > 20.0 {
                m.back_to = None;
            }
        }
        let free = !settings.open && !settings.keys_taken && others.playing.title.is_none() && !others.inspect.on && !others.reader.is_open();
        let select = pads.iter().any(|p| p.pressed(GamepadButton::Select));
        if free && pad(GamepadButton::Select) {
            m.select_since = Some(now);
        }
        let held = m.select_since.is_some_and(|t| select && now - t >= HOLD);
        if !select {
            if let Some(t) = m.select_since.take() {
                settings.select_tapped = free && now - t < HOLD;
            }
        }
        if free && (keys.just_pressed(KeyCode::KeyM) || held || reopen) {
            m.select_since = None;
            m.open = true;
            m.grouping = l.config.raw().get("ui").and_then(|u| u.get("museumStyle")).and_then(|x| x.get("grouping")).and_then(|g| g.as_str()).unwrap_or("classic").to_string();
            m.kept = kept(l, &others.plays.0);
            let ui = l.config.raw().get("ui");
            let reading_root = rustyshelf_core::reading::root(ui.and_then(|u| u.get("readingLibrary")).and_then(|v| v.as_str()), &l.app_root);
            m.reading = rustyshelf_core::reading::scan(&reading_root);
            m.lang = ui.and_then(|u| u.get("language")).and_then(|v| v.as_str()).unwrap_or("en").to_string();
            let favs: std::collections::HashSet<&str> = ui.and_then(|u| u.get("favorites")).and_then(|f| f.as_array()).map(|f| f.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
            let sort = ui.and_then(|u| u.get("sort")).and_then(|v| v.as_str()).unwrap_or("az");
            let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
            m.about = m
                .kept
                .iter()
                .flat_map(|(si, games)| games.iter().map(|&gi| &l.shelves[*si].games[gi]))
                .map(|g| {
                    let p = rustyshelf_core::shelfview::played(g, &others.plays.0);
                    let about = About { plays: p.map_or(0, |p| p.count), ms: p.map_or(0, |p| p.ms), last: p.map_or(0, |p| p.last), group: rustyshelf_core::shelfview::group_of(g, sort, &others.plays.0, now_ms), fav: favs.contains(g.id.as_str()) };
                    (g.id.clone(), about)
                })
                .collect();
            m.plan = (m.grouping != "classic").then(|| rustyshelf_core::museum_plan::plan_museum(&entries(l, &m.kept), &m.grouping));
            // the room the game under the cursor is in
            let here = l.shelves.get(cursor.shelf).and_then(|s| s.games.get(cursor.game)).map(|g| g.id.clone());
            m.room = match (&m.plan, here) {
                (Some(p), Some(id)) => p.by_game.get(&id).and_then(|at| p.rooms.iter().position(|r| r.id == at.room)).unwrap_or(0),
                _ => 0,
            };
            // Room → "Lights come on as you walk in" off: straight to the wall with every lamp lit
            let walk_in = ui.and_then(|u| u.get("museumStyle")).and_then(|x| x.get("walkIn")) != Some(&serde_json::Value::Bool(false));
            m.intro = walk_in.then_some(0.0);
            m.wide = !walk_in;
            m.style = Style::read(ui);
            build(&mut commands, m, l, &cursor, meshes, materials, covers, loader, images);
            if let Ok((mut cam, _, _)) = camera.single_mut() {
                cam.is_active = true;
                cam.clear_color = ClearColorConfig::Custom(hex(m.style.scene.bg));
            }
            if let Some(cam) = m.camera {
                let sc = m.style.scene;
                commands.entity(cam).insert(AmbientLight { color: hex(sc.amb_c), brightness: sc.amb * AMB_UNIT * m.style.bright, ..default() });
            }
            if let Ok(mut shelf) = others.shelf_camera.single_mut() {
                shelf.is_active = false;
            }
            spawn_text(&mut commands, m.camera, mona);
            (m.pull, m.moved_at, m.shown_front) = (0.0, now, None);
            // back from a game started in the arcade: where you stood
            let walked = m.back_to.take().and_then(|b| b.walk);
            let cap = ui.and_then(|u| u.get("arcadeCap")).and_then(|c| c.as_f64());
            if let Some(((x, z, ry), mut w)) = walked.zip(crate::arcade::Walk::start(l, cap)) {
                for (e, _) in things.iter().filter(|(_, hud)| !hud) {
                    commands.entity(e).despawn();
                }
                (w.x, w.z, w.ry) = (x, z, ry);
                crate::arcade::build(&mut commands, &w, l, meshes, materials, covers, loader, || (MuseumThing, layer()));
                (m.walk, m.intro, m.wide) = (Some(w), None, false);
            }
            if m.intro.is_some() {
                let aspect = camera.single().ok().and_then(|(_, _, p)| if let Projection::Perspective(p) = p { Some(p.aspect_ratio) } else { None }).unwrap_or(16.0 / 9.0);
                let (eye, look) = m.overview(aspect);
                (m.eye, m.look) = (eye + Vec3::new(0.0, -0.3, 1.6), look);
            }
            sounds.play("open", 1);
            settings.keys_taken = true;
        }
        return;
    }
    settings.keys_taken = true;
    m.rack.poll(covers, loader, materials);
    let ready = m.opening.as_ref().and_then(|o| o.lock().ok()?.try_recv().ok());
    if let Some(pages) = ready {
        m.opening = None;
        match pages {
            Ok(files) if !files.is_empty() => settings.manual = Some(files),
            Ok(_) => m.note = Some((rustyshelf_core::i18n::tr(&m.lang, "No pages in it"), now + 3.0)),
            Err(e) => m.note = Some((format!("{}: {e}", rustyshelf_core::i18n::tr(&m.lang, "It couldn't be opened")), now + 4.0)),
        }
    }
    if others.reader.is_open() || settings.manual.is_some() {
        m.reading_was = true;
        return;
    }
    if std::mem::take(&mut m.reading_was) {
        return;
    }
    if m.walk.is_some() {
        return walk_on(&mut commands, m, l, &time, &keys, &pads, &settings, &mut cursor, &mut sounds, (covers, loader, meshes, materials, images), &mut camera, &things, &mut text, &mut others.shelf_camera);
    }
    if let Some(t) = m.intro.as_mut() {
        *t += time.delta_secs();
        let t = *t;
        let skip = keys.get_just_pressed().next().is_some() || pads.iter().any(|p| p.get_just_pressed().next().is_some());
        if skip || t > 0.9 + m.rows.len() as f32 * 0.36 + 0.8 {
            m.intro = None;
            m.wide = true;
            m.rows.iter_mut().for_each(|r| r.lamp_on = 1.0);
            m.moved_at = now;
            for r in &m.rows {
                if let Some(mut mat) = materials.get_mut(&r.plaque) {
                    mat.base_color = plaque_tint(1.0);
                }
            }
            return;
        }
        let mut lamps = Vec::with_capacity(m.rows.len());
        for (i, r) in m.rows.iter_mut().enumerate() {
            let k = (t - (0.9 + i as f32 * 0.36)) / 0.7;
            if k <= 0.0 {
                lamps.push(0.0);
                continue;
            }
            if r.lamp_on == 0.0 {
                sounds.play("lamp", 1);
            }
            r.lamp_on = r.lamp_on.max(k.min(1.0));
            if let Some(mut mat) = materials.get_mut(&r.plaque) {
                mat.base_color = plaque_tint(r.lamp_on);
            }
            // strike, dip, bloom, and a touch over before it settles
            let e = k.min(1.0);
            let strike = if e < 0.18 { e / 0.18 * 0.55 } else if e < 0.3 { 0.55 - (e - 0.18) / 0.12 * 0.35 } else { 0.2 + (1.0 - (1.0 - (e - 0.3) / 0.7).powi(3)) * 0.8 };
            let over = if e > 0.6 && e < 1.0 { 1.0 + 0.12 * ((e - 0.6) / 0.4 * std::f32::consts::PI).sin() } else { 1.0 };
            lamps.push(m.style.lamp() * strike.max(0.0) * over);
        }
        for (row, mut s) in &mut wall.lights {
            s.intensity = lamps.get(row.0).copied().unwrap_or(0.0);
        }
        let aspect = camera.single().ok().and_then(|(_, _, p)| if let Projection::Perspective(p) = p { Some(p.aspect_ratio) } else { None }).unwrap_or(16.0 / 9.0);
        let (eye, look) = m.overview(aspect);
        let f = 1.0 - (-time.delta_secs() * 1.6).exp();
        m.eye = m.eye.lerp(eye, f);
        m.look = m.look.lerp(look, f);
        if let Ok((_, mut t, _)) = camera.single_mut() {
            *t = Transform::from_translation(m.eye).looking_at(m.look, Vec3::Y);
        }
        if let Ok(mut text) = text.single_mut() {
            text.0 = m.room_title().unwrap_or_default().to_string();
        }
        return;
    }
    // into the arcade: W, or Y on a pad
    if m.held.is_none() && (keys.just_pressed(KeyCode::KeyW) || pad(GamepadButton::North)) {
        let cap = l.config.raw().get("ui").and_then(|u| u.get("arcadeCap")).and_then(|c| c.as_f64());
        if let Some(w) = crate::arcade::Walk::start(l, cap) {
            for (e, _) in things.iter().filter(|(_, hud)| !hud) {
                commands.entity(e).despawn();
            }
            crate::arcade::build(&mut commands, &w, l, meshes, materials, covers, loader, || (MuseumThing, layer()));
            m.walk = Some(w);
            sounds.play("open", 1);
            return;
        }
    }
    // another room: [ and ], or L1 and R1
    let to_room = (keys.just_pressed(KeyCode::BracketRight) || pad(GamepadButton::RightTrigger)) as i32 - (keys.just_pressed(KeyCode::BracketLeft) || pad(GamepadButton::LeftTrigger)) as i32;
    let shown = |r: &&rustyshelf_core::museum_plan::Room| !r.segments.is_empty() || r.kind == "media";
    if let Some(n) = m.plan.as_ref().map(|p| p.rooms.iter().filter(shown).count()).filter(|n| *n > 1 && to_room != 0) {
        let rooms: Vec<usize> = m.plan.as_ref().map(|p| p.rooms.iter().enumerate().filter(|(_, r)| shown(r)).map(|(i, _)| i).collect()).unwrap_or_default();
        let at = rooms.iter().position(|&i| i == m.room).unwrap_or(0) as i32;
        m.room = rooms[(at + to_room).rem_euclid(n as i32) as usize];
        for (e, _) in things.iter().filter(|(_, hud)| !hud) {
            commands.entity(e).despawn();
        }
        let held_cursor = Cursor { shelf: usize::MAX, ..Cursor::default() };
        build(&mut commands, m, l, &held_cursor, meshes, materials, covers, loader, images);
        (m.pull, m.moved_at, m.shown_front) = (0.0, now, None);
        sounds.play("shelf", to_room);
        return;
    }
    // a case in your hands: the arrows (or the stick) turn it, Space / X or back puts it down, confirm still plays it
    let pick = keys.just_pressed(KeyCode::Space) || pad(GamepadButton::West);
    let backing = keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(settings.confirm().1);
    let holding = m.held.is_some();
    let manual = m.rows.get(m.row).and_then(|r| r.games.get(m.col).map(|&gi| l.shelves[r.shelf].games[gi].art.manual.clone())).filter(|p| !p.is_empty());
    if let (true, Some(pages)) = (holding && (keys.just_pressed(KeyCode::KeyM) || pad(GamepadButton::North)), manual.clone()) {
        settings.manual = Some(pages);
        return;
    }
    if let Some(h) = m.held.as_mut() {
        if pick || backing {
            m.held = None;
            sounds.play("slideBack", 1);
            return;
        }
        let turn = ((keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32) as f32;
        let tilt = ((keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32) as f32;
        let stick = pads.iter().map(|p| p.left_stick() + p.right_stick()).find(|v| v.length() > 0.2).unwrap_or(Vec2::ZERO);
        let dt = time.delta_secs();
        h.to_ry += turn * std::f32::consts::FRAC_PI_2 + stick.x * 2.6 * dt;
        h.to_rx = (h.to_rx + tilt * 30f32.to_radians() - stick.y * 1.6 * dt).clamp(-60f32.to_radians(), 60f32.to_radians());
        if turn != 0.0 || tilt != 0.0 {
            sounds.play("tick", 1);
        }
    } else if pick && !m.wide && m.rack_sel.is_none() && m.pull > 0.9 && !m.rows[m.row].games.is_empty() {
        m.held = Some(Held::default());
        sounds.play("lift", 1);
        // its back, put on the first time it's picked up
        if m.backed.insert((m.row, m.col)) {
            let r = &m.rows[m.row];
            let s = &l.shelves[r.shelf];
            let g = &s.games[r.games[m.col]];
            let look = looks::look_for(&s.id, g);
            let art = match g.art.back.clone() {
                Some(path) => Art::File(path),
                None => {
                    let facts = crate::settings::facts_of(g, &crate::settings::emulator_of(s), rustyshelf_core::shelfview::played(g, &others.plays.0));
                    Art::Back { look: Box::new(look.clone()), title: g.title.clone(), facts: facts.into_iter().take(5).collect(), blurb: g.info.as_ref().and_then(|i| i.description()).unwrap_or_default() }
                }
            };
            let sheet = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.42, ..default() });
            covers.want(art, (sheet.clone(), look.ratio), loader, materials);
            if let Some((e, c, _)) = wall.cases.iter().find(|(_, c, _)| c.row == m.row && c.col == m.col) {
                commands.entity(e).with_child((layer(), Mesh3d(meshes.add(Rectangle::new(c.w * 0.96, c.h * 0.98))), MeshMaterial3d(sheet), Transform::from_xyz(-c.d / 2.0 - 0.001, 0.0, 0.0).with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2))));
            }
        }
    }
    let back = !holding && backing;
    if m.rows.is_empty() || (keys.just_pressed(KeyCode::KeyM) && !holding) || (back && m.wide) {
        return close(&mut commands, m, &things, &mut camera, &mut others.shelf_camera, &mut sounds);
    }
    if back {
        m.wide = true;
        if let Some(p) = m.rack_sel.take().and_then(|i| m.rack.pieces.get(i)) {
            m.wide_unit = Some(p.unit);
        }
        sounds.play("slideBack", 1);
        return;
    }
    // the arrows, the d-pad and the left stick; held, they repeat
    let stick = pads.iter().map(|p| p.left_stick() + p.dpad()).find(|v| v.length() > 0.5).unwrap_or(Vec2::ZERO);
    let held = Vec2::new(
        (keys.pressed(KeyCode::ArrowRight) as i32 - keys.pressed(KeyCode::ArrowLeft) as i32) as f32 + if stick.x.abs() > 0.5 { stick.x.signum() } else { 0.0 },
        (keys.pressed(KeyCode::ArrowDown) as i32 - keys.pressed(KeyCode::ArrowUp) as i32) as f32 + if stick.y.abs() > 0.5 { -stick.y.signum() } else { 0.0 },
    );
    // a tap can start and end within one frame, so it's never seen as held
    let tap = Vec2::new(
        ((keys.just_pressed(KeyCode::ArrowRight) || pad(GamepadButton::DPadRight)) as i32 - (keys.just_pressed(KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft)) as i32) as f32,
        ((keys.just_pressed(KeyCode::ArrowDown) || pad(GamepadButton::DPadDown)) as i32 - (keys.just_pressed(KeyCode::ArrowUp) || pad(GamepadButton::DPadUp)) as i32) as f32,
    );
    let (held, tap) = if holding { (Vec2::ZERO, Vec2::ZERO) } else { (held, tap) };
    let fresh = tap != Vec2::ZERO;
    let held = if fresh { tap } else { held };
    if held == Vec2::ZERO {
        m.repeat_at = 0.0;
    } else if fresh || m.repeat_at == 0.0 || now >= m.repeat_at {
        m.repeat_at = now + if fresh || m.repeat_at == 0.0 { 0.38 } else { 0.09 };
        if let (true, Some(u)) = (m.wide, m.wide_unit) {
            // the frame on a rack: left and right between the racks, back to the shelves past the first
            let to = u as i32 + held.x.signum() as i32;
            if held.x != 0.0 && to >= 0 && (to as usize) < m.rack.units.len() {
                m.wide_unit = Some(to as usize);
                sounds.play("tick", held.x as i32);
            } else if to < 0 && m.rows.iter().any(|r| !r.games.is_empty() && r.wall == 0) {
                m.wide_unit = None;
                sounds.play("tick", -1);
            }
        } else if m.wide {
            // up and down the shelves on this wall; left and right to the room's other walls, then the racks
            let wall = m.rows[m.row].wall;
            let to = if held.y != 0.0 {
                let on: Vec<usize> = (0..m.rows.len()).filter(|&i| m.rows[i].wall == wall).collect();
                let at = on.iter().position(|&i| i == m.row).unwrap_or(0) as i32;
                on.get((at + held.y.signum() as i32).clamp(0, on.len() as i32 - 1) as usize).copied()
            } else {
                let mut w = wall as i32 + held.x.signum() as i32;
                let mut found = None;
                while (0..3).contains(&w) && found.is_none() {
                    found = m.rows.iter().position(|r| r.wall == w as usize);
                    w += held.x.signum() as i32;
                }
                if found.is_none() && held.x > 0.0 && wall == 0 && !m.rack.units.is_empty() {
                    m.wide_unit = Some(0);
                    sounds.play("tick", 1);
                }
                found
            };
            if let Some(to) = to.filter(|&t| t != m.row) {
                let x = m.rows[m.row].xs.get(m.col).copied().unwrap_or(0.0);
                let wall_changed = m.rows[to].wall != wall;
                m.row = to;
                m.col = m.rows[to].xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
                sounds.play(if wall_changed { "slide" } else { "tick" }, if held.y != 0.0 { held.y as i32 } else { held.x as i32 });
                paint(m, l, covers, loader, materials);
            }
        } else if let Some(i) = m.rack_sel {
            let p = &m.rack.pieces[i];
            if held.y != 0.0 {
                // the nearest issue on the next ledge up or down that has any
                let (x, mut row) = (p.x, p.row as i32);
                loop {
                    row += held.y.signum() as i32;
                    if row < 0 || row as usize >= m.rows.len() {
                        break;
                    }
                    let on = m.rack.on_row(row as usize);
                    if let Some(&to) = on.iter().min_by(|a, b| (m.rack.pieces[**a].x - x).abs().total_cmp(&(m.rack.pieces[**b].x - x).abs())) {
                        (m.rack_sel, m.row) = (Some(to), row as usize);
                        sounds.play("shelf", held.y as i32);
                        break;
                    }
                }
            } else {
                let on = m.rack.on_row(p.row);
                let at = on.iter().position(|&k| k == i).unwrap_or(0) as i32 + held.x.signum() as i32;
                if at >= 0 && (at as usize) < on.len() {
                    m.rack_sel = Some(on[at as usize]);
                    sounds.play("tick", held.x as i32);
                } else if at < 0 && !m.rows[p.row].games.is_empty() {
                    // past the first issue: back to the shelf's cases
                    (m.rack_sel, m.row, m.col) = (None, p.row, m.rows[p.row].games.len() - 1);
                    sounds.play("tick", -1);
                }
            }
        } else if held.y != 0.0 {
            let to = (m.row as i32 + held.y.signum() as i32).clamp(0, m.rows.len() as i32 - 1) as usize;
            if to != m.row {
                let x = m.rows[m.row].xs.get(m.col).copied().unwrap_or(0.0);
                m.row = to;
                // the case nearest the one left, on the row come to
                m.col = m.rows[to].xs.iter().enumerate().min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs())).map_or(0, |(i, _)| i);
                sounds.play("shelf", held.y as i32);
                paint(m, l, covers, loader, materials);
            }
        } else {
            let to = (m.col as i32 + held.x.signum() as i32).clamp(0, m.rows[m.row].games.len() as i32 - 1) as usize;
            if to != m.col {
                m.col = to;
                sounds.play("tick", held.x as i32);
            } else if held.x > 0.0 && m.rows[m.row].wall == 0 {
                // past the last case: on to the reading corner's issues on this ledge
                if let Some(&first) = m.rack.on_row(m.row).first() {
                    m.rack_sel = Some(first);
                    sounds.play("tick", 1);
                }
            }
        }
        m.moved_at = now;
    }
    let confirm = keys.just_pressed(KeyCode::Enter) || pad(settings.confirm().0);
    if m.wide && confirm {
        if let Some(u) = m.wide_unit {
            // into a rack: its first issue, top left
            let first = (0..m.rack.pieces.len()).filter(|&i| m.rack.pieces[i].unit == u).min_by(|a, b| {
                let (pa, pb) = (&m.rack.pieces[*a], &m.rack.pieces[*b]);
                pa.row.cmp(&pb.row).then(pa.x.total_cmp(&pb.x))
            });
            if let Some(i) = first {
                (m.rack_sel, m.row, m.wide, m.wide_unit) = (Some(i), m.rack.pieces[i].row, false, None);
                sounds.play("slide", 1);
            }
        } else if !m.rows[m.row].games.is_empty() {
            m.wide = false;
            sounds.play("slide", 1);
        }
        m.moved_at = now;
    } else if let (true, Some(i)) = (confirm, m.rack_sel) {
        // an issue: its pages made ready off the frame (a PDF's drawn, an archive's taken out), then the reader
        if m.opening.is_none() {
            let item = m.rack.pieces[i].item.clone();
            let cache = rustyshelf_core::reading::cache_dir(&l.app_root);
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let pages = rustyshelf_core::reading::pages(&item, &cache).and_then(|p| match p {
                    rustyshelf_core::reading::Pages::Images(files) => Ok(files),
                    rustyshelf_core::reading::Pages::Pdf(_) => rustyshelf_core::reading::pdf_images(&item, &cache),
                });
                let _ = tx.send(pages);
            });
            m.opening = Some(std::sync::Mutex::new(rx));
            sounds.play("lift", 1);
        }
    } else if confirm {
        let r = &m.rows[m.row];
        (cursor.shelf, cursor.game, cursor.land) = (r.shelf, r.games[m.col], false);
        settings.play = true;
        m.back_to = Some(BackTo { walk: None, at: now, started: false });
        return close(&mut commands, m, &things, &mut camera, &mut others.shelf_camera, &mut sounds);
    }
    // the chosen case's cover, once it has stood still a moment
    let chosen = (m.row, m.col);
    if m.shown_front != Some(chosen) && now - m.moved_at > 0.2 && m.rack_sel.is_none() && !m.rows[m.row].games.is_empty() {
        m.shown_front = Some(chosen);
        let r = &m.rows[m.row];
        let s = &l.shelves[r.shelf];
        let g = &s.games[r.games[m.col]];
        let look = looks::look_for(&s.id, g);
        if let Some(mut mat) = materials.get_mut(&r.sheets[m.col].1) {
            mat.base_color = Color::WHITE;
        }
        covers.want(cover_of(g, &s.id, &look).0, (r.sheets[m.col].1.clone(), look.ratio), loader, materials);
    }
    // the chosen case slides out and turns its cover to you; the camera follows
    let dt = time.delta_secs();
    let settle = now - m.moved_at > 0.15 && !m.wide && m.rack_sel.is_none();
    m.pull = if settle { (m.pull + dt * 3.0).min(1.0) } else { (m.pull - dt * 6.0).max(0.0) };
    let mut chosen_at = Vec3::ZERO;
    m.lift += ((m.held.is_some() as u8 as f32) - m.lift) * (1.0 - (-dt * 6.0).exp());
    if let Some(h) = m.held.as_mut() {
        let k = 1.0 - (-dt * 6.0).exp();
        h.rx += (h.to_rx - h.rx) * k;
        h.ry += (h.to_ry - h.ry) * k;
    }
    let (turned_x, turned_y) = m.held.as_ref().map_or((0.0, 0.0), |h| (h.rx, h.ry));
    let hand = m.rows.get(m.row).map(|r| Vec3::new(r.xs.get(m.col).copied().unwrap_or(0.0) - 0.2, r.y, 2.2));
    for (_, c, mut t) in &mut wall.cases {
        let mine = c.row == m.row && c.col == m.col;
        let k = if mine { m.pull } else { 0.0 };
        let (out, face) = (ease((k / 0.62).min(1.0)), ease(((k - 0.55) / 0.45).clamp(0.0, 1.0)));
        t.translation = c.home + Vec3::new(0.0, out * 0.06, out * (c.w * PULL + c.d));
        t.rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2 * face);
        if mine {
            chosen_at = t.translation;
            // in your hands: up in front of you, turned as you turn it
            if let (Some(hand), true) = (hand, m.lift > 0.001) {
                let e = ease(m.lift.min(1.0));
                t.translation = t.translation.lerp(hand, e);
                t.rotation = t.rotation.slerp(Quat::from_rotation_x(turned_x) * Quat::from_rotation_y(turned_y) * t.rotation, e);
                chosen_at = t.translation;
            }
        }
    }
    show_card(m, l, chosen_at, dt, materials, images, &mut wall.card);
    // the beam: from above and in front of the pulled case, brightening as it comes out
    if let (Some(r), Ok((mut beam, mut t))) = (m.rows.get(m.row), wall.spot.single_mut()) {
        let (at, turn) = m.walls[r.wall];
        let world = |v: Vec3| at + Quat::from_rotation_y(turn) * v;
        let k = if m.rack_sel.is_none() && !m.wide && !r.games.is_empty() { ease(m.pull) } else { 0.0 };
        beam.intensity = m.style.scene.spot_i * LAMP_UNIT * 1.6 * m.style.spot * k;
        *t = Transform::from_translation(world(chosen_at + Vec3::new(0.3, 1.6, 1.4))).looking_at(world(chosen_at), Vec3::Y);
    }
    let lean = 1.0 - (-dt * 8.0).exp();
    for (piece, mut t) in &mut wall.pieces {
        let Some(p) = m.rack.pieces.get(piece.0) else { continue };
        let want = crate::rack::home(p.x, m.rows.get(p.row).map_or(0.0, |r| r.y), m.rack_sel == Some(piece.0) && !m.wide);
        t.translation = t.translation.lerp(want.translation, lean);
        t.rotation = t.rotation.slerp(want.rotation, lean);
    }
    let aspect = camera.single().ok().and_then(|(_, _, p)| if let Projection::Perspective(p) = p { Some(p.aspect_ratio) } else { None }).unwrap_or(16.0 / 9.0);
    let (eye, look) = if m.wide { m.overview(aspect) } else { m.view() };
    let f = 1.0 - (-dt * 5.0).exp();
    m.eye = m.eye.lerp(eye, f);
    m.look = m.look.lerp(look, f);
    if let Ok((_, mut t, _)) = camera.single_mut() {
        *t = Transform::from_translation(m.eye).looking_at(m.look, Vec3::Y);
    }
    for (_, mut s) in &mut wall.lights {
        s.intensity += (m.style.lamp() * (1.0 - 0.55 * m.lift) - s.intensity) * f;
    }
    show_frame(m, now, dt, materials, &mut wall.frame);
    drift(m, now, dt, meshes, materials);
    if let Ok(mut text) = text.single_mut() {
        let r = &m.rows[m.row];
        let room = m.room_title().map(|t| format!("{t}  ·  ")).unwrap_or_default();
        let tr = |t: &str| rustyshelf_core::i18n::tr(&m.lang, t);
        let n = r.games.len();
        let unit = m.wide_unit.and_then(|u| m.rack.units.get(u).map(|x| (u, x)));
        let piece = m.rack_sel.and_then(|i| m.rack.pieces.get(i));
        let note = m.note.as_ref().filter(|(_, until)| now < *until).map(|(t, _)| t.clone());
        text.0 = if let Some(note) = note {
            note
        } else if m.held.is_some() && !r.games.is_empty() {
            let g = &l.shelves[r.shelf].games[r.games[m.col]];
            let read = if g.art.manual.is_empty() { String::new() } else { format!("  ·  {}", tr("M  Manual")) };
            format!("{}\n{}{read}", g.title, tr("Arrows  Turn it  ·  Enter  Play  ·  Esc  Put it back"))
        } else if let (true, Some((u, x))) = (m.wide, unit) {
            let k = m.rack.pieces.iter().filter(|p| p.unit == u).count();
            format!("{room}{}  ·  {}\n{}", tr(x.label), tr(&if k == 1 { "1 issue".to_string() } else { format!("{k} issues") }), tr("Enter  Zoom in  ·  Esc  Leave"))
        } else if let Some(p) = piece {
            let it = &p.item;
            let what = if m.opening.is_some() { tr("Opening…") } else { tr("Enter  Read") };
            let series = if it.series.is_empty() { String::new() } else { format!("  ·  {}", it.series) };
            format!("{}{series}\n{room}{what}", it.title)
        } else if r.games.is_empty() {
            r.name.clone()
        } else if m.wide {
            format!("{room}{}  ·  {}\n{}", r.name, tr(&if n == 1 { "1 game".to_string() } else { format!("{n} games") }), tr("Enter  Zoom in  ·  Esc  Leave"))
        } else {
            let g = &l.shelves[r.shelf].games[r.games[m.col]];
            format!("{}  ·  {}\n{room}{} of {}", g.title, r.name, m.col + 1, n)
        };
    }
}

/// The dust drifts in the lamps' light: slowly sideways, settling, back to the top once it reaches the shelf; only
/// near the rows in view.
fn drift(m: &mut Museum, now: f32, dt: f32, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let near_y = m.rows.get(m.row).map_or(0.0, |r| r.y);
    let f60 = dt * 60.0;
    for d in &mut m.dust {
        let Some(r) = m.rows.get(d.row) else { continue };
        let alpha = 0.55 * r.lamp_on * m.style.dust * (1.0 - 0.5 * m.lift);
        if let Some(mut mat) = materials.get_mut(&d.look) {
            if (mat.base_color.alpha() - alpha).abs() > 0.01 {
                mat.base_color.set_alpha(alpha);
            }
        }
        if r.lamp_on <= 0.0 || (r.y - near_y).abs() > 3.2 && !m.wide {
            continue;
        }
        for (p, seed) in d.at.iter_mut().zip(&d.seed) {
            p[0] += (now * 1000.0 / 1700.0 + seed).sin() * 0.0004 * f60;
            p[1] += ((now * 1000.0 / 2300.0 + seed * 1.3).cos() * 0.0003 - 0.00006) * f60;
            if p[1] < r.y - 0.5 {
                p[1] = r.y + 0.2;
            }
        }
        if let Some(mut mesh) = meshes.get_mut(&d.mesh) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.at.clone());
        }
    }
}

/// The wide view's frame: it eases to the shelf in view and breathes softly; gone when you zoom in.
fn show_frame(m: &mut Museum, now: f32, dt: f32, materials: &mut Assets<StandardMaterial>, sides: &mut Query<(&FrameSide, &mut Transform, &mut Visibility), (Without<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>) {
    let unit = m.wide_unit.and_then(|u| m.rack.units.get(u));
    let want = match unit {
        Some(u) => {
            let ys = m.rows.iter().filter(|r| r.wall == 0).map(|r| r.y);
            let (top, bottom) = ys.fold((f32::MIN, f32::MAX), |(t, b), y| (t.max(y), b.min(y)));
            [u.x0 - 0.16, u.x1 + 0.16, bottom - 0.5 - 0.1, top + ROW - 0.5 + 0.1]
        }
        None => m.frame_of(m.row),
    };
    let wall = if unit.is_some() { 0 } else { m.rows.get(m.row).map_or(0, |r| r.wall) };
    let snap = m.frame_k < 0.05 || wall != m.frame_wall;
    let k = if snap { 1.0 } else { 1.0 - (-dt * 14.0).exp() };
    for (a, b) in m.frame.iter_mut().zip(want) {
        *a += (b - *a) * k;
    }
    m.frame_wall = wall;
    m.frame_k += ((m.wide as u8 as f32) - m.frame_k) * (1.0 - (-dt * 8.0).exp());
    if let Some(mut mat) = materials.get_mut(&m.frame_mat) {
        mat.base_color = Color::srgba_u8(0xff, 0xd9, 0x8a, 0).with_alpha(m.frame_k * (0.55 + 0.2 * (now * 1000.0 / 420.0).sin()));
    }
    let [x0, x1, y0, y1] = m.frame;
    let (w, h, th, z) = (x1 - x0, y1 - y0, 0.035, DEPTH / 2.0 + 0.12);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (at, turn) = m.walls[wall];
    let turn = Quat::from_rotation_y(turn);
    for (side, mut t, mut vis) in sides {
        let (p, size) = match side.0 {
            0 => (Vec3::new(cx, y1, z), Vec3::new(w + th, th, 1.0)),
            1 => (Vec3::new(cx, y0, z), Vec3::new(w + th, th, 1.0)),
            2 => (Vec3::new(x0, cy, z), Vec3::new(th, h, 1.0)),
            _ => (Vec3::new(x1, cy, z), Vec3::new(th, h, 1.0)),
        };
        *t = Transform::from_translation(at + turn * p).with_rotation(turn).with_scale(size);
        *vis = if m.frame_k > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// The card beside the pulled case: drawn (off the frame) once the case is mostly out, faded in when it's out,
/// standing on the plank to the case's right unless that runs into the console, then to its left.
fn show_card(
    m: &mut Museum,
    l: &rustyshelf_core::library::Library,
    chosen_at: Vec3,
    dt: f32,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    card: &mut Query<(&mut Transform, &mut Visibility), (With<MuseumCard>, Without<Case>, Without<MuseumCamera>, Without<RowLight>)>,
) {
    let sel = (m.row, m.col);
    let Some(r) = m.rows.get(m.row).filter(|r| !r.games.is_empty()) else { return };
    let (wall_at, turn) = m.walls[r.wall];
    let Some(parts) = m.card.as_mut() else { return };
    if parts.asked != Some(sel) && m.pull > 0.5 {
        parts.asked = Some(sel);
        let s = &l.shelves[r.shelf];
        let about = card_of(&s.games[r.games[m.col]], &s.id, &r.name, m.about.get(&s.games[r.games[m.col]].id), &m.lang);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send((sel, rustyshelf_core::drawn::museum_card(&about)));
        });
        parts.drawing = Some(std::sync::Mutex::new(rx));
    }
    let drawn = parts.drawing.as_ref().and_then(|d| d.lock().ok()?.try_recv().ok());
    if let Some((for_sel, pic)) = drawn {
        parts.drawing = None;
        if let Some(mut img) = images.get_mut(&parts.image) {
            *img = crate::loading::to_image(pic.width, pic.height, pic.rgba);
        }
        parts.shows = Some(for_sel);
    }
    let mine = parts.shows == Some(sel) && parts.asked == Some(sel) && m.held.is_none();
    let want = if mine { ease(((m.pull - 0.7) / 0.3).clamp(0.0, 1.0)) } else { 0.0 };
    parts.fade += (want - parts.fade) * (1.0 - (-dt * if mine { 10.0 } else { 25.0 }).exp());
    for (h, rgb) in [(&parts.face, [1.0, 1.0, 1.0]), (&parts.board, [0x8a as f32 / 255.0, 0x6a as f32 / 255.0, 0x34 as f32 / 255.0])] {
        if let Some(mut mat) = materials.get_mut(h) {
            mat.base_color = Color::srgba(rgb[0], rgb[1], rgb[2], parts.fade);
        }
    }
    let Ok((mut t, mut vis)) = card.get_mut(parts.entity) else { return };
    *vis = if parts.fade > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
    let (right, left) = (chosen_at.x + r.w / 2.0 + 0.52 * CARD_S, chosen_at.x - r.w / 2.0 - 0.52 * CARD_S);
    let (y, z) = (r.y - 0.5 + 0.25 * CARD_S + 0.01, chosen_at.z.max((DEPTH + 0.1) / 2.0 - 0.02));
    let local = if right + 0.47 * CARD_S < r.console.0 - r.console.1 / 2.0 - 0.2 {
        Vec3::new(right, y, z)
    } else if left - 0.47 * CARD_S > -0.25 {
        Vec3::new(left, y, z)
    } else {
        Vec3::new(right, y, (DEPTH + 0.1) / 2.0 + 0.2)
    };
    let turn = Quat::from_rotation_y(turn);
    *t = Transform::from_translation(wall_at + turn * local).with_rotation(turn * Quat::from_rotation_x(-4f32.to_radians())).with_scale(Vec3::splat(CARD_S));
}

/// What the card says of a game, in the reader's language.
fn card_of(g: &rustyshelf_core::scanner::Game, shelf: &str, row: &str, about: Option<&About>, lang: &str) -> rustyshelf_core::drawn::Card {
    let tr = |t: &str| rustyshelf_core::i18n::tr(lang, t);
    let i = g.info.clone().unwrap_or_default();
    let year = i.year().map(|y| y.to_string());
    let maker = [i.dev.clone(), year].into_iter().flatten().collect::<Vec<_>>().join(", ");
    let publisher = i.publisher.clone().filter(|p| Some(p) != i.dev.as_ref());
    let line2 = [publisher, i.genre().map(String::from), Some(row.to_string())].into_iter().flatten().collect::<Vec<_>>().join("  ·  ");
    let (plays, ms) = about.map_or((0, 0), |a| (a.plays, a.ms));
    let history = if plays > 0 {
        let sessions = if plays == 1 { tr("1 session") } else { tr(&format!("{plays} sessions")) };
        let mut parts = vec![tr(&format!("{} played", play_time(ms))), sessions];
        if let Some(day) = about.map(|a| a.last).filter(|l| *l > 0) {
            parts.push(tr(&format!("last played {}", day_of(day))));
        }
        parts.join("  ·  ")
    } else {
        tr("Not played yet")
    };
    let since = (g.added > 0).then(|| { let (y, mo, _) = civil(g.added); tr(&format!("In your collection since {} {y}", MONTHS[mo as usize - 1])) });
    let a = &g.art;
    let mut chips = vec![
        (tr(if a.manual.is_empty() && a.manual_pdf.is_none() { "No manual" } else { "✓ Manual" }), !(a.manual.is_empty() && a.manual_pdf.is_none())),
        (tr(if a.front.is_some() && a.back.is_some() && a.spine.is_some() { "✓ Full box art" } else { "Box art incomplete" }), a.front.is_some() && a.back.is_some() && a.spine.is_some()),
    ];
    if !matches!(shelf, "sw" | "sw2" | "n3ds") {
        chips.push((tr(if a.disc.is_some() { "✓ Disc art" } else { "No disc art" }), a.disc.is_some()));
    }
    if let Some(n) = i.players.as_deref().and_then(|p| p.split(|c: char| !c.is_ascii_digit()).filter_map(|d| d.parse::<u32>().ok()).max()) {
        chips.push((if n > 1 { tr(&format!("1–{n} players")) } else { tr("1 player") }, true));
    }
    if let Some(e) = i.esrb.as_deref().filter(|e| !e.is_empty()) {
        let e = e.split(" (").next().unwrap_or(e).trim();
        let short = match e.to_ascii_lowercase().as_str() {
            "everyone 10+" => "E10+",
            "everyone" => "E",
            "teen" => "T",
            "mature" => "M",
            _ => e,
        };
        chips.push((short.to_string(), true));
    }
    let fav = about.is_some_and(|a| a.fav);
    rustyshelf_core::drawn::Card {
        title: g.title.clone(),
        lines: [if maker.is_empty() { row.to_string() } else { maker }, line2],
        rating: i.rating.map(|r| { let r = r.clamp(0.0, 5.0); ((r * 10.0).round() as u32, tr(&format!("{r:.1} community"))) }),
        metacritic: i.metacritic.map(|m| m.round() as u32),
        history,
        since,
        chips,
        foot: format!("{}{}", if fav { format!("{}    ", tr("♥ favorite")) } else { String::new() }, tr("Enter  Play  ·  Space  Pick it up")),
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Play time as the Electron app words it: "under a minute", "45 min", "2 h", "2 h 5 min".
fn play_time(ms: u64) -> String {
    let m = (ms as f64 / 60_000.0).round() as u64;
    match m {
        0 => "under a minute".into(),
        1..=59 => format!("{m} min"),
        _ if m.is_multiple_of(60) => format!("{} h", m / 60),
        _ => format!("{} h {} min", m / 60, m % 60),
    }
}

/// A day mid-sentence: "today", "yesterday", "3 days ago", else its date ("Mar 5, 2025").
fn day_of(ms: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    match now.saturating_sub(ms) / 86_400_000 {
        0 => "today".into(),
        1 => "yesterday".into(),
        d @ 2..=6 => format!("{d} days ago"),
        _ => {
            let (y, m, d) = civil(ms);
            format!("{} {d}, {y}", &MONTHS[m as usize - 1][..3])
        }
    }
}

/// The calendar day (UTC) of a time in milliseconds since 1970: year, month 1 to 12, day.
fn civil(ms: u64) -> (i64, u32, u32) {
    let z = (ms / 86_400_000) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The gallery pieces the room asks for, kept to the sides and the foreground so they frame the shelves without ever
/// covering a case: a white plinth each side with a favourite game under glass and its own light, velvet ropes in
/// front of them, a bench facing the shelves, and two framed posters of favourites. `at`: the back wall's width, the
/// reading corner's end, whether the room has side walls, the floor's height and the shelves' middle.
#[allow(clippy::too_many_arguments)]
fn gallery(
    commands: &mut Commands,
    parent: Entity,
    style: &Style,
    (max_w, rack_end, rooms, floor, mid_y): (f32, Option<f32>, bool, f32, f32),
    favourites: &[Art],
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    covers: &mut Covers,
    loader: &ArtLoader,
) {
    let mut put = |c: &mut Commands, mesh: Mesh, mat: Handle<StandardMaterial>, t: Transform| {
        c.spawn((layer(), Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat), t, ChildOf(parent)));
    };
    let metal = materials.add(StandardMaterial { base_color: hex(0x2a2c31), metallic: 0.85, perceptual_roughness: 0.3, ..default() });
    let brass = materials.add(StandardMaterial { base_color: hex(0xb08a48), metallic: 0.9, perceptual_roughness: 0.25, ..default() });
    let sides = if rooms { [-0.62, max_w + 0.62] } else { [-2.6, rack_end.unwrap_or(max_w) + 2.6] };
    let fav = |k: usize| favourites.get((k + 2) % favourites.len().max(1)).or(favourites.get(k)).cloned();
    if style.plinths {
        let white = materials.add(StandardMaterial { base_color: hex(0xe9e6df), perceptual_roughness: 0.85, ..default() });
        let glass = materials.add(StandardMaterial { base_color: Color::srgba(0.87, 0.93, 1.0, 0.12), perceptual_roughness: 0.02, metallic: 0.1, alpha_mode: AlphaMode::Blend, ..default() });
        for (k, px) in sides.into_iter().enumerate() {
            put(commands, Cuboid::new(1.05, 1.45, 1.05).into(), white.clone(), Transform::from_xyz(px, floor + 0.725, 0.75));
            let case = materials.add(StandardMaterial { base_color: hex(0x1a1b20), perceptual_roughness: 0.35, ..default() });
            let turn = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2 + if k == 1 { -0.35 } else { 0.35 });
            put(commands, Cuboid::new(0.055, 0.8, 0.56).into(), case, Transform::from_xyz(px, floor + 1.45 + 0.42, 0.75).with_rotation(turn));
            if let Some(art) = fav(k) {
                let front = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.35, ..default() });
                covers.want(art, (front.clone(), 0.56 / 0.8), loader, materials);
                // the cover on the case's face toward the room
                put(commands, Rectangle::new(0.55, 0.79).into(), front, Transform::from_xyz(px, floor + 1.45 + 0.42, 0.75).with_rotation(turn) * Transform::from_xyz(0.029, 0.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)));
            }
            put(commands, Cuboid::new(0.88, 0.98, 0.88).into(), glass.clone(), Transform::from_xyz(px, floor + 1.45 + 0.49, 0.75));
            commands.spawn((layer(), PointLight { color: hex(0xfff0dc), intensity: 22_000.0, range: 2.2, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(px, floor + 1.45 + 0.95, 0.78), ChildOf(parent)));
        }
    }
    if style.ropes {
        let velvet = materials.add(StandardMaterial { base_color: hex(0x7a0e18), perceptual_roughness: 0.85, ..default() });
        let spread = if rooms { 0.55 } else { 0.9 };
        for px in sides {
            for x in [px - spread, px + spread] {
                put(commands, Cylinder::new(0.042, 1.35).into(), brass.clone(), Transform::from_xyz(x, floor + 0.675, 1.75));
                put(commands, Sphere::new(0.07).into(), brass.clone(), Transform::from_xyz(x, floor + 1.38, 1.75));
                put(commands, Cylinder::new(0.235, 0.055).into(), brass.clone(), Transform::from_xyz(x, floor + 0.028, 1.75));
            }
            // the rope sags between the posts: short straight lengths along the curve
            let at = |t: f32| Vec3::new(px - spread + 2.0 * spread * t, floor + 1.26 - 0.28 * (1.0 - (2.0 * t - 1.0).powi(2)), 1.75);
            for i in 0..12 {
                let (a, b) = (at(i as f32 / 12.0), at((i + 1) as f32 / 12.0));
                let mid = (a + b) / 2.0;
                let len = a.distance(b);
                let turn = Quat::from_rotation_arc(Vec3::Y, (b - a).normalize());
                put(commands, Cylinder::new(0.032, len + 0.01).into(), velvet.clone(), Transform::from_translation(mid).with_rotation(turn));
            }
        }
    }
    if style.bench {
        let (bz, bx) = (4.4, max_w / 2.0 - if rooms { 3.4 } else { 0.0 });
        put(commands, Cuboid::new(2.8, 0.14, 0.72).into(), materials.add(StandardMaterial { base_color: hex(0x23201d), perceptual_roughness: 0.55, ..default() }), Transform::from_xyz(bx, floor + 0.62, bz));
        put(commands, Cuboid::new(2.72, 0.07, 0.64).into(), materials.add(StandardMaterial { base_color: hex(0x3a1f1c), perceptual_roughness: 0.9, ..default() }), Transform::from_xyz(bx, floor + 0.725, bz));
        for (dx, dz) in [(-1.3, -0.28), (-1.3, 0.28), (1.3, -0.28), (1.3, 0.28)] {
            put(commands, Cuboid::new(0.055, 0.55, 0.055).into(), metal.clone(), Transform::from_xyz(bx + dx, floor + 0.275, bz + dz));
        }
    }
    if style.posters {
        let frame = materials.add(StandardMaterial { base_color: hex(0x1c140c), perceptual_roughness: 0.4, metallic: 0.2, ..default() });
        let mount = materials.add(StandardMaterial { base_color: hex(0xeee8dc), perceptual_roughness: 0.9, ..default() });
        let (fw, fh, py) = (0.95, 1.3, mid_y + 0.35);
        for (k, art) in favourites.iter().take(2).enumerate() {
            let px = if k == 1 { rack_end.unwrap_or(max_w) + 1.75 } else { -1.75 };
            put(commands, Cuboid::new(fw + 0.1, fh + 0.1, 0.05).into(), frame.clone(), Transform::from_xyz(px, py, -DEPTH / 2.0 + 0.03));
            put(commands, Rectangle::new(fw, fh).into(), mount.clone(), Transform::from_xyz(px, py, -DEPTH / 2.0 + 0.056));
            let pic = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.5, ..default() });
            covers.want(art.clone(), (pic.clone(), fw / fh), loader, materials);
            put(commands, Rectangle::new(fw * 0.8, fh * 0.8).into(), pic, Transform::from_xyz(px, py, -DEPTH / 2.0 + 0.058));
        }
    }
}

/// A shelf's own glow colour, which its row's lamp takes in Neon night.
fn style_glow(shelf: &str) -> rustyshelf_core::looks::Rgba {
    looks::look(shelf).glow
}

/// A plaque as bright as its row's light has come on.
fn plaque_tint(lamp_on: f32) -> Color {
    let v = 0.3 + 0.6 * lamp_on;
    Color::srgb(v, v, v)
}

fn ease(k: f32) -> f32 {
    k * k * (3.0 - 2.0 * k)
}

/// A step through the arcade: walking and turning, Enter at a cabinet plays its game, Escape walks back out to the
/// museum's wall.
#[allow(clippy::too_many_arguments)]
fn walk_on(
    commands: &mut Commands,
    m: &mut Museum,
    l: &rustyshelf_core::library::Library,
    time: &Time,
    keys: &ButtonInput<KeyCode>,
    pads: &Query<&Gamepad>,
    settings: &crate::settings::Settings,
    cursor: &mut Cursor,
    sounds: &mut crate::sound::Sounds,
    (covers, loader, meshes, materials, images): (&mut Covers, &ArtLoader, &mut Assets<Mesh>, &mut Assets<StandardMaterial>, &mut Assets<Image>),
    camera: &mut Cam,
    things: &Query<(Entity, Has<MuseumHud>), Or<(With<MuseumThing>, With<MuseumHud>)>>,
    text: &mut Query<&mut Text, With<MuseumText>>,
    shelf_camera: &mut Query<&mut Camera, (With<crate::shelf::ShelfCamera>, Without<MuseumCamera>)>,
) {
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || pad(settings.confirm().1) {
        m.walk = None;
        for (e, _) in things.iter().filter(|(_, hud)| !hud) {
            commands.entity(e).despawn();
        }
        let held_cursor = Cursor { shelf: usize::MAX, ..Cursor::default() };
        build(commands, m, l, &held_cursor, meshes, materials, covers, loader, images);
        sounds.play("back", 1);
        return;
    }
    let Some(w) = m.walk.as_mut() else { return };
    if keys.just_pressed(KeyCode::Enter) || pad(settings.confirm().0) {
        if let Some(k) = w.nearest() {
            (cursor.shelf, cursor.game, cursor.land) = (w.shelf, w.cabinets[k].0, false);
            settings_play(commands);
            m.back_to = Some(BackTo { walk: Some((w.x, w.z, w.ry)), at: time.elapsed_secs(), started: false });
            m.walk = None;
            return close(commands, m, things, camera, shelf_camera, sounds);
        }
    }
    let dt = time.delta_secs().min(0.05);
    let key = |a: KeyCode, b: KeyCode| (keys.pressed(a) || keys.pressed(b)) as i32 as f32;
    let (mut forward, mut side, mut turn) = (key(KeyCode::ArrowUp, KeyCode::KeyW) - key(KeyCode::ArrowDown, KeyCode::KeyS), key(KeyCode::KeyD, KeyCode::KeyD) - key(KeyCode::KeyA, KeyCode::KeyA), key(KeyCode::ArrowLeft, KeyCode::KeyQ) - key(KeyCode::ArrowRight, KeyCode::KeyE));
    for p in pads {
        let (l, r, d) = (p.left_stick(), p.right_stick(), p.dpad());
        let dead = |v: f32| if v.abs() > 0.18 { v } else { 0.0 };
        forward += dead(l.y) + d.y;
        side += dead(l.x);
        turn -= dead(r.x) + d.x;
    }
    let fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let speed = if fast { 4.2 } else { 3.2 } * dt;
    w.ry += turn.clamp(-1.0, 1.0) * 2.2 * dt;
    if forward != 0.0 || side != 0.0 {
        let k = Vec2::new(side, forward).length().max(1.0);
        w.step(forward / k * speed, side / k * speed);
    }
    if let Ok((_, mut t, _)) = camera.single_mut() {
        *t = w.eye();
    }
    if let Ok(mut text) = text.single_mut() {
        let lang = l.config.raw().get("ui").and_then(|u| u.get("language")).and_then(|v| v.as_str()).unwrap_or("en");
        let near = w.nearest().map(|k| {
            let g = &l.shelves[w.shelf].games[w.cabinets[k].0];
            format!("{}  {}", rustyshelf_core::i18n::tr(lang, "Enter  Play"), g.title)
        });
        text.0 = format!("{}\n{}", w.place(), near.unwrap_or_else(|| rustyshelf_core::i18n::tr(lang, "Up / Down  walk  ·  Left / Right  turn  ·  Esc  back to the museum")));
    }
}

/// Asks the shelf to play the game under the cursor once the museum has closed.
fn settings_play(commands: &mut Commands) {
    commands.queue(|world: &mut World| world.resource_mut::<crate::settings::Settings>().play = true);
}

/// The game in view and its row, under the wall.
fn spawn_text(commands: &mut Commands, camera: Option<Entity>, mona: &crate::hud::MonaSans) {
    let Some(camera) = camera else { return };
    commands.spawn((
        MuseumHud,
        UiTargetCamera(camera),
        Node { position_type: PositionType::Absolute, bottom: px(28), width: percent(100), justify_content: JustifyContent::Center, ..default() },
        children![(MuseumText, Text::default(), mona.font(500, 20.0), TextLayout::justify(Justify::Center), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)))],
    ));
}

fn close(
    commands: &mut Commands,
    m: &mut Museum,
    things: &Query<(Entity, Has<MuseumHud>), Or<(With<MuseumThing>, With<MuseumHud>)>>,
    camera: &mut Cam,
    shelf_camera: &mut Query<&mut Camera, (With<crate::shelf::ShelfCamera>, Without<MuseumCamera>)>,
    sounds: &mut crate::sound::Sounds,
) {
    if let Ok(mut shelf) = shelf_camera.single_mut() {
        shelf.is_active = true;
    }
    for (e, _) in things {
        commands.entity(e).despawn();
    }
    if let Ok((mut cam, _, _)) = camera.single_mut() {
        cam.is_active = false;
    }
    m.open = false;
    m.rows.clear();
    sounds.play("back", 1);
}

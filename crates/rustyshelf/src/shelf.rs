// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the shelf is laid
// out as Boxshelf's renderer/app.js (measure, place) and renderer/style.css lay it out, as RustyShelf has them.
//! The shelf: the chosen game's case forward in the middle, the rest in two angled stacks beside it, over the
//! theme's backdrop. Everything is measured in the Electron app's CSS pixels, seen through its perspective.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::camera::{ClearColorConfig, SubCameraView};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use rustyshelf_core::drawn::Picture;
use rustyshelf_core::looks::{self, Rgba};
use rustyshelf_core::scanner::{Source, Tile};
use rustyshelf_core::themefx as themes_fx;
use rustyshelf_core::themes;

use crate::loading::{to_image, Art, ArtLoader, LibraryRes, LibraryState};

/// How far the eye is from the shelf, and how far down the window it looks (style.css: #scene's perspective).
const EYE: f32 = 1800.0;
const EYE_Y: f32 = 0.42;
/// how far a printed sheet sits off the case under it, for the case's height
const SKIN: f32 = 0.0008;
/// cases further from the chosen one than this aren't shown (app.js WIN, less the two that fade)
const IN_VIEW: usize = 13;
/// seconds a case takes to its place, and to turn round (style.css: .slot and .box)
const SLIDE: f32 = 0.55;
const TURN: f32 = 0.75;
/// how much of a cover shows again under the chosen case, and how strongly at its top (setup.css: the front's
/// -webkit-box-reflect)
const MIRRORED: f32 = 0.42;
/// (.3 in the browser: a light picture over the dark floor is this strong in linear light)
const MIRROR_TOP: f32 = 0.11;
/// White blended in linear light comes out stronger than a browser's: this much of a CSS white's alpha looks the
/// same over a cover.
const WHITE: f32 = 0.7;

/// Which game is chosen, on which shelf.
#[derive(Resource)]
pub struct Cursor {
    pub shelf: usize,
    pub game: usize,
    /// the shelf whose boxes are standing now
    pub built: Option<usize>,
    /// the chosen case shows its back
    pub flipped: bool,
    /// a shelf just come to: its first game is chosen (past the shelf's own cases) when it's stood up
    pub land: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor { shelf: 0, game: 0, built: None, flipped: false, land: true }
    }
}

/// The hue the floor, the halo and the tabs are lit in: the theme's, for the shelf's colour under the chosen case.
#[derive(Resource, Default)]
pub struct Lit {
    pub glow: f32,
    /// the shelf's own colour there, before the theme turns it
    pub shelf: f32,
}

/// The window and a case on the shelf, in CSS pixels (app.js measure()).
#[derive(Resource, Clone, Copy, PartialEq, Default)]
pub struct Stage {
    pub w: f32,
    pub h: f32,
    pub bw: f32,
    pub bh: f32,
    pub bd: f32,
}

/// Where a case stands: across, toward the eye, and turned (degrees).
#[derive(Clone, Copy, PartialEq, Default)]
struct Place {
    x: f32,
    z: f32,
    ry: f32,
}

/// A value on its way from one number to another.
#[derive(Clone, Copy)]
struct Eased {
    from: f32,
    to: f32,
    since: f32,
}

impl Eased {
    fn at(v: f32) -> Eased {
        Eased { from: v, to: v, since: f32::MIN }
    }

    /// Where it is `now`, on a CSS cubic-bezier curve `seconds` long.
    fn now(&self, now: f32, seconds: f32, curve: [f32; 4]) -> f32 {
        self.from + (self.to - self.from) * bezier(curve, (now - self.since) / seconds)
    }

    fn head_for(&mut self, to: f32, here: f32, now: f32) {
        if to != self.to {
            *self = Eased { from: here, to, since: now };
        }
    }
}

#[derive(Component)]
pub struct Slot {
    pub index: usize,
    /// the game's own colour: its title's, then its cover's once that's loaded
    hue: f32,
    front_key: PathBuf,
    from: Place,
    to: Place,
    since: f32,
    /// how far round it's turned, in degrees
    turn: Eased,
    /// how dark the film over its cover is
    dim: Eased,
    front: Handle<StandardMaterial>,
    /// its cover again, in the floor, and the shape of the sheet both are on
    mirrored: Handle<StandardMaterial>,
    sheet: f32,
    glare: Handle<StandardMaterial>,
    /// its back and its spine's picture, asked for once it's chosen: each one's picture, and the sheet it goes on
    late: Vec<(Art, Sheet)>,
}

/// The part of a case that turns round.
#[derive(Component)]
pub struct Turning;

/// A layer that looks one way on the chosen case and another on the rest: the plastic over a cover, a spine's
/// shading.
#[derive(Component)]
pub struct Either([Handle<StandardMaterial>; 2]);

/// What only the chosen case wears: the light inside its covers' edges, its cover mirrored in the floor.
#[derive(Component)]
pub struct OnChosen;

#[derive(Component)]
pub struct ShelfCamera;

/// The backdrop's two pictures (the one fading in over the other), what moves over them in the theme, and the glow
/// behind the chosen case.
#[derive(Component)]
pub enum Behind {
    Old,
    New,
    Moving,
    Halo,
}

/// A sheet waiting for its picture: its material, and its width for its height (the picture is cut to it; a sheet
/// of no shape takes the picture whole).
type Sheet = (Handle<StandardMaterial>, f32);

/// Pictures in flight or loaded: which sheets wait for which file.
#[derive(Resource, Default)]
pub struct Covers {
    waiting: HashMap<PathBuf, Vec<Sheet>>,
    loaded: HashMap<PathBuf, Option<(Handle<Image>, f32)>>,
    /// a cover's most colourful colour, as a hue
    hues: HashMap<PathBuf, f32>,
    /// a cover shrunk to a few pixels, for the backdrop
    swatches: HashMap<PathBuf, Arc<Vec<u8>>>,
}

impl Covers {
    /// A game's cover as it's on the shelf `own`, once it's loaded.
    pub fn of(&self, g: &rustyshelf_core::scanner::Game, own: &str) -> Option<Handle<Image>> {
        let key = cover_of(g, own, &looks::look_for(own, g)).0.key();
        self.loaded.get(&key).and_then(|c| c.as_ref()).map(|c| c.0.clone())
    }

    /// Puts a picture on a sheet: at once when it's loaded, else when it arrives.
    fn want(&mut self, art: Art, sheet: Sheet, loader: &ArtLoader, materials: &mut Assets<StandardMaterial>) {
        let key = art.key();
        match self.loaded.get(&key) {
            Some(Some((img, aspect))) => show(materials, &sheet, img, *aspect),
            Some(None) => {}
            None => {
                if !self.waiting.contains_key(&key) {
                    loader.request(art);
                }
                self.waiting.entry(key).or_default().push(sheet);
            }
        }
    }
}

/// A picture on its sheet, cut to the sheet's shape from its middle (never stretched).
fn show(materials: &mut Assets<StandardMaterial>, sheet: &Sheet, img: &Handle<Image>, aspect: f32) {
    let Some(mut m) = materials.get_mut(&sheet.0) else { return };
    let (scale, offset) = if sheet.1 <= 0.0 {
        (Vec2::ONE, Vec2::ZERO)
    } else if aspect > sheet.1 {
        let part = sheet.1 / aspect;
        (Vec2::new(part, 1.0), Vec2::new((1.0 - part) / 2.0, 0.0))
    } else {
        let part = aspect / sheet.1;
        (Vec2::new(1.0, part), Vec2::new(0.0, (1.0 - part) / 2.0))
    };
    m.base_color_texture = Some(img.clone());
    m.uv_transform = bevy::math::Affine2::from_scale_angle_translation(scale, 0.0, offset);
}

fn color(c: Rgba) -> Color {
    Color::srgba(c[0], c[1], c[2], c[3])
}

/// CSS's hsl(hue, 95%, 62%): the colour the floor, the halo and a cover's glow are lit in.
fn lit(hue: f32, saturation: f32) -> Color {
    Color::hsl(hue.rem_euclid(360.0), saturation, 0.62)
}

/// A CSS cubic-bezier(x1, y1, x2, y2) timing curve at `t`.
pub fn bezier([x1, y1, x2, y2]: [f32; 4], t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let at = |a: f32, b: f32, s: f32| 3.0 * a * s * (1.0 - s) * (1.0 - s) + 3.0 * b * s * s * (1.0 - s) + s * s * s;
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..20 {
        let mid = (lo + hi) / 2.0;
        if at(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(y1, y2, (lo + hi) / 2.0)
}

/// CSS's "ease", "ease-in-out", and the curves a case slides and turns on.
const EASE: [f32; 4] = [0.25, 0.1, 0.25, 1.0];
const EASE_IN_OUT: [f32; 4] = [0.42, 0.0, 0.58, 1.0];
const SLIDE_CURVE: [f32; 4] = [0.22, 0.9, 0.25, 1.0];
const TURN_CURVE: [f32; 4] = [0.3, 0.95, 0.25, 1.0];

/// The most colourful bright colour of a picture seen 12 pixels across, as a hue (app.js hueOf).
fn hue_of(w: u32, h: u32, rgba: &[u8]) -> Option<f32> {
    let (mut best, mut most) = (None, -1.0);
    for cy in 0..12 {
        for cx in 0..12 {
            let (x0, y0) = (cx * w / 12, cy * h / 12);
            let (x1, y1) = (((cx + 1) * w / 12).max(x0 + 1).min(w), ((cy + 1) * h / 12).max(y0 + 1).min(h));
            let mut sum = [0.0f32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += f32::from(rgba[i + c]);
                    }
                }
            }
            let n = ((x1.saturating_sub(x0)) * (y1.saturating_sub(y0))).max(1) as f32 * 255.0;
            let (r, g, b) = (sum[0] / n, sum[1] / n, sum[2] / n);
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let s = mx - mn;
            if s > most && mx > 0.25 {
                most = s;
                let hue = if s == 0.0 {
                    0.0
                } else if mx == r {
                    ((g - b) / s) % 6.0
                } else if mx == g {
                    (b - r) / s + 2.0
                } else {
                    (r - g) / s + 4.0
                };
                best = Some(((hue * 60.0).round() + 360.0) % 360.0);
            }
        }
    }
    best
}

fn picture(images: &mut Assets<Image>, p: Picture) -> Handle<Image> {
    images.add(to_image(p.width, p.height, p.rgba))
}

/// A CSS background of white and black over a cover, at twice the pixels it's shown in, its alpha set for
/// blending in linear light: a black as dark as the browser's, a white as bright.
fn overlay(images: &mut Assets<Image>, css: &str, w: f32, h: f32) -> Handle<Image> {
    let mut p = rustyshelf_core::cssbg::paint(css, (w * 2.0).round().max(2.0) as u32, (h * 2.0).round().max(2.0) as u32, 2.0);
    for px in p.rgba.as_chunks_mut::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        px[3] = (if px[0] > 128 { a * WHITE } else { 1.0 - (1.0 - a).powf(2.2) } * 255.0) as u8;
    }
    picture(images, p)
}

/// The band of light across a cover (style.css .glare), for a face `bw` × `bh`: the layer is 1.2 faces wide.
fn glare_css(bw: f32, bh: f32) -> String {
    let angle = 104f32.to_radians();
    let len = (1.2 * bw * angle.sin()).abs() + (bh * angle.cos()).abs();
    let at = |part: f32, faces: f32| format!("{:.2}px", part / 100.0 * len + faces * bw);
    format!(
        "linear-gradient(104deg,transparent {},rgba(255,255,255,.07) {},rgba(255,255,255,.34) {},rgba(255,255,255,.30) {},rgba(255,255,255,.06) {},transparent {}),linear-gradient(104deg,transparent {},rgba(255,255,255,.12) {},transparent {})",
        at(41.3, -0.151948),
        at(46.3, -0.064622),
        at(49.7, -0.00524),
        at(50.3, 0.00524),
        at(53.3, 0.057636),
        at(58.0, 0.139723),
        at(54.7, 0.082087),
        at(56.7, 0.117018),
        at(58.3, 0.144962)
    )
}

fn erf(x: f32) -> f32 {
    let a = 0.147;
    x.signum() * (1.0 - (-x * x * (4.0 / PI + a * x * x) / (1.0 + a * x * x)).exp()).sqrt()
}

/// The light inside the edges of the chosen case's cover (setup.css .slot.sel's inset box-shadow: 22px wide, 4px
/// back from the edge), white for the game's colour to tint: a face `w` × `h` pixels.
fn rim_picture(w: f32, h: f32) -> Picture {
    let (pw, ph) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
    let mut rgba = vec![255u8; (pw * ph * 4) as usize];
    // what's left of the face's own shape, grown 4px, under a Gaussian 11px wide
    let clear = |at: f32, len: f32| 0.5 * (erf((at + 4.0) / (11.0 * std::f32::consts::SQRT_2)) + erf((len + 4.0 - at) / (11.0 * std::f32::consts::SQRT_2)));
    for y in 0..ph {
        for x in 0..pw {
            let a = 1.0 - clear(x as f32 + 0.5, w) * clear(y as f32 + 0.5, h);
            rgba[((y * pw + x) * 4 + 3) as usize] = (a * 255.0) as u8;
        }
    }
    Picture { width: pw, height: ph, rgba }
}

/// A cover's sheet mirrored below it: as wide, `MIRRORED` as tall, fading out downward.
fn mirror() -> Mesh {
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [0.5, -MIRRORED, 0.0], [-0.5, -MIRRORED, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 1.0 - MIRRORED], [0.0, 1.0 - MIRRORED]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, MIRROR_TOP], [1.0, 1.0, 1.0, MIRROR_TOP], [1.0, 1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 0.0]])
        .with_inserted_indices(Indices::U32(vec![0, 3, 2, 0, 2, 1]))
}

/// What every case is made of: the shape of its faces, the plastic and the light.
#[derive(Resource)]
pub struct Kit {
    face: Handle<Mesh>,
    /// over the chosen case, and over the rest
    plastic: [Handle<StandardMaterial>; 2],
    glare: Handle<Image>,
    /// the light inside the chosen cover's edges, in its game's colour
    rim: Handle<StandardMaterial>,
    /// a left spine's shading on the chosen case and on the rest, and a right spine's
    shade: [Handle<StandardMaterial>; 3],
    /// the pale strip of floor that mirrors a case
    strip: Handle<StandardMaterial>,
    mirror: Handle<Mesh>,
    heart: Handle<StandardMaterial>,
    /// the case the plastic and the light were painted for
    made_for: (f32, f32),
}

#[derive(Clone)]
struct Wanted {
    theme: String,
    sh: f64,
    hue: f64,
    /// the chosen game's cover, and its swatch once that's made
    cover: (PathBuf, Option<Arc<Vec<u8>>>),
    stage: Stage,
    pixels: (u32, u32),
}

impl PartialEq for Wanted {
    fn eq(&self, o: &Wanted) -> bool {
        (&self.theme, self.sh, self.hue, &self.cover.0, self.cover.1.is_some(), self.stage, self.pixels) == (&o.theme, o.sh, o.hue, &o.cover.0, o.cover.1.is_some(), o.stage, o.pixels)
    }
}

/// What paints the backdrop, off the frame: the newest one asked for is the one painted.
#[derive(Resource)]
pub struct Backdrop {
    ask: Sender<Wanted>,
    done: Mutex<Receiver<Picture>>,
    asked: Option<Wanted>,
    shown: bool,
    /// when the newest picture began to fade in
    since: f32,
    /// the size the halo's picture was painted for
    halo: (f32, f32),
}

impl Default for Backdrop {
    fn default() -> Self {
        let (ask, jobs) = channel::<Wanted>();
        let (finished, done) = channel();
        std::thread::spawn(move || {
            while let Ok(mut job) = jobs.recv() {
                while let Ok(newer) = jobs.try_recv() {
                    job = newer;
                }
                let stage = themes::Stage { w: job.stage.w, h: job.stage.h, case_h: job.stage.bh };
                let cover = job.cover.1.as_ref().map(|c| c.as_slice());
                if finished.send(themes::backdrop(&job.theme, job.sh, job.hue, None, cover, stage, job.pixels)).is_err() {
                    return;
                }
            }
        });
        Backdrop { ask, done: Mutex::new(done), asked: None, shown: false, since: 0.0, halo: (0.0, 0.0) }
    }
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(ClearColor(Color::srgb_u8(7, 7, 12)));
    let behind = commands.spawn((Camera2d, Camera { order: 0, ..default() })).id();
    commands.spawn((ShelfCamera, Camera3d::default(), Camera { order: 1, clear_color: ClearColorConfig::None, ..default() }, IsDefaultUiCamera, Transform::from_xyz(0.0, 0.0, EYE)));
    let whole = || Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() };
    commands.spawn((
        whole(),
        UiTargetCamera(behind),
        children![
            (Behind::Old, whole(), ImageNode::default().with_color(Color::NONE)),
            (Behind::New, whole(), ImageNode::default().with_color(Color::NONE)),
            (Behind::Moving, whole(), ImageNode::default().with_color(Color::NONE)),
            (Behind::Halo, Node { position_type: PositionType::Absolute, ..default() }, ImageNode::default().with_color(Color::NONE)),
        ],
    ));
    let clear = |m: &mut Assets<StandardMaterial>, c: Color| m.add(StandardMaterial { base_color: c, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
    commands.insert_resource(Kit {
        face: meshes.add(Rectangle::new(1.0, 1.0)),
        plastic: [clear(&mut materials, Color::WHITE), clear(&mut materials, Color::srgba(1.0, 1.0, 1.0, 0.55))],
        glare: Handle::default(),
        rim: clear(&mut materials, Color::NONE),
        shade: [clear(&mut materials, Color::WHITE), clear(&mut materials, Color::WHITE), clear(&mut materials, Color::WHITE)],
        strip: materials.add(StandardMaterial { unlit: true, alpha_mode: AlphaMode::Blend, cull_mode: None, double_sided: true, ..default() }),
        mirror: meshes.add(mirror()),
        heart: clear(&mut materials, Color::WHITE),
        made_for: (0.0, 0.0),
    });
}

/// The window's and the cases' measures, and the eye looking at the shelf through them.
pub fn measure(
    lib: Res<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    settings: Res<crate::settings::Settings>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut stage: ResMut<Stage>,
    mut camera: Query<(&mut Camera, &mut Projection), With<ShelfCamera>>,
    mut kit: ResMut<Kit>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(window) = window.single() else { return };
    let (vw, vh) = (window.width().max(1.0), window.height().max(1.0));
    let shelf = match &lib.0 {
        LibraryState::Ready(library) => library.shelves.get(cursor.shelf).map_or("home", |s| s.id.as_str()),
        _ => "home",
    };
    let look = looks::look(shelf);
    let bh = if vw < 760.0 { (vh * 0.32).min(vw * 0.42 / look.ratio) } else { (vh * 0.4).min(vw * 0.22 / look.ratio).min(620.0) } * settings.box_scale(shelf);
    let now = Stage { w: vw, h: vh, bw: bh * look.ratio, bh, bd: bh * look.on_shelf(shelf).depth };
    if *stage == now {
        return;
    }
    *stage = now;
    if let Ok((mut cam, mut projection)) = camera.single_mut() {
        // the eye looks 42% of the way down the window: the window is that part of a taller view
        let below = 1.0 - EYE_Y;
        *projection = Projection::Perspective(PerspectiveProjection { fov: 2.0 * (vh * below / EYE).atan(), near: 50.0, far: 20_000.0, ..default() });
        let (w, h) = (vw.round() as u32, vh.round() as u32);
        cam.sub_camera_view = Some(SubCameraView {
            full_size: UVec2::new(w * 100, h * (below * 200.0).round() as u32),
            offset: Vec2::new(0.0, h as f32 * (below * 200.0 - 100.0).round()),
            size: UVec2::new(w * 100, h * 100),
        });
    }
    if kit.made_for != (now.bw, now.bh) {
        kit.made_for = (now.bw, now.bh);
        // the cases standing wear the old size's light
        cursor.built = None;
        let plastic = overlay(
            &mut images,
            "linear-gradient(180deg,rgba(255,255,255,.16),rgba(255,255,255,.04) 18%,transparent 34%),linear-gradient(90deg,rgba(0,0,0,.14),transparent 10%,transparent 88%,rgba(0,0,0,.18))",
            now.bw,
            now.bh,
        );
        for m in &kit.plastic {
            if let Some(mut m) = materials.get_mut(m) {
                m.base_color_texture = Some(plastic.clone());
            }
        }
        kit.glare = overlay(&mut images, &glare_css(now.bw, now.bh), now.bw * 1.2, now.bh);
        let mut wear = |m: &Handle<StandardMaterial>, image: Handle<Image>| {
            if let Some(mut m) = materials.get_mut(m) {
                m.base_color_texture = Some(image);
            }
        };
        wear(&kit.rim, picture(&mut images, rim_picture(now.bw, now.bh)));
        wear(&kit.heart, picture(&mut images, rustyshelf_core::drawn::heart(104)));
        // the curve of a spine, as style.css and setup.css shade it (.f-left::after chosen and not, .f-right::after)
        for (m, css) in kit.shade.iter().zip([
            "linear-gradient(90deg,rgba(0,0,0,.15),rgba(255,255,255,.14) 45%,rgba(0,0,0,.2))",
            "linear-gradient(90deg,rgba(0,0,0,.35),rgba(255,255,255,.06) 45%,rgba(0,0,0,.25))",
            "linear-gradient(90deg,rgba(0,0,0,.25),rgba(255,255,255,.06) 55%,rgba(0,0,0,.35))",
        ]) {
            wear(m, overlay(&mut images, css, now.bd, 8.0));
        }
        wear(&kit.strip, overlay(&mut images, "linear-gradient(180deg,rgba(255,255,255,.08),transparent 80%)", 8.0, now.bh * 0.38));
    }
}

/// The colour a media app's drawn cover is made around, for the apps that have their own.
fn app_hue(app: &str) -> Option<f32> {
    match app {
        "stremio" => Some(265.0),
        "nuvio" => Some(200.0),
        _ => None,
    }
}

/// A game's cover on the shelf `own`, and the colour that's the game's until its cover is seen: its own picture,
/// or one drawn from its title; the shelf's own cases have theirs (the emulator's is drawn from its name and its
/// shelf's).
fn cover_of(g: &rustyshelf_core::scanner::Game, own: &str, look: &looks::Look) -> (Art, f32) {
    let tile = match g.source {
        Source::Tile(t) => Some(t),
        _ => None,
    };
    let app = matches!(g.source, Source::App { .. }) || tile == Some(Tile::Emulator);
    let hue = g.id.strip_prefix("app-").filter(|_| app).and_then(app_hue);
    let seed = tile.map(|_| rustyshelf_core::drawn::hash(&format!("{}{own}", g.title)));
    let cover = match (g.art.front.clone(), tile) {
        (Some(path), _) => Art::File(path),
        (None, Some(t)) if t != Tile::Emulator => Art::Tile { look: Box::new(look.clone()), tile: t },
        (None, _) => Art::Front { look: Box::new(look.clone()), title: g.title.clone(), region: g.region.clone(), scene: app.then_some("media"), hue, seed },
    };
    (cover, hue.unwrap_or((seed.unwrap_or_else(|| rustyshelf_core::drawn::hash(&g.title)) % 360) as f32))
}

/// The Surprise me case wears the cover of the game it landed on, and its own again once that's played.
pub fn reveal(
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    surprise: Res<crate::play::Surprise>,
    loader: Res<ArtLoader>,
    mut covers: ResMut<Covers>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    slots: Query<&Slot>,
    mut worn: Local<Option<(String, usize)>>,
) {
    if surprise.pick == *worn {
        return;
    }
    *worn = surprise.pick.clone();
    let LibraryState::Ready(library) = &lib.0 else { return };
    let Some(shelf) = library.shelves.get(cursor.shelf) else { return };
    let Some(at) = shelf.games.iter().position(|g| g.source == Source::Tile(Tile::Surprise)) else { return };
    let Some(slot) = slots.iter().find(|s| s.index == at) else { return };
    let shows = surprise.on(&shelf.id).and_then(|i| shelf.games.get(i)).unwrap_or(&shelf.games[at]);
    let (cover, _) = cover_of(shows, &shelf.id, &looks::look_for(&shelf.id, shows));
    covers.want(cover.clone(), (slot.front.clone(), slot.sheet), &loader, &mut materials);
    covers.want(cover, (slot.mirrored.clone(), slot.sheet), &loader, &mut materials);
}

/// Stands the chosen shelf's games up, after the old shelf's cases are cleared away.
pub fn build_shelf(
    mut commands: Commands,
    lib: Res<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    mut covers: ResMut<Covers>,
    loader: Res<ArtLoader>,
    kit: Res<Kit>,
    stage: Res<Stage>,
    plays: Res<crate::play::Plays>,
    mut surprise: ResMut<crate::play::Surprise>,
    old: Query<Entity, With<Slot>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let LibraryState::Ready(library) = &lib.0 else { return };
    if cursor.built == Some(cursor.shelf) || library.shelves.is_empty() || stage.bh <= 0.0 {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    covers.waiting.clear();
    let shelf = &library.shelves[cursor.shelf];
    cursor.built = Some(cursor.shelf);
    if std::mem::take(&mut cursor.land) {
        cursor.game = shelf.games.iter().position(|g| !matches!(g.source, Source::Tile(_))).unwrap_or(0);
    }
    cursor.game = cursor.game.min(shelf.games.len().saturating_sub(1));
    let favourites: Vec<&str> = library.config.raw().get("ui").and_then(|u| u.get("favorites")).and_then(|f| f.as_array()).map(|f| f.iter().filter_map(|v| v.as_str()).collect()).unwrap_or_default();
    cursor.flipped = false;
    surprise.pick = None;

    let plain = |m: &mut Assets<StandardMaterial>, c: Color| m.add(StandardMaterial { base_color: c, unlit: true, alpha_mode: if c.alpha() < 1.0 { AlphaMode::Blend } else { AlphaMode::Opaque }, ..default() });
    let mut cases: HashMap<String, [Handle<StandardMaterial>; 2]> = HashMap::new();
    for (i, g) in shelf.games.iter().enumerate() {
        // on Home, All or a collection each game keeps its own shelf's case
        let own = shelf.from.get(i).and_then(|id| library.shelves.iter().find(|s| s.id == *id)).unwrap_or(shelf);
        let look = looks::look_for(&own.id, g);
        let shape = look.on_shelf(&own.id);
        let (w, d) = (look.ratio, shape.depth);
        // a PC game's case has no border round its cover
        let (edge, edge_in) = if look.edge == 0.0 { (0.0, 0.0) } else { (shape.edge, shape.edge_in) };
        let (sheet_w, sheet_h, sheet_x) = (w - edge - edge_in, 1.0 - 2.0 * edge, (edge_in - edge) / 2.0);
        let [case, paper] = cases.entry(look.id.clone()).or_insert_with(|| [plain(&mut materials, color(look.case_color)), plain(&mut materials, color(look.paper()))]).clone();
        let front = plain(&mut materials, Color::WHITE);
        let back = plain(&mut materials, Color::WHITE);
        let spine = plain(&mut materials, Color::WHITE);
        let glare = materials.add(StandardMaterial { base_color: Color::srgba(1.0, 1.0, 1.0, 0.55), base_color_texture: Some(kit.glare.clone()), unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
        let mirrored = materials.add(StandardMaterial { unlit: true, alpha_mode: AlphaMode::Blend, ..default() });

        let tile = match g.source {
            Source::Tile(t) => Some(t),
            _ => None,
        };
        let (cover, hue) = cover_of(g, &own.id, &look);
        // a game that isn't installed stands in greys (style.css .slot.notinst)
        let grey = |art: Art, bright: f32, contrast: f32| if g.not_installed { Art::Grey(Box::new(art), bright, contrast) } else { art };
        let cover = grey(cover, 0.55, 0.9);
        let front_key = cover.key();
        covers.want(cover.clone(), (front.clone(), sheet_w / sheet_h), &loader, &mut materials);
        covers.want(cover, (mirrored.clone(), sheet_w / sheet_h), &loader, &mut materials);
        // painted at twice the pixels it stands in: the chosen case comes nearer
        let label = match tile {
            Some(Tile::Emulator) => "EMU".into(),
            Some(Tile::AddGames | Tile::AddCustom) => "ADD".into(),
            _ => look.spine_band.as_ref().map_or(String::new(), |b| b.label.clone()),
        };
        let drawn = Art::Spine { look: Box::new(look.clone()), title: g.title.clone(), label, width: (stage.bh * d * 2.0).round().max(2.0) as u32, height: (stage.bh * 2.0).round().max(2.0) as u32, edge: edge * stage.bh };
        covers.want(grey(drawn, 0.6, 1.0), (spine.clone(), 0.0), &loader, &mut materials);
        let mut late = vec![(
            match g.art.back.clone() {
                Some(path) => Art::File(path),
                None => {
                    let facts = crate::settings::facts_of(g, &crate::settings::emulator_of(own), rustyshelf_core::shelfview::played(g, &plays.0));
                    Art::Back { look: Box::new(look.clone()), title: g.title.clone(), facts: facts.into_iter().take(5).collect(), blurb: g.info.as_ref().and_then(|i| i.overview.clone()).unwrap_or_default() }
                }
            },
            (back.clone(), sheet_w / sheet_h),
        )];
        // a picture of the spine lies over the painted one, stretched to the sheet
        let spine_art = g.art.spine.clone().map(|path| {
            let sheet = materials.add(StandardMaterial { base_color: Color::NONE, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
            late.push((grey(Art::File(path), 0.6, 1.0), (sheet.clone(), 0.0)));
            sheet
        });

        let at = |x: f32, z: f32, turn: f32, sx: f32, sy: f32| Transform::from_xyz(x, 0.0, z).with_rotation(Quat::from_rotation_y(turn)).with_scale(Vec3::new(sx, sy, 1.0));
        let mesh = || Mesh3d(kit.face.clone());
        let slot = Slot {
            index: i,
            // (a case of the shelf's own has its own colour: app.js prepare())
            hue: match tile {
                Some(Tile::Surprise) => 300.0,
                Some(Tile::AddGames | Tile::AddCustom) => 210.0,
                _ => hue,
            },
            front_key,
            from: Place::default(),
            to: Place::default(),
            since: f32::MIN,
            turn: Eased::at(0.0),
            dim: Eased::at(0.34),
            front: front.clone(),
            mirrored: mirrored.clone(),
            sheet: sheet_w / sheet_h,
            glare: glare.clone(),
            late,
        };
        commands.spawn((slot, Transform::default(), Visibility::Hidden)).with_children(|s| {
            s.spawn((Turning, Transform::default(), Visibility::default())).with_children(|b| {
                let either = || Either([kit.plastic[0].clone(), kit.plastic[1].clone()]);
                // the front: the case's border, the printed sheet in it, the plastic and the band of light over them
                b.spawn((mesh(), MeshMaterial3d(case.clone()), at(0.0, d / 2.0, 0.0, w, 1.0)));
                b.spawn((mesh(), MeshMaterial3d(front), at(sheet_x, d / 2.0 + SKIN, 0.0, sheet_w, sheet_h)));
                b.spawn((either(), mesh(), MeshMaterial3d(kit.plastic[1].clone()), at(0.0, d / 2.0 + 2.0 * SKIN, 0.0, w, 1.0)));
                b.spawn((mesh(), MeshMaterial3d(glare), at(0.0, d / 2.0 + 3.0 * SKIN, 0.0, w, 1.0)));
                // the back, the spine's side on its right
                b.spawn((mesh(), MeshMaterial3d(case.clone()), at(0.0, -d / 2.0, PI, w, 1.0)));
                b.spawn((mesh(), MeshMaterial3d(back), at(-sheet_x, -d / 2.0 - SKIN, PI, sheet_w, sheet_h)));
                b.spawn((either(), mesh(), MeshMaterial3d(kit.plastic[1].clone()), at(0.0, -d / 2.0 - 2.0 * SKIN, PI, w, 1.0)));
                // a spine on either side: the left stack shows the one, the right stack the other
                for (x, turn, shade) in [(-w / 2.0, -PI / 2.0, 1), (w / 2.0, PI / 2.0, 2)] {
                    let side = |out: f32, m: &Handle<StandardMaterial>, tall: f32| (mesh(), MeshMaterial3d(m.clone()), Transform::from_xyz(x + x.signum() * out, 0.0, 0.0).with_rotation(Quat::from_rotation_y(turn)).with_scale(Vec3::new(d, tall, 1.0)));
                    b.spawn(side(0.0, &paper, 1.0));
                    b.spawn(side(SKIN, &spine, 1.0));
                    if let Some(art) = &spine_art {
                        b.spawn(side(2.0 * SKIN, art, sheet_h));
                    }
                    let shaded = b.spawn(side(3.0 * SKIN, &kit.shade[shade], 1.0)).id();
                    if shade == 1 {
                        b.commands().entity(shaded).insert(Either([kit.shade[0].clone(), kit.shade[1].clone()]));
                    }
                }
                // the chosen case: light inside its covers' edges, and its cover mirrored in the floor
                for (z, turn) in [(d / 2.0 + 4.0 * SKIN, 0.0), (-d / 2.0 - 4.0 * SKIN, PI)] {
                    b.spawn((OnChosen, mesh(), MeshMaterial3d(kit.rim.clone()), at(0.0, z, turn, w, 1.0), Visibility::Hidden));
                }
                let floor = -0.5 - 3.0 / stage.bh;
                b.spawn((OnChosen, Mesh3d(kit.mirror.clone()), MeshMaterial3d(mirrored), Transform::from_xyz(sheet_x, floor - edge, d / 2.0).with_scale(Vec3::new(sheet_w, sheet_h, 1.0)), Visibility::Hidden));
                // a favourite's heart, in the corner of its cover: 26px across, 8px in
                if favourites.contains(&g.id.as_str()) {
                    let (size, inset) = (26.0 / stage.bh, 21.0 / stage.bh);
                    b.spawn((mesh(), MeshMaterial3d(kit.heart.clone()), Transform::from_xyz(w / 2.0 - inset, 0.5 - inset, d / 2.0 + 5.0 * SKIN).with_scale(Vec3::new(size, size, 1.0))));
                }
                // every case: a pale strip of floor under it
                b.spawn((mesh(), MeshMaterial3d(kit.strip.clone()), Transform::from_xyz(0.0, floor - 0.19, d / 2.0).with_scale(Vec3::new(w, 0.38, 1.0))));
            });
        });
    }
}

/// Pictures decoded since the last frame go onto the sheets that wait for them; the chosen case's back and spine
/// are asked for when it's chosen.
pub fn apply_covers(
    loader: Res<ArtLoader>,
    cursor: Res<Cursor>,
    mut covers: ResMut<Covers>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut slots: Query<&mut Slot>,
) {
    for d in loader.finished() {
        if let Some((w, h, data)) = &d.image {
            if let Some(hue) = hue_of(*w, *h, data) {
                covers.hues.insert(d.path.clone(), hue);
            }
            covers.swatches.insert(d.path.clone(), Arc::new(themes::swatch(*w, *h, data)));
        }
        let done = d.image.map(|(w, h, data)| (images.add(to_image(w, h, data)), w as f32 / h as f32));
        for sheet in covers.waiting.remove(&d.path).unwrap_or_default() {
            if let Some((img, aspect)) = &done {
                show(&mut materials, &sheet, img, *aspect);
                if let Some(mut m) = materials.get_mut(&sheet.0) {
                    if m.base_color == Color::NONE {
                        m.base_color = Color::WHITE;
                    }
                }
            }
        }
        covers.loaded.insert(d.path, done);
    }
    for mut s in slots.iter_mut().filter(|s| s.index == cursor.game && !s.late.is_empty()) {
        for (art, sheet) in std::mem::take(&mut s.late) {
            covers.want(art, sheet, &loader, &mut materials);
        }
    }
}

/// Where a case stands when `chosen` is the chosen one (app.js place()).
fn place(index: usize, chosen: usize, stage: &Stage) -> Place {
    let d = index as f32 - chosen as f32;
    if d == 0.0 {
        return Place { x: 0.0, z: stage.bw * 1.05, ry: 22.0 };
    }
    let (away, side) = (d.abs(), d.signum());
    Place { x: side * (stage.bw * 1.22 + (away - 1.0) * (stage.bw * 0.22 + stage.bd * 0.9)), z: -stage.bw * 0.7 - away * 6.0, ry: -side * 60.0 }
}

/// Eases every case toward its place: the chosen one forward and turned a little (round to its back when that's
/// asked for), the rest in the two stacks, under a darker film.
pub fn arrange(
    time: Res<Time>,
    cursor: Res<Cursor>,
    stage: Res<Stage>,
    kit: Res<Kit>,
    covers: Res<Covers>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut slots: Query<(&mut Slot, &mut Transform, &mut Visibility, &Children), (Without<Turning>, Without<OnChosen>)>,
    mut turning: Query<(&mut Transform, &Children), With<Turning>>,
    mut either: Query<(&Either, &mut MeshMaterial3d<StandardMaterial>)>,
    mut worn: Query<&mut Visibility, (With<OnChosen>, Without<Slot>)>,
) {
    let now = time.elapsed_secs();
    for (mut s, mut t, mut seen, children) in &mut slots {
        let chosen = s.index == cursor.game;
        let to = place(s.index, cursor.game, &stage);
        let here = |s: &Slot| {
            let k = bezier(SLIDE_CURVE, (now - s.since) / SLIDE);
            Place { x: s.from.x + (s.to.x - s.from.x) * k, z: s.from.z + (s.to.z - s.from.z) * k, ry: s.from.ry + (s.to.ry - s.from.ry) * k }
        };
        if s.since == f32::MIN {
            // a new shelf's cases appear in place
            (s.from, s.to, s.since) = (to, to, now - SLIDE);
        } else if to != s.to {
            (s.from, s.to, s.since) = (here(&s), to, now);
        }
        let at = here(&s);
        t.translation = Vec3::new(at.x, 0.0, at.z);
        t.rotation = Quat::from_rotation_y(at.ry.to_radians());
        t.scale = Vec3::splat(stage.bh);
        seen.set_if_neq(if s.index.abs_diff(cursor.game) <= IN_VIEW { Visibility::Visible } else { Visibility::Hidden });

        let film = s.dim.now(now, 0.45, EASE);
        s.dim.head_for(if chosen { 0.0 } else { 0.34 }, film, now);
        if let Some(mut m) = materials.get_mut(&s.front) {
            let want = Color::srgb(1.0 - film, 1.0 - film, 1.0 - film);
            if m.base_color != want {
                m.base_color = want;
            }
        }
        if let Some(hue) = covers.hues.get(&s.front_key) {
            s.hue = *hue;
        }
        // the band of light slides with the case's angle and place
        let slide = (-at.ry / 110.0 - at.x / (stage.bw * 7.0)) * 0.8325;
        if let Some(mut m) = materials.get_mut(&s.glare) {
            m.uv_transform = bevy::math::Affine2::from_scale_angle_translation(Vec2::new(1.0 / 1.2, 1.0), 0.0, Vec2::new(0.1 / 1.2 - slide, 0.0));
            m.base_color = Color::srgba(1.0, 1.0, 1.0, if chosen { 1.0 } else { 0.55 });
        }

        let angle = s.turn.now(now, TURN, TURN_CURVE);
        s.turn.head_for(if chosen && cursor.flipped { 180.0 } else { 0.0 }, angle, now);
        for child in children.iter() {
            let Ok((mut turn, parts)) = turning.get_mut(child) else { continue };
            turn.rotation = Quat::from_rotation_y(angle.to_radians());
            for part in parts.iter() {
                if let Ok((looks, mut m)) = either.get_mut(part) {
                    let want = &looks.0[usize::from(!chosen)];
                    if m.0 != *want {
                        m.0 = want.clone();
                    }
                }
                if let Ok(mut seen) = worn.get_mut(part) {
                    seen.set_if_neq(if chosen { Visibility::Inherited } else { Visibility::Hidden });
                }
            }
        }
        if chosen {
            if let Some(mut m) = materials.get_mut(&kit.rim) {
                m.base_color = lit(s.hue, 0.9);
            }
        }
    }
}

/// The backdrop behind the shelf, painted again when the theme, the colour under the chosen case or the window
/// changes, and the glow behind the chosen case.
pub fn backdrop(
    time: Res<Time>,
    lib: Res<LibraryRes>,
    cursor: Res<Cursor>,
    stage: Res<Stage>,
    settings: Res<crate::settings::Settings>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut back: ResMut<Backdrop>,
    mut images: ResMut<Assets<Image>>,
    covers: Res<Covers>,
    mut lit_in: ResMut<Lit>,
    slots: Query<&Slot>,
    mut behind: Query<(&Behind, &mut ImageNode, &mut Node)>,
) {
    let LibraryState::Ready(library) = &lib.0 else { return };
    let (Some(shelf), Ok(window)) = (library.shelves.get(cursor.shelf), window.single()) else { return };
    if stage.bh <= 0.0 {
        return;
    }
    let now = time.elapsed_secs();
    let theme = settings.theme();
    let chosen = slots.iter().find(|s| s.index == cursor.game);
    let own = chosen.map_or(0.0, |s| s.hue);
    let cover = chosen.map_or_else(Default::default, |s| (s.front_key.clone(), covers.swatches.get(&s.front_key).cloned()));
    // the shelf's colour walks a little from its first case to its last (app.js shelfHue)
    let along = cursor.game as f64 / (shelf.games.len().max(2) - 1) as f64;
    let sh = f64::from(looks::look(&shelf.id).hue.unwrap_or(own)) + (along * 2.0 - 1.0) * 16.0;
    (lit_in.glow, lit_in.shelf) = (themes::halo(&theme, sh, None) as f32, sh as f32);
    let lit_hue = lit_in.glow;
    // a backdrop of soft colours is painted small and stretched
    let part = if themes::fine(&theme) { window.scale_factor().min(1.5) } else { 0.25 };
    let wanted = Wanted { theme: theme.clone(), sh: (sh * 2.0).round() / 2.0, hue: f64::from(own), cover, stage: *stage, pixels: ((stage.w * part).round().max(2.0) as u32, (stage.h * part).round().max(2.0) as u32) };
    if back.asked.as_ref() != Some(&wanted) {
        back.asked = Some(wanted.clone());
        let _ = back.ask.send(wanted);
    }
    let newest = back.done.lock().ok().and_then(|done| done.try_iter().last());
    if let Some(p) = newest {
        // the picture that was fading in becomes the one underneath
        let fresh = picture(&mut images, p);
        let was = behind.iter().find(|(which, ..)| matches!(which, Behind::New)).map(|(_, image, _)| image.image.clone());
        for (which, mut image, _) in &mut behind {
            match (which, &was) {
                (Behind::Old, Some(was)) if back.shown => (image.image, image.color) = (was.clone(), Color::WHITE),
                (Behind::New, _) => image.image = fresh.clone(),
                _ => {}
            }
        }
        (back.shown, back.since) = (true, now);
    }
    let fade = if back.shown { bezier(EASE, (now - back.since) / 0.8) } else { 0.0 };
    let (w, h) = (stage.bw * 2.6, stage.bh * 1.9);
    let repaint = back.halo != (w, h);
    back.halo = (w, h);
    for (which, mut image, mut node) in &mut behind {
        match which {
            Behind::Old | Behind::Moving => {}
            Behind::New => image.color = Color::srgba(1.0, 1.0, 1.0, fade),
            Behind::Halo => {
                if repaint {
                    let mut p = rustyshelf_core::cssbg::paint("radial-gradient(closest-side,#fff,transparent)", (w / 2.0).round().max(2.0) as u32, (h / 2.0).round().max(2.0) as u32, 0.5);
                    for px in p.rgba.as_chunks_mut::<4>().0 {
                        // light over a dark backdrop fades faster in linear light than in the browser
                        let a = f32::from(px[3]) / 255.0 * 0.62;
                        px[3] = (a.powf(1.5) * 255.0) as u8;
                    }
                    image.image = picture(&mut images, p);
                }
                (node.left, node.top, node.width, node.height) = (px(stage.w / 2.0 - w / 2.0), px(stage.h * EYE_Y - h / 2.0), px(w), px(h));
                // it breathes: down to .55 and back every 3.4 s
                let beat = (now / 3.4).fract();
                let breath = 1.0 - 0.45 * bezier(EASE_IN_OUT, 1.0 - (beat * 2.0 - 1.0).abs());
                image.color = lit(lit_hue, 0.95).with_alpha(breath * themes::halo_strength(&theme));
            }
        }
    }
}

/// A frame of the theme's moving layer to draw.
struct Tick {
    theme: String,
    dt: f32,
    hue: f32,
    /// the layer's size in pixels, how many of them go to a pixel of the window, and how far down it the floor is
    size: (u32, u32),
    scale: f32,
    floor: f32,
}

/// What moves behind the shelf in the theme, drawn off the frame thirty times a second.
#[derive(Resource)]
pub struct Moving {
    ask: Sender<Tick>,
    done: Mutex<Receiver<Picture>>,
    /// a frame is being drawn
    busy: bool,
    last: f32,
    image: Option<Handle<Image>>,
}

/// As many pixels as the layer is drawn with at most: a larger window's is drawn smaller and stretched.
const MOVING_PIXELS: f32 = 1280.0 * 720.0;

impl Default for Moving {
    fn default() -> Self {
        let (ask, ticks) = channel::<Tick>();
        let (finished, done) = channel();
        std::thread::spawn(move || {
            // light over a dark backdrop comes out stronger in linear light than in the browser: its alpha, taken down
            let dimmer: Vec<u8> = (0..256).map(|a| ((a as f32 / 255.0).powf(1.6) * 255.0).round() as u8).collect();
            let mut made: Option<(themes_fx::Layer, (u32, u32), String, Vec<u8>)> = None;
            while let Ok(tick) = ticks.recv() {
                if made.as_ref().is_none_or(|m| m.1 != tick.size || m.2 != tick.theme) {
                    let layer = match made.take() {
                        Some((layer, size, ..)) if size == tick.size => layer,
                        _ => themes_fx::Layer::new(tick.size.0, tick.size.1, tick.scale, tick.floor, tick.hue),
                    };
                    made = Some((layer, tick.size, tick.theme.clone(), themes::corners(&tick.theme, tick.size.0, tick.size.1)));
                }
                let Some((layer, _, _, corners)) = made.as_mut() else { continue };
                let mut p = layer.frame(&tick.theme, tick.dt, tick.hue, 200.0);
                // the theme's dark corners lie over what moves
                for (px, light) in p.rgba.as_chunks_mut::<4>().0.iter_mut().zip(corners.iter()) {
                    if px[3] > 0 {
                        for c in &mut px[..3] {
                            *c = ((u16::from(*c) * u16::from(*light) + 127) / 255) as u8;
                        }
                        px[3] = dimmer[usize::from(px[3])];
                    }
                }
                if finished.send(p).is_err() {
                    return;
                }
            }
        });
        Moving { ask, done: Mutex::new(done), busy: false, last: 0.0, image: None }
    }
}

/// Draws the theme's moving layer on, while the shelf is the thing on screen.
pub fn moving(
    time: Res<Time>,
    stage: Res<Stage>,
    lit: Res<Lit>,
    settings: Res<crate::settings::Settings>,
    playing: Res<crate::play::Playing>,
    mut moving: ResMut<Moving>,
    mut images: ResMut<Assets<Image>>,
    mut behind: Query<(&Behind, &mut ImageNode)>,
) {
    let Some((_, mut node)) = behind.iter_mut().find(|(which, _)| matches!(which, Behind::Moving)) else { return };
    let theme = settings.theme();
    if !themes_fx::moves(&theme) || stage.bh <= 0.0 {
        node.color = Color::NONE;
        return;
    }
    let now = time.elapsed_secs();
    let frame = moving.done.lock().ok().and_then(|done| done.try_iter().last());
    if let Some(p) = frame {
        moving.busy = false;
        let mut image = Image::new(
            bevy::render::render_resource::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
            bevy::render::render_resource::TextureDimension::D2,
            p.rgba,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        // Arcade is made of pixels
        if theme == "arcade" {
            image.sampler = bevy::image::ImageSampler::nearest();
        }
        let handle = images.add(image);
        node.image = handle.clone();
        node.color = Color::WHITE;
        moving.image = Some(handle);
    }
    if moving.busy || playing.title.is_some() || now - moving.last < 1.0 / 30.0 {
        return;
    }
    let part = (MOVING_PIXELS / (stage.w * stage.h)).sqrt().min(1.0);
    let tick = Tick {
        theme,
        dt: (now - moving.last).min(0.1),
        hue: lit.shelf,
        size: ((stage.w * part).round() as u32, (stage.h * part).round() as u32),
        scale: part,
        floor: (stage.h * EYE_Y + stage.bh / 2.0 - 10.0) / stage.h,
    };
    moving.last = now;
    moving.busy = moving.ask.send(tick).is_ok();
}

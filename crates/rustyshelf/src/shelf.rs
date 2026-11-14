//! The 3D shelf: a shelf's games standing on a plank in their consoles' cases, covers facing out; the chosen one
//! comes forward, and turns round to show its back.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rustyshelf_core::looks::{self, Rgba};

use crate::loading::{to_image, ArtLoader, LibraryRes, LibraryState};

/// A keep case's height; a console's own case may be shorter.
const BOX_H: f32 = 0.95;
const GAP: f32 = 0.1;
const PLANK_TOP: f32 = -BOX_H / 2.0 - 0.005;
/// how far a printed sheet sits off the case under it
const SKIN: f32 = 0.0012;

/// Which game is chosen, on which shelf.
#[derive(Resource, Default)]
pub struct Cursor {
    pub shelf: usize,
    pub game: usize,
    /// the shelf whose boxes are standing now
    pub built: Option<usize>,
    /// the chosen case shows its back
    pub flipped: bool,
}

#[derive(Component)]
pub struct GameBox {
    pub index: usize,
    w: f32,
    h: f32,
    x: f32,
    /// its back and its spine, shown once it's chosen: each one's picture, and the sheet it goes on
    back: Option<(PathBuf, Handle<StandardMaterial>, f32)>,
    spine: Option<(PathBuf, Handle<StandardMaterial>, f32)>,
    asked: bool,
}

#[derive(Component)]
pub struct ShelfPart;

/// The light that follows the chosen case, in its shelf's colour.
#[derive(Component)]
pub struct Glow;

/// A sheet waiting for its picture: its material, and its width for its height (the picture is cut to it).
type Sheet = (Handle<StandardMaterial>, f32);

/// Pictures in flight or loaded: which sheets wait for which file.
#[derive(Resource, Default)]
pub struct Covers {
    waiting: HashMap<PathBuf, Vec<Sheet>>,
    loaded: HashMap<PathBuf, Option<(Handle<Image>, f32)>>,
}

impl Covers {
    /// Puts a picture on a sheet: at once when it's loaded, else when it arrives.
    fn want(&mut self, path: PathBuf, sheet: Sheet, loader: &ArtLoader, materials: &mut Assets<StandardMaterial>) {
        match self.loaded.get(&path) {
            Some(Some((img, aspect))) => show(materials, &sheet, img, *aspect),
            Some(None) => {}
            None => {
                if !self.waiting.contains_key(&path) {
                    loader.request(path.clone());
                }
                self.waiting.entry(path).or_default().push(sheet);
            }
        }
    }
}

/// A picture on its sheet, cut to the sheet's shape from its middle (never stretched).
fn show(materials: &mut Assets<StandardMaterial>, sheet: &Sheet, img: &Handle<Image>, aspect: f32) {
    let Some(mut m) = materials.get_mut(&sheet.0) else { return };
    let (scale, offset) = if aspect > sheet.1 {
        let part = sheet.1 / aspect;
        (Vec2::new(part, 1.0), Vec2::new((1.0 - part) / 2.0, 0.0))
    } else {
        let part = aspect / sheet.1;
        (Vec2::new(1.0, part), Vec2::new(0.0, (1.0 - part) / 2.0))
    };
    m.base_color = Color::WHITE;
    m.base_color_texture = Some(img.clone());
    m.uv_transform = bevy::math::Affine2::from_scale_angle_translation(scale, 0.0, offset);
}

fn color(c: Rgba) -> Color {
    Color::srgba(c[0], c[1], c[2], c[3])
}

/// A case's own colour; a clear case shows the white sheet behind it.
fn case_color(c: Rgba) -> Color {
    if c[3] < 1.0 {
        Color::srgb(0.9, 0.9, 0.92)
    } else {
        color(c)
    }
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(ClearColor(Color::srgb(0.035, 0.04, 0.05)));
    commands.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 350.0, ..default() });
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 0.1, 3.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y)));
    commands.spawn((
        DirectionalLight { illuminance: 6_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(-2.0, 4.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((Glow, PointLight { intensity: 0.0, range: 6.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(0.0, 0.9, 1.4)));
}

fn placeholder_color(title: &str) -> Color {
    let h = title.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    Color::hsl((h % 360) as f32, 0.35, 0.32)
}

/// The wall behind the shelf: the shelf's tint, lighter at the top.
fn backdrop(width: f32, tint: [Rgba; 2]) -> Mesh {
    let (w, h) = (width / 2.0, 6.0);
    let shade = |c: Rgba| Color::srgb(c[0], c[1], c[2]).to_linear().to_f32_array();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-w, -h, 0.0], [w, -h, 0.0], [w, h, 0.0], [-w, h, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![shade(tint[1]), shade(tint[1]), shade(tint[0]), shade(tint[0])])
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Stands the chosen shelf's games up, after the old shelf's boxes are cleared away.
pub fn build_shelf(
    mut commands: Commands,
    lib: Res<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    mut covers: ResMut<Covers>,
    loader: Res<ArtLoader>,
    old: Query<Entity, Or<(With<GameBox>, With<ShelfPart>)>>,
    mut glow: Query<&mut PointLight, With<Glow>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let LibraryState::Ready(library) = &lib.0 else { return };
    if cursor.built == Some(cursor.shelf) || library.shelves.is_empty() {
        return;
    }
    for e in &old {
        commands.entity(e).despawn();
    }
    covers.waiting.clear();
    let shelf = &library.shelves[cursor.shelf];
    cursor.built = Some(cursor.shelf);
    cursor.game = cursor.game.min(shelf.games.len().saturating_sub(1));
    cursor.flipped = false;
    let own = looks::look(&shelf.id);

    let body = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let face = meshes.add(Rectangle::new(1.0, 1.0));
    let width = shelf.games.len() as f32 * (BOX_H * own.ratio + GAP) + 4.0;
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(Cuboid::new(width, 0.05, 0.7))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.24, 0.13), perceptual_roughness: 0.8, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, PLANK_TOP - 0.025, 0.1),
    ));
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(Cuboid::new(width, 3.0, 0.6))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.13, 0.07), perceptual_roughness: 0.9, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, PLANK_TOP - 1.55, 0.05),
    ));
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(backdrop(width + 20.0, own.tint))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, 2.0, -0.5),
    ));
    if let Ok(mut light) = glow.single_mut() {
        light.color = color(own.glow);
        light.intensity = 40_000.0;
    }

    let mut cases: HashMap<String, Handle<StandardMaterial>> = HashMap::new();
    for (i, g) in shelf.games.iter().enumerate() {
        // on Home, All or a collection each game keeps its own shelf's case
        let look = looks::look_for(shelf.from.get(i).map_or(shelf.id.as_str(), String::as_str), g);
        let h = BOX_H * look.h.unwrap_or(1.0);
        let (w, d, edge) = (h * look.shape().0, h * look.shape().1, h * look.shape().2);
        // the printed sheet inside the case's border; one that wraps runs to the spine's side
        let sheet_w = w - if look.wrap { edge } else { 2.0 * edge };
        let (sheet_h, sheet_x) = (h - 2.0 * edge, if look.wrap { -edge / 2.0 } else { 0.0 });
        let case = cases.entry(look.id.clone()).or_insert_with(|| materials.add(StandardMaterial { base_color: case_color(look.case_color), perceptual_roughness: 0.4, ..default() })).clone();
        let paper = if look.case_color[3] < 1.0 { Color::srgb(0.93, 0.93, 0.95) } else { color(look.case_side) };
        let sheet = |c: Color, m: &mut Assets<StandardMaterial>| m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.35, ..default() });
        let front = sheet(placeholder_color(&g.title), &mut materials);
        let back = sheet(paper, &mut materials);
        let spine = sheet(look.spine_band.as_ref().map_or(paper, |b| color(b.bg)), &mut materials);
        if let Some(path) = g.art.front.clone() {
            covers.want(path, (front.clone(), sheet_w / sheet_h), &loader, &mut materials);
        }
        commands
            .spawn((
                GameBox {
                    index: i,
                    w,
                    h,
                    x: 0.0,
                    back: g.art.back.clone().map(|p| (p, back.clone(), sheet_w / sheet_h)),
                    spine: g.art.spine.clone().map(|p| (p, spine.clone(), d / sheet_h)),
                    asked: false,
                },
                Transform::default(),
                Visibility::default(),
            ))
            .with_children(|b| {
                b.spawn((Mesh3d(body.clone()), MeshMaterial3d(case), Transform::from_scale(Vec3::new(w, h, d))));
                b.spawn((Mesh3d(face.clone()), MeshMaterial3d(front), Transform::from_xyz(sheet_x, 0.0, d / 2.0 + SKIN).with_scale(Vec3::new(sheet_w, sheet_h, 1.0))));
                b.spawn((
                    Mesh3d(face.clone()),
                    MeshMaterial3d(back),
                    Transform::from_xyz(-sheet_x, 0.0, -d / 2.0 - SKIN).with_rotation(Quat::from_rotation_y(PI)).with_scale(Vec3::new(sheet_w, sheet_h, 1.0)),
                ));
                // the spine is on the left, as a case stands on a shelf
                b.spawn((
                    Mesh3d(face.clone()),
                    MeshMaterial3d(spine),
                    Transform::from_xyz(-w / 2.0 - SKIN, 0.0, 0.0).with_rotation(Quat::from_rotation_y(-PI / 2.0)).with_scale(Vec3::new(d - 2.0 * SKIN, sheet_h, 1.0)),
                ));
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
    mut boxes: Query<&mut GameBox>,
) {
    for d in loader.finished() {
        let done = d.image.map(|(w, h, data)| (images.add(to_image(w, h, data)), w as f32 / h as f32));
        for sheet in covers.waiting.remove(&d.path).unwrap_or_default() {
            if let Some((img, aspect)) = &done {
                show(&mut materials, &sheet, img, *aspect);
            }
        }
        covers.loaded.insert(d.path, done);
    }
    for mut b in boxes.iter_mut().filter(|b| b.index == cursor.game && !b.asked) {
        b.asked = true;
        for (path, mat, shape) in b.back.iter().chain(b.spine.iter()) {
            covers.want(path.clone(), (mat.clone(), *shape), &loader, &mut materials);
        }
    }
}

/// Lines the cases up on the plank by their widths and eases each toward its place: the chosen one forward (turned
/// round when its back is asked for), the rest turned a little toward it.
pub fn arrange(
    time: Res<Time>,
    cursor: Res<Cursor>,
    mut boxes: Query<(&mut GameBox, &mut Transform), (Without<Camera3d>, Without<Glow>)>,
    mut camera: Query<&mut Transform, (With<Camera3d>, Without<Glow>)>,
    mut glow: Query<&mut Transform, (With<Glow>, Without<Camera3d>)>,
) {
    let mut list: Vec<_> = boxes.iter_mut().collect();
    list.sort_by_key(|(b, _)| b.index);
    let mut x = 0.0;
    for (b, _) in list.iter_mut() {
        b.x = x + b.w / 2.0;
        x += b.w + GAP;
    }
    let Some(sel_x) = list.iter().find(|(b, _)| b.index == cursor.game).map(|(b, _)| b.x) else { return };
    let k = 1.0 - (-time.delta_secs() * 10.0).exp();
    for (b, t) in list.iter_mut() {
        let chosen = b.index == cursor.game;
        let stand = PLANK_TOP + b.h / 2.0;
        let (z, y, yaw) = match (chosen, cursor.flipped) {
            (true, true) => (0.45, stand + 0.06, PI),
            (true, false) => (0.38, stand + 0.06, 0.0),
            _ => (0.0, stand, ((b.x - sel_x) * -0.35).clamp(-0.6, 0.6)),
        };
        t.translation = t.translation.lerp(Vec3::new(b.x, y, z), k);
        t.rotation = t.rotation.slerp(Quat::from_rotation_y(yaw), k);
    }
    if let Ok(mut cam) = camera.single_mut() {
        let want = Vec3::new(sel_x, 0.12, 2.9);
        cam.translation = cam.translation.lerp(want, k * 0.8);
        let look = cam.translation + Vec3::new(0.0, -0.06, -1.0);
        cam.look_at(look, Vec3::Y);
        if let Ok(mut light) = glow.single_mut() {
            light.translation = Vec3::new(cam.translation.x, 0.9, 1.4);
        }
    }
}

//! The 3D shelf: one console's games standing on a plank, covers facing out; the chosen one comes forward.

use std::collections::HashMap;
use std::path::PathBuf;

use bevy::prelude::*;

use crate::loading::{to_image, ArtLoader, LibraryRes, LibraryState};

const BOX_H: f32 = 0.95;
/// a DVD keep case's width for its height, until the cover says otherwise
const DEFAULT_ASPECT: f32 = 0.716;
const BOX_DEPTH: f32 = 0.07;
const GAP: f32 = 0.1;
const PLANK_Y: f32 = -BOX_H / 2.0 - 0.03;

/// Which game is chosen, on which shelf.
#[derive(Resource, Default)]
pub struct Cursor {
    pub shelf: usize,
    pub game: usize,
    /// the shelf whose boxes are standing now
    pub built: Option<usize>,
}

#[derive(Component)]
pub struct GameBox {
    pub index: usize,
    /// width over height, from the cover once it's loaded
    pub aspect: f32,
    pub x: f32,
}

#[derive(Component)]
pub struct ShelfPart;

/// Covers in flight or loaded: which boxes wait for which file.
#[derive(Resource, Default)]
pub struct Covers {
    waiting: HashMap<PathBuf, Vec<(Entity, Handle<StandardMaterial>)>>,
    loaded: HashMap<PathBuf, Option<(Handle<Image>, f32)>>,
}

pub fn setup(mut commands: Commands) {
    commands.insert_resource(ClearColor(Color::srgb(0.035, 0.04, 0.05)));
    commands.insert_resource(GlobalAmbientLight { color: Color::WHITE, brightness: 350.0, ..default() });
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 0.1, 3.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y)));
    commands.spawn((
        DirectionalLight { illuminance: 6_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(-2.0, 4.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn placeholder_color(title: &str) -> Color {
    let h = title.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    Color::hsl((h % 360) as f32, 0.35, 0.32)
}

/// Stands the chosen shelf's games up, after the old shelf's boxes are cleared away.
pub fn build_shelf(
    mut commands: Commands,
    lib: Res<LibraryRes>,
    mut cursor: ResMut<Cursor>,
    mut covers: ResMut<Covers>,
    loader: Res<ArtLoader>,
    old: Query<Entity, Or<(With<GameBox>, With<ShelfPart>)>>,
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

    let body = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let face = meshes.add(Rectangle::new(1.0, 1.0));
    let body_mat = materials.add(StandardMaterial { base_color: Color::srgb(0.06, 0.06, 0.07), perceptual_roughness: 0.4, ..default() });
    let width = shelf.games.len() as f32 * (BOX_H * DEFAULT_ASPECT + GAP) + 4.0;
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(Cuboid::new(width, 0.05, 0.7))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.42, 0.24, 0.13), perceptual_roughness: 0.8, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, PLANK_Y, 0.1),
    ));
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(Cuboid::new(width, 3.0, 0.6))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.24, 0.13, 0.07), perceptual_roughness: 0.9, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, PLANK_Y - 1.525, 0.05),
    ));
    commands.spawn((
        ShelfPart,
        Mesh3d(meshes.add(Rectangle::new(width + 20.0, 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.07, 0.075, 0.09), perceptual_roughness: 1.0, ..default() })),
        Transform::from_xyz(width / 2.0 - 2.0, 2.0, -0.5),
    ));

    for (i, g) in shelf.games.iter().enumerate() {
        let front_mat = materials.add(StandardMaterial { base_color: placeholder_color(&g.title), perceptual_roughness: 0.35, ..default() });
        let mut aspect = DEFAULT_ASPECT;
        if let Some(path) = g.art.front.clone() {
            match covers.loaded.get(&path) {
                Some(Some((img, a))) => {
                    aspect = *a;
                    let mut m = materials.get_mut(&front_mat).unwrap();
                    m.base_color = Color::WHITE;
                    m.base_color_texture = Some(img.clone());
                }
                Some(None) => {}
                None => {
                    if !covers.waiting.contains_key(&path) {
                        loader.request(path.clone());
                    }
                    covers.waiting.entry(path).or_default();
                }
            }
        }
        let id = commands
            .spawn((GameBox { index: i, aspect, x: 0.0 }, Transform::default(), Visibility::default()))
            .with_children(|b| {
                b.spawn((Mesh3d(body.clone()), MeshMaterial3d(body_mat.clone()), Transform::from_scale(Vec3::new(1.0, 1.0, BOX_DEPTH))));
                b.spawn((Mesh3d(face.clone()), MeshMaterial3d(front_mat.clone()), Transform::from_xyz(0.0, 0.0, BOX_DEPTH / 2.0 + 0.001)));
            })
            .id();
        if let Some(path) = &g.art.front {
            if let Some(w) = covers.waiting.get_mut(path) {
                w.push((id, front_mat));
            }
        }
    }
}

/// Covers decoded since the last frame go onto their boxes, which take the cover's shape.
pub fn apply_covers(
    loader: Res<ArtLoader>,
    mut covers: ResMut<Covers>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut boxes: Query<&mut GameBox>,
) {
    for d in loader.finished() {
        let done = d.image.map(|(w, h, data)| {
            let aspect = (w as f32 / h as f32).clamp(0.5, 1.6);
            (images.add(to_image(w, h, data)), aspect)
        });
        for (e, mat) in covers.waiting.remove(&d.path).unwrap_or_default() {
            let Some((img, aspect)) = &done else { continue };
            if let Some(mut m) = materials.get_mut(&mat) {
                m.base_color = Color::WHITE;
                m.base_color_texture = Some(img.clone());
            }
            if let Ok(mut b) = boxes.get_mut(e) {
                b.aspect = *aspect;
            }
        }
        covers.loaded.insert(d.path, done);
    }
}

/// Lines the boxes up by their widths and eases each toward its place: the chosen box forward, the rest turned
/// a little toward it.
pub fn arrange(
    time: Res<Time>,
    cursor: Res<Cursor>,
    mut boxes: Query<(&mut GameBox, &mut Transform), Without<Camera3d>>,
    mut camera: Query<&mut Transform, With<Camera3d>>,
) {
    let mut list: Vec<_> = boxes.iter_mut().collect();
    list.sort_by_key(|(b, _)| b.index);
    let mut x = 0.0;
    for (b, _) in list.iter_mut() {
        let w = BOX_H * b.aspect;
        b.x = x + w / 2.0;
        x += w + GAP;
    }
    let Some(sel_x) = list.iter().find(|(b, _)| b.index == cursor.game).map(|(b, _)| b.x) else { return };
    let k = 1.0 - (-time.delta_secs() * 10.0).exp();
    for (b, t) in list.iter_mut() {
        let w = BOX_H * b.aspect;
        let chosen = b.index == cursor.game;
        let (z, y, yaw) = if chosen { (0.38, 0.06, 0.0) } else { (0.0, 0.0, ((b.x - sel_x) * -0.35).clamp(-0.6, 0.6)) };
        let want = Transform::from_xyz(b.x, y, z).with_rotation(Quat::from_rotation_y(yaw)).with_scale(Vec3::new(w, BOX_H, 1.0));
        t.translation = t.translation.lerp(want.translation, k);
        t.rotation = t.rotation.slerp(want.rotation, k);
        t.scale = t.scale.lerp(want.scale, k);
    }
    if let Ok(mut cam) = camera.single_mut() {
        let want = Vec3::new(sel_x, 0.12, 2.9);
        cam.translation = cam.translation.lerp(want, k * 0.8);
        let look = cam.translation + Vec3::new(0.0, -0.06, -1.0);
        cam.look_at(look, Vec3::Y);
    }
}

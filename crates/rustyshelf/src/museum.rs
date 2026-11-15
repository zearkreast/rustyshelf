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

#[derive(Component)]
pub struct RowLight;

/// A case on the wall: its row, its place there, and where it stands.
#[derive(Component)]
pub struct Case {
    row: usize,
    col: usize,
    home: Vec3,
    w: f32,
    d: f32,
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
    painted: bool,
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
    /// each shelf's games that have a case, for this visit
    kept: Vec<(usize, Vec<usize>)>,
}

impl Museum {
    /// Where the camera stands for the row and case in view, in the room.
    fn view(&self) -> (Vec3, Vec3) {
        let Some(r) = self.rows.get(self.row) else { return (Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO) };
        let x = r.xs.get(self.col).copied().unwrap_or(0.0);
        let (at, turn) = self.walls[r.wall];
        let to_room = |p: Vec3| at + Quat::from_rotation_y(turn) * p;
        (to_room(Vec3::new(x - 0.2, r.y + 0.02, 3.7)), to_room(Vec3::new(x - 0.2, r.y - 0.03, 0.0)))
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
) {
    m.rows.clear();
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
    let wood = materials.add(StandardMaterial { base_color: Color::srgb(0.23, 0.15, 0.1), perceptual_roughness: 0.55, ..default() });
    let wall = materials.add(StandardMaterial { base_color: Color::srgb(0.09, 0.1, 0.14), perceptual_roughness: 0.95, ..default() });
    let floor = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.11, 0.1), perceptual_roughness: 0.32, metallic: 0.25, ..default() });
    // each wall's width: its longest row
    let width_of = |si: usize, games: &[usize]| -> f32 {
        let s = &lib.shelves[si];
        games.iter().map(|&gi| { let look = looks::look_for(&s.id, &s.games[gi]); (look.h.unwrap_or(1.0) * look.ratio * look.depth).max(0.045) + GAP }).sum::<f32>() + 0.6
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
    for (wall, si, games) in groups {
        let s = &lib.shelves[si];
        let ri = m.rows.len();
        let li = on_wall[wall];
        on_wall[wall] += 1;
        let y = -(li as f32) * ROW;
        let parent = parents[wall];
        let mut x = 0.0;
        let (mut sheets, mut xs) = (Vec::new(), Vec::new());
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
                .spawn((Case { row: ri, col, home, w, d }, layer(), Mesh3d(meshes.add(Cuboid::new(d, ch, w))), MeshMaterial3d(case), Transform::from_translation(home), ChildOf(parent)))
                .with_children(|c| {
                    c.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(d * 0.96, ch * 0.98))), MeshMaterial3d(spine.clone()), Transform::from_xyz(0.0, 0.0, w / 2.0 + 0.001)));
                    c.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(w * 0.96, ch * 0.98))), MeshMaterial3d(front.clone()), Transform::from_xyz(d / 2.0 + 0.001, 0.0, 0.0).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))));
                });
            sheets.push((spine, front));
            xs.push(x);
            x += d / 2.0 + GAP;
        }
        widest = widest.max(x + 0.6);
        // the plank under the row, and the lamp over it
        commands.spawn((layer(), Mesh3d(meshes.add(Cuboid::new(x + 0.66, PLANK, DEPTH + 0.1))), MeshMaterial3d(wood.clone()), Transform::from_xyz(x / 2.0, y - 0.5 - PLANK / 2.0, 0.0), ChildOf(parent)));
        commands.spawn((
            ChildOf(parent),
            RowLight,
            layer(),
            SpotLight { color: Color::srgb(1.0, 0.88, 0.69), intensity: 0.0, range: 7.5, outer_angle: 0.95, inner_angle: 0.5, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(x / 2.0, y + ROW - 0.62, 0.9).looking_at(Vec3::new(x / 2.0, y - 0.5, 0.0), Vec3::Y),
        ));
        m.rows.push(Row { wall, shelf: si, name: s.name.clone(), games, sheets, xs, y, painted: false });
    }
    // each wall's panel behind its rows (a room's are its own width; the classic wall runs on); one floor for the room
    let tallest = on_wall.iter().copied().max().unwrap_or(1).max(1) as f32;
    for (k, &n) in on_wall.iter().enumerate() {
        if n == 0 && k > 0 {
            continue;
        }
        let n = n.max(1) as f32;
        let w = if side { wall_w[k] + 2.0 } else { widest + 60.0 };
        commands.spawn((layer(), Mesh3d(meshes.add(Rectangle::new(w, n * ROW + 8.0))), MeshMaterial3d(wall.clone()), Transform::from_xyz(wall_w[k] / 2.0, -(n - 1.0) * ROW / 2.0, -DEPTH / 2.0 - 0.01), ChildOf(parents[k])));
    }
    // the dark all round the room: the shelf's own backdrop is drawn by a camera before this one, and must never show
    let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.02, 0.02, 0.03), unlit: true, cull_mode: None, ..default() });
    commands.spawn((MuseumThing, layer(), Mesh3d(meshes.add(Sphere::new(400.0))), MeshMaterial3d(dark), Transform::from_xyz(widest / 2.0, 0.0, 0.0)));
    let bottom = -(tallest - 1.0) * ROW - 0.5 - PLANK - 0.4;
    commands.spawn((MuseumThing, layer(), Mesh3d(meshes.add(Plane3d::default().mesh().size(widest + 60.0, 40.0 + wall_w[1].max(wall_w[2])))), MeshMaterial3d(floor), Transform::from_xyz(widest / 2.0, bottom, 10.0)));
    // begin at the shelf and game the cursor is on
    m.row = m.rows.iter().position(|r| r.shelf == cursor.shelf).unwrap_or(0);
    m.col = m.rows.get(m.row).and_then(|r| r.games.iter().position(|&g| g == cursor.game)).unwrap_or(0);
    let (eye, look) = m.view();
    (m.eye, m.look) = (eye + Vec3::new(0.0, 0.0, 6.0), look);
    paint(m, lib, covers, loader, materials);
}

/// The spines of the rows near the one in view, painted once each.
fn paint(m: &mut Museum, lib: &rustyshelf_core::library::Library, covers: &mut Covers, loader: &ArtLoader, materials: &mut Assets<StandardMaterial>) {
    let (from, to) = (m.row.saturating_sub(NEAR), (m.row + NEAR + 1).min(m.rows.len()));
    for r in &mut m.rows[from..to] {
        if std::mem::replace(&mut r.painted, true) {
            continue;
        }
        let s = &lib.shelves[r.shelf];
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
    mut camera: Query<(&mut Camera, &mut Transform), (With<MuseumCamera>, Without<Case>, Without<RowLight>)>,
    mut cases: Query<(&Case, &mut Transform), (Without<MuseumCamera>, Without<RowLight>)>,
    mut lights: Query<(&mut SpotLight, &Transform), (With<RowLight>, Without<Case>, Without<MuseumCamera>)>,
    things: Query<(Entity, Has<MuseumHud>), Or<(With<MuseumThing>, With<MuseumHud>)>>,
    mut text: Query<&mut Text, With<MuseumText>>,
) {
    let LibraryState::Ready(l) = &lib.0 else { return };
    let Making { covers, loader, meshes, materials, mona } = &mut making;
    let mona = &**mona;
    let (covers, loader, meshes, materials) = (&mut **covers, &**loader, &mut **meshes, &mut **materials);
    let now = time.elapsed_secs();
    let m = &mut *museum;
    let pad = |b: GamepadButton| pads.iter().any(|p| p.just_pressed(b));
    if !m.open {
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
        if free && (keys.just_pressed(KeyCode::KeyM) || held) {
            m.select_since = None;
            m.open = true;
            m.grouping = l.config.raw().get("ui").and_then(|u| u.get("museumStyle")).and_then(|x| x.get("grouping")).and_then(|g| g.as_str()).unwrap_or("classic").to_string();
            m.kept = kept(l, &others.plays.0);
            m.plan = (m.grouping != "classic").then(|| rustyshelf_core::museum_plan::plan_museum(&entries(l, &m.kept), &m.grouping));
            // the room the game under the cursor is in
            let here = l.shelves.get(cursor.shelf).and_then(|s| s.games.get(cursor.game)).map(|g| g.id.clone());
            m.room = match (&m.plan, here) {
                (Some(p), Some(id)) => p.by_game.get(&id).and_then(|at| p.rooms.iter().position(|r| r.id == at.room)).unwrap_or(0),
                _ => 0,
            };
            build(&mut commands, m, l, &cursor, meshes, materials, covers, loader);
            if let Ok((mut cam, _)) = camera.single_mut() {
                cam.is_active = true;
            }
            if let Ok(mut shelf) = others.shelf_camera.single_mut() {
                shelf.is_active = false;
            }
            spawn_text(&mut commands, m.camera, mona);
            (m.pull, m.moved_at, m.shown_front) = (0.0, now, None);
            sounds.play("open", 1);
            settings.keys_taken = true;
        }
        return;
    }
    settings.keys_taken = true;
    if m.walk.is_some() {
        return walk_on(&mut commands, m, l, &time, &keys, &pads, &settings, &mut cursor, &mut sounds, (covers, loader, meshes, materials), &mut camera, &things, &mut text, &mut others.shelf_camera);
    }
    // into the arcade: W, or Y on a pad
    if keys.just_pressed(KeyCode::KeyW) || pad(GamepadButton::North) {
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
    // another room: [ and ], or L1 and R1 (the media room has no shelves yet: it's passed by)
    let to_room = (keys.just_pressed(KeyCode::BracketRight) || pad(GamepadButton::RightTrigger)) as i32 - (keys.just_pressed(KeyCode::BracketLeft) || pad(GamepadButton::LeftTrigger)) as i32;
    if let Some(n) = m.plan.as_ref().map(|p| p.rooms.iter().filter(|r| !r.segments.is_empty()).count()).filter(|n| *n > 1 && to_room != 0) {
        let rooms: Vec<usize> = m.plan.as_ref().map(|p| p.rooms.iter().enumerate().filter(|(_, r)| !r.segments.is_empty()).map(|(i, _)| i).collect()).unwrap_or_default();
        let at = rooms.iter().position(|&i| i == m.room).unwrap_or(0) as i32;
        m.room = rooms[(at + to_room).rem_euclid(n as i32) as usize];
        for (e, _) in things.iter().filter(|(_, hud)| !hud) {
            commands.entity(e).despawn();
        }
        let held_cursor = Cursor { shelf: usize::MAX, ..Cursor::default() };
        build(&mut commands, m, l, &held_cursor, meshes, materials, covers, loader);
        (m.pull, m.moved_at, m.shown_front) = (0.0, now, None);
        sounds.play("shelf", to_room);
        return;
    }
    if m.rows.is_empty() || keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::Backspace) || keys.just_pressed(KeyCode::KeyM) || pad(settings.confirm().1) {
        return close(&mut commands, m, &things, &mut camera, &mut others.shelf_camera, &mut sounds);
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
    let fresh = tap != Vec2::ZERO;
    let held = if fresh { tap } else { held };
    if held == Vec2::ZERO {
        m.repeat_at = 0.0;
    } else if fresh || m.repeat_at == 0.0 || now >= m.repeat_at {
        m.repeat_at = now + if fresh || m.repeat_at == 0.0 { 0.38 } else { 0.09 };
        if held.y != 0.0 {
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
            }
        }
        m.moved_at = now;
    }
    if keys.just_pressed(KeyCode::Enter) || pad(settings.confirm().0) {
        let r = &m.rows[m.row];
        (cursor.shelf, cursor.game, cursor.land) = (r.shelf, r.games[m.col], false);
        settings.play = true;
        return close(&mut commands, m, &things, &mut camera, &mut others.shelf_camera, &mut sounds);
    }
    // the chosen case's cover, once it has stood still a moment
    let chosen = (m.row, m.col);
    if m.shown_front != Some(chosen) && now - m.moved_at > 0.2 {
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
    let settle = now - m.moved_at > 0.15;
    m.pull = if settle { (m.pull + dt * 3.0).min(1.0) } else { (m.pull - dt * 6.0).max(0.0) };
    for (c, mut t) in &mut cases {
        let k = if c.row == m.row && c.col == m.col { m.pull } else { 0.0 };
        let (out, face) = (ease((k / 0.62).min(1.0)), ease(((k - 0.55) / 0.45).clamp(0.0, 1.0)));
        t.translation = c.home + Vec3::new(0.0, out * 0.06, out * (c.w * PULL + c.d));
        t.rotation = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2 * face);
    }
    let (eye, look) = m.view();
    let f = 1.0 - (-dt * 5.0).exp();
    m.eye = m.eye.lerp(eye, f);
    m.look = m.look.lerp(look, f);
    if let Ok((_, mut t)) = camera.single_mut() {
        *t = Transform::from_translation(m.eye).looking_at(m.look, Vec3::Y);
    }
    // the lamp over the row in view is on, the others dim
    let y = m.rows[m.row].y;
    for (mut s, t) in &mut lights {
        let want = if (t.translation.y - (y + ROW - 0.62)).abs() < 0.01 { 260_000.0 } else { 60_000.0 };
        s.intensity += (want - s.intensity) * f;
    }
    if let Ok(mut text) = text.single_mut() {
        let r = &m.rows[m.row];
        let g = &l.shelves[r.shelf].games[r.games[m.col]];
        let room = m.room_title().map(|t| format!("{t}  ·  ")).unwrap_or_default();
        text.0 = format!("{}  ·  {}\n{room}{} of {}", g.title, r.name, m.col + 1, r.games.len());
    }
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
    (covers, loader, meshes, materials): (&mut Covers, &ArtLoader, &mut Assets<Mesh>, &mut Assets<StandardMaterial>),
    camera: &mut Query<(&mut Camera, &mut Transform), (With<MuseumCamera>, Without<Case>, Without<RowLight>)>,
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
        build(commands, m, l, &held_cursor, meshes, materials, covers, loader);
        sounds.play("back", 1);
        return;
    }
    let Some(w) = m.walk.as_mut() else { return };
    if keys.just_pressed(KeyCode::Enter) || pad(settings.confirm().0) {
        if let Some(k) = w.nearest() {
            (cursor.shelf, cursor.game, cursor.land) = (w.shelf, w.cabinets[k].0, false);
            settings_play(commands);
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
    if let Ok((_, mut t)) = camera.single_mut() {
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
    camera: &mut Query<(&mut Camera, &mut Transform), (With<MuseumCamera>, Without<Case>, Without<RowLight>)>,
    shelf_camera: &mut Query<&mut Camera, (With<crate::shelf::ShelfCamera>, Without<MuseumCamera>)>,
    sounds: &mut crate::sound::Sounds,
) {
    if let Ok(mut shelf) = shelf_camera.single_mut() {
        shelf.is_active = true;
    }
    for (e, _) in things {
        commands.entity(e).despawn();
    }
    if let Ok((mut cam, _)) = camera.single_mut() {
        cam.is_active = false;
    }
    m.open = false;
    m.rows.clear();
    sounds.play("back", 1);
}

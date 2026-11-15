// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the walkable
// arcade by TrapEmAll (https://github.com/TrapEmAll) in RustyShelf's renderer/museum.js (enterArcade, buildArcade,
// stepWalk), as RustyShelf has it.
//! The arcade wing of the museum, walked through on foot: a foyer, a corridor, and genre rooms off it with a
//! cabinet per arcade game, its cover on the screen. The plan (where every room, wall and cabinet stands) is
//! rustyshelf_core::museum_plan::plan_arcade; walls and cabinets can't be walked through.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rustyshelf_core::museum_plan::{plan_arcade, Arcade, Entry, CABINET};

use crate::loading::ArtLoader;
use crate::shelf::{cover_of, Covers};

/// eye height, and how close the player comes to a wall
pub const EYE: f32 = 1.6;
const PLAYER_R: f32 = 0.34;
/// a cabinet's own radius for walking round it
const CAB_R: f32 = 0.5;
/// how near a cabinet has to be, in front of you, to be played
const REACH: f32 = 2.3;

/// The player in the arcade: where, which way (0 faces +z), and the room or the hall they're in.
pub struct Walk {
    pub plan: Arcade,
    pub x: f32,
    pub z: f32,
    pub ry: f32,
    /// the shelf the arcade games are on, and each cabinet's game there with where it stands
    pub shelf: usize,
    pub cabinets: Vec<(usize, Vec2, String)>,
}

impl Walk {
    /// Plans the arcade from the arcade shelf's games and starts at the foyer, facing the corridor.
    pub fn start(lib: &rustyshelf_core::library::Library, cap: Option<f64>) -> Option<Walk> {
        let shelf = lib.shelves.iter().position(|s| s.id == "arcade")?;
        let s = &lib.shelves[shelf];
        let entries: Vec<Entry> = s
            .games
            .iter()
            .filter(|g| !matches!(g.source, rustyshelf_core::scanner::Source::Tile(_)))
            .map(|g| Entry { id: g.id.clone(), system: "arcade".into(), genre: g.info.as_ref().and_then(|i| i.genres.as_deref()).and_then(|x| x.split(';').next()).unwrap_or("").to_string() })
            .collect();
        if entries.is_empty() {
            return None;
        }
        let plan = plan_arcade(&entries, cap);
        let mut cabinets = Vec::new();
        for r in &plan.rooms {
            for (id, slot) in r.game_ids.iter().zip(&r.slots) {
                if let Some(gi) = s.games.iter().position(|g| &g.id == id) {
                    cabinets.push((gi, Vec2::new(slot.x as f32, slot.z as f32), r.title.clone()));
                }
            }
        }
        Some(Walk { plan, x: -1.2, z: 0.0, ry: std::f32::consts::FRAC_PI_2, shelf, cabinets })
    }

    /// Moves by `forward` and `side` (metres) along the heading, then out of any wall or cabinet it went into, in
    /// small steps so a fast move can't pass through a wall.
    pub fn step(&mut self, forward: f32, side: f32) {
        let (sin, cos) = self.ry.sin_cos();
        let (vx, vz) = (forward * sin + side * cos, forward * cos - side * sin);
        let n = ((vx.hypot(vz) / 0.25).ceil() as usize).max(1);
        for _ in 0..n {
            self.x += vx / n as f32;
            self.z += vz / n as f32;
            self.push_out();
        }
    }

    fn push_out(&mut self) {
        let p = Vec2::new(self.x, self.z);
        let mut p2 = p;
        for w in &self.plan.walls {
            let (a, b) = (Vec2::new(w[0] as f32, w[1] as f32), Vec2::new(w[2] as f32, w[3] as f32));
            let ab = b - a;
            let t = if ab.length_squared() > 0.0 { ((p2 - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
            let near = a + ab * t;
            let d = p2 - near;
            let len = d.length();
            if len < PLAYER_R {
                p2 = near + if len > 1e-4 { d / len } else { Vec2::new(-ab.y, ab.x).normalize_or_zero() } * PLAYER_R;
            }
        }
        for (_, at, _) in &self.cabinets {
            let d = p2 - *at;
            let len = d.length();
            if len < PLAYER_R + CAB_R && len > 1e-4 {
                p2 = *at + d / len * (PLAYER_R + CAB_R);
            }
        }
        (self.x, self.z) = (p2.x, p2.y);
    }

    /// The cabinet within reach and roughly in front: its place in the list.
    pub fn nearest(&self) -> Option<usize> {
        let (fx, fz) = (self.ry.sin(), self.ry.cos());
        self.cabinets
            .iter()
            .enumerate()
            .filter_map(|(i, (_, at, _))| {
                let (dx, dz) = (at.x - self.x, at.y - self.z);
                let d = dx.hypot(dz);
                (d <= REACH && (dx * fx + dz * fz) / d.max(0.001) >= 0.45).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// The room the player stands in, or the hall.
    pub fn place(&self) -> String {
        let hit = self.plan.rooms.iter().find(|r| (self.x as f64 - r.x).abs() <= r.w / 2.0 && (self.z as f64 - r.z).abs() <= r.d / 2.0);
        hit.map_or_else(|| "Arcade".to_string(), |r| r.title.clone())
    }

    /// The camera at eye height, looking the way the player faces.
    pub fn eye(&self) -> Transform {
        let at = Vec3::new(self.x, EYE, self.z);
        Transform::from_translation(at).looking_at(at + Vec3::new(self.ry.sin(), 0.0, self.ry.cos()) * 3.0, Vec3::Y)
    }
}

/// Builds the arcade: floors, walls, a warm light per room, and the cabinets with their games' covers.
pub fn build(
    commands: &mut Commands,
    w: &Walk,
    lib: &rustyshelf_core::library::Library,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    covers: &mut Covers,
    loader: &ArtLoader,
    mark: impl Fn() -> (crate::museum::MuseumThing, RenderLayers),
) {
    let mat = |m: &mut Assets<StandardMaterial>, c: Color, rough: f32| m.add(StandardMaterial { base_color: c, perceptual_roughness: rough, ..default() });
    let floor = mat(materials, Color::srgb(0.14, 0.15, 0.2), 0.85);
    let wall = mat(materials, Color::srgb(0.1, 0.11, 0.15), 0.9);
    let body = mat(materials, Color::srgb(0.08, 0.085, 0.11), 0.55);
    let dark = materials.add(StandardMaterial { base_color: Color::srgb(0.02, 0.02, 0.03), unlit: true, cull_mode: None, ..default() });
    let p = &w.plan;
    let east = p.corridor_end as f32;
    commands.spawn((mark(), Mesh3d(meshes.add(Sphere::new(400.0))), MeshMaterial3d(dark), Transform::from_xyz(east / 2.0, 0.0, 0.0)));
    // one floor under the foyer, the corridor and every room
    let deep = p.rooms.iter().map(|r| (r.z.abs() + r.d / 2.0) as f32).fold(4.0, f32::max);
    commands.spawn((mark(), Mesh3d(meshes.add(Plane3d::default().mesh().size(east + 12.0, deep * 2.0 + 2.0))), MeshMaterial3d(floor), Transform::from_xyz(east / 2.0, 0.0, 0.0)));
    for seg in &p.walls {
        let (a, b) = (Vec2::new(seg[0] as f32, seg[1] as f32), Vec2::new(seg[2] as f32, seg[3] as f32));
        let len = a.distance(b);
        if len < 0.01 {
            continue;
        }
        let mid = (a + b) / 2.0;
        let turn = -(b.y - a.y).atan2(b.x - a.x);
        commands.spawn((mark(), Mesh3d(meshes.add(Cuboid::new(len, 3.2, 0.14))), MeshMaterial3d(wall.clone()), Transform::from_xyz(mid.x, 1.6, mid.y).with_rotation(Quat::from_rotation_y(turn))));
    }
    // light: the foyer, the corridor and each room
    let lamp = |c: &mut Commands, x: f32, z: f32, power: f32| {
        c.spawn((mark(), PointLight { color: Color::srgb(1.0, 0.86, 0.66), intensity: power, range: 14.0, shadow_maps_enabled: false, ..default() }, Transform::from_xyz(x, 3.0, z)));
    };
    lamp(commands, 0.0, 0.0, 900_000.0);
    let mut x = 4.5;
    while x < east {
        lamp(commands, x + 3.0, 0.0, 600_000.0);
        x += 8.0;
    }
    for r in &p.rooms {
        lamp(commands, r.x as f32, r.z as f32, 1_400_000.0);
    }
    // the cabinets: a body, its screen (the game's cover) on the side it faces with, a marquee in its room's colour
    let s = &lib.shelves[w.shelf];
    let body_mesh = meshes.add(Cuboid::new(CABINET.0 as f32, 1.72, CABINET.1 as f32));
    let screen_mesh = meshes.add(Rectangle::new(0.62, 0.5));
    let marquee_mesh = meshes.add(Cuboid::new(0.8, 0.26, 0.16));
    for (gi, at, room) in &w.cabinets {
        let g = &s.games[*gi];
        let slot = p.rooms.iter().flat_map(|r| r.game_ids.iter().zip(&r.slots)).find(|(id, _)| **id == g.id).map(|(_, sl)| sl.ry as f32).unwrap_or(0.0);
        let hue = (rustyshelf_core::drawn::hash(room) % 360) as f32;
        let marquee = materials.add(StandardMaterial { base_color: Color::hsl(hue, 0.75, 0.62), emissive: LinearRgba::from(Color::hsl(hue, 0.75, 0.4)) * 3.0, ..default() });
        let screen = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
        let look = rustyshelf_core::looks::look_for(&s.id, g);
        covers.want(cover_of(g, &s.id, &look).0, (screen.clone(), 0.62 / 0.5), loader, materials);
        // the screen faces the cabinet's -z side: a north room's cabinets are turned round to face the corridor
        commands.spawn((mark(), Mesh3d(body_mesh.clone()), MeshMaterial3d(body.clone()), Transform::from_xyz(at.x, 0.86, at.y).with_rotation(Quat::from_rotation_y(slot)))).with_children(|c| {
            c.spawn((mark().1, Mesh3d(screen_mesh.clone()), MeshMaterial3d(screen), Transform::from_xyz(0.0, 0.32, -(CABINET.1 as f32) / 2.0 - 0.005).with_rotation(Quat::from_rotation_y(std::f32::consts::PI))));
            c.spawn((mark().1, Mesh3d(marquee_mesh.clone()), MeshMaterial3d(marquee), Transform::from_xyz(0.0, 0.98, -(CABINET.1 as f32) / 2.0 + 0.06)));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk() -> Walk {
        let entries: Vec<Entry> = (0..30).map(|i| Entry { id: format!("a{i}"), system: "arcade".into(), genre: if i % 2 == 0 { "Fighting".into() } else { "Racing".into() } }).collect();
        let plan = plan_arcade(&entries, None);
        let cabinets = plan.rooms.iter().flat_map(|r| r.slots.iter().map(|s| (0, Vec2::new(s.x as f32, s.z as f32), r.title.clone()))).collect();
        Walk { plan, x: -1.2, z: 0.0, ry: std::f32::consts::FRAC_PI_2, shelf: 0, cabinets }
    }

    #[test]
    fn the_foyer_walls_and_the_corridor_end_hold() {
        let mut w = walk();
        // backwards into the museum door wall (x = -4.5): stopped short of it
        w.step(-10.0, 0.0);
        assert!(w.x >= -4.5 + PLAYER_R - 0.01, "{}", w.x);
        // forward all the way down the corridor: stopped at its east end
        w.step(500.0, 0.0);
        assert!((w.x as f64) <= w.plan.corridor_end - PLAYER_R as f64 + 0.01, "{} {}", w.x, w.plan.corridor_end);
        assert!(w.z.abs() < 2.1);
    }

    #[test]
    fn a_room_is_walked_into_and_its_cabinets_reached() {
        let mut w = walk();
        let r = &w.plan.rooms[0];
        let (dx, dz) = (r.door.x as f32, r.door.z as f32);
        // to the doorway along the corridor, then turned to the room (north: -z) and in
        w.step(dx - w.x, 0.0);
        w.ry = if dz < 0.0 { std::f32::consts::PI } else { 0.0 };
        w.step(3.0, 0.0);
        assert_eq!(w.place(), w.plan.rooms[0].title);
        // a step in front of a cabinet, facing it: that one is in reach; turned away, none is
        let at = w.cabinets[0].1;
        (w.x, w.z) = (at.x, at.y + if dz < 0.0 { 1.5 } else { -1.5 });
        assert_eq!(w.nearest(), Some(0));
        w.ry += std::f32::consts::PI;
        assert_eq!(w.nearest(), None);
    }
}

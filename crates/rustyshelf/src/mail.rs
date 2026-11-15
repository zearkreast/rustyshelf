// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/museum.js (the delivery: buildDelivery, stepDelivery, finishDelivery), as RustyShelf has it.
//! New games by mail: the games added since your last visit wait in a cardboard box that's brought to you after the
//! lights come on. Opening it tears the tape and folds the flaps back; each game rises out with its cover to you and
//! flies to its place on the shelf, turning to stand spine out; the empty box drops away. A game still in its box
//! waits for the next visit to that room.

use std::collections::HashSet;
use std::path::Path;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use serde_json::{json, Value};

/// How far in front of you the box is held, and how big it's drawn.
const DIST: f32 = 2.05;
const SCALE: f32 = 0.4;

/// What was on the walls last visit, and the games still waiting in a box; None the first time.
pub fn load(data_dir: &Path) -> (Option<HashSet<String>>, Vec<String>) {
    let Some(v) = std::fs::read_to_string(data_dir.join("museum.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return (None, Vec::new()) };
    let ids = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>()).unwrap_or_default();
    (Some(ids("seen").into_iter().collect()), ids("mail"))
}

pub fn save(data_dir: &Path, seen: &HashSet<String>, mail: &[String]) {
    let mut seen: Vec<&String> = seen.iter().collect();
    seen.sort();
    let _ = std::fs::write(data_dir.join("museum.json"), json!({ "seen": seen, "mail": mail }).to_string());
}

/// A box of new games: the cases inside (each its row and place), its parts, and how far it has got.
pub struct Delivery {
    pub cases: Vec<(usize, usize)>,
    root: Entity,
    flaps: [Entity; 2],
    tape: Entity,
    width: f32,
    height: f32,
    materials: Vec<(Handle<StandardMaterial>, Color)>,
    /// seconds since it was brought to you, and since it was opened
    pub shown: f32,
    pub opened: Option<f32>,
}

/// Makes the box (out of sight until it's shown) for `cases`, whose cases are `case_w` wide; `label` is drawn on it.
pub fn build(commands: &mut Commands, cases: Vec<(usize, usize)>, case_w: f32, label: crate::loading::Art, layer: RenderLayers, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, covers: &mut crate::shelf::Covers, loader: &crate::loading::ArtLoader) -> Delivery {
    let n = cases.len();
    let (ph, pw, pd, t) = (1.06, (case_w * 1.14).max((0.7 + 0.14 * (n as f32 - 1.0)).min(1.3)), 0.7, 0.012);
    let mut mats = Vec::new();
    let mut mat = |c: Color, rough: f32| {
        let h = materials.add(StandardMaterial { base_color: c, perceptual_roughness: rough, alpha_mode: AlphaMode::Blend, ..default() });
        mats.push((h.clone(), c));
        h
    };
    let (card, inner, tape) = (mat(Color::srgb_u8(0xb9, 0x8b, 0x55), 0.9), mat(Color::srgb_u8(0x5a, 0x3c, 0x1c), 1.0), mat(Color::srgba_u8(0xd9, 0xb2, 0x7a, 235), 0.35));
    let label_mat = mat(Color::WHITE, 0.7);
    covers.want(label, (label_mat.clone(), 0.0), loader, materials);
    let root = commands.spawn((crate::museum::MuseumThing, layer.clone(), Transform::from_xyz(0.0, -50.0, 0.0).with_scale(Vec3::splat(SCALE)), Visibility::Hidden)).id();
    let mut part = |c: &mut Commands, mesh: Mesh, m: &Handle<StandardMaterial>, at: Vec3, parent: Entity| c.spawn((layer.clone(), Mesh3d(meshes.add(mesh)), MeshMaterial3d(m.clone()), Transform::from_translation(at), ChildOf(parent))).id();
    part(commands, Cuboid::new(pw, t, pd).into(), &card, Vec3::new(0.0, -ph / 2.0 + t / 2.0, 0.0), root);
    for sx in [-1.0, 1.0] {
        part(commands, Cuboid::new(t, ph, pd).into(), &card, Vec3::new(sx * (pw / 2.0 - t / 2.0), 0.0, 0.0), root);
    }
    part(commands, Cuboid::new(pw, ph, t).into(), &inner, Vec3::new(0.0, 0.0, -pd / 2.0 + t / 2.0), root);
    part(commands, Cuboid::new(pw, ph, t).into(), &card, Vec3::new(0.0, 0.0, pd / 2.0 - t / 2.0), root);
    let lh = (ph * 0.8).min(pw * 0.78 * 420.0 / 320.0);
    part(commands, Rectangle::new(lh * 320.0 / 420.0, lh).into(), &label_mat, Vec3::new(if n == 1 { 0.0 } else { -pw * 0.18 }, -ph * 0.06, pd / 2.0 + 0.002), root);
    let flaps = [-1.0f32, 1.0].map(|sx| {
        let hinge = commands.spawn((layer.clone(), Transform::from_xyz(sx * pw / 2.0, ph / 2.0, 0.0), Visibility::Inherited, ChildOf(root))).id();
        part(commands, Cuboid::new(pw / 2.0, t, pd).into(), &card, Vec3::new(-sx * pw / 4.0, t / 2.0, 0.0), hinge);
        part(commands, Cuboid::new(pw * 0.09, t * 0.5, pd * 1.02).into(), &tape, Vec3::new(-sx * (pw / 2.0 - pw * 0.045), t + 0.002, 0.0), hinge);
        hinge
    });
    let tape = part(commands, Cuboid::new(pw * 0.18, ph * 0.22, t * 0.5).into(), &tape, Vec3::new(0.0, ph / 2.0 - ph * 0.11, pd / 2.0 + 0.004), root);
    Delivery { cases, root, flaps, tape, width: pw, height: ph, materials: mats, shown: 0.0, opened: None }
}

fn ease(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// The part of the way from `a` to `b` seconds that `t` is, eased.
fn between(a: f32, b: f32, t: f32) -> f32 {
    ease((t - a) / (b - a))
}

/// The games come out quicker the more there are: about five seconds for a hundred.
fn stagger(n: usize) -> f32 {
    (3.4 / n as f32).min(0.34)
}

fn start(k: usize, n: usize) -> f32 {
    0.7 + k as f32 * stagger(n)
}

impl Delivery {
    /// How long it takes from opening to the last game on its shelf.
    pub fn length(&self) -> f32 {
        let n = self.cases.len();
        start(n - 1, n) + 1.35
    }

    /// Moves the box in front of the camera (`eye`, looking along `turn`) and opens it; returns each game's place in
    /// the room while it's on its way (None: still in the box), its turn and its size.
    pub fn step(&mut self, dt: f32, now: f32, eye: Transform, case_h: &[f32], commands: &mut Commands, materials: &mut Assets<StandardMaterial>) -> Vec<Option<(Vec3, Quat, f32)>> {
        self.shown += dt;
        // it comes up into view with a little bump
        let pop = (self.shown / 0.6).min(1.0);
        let grow = if pop < 1.0 { 1.0 + 2.2 * (pop - 1.0).powi(3) + 1.2 * (pop - 1.0).powi(2) } else { 1.0 };
        let drop = (1.0 - ease(self.shown / 0.45)) * 0.55;
        let bump = if self.shown > 0.45 && self.shown < 0.8 { ((self.shown - 0.45) / 0.35 * std::f32::consts::PI).sin() * 0.025 } else { 0.0 };
        let n = self.cases.len();
        let t = self.opened.unwrap_or(0.0);
        let gone = if self.opened.is_some() { between(start(n - 1, n) + 0.45, start(n - 1, n) + 1.0, t) } else { 0.0 };
        let mut place = Transform::from_translation(eye.translation + eye.forward() * DIST + eye.up() * (-0.24 + drop + bump - gone * 0.5 * SCALE)).with_rotation(eye.rotation).with_scale(Vec3::splat(SCALE * grow.max(0.01)));
        if self.opened.is_none() {
            // waiting: it sways a little
            place.rotate_local_y((now / 1.4).sin() * 0.05);
        }
        commands.entity(self.root).insert((place, Visibility::Inherited));
        let length = self.length();
        let Some(t) = self.opened.as_mut() else { return vec![None; n] };
        *t = (*t + dt).min(length);
        let t = *t;
        let tape_at = Vec3::new(0.0, self.height / 2.0 - self.height * 0.11, 0.35 + 0.004);
        commands.entity(self.tape).insert(Transform::from_translation(tape_at).with_scale(Vec3::new(1.0, (1.0 - between(0.0, 0.3, t)).max(0.001), 1.0)));
        let fold = between(0.15, 0.7, t) * 2.1;
        for (k, flap) in self.flaps.iter().enumerate() {
            let sx = if k == 0 { -1.0 } else { 1.0 };
            commands.entity(*flap).insert(Transform::from_xyz(sx * self.width / 2.0, self.height / 2.0, 0.0).with_rotation(Quat::from_rotation_z(-sx * fold)));
        }
        if gone > 0.0 {
            for (m, c) in &self.materials {
                if let Some(mut mat) = materials.get_mut(m) {
                    mat.base_color = c.with_alpha(c.alpha() * (1.0 - gone));
                }
            }
        }
        // each game: up out of the box with its cover to you, then off to its own place
        let cols = n.min(5);
        (0..n)
            .map(|k| {
                let s0 = start(k, n);
                let (up, go) = (between(s0, s0 + 0.5, t), between(s0 + 0.5, s0 + 1.25, t));
                if up <= 0.0 {
                    return None;
                }
                let xk = if n == 1 { 0.0 } else { ((k % cols) as f32 / (cols - 1).max(1) as f32 - 0.5) * self.width * 0.55 };
                let bh = case_h.get(k).copied().unwrap_or(1.0);
                let local = Vec3::new(xk, -self.height / 2.0 + bh / 2.0 + 0.015 + up * (bh + 0.1), 0.0);
                Some((place.transform_point(local), place.rotation * Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2), go))
            })
            .collect()
    }

    pub fn done(&self) -> bool {
        self.opened.is_some_and(|t| t >= self.length())
    }

    pub fn root(&self) -> Entity {
        self.root
    }
}

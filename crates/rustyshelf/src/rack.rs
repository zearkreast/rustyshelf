// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/museum.js (the reading corner: buildRack, rackCovers, placeholderTex), as RustyShelf has it.
//! The museum's reading corner: two racks beside the bookcase (or in the Reading & media room), Magazines then
//! Strategy guides, built like the bookcase, a slanted ledge level with each of its shelves. Each issue or guide
//! stands on a ledge with its cover, a brass tab where a brand's run starts; an empty rack holds a card saying so.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use rustyshelf_core::reading::{self, Item};

use crate::loading::{Art, ArtLoader};
use crate::shelf::Covers;

/// An issue's height and width, and the room between issues on a ledge.
pub const RACK_H: f32 = 0.9;
pub const RACK_W: f32 = RACK_H * 0.76;
const GAP: f32 = 0.1;
const PLANK: f32 = 0.055;
const ROW: f32 = 1.34;

/// One issue or guide on a ledge: what it is, the ledge (the museum row it's level with), where it stands, and its
/// cover's sheet.
pub struct Piece {
    pub item: Item,
    pub row: usize,
    pub x: f32,
    pub unit: usize,
    front: Handle<StandardMaterial>,
}

/// One rack: its name, and where it starts and ends along the wall.
pub struct Unit {
    pub label: &'static str,
    pub x0: f32,
    pub x1: f32,
}

#[derive(Default)]
pub struct Rack {
    pub pieces: Vec<Piece>,
    pub units: Vec<Unit>,
    covers: Option<Mutex<Receiver<(usize, PathBuf)>>>,
}

/// An issue on its ledge, by its place in the rack, leaning back until it's chosen.
#[derive(Component)]
pub struct RackPiece(pub usize);

impl Rack {
    /// The pieces on a ledge, in order along it.
    pub fn on_row(&self, row: usize) -> Vec<usize> {
        let mut on: Vec<usize> = (0..self.pieces.len()).filter(|&i| self.pieces[i].row == row).collect();
        on.sort_by(|a, b| self.pieces[*a].x.total_cmp(&self.pieces[*b].x));
        on
    }

    /// Puts each cover on its issue as it's made (off the frame: a PDF's page 1 is drawn, an archive's taken out).
    pub fn poll(&mut self, covers: &mut Covers, loader: &ArtLoader, materials: &mut Assets<StandardMaterial>) {
        let Some(rx) = self.covers.as_ref() else { return };
        let Ok(rx) = rx.lock() else { return };
        for (i, path) in rx.try_iter() {
            if let Some(p) = self.pieces.get(i) {
                covers.want(Art::File(path), (p.front.clone(), RACK_W / RACK_H), loader, materials);
            }
        }
    }
}

/// Builds the racks on the wall `parent` from `x0` on, a ledge level with each of `rows` (their heights, top down;
/// their places in the museum's rows), from the reading folder's items; `lamp` is its picture lamp's strength and colour,
/// `wood` the bookcase's.
#[allow(clippy::too_many_arguments)]
pub fn build(
    commands: &mut Commands,
    parent: Entity,
    rows: &[(usize, f32)],
    x0: f32,
    (lamp, lamp_color): (f32, Color),
    wood: Handle<StandardMaterial>,
    items: Vec<Item>,
    cache: PathBuf,
    lang: &str,
    layer: RenderLayers,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    covers: &mut Covers,
    loader: &ArtLoader,
) -> Rack {
    let tr = |t: &str| rustyshelf_core::i18n::tr(lang, t);
    let mut rack = Rack::default();
    if rows.is_empty() {
        return rack;
    }
    let (mut mags, mut guides): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.kind != "guide");
    reading::rack_order(&mut mags);
    reading::rack_order(&mut guides);
    let lip = materials.add(StandardMaterial { base_color: Color::srgb_u8(0xc9, 0xa8, 0x6a), metallic: 0.75, perceptual_roughness: 0.32, ..default() });
    let back = materials.add(StandardMaterial { base_color: Color::srgb_u8(0x4a, 0x36, 0x26), perceptual_roughness: 0.8, ..default() });
    let edge = materials.add(StandardMaterial { base_color: Color::srgb_u8(0xec, 0xe6, 0xd8), perceptual_roughness: 0.9, ..default() });
    let depth = 0.8;
    let (top_y, bottom_y) = (rows[0].1, rows[rows.len() - 1].1);
    let up_h = (top_y - bottom_y) + ROW + 0.4;
    let mid_y = (top_y + bottom_y) / 2.0;
    let rd = depth * 0.62;
    let rz = -depth / 2.0 + rd / 2.0;
    let crown_y = top_y + ROW - 0.5 + PLANK / 2.0 - 0.001;
    let mut x = x0;
    let at = |c: &mut Commands, mesh: Handle<Mesh>, mat: Handle<StandardMaterial>, t: Transform| {
        c.spawn((layer.clone(), Mesh3d(mesh), MeshMaterial3d(mat), t, ChildOf(parent)));
    };
    for (label, list, guide) in [("MAGAZINES", mags, false), ("STRATEGY GUIDES", guides, true)] {
        let mut per = 3;
        while list.len().div_ceil(per) > rows.len() && per < 300 {
            per += 1;
        }
        let list: Vec<Item> = list.into_iter().take(per * rows.len()).collect();
        let width = per as f32 * (RACK_W + GAP) + GAP;
        let (rx0, rx1) = (x, x + width);
        let cx = (rx0 + rx1) / 2.0;
        for px in [rx0 - 0.07, rx1 + 0.07] {
            at(commands, meshes.add(Cuboid::new(0.14, up_h, rd + 0.04)), wood.clone(), Transform::from_xyz(px, mid_y - 0.1, rz));
        }
        at(commands, meshes.add(Cuboid::new(width + 0.34, PLANK * 1.6, rd + 0.1)), wood.clone(), Transform::from_xyz(cx, crown_y + 0.02, rz + 0.03));
        at(commands, meshes.add(Cuboid::new(width + 0.28, 0.16, 0.05)), wood.clone(), Transform::from_xyz(cx, crown_y - 0.1, rz + rd / 2.0 + 0.03));
        at(commands, meshes.add(Rectangle::new(width, up_h - 0.1)), back.clone(), Transform::from_xyz(cx, mid_y - 0.1, -depth / 2.0 + 0.008));
        // the name plate, engraved brass on the crown
        let plate_w = (width * 0.7).min(1.7);
        let plate = materials.add(StandardMaterial { base_color: Color::WHITE, metallic: 0.6, perceptual_roughness: 0.32, ..default() });
        covers.want(Art::NamePlate { label: tr(label) }, (plate.clone(), 0.0), loader, materials);
        at(commands, meshes.add(Rectangle::new(plate_w, plate_w / 6.5)), plate, Transform::from_xyz(cx, crown_y - 0.1, rz + rd / 2.0 + 0.057));
        // its picture lamp
        commands.spawn((
            layer.clone(),
            SpotLight { color: lamp_color, intensity: lamp, range: up_h + 3.0, outer_angle: 0.9, inner_angle: 0.5, shadow_maps_enabled: false, ..default() },
            Transform::from_xyz(cx, crown_y + 0.4, 1.4).looking_at(Vec3::new(cx, mid_y, 0.0), Vec3::Y),
            ChildOf(parent),
        ));
        for &(_, y) in rows {
            at(commands, meshes.add(Cuboid::new(width, PLANK, 0.34)), wood.clone(), Transform::from_xyz(cx, y - 0.5 - PLANK / 2.0, -depth / 2.0 + 0.17));
            at(commands, meshes.add(Cuboid::new(width, 0.03, 0.015)), lip.clone(), Transform::from_xyz(cx, y - 0.5 + 0.015, -depth / 2.0 + 0.33));
        }
        let unit = rack.units.len();
        if list.is_empty() {
            let note = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.8, ..default() });
            let heading = if guide { [tr("No guides"), tr("yet")] } else { [tr("No magazines"), tr("yet")] };
            let lines = ["Put them in the", "reading folder", "(Settings → Magazines)"].iter().map(|l| tr(l)).collect();
            covers.want(Art::RackNote { heading, lines }, (note.clone(), 0.0), loader, materials);
            let h = RACK_W * 820.0 / 640.0;
            at(commands, meshes.add(Rectangle::new(RACK_W, h)), note, Transform::from_xyz(cx, rows[0].1 - 0.5 + h / 2.0 + 0.02, -depth / 2.0 + 0.16).with_rotation(Quat::from_rotation_x(-12f32.to_radians())));
        }
        let mut last_group = String::new();
        for (k, item) in list.into_iter().enumerate() {
            let (ri, c) = (k / per, k % per);
            let (row, y) = rows[ri];
            let th = if guide { 0.035 } else { 0.012 };
            let ix = rx0 + GAP + RACK_W / 2.0 + c as f32 * (RACK_W + GAP);
            let front = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.45, ..default() });
            covers.want(Art::RackCover { guide, title: item.title.clone(), series: item.series.clone() }, (front.clone(), RACK_W / RACK_H), loader, materials);
            let i = rack.pieces.len();
            commands
                .spawn((RackPiece(i), layer.clone(), Mesh3d(meshes.add(Cuboid::new(RACK_W, RACK_H, th))), MeshMaterial3d(edge.clone()), home(ix, y, false), ChildOf(parent)))
                .with_children(|p| {
                    p.spawn((layer.clone(), Mesh3d(meshes.add(Rectangle::new(RACK_W * 0.995, RACK_H * 0.995))), MeshMaterial3d(front.clone()), Transform::from_xyz(0.0, 0.0, th / 2.0 + 0.0005)));
                });
            // a brass tab on the lip where a brand's run starts
            let group = reading::group_of(&item);
            if !group.is_empty() && (c == 0 || group != last_group) {
                let tab = materials.add(StandardMaterial { base_color: Color::WHITE, metallic: 0.5, perceptual_roughness: 0.35, ..default() });
                covers.want(Art::Tabs { labels: vec![(group.clone(), 256)], tall: 48 }, (tab.clone(), 0.0), loader, materials);
                let th = RACK_W * 0.9 * 48.0 / 256.0;
                at(commands, meshes.add(Rectangle::new(RACK_W * 0.9, th)), tab, Transform::from_xyz(ix - RACK_W * 0.05, y - 0.5 - 0.055 - th / 2.0 + 0.012, -depth / 2.0 + 0.345));
            }
            last_group = group;
            rack.pieces.push(Piece { item, row, x: ix, unit, front });
        }
        rack.units.push(Unit { label: if guide { "Strategy guides" } else { "Magazines" }, x0: rx0, x1: rx1 });
        x = rx1 + 0.14 + 0.35;
    }
    // the covers, made off the frame, nearest the top first
    let todo: Vec<(usize, Item)> = rack.pieces.iter().enumerate().map(|(i, p)| (i, p.item.clone())).collect();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for (i, item) in todo {
            if let Ok(Some(path)) = reading::cover(&item, &cache, false) {
                if tx.send((i, path)).is_err() {
                    return;
                }
            }
        }
    });
    rack.covers = Some(Mutex::new(rx));
    rack
}

/// Where an issue stands on its ledge: leaning back, or (`chosen`) upright and out toward you.
pub fn home(x: f32, row_y: f32, chosen: bool) -> Transform {
    let (lean, out) = if chosen { (0.0, 0.18) } else { (-12f32.to_radians(), 0.0) };
    Transform::from_xyz(x, row_y - 0.5 + RACK_H / 2.0 + 0.01 + if chosen { 0.06 } else { 0.0 }, -0.4 + 0.2 + out).with_rotation(Quat::from_rotation_x(lean))
}

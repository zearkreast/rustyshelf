// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/consolefit.js and renderer/museum.js (loadModel), as RustyShelf has it.
//! Your own 3D console (`themes/<system>/console.glb`) at a museum row's end, instead of the stand-in: only the console
//! itself counts (a controller, cable or memory card that came with the model is hidden, as is any loose piece that
//! doesn't touch the console), it turns to face you (the side with the most small parts and detail, or
//! `console.json`'s museumYaw) and is sized to the console's slot on the shelf, standing on it.

use std::path::{Path, PathBuf};

use bevy::camera::visibility::RenderLayers;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;

/// What `console.json` says of the museum's console: its facing in degrees, its size, tilts, and which part names hide.
#[derive(Clone, Default)]
pub struct Opts {
    yaw: Option<f32>,
    scale: f32,
    pitch: f32,
    roll: f32,
    /// the words a part's name hides it by (lower case); none, nothing is hidden by name
    hide: Vec<String>,
}

const DEFAULT_HIDE: &str = "controller|gamepad|joystick|cable|memory card|memorycard|memory_card|memory-card|wire";

impl Opts {
    fn hides(&self, names: &str) -> bool {
        let names = names.to_lowercase();
        self.hide.iter().any(|w| names.contains(w.as_str()))
    }
}

/// A console's model, if one is there, and its options.
pub fn find(app_root: &Path, shelf: &str) -> Option<(PathBuf, Opts)> {
    if !shelf.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return None;
    }
    let dir = app_root.join("themes").join(shelf);
    let path = ["glb", "gltf"].iter().map(|e| dir.join(format!("console.{e}"))).find(|p| p.is_file())?;
    let json: serde_json::Value = std::fs::read_to_string(dir.join("console.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let num = |k: &str| json.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
    // (the Electron app reads `hide` as a pattern; its alternatives are taken here as plain words)
    let words = |s: &str| s.split('|').map(|w| w.replace(".?", " ").replace(['\\', '^', '$', '(', ')'], "").trim().to_lowercase()).filter(|w| !w.is_empty()).collect();
    let hide = match json.get("hide") {
        Some(serde_json::Value::Bool(false)) => Vec::new(),
        Some(serde_json::Value::String(s)) if !s.is_empty() => words(s),
        _ => words(DEFAULT_HIDE),
    };
    Some((path, Opts { yaw: num("museumYaw"), scale: num("museumScale").unwrap_or(1.0).clamp(0.3, 2.0), pitch: num("pitch").unwrap_or(0.0), roll: num("roll").unwrap_or(0.0), hide }))
}

/// A console model waiting to be fitted: the slot it fills (the middle of its foot, wall-local), its width, the space
/// above the shelf and the shelf's depth; the stand-in it replaces; and the frames it has been seen loaded.
#[derive(Component)]
pub struct Pending {
    at: Vec3,
    width: f32,
    height: f32,
    depth: f32,
    opts: Opts,
    stand_in: Option<Entity>,
    seen: u8,
}

/// Puts the model in place under `parent` (the row's wall) and returns its holder; it's fitted once it has loaded.
#[allow(clippy::too_many_arguments)]
pub fn spawn(commands: &mut Commands, assets: &AssetServer, parent: Entity, path: &Path, opts: Opts, (at, width, height, depth): (Vec3, f32, f32, f32), stand_in: Option<Entity>, layer: RenderLayers) -> Entity {
    let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_string_lossy().into_owned()));
    let holder = commands.spawn((layer.clone(), Transform::from_translation(at), Visibility::Hidden, ChildOf(parent), Pending { at, width, height, depth, opts, stand_in, seen: 0 })).id();
    commands.spawn((layer, bevy::world_serialization::WorldAssetRoot(scene), Transform::default(), ChildOf(holder)));
    holder
}

struct Part {
    entity: Entity,
    min: Vec3,
    max: Vec3,
    points: Vec<Vec3>,
    lit: bool,
}

/// Fits each loaded console: the parts that show, its facing, its size and its place on the shelf.
#[allow(clippy::type_complexity)]
pub fn fit(
    mut commands: Commands,
    mut pending: Query<(Entity, &mut Pending, &mut Transform, &mut Visibility, &GlobalTransform)>,
    children: Query<&Children>,
    nodes: Query<(Option<&Name>, Option<&Mesh3d>, Option<&MeshMaterial3d<StandardMaterial>>, &GlobalTransform, Option<&ChildOf>)>,
    meshes: Res<Assets<Mesh>>,
    materials: Res<Assets<StandardMaterial>>,
) {
    for (holder, mut p, mut t, mut vis, root_gt) in &mut pending {
        let all: Vec<Entity> = children.iter_descendants(holder).collect();
        let mesh_ents: Vec<Entity> = all.iter().copied().filter(|e| nodes.get(*e).is_ok_and(|n| n.1.is_some())).collect();
        if mesh_ents.is_empty() || mesh_ents.iter().any(|e| nodes.get(*e).ok().and_then(|n| n.1).is_none_or(|m| meshes.get(&m.0).is_none())) {
            continue;
        }
        // transforms settle a frame after the scene appears
        p.seen += 1;
        if p.seen < 3 {
            continue;
        }
        let to_root = root_gt.affine().inverse();
        let mut parts = Vec::new();
        let mut hidden = Vec::new();
        for &e in &mesh_ents {
            let Ok((_, Some(mesh), mat, gt, _)) = nodes.get(e) else { continue };
            let Some(m) = meshes.get(&mesh.0) else { continue };
            // the names on its way up, to hide a controller or a cable by name
            let mut names = String::new();
            let mut at = Some(e);
            while let Some(x) = at.filter(|x| *x != holder) {
                let Ok((name, _, _, _, up)) = nodes.get(x) else { break };
                if let Some(n) = name {
                    names.push(' ');
                    names.push_str(n.as_str());
                }
                at = up.map(|u| u.parent());
            }
            if p.opts.hides(&names) {
                hidden.push(e);
                continue;
            }
            let rel = to_root * gt.affine();
            let points: Vec<Vec3> = match m.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()) {
                Some(ps) => ps.iter().map(|q| rel.transform_point3(Vec3::from(*q))).collect(),
                None => continue,
            };
            if points.is_empty() {
                continue;
            }
            let (min, max) = points.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), q| (a.min(*q), b.max(*q)));
            let lit = mat.and_then(|h| materials.get(&h.0)).is_some_and(|m| m.emissive.red + m.emissive.green + m.emissive.blue > 0.0);
            parts.push(Part { entity: e, min, max, points, lit });
        }
        if parts.is_empty() {
            commands.entity(holder).remove::<Pending>();
            continue;
        }
        // the console: the biggest group of parts that touch each other; loose pieces beside it hide
        let (amin, amax) = parts.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), q| (a.min(q.min), b.max(q.max)));
        let pad = (amax - amin).length().max(1e-6) * 0.02;
        let mut up: Vec<usize> = (0..parts.len()).collect();
        fn root(up: &mut [usize], i: usize) -> usize {
            if up[i] != i {
                up[i] = root(up, up[i]);
            }
            up[i]
        }
        if parts.len() <= 1500 {
            for i in 0..parts.len() {
                for j in i + 1..parts.len() {
                    let (a, b) = (&parts[i], &parts[j]);
                    if (a.min - pad).cmple(b.max).all() && (a.max + pad).cmpge(b.min).all() {
                        let (ri, rj) = (root(&mut up, i), root(&mut up, j));
                        up[ri] = rj;
                    }
                }
            }
        }
        let vol = |q: &Part| ((q.max - q.min).max(Vec3::splat(1e-6))).element_product();
        let mut groups: std::collections::HashMap<usize, f32> = std::collections::HashMap::new();
        for (i, q) in parts.iter().enumerate() {
            *groups.entry(root(&mut up, i)).or_default() += vol(q);
        }
        let main = groups.iter().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |g| *g.0);
        let mut console = Vec::new();
        for (i, q) in parts.iter().enumerate() {
            if root(&mut up, i) == main {
                console.push(i);
            } else {
                hidden.push(q.entity);
            }
        }
        let (cmin, cmax) = console.iter().fold((Vec3::MAX, Vec3::MIN), |(a, b), &i| (a.min(parts[i].min), b.max(parts[i].max)));
        let yaw = p.opts.yaw.unwrap_or_else(|| guess_yaw(&parts, &console, cmin, cmax)).to_radians();
        let turn = Quat::from_euler(EulerRot::YXZ, yaw, p.opts.pitch.to_radians(), p.opts.roll.to_radians());
        // the size at its own facing, so a turned console isn't refitted to the turn
        let auto = Quat::from_euler(EulerRot::YXZ, p.opts.yaw.map_or(yaw, |_| guess_yaw(&parts, &console, cmin, cmax).to_radians()), p.opts.pitch.to_radians(), p.opts.roll.to_radians());
        let bounds = |q: Quat| console.iter().flat_map(|&i| parts[i].points.iter()).fold((Vec3::MAX, Vec3::MIN), |(a, b), v| (a.min(q * *v), b.max(q * *v)));
        let (smin, smax) = bounds(auto);
        let size = smax - smin;
        let s = (p.width * 0.95 / size.x.max(1e-3)).min(p.height * 0.92 / size.y.max(1e-3)).min(p.depth * 0.9 / size.z.max(1e-3)) * p.opts.scale;
        let (bmin, bmax) = bounds(turn);
        let c = (bmin + bmax) / 2.0;
        // a console running deeper than the shelf stands against the wall and pokes out a little
        let z = if (bmax.z - bmin.z) * s <= p.depth * 0.95 { -c.z * s } else { -p.depth / 2.0 + 0.04 - bmin.z * s };
        *t = Transform::from_translation(p.at + Vec3::new(-c.x * s, -bmin.y * s, z)).with_rotation(turn).with_scale(Vec3::splat(s));
        *vis = Visibility::Inherited;
        for e in hidden {
            commands.entity(e).insert(Visibility::Hidden);
        }
        // the museum's camera sees only its own layer
        for e in all {
            commands.entity(e).insert(RenderLayers::layer(3));
        }
        if let Some(e) = p.stand_in {
            commands.entity(e).despawn();
        }
        commands.entity(holder).remove::<Pending>();
    }
}

/// Which way is the front, in degrees: scored per side (+z, -x, -z, +x), the front is where the separate little pieces
/// sit (buttons, ports, the disc slot, the power light) and where the detail bunches up; lit parts count extra. A
/// console on its edge has its front on a narrow end, a flat one on a long side; glTF's own front (+z) wins a tie.
fn guess_yaw(parts: &[Part], console: &[usize], min: Vec3, max: Vec3) -> f32 {
    let size = max - min;
    if size.x <= 0.0 || size.z <= 0.0 {
        return 0.0;
    }
    // (axis: 0 x, 2 z; the high side; the yaw that brings it to face you)
    let faces = [(2, true, 0.0), (0, false, 90.0), (2, false, 180.0), (0, true, 270.0)];
    let near = |ax: usize, hi: bool, a: f32, k: f32| {
        let slab = max[ax] - min[ax];
        if hi {
            a > max[ax] - slab * k
        } else {
            a < min[ax] + slab * k
        }
    };
    let total: usize = console.iter().map(|&i| parts[i].points.len()).sum();
    let stride = total.div_ceil(120_000).max(1);
    let mut score = [(0.0f32, 0.0f32, 0.0f32); 4];
    for &i in console {
        let q = &parts[i];
        let c = (q.min + q.max) / 2.0;
        for (f, &(ax, hi, _)) in faces.iter().enumerate() {
            if near(ax, hi, c[ax], 0.25) {
                score[f].0 += 1.0;
                if q.lit {
                    score[f].2 += 1.0;
                }
            }
        }
        for v in q.points.iter().step_by(stride) {
            for (f, &(ax, hi, _)) in faces.iter().enumerate() {
                if near(ax, hi, v[ax], 0.08) {
                    score[f].1 += 1.0;
                }
            }
        }
    }
    let vt = score.iter().map(|s| s.1).sum::<f32>().max(1.0);
    let standing = size.x.min(size.z) < size.y * 0.5;
    let long = if size.x >= size.z { 0 } else { 2 };
    let square = (size.x - size.z).abs() / size.x.max(size.z) < 0.15;
    let total_of = |f: usize| {
        let (ax, hi, _) = faces[f];
        let shape = if square {
            0.0
        } else if standing {
            if ax == long { 1.0 } else { 0.0 }
        } else if ax != long {
            0.5
        } else {
            0.0
        };
        score[f].0 + 3.0 * score[f].2 + 4.0 * score[f].1 / vt + if ax == 2 && hi { 0.3 } else { 0.0 } + shape
    };
    // the first best side wins a tie
    faces[(1..4).fold(0, |best, f| if total_of(f) > total_of(best) { f } else { best })].2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(min: Vec3, max: Vec3, lit: bool) -> Part {
        let points = (0..8).map(|i| Vec3::new(if i & 1 == 0 { min.x } else { max.x }, if i & 2 == 0 { min.y } else { max.y }, if i & 4 == 0 { min.z } else { max.z })).collect();
        Part { entity: Entity::PLACEHOLDER, min, max, points, lit }
    }

    #[test]
    fn the_front_is_where_the_small_lit_parts_are() {
        let body = part(Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.4, 1.0), false);
        // a power light on the -x side: turned a quarter so that side faces you
        let light = part(Vec3::new(-1.02, 0.1, 0.2), Vec3::new(-0.98, 0.2, 0.3), true);
        let parts = [body, light];
        let (min, max) = (Vec3::new(-1.02, 0.0, -1.0), Vec3::new(1.0, 0.4, 1.0));
        assert_eq!(guess_yaw(&parts, &[0, 1], min, max), 90.0);
        // nothing to tell the sides apart: glTF's own front
        assert_eq!(guess_yaw(&parts[..1], &[0], Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.4, 1.0)), 0.0);
    }

    #[test]
    fn controllers_and_cables_hide_by_name() {
        let o = Opts { hide: DEFAULT_HIDE.split('|').map(String::from).collect(), ..Opts::default() };
        assert!(o.hides(" Scene Controller_L"));
        assert!(o.hides(" Memory Card 1"));
        assert!(!o.hides(" Body Lid"));
    }
}

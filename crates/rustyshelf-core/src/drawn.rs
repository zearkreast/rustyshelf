// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/covers.js, as RustyShelf has it.
//! Box art drawn for a game that has none yet: a scene made up from its title (the same title always draws the same
//! cover), the title across it, the console's band on top; and a back to match, with what's known of the game.

use std::f32::consts::PI;
use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, OutlineCurve};
use tiny_skia::{Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform};

use crate::looks::{Band, Look};

/// A drawn picture: its size, and its pixels as red, green, blue, alpha.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The width every cover is drawn at; its height follows the case's shape.
const W: f32 = 420.0;
const KINDS: [&str; 9] = ["platform", "party", "race", "rpg", "flight", "action", "puzzle", "sports", "horror"];

/// A number from a text: the same text, the same number (FNV-1a over its UTF-16 units, as the Electron app has it).
pub fn hash(s: &str) -> u32 {
    s.encode_utf16().fold(2_166_136_261u32, |h, c| (h ^ u32::from(c)).wrapping_mul(16_777_619))
}

/// The run of chances a cover is drawn with, started from the title's number.
struct Chance(u32);

impl Chance {
    fn new(seed: u32) -> Chance {
        Chance(if seed == 0 { 1 } else { seed })
    }

    /// 0 up to 1.
    fn roll(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (f64::from(t ^ (t >> 14)) / 4_294_967_296.0) as f32
    }
}

/// CSS's hsla(): a hue in degrees, saturation and lightness in percent.
fn hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    let (h, s, l) = (h.rem_euclid(360.0) / 360.0, s / 100.0, l / 100.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color::from_rgba(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0), a).unwrap_or(Color::BLACK)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, a).unwrap_or(Color::BLACK)
}

fn of(c: crate::looks::Rgba) -> Color {
    Color::from_rgba(c[0], c[1], c[2], c[3]).unwrap_or(Color::BLACK)
}

fn solid(c: Color) -> Paint<'static> {
    Paint { shader: Shader::SolidColor(c), anti_alias: true, ..Paint::default() }
}

/// A colour running into another from the height `y0` down to `y1`.
fn fade(y0: f32, y1: f32, top: Color, bottom: Color) -> Paint<'static> {
    let stops = vec![GradientStop::new(0.0, top), GradientStop::new(1.0, bottom)];
    let shader = LinearGradient::new(Point::from_xy(0.0, y0), Point::from_xy(0.0, y1), stops, SpreadMode::Pad, Transform::identity()).unwrap_or(Shader::SolidColor(bottom));
    Paint { shader, anti_alias: true, ..Paint::default() }
}

fn poly(points: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, (x, y)) in points.iter().enumerate() {
        if i == 0 {
            pb.move_to(*x, *y);
        } else {
            pb.line_to(*x, *y);
        }
    }
    pb.close();
    pb.finish()
}

/// Part of an ellipse's rim, from one angle round to another (clockwise on the page, as the canvas goes).
fn arc(cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32) -> Option<Path> {
    let sweep = (to - from).clamp(0.0, 2.0 * PI);
    let steps = ((sweep / (PI / 48.0)).ceil() as usize).max(2);
    let mut pb = PathBuilder::new();
    for i in 0..=steps {
        let a = from + sweep * i as f32 / steps as f32;
        let (x, y) = (cx + a.cos() * rx, cy + a.sin() * ry);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    if sweep >= 2.0 * PI - 1e-4 {
        pb.close();
    }
    pb.finish()
}

fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    PathBuilder::from_circle(cx, cy, r)
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    // how far a curve's handles reach to round a corner
    let k = r * 0.552_284_8;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn line(a: (f32, f32), b: (f32, f32)) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    pb.line_to(b.0, b.1);
    pb.finish()
}

// ---------- text ----------

/// The fonts of the Electron app's own covers, and one with far more letters for what they lack.
struct Fonts {
    title: [FontRef<'static>; 3],
    lexend: [FontRef<'static>; 5],
    wide: FontRef<'static>,
}

static FONTS: LazyLock<Fonts> = LazyLock::new(|| {
    let font = |bytes: &'static [u8]| FontRef::try_from_slice(bytes).expect("a font that ships with the app");
    Fonts {
        title: [font(include_bytes!("../assets/fonts/LilitaOne.ttf")), font(include_bytes!("../assets/fonts/Bungee.ttf")), font(include_bytes!("../assets/fonts/Audiowide.ttf"))],
        lexend: [
            font(include_bytes!("../assets/fonts/Lexend-Regular.ttf")),
            font(include_bytes!("../assets/fonts/Lexend-Medium.ttf")),
            font(include_bytes!("../assets/fonts/Lexend-Bold.ttf")),
            font(include_bytes!("../assets/fonts/Lexend-ExtraBold.ttf")),
            font(include_bytes!("../assets/fonts/Lexend-SemiBold.ttf")),
        ],
        wide: font(include_bytes!("../../img-tools/assets/DejaVuSans-Bold.ttf")),
    }
});

#[derive(Clone, Copy)]
enum Face {
    /// one of the three title fonts
    Title(usize),
    /// Lexend at a weight: 0 regular, 1 medium, 2 bold, 3 extra bold, 4 semibold
    Lexend(usize),
}

/// The font a letter is drawn in: the face's own when it has the letter.
fn font_for(face: Face, c: char) -> &'static FontRef<'static> {
    let own = match face {
        Face::Title(i) => &FONTS.title[i % 3],
        Face::Lexend(w) => &FONTS.lexend[w.min(4)],
    };
    if own.glyph_id(c).0 != 0 {
        own
    } else if FONTS.lexend[2].glyph_id(c).0 != 0 {
        &FONTS.lexend[2]
    } else {
        &FONTS.wide
    }
}

fn text_width(face: Face, text: &str, size: f32) -> f32 {
    text.chars().map(|c| { let f = font_for(face, c); f.h_advance_unscaled(f.glyph_id(c)) * size / f.units_per_em().unwrap_or(1000.0) }).sum()
}

/// A line of text as a shape, its left end on the baseline at (x, y).
fn text_path(face: Face, text: &str, size: f32, x: f32, y: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let mut pen = x;
    for c in text.chars() {
        let f = font_for(face, c);
        let (id, k) = (f.glyph_id(c), size / f.units_per_em().unwrap_or(1000.0));
        if let Some(outline) = f.outline(id) {
            let at = |p: ab_glyph::Point| (pen + p.x * k, y - p.y * k);
            let mut last: Option<(f32, f32)> = None;
            for curve in &outline.curves {
                let (from, to) = match curve {
                    OutlineCurve::Line(a, b) => (at(*a), at(*b)),
                    OutlineCurve::Quad(a, _, b) => (at(*a), at(*b)),
                    OutlineCurve::Cubic(a, _, _, b) => (at(*a), at(*b)),
                };
                // a stroke of the letter starts where the last one didn't end
                if last.is_none_or(|l| (l.0 - from.0).abs() > 1e-3 || (l.1 - from.1).abs() > 1e-3) {
                    if last.is_some() {
                        pb.close();
                    }
                    pb.move_to(from.0, from.1);
                }
                match curve {
                    OutlineCurve::Line(..) => pb.line_to(to.0, to.1),
                    OutlineCurve::Quad(_, c1, _) => pb.quad_to(at(*c1).0, at(*c1).1, to.0, to.1),
                    OutlineCurve::Cubic(_, c1, c2, _) => pb.cubic_to(at(*c1).0, at(*c1).1, at(*c2).0, at(*c2).1, to.0, to.1),
                }
                last = Some(to);
            }
            if last.is_some() {
                pb.close();
            }
        }
        pen += f.h_advance_unscaled(id) * k;
    }
    pb.finish()
}

/// Words onto lines no wider than `most`.
fn wrap(face: Face, text: &str, size: f32, most: f32) -> Vec<String> {
    let (mut lines, mut cur) = (Vec::new(), String::new());
    for word in text.split(' ') {
        let longer = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if text_width(face, &longer, size) > most && !cur.is_empty() {
            lines.push(std::mem::replace(&mut cur, word.to_string()));
        } else {
            cur = longer;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

// ---------- the canvas ----------

struct Canvas {
    pix: Pixmap,
    /// drawing only shows inside it
    clip: Option<Mask>,
    t: Transform,
}

impl Canvas {
    fn fill(&mut self, path: Option<Path>, paint: &Paint) {
        if let Some(p) = path {
            self.pix.fill_path(&p, paint, FillRule::Winding, self.t, self.clip.as_ref());
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, paint: &Paint) {
        self.fill(Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect), paint);
    }

    fn stroke(&mut self, path: Option<Path>, paint: &Paint, stroke: &Stroke) {
        if let Some(p) = path {
            self.pix.stroke_path(&p, paint, stroke, self.t, self.clip.as_ref());
        }
    }

    /// Text with its left, middle (0.5) or right (1) on x, its baseline on y.
    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, face: Face, text: &str, size: f32, x: f32, y: f32, align: f32, paint: &Paint) {
        let left = x - text_width(face, text, size) * align;
        self.fill(text_path(face, text, size, left, y), paint);
    }
}

fn width(w: f32) -> Stroke {
    Stroke { width: w, ..Stroke::default() }
}

/// One of the scenes a cover is made of, drawn into the box (x, y, w, h) around the hue `hue`.
#[allow(clippy::too_many_arguments)]
fn scene(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, hue: f32, r: &mut Chance, kind: &str) {
    let kept = c.clip.take();
    let mut clip = Mask::new(c.pix.width(), c.pix.height());
    if let (Some(m), Some(rect)) = (clip.as_mut(), Rect::from_xywh(x, y, w, h)) {
        m.fill_path(&PathBuilder::from_rect(rect), FillRule::Winding, true, Transform::identity());
    }
    c.clip = clip;
    let dark = kind == "horror" || (kind == "rpg" && r.roll() < 0.5);
    let sky = if dark { fade(y, y + h, hsl(hue + 200.0, 45.0, 14.0, 1.0), hsl(hue + 230.0, 50.0, 6.0, 1.0)) } else { fade(y, y + h, hsl(hue, 75.0, 62.0, 1.0), hsl(hue + 40.0, 70.0, 32.0, 1.0)) };
    c.rect(x, y, w, h, &sky);
    let cx = x + w / 2.0;
    match kind {
        "platform" | "party" => {
            c.fill(circle(x + w * (0.2 + r.roll() * 0.6), y + h * 0.3, w * 0.16), &solid(hsl(hue + 60.0, 95.0, 80.0, 0.9)));
            for k in 0..4 {
                let kf = k as f32;
                let base = y + h * (0.58 + kf * 0.1);
                let mut pb = PathBuilder::new();
                pb.move_to(x, base);
                for i in 0..=6 {
                    let f = i as f32 / 6.0;
                    pb.quad_to(x + w * (f - 0.08), base - h * (0.06 + r.roll() * 0.1), x + w * f, base);
                }
                pb.line_to(x + w, y + h);
                pb.line_to(x, y + h);
                pb.close();
                c.fill(pb.finish(), &solid(hsl(hue + 90.0 + kf * 18.0, 55.0, 48.0 - kf * 8.0, 1.0)));
            }
            for _ in 0..4 {
                let px = x + w * r.roll() * 0.8;
                let py = y + h * (0.25 + r.roll() * 0.35);
                c.fill(round_rect(px, py, w * 0.18, h * 0.03, 6.0), &solid(hsl(hue + 20.0, 40.0, 92.0, 1.0)));
                c.fill(circle(px + w * 0.09, py - h * 0.035, w * 0.022), &solid(hsl(50.0, 95.0, 60.0, 1.0)));
            }
            if kind == "party" {
                for _ in 0..60 {
                    let paint = solid(hsl(r.roll() * 360.0, 85.0, 62.0, 1.0));
                    let (tx, ty) = (x + r.roll() * w, y + r.roll() * h * 0.7);
                    let was = c.t;
                    c.t = c.t.pre_translate(tx, ty).pre_rotate(r.roll() * 6.0 * 180.0 / PI);
                    c.rect(-4.0, -2.0, 8.0, 4.0, &paint);
                    c.t = was;
                }
            }
        }
        "race" => {
            let hz = y + h * 0.48;
            c.rect(x, hz, w, h, &solid(hsl(hue + 180.0, 40.0, 18.0, 1.0)));
            c.fill(poly(&[(cx - w * 0.03, hz), (cx + w * 0.03, hz), (x + w * 1.1, y + h), (x - w * 0.1, y + h)]), &solid(hsl(0.0, 0.0, 12.0, 1.0)));
            let dashed = Stroke { width: w * 0.012, dash: StrokeDash::new(vec![h * 0.04, h * 0.04], 0.0), ..Stroke::default() };
            c.stroke(line((cx, hz), (cx, y + h)), &solid(hsl(50.0, 95.0, 65.0, 1.0)), &dashed);
            for _ in 0..26 {
                let paint = solid(hsl(hue + r.roll() * 60.0, 90.0, 75.0, 0.55));
                let lw = 1.0 + r.roll() * 3.0;
                let sy = y + r.roll() * h * 0.45;
                let sx = x + r.roll() * w;
                let reach = if sx < cx { -1.0 } else { 1.0 } * w * (0.15 + r.roll() * 0.3);
                c.stroke(line((sx, sy), (sx + reach, sy + h * 0.02)), &paint, &width(lw));
            }
            c.fill(round_rect(cx - w * 0.14, y + h * 0.72, w * 0.28, h * 0.1, 10.0), &solid(hsl(hue + 170.0, 85.0, 55.0, 1.0)));
            let tyre = solid(hsl(0.0, 0.0, 8.0, 1.0));
            c.rect(cx - w * 0.11, y + h * 0.8, w * 0.06, h * 0.03, &tyre);
            c.rect(cx + w * 0.05, y + h * 0.8, w * 0.06, h * 0.03, &tyre);
        }
        "rpg" | "flight" => {
            for _ in 0..70 {
                let paint = solid(rgba(255, 255, 255, r.roll() * 0.8));
                let (sx, sy) = (x + r.roll() * w, y + r.roll() * h * 0.55);
                c.rect(sx, sy, 1.5, 1.5, &paint);
            }
            c.fill(circle(x + w * (0.25 + r.roll() * 0.5), y + h * 0.24, w * 0.12), &solid(hsl(hue + 40.0, 60.0, 88.0, 0.95)));
            for k in 0..3 {
                let kf = k as f32;
                let base = y + h * (0.62 + kf * 0.12);
                let mut pb = PathBuilder::new();
                pb.move_to(x, y + h);
                pb.line_to(x, base);
                let mut px = x;
                while px < x + w {
                    let pw = w * (0.12 + r.roll() * 0.18);
                    pb.line_to(px + pw / 2.0, base - h * (0.08 + r.roll() * 0.18));
                    pb.line_to(px + pw, base);
                    px += pw;
                }
                pb.line_to(x + w, y + h);
                pb.close();
                c.fill(pb.finish(), &solid(hsl(hue + 200.0 + kf * 10.0, 35.0, 30.0 - kf * 9.0, 1.0)));
            }
            if kind == "flight" {
                let paint = solid(hsl(hue + 180.0, 80.0, 70.0, 1.0));
                let was = c.t;
                c.t = c.t.pre_translate(cx, y + h * 0.45).pre_rotate(-0.25 * 180.0 / PI);
                c.fill(poly(&[(w * 0.18, 0.0), (-w * 0.12, -h * 0.03), (-w * 0.04, 0.0), (-w * 0.12, h * 0.03)]), &paint);
                c.rect(-w * 0.06, -h * 0.07, w * 0.05, h * 0.14, &paint);
                c.t = was;
            } else {
                let stone = solid(hsl(hue + 220.0, 30.0, 10.0, 1.0));
                let (bx, by) = (x + w * (0.55 + r.roll() * 0.2), y + h * 0.62);
                c.rect(bx, by - h * 0.12, w * 0.16, h * 0.14, &stone);
                c.rect(bx - w * 0.03, by - h * 0.2, w * 0.05, h * 0.2, &stone);
                c.rect(bx + w * 0.14, by - h * 0.17, w * 0.05, h * 0.17, &stone);
                c.fill(poly(&[(bx - w * 0.04, by - h * 0.2), (bx - w * 0.005, by - h * 0.26), (bx + w * 0.03, by - h * 0.2)]), &stone);
                c.rect(bx + w * 0.06, by - h * 0.08, w * 0.02, h * 0.025, &solid(hsl(45.0, 95.0, 65.0, 1.0)));
            }
        }
        "action" => {
            let was = c.t;
            c.t = c.t.pre_translate(cx, y + h * 0.55);
            for i in 0..18 {
                c.t = c.t.pre_rotate(360.0 / 18.0);
                let paint = if i % 2 == 1 { solid(hsl(hue, 90.0, 70.0, 0.18)) } else { solid(hsl(hue + 30.0, 90.0, 50.0, 0.1)) };
                c.fill(poly(&[(0.0, 0.0), (w, -w * 0.12), (w, w * 0.12)]), &paint);
            }
            c.t = was;
            let hexagon = |rad: f32| -> Vec<(f32, f32)> { (0..6).map(|i| { let a = i as f32 / 6.0 * 2.0 * PI - PI / 2.0; (cx + a.cos() * rad, y + h * 0.58 + a.sin() * rad) }).collect() };
            c.fill(poly(&hexagon(w * 0.22)), &solid(hsl(hue + 180.0, 85.0, 60.0, 1.0)));
            c.fill(poly(&hexagon(w * 0.15)), &solid(hsl(hue + 180.0, 60.0, 20.0, 1.0)));
            for i in 0..3 {
                let f = i as f32;
                c.stroke(line((x + w * (0.1 + f * 0.08), y + h * (0.9 - f * 0.05)), (x + w * (0.9 - f * 0.05), y + h * (0.35 + f * 0.04))), &solid(rgba(255, 255, 255, 0.85)), &width(w * 0.02));
            }
        }
        "puzzle" => {
            let n = 6;
            let s = w / n as f32;
            for i in 0..n {
                for j in 0..(h / s).ceil() as usize {
                    if r.roll() < 0.55 {
                        let shift = [0.0, 60.0, 150.0, 210.0][((r.roll() * 4.0) as usize).min(3)];
                        let (bx, by) = (x + i as f32 * s, y + j as f32 * s);
                        c.fill(round_rect(bx + 3.0, by + 3.0, s - 6.0, s - 6.0, 8.0), &solid(hsl(hue + shift, 80.0, 58.0, 0.95)));
                        c.fill(round_rect(bx + 7.0, by + 7.0, s - 14.0, (s - 14.0) * 0.3, 4.0), &solid(rgba(255, 255, 255, 0.3)));
                    }
                }
            }
        }
        "sports" => {
            c.rect(x, y + h * 0.55, w, h, &solid(hsl(130.0, 50.0, 38.0, 1.0)));
            let chalk = solid(rgba(255, 255, 255, 0.7));
            c.stroke(arc(cx, y + h * 0.78, w * 0.4, h * 0.1, 0.0, 2.0 * PI), &chalk, &width(3.0));
            c.stroke(line((x, y + h * 0.78), (x + w, y + h * 0.78)), &chalk, &width(3.0));
            for k in 0..3 {
                let kf = k as f32;
                c.stroke(arc(cx, y + h * 0.55, w * (0.7 + kf * 0.15), h * (0.25 + kf * 0.06), PI, 2.0 * PI), &solid(hsl(hue, 30.0, 90.0, 0.25)), &width(h * 0.03));
            }
            c.fill(circle(cx + w * 0.1, y + h * 0.45, w * 0.09), &solid(Color::WHITE));
            c.stroke(arc(cx + w * 0.1, y + h * 0.45, w * 0.09, w * 0.09, 0.4, 2.4), &solid(hsl(hue, 70.0, 40.0, 1.0)), &width(3.0));
        }
        "media" => {
            c.rect(x, y, w, h, &solid(hsl(hue + 200.0, 40.0, 8.0, 0.55)));
            for k in (1..=6).rev() {
                let kf = k as f32;
                c.stroke(circle(cx, y + h * 0.62, w * 0.08 * kf), &solid(hsl(hue + kf * 8.0, 80.0, 65.0, 0.08 + kf * 0.03)), &width(2.0));
            }
            c.fill(circle(cx, y + h * 0.62, w * 0.17), &solid(hsl(hue, 85.0, 62.0, 1.0)));
            c.fill(poly(&[(cx - w * 0.05, y + h * 0.62 - w * 0.075), (cx + w * 0.08, y + h * 0.62), (cx - w * 0.05, y + h * 0.62 + w * 0.075)]), &solid(Color::WHITE));
            for i in 0..5 {
                let f = i as f32;
                c.fill(round_rect(x + w * (0.08 + f * 0.18), y + h * 0.88, w * 0.14, h * 0.07, 4.0), &solid(hsl(hue + f * 20.0, 60.0, 70.0, 0.18)));
            }
        }
        "horror" => {
            c.fill(circle(x + w * 0.72, y + h * 0.22, w * 0.1), &solid(hsl(60.0, 40.0, 85.0, 0.9)));
            let night = solid(rgba(0x05, 0x05, 0x0a, 1.0));
            let (mx, my) = (x + w * 0.18, y + h * 0.48);
            c.rect(mx, my, w * 0.64, h * 0.4, &night);
            c.fill(poly(&[(mx - w * 0.04, my), (cx, my - h * 0.14), (mx + w * 0.68, my)]), &night);
            c.rect(mx + w * 0.46, my - h * 0.16, w * 0.07, h * 0.16, &night);
            for i in 0..6 {
                let lit = if r.roll() < 0.4 { hsl(40.0, 95.0, 60.0, 1.0) } else { rgba(0x14, 0x14, 0x1c, 1.0) };
                c.rect(mx + w * (0.06 + (i % 3) as f32 * 0.2), my + h * (0.06 + (i / 3) as f32 * 0.14), w * 0.08, h * 0.07, &solid(lit));
            }
            for _ in 0..5 {
                let (ex, ey) = (x + r.roll() * w, y + h * (0.8 + r.roll() * 0.2));
                c.fill(arc(ex, ey, w * 0.6, h * 0.05, 0.0, 2.0 * PI), &solid(rgba(200, 210, 255, 0.06)));
            }
        }
        _ => {}
    }
    c.clip = kept;
}

/// The console's band across the top, its name on the left and `right` on the right; how tall it is.
fn band(c: &mut Canvas, height: f32, band: Option<&Band>, right: &str) -> f32 {
    let Some(b) = band else { return 0.0 };
    let tall = (height * 0.085).round();
    c.rect(0.0, 0.0, W, tall, &solid(of(b.bg)));
    // text set on the middle of its letters' height
    let middle = |face: Face, size: f32| {
        let f = font_for(face, 'H');
        tall / 2.0 + (f.ascent_unscaled() + f.descent_unscaled()) / 2.0 * size / f.units_per_em().unwrap_or(1000.0)
    };
    let ink = solid(of(b.fg));
    c.text(Face::Lexend(2), &b.label, tall * 0.42, W * 0.05, middle(Face::Lexend(2), tall * 0.42), 0.0, &ink);
    if !right.is_empty() {
        c.text(Face::Lexend(1), right, tall * 0.3, W * 0.95, middle(Face::Lexend(1), tall * 0.3), 1.0, &ink);
    }
    tall
}

fn canvas(look: &Look) -> (Canvas, f32) {
    let height = (W / look.ratio).round();
    (Canvas { pix: Pixmap::new(W as u32, height as u32).expect("a cover's size"), clip: None, t: Transform::identity() }, height)
}

fn picture(c: Canvas) -> Picture {
    let (width, height) = (c.pix.width(), c.pix.height());
    let rgba = c.pix.pixels().iter().flat_map(|p| { let d = p.demultiply(); [d.red(), d.green(), d.blue(), d.alpha()] }).collect();
    Picture { width, height, rgba }
}

/// A front cover for a game without one. `scene_kind`: the scene to draw ("media" for an app), None for one picked
/// by the title; `hue`: the colour it's drawn around, None for the title's own.
pub fn front(look: &Look, title: &str, region: &str, scene_kind: Option<&str>, hue: Option<f32>) -> Picture {
    let seed = hash(title);
    let hue = hue.unwrap_or((seed % 360) as f32);
    let (mut c, height) = canvas(look);
    let mut r = Chance::new(seed);
    let top = band(&mut c, height, look.band.as_ref(), &region.to_uppercase());
    scene(&mut c, 0.0, top, W, height - top, hue, &mut r, scene_kind.unwrap_or(KINDS[(seed % 9) as usize]));
    // the title: as large as fits on four lines at most
    let face = Face::Title((seed % 3) as usize);
    let text = title.to_uppercase();
    let mut size = W * 0.13;
    let mut lines;
    loop {
        lines = wrap(face, &text, size, W * 0.84);
        let fits = lines.len() <= 4 && lines.iter().all(|l| text_width(face, l, size) <= W * 0.9);
        if fits || size * 0.92 <= 12.0 {
            break;
        }
        size *= 0.92;
    }
    let (step, first) = (size * 1.04, top + height * 0.08);
    for (i, l) in lines.iter().enumerate() {
        let y = first + i as f32 * step + size * 0.8;
        let left = W / 2.0 - text_width(face, l, size) / 2.0;
        let outline = Stroke { width: size * 0.16, line_join: LineJoin::Round, line_cap: LineCap::Butt, ..Stroke::default() };
        c.stroke(text_path(face, l, size, left, y), &solid(hsl(hue + 200.0, 60.0, 12.0, 1.0)), &outline);
        c.fill(text_path(face, l, size, left, y), &fade(y - size, y, Color::WHITE, hsl(hue + 50.0, 95.0, 78.0, 1.0)));
    }
    picture(c)
}

/// A back to match: the title, four small scenes, a few words about the game (`blurb`) and what's known of it
/// (`facts`: a name and a value a line).
pub fn back(look: &Look, title: &str, facts: &[(String, String)], blurb: &str) -> Picture {
    let seed = hash(title);
    let hue = (seed % 360) as f32;
    let (mut c, height) = canvas(look);
    let mut r = Chance::new(seed ^ 0x9e37_79b9);
    c.rect(0.0, 0.0, W, height, &fade(0.0, height, hsl(hue + 200.0, 40.0, 16.0, 1.0), hsl(hue + 230.0, 45.0, 7.0, 1.0)));
    let top_band = band(&mut c, height, look.band.as_ref(), "");
    let white = solid(Color::WHITE);
    let face = Face::Title((seed % 3) as usize);
    for (i, l) in wrap(face, &title.to_uppercase(), 22.0, W * 0.88).iter().take(2).enumerate() {
        c.text(face, l, 22.0, W * 0.06, top_band + 40.0 + i as f32 * 26.0, 0.0, &white);
    }
    let shot_w = W * 0.42;
    let (shot_h, top) = (shot_w * 0.6, top_band + 90.0);
    for (i, (sx, sy)) in [(W * 0.06, top), (W * 0.52, top), (W * 0.06, top + shot_h + 10.0), (W * 0.52, top + shot_h + 10.0)].into_iter().enumerate() {
        scene(&mut c, sx, sy, shot_w, shot_h, hue + i as f32 * 35.0, &mut r, KINDS[((seed >> i) % 9) as usize]);
        c.stroke(Rect::from_xywh(sx, sy, shot_w, shot_h).map(PathBuilder::from_rect), &solid(rgba(255, 255, 255, 0.7)), &width(2.0));
    }
    let mut y = top + shot_h * 2.0 + 44.0;
    let plain = Face::Lexend(0);
    if !blurb.is_empty() {
        for l in wrap(plain, blurb, 13.0, W * 0.88).iter().take(6) {
            c.text(plain, l, 13.0, W * 0.06, y, 0.0, &solid(rgba(255, 255, 255, 0.9)));
            y += 17.0;
        }
        y += 10.0;
    }
    for (name, value) in facts {
        if y > height - 40.0 {
            continue;
        }
        c.text(plain, name, 13.0, W * 0.06, y, 0.0, &solid(rgba(255, 255, 255, 0.55)));
        if let Some(first) = wrap(plain, value, 13.0, W * 0.6).first() {
            c.text(plain, first, 13.0, W * 0.34, y, 0.0, &white);
        }
        y += 22.0;
    }
    picture(c)
}

/// A case's spine as the shelf shows it when there's no picture of it: the printed sheet inside the case's border,
/// the console's band at the top and the title running down it. `width` × `height` pixels, `scale` of them to a
/// pixel of the shelf's own measure; `edge`: the border at the top and bottom, in that measure.
pub fn spine(look: &Look, title: &str, label: &str, width: u32, height: u32, edge: f32, scale: f32) -> Picture {
    let mut c = Canvas { pix: Pixmap::new(width.max(1), height.max(1)).expect("a spine's size"), clip: None, t: Transform::from_scale(scale, scale) };
    let (w, h) = (width as f32 / scale, height as f32 / scale);
    let opaque = |c: crate::looks::Rgba| if c[3] < 1.0 { look.paper() } else { c };
    c.rect(0.0, 0.0, w, h, &solid(of(opaque(look.case_color))));
    let sheet = h - 2.0 * edge;
    c.rect(0.0, edge, w, sheet, &solid(of(look.paper())));
    let mut top = edge;
    if let Some(b) = &look.spine_band {
        let tall = sheet * 0.09;
        c.rect(0.0, top, w, tall, &solid(of(b.bg)));
        let (face, size) = (Face::Lexend(2), w * 0.3);
        // letters .02em apart, the line on the middle of the band
        let f = font_for(face, 'H');
        let k = size / f.units_per_em().unwrap_or(1000.0);
        let wide = text_width(face, label, size) + size * 0.02 * label.chars().count() as f32;
        let mut pen = (w - wide) / 2.0;
        let base = top + tall / 2.0 + (f.ascent_unscaled() + f.descent_unscaled()) / 2.0 * k * 0.72;
        for ch in label.chars() {
            let one = ch.to_string();
            c.fill(text_path(face, &one, size, pen, base), &solid(of(b.fg)));
            pen += text_width(face, &one, size) + size * 0.02;
        }
        top += tall;
    }
    let (face, size) = (Face::Lexend(4), w * 0.46);
    let room = (sheet - (top - edge)).min(sheet * 0.88) - 16.0;
    let mut shown = title.to_string();
    if text_width(face, &shown, size) > room {
        while !shown.is_empty() && text_width(face, &format!("{shown}…"), size) > room {
            shown.pop();
        }
        shown = format!("{}…", shown.trim_end());
    }
    let f = font_for(face, 'H');
    let middle = (f.ascent_unscaled() + f.descent_unscaled()) / 2.0 * size / f.units_per_em().unwrap_or(1000.0);
    // turned a quarter: it reads from the top down, the letters' tops to the right
    if let Some(p) = text_path(face, &shown, size, 0.0, 0.0).and_then(|p| p.transform(Transform::from_row(0.0, 1.0, -1.0, 0.0, w / 2.0 - middle, top + 8.0))) {
        c.fill(Some(p), &solid(of(look.spine_ink)));
    }
    let mut out = picture(c);
    // the curve of the spine, as the Electron app shades it
    let shade = crate::cssbg::paint("linear-gradient(90deg,rgba(0,0,0,.28),rgba(0,0,0,.05) 18%,rgba(255,255,255,.12) 50%,rgba(0,0,0,.05) 82%,rgba(0,0,0,.3))", width.max(1), height.max(1), scale);
    for (px, over) in out.rgba.as_chunks_mut::<4>().0.iter_mut().zip(shade.rgba.as_chunks::<4>().0) {
        let a = f32::from(over[3]) / 255.0;
        for i in 0..3 {
            px[i] = (f32::from(over[i]) * a + f32::from(px[i]) * (1.0 - a)).round() as u8;
        }
        px[3] = 255;
    }
    out
}

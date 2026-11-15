// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/covers.js, as RustyShelf has it.
//! Box art drawn for a game that has none yet: a scene made up from its title (the same title always draws the same
//! cover), the title across it, the console's band on top; and a back to match, with what's known of the game.

use std::f32::consts::PI;
use std::sync::LazyLock;

use ab_glyph::{Font, FontRef, OutlineCurve, VariableFont};
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
    /// the consoles' own lettering: Chakra Petch, Syne, Unbounded, Michroma, IBM Plex Mono, DM Serif Display
    word: [FontRef<'static>; 6],
    /// Mona Sans as a console's name is lettered without a font of its own (.sys-word): extra bold, wide; Latin
    /// and Latin Extended
    mona: [FontRef<'static>; 2],
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
        word: [
            font(include_bytes!("../assets/fonts/ChakraPetch-Bold.ttf")),
            font(include_bytes!("../assets/fonts/Syne-ExtraBold.ttf")),
            font(include_bytes!("../assets/fonts/Unbounded-Bold.ttf")),
            font(include_bytes!("../assets/fonts/Michroma.ttf")),
            font(include_bytes!("../assets/fonts/IBMPlexMono-SemiBold.ttf")),
            font(include_bytes!("../assets/fonts/DMSerifDisplay-Italic.ttf")),
        ],
        wide: font(include_bytes!("../../img-tools/assets/DejaVuSans-Bold.ttf")),
        mona: mona_sans().map(|bytes| {
            let mut f = font(bytes);
            f.set_variation(b"wght", 800.0);
            f.set_variation(b"wdth", 125.0);
            f
        }),
    }
});

#[derive(Clone, Copy)]
enum Face {
    /// one of the three title fonts
    Title(usize),
    /// Lexend at a weight: 0 regular, 1 medium, 2 bold, 3 extra bold, 4 semibold
    Lexend(usize),
    /// one of the consoles' lettering fonts
    Word(usize),
    /// Mona Sans, extra bold and wide
    Mona,
}

/// The font a letter is drawn in: the face's own when it has the letter.
fn font_for(face: Face, c: char) -> &'static FontRef<'static> {
    let own = match face {
        Face::Title(i) => &FONTS.title[i % 3],
        Face::Lexend(w) => &FONTS.lexend[w.min(4)],
        Face::Word(i) => &FONTS.word[i.min(5)],
        Face::Mona => &FONTS.mona[0],
    };
    if own.glyph_id(c).0 != 0 {
        own
    } else if matches!(face, Face::Mona) && FONTS.mona[1].glyph_id(c).0 != 0 {
        &FONTS.mona[1]
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

/// How far under the middle of a line its baseline is, for text the canvas sets by its middle: the font's rise
/// and fall, taken as one letter-size together (as Chromium takes them).
fn below_middle(face: Face, size: f32) -> f32 {
    let f = font_for(face, 'H');
    let (up, down) = (f.ascent_unscaled(), -f.descent_unscaled());
    (up - down) / (up + down) / 2.0 * size
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
    let middle = |face: Face, size: f32| tall / 2.0 + below_middle(face, size);
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
    front_from(look, title, region, scene_kind, hue, hash(title))
}

/// A front cover drawn from the number `seed` instead of its title's own (a shelf's "Open <emulator>" case takes
/// the shelf's into it).
pub fn front_from(look: &Look, title: &str, region: &str, scene_kind: Option<&str>, hue: Option<f32>, seed: u32) -> Picture {
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

/// The two lines at the foot of a tile's cover: its name, and what it does in small capitals.
fn tile_words(c: &mut Canvas, height: f32, name: &str, does: &str) {
    c.text(Face::Lexend(2), name, W * 0.1, W / 2.0, height * 0.84, 0.5, &solid(rgba(255, 255, 255, 0.96)));
    c.text(Face::Lexend(1), does, W * 0.04, W / 2.0, height * 0.84 + W * 0.07, 0.5, &solid(rgba(255, 255, 255, 0.45)));
}

/// The cover of the "Surprise me" case: a card of foil with a big question mark, under the console's band.
pub fn surprise(look: &Look) -> Picture {
    let (mut c, height) = canvas(look);
    c.rect(0.0, 0.0, W, height, &fade(0.0, height, rgba(0x14, 0x12, 0x1f, 1.0), rgba(0x07, 0x06, 0x0c, 1.0)));
    let top = band(&mut c, height, look.band.as_ref(), "");
    let size = W * 0.58;
    let (x, y) = ((W - size) / 2.0, top + (height - top) * 0.42 - size / 2.0);
    let card = || round_rect(x, y, size, size, size * 0.12);
    // a soft white light round the card
    let (pw, ph) = (c.pix.width() as usize, c.pix.height() as usize);
    let mut shape = Pixmap::new(pw as u32, ph as u32).expect("a cover's size");
    if let Some(p) = card() {
        shape.fill_path(&p, &solid(Color::WHITE), FillRule::Winding, Transform::identity(), None);
    }
    let light = blurred(&shape.pixels().iter().map(|p| f32::from(p.alpha()) / 255.0).collect::<Vec<_>>(), pw, ph, 20.0);
    for (px, a) in c.pix.pixels_mut().iter_mut().zip(light) {
        let a = a * 0.35;
        let mix = |v: u8| (255.0 * a + f32::from(v) * (1.0 - a)).round() as u8;
        *px = tiny_skia::PremultipliedColorU8::from_rgba(mix(px.red()), mix(px.green()), mix(px.blue()), 255).unwrap_or(*px);
    }
    // the foil turns through its colours round the card's middle (the canvas starts it .6 of a radian before 3 o'clock)
    let side = size.ceil() as u32;
    let foil = crate::cssbg::paint("conic-gradient(from 55.62deg at 50% 50%,#ff4fa3,#ffd54a,#5affc8,#5a8cff,#e65aff,#ff4fa3)", side, side, side as f32 / size);
    if let Some(foil) = tiny_skia::IntSize::from_wh(side, side).and_then(|s| Pixmap::from_vec(foil.rgba, s)) {
        let shader = tiny_skia::Pattern::new(foil.as_ref(), SpreadMode::Pad, tiny_skia::FilterQuality::Bilinear, 1.0, Transform::from_translate(x, y));
        c.fill(card(), &Paint { shader, anti_alias: true, ..Paint::default() });
    }
    let stops = vec![
        GradientStop::new(0.0, rgba(255, 255, 255, 0.55)),
        GradientStop::new(0.45, rgba(255, 255, 255, 0.0)),
        GradientStop::new(0.55, rgba(0, 0, 0, 0.0)),
        GradientStop::new(1.0, rgba(0, 0, 0, 0.35)),
    ];
    if let Some(shader) = LinearGradient::new(Point::from_xy(x, y), Point::from_xy(x + size, y + size), stops, SpreadMode::Pad, Transform::identity()) {
        c.fill(card(), &Paint { shader, anti_alias: true, ..Paint::default() });
    }
    let (face, tall) = (Face::Lexend(3), size * 0.72);
    c.text(face, "?", tall, x + size / 2.0, y + size * 0.56 + below_middle(face, tall), 0.5, &solid(rgba(10, 8, 16, 0.9)));
    tile_words(&mut c, height, "Surprise me", "SHUFFLE");
    picture(c)
}

/// The cover of the "Add games" case on a shelf without games: a dashed slot with a plus.
pub fn add_games(look: &Look) -> Picture {
    let (mut c, height) = canvas(look);
    c.rect(0.0, 0.0, W, height, &fade(0.0, height, rgba(0x1a, 0x1d, 0x24, 1.0), rgba(0x0b, 0x0c, 0x10, 1.0)));
    let top = band(&mut c, height, look.band.as_ref(), "");
    let (size, cx, cy) = (W * 0.5, W / 2.0, top + (height - top) * 0.42);
    let dashed = Stroke { width: W * 0.012, dash: StrokeDash::new(vec![W * 0.035, W * 0.025], 0.0), ..Stroke::default() };
    c.stroke(round_rect(cx - size / 2.0, cy - size / 2.0, size, size, size * 0.14), &solid(rgba(255, 255, 255, 0.4)), &dashed);
    let plus = Stroke { width: W * 0.03, line_cap: LineCap::Round, ..Stroke::default() };
    for (a, b) in [((cx - size * 0.2, cy), (cx + size * 0.2, cy)), ((cx, cy - size * 0.2), (cx, cy + size * 0.2))] {
        c.stroke(line(a, b), &solid(rgba(255, 255, 255, 0.88)), &plus);
    }
    tile_words(&mut c, height, "Add games", "OPEN SETTINGS");
    picture(c)
}

/// The mark in the corner of a favourite's cover: a heart on a dark disc, `size` pixels across.
pub fn heart(size: u32) -> Picture {
    let mut c = Canvas { pix: Pixmap::new(size.max(1), size.max(1)).expect("a mark's size"), clip: None, t: Transform::identity() };
    let s = size as f32;
    c.fill(circle(s / 2.0, s / 2.0, s / 2.0), &solid(rgba(10, 10, 16, 0.75)));
    let (face, tall) = (Face::Lexend(0), s * 14.0 / 26.0);
    let f = font_for(face, '♥');
    let rise = (f.ascent_unscaled() + f.descent_unscaled()) / 2.0 * tall / f.units_per_em().unwrap_or(1000.0);
    c.text(face, "♥", tall, s / 2.0, s / 2.0 + rise, 0.5, &solid(rgba(0xff, 0x5c, 0x8a, 1.0)));
    picture(c)
}

/// The mark in the corner of a RomM game's cover that isn't on this device yet: a cloud with an arrow down, on a dark
/// disc (style.css `.slot.cloud`).
pub fn cloud(size: u32) -> Picture {
    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><circle cx='50' cy='50' r='50' fill='rgb(10,10,16)' fill-opacity='.75'/>\
<g transform='translate(19 19) scale(2.583)' fill='none' stroke='#a9bfff' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'>\
<path d='M7 18h10a4 4 0 0 0 .5-7.97A6 6 0 0 0 6.1 9.2 4.5 4.5 0 0 0 7 18z'/><path d='M12 10v6m-2.5-2.5L12 16l2.5-2.5'/></g></svg>";
    let size = size.max(1);
    let mut pix = resvg::tiny_skia::Pixmap::new(size, size).expect("a mark's size");
    if let Ok(tree) = resvg::usvg::Tree::from_str(SVG, &resvg::usvg::Options::default()) {
        let k = size as f32 / 100.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    }
    let rgba = pix.pixels().iter().flat_map(|p| { let d = p.demultiply(); [d.red(), d.green(), d.blue(), d.alpha()] }).collect();
    Picture { width: size, height: size, rgba }
}

/// The Wii remote's pointer: a white glove, its fingertip at 18 × 5 of its 52 px (style.css `#wmHand`).
pub fn hand(size: u32) -> Picture {
    const SVG: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><path fill='#fff' stroke='#2b5fd9' stroke-width='3' stroke-linejoin='round' \
d='M22 6c3.3 0 6 2.7 6 6v17.5l2-.4V24c0-3 2.4-5.3 5.3-5.3S40.6 21 40.6 24v5.6l1.6-.2v-1.6c0-2.9 2.4-5.2 5.3-5.2s5.2 2.3 5.2 5.2v3.6l.8-.1c2.9-.4 5.5 1.9 5.5 4.8V44c0 8.8-7.2 16-16 16H34c-5.5 0-10.6-2.7-13.6-7.3L9.6 36.4c-1.6-2.4-1-5.6 1.4-7.2 2.2-1.5 5.1-1.1 6.8.9L16 30V12c0-3.3 2.7-6 6-6z'/></svg>";
    let size = size.max(1);
    let mut pix = resvg::tiny_skia::Pixmap::new(size, size).expect("a mark's size");
    if let Ok(tree) = resvg::usvg::Tree::from_str(SVG, &resvg::usvg::Options::default()) {
        let k = size as f32 / 64.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
    }
    let rgba = pix.pixels().iter().flat_map(|p| { let d = p.demultiply(); [d.red(), d.green(), d.blue(), d.alpha()] }).collect();
    Picture { width: size, height: size, rgba }
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

/// A picture's alpha blurred as a Gaussian `sigma` pixels wide would (three box blurs).
fn blurred(alpha: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let pass = |src: &[f32], len: usize, lines: usize, at: &dyn Fn(usize, usize) -> usize| {
        let mut out = vec![0.0; src.len()];
        for line in 0..lines {
            let mut sum = 0.0;
            for i in 0..len + r {
                if i < len {
                    sum += src[at(line, i)];
                }
                if i > 2 * r {
                    sum -= src[at(line, i - 2 * r - 1)];
                }
                if i >= r {
                    out[at(line, i - r)] = sum / (2 * r + 1) as f32;
                }
            }
        }
        out
    };
    let mut a = alpha.to_vec();
    for _ in 0..3 {
        a = pass(&a, w, h, &|line, i| line * w + i);
        a = pass(&a, h, w, &|line, i| i * w + line);
    }
    a
}

/// A shelf's name as it's lettered over the shelf, for a window `window_w` wide: in the shelf's font and gradient,
/// a glow of its colour under the letters. `scale` pixels to a pixel of the window; the letters sit `WORD_ROOM` in
/// from the picture's edges, for the glow.
pub fn wordmark(text: &str, lettering: &crate::looks::Lettering, window_w: f32, scale: f32) -> Picture {
    lettered(text, lettering, lettering.size_in(window_w), &lettering.grad, true, scale)
}

/// A shelf's name in its own letters `size` pixels tall, filled with the CSS background `fill`, with the glow
/// under them or without.
pub fn lettered(text: &str, lettering: &crate::looks::Lettering, size: f32, fill: &str, glow: bool, scale: f32) -> Picture {
    let face = match lettering.font.as_str() {
        "Chakra Petch" => Face::Word(0),
        "Syne" => Face::Word(1),
        "Unbounded" => Face::Word(2),
        "Michroma" => Face::Word(3),
        "IBM Plex Mono" => Face::Word(4),
        "DM Serif Display" => Face::Word(5),
        "Bungee" => Face::Title(1),
        "Audiowide" => Face::Title(2),
        "Mona Sans" => Face::Mona,
        _ => Face::Lexend(3),
    };
    let text = if lettering.upper { text.to_uppercase() } else { text.to_string() };
    let apart = lettering.spacing * size;
    let wide = text_width(face, &text, size) + apart * text.chars().count() as f32 + size * 0.1;
    let (w, h) = (((wide + 2.0 * WORD_ROOM) * scale).ceil() as u32, ((size + 2.0 * WORD_ROOM) * scale).ceil() as u32);
    let mut c = Canvas { pix: Pixmap::new(w.max(1), h.max(1)).expect("a wordmark's size"), clip: None, t: Transform::from_scale(scale, scale) };
    // a line as tall as its letters' size: the font's own height sits on its middle
    let f = font_for(face, 'H');
    let k = size / f.units_per_em().unwrap_or(1000.0);
    let base = WORD_ROOM + (size - (f.ascent_unscaled() - f.descent_unscaled()) * k) / 2.0 + f.ascent_unscaled() * k;
    let mut pen = WORD_ROOM;
    for ch in text.chars() {
        let one = ch.to_string();
        c.fill(text_path(face, &one, size, pen, base), &solid(Color::WHITE));
        pen += text_width(face, &one, size) + apart;
    }
    let (w, h) = (w as usize, h as usize);
    let letters: Vec<f32> = c.pix.pixels().iter().map(|p| f32::from(p.alpha()) / 255.0).collect();
    let glow = if glow { blurred(&letters, w, h, 8.0 * scale) } else { vec![0.0; w * h] };
    // the gradient runs over the letters' own box
    let fill = crate::cssbg::paint(fill, (wide * scale).round().max(1.0) as u32, (size * scale).round().max(1.0) as u32, scale);
    let (room, drop) = ((WORD_ROOM * scale).round() as usize, (2.0 * scale).round() as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let row = y.saturating_sub(room).min(fill.height as usize - 1);
        for x in 0..w {
            let at = (row * fill.width as usize + x.saturating_sub(room).min(fill.width as usize - 1)) * 4;
            let ink = &fill.rgba[at..at + 3];
            let a = letters[y * w + x];
            let under = if y >= drop { glow[(y - drop) * w + x] } else { 0.0 } * lettering.glow[3];
            let all = a + under * (1.0 - a);
            if all > 0.0 {
                for i in 0..3 {
                    rgba[(y * w + x) * 4 + i] = ((f32::from(ink[i]) * a + lettering.glow[i] * 255.0 * under * (1.0 - a)) / all).round() as u8;
                }
                rgba[(y * w + x) * 4 + 3] = (all * 255.0).round() as u8;
            }
        }
    }
    Picture { width: w as u32, height: h as u32, rgba }
}

/// The room a wordmark's picture keeps round its letters, in the letters' own pixels.
pub const WORD_ROOM: f32 = 24.0;

/// Mona Sans, the interface font: one variable file (weights 200-900, widths 75%-125%) for Latin, one for Latin
/// Extended.
pub fn mona_sans() -> [&'static [u8]; 2] {
    [include_bytes!("../assets/fonts/MonaSans-Latin.ttf"), include_bytes!("../assets/fonts/MonaSans-LatinExt.ttf")]
}

// ---------- a made back (ported from renderer/img-tools.js back) ----------

/// What a made back prints: the description (`overview`), and on its bottom bar the age rating, players, publisher
/// and year, each empty when unknown.
pub struct BackText<'a> {
    pub title: &'a str,
    pub overview: &'a str,
    pub esrb: &'a str,
    pub players: &'a str,
    pub publisher: &'a str,
    pub year: &'a str,
}

fn pixmap_of(img: &image::RgbaImage) -> Option<Pixmap> {
    let mut p = Pixmap::new(img.width(), img.height())?;
    for (d, s) in p.pixels_mut().iter_mut().zip(img.pixels()) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(p)
}

/// `img` scaled to fill the box at (x, y, w, h), its middle kept, painted inside `shape`.
fn cover_into(c: &mut Canvas, img: &Pixmap, shape: Option<Path>, x: f32, y: f32, w: f32, h: f32) {
    let s = (w / img.width() as f32).max(h / img.height() as f32);
    let (dx, dy) = (x + (w - img.width() as f32 * s) / 2.0, y + (h - img.height() as f32 * s) / 2.0);
    let shader = tiny_skia::Pattern::new(img.as_ref(), SpreadMode::Pad, tiny_skia::FilterQuality::Bilinear, 1.0, Transform::from_row(s, 0.0, 0.0, s, dx, dy));
    c.fill(shape, &Paint { shader, anti_alias: true, ..Paint::default() });
}

/// A back for a game that has none: its console's band, its artwork blurred behind, a big screenshot, the title and
/// description, two more screenshots, and a bar with its rating, players, publisher and year. `front`: its cover,
/// which sets the shape and stands in for missing artwork.
pub fn made_back(front: &image::RgbaImage, shots: &[image::RgbaImage], art: Option<&image::RgbaImage>, band: &Band, t: &BackText) -> Option<Picture> {
    let h = 1550.0_f32;
    let w = (h * (front.width() as f32 / front.height().max(1) as f32).clamp(0.55, 0.8)).round();
    let mut c = Canvas { pix: Pixmap::new(w as u32, h as u32)?, clip: None, t: Transform::identity() };
    let shots: Vec<Pixmap> = shots.iter().take(3).filter_map(pixmap_of).collect();
    let art = art.unwrap_or(front);
    let art_pix = pixmap_of(art)?;
    let m = (w * 0.06).round();
    let hw = w - 2.0 * m;
    c.rect(0.0, 0.0, w, h, &solid(rgba(0x0d, 0x0e, 0x12, 1.0)));
    // a blur: the artwork shrunk to a few dozen pixels and stretched back
    let small = image::imageops::resize(art, 24, (24.0 * art.height() as f32 / art.width().max(1) as f32).max(1.0) as u32, image::imageops::FilterType::Triangle);
    if let Some(sp) = pixmap_of(&small) {
        cover_into(&mut c, &sp, Rect::from_xywh(0.0, 0.0, w, h).map(PathBuilder::from_rect), -40.0, -40.0, w + 80.0, h + 80.0);
    }
    let stops = vec![GradientStop::new(0.0, rgba(8, 9, 13, 0.45)), GradientStop::new(0.45, rgba(8, 9, 13, 0.62)), GradientStop::new(1.0, rgba(8, 9, 13, 0.9))];
    if let Some(shader) = LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(0.0, h), stops, SpreadMode::Pad, Transform::identity()) {
        c.rect(0.0, 0.0, w, h, &Paint { shader, anti_alias: true, ..Paint::default() });
    }
    let bh = (h * 0.062).round();
    c.rect(0.0, 0.0, w, bh, &solid(of(band.bg)));
    if !band.label.is_empty() {
        let size = (bh * 0.42).round();
        c.text(Face::Lexend(3), &band.label.to_uppercase(), size, m, bh / 2.0 + 1.0 + below_middle(Face::Lexend(3), size), 0.0, &solid(of(band.fg)));
    }
    static BREAK: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"([^.!?:;,\s])[ \t]*(\r?\n)+\s*").unwrap());
    let text = BREAK.replace_all(t.overview, "$1. ").split_whitespace().collect::<Vec<_>>().join(" ");
    let mut y = bh + (h * 0.03).round();
    let hh = (hw * if text.is_empty() { 1.05 } else { 9.0 / 16.0 }).round();
    cover_into(&mut c, shots.first().unwrap_or(&art_pix), round_rect(m, y, hw, hh, 18.0), m, y, hw, hh);
    c.stroke(round_rect(m, y, hw, hh, 18.0), &solid(rgba(255, 255, 255, 0.25)), &width(3.0));
    y += hh + (h * 0.03).round();
    let wrap_text = |c: &mut Canvas, y: &mut f32, text: &str, face: Face, size: f32, most: usize, lh: f32, ink: &Paint| {
        let mut lines = wrap(face, text, size, hw);
        if lines.len() > most {
            lines.truncate(most);
            let mut l = lines[most - 1].clone();
            while !l.is_empty() && text_width(face, &format!("{l}…"), size) > hw {
                l = l.trim_end().rsplit_once(' ').map_or(String::new(), |(a, _)| a.to_string());
            }
            lines[most - 1] = format!("{l}…");
        }
        for l in &lines {
            *y += size * lh;
            c.text(face, l, size, m, *y, 0.0, ink);
        }
    };
    let title = t.title.split_whitespace().collect::<Vec<_>>().join(" ");
    wrap_text(&mut c, &mut y, &title, Face::Lexend(3), (h * 0.034).round(), 2, 1.18, &solid(Color::WHITE));
    y += (h * 0.012).round();
    let strip = shots.len() >= 3;
    let room = (h * if strip { 0.6 } else { 0.82 }).round() - y;
    let fs = (h * 0.0195).round();
    if !text.is_empty() {
        let most = ((room / (fs * 1.42)).floor() as usize).max(3);
        wrap_text(&mut c, &mut y, &text, Face::Lexend(0), fs, most, 1.42, &solid(rgba(255, 255, 255, 0.86)));
    }
    if strip {
        let (top, gap) = ((h * 0.64).round(), (m * 0.5).round());
        let sw = (hw - gap) / 2.0;
        let sh = (sw * 9.0 / 16.0).round();
        for (i, shot) in shots[1..3].iter().enumerate() {
            let x = m + i as f32 * (sw + gap);
            cover_into(&mut c, shot, round_rect(x, top, sw, sh, 12.0), x, top, sw, sh);
            c.stroke(round_rect(x, top, sw, sh, 12.0), &solid(rgba(255, 255, 255, 0.22)), &width(2.0));
        }
    }
    static RATING: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"^(EC|E10\+|E|T|M|AO|RP)(\s|$|-)").unwrap());
    let esrb = RATING.captures(t.esrb.trim()).map(|m| m[1].to_string());
    let players = t.players.trim().parse::<f64>().unwrap_or(0.0);
    let line1 = if players > 1.0 { format!("1–{players} players") } else if players == 1.0 { "1 player".to_string() } else { String::new() };
    let line2 = [t.publisher, t.year].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    if esrb.is_some() || !line1.is_empty() || !line2.is_empty() {
        let (fy, fh) = ((h * 0.885).round(), (h * 0.085).round());
        c.fill(round_rect(m, fy, hw, fh, 14.0), &solid(rgba(255, 255, 255, 0.08)));
        let mut fx = m + (fh * 0.18).round();
        if let Some(r) = &esrb {
            let (bw2, bh2) = ((fh * 0.62).round(), (fh * 0.8).round());
            let by = fy + (fh - bh2) / 2.0;
            c.rect(fx, by, bw2, bh2, &solid(Color::WHITE));
            c.rect(fx + 3.0, by + 3.0, bw2 - 6.0, bh2 - 6.0, &solid(Color::BLACK));
            c.rect(fx + 6.0, by + 6.0, bw2 - 12.0, bh2 * 0.66, &solid(Color::WHITE));
            let size = (bh2 * if r.len() > 2 { 0.3 } else { 0.5 }).round();
            c.text(Face::Lexend(3), r, size, fx + bw2 / 2.0, by + 6.0 + bh2 * 0.33 + below_middle(Face::Lexend(3), size), 0.5, &solid(Color::BLACK));
            let size = (bh2 * 0.17).round();
            c.text(Face::Lexend(3), "ESRB", size, fx + bw2 / 2.0, by + bh2 * 0.85 + below_middle(Face::Lexend(3), size), 0.5, &solid(Color::WHITE));
            fx += bw2 + (fh * 0.25).round();
        }
        if !line1.is_empty() {
            let size = (fh * 0.26).round();
            c.text(Face::Lexend(2), &line1, size, fx, fy + fh * if line2.is_empty() { 0.5 } else { 0.34 } + below_middle(Face::Lexend(2), size), 0.0, &solid(Color::WHITE));
        }
        if !line2.is_empty() {
            let size = (fh * 0.22).round();
            c.text(Face::Lexend(1), &line2, size, fx, fy + fh * if line1.is_empty() { 0.5 } else { 0.68 } + below_middle(Face::Lexend(1), size), 0.0, &solid(rgba(255, 255, 255, 0.7)));
        }
    }
    Some(picture(c))
}

#[cfg(test)]
mod made_back_tests {
    use super::*;

    #[test]
    fn a_made_back_fills_its_case() {
        let front = image::RgbaImage::from_pixel(600, 900, image::Rgba([200, 40, 40, 255]));
        let shot = image::RgbaImage::from_fn(320, 180, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 120, 255]));
        let band = Band { bg: [0.07, 0.08, 0.1, 1.0], fg: [0.56, 0.7, 0.85, 1.0], label: "PC".into() };
        let words = "Heading\nA long description of the game. ".repeat(20);
        let t = BackText { title: "A Game", overview: &words, esrb: "T - Teen", players: "2", publisher: "Someone", year: "2020" };
        let p = made_back(&front, &[shot.clone(), shot.clone(), shot], None, &band, &t).unwrap();
        assert_eq!((p.width, p.height), (1033, 1550));
        if let Ok(out) = std::env::var("MADE_BACK_OUT") {
            image::RgbaImage::from_raw(p.width, p.height, p.rgba).unwrap().save(out).unwrap();
        }
    }
}

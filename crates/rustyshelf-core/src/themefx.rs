// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/themefx.js and the moving parts of renderer/themes-extra.js, as RustyShelf has them.
//! What moves behind the shelf in a theme, drawn a frame at a time over the theme's backdrop: Spectrum's ribbons of
//! light and dust, Keys' rolling field of dots, Arcade's starfield and mountains, Aurora's curtains, Midnight's
//! line of light, and the extra themes' trails, sparks, fireflies, roll bar, snow and neon lanes.

use std::f32::consts::{PI, TAU};

use tiny_skia::{BlendMode, Color, FillRule, GradientStop, LineCap, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Rect, Shader, SpreadMode, Stroke, Transform};

use crate::drawn::Picture;

struct Star {
    x: f32,
    y: f32,
    z: f32,
    p: f32,
}

struct Dust {
    x: f32,
    y: f32,
    s: f32,
    v: f32,
    p: f32,
}

/// A note the menu played, spreading over Keys' dots.
struct Ripple {
    u: f32,
    t0: f32,
    v: f32,
    hue: f32,
}

struct Flare {
    x: f32,
    y: f32,
    t0: f32,
}

struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 >> 8) as f32 / 16_777_216.0
    }

    fn between(&mut self, a: f32, b: f32) -> f32 {
        a + self.next() * (b - a)
    }
}

/// The extra themes' generator: the same pattern every start (themes-extra.js seeded()).
struct Seeded(i32);

impl Seeded {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let s = self.0 as u32;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f32 / 4_294_967_296.0
    }
}

/// CSS's hsla(): a hue in degrees, saturation and lightness in percent.
fn hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    let (s, l, h) = (s / 100.0, l / 100.0, h.rem_euclid(360.0) / 30.0);
    let k = s * l.min(1.0 - l);
    let [r, g, b] = [0.0, 8.0, 4.0].map(|n| {
        let x = (n + h) % 12.0;
        (l - k * (x - 3.0).min(9.0 - x).clamp(-1.0, 1.0)).clamp(0.0, 1.0)
    });
    Color::from_rgba(r, g, b, a.clamp(0.0, 1.0)).unwrap_or(Color::TRANSPARENT)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba(f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, a.clamp(0.0, 1.0)).unwrap_or(Color::TRANSPARENT)
}

/// A console's hue drawn part of the way to the theme's own (themes-extra.js pull()).
fn pull(sh: f32, to: f32, keep: f32) -> f32 {
    to + ((sh - to).rem_euclid(360.0) + 540.0).rem_euclid(360.0) * keep - 180.0 * keep
}

/// The canvas a frame is drawn on, with the 2D canvas's own few moves.
struct Canvas {
    pix: Pixmap,
    /// light adds to what's there (the canvas's "lighter") instead of covering it
    lighter: bool,
}

impl Canvas {
    fn paint(&self, shader: Shader<'static>) -> Paint<'static> {
        Paint { shader, blend_mode: if self.lighter { BlendMode::Plus } else { BlendMode::SourceOver }, anti_alias: true, ..Paint::default() }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Shader<'static>) {
        if let Some(r) = Rect::from_xywh(x, y, w, h) {
            // (tiny-skia can't smooth the edges of a rectangle under a pixel wide)
            let paint = Paint { anti_alias: false, ..self.paint(fill) };
            self.pix.fill_rect(r, &paint, Transform::identity(), None);
        }
    }

    fn circle(&mut self, x: f32, y: f32, r: f32, c: Color) {
        if let Some(p) = PathBuilder::from_circle(x, y, r.max(0.05)) {
            let paint = self.paint(Shader::SolidColor(c));
            self.pix.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    fn line(&mut self, points: &[(f32, f32)], c: Color, width: f32, round: bool) {
        let mut pb = PathBuilder::new();
        for (i, (x, y)) in points.iter().enumerate() {
            if i == 0 {
                pb.move_to(*x, *y);
            } else {
                pb.line_to(*x, *y);
            }
        }
        if let Some(p) = pb.finish() {
            let paint = self.paint(Shader::SolidColor(c));
            let stroke = Stroke { width, line_cap: if round { LineCap::Round } else { LineCap::Butt }, ..Stroke::default() };
            self.pix.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }
    }

    /// A soft dot of light: `stops` from its middle to `r` out.
    fn glow(&mut self, x: f32, y: f32, r: f32, stops: Vec<GradientStop>) {
        if let Some(s) = RadialGradient::new(Point::from_xy(x, y), Point::from_xy(x, y), r, stops, SpreadMode::Pad, Transform::identity()) {
            self.rect(x - r, y - r, r * 2.0, r * 2.0, s);
        }
    }
}

fn down(y0: f32, y1: f32, stops: Vec<GradientStop>) -> Shader<'static> {
    LinearGradient::new(Point::from_xy(0.0, y0), Point::from_xy(0.0, y1), stops, SpreadMode::Pad, Transform::identity()).unwrap_or(Shader::SolidColor(Color::TRANSPARENT))
}

fn across(x0: f32, x1: f32, stops: Vec<GradientStop>) -> Shader<'static> {
    LinearGradient::new(Point::from_xy(x0, 0.0), Point::from_xy(x1, 0.0), stops, SpreadMode::Pad, Transform::identity()).unwrap_or(Shader::SolidColor(Color::TRANSPARENT))
}

fn stop(at: f32, c: Color) -> GradientStop {
    GradientStop::new(at, c)
}

/// The moving layer of a theme: its particles, its clock, and the canvas it's drawn on.
pub struct Layer {
    c: Canvas,
    /// Aurora's curtains, drawn small and stretched
    small: Pixmap,
    w: f32,
    h: f32,
    /// canvas pixels to a pixel of the window
    scale: f32,
    /// how far down the window the floor is
    floor: f32,
    t: f32,
    hue: f32,
    stars: Vec<Star>,
    dust: Vec<Dust>,
    peaks: [Vec<f32>; 2],
    ripples: Vec<Ripple>,
    flares: Vec<Flare>,
    rng: Rng,
}

/// Whether a theme has anything moving.
pub fn moves(theme: &str) -> bool {
    matches!(crate::themes::id_of(theme), "spectrum" | "keys" | "arcade" | "aurora" | "midnight" | "blueshift" | "swirl" | "forest" | "synth")
}

impl Layer {
    /// A layer `width` × `height` pixels, `scale` of them to a pixel of the window, the floor `floor` of the way
    /// down it.
    pub fn new(width: u32, height: u32, scale: f32, floor: f32, hue: f32) -> Layer {
        let (width, height) = (width.max(8), height.max(8));
        let mut rng = Rng(0x9E37_79B9);
        let stars = (0..220).map(|_| Star { x: rng.next(), y: rng.next(), z: rng.next(), p: rng.next() * TAU }).collect();
        let dust = (0..80).map(|_| Dust { x: rng.next(), y: rng.next(), s: rng.between(0.4, 1.6), v: rng.between(0.004, 0.016), p: rng.next() * TAU }).collect();
        // a stepped mountain range for Arcade, two layers of it
        let peaks = [0, 1].map(|_| {
            let mut y = 0.4f32;
            (0..96)
                .map(|i| {
                    y = (y + rng.between(-0.16, 0.16) + if i % 16 < 8 { 0.04 } else { -0.04 }).clamp(0.08, 1.0);
                    y
                })
                .collect()
        });
        Layer {
            c: Canvas { pix: Pixmap::new(width, height).expect("a layer's size"), lighter: false },
            small: Pixmap::new((width / 4).max(80), (height / 4).max(45)).expect("a layer's size"),
            w: width as f32,
            h: height as f32,
            scale,
            floor: floor.clamp(0.45, 0.8),
            t: 0.0,
            hue,
            stars,
            dust,
            peaks,
            ripples: Vec::new(),
            flares: Vec::new(),
            rng,
        }
    }

    /// A note the menu played: Keys' dots ripple from it, a star flares in Arcade. `f`: its pitch in hertz, `v` how
    /// loud (0 to 1), `hue` the song's colour.
    pub fn note(&mut self, f: f32, v: f32, hue: Option<f32>, song: f32) {
        let u = ((f / 110.0).log2() / 5.0).clamp(0.0, 1.0);
        self.ripples.push(Ripple { u, t0: self.t, v: (0.45 + v).min(1.0), hue: hue.map_or(song, |h| h + (u - 0.5) * 60.0) });
        if self.ripples.len() > 14 {
            self.ripples.remove(0);
        }
        let (x, y) = (self.rng.next(), self.rng.between(0.04, 0.5));
        self.flares.push(Flare { x, y, t0: self.t });
        if self.flares.len() > 10 {
            self.flares.remove(0);
        }
    }

    /// The next frame, `dt` seconds after the last: the theme's layer in the shelf's colour `hue` (it follows a
    /// change over a moment) and the song's `song`. Clear for a theme with nothing moving.
    pub fn frame(&mut self, theme: &str, dt: f32, hue: f32, song: f32) -> Picture {
        self.t += dt;
        self.hue += (hue - self.hue) * (dt * 1.5).min(1.0);
        self.c.pix.fill(Color::TRANSPARENT);
        self.c.lighter = false;
        match crate::themes::id_of(theme) {
            "spectrum" => self.spectrum(dt),
            "keys" => self.keys(song),
            "arcade" => self.arcade(dt),
            "aurora" => self.aurora(),
            "midnight" => self.midnight(dt),
            "blueshift" => self.blue_shift(),
            "swirl" => self.swirl(),
            "forest" => self.forest(),
            "synth" => self.synth(),
            _ => {}
        }
        Picture::of(&self.c.pix)
    }

    fn spectrum(&mut self, dt: f32) {
        let (w, h, t) = (self.w, self.h, self.t);
        self.c.lighter = true;
        for k in 0..3 {
            let (kf, yb, hh) = (k as f32, h * (0.2 + k as f32 * 0.14), self.hue + (k as f32 - 1.0) * 28.0);
            let ribbon: Vec<(f32, f32)> = (0..=(w / 8.0) as usize)
                .map(|i| {
                    let x = i as f32 * 8.0;
                    (x, yb + (x / w * 5.0 + t * (0.16 + kf * 0.05) + kf * 2.0).sin() * h * 0.06 + (x / w * 11.0 - t * 0.21 + kf).sin() * h * 0.02)
                })
                .collect();
            self.c.line(&ribbon, hsl(hh, 90.0, 62.0, 0.06), h * 0.09, false);
            self.c.line(&ribbon, hsl(hh, 95.0, 75.0, 0.12), h * 0.012, false);
            self.c.line(&ribbon, hsl(hh + 10.0, 100.0, 88.0, 0.18), (h * 0.0025).max(1.0), false);
        }
        for i in 0..self.dust.len() {
            let d = &mut self.dust[i];
            d.y -= d.v * dt;
            if d.y < -0.02 {
                (d.y, d.x) = (1.02, self.rng.next());
            }
            let a = 0.25 + 0.25 * (t * 1.3 + d.p).sin();
            let (x, y, r) = (d.x * w + (t * 0.4 + d.p).sin() * 6.0, d.y * h, d.s * self.scale * 2.2);
            self.c.circle(x, y, r, hsl(self.hue + 20.0, 90.0, 80.0, a * 0.7));
        }
    }

    fn keys(&mut self, song: f32) {
        const COLS: usize = 64;
        const ROWS: usize = 24;
        let (w, h, now) = (self.w, self.h, self.t);
        let hy = h * (self.floor - 0.1);
        // the horizon glows in the song's colour
        let glow = down(hy - h * 0.25, hy + h * 0.1, vec![stop(0.0, hsl(song, 90.0, 50.0, 0.0)), stop(0.75, hsl(song, 90.0, 55.0, 0.28)), stop(1.0, hsl(song, 90.0, 55.0, 0.0))]);
        self.c.rect(0.0, hy - h * 0.25, w, h * 0.35, glow);
        self.c.lighter = true;
        self.ripples.retain(|r| now - r.t0 <= 3.2);
        for r in 0..ROWS {
            let z = r as f32 / (ROWS - 1) as f32;
            let (persp, row_y) = (0.18 + z * z * 0.95, hy + (h * 1.08 - hy) * z.powf(1.6));
            let span = w * (0.75 + persp * 1.1);
            let x0 = (w - span) / 2.0;
            for c in 0..COLS {
                let u = c as f32 / (COLS - 1) as f32;
                let mut height = (u * 9.0 + now * 0.9 + z * 3.2).sin() * 0.6
                    + (-((u * 1.6 - now * 0.08).rem_euclid(1.6) - 0.8).powi(2) * 30.0).exp() * 0.5
                    + (u * 3.4 - now * 0.55 + z * 1.4).sin() * 0.35
                    + ((u + z) * 15.0 + now * 1.6).sin() * 0.08;
                let (mut lit, mut ripple_hue) = (0.0, 0.0);
                for rp in &self.ripples {
                    let (dx, dz, age) = ((u - rp.u) * 3.2, (z - 0.55) * 1.6, now - rp.t0);
                    let e = (-(dx.hypot(dz) - age * 0.9).powi(2) * 40.0).exp() * rp.v * (1.0 - age / 3.2).max(0.0);
                    height += e * 1.6;
                    lit += e;
                    ripple_hue = rp.hue;
                }
                let y = row_y - height * h * 0.04 * persp * 2.2;
                let size = ((1.0 + persp * 2.2 + lit * 1.4) * self.scale * 1.5).max(0.8).min(h * 0.011);
                // the game's name stays readable in the middle
                let clear = if z > 0.35 { 1.0 - 0.75 * (-((u - 0.5) / 0.16).powi(2)).exp() } else { 1.0 };
                let a = (((0.3 + z * 0.5) * (0.7 + height * 0.3) + lit * 0.6) * clear).min(0.95);
                if a < 0.03 {
                    continue;
                }
                let hue = if lit > 0.05 { ripple_hue } else { song } + (u - 0.5) * 90.0 + height * 16.0;
                self.c.circle(x0 + u * span, y, size / 2.0, hsl(hue.round(), 90.0, (52.0 + height * 10.0 + lit * 18.0).round().clamp(0.0, 74.0), a));
            }
        }
    }

    fn arcade(&mut self, dt: f32) {
        let (w, h, t) = (self.w, self.h, self.t);
        let px = (h / 180.0).round().max(2.0);
        let hy = h * (self.floor - 0.04);
        let snap = |v: f32| (v / px).round() * px;
        self.c.lighter = true;
        for i in 0..self.stars.len() {
            let s = &mut self.stars[i];
            s.x -= (0.004 + s.z * 0.02) * dt;
            if s.x < 0.0 {
                (s.x, s.y) = (1.0, self.rng.next());
            }
            if s.y * h > hy - px * 4.0 {
                continue;
            }
            let twinkle = 0.5 + 0.5 * (t * (2.0 + s.z * 4.0) + s.p).sin();
            let size = px * if s.z > 0.85 { 2.0 } else { 1.0 };
            let c = if s.z > 0.9 { hsl(if s.p > 3.0 { 190.0 } else { 315.0 }, 100.0, 78.0, 0.5 + twinkle * 0.5) } else { rgba(235, 240, 255, 0.2 + twinkle * 0.55 * s.z) };
            self.c.rect(snap(s.x * w), snap(s.y * h), size, size, Shader::SolidColor(c));
        }
        self.flares.retain(|f| t - f.t0 < 0.6);
        for f in &self.flares {
            let a = 1.0 - (t - f.t0) / 0.6;
            let (cx, cy, r) = (snap(f.x * w), snap(f.y * (hy - px * 10.0)), px * (1.0 + (1.0 - a) * 3.0));
            let c = Shader::SolidColor(hsl(if f.x > 0.5 { 190.0 } else { 315.0 }, 100.0, 80.0, a));
            let paint = Paint { shader: c, blend_mode: BlendMode::Plus, ..Paint::default() };
            for r in [Rect::from_xywh(cx - r, cy - px / 2.0, r * 2.0 + px, px), Rect::from_xywh(cx, cy - r, px, r * 2.0 + px)].into_iter().flatten() {
                self.c.pix.fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        self.c.lighter = false;
        // two mountain ranges of pixels drifting at different speeds: dark violet, outlined in cyan (far) and magenta (near)
        for (layer, amp0, speed, fill_hue, line_hue) in [(1usize, 0.09, 5.0, 275.0, 190.0), (0, 0.15, 2.5, 285.0, 315.0)] {
            let pk = &self.peaks[layer];
            let col = ((w / 110.0 / px).round() * px).max(px * 2.0);
            let (n, amp, shift) = ((w / col).ceil() as usize + 2, h * amp0, t * speed * self.scale);
            let (first, off) = ((shift / col).floor() as usize, shift % col);
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, hy);
            for i in 0..n {
                let (x, y) = (i as f32 * col - off, hy - (pk[(first + i) % pk.len()] * amp / px).round() * px);
                pb.line_to(x, y);
                pb.line_to(x + col, y);
            }
            pb.line_to(w, hy);
            pb.close();
            if let Some(p) = pb.finish() {
                let far = layer == 1;
                let fill = self.c.paint(down(hy - amp, hy, vec![stop(0.0, hsl(fill_hue, 55.0, if far { 14.0 } else { 10.0 }, 1.0)), stop(1.0, hsl(fill_hue, 60.0, if far { 6.0 } else { 4.0 }, 1.0))]));
                self.c.pix.fill_path(&p, &fill, FillRule::Winding, Transform::identity(), None);
                let edge = self.c.paint(Shader::SolidColor(hsl(line_hue, 100.0, 65.0, if far { 0.45 } else { 0.8 })));
                self.c.pix.stroke_path(&p, &edge, &Stroke { width: px, ..Stroke::default() }, Transform::identity(), None);
            }
        }
        // the neon horizon
        let glow = down(hy - px * 2.0, hy + h * 0.1, vec![stop(0.0, hsl(315.0, 100.0, 65.0, 0.45)), stop(1.0, hsl(315.0, 100.0, 50.0, 0.0))]);
        self.c.rect(0.0, hy - px, w, h * 0.1, glow);
    }

    fn aurora(&mut self) {
        let (w, h, t, hue) = (self.w, self.h, self.t, self.hue);
        self.c.lighter = true;
        for s in &self.stars {
            if s.y > 0.62 {
                continue;
            }
            let twinkle = 0.45 + 0.55 * (t * (1.0 + s.z * 2.0) + s.p).sin();
            let size = if s.z > 0.9 { 2.0 } else { 1.0 };
            let paint = Paint { shader: Shader::SolidColor(rgba(220, 235, 255, (0.15 + s.z * 0.5) * twinkle)), blend_mode: BlendMode::Plus, ..Paint::default() };
            if let Some(r) = Rect::from_xywh(s.x * w, s.y * h, size, size) {
                self.c.pix.fill_rect(r, &paint, Transform::identity(), None);
            }
        }
        // the curtains are drawn small and stretched up, which softens them like real aurora. They take the
        // console's colour, with a lighter and a complementary band
        let (ow, oh) = (self.small.width() as f32, self.small.height() as f32);
        self.small.fill(Color::TRANSPARENT);
        for (h0, yb, amp, ph) in [(hue, 0.16, 1.1, 0.0), (hue + 25.0, 0.22, 0.9, 2.1), (hue + 120.0, 0.1, 0.85, 4.2)] {
            for x in 0..ow as usize {
                let u = x as f32 / ow;
                let fold = (u * 6.0 + t * 0.12 + ph).sin() * 0.5 + (u * 13.0 - t * 0.19 + ph * 2.0).sin() * 0.25;
                let (top, len) = (oh * (yb + fold * 0.05), oh * (0.16 + 0.1 * (u * 4.0 + t * 0.09 + ph).sin()) * amp);
                let ray = (0.5 + 0.5 * (u * 55.0 + t * 0.5 + ph * 3.0 + (u * 17.0 + t * 0.25).sin() * 3.0).sin()).powi(3);
                let alpha = ((0.1 + ray * 0.22) * amp * (0.6 + 0.4 * (t * 0.23 + u * 3.0 + ph).sin())).max(0.0);
                let fade = |c: Color| Color::from_rgba(c.red(), c.green(), c.blue(), c.alpha() * alpha).unwrap_or(Color::TRANSPARENT);
                let column = down(top, top + len, vec![stop(0.0, fade(hsl(h0 + 90.0, 80.0, 60.0, 0.0))), stop(0.35, fade(hsl(h0 + 40.0, 85.0, 60.0, 0.7))), stop(0.85, fade(hsl(h0, 95.0, 58.0, 1.0))), stop(1.0, fade(hsl(h0, 95.0, 58.0, 0.0)))]);
                if let Some(r) = Rect::from_xywh(x as f32, top, 1.0, len.max(0.01)) {
                    self.small.fill_rect(r, &Paint { shader: column, blend_mode: BlendMode::Plus, ..Paint::default() }, Transform::identity(), None);
                }
            }
        }
        let stretch = PixmapPaint { blend_mode: BlendMode::Plus, quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
        self.c.pix.draw_pixmap(0, 0, self.small.as_ref(), &stretch, Transform::from_scale(w / ow, h / oh), None);
        let glow = down(h * (self.floor - 0.08), h * self.floor + h * 0.05, vec![stop(0.0, hsl(hue, 100.0, 60.0, 0.0)), stop(1.0, hsl(hue, 100.0, 60.0, 0.07))]);
        self.c.rect(0.0, h * (self.floor - 0.08), w, h * 0.13, glow);
    }

    fn midnight(&mut self, dt: f32) {
        let (w, h, t, hue) = (self.w, self.h, self.t, self.hue);
        let ly = h * self.floor;
        self.c.lighter = true;
        let breathe = 0.85 + 0.15 * (t * 0.5).sin();
        // a pool of light on the floor, flattened to a sixth of its height
        let flat = Transform::from_translate(-w / 2.0, -ly).post_scale(1.0, 0.16).post_translate(w / 2.0, ly);
        if let (Some(pool), Some(r)) = (
            RadialGradient::new(Point::from_xy(w / 2.0, ly), Point::from_xy(w / 2.0, ly), w * 0.5, vec![stop(0.0, hsl(hue, 90.0, 55.0, 0.22 * breathe)), stop(1.0, hsl(hue, 90.0, 40.0, 0.0))], SpreadMode::Pad, Transform::identity()),
            Rect::from_xywh(0.0, ly - w * 0.5, w, w),
        ) {
            let paint = self.c.paint(pool);
            self.c.pix.fill_rect(r, &paint, flat, None);
        }
        let line = across(w * 0.08, w * 0.92, vec![stop(0.0, hsl(hue, 90.0, 60.0, 0.0)), stop(0.5, hsl(hue, 80.0, 85.0, 0.9 * breathe)), stop(1.0, hsl(hue, 90.0, 60.0, 0.0))]);
        self.c.rect(w * 0.08, ly - (h * 0.0018).max(1.0), w * 0.84, (h * 0.0035).max(1.2), line);
        // dust, brightest as it passes through the light
        for i in 0..self.dust.len() {
            let d = &mut self.dust[i];
            d.y -= d.v * dt * 0.625;
            d.x += (t * 0.3 + d.p).sin() * 0.0048 * dt;
            if d.y < 0.25 {
                (d.y, d.x) = (0.98, self.rng.next());
            }
            let (near, middle) = ((-((d.y - self.floor) * 9.0).powi(2)).exp(), 1.0 - (d.x - 0.5).abs() * 1.6);
            let a = ((0.06 + near * 0.7) * middle).max(0.0);
            if a < 0.02 {
                continue;
            }
            let (x, y, r) = (d.x * w, d.y * h, d.s * self.scale * 1.6);
            self.c.circle(x, y, r, hsl(hue, 70.0, 85.0, a));
        }
    }

    /// Blue Shift: heads of light drifting across (one crossing every half minute or so), each leaving a tail.
    fn blue_shift(&mut self) {
        // each path: its height, speed, wave height, waves and phase, as parts of the screen
        const TRAILS: [[f32; 5]; 4] = [[0.24, 0.5, 0.05, 0.9, 0.0], [0.36, 0.42, 0.04, 0.7, 2.1], [0.7, 0.64, 0.04, 1.1, 4.4], [0.88, 0.9, 0.02, 0.8, 1.3]];
        const N: usize = 18;
        let (w, h, t) = (self.w, self.h, self.t);
        self.c.lighter = true;
        for [y0, speed, amp, waves, phase] in TRAILS {
            let at = |s: f32| {
                let u = (s * speed * 0.035 + phase * 0.17) % 1.3 - 0.15;
                (u * w, h * (y0 + (u * TAU * waves + phase).sin() * amp))
            };
            let (mut px, mut py) = at(t - 3.2);
            for i in 1..=N {
                let part = i as f32 / N as f32;
                let (x, y) = at(t - 3.2 + part * 3.2);
                if (x - px).abs() < w * 0.3 {
                    self.c.line(&[(px, py), (x, y)], hsl(pull(self.hue, 205.0, 0.25), 90.0, 72.0, part * 0.32), (h * 0.004 * part + 0.5).max(1.0), true);
                }
                (px, py) = (x, y);
            }
            self.c.glow(px, py, h * 0.016, vec![stop(0.0, rgba(235, 245, 255, 0.95)), stop(0.3, rgba(150, 200, 255, 0.4)), stop(1.0, rgba(120, 170, 255, 0.0))]);
        }
    }

    /// Swirl: sparks riding the backdrop's three arms inwards, brightest halfway in.
    fn swirl(&mut self) {
        let (w, h, t) = (self.w, self.h, self.t);
        // the arms are drawn in a 1600 × 900 space that covers the screen, round (800, 380)
        let s = (w / 1600.0).max(h / 900.0);
        let (ox, oy) = ((w - 1600.0 * s) / 2.0, (h - 900.0 * s) / 2.0);
        self.c.lighter = true;
        for k in 0..12 {
            let life = (t * 0.04 + k as f32 * 0.211) % 1.0;
            let th = 15.0 - life * 10.0;
            let r = 26.0 * (0.235 * th).exp();
            let (dx, dy, turn) = (th.cos() * r, th.sin() * r * 0.82, (k % 3) as f32 * 2.0944);
            let (x, y) = (ox + (800.0 + dx * turn.cos() - dy * turn.sin()) * s, oy + (380.0 + dx * turn.sin() + dy * turn.cos()) * s);
            let (bright, size) = ((life * PI).sin(), (h * (0.0035 + 0.004 * (1.0 - life))).max(1.5));
            self.c.glow(x, y, size * 4.0, vec![stop(0.0, rgba(255, 236, 200, bright * 0.9)), stop(0.25, rgba(255, 150, 60, bright * 0.45)), stop(1.0, rgba(255, 110, 30, 0.0))]);
        }
    }

    /// Forest: fireflies that wander slowly and blink, kept out of the band where the game's name sits.
    fn forest(&mut self) {
        let (w, h, t) = (self.w, self.h, self.t);
        self.c.lighter = true;
        for k in 0..22 {
            let kf = k as f32;
            let x = w * (0.04 + ((kf * 0.618 + (t * 0.05 + kf).sin() * 0.04) % 1.0) * 0.92);
            let y = h * (0.18 + ((kf * 0.371) % 1.0) * 0.62 + (t * 0.11 + kf * 2.3).sin() * 0.025);
            if y > h * 0.66 && (x / w - 0.5).abs() < 0.22 {
                continue;
            }
            let blink = (t * (0.5 + (k % 5) as f32 * 0.13) + kf * 1.7).sin().max(0.0);
            let size = (h * 0.006).max(1.5);
            self.c.glow(x, y, size * 2.4, vec![stop(0.0, rgba(226, 255, 150, 0.12 + blink * 0.6)), stop(1.0, rgba(160, 255, 90, 0.0))]);
        }
    }

    /// Synthwave: stars over the horizon, lines of the grid rolling toward the eye, lights running down its lanes.
    fn synth(&mut self) {
        let (w, h, t) = (self.w, self.h, self.t);
        // the horizon, as the backdrop's grid has it
        let hy = h * 0.57;
        self.c.lighter = true;
        let mut seeded = Seeded(5);
        for i in 0..70 {
            let (x, y, z) = (seeded.next() * w, seeded.next() * hy * 0.62, seeded.next());
            let twinkle = 0.5 + 0.5 * (t * (1.0 + z * 2.0) + i as f32).sin();
            let size = if z > 0.9 { 2.0 } else { 1.2 };
            self.c.rect(x, y, size, size, Shader::SolidColor(rgba(255, 230, 255, (0.12 + z * 0.4) * twinkle)));
        }
        for k in 0..2 {
            let u = (t * 0.42 + k as f32 * 0.5) % 1.0;
            let y = hy + (h - hy) * u.powf(2.1);
            let line = across(0.0, w, vec![stop(0.0, rgba(255, 80, 220, 0.0)), stop(0.5, rgba(255, 120, 235, 0.08 + u * 0.3)), stop(1.0, rgba(255, 80, 220, 0.0))]);
            self.c.rect(0.0, y - 1.0, w, (2.5 * u * self.scale).max(1.5), line);
        }
        for (k, lane) in [-9.0, -6.0, -4.0, 4.0, 6.0, 9.0f32].into_iter().enumerate() {
            let u = (t * (0.18 + k as f32 * 0.03) + k as f32 * 0.29) % 1.0;
            let e = u.powf(2.1);
            let (x, y, size) = (w / 2.0 + lane * 175.0 / 1600.0 * w * e, hy + (h - hy) * e, h * (0.004 + 0.012 * e));
            self.c.glow(x, y, size * 3.0, vec![stop(0.0, hsl((self.hue + 180.0 * (k % 2) as f32).round(), 100.0, 75.0, 0.7)), stop(1.0, rgba(0, 0, 0, 0.0))]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_moving_theme_draws_something() {
        for t in crate::themes::themes() {
            let mut layer = Layer::new(320, 180, 0.25, 0.61, 258.0);
            layer.note(440.0, 0.3, Some(96.0), 200.0);
            let mut lit = 0;
            // (half a second a frame: CRT's bar is off the screen at first)
            for _ in 0..12 {
                let p = layer.frame(&t.id, 0.5, 258.0, 200.0);
                assert_eq!(p.rgba.len(), 320 * 180 * 4);
                lit += p.rgba.chunks(4).filter(|px| px[3] > 0).count();
            }
            assert_eq!(lit > 0, moves(&t.id), "{}", t.id);
            assert_eq!(moves(&t.id), t.moves, "{}", t.id);
        }
    }

    #[test]
    fn the_extra_themes_generator_is_the_js_one() {
        // the first numbers of themes-extra.js seeded(5), as Node prints them
        let mut s = Seeded(5);
        let got: Vec<f32> = (0..3).map(|_| (s.next() * 1e6).round() / 1e6).collect();
        assert_eq!(got, SEEDED_5);
    }

    const SEEDED_5: [f32; 3] = [0.689775, 0.772743, 0.219763];
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/wiiu-manual.js, as RustyShelf has it.
//! A Wii U e-manual's page model (`wiiumanual`) drawn into book pages: a Contents page, then every manual page cut
//! where the model says, each with a running header (chapter + title) and a page number. JPEGs, in Lexend where the
//! Electron app uses the system's sans-serif.

use std::collections::HashMap;

use tiny_skia::{Color, FillRule, FilterQuality, GradientStop, LinearGradient, Paint, Path, PathBuilder, Pattern, Pixmap, Point, Rect, Shader, SpreadMode, Stroke, Transform};

use crate::bflyt::Rgba;
use crate::drawn::{font_for, spaced_path, Face};
use crate::wiiumanual::{Colors, Image, Line, Model, Op, Page};

/// render scale: pages are 1376 px wide
const S: f32 = 2.0;
/// header and footer bands (page px)
const HEAD: f32 = 84.0;
const FOOT: f32 = 46.0;
/// past this a page is damaged, not long (page px)
const MAX_PAGE_H: f64 = 20000.0;

const MEDIUM: Face = Face::Lexend(1);
const SEMIBOLD: Face = Face::Lexend(4);
const BOLD: Face = Face::Lexend(2);

type Rgb = [f32; 3];

fn color(c: Rgb, a: f32) -> Color {
    // (as CSS rgba() takes them: the channels cut to whole numbers)
    let ch = |v: f32| v.clamp(0.0, 255.0).trunc() / 255.0;
    Color::from_rgba(ch(c[0]), ch(c[1]), ch(c[2]), a.clamp(0.0, 1.0)).unwrap_or(Color::BLACK)
}

fn rgba_color(c: [f32; 4], a: f32) -> Color {
    color([c[0], c[1], c[2]], c[3] / 255.0 * a)
}

fn solid(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(c);
    p.anti_alias = true;
    p
}

fn lum(c: Rgb) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn js_round(v: f32) -> f32 {
    (v + 0.5).floor()
}

// ---------- pictures ----------

/// A straight-alpha RGBA picture.
struct Pic {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

fn premultiplied(p: &Pic) -> Option<Pixmap> {
    let mut pix = Pixmap::new(p.w, p.h)?;
    for (d, s) in pix.pixels_mut().iter_mut().zip(p.rgba.chunks_exact(4)) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(pix)
}

/// Rounds as a canvas's pixel array stores a number: to the nearest, a half to even.
fn clamped_u8(v: f64) -> u8 {
    v.clamp(0.0, 255.0).round_ties_even() as u8
}

struct Pictures {
    raw: HashMap<String, Pic>,
    /// by picture, material colours and vertex colour
    cache: HashMap<String, Option<Pixmap>>,
}

impl Pictures {
    fn load(model: &Model) -> Pictures {
        let mut raw = HashMap::new();
        for (k, img) in &model.images {
            let pic = match img {
                Image::Texture(t) => Some(Pic { w: t.width as u32, h: t.height as u32, rgba: t.rgba.clone() }),
                Image::Jpeg(b) => image::load_from_memory_with_format(b, image::ImageFormat::Jpeg).ok().map(|i| {
                    let i = i.to_rgba8();
                    Pic { w: i.width(), h: i.height(), rgba: i.into_raw() }
                }),
            };
            if let Some(p) = pic {
                raw.insert(k.clone(), p);
            }
        }
        Pictures { raw, cache: HashMap::new() }
    }

    /// The picture in the material's colours, out = black + tex × (white − black), then × the vertex colour.
    fn variant(&mut self, key: &str, tint: Option<(Rgba, Rgba)>, vc: &Colors) -> Option<&Pixmap> {
        let plain_vc = match vc {
            Colors::One(c) if *c != [255; 4] => Some(*c),
            _ => None,
        };
        let id = format!("{key}|{tint:?}|{plain_vc:?}");
        if !self.cache.contains_key(&id) {
            let made = self.raw.get(key).and_then(|im| {
                if tint.is_none() && plain_vc.is_none() {
                    return premultiplied(im);
                }
                let (bk, wh) = tint.unwrap_or(([0; 4], [255; 4]));
                let v = plain_vc.unwrap_or([255; 4]);
                let rgba = im
                    .rgba
                    .chunks_exact(4)
                    .flat_map(|p| {
                        let ch = |k: usize| clamped_u8((f64::from(bk[k]) + f64::from(p[k]) * (f64::from(wh[k]) - f64::from(bk[k])) / 255.0) * f64::from(v[k]) / 255.0);
                        [ch(0), ch(1), ch(2), ch(3)]
                    })
                    .collect();
                premultiplied(&Pic { w: im.w, h: im.h, rgba })
            });
            self.cache.insert(id.clone(), made);
        }
        self.cache.get(&id).and_then(Option::as_ref)
    }
}

/// A rect from two corners in any order; None when it's empty.
fn rect_of(x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
    let (l, r) = if w < 0.0 { (x + w, x) } else { (x, x + w) };
    let (t, b) = if h < 0.0 { (y + h, y) } else { (y, y + h) };
    Rect::from_ltrb(l, t, r, b)
}

fn draw_img(pix: &mut Pixmap, pics: &mut Pictures, op: &Op) {
    let (Some(key), base) = (op.img.as_deref(), Transform::from_scale(S, S)) else { return };
    let Some(src) = pics.variant(key, op.tint, &op.c) else { return };
    let (iw, ih) = (f64::from(src.width()), f64::from(src.height()));
    let (u0, u1, v0, v1) = (op.u0, op.u1, op.v0, op.v1);
    if u0 == u1 || v0 == v1 {
        return;
    }
    let outside = u0.min(u1) < -1e-4 || u0.max(u1) > 1.0001 || v0.min(v1) < -1e-4 || v0.max(v1) > 1.0001;
    // clamped: the edge texels stretch past 0 and 1
    let spread = if outside && (op.ws == 1 || op.wt == 1) { SpreadMode::Repeat } else { SpreadMode::Pad };
    let (sx, sy) = (op.w / ((u1 - u0) * iw), op.h / ((v1 - v0) * ih));
    let ts = Transform::from_row(sx as f32, 0.0, 0.0, sy as f32, (op.x - u0 * iw * sx) as f32, (op.y - v0 * ih * sy) as f32);
    let paint = Paint { shader: Pattern::new(src.as_ref(), spread, FilterQuality::Bilinear, op.a as f32, ts), anti_alias: true, ..Paint::default() };
    // (a hair wider, as the Electron app draws them: no seam between a frame's pieces)
    if let Some(r) = rect_of(op.x as f32, op.y as f32, op.w as f32 + 0.05, op.h as f32 + 0.05) {
        pix.fill_rect(r, &paint, base, None);
    }
}

fn draw_rect(pix: &mut Pixmap, op: &Op) {
    let mul = |c: Rgba| -> [f32; 4] {
        let w = op.tint.map_or([255; 4], |t| t.1);
        std::array::from_fn(|i| f32::from(c[i]) * f32::from(w[i]) / 255.0)
    };
    let Some(r) = rect_of(op.x as f32, op.y as f32, op.w as f32, op.h as f32) else { return };
    let a = op.a as f32;
    let shader = match &op.c {
        Colors::One(c) => Shader::SolidColor(rgba_color(mul(*c), a)),
        Colors::Four(c) => {
            let stops = vec![GradientStop::new(0.0, rgba_color(mul(c[0]), a)), GradientStop::new(1.0, rgba_color(mul(c[2]), a))];
            let Some(g) = LinearGradient::new(Point::from_xy(0.0, op.y as f32), Point::from_xy(0.0, (op.y + op.h) as f32), stops, SpreadMode::Pad, Transform::identity()) else { return };
            g
        }
    };
    pix.fill_rect(r, &Paint { shader, anti_alias: true, ..Paint::default() }, Transform::from_scale(S, S), None);
}

// ---------- text ----------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Icon {
    Btn,
    Pill,
    Pad,
    Stick,
    Arrow,
    Nfc,
}

/// Wii U system-font icons (private-use characters), drawn as small badges
fn icon(k: u32) -> (Icon, &'static str) {
    use Icon::*;
    match k {
        0xE000 => (Btn, "A"),
        0xE001 => (Btn, "B"),
        0xE002 => (Btn, "X"),
        0xE003 => (Btn, "Y"),
        0xE004 => (Pill, "L"),
        0xE005 => (Pill, "R"),
        0xE006 => (Pad, ""),
        0xE045 => (Btn, "+"),
        0xE046 => (Btn, "−"),
        0xE047 => (Btn, "1"),
        0xE048 => (Btn, "2"),
        0xE073 => (Btn, "⌂"),
        0xE079 => (Pad, "u"),
        0xE07A => (Pad, "d"),
        0xE07B => (Pad, "l"),
        0xE07C => (Pad, "r"),
        0xE07D => (Pad, "ud"),
        0xE07E => (Pad, "lr"),
        0xE080 => (Stick, ""),
        0xE081 => (Stick, "L"),
        0xE082 => (Stick, "R"),
        0xE08C => (Arrow, "lr"),
        0xE08D => (Arrow, "ud"),
        0xE092 => (Arrow, "u"),
        0xE093 => (Arrow, "d"),
        0xE094 => (Arrow, "l"),
        0xE095 => (Arrow, "r"),
        0xE099 => (Nfc, "NFC"),
        _ => (Btn, "•"),
    }
}

fn is_icon(c: char) -> bool {
    (0xE000..=0xF8FF).contains(&u32::from(c))
}

enum Run {
    Text(String),
    Icon(u32),
}

fn runs(s: &str) -> Vec<Run> {
    let (mut out, mut cur) = (Vec::new(), String::new());
    for c in s.chars() {
        if is_icon(c) {
            if !cur.is_empty() {
                out.push(Run::Text(std::mem::take(&mut cur)));
            }
            out.push(Run::Icon(c.into()));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(Run::Text(cur));
    }
    out
}

fn icon_w(k: u32, px: f32) -> f32 {
    match icon(k).0 {
        Icon::Nfc => px * 1.9,
        Icon::Pill => px * 1.35,
        _ => px * 1.02,
    }
}

/// A text's width with `spacing` after every letter, as a canvas measures it with letterSpacing.
fn width(face: Face, s: &str, size: f32, spacing: f32) -> f32 {
    s.chars()
        .map(|c| {
            let f = font_for(face, c);
            ab_glyph::Font::h_advance_unscaled(f, ab_glyph::Font::glyph_id(f, c)) * size / ab_glyph::Font::units_per_em(f).unwrap_or(1000.0) + spacing
        })
        .sum()
}

/// How far under a line's middle its baseline is (a canvas's textBaseline middle).
fn below_middle(face: Face, size: f32) -> f32 {
    let f = font_for(face, 'H');
    let (up, down) = (ab_glyph::Font::ascent_unscaled(f), -ab_glyph::Font::descent_unscaled(f));
    (up - down) / (up + down) / 2.0 * size
}

#[allow(clippy::too_many_arguments)]
fn text(pix: &mut Pixmap, t: Transform, face: Face, s: &str, size: f32, x: f32, y: f32, spacing: f32, c: Color) {
    if let Some(p) = spaced_path(face, s, size, x, y, spacing) {
        pix.fill_path(&p, &solid(c), FillRule::Winding, t, None);
    }
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
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

#[allow(clippy::too_many_arguments)]
fn draw_icon(pix: &mut Pixmap, t: Transform, k: u32, x: f32, base: f32, px: f32, ink: [u8; 3]) {
    let (kind, label) = icon(k);
    let (r, cx, cy) = (px * 0.46, x + icon_w(k, px) / 2.0, base - px * 0.34);
    let ink_f = [f32::from(ink[0]), f32::from(ink[1]), f32::from(ink[2])];
    let dark = lum(ink_f) < 140.0;
    let (fg, inv) = (color(ink_f, 1.0), if dark { Color::WHITE } else { color([58.0, 42.0, 26.0], 1.0) });
    let fill = |pix: &mut Pixmap, p: Option<Path>, c: Color| {
        if let Some(p) = p {
            pix.fill_path(&p, &solid(c), FillRule::Winding, t, None);
        }
    };
    let stroke_with = |pix: &mut Pixmap, p: Option<Path>, st: &Stroke| {
        if let Some(p) = p {
            pix.stroke_path(&p, &solid(fg), st, t, None);
        }
    };
    let label_at = |pix: &mut Pixmap, s: &str, size: f32, c: Color| {
        let w = width(BOLD, s, size, 0.0);
        text(pix, t, BOLD, s, size, cx - w / 2.0, cy + px * 0.03 + below_middle(BOLD, size), 0.0, c);
    };
    let thin = Stroke { width: px * 0.09, ..Stroke::default() };
    match kind {
        Icon::Btn => {
            fill(pix, PathBuilder::from_circle(cx, cy, r), fg);
            label_at(pix, label, px * 0.62, inv);
        }
        Icon::Pill => {
            let (w, h) = (icon_w(k, px) * 0.92, r * 1.7);
            fill(pix, round_rect(cx - w / 2.0, cy - h / 2.0, w, h, h / 2.0), fg);
            label_at(pix, label, px * 0.58, inv);
        }
        Icon::Nfc => {
            let (w, h) = (icon_w(k, px) * 0.94, r * 1.7);
            stroke_with(pix, round_rect(cx - w / 2.0, cy - h / 2.0, w, h, h * 0.3), &thin);
            label_at(pix, label, px * 0.5, fg);
        }
        Icon::Pad => {
            let (a, l) = (r * 0.36, r);
            let mut pb = PathBuilder::new();
            pb.push_rect(Rect::from_xywh(cx - a, cy - l, a * 2.0, l * 2.0).unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()));
            pb.push_rect(Rect::from_xywh(cx - l, cy - a, l * 2.0, a * 2.0).unwrap_or(Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()));
            fill(pix, pb.finish(), fg);
            let tri = |pix: &mut Pixmap, dx: f32, dy: f32| {
                let (tx, ty, s) = (cx + dx * l * 0.62, cy + dy * l * 0.62, a * 0.75);
                let mut pb = PathBuilder::new();
                if dx != 0.0 {
                    pb.move_to(tx + dx * s, ty);
                    pb.line_to(tx - dx * s * 0.4, ty - s);
                    pb.line_to(tx - dx * s * 0.4, ty + s);
                } else {
                    pb.move_to(tx, ty + dy * s);
                    pb.line_to(tx - s, ty - dy * s * 0.4);
                    pb.line_to(tx + s, ty - dy * s * 0.4);
                }
                pb.close();
                fill(pix, pb.finish(), inv);
            };
            for (ch, dx, dy) in [('u', 0.0, -1.0), ('d', 0.0, 1.0), ('l', -1.0, 0.0), ('r', 1.0, 0.0)] {
                if label.contains(ch) {
                    tri(pix, dx, dy);
                }
            }
        }
        Icon::Stick => {
            stroke_with(pix, PathBuilder::from_circle(cx, cy, r), &thin);
            fill(pix, PathBuilder::from_circle(cx, cy, r * 0.55), fg);
            if !label.is_empty() {
                label_at(pix, label, px * 0.42, inv);
            }
        }
        Icon::Arrow => {
            let (l, h) = (r * 0.95, r * 0.42);
            let st = Stroke { width: px * 0.1, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Stroke::default() };
            for (ch, dx, dy) in [('u', 0.0f32, -1.0f32), ('d', 0.0, 1.0), ('l', -1.0, 0.0), ('r', 1.0, 0.0)] {
                if !label.contains(ch) {
                    continue;
                }
                let mut pb = PathBuilder::new();
                pb.move_to(cx, cy);
                pb.line_to(cx + dx * l, cy + dy * l);
                stroke_with(pix, pb.finish(), &st);
                let mut pb = PathBuilder::new();
                if dx != 0.0 {
                    pb.move_to(cx + dx * l - dx * h, cy - h);
                    pb.line_to(cx + dx * l, cy);
                    pb.line_to(cx + dx * l - dx * h, cy + h);
                } else {
                    pb.move_to(cx - h, cy + dy * l - dy * h);
                    pb.line_to(cx, cy + dy * l);
                    pb.line_to(cx + h, cy + dy * l - dy * h);
                }
                stroke_with(pix, pb.finish(), &st);
            }
        }
    }
}

fn font_px(l: &Line) -> f32 {
    (l.sy * 0.8) as f32
}

fn measure(l: &Line, spacing: f32) -> f32 {
    let px = font_px(l);
    runs(&l.s)
        .iter()
        .map(|r| match r {
            Run::Text(s) => width(MEDIUM, s, px, spacing),
            Run::Icon(k) => icon_w(*k, px) + spacing,
        })
        .sum()
}

/// A photo the text keeps clear of.
struct Obstacle {
    x: f64,
    y: f64,
    h: f64,
}

/// Where a line has to stop: its box's right edge, or a picture to its right.
fn limit_for(l: &Line, obst: &[Obstacle], page_w: f64) -> f32 {
    let mut right = (page_w - 8.0).min(l.bx + l.bw);
    for o in obst {
        if o.x > l.x + 30.0 && o.y < l.y + l.sy && o.y + o.h > l.y {
            right = right.min(o.x - 8.0);
        }
    }
    (right - l.x) as f32
}

fn draw_text(pix: &mut Pixmap, page: &Page, obst: &[Obstacle]) {
    // justified lines (spacing, not a paragraph's last) fill the paragraph's width as measured in this font; lines
    // beside a picture justify among themselves, to the space next to it
    let key = |l: &Line, max: f32| format!("{}|{}", l.in_box, js_round(max / 24.0));
    let measured: Vec<(f32, f32)> = page.text.iter().map(|l| (limit_for(l, obst, page.width), measure(l, 0.0))).collect();
    let mut widest: HashMap<String, f32> = HashMap::new();
    for (l, &(max, w)) in page.text.iter().zip(&measured) {
        let e = widest.entry(key(l, max)).or_insert(0.0);
        *e = e.max(w.min(max));
    }
    for (l, &(max, w0)) in page.text.iter().zip(&measured) {
        if l.s.trim().is_empty() {
            continue;
        }
        let (px, chars) = (font_px(l), l.s.chars().count());
        let (mut w, mut spacing) = (w0, 0.0);
        let target = widest.get(&key(l, max)).copied().filter(|&v| v != 0.0).unwrap_or(w).min(max);
        if !l.last && l.sp > 0.05 && target > w && chars > 1 && (target - w) / (chars as f32 - 1.0) < px * 0.14 {
            spacing = (target - w) / (chars as f32 - 1.0);
            w = measure(l, spacing);
        }
        let squeeze = if w > max && max > 20.0 { max / w } else { 1.0 };
        let t = Transform::from_scale(S, S).pre_translate(l.x as f32, 0.0).pre_scale(squeeze, 1.0);
        let ink = color([f32::from(l.color[0]), f32::from(l.color[1]), f32::from(l.color[2])], 1.0);
        let base = (l.y + l.sy * 0.8) as f32;
        let mut x = 0.0;
        for r in runs(&l.s) {
            match r {
                Run::Text(s) => {
                    text(pix, t, MEDIUM, &s, px, x, base, spacing, ink);
                    x += width(MEDIUM, &s, px, spacing);
                }
                Run::Icon(k) => {
                    draw_icon(pix, t, k, x, base, px, l.color);
                    x += icon_w(k, px) + spacing;
                }
            }
        }
    }
}

// ---------- pages ----------

fn canvas(w: f64, h: f64) -> Result<Pixmap, String> {
    Pixmap::new(js_round(w as f32 * S).max(1.0) as u32, js_round(h as f32 * S).max(1.0) as u32).ok_or_else(|| "page too big to draw".to_string())
}

fn draw_page(page: &Page, pics: &mut Pictures) -> Result<Pixmap, String> {
    if page.height > MAX_PAGE_H {
        return Err(format!("a page {} px tall", page.height));
    }
    let mut pix = canvas(page.width, page.height)?;
    pix.fill(Color::WHITE);
    for op in &page.ops {
        if op.img.is_some() {
            draw_img(&mut pix, pics, op);
        } else {
            draw_rect(&mut pix, op);
        }
    }
    // only photos push text aside (frames, banners and decorations sit around or behind it), with room for a frame
    let obst: Vec<Obstacle> = page.ops.iter().filter(|o| !o.bg && o.photo && o.w < page.width * 0.7).map(|o| Obstacle { x: o.x - 10.0, y: o.y - 10.0, h: o.h + 20.0 }).collect();
    draw_text(&mut pix, page, &obst);
    Ok(pix)
}

/// The average colour of a band of a drawn page (page px), for a header or footer that blends with its paper.
fn band_color(c: &Pixmap, y: f64, h: f64) -> Rgb {
    let ch = c.height() as f32;
    let yy = js_round(y as f32 * S).clamp(0.0, ch - 1.0) as usize;
    let hh = js_round(h as f32 * S).min(ch - yy as f32).max(1.0) as usize;
    let w = c.width() as usize;
    let px = &c.pixels()[yy * w..((yy + hh) * w).min(c.pixels().len())];
    let (mut sum, mut n) = ([0f32; 3], 0f32);
    for p in px.iter().step_by(4) {
        let p = p.demultiply();
        sum[0] += f32::from(p.red());
        sum[1] += f32::from(p.green());
        sum[2] += f32::from(p.blue());
        n += 1.0;
    }
    sum.map(|v| v / n.max(1.0))
}

/// The manual's paper: the average of its repeating background texture (drawn small, as onto an 8×8 canvas).
fn paper_of(page: &Page, pics: &Pictures) -> Option<Rgb> {
    let op = page.ops.iter().find(|o| o.bg && o.img.is_some() && o.ws == 1 && o.wt == 1)?;
    let im = pics.raw.get(op.img.as_deref()?)?;
    let (w, h) = (im.w as usize, im.h as usize);
    let mut sum = [0f32; 3];
    for cy in 0..8 {
        for cx in 0..8 {
            let (x0, x1, y0, y1) = (cx * w / 8, ((cx + 1) * w / 8).max(cx * w / 8 + 1).min(w), cy * h / 8, ((cy + 1) * h / 8).max(cy * h / 8 + 1).min(h));
            let mut cell = [0f32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = &im.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    let a = f32::from(p[3]);
                    cell = [cell[0] + f32::from(p[0]) * a, cell[1] + f32::from(p[1]) * a, cell[2] + f32::from(p[2]) * a, cell[3] + a];
                }
            }
            if cell[3] > 0.0 {
                for k in 0..3 {
                    sum[k] += cell[k] / cell[3];
                }
            }
        }
    }
    Some(sum.map(|v| v / 64.0))
}

fn frame(w: f64, h: f64, paper: Rgb) -> Result<Pixmap, String> {
    let mut pix = canvas(w, h)?;
    pix.fill(color(paper, 1.0));
    Ok(pix)
}

/// `src` (device px) onto `dst` with its row `from_y` at `to_y`, over `rows` rows.
fn blit(dst: &mut Pixmap, src: &Pixmap, from_y: f32, to_y: f32, rows: f32) {
    let ts = Transform::from_translate(0.0, to_y - from_y);
    let paint = Paint { shader: Pattern::new(src.as_ref(), SpreadMode::Pad, FilterQuality::Nearest, 1.0, ts), ..Paint::default() };
    if let Some(r) = Rect::from_xywh(0.0, to_y, dst.width() as f32, rows) {
        dst.fill_rect(r, &paint, Transform::identity(), None);
    }
}

fn header(pix: &mut Pixmap, w: f32, paper: Rgb, chapter: &str, title: &str, cont: bool) {
    let t = Transform::from_scale(S, S);
    let ink = if lum(paper) > 140.0 { [48.0, 44.0, 52.0] } else { [245.0, 245.0, 250.0] };
    let soft = mix(ink, paper, 0.45);
    if let Some(r) = Rect::from_xywh(0.0, 0.0, w, HEAD) {
        pix.fill_rect(r, &solid(color(mix(paper, ink, 0.035), 1.0)), t, None);
    }
    if !chapter.is_empty() {
        text(pix, t, SEMIBOLD, &chapter.to_uppercase(), 15.0, 32.0, 30.0, 2.2, color(soft, 1.0));
    }
    let line = if cont { format!("{title}  ·  continued") } else { title.to_string() };
    text(pix, t, BOLD, &line, if cont { 25.0 } else { 30.0 }, 32.0, if chapter.is_empty() { 54.0 } else { 66.0 }, 0.0, color(ink, 1.0));
    if let Some(r) = Rect::from_xywh(32.0, HEAD - 1.5, w - 64.0, 1.5) {
        pix.fill_rect(r, &solid(color(mix(paper, ink, 0.16), 1.0)), t, None);
    }
}

fn footer(pix: &mut Pixmap, w: f32, y: f32, paper: Rgb, n: usize) {
    let t = Transform::from_scale(S, S);
    let ink = if lum(paper) > 140.0 { [60.0, 56.0, 64.0] } else { [235.0, 235.0, 240.0] };
    if let Some(r) = Rect::from_xywh(0.0, y, w, FOOT) {
        pix.fill_rect(r, &solid(color(paper, 1.0)), t, None);
    }
    let s = n.to_string();
    let tw = width(SEMIBOLD, &s, 17.0, 0.0);
    text(pix, t, SEMIBOLD, &s, 17.0, w / 2.0 - tw / 2.0, y + FOOT / 2.0 + 1.0 + below_middle(SEMIBOLD, 17.0), 0.0, color(mix(ink, paper, 0.3), 1.0));
}

/// A page's closing artwork (its bottom background picture), shown again on the Contents page.
struct Art {
    /// the band of the drawn page, device px
    pix: Pixmap,
    h: f32,
}

fn contents(model: &Model, starts: &[usize], paper: Rgb, art: Option<&Art>) -> Result<Pixmap, String> {
    enum Entry<'a> {
        Chapter(&'a str),
        Title(&'a str, usize),
    }
    let w = model.width as f32;
    let mut entries = Vec::new();
    for ch in &model.toc {
        entries.push(Entry::Chapter(if ch.name.is_empty() { "More" } else { &ch.name }));
        for &i in &ch.pages {
            entries.push(Entry::Title(&model.pages[i].title, starts[i]));
        }
    }
    let (line_h, chap_h, top) = (38.0, 52.0, 150.0);
    let mut h: f32 = top + entries.iter().map(|e| if matches!(e, Entry::Chapter(_)) { chap_h } else { line_h }).sum::<f32>() + 40.0;
    let art_h = art.map_or(0.0, |a| a.h.min(300.0));
    h = (h + art_h).max(980.0);
    let mut pix = frame(f64::from(w), f64::from(h), paper)?;
    let t = Transform::from_scale(S, S);
    let ink = if lum(paper) > 140.0 { [48.0, 44.0, 52.0] } else { [245.0, 245.0, 250.0] };
    let soft = mix(ink, paper, 0.45);
    if let Some(a) = art {
        let to = (h - a.h) * S;
        blit(&mut pix, &a.pix, 0.0, to, a.pix.height() as f32);
    }
    text(&mut pix, t, SEMIBOLD, "INSTRUCTION MANUAL", 15.0, 40.0, 58.0, 2.2, color(soft, 1.0));
    let mut title: String = if model.title.is_empty() { "Contents".into() } else { model.title.clone() };
    while width(BOLD, &title, 34.0, 0.0) > w - 80.0 && title.chars().count() > 4 {
        let keep = title.chars().count() - 2;
        title = title.chars().take(keep).collect::<String>().trim_end().to_string() + "…";
    }
    text(&mut pix, t, BOLD, &title, 34.0, 40.0, 102.0, 0.0, color(ink, 1.0));
    if let Some(r) = Rect::from_xywh(40.0, 124.0, w - 80.0, 2.0) {
        pix.fill_rect(r, &solid(color(mix(paper, ink, 0.18), 1.0)), t, None);
    }
    let mut y = top;
    for e in &entries {
        match e {
            Entry::Chapter(name) => {
                y += chap_h;
                text(&mut pix, t, BOLD, name, 21.0, 40.0, y - 12.0, 0.0, color(ink, 1.0));
            }
            Entry::Title(title, page) => {
                y += line_h;
                let num = page.to_string();
                let (nw, tw) = (width(MEDIUM, &num, 19.0, 0.0), width(MEDIUM, title, 19.0, 0.0));
                let c = color(mix(ink, paper, 0.12), 1.0);
                text(&mut pix, t, MEDIUM, title, 19.0, 62.0, y - 12.0, 0.0, c);
                text(&mut pix, t, MEDIUM, &num, 19.0, w - 40.0 - nw, y - 12.0, 0.0, c);
                let dot = solid(color(mix(ink, paper, 0.6), 1.0));
                let mut x = 62.0 + tw + 10.0;
                while x < w - 52.0 - nw {
                    if let Some(r) = Rect::from_xywh(x, y - 16.0, 2.0, 2.0) {
                        pix.fill_rect(r, &dot, t, None);
                    }
                    x += 7.0;
                }
            }
        }
    }
    Ok(pix)
}

fn jpeg(pix: &Pixmap) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = pix
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue()]
        })
        .collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 93).encode(&rgb, pix.width(), pix.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Every book page of the manual as a JPEG: the Contents page first.
pub fn render(model: &Model) -> Result<Vec<Vec<u8>>, String> {
    let mut pics = Pictures::load(model);
    let mut starts = Vec::new();
    let mut n = 2;
    for p in &model.pages {
        starts.push(n);
        n += p.parts.len();
    }
    let (mut out, mut num) = (Vec::new(), 2);
    let (mut art, mut paper_all): (Option<Art>, Option<Rgb>) = (None, None);
    for page in &model.pages {
        let c = draw_page(page, &mut pics)?;
        if art.is_none() {
            let band = page.ops.iter().find(|o| o.bg && o.img.is_some() && (o.y + o.h - page.height).abs() < 2.0 && o.h > 150.0 && o.h < 520.0 && o.w > page.width * 0.9);
            if let Some(b) = band {
                let (y0, rows) = (js_round(b.y as f32 * S).max(0.0), js_round(b.h as f32 * S).max(1.0));
                let mut pix = Pixmap::new(c.width(), rows as u32).ok_or("art too big")?;
                blit(&mut pix, &c, y0, 0.0, rows);
                art = Some(Art { pix, h: b.h as f32 });
            }
        }
        // one paper colour for every header of a themed manual, sampled once from a plain stretch of background
        let themed = page.ops.iter().any(|o| o.bg && o.img.is_some());
        if paper_all.is_none() && themed {
            paper_all = Some(paper_of(page, &pics).unwrap_or_else(|| band_color(&c, (page.height - 10.0).min(page.height * 0.45), 6.0)));
        }
        for &(y0, y1) in &page.parts {
            let paper_top = match paper_all {
                Some(p) if themed => p,
                _ => band_color(&c, y0 + 2.0, 6.0),
            };
            let paper_bot = band_color(&c, y1 - 8.0, 6.0);
            let body = (y1 - y0) as f32;
            let mut pc = frame(page.width, f64::from(HEAD + body + FOOT), paper_top)?;
            blit(&mut pc, &c, y0 as f32 * S, HEAD * S, body * S);
            header(&mut pc, page.width as f32, paper_top, &page.chapter, &page.title, y0 != 0.0);
            footer(&mut pc, page.width as f32, HEAD + body, paper_bot, num);
            num += 1;
            out.push(jpeg(&pc)?);
        }
    }
    let toc = contents(model, &starts, paper_all.unwrap_or([250.0, 249.0, 246.0]), art.as_ref())?;
    out.insert(0, jpeg(&toc)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_a_manual() {
        let m = crate::wiiumanual::build(&crate::wiiumanual::tests::manual(), "A Test Game With A Rather Long Name That Will Not Fit").unwrap();
        let pages = render(&m).unwrap();
        let parts: usize = m.pages.iter().map(|p| p.parts.len()).sum();
        assert_eq!(pages.len(), parts + 1);
        let imgs: Vec<image::RgbImage> = pages.iter().map(|b| image::load_from_memory_with_format(b, image::ImageFormat::Jpeg).unwrap().to_rgb8()).collect();
        for (i, im) in imgs.iter().enumerate() {
            assert_eq!(im.width(), 1376, "page {i}");
        }
        assert!(imgs[0].height() >= 1960, "the Contents page is at least 980 px tall");
        let first = &imgs[1];
        let part = m.pages[0].parts[0];
        assert_eq!(first.height(), ((84.0 + part.1 - part.0 + 46.0) * 2.0) as u32);
        // the paper texture (200 grey) shows through under the header
        let p = first.get_pixel(20, 84 * 2 + 40);
        assert!(p.0.iter().all(|&v| (200..=225).contains(&v)), "{p:?}");
        // text was drawn: dark pixels where the body's first line is
        let l = &m.pages[0].text[0];
        let (y0, y1) = (((84.0 + l.y) * 2.0) as u32, ((84.0 + l.y + l.sy) * 2.0) as u32);
        let x0 = (l.x * 2.0) as u32;
        let dark = (y0..y1).flat_map(|y| (x0..x0 + 400).map(move |x| (x, y))).filter(|&(x, y)| first.get_pixel(x, y).0[0] < 100).count();
        assert!(dark > 500, "{dark}");
        // the photo's grey (120) where it was put
        let p = first.get_pixel(1100, (84 + 250) * 2);
        assert!(p.0.iter().all(|&v| (110..=130).contains(&v)), "{p:?}");
    }

    #[test]
    fn icons_and_runs() {
        let r = runs("a\u{E000}b\u{E099}");
        assert_eq!(r.len(), 4);
        assert!(matches!(r[1], Run::Icon(0xE000)));
        assert_eq!(icon(0xE0FF), (Icon::Btn, "•"));
        assert!((icon_w(0xE099, 10.0) - 19.0).abs() < 1e-4);
        let l = Line { in_box: "b".into(), bx: 0.0, bw: 100.0, x: 0.0, y: 0.0, sx: 20.0, sy: 20.0, sp: 0.0, color: [0; 3], s: "ab\u{E000}".into(), last: true };
        let w0 = measure(&l, 0.0);
        assert!((measure(&l, 2.0) - w0 - 6.0).abs() < 1e-3, "spacing after each letter and icon");
    }
}

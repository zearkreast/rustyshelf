//! Paints a CSS `background` the way a browser does: the Electron app's backdrops, glows and glare are written as
//! CSS gradients, and the Rust app paints the same text. Takes a comma-separated list of layers (the first on top):
//! linear, radial, conic and repeating linear gradients, each with an optional position, size and repeat, and a
//! pictures given as SVG data URLs, and a colour last.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

use crate::drawn::Picture;

/// Red, green, blue and alpha, the colours already multiplied by the alpha (a browser blends gradients that way).
type Premul = [f32; 4];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Len {
    Px(f32),
    /// of the box, the gradient's length or a full turn
    Part(f32),
}

impl Len {
    fn of(self, whole: f32) -> f32 {
        match self {
            Len::Px(v) => v,
            Len::Part(p) => whole * p,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Extent {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    FarthestCorner,
    Radii(Len, Len),
}

#[derive(Debug)]
enum Shape {
    /// the way it runs, in radians clockwise from straight up
    Linear(f32),
    Radial { circle: bool, extent: Extent, at: (Len, Len) },
    Conic { from: f32, at: (Len, Len) },
}

#[derive(Debug)]
struct Gradient {
    shape: Shape,
    stops: Vec<(Premul, Option<Len>)>,
    repeating: bool,
}

#[derive(Debug)]
enum Source {
    Gradient(Gradient),
    Color(Premul),
    /// an SVG's text
    Svg(String),
}

#[derive(Debug, Clone, Copy)]
enum Size {
    Cover,
    Contain,
    /// a side left out is "auto"
    Sides(Option<Len>, Option<Len>),
}

impl Size {
    /// The tile in a box; `shape`: the picture's width for its height, when it has one.
    fn tile(self, w: f32, h: f32, shape: Option<f32>) -> (f32, f32) {
        match (self, shape) {
            (Size::Sides(Some(tw), Some(th)), _) => (tw.of(w), th.of(h)),
            (Size::Sides(Some(tw), None), Some(r)) => (tw.of(w), tw.of(w) / r),
            (Size::Sides(None, Some(th)), Some(r)) => (th.of(h) * r, th.of(h)),
            (Size::Sides(tw, th), None) => (tw.map_or(w, |l| l.of(w)), th.map_or(h, |l| l.of(h))),
            (Size::Cover, Some(r)) if w / h > r => (w, w / r),
            (Size::Contain | Size::Sides(None, None), Some(r)) if w / h <= r => (w, w / r),
            (_, Some(r)) => (h * r, h),
            (_, None) => (w, h),
        }
    }
}

#[derive(Debug)]
struct Layer {
    source: Source,
    position: (Len, Len),
    size: Size,
    repeat: (bool, bool),
}

/// The parts of `text` between the `by`s that aren't inside brackets or quotes.
fn split(text: &str, by: char) -> Vec<&str> {
    let (mut depth, mut quote, mut start, mut out) = (0, None, 0, Vec::new());
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, _) if c == by && depth == 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

fn number(text: &str) -> Option<f32> {
    text.trim().parse().ok()
}

fn length(text: &str) -> Option<Len> {
    let t = text.trim();
    if let Some(v) = t.strip_suffix('%') {
        return number(v).map(|v| Len::Part(v / 100.0));
    }
    if let Some(v) = t.strip_suffix("deg") {
        return number(v).map(|v| Len::Part(v / 360.0));
    }
    match t {
        "left" | "top" => Some(Len::Part(0.0)),
        "center" => Some(Len::Part(0.5)),
        "right" | "bottom" => Some(Len::Part(1.0)),
        _ => number(t.strip_suffix("px").unwrap_or(t)).map(Len::Px),
    }
}

fn hsl(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(360.0) / 30.0;
    let a = s * l.min(1.0 - l);
    [0.0, 8.0, 4.0].map(|n| {
        let k = (n + h) % 12.0;
        l - a * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    })
}

/// A CSS colour: "#rgb", "#rrggbb", "#rrggbbaa", rgb(), rgba(), hsl(), hsla() or "transparent".
pub fn color(text: &str) -> Option<[f32; 4]> {
    let t = text.trim();
    if t == "transparent" {
        return Some([0.0; 4]);
    }
    if let Some(hex) = t.strip_prefix('#') {
        let digits: Vec<f32> = hex.chars().map(|c| c.to_digit(16).map(|d| d as f32)).collect::<Option<_>>()?;
        return match digits.len() {
            3 => Some([digits[0] / 15.0, digits[1] / 15.0, digits[2] / 15.0, 1.0]),
            6 | 8 => {
                let byte = |i: usize| (digits[i] * 16.0 + digits[i + 1]) / 255.0;
                Some([byte(0), byte(2), byte(4), if digits.len() == 8 { byte(6) } else { 1.0 }])
            }
            _ => None,
        };
    }
    let (name, inner) = t.strip_suffix(')')?.split_once('(')?;
    let parts: Vec<f32> = inner.split(',').map(|p| number(p.trim().trim_end_matches('%'))).collect::<Option<_>>()?;
    let alpha = parts.get(3).copied().unwrap_or(1.0).clamp(0.0, 1.0);
    match (name.trim(), parts.len()) {
        ("rgb" | "rgba", 3 | 4) => Some([parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0, alpha]),
        ("hsl" | "hsla", 3 | 4) => {
            let [r, g, b] = hsl(parts[0], (parts[1] / 100.0).clamp(0.0, 1.0), (parts[2] / 100.0).clamp(0.0, 1.0));
            Some([r, g, b, alpha])
        }
        _ => None,
    }
}

fn premul(c: [f32; 4]) -> Premul {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// A colour stop: a colour, then no, one or two places (two: the colour holds from the one to the other).
fn stop(text: &str, into: &mut Vec<(Premul, Option<Len>)>) -> Option<()> {
    let t = text.trim();
    let cut = match t.find('(') {
        Some(open) if !t[..open].contains(' ') => t.find(')')? + 1,
        _ => t.find(' ').unwrap_or(t.len()),
    };
    let c = premul(color(&t[..cut])?);
    let places: Vec<Len> = t[cut..].split_whitespace().map(length).collect::<Option<_>>()?;
    match places.as_slice() {
        [] => into.push((c, None)),
        some => into.extend(some.iter().map(|p| (c, Some(*p)))),
    }
    Some(())
}

fn place(words: &[&str]) -> Option<(Len, Len)> {
    match words {
        [] => Some((Len::Part(0.5), Len::Part(0.5))),
        [one] => Some((length(one)?, Len::Part(0.5))),
        [x, y] => Some((length(x)?, length(y)?)),
        _ => None,
    }
}

fn gradient(name: &str, inner: &str) -> Option<Gradient> {
    let parts = split(inner, ',');
    let first: Vec<&str> = parts.first()?.split_whitespace().collect();
    let at = |words: &[&str]| -> Option<(usize, (Len, Len))> {
        match words.iter().position(|w| *w == "at") {
            Some(i) => Some((i, place(&words[i + 1..])?)),
            None => Some((words.len(), (Len::Part(0.5), Len::Part(0.5)))),
        }
    };
    let (shape, skip) = match name.trim_start_matches("repeating-") {
        "linear-gradient" => match first.as_slice() {
            [angle] if angle.ends_with("deg") => (Shape::Linear(number(angle.trim_end_matches("deg"))?.to_radians()), 1),
            ["to", side] => (
                Shape::Linear(
                    match *side {
                        "top" => 0.0f32,
                        "right" => 90.0,
                        "bottom" => 180.0,
                        "left" => 270.0,
                        _ => return None,
                    }
                    .to_radians(),
                ),
                1,
            ),
            _ => (Shape::Linear(180f32.to_radians()), 0),
        },
        "radial-gradient" => {
            let own = matches!(first.first(), Some(w) if ["ellipse", "circle", "at", "closest-side", "farthest-side", "closest-corner", "farthest-corner"].contains(w));
            if own {
                let (end, at) = at(&first)?;
                let mut circle = false;
                let mut extent = Extent::FarthestCorner;
                let sizes: Vec<&str> = first[..end].iter().copied().filter(|w| !matches!(*w, "ellipse" | "circle")).collect();
                circle |= first[..end].contains(&"circle");
                match sizes.as_slice() {
                    [] => {}
                    ["closest-side"] => extent = Extent::ClosestSide,
                    ["farthest-side"] => extent = Extent::FarthestSide,
                    ["closest-corner"] => extent = Extent::ClosestCorner,
                    ["farthest-corner"] => extent = Extent::FarthestCorner,
                    [rx, ry] => extent = Extent::Radii(length(rx)?, length(ry)?),
                    _ => return None,
                }
                (Shape::Radial { circle, extent, at }, 1)
            } else {
                (Shape::Radial { circle: false, extent: Extent::FarthestCorner, at: (Len::Part(0.5), Len::Part(0.5)) }, 0)
            }
        }
        "conic-gradient" => match first.as_slice() {
            ["from", angle, rest @ ..] => (Shape::Conic { from: number(angle.trim_end_matches("deg"))? / 360.0, at: at(rest)?.1 }, 1),
            ["at", ..] => (Shape::Conic { from: 0.0, at: at(&first)?.1 }, 1),
            _ => (Shape::Conic { from: 0.0, at: (Len::Part(0.5), Len::Part(0.5)) }, 0),
        },
        _ => return None,
    };
    let mut stops = Vec::new();
    for p in &parts[skip..] {
        stop(p, &mut stops)?;
    }
    (stops.len() >= 2).then_some(Gradient { shape, stops, repeating: name.starts_with("repeating-") })
}

fn layer(text: &str) -> Option<Layer> {
    let t = text.trim();
    if let Some(c) = color(t) {
        return Some(Layer { source: Source::Color(premul(c)), position: (Len::Px(0.0), Len::Px(0.0)), size: Size::Sides(None, None), repeat: (true, true) });
    }
    let open = t.find('(')?;
    let mut depth = 0;
    let close = t.char_indices().find_map(|(i, c)| {
        depth += (c == '(') as i32 - (c == ')') as i32;
        (depth == 0 && c == ')').then_some(i)
    })?;
    let source = match &t[..open] {
        "url" => Source::Svg(unescaped(t[open + 1..close].trim_matches(['"', '\'']).strip_prefix("data:image/svg+xml,")?)),
        name => Source::Gradient(gradient(name, &t[open + 1..close])?),
    };
    let mut repeat = (true, true);
    let rest: String = t[close + 1..]
        .split_whitespace()
        .filter(|w| match *w {
            "no-repeat" => {
                repeat = (false, false);
                false
            }
            "repeat-x" => {
                repeat = (true, false);
                false
            }
            "repeat-y" => {
                repeat = (false, true);
                false
            }
            "repeat" => false,
            _ => true,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let (position, size) = match rest.split_once('/') {
        Some((p, s)) => (p, Some(s)),
        None => (rest.as_str(), None),
    };
    let position = match position.split_whitespace().collect::<Vec<_>>().as_slice() {
        [] => (Len::Px(0.0), Len::Px(0.0)),
        words => place(words)?,
    };
    let side = |word: &str| if word == "auto" { Some(None) } else { length(word).map(Some) };
    let size = match size.map(|s| s.split_whitespace().collect::<Vec<_>>()).as_deref() {
        None => Size::Sides(None, None),
        Some(["cover"]) => Size::Cover,
        Some(["contain"]) => Size::Contain,
        Some([w]) => Size::Sides(side(w)?, None),
        Some([w, h]) => Size::Sides(side(w)?, side(h)?),
        _ => return None,
    };
    Some(Layer { source, position, size, repeat })
}

/// A gradient worked out for the box it fills: where a point falls along it, and the colour there.
struct Ramp {
    stops: Vec<(f32, Premul)>,
    repeating: bool,
    along: Along,
}

enum Along {
    Line { dx: f32, dy: f32, cx: f32, cy: f32, length: f32 },
    Rings { cx: f32, cy: f32, rx: f32, ry: f32 },
    Turn { cx: f32, cy: f32, from: f32 },
}

impl Ramp {
    fn new(g: &Gradient, w: f32, h: f32) -> Ramp {
        let (along, length) = match &g.shape {
            Shape::Linear(angle) => {
                let (dx, dy) = (angle.sin(), -angle.cos());
                let length = (w * dx).abs() + (h * dy).abs();
                (Along::Line { dx, dy, cx: w / 2.0, cy: h / 2.0, length }, length)
            }
            Shape::Radial { circle, extent, at } => {
                let (cx, cy) = (at.0.of(w), at.1.of(h));
                let (near, far) = ((cx.abs().min((w - cx).abs()), cy.abs().min((h - cy).abs())), (cx.abs().max((w - cx).abs()), cy.abs().max((h - cy).abs())));
                // an ellipse through a corner keeps the shape of the one that touches the sides
                let through = |side: (f32, f32), corner: (f32, f32)| {
                    let k = (corner.0 / side.0).hypot(corner.1 / side.1);
                    (side.0 * k, side.1 * k)
                };
                let (rx, ry) = match (extent, circle) {
                    (Extent::Radii(rx, ry), _) => (rx.of(w), ry.of(h)),
                    (Extent::ClosestSide, false) => near,
                    (Extent::FarthestSide, false) => far,
                    (Extent::ClosestCorner, false) => through(near, near),
                    (Extent::FarthestCorner, false) => through(far, far),
                    (Extent::ClosestSide, true) => (near.0.min(near.1), near.0.min(near.1)),
                    (Extent::FarthestSide, true) => (far.0.max(far.1), far.0.max(far.1)),
                    (Extent::ClosestCorner, true) => (near.0.hypot(near.1), near.0.hypot(near.1)),
                    (Extent::FarthestCorner, true) => (far.0.hypot(far.1), far.0.hypot(far.1)),
                };
                (Along::Rings { cx, cy, rx: rx.max(1e-6), ry: ry.max(1e-6) }, rx)
            }
            Shape::Conic { from, at } => (Along::Turn { cx: at.0.of(w), cy: at.1.of(h), from: *from }, 1.0),
        };
        // a stop with no place sits evenly between its neighbours; none comes before the one ahead of it
        let n = g.stops.len();
        let mut at: Vec<Option<f32>> = g.stops.iter().map(|(_, p)| p.map(|l| l.of(length) / length.max(1e-6))).collect();
        at[0] = at[0].or(Some(0.0));
        at[n - 1] = at[n - 1].or(Some(1.0));
        let mut most = f32::MIN;
        for a in at.iter_mut().flatten() {
            *a = a.max(most);
            most = *a;
        }
        let mut i = 1;
        while i < n {
            if at[i].is_some() {
                i += 1;
                continue;
            }
            let j = (i..n).find(|&k| at[k].is_some()).unwrap_or(n - 1);
            let (a, b) = (at[i - 1].unwrap_or(0.0), at[j].unwrap_or(1.0));
            for (n, place) in at[i..j].iter_mut().enumerate() {
                *place = Some(a + (b - a) * (n + 1) as f32 / (j - i + 1) as f32);
            }
            i = j;
        }
        Ramp { stops: at.into_iter().zip(&g.stops).map(|(a, (c, _))| (a.unwrap_or(0.0), *c)).collect(), repeating: g.repeating, along }
    }

    fn at(&self, x: f32, y: f32) -> Premul {
        let t = match self.along {
            Along::Line { dx, dy, cx, cy, length } => ((x - cx) * dx + (y - cy) * dy) / length + 0.5,
            Along::Rings { cx, cy, rx, ry } => ((x - cx) / rx).hypot((y - cy) / ry),
            Along::Turn { cx, cy, from } => ((x - cx).atan2(cy - y) / std::f32::consts::TAU - from).rem_euclid(1.0),
        };
        let (first, last) = (self.stops[0].0, self.stops[self.stops.len() - 1].0);
        let t = if self.repeating && last > first { first + (t - first).rem_euclid(last - first) } else { t };
        // a point right on a hard stop keeps the colour before it, as Chromium paints it
        let Some(i) = self.stops.iter().rposition(|s| s.0 < t - 1e-5) else { return self.stops[0].1 };
        let Some(next) = self.stops.get(i + 1) else { return self.stops[i].1 };
        let (a, b) = (self.stops[i], *next);
        let k = ((t - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
        std::array::from_fn(|c| a.1[c] + (b.1[c] - a.1[c]) * k)
    }
}

/// A URL's text with its %xx escapes read.
fn unescaped(text: &str) -> String {
    let (bytes, mut out, mut i) = (text.as_bytes(), Vec::with_capacity(text.len()), 0);
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())).flatten();
        out.push(hex.unwrap_or(bytes[i]));
        i += if hex.is_some() { 3 } else { 1 };
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What an attribute of an SVG's first tag says.
fn attribute<'a>(svg: &'a str, name: &str) -> Option<&'a str> {
    let open = svg.find("<svg")?;
    let tag = &svg[open..open + svg[open..].find('>')?];
    let from = tag.find(&format!("{name}="))? + name.len() + 1;
    let quote = tag[from..].chars().next().filter(|q| *q == '\'' || *q == '"')?;
    Some(&tag[from + 1..from + 1 + tag[from + 1..].find(quote)?])
}

fn view_box(svg: &str) -> Option<[f32; 4]> {
    let v: Vec<f32> = attribute(svg, "viewBox")?.split_whitespace().map(number).collect::<Option<_>>()?;
    <[f32; 4]>::try_from(v).ok().filter(|v| v[2] > 0.0 && v[3] > 0.0)
}

/// Pictures drawn already, by their SVG and size.
type Kept = HashMap<(String, u32, u32), Arc<Pixmap>>;
static DRAWN: LazyLock<Mutex<Kept>> = LazyLock::new(Default::default);

/// An SVG drawn to fill a tile of `w` × `h` pixels, `scale` of them to a CSS pixel; kept, as a backdrop is painted
/// again at every step along the shelf.
fn drawn(svg: &str, w: u32, h: u32, scale: f32) -> Option<Arc<Pixmap>> {
    let key = (svg.to_string(), w, h);
    if let Some(p) = DRAWN.lock().ok()?.get(&key) {
        return Some(p.clone());
    }
    let [vx, vy, vw, vh] = view_box(svg)?;
    let (mut sx, mut sy) = (w as f32 / vw, h as f32 / vh);
    let (mut tx, mut ty) = (0.0, 0.0);
    let fit = attribute(svg, "preserveAspectRatio").unwrap_or("xMidYMid meet");
    if fit != "none" {
        let k = if fit.ends_with("slice") { sx.max(sy) } else { sx.min(sy) };
        let spare = |room: u32, used: f32, min: &str, max: &str| if fit.contains(min) { 0.0 } else if fit.contains(max) { room as f32 - used } else { (room as f32 - used) / 2.0 };
        (tx, ty) = (spare(w, vw * k, "xMin", "xMax"), spare(h, vh * k, "YMin", "YMax"));
        (sx, sy) = (k, k);
    }
    // resvg scales every stroke: one asked to keep its width is given the width that comes out the same
    let mut text = svg.to_string();
    let fixed = " vector-effect='non-scaling-stroke'";
    while let Some(at) = text.find(fixed) {
        text.replace_range(at..at + fixed.len(), "");
        let start = text[..at].rfind('<').unwrap_or(0);
        if let Some(found) = text[start..at].find("stroke-width='") {
            let from = start + found + 14;
            let to = from + text[from..].find('\'').unwrap_or(0);
            if let Some(width) = number(&text[from..to]) {
                text.replace_range(from..to, &format!("{}", width * scale / (sx * sy).sqrt()));
            }
        }
    }
    let tree = usvg::Tree::from_str(&text, &usvg::Options::default()).ok()?;
    let mut pixmap = Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(&tree, Transform::from_row(sx, 0.0, 0.0, sy, tx - vx * sx, ty - vy * sy), &mut pixmap.as_mut());
    let pixmap = Arc::new(pixmap);
    let mut kept = DRAWN.lock().ok()?;
    if kept.len() >= 24 {
        kept.clear();
    }
    kept.insert(key, pixmap.clone());
    Some(pixmap)
}

enum Fill {
    Flat(Premul),
    Ramp(Ramp),
    /// a picture, and how many of its pixels go to a CSS pixel
    Drawn(Arc<Pixmap>, f32),
}

/// A layer worked out for the box: its tile, and what fills the tile.
struct Tile {
    fill: Fill,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    repeat: (bool, bool),
}

impl Tile {
    fn new(l: &Layer, w: f32, h: f32, scale: f32) -> Option<Tile> {
        let shape = match &l.source {
            Source::Svg(svg) => view_box(svg).map(|v| v[2] / v[3]),
            _ => None,
        };
        let (tw, th) = l.size.tile(w, h, shape);
        let offset = |p: Len, room: f32| match p {
            Len::Px(v) => v,
            Len::Part(k) => room * k,
        };
        let (x, y) = (offset(l.position.0, w - tw), offset(l.position.1, h - th));
        // a picture is drawn on whole pixels, as Chromium places it
        let whole = |v: f32| (v * scale).round() / scale;
        Some(match &l.source {
            Source::Gradient(g) => Tile { fill: Fill::Ramp(Ramp::new(g, tw, th)), x, y, w: tw, h: th, repeat: l.repeat },
            Source::Color(c) => Tile { fill: Fill::Flat(*c), x, y, w: tw, h: th, repeat: l.repeat },
            Source::Svg(svg) => {
                let (tw, th) = (whole(tw).max(1.0 / scale), whole(th).max(1.0 / scale));
                Tile { fill: Fill::Drawn(drawn(svg, (tw * scale).round() as u32, (th * scale).round() as u32, scale)?, scale), x: whole(x), y: whole(y), w: tw, h: th, repeat: l.repeat }
            }
        })
    }

    fn at(&self, x: f32, y: f32) -> Premul {
        if let Fill::Flat(c) = &self.fill {
            return *c;
        }
        let (mut u, mut v) = (x - self.x, y - self.y);
        if self.repeat.0 {
            u = u.rem_euclid(self.w);
        }
        if self.repeat.1 {
            v = v.rem_euclid(self.h);
        }
        if u < 0.0 || v < 0.0 || u >= self.w || v >= self.h {
            return [0.0; 4];
        }
        match &self.fill {
            Fill::Flat(c) => *c,
            Fill::Ramp(ramp) => ramp.at(u, v),
            Fill::Drawn(p, scale) => {
                // between the four pixels round the point
                let (fx, fy) = ((u * scale - 0.5).max(0.0), (v * scale - 0.5).max(0.0));
                let (x0, y0) = ((fx as u32).min(p.width() - 1), (fy as u32).min(p.height() - 1));
                let (x1, y1) = ((x0 + 1).min(p.width() - 1), (y0 + 1).min(p.height() - 1));
                let (kx, ky) = (fx - x0 as f32, fy - y0 as f32);
                let px = |x: u32, y: u32| {
                    let c = p.pixels()[(y * p.width() + x) as usize];
                    [c.red(), c.green(), c.blue(), c.alpha()].map(|b| f32::from(b) / 255.0)
                };
                let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
                std::array::from_fn(|i| (a[i] * (1.0 - kx) + b[i] * kx) * (1.0 - ky) + (c[i] * (1.0 - kx) + d[i] * kx) * ky)
            }
        }
    }
}

/// An SVG picture drawn `width` × `height` pixels, fitted as it asks to be (covering them, by default inside them).
pub fn svg(text: &str, width: u32, height: u32) -> Option<Picture> {
    let p = drawn(text, width.max(1), height.max(1), 1.0)?;
    let rgba = p.pixels().iter().flat_map(|px| { let d = px.demultiply(); [d.red(), d.green(), d.blue(), d.alpha()] }).collect();
    Some(Picture { width: p.width(), height: p.height(), rgba })
}

/// A picture being painted: backgrounds go onto it one over another, blended as a browser blends them.
pub struct Canvas {
    width: u32,
    height: u32,
    /// pixels to a CSS pixel
    scale: f32,
    px: Vec<Premul>,
}

impl Canvas {
    /// A clear canvas of `width` × `height` pixels, `scale` of them to a CSS pixel.
    pub fn new(width: u32, height: u32, scale: f32) -> Canvas {
        Canvas { width, height, scale, px: vec![[0.0; 4]; (width * height) as usize] }
    }

    /// Paints a background over what's there, into the box `(x, y, w, h)` in CSS pixels, `opacity` strong. A layer
    /// that can't be read is left out.
    pub fn paint(&mut self, css: &str, (x, y, w, h): (f32, f32, f32, f32), opacity: f32) {
        let scale = self.scale;
        let tiles: Vec<Tile> = split(css, ',').into_iter().rev().filter_map(layer).filter_map(|l| Tile::new(&l, w, h, scale)).collect();
        if tiles.is_empty() || self.width == 0 || self.height == 0 {
            return;
        }
        let edge = |v: f32, most: u32| ((v * scale).round().max(0.0) as u32).min(most) as usize;
        let (left, right, top, bottom) = (edge(x, self.width), edge(x + w, self.width), edge(y, self.height), edge(y + h, self.height));
        if left >= right || top >= bottom {
            return;
        }
        let row = |py: usize, line: &mut [Premul]| {
            for (i, px) in line[left..right].iter_mut().enumerate() {
                let (cx, cy) = (((left + i) as f32 + 0.5) / scale - x, (py as f32 + 0.5) / scale - y);
                let mut sum = [0.0f32; 4];
                for t in &tiles {
                    let c = t.at(cx, cy);
                    if c[3] > 0.0 {
                        let keep = 1.0 - c[3];
                        sum = std::array::from_fn(|i| c[i] + sum[i] * keep);
                    }
                }
                let keep = 1.0 - sum[3] * opacity;
                *px = std::array::from_fn(|i| sum[i] * opacity + px[i] * keep);
            }
        };
        let stride = self.width as usize;
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let rows_each = (bottom - top).div_ceil(threads).max(1);
        std::thread::scope(|s| {
            for (i, band) in self.px[top * stride..bottom * stride].chunks_mut(stride * rows_each).enumerate() {
                let row = &row;
                s.spawn(move || {
                    for (j, line) in band.chunks_exact_mut(stride).enumerate() {
                        row(top + i * rows_each + j, line);
                    }
                });
            }
        });
    }

    /// Lays a small picture (`pw` × `ph`, red, green and blue a pixel) over what's there, stretched smoothly to
    /// cover the box `(x, y, w, h)` in CSS pixels, `opacity` strong.
    pub fn stretch(&mut self, rgb: &[u8], (pw, ph): (usize, usize), (x, y, w, h): (f32, f32, f32, f32), opacity: f32) {
        if rgb.len() < pw * ph * 3 || pw == 0 || ph == 0 {
            return;
        }
        // as much of the picture as has the box's shape, from its middle
        let k = (w / pw as f32).max(h / ph as f32);
        let (left, top) = ((pw as f32 - w / k) / 2.0, (ph as f32 - h / k) / 2.0);
        let px = |ix: usize, iy: usize, c: usize| f32::from(rgb[(iy.min(ph - 1) * pw + ix.min(pw - 1)) * 3 + c]) / 255.0;
        let scale = self.scale;
        for (i, out) in self.px.iter_mut().enumerate() {
            let (cx, cy) = (((i % self.width as usize) as f32 + 0.5) / scale, ((i / self.width as usize) as f32 + 0.5) / scale);
            let (u, v) = ((left + (cx - x) / k - 0.5).max(0.0), (top + (cy - y) / k - 0.5).max(0.0));
            let (ix, iy, fx, fy) = (u as usize, v as usize, u.fract(), v.fract());
            for (c, under) in out.iter_mut().take(3).enumerate() {
                let over = (px(ix, iy, c) * (1.0 - fx) + px(ix + 1, iy, c) * fx) * (1.0 - fy) + (px(ix, iy + 1, c) * (1.0 - fx) + px(ix + 1, iy + 1, c) * fx) * fy;
                *under = over * opacity + *under * (1.0 - opacity);
            }
            out[3] = opacity + out[3] * (1.0 - opacity);
        }
    }

    pub fn picture(&self) -> Picture {
        let byte = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        let rgba = self.px.iter().flat_map(|c| if c[3] > 0.0 { [byte(c[0] / c[3]), byte(c[1] / c[3]), byte(c[2] / c[3]), byte(c[3])] } else { [0; 4] }).collect();
        Picture { width: self.width, height: self.height, rgba }
    }
}

/// The background painted `width` × `height` pixels, `scale` pixels to a CSS pixel.
pub fn paint(css: &str, width: u32, height: u32, scale: f32) -> Picture {
    let mut c = Canvas::new(width, height, scale);
    c.paint(css, (0.0, 0.0, width as f32 / scale, height as f32 / scale), 1.0);
    c.picture()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_colours() {
        assert_eq!(color("#fff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(color("#000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(color("rgba(255,0,0,.5)"), Some([1.0, 0.0, 0.0, 0.5]));
        assert_eq!(color("transparent"), Some([0.0; 4]));
        let c = color("hsla(120,100%,50%,1)").unwrap();
        assert!(c[0] < 1e-6 && (c[1] - 1.0).abs() < 1e-6 && c[2] < 1e-6);
        assert_eq!(color("wood"), None);
    }

    #[test]
    fn paints_layers() {
        let p = paint("linear-gradient(90deg,#f00,#00f)", 100, 2, 1.0);
        assert_eq!(&p.rgba[..4], &[254, 0, 1, 255]);
        assert_eq!(&p.rgba[99 * 4..100 * 4], &[1, 0, 254, 255]);
        // hard stops, a tile, and a colour under it
        let p = paint("linear-gradient(90deg,#fff 0 50%,transparent 50%) 0 0/4px 4px,#000", 8, 1, 1.0);
        let reds: Vec<u8> = p.rgba.chunks(4).map(|c| c[0]).collect();
        assert_eq!(reds, [255, 255, 0, 0, 255, 255, 0, 0]);
        let p = paint("radial-gradient(circle closest-side at 50% 50%,#fff,#fff 50%,#000 50%)", 8, 8, 1.0);
        assert_eq!(p.rgba[(3 * 8 + 3) * 4], 255);
        assert_eq!(p.rgba[0], 0);
        // half the pixels, the same picture
        // one background over another, in a box of its own, half as strong
        let mut c = Canvas::new(4, 2, 1.0);
        c.paint("#000", (0.0, 0.0, 4.0, 2.0), 1.0);
        c.paint("linear-gradient(90deg,#fff,#fff)", (2.0, 1.0, 2.0, 1.0), 0.5);
        let p = c.picture();
        assert_eq!((p.rgba[0], p.rgba[2 * 4], p.rgba[(4 + 1) * 4], p.rgba[(4 + 2) * 4]), (0, 0, 0, 128));
        let p = paint("linear-gradient(90deg,#fff 0 50%,#000 50%)", 4, 1, 0.5);
        assert_eq!(p.rgba.chunks(4).map(|c| c[0]).collect::<Vec<_>>(), [255, 255, 0, 0]);
        // a picture, stretched over the right half
        let svg = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 2 2' preserveAspectRatio='none'%3E%3Crect width='2' height='1' fill='%23ff0000'/%3E%3C/svg%3E\")";
        let p = paint(&format!("{svg} 100% 0/50% 100% no-repeat,#00f"), 8, 4, 1.0);
        assert_eq!(&p.rgba[..4], &[0, 0, 255, 255]);
        assert_eq!(&p.rgba[6 * 4..7 * 4], &[255, 0, 0, 255]);
        assert_eq!(&p.rgba[(3 * 8 + 6) * 4..(3 * 8 + 7) * 4], &[0, 0, 255, 255]);
    }
}

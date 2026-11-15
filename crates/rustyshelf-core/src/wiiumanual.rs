// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/wiiumanual.js, as RustyShelf has it.
//! Wii U e-manuals → page pictures for the manual reader. Every Wii U game carries its manual as meta/Manual.bfma: a
//! SARC of zlib-packed SARCs. Per language there are page layouts (BFLYT: background, content layers, a text layer),
//! laid-out text lines and photos (JPEG); textures (BFLIM) are shared. Index.bflyt holds the page titles and chapters.
//! This reads all that into a page model — drawing ops in page pixels, y down — that `wiiumanual_draw` draws, and
//! saves the pages as `<game>-manual-NN.jpg` beside the cover. Caches and markers in `data/wiiu-manuals` are the
//! Electron app's.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use regex::Regex;

use crate::bflyt::{self, Layout, Rgba, UserValue};
use crate::gx2;

/// what the `.failed` and `.made` markers hold: a manual that failed is tried again by another version
pub const VERSION: &str = concat!("rust-", env!("CARGO_PKG_VERSION"));

// ---------- SARC ----------

/// A SARC's files: "SARC" header (its byte-order mark says the endianness), the SFAT node table, the SFNT names.
pub fn sarc_list(buf: &[u8]) -> Option<Vec<(String, &[u8])>> {
    if buf.len() < 0x20 || &buf[..4] != b"SARC" {
        return None;
    }
    let le = u16::from_be_bytes([buf[6], buf[7]]) == 0xFFFE;
    let u16_at = |o: usize| buf.get(o..o + 2).map(|b| if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) });
    let u32_at = |o: usize| buf.get(o..o + 4).map(|b| if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } as usize);
    let (sfat, data_off) = (usize::from(u16_at(4)?), u32_at(12)?);
    if buf.get(sfat..sfat + 4) != Some(b"SFAT") {
        return None;
    }
    let count = usize::from(u16_at(sfat + 6)?);
    let nodes = sfat + 12;
    let names = nodes + count * 16 + 8;
    let mut out = Vec::new();
    for i in 0..count {
        let o = nodes + i * 16;
        let (attr, start, end) = (u32_at(o + 4)?, u32_at(o + 8)?, u32_at(o + 12)?);
        let mut name = String::new();
        if attr & 0x0100_0000 != 0 {
            let no = (names + (attr & 0xFFFF) * 4).min(buf.len());
            let e = buf[no..].iter().position(|&c| c == 0).map_or(buf.len(), |p| no + p);
            name = String::from_utf8_lossy(&buf[no..e]).into_owned();
        }
        let (a, b) = ((data_off + start).min(buf.len()), (data_off + end).min(buf.len()));
        out.push((name, &buf[a..b.max(a)]));
    }
    Some(out)
}

// untrusted input: a real .arc is a few MB
const MAX_ARC: u64 = 128 << 20;

/// A SARC's files by name without folders, in the order the SARC first has each name.
#[derive(Default)]
struct Files {
    order: Vec<String>,
    map: HashMap<String, Vec<u8>>,
}

impl Files {
    fn get(&self, name: &str) -> Option<&[u8]> {
        self.map.get(name).map(Vec::as_slice)
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &[u8])> {
        self.order.iter().map(|n| (n, self.map[n].as_slice()))
    }
}

/// An .arc member: a zlib-compressed SARC (some tools leave them raw).
fn arc(data: &[u8]) -> Result<Files, String> {
    let unpacked;
    let d = if data.first() == Some(&0x78) {
        let mut out = Vec::new();
        if flate2::read::ZlibDecoder::new(data).take(MAX_ARC + 1).read_to_end(&mut out).is_err() {
            return Ok(Files::default());
        }
        if out.len() as u64 > MAX_ARC {
            return Err(format!("manual part unpacks past {} MB", MAX_ARC >> 20));
        }
        unpacked = out;
        &unpacked[..]
    } else {
        data
    };
    let mut f = Files::default();
    for (n, b) in sarc_list(d).unwrap_or_default() {
        let n = n.rsplit('/').next().unwrap_or("").to_string();
        if f.map.insert(n.clone(), b.to_vec()).is_none() {
            f.order.push(n);
        }
    }
    Ok(f)
}

/// The manual inside a .wua: in the game's own folder (0005000 0…), else the first folder.
pub fn from_wua(file: &Path) -> Result<Option<Vec<u8>>, String> {
    let mut z = crate::zarchive::ZArchive::open(file)?;
    static BASE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^00050000[0-9a-f]{8}").unwrap());
    let root = z.root()?;
    let Some(base) = root.iter().find(|n| !n.file && BASE.is_match(&n.name)).or_else(|| root.iter().find(|n| !n.file)) else { return Ok(None) };
    match z.lookup(&format!("{}/meta/Manual.bfma", base.name)) {
        Some(node) if node.file => z.read(&node).map(Some),
        _ => Ok(None),
    }
}

/// An unpacked game's manual: <game>/meta/Manual.bfma beside <game>/code/<name>.rpx.
fn from_folder(rpx: &Path) -> Option<Vec<u8>> {
    fs::read(rpx.parent()?.parent()?.join("meta").join("Manual.bfma")).ok()
}

// ---------- text ----------

#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    pub x: f64,
    pub y: f64,
    pub sx: f64,
    pub sy: f64,
    /// letter spacing: a justified line has some
    pub sp: f64,
    pub color: [u8; 3],
    pub s: String,
}

/// A textset file: one record a line — u16 1, u16 0, u16 letters, f32 x, y (from the text box), size x, size y,
/// letter spacing, u8 r g b and 5 more bytes, then the UTF-16BE text.
pub fn text_lines(b: &[u8]) -> Vec<TextLine> {
    let be16 = |o: usize| usize::from(u16::from_be_bytes([b[o], b[o + 1]]));
    let f = |o: usize| f64::from(f32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]));
    let (mut out, mut o) = (Vec::new(), 0);
    while o + 34 <= b.len() && be16(o) == 1 {
        let n = be16(o + 4);
        if o + 34 + n * 2 > b.len() {
            break;
        }
        let units: Vec<u16> = b[o + 34..o + 34 + n * 2].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        out.push(TextLine { x: f(o + 6), y: f(o + 10), sx: f(o + 14), sy: f(o + 18), sp: f(o + 22), color: [b[o + 26], b[o + 27], b[o + 28]], s: String::from_utf16_lossy(&units) });
        o += 34 + n * 2;
    }
    out
}

// ---------- layout → ops ----------

#[derive(Debug, Clone, PartialEq)]
pub enum Colors {
    One(Rgba),
    /// the corners: top left, top right, bottom left, bottom right
    Four([Rgba; 4]),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Op {
    /// a picture's name in `Model::images`; None for a plain (or shaded) rectangle
    pub img: Option<String>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
    /// 1: the texture repeats
    pub ws: u8,
    pub wt: u8,
    /// the material's black and white colours
    pub tint: Option<(Rgba, Rgba)>,
    pub c: Colors,
    pub a: f64,
    pub photo: bool,
    /// from the background layer
    pub bg: bool,
}

#[derive(Debug, Clone)]
struct TextBox {
    x: f64,
    y: f64,
    w: f64,
}

const ORIGIN: [f64; 3] = [0.5, 0.0, 1.0];

fn origin(k: u8) -> f64 {
    ORIGIN.get(usize::from(k & 3)).copied().unwrap_or(f64::NAN)
}

fn colors_of(p: &bflyt::Pane) -> Colors {
    match p.colors {
        Some(c) if c.iter().all(|x| *x == c[0]) => Colors::One(c[0]),
        Some(c) => Colors::Four(c),
        None => Colors::One([255; 4]),
    }
}

fn tint_of(m: Option<&bflyt::Material>) -> Option<(Rgba, Rgba)> {
    m.filter(|m| m.black.iter().any(|&v| v != 0) || m.white.iter().any(|&v| v != 255)).map(|m| (m.black, m.white))
}

#[derive(Debug, Clone)]
pub enum Image {
    Texture(gx2::Texture),
    Jpeg(Vec<u8>),
}

/// The pictures a manual's pages use, decoded the first time a page asks.
struct Assets<'a> {
    textures: &'a HashMap<String, Vec<u8>>,
    jpegs: [&'a Files; 2],
    images: HashMap<String, Image>,
    sizes: HashMap<String, (f64, f64)>,
}

impl Assets<'_> {
    fn use_texture(&mut self, name: &str) -> Option<String> {
        if self.images.contains_key(name) {
            return Some(name.to_string());
        }
        let t = gx2::read_flim(self.textures.get(name)?).ok()??;
        self.sizes.insert(name.to_string(), (t.width as f64, t.height as f64));
        self.images.insert(name.to_string(), Image::Texture(t));
        Some(name.to_string())
    }

    fn use_jpeg(&mut self, name: &str) -> Option<String> {
        if self.images.contains_key(name) {
            return Some(name.to_string());
        }
        let alt = format!("{}.jpg", name.strip_suffix(".jpg").or_else(|| name.strip_suffix(".JPG")).unwrap_or(name));
        let b = self.jpegs.iter().find_map(|m| m.get(name).or_else(|| m.get(&alt)))?.to_vec();
        let (w, h) = jpeg_size(&b);
        self.sizes.insert(name.to_string(), (f64::from(w), f64::from(h)));
        self.images.insert(name.to_string(), Image::Jpeg(b));
        Some(name.to_string())
    }
}

struct Tex {
    key: Option<String>,
    ws: u8,
    wt: u8,
    name: String,
}

/// A material's first texture, decoded (when there are `assets`).
fn tex_ref(l: &Layout, assets: &mut Option<&mut Assets>, m: Option<&bflyt::Material>) -> Option<Tex> {
    let t = m?.tex.first()?;
    let name = l.textures.get(usize::from(t.index))?;
    let key = assets.as_mut().and_then(|a| a.use_texture(name));
    Some(Tex { key, ws: t.wrap_s, wt: t.wrap_t, name: name.clone() })
}

/// A layer's drawing ops, and the text boxes it places into `boxes`. Without `assets` (the info layer) nothing is
/// looked up: only the boxes count.
fn flatten(l: &Layout, mut assets: Option<&mut Assets>, boxes: &mut HashMap<String, TextBox>) -> Vec<Op> {
    let mut ops = Vec::new();
    let Some(root) = l.root else { return ops };
    // (pane, its parent's position and scale, alpha, depth)
    let mut stack = vec![(root, 0.0, 0.0, 1.0, 1.0, 1.0, 0usize)];
    let mut order = Vec::new();
    while let Some((i, px, py, sx, sy, alpha, depth)) = stack.pop() {
        let p = l.pane(i);
        if !p.visible {
            continue;
        }
        let or1 = |v: f64| if v == 0.0 || v.is_nan() { 1.0 } else { v };
        let (wx, wy) = (px + p.x * sx, py - p.y * sy);
        let (ssx, ssy) = (sx * or1(p.sx), sy * or1(p.sy));
        let a = alpha * (f64::from(p.alpha) / 255.0);
        let (w, h) = (p.w * ssx, p.h * ssy);
        let (x, y) = (wx - w * origin(p.origin), wy - h * origin(p.origin >> 2));
        order.push((i, x, y, w, h, wx, wy, a, depth));
        for &c in p.children.iter().rev() {
            stack.push((c, wx, wy, ssx, ssy, a, depth + 1));
        }
    }
    for (i, x, y, w, h, wx, wy, a, depth) in order {
        let p = l.pane(i);
        if depth == 0 {
            continue;
        }
        let rect = |c: Colors, tint| Op { img: None, x, y, w, h, u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0, ws: 0, wt: 0, tint, c, a, photo: false, bg: false };
        match p.kind {
            "pic" => {
                let m = l.materials.get(usize::from(p.mat));
                let mut t = tex_ref(l, &mut assets, m);
                let jpg = p.user.iter().find_map(|u| match &u.value {
                    UserValue::Str(s) if u.name == "JpegFileName" => Some(s.clone()),
                    _ => None,
                });
                if let Some(j) = &jpg {
                    t = match assets.as_mut().and_then(|a| a.use_jpeg(j)) {
                        Some(k) => Some(Tex { key: Some(k), ws: 0, wt: 0, name: String::new() }),
                        None => continue,
                    };
                }
                let uv = p.uv.first().copied().unwrap_or([[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]);
                match t {
                    Some(Tex { key: Some(key), ws, wt, .. }) => ops.push(Op {
                        img: Some(key),
                        u0: uv[0][0],
                        v0: uv[0][1],
                        u1: uv[3][0],
                        v1: uv[3][1],
                        ws,
                        wt,
                        photo: jpg.is_some(),
                        ..rect(colors_of(p), if jpg.is_some() { None } else { tint_of(m) })
                    }),
                    _ => {
                        if m.is_none_or(|m| m.tex.is_empty()) {
                            ops.push(rect(colors_of(p), tint_of(m)));
                        }
                    }
                }
            }
            "wnd" => wnd(l, p, (x, y, w, h), a, &mut assets, &mut ops),
            "txt" => {
                boxes.insert(p.name.clone(), TextBox { x: wx, y: wy, w });
            }
            _ => {}
        }
    }
    ops
}

#[derive(Clone)]
struct FrameTex {
    key: String,
    flip: u8,
    tint: Option<(Rgba, Rgba)>,
    size: Option<(f64, f64)>,
}

fn wnd(
    l: &Layout,
    p: &bflyt::Pane,
    (x, y, w, h): (f64, f64, f64, f64),
    a: f64,
    assets: &mut Option<&mut Assets>,
    ops: &mut Vec<Op>,
) {
    let frames: Vec<Option<FrameTex>> = p
        .frames
        .iter()
        .map(|f| {
            let m = l.materials.get(usize::from(f.mat));
            let key = tex_ref(l, assets, m)?.key?;
            let size = assets.as_ref().and_then(|a| a.sizes.get(&key).copied());
            Some(FrameTex { key, flip: f.flip, tint: tint_of(m), size })
        })
        .collect();
    let f4: Vec<Option<FrameTex>> = match (frames.len(), frames.first()) {
        (1, Some(Some(one))) => [0, 1, 2, 4].iter().map(|&flip| Some(FrameTex { flip: if flip == 0 { one.flip } else { flip }, ..one.clone() })).collect(),
        (4, _) => frames,
        _ => Vec::new(),
    };
    let sz = |i: usize| f4.get(i).and_then(|f| f.as_ref()).and_then(|f| f.size).unwrap_or((0.0, 0.0));
    let (fl, ft, fr, fb) = if f4.is_empty() { (0.0, 0.0, 0.0, 0.0) } else { (sz(0).0, sz(0).1, sz(1).0, sz(3).1) };
    let inf = p.inflate.unwrap_or([0.0; 4]);
    let cm = l.materials.get(usize::from(p.mat));
    let (cx, cy, cw, ch) = (x + fl - inf[0], y + ft - inf[2], w - fl - fr + inf[0] + inf[1], h - ft - fb + inf[2] + inf[3]);
    let base = Op { img: None, x: cx, y: cy, w: cw, h: ch, u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0, ws: 0, wt: 0, tint: tint_of(cm), c: colors_of(p), a, photo: false, bg: false };
    if cw > 0.0 && ch > 0.0 {
        static DUMMY: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^DummyTexture").unwrap());
        match tex_ref(l, assets, cm) {
            Some(Tex { key: Some(key), ws, wt, name }) if !DUMMY.is_match(&name) => ops.push(Op { img: Some(key), ws, wt, ..base }),
            _ => ops.push(base),
        }
    }
    if f4.is_empty() || f4.iter().any(Option::is_none) {
        return;
    }
    let f4: Vec<FrameTex> = f4.into_iter().flatten().collect();
    // each frame covers its corner and the edge after it (clamped texture coordinates stretch the edge texels); the
    // anchors are in displayed-texture space, flips map back
    let mut piece = |f: &FrameTex, x: f64, y: f64, w: f64, h: f64, ax: bool, ay: bool| {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let (tw, th) = f.size.unwrap_or((0.0, 0.0));
        let (mut u0, mut u1, mut v0, mut v1) = (0.0, w / tw, 0.0, h / th);
        if ax {
            (u0, u1) = (1.0 - w / tw, 1.0);
        }
        if ay {
            (v0, v1) = (1.0 - h / th, 1.0);
        }
        if f.flip == 1 || f.flip == 4 {
            (u0, u1) = (1.0 - u0, 1.0 - u1);
        }
        if f.flip == 2 || f.flip == 4 {
            (v0, v1) = (1.0 - v0, 1.0 - v1);
        }
        ops.push(Op { img: Some(f.key.clone()), x, y, w, h, u0, v0, u1, v1, ws: 0, wt: 0, tint: f.tint, c: Colors::One([255; 4]), a, photo: false, bg: false });
    };
    piece(&f4[0], x, y, w - fr, ft, false, false);
    piece(&f4[1], x + w - fr, y, fr, h - fb, true, false);
    piece(&f4[3], x + fl, y + h - fb, w - fl, fb, true, true);
    piece(&f4[2], x, y + ft, fl, h - ft, false, true);
}

// ---------- the whole manual ----------

const PAGE_MIN: f64 = 640.0;
const PAGE_TARGET: f64 = 1000.0;
const PAGE_MAX: f64 = 1180.0;

fn js_round(v: f64) -> f64 {
    (v + 0.5).floor()
}

/// Where a page is cut into book pages: between content blocks, never through a picture, frame or line of text
/// when that can be helped. `keep`: the spans (top, bottom) not to cut through.
pub fn split_page(height: f64, keep: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut iv: Vec<(f64, f64)> = keep.iter().filter(|(a, b)| b - a < PAGE_MAX * 0.8).map(|&(a, b)| ((a - 6.0).max(0.0), (b + 6.0).min(height))).collect();
    iv.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (a, b) in iv {
        match merged.last_mut() {
            Some(m) if a <= m.1 => m.1 = m.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let mut gaps = Vec::new();
    let mut prev: f64 = 0.0;
    for &(a, b) in &merged {
        if a > prev {
            gaps.push((prev, a));
        }
        prev = prev.max(b);
    }
    if prev < height {
        gaps.push((prev, height));
    }
    let (mut cuts, mut s) = (Vec::new(), 0.0);
    while height - s > PAGE_MAX {
        let (mut best, mut best_d) = (None, f64::INFINITY);
        for &(a, b) in &gaps {
            let (lo, hi) = (a.max(s + PAGE_MIN), b.min(s + PAGE_MAX));
            if lo > hi {
                continue;
            }
            let c = hi.min(lo.max(s + PAGE_TARGET));
            let d = (c - (s + PAGE_TARGET)).abs();
            if d < best_d {
                (best_d, best) = (d, Some(c));
            }
        }
        let c = js_round(best.unwrap_or(s + PAGE_TARGET));
        cuts.push(c);
        s = c;
    }
    let mut out = Vec::new();
    let mut a = 0.0;
    for c in cuts {
        out.push((a, c));
        a = c;
    }
    out.push((a, height));
    // a tiny last piece joins the one before when that still fits
    let n = out.len();
    if n > 1 && out[n - 1].1 - out[n - 1].0 < 220.0 && out[n - 1].1 - out[n - 2].0 <= PAGE_MAX + 120.0 {
        let l = out.pop().unwrap_or_default();
        out[n - 2].1 = l.1;
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// the text box it's in, the box's left edge and width
    pub in_box: String,
    pub bx: f64,
    pub bw: f64,
    pub x: f64,
    pub y: f64,
    pub sx: f64,
    pub sy: f64,
    pub sp: f64,
    pub color: [u8; 3],
    pub s: String,
    /// the last line of its box
    pub last: bool,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub id: String,
    pub num: u64,
    pub width: f64,
    pub height: f64,
    pub title: String,
    pub chapter: String,
    pub ops: Vec<Op>,
    pub text: Vec<Line>,
    /// where it's cut into book pages: (top, bottom)
    pub parts: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    /// "" for the pages after the chapters
    pub name: String,
    /// indexes into `Model::pages`
    pub pages: Vec<usize>,
}

pub struct Model {
    pub title: String,
    pub lang: String,
    pub width: f64,
    pub pages: Vec<Page>,
    pub toc: Vec<Chapter>,
    pub images: HashMap<String, Image>,
    pub sizes: HashMap<String, (f64, f64)>,
}

/// The page model of a manual (a .bfma), in its American English, else any English, else its first language.
pub fn build(buf: &[u8], title: &str) -> Result<Model, String> {
    let top = sarc_list(buf).ok_or("not a Wii U manual")?;
    let mut files: HashMap<&str, &[u8]> = HashMap::new();
    let mut names: Vec<&str> = Vec::new();
    for (n, d) in &top {
        if files.insert(n, d).is_none() {
            names.push(n);
        }
    }
    static MIDDLE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^([A-Z]{3}_[a-z]{2})_middle\\.arc$").unwrap());
    let mut langs: Vec<String> = Vec::new();
    for (n, _) in &top {
        if let Some(c) = MIDDLE.captures(n) {
            if !langs.contains(&c[1].to_string()) {
                langs.push(c[1].to_string());
            }
        }
    }
    let want = langs.iter().find(|l| *l == "USA_en").or_else(|| langs.iter().find(|l| l.ends_with("_en"))).or(langs.first()).ok_or("no pages in this manual")?.clone();
    let get = |n: &str| -> Result<Files, String> { files.get(n).map_or(Ok(Files::default()), |d| arc(d)) };
    let middle = get(&format!("{want}_middle.arc"))?;
    let textset = get(&format!("{want}_textset.arc"))?;
    let titles = get(&format!("{want}_text_title.arc"))?;
    let index = get(&format!("{want}_index.arc"))?;
    let jpegs = [get(&format!("{want}_jpeg.arc"))?, get("Common_jpeg.arc")?];
    let mut textures = HashMap::new();
    for n in &names {
        if n.to_lowercase().ends_with("texture.arc") && (n.starts_with("Common") || n.starts_with(want.as_str())) {
            textures.extend(get(n)?.map);
        }
    }
    let mut assets = Assets { textures: &textures, jpegs: [&jpegs[0], &jpegs[1]], images: HashMap::new(), sizes: HashMap::new() };

    // the index: titles and chapters (any one of its layouts)
    let idx = index.iter().next().and_then(|(_, b)| bflyt::parse(b).ok());
    let mut page_title: HashMap<u64, String> = HashMap::new();
    let mut chapters: Vec<(String, bool, Vec<f64>)> = Vec::new();
    static PAGE_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^PageTitle_(\d+)$").unwrap());
    static CATEGORY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Category_(\d+)$").unwrap());
    if let Some(idx) = idx.as_ref() {
        for &c in idx.root.map(|r| idx.pane(r).children.as_slice()).unwrap_or(&[]) {
            let p = idx.pane(c);
            if let Some(m) = PAGE_TITLE.captures(&p.name) {
                if let Ok(n) = m[1].parse() {
                    page_title.insert(n, p.text.clone().unwrap_or_default());
                }
                continue;
            }
            if CATEGORY.is_match(&p.name) {
                let valid = p.user.iter().find(|u| u.name == "IsValid").is_none_or(|u| match &u.value {
                    UserValue::None => false,
                    UserValue::Str(s) => !s.is_empty(),
                    UserValue::Nums(v) => v.first() != Some(&0.0),
                });
                let ids = p.user.iter().filter(|u| u.name.starts_with("PageID_")).filter_map(|u| if let UserValue::Nums(v) = &u.value { v.first().copied() } else { None }).collect();
                chapters.push((p.text.clone().unwrap_or_default(), valid, ids));
            }
        }
    }

    static PAGE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Page_(\d+)_middle_").unwrap());
    let mut ids: Vec<String> = middle.order.iter().filter_map(|n| PAGE_ID.captures(n).map(|c| c[1].to_string())).collect::<HashSet<_>>().into_iter().collect();
    ids.sort();
    let mut pages = Vec::new();
    for id in &ids {
        let num: u64 = id.parse().unwrap_or(0);
        let pre = format!("Page_{id}_middle_");
        let mut numbered: Vec<String> = middle.order.iter().filter_map(|n| n.strip_prefix(&pre)).map(|n| n.strip_suffix(".bflyt").unwrap_or(n).to_string()).filter(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())).collect();
        numbered.sort_by(|a, b| a.parse::<f64>().unwrap_or(0.0).total_cmp(&b.parse::<f64>().unwrap_or(0.0)).then_with(|| a.cmp(b)));
        let order = std::iter::once("bg".to_string()).chain(numbered);
        let mut boxes = HashMap::new();
        let (mut width, mut height) = (688.0, 0.0f64);
        let (mut ops, mut keep) = (Vec::new(), Vec::new());
        for ln in order {
            let Some(b) = middle.get(&format!("{pre}{ln}.bflyt")) else { continue };
            let Ok(l) = bflyt::parse(b) else { continue };
            if l.width != 0.0 && !l.width.is_nan() {
                width = l.width;
            }
            height = height.max(if l.height.is_nan() { 0.0 } else { l.height });
            for mut op in flatten(&l, Some(&mut assets), &mut boxes) {
                op.bg = ln == "bg";
                if !op.bg {
                    keep.push((op.y, op.y + op.h));
                }
                ops.push(op);
            }
        }
        // text boxes the content layers don't have come from the info layer
        if let Some(l) = middle.get(&format!("{pre}info.bflyt")).and_then(|b| bflyt::parse(b).ok()) {
            let mut info = HashMap::new();
            flatten(&l, None, &mut info);
            for (k, v) in info {
                boxes.entry(k).or_insert(v);
            }
        }
        let mut text = Vec::new();
        let tpre = format!("Page_{id}_middle_textset_");
        for (n, data) in textset.iter() {
            let Some(name) = n.strip_prefix(&tpre).and_then(|r| r.strip_suffix(".bin")).filter(|r| !r.is_empty()) else { continue };
            let Some(bx) = boxes.get(name) else { continue };
            let lines = text_lines(data);
            let count = lines.len();
            for (i, l) in lines.into_iter().enumerate() {
                let y = bx.y - l.y;
                keep.push((y, y + l.sy));
                text.push(Line { in_box: name.to_string(), bx: bx.x, bw: bx.w, x: bx.x + l.x, y, sx: l.sx, sy: l.sy, sp: l.sp, color: l.color, s: l.s, last: i + 1 == count });
            }
        }
        let tt = (0..3).find_map(|k| titles.get(&format!("Page_{id}_title_text_TextBox_0{k}.bin")).map(bflyt::utf16be));
        let chapter = chapters.iter().find(|c| c.1 && c.2.contains(&(num as f64))).map(|c| c.0.clone()).unwrap_or_default();
        let title = page_title.get(&num).filter(|t| !t.is_empty()).cloned().or(tt.filter(|t| !t.is_empty())).unwrap_or_else(|| format!("Page {}", num + 1));
        let parts = split_page(height, &keep);
        pages.push(Page { id: id.clone(), num, width, height, title, chapter, ops, text, parts });
    }
    // legal and health pages ("Important Information") have no chapter: they go after the game's pages
    let (mut ordered, rest): (Vec<Page>, Vec<Page>) = pages.into_iter().partition(|p| !p.chapter.is_empty());
    ordered.extend(rest);
    let mut toc: Vec<Chapter> = chapters
        .iter()
        .filter(|c| c.1 && !c.2.is_empty())
        .map(|c| Chapter { name: c.0.clone(), pages: c.2.iter().filter_map(|&n| ordered.iter().position(|p| p.num as f64 == n)).collect() })
        .collect();
    let extra: Vec<usize> = ordered.iter().enumerate().filter(|(_, p)| p.chapter.is_empty()).map(|(i, _)| i).collect();
    if !extra.is_empty() {
        toc.push(Chapter { name: String::new(), pages: extra });
    }
    let (images, sizes) = (assets.images, assets.sizes);
    Ok(Model { title: title.to_string(), lang: want, width: 688.0, pages: ordered, toc, images, sizes })
}

/// A JPEG's width and height from its frame header; (0, 0) when there's none.
pub fn jpeg_size(b: &[u8]) -> (u16, u16) {
    let be = |o: usize| u16::from_be_bytes([b[o], b[o + 1]]);
    let mut o = 2;
    while o + 9 < b.len() {
        if b[o] != 0xFF {
            break;
        }
        let (m, len) = (b[o + 1], usize::from(be(o + 2)));
        if (0xC0..=0xCF).contains(&m) && ![0xC4, 0xC8, 0xCC].contains(&m) {
            return (be(o + 7), be(o + 5));
        }
        o += 2 + len;
    }
    (0, 0)
}

/// Whether the manual is an e-book (Super Mario Maker, Paper Mario Color Splash and other late games: XHTML pages
/// that need a browser to be laid out, which this app doesn't have).
pub fn is_epub(buf: &[u8]) -> bool {
    static EPUB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{3}_[a-z]{2}_Manual\.epub$").unwrap());
    sarc_list(buf).is_some_and(|top| top.iter().any(|(n, _)| EPUB.is_match(n)))
}

// ---------- the pages on disk ----------

fn safe(s: &str) -> String {
    static BAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*]+"#).unwrap());
    BAD.replace_all(s, "_").into_owned()
}

fn dir_of(data_dir: &Path) -> PathBuf {
    data_dir.join("wiiu-manuals")
}

fn read_bfma(file: &Path) -> Result<Option<Vec<u8>>, String> {
    let low = file.to_string_lossy().to_lowercase();
    if low.ends_with(".wua") {
        from_wua(file)
    } else if low.ends_with(".rpx") {
        Ok(from_folder(file))
    } else {
        Ok(None)
    }
}

/// A Wii U game whose manual may be made: where its manual pages go and the name they start with.
#[derive(Debug, Clone)]
pub struct Want {
    pub title: String,
    pub file: PathBuf,
    pub art_dir: PathBuf,
    pub base: String,
}

/// A manual's book pages as JPEGs.
pub type Render<'a> = dyn Fn(&Model) -> Result<Vec<Vec<u8>>, String> + 'a;

/// the e-book manuals already found this session (not read out of their game again)
static EBOOKS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn has_pages(names: &[String], base: &str) -> bool {
    let pre = format!("{base}-manual-").to_lowercase();
    names.iter().any(|f| {
        let l = f.to_lowercase();
        l.starts_with(&pre) && [".jpg", ".jpeg", ".png", ".webp"].iter().any(|e| l.ends_with(e))
    })
}

/// Every game here without manual pages gets its e-manual drawn and saved: `render` turns a model into JPEGs. A
/// manual that couldn't be made is tried again only by another version. The games whose pages were made.
pub fn collect(data_dir: &Path, games: &[Want], render: &Render) -> usize {
    let dir = dir_of(data_dir);
    let mut listed: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut made = 0;
    for g in games {
        if g.base.is_empty() || g.art_dir.as_os_str().is_empty() {
            continue;
        }
        let stem = safe(&g.title);
        let cache = dir.join(format!("{stem}.bfma"));
        let failed = dir.join(format!("{stem}.bfma.failed"));
        let tries = dir.join(format!("{stem}.bfma.tries"));
        let mark = dir.join(format!("{stem}.made"));
        let names = listed.entry(g.art_dir.clone()).or_insert_with(|| fs::read_dir(&g.art_dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect());
        // it has a manual already (or one was picked): a copy of the game's manual kept from before goes, a tiny
        // marker remembers the pages were made here
        if has_pages(names, &g.base) {
            if cache.is_file() && fs::write(&mark, VERSION).is_ok() {
                let _ = fs::remove_file(&cache);
            }
            continue;
        }
        if fs::read_to_string(&failed).is_ok_and(|v| v == VERSION) {
            let _ = fs::remove_file(&cache);
            continue;
        }
        if EBOOKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).contains(&g.title) {
            continue;
        }
        let fail = || {
            let _ = fs::create_dir_all(&dir).and_then(|()| fs::write(&failed, VERSION));
        };
        let run = || -> Result<Option<Vec<Vec<u8>>>, String> {
            let buf = match fs::read(&cache) {
                Ok(b) => b,
                Err(_) => {
                    let Some(b) = read_bfma(&g.file)? else {
                        // no e-manual in this game: its .wua isn't read again every time
                        let low = g.file.to_string_lossy().to_lowercase();
                        if low.ends_with(".wua") || low.ends_with(".rpx") {
                            fail();
                        }
                        return Ok(None);
                    };
                    if is_epub(&b) {
                        EBOOKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(g.title.clone());
                        return Ok(None);
                    }
                    let part = dir.join(format!("{stem}.bfma.part"));
                    let _ = fs::create_dir_all(&dir).and_then(|()| fs::write(&part, &b)).and_then(|()| fs::rename(&part, &cache));
                    b
                }
            };
            if is_epub(&buf) {
                EBOOKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(g.title.clone());
                return Ok(None);
            }
            let model = build(&buf, &g.title)?;
            render(&model).map(Some)
        };
        // the files are untrusted: a page that trips the drawing up fails this game's manual, not the upkeep
        let imgs = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
            Ok(Ok(Some(imgs))) => imgs,
            Ok(Ok(None)) => continue,
            Ok(Err(_)) | Err(_) => {
                fail();
                continue;
            }
        };
        if imgs.is_empty() {
            fail();
            continue;
        }
        match place(&g.art_dir, &g.base, &imgs) {
            Ok(files) => names.extend(files),
            Err(_) => {
                fail();
                continue;
            }
        }
        let _ = fs::remove_file(&failed);
        let _ = fs::remove_file(&tries);
        // the pages are saved: the manual's copy isn't needed
        if fs::write(&mark, VERSION).is_ok() {
            let _ = fs::remove_file(&cache);
        }
        made += 1;
    }
    made
}

/// The pages go in under hidden names and are renamed once all are written: a write failing half-way (a full disk)
/// would otherwise leave the first few, which would count as the game's whole manual.
fn place(dir: &Path, base: &str, imgs: &[Vec<u8>]) -> Result<Vec<String>, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let pid = std::process::id();
    let tmps: Vec<PathBuf> = (0..imgs.len()).map(|i| dir.join(format!(".{base}-wum-{pid}-{i}.tmp"))).collect();
    let (mut placed, mut names) = (Vec::new(), Vec::new());
    let res = (|| -> std::io::Result<()> {
        for (t, b) in tmps.iter().zip(imgs) {
            fs::write(t, b)?;
        }
        for (i, t) in tmps.iter().enumerate() {
            let name = format!("{base}-manual-{:02}.jpg", i + 1);
            fs::rename(t, dir.join(&name))?;
            placed.push(dir.join(&name));
            names.push(name);
        }
        Ok(())
    })();
    if let Err(e) = res {
        for f in tmps.iter().chain(&placed) {
            let _ = fs::remove_file(f);
        }
        return Err(e.to_string());
    }
    Ok(names)
}

/// Whether this game's manual pages were made here from its e-manual (they open on the contents page: the box front
/// goes before it).
pub fn generated(data_dir: &Path, title: &str) -> bool {
    let dir = dir_of(data_dir);
    let stem = safe(title);
    if dir.join(format!("{stem}.made")).exists() {
        return true;
    }
    if !dir.join(format!("{stem}.bfma")).is_file() {
        return false;
    }
    // a marker left by a version that couldn't convert it doesn't count
    fs::read_to_string(dir.join(format!("{stem}.bfma.failed"))).map_or(true, |v| v != VERSION)
}

/// Pages made from a Wii U e-manual, known by their shape: 688 px wide (or twice or three times that) and tall. The
/// marker `generated` looks for lives in the data folder that made them; the games folder may be shared.
pub fn looks_made(first_page: &Path) -> bool {
    crate::boxart::size_of(first_page).is_some_and(|(w, h)| w > 0 && w % 688 == 0 && w <= 2064 && f64::from(h) > f64::from(w) * 1.2)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::bflyt::tests::{Writer, P, WHITE};

    /// A SARC of `files`, big-endian.
    pub(crate) fn sarc(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut names = Vec::new();
        let mut nodes = Vec::new();
        let mut data = Vec::new();
        for (n, d) in files {
            while data.len() % 4 != 0 {
                data.push(0);
            }
            let name_off = names.len() / 4;
            names.extend_from_slice(n.as_bytes());
            names.push(0);
            while names.len() % 4 != 0 {
                names.push(0);
            }
            nodes.extend_from_slice(&0u32.to_be_bytes());
            nodes.extend_from_slice(&(0x0100_0000u32 | name_off as u32).to_be_bytes());
            nodes.extend_from_slice(&(data.len() as u32).to_be_bytes());
            data.extend_from_slice(d);
            nodes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        }
        let sfat = 0x14;
        let data_off = sfat + 12 + nodes.len() + 8 + names.len();
        let mut b = b"SARC".to_vec();
        b.extend_from_slice(&0x14u16.to_be_bytes());
        b.extend_from_slice(&[0xFE, 0xFF]);
        b.extend_from_slice(&((data_off + data.len()) as u32).to_be_bytes());
        b.extend_from_slice(&(data_off as u32).to_be_bytes());
        b.extend_from_slice(&[1, 0, 0, 0]);
        b.extend_from_slice(b"SFAT");
        b.extend_from_slice(&12u16.to_be_bytes());
        b.extend_from_slice(&(files.len() as u16).to_be_bytes());
        b.extend_from_slice(&0x65u32.to_be_bytes());
        b.extend_from_slice(&nodes);
        b.extend_from_slice(b"SFNT");
        b.extend_from_slice(&8u16.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        b.extend_from_slice(&names);
        b.extend_from_slice(&data);
        b
    }

    fn zlib(b: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(b).unwrap();
        e.finish().unwrap()
    }

    fn line(x: f32, y: f32, sy: f32, sp: f32, color: [u8; 3], s: &str) -> Vec<u8> {
        let mut b = vec![0, 1, 0, 0];
        let units: Vec<u16> = s.encode_utf16().collect();
        b.extend_from_slice(&(units.len() as u16).to_be_bytes());
        for v in [x, y, sy, sy, sp] {
            b.extend_from_slice(&v.to_be_bytes());
        }
        b.extend_from_slice(&color);
        b.extend_from_slice(&[0; 5]);
        for u in units {
            b.extend_from_slice(&u.to_be_bytes());
        }
        b
    }

    #[test]
    fn reads_a_sarc() {
        let s = sarc(&[("a/one.bin", b"first".to_vec()), ("two.bin", vec![1, 2, 3])]);
        let l = sarc_list(&s).unwrap();
        assert_eq!(l, [("a/one.bin".to_string(), &b"first"[..]), ("two.bin".to_string(), &[1u8, 2, 3][..])]);
        let a = arc(&zlib(&s)).unwrap();
        assert_eq!(a.order, ["one.bin", "two.bin"]);
        assert_eq!(a.get("one.bin"), Some(&b"first"[..]));
        assert_eq!(arc(&s).unwrap().get("two.bin"), Some(&[1u8, 2, 3][..]), "a raw member");
        assert!(sarc_list(b"SARX").is_none());
        assert!(arc(&[0x78, 1, 2, 3]).unwrap().order.is_empty(), "a damaged member is empty");
    }

    #[test]
    fn text_lines_of_a_textset() {
        let mut b = line(10.0, -4.5, 22.0, 0.0, [1, 2, 3], "Press \u{E000} to jump.");
        b.extend(line(10.0, -30.0, 22.0, 0.25, [4, 5, 6], "Ünïcødé"));
        b.extend_from_slice(&[0, 1, 0, 0, 0, 9]);
        let l = text_lines(&b);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0], TextLine { x: 10.0, y: -4.5, sx: 22.0, sy: 22.0, sp: 0.0, color: [1, 2, 3], s: "Press \u{E000} to jump.".into() });
        assert_eq!((l[1].s.as_str(), l[1].sp, l[1].color), ("Ünïcødé", 0.25, [4, 5, 6]));
        assert!(text_lines(&[0, 2, 0, 0]).is_empty());
    }

    #[test]
    fn split_page_like_the_electron_app() {
        // the cuts main/wiiumanual.js splitPage makes of the same input
        type Case<'a> = (f64, &'a [(f64, f64)], &'a [(f64, f64)]);
        let cases: [Case; 10] = [
            (900.0, &[], &[(0.0, 900.0)]),
            (2500.0, &[], &[(0.0, 1000.0), (1000.0, 2000.0), (2000.0, 2500.0)]),
            (2100.0, &[], &[(0.0, 1000.0), (1000.0, 2100.0)]),
            (2500.0, &[(950.0, 1100.0)], &[(0.0, 944.0), (944.0, 1944.0), (1944.0, 2500.0)]),
            (3000.0, &[(600.0, 1300.0), (1290.0, 1500.0)], &[(0.0, 1000.0), (1000.0, 2000.0), (2000.0, 3000.0)]),
            (2400.5, &[(100.0, 2000.0), (990.25, 1010.75)], &[(0.0, 984.0), (984.0, 1984.0), (1984.0, 2400.5)]),
            (1300.0, &[(-3.0, 5.0)], &[(0.0, 1000.0), (1000.0, 1300.0)]),
            (2210.0, &[(1990.0, 2100.0)], &[(0.0, 1000.0), (1000.0, 1984.0), (1984.0, 2210.0)]),
            (3000.0, &[(900.0, 1150.0), (1150.0, 1190.0), (1700.0, 2300.0)], &[(0.0, 894.0), (894.0, 1694.0), (1694.0, 2694.0), (2694.0, 3000.0)]),
            (1500.0, &[(1000.0, 1400.0)], &[(0.0, 994.0), (994.0, 1500.0)]),
        ];
        for (h, keep, want) in cases {
            assert_eq!(split_page(h, keep), want, "{h} {keep:?}");
        }
        assert_eq!(split_page(1300.0, &[(f64::NAN, 5.0)]), [(0.0, 1000.0), (1000.0, 1300.0)]);
    }

    /// A small manual: two pages (one in a chapter, one "Important Information"), a repeating paper texture, a
    /// photo, a framed window and text lines.
    pub(crate) fn manual() -> Vec<u8> {
        let paper = crate::gx2::tests::flim(&[200u8; 64 * 4 * 8], 8, 8, 9, 1);
        let mut frame_px = vec![0u8; 64 * 4 * 8];
        for (i, p) in frame_px.chunks_exact_mut(4).enumerate() {
            p.copy_from_slice(&[(i * 3) as u8, 90, 160, 255]);
        }
        let frame = crate::gx2::tests::flim(&frame_px, 8, 8, 9, 1);
        let mut photo = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut photo, 90).encode(&[120u8; 40 * 30 * 3], 40, 30, image::ExtendedColorType::Rgb8).unwrap();

        let mut bg = Writer::default();
        bg.lyt(688.0, 2300.0).textures(&["paper_00^l.bflim"]).materials(&[([0, 0, 0, 0], WHITE, &[(0, 1, 1)])]);
        bg.pane(&P { name: "RootPane", ..P::default() }).open();
        bg.pic(&P { name: "P_paper", origin: 5, wh: (688.0, 2300.0), ..P::default() }, [WHITE; 4], 0, Some([[0.0, 0.0], [10.0, 0.0], [0.0, 30.0], [10.0, 30.0]]));
        bg.pic(&P { name: "P_band", origin: 9, xy: (0.0, -2300.0), wh: (688.0, 200.0), ..P::default() }, [[250, 200, 100, 255], [250, 200, 100, 255], [90, 40, 20, 255], [90, 40, 20, 255]], 9, None);
        bg.pic(&P { name: "P_art", origin: 9, xy: (0.0, -2300.0), wh: (688.0, 160.0), ..P::default() }, [[255, 200, 200, 255]; 4], 0, None);
        bg.close();
        let mut c0 = Writer::default();
        c0.lyt(688.0, 2300.0).textures(&["frame_00^s.bflim"]).materials(&[([20, 0, 0, 0], [255, 240, 230, 255], &[(0, 0, 0)])]);
        c0.pane(&P { name: "RootPane", ..P::default() }).open();
        c0.pic(&P { name: "P_photo", origin: 5, xy: (380.0, -150.0), wh: (280.0, 210.0), ..P::default() }, [WHITE; 4], 0, None);
        c0.user(&[("JpegFileName", UserValue::Str("Photo_00.jpg".into()))]);
        c0.wnd(&P { name: "W_box", origin: 5, xy: (20.0, -700.0), wh: (500.0, 300.0), ..P::default() }, [0.0; 4], 0, &[(0, 0)]);
        c0.txt(&P { name: "T_body", origin: 5, xy: (24.0, -160.0), wh: (640.0, 300.0), ..P::default() }, "");
        c0.close();
        let mut info = Writer::default();
        info.lyt(688.0, 2300.0).pane(&P { name: "RootPane", ..P::default() }).open();
        info.txt(&P { name: "T_side", origin: 5, xy: (24.0, -1500.0), wh: (600.0, 100.0), ..P::default() }, "");
        info.txt(&P { name: "T_body", xy: (999.0, 999.0), ..P::default() }, "");
        info.close();
        let mut legal = Writer::default();
        legal.lyt(688.0, 700.0).pane(&P { name: "RootPane", ..P::default() }).open();
        legal.txt(&P { name: "T_body", origin: 5, xy: (24.0, -40.0), wh: (600.0, 600.0), ..P::default() }, "");
        legal.close();
        let mut index = Writer::default();
        index.lyt(688.0, 400.0).pane(&P { name: "RootPane", ..P::default() }).open();
        index.txt(&P { name: "PageTitle_0", ..P::default() }, "Controls");
        index.txt(&P { name: "Category_0", ..P::default() }, "Getting Started");
        index.user(&[("IsValid", UserValue::Nums(vec![1.0])), ("PageID_00", UserValue::Nums(vec![0.0]))]);
        index.txt(&P { name: "Category_1", ..P::default() }, "Hidden");
        index.user(&[("IsValid", UserValue::Nums(vec![0.0])), ("PageID_00", UserValue::Nums(vec![1.0]))]);
        index.close();

        let mut body = line(4.0, 0.0, 26.0, 0.5, [40, 30, 20], "Move with \u{E081} and jump with \u{E000}, then run.");
        body.extend(line(4.0, -34.0, 26.0, 0.5, [40, 30, 20], "Short line."));
        body.extend(line(4.0, -68.0, 26.0, 0.0, [40, 30, 20], "The last line of the paragraph."));
        let side = line(0.0, 0.0, 22.0, 0.0, [200, 30, 20], "Beside \u{E099} the photo \u{E07D}\u{E08C}\u{E046}\u{E073}\u{E004}.");
        let legal_text = line(0.0, 0.0, 20.0, 0.0, [0, 0, 0], "Important Information");
        let middle = sarc(&[
            ("Page_00_middle_bg.bflyt", bg.bytes()),
            ("Page_00_middle_00.bflyt", c0.bytes()),
            ("Page_00_middle_info.bflyt", info.bytes()),
            ("Page_01_middle_00.bflyt", legal.bytes()),
        ]);
        let textset = sarc(&[("Page_00_middle_textset_T_body.bin", body), ("Page_00_middle_textset_T_side.bin", side), ("Page_01_middle_textset_T_body.bin", legal_text)]);
        let title_text: Vec<u8> = "Health and Safety\0".encode_utf16().flat_map(u16::to_be_bytes).collect();
        sarc(&[
            ("USA_en_middle.arc", zlib(&middle)),
            ("USA_en_textset.arc", zlib(&textset)),
            ("USA_en_text_title.arc", zlib(&sarc(&[("Page_01_title_text_TextBox_00.bin", title_text)]))),
            ("USA_en_index.arc", zlib(&sarc(&[("Index.bflyt", index.bytes())]))),
            ("USA_en_jpeg.arc", zlib(&sarc(&[("Photo_00.jpg", photo)]))),
            ("Common_texture.arc", zlib(&sarc(&[("timg/paper_00^l.bflim", paper), ("timg/frame_00^s.bflim", frame)]))),
            ("JPN_ja_middle.arc", zlib(&sarc(&[]))),
        ])
    }

    // the same as main/wiiumanual.js build() makes of this manual
    #[test]
    fn builds_the_page_model() {
        let m = build(&manual(), "Test Game").unwrap();
        assert_eq!((m.title.as_str(), m.lang.as_str(), m.pages.len()), ("Test Game", "USA_en", 2));
        let p = &m.pages[0];
        assert_eq!((p.title.as_str(), p.chapter.as_str(), p.height), ("Controls", "Getting Started", 2300.0));
        assert_eq!(p.parts, [(0.0, 1006.0), (1006.0, 2006.0), (2006.0, 2300.0)]);
        assert_eq!((m.pages[1].title.as_str(), m.pages[1].chapter.as_str(), m.pages[1].parts.as_slice()), ("Health and Safety", "", &[(0.0, 700.0)][..]));
        assert_eq!(m.toc, [Chapter { name: "Getting Started".into(), pages: vec![0] }, Chapter { name: String::new(), pages: vec![1] }]);
        type Seen<'a> = (Option<&'a str>, [f64; 8], u8, bool, bool);
        let o: Vec<Seen> = p.ops.iter().map(|o| (o.img.as_deref(), [o.x, o.y, o.w, o.h, o.u0, o.v0, o.u1, o.v1], o.ws, o.photo, o.bg)).collect();
        assert_eq!(
            o,
            [
                (Some("paper_00^l.bflim"), [0.0, 0.0, 688.0, 2300.0, 0.0, 0.0, 10.0, 30.0], 1, false, true),
                (None, [0.0, 2100.0, 688.0, 200.0, 0.0, 0.0, 1.0, 1.0], 0, false, true),
                (Some("paper_00^l.bflim"), [0.0, 2140.0, 688.0, 160.0, 0.0, 0.0, 1.0, 1.0], 1, false, true),
                (Some("Photo_00.jpg"), [380.0, 150.0, 280.0, 210.0, 0.0, 0.0, 1.0, 1.0], 0, true, false),
                (Some("frame_00^s.bflim"), [28.0, 708.0, 484.0, 284.0, 0.0, 0.0, 1.0, 1.0], 0, false, false),
                (Some("frame_00^s.bflim"), [20.0, 700.0, 492.0, 8.0, 0.0, 0.0, 61.5, 1.0], 0, false, false),
                (Some("frame_00^s.bflim"), [512.0, 700.0, 8.0, 292.0, 1.0, 0.0, 0.0, 36.5], 0, false, false),
                (Some("frame_00^s.bflim"), [28.0, 992.0, 492.0, 8.0, 61.5, 1.0, 0.0, 0.0], 0, false, false),
                (Some("frame_00^s.bflim"), [20.0, 708.0, 8.0, 292.0, 0.0, 36.5, 1.0, 0.0], 0, false, false),
            ]
        );
        assert_eq!(p.ops[1].c, Colors::Four([[250, 200, 100, 255], [250, 200, 100, 255], [90, 40, 20, 255], [90, 40, 20, 255]]));
        assert_eq!(p.ops[2].c, Colors::One([255, 200, 200, 255]));
        assert_eq!((p.ops[3].tint, p.ops[4].tint), (None, Some(([20, 0, 0, 0], [255, 240, 230, 255]))));
        assert_eq!(m.sizes["Photo_00.jpg"], (40.0, 30.0));
        assert_eq!(m.sizes["frame_00^s.bflim"], (8.0, 8.0));
        let t: Vec<(&str, f64, f64, f64, f64, bool)> = p.text.iter().map(|l| (l.in_box.as_str(), l.bx, l.bw, l.x, l.y, l.last)).collect();
        assert_eq!(t, [("T_body", 24.0, 640.0, 28.0, 160.0, false), ("T_body", 24.0, 640.0, 28.0, 194.0, false), ("T_body", 24.0, 640.0, 28.0, 228.0, true), ("T_side", 24.0, 600.0, 24.0, 1500.0, true)]);
        assert_eq!(m.pages[1].text[0].y, 40.0);
        assert!(build(b"nothing", "").is_err());
        assert!(!is_epub(&manual()));
        assert!(is_epub(&sarc(&[("EUR_en_Manual.epub", vec![1])])));
    }

    #[test]
    fn collects_pages_with_the_electron_apps_markers() {
        let tmp = tempfile::tempdir().unwrap();
        let (data, games, art) = (tmp.path().join("data"), tmp.path().join("games"), tmp.path().join("art"));
        fs::create_dir_all(&games).unwrap();
        let wua = games.join("Test: Game.wua");
        fs::write(&wua, crate::zarchive::tests::build(&[("0005000010101A00/meta/Manual.bfma", &manual())])).unwrap();
        let none = games.join("No Manual.wua");
        fs::write(&none, crate::zarchive::tests::build(&[("0005000010101B00/meta/meta.xml", b"<menu/>")])).unwrap();
        let ebook = games.join("Book.wua");
        fs::write(&ebook, crate::zarchive::tests::build(&[("0005000010101C00/meta/Manual.bfma", &sarc(&[("USA_en_Manual.epub", b"PK".to_vec())]))])).unwrap();
        let rpx = games.join("Loose").join("code").join("loose.rpx");
        fs::create_dir_all(rpx.parent().unwrap()).unwrap();
        fs::create_dir_all(games.join("Loose").join("meta")).unwrap();
        fs::write(games.join("Loose").join("meta").join("Manual.bfma"), b"SARC but not really").unwrap();
        let want = |title: &str, file: &Path, base: &str| Want { title: title.into(), file: file.to_path_buf(), art_dir: art.clone(), base: base.into() };
        let all = [want("Test: Game", &wua, "Test Game"), want("No Manual", &none, "No Manual"), want("Book", &ebook, "Book"), want("Loose", &rpx, "loose")];
        let calls = std::cell::Cell::new(0);
        let render = |m: &Model| {
            calls.set(calls.get() + 1);
            Ok(vec![vec![0xFF, 0xD8, m.pages.len() as u8]; 3])
        };
        assert_eq!(collect(&data, &all, &render), 1);
        let dir = data.join("wiiu-manuals");
        let mut pages: Vec<String> = fs::read_dir(&art).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        pages.sort();
        assert_eq!(pages, ["Test Game-manual-01.jpg", "Test Game-manual-02.jpg", "Test Game-manual-03.jpg"]);
        assert_eq!(fs::read_to_string(dir.join("Test_ Game.made")).unwrap(), VERSION);
        assert!(!dir.join("Test_ Game.bfma").exists(), "the manual's copy goes once the pages are saved");
        assert!(generated(&data, "Test: Game"));
        assert_eq!(fs::read_to_string(dir.join("No Manual.bfma.failed")).unwrap(), VERSION, "no e-manual: not read again");
        assert!(!generated(&data, "No Manual"));
        assert!(!dir.join("Book.bfma.failed").exists() && !dir.join("Book.bfma").exists(), "an e-book is left for later");
        assert_eq!(fs::read_to_string(dir.join("Loose.bfma.failed")).unwrap(), VERSION, "a manual that isn't one fails");
        assert!(fs::read_dir(&art).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
        // the next pass draws nothing again
        assert_eq!(collect(&data, &all, &render), 0);
        assert_eq!(calls.get(), 1);
        // a copy left by the Electron app for a game that has pages now: a marker in its place
        fs::write(dir.join("Test_ Game.bfma"), b"x").unwrap();
        fs::remove_file(dir.join("Test_ Game.made")).unwrap();
        collect(&data, &all, &render);
        assert!(dir.join("Test_ Game.made").exists() && !dir.join("Test_ Game.bfma").exists());
        // a failure left by another version is tried again
        fs::write(dir.join("No Manual.bfma.failed"), "1.10.0+0").unwrap();
        collect(&data, &all, &render);
        assert_eq!(fs::read_to_string(dir.join("No Manual.bfma.failed")).unwrap(), VERSION);
        // a copy waiting without a failure counts as made (as main/wiiumanual.js generated() has it)
        fs::write(dir.join("Other.bfma"), b"x").unwrap();
        assert!(generated(&data, "Other"));
        fs::write(dir.join("Other.bfma.failed"), VERSION).unwrap();
        assert!(!generated(&data, "Other"));
    }

    #[test]
    fn a_failed_drawing_fails_the_game() {
        let tmp = tempfile::tempdir().unwrap();
        let rpx = tmp.path().join("G").join("code").join("g.rpx");
        fs::create_dir_all(rpx.parent().unwrap()).unwrap();
        fs::create_dir_all(tmp.path().join("G").join("meta")).unwrap();
        fs::write(tmp.path().join("G").join("meta").join("Manual.bfma"), manual()).unwrap();
        let g = [Want { title: "G".into(), file: rpx, art_dir: tmp.path().join("art"), base: "G".into() }];
        assert_eq!(collect(tmp.path(), &g, &|_| panic!("a drawing that trips")), 0);
        assert!(tmp.path().join("wiiu-manuals").join("G.bfma.failed").exists());
        assert!(!tmp.path().join("art").exists());
    }

    #[test]
    fn jpeg_sizes() {
        let mut j = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut j, 80).encode(&[0u8; 17 * 9 * 3], 17, 9, image::ExtendedColorType::Rgb8).unwrap();
        assert_eq!(jpeg_size(&j), (17, 9));
        assert_eq!(jpeg_size(b"not a jpeg at all"), (0, 0));
    }
}


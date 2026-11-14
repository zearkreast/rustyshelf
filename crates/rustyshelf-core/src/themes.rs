// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// themes in Boxshelf's renderer/app.js and renderer/themes-extra.js, as RustyShelf has them.
//! The shelf's colour themes: each paints the backdrop behind the shelf, as the CSS the Electron app gives its own
//! backdrop (painted here by `cssbg`), and picks the hue of the glow under the chosen case.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub id: String,
    pub name: String,
    /// something moves over its backdrop
    pub moves: bool,
}

#[derive(Deserialize)]
struct Extra {
    id: String,
    name: String,
    moves: bool,
}

#[derive(Deserialize)]
struct Table {
    parts: HashMap<String, String>,
    extra: Vec<Extra>,
}

static TABLE: LazyLock<Table> = LazyLock::new(|| serde_json::from_str(include_str!("../data/themes.json")).expect("data/themes.json"));

const OWN: [(&str, &str, bool); 8] = [
    ("spectrum", "Spectrum", true),
    ("classic", "Spectrum Classic", false),
    ("midnight", "Midnight", true),
    ("aurora", "Aurora", true),
    ("keys", "Keys", true),
    ("arcade", "Arcade", true),
    ("artwork", "Artwork", false),
    ("blur", "Blur", false),
];

/// Every theme, in the order Settings shows them.
pub fn themes() -> Vec<Theme> {
    OWN.iter()
        .map(|(id, name, moves)| Theme { id: id.to_string(), name: name.to_string(), moves: *moves })
        .chain(TABLE.extra.iter().map(|t| Theme { id: t.id.clone(), name: t.name.clone(), moves: t.moves }))
        .collect()
}

/// The theme a saved name stands for: itself, the one an old name became, or Spectrum.
pub fn id_of(saved: &str) -> &str {
    match saved {
        "sunset" => "spectrum",
        "gallery" => "midnight",
        "synthwave" => "arcade",
        known if OWN.iter().any(|t| t.0 == known) || TABLE.extra.iter().any(|t| t.id == known) => known,
        _ => "spectrum",
    }
}

fn turn(h: f64) -> f64 {
    ((h % 360.0) + 360.0) % 360.0
}

fn hsl(h: f64, s: u32, l: u32, a: &str) -> String {
    format!("hsla({},{s}%,{l}%,{a})", turn(h))
}

/// The extra themes' colours keep whole degrees.
fn whole(h: f64, s: u32, l: u32, a: &str) -> String {
    format!("hsla({},{s}%,{l}%,{a})", turn((h + 0.5).floor()))
}

/// A console's hue drawn part of the way to the theme's own: `keep` of the console's colour stays.
fn pull(sh: f64, to: f64, keep: f64) -> f64 {
    to + ((((sh - to) % 360.0) + 540.0) % 360.0 - 180.0) * keep
}

/// The hue of the glow under the chosen case. `sh`: the shelf's hue where the chosen case stands; `song`: the hue of
/// the song the menu plays, when it plays one.
pub fn halo(id: &str, sh: f64, song: Option<f64>) -> f64 {
    match id_of(id) {
        "keys" => song.unwrap_or(sh),
        "arcade" => sh + 300.0,
        "blueshift" => pull(sh, 215.0, 0.35),
        "swirl" => 22.0,
        "pocket" => 78.0,
        "walnut" => 30.0,
        "forest" => pull(sh, 120.0, 0.3),
        _ => sh,
    }
}

/// The backdrop as CSS, as the Electron app writes it. `hue`: the chosen game's own.
pub fn tint(id: &str, sh: f64, hue: f64, song: Option<f64>) -> String {
    let part = |name: &str| TABLE.parts.get(name).map_or("", String::as_str);
    match id_of(id) {
        "midnight" => format!(
            "radial-gradient(ellipse 55% 34% at 50% 74%,{},transparent 72%),radial-gradient(ellipse 90% 40% at 50% 0%,{},transparent 70%),#000",
            hsl(sh, 80, 42, "0.28"),
            hsl(sh, 40, 30, "0.12")
        ),
        "aurora" => format!(
            "radial-gradient(ellipse 70% 45% at 25% 18%,{},transparent 70%),radial-gradient(ellipse 60% 40% at 78% 26%,{},transparent 70%),radial-gradient(ellipse 80% 50% at 50% 100%,{},transparent 70%),linear-gradient(180deg,#04101c,#020610 70%,#01030a)",
            hsl(sh, 80, 45, "0.45"),
            hsl(sh + 70.0, 75, 48, "0.38"),
            hsl(sh + 20.0, 70, 25, "0.35")
        ),
        "keys" => {
            let s = song.unwrap_or(sh);
            format!(
                "radial-gradient(ellipse 70% 40% at 50% 62%,{},transparent 72%),radial-gradient(ellipse 80% 50% at 50% 0%,{},transparent 70%),linear-gradient(180deg,#05040f,#0a0a22 55%,#030309)",
                hsl(s, 70, 30, "0.28"),
                hsl(s + 40.0, 50, 18, "0.35")
            )
        }
        "arcade" => format!(
            "radial-gradient(ellipse 80% 30% at 50% 60%,{},transparent 70%),linear-gradient(180deg,#04020c 0%,#0d0524 45%,#1c0838 58%,#07021a 62%,#030108 100%)",
            hsl(sh + 300.0, 90, 45, "0.32")
        ),
        "blur" => format!(
            "radial-gradient(ellipse 55% 30% at 50% 60%,{},transparent 72%),linear-gradient(180deg,rgba(0,0,0,.42),rgba(0,0,0,.12) 34%,rgba(0,0,0,.18) 58%,rgba(0,0,0,.62))",
            hsl(sh, 60, 50, "0.16")
        ),
        "cube" => [
            format!("radial-gradient(ellipse 46% 30% at 50% 57%,{},transparent 72%)", whole(sh, 72, 52, "0.36")),
            "radial-gradient(ellipse 44% 48% at 50% 44%,rgba(14,11,40,.8),transparent 78%)".into(),
            format!("radial-gradient(ellipse 55% 45% at 18% 8%,{},transparent 70%)", whole(sh, 60, 48, "0.16")),
            "linear-gradient(180deg,rgba(8,6,24,.55),transparent 24%,transparent 58%,rgba(5,4,16,.9))".into(),
            part("CUBE").into(),
        ]
        .join(","),
        "blueshift" => [
            "radial-gradient(ellipse 70% 20% at 50% 57%,rgba(70,140,255,.42),transparent 70%)".into(),
            format!("radial-gradient(ellipse 46% 34% at 50% 58%,{},transparent 70%)", whole(sh, 80, 52, "0.16")),
            format!("{} 0 0/100% 100% no-repeat", part("BLUE")),
            "linear-gradient(180deg,#000514 0%,#000f3c 38%,#08287f 55.5%,#0b3197 57%,#03185a 59%,#000a2a 76%,#00030e 100%)".into(),
        ]
        .join(","),
        "swirl" => [
            "radial-gradient(ellipse 34% 30% at 50% 44%,rgba(255,120,40,.14),transparent 70%)".into(),
            format!("radial-gradient(ellipse 60% 30% at 50% 102%,{},transparent 72%)", whole(sh, 55, 45, "0.16")),
            format!("{} 50% 50%/cover no-repeat", part("SWIRL")),
            "radial-gradient(ellipse 90% 80% at 50% 45%,transparent 50%,rgba(0,0,0,.55))".into(),
            "linear-gradient(180deg,#1c1d22,#131418 60%,#0b0c0e)".into(),
        ]
        .join(","),
        "pocket" => [
            "repeating-linear-gradient(0deg,rgba(6,22,8,.32) 0 .75px,transparent .75px 3px)".into(),
            "repeating-linear-gradient(90deg,rgba(6,22,8,.32) 0 .75px,transparent .75px 3px)".into(),
            "linear-gradient(112deg,transparent 22%,rgba(235,255,200,.035) 22.4%,rgba(235,255,200,.035) 38%,transparent 38.4%)".into(),
            "radial-gradient(ellipse 90% 80% at 50% 45%,transparent 55%,rgba(6,18,8,.78))".into(),
            format!("radial-gradient(ellipse 50% 26% at 50% 60%,{},transparent 72%)", whole(sh, 35, 45, "0.1")),
            "radial-gradient(ellipse 72% 58% at 50% 40%,rgba(150,176,60,.30),transparent 74%)".into(),
            "linear-gradient(180deg,#22401f,#2c5229 42%,#213f20 70%,#112613)".into(),
        ]
        .join(","),
        "oled" => format!("radial-gradient(ellipse 24% 6% at 50% 60%,{},transparent 75%),#000", whole(sh, 85, 52, "0.22")),
        "walnut" => [
            "radial-gradient(ellipse 70% 52% at 50% 12%,rgba(255,170,90,.24),transparent 72%)".into(),
            format!("radial-gradient(ellipse 40% 34% at 14% 0%,{},transparent 70%)", whole(sh, 60, 55, "0.1")),
            "linear-gradient(180deg,transparent 58.2%,rgba(255,196,130,.13) 58.5%,rgba(255,196,130,.04) 59.1%,rgba(10,5,2,.55) 59.4%,rgba(8,4,2,.78))".into(),
            "repeating-linear-gradient(90deg,rgba(0,0,0,.34) 0 2px,rgba(255,215,175,.05) 2px 3px,transparent 3px 240px)".into(),
            "repeating-linear-gradient(90.6deg,rgba(0,0,0,.10) 0 2px,transparent 2px 9px,rgba(255,220,180,.03) 9px 12px,transparent 12px 23px)".into(),
            "repeating-linear-gradient(89.2deg,rgba(0,0,0,.08) 0 1px,transparent 1px 37px)".into(),
            "repeating-linear-gradient(91.4deg,rgba(255,200,150,.035) 0 6px,transparent 6px 61px)".into(),
            "linear-gradient(180deg,#3c2416,#2c180e 55%,#1a0d07)".into(),
        ]
        .join(","),
        "forest" => [
            "radial-gradient(ellipse 30% 14% at 50% 77%,rgba(3,12,8,.55),transparent 75%)".into(),
            format!("{} 0 100%/100% 46% no-repeat", part("FOREST")),
            "conic-gradient(from 112deg at 16% -32%,transparent 0deg,rgba(210,255,190,.1) 5deg,transparent 11deg,transparent 15deg,rgba(210,255,190,.075) 19deg,transparent 25deg,transparent 29deg,rgba(225,255,205,.11) 33deg,transparent 39deg,transparent 44deg,rgba(210,255,190,.06) 47deg,transparent 53deg,transparent 360deg)".into(),
            format!("radial-gradient(ellipse 44% 30% at 50% 58%,{},transparent 72%)", whole(pull(sh, 120.0, 0.3), 40, 40, "0.2")),
            "radial-gradient(ellipse 50% 40% at 16% 0%,rgba(170,230,160,.13),transparent 70%)".into(),
            "linear-gradient(180deg,#0e2e20,#0a2418 45%,#071a12 66%,#04100b)".into(),
        ]
        .join(","),
        "crt" => [
            format!("{} 0 0/100% 100% no-repeat", part("CRT_BEZEL")),
            "linear-gradient(158deg,rgba(255,255,255,.06),transparent 30%)".into(),
            "radial-gradient(ellipse 80% 76% at 50% 50%,transparent 60%,rgba(0,0,0,.55))".into(),
            "repeating-linear-gradient(0deg,rgba(0,0,0,.36) 0 1.5px,transparent 1.5px 3px)".into(),
            "repeating-linear-gradient(90deg,rgba(255,70,70,.05) 0 1px,rgba(70,255,110,.05) 1px 2px,rgba(80,120,255,.05) 2px 3px)".into(),
            format!("radial-gradient(ellipse 64% 58% at 50% 48%,{},{} 55%,transparent 82%)", whole(sh, 75, 50, "0.56"), whole(sh, 65, 28, "0.32")),
            "linear-gradient(180deg,#0b0e13,#07090c)".into(),
        ]
        .join(","),
        "vhs" => [
            format!("{} 0 0/100% 100% no-repeat", part("VHS_NOISE")),
            "repeating-linear-gradient(0deg,rgba(0,0,0,.24) 0 1.5px,transparent 1.5px 4.5px)".into(),
            "radial-gradient(ellipse 92% 76% at 50% 50%,transparent 52%,rgba(0,0,0,.6))".into(),
            "radial-gradient(ellipse 34% 32% at 43.5% 46%,rgba(40,210,255,.27),transparent 70%)".into(),
            "radial-gradient(ellipse 34% 32% at 56.5% 49%,rgba(255,40,120,.25),transparent 70%)".into(),
            format!("radial-gradient(ellipse 44% 36% at 50% 48%,{},transparent 72%)", whole(sh, 70, 45, "0.22")),
            "linear-gradient(90deg,rgba(255,30,110,.08),transparent 24%,transparent 76%,rgba(30,190,255,.08))".into(),
            "linear-gradient(180deg,#0d0b1f,#110e29 50%,#08071b)".into(),
        ]
        .join(","),
        "synth" => [
            "radial-gradient(ellipse 30% 13% at 50% 77%,rgba(8,2,18,.6),transparent 75%)".into(),
            format!("{} 0 100%/100% 43% no-repeat", part("SYN_GRID")),
            format!("radial-gradient(ellipse 60% 7% at 50% 57%,{},transparent 70%)", whole(sh, 95, 66, "0.5")),
            format!("{} 50% 34.85%/auto 34% no-repeat", part("SYN_SUN")),
            "radial-gradient(ellipse 40% 30% at 50% 50%,rgba(255,90,140,.22),transparent 72%)".into(),
            "linear-gradient(180deg,#0b0420 0%,#1c0838 28%,#47105c 45%,#a3246e 53%,#ff6a5a 56.9%,#14062a 57%,#0c0418 76%,#05020c 100%)".into(),
        ]
        .join(","),
        // Spectrum, and the two that wear its wash: Spectrum Classic, and Artwork under a game's own picture
        _ => format!(
            "radial-gradient(ellipse 75% 65% at 28% 22%,{},transparent 70%),radial-gradient(ellipse 60% 60% at 82% 72%,{},transparent 70%),radial-gradient(ellipse 40% 40% at 55% 55%,{},transparent 70%),linear-gradient(160deg,{},{})",
            hsl(sh, 72, 46, "0.62"),
            hsl(sh + 10.0, 70, 34, "0.5"),
            hsl(hue, 60, 42, "0.14"),
            hsl(sh, 55, 22, "1"),
            hsl(sh + 4.0, 50, 7, "1")
        ),
    }
}

/// The window the backdrop is painted for, in CSS pixels: its size, and the height of a case on the shelf.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stage {
    pub w: f32,
    pub h: f32,
    pub case_h: f32,
}

/// How strong the floor's glow, the dark corners and the glow behind the chosen case are in a theme (the Electron
/// app's style.css and themes-extra.css, keep in sync).
fn strengths(id: &str) -> (f32, f32, f32) {
    match id_of(id) {
        "midnight" => (0.25, 1.0, 1.0),
        "keys" | "arcade" => (0.4, 1.0, 1.0),
        "blur" => (0.3, 1.0, 1.0),
        "cube" | "blueshift" | "synth" => (0.42, 1.0, 1.0),
        "swirl" | "walnut" | "forest" => (0.34, 1.0, 1.0),
        "crt" | "vhs" => (0.34, 0.5, 1.0),
        "pocket" => (0.28, 1.0, 0.5),
        "oled" => (0.16, 0.0, 0.45),
        _ => (1.0, 1.0, 1.0),
    }
}

/// How strong the glow behind the chosen case is in a theme.
pub fn halo_strength(id: &str) -> f32 {
    strengths(id).2
}

/// The glow's colour, as the floor and the glow behind the chosen case take it.
pub fn glow(id: &str, sh: f64, song: Option<f64>) -> String {
    hsl(halo(id, sh, song), 95, 62, "0.62")
}

/// Whether a theme's backdrop has patterns too fine to be painted small and stretched.
pub fn fine(id: &str) -> bool {
    matches!(id_of(id), "arcade" | "cube" | "blueshift" | "swirl" | "pocket" | "walnut" | "forest" | "crt" | "vhs" | "synth")
}

/// The dark corners every theme has over its backdrop (style.css .vignette, Midnight's own).
fn corners_css(id: &str) -> &'static str {
    match id {
        "midnight" => "radial-gradient(ellipse at 50% 45%,transparent 35%,rgba(0,0,0,.85))",
        _ => "radial-gradient(ellipse 80% 70% at 50% 42%,transparent 35%,rgba(0,0,0,.5) 100%),linear-gradient(to bottom,rgba(0,0,0,.25),transparent 20%,transparent 62%,rgba(0,0,0,.5))",
    }
}

/// How much light the theme's dark corners leave at each pixel of a picture `width` × `height` (255: all of it),
/// for what's drawn between the backdrop and them.
pub fn corners(id: &str, width: u32, height: u32) -> Vec<u8> {
    let id = id_of(id);
    let mut c = crate::cssbg::Canvas::new(width, height, 1.0);
    let whole = (0.0, 0.0, width as f32, height as f32);
    c.paint("#fff", whole, 1.0);
    c.paint(corners_css(id), whole, strengths(id).1);
    c.picture().rgba.as_chunks::<4>().0.iter().map(|px| px[0]).collect()
}

/// The size of the chosen game's cover as the backdrop takes its colours: `SWATCH.0` × `SWATCH.1` pixels.
pub const SWATCH: (usize, usize) = (18, 24);

/// A cover shrunk to a swatch's red, green and blue (`SWATCH.0 * SWATCH.1 * 3` bytes), its colours a little
/// stronger and darker, as the Electron app shrinks it (CSS saturate(1.3) brightness(.95)).
pub fn swatch(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let (sw, sh) = (SWATCH.0 as u32, SWATCH.1 as u32);
    let mut out = Vec::with_capacity(SWATCH.0 * SWATCH.1 * 3);
    for cy in 0..sh {
        for cx in 0..sw {
            let (x0, y0) = (cx * w / sw, cy * h / sh);
            let (x1, y1) = (((cx + 1) * w / sw).max(x0 + 1).min(w.max(1)), ((cy + 1) * h / sh).max(y0 + 1).min(h.max(1)));
            let mut sum = [0.0f32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * w + x) * 4) as usize;
                    for (c, total) in sum.iter_mut().enumerate() {
                        *total += rgba.get(i + c).copied().map_or(0.0, f32::from);
                    }
                }
            }
            let n = (x1.saturating_sub(x0) * y1.saturating_sub(y0)).max(1) as f32;
            let [r, g, b] = sum.map(|v| v / n);
            let grey = 0.213 * r + 0.715 * g + 0.072 * b;
            out.extend([r, g, b].map(|v| (((grey + (v - grey) * 1.3) * 0.95).round()).clamp(0.0, 255.0) as u8));
        }
    }
    out
}

/// Everything that lies still behind the shelf, painted `width` × `height` pixels: the theme's backdrop, a wash of
/// the chosen game's cover over it (`cover`: its swatch), the dark corners, and the floor's glow under the cases.
pub fn backdrop(id: &str, sh: f64, hue: f64, song: Option<f64>, cover: Option<&[u8]>, stage: Stage, (width, height): (u32, u32)) -> crate::drawn::Picture {
    let id = id_of(id);
    let (floor, corners, _) = strengths(id);
    let whole = (0.0, 0.0, stage.w, stage.h);
    let mut c = crate::cssbg::Canvas::new(width, height, width as f32 / stage.w);
    c.paint("#07070c", whole, 1.0);
    c.paint(&tint(id, sh, hue, song), whole, 1.0);
    if let Some(rgb) = cover {
        c.stretch(rgb, SWATCH, (-0.15 * stage.w, -0.15 * stage.h, 1.3 * stage.w, 1.3 * stage.h), 0.16);
    }
    if id == "arcade" {
        c.paint("repeating-linear-gradient(0deg,rgba(0,0,0,.22) 0 1px,transparent 1px 3px)", whole, 1.0);
        c.paint("radial-gradient(ellipse 120% 100% at 50% 50%,transparent 60%,rgba(0,0,0,.55))", whole, 1.0);
    }
    c.paint(corners_css(id), whole, corners);
    let wide = (stage.w * 1.4).min(1400.0);
    c.paint(
        &format!("radial-gradient(ellipse 18% 26% at 50% 16%,{},transparent 75%),radial-gradient(ellipse 55% 34% at 50% 26%,rgba(0,0,0,.55),transparent 72%)", glow(id, sh, song)),
        ((stage.w - wide) / 2.0, stage.h * 0.42 + stage.case_h / 2.0 - 10.0, wide, 160.0),
        floor,
    );
    c.picture()
}

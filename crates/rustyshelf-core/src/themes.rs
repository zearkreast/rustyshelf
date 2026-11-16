// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// themes in Boxshelf's renderer/app.js and renderer/themes-extra.js, as RustyShelf has them. Pastel Pop is from
// ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf: ported
// from its renderer/themes-extra.js.
//! The shelf's colour themes: each paints the backdrop behind the shelf, as the CSS the Electron app gives its own
//! backdrop (painted here by `cssbg`), and picks the hue of the glow under the chosen case. The pictures Boxshelf
//! beta.14's themes and Pastel Pop are made of come from `data/themes-14.json`; those themes are still
//! pictures here.

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

#[derive(Deserialize)]
struct Table14 {
    parts: HashMap<String, serde_json::Value>,
    /// a retired theme's id → the one it becomes
    retired: HashMap<String, String>,
    /// the extra themes, in beta.14's order
    extra: Vec<Extra>,
}

static TABLE14: LazyLock<Table14> = LazyLock::new(|| serde_json::from_str(include_str!("../data/themes-14.json")).expect("data/themes-14.json"));

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

/// Every theme, in the order Settings shows them. The extra themes move only where this app moves them (the older
/// ones' layers); beta.14's are still pictures.
pub fn themes() -> Vec<Theme> {
    OWN.iter()
        .map(|(id, name, moves)| Theme { id: id.to_string(), name: name.to_string(), moves: *moves })
        .chain(TABLE14.extra.iter().map(|t| Theme { id: t.id.clone(), name: t.name.clone(), moves: TABLE.extra.iter().find(|o| o.id == t.id).is_some_and(|o| o.moves) }))
        .collect()
}

/// The theme a saved name stands for: itself, the one an old name became, or Spectrum.
pub fn id_of(saved: &str) -> &str {
    match saved {
        "sunset" => "spectrum",
        "gallery" => "midnight",
        "synthwave" => "arcade",
        old if TABLE14.retired.contains_key(old) => TABLE14.retired[old].as_str(),
        known if OWN.iter().any(|t| t.0 == known) || TABLE14.extra.iter().any(|t| t.id == known) => known,
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
        "forest" => pull(sh, 120.0, 0.3),
        "nebula" => pull(sh, 270.0, 0.3),
        "abyss" => pull(sh, 200.0, 0.4),
        "ember" => pull(sh, 22.0, 0.25),
        "dusk" => pull(sh, 25.0, 0.5),
        "holo" => pull(sh, 195.0, 0.25),
        "starfield" => pull(sh, 225.0, 0.3),
        "rain" => pull(sh, 210.0, 0.35),
        "sakura" => pull(sh, 340.0, 0.45),
        "snow" => pull(sh, 205.0, 0.4),
        "eventhorizon" => pull(sh, 30.0, 0.2),
        "glasslake" => pull(sh, 160.0, 0.4),
        "neoncity" => pull(sh, 320.0, 0.35),
        "deepsea" => pull(sh, 195.0, 0.4),
        "storm" => pull(sh, 222.0, 0.4),
        "fireflies" => pull(sh, 75.0, 0.35),
        "desertnight" => pull(sh, 230.0, 0.35),
        "pastelpop" => pull(sh, 290.0, 0.3),
        _ => sh,
    }
}

/// The backdrop as CSS, as the Electron app writes it. `hue`: the chosen game's own.
pub fn tint(id: &str, sh: f64, hue: f64, song: Option<f64>) -> String {
    let part = |name: &str| TABLE.parts.get(name).map_or("", String::as_str);
    let new = |name: &str| TABLE14.parts.get(name).and_then(serde_json::Value::as_str).unwrap_or("");
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
        "oled" => format!("radial-gradient(ellipse 24% 6% at 50% 60%,{},transparent 75%),#000", whole(sh, 85, 52, "0.22")),
        "forest" => [
            "radial-gradient(ellipse 30% 14% at 50% 77%,rgba(3,12,8,.55),transparent 75%)".into(),
            format!("{} 0 100%/100% 46% no-repeat", part("FOREST")),
            "conic-gradient(from 112deg at 16% -32%,transparent 0deg,rgba(210,255,190,.1) 5deg,transparent 11deg,transparent 15deg,rgba(210,255,190,.075) 19deg,transparent 25deg,transparent 29deg,rgba(225,255,205,.11) 33deg,transparent 39deg,transparent 44deg,rgba(210,255,190,.06) 47deg,transparent 53deg,transparent 360deg)".into(),
            format!("radial-gradient(ellipse 44% 30% at 50% 58%,{},transparent 72%)", whole(pull(sh, 120.0, 0.3), 40, 40, "0.2")),
            "radial-gradient(ellipse 50% 40% at 16% 0%,rgba(170,230,160,.13),transparent 70%)".into(),
            "linear-gradient(180deg,#0e2e20,#0a2418 45%,#071a12 66%,#04100b)".into(),
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
        "nebula" => [
            format!("radial-gradient(ellipse 34% 24% at 50% 60%,{},transparent 72%)", whole(sh, 70, 50, "0.2")),
            format!("radial-gradient(ellipse 46% 36% at 30% 38%,{},transparent 70%)", whole(pull(sh, 275.0, 0.35), 75, 52, "0.46")),
            format!("radial-gradient(ellipse 40% 32% at 70% 58%,{},transparent 70%)", whole(pull(sh, 185.0, 0.35), 75, 42, "0.38")),
            "radial-gradient(ellipse 30% 22% at 56% 26%,rgba(255,90,170,.16),transparent 70%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("NEB_STARS")),
            "linear-gradient(180deg,#05040f,#03030a 60%,#020208)".into(),
        ]
        .join(","),
        "abyss" => [
            "radial-gradient(ellipse 60% 25% at 50% 0%,rgba(120,220,255,.26),transparent 70%)".into(),
            format!("radial-gradient(ellipse 40% 26% at 50% 60%,{},transparent 72%)", whole(pull(sh, 200.0, 0.4), 70, 45, "0.18")),
            "linear-gradient(100deg,transparent 30%,rgba(160,230,255,.07) 40%,transparent 50%,transparent 60%,rgba(160,230,255,.045) 66%,transparent 74%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("ABYSS_SNOW")),
            "radial-gradient(ellipse 90% 80% at 50% 45%,transparent 55%,rgba(0,4,10,.5))".into(),
            "linear-gradient(180deg,#06365a 0%,#041d36 45%,#020914 100%)".into(),
        ]
        .join(","),
        "ember" => [
            "radial-gradient(ellipse 60% 30% at 50% 100%,rgba(255,110,40,.5),transparent 70%)".into(),
            format!("radial-gradient(ellipse 40% 24% at 50% 62%,{},transparent 72%)", whole(pull(sh, 22.0, 0.25), 85, 48, "0.18")),
            format!("{} 0 0/100% 100% no-repeat", new("EMBER_STILL")),
            "radial-gradient(ellipse 90% 80% at 50% 45%,transparent 55%,rgba(0,0,0,.5))".into(),
            "linear-gradient(180deg,#0e0a09 0%,#1a0f0b 60%,#2b1309 100%)".into(),
        ]
        .join(","),
        "dusk" => [
            format!("radial-gradient(ellipse 30% 12% at 50% 57%,{},transparent 70%)", whole(pull(sh, 32.0, 0.5), 95, 72, "0.5")),
            format!("radial-gradient(ellipse 70% 22% at 50% 58%,{},transparent 72%)", whole(pull(sh, 345.0, 0.5), 70, 52, "0.3")),
            format!("{} 0 0/100% 42% no-repeat", new("DUSK_STARS")),
            format!(
                "linear-gradient(180deg,#0b1030 0%,#1b1a4c 30%,#3a2a6a 42%,{} 51%,{} 56.6%,{} 57.6%,#140b20 72%,#07050d 100%)",
                whole(pull(sh, 335.0, 0.45), 48, 42, "1"),
                whole(pull(sh, 22.0, 0.45), 82, 64, "1"),
                whole(pull(sh, 300.0, 0.45), 36, 16, "1")
            ),
        ]
        .join(","),
        "prism" => {
            let pools = TABLE14.parts.get("PRISM_POOLS").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
            let mut layers: Vec<String> = vec!["radial-gradient(ellipse 36% 30% at 58% 36%,rgba(255,255,255,.1),transparent 62%)".into(), "radial-gradient(ellipse 34% 26% at 50% 60%,rgba(5,5,12,.55),transparent 75%)".into()];
            for (k, p) in pools.iter().enumerate() {
                let n = |i: usize| p.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
                layers.push(format!("radial-gradient(ellipse {}% {}% at {}% {}%,{},transparent 70%)", 30 + k % 3 * 4, 26 + k % 2 * 6, (n(0) * 100.0).round(), (n(1) * 100.0).round(), whole(n(2) + sh * 0.15, 92, 55, "0.34")));
            }
            layers.push("linear-gradient(180deg,#07061a,#05050c 60%,#030308)".into());
            layers.join(",")
        }
        "holo" => [
            "radial-gradient(ellipse 30% 13% at 50% 77%,rgba(2,6,16,.6),transparent 75%)".into(),
            format!("{} 0 100%/100% 43% no-repeat", new("HOLO_GRID")),
            format!("radial-gradient(ellipse 55% 7% at 50% 57%,{},transparent 70%)", whole(pull(sh, 195.0, 0.25), 95, 66, "0.5")),
            "radial-gradient(ellipse 80% 40% at 50% 60%,rgba(40,160,255,.3),transparent 70%)".into(),
            "repeating-linear-gradient(0deg,rgba(120,230,255,.025) 0 1px,transparent 1px 4px)".into(),
            "linear-gradient(180deg,#02040e 0%,#05132a 45%,#061a33 56.8%,#030a18 57.4%,#020610 100%)".into(),
        ]
        .join(","),
        "starfield" => [
            "radial-gradient(ellipse 34% 14% at 50% 77%,rgba(0,0,4,.55),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 26% at 50% 48%,rgba(1,2,8,.5),transparent 75%)".into(),
            "radial-gradient(ellipse 20% 15% at 50% 38%,rgba(190,210,255,.13),transparent 70%)".into(),
            format!("radial-gradient(ellipse 62% 46% at 50% 38%,{},transparent 72%)", whole(pull(sh, 225.0, 0.3), 70, 45, "0.13")),
            format!("radial-gradient(ellipse 40% 7% at 50% 58%,{},transparent 72%)", whole(sh, 70, 55, "0.12")),
            format!("{} 50% 50%/cover no-repeat", new("SF_STREAKS")),
            format!("{} 50% 50%/cover no-repeat", new("SF_STARS")),
            "linear-gradient(155deg,transparent 26%,rgba(140,160,230,.05) 40%,rgba(170,150,230,.045) 50%,transparent 64%)".into(),
            "radial-gradient(ellipse 80% 70% at 50% 38%,#060a18,#020309 66%,#000)".into(),
        ]
        .join(","),
        "rain" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(2,4,10,.6),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 28% at 50% 47%,rgba(4,8,18,.5),transparent 75%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("RAIN_GLASS")),
            format!("radial-gradient(ellipse 44% 8% at 50% 58%,{},transparent 72%)", whole(pull(sh, 215.0, 0.4), 70, 55, "0.15")),
            "radial-gradient(ellipse 80% 22% at 50% 60%,rgba(255,160,70,.13),transparent 70%)".into(),
            "radial-gradient(ellipse 70% 40% at 72% 8%,rgba(60,110,210,.22),transparent 70%)".into(),
            "linear-gradient(180deg,#070b16 0%,#0b1426 45%,#111b2e 58%,#080c16 75%,#04060c 100%)".into(),
        ]
        .join(","),
        "sakura" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(12,4,12,.55),transparent 75%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("SAKURA_BRANCH")),
            "radial-gradient(ellipse 46% 28% at 50% 46%,rgba(16,8,26,.45),transparent 75%)".into(),
            "radial-gradient(circle at 77% 14%,rgba(255,240,244,.8) 0 1.4%,rgba(255,200,220,.16) 1.8%,transparent 7%)".into(),
            format!("radial-gradient(ellipse 34% 10% at 50% 57%,{},transparent 70%)", whole(pull(sh, 345.0, 0.4), 85, 72, "0.4")),
            format!("radial-gradient(ellipse 70% 20% at 50% 58%,{},transparent 72%)", whole(pull(sh, 320.0, 0.4), 60, 48, "0.26")),
            "linear-gradient(180deg,#120c28 0%,#24163e 26%,#47264f 42%,#8a4466 52%,#d98299 56.8%,#3a1a36 57.8%,#1a0c1e 72%,#0a0510 100%)".into(),
        ]
        .join(","),
        "snow" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(2,6,14,.55),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 26% at 50% 47%,rgba(4,10,24,.42),transparent 75%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("SNOW_STILL")),
            format!("{} 0 0/100% 100% no-repeat", new("SNOW_HILLS")),
            "radial-gradient(circle at 80% 13%,rgba(240,246,255,.95) 0 1.3%,rgba(200,220,255,.24) 1.7%,transparent 8%)".into(),
            "radial-gradient(ellipse 40% 34% at 80% 10%,rgba(150,185,240,.12),transparent 70%)".into(),
            format!("radial-gradient(ellipse 42% 8% at 50% 57%,{},transparent 72%)", whole(pull(sh, 205.0, 0.4), 65, 62, "0.16")),
            "linear-gradient(180deg,#040a1a 0%,#091631 35%,#11264a 52%,#1a3560 57%,#081222 75%,#040810 100%)".into(),
        ]
        .join(","),
        "eventhorizon" => [
            "radial-gradient(ellipse 30% 12% at 50% 78%,rgba(0,0,0,.5),transparent 75%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("EH")),
            format!("radial-gradient(ellipse 40% 7% at 50% 58%,{},transparent 72%)", whole(pull(sh, 28.0, 0.3), 80, 55, "0.12")),
            format!("{} 50% 50%/cover no-repeat", new("EH_STARS")),
            "radial-gradient(ellipse 75% 65% at 50% 37%,#140b08,#060407 58%,#010102)".into(),
        ]
        .join(","),
        // (the shelf's middle: the mountains stand where the Electron app puts them with the chosen game in the middle)
        "glasslake" => [
            "radial-gradient(ellipse 32% 12% at 50% 77%,rgba(1,5,10,.55),transparent 75%)".into(),
            format!("{} 50% 0/112% 100% no-repeat", new("GL_NEAR")),
            format!("{} 50% 0/106% 100% no-repeat", new("GL_FAR")),
            "linear-gradient(180deg,transparent 57.3%,rgba(150,230,255,.1) 57.55%,transparent 58.6%)".into(),
            format!("radial-gradient(ellipse 42% 7% at 50% 58%,{},transparent 72%)", whole(pull(sh, 165.0, 0.35), 70, 55, "0.14")),
            "radial-gradient(ellipse 50% 20% at 36% 19%,rgba(60,255,170,.16),transparent 70%)".into(),
            "radial-gradient(ellipse 40% 16% at 70% 13%,rgba(90,190,255,.12),transparent 70%)".into(),
            "radial-gradient(ellipse 50% 12% at 36% 92%,rgba(60,255,170,.07),transparent 70%)".into(),
            format!("{} 0 0/100% 50% no-repeat", new("GL_STARS")),
            "linear-gradient(180deg,#02050f 0%,#041128 32%,#06233a 50%,#0c3846 57.5%,#031019 57.6%,#03121d 72%,#01070d 100%)".into(),
        ]
        .join(","),
        "neoncity" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(4,2,12,.6),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 28% at 50% 46%,rgba(6,3,18,.45),transparent 75%)".into(),
            format!("radial-gradient(ellipse 44% 7% at 50% 58%,{},transparent 72%)", whole(pull(sh, 310.0, 0.35), 85, 60, "0.16")),
            "radial-gradient(ellipse 1.3% 15% at 17% 68%,rgba(255,90,200,.16),transparent 70%)".into(),
            "radial-gradient(ellipse 1.1% 12% at 84% 66%,rgba(80,224,255,.14),transparent 70%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("NC_SKY")),
            "conic-gradient(from -32deg at 23% 60%,transparent 0deg,rgba(170,140,255,.07) 2deg,transparent 5deg,transparent 360deg)".into(),
            "conic-gradient(from 24deg at 80% 60%,transparent 0deg,rgba(120,210,255,.06) 2deg,transparent 5deg,transparent 360deg)".into(),
            "radial-gradient(ellipse 70% 24% at 50% 57%,rgba(255,60,170,.24),transparent 70%)".into(),
            "linear-gradient(180deg,#05031a 0%,#0c0630 30%,#240c46 46%,#4a1260 55%,#6a1a6a 57.4%,#0b0616 57.6%,#08040f 75%,#030208 100%)".into(),
        ]
        .join(","),
        "deepsea" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(0,4,10,.55),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 28% at 50% 46%,rgba(1,8,20,.45),transparent 75%)".into(),
            format!("radial-gradient(ellipse 42% 8% at 50% 58%,{},transparent 72%)", whole(pull(sh, 195.0, 0.4), 75, 55, "0.15")),
            "radial-gradient(ellipse 2.2% 3% at 13% 30%,rgba(255,140,220,.3),transparent 70%)".into(),
            "radial-gradient(ellipse 1.7% 2.3% at 87% 72%,rgba(110,230,255,.26),transparent 70%)".into(),
            "radial-gradient(ellipse 1.3% 1.8% at 29% 80%,rgba(110,230,255,.2),transparent 70%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("DS_SNOW")),
            "conic-gradient(from 158deg at 50% -18%,transparent 0deg,rgba(150,230,255,.08) 4deg,transparent 9deg,transparent 15deg,rgba(150,230,255,.06) 18deg,transparent 23deg,rgba(170,240,255,.07) 29deg,transparent 35deg,transparent 360deg)".into(),
            "radial-gradient(ellipse 70% 30% at 50% -4%,rgba(110,210,255,.3),transparent 70%)".into(),
            "linear-gradient(180deg,#0b4a6c 0%,#073656 18%,#04223e 40%,#03172c 57%,#020d1a 75%,#01050c 100%)".into(),
        ]
        .join(","),
        "storm" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(2,3,6,.55),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 26% at 50% 46%,rgba(6,8,14,.42),transparent 75%)".into(),
            format!("radial-gradient(ellipse 42% 7% at 50% 58%,{},transparent 72%)", whole(pull(sh, 220.0, 0.4), 50, 60, "0.14")),
            "repeating-linear-gradient(104deg,transparent 0 9px,rgba(170,190,230,.035) 9px 10px)".into(),
            format!("{} 50% 50%/cover no-repeat", new("ST_SKY")),
            "radial-gradient(ellipse 60% 18% at 50% 56%,rgba(120,140,190,.16),transparent 72%)".into(),
            "linear-gradient(180deg,#0c111d 0%,#182031 30%,#283247 48%,#39435c 56.5%,#0b0e16 57.6%,#080a10 75%,#040508 100%)".into(),
        ]
        .join(","),
        "fireflies" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(2,5,3,.55),transparent 75%)".into(),
            "radial-gradient(ellipse 46% 26% at 50% 46%,rgba(6,8,22,.4),transparent 75%)".into(),
            format!("radial-gradient(ellipse 42% 7% at 50% 58%,{},transparent 72%)", whole(pull(sh, 70.0, 0.35), 70, 58, "0.14")),
            format!("{} 50% 50%/cover no-repeat", new("FF_LAND")),
            format!("{} 0 0/100% 42% no-repeat", new("FF_STARS")),
            "radial-gradient(ellipse 60% 14% at 50% 57%,rgba(255,190,120,.35),transparent 70%)".into(),
            "linear-gradient(180deg,#070c26 0%,#121a46 26%,#2a2c5e 40%,#5c4870 49%,#b07a6c 54.5%,#f0b27a 57%,#0a1209 57.6%,#081008 72%,#040704 100%)".into(),
        ]
        .join(","),
        "desertnight" => [
            "radial-gradient(ellipse 32% 13% at 50% 77%,rgba(2,3,10,.55),transparent 75%)".into(),
            format!("{} 0 0/100% 100% no-repeat", new("DN_DUNES")),
            "radial-gradient(ellipse 46% 24% at 50% 42%,rgba(3,5,16,.4),transparent 75%)".into(),
            format!("radial-gradient(ellipse 44% 8% at 50% 54%,{},transparent 72%)", whole(pull(sh, 230.0, 0.35), 60, 60, "0.14")),
            new("DN_MOON").to_string(),
            format!("{} 0 0/100% 58% no-repeat", new("DN_STARS")),
            "linear-gradient(152deg,transparent 30%,rgba(170,170,230,.05) 42%,rgba(200,180,230,.06) 50%,transparent 62%)".into(),
            "linear-gradient(180deg,#02040f 0%,#060d26 30%,#0f1c44 46%,#26346a 54%,#3c4478 58%,#0b0f22 70%,#05070f 100%)".into(),
        ]
        .join(","),
        "pastelpop" => [
            "radial-gradient(ellipse 32% 12% at 50% 77%,rgba(60,30,90,.32),transparent 75%)".into(),
            format!("{} 50% 50%/cover no-repeat", new("PP_STILL")),
            format!("radial-gradient(ellipse 40% 14% at 50% 56%,{},transparent 72%)", whole(pull(sh, 300.0, 0.35), 90, 88, "0.55")),
            "radial-gradient(ellipse 55% 35% at 12% 8%,rgba(255,214,240,.85),transparent 70%)".into(),
            "radial-gradient(ellipse 50% 40% at 92% 18%,rgba(190,240,255,.7),transparent 70%)".into(),
            "radial-gradient(ellipse 70% 30% at 50% 100%,rgba(200,250,232,.55),transparent 72%)".into(),
            "linear-gradient(180deg,#86aaf8 0%,#a08ff2 30%,#c89aee 50%,#f2a2d6 66%,#ffc0c4 82%,#ffd7b4 100%)".into(),
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
        "swirl" | "forest" => (0.34, 1.0, 1.0),
        "oled" => (0.16, 0.0, 0.45),
        "nebula" | "prism" | "rain" | "snow" | "glasslake" | "neoncity" | "deepsea" => (0.38, 1.0, 1.0),
        "abyss" | "holo" => (0.42, 1.0, 1.0),
        "ember" | "dusk" | "storm" | "fireflies" | "desertnight" => (0.32, 1.0, 1.0),
        "starfield" | "eventhorizon" => (0.3, 1.0, 1.0),
        "sakura" => (0.34, 1.0, 1.0),
        "pastelpop" => (0.22, 1.0, 1.0),
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
    matches!(id_of(id), "arcade" | "cube" | "blueshift" | "swirl" | "forest" | "synth")
        || TABLE14.extra.iter().skip_while(|t| t.id != "nebula").any(|t| t.id == id_of(id))
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

/// A custom theme's picture as the backdrop takes it: red, green and blue a pixel, `size` pixels; how much darker
/// it's made (0 to 0.9), whether it's softened, how bright the floor's glow is, and the hue of that glow.
pub struct Wall<'a> {
    pub rgb: &'a [u8],
    pub size: (usize, usize),
    pub shade: f64,
    pub blur: bool,
    pub glow: f64,
    pub hue: f64,
}

/// The hue of a "#rrggbb" colour, in whole degrees; 225 for a grey or for what isn't one (app.js hexHue).
pub fn hex_hue(hex: &str) -> f64 {
    let digits = hex.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return 225.0;
    }
    let part = |i: usize| f64::from(u8::from_str_radix(&digits[i..i + 2], 16).unwrap_or(0)) / 255.0;
    let (r, g, b) = (part(0), part(2), part(4));
    let (most, least) = (r.max(g).max(b), r.min(g).min(b));
    let d = most - least;
    if d == 0.0 {
        return 225.0;
    }
    let h = if most == r { ((g - b) / d) % 6.0 } else if most == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    ((h * 60.0 + 360.0) % 360.0 + 0.5).floor()
}

/// How strong the wash of the chosen game's cover is over a theme's backdrop: the themes whose backdrop is a
/// designed picture leave it out, and in Blur it's the whole picture (style.css and themes-extra.css, keep in
/// sync).
fn wash(id: &str) -> f32 {
    match id {
        "spectrum" | "classic" | "artwork" => 0.16,
        "blur" => 0.85,
        _ => 0.0,
    }
}

/// Whether the chosen game's cover shows in a theme's backdrop at all.
pub fn washes(id: &str) -> bool {
    wash(id_of(id)) > 0.0
}

/// The chosen game's own picture behind the shelf in the Artwork theme (red, green and blue a pixel, and its
/// size): wide art fills the screen; a cover stands at the right, faint, over the theme's wash.
pub enum Art<'a> {
    Wide(&'a [u8], (usize, usize)),
    Poster(&'a [u8], (usize, usize)),
}

/// Everything that lies still behind the shelf, painted `width` × `height` pixels: the theme's backdrop, a wash of
/// the chosen game's cover over it (`cover`: its swatch), the dark corners, and the floor's glow under the cases.
/// `wall`: a custom theme's picture, which is the backdrop then, under its shade. `art`: the game's own picture,
/// which takes the wash's place.
#[allow(clippy::too_many_arguments)]
pub fn backdrop(id: &str, sh: f64, hue: f64, song: Option<f64>, cover: Option<&[u8]>, wall: Option<&Wall>, art: Option<&Art>, stage: Stage, (width, height): (u32, u32)) -> crate::drawn::Picture {
    let id = id_of(id);
    let (mut floor, corners, _) = strengths(id);
    let whole = (0.0, 0.0, stage.w, stage.h);
    let mut c = crate::cssbg::Canvas::new(width, height, width as f32 / stage.w);
    c.paint("#07070c", whole, 1.0);
    let mut lit = glow(id, sh, song);
    match wall {
        Some(w) => {
            c.paint("#0b0c14", whole, 1.0);
            // a softened picture is laid a little larger, as the browser lays its blur
            let grown = if w.blur { (-0.04 * stage.w, -0.04 * stage.h, 1.08 * stage.w, 1.08 * stage.h) } else { whole };
            c.stretch(w.rgb, w.size, grown, 1.0);
            let dark = |v: f64| format!("rgba(0,0,0,{:.2})", v.min(0.95));
            c.paint(&format!("linear-gradient(180deg,{},{} 35%,{} 60%,{})", dark(w.shade + 0.15), dark(w.shade * 0.5), dark(w.shade * 0.6), dark(w.shade + 0.3)), whole, 1.0);
            (floor, lit) = (w.glow as f32, hsl(w.hue, 95, 62, "0.62"));
        }
        None => {
            c.paint(&tint(id, sh, hue, song), whole, 1.0);
            match (art, cover) {
                (Some(Art::Wide(rgb, size)), _) => c.stretch(rgb, *size, whole, 1.0),
                (Some(Art::Poster(rgb, size)), _) => {
                    let across = stage.h * size.0 as f32 / size.1.max(1) as f32;
                    c.faded(rgb, *size, (stage.w * 0.96 - across, 0.0, across, stage.h), 0.38);
                }
                (None, Some(rgb)) if wash(id) > 0.0 => c.stretch(rgb, SWATCH, (-0.15 * stage.w, -0.15 * stage.h, 1.3 * stage.w, 1.3 * stage.h), wash(id)),
                (None, _) => {}
            }
            // the shelf and what's written stay readable over busy art: darker at the top and the foot, a soft pool
            // behind the cases
            if art.is_some() {
                c.paint("linear-gradient(180deg,rgba(0,0,0,.78),rgba(0,0,0,.5) 28%,rgba(0,0,0,.52) 60%,rgba(0,0,0,.8)),radial-gradient(ellipse 60% 50% at 50% 45%,rgba(0,0,0,.25),transparent 70%)", whole, 1.0);
            }
            if id == "arcade" {
                c.paint("repeating-linear-gradient(0deg,rgba(0,0,0,.22) 0 1px,transparent 1px 3px)", whole, 1.0);
                c.paint("radial-gradient(ellipse 120% 100% at 50% 50%,transparent 60%,rgba(0,0,0,.55))", whole, 1.0);
            }
        }
    }
    c.paint(corners_css(id), whole, corners);
    let wide = (stage.w * 1.4).min(1400.0);
    c.paint(
        &format!("radial-gradient(ellipse 18% 26% at 50% 16%,{lit},transparent 75%),radial-gradient(ellipse 55% 34% at 50% 26%,rgba(0,0,0,.55),transparent 72%)"),
        ((stage.w - wide) / 2.0, stage.h * 0.42 + stage.case_h / 2.0 - 10.0, wide, 160.0),
        floor,
    );
    c.picture()
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    fn painted(id: &str, cover: Option<&[u8]>, art: Option<&Art>) -> Vec<u8> {
        backdrop(id, 250.0, 250.0, None, cover, None, art, Stage { w: 320.0, h: 180.0, case_h: 70.0 }, (64, 36)).rgba
    }

    #[test]
    fn a_covers_wash_is_only_on_the_themes_that_show_it() {
        let red: Vec<u8> = [255, 0, 0].repeat(SWATCH.0 * SWATCH.1);
        for (id, shows) in [("spectrum", true), ("classic", true), ("blur", true), ("artwork", true), ("midnight", false), ("aurora", false), ("keys", false), ("arcade", false), ("oled", false), ("synth", false)] {
            assert_eq!(painted(id, Some(&red), None) != painted(id, None, None), shows, "{id}");
        }
        // in Blur the cover is most of the picture
        let at = |p: &[u8], x: usize, y: usize| [p[(y * 64 + x) * 4], p[(y * 64 + x) * 4 + 1], p[(y * 64 + x) * 4 + 2]];
        let (blur, spectrum) = (painted("blur", Some(&red), None), painted("spectrum", Some(&red), None));
        assert!(at(&blur, 10, 12)[0] > 120 && at(&blur, 10, 12)[2] < 40, "{:?}", at(&blur, 10, 12));
        assert!(at(&spectrum, 10, 12)[0] < 120);
    }

    #[test]
    fn artwork_is_the_games_own_picture_under_a_shade() {
        let at = |p: &[u8], x: usize, y: usize| [p[(y * 64 + x) * 4], p[(y * 64 + x) * 4 + 1], p[(y * 64 + x) * 4 + 2]];
        let green: Vec<u8> = [0, 255, 0].repeat(16 * 9);
        let wide = painted("artwork", None, Some(&Art::Wide(&green, (16, 9))));
        // everywhere, darker at the top than in the middle
        for (x, y) in [(2, 2), (32, 14), (60, 30)] {
            let [r, g, b] = at(&wide, x, y);
            assert!(g > 20 && r < 12 && b < 12, "{x},{y}: {r} {g} {b}");
        }
        assert!(at(&wide, 5, 1)[1] < at(&wide, 5, 14)[1]);
        // a cover stands at the right and leaves the left to the theme
        let tall: Vec<u8> = [0, 255, 0].repeat(10 * 14);
        let (poster, plain) = (painted("artwork", None, Some(&Art::Poster(&tall, (10, 14)))), painted("artwork", None, Some(&Art::Poster(&[0; 3], (1, 1)))));
        assert!(at(&poster, 50, 14)[1] > at(&plain, 50, 14)[1] + 10);
        assert_eq!(at(&poster, 8, 14), at(&plain, 8, 14));
    }
}

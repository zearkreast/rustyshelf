// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the table is
// Boxshelf's (renderer/app.js STYLE and WORDMARKS, renderer/systems-extra.js), as RustyShelf has it.
//! Each shelf's looks: the shape and colours of its games' cases, the backdrop's tint, its name on the shelf. The
//! table is `data/looks.json`, from the Electron app's own.

use std::collections::HashMap;
use std::sync::LazyLock;

use serde::Deserialize;

use crate::scanner::{Game, Source};

/// A colour: red, green, blue and how solid it is, each 0 to 1 (sRGB, as CSS writes them).
pub type Rgba = [f32; 4];

/// The strip a case carries its console's name on.
#[derive(Debug, Clone, Deserialize)]
pub struct Band {
    #[serde(deserialize_with = "css")]
    pub bg: Rgba,
    #[serde(deserialize_with = "css")]
    pub fg: Rgba,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Look {
    pub id: String,
    /// a case's width for its height
    pub ratio: f32,
    /// its thickness for its width
    pub depth: f32,
    /// its height against a keep case's (a 3DS case is shorter)
    #[serde(default)]
    pub h: Option<f32>,
    /// the border of case round the printed sheet, for the case's width
    #[serde(default)]
    pub edge: f32,
    /// the sheet runs on round the spine: no border on that side
    #[serde(default, deserialize_with = "flag")]
    pub wrap: bool,
    /// how round its corners are, for its width
    #[serde(default)]
    pub rc: Option<f32>,
    #[serde(deserialize_with = "css")]
    pub case_color: Rgba,
    #[serde(deserialize_with = "css")]
    pub case_side: Rgba,
    #[serde(deserialize_with = "css")]
    pub spine_ink: Rgba,
    #[serde(default)]
    pub band: Option<Band>,
    #[serde(default)]
    pub spine_band: Option<Band>,
    /// the backdrop behind the shelf, top and bottom
    #[serde(deserialize_with = "pair")]
    pub tint: [Rgba; 2],
    #[serde(deserialize_with = "css")]
    pub glow: Rgba,
    /// the colour its shelf's backdrop is washed in; a shelf without one takes the chosen game's
    #[serde(default)]
    pub hue: Option<f32>,
}

/// A case on the shelf, each measure for its height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub depth: f32,
    /// the border of case round the printed sheet
    pub edge: f32,
    /// the border on the spine's side: none where the sheet runs on round the spine
    pub edge_in: f32,
    /// the corners' radius
    pub rc: f32,
}

/// The consoles whose games open into the app's one keep case model (the Electron app's renderer/inspect3d.js
/// KEEP_CASES without its CASE_MODELS, keep in sync): their cases stand on the shelf in that model's shape.
const KEEP_CASE: [&str; 9] = ["ps3", "ps4", "ps5", "xbox", "x360", "xboxone", "sw", "sw2", "psp"];

impl Look {
    /// A case's width, thickness and border, each for its height.
    pub fn shape(&self) -> (f32, f32, f32) {
        (self.ratio, self.ratio * self.depth, self.ratio * self.edge)
    }

    /// The case as it stands on the shelf `shelf`, the one its game opens from.
    pub fn on_shelf(&self, shelf: &str) -> Shape {
        if KEEP_CASE.contains(&shelf) {
            return Shape { depth: 0.066, edge: 0.024, edge_in: 0.0, rc: 0.02 };
        }
        let edge = self.ratio * self.edge;
        Shape { depth: self.ratio * self.depth, edge, edge_in: if self.wrap { 0.0 } else { edge }, rc: self.ratio * self.rc.unwrap_or(0.03) }
    }

    /// The printed sheet's colour where no art covers it: white behind a clear case, else the case's own.
    pub fn paper(&self) -> Rgba {
        if self.id == "sw" || self.case_color[3] < 1.0 || self.case_side[3] < 1.0 {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            self.case_color
        }
    }
}

/// "#rrggbb", "#rgb" or "rgba(r, g, b, a)"; black for anything else.
pub fn parse_css(text: &str) -> Rgba {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('#') {
        let digit = |i: usize, n: usize| u8::from_str_radix(hex.get(i..i + n).unwrap_or("0"), 16).unwrap_or(0);
        return match hex.len() {
            3 => [digit(0, 1) * 17, digit(1, 1) * 17, digit(2, 1) * 17].map(|c| f32::from(c) / 255.0).into_iter().chain([1.0]).collect::<Vec<_>>().try_into().unwrap_or([0.0, 0.0, 0.0, 1.0]),
            _ => [f32::from(digit(0, 2)) / 255.0, f32::from(digit(2, 2)) / 255.0, f32::from(digit(4, 2)) / 255.0, 1.0],
        };
    }
    if let Some(inner) = t.strip_prefix("rgba(").or(t.strip_prefix("rgb(")).and_then(|x| x.strip_suffix(')')) {
        let parts: Vec<f32> = inner.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() >= 3 {
            return [parts[0] / 255.0, parts[1] / 255.0, parts[2] / 255.0, parts.get(3).copied().unwrap_or(1.0)];
        }
    }
    [0.0, 0.0, 0.0, 1.0]
}

fn css<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Rgba, D::Error> {
    Ok(parse_css(&String::deserialize(d)?))
}

fn pair<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[Rgba; 2], D::Error> {
    let [a, b] = <[String; 2]>::deserialize(d)?;
    Ok([parse_css(&a), parse_css(&b)])
}

/// The table writes a switch as 1.
fn flag<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_bool().unwrap_or_else(|| v.as_f64().is_some_and(|n| n != 0.0)))
}

/// How a shelf's name is lettered: the letters' gradient (as CSS), the glow under them, the colour of the line
/// below; their font and weight, how far apart they stand (for their size), and their size for the window (least,
/// hundredths of the window's width, most).
#[derive(Debug, Clone, Deserialize)]
pub struct Lettering {
    pub grad: String,
    #[serde(deserialize_with = "css")]
    pub glow: Rgba,
    #[serde(deserialize_with = "css")]
    pub tag: Rgba,
    pub font: String,
    pub weight: u32,
    pub spacing: f32,
    pub size: [f32; 3],
    /// in capitals
    pub upper: bool,
}

impl Lettering {
    /// The letters' size in a window `window_w` wide.
    pub fn size_in(&self, window_w: f32) -> f32 {
        (window_w * self.size[1] / 100.0).clamp(self.size[0], self.size[2])
    }
}

/// The boot slide's colours as the style sheet has them for a console: only what it changes.
#[derive(Deserialize, Default, Clone)]
struct BootVars {
    bg: Option<String>,
    ink: Option<String>,
    dim: Option<String>,
    acc: Option<String>,
    word: Option<String>,
}

/// The slide a console's game starts behind: its backdrop (a CSS background), the colours of its words, of its
/// quieter words and of its line, and what the console's name is lettered in there (None: the shelf's own).
#[derive(Debug, Clone, PartialEq)]
pub struct Boot {
    pub bg: String,
    pub ink: Rgba,
    pub dim: Rgba,
    pub acc: Rgba,
    pub word: Option<String>,
}

#[derive(Deserialize)]
struct Table {
    style: HashMap<String, Look>,
    wordmarks: HashMap<String, (String, String)>,
    lettering: HashMap<String, Lettering>,
    boot: HashMap<String, BootVars>,
    media: HashMap<String, Media>,
}

/// What a console's games come on: `kind` "disc", "cart" or "umd", and a `form` for a cartridge that's a floppy disk.
#[derive(Deserialize)]
struct Media {
    kind: String,
    form: Option<String>,
}

/// What a console's games come on, in a word: "Floppy disk", "Cartridge", "UMD" or "Disc".
pub fn media_word(console: &str) -> &'static str {
    match TABLE.media.get(console) {
        Some(m) if m.form.as_deref() == Some("floppy") => "Floppy disk",
        Some(m) if m.kind == "cart" => "Cartridge",
        _ if console == "psp" => "UMD",
        _ => "Disc",
    }
}

/// The same word inside a sentence: "the cartridge", but "the UMD".
pub fn media_word_lower(console: &str) -> String {
    match media_word(console) {
        "UMD" => "UMD".into(),
        w => w.to_lowercase(),
    }
}

static TABLE: LazyLock<Table> = LazyLock::new(|| serde_json::from_str(include_str!("../data/looks.json")).expect("data/looks.json"));

/// The table's shelf whose looks another shelf takes: a collection takes Home's, Game streaming the PC's.
fn styled_as(shelf: &str) -> &str {
    match shelf {
        s if s.starts_with("col-") => "home",
        "streaming" => "pc",
        s => s,
    }
}

/// A shelf's looks; the Media shelf's for a shelf the table doesn't have.
pub fn look(shelf: &str) -> &'static Look {
    TABLE.style.get(styled_as(shelf)).unwrap_or_else(|| &TABLE.style["media"])
}

/// A shelf's name as the shelf writes it large, and the line under it.
pub fn wordmark(shelf: &str) -> Option<(&'static str, &'static str)> {
    TABLE.wordmarks.get(shelf).map(|(a, b)| (a.as_str(), b.as_str()))
}

/// The colours a shelf's name is lettered in; white for a shelf without its own.
pub fn lettering(shelf: &str) -> &'static Lettering {
    TABLE.lettering.get(styled_as(shelf)).unwrap_or_else(|| &TABLE.lettering[""])
}

/// The boot slide of the shelf `shelf`: its console's own, the Switch's for the Switch 2, and for a console
/// without one the shelf's colours.
pub fn boot(shelf: &str) -> Boot {
    let id = if shelf == "sw2" { "sw" } else { shelf };
    let (plain, own) = (&TABLE.boot[""], TABLE.boot.get(id).filter(|_| !id.is_empty() && id != "other"));
    let own_or = |pick: fn(&BootVars) -> &Option<String>| own.and_then(|o| pick(o).clone()).or_else(|| pick(plain).clone()).unwrap_or_default();
    let style = look(shelf);
    let css = |c: Rgba| format!("rgba({},{},{},{})", (c[0] * 255.0).round(), (c[1] * 255.0).round(), (c[2] * 255.0).round(), c[3]);
    match own {
        Some(o) => Boot { bg: own_or(|b| &b.bg), ink: parse_css(&own_or(|b| &b.ink)), dim: parse_css(&own_or(|b| &b.dim)), acc: parse_css(&own_or(|b| &b.acc)), word: o.word.clone() },
        None => Boot {
            bg: TABLE.boot.get("other").and_then(|o| o.bg.clone()).unwrap_or_default().replace("var(--bs-t0)", &css(style.tint[0])).replace("var(--bs-t1)", &css(style.tint[1])),
            ink: parse_css(&own_or(|b| &b.ink)),
            dim: parse_css(&own_or(|b| &b.dim)),
            acc: style.glow,
            word: None,
        },
    }
}

/// The case a game stands in on the shelf `shelf` (its own shelf, also when it's shown on Home): the shelf's, or
/// its console release's for a PC game shown in a console box. A PC game's case has no border round its cover and
/// carries the PC spine.
pub fn look_for(shelf: &str, g: &Game) -> Look {
    let boxed = g.box_plat.as_deref().and_then(|p| TABLE.style.get(p));
    let base = boxed.unwrap_or_else(|| look(shelf));
    let pc = matches!(g.source, Source::Steam { .. }) || matches!(shelf, "pc" | "custom" | "streaming");
    if pc {
        Look { edge: 0.0, spine_band: look("pc").spine_band.clone(), ..base.clone() }
    } else {
        base.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_table() {
        let gc = look("gc");
        assert_eq!((gc.ratio, gc.depth, gc.edge, gc.wrap, gc.h), (0.712, 0.105, 0.022, false, None));
        assert_eq!(gc.case_color, [22.0 / 255.0, 22.0 / 255.0, 27.0 / 255.0, 1.0]);
        assert_eq!(gc.band.as_ref().map(|b| b.label.as_str()), Some("GAMECUBE"));
        let (w, d, e) = gc.shape();
        assert!((w - 0.712).abs() < 1e-6 && (d - 0.07476).abs() < 1e-6 && (e - 0.015664).abs() < 1e-6);
        // the Switch's clear case, a sheet that wraps, a shorter case
        assert_eq!(look("sw").case_color, [1.0, 1.0, 1.0, 0.22]);
        assert!(look("sw").wrap && look("x360").wrap && !look("ps2").wrap);
        assert_eq!(look("n3ds").h, Some(0.64));
        assert_eq!((look("nothing").id.as_str(), look("col-c1").id.as_str(), look("all").id.as_str()), ("media", "home", "home"));
        assert_eq!(wordmark("gc"), Some(("GameCube", "Home console")));
        assert_eq!(parse_css("#fff"), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(lettering("gc").grad, "linear-gradient(180deg,#e2dbff 10%,#8b75ff 95%)");
        let wii = boot("wii");
        assert_eq!((wii.ink, wii.acc, wii.word.as_deref()), (parse_css("#3b414b"), parse_css("#34bdf2"), Some("linear-gradient(180deg,#9aa1ab,#6c737e)")));
        assert_eq!((boot("sw2"), boot("gc").dim), (boot("sw"), [225.0 / 255.0, 215.0 / 255.0, 1.0, 0.78]));
        let snes = boot("snes");
        assert!(snes.bg.starts_with("radial-gradient(ellipse 80% 70% at 50% 40%,rgba(") && !snes.bg.contains("var("), "{}", snes.bg);
        assert_eq!((snes.acc, snes.ink, snes.word), (look("snes").glow, [1.0; 4], None));
        assert_eq!((lettering("gc").font.as_str(), lettering("gc").size_in(1280.0), lettering("media").upper), ("Chakra Petch", 30.72, false));
        assert_eq!((lettering("ps5").tag, lettering("col-c1").tag), (parse_css("#c8cfdd"), lettering("home").tag));

        let steam = Game { source: Source::Steam { appid: "1".into(), minutes: 0, last_played: 0 }, ..Game::default() };
        let on_pc = look_for("pc", &steam);
        assert_eq!((on_pc.id.as_str(), on_pc.edge), ("pc", 0.0));
        // a PC game in its console box: that console's case, without the border, with the PC spine
        let boxed = look_for("pc", &Game { box_plat: Some("gc".into()), ..steam });
        assert_eq!((boxed.id.as_str(), boxed.edge, boxed.spine_band.map(|b| b.label)), ("gc", 0.0, Some("PC".into())));
        assert_eq!(look_for("gc", &Game::default()).edge, 0.022);
        let streamed = look_for("streaming", &Game { box_plat: Some("gc".into()), ..Game::default() });
        assert_eq!((look("streaming").id.as_str(), streamed.id.as_str(), streamed.edge, streamed.spine_band.map(|b| b.label)), ("pc", "gc", 0.0, Some("PC".into())));
        assert_eq!(wordmark("streaming"), Some(("Streaming", "Games from another PC")));

        let on = gc.on_shelf("gc");
        assert!((on.depth - 0.07476).abs() < 1e-6 && on.edge == on.edge_in && (on.rc - 0.712 * 0.03).abs() < 1e-6);
        assert_eq!(look("ps3").on_shelf("ps3"), Shape { depth: 0.066, edge: 0.024, edge_in: 0.0, rc: 0.02 });
        assert_eq!(look("x360").on_shelf("home").edge_in, 0.0);
        assert_eq!((look("sw").paper(), gc.paper()), ([1.0; 4], gc.case_color));
    }

    #[test]
    fn each_console_s_games_come_on_their_own_media() {
        assert_eq!([media_word("ps2"), media_word("snes"), media_word("sw"), media_word("psp"), media_word("dos"), media_word("fds"), media_word("nothing")], ["Disc", "Cartridge", "Cartridge", "UMD", "Floppy disk", "Floppy disk", "Disc"]);
        assert_eq!((media_word_lower("nds"), media_word_lower("psp")), ("cartridge".to_string(), "UMD".to_string()));
    }
}

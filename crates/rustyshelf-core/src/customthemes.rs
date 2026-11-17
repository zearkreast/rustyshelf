// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/customthemes.js, as RustyShelf has it.
//! Custom themes: a picture behind the shelf, made by the user. Each is a folder in "Custom themes" beside
//! RustyShelf: `background.jpg` (or .png, .webp, .svg; a folder with one picture uses that one), a picture named
//! after a console's id for that console's shelf, and an optional `theme.json` with a name, an accent, how much
//! darker the picture is made, whether it's blurred and how bright the floor's glow is. Only pictures and those few
//! settings: a theme never runs code.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

const MOST_THEMES: usize = 60;
const MOST_JSON: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Custom {
    /// "custom-<its folder's name, plainly>"
    pub id: String,
    pub name: String,
    pub folder: String,
    /// the picture behind every shelf, and the ones for single shelves by their ids
    pub bg: PathBuf,
    pub shelves: BTreeMap<String, PathBuf>,
    /// "#rrggbb" when theme.json gives one: the glow's colour (else it's taken from the picture)
    pub accent: Option<String>,
    /// how much darker the picture is made, 0 to 0.9
    pub shade: f64,
    pub blur: bool,
    /// how bright the light under the shelf is, 0 to 1
    pub glow: f64,
}

/// Where the themes' folders are.
pub fn root(app_root: &Path) -> PathBuf {
    app_root.join("Custom themes")
}

fn is_picture(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".jpg", ".jpeg", ".png", ".webp", ".svg"].iter().any(|e| lower.ends_with(e))
}

fn stem(name: &str) -> &str {
    name.rfind('.').map_or(name, |i| &name[..i])
}

/// A folder's name as an id: lower case, plain letters and digits, a dash between words, 40 at most.
pub fn slug(name: &str) -> String {
    let kept: String = name.to_lowercase().nfkd().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || c.is_whitespace()).collect();
    let dashed = kept.split(|c: char| c.is_whitespace() || c == '_' || c == '-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    dashed.chars().take(40).collect()
}

fn theme_json(dir: &Path) -> Value {
    let file = dir.join("theme.json");
    if fs::metadata(&file).map_or(true, |m| m.len() > MOST_JSON) {
        return json!({});
    }
    crate::files::read_json(&file).filter(Value::is_object).unwrap_or(json!({}))
}

/// Every theme in the folder, by name. `shelf_ids`: the shelves a theme can have its own picture for. The folder
/// is made with three examples the first time.
pub fn list(root: &Path, shelf_ids: &[&str]) -> Vec<Custom> {
    ensure_examples(root);
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut dirs: Vec<String> = entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    dirs.sort();
    let (mut out, mut seen): (Vec<Custom>, Vec<String>) = Default::default();
    for folder in dirs.into_iter().take(MOST_THEMES) {
        let dir = root.join(&folder);
        let Ok(inside) = fs::read_dir(&dir) else { continue };
        let mut files: Vec<String> = inside.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_picture(n)).collect();
        files.sort();
        let Some(bg) = files.iter().find(|n| n.to_lowercase().starts_with("background.")).or(if files.len() == 1 { files.first() } else { None }).cloned() else { continue };
        let j = theme_json(&dir);
        let plain = slug(&folder);
        let mut id = format!("custom-{}", if plain.is_empty() { "theme" } else { &plain });
        let mut n = 2;
        while seen.contains(&id) {
            id = format!("custom-{plain}-{n}");
            n += 1;
        }
        seen.push(id.clone());
        let shelves = files.iter().filter(|n| **n != bg && shelf_ids.contains(&stem(n).to_lowercase().as_str())).map(|n| (stem(n).to_lowercase(), dir.join(n))).collect();
        let number = |key: &str, most: f64, unset: f64| j.get(key).and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).filter(|v| v.is_finite()).map_or(unset, |v| v.clamp(0.0, most));
        let hex = j.get("accent").and_then(Value::as_str).map(str::trim).filter(|a| a.len() == 7 && a.starts_with('#') && a[1..].chars().all(|c| c.is_ascii_hexdigit()));
        out.push(Custom {
            id,
            name: j.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&folder).chars().take(40).collect(),
            folder: folder.clone(),
            bg: dir.join(&bg),
            shelves,
            accent: hex.map(str::to_lowercase),
            shade: number("shade", 0.9, 0.35),
            blur: j.get("blur") == Some(&Value::Bool(true)),
            glow: number("glow", 1.0, 0.35),
        });
    }
    out.sort_by(|a, b| crate::collate::compare(&a.name, &b.name));
    out
}

/// Makes a theme from a picture: a new folder named after the picture (or `name`), the picture copied in as its
/// background, and a theme.json to edit later. The new theme's id.
pub fn create(root: &Path, file: &Path, name: &str) -> Result<String, String> {
    let file_name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !is_picture(&file_name) {
        return Err("Choose a JPG, PNG, WebP or SVG picture.".into());
    }
    if !file.is_file() {
        return Err("That picture is not there any more.".into());
    }
    let from_file = stem(&file_name).split(['_', '-']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let named: String = Some(name.trim()).filter(|n| !n.is_empty()).or(Some(from_file.trim()).filter(|n| !n.is_empty())).unwrap_or("My theme").chars().take(40).collect();
    let base: String = named.chars().filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control()).collect();
    let base = Some(base.trim()).filter(|b| !b.is_empty()).unwrap_or("My theme").to_string();
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut folder = base.clone();
    let mut n = 2;
    while root.join(&folder).exists() {
        folder = format!("{base} {n}");
        n += 1;
    }
    let dir = root.join(&folder);
    fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let ext = file_name.rfind('.').map_or(String::new(), |i| file_name[i..].to_lowercase());
    fs::copy(file, dir.join(format!("background{ext}"))).map_err(|e| e.to_string())?;
    let settings = crate::js::stringify_pretty(&json!({ "name": named, "shade": 0.35, "blur": false, "glow": 0.35 }));
    fs::write(dir.join("theme.json"), settings).map_err(|e| e.to_string())?;
    let plain = slug(&folder);
    Ok(format!("custom-{}", if plain.is_empty() { "theme" } else { &plain }))
}

// ---------- the examples, written once when the folder doesn't exist yet ----------

/// The Electron app's generator for the examples' shapes: the same pictures every time.
struct Seeded(i32);

impl Seeded {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6D2B_79F5);
        let s = self.0 as u32;
        let mut t = (s ^ (s >> 15)).wrapping_mul(1 | s);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// A number to a tenth, written as JavaScript writes it.
fn r1(n: f64) -> String {
    let v = (n * 10.0 + 0.5).floor() / 10.0;
    format!("{}", if v == 0.0 { 0.0 } else { v })
}

fn hills(rnd: &mut Seeded, y0: f64, amp: f64, n: usize) -> String {
    let mut d = format!("M0 1080L0 {}", r1(y0));
    for i in 1..=n {
        let x = i as f64 * 1920.0 / n as f64;
        let y = y0 - amp * (0.4 + 0.6 * rnd.next());
        d += &format!("Q{} {} {} {}", r1(x - 960.0 / n as f64), r1(y - amp * 0.5), r1(x), r1(y0 - amp * 0.2 * rnd.next()));
    }
    d + "L1920 1080Z"
}

const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice"><defs>"#;

fn sunset_hills() -> String {
    let mut rnd = Seeded(11);
    let (far, mid, near) = (hills(&mut rnd, 700.0, 120.0, 5), hills(&mut rnd, 790.0, 140.0, 4), hills(&mut rnd, 900.0, 120.0, 3));
    format!(
        concat!(
            "{}",
            r##"<linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1b1446"/><stop offset=".45" stop-color="#7a2f6b"/><stop offset=".7" stop-color="#e2694a"/><stop offset="1" stop-color="#ffc277"/></linearGradient>"##,
            r##"<radialGradient id="g" cx=".5" cy=".62" r=".35"><stop offset="0" stop-color="#ffe2a6" stop-opacity=".9"/><stop offset="1" stop-color="#ffb070" stop-opacity="0"/></radialGradient></defs>"##,
            r##"<rect width="1920" height="1080" fill="url(#s)"/><circle cx="960" cy="650" r="420" fill="url(#g)"/><circle cx="960" cy="640" r="120" fill="#ffd89a"/>"##,
            r##"<path d="{}" fill="#8a3a5e" opacity=".85"/><path d="{}" fill="#55224a"/><path d="{}" fill="#2a1030"/></svg>"##
        ),
        SVG, far, mid, near
    )
}

fn deep_space() -> String {
    let mut rnd = Seeded(23);
    let mut stars = String::new();
    for _ in 0..260 {
        let z = rnd.next();
        let (x, y) = (rnd.next() * 1920.0, rnd.next() * 1080.0);
        stars += &format!(r#"<circle cx="{}" cy="{}" r="{}" opacity="{}"/>"#, r1(x), r1(y), r1(0.6 + z * z * 1.8), r1(0.3 + z * 0.7));
    }
    format!(
        concat!(
            "{}",
            r##"<radialGradient id="n1" cx=".28" cy=".32" r=".45"><stop offset="0" stop-color="#5b2bb8" stop-opacity=".75"/><stop offset="1" stop-color="#5b2bb8" stop-opacity="0"/></radialGradient>"##,
            r##"<radialGradient id="n2" cx=".74" cy=".58" r=".5"><stop offset="0" stop-color="#1f7ad6" stop-opacity=".6"/><stop offset="1" stop-color="#1f7ad6" stop-opacity="0"/></radialGradient>"##,
            r##"<radialGradient id="p" cx=".35" cy=".3" r=".8"><stop offset="0" stop-color="#ffcf9a"/><stop offset=".55" stop-color="#c0603a"/><stop offset="1" stop-color="#3a1408"/></radialGradient></defs>"##,
            r##"<rect width="1920" height="1080" fill="#05060f"/><rect width="1920" height="1080" fill="url(#n1)"/><rect width="1920" height="1080" fill="url(#n2)"/>"##,
            r##"<g fill="#ffffff">{}</g><circle cx="1560" cy="230" r="120" fill="url(#p)"/><ellipse cx="1560" cy="230" rx="210" ry="34" fill="none" stroke="#f2d2b0" stroke-opacity=".45" stroke-width="5" transform="rotate(-14 1560 230)"/></svg>"##
        ),
        SVG, stars
    )
}

fn ocean_dusk() -> String {
    let mut waves = String::new();
    for i in 0..14 {
        let f = f64::from(i);
        let (y, a, ph) = (640.0 + f * 32.0 + f * f * 1.6, 10.0 + f * 2.2, f * 37.0);
        let mut d = format!("M0 {}", r1(y));
        for x in (0..=1920).step_by(120) {
            d += &format!("L{x} {}", r1(y + ((f64::from(x) + ph) / 140.0).sin() * a));
        }
        waves += &format!(r##"<path d="{d}" fill="none" stroke="#9fe3ff" stroke-opacity="{}" stroke-width="{}"/>"##, r1(0.08 + f * 0.02), r1(1.0 + f * 0.25));
    }
    format!(
        concat!(
            "{}",
            r##"<linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#071a2e"/><stop offset=".5" stop-color="#14506a"/><stop offset=".6" stop-color="#2d8a96"/><stop offset=".61" stop-color="#0a3348"/><stop offset="1" stop-color="#03121e"/></linearGradient>"##,
            r##"<radialGradient id="m" cx=".5" cy=".58" r=".3"><stop offset="0" stop-color="#bff3ff" stop-opacity=".55"/><stop offset="1" stop-color="#bff3ff" stop-opacity="0"/></radialGradient></defs>"##,
            r##"<rect width="1920" height="1080" fill="url(#s)"/><ellipse cx="960" cy="640" rx="620" ry="190" fill="url(#m)"/><circle cx="1320" cy="250" r="54" fill="#e8fbff" opacity=".9"/>{}</svg>"##
        ),
        SVG, waves
    )
}

/// Makes the themes' folder with three example themes and a note on how they work, when it isn't there yet.
pub fn ensure_examples(root: &Path) {
    if root.as_os_str().is_empty() || root.exists() {
        return;
    }
    let examples: [(&str, String, &str, f64, f64); 3] =
        [("Sunset Hills", sunset_hills(), "#e2694a", 0.25, 0.4), ("Deep Space", deep_space(), "#5b6bff", 0.2, 0.35), ("Ocean Dusk", ocean_dusk(), "#2d8a96", 0.25, 0.4)];
    for (name, picture, accent, shade, glow) in examples {
        let dir = root.join(name);
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let settings = crate::js::stringify_pretty(&json!({ "name": name, "accent": accent, "shade": shade, "glow": glow, "blur": false }));
        let _ = fs::write(dir.join("background.svg"), picture);
        let _ = fs::write(dir.join("theme.json"), settings);
    }
    let note = [
        "Each folder here is a theme. Put a picture in it named background (.jpg, .png, .webp or .svg).",
        "For one console only, add a picture named after its shelf: gc, wii, wiiu, sw, sw2, ps2, ps3, ps4, ps5, psp, x360, n3ds, pc.",
        "theme.json (optional): {\"name\": \"My theme\", \"accent\": \"#6b8cff\", \"shade\": 0.35, \"blur\": false, \"glow\": 0.35}",
        "  shade: how much darker the picture is made (0 to 0.9), so the shelf stays easy to read.",
        "  blur: true softens the picture.   glow: how bright the light under the shelf is (0 to 1).",
        "Then in RustyShelf: Settings > Display > Find themes again. Themes are only pictures and settings, never programs.",
        "",
    ]
    .join("\r\n");
    let _ = fs::write(root.join("How custom themes work.txt"), note);
}

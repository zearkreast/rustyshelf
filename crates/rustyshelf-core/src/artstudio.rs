// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/artstudio.js and the candidates of its main/covers.js, as RustyShelf has them.
//! Art Studio: every version of a game's box art the sources know (LaunchBox, libretro, GameTDB, the picture
//! in use and the ones replaced before it), ranked by a look (authentic, HD, a region, special edition) and
//! applied safely: the new file first, the old one moved to `replaced\`, never deleted.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use crate::lbindex::{GameImage, ShardFile};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use crate::covers::{self, LibretroNames};
use crate::{gametdb, js, settings};

pub const PARTS: [&str; 4] = ["front", "back", "spine", "disc"];
const LOOKS: [&str; 6] = ["authentic", "hd", "region:usa", "region:eur", "region:jpn", "special"];
const REGIONS: [&str; 3] = ["usa", "eur", "jpn"];

/// What a part's file name ends with, before its type.
pub fn suffix(part: &str) -> &'static str {
    match part {
        "back" => "-back",
        "spine" => "-spine",
        "disc" => "-disc",
        _ => "",
    }
}

fn sha(s: &str) -> String {
    Sha1::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

// ---------- styles and looks ----------

/// A console's style: its own (`art.styles.<console>`), else the one every console starts with.
pub fn style_for(cfg: &Value, sys: &str) -> Value {
    let base = settings::default_art_style(cfg.get("ui"));
    match cfg.get("art").and_then(|a| a.get("styles")).and_then(|s| s.get(sys)).filter(|s| js::truthy(Some(s))) {
        Some(own) => settings::clean_art_style(own, &base),
        None => base,
    }
}

fn region_of(style: &Value) -> &str {
    style["region"].as_str().filter(|r| REGIONS.contains(r)).unwrap_or("usa")
}

/// The look one part follows under a style: "mix" uses only its parts (a part not set is authentic); the other
/// presets hold for every part.
pub fn look_for(style: &Value, part: &str) -> String {
    let named = |l: Option<&str>| match l {
        Some("region") => Some(format!("region:{}", style["region"].as_str().unwrap_or(""))),
        Some(l) if LOOKS.contains(&l) => Some(l.to_string()),
        _ => None,
    };
    match style["preset"].as_str() {
        Some("mix") => named(style["parts"][part].as_str()).unwrap_or_else(|| "authentic".into()),
        // (a part no complete set covers)
        Some("sets") if js::truthy(style.get("setsHd")) => "hd".into(),
        Some("sets" | "region") => format!("region:{}", region_of(style)),
        Some(preset) if LOOKS.contains(&preset) => preset.into(),
        _ => "authentic".into(),
    }
}

/// A game's own region from its file name's tags ("(USA)", "(Europe)", "(Japan)").
pub fn game_region(region: &str) -> &'static str {
    static USA: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)usa|canada|america").unwrap());
    static EUR: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)europe|uk|germany|france|spain|italy|australia|netherlands|sweden|scandinavia").unwrap());
    if USA.is_match(region) {
        "usa"
    } else if region.to_lowercase().contains("japan") {
        "jpn"
    } else if EUR.is_match(region) {
        "eur"
    } else {
        ""
    }
}

/// Authentic is the release of the copy you own (its region), unless an explicit region choice said otherwise.
fn pref_region(style: &Value, game: &str) -> String {
    let own = if js::truthy(style.get("regionFirst")) { "" } else { game_region(game) };
    if !own.is_empty() {
        return own.into();
    }
    style["region"].as_str().filter(|r| !r.is_empty()).unwrap_or("usa").into()
}

/// The region a look asks for.
pub fn region_for_look(style: &Value, look: &str, game: &str) -> String {
    match look.strip_prefix("region:").filter(|r| REGIONS.contains(r)) {
        Some(r) => r.into(),
        None if look == "authentic" => pref_region(style, game),
        None => style["region"].as_str().filter(|r| !r.is_empty()).unwrap_or("usa").into(),
    }
}

// ---------- the candidates ----------

/// One version of a part of a game's box art.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cand {
    /// what the screen names it by
    pub key: String,
    pub url: String,
    /// the type it's kept as, with its dot; and the type it comes down as, when it's cut from a bigger picture
    pub ext: String,
    pub src: Option<String>,
    /// "launchbox", "libretro", "gametdb", "steamgriddb", "current", "earlier" …
    pub source: String,
    /// the source's own name for the kind of picture ("Box - Front", "coverHQ" …)
    pub kind_of: String,
    /// "usa", "eur", "jpn", "world" or ""
    pub region: String,
    pub region_name: String,
    /// "original", "reprint" or "special"
    pub edition: String,
    /// "scan", "clean" or "hd"
    pub kind: String,
    pub fan: bool,
    /// "spine" or "back": cut from a full wrap
    pub cut: Option<String>,
    /// a libretro cover named exactly as the game's file
    pub exact: bool,
    /// an address that's built, not listed: shown once it has really downloaded
    pub guess: bool,
    pub label: String,
    /// libretro's name for it
    pub name: Option<String>,
    /// a picture on this computer: the one in use, or an earlier one
    pub file: Option<PathBuf>,
    pub w: Option<u32>,
    pub h: Option<u32>,
    pub bytes: u64,
    pub cached: bool,
    pub in_use: bool,
    /// the same picture as another of the list, by that one's key
    pub dup: Option<String>,
    /// SteamGridDB's own order
    pub rank: f64,
}

impl Cand {
    /// As the Electron app's lists have it, for the sources' part of it.
    pub fn to_json(&self) -> Value {
        let mut o = json!({ "url": self.url, "ext": self.ext, "source": self.source, "type": self.kind_of, "region": self.region, "edition": self.edition, "kind": self.kind, "label": self.label });
        let m = o.as_object_mut().expect("an object");
        if self.source == "launchbox" {
            m.extend([("regionName".to_string(), json!(self.region_name)), ("fan".to_string(), json!(self.fan)), ("cut".to_string(), json!(self.cut)), ("src".to_string(), json!(self.src))]);
        } else {
            if let Some(cut) = &self.cut {
                m.extend([("cut".to_string(), json!(cut)), ("src".to_string(), json!(self.src))]);
            }
            if self.source == "libretro" {
                m.extend([("name".to_string(), json!(self.name)), ("exact".to_string(), json!(self.exact))]);
            }
            if self.source == "gametdb" && self.name.as_deref() == Some("disc id") {
                m.insert("guess".into(), json!(self.guess));
            }
        }
        o
    }
}

fn region_word(region: &str) -> &'static str {
    match region {
        "usa" => "USA",
        "eur" => "Europe",
        "jpn" => "Japan",
        "world" => "World",
        _ => "",
    }
}

fn label(bits: &[&str]) -> String {
    bits.iter().filter(|b| !b.is_empty()).copied().collect::<Vec<_>>().join(" · ")
}

fn title_case(s: &str) -> String {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?-u:\b)[a-z]").unwrap());
    static OWNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"'S(?-u:\b)").unwrap());
    let upper = WORD.replace_all(s, |m: &regex::Captures| m[0].to_uppercase());
    OWNS.replace_all(&upper, "'s").into_owned()
}

fn ext_of(file: &str) -> String {
    let ext = js::extname(file).to_lowercase();
    if matches!(ext.as_str(), ".png" | ".jpg" | ".jpeg" | ".webp") { ext } else { ".jpg".into() }
}

/// A picture of LaunchBox's as a candidate.
pub fn lb_cand(im: &GameImage) -> Cand {
    let kind_of = im.r#type.as_str();
    let (fan, recon, full) = (kind_of.starts_with("Fanart") || kind_of.starts_with("Steam Poster"), kind_of.contains("Reconstructed"), kind_of == "Box - Full");
    let region = covers::lb_region(&im.region);
    let what = if full {
        "Cut from full box"
    } else if fan {
        "Fan art"
    } else if recon {
        "Clean rebuild"
    } else if kind_of.starts_with("Cart") || kind_of.starts_with("Fanart - Cart") {
        "Cartridge"
    } else {
        "Box scan"
    };
    Cand {
        url: format!("{}{}", covers::LB_IMG, js::encode_uri(&im.file)),
        ext: if full { ".jpg".into() } else { ext_of(&im.file) },
        src: full.then(|| ext_of(&im.file)),
        source: "launchbox".into(),
        kind_of: kind_of.into(),
        region: region.into(),
        region_name: im.region.clone(),
        edition: "original".into(),
        kind: if recon || fan { "clean" } else { "scan" }.into(),
        fan,
        cut: full.then(|| "spine".to_string()),
        label: label(&[what, Some(region_word(region)).filter(|w| !w.is_empty()).unwrap_or(&im.region)]),
        ..Cand::default()
    }
}

/// One of libretro's covers as a candidate. `base`: the name the game's art has.
pub fn lr_cand(sys: &str, name: &str, base: &str) -> Cand {
    static TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(([^)]*)\)|\[([^\]]*)\]").unwrap());
    // only the bracketed tags: "… Deluxe" in a title is no edition
    let tags = TAGS.find_iter(name).map(|m| m.as_str()).collect::<Vec<_>>().join(" ");
    let (edition, word) = covers::edition(&tags);
    let region = covers::lr_region(name);
    Cand {
        url: covers::libretro_url(sys, name).unwrap_or_default(),
        ext: ".png".into(),
        source: "libretro".into(),
        kind_of: "Named_Boxarts".into(),
        name: Some(name.into()),
        region: region.into(),
        edition: edition.into(),
        kind: "clean".into(),
        exact: !base.is_empty() && (name == base || name == covers::libretro_safe(base)),
        label: label(&[&if word.is_empty() { "Clean cover".to_string() } else { title_case(&word) }, region_word(region)]),
        ..Cand::default()
    }
}

/// What the candidates are made from: each source as it's kept on this computer. Nothing is asked of the web.
#[derive(Default)]
pub struct Sources<'a> {
    /// the console's part of the LaunchBox index
    pub launchbox: Option<&'a ShardFile>,
    /// libretro's list of the console's covers
    pub libretro: Option<&'a LibretroNames>,
    /// GameTDB's games by title (Switch, 3DS)
    pub gametdb: Option<&'a serde_json::Map<String, Value>>,
    /// a GameCube or Wii disc's id
    pub disc_id: Option<String>,
    /// the box art region chosen
    pub region: &'a str,
}

/// For one part of one game, every version the sources have, in the sources' own order.
pub fn candidates(sys: &str, title: &str, file: &Path, part: &str, from: &Sources) -> Vec<Cand> {
    static JOINED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s(?:\+|&|and)\s").unwrap());
    let mut out: Vec<Cand> = Vec::new();
    if let Some(shard) = from.launchbox {
        let found = covers::launchbox_match(&shard.games, &shard.images, title).or_else(|| JOINED.split(title).next().filter(|_| JOINED.is_match(title)).and_then(|first| covers::launchbox_match(&shard.games, &shard.images, first)));
        let imgs: &[GameImage] = found.and_then(|g| shard.images.get(&g.id)).map_or(&[], Vec::as_slice);
        let types: &[&str] = match part {
            "front" => &["Box - Front", "Box - Front - Reconstructed", "Fanart - Box - Front", "Steam Poster"],
            "back" => &["Box - Back", "Box - Back - Reconstructed", "Fanart - Box - Back"],
            "spine" => &["Box - Spine", "Box - Full"],
            "disc" => &["Disc", "Cart - Front", "Fanart - Cart - Front", "Fanart - Disc"],
            _ => &[],
        };
        for t in types {
            out.extend(imgs.iter().filter(|im| im.r#type == *t && !im.file.is_empty()).map(lb_cand));
        }
    }
    if let (Some(names), "front") = (from.libretro, part) {
        let base = crate::art::art_base(file);
        out.extend(names.all(file, title).iter().map(|n| lr_cand(sys, n, &base)));
    }
    let tdb_sys = match sys {
        "gc" | "wii" => Some("wii"),
        "wiiu" => Some("wiiu"),
        _ => None,
    };
    if let (Some(tdb), Some(id)) = (tdb_sys, &from.disc_id) {
        let (root, own) = (format!("https://art.gametdb.com/{tdb}"), covers::tdb_own(id));
        // the disc's own regions are real for the cover, the full cover and the disc; another region, and the
        // clean disc artwork, are guesses
        for r in covers::tdb_regions(id, from.region) {
            let (region, guess) = (covers::tdb_region(r), !own.contains(&r));
            let word = Some(region_word(region)).filter(|w| !w.is_empty()).unwrap_or(r);
            let mut add = |folder: &str, ext: &str, kind: &str, what: &str, cut: Option<&str>, guess: bool| {
                out.push(Cand {
                    url: format!("{root}/{folder}/{r}/{id}.png"),
                    ext: ext.into(),
                    src: cut.map(|_| ".png".to_string()),
                    source: "gametdb".into(),
                    kind_of: folder.into(),
                    region: region.into(),
                    edition: "original".into(),
                    kind: kind.into(),
                    cut: cut.map(String::from),
                    label: label(&[what, word]),
                    guess,
                    name: Some("disc id".into()),
                    ..Cand::default()
                });
            };
            match part {
                "front" => {
                    add("coverHQ", ".png", "hd", "HD cover", None, guess);
                    add("cover", ".png", "scan", "Cover", None, guess);
                }
                "back" => {
                    add("coverfullHQ", ".jpg", "hd", "From GameTDB's full cover", Some("back"), guess);
                    add("coverfull", ".jpg", "scan", "From GameTDB's full cover", Some("back"), guess);
                }
                "spine" => {
                    add("coverfullHQ", ".jpg", "hd", "Cut from full cover", Some("spine"), guess);
                    add("coverfull", ".jpg", "scan", "Cut from full cover", Some("spine"), guess);
                }
                "disc" => {
                    add("disc", ".png", "scan", "Disc", None, guess);
                    add("discM", ".png", "clean", "Disc artwork", None, true);
                }
                _ => {}
            }
        }
    }
    // Switch / 3DS: GameTDB's scans found by title (the front, and the back and spine cut from its full cover)
    if let (Some(map), true, true) = (from.gametdb, gametdb::has(sys), part != "disc") {
        let prefer = covers::art_region(from.region).map_or(&[][..], |p| p.tdb);
        for f in gametdb::full_covers(map, sys, &covers::lookup_names(title), prefer) {
            let (region, hq) = (covers::tdb_region(&f.region), f.kind == "coverfullHQ");
            let word = Some(region_word(region)).filter(|w| !w.is_empty()).unwrap_or(&f.region);
            let mut c = Cand { ext: ".jpg".into(), source: "gametdb".into(), region: region.into(), edition: "original".into(), kind: if hq { "hd" } else { "scan" }.into(), ..Cand::default() };
            if part == "front" {
                c.url = f.url.replace("/coverfullHQ/", "/coverHQ/").replace("/coverfull/", "/cover/");
                c.kind_of = if hq { "coverHQ" } else { "cover" }.into();
                c.label = label(&[if hq { "HD cover" } else { "Cover" }, word]);
            } else {
                (c.url, c.src, c.kind_of, c.cut) = (f.url.clone(), Some(".jpg".into()), f.kind.into(), Some(part.into()));
                c.label = label(&[if part == "back" { "From GameTDB's full cover" } else { "Cut from full cover" }, word]);
            }
            out.push(c);
        }
    }
    // the same address twice (two fallback types): once
    let mut seen = std::collections::HashSet::new();
    out.retain(|c| seen.insert(format!("{}|{}", c.url, c.cut.as_deref().unwrap_or(""))));
    out
}

// ---------- ranking ----------

/// A picture's longest side in pixels: known once it's downloaded, otherwise what its source usually has.
fn est(c: &Cand) -> f64 {
    match (c.w, c.h) {
        (Some(w), Some(h)) if w > 0 && h > 0 => f64::from(w.max(h)),
        _ if c.kind == "hd" => 1400.0,
        _ => match (c.source.as_str(), c.cut.is_some()) {
            ("launchbox", true) => 1900.0,
            ("launchbox", false) => if c.kind == "scan" { 1200.0 } else { 1000.0 },
            ("libretro", _) => 500.0,
            ("gametdb", true) => 1000.0,
            ("gametdb", false) => 500.0,
            _ => 600.0,
        },
    }
}

fn reg_pts(c: &Cand, want: &str, hit: f64, miss: f64) -> f64 {
    if c.region == want {
        hit
    } else if c.region == "world" || c.region.is_empty() {
        miss
    } else {
        0.0
    }
}

/// SteamGridDB's posters are community-made, not a release's box: never an authentic, region or special pick.
const SG_OFF: f64 = 100.0;

fn is_sg(c: &Cand) -> bool {
    c.source == "steamgriddb"
}

fn kind_pts(c: &Cand) -> f64 {
    if c.fan {
        -15.0
    } else if c.kind == "clean" {
        4.0
    } else {
        8.0
    }
}

fn authentic_pts(c: &Cand, pref: &str, sharp: bool) -> f64 {
    if is_sg(c) {
        return -SG_OFF + if c.rank != 0.0 { -c.rank.min(40.0) / 10.0 } else { 0.0 };
    }
    let edition = match c.edition.as_str() {
        "original" => 30.0,
        "special" => 12.0,
        "reprint" => 0.0,
        _ => 20.0,
    };
    // sharper on: a bigger scan of the same kind wins the tie; off: libretro's covers (named as the files) first
    let size = if sharp { f64::min(12.0, est(c) / 150.0) } else if c.source == "libretro" { 3.0 } else { 0.0 };
    edition + reg_pts(c, pref, 20.0, 10.0) + kind_pts(c) + if c.exact { 6.0 } else { 0.0 } + size + if c.in_use { 2.0 } else { 0.0 }
}

fn hd_pts(c: &Cand, pref: &str) -> f64 {
    est(c) / 40.0 + reg_pts(c, pref, 3.0, 1.0) + if c.edition == "original" { 2.0 } else { 0.0 } - if c.fan { 5.0 } else { 0.0 } + if c.in_use { 1.0 } else { 0.0 } + if is_sg(c) { f64::max(0.0, 2.0 - c.rank * 0.2) } else { 0.0 }
}

fn region_pts(c: &Cand, region: &str, sharp: bool) -> f64 {
    reg_pts(c, region, 40.0, 12.0) + authentic_pts(c, region, sharp) / 4.0
}

fn special_pts(c: &Cand, pref: &str, sharp: bool) -> f64 {
    let edition = match c.edition.as_str() {
        "special" => 50.0,
        "original" => 20.0,
        _ => 0.0,
    };
    let sg = if is_sg(c) { -SG_OFF } else { 0.0 };
    sg + edition + reg_pts(c, pref, 10.0, 5.0) + kind_pts(c) + if sharp { est(c) / 300.0 } else { 0.0 } + if c.in_use { 2.0 } else { 0.0 }
}

/// What a ranking goes by: the console's style, the game's region, and whether a bigger scan wins a tie (None:
/// as the style says).
pub struct Ctx<'a> {
    pub style: &'a Value,
    pub game_region: &'a str,
    pub sharp: Option<bool>,
}

impl Ctx<'_> {
    fn sharp(&self) -> bool {
        self.sharp.unwrap_or_else(|| js::truthy(self.style.get("sharper")))
    }
}

/// A candidate's points under each look: `{ authentic, hd, region (the style's), special }`, to one decimal.
pub fn scores(c: &Cand, ctx: &Ctx) -> Value {
    let (pref, sharp) = (pref_region(ctx.style, ctx.game_region), ctx.sharp());
    let r1 = |x: f64| js::num(js::round(x * 10.0) / 10.0);
    let region = ctx.style["region"].as_str().filter(|r| !r.is_empty()).unwrap_or("usa");
    json!({ "authentic": r1(authentic_pts(c, &pref, sharp)), "hd": r1(hd_pts(c, &pref)), "region": r1(region_pts(c, region, sharp)), "special": r1(special_pts(c, &pref, sharp)) })
}

/// A candidate's points under one look.
pub fn rank_score(c: &Cand, look: &str, ctx: &Ctx) -> f64 {
    // made from the cover: only when nothing real was found
    if c.source == "made" {
        return -1000.0;
    }
    let (pref, sharp) = (pref_region(ctx.style, ctx.game_region), ctx.sharp());
    match look.strip_prefix("region:").filter(|r| REGIONS.contains(r)) {
        Some(region) => region_pts(c, region, sharp),
        None if look == "hd" => hd_pts(c, &pref),
        None if look == "special" => special_pts(c, &pref, sharp),
        None => authentic_pts(c, &pref, sharp),
    }
}

/// The best first; ties keep the order given (the sources come in their own order of preference).
pub fn rank_list<'a>(list: &'a [Cand], look: &str, ctx: &Ctx) -> Vec<&'a Cand> {
    let mut scored: Vec<(f64, &Cand)> = list.iter().map(|c| (rank_score(c, look, ctx), c)).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().map(|x| x.1).collect()
}

/// A candidate's key: the same for the same address (and cut) every time.
pub fn remote_key(url: &str, cut: Option<&str>) -> String {
    format!("r{}", &sha(&format!("{url}|{}", cut.unwrap_or("")))[..15])
}

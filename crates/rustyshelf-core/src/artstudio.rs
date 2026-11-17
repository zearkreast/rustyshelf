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
    /// drawn by RustyShelf from this picture (a cartridge from the front cover)
    pub made: Option<PathBuf>,
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

/// a part ("front" or "back") → the wiki's best box for it
pub type WikiFind<'a> = dyn Fn(&str) -> Option<crate::wikiart::Found> + 'a;

/// What the candidates are made from: each source as it's kept on this computer; only `wiki` may ask the web.
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
    /// the Super Mario Wiki's best box for a part (Switch 2)
    pub wiki: Option<&'a WikiFind<'a>>,
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
    if let (Some(wiki), true, "front" | "back") = (from.wiki, crate::wikiart::has(sys), part) {
        if let Some(w) = wiki(part) {
            let name = w.name.rsplit_once('.').filter(|(_, e)| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphabetic())).map_or(w.name.as_str(), |(n, _)| n);
            out.push(Cand { label: label(&["Super Mario Wiki", name]), url: w.url, ext: format!(".{}", w.ext), source: "mariowiki".into(), kind_of: if part == "back" { "Box - Back" } else { "Box - Front" }.into(), edition: "original".into(), kind: "scan".into(), ..Cand::default() });
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

// ---------- a game's list, and putting a picture in use ----------

/// A game as the Art Studio works on it.
#[derive(Debug, Clone, Default)]
pub struct GameArt {
    pub sys: String,
    pub title: String,
    pub file: PathBuf,
    /// its file name's region tags
    pub region: String,
    /// where its art goes, and the name its art has
    pub dir: PathBuf,
    pub base: String,
    /// the picture in use for each of `PARTS`
    pub current: [Option<PathBuf>; 4],
    /// a PC game on a console's shelf: its art is kept as its chosen cover is, `<id>.<ext>`, `<id>.back.<ext>` …
    pub pc: bool,
}

impl GameArt {
    /// What a part's file name ends with, before its type.
    pub fn suffix(&self, part: &str) -> &'static str {
        if !self.pc {
            return suffix(part);
        }
        match part {
            "back" => ".back",
            "spine" => ".spine",
            "disc" => ".disc",
            _ => "",
        }
    }

    /// The type a picture is kept as: a PC game's art is only read as a JPG, PNG or WebP.
    fn ext<'a>(&self, ext: &'a str) -> &'a str {
        match ext {
            _ if !self.pc => ext,
            e if e.eq_ignore_ascii_case(".png") => ".png",
            e if e.eq_ignore_ascii_case(".webp") => ".webp",
            _ => ".jpg",
        }
    }
}

fn part_index(part: &str) -> usize {
    PARTS.iter().position(|p| *p == part).unwrap_or(0)
}

/// The name a game's locks are kept under in the settings (`art.locks`).
pub fn lock_key(sys: &str, file: &Path) -> String {
    format!("{sys}|{}", crate::art::resolve(file).to_string_lossy().to_lowercase())
}

/// The parts automatic work must leave alone: a locked game's every part (missing ones too), else the parts locked one
/// by one.
pub fn locked_parts(cfg: &Value, sys: &str, file: &Path) -> Vec<&'static str> {
    let e = &cfg["art"]["locks"][lock_key(sys, file).as_str()];
    let all = js::truthy(e.get("all"));
    PARTS.iter().copied().filter(|p| all || js::truthy(e.get(*p))).collect()
}

/// A picture file's size on disk and in pixels.
fn file_info(f: &Path) -> Option<(u64, Option<u32>, Option<u32>)> {
    let meta = std::fs::metadata(f).ok().filter(|m| m.is_file())?;
    let size = crate::boxart::size_of(f);
    Some((meta.len(), size.map(|z| z.0), size.map(|z| z.1)))
}

/// "5 Nov 2026" for a stamp that starts with its day ("2026-11-05-…").
fn date_word(stamp: &str) -> Option<String> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"];
    let mut parts = stamp.splitn(4, '-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if y.len() != 4 || m.len() != 2 || d.len() != 2 || ![y, m, d].iter().all(|x| x.bytes().all(|c| c.is_ascii_digit())) {
        return None;
    }
    Some(format!("{} {} {y}", d.parse::<u32>().ok()?, MONTHS.get(m.parse::<usize>().ok()?.checked_sub(1)?)?))
}

fn local_cand(file: &Path, source: &str, label: String) -> Cand {
    let info = file_info(file);
    Cand {
        key: format!("{}{}", if source == "current" { "c" } else { "e" }, &sha(&crate::art::resolve(file).to_string_lossy().to_lowercase())[..15]),
        source: source.into(),
        file: Some(file.to_path_buf()),
        ext: js::extname(&file.to_string_lossy()).to_lowercase(),
        edition: "original".into(),
        kind: "scan".into(),
        w: info.and_then(|i| i.1),
        h: info.and_then(|i| i.2),
        bytes: info.map_or(0, |i| i.0),
        in_use: source == "current",
        label,
        ..Cand::default()
    }
}

fn same_image(a: (u64, Option<u32>, Option<u32>), b: (u64, Option<u32>, Option<u32>)) -> bool {
    a == b
}

/// Marks the pictures that are the same as an earlier one of the list (by size on disk and in pixels): the one in
/// use hands its place to its twin, any other twin is the copy.
pub fn dedupe(list: &mut [Cand]) {
    let cur = list.iter().position(|c| c.source == "current");
    for i in 0..list.len() {
        if list[i].dup.is_some() || list[i].bytes == 0 {
            continue;
        }
        for j in i + 1..list.len() {
            let (c, d) = (&list[i], &list[j]);
            if d.dup.is_some() || d.bytes == 0 || !same_image((c.bytes, c.w, c.h), (d.bytes, d.w, d.h)) {
                continue;
            }
            if Some(i) == cur {
                list[j].in_use = true;
                list[i].dup = Some(list[j].key.clone());
                break;
            }
            list[j].dup = Some(list[i].key.clone());
        }
    }
}

/// addresses that answered "not there" aren't offered again for this long (ms)
const MISS_MS: f64 = 7.0 * 86_400_000.0;

/// The Art Studio's own files in the data folder: the candidates downloaded (`art-cand`, where the picker's
/// previews come from) and the addresses that weren't there (`art-missing.json`).
pub struct Studio {
    data_dir: PathBuf,
    cand_dir: PathBuf,
}

impl Studio {
    pub fn new(data_dir: &Path) -> Studio {
        Studio { data_dir: data_dir.to_path_buf(), cand_dir: data_dir.join("art-cand") }
    }

    /// Where a candidate is kept once it's downloaded.
    pub fn cache_path(&self, c: &Cand) -> PathBuf {
        self.cand_dir.join(format!("{}{}", &sha(&format!("{}|{}", c.url, c.cut.as_deref().unwrap_or("")))[..24], c.ext))
    }

    fn missing(&self) -> serde_json::Map<String, Value> {
        match std::fs::read(self.data_dir.join("art-missing.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        }
    }

    fn mark_missing(&self, url: &str, now: f64) {
        let mut m = self.missing();
        m.retain(|_, at| js::number(Some(at)) > now - MISS_MS);
        m.insert(url.to_string(), js::num(now));
        let (file, tmp) = (self.data_dir.join("art-missing.json"), self.data_dir.join("art-missing.json.tmp"));
        if std::fs::write(&tmp, Value::Object(m).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }

    /// The list for one part of a game: the picture in use, the sources' (`remote`, with what's known of the ones
    /// downloaded before), and the ones replaced earlier. An address that wasn't there this week is left out.
    pub fn list_for(&self, g: &GameArt, part: &str, remote: Vec<Cand>, now: f64) -> Vec<Cand> {
        let missing = self.missing();
        let mut list = Vec::new();
        let cur = g.current[part_index(part)].as_ref().filter(|f| f.is_file());
        if let Some(cur) = cur {
            list.push(local_cand(cur, "current", "Current".into()));
        }
        for mut c in remote {
            if missing.get(&c.url).is_some_and(|at| js::truthy(Some(at)) && now - js::number(Some(at)) < MISS_MS) {
                continue;
            }
            c.key = remote_key(&c.url, c.cut.as_deref());
            let info = file_info(&self.cache_path(&c));
            if let Some((bytes, w, h)) = info {
                (c.bytes, c.w, c.h) = (bytes, w, h);
            }
            (c.cached, c.in_use) = (info.is_some(), false);
            list.push(c);
        }
        // a Switch / 3DS cartridge drawn from the game's own front cover: always offered, ranked last (its address names
        // the cover and its date: a new cover makes a new one)
        if let (Some(front), "disc", true) = (g.current[0].as_ref().filter(|f| f.is_file()), part, crate::boxart::CART_SYS.contains(&g.sys.as_str())) {
            if let Ok(meta) = std::fs::metadata(front) {
                let at = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0.0, |d| (d.as_secs_f64() * 1000.0).round());
                let abs = std::path::absolute(front).unwrap_or_else(|_| front.clone()).to_string_lossy().to_lowercase();
                let url = format!("made:cart:{}", &sha(&format!("{abs}|{}|{}", meta.len(), js::num(at)))[..24]);
                let mut c = Cand { key: format!("r{}", &sha(&url)[..15]), url, made: Some(front.clone()), ext: ".png".into(), source: "made".into(), kind_of: "cart".into(), edition: "original".into(), kind: "clean".into(), label: "Cartridge made from the cover".into(), ..Cand::default() };
                let info = file_info(&self.cache_path(&c));
                if let Some((bytes, w, h)) = info {
                    (c.bytes, c.w, c.h) = (bytes, w, h);
                }
                c.cached = info.is_some();
                list.push(c);
            }
        }
        let mut dirs = vec![g.dir.clone()];
        dirs.extend(cur.and_then(|c| c.parent()).map(Path::to_path_buf).filter(|d| *d != g.dir));
        for d in dirs {
            let old = d.join("replaced");
            let names: Vec<String> = std::fs::read_dir(&old).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            for (name, stamp) in covers::replaced_for(&names, &g.base, g.suffix(part)).into_iter().take(12) {
                let when = date_word(&stamp).map_or(String::new(), |w| format!(" · {w}"));
                let mut c = local_cand(&old.join(name), "earlier", format!("Earlier{when}"));
                c.in_use = false;
                list.push(c);
            }
        }
        dedupe(&mut list);
        list
    }

    /// A candidate as a file on this computer: itself, its downloaded copy, or downloaded now (a spine or a back
    /// cut from its wrap). None when it isn't there; an error when the web can't be reached.
    pub fn fetch_cand(&self, c: &Cand, sys: &str, now: f64) -> Result<Option<PathBuf>, String> {
        if let Some(file) = &c.file {
            return Ok(Some(file.clone()).filter(|f| f.is_file()));
        }
        let dest = self.cache_path(c);
        if dest.is_file() {
            // (its date says when it was last used, for the cache's clean-up)
            if let Ok(f) = std::fs::File::options().write(true).open(&dest) {
                let _ = f.set_modified(std::time::SystemTime::now());
            }
            return Ok(Some(dest));
        }
        std::fs::create_dir_all(&self.cand_dir).map_err(|e| e.to_string())?;
        if let Some(front) = &c.made {
            let Some(png) = crate::boxart::make_cart(front, sys) else { return Ok(None) };
            let part = PathBuf::from(format!("{}.part", dest.display()));
            std::fs::write(&part, png).and_then(|()| std::fs::rename(&part, &dest)).map_err(|e| e.to_string())?;
            return Ok(Some(dest));
        }
        let get = |to: &Path| -> Result<bool, String> {
            // (ScreenScraper's addresses are signed with the account as they're fetched)
            if crate::screenscraper::is_media(&c.url) {
                let Some(site) = crate::screenscraper::ScreenScraper::from_settings(&self.data_dir) else { return Ok(false) };
                let got = site.download(&c.url, to)?;
                if got == crate::screenscraper::Got::Missing {
                    self.mark_missing(&c.url, now);
                }
                return Ok(got == crate::screenscraper::Got::Ok);
            }
            let got = crate::boxart::download_ex(&c.url, to, false)?;
            if got == crate::boxart::Outcome::Missing {
                self.mark_missing(&c.url, now);
            }
            Ok(got == crate::boxart::Outcome::Ok)
        };
        match &c.cut {
            None => {
                if !get(&dest)? {
                    return Ok(None);
                }
            }
            Some(cut) => {
                let raw = self.cand_dir.join(format!(".raw-{}-{:x}{}", std::process::id(), now as u64, c.src.as_deref().unwrap_or(".jpg")));
                let piece = get(&raw).map(|ok| if ok { crate::boxart::cut_wrap(&raw, sys, cut) } else { None });
                let _ = std::fs::remove_file(&raw);
                let Some(bytes) = piece? else { return Ok(None) };
                let part = PathBuf::from(format!("{}.part", dest.display()));
                if std::fs::write(&part, bytes).and_then(|()| std::fs::rename(&part, &dest)).is_err() {
                    let _ = std::fs::remove_file(&part);
                    return Ok(None);
                }
            }
        }
        // an error page, not a picture
        if !file_info(&dest).is_some_and(|i| i.1.is_some_and(|w| w > 0) || c.ext == ".webp") {
            let _ = std::fs::remove_file(&dest);
            return Ok(None);
        }
        Ok(Some(dest))
    }

    fn incoming(g: &GameArt, part: &str, ext: &str, now: f64) -> PathBuf {
        g.dir.join(format!("{}{}.incoming-{:08x}{}", g.base, g.suffix(part), (now as u64 ^ u64::from(std::process::id()) << 20) & 0xffff_ffff, g.ext(ext)))
    }

    fn place(g: &GameArt, part: &str, inc: &Path, ext: &str, origin: &str) -> std::io::Result<PathBuf> {
        let (dest, _) = crate::boxart::place_art_as(&g.dir, &g.base, g.suffix(part), inc, g.ext(ext), origin)?;
        // a PC game's front picked here is no longer the console box it may have been picked as
        if g.pc && part == "front" {
            for note in ["plat", "from"] {
                let _ = std::fs::remove_file(g.dir.join(format!("{}.{note}", g.base)));
            }
        }
        Ok(dest)
    }

    /// A picture from `replaced\` back in use (the one in use goes to `replaced\` in its turn).
    fn restore(&self, g: &mut GameArt, part: &str, file: &Path, now: f64) -> Result<bool, String> {
        let ext = js::extname(&file.to_string_lossy()).to_lowercase();
        std::fs::create_dir_all(&g.dir).map_err(|e| e.to_string())?;
        // out of replaced\ first: it mustn't be taken for an old one there
        let inc = Self::incoming(g, part, &ext, now);
        std::fs::rename(file, &inc).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "That earlier picture is no longer there.".to_string() } else { e.to_string() })?;
        match Self::place(g, part, &inc, &ext, "restored") {
            Ok(dest) => {
                g.current[part_index(part)] = Some(dest);
                Ok(true)
            }
            Err(e) => {
                let _ = std::fs::rename(&inc, file);
                Err(e.to_string())
            }
        }
    }

    /// Puts a candidate in use for a part; false when it's what's there already. The picture it replaces goes to
    /// `replaced\`.
    pub fn apply(&self, g: &mut GameArt, part: &str, c: &Cand, now: f64) -> Result<bool, String> {
        match (c.source.as_str(), &c.file) {
            ("current", _) => return Ok(false),
            ("earlier", Some(file)) => return self.restore(g, part, &file.clone(), now),
            _ => {}
        }
        let src = self.fetch_cand(c, &g.sys, now)?.ok_or("That picture couldn't be downloaded.")?;
        let cur = g.current[part_index(part)].clone().filter(|f| f.is_file());
        // the same picture: nothing moves
        if cur.as_ref().and_then(|cur| file_info(cur)).zip(file_info(&src)).is_some_and(|(a, b)| same_image(a, b)) {
            return Ok(false);
        }
        std::fs::create_dir_all(&g.dir).map_err(|e| e.to_string())?;
        let inc = Self::incoming(g, part, &c.ext, now);
        std::fs::copy(&src, &inc).map_err(|e| e.to_string())?;
        match Self::place(g, part, &inc, &c.ext, "studio") {
            Ok(dest) => {
                g.current[part_index(part)] = Some(dest);
                Ok(true)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&inc);
                Err(e.to_string())
            }
        }
    }

    /// Puts back the picture a part had before the one in use.
    pub fn undo(&self, g: &mut GameArt, part: &str, now: f64) -> Result<(), String> {
        let mut dirs = vec![g.dir.clone()];
        dirs.extend(g.current[part_index(part)].as_ref().and_then(|c| c.parent()).map(Path::to_path_buf).filter(|d| *d != g.dir));
        let mut earlier: Vec<(String, PathBuf)> = Vec::new();
        for d in dirs {
            let old = d.join("replaced");
            let names: Vec<String> = std::fs::read_dir(&old).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
            earlier.extend(covers::replaced_for(&names, &g.base, g.suffix(part)).into_iter().map(|(name, stamp)| (stamp, old.join(name))));
        }
        // (the newest stamp first; of two the same, the first found)
        let last = earlier.iter().fold(None::<&(String, PathBuf)>, |best, e| if best.is_none_or(|b| e.0 > b.0) { Some(e) } else { best }).ok_or("Nothing to undo")?.1.clone();
        self.restore(g, part, &last, now).map(|_| ())
    }
}

/// A picture by its first bytes, not its name: its type (with the dot), and its size where the header says it
/// (a JPEG's is read from the file). None for anything but a PNG, a JPEG or a WebP.
pub fn image_info(head: &[u8]) -> Option<(&'static str, Option<(u32, u32)>)> {
    let le = |at: usize, n: usize| head[at..at + n].iter().rev().fold(0u32, |v, b| (v << 8) | u32::from(*b));
    if head.len() >= 24 && head.starts_with(b"\x89PNG") {
        let be = |at: usize| u32::from_be_bytes(head[at..at + 4].try_into().expect("four bytes"));
        return Some((".png", Some((be(16), be(20)))));
    }
    if head.len() >= 4 && head.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some((".jpg", None));
    }
    if head.len() >= 30 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        return match &head[12..16] {
            b"VP8X" => Some((".webp", Some((1 + le(24, 3), 1 + le(27, 3))))),
            b"VP8 " if head[23..26] == [0x9d, 0x01, 0x2a] => Some((".webp", Some((le(26, 2) & 0x3fff, le(28, 2) & 0x3fff)))),
            b"VP8L" if head[20] == 0x2f => {
                let b = le(21, 4);
                Some((".webp", Some((1 + (b & 0x3fff), 1 + ((b >> 14) & 0x3fff)))))
            }
            _ => None,
        };
    }
    None
}

/// the biggest file taken as a picture of the player's own
const OWN_MAX: u64 = 40 * 1024 * 1024;

/// A picture of the player's own, checked: its type and size, or a sentence for the screen.
fn check_image(file: &Path, part: &str, cut: bool) -> Result<(&'static str, u32, u32), String> {
    use std::io::Read;
    let mut head = Vec::with_capacity(64);
    std::fs::File::open(file).and_then(|f| f.take(64).read_to_end(&mut head)).map_err(|_| "That file can't be opened.".to_string())?;
    let (ext, size) = image_info(&head).ok_or("That file isn't a PNG, JPEG or WebP picture.")?;
    let (w, h) = match size {
        Some(z) => z,
        None => crate::boxart::size_of(file).ok_or("That JPEG picture couldn't be read.")?,
    };
    let (lo, hi) = (w.min(h), w.max(h));
    if lo < if part == "spine" && !cut { 12 } else { 64 } || hi < 120 {
        return Err(format!("That picture is too small ({w} × {h})."));
    }
    if hi > 20_000 || u64::from(w) * u64::from(h) > 200_000_000 {
        return Err(format!("That picture is too big ({w} × {h})."));
    }
    if cut && f64::from(w) < f64::from(h) * 1.2 {
        return Err("For a spine cut, pick a picture of the whole box opened flat: back, spine and front side by side.".into());
    }
    Ok((ext, w, h))
}

impl Studio {
    /// Puts a picture of the player's own in use for a part. The file is copied, never moved; the picture it
    /// replaces goes to `replaced\`. `cut`: it's a picture of the whole box opened flat, and the spine is cut
    /// from it.
    pub fn own(&self, g: &mut GameArt, part: &str, src: &Path, cut: bool, now: f64) -> Result<(), String> {
        let cut = cut && part == "spine";
        let meta = std::fs::metadata(src).map_err(|_| "That file can't be opened.".to_string())?;
        if !meta.is_file() {
            return Err("Pick a picture file.".into());
        }
        if meta.len() > OWN_MAX {
            return Err("That picture is over 40 MB. Pick a smaller one.".into());
        }
        if meta.len() < 200 {
            return Err("That file is empty or damaged.".into());
        }
        std::fs::create_dir_all(&g.dir).map_err(|e| e.to_string())?;
        let i = part_index(part);
        let same = |a: &Path, b: &Path| crate::art::resolve(a).to_string_lossy().to_lowercase() == crate::art::resolve(b).to_string_lossy().to_lowercase();
        // the picture already in use, picked again: nothing to copy
        if !cut && g.current[i].as_ref().is_some_and(|cur| same(cur, src)) {
            return check_image(src, part, false).map(|_| ());
        }
        let (ext, _, _) = check_image(src, part, cut)?;
        let inc = Self::incoming(g, part, if cut { ".jpg" } else { ext }, now);
        let made = if cut {
            let piece = crate::boxart::cut_wrap(src, &g.sys, "spine").ok_or("No spine could be cut from that picture. Pick a picture of the whole box opened flat (back, spine and front).")?;
            std::fs::write(&inc, piece)
        } else {
            std::fs::copy(src, &inc).map(|_| ())
        };
        let placed = made.and_then(|()| Self::place(g, part, &inc, if cut { ".jpg" } else { ext }, "own"));
        match placed {
            Ok(dest) => {
                g.current[i] = Some(dest);
                Ok(())
            }
            Err(e) => {
                let _ = std::fs::remove_file(&inc);
                Err(e.to_string())
            }
        }
    }
}

/// The candidates of a list the picker shows: not a copy of another, and a guessed address only once it has
/// really downloaded.
pub fn shown(list: &[Cand]) -> impl Iterator<Item = &Cand> {
    list.iter().filter(|c| c.dup.is_none() && !(c.guess && !c.cached && !c.in_use))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pc_game_s_art_is_kept_beside_its_cover() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("pc-covers");
        std::fs::create_dir_all(&dir).unwrap();
        let pic = t.path().join("mine.png");
        image::RgbImage::from_fn(200, 300, |x, y| image::Rgb([x as u8, y as u8, (x ^ y) as u8])).save(&pic).unwrap();
        for f in ["custom-zeta.jpg", "custom-zeta.plat", "custom-zeta.from"] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let mut g = GameArt { sys: "n64".into(), title: "Zeta".into(), file: "Zeta".into(), dir: dir.clone(), base: "custom-zeta".into(), current: [Some(dir.join("custom-zeta.jpg")), None, None, None], pc: true, ..GameArt::default() };
        assert_eq!([".JPEG", ".png", ".webp", ".gif"].map(|e| g.ext(e)), [".jpg", ".png", ".webp", ".jpg"]);
        let st = Studio::new(t.path());
        st.own(&mut g, "spine", &pic, false, 1.0).unwrap();
        assert_eq!(g.current[2], Some(dir.join("custom-zeta.spine.png")));
        assert!(dir.join("custom-zeta.plat").is_file(), "a spine leaves the front as it was picked");
        st.own(&mut g, "front", &pic, false, 2.0).unwrap();
        assert_eq!(g.current[0], Some(dir.join("custom-zeta.png")));
        assert!(["jpg", "plat", "from"].iter().all(|e| !dir.join(format!("custom-zeta.{e}")).exists()));
        assert_eq!((crate::pccovers::cover(t.path(), "custom-zeta"), crate::pccovers::part(t.path(), "custom-zeta", "spine")), (g.current[0].clone(), g.current[2].clone()));
        st.undo(&mut g, "front", 3.0).unwrap();
        assert_eq!(g.current[0], Some(dir.join("custom-zeta.jpg")));
        assert!(st.undo(&mut g, "spine", 4.0).is_err(), "the front's earlier pictures aren't the spine's");
    }
}

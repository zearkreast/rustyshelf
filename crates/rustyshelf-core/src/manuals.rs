// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/manuals.js (matching, the searches, the download of a chosen manual and the automatic pick).
//! Game manuals from the Internet Archive (archive.org), and one English PDF a game from GamesDatabase.org:
//! searched for by a game's title, scored against it, and saved as page pictures beside its box art
//! (`<game file>-manual-01.jpg` …), where the shelf finds them.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use md5::{Digest, Md5};
use regex::Regex;
use serde_json::{json, Map, Value};

use crate::{collate, js};

/// the most pages a manual is given, how wide a page is asked for (small print stays readable), and how many
/// come down at once
const MAX_PAGES: usize = 60;
const PAGE_WIDTH: u32 = 1600;
const PARALLEL: usize = 4;
/// A candidate this good is taken without asking.
pub const AUTO_MIN: f64 = 7.0;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

// ---------- how the sites are asked ----------

/// Why a site gave no answer.
#[derive(Debug, Clone, PartialEq)]
pub enum WebError {
    /// it answered with this status
    Http(u16),
    /// it answered with something that isn't what was asked for
    Unreadable,
    /// it couldn't be reached, or took too long
    Offline(String),
}

impl WebError {
    pub fn words(&self) -> String {
        match self {
            WebError::Http(code) => format!("HTTP {code}"),
            WebError::Unreadable => "an answer that couldn't be read".into(),
            WebError::Offline(why) => why.clone(),
        }
    }
}

/// The sites, as the searches and the downloads ask them. The tests put a stand-in here.
pub trait Web: Sync {
    fn json(&self, url: &str) -> Result<Value, WebError>;
    fn text(&self, url: &str) -> Result<String, WebError>;
    /// Saves a file; false when the site hasn't got it (or gave too little to be it).
    fn file(&self, url: &str, dest: &Path) -> Result<bool, WebError>;
    /// A wait before a second try.
    fn pause(&self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// The sites themselves.
pub struct Online;

fn agent() -> String {
    format!("RustyShelf/{}", env!("CARGO_PKG_VERSION"))
}

impl Web for Online {
    fn json(&self, url: &str) -> Result<Value, WebError> {
        let (status, _, body) = crate::net::ask(url, &[("User-Agent", agent().as_str())], Duration::from_secs(60), 8_000_000).map_err(|slow| WebError::Offline(if slow { "timed out" } else { "couldn't reach the site" }.into()))?;
        if status != 200 {
            return Err(WebError::Http(status));
        }
        serde_json::from_slice(&body).map_err(|_| WebError::Unreadable)
    }

    fn text(&self, url: &str) -> Result<String, WebError> {
        let (status, _, body) = crate::net::ask(url, &[("User-Agent", agent().as_str()), ("Accept", "text/html")], Duration::from_secs(40), 2_000_000).map_err(|slow| WebError::Offline(if slow { "timed out" } else { "couldn't reach the site" }.into()))?;
        if status != 200 {
            return Err(WebError::Http(status));
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    fn file(&self, url: &str, dest: &Path) -> Result<bool, WebError> {
        let (status, _, body) = crate::net::ask(url, &[("User-Agent", agent().as_str())], Duration::from_secs(300), 600_000_000).map_err(|slow| WebError::Offline(if slow { "timed out" } else { "couldn't reach the site" }.into()))?;
        if status != 200 || body.len() < 2048 {
            return Ok(false);
        }
        // through a .part: a page half written must never read as a manual's page
        let part = PathBuf::from(format!("{}.part", dest.display()));
        let written = fs::write(&part, &body).and_then(|()| fs::rename(&part, dest));
        if written.is_err() {
            let _ = fs::remove_file(&part);
        }
        written.map(|()| true).map_err(|e| WebError::Offline(e.to_string()))
    }
}

// ---------- matching a manual's title to a game's ----------

fn roman(word: &str) -> Option<u32> {
    Some(match word {
        "ii" => 2,
        "iii" => 3,
        "iv" => 4,
        "v" => 5,
        "vi" => 6,
        "vii" => 7,
        "viii" => 8,
        "ix" => 9,
        "xi" => 11,
        "xii" => 12,
        "xiii" => 13,
        "xiv" => 14,
        "xv" => 15,
        "xvi" => 16,
        "xvii" => 17,
        "xviii" => 18,
        "xix" => 19,
        "xx" => 20,
        _ => return None,
    })
}

/// A title as it's compared: lower case, "&" as "and", apostrophes off, anything else that isn't a letter or a
/// digit a space, no "the", "a", "an" or "of", and Roman numbers as digits.
pub fn norm(s: &str) -> String {
    let plain = s.to_lowercase().replace('&', " and ").replace(['\'', '’'], "");
    let spaced: String = plain.chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { ' ' }).collect();
    spaced.split(' ').filter(|w| !w.is_empty() && !matches!(*w, "the" | "a" | "an" | "of")).map(|w| roman(w).map_or(w.to_string(), |n| n.to_string())).collect::<Vec<_>>().join(" ")
}

/// A title's words (a single letter isn't one, a digit is: Black Ops 3 is not Black Ops II).
pub fn words(s: &str) -> BTreeSet<String> {
    norm(s).split(' ').filter(|w| w.chars().count() > 1 || w.chars().any(|c| c.is_ascii_digit())).map(String::from).collect()
}

fn small_number(w: &str) -> bool {
    (1..=2).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_digit())
}

/// The sequel numbers of a title: a number that ends the name or a part of it ("Black Ops 3", "Galaxy 2: …",
/// "Mario Party 9 (USA)"). A number inside a name isn't one.
pub fn seq_nums(t: &str) -> BTreeSet<String> {
    static PARTS: LazyLock<Regex> = LazyLock::new(|| re(r"\s+[-–:]\s*|:\s*|\("));
    PARTS.split(t).filter_map(|part| norm(part).split(' ').next_back().filter(|w| small_number(w)).map(String::from)).collect()
}

/// How a manual's title `b` stands to a game's `a`: how much of the game's title it covers, how many other
/// words it has that aren't a manual's usual ones, and whether its sequel number is another (or missing).
pub fn sim_parts(a: &str, b: &str) -> (f64, usize, bool) {
    // words a manual's title may carry besides the game's name without being about something else
    static GENERIC: LazyLock<Regex> = LazyLock::new(|| {
        re("^(manual|manuals|instruction|instructions|booklet|user|users|owner|owners|nintendo|sony|sega|wii|gamecube|psp|switch|ps1|ps2|ps3|ps4|ps5|psx|playstation|xbox|360|x360|microsoft|eng|english|usa|us|eu|europe|pal|ntsc|scan|scans|game|games|ds|nds|3ds|gba|snes|nes|n64|genesis|dreamcast|saturn|en)$")
    });
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() {
        return (0.0, 0, false);
    }
    let hit = wa.iter().filter(|w| wb.contains(*w)).count();
    // (a year or a catalogue number isn't another game)
    let noise = wb.iter().filter(|w| !wa.contains(*w) && !GENERIC.is_match(w) && !(w.len() >= 3 && w.bytes().all(|c| c.is_ascii_digit()))).count();
    let got: Vec<&String> = wb.iter().filter(|w| small_number(w)).collect();
    let want: Vec<&String> = wa.iter().filter(|w| small_number(w)).collect();
    let wrong = got.iter().any(|n| !wa.contains(*n)) || (!want.is_empty() && !want.iter().all(|n| wb.contains(*n)) && !got.is_empty()) || seq_nums(a).iter().any(|n| !wb.contains(n));
    (hit as f64 / wa.len() as f64, noise, wrong)
}

pub fn similarity(a: &str, b: &str) -> f64 {
    let (recall, noise, wrong) = sim_parts(a, b);
    recall * match noise {
        0 | 1 => 1.0,
        2 => 0.8,
        3 => 0.6,
        _ => 0.45,
    } * if wrong { 0.4 } else { 1.0 }
}

/// A console as a manual's title may name it: what names it, and what mustn't follow for it to be that one
/// ("wii" not before "u").
struct Named {
    id: &'static str,
    name: Regex,
    not_before: Option<Regex>,
}

impl Named {
    fn in_text(&self, t: &str) -> bool {
        self.name.find_iter(t).any(|m| !self.not_before.as_ref().is_some_and(|after| after.is_match(&t[m.end()..])))
    }
}

// (main/manuals.js PLATFORM_RE, keep in sync)
static PLATFORMS: LazyLock<Vec<Named>> = LazyLock::new(|| {
    let b = r"(?-u:\b)";
    let table: [(&str, &str, &str); 45] = [
        ("gc", "(game ?cube|gcn|ngc)", ""),
        ("wii", "wii", " ?u"),
        ("wiiu", "wii ?u", ""),
        ("sw", "switch", ""),
        ("sw2", "switch ?2", ""),
        ("psvita", "(ps ?vita|playstation vita)", ""),
        ("psp", "(psp|playstation portable)", ""),
        ("ps2", "(ps2|playstation 2)", ""),
        ("ps3", "(ps3|playstation 3)", ""),
        ("ps4", "(ps4|playstation 4)", ""),
        ("ps5", "(ps5|playstation 5)", ""),
        ("x360", "(xbox ?360|x360)", ""),
        ("xbox", "xbox", " ?(360|one)"),
        ("pc", "(pc|windows|dos|mac)", ""),
        ("xone", "xbox one", ""),
        ("ds", "(nds|nintendo ds)", ""),
        ("n3ds", "(3ds|nintendo 3ds)", ""),
        ("nds", "(nds|nintendo ds)", ""),
        ("psx", "(playstation|psx|ps1)", " ?[2-5]| portable"),
        ("nes", "(nes|famicom)", ""),
        ("fds", "(fds|famicom ?disk ?system)", ""),
        ("snes", "(snes|super nintendo|super famicom)", ""),
        ("n64", "(n64|nintendo 64)", ""),
        ("gb", "game ?boy", " ?(colou?r|advance)"),
        ("gbc", "(game ?boy colou?r|gbc)", ""),
        ("gba", "(game ?boy advance|gba)", ""),
        ("megadrive", "(genesis|mega ?drive)", ""),
        ("mastersystem", "master system", ""),
        ("gamegear", "game gear", ""),
        ("dreamcast", "dreamcast", ""),
        ("saturn", "(sega )?saturn", ""),
        ("pcengine", "(turbografx(-16)?|pc[ -]?engine|tg-?16)", ""),
        ("neogeo", "neo[ -]?geo", "[ -]?pocket"),
        ("ngp", "neo[ -]?geo[ -]?pocket", " colou?r"),
        ("ngpc", "(neo[ -]?geo[ -]?pocket colou?r|ngpc)", ""),
        ("gamecom", r"game\.?com", ""),
        ("arcade", "arcade", ""),
        ("segacd", "(sega|mega)[ -]?cd", ""),
        ("sega32x", "(sega )?32x", ""),
        ("atari2600", "atari 2600", ""),
        ("atari2600", "2600", ""),
        ("atarilynx", "(atari )?lynx", ""),
        ("wonderswan", "wonderswan", ""),
        ("dos", "(ms-?dos|dos)", ""),
        ("", "", ""),
    ];
    table.iter().filter(|p| !p.0.is_empty()).map(|(id, name, not)| Named { id, name: re(&format!("(?i){b}{name}{b}")), not_before: Some(*not).filter(|n| !n.is_empty()).map(|n| re(&format!("^(?i)(?:{n})"))) }).collect()
});

/// Whether a manual's title names a console that isn't this one. `any_platform`: a console of the same
/// generation that shared its releases counts as this one (the last resort).
pub fn foreign_platform(sys: &str, t: &str, any_platform: bool) -> bool {
    if PLATFORMS.iter().any(|p| p.id == sys && p.in_text(t)) {
        return false;
    }
    let siblings: &[&str] = if !any_platform {
        &[]
    } else {
        match sys {
            "gc" => &["ps2", "xbox", "wii"],
            "wii" => &["ps3", "x360", "ps2", "gc"],
            "ps2" => &["gc", "xbox"],
            "ps3" => &["x360", "wii", "ps4"],
            "x360" => &["ps3", "wii", "xone"],
            "xbox" => &["ps2", "gc"],
            "ps4" => &["xone", "ps5", "ps3"],
            _ => &[],
        }
    };
    let own = |id: &str| id == sys || (sys == "nds" && id == "ds") || (sys == "ds" && id == "nds");
    PLATFORMS.iter().any(|p| !own(p.id) && !siblings.contains(&p.id) && p.in_text(t))
}

// uploads that aren't a game's manual at all: promotional booklets, posters, catalogues, service manuals
static NOT_MANUAL: LazyLock<Regex> = LazyLock::new(|| {
    re(r"(?i)promotional|(?-u:\b)promo(?-u:\b)|poster|flyer|leaflet|catalog(ue)?|brochure|advert|press kit|newsletter|price ?list|reading room|cia-rdp|declassified|compliance|policy|for site|service manual|repair|schematic|walkthrough|cheat")
});

/// A candidate's score for a game: how well its title matches, its language, its console and what kind of
/// upload it is. `lang`: the site's language for it, `downloads`: how often it was fetched there.
pub fn score_hit(sys: &str, title: &str, t: &str, lang: &str, any_platform: bool, downloads: f64) -> f64 {
    static FOREIGN: LazyLock<Regex> = LazyLock::new(|| re("(?i)mode d.emploi|spielanleitung|manuale|manual de|bedienungsanleitung"));
    static MANUAL: LazyLock<Regex> = LazyLock::new(|| re("(?i)manual|instruction|booklet"));
    static OTHER: LazyLock<Regex> = LazyLock::new(|| re("(?i)guide|strategy|magazine|catalog|price"));
    let (_, noise, _) = sim_parts(title, t);
    let english = lang.is_empty() || lang.starts_with("en");
    let mut score = similarity(title, t) * 10.0;
    if FOREIGN.is_match(t) {
        score -= 3.0;
    }
    if !english {
        score -= 4.0;
    }
    if MANUAL.is_match(t) {
        score += 1.0;
    }
    if OTHER.is_match(t) {
        score -= 3.0;
    }
    if NOT_MANUAL.is_match(t) {
        score -= 5.0;
    }
    // another console's booklet is not this game's manual
    if foreign_platform(sys, t, any_platform) {
        score -= 6.0;
    }
    // a one-word name inside a longer, different title
    if words(title).len() <= 1 && noise >= 2 {
        score -= 4.0;
    }
    score + (if downloads.is_nan() { 0.0 } else { downloads } / 500.0).min(1.0)
}

/// A candidate's language as a two-letter code ("en", "fr" …): from the site's language field, else from the
/// title's own words; "" when neither says.
pub fn lang_code(raw: &str, title: &str) -> &'static str {
    let code = |w: &str| {
        Some(match w {
            "eng" | "en" | "english" => "en",
            "fre" | "fra" | "fr" | "french" | "francais" => "fr",
            "ger" | "deu" | "de" | "german" | "deutsch" => "de",
            "spa" | "es" | "spanish" | "espanol" => "es",
            "ita" | "it" | "italian" | "italiano" => "it",
            "jpn" | "ja" | "jp" | "japanese" => "ja",
            "por" | "pt" | "portuguese" => "pt",
            "dut" | "nld" | "nl" | "dutch" => "nl",
            _ => return None,
        })
    };
    if let Some(c) = raw.to_lowercase().split(|c: char| !c.is_ascii_lowercase()).find_map(code) {
        return c;
    }
    let t = title.to_lowercase();
    static FR: LazyLock<Regex> = LazyLock::new(|| re("mode d.emploi|notice|livret"));
    if FR.is_match(&t) {
        "fr"
    } else if t.contains("anleitung") {
        "de"
    } else if t.contains("manuale") || t.contains("istruzioni") {
        "it"
    } else if t.contains("instrucciones") {
        "es"
    } else if t.contains("説明書") {
        "ja"
    } else {
        ""
    }
}

/// The region a title names: "eur", "jpn", "usa" or "".
pub fn region_of(text: &str) -> &'static str {
    static EUR: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?-u:\b)(pal|eur|europe|european|uk|australia|aus)(?-u:\b)"));
    static JPN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?-u:\b)(jpn|japan|japanese|ntsc[- ]?j)(?-u:\b)"));
    static USA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)(?-u:\b)(usa|us|ntsc[- ]?u|ntsc|north america|canada)(?-u:\b)"));
    if EUR.is_match(text) {
        "eur"
    } else if JPN.is_match(text) {
        "jpn"
    } else if USA.is_match(text) {
        "usa"
    } else {
        ""
    }
}

// ---------- the searches ----------

/// A manual on offer for a game.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// the upload's name on archive.org, or "gdb:" and a PDF's address on GamesDatabase
    pub id: String,
    pub title: String,
    /// the site's language for it, in lower case; "" when it gives none
    pub language: String,
    pub pages: u32,
    pub score: f64,
    pub source: &'static str,
    pub thumb: String,
    /// its page on the site
    pub url: String,
}

impl Candidate {
    pub fn english(&self) -> bool {
        self.language.is_empty() || self.language.starts_with("en")
    }

    /// Its language in a word, as the list of candidates says it.
    pub fn language_name(&self) -> String {
        let named = match self.language.as_str() {
            "eng" | "en" => "English",
            "fre" | "fra" | "fr" => "French",
            "ger" | "deu" | "de" => "German",
            "spa" | "es" => "Spanish",
            "ita" | "it" => "Italian",
            "jpn" | "ja" => "Japanese",
            "por" => "Portuguese",
            "dut" | "nld" => "Dutch",
            "" => "English",
            other => other,
        };
        named.to_string()
    }

    /// As the Electron app's search gives it.
    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "title": self.title, "language": self.language, "pages": self.pages, "score": js::num(self.score), "source": self.source, "thumb": self.thumb, "url": self.url,
            "languageName": self.language_name(), "english": self.english() })
    }
}

/// A console's words, as uploads name it.
// (main/manuals.js SYSTEM_WORDS and GDB_SYSTEM, keep in sync)
fn system_words(sys: &str) -> &'static [&'static str] {
    match sys {
        "gc" => &["gamecube", "game cube", "gcn", "ngc"],
        "wii" => &["wii"],
        "wiiu" => &["wii u", "wiiu"],
        "psp" => &["psp", "playstation portable"],
        "psvita" => &["ps vita", "playstation vita", "psvita", "vita"],
        "sw" => &["switch"],
        "sw2" => &["switch 2", "switch"],
        "ps3" => &["ps3", "playstation 3"],
        "ps4" => &["ps4", "playstation 4"],
        "ps5" => &["ps5", "playstation 5"],
        "ps2" => &["ps2", "playstation 2"],
        "x360" => &["xbox 360", "xbox360", "x360"],
        "xbox" => &["xbox", "original xbox"],
        "n3ds" => &["3ds", "nintendo 3ds"],
        "psx" => &["playstation", "psx", "ps1"],
        "nes" => &["nes", "nintendo entertainment system", "famicom"],
        "fds" => &["fds", "famicom disk system", "family computer disk system"],
        "snes" => &["snes", "super nintendo", "super famicom"],
        "n64" => &["n64", "nintendo 64"],
        "gb" => &["game boy", "gameboy"],
        "gbc" => &["game boy color", "gameboy color", "gbc"],
        "gba" => &["game boy advance", "gba"],
        "nds" => &["nintendo ds", "nds"],
        "megadrive" => &["genesis", "mega drive", "megadrive"],
        "mastersystem" => &["master system", "sms"],
        "gamegear" => &["game gear"],
        "dreamcast" => &["dreamcast"],
        "saturn" => &["saturn", "sega saturn"],
        "pcengine" => &["turbografx", "turbografx-16", "pc engine", "pc-engine", "tg-16", "tg16"],
        "neogeo" => &["neo geo", "neo-geo", "neogeo"],
        "arcade" => &["arcade"],
        "segacd" => &["sega cd", "mega cd", "mega-cd"],
        "sega32x" => &["32x", "sega 32x"],
        "atari2600" => &["atari 2600", "2600"],
        "atarilynx" => &["lynx", "atari lynx"],
        "wonderswan" => &["wonderswan"],
        "dos" => &["dos", "ms-dos", "msdos"],
        "ngp" => &["neo geo pocket", "neogeo pocket"],
        "ngpc" => &["neo geo pocket color", "neogeo pocket color", "ngpc"],
        "gamecom" => &["game.com", "gamecom", "tiger game.com"],
        _ => &[],
    }
}

/// A console's name in GamesDatabase's addresses.
fn gdb_system(sys: &str) -> Option<&'static str> {
    Some(match sys {
        "gc" => "nintendo-gamecube",
        "wii" => "nintendo-wii",
        "wiiu" => "nintendo-wii-u",
        "psp" => "sony-psp",
        "psvita" => "sony-playstation-vita",
        "sw" => "nintendo-switch",
        "ps3" => "sony-playstation-3",
        "ps4" => "sony-playstation-4",
        "ps2" => "sony-playstation-2",
        "x360" => "microsoft-xbox-360",
        "xbox" => "microsoft-xbox",
        "n3ds" => "nintendo-3ds",
        "psx" => "sony-playstation",
        "nes" => "nintendo-nes",
        "fds" => "nintendo-famicom-disk-system",
        "snes" => "nintendo-snes",
        "n64" => "nintendo-n64",
        "gb" => "nintendo-game-boy",
        "gbc" => "nintendo-game-boy-color",
        "gba" => "nintendo-game-boy-advance",
        "nds" => "nintendo-ds",
        "megadrive" => "sega-genesis",
        "mastersystem" => "sega-master-system",
        "gamegear" => "sega-game-gear",
        "dreamcast" => "sega-dreamcast",
        "saturn" => "sega-saturn",
        "pcengine" => "nec-turbografx-16",
        "neogeo" => "snk-neo-geo-cd",
        "arcade" => "arcade",
        "segacd" => "sega-cd",
        "sega32x" => "sega-32x",
        "atari2600" => "atari-2600",
        "atarilynx" => "atari-lynx",
        "wonderswan" => "bandai-wonderswan",
        "dos" => "microsoft-dos",
        "ngp" => "snk-neo-geo-pocket",
        "ngpc" => "snk-neo-geo-pocket-color",
        "gamecom" => "tiger-game-com",
        _ => return None,
    })
}

/// The spellings a game's page on GamesDatabase may have: its addresses come from the title.
pub fn gdb_slugs(title: &str) -> Vec<String> {
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\s*\(.*?\)"));
    static OTHER: LazyLock<Regex> = LazyLock::new(|| re("[^a-z0-9]+"));
    static OTHER_DASH: LazyLock<Regex> = LazyLock::new(|| re("[^a-z0-9-]+"));
    static DASHES: LazyLock<Regex> = LazyLock::new(|| re("-+"));
    static ARTICLE: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(The|A)\s+(.*)$"));
    static ARTICLE_LAST: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(.*?),\s*(The|A)(\s*-\s*.*)?$"));
    static DASH_PART: LazyLock<Regex> = LazyLock::new(|| re(r"\s+-\s+"));
    let t = BRACKETS.replace_all(title, "").into_owned();
    let low = t.to_lowercase().replace('&', "and");
    let base = OTHER.replace_all(&low.replace(['\'', '’'], ""), "-").trim_start_matches('-').to_string();
    let trimmed = base.trim_end_matches('-').to_string();
    let mut out: Vec<String> = Vec::new();
    let mut add = |s: Option<String>| {
        if let Some(s) = s.filter(|s| !out.contains(s)) {
            out.push(s);
        }
    };
    add(Some(trimmed.clone()));
    add(Some(format!("{trimmed}-")));
    add(Some(base));
    // GamesDatabase writes an apostrophe as a dash
    let dashed = OTHER_DASH.replace_all(&low.replace(['\'', '’'], "-"), "-").into_owned();
    add(Some(DASHES.replace_all(&dashed, "-").trim_matches('-').to_string()));
    if let Some(m) = ARTICLE.captures(&t) {
        add(gdb_slugs(&m[2]).into_iter().next());
        let parts: Vec<&str> = DASH_PART.split(&m[2]).collect();
        let moved = format!("{} the{}", parts[0], if parts.len() > 1 { format!(" - {}", parts[1..].join(" - ")) } else { String::new() });
        add(gdb_slugs(&moved).into_iter().next());
    }
    if let Some(m) = ARTICLE_LAST.captures(&t) {
        add(gdb_slugs(&format!("{} {}{}", &m[2], &m[1], m.get(3).map_or("", |x| x.as_str()))).into_iter().next());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// GamesDatabase's English PDF manual of a game, when it has one.
pub fn search_games_database(web: &dyn Web, sys: &str, title: &str) -> Option<Candidate> {
    static PDF: LazyLock<Regex> = LazyLock::new(|| re(r#"(?i)href="([^"]*/Manual/[^"]+\.pdf)""#));
    let on = gdb_system(sys)?;
    for slug in gdb_slugs(title) {
        let Ok(html) = web.text(&format!("https://www.gamesdatabase.org/game/{on}/{slug}")) else { continue };
        if html.is_empty() || html.to_lowercase().contains("game not found") {
            continue;
        }
        let found = PDF.captures(&html)?;
        let url = if found[1].starts_with("http") { found[1].to_string() } else { format!("https://www.gamesdatabase.org{}", &found[1]) };
        let file = js::decode_uri_component(url.rsplit('/').next().unwrap_or(""))?;
        static PDF_END: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.pdf$"));
        let name = PDF_END.replace(&file, "").replace('_', " ");
        // the page is this console's, but the PDF on it can be another edition's: offered to pick by hand, never
        // taken by itself
        let foreign = foreign_platform(sys, &name, false) || NOT_MANUAL.is_match(&name);
        return Some(Candidate {
            id: format!("gdb:{url}"),
            title: name,
            language: "eng".into(),
            pages: 0,
            score: if foreign { 5.0 } else { 9.5 },
            source: "GamesDatabase (PDF)",
            thumb: String::new(),
            url: format!("https://www.gamesdatabase.org/game/{on}/{slug}"),
        });
    }
    None
}

/// A title with its number written the other way ("Black Ops III" and "Black Ops 3"); None when it has none.
pub fn alt_numbers(t: &str) -> Option<String> {
    static ROMAN: LazyLock<Regex> = LazyLock::new(|| re(r"(?-u:\b)(XIII|XII|XI|IX|VIII|VII|VI|IV|V|III|II)(?-u:\b)"));
    static DIGITS: LazyLock<Regex> = LazyLock::new(|| re(r"(?-u:\b)([2-9]|1[0-3])(?-u:\b)"));
    static COLON: LazyLock<Regex> = LazyLock::new(|| re(r"\s*:\s*"));
    let digits = ROMAN.replace_all(t, |m: &regex::Captures| roman(&m[1].to_lowercase()).map_or(m[1].to_string(), |n| n.to_string())).into_owned();
    if digits != t {
        return Some(COLON.replace_all(&digits, " ").into_owned());
    }
    let numerals = ["II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII"];
    let written = DIGITS.replace_all(t, |m: &regex::Captures| numerals[m[1].parse::<usize>().unwrap_or(2) - 2].to_string()).into_owned();
    (written != t).then_some(written)
}

fn archive_query(q: &str) -> String {
    format!("https://archive.org/advancedsearch.php?q={}&fl[]=identifier&fl[]=title&fl[]=language&fl[]=imagecount&fl[]=downloads&rows=12&output=json", js::encode_uri_component(q))
}

/// The uploads on archive.org that may be a game's manual: five at most, the best first. An error is one that
/// says the site couldn't be asked, which isn't "nothing found".
pub fn search_archive(web: &dyn Web, sys: &str, title: &str, any_platform: bool) -> Result<Vec<Candidate>, WebError> {
    static NOT_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"[^A-Za-z0-9_\s]"));
    let quoted = |s: &str| Value::from(s).to_string();
    let named = quoted(&title.replace('"', ""));
    let console = system_words(sys);
    let narrowed = if any_platform || console.is_empty() { String::new() } else { format!(" AND ({})", console.iter().map(|w| quoted(w)).collect::<Vec<_>>().join(" OR ")) };
    let docs_of = |j: &Value| j.get("response").and_then(|r| r.get("docs")).and_then(Value::as_array).cloned();
    let mut docs = match web.json(&archive_query(&format!("({named}) AND mediatype:texts AND (manual OR instruction OR booklet){narrowed}"))) {
        Ok(j) => docs_of(&j).unwrap_or_default(),
        // an answer that wasn't a results list, or a refusal of the question: nothing found
        Err(WebError::Unreadable) => return Ok(Vec::new()),
        Err(WebError::Http(code)) if (400..500).contains(&code) && code != 429 => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    // uploads that don't name the console, matched on the title alone; then plain words, in anything that holds
    // page pictures
    if docs.is_empty() {
        docs = web.json(&archive_query(&format!("title:({named}) AND mediatype:texts AND (manual OR instruction OR booklet)"))).ok().and_then(|j| docs_of(&j)).unwrap_or_default();
    }
    if docs.is_empty() {
        let plain = format!("{} {} manual AND (mediatype:texts OR mediatype:image)", NOT_WORD.replace_all(title, " "), console.first().unwrap_or(&""));
        docs = web.json(&archive_query(&plain)).ok().and_then(|j| docs_of(&j)).unwrap_or_default();
    }
    let text = |v: Option<&Value>| if js::truthy(v) { js::string(v) } else { String::new() };
    let mut found: Vec<Candidate> = docs
        .iter()
        .map(|d| {
            let (t, id) = (text(d.get("title")), js::string(d.get("identifier")));
            let lang = match d.get("language") {
                Some(Value::Array(l)) => js::string(l.first()),
                other => text(other),
            }
            .to_lowercase();
            let lang = if lang == "undefined" { String::new() } else { lang };
            let pages = js::number(d.get("imagecount"));
            Candidate {
                score: score_hit(sys, title, &t, &lang, any_platform, js::number(d.get("downloads"))),
                title: t,
                language: lang,
                pages: if pages.is_finite() && pages > 0.0 { pages as u32 } else { 0 },
                source: "Internet Archive",
                thumb: format!("https://archive.org/services/img/{}", js::encode_uri_component(&id)),
                url: format!("https://archive.org/details/{}", js::encode_uri_component(&id)),
                id,
            }
        })
        .filter(|c| c.score >= 4.0)
        .collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    found.truncate(5);
    Ok(found)
}

/// The manuals on offer for a game: GamesDatabase's PDF first, then archive.org's uploads, six at most. With
/// nothing found, the title's number is tried the other way round once, and a bundle ("A + B") by its first
/// game.
pub fn search(web: &dyn Web, sys: &str, title: &str) -> Result<Vec<Candidate>, WebError> {
    static PLUS: LazyLock<Regex> = LazyLock::new(|| re(r"\s\+\s"));
    fn once(web: &dyn Web, sys: &str, title: &str, alt: bool) -> Result<Vec<Candidate>, WebError> {
        let gdb = search_games_database(web, sys, title);
        let mut all = match (search_archive(web, sys, title, false), &gdb) {
            (Ok(found), _) => found,
            (Err(_), Some(_)) => Vec::new(),
            (Err(e), None) => return Err(e),
        };
        if let Some(pdf) = gdb {
            all.insert(0, pdf);
        }
        if all.is_empty() && !alt {
            if let Some(other) = alt_numbers(title).filter(|o| o != title) {
                return once(web, sys, &other, true);
            }
        }
        // only "+": "&" is part of most names
        if all.is_empty() {
            if let Some(first) = PLUS.split(title).next().filter(|f| *f != title && words(f).len() >= 2) {
                return once(web, sys, first, alt);
            }
        }
        all.truncate(6);
        Ok(all)
    }
    once(web, sys, title, false)
}

// ---------- the download ----------

/// Whether a file of an upload is one of its pages: the uploader's own scan (not one of archive.org's
/// thumbnails or tiles), more than a thumbnail's few kB, and small enough to show.
pub fn is_page_image(f: &Value) -> bool {
    static PICTURE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(jpe?g|png)$"));
    static THUMB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)__ia_thumb|_thumb\.|\.thumbs/|_itemimage|(^|/)thumb"));
    static DERIVED: LazyLock<Regex> = LazyLock::new(|| re("(?i)thumbnail|item tile|spectrogram"));
    let name = f.get("name").and_then(Value::as_str).unwrap_or("");
    if !js::truthy(Some(f)) || !PICTURE.is_match(name) || THUMB.is_match(name) {
        return false;
    }
    if js::truthy(f.get("source")) && f["source"] != "original" {
        return false;
    }
    if js::truthy(f.get("format")) && DERIVED.is_match(&js::string(f.get("format"))) {
        return false;
    }
    let size = js::number(f.get("size"));
    !(size.is_finite() && size != 0.0 && !(15_000.0..=80.0 * 1_048_576.0).contains(&size))
}

fn page_file(dir: &Path, base: &str, n: usize, ext: &str) -> PathBuf {
    dir.join(format!("{base}-manual-{n:02}{ext}"))
}

/// Downloads an upload of archive.org as a game's manual pages into `dir`, named after `base`. Returns how
/// many pages are there. A manual with pages missing isn't kept: what this run saved goes, with an error.
/// `progress(pages down, of)`.
pub fn fetch_manual(web: &dyn Web, id: &str, dir: &Path, base: &str, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<usize, String> {
    static ZIP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)_jp2\.zip$"));
    let enc = js::encode_uri_component;
    let meta = web.json(&format!("https://archive.org/metadata/{}", enc(id))).map_err(|e| e.words())?;
    let files: Vec<Value> = meta.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
    let name_of = |f: &Value| f.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let made: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    let gaps = |n: usize, of: usize| -> String {
        for f in made.lock().unwrap().iter() {
            let _ = fs::remove_file(f);
        }
        format!("only {n} of {of} pages came down")
    };
    let Some(zip) = files.iter().map(name_of).find(|n| ZIP.is_match(n)) else {
        // loose pictures: only the uploader's own scans
        let mut scans: Vec<String> = files.iter().filter(|f| is_page_image(f)).map(name_of).collect();
        scans.sort_by(|a, b| collate::compare(a, b).then_with(|| collate::compare_exact(a, b)));
        scans.truncate(MAX_PAGES);
        let mut saved = 0;
        for (i, name) in scans.iter().enumerate() {
            let dest = page_file(dir, base, i + 1, &js::extname(name).to_lowercase());
            if dest.is_file() {
                saved += 1;
                continue;
            }
            made.lock().unwrap().push(dest.clone());
            let url = format!("https://archive.org/download/{}/{}", enc(id), enc(name));
            let mut ok = web.file(&url, &dest).unwrap_or(false);
            if !ok {
                web.pause(3000);
                ok = web.file(&url, &dest).unwrap_or(false);
            }
            saved += usize::from(ok);
            progress(i + 1, scans.len());
        }
        return if saved < scans.len() { Err(gaps(saved, scans.len())) } else { Ok(saved) };
    };
    let stem = ZIP.replace(&zip, "").into_owned();
    // how many pages: the upload says, else its page numbers do, else its scan's data
    let said = js::number(meta.get("metadata").and_then(|m| m.get("imagecount")));
    let mut pages = if said.is_finite() && said > 0.0 { said as usize } else { 0 };
    if pages == 0 {
        if let Some(numbers) = files.iter().map(name_of).find(|n| n.to_lowercase().ends_with("_page_numbers.json")) {
            pages = web.json(&format!("https://archive.org/download/{}/{}", enc(id), enc(&numbers))).ok().and_then(|j| j.get("pages").and_then(Value::as_array).map(Vec::len)).unwrap_or(0);
        }
    }
    if pages == 0 {
        if let Some(scan) = files.iter().map(name_of).find(|n| n.to_lowercase().ends_with("_scandata.xml")) {
            let tmp = dir.join(format!(".{base}-scandata.tmp"));
            if web.file(&format!("https://archive.org/download/{}/{}", enc(id), enc(&scan)), &tmp).unwrap_or(false) {
                static PAGE: LazyLock<Regex> = LazyLock::new(|| re(r"<page\s"));
                pages = fs::read_to_string(&tmp).map_or(0, |xml| PAGE.find_iter(&xml).count());
            }
            let _ = fs::remove_file(&tmp);
        }
    }
    let total = if pages > 0 { pages } else { 40 }.min(MAX_PAGES);
    let got: Vec<Mutex<bool>> = (0..total).map(|_| Mutex::new(false)).collect();
    let (server, at) = (js::string(meta.get("server")), js::string(meta.get("dir")));
    let down = || got.iter().filter(|g| *g.lock().unwrap()).count();
    let run = |todo: &[usize], workers: usize| {
        let next = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..workers.min(todo.len()) {
                scope.spawn(|| loop {
                    let Some(&i) = todo.get(next.fetch_add(1, Ordering::Relaxed)) else { return };
                    let dest = page_file(dir, base, i + 1, ".jpg");
                    if dest.is_file() {
                        *got[i].lock().unwrap() = true;
                        continue;
                    }
                    made.lock().unwrap().push(dest.clone());
                    let mut ok = web.file(&format!("https://archive.org/download/{}/page/n{i}_w{PAGE_WIDTH}.jpg", enc(id)), &dest).unwrap_or(false);
                    if !ok {
                        let inner = format!("{stem}_jp2/{stem}_{i:04}.jp2");
                        ok = web.file(&format!("https://{server}/BookReader/BookReaderImages.php?zip={}&file={}&id={}&scale=2&rotate=0", enc(&format!("{at}/{zip}")), enc(&inner), enc(id)), &dest).unwrap_or(false);
                    }
                    *got[i].lock().unwrap() = ok;
                    progress(down(), total);
                });
            }
        });
    };
    run(&(0..total).collect::<Vec<_>>(), PARALLEL);
    // pages that didn't come down get one more go, a little later
    let missing: Vec<usize> = (0..total).filter(|i| !*got[*i].lock().unwrap()).collect();
    if pages > 0 && !missing.is_empty() {
        web.pause(3000);
        run(&missing, 2);
    }
    // the page count was a guess: past the end the site repeats the last page, and those go
    if pages == 0 {
        let mut last: Option<Vec<u8>> = None;
        for (i, there) in got.iter().enumerate() {
            let file = page_file(dir, base, i + 1, ".jpg");
            let Ok(bytes) = fs::read(&file) else { continue };
            let hash = Md5::digest(&bytes).to_vec();
            if last.as_ref() == Some(&hash) {
                let _ = fs::remove_file(&file);
                *there.lock().unwrap() = false;
            } else {
                last = Some(hash);
            }
        }
    }
    let saved = down();
    if pages > 0 && saved < total {
        return Err(gaps(saved, total));
    }
    Ok(saved)
}

/// The widest a page is kept: a bigger scan is made smaller once it's down.
const PAGE_MAX_W: u32 = 3000;

/// Makes a manual's oversized pages smaller, as JPEGs. Returns how many.
pub fn shrink_pages(dir: &Path, base: &str) -> usize {
    let prefix = format!("{base}-manual-").to_lowercase();
    let mut n = 0;
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let picture = name.ends_with(".jpg") || name.ends_with(".jpeg") || name.ends_with(".png");
        if !name.starts_with(&prefix) || !picture || !crate::boxart::size_of(&entry.path()).is_some_and(|(w, _)| w > PAGE_MAX_W) {
            continue;
        }
        let Ok(img) = image::open(entry.path()) else { continue };
        let small = img.resize(PAGE_MAX_W, u32::MAX, image::imageops::FilterType::Triangle).into_rgb8();
        let out = entry.path().with_extension("jpg");
        let tmp = PathBuf::from(format!("{}.tmp", out.display()));
        let written = fs::File::create(&tmp).map_err(|e| e.to_string()).and_then(|f| image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 88).encode_image(&small).map_err(|e| e.to_string()));
        if written.is_ok() && fs::rename(&tmp, &out).is_ok() {
            if out != entry.path() {
                let _ = fs::remove_file(entry.path());
            }
            n += 1;
        } else {
            let _ = fs::remove_file(&tmp);
        }
    }
    n
}

/// What a download gave: a manual's pages, or GamesDatabase's PDF.
#[derive(Debug, Clone, PartialEq)]
pub enum Got {
    Pages(usize),
    Pdf(PathBuf),
}

/// Downloads a chosen candidate (by its `id`) as the manual of the game whose art is named `base` in `dir`.
pub fn download(web: &dyn Web, id: &str, dir: &Path, base: &str, progress: &(dyn Fn(usize, usize) + Sync)) -> Result<Got, String> {
    static GDB: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)https?://(www\.)?gamesdatabase\.org/[^?#]*\.pdf$"));
    if let Some(url) = id.strip_prefix("gdb:") {
        // only a PDF on GamesDatabase, as its search offers: never any other address
        if !GDB.is_match(url) {
            return Err("Not a GamesDatabase manual.".into());
        }
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let pdf = dir.join(format!("{base}-manual.pdf"));
        return match web.file(url, &pdf) {
            Ok(true) => Ok(match pdf_to_pages(&pdf, dir, base) {
                Ok(n) if n > 0 => Got::Pages(n),
                _ => Got::Pdf(pdf),
            }),
            _ => Err("download failed".into()),
        };
    }
    let n = fetch_manual(web, id, dir, base, progress)?;
    if n > 0 {
        shrink_pages(dir, base);
    }
    Ok(Got::Pages(n))
}

/// the most pages a PDF is drawn into (renderer/pdf-pages.js): a longer one is kept beside its pages
pub const PDF_MAX_PAGES: usize = 80;

/// A PDF manual drawn into `<base>-manual-NN.jpg` pages beside it (the pages it had before give way), 1600 pixels
/// across as the Electron app draws them. The PDF goes once all of it is in pages. Returns the page count.
pub fn pdf_to_pages(pdf: &Path, dir: &Path, base: &str) -> Result<usize, String> {
    let bytes = fs::read(pdf).map_err(|e| e.to_string())?;
    let pages = crate::pdfpages::pages(&bytes, 1600, PDF_MAX_PAGES, false)?;
    if pages.is_empty() {
        return Ok(0);
    }
    let pre = format!("{base}-manual-").to_lowercase();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.starts_with(&pre) && [".jpg", ".jpeg", ".png", ".webp"].iter().any(|x| name.ends_with(x)) {
            let _ = fs::remove_file(e.path());
        }
    }
    let mut made = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let f = dir.join(format!("{base}-manual-{:02}.jpg", i + 1));
        if let Err(e) = fs::write(&f, page) {
            for m in &made {
                let _ = fs::remove_file(m);
            }
            return Err(e.to_string());
        }
        made.push(f);
    }
    if pages.len() < PDF_MAX_PAGES {
        let _ = fs::remove_file(pdf);
    }
    Ok(pages.len())
}

// ---------- the automatic pick ----------

/// Whether a candidate gives what a console's choice asks for: "best" (English; a language that isn't said
/// counts as English), "lang:xx", "region:eur" (English from Europe) or "region:jpn".
pub fn pref_match(c: &Candidate, pref: &str) -> bool {
    let (lang, region) = (lang_code(&c.language, &c.title), region_of(&c.title));
    let en = lang.is_empty() || lang == "en";
    match pref {
        "region:eur" => en && region == "eur",
        "region:jpn" => lang == "ja" || region == "jpn",
        p => match p.strip_prefix("lang:").filter(|l| l.len() == 2 && l.bytes().all(|b| b.is_ascii_lowercase())) {
            Some(l) => lang == l,
            None => en,
        },
    }
}

/// The pick among a game's candidates for its console's choice, without the ones set aside for it
/// (`rejected`): the best that gives the choice (a copy in another language gets the search's language
/// penalty back), else the best English one, so a game still gets a manual.
pub fn pref_pick(list: &[Candidate], pref: &str, rejected: &dyn Fn(&str) -> bool) -> Option<Candidate> {
    let english = || list.iter().find(|x| x.english() && !rejected(&x.id)).cloned();
    if pref.is_empty() || pref == "best" {
        return english();
    }
    let mut fits: Vec<Candidate> = list.iter().filter(|x| !rejected(&x.id) && pref_match(x, pref)).map(|x| Candidate { score: x.score + if x.english() { 0.0 } else { 4.0 }, ..x.clone() }).collect();
    fits.sort_by(|a, b| b.score.total_cmp(&a.score));
    fits.into_iter().next().filter(|hit| hit.score >= AUTO_MIN).or_else(english)
}

/// The games the automatic search has looked at (`manual-auto.json`), the copies set aside for a game
/// (`manual-rejects.json`), and each console's choice and the games whose manual was chosen by hand
/// (`manual-studio.json`): the Electron app's own files in the data folder.
pub struct Stores {
    data_dir: PathBuf,
    auto_file: PathBuf,
    pub auto: Map<String, Value>,
    rejects: Map<String, Value>,
    studio: Value,
}

/// the version of the matching: a game that found nothing under an older one is looked at again
// (main/manuals.js AUTO_V, keep in sync)
const AUTO_V: u64 = 4;
const DAY: f64 = 86_400_000.0;

impl Stores {
    pub fn load(data_dir: &Path) -> Stores {
        let read = |name: &str| fs::read(data_dir.join(name)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let map = |v: Option<Value>| match v {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        Stores { data_dir: data_dir.to_path_buf(), auto_file: data_dir.join("manual-auto.json"), auto: map(read("manual-auto.json")), rejects: map(read("manual-rejects.json")), studio: read("manual-studio.json").unwrap_or(Value::Null) }
    }

    pub fn save(&self) {
        let tmp = PathBuf::from(format!("{}.tmp", self.auto_file.display()));
        if fs::write(&tmp, Value::Object(self.auto.clone()).to_string()).is_ok() {
            let _ = fs::rename(&tmp, &self.auto_file);
        }
    }

    pub fn rejected(&self, key: &str, id: &str) -> bool {
        !id.is_empty() && self.rejects.get(key).and_then(Value::as_array).is_some_and(|r| r.iter().any(|x| x.as_str() == Some(id)))
    }

    fn write(&self, name: &str, what: &Value) {
        let (file, tmp) = (self.data_dir.join(name), self.data_dir.join(format!("{name}.tmp")));
        if fs::create_dir_all(&self.data_dir).is_ok() && fs::write(&tmp, what.to_string()).is_ok() {
            let _ = fs::rename(&tmp, &file);
        }
    }

    /// Sets copies aside for a game: neither the automatic search nor the collections bring them again.
    pub fn reject(&mut self, key: &str, ids: &[String]) {
        let mut all: Vec<Value> = self.rejects.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        for id in ids {
            if !all.iter().any(|x| x.as_str() == Some(id)) {
                all.push(json!(id));
            }
        }
        self.rejects.insert(key.to_string(), Value::Array(all));
        self.write("manual-rejects.json", &Value::Object(self.rejects.clone()));
    }

    /// The choices a console's manuals can follow, by id and name.
    pub const PREFS: [(&'static str, &'static str); 7] =
        [("best", "Best English manual"), ("region:eur", "English · Europe"), ("region:jpn", "Japanese"), ("lang:fr", "French"), ("lang:de", "German"), ("lang:es", "Spanish"), ("lang:it", "Italian")];

    fn studio_part(&mut self, part: &str) -> &mut Map<String, Value> {
        if !self.studio.is_object() {
            self.studio = json!({});
        }
        let whole = self.studio.as_object_mut().expect("an object");
        if !whole.get(part).is_some_and(Value::is_object) {
            whole.insert(part.to_string(), json!({}));
        }
        whole[part].as_object_mut().expect("an object")
    }

    /// Makes a console's manuals follow a language or a region (one of `PREFS`). The games that found nothing
    /// under the old choice are looked up again.
    pub fn set_pref(&mut self, sys: &str, pref: &str) -> Result<(), String> {
        if !Self::PREFS.iter().any(|p| p.0 == pref) {
            return Err("Unknown choice.".into());
        }
        if pref == "best" {
            self.studio_part("prefs").remove(sys);
        } else {
            self.studio_part("prefs").insert(sys.to_string(), json!(pref));
        }
        self.write("manual-studio.json", &self.studio);
        let before = self.auto.len();
        self.auto.retain(|k, v| !k.starts_with(&format!("{sys}|")) || js::truthy(v.get("got")));
        if self.auto.len() != before {
            self.save();
        }
        Ok(())
    }

    /// Keeps a game's manual as it is (or lets it go again): no automatic pass and no new console choice
    /// changes it. `has`: whether it has a manual now.
    pub fn set_lock(&mut self, key: &str, locked: bool, has: bool, now: f64) {
        if locked {
            let src = self.studio.get("cur").and_then(|c| c.get(key)).and_then(|c| c.get("src")).filter(|s| js::truthy(Some(s))).cloned().unwrap_or(json!(if has { "cur" } else { "none" }));
            self.studio_part("locks").insert(key.to_string(), json!({ "at": js::num(now), "src": src }));
        } else {
            self.studio_part("locks").remove(key);
        }
        self.write("manual-studio.json", &self.studio);
    }

    /// The games whose manual the player locked, by key.
    pub fn locked_keys(&self) -> std::collections::HashSet<String> {
        self.studio.get("locks").and_then(Value::as_object).map(|l| l.iter().filter(|(_, v)| js::truthy(Some(v))).map(|(k, _)| k.clone()).collect()).unwrap_or_default()
    }

    /// Whether the player locked a game's manual.
    pub fn is_locked(&self, key: &str) -> bool {
        js::truthy(self.studio.get("locks").and_then(|l| l.get(key)))
    }

    /// A console's choice of language or region; "best" when none was made.
    pub fn pref(&self, sys: &str) -> String {
        self.studio.get("prefs").and_then(|p| p.get(sys)).and_then(Value::as_str).filter(|p| !p.is_empty()).unwrap_or("best").to_string()
    }

    /// Whether a game's manual was chosen by hand (or set to none): the automatic search leaves it alone.
    pub fn locked(&self, key: &str) -> bool {
        let studio = |part: &str| self.studio.get(part).and_then(|m| m.get(key));
        js::truthy(studio("locks")) || studio("cur").is_some_and(|c| matches!(c["origin"].as_str(), Some("studio" | "own")))
    }

    /// Whether the automatic search is due for a game: not looked at under this version of the matching, or
    /// long enough ago (a day after an error, a month after finding nothing).
    pub fn due(&self, key: &str, now: f64) -> bool {
        let Some(seen) = self.auto.get(key) else { return true };
        seen.get("v").and_then(Value::as_u64) != Some(AUTO_V) || now - js::number(seen.get("at")) >= if js::truthy(seen.get("err")) { DAY } else { 30.0 * DAY }
    }

    fn note(&mut self, key: &str, now: f64, what: Value) {
        let mut entry = json!({ "v": AUTO_V, "at": js::num(now) });
        if let (Some(e), Some(w)) = (entry.as_object_mut(), what.as_object()) {
            e.extend(w.clone());
        }
        self.auto.insert(key.to_string(), entry);
    }
}

/// A game without a manual, for the automatic search.
pub struct Wanted {
    pub sys: String,
    pub title: String,
    /// its file, as the stores name it with its console
    pub file: String,
    /// where its manual goes, and the name its art has
    pub dir: PathBuf,
    pub base: String,
}

impl Wanted {
    pub fn key(&self) -> String {
        format!("{}|{}", self.sys, self.file.to_lowercase())
    }
}

/// Looks for a manual for each of these games (forty at most a run) and downloads the confident matches:
/// English, or what the console's choice asks for. What was found, or not, is remembered. Returns the games
/// searched and the manuals that came in.
pub fn auto_fetch(web: &dyn Web, games: &[Wanted], stores: &mut Stores, now: f64, cancelled: &dyn Fn() -> bool) -> (usize, usize) {
    static BRAND: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(nickelodeon|disney[- ]pixar|disney's|disney|pixar|dreamworks|cartoon network|mtv)\s+"));
    const AUTO_MAX: usize = 40;
    let todo: Vec<&Wanted> = games.iter().filter(|g| !stores.locked(&g.key()) && stores.due(&g.key(), now)).take(AUTO_MAX).collect();
    let mut picks: Vec<(&Wanted, Candidate)> = Vec::new();
    for g in &todo {
        if cancelled() {
            break;
        }
        let key = g.key();
        let found = match search(web, &g.sys, &g.title) {
            Ok(found) => found,
            Err(e) => {
                stores.note(&key, now, json!({ "err": e.words() }));
                continue;
            }
        };
        let mut best = pref_pick(&found, &stores.pref(&g.sys), &|id| stores.rejected(&key, id));
        // the last resort: nothing for this console anywhere. A game on several consoles of a generation has the
        // same manual but for the controls page: an exact title match from a sibling console beats having none
        if !best.as_ref().is_some_and(|b| b.score >= AUTO_MIN) && matches!(g.sys.as_str(), "gc" | "wii" | "ps2" | "ps3" | "x360" | "ps4") {
            let plain = BRAND.replace(&g.title, "").into_owned();
            let other = search_archive(web, &g.sys, &plain, true).unwrap_or_default().into_iter().find(|x| matches!(x.language.as_str(), "eng" | "en" | "english" | "") && similarity(&plain, &x.title) >= 0.95 && !stores.rejected(&key, &x.id));
            if let Some(other) = other.filter(|o| o.score >= AUTO_MIN) {
                best = Some(other);
            }
        }
        match best {
            Some(b) if b.score >= AUTO_MIN => picks.push((g, b)),
            other => stores.note(&key, now, json!({ "none": true, "best": other.map_or(String::new(), |b| format!("{} ({:.1})", b.title, b.score)) })),
        }
        web.pause(300);
    }
    stores.save();
    let mut got = 0;
    for (g, pick) in picks {
        if cancelled() {
            break;
        }
        match download(web, &pick.id, &g.dir, &g.base, &|_, _| {}) {
            Ok(Got::Pages(n)) if n > 0 => {
                got += 1;
                stores.note(&g.key(), now, json!({ "got": pick.id, "title": pick.title }));
            }
            Ok(Got::Pdf(_)) => {
                got += 1;
                stores.note(&g.key(), now, json!({ "got": pick.id, "title": pick.title }));
            }
            Ok(Got::Pages(_)) => stores.note(&g.key(), now, json!({ "err": "no pages" })),
            Err(why) => stores.note(&g.key(), now, json!({ "err": why })),
        }
    }
    stores.save();
    (todo.len(), got)
}

/// What setting a manual aside came to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rejected {
    /// the files moved aside
    pub moved: usize,
    /// the pages of the manual that came in its place (0: none did)
    pub got: usize,
    /// where that one is from
    pub from: Option<String>,
    /// it came as a PDF
    pub pdf: bool,
    /// the search couldn't be made
    pub offline: bool,
}

/// "Wrong manual": the pages belong to another game. The copy is moved aside (to `.manual-originals`, so
/// nothing is lost) and where it came from is remembered, so that neither the search nor the collections bring
/// it again; then the next best takes its place: a scan collection's copy first, then the search's.
pub fn reject_manual(web: &dyn Web, data_dir: &Path, g: &crate::manualcoll::Game, now: &dyn Fn() -> f64) -> Result<Rejected, String> {
    use crate::manualcoll;
    let key = g.key();
    let mut stores = Stores::load(data_dir);
    let mut memo = manualcoll::Memo::load(data_dir);
    // which copy this was: the collections' record, the search's own, or (older downloads) the match the search
    // makes now
    let mut bad: Vec<String> = Vec::new();
    if let Some(src) = memo.forget(&key, now()) {
        bad.push(format!("coll:{src}"));
        if let Some(pdf) = src.strip_prefix("gdb/") {
            bad.push(format!("gdb:{pdf}"));
        }
    }
    if let Some(got) = stores.auto.get(&key).and_then(|m| m.get("got")).filter(|got| js::truthy(Some(got))) {
        bad.push(js::string(Some(got)));
    }
    let found = search(web, &g.sys, &g.title);
    let cands: &[Candidate] = found.as_deref().unwrap_or(&[]);
    let next = |stores: &Stores| cands.iter().find(|x| x.english() && x.score >= AUTO_MIN && !stores.rejected(&key, &x.id)).cloned();
    if bad.is_empty() {
        bad.extend(next(&stores).map(|top| top.id));
    }
    stores.reject(&key, &bad);
    // the pages go aside
    let pre = format!("{}-manual", g.base.to_lowercase());
    let files: Vec<String> = fs::read_dir(&g.dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| {
            let l = f.to_lowercase();
            l.starts_with(&format!("{pre}-")) || l == format!("{pre}.pdf")
        })
        .collect();
    let mut moved = 0;
    if let Some(keep) = Some(()).filter(|()| !files.is_empty()).and_then(|()| crate::spreads::backup_dir(&g.dir, &g.base).ok()) {
        for (i, f) in files.iter().enumerate() {
            if let Err(e) = fs::rename(g.dir.join(f), keep.join(f)) {
                // what moved goes back: the manual isn't left half there
                for d in &files[..i] {
                    let _ = fs::rename(keep.join(d), g.dir.join(d));
                }
                return Err(format!("Couldn't move the manual aside ({e}). Is a page open in another program?"));
            }
        }
        moved = files.len();
    }
    if let Some((got, from)) = manualcoll::fetch_for(web, data_dir, g, &stores, &mut memo, now, None) {
        stores.auto.remove(&key);
        stores.save();
        return Ok(Rejected { moved, got, from: Some(from), ..Rejected::default() });
    }
    let Some(next) = next(&stores) else {
        let what = match &found {
            Err(e) => json!({ "err": e.words(), "rejected": true }),
            Ok(_) => json!({ "none": true, "rejected": true }),
        };
        stores.note(&key, now(), what);
        stores.save();
        return Ok(Rejected { moved, offline: found.is_err(), ..Rejected::default() });
    };
    let got = download(web, &next.id, &g.dir, &g.base, &|_, _| {});
    let what = match &got {
        Ok(_) => json!({ "got": next.id, "title": next.title }),
        Err(why) => json!({ "err": why }),
    };
    stores.note(&key, now(), what);
    stores.save();
    Ok(match got {
        Ok(Got::Pages(n)) => Rejected { moved, got: n, from: Some(next.title), ..Rejected::default() },
        Ok(Got::Pdf(_)) => Rejected { moved, got: 1, from: Some(next.title), pdf: true, ..Rejected::default() },
        Err(_) => Rejected { moved, from: Some(next.title), ..Rejected::default() },
    })
}

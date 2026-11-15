// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/manualcoll.js, as RustyShelf has it.
//! Manual collections on the Internet Archive: single uploads holding hundreds of manuals for one console,
//! scanned at full resolution. These are the sharpest manuals around, so they're used first, and they replace
//! a manual that's there when they aren't clearly blurrier. Old pages are moved to
//! `.manual-originals/<game>/<time>` beside them, never deleted or overwritten.
//!
//! A collection's manual is a PDF with its page scans beside it; the scans are what's downloaded here. A
//! manual that only exists as a PDF (Microsoft's digital Xbox 360 manuals, and a small scan's sharper copy on
//! GamesDatabase) needs the Electron app, which draws PDFs into pages.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::boxart::size_of;
use crate::manuals::{self, Stores, Web};
use crate::spreads::{self, Split};
use crate::{collate, js};

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// One of the Internet Archive's uploads of a console's manuals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Collection {
    pub id: &'static str,
    /// the folder of the upload this console's manuals are in ("" for all of it)
    pub folder: &'static str,
    /// how its files are named: "" for "Title (Region).pdf"; "squash" for one word with the region at the end
    /// ("ProfessorLaytonAndTheUnwoundFutureusa.pdf"); "loose" for Nintendo's own short names
    /// ("Wii_Super_Smash_Brawl.pdf"); "set" for an arcade set's name ("2020bb.pdf")
    pub names: &'static str,
    /// digital manuals, kept as PDFs only
    pub digital: bool,
}

const fn scans(id: &'static str) -> Collection {
    Collection { id, folder: "", names: "", digital: false }
}

/// Nintendo of America's high-resolution manuals, a folder for each console.
const fn hires(folder: &'static str) -> Collection {
    Collection { id: "Nintendo-Hi-Res-Manuals-2026", folder, names: "loose", digital: false }
}

/// Every console that has a scan collection. The Wii U reads its games' own manuals, and Switch, PS3, PS4 and
/// PS5 games have none to collect.
pub fn collections(sys: &str) -> &'static [Collection] {
    const NEOGEO: [Collection; 1] = [Collection { id: "SNKNeoGeoMVSManuals", folder: "", names: "set", digital: false }];
    match sys {
        "gc" => &const { [scans("NintendoGameCubeManuals"), Collection { id: "GCN-GBA-manuals", folder: "Gamecube/Game Manuals/", names: "", digital: false }, hires("Gamecube Games/")] },
        "wii" => &const { [scans("wii-manuals_202508"), hires("Wii Games/")] },
        "ps2" => &const { [scans("SonyPlaystation2Manuals_201812"), scans("Ps2-game-manual")] },
        "psp" => &const { [scans("SonyPSPManuals")] },
        "x360" => &const { [Collection { id: "xbox-360-marketplace-game-manuals", folder: "", names: "", digital: true }, scans("xbox-360-manuals")] },
        "psx" => &const { [scans("SonyPlaystationManuals")] },
        "nes" => &const { [scans("NESManuals")] },
        "snes" => &const { [scans("SNESManuals")] },
        "n64" => &const { [scans("Nintendo64Manuals_201812"), hires("Nintendo 64 Games/")] },
        "gb" => &const { [scans("NintendoGameBoyManuals")] },
        "gbc" => &const { [scans("NintendoGameBoyColorManuals")] },
        "gba" => &const { [scans("NintendoGameBoyAdvanceManuals"), hires("GBA Games/")] },
        "nds" => &const { [Collection { id: "NintendoDSManuals", folder: "", names: "squash", digital: false }, hires("DS Games/")] },
        "n3ds" => &const { [scans("Nintendo3DSManuals"), hires("3DS Games/")] },
        "megadrive" => &const { [scans("SEGAGenesisMegaDriveManuals")] },
        "mastersystem" => &const { [scans("SEGAMasterSystemManuals")] },
        "gamegear" => &const { [scans("SEGAGameGearManuals")] },
        "dreamcast" => &const { [scans("SEGADreamcastManuals_201812")] },
        "saturn" => &const { [scans("SEGASaturnManuals_201812")] },
        "segacd" => &const { [scans("SEGACDManuals")] },
        "sega32x" => &const { [scans("SEGA32XManuals_201812")] },
        "pcengine" => &const { [scans("NECTurboGrafx-16Manuals")] },
        "atari2600" => &const { [scans("Atari2600Manuals_201812")] },
        "atarilynx" => &const { [scans("AtariLynxManuals_201812")] },
        "neogeo" | "arcade" => &NEOGEO,
        _ => &[],
    }
}

/// download failures recorded before a download fix are tried again at once: goes up with a fix
// (main/manualcoll.js ERR_V, INDEX_V, VERSION: keep in sync, both apps write the same files)
const ERR_V: u64 = 1;
const INDEX_V: u64 = 3;
const VERSION: u64 = 3;
const INDEX_DAYS: f64 = 30.0;
const DAY: f64 = 86_400_000.0;
/// downloads a run; the rest follow on the next ones
const MAX_GAMES: usize = 80;
/// a page narrower than this can't be read; a manual is "much sharper" at twice the width; a scan under LOW_RES
/// is a small one
const MIN_READABLE: u32 = 560;
const MUCH_SHARPER: u32 = 2;
/// more scans than this is more than any manual has
const MAX_SCANS: usize = 240;

/// A file's name as a game's: its title, the tags in brackets, a title id among them, the disc.
#[derive(Debug, Clone, PartialEq)]
pub struct Cleaned {
    pub title: String,
    pub tags: Vec<String>,
    pub title_id: Option<String>,
    pub disc: u32,
}

/// "Legend of Zelda, The - The Wind Waker (USA)" → "The Legend of Zelda - The Wind Waker", with the region and
/// version tags and the title id kept aside.
pub fn clean(stem: &str) -> Cleaned {
    static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"\(([^)]*)\)"));
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| re(r"\s*[(\[][^)\]]*[)\]]"));
    static ARTICLE: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(.*?), (The|A|An)(\s+-\s+.*)?$"));
    static TITLE_ID: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)[0-9A-F]{8}$"));
    static DISC: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)disc [0-9]"));
    let t = stem.replace('_', " ");
    let tags: Vec<String> = TAG.captures_iter(&t).map(|m| m[1].to_string()).collect();
    let t = BRACKETS.replace_all(&t, "");
    let t = js::trim(&t);
    let title = match ARTICLE.captures(t) {
        Some(m) => format!("{} {}{}", &m[2], &m[1], m.get(3).map_or("", |c| c.as_str())),
        None => t.to_string(),
    };
    let disc = tags.iter().find(|x| DISC.is_match(x)).map_or(0, |d| d.chars().filter(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0));
    Cleaned { title, title_id: tags.iter().find(|x| TITLE_ID.is_match(x)).map(|x| x.to_uppercase()), tags, disc }
}

/// region and language codes a squashed name ends with ("…Futureusa", "…Skiesusaenfres"), taken off one at a time
const CODES: [&str; 25] = ["usa", "eur", "europe", "jpn", "japan", "aus", "kor", "uk", "en", "eng", "fr", "de", "es", "it", "ja", "nl", "sv", "da", "no", "fi", "pt", "ko", "zh", "ru", "pl"];

fn camel(s: &str) -> String {
    static CASE: LazyLock<Regex> = LazyLock::new(|| re(r"([a-z])([A-Z])"));
    static THEN_DIGIT: LazyLock<Regex> = LazyLock::new(|| re(r"([A-Za-z])([0-9])"));
    static THEN_LETTER: LazyLock<Regex> = LazyLock::new(|| re(r"([0-9])([A-Za-z])"));
    let s = CASE.replace_all(s, "$1 $2");
    let s = THEN_DIGIT.replace_all(&s, "$1 $2");
    THEN_LETTER.replace_all(&s, "$1 $2").into_owned()
}

fn squash(s: &str) -> String {
    manuals::norm(s).chars().filter(|c| !c.is_whitespace()).collect()
}

fn posix_basename(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or("")
}

/// The ways a squashed name can be read: as it is, then with one more code off its end.
fn squash_variants(stem: &str) -> Vec<String> {
    let mut s = squash(&camel(stem));
    let mut out = vec![s.clone()];
    for _ in 0..6 {
        let Some(code) = CODES.iter().find(|k| s.chars().count() > k.len() + 4 && s.ends_with(**k)) else { break };
        s.truncate(s.len() - code.len());
        out.push(s.clone());
    }
    out
}

// Nintendo's short names: the console in front, the language and "single sheet" or "e-manual" behind
static LOOSE_PRE: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)(manual[-_ ]+)?((nintendo[-_ ]?)?(wii[-_ ]?u|wii|3ds|ds|64|gamecube|gcn|gc)|nintendo[-_ ]?64|n64|agb|gba|ntr|ctr|rvl)[-_ ]+"));
static LOOSE_POST: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[-_ ]+(en|eng|english|single[-_ ]sheet|e[-_ ]?manual|manual|digital|trilingual|3ds|ds|wii|wiiu|n64)$"));

fn abbreviation(word: &str) -> Option<&'static str> {
    Some(match word {
        "dkc" => "donkey kong country",
        "dk" => "donkey kong",
        "nsmbw" => "new super mario bros wii",
        "nsmb" => "new super mario bros",
        "smb" => "super mario bros",
        "loz" => "legend of zelda",
        _ => return None,
    })
}

fn loose_name(stem: &str) -> String {
    static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| re(r"[-_]+"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
    let mut s = posix_basename(stem).to_string();
    for _ in 0..3 {
        s = LOOSE_PRE.replace(&s, "").into_owned();
    }
    for _ in 0..4 {
        s = LOOSE_POST.replace(&s, "").into_owned();
    }
    let spaced = SEPARATORS.replace_all(&camel(&s), " ").into_owned();
    SPACES.split(&spaced).map(|w| abbreviation(&w.to_lowercase()).unwrap_or(w)).collect::<Vec<_>>().join(" ")
}

/// A name's words that say which game it is: the ones that say what a manual is for are left out on both sides
/// of a loose comparison ("New Play Control! Pikmin" is "Play_Control_Pikmin").
fn sig(s: &str) -> BTreeSet<String> {
    const IGNORE: [&str; 17] = ["wii", "ds", "nds", "3ds", "gba", "agb", "n64", "nintendo", "gamecube", "manual", "edition", "special", "collectors", "collection", "version", "eng", "en"];
    let mut out = BTreeSet::new();
    for w in manuals::words(s) {
        for x in abbreviation(&w).unwrap_or(&w).split(' ') {
            if !x.is_empty() && !IGNORE.contains(&x) && x != "control" && x != "new" {
                out.insert(x.to_string());
            }
        }
    }
    out
}

/// Every word of the file's name is in the game's name, they cover most of it, and the numbers agree.
fn loose_match(file: &BTreeSet<String>, game: &BTreeSet<String>) -> bool {
    if file.is_empty() || game.is_empty() {
        return false;
    }
    if !file.iter().all(|w| game.contains(w) || game.contains(&format!("{w}s")) || w.strip_suffix('s').is_some_and(|one| game.contains(one))) {
        return false;
    }
    let numbers = |s: &BTreeSet<String>| s.iter().filter(|w| !w.is_empty() && w.bytes().all(|c| c.is_ascii_digit())).cloned().collect::<Vec<_>>();
    numbers(file) == numbers(game) && (file.len() >= 2 || file.len() == game.len()) && file.len() as f64 / game.len() as f64 >= 0.75
}

/// What an entry is matched on besides its cleaned title, by how its collection names its files: a title of
/// its own, the readings of a squashed name, an arcade set's name.
pub fn name_keys(stem: &str, names: &str) -> (Option<String>, Option<Vec<String>>, Option<String>) {
    match names {
        "squash" => (None, Some(squash_variants(posix_basename(stem))), None),
        "loose" => {
            let title = loose_name(stem);
            let plain = LOOSE_POST.replace(&LOOSE_PRE.replace(posix_basename(stem), ""), "").into_owned();
            let sq = vec![squash(&title), squash(&plain)];
            (Some(title), Some(sq), None)
        }
        "set" => (None, None, Some(posix_basename(stem).to_lowercase())),
        _ => (None, None, None),
    }
}

/// A manual in a collection, as the collection's index keeps it (`ia-<collection>.json` in the box art cache,
/// the Electron app's own file).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub stem: String,
    pub title: String,
    #[serde(rename = "titleId")]
    pub title_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub disc: u32,
    /// its page scans are there beside the PDF
    #[serde(default)]
    pub jp2: bool,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub digital: bool,
    /// a squashed or loose name's readings; an arcade set's name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sq: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<String>,
}

/// A collection's manuals, as last listed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub v: u64,
    pub at: f64,
    pub id: String,
    /// the server the upload is on and its folder there, for its page scans at full size
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub alt: String,
    #[serde(default)]
    pub dir: String,
    pub entries: Vec<Entry>,
}

/// A collection's manuals: from the box art cache when listed within the month, else from the Internet Archive.
pub fn index(web: &dyn Web, cache_dir: &Path, col: &Collection, now: f64) -> Result<Index, String> {
    static NOT_PLAIN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)[^a-z0-9]+"));
    static PDF: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.pdf$"));
    static TEXT_PDF: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)_text\.pdf$"));
    // not a game's manual: the console's own booklets, safety leaflets, accessories
    static HARDWARE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)operations? manual|health and safety|precautions|wi-?fi|circle pad|usb connector|ar booklet|browser|agbvideo|hardware"));
    static DIGITAL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)digital|e-?manual"));
    static CONSOLE: LazyLock<Regex> = LazyLock::new(|| re(r"^(GCN|AGB)_"));
    let folder = if col.folder.is_empty() { String::new() } else { format!("-{}", NOT_PLAIN.replace_all(col.folder, "_")) };
    let file = cache_dir.join(format!("ia-{}{folder}.json", col.id));
    if let Some(kept) = fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Index>(&b).ok()).filter(|c| c.v == INDEX_V && now - c.at < INDEX_DAYS * DAY) {
        return Ok(kept);
    }
    let meta = web.json(&format!("https://archive.org/metadata/{}", js::encode_uri_component(col.id))).map_err(|e| e.words())?;
    let files: Vec<&Value> = meta.get("files").and_then(Value::as_array).map(|f| f.iter().collect()).unwrap_or_default();
    let name_of = |f: &Value| js::string(f.get("name"));
    let names: std::collections::HashSet<String> = files.iter().map(|f| name_of(f)).collect();
    let entries = files
        .iter()
        .filter_map(|f| {
            let name = name_of(f);
            if !PDF.is_match(&name) || TEXT_PDF.is_match(&name) || !name.starts_with(col.folder) || (!col.names.is_empty() && HARDWARE.is_match(&name)) {
                return None;
            }
            let stem = PDF.replace(&name, "").into_owned();
            let c = clean(&CONSOLE.replace(posix_basename(&stem), ""));
            let size = Some(js::number(f.get("size"))).filter(|n| n.is_finite() && *n > 0.0).map_or(0, |n| n as u64);
            let mut e = Entry { jp2: names.contains(&format!("{stem}_jp2.zip")), digital: DIGITAL.is_match(&name), title: c.title, title_id: c.title_id, tags: c.tags, disc: c.disc, size, mode: col.names.into(), name, ..Entry::default() };
            let (title, sq, set) = name_keys(&stem, col.names);
            if let Some(title) = title {
                e.title = title;
            }
            (e.sq, e.set, e.stem) = (sq, set, stem);
            Some(e)
        })
        .collect();
    let first = |keys: &[&str]| keys.iter().map(|k| js::string(meta.get(*k).filter(|v| js::truthy(Some(v))))).find(|v| v != "undefined").unwrap_or_default();
    let out = Index { v: INDEX_V, at: now, id: col.id.into(), server: first(&["server", "d1"]), alt: first(&["d2"]), dir: first(&["dir"]), entries };
    if fs::create_dir_all(cache_dir).is_ok() {
        let _ = fs::write(&file, serde_json::to_string(&out).unwrap_or_default());
    }
    Ok(out)
}

/// A game, as the collections are asked about it.
#[derive(Debug, Clone, Default)]
pub struct Game {
    /// its console's id and name
    pub sys: String,
    pub system: String,
    pub title: String,
    pub title_id: Option<String>,
    /// its file, in full
    pub file: String,
    pub region: String,
    /// where its manual goes, and the name its art has
    pub dir: PathBuf,
    pub base: String,
}

impl Game {
    pub fn key(&self) -> String {
        format!("{}|{}", self.sys, self.file.to_lowercase())
    }
}

// publisher and licence prefixes dumps carry and collections don't ("Nickelodeon SpongeBob…")
fn unbrand(x: &str) -> String {
    static BRAND: LazyLock<Regex> = LazyLock::new(|| re(r"^(disney s|disneys|disney pixar|disney|pixar|nickelodeon|nick|mtv|cartoon network|dreamworks|marvel|lego)\s+"));
    // ("Ocarina of Time & Master Quest" is "Ocarina of Time - Master Quest")
    static AND: LazyLock<Regex> = LazyLock::new(|| re(r"\band\b"));
    static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s+"));
    let mut y = x.to_string();
    for _ in 0..2 {
        y = BRAND.replace(&y, "").into_owned();
    }
    js::trim(&SPACES.replace_all(&AND.replace_all(&y, " "), " ")).to_string()
}

/// An entry for one region only, and not one an English player reads (Japan, France…), is only for a game of
/// that region.
fn region_ok(e: &Entry, g: &Game) -> bool {
    static ENGLISH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(USA|Europe|World|Australia|Canada|UK|U|E)\b|USA|Europe|\bEn\b"));
    static OTHER: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)Japan|France|Germany|Spain|Italy|Korea|China|Brazil|Netherlands|Sweden|Asia|Taiwan"));
    if !e.tags.iter().any(|t| OTHER.is_match(t)) || e.tags.iter().any(|t| ENGLISH.is_match(t)) {
        return true;
    }
    g.region.to_lowercase().split([' ', ',']).filter(|r| !r.is_empty()).any(|r| e.tags.iter().any(|t| t.to_lowercase().contains(r)))
}

/// Every entry that is this game, the best first: its title id (Xbox 360), else the same name after cleaning,
/// else the same name squashed, else Nintendo's short name for it, else a very close one.
pub fn matches<'a>(entries: &'a [Entry], g: &Game) -> Vec<&'a Entry> {
    static USA: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^USA\b|^U$|EN-US|^World"));
    static EUROPE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)Europe|^E$"));
    static REVISION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^v[0-9]"));
    let cleaned = clean(&g.title).title;
    let want = unbrand(&manuals::norm(&cleaned));
    // (on a 3DS the digital manual is the whole one, the single sheet a leaflet)
    let rank = |e: &Entry| -> f64 {
        let region = if e.tags.iter().any(|t| USA.is_match(t)) { 2.0 } else if e.tags.iter().any(|t| EUROPE.is_match(t)) { 1.0 } else { 0.0 };
        region - if e.tags.iter().any(|t| REVISION.is_match(t)) { 0.1 } else { 0.0 } - if e.disc > 1 { 0.5 } else { 0.0 } + if e.digital { 0.3 } else { 0.0 } - if e.name.to_lowercase().contains("single sheet") { 0.3 } else { 0.0 }
    };
    let best_first = |mut hits: Vec<&'a Entry>| {
        hits.retain(|e| region_ok(e, g));
        hits.sort_by(|a, b| rank(b).total_cmp(&rank(a)));
        hits
    };
    // an arcade set: the zip's own name is the set
    if entries.first().is_some_and(|e| e.mode == "set") {
        let set = js::stem(&js::basename(&g.file)).to_lowercase();
        return if set.is_empty() { Vec::new() } else { entries.iter().filter(|e| e.set.as_deref() == Some(set.as_str())).collect() };
    }
    let tid = g.title_id.as_ref().map(|t| t.to_uppercase());
    let mut hits: Vec<&Entry> = if tid.is_some() { entries.iter().filter(|e| e.title_id == tid).collect() } else { Vec::new() };
    if hits.is_empty() {
        hits = entries.iter().filter(|e| unbrand(&manuals::norm(&e.title)) == want).collect();
    }
    if hits.is_empty() {
        let (q, qu) = (squash(&cleaned), want.chars().filter(|c| !c.is_whitespace()).collect::<String>());
        let fits = |v: &String| v.chars().count() >= 5 && (*v == q || *v == qu);
        hits = entries.iter().filter(|e| e.sq.as_ref().map_or_else(|| fits(&squash(&e.title)), |sq| sq.iter().any(fits))).collect();
    }
    if hits.is_empty() && entries.iter().any(|e| e.mode == "loose") {
        let game = sig(&g.title);
        hits = entries.iter().filter(|e| e.mode == "loose" && loose_match(&sig(&e.title), &game)).collect();
    }
    if !hits.is_empty() {
        return best_first(hits);
    }
    if want.chars().count() > 3 {
        let a = manuals::words(&g.title);
        hits = entries
            .iter()
            .filter(|e| {
                let b = manuals::words(&e.title);
                if a.is_empty() || b.is_empty() {
                    return false;
                }
                let hit = a.iter().filter(|w| b.contains(*w)).count() as f64;
                hit / a.len() as f64 >= 0.95 && hit / b.len() as f64 >= 0.95 && manuals::similarity(&g.title, &e.title) >= 0.95 && manuals::similarity(&e.title, &g.title) >= 0.95
            })
            .collect();
    }
    best_first(hits)
}

/// The best entry for a game.
pub fn pick<'a>(entries: &'a [Entry], g: &Game) -> Option<&'a Entry> {
    matches(entries, g).into_iter().next()
}

/// A file in a folder of the collection: each part of its path is encoded on its own (the "/" stays one).
fn enc_path(p: &str) -> String {
    p.split('/').map(js::encode_uri_component).collect::<Vec<_>>().join("/")
}

/// A scan's page pictures, in order: each from the upload's own server at full size, and archive.org's own
/// link to it, which is half the size (for when the first fails).
pub fn scan_pages(web: &dyn Web, col: &Collection, ix: &Index, e: &Entry) -> Result<Vec<(Option<String>, String)>, String> {
    static PAGE: LazyLock<Regex> = LazyLock::new(|| re(r#"_jp2(?:%2F|/)([^"&]+?_([0-9]{4})\.jp2)"#));
    let enc = js::encode_uri_component;
    // inside the zip the pages sit under the file's own name, without the folder
    let in_zip = posix_basename(&e.stem);
    let zip = format!("https://archive.org/download/{}/{}", enc(col.id), enc_path(&format!("{}_jp2.zip", e.stem)));
    let html = web.text(&format!("{zip}/")).map_err(|e| e.words())?;
    let mut inner: Vec<(u32, String)> = Vec::new();
    for m in PAGE.captures_iter(&html) {
        let Some(name) = js::decode_uri_component(&m[1]) else { return Err("the scan's page list couldn't be read".into()) };
        if !inner.iter().any(|x| x.1 == name) {
            inner.push((m[2].parse().unwrap_or(0), name));
        }
    }
    inner.sort_by_key(|x| x.0);
    if inner.len() > MAX_SCANS {
        return Err(format!("{} page scans: more than a manual has", inner.len()));
    }
    Ok(inner
        .into_iter()
        .map(|(_, f)| {
            let path = format!("{in_zip}_jp2/{f}");
            let full = (!ix.server.is_empty() && !ix.dir.is_empty())
                .then(|| format!("https://{}/BookReader/BookReaderImages.php?zip={}&file={}&id={}&scale=1&rotate=0", ix.server, enc(&format!("{}/{}_jp2.zip", ix.dir, e.stem)), enc(&path), enc(col.id)));
            (full, format!("{zip}/{}&ext=jpg", enc(&path)))
        })
        .collect())
}

/// Downloads a collection's manual into `stage` as `<base>-manual-NN.jpg`. Returns the page count.
pub fn fetch_to(web: &dyn Web, col: &Collection, ix: &Index, e: &Entry, stage: &Path, base: &str, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<usize, String> {
    if col.digital || !e.jp2 {
        return Err("no PDF renderer".into());
    }
    let urls = scan_pages(web, col, ix, e)?;
    if urls.is_empty() {
        return Err("no page images listed".into());
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let ok = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..4.min(urls.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some((full, half)) = urls.get(i).filter(|_| !cancelled()) else { break };
                let dest = stage.join(format!("{base}-manual-{:02}.jpg", i + 1));
                if full.as_ref().is_some_and(|u| web.file(u, &dest).unwrap_or(false)) || web.file(half, &dest).unwrap_or(false) {
                    ok.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            });
        }
    });
    let ok = ok.into_inner();
    if ok != urls.len() {
        return Err(format!("only {ok} of {} pages came down", urls.len()));
    }
    Ok(ok)
}

/// A game's manual files in a folder: its pages, or its PDF.
pub fn manual_files(dir: &Path, base: &str) -> Vec<String> {
    let pre = format!("{base}-manual").to_lowercase();
    let mut files: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| {
            let l = f.to_lowercase();
            (l.starts_with(&format!("{pre}-")) && [".jpg", ".jpeg", ".png", ".webp"].iter().any(|e| l.ends_with(e))) || l == format!("{pre}.pdf")
        })
        .collect();
    files.sort();
    files
}

/// A manual's pages as they are: how many are real pages, how wide the middle one is, how many are junk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub files: Vec<String>,
    pub count: usize,
    pub junk: usize,
    pub width: u32,
}

pub fn inspect(dir: &Path, base: &str) -> Seen {
    let files: Vec<String> = manual_files(dir, base).into_iter().filter(|f| !f.to_lowercase().ends_with(".pdf")).collect();
    let widths: Vec<u32> = files.iter().map(|f| size_of(&dir.join(f)).map_or(0, |z| z.0)).collect();
    let mut real: Vec<u32> = widths.iter().copied().filter(|w| *w >= 300).collect();
    real.sort_unstable();
    Seen { count: real.len(), junk: widths.len() - real.len(), width: real.get(real.len() / 2).copied().unwrap_or(0), files }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to).or_else(|_| fs::copy(from, to).and_then(|_| fs::remove_file(from)))
}

/// What the collections did for each game (`manual-collections.json` in the data folder, the Electron app's
/// own file), by console and file.
pub struct Memo {
    file: PathBuf,
    pub seen: Map<String, Value>,
}

impl Memo {
    pub fn load(data_dir: &Path) -> Memo {
        let file = data_dir.join("manual-collections.json");
        let seen = match fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        Memo { file, seen }
    }

    pub fn save(&self) {
        let tmp = PathBuf::from(format!("{}.tmp", self.file.display()));
        if fs::write(&tmp, Value::Object(self.seen.clone()).to_string()).is_ok() {
            let _ = fs::rename(&tmp, &self.file);
        }
    }

    /// What the collections changed lately, the newest first: `{ title, system, at, upgraded, oldW, w, pages,
    /// source }`.
    pub fn recent(&self, limit: usize) -> Vec<Value> {
        let mut list: Vec<&Value> = self.seen.values().filter(|x| js::truthy(x.get("src")) && js::truthy(x.get("title"))).collect();
        list.sort_by(|a, b| js::number(b.get("at")).total_cmp(&js::number(a.get("at"))));
        let or_zero = |v: Option<&Value>| if js::truthy(v) { v.cloned().unwrap_or(json!(0)) } else { json!(0) };
        list.into_iter()
            .take(limit)
            .map(|x| {
                let src = x["src"].as_str().unwrap_or("");
                json!({ "title": x["title"], "system": if js::truthy(x.get("system")) { x["system"].clone() } else { json!("") }, "at": x["at"], "upgraded": js::truthy(x.get("oldW")), "oldW": or_zero(x.get("oldW")),
                    "w": or_zero(x.get("w")), "pages": or_zero(x.get("pages")), "source": if src.starts_with("gdb/") { "GamesDatabase" } else { src.split('/').next().unwrap_or("") } })
            })
            .collect()
    }

    /// "Wrong manual": forgets a game's collection copy, and says which it was, so that the next look starts
    /// fresh.
    pub fn forget(&mut self, key: &str, now: f64) -> Option<String> {
        let src = self.seen.get(key).and_then(|s| s.get("src")).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        if src.is_some() {
            self.seen.insert(key.into(), json!({ "v": VERSION, "at": js::num(now), "none": true, "rejected": true }));
        } else {
            self.seen.remove(key);
        }
        self.save();
        src
    }
}

/// A collection's manual for a game, and where it's from.
pub struct Hit<'a> {
    pub col: &'a Collection,
    pub ix: &'a Index,
    pub e: &'a Entry,
}

/// Downloads a game's manual from a collection and puts it in place of the one there, unless that one is
/// complete and much sharper (and `force` isn't set). True when the game's manual changed.
pub fn one(web: &dyn Web, g: &Game, hit: &Hit, memo: &mut Memo, now: f64, force: bool, cancelled: &(dyn Fn() -> bool + Sync)) -> Result<bool, String> {
    static PAGE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)-manual-[0-9]+\.(jpe?g|png)$"));
    let key = g.key();
    let old = inspect(&g.dir, &g.base);
    let seen = memo.seen.get(&key).cloned().unwrap_or(json!({}));
    // the manual there is an earlier copy from a collection
    let ours = js::truthy(seen.get("src")) && !old.files.is_empty();
    let stage = std::env::temp_dir().join(format!("rustyshelf-manual-{}-{:x}", std::process::id(), now as u64 ^ (key.len() as u64) << 40 ^ old.files.len() as u64));
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let stopped = || if cancelled() { Err("stopped".to_string()) } else { Ok(()) };
    let done = (|| -> Result<bool, String> {
        fetch_to(web, hit.col, hit.ix, hit.e, &stage, &g.base, cancelled)?;
        // (a digital manual's screens stay whole)
        if !hit.e.digital {
            spreads::split_manual(&stage, &g.base, Split { scan: !hit.col.digital, min_w: 0, discard: true })?;
        }
        manuals::shrink_pages(&stage, &g.base);
        stopped()?;
        let neu = inspect(&stage, &g.base);
        let src = format!("{}/{}", hit.col.id, hit.e.name);
        if neu.width < MIN_READABLE && !ours && old.count > 0 {
            return Err(format!("the collection copy is too blurry to read ({}px)", neu.width));
        }
        // English first: the collection copy replaces a manual of unknown origin when it can be read; an old
        // manual is kept only when it's complete, has no junk pages and is more than twice as sharp
        let incomplete = old.junk > 0 || (old.count as f64) < f64::max(2.0, neu.count as f64 * 0.6);
        if old.count > 0 && !ours && !incomplete && old.width > neu.width * MUCH_SHARPER && !force {
            stopped()?;
            memo.seen.insert(key.clone(), json!({ "v": VERSION, "at": js::num(now), "kept": old.width, "offered": neu.width }));
            return Ok(false);
        }
        let mut pages: Vec<String> = fs::read_dir(&stage).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| PAGE.is_match(f)).collect();
        pages.sort_by(|a, b| collate::compare(a, b));
        stopped()?;
        fs::create_dir_all(&g.dir).map_err(|e| e.to_string())?;
        // the pages there always go to a backup folder of their own; a failed move puts everything back, so
        // the game never ends up with half a manual
        let olds = manual_files(&g.dir, &g.base);
        let keep = if olds.is_empty() { None } else { Some(spreads::backup_dir(&g.dir, &g.base).map_err(|e| e.to_string())?) };
        let (mut moved, mut placed) = (Vec::new(), Vec::new());
        let put = (|| -> std::io::Result<()> {
            for f in &olds {
                move_file(&g.dir.join(f), &keep.as_ref().expect("a backup folder").join(f))?;
                moved.push(f);
            }
            for (i, f) in pages.iter().enumerate() {
                let d = g.dir.join(format!("{}-manual-{:02}{}", g.base, i + 1, js::extname(f).to_lowercase()));
                move_file(&stage.join(f), &d)?;
                placed.push(d);
            }
            Ok(())
        })();
        if let Err(e) = put {
            for d in &placed {
                let _ = fs::remove_file(d);
            }
            for f in &moved {
                let _ = move_file(&keep.as_ref().expect("a backup folder").join(f), &g.dir.join(f));
            }
            // (only goes when everything went back)
            let _ = keep.as_ref().map(fs::remove_dir);
            return Err(format!("could not put the new manual in place ({e}), the old one was kept"));
        }
        let number = |v: Option<&Value>| Some(js::number(v)).filter(|n| !n.is_nan()).unwrap_or(0.0);
        let before = if ours { number(seen.get("oldW")) } else { f64::from(old.width) };
        let had = if ours { js::truthy(seen.get("oldW")) } else { old.count > 0 };
        // (altTried stays off: a small scan's sharper PDF is the Electron app's to look for)
        memo.seen.insert(key.clone(), json!({ "v": VERSION, "at": js::num(now), "src": src, "altTried": false, "w": neu.width, "oldW": if had { js::num(before) } else { json!(0) }, "pages": pages.len(), "title": g.title, "system": g.system }));
        Ok(true)
    })();
    let _ = fs::remove_dir_all(&stage);
    done
}

/// One game, now ("wrong manual"): the best collection copy that isn't one the player set aside (or `skip`, the
/// copy that was there), downloaded and put in place. The pages there are now and where they're from; None
/// when no collection has the game.
pub fn fetch_for(web: &dyn Web, data_dir: &Path, g: &Game, stores: &Stores, memo: &mut Memo, now: &dyn Fn() -> f64, skip: Option<&str>) -> Option<(usize, String)> {
    let key = g.key();
    for col in collections(&g.sys) {
        let Ok(ix) = index(web, &data_dir.join("boxart-cache"), col, now()) else { continue };
        let Some(e) = matches(&ix.entries, g).into_iter().find(|x| {
            let id = format!("{}/{}", col.id, x.name);
            Some(id.as_str()) != skip && !stores.rejected(&key, &format!("coll:{id}"))
        }) else {
            continue;
        };
        let got = one(web, g, &Hit { col, ix: &ix, e }, memo, now(), true, &|| false);
        memo.save();
        if got == Ok(true) {
            return Some((inspect(&g.dir, &g.base).count.max(1), format!("{} · {}", col.id, posix_basename(&e.name))));
        }
    }
    None
}

/// A game the collections have a manual for, and whether its manual changed.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub title: String,
    /// it replaced one that was there
    pub upgraded: bool,
}

/// Looks through the collections for every game of these consoles and downloads what they have: the games
/// without a manual first, then the ones whose manual a collection can better. `skip(console)`: a console set
/// to another language or region (the collections are English, North American). Returns what changed.
pub fn run(web: &dyn Web, data_dir: &Path, games: &[Game], stores: &Stores, now: &dyn Fn() -> f64, skip: &dyn Fn(&str) -> bool, cancelled: &(dyn Fn() -> bool + Sync)) -> Vec<Change> {
    let cache_dir = data_dir.join("boxart-cache");
    let mut memo = Memo::load(data_dir);
    let mut indexes: Vec<(String, Vec<(&'static Collection, Index)>)> = Vec::new();
    let mut jobs: Vec<(u8, &Game, usize, usize, Entry)> = Vec::new();
    for g in games {
        let cols = collections(&g.sys);
        if cols.is_empty() || skip(&g.sys) {
            continue;
        }
        let at = indexes.iter().position(|x| x.0 == g.sys).unwrap_or_else(|| {
            indexes.push((g.sys.clone(), cols.iter().filter_map(|col| Some((col, index(web, &cache_dir, col, now()).ok()?))).collect()));
            indexes.len() - 1
        });
        if indexes[at].1.is_empty() {
            continue;
        }
        let key = g.key();
        // chosen by hand: never swapped
        if stores.locked(&key) {
            continue;
        }
        let t = now();
        if let Some(seen) = memo.seen.get(&key) {
            let (err, src) = (js::truthy(seen.get("err")), js::truthy(seen.get("src")));
            let (v, age) = (seen.get("v").and_then(Value::as_u64), t - js::number(seen.get("at")));
            let this_fix = err && seen.get("ev").and_then(Value::as_u64) == Some(ERR_V);
            // failed lately: tomorrow; failed three times (a page that never comes down): not until an update
            if this_fix && (age < DAY || (seen.get("failV").and_then(Value::as_u64) == Some(VERSION) && js::number(seen.get("fails")) >= 3.0)) {
                continue;
            }
            // already holds the collection's copy; or nothing was there for it when last looked
            if (src && v.is_some_and(|v| v >= VERSION)) || (!src && v == Some(VERSION) && (err || seen.get("ix").and_then(Value::as_u64) == Some(INDEX_V)) && age < if err { DAY } else { INDEX_DAYS * DAY }) {
                continue;
            }
            // its own manual was kept over the collection's: not downloaded again to say so, for a month (the
            // Electron app does, every start)
            if !src && !err && v == Some(VERSION) && js::truthy(seen.get("kept")) && age < INDEX_DAYS * DAY {
                continue;
            }
        }
        // never a copy the player marked as the wrong manual
        let hit = indexes[at].1.iter().enumerate().find_map(|(c, (col, ix))| pick(&ix.entries, g).filter(|e| !stores.rejected(&key, &format!("coll:{}/{}", col.id, e.name))).map(|e| (c, e.clone())));
        let Some((c, e)) = hit else {
            memo.seen.insert(key, json!({ "v": VERSION, "at": js::num(t), "none": true, "ix": INDEX_V }));
            continue;
        };
        let has = !manual_files(&g.dir, &g.base).is_empty();
        let prio = if !has { 0 } else if memo.seen.get(&key).is_some_and(|s| js::truthy(s.get("src"))) { 2 } else { 1 };
        jobs.push((prio, g, at, c, e));
    }
    jobs.sort_by_key(|j| j.0);
    jobs.truncate(MAX_GAMES);
    let mut changes = Vec::new();
    for (_, g, at, c, e) in &jobs {
        if cancelled() {
            break;
        }
        let (col, ix) = &indexes[*at].1[*c];
        let (key, had) = (g.key(), inspect(&g.dir, &g.base).count > 0);
        let ours = memo.seen.get(&key).is_some_and(|s| js::truthy(s.get("src"))) && had;
        match one(web, g, &Hit { col, ix, e }, &mut memo, now(), false, cancelled) {
            Ok(true) if !ours => changes.push(Change { title: g.title.clone(), upgraded: had }),
            Ok(_) => {}
            Err(why) => {
                let seen = memo.seen.get(&key).cloned().unwrap_or(json!({}));
                let kept = js::truthy(seen.get("src"));
                let again = js::truthy(seen.get("err")) && seen.get("failV").and_then(Value::as_u64) == Some(VERSION) && seen.get("ev").and_then(Value::as_u64) == Some(ERR_V);
                let fails = if again { Some(js::number(seen.get("fails"))).filter(|f| *f != 0.0 && !f.is_nan()).unwrap_or(1.0) } else { 0.0 } + if why == "stopped" { 0.0 } else { 1.0 };
                let mut entry = if kept { seen.as_object().cloned().unwrap_or_default() } else { Map::new() };
                entry.insert("v".into(), if kept { seen["v"].clone() } else { json!(VERSION) });
                for (k, v) in [("at", js::num(now())), ("err", json!(why)), ("ev", json!(ERR_V)), ("fails", js::num(fails)), ("failV", json!(VERSION))] {
                    entry.insert(k.into(), v);
                }
                memo.seen.insert(key, Value::Object(entry));
            }
        }
        memo.save();
    }
    memo.save();
    changes
}

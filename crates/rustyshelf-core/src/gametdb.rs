// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/gametdb.js, as RustyShelf has it.
//! GameTDB (gametdb.com) for Switch and 3DS: which GameTDB id a game is, by its title. GameTDB keeps a full cover
//! scan (back | spine | front) for most boxed games, and the art check cuts a real spine and back out of it when
//! no other source has one. The database is fetched at most once a week; only the boxed games' ids, titles and
//! regions are kept (`gametdb-<console>.json` in the box art cache).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::lbindex::norm;

const WEEK_MS: u64 = 7 * 86_400_000;
const DAY_MS: u64 = 86_400_000;
/// the version of the kept file
const V: u64 = 1;

/// A console's database: its zip, its name in the art addresses, and the game types that are boxed games.
fn db(console: &str) -> Option<(&'static str, &'static str, &'static [&'static str])> {
    match console {
        "sw" => Some(("https://www.gametdb.com/switchtdb.zip", "switch", &["Switch"])),
        "n3ds" => Some(("https://www.gametdb.com/3dstdb.zip", "3ds", &["3DS", "New3DS"])),
        _ => None,
    }
}

pub fn has(console: &str) -> bool {
    db(console).is_some()
}

/// The art regions a GameTDB region's scans are filed under, best first.
fn regions_of(region: &str) -> &'static [&'static str] {
    match region {
        "NTSC-U" => &["US"],
        "PAL" => &["EN", "AU", "FR", "DE", "ES", "IT", "NL"],
        "NTSC-J" => &["JA"],
        "KOR" => &["KO"],
        _ => &["US", "EN"],
    }
}

fn u16_at(b: &[u8], o: usize) -> usize {
    b.get(o..o + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn u32_at(b: &[u8], o: usize) -> usize {
    b.get(o..o + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// The one .xml inside a zip (stored or deflated).
pub fn xml_of(zip: &[u8]) -> Option<String> {
    let last = zip.len().checked_sub(22)?;
    let end = (last.saturating_sub(65_535)..=last).rev().find(|&i| u32_at(zip, i) == 0x0605_4b50)?;
    let mut o = u32_at(zip, end + 16);
    for _ in 0..u16_at(zip, end + 10) {
        if o + 46 > zip.len() || u32_at(zip, o) != 0x0201_4b50 {
            break;
        }
        let (method, size, name_len, extra_len, comment_len, local) = (u16_at(zip, o + 10), u32_at(zip, o + 20), u16_at(zip, o + 28), u16_at(zip, o + 30), u16_at(zip, o + 32), u32_at(zip, o + 42));
        let name = String::from_utf8_lossy(zip.get(o + 46..o + 46 + name_len)?).to_lowercase();
        o += 46 + name_len + extra_len + comment_len;
        if !name.ends_with(".xml") || local + 30 > zip.len() {
            continue;
        }
        let start = local + 30 + u16_at(zip, local + 26) + u16_at(zip, local + 28);
        let raw = zip.get(start..(start + size).min(zip.len()))?;
        let bytes = match method {
            0 => raw.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, 256 << 20).ok()?,
            _ => Vec::new(),
        };
        return Some(String::from_utf8_lossy(&bytes).into_owned());
    }
    None
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let from = text.find(open)? + open.len();
    Some(&text[from..from + text[from..].find(close)?])
}

/// The boxed games of a database: by their English title as it's matched, each one's ids and regions; and how
/// many there are.
pub fn parse(xml: &str, types: &[&str]) -> (Map<String, Value>, usize) {
    let (mut map, mut n) = (Map::new(), 0);
    let mut rest = xml;
    while let Some(at) = rest.find("<game name=\"") {
        let Some(body_from) = rest[at..].find("\">").map(|i| at + i + 2) else { break };
        let Some(body_len) = rest[body_from..].find("</game>") else { break };
        let b = &rest[body_from..body_from + body_len];
        rest = &rest[body_from + body_len + 7..];
        if !types.contains(&between(b, "<type>", "</type>").unwrap_or("")) {
            continue;
        }
        let Some(id) = between(b, "<id>", "</id>").filter(|id| (4..=6).contains(&id.len()) && id.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())) else { continue };
        let region = between(b, "<region>", "</region>").filter(|r| !r.is_empty()).unwrap_or("ALL");
        let Some(title) = b.find("<locale lang=\"EN\">").and_then(|i| b[i + 18..].trim_start().strip_prefix("<title>")).and_then(|t| t.find("</title>").map(|end| &t[..end])).filter(|t| !t.contains('<')) else { continue };
        let title = title.replace("&amp;", "&").replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">");
        let key = norm(&title);
        if key.is_empty() {
            continue;
        }
        if let Some(list) = map.entry(key).or_insert_with(|| json!([])).as_array_mut() {
            list.push(json!([id, region]));
        }
        n += 1;
    }
    (map, n)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn kept_file(cache_dir: &Path, console: &str) -> PathBuf {
    cache_dir.join(format!("gametdb-{console}.json"))
}

/// A console's title map, fetched when it's missing or a week old (an old one still answers when the fetch
/// fails, and that's tried again a day later).
pub fn index(cache_dir: &Path, console: &str) -> Option<Map<String, Value>> {
    let (zip, _, types) = db(console)?;
    let file = kept_file(cache_dir, console);
    let kept = fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(|j| j["v"] == V && j["map"].is_object());
    let (now, map_of) = (now_ms(), |j: &Value| j["map"].as_object().cloned().unwrap_or_default());
    if let Some(j) = &kept {
        let fresh = |key: &str, age: u64| j[key].as_u64().is_some_and(|at| now.saturating_sub(at) < age);
        if fresh("at", WEEK_MS) || fresh("tried", DAY_MS) {
            return Some(map_of(j));
        }
    }
    let _ = fs::create_dir_all(cache_dir);
    let tmp = cache_dir.join(format!(".gametdb-{console}-{}.zip", std::process::id()));
    let fetched = (|| {
        if !matches!(crate::boxart::download_ex(zip, &tmp, true), Ok(crate::boxart::Outcome::Ok)) {
            return None;
        }
        let (map, n) = parse(&xml_of(&fs::read(&tmp).ok()?)?, types);
        (n >= 100).then_some(map)
    })();
    let _ = fs::remove_file(&tmp);
    match (fetched, kept) {
        (Some(map), _) => {
            let _ = fs::write(&file, json!({ "v": V, "at": now, "map": map }).to_string());
            Some(map)
        }
        (None, Some(mut old)) => {
            old["tried"] = json!(now);
            let _ = fs::write(&file, old.to_string());
            Some(map_of(&old))
        }
        (None, None) => {
            let _ = fs::write(&file, json!({ "v": V, "at": 0, "tried": now, "map": {} }).to_string());
            Some(Map::new())
        }
    }
}

/// A full cover scan's address, with the game's id there, the art region it's filed under and which scan it is
/// ("coverfullHQ" or "coverfull").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Full {
    pub url: String,
    pub id: String,
    pub region: String,
    pub kind: &'static str,
}

/// The full-cover addresses for a game, best first, eight at most. `names`: the game's names, matched exactly;
/// `prefer`: the art regions chosen in Settings, first where that release exists.
pub fn full_covers(map: &Map<String, Value>, console: &str, names: &[String], prefer: &[&str]) -> Vec<Full> {
    let Some((_, art, _)) = db(console) else { return Vec::new() };
    let hits = names.iter().find_map(|t| map.get(&norm(t)).and_then(Value::as_array).filter(|h| !h.is_empty()));
    let mut out: Vec<Full> = Vec::new();
    let mut seen: HashMap<String, ()> = HashMap::new();
    for hit in hits.map(Vec::as_slice).unwrap_or_default() {
        let (id, region) = (hit[0].as_str().unwrap_or(""), hit[1].as_str().unwrap_or("ALL"));
        let own = regions_of(region);
        for r in prefer.iter().filter(|p| own.contains(p)).chain(own.iter()) {
            for kind in ["coverfullHQ", "coverfull"] {
                let url = format!("https://art.gametdb.com/{art}/{kind}/{r}/{id}.jpg");
                if seen.insert(url.clone(), ()).is_none() {
                    out.push(Full { url, id: id.to_string(), region: r.to_string(), kind });
                }
            }
        }
    }
    out.truncate(8);
    out
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/wikiart.js, as RustyShelf has it.
//! Box art from the Super Mario Wiki (mariowiki.com, a MediaWiki): official box scans of Nintendo's own games
//! for every region, uploaded as "<Game> NA box art.jpg", "<Game> Box EUR.png", "<Game> EUR back.jpg"… It's
//! asked only when no other source had the part, on the Switch 2 shelf: its boxes are there months before the
//! art databases have them. Found through the wiki's own search, matched by the file name starting with the
//! game's name; answers are kept for a week (`boxart-cache/wikiart.json`).

use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::js;
use crate::lbindex::norm;

const API: &str = "https://www.mariowiki.com/api.php";
/// answers are kept this long (ms)
const TTL: f64 = 7.0 * 86_400_000.0;
// (main/wikiart.js CACHE_V: keep in sync, both apps write the same file)
const CACHE_V: u64 = 2;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// Switch 2 only: a file name can't say which console its box is for, and older consoles share titles with
/// their re-releases.
pub fn has(sys: &str) -> bool {
    sys == "sw2"
}

/// A picture the wiki has.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub url: String,
    pub name: String,
    /// "png", "jpg" or "webp"
    pub ext: String,
}

/// An image the wiki serves from its own hosts (its type, when the answer leaves it out, from its name).
fn usable(f: &Value) -> bool {
    static TYPE: LazyLock<Regex> = LazyLock::new(|| re(r"^image/(jpeg|png|webp)$"));
    static NAMED: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(jpe?g|png|webp)$"));
    static HOST: LazyLock<Regex> = LazyLock::new(|| re(r#"^(?i)https://([a-z0-9-]+\.)*(mariowiki\.com|mario\.wiki\.gallery)/[^\s"<>]+$"#));
    let url = f["url"].as_str().unwrap_or("");
    let picture = match f["mime"].as_str().filter(|m| !m.is_empty()) {
        Some(mime) => TYPE.is_match(mime),
        None => NAMED.is_match(url),
    };
    picture && HOST.is_match(url)
}

/// How much a file's region words are wanted: North America first, then English PAL, then the rest.
pub fn region_score(name: &str, want: &str) -> i32 {
    static US: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(na|us|usa|ca|canada|north america)\b"));
    static EN: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(uk|usk|eur|europe|au|aus|australia|pal|nz)\b"));
    static JP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\b(jp|jpn|japan)\b"));
    let (us, en, jp) = (US.is_match(name), EN.is_match(name), JP.is_match(name));
    match want {
        "jpn" => if jp { 3 } else if us { 2 } else if en { 1 } else { 0 },
        "eur" => if en { 3 } else if us { 2 } else if jp { 0 } else { 1 },
        _ => if us { 3 } else if en { 2 } else if jp { 0 } else { 1 },
    }
}

/// words a box scan's file name may carry after the game's name: region codes and box words
const ALLOW: &str = "na us usa ca can canada north america uk usk eur eu europe european au aus australia nz pal jp jpn japan japanese kr kor korea \
south korean tw taiwan hk hong kong sg singapore th thailand za africa sa sau saudi arabia ae uae br brazil mx mex mexico de germany fr france \
es spain it italy nl netherlands dutch pt portugal se sweden dk no fi ru russia cn chn china asia latam sca scandinavia nordic \
box boxart art cover front back final release standard physical version packaging alt alternate nsw ns2 switch nintendo";

/// The wiki's own search, asked of the web: the answer's JSON, or why not.
pub fn ask(words: &str) -> Result<Value, String> {
    let agent = format!("RustyShelf/{} (box art for a personal game launcher)", env!("CARGO_PKG_VERSION"));
    let query = js::query(&[
        ("action", "query".into()),
        ("format", "json".into()),
        ("generator", "search".into()),
        ("gsrnamespace", "6".into()),
        ("gsrlimit", "50".into()),
        ("gsrsearch", format!("intitle:\"{}\"", words.replace('"', ""))),
        ("prop", "imageinfo".into()),
        ("iiprop", "url|size|mime".into()),
    ]);
    let (status, _, body) = crate::net::ask(&format!("{API}?{query}"), &[("User-Agent", agent.as_str()), ("Accept", "application/json")], Duration::from_secs(10), 16 << 20).map_err(|_| "the Mario Wiki couldn't be reached".to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("Mario Wiki answered {status}"));
    }
    serde_json::from_slice(&body).map_err(|_| "the Mario Wiki's answer couldn't be read".to_string())
}

/// Every image file whose name has these words, with its address and size: from the week's answers, else asked.
fn search(cache_dir: &Path, words: &str, now: f64, ask: &dyn Fn(&str) -> Result<Value, String>) -> Result<Vec<Value>, String> {
    // one request at a time, and one writer of the file
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = cache_dir.join("wikiart.json");
    let mut cache = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(|j| j["v"].as_u64() == Some(CACHE_V) && j["q"].is_object()).unwrap_or_else(|| json!({ "v": CACHE_V, "q": {} }));
    let key = norm(words);
    if let Some(hit) = cache["q"].get(&key).filter(|h| now - js::number(h.get(0)) < TTL) {
        return Ok(hit[1].as_array().cloned().unwrap_or_default());
    }
    let answer = ask(words)?;
    let list: Vec<Value> = answer["query"]["pages"]
        .as_object()
        .map(js::entries)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(_, p)| {
            let info = p["imageinfo"].get(0)?;
            let url = info["url"].as_str()?;
            let title = js::string(p.get("title").filter(|t| js::truthy(Some(t))).or(Some(&json!(""))));
            let number = |v: &Value| Some(js::number(Some(v))).filter(|n| !n.is_nan()).map_or(json!(0), js::num);
            Some(json!({ "name": js::slice(title.strip_prefix("File:").unwrap_or(&title), 300), "url": js::slice(url, 600), "mime": info["mime"].as_str().unwrap_or(""), "w": number(&info["width"]), "h": number(&info["height"]) }))
        })
        .collect();
    // the whole answer is kept: what's usable is decided when it's read
    let entries = cache["q"].as_object_mut().expect("the answers kept");
    entries.retain(|_, v| now - js::number(v.get(0)) <= TTL);
    entries.insert(key, json!([js::num(now), list]));
    if std::fs::create_dir_all(cache_dir).is_ok() {
        let _ = std::fs::write(&file, cache.to_string());
    }
    Ok(list)
}

/// The best file for a game's front or back. `names`: its names, the most specific first; the file's name must
/// start with one of them. `region`: "usa", "eur", "jpn" or "".
pub fn find(cache_dir: &Path, sys: &str, names: &[String], part: &str, region: &str, now: f64, ask: &dyn Fn(&str) -> Result<Value, String>) -> Option<Found> {
    static TYPE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.[a-z]+$"));
    static BOX: LazyLock<Regex> = LazyLock::new(|| re(r"\b(box|boxart|cover|packaging)\b"));
    static BACK: LazyLock<Regex> = LazyLock::new(|| re(r"\bback\b"));
    static NOT_A_BOX: LazyLock<Regex> = LazyLock::new(|| re(r"\b(pre ?release|prerelease|prototype|mock ?up|concept|sticker|standee|wallpaper|logo|icon|guide)\b"));
    static NUMBER: LazyLock<Regex> = LazyLock::new(|| re(r"^([0-9]+|ii|iii|iv)$"));
    static PICTURE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\.(png|jpe?g|webp)$"));
    if !has(sys) || !matches!(part, "front" | "back") {
        return None;
    }
    let allow: std::collections::HashSet<&str> = ALLOW.split(' ').collect();
    // (Switch 2: its own name only; without "Nintendo Switch 2 Edition" it would find the Switch game's box)
    for name in names.iter().take(if sys == "sw2" { 1 } else { 2 }) {
        let want = norm(name);
        // one short word finds every file
        if want.split(' ').count() < 2 && want.chars().count() < 6 {
            continue;
        }
        let list = search(cache_dir, name, now, ask).ok()?;
        let size = |f: &Value, k: &str| js::number(f.get(k));
        let mut ok: Vec<&Value> = list
            .iter()
            .filter(|f| usable(f))
            .filter(|f| {
                let file_name = f["name"].as_str().unwrap_or("");
                let n = norm(&TYPE.replace(file_name, ""));
                let Some(rest) = n.strip_prefix(&want) else { return false };
                if (!BOX.is_match(&n) && !BACK.is_match(rest)) || NOT_A_BOX.is_match(&n) {
                    return false;
                }
                // after the name only region codes and box words; other words only after a "+" (a bundle's
                // add-on), and never a number (another game of the series)
                let extra: Vec<&str> = rest.split(' ').filter(|w| !w.is_empty() && !allow.contains(w)).collect();
                if extra.iter().any(|w| NUMBER.is_match(w)) || (!extra.is_empty() && !file_name.contains('+')) {
                    return false;
                }
                // a box is portrait, and big enough to use
                (part == "back") == BACK.is_match(rest) && size(f, "h") >= 300.0 && size(f, "h") >= size(f, "w") * 1.1
            })
            .collect();
        if ok.is_empty() {
            continue;
        }
        ok.sort_by(|a, b| {
            let (na, nb) = (a["name"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""));
            region_score(nb, region).cmp(&region_score(na, region)).then(size(b, "h").total_cmp(&size(a, "h")))
        });
        let url = ok[0]["url"].as_str().unwrap_or("").to_string();
        let ext = PICTURE.captures(&url).map_or("jpg".to_string(), |m| m[1].to_lowercase().replace("jpeg", "jpg"));
        return Some(Found { name: ok[0]["name"].as_str().unwrap_or("").to_string(), url, ext });
    }
    None
}

/// The answers kept, for a test to fill.
pub fn keep(cache_dir: &Path, answers: &Map<String, Value>) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(cache_dir.join("wikiart.json"), json!({ "v": CACHE_V, "q": answers }).to_string())
}

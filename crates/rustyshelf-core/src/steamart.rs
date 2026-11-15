// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/steamart.js and main/imgcheck.js, as RustyShelf has them.
//! Steam art and game info. Steam only keeps a portrait cover in its librarycache for games looked at in its library
//! grid (and some older games never get one), so the gaps are filled from Steam's own public store CDN: no account
//! or key. Release date, developer, publisher, genres, Metacritic score and screenshots come from the public store
//! API, kept for a month in `data/steam-info.json`. The covers go to `data/steam-art`; the Electron app shares both.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::js;
use crate::paths::is_file;

const MONTH: f64 = 30.0 * 86_400_000.0;
/// A cover the CDN doesn't have, or info the store doesn't give, is asked for again after a week.
const RETRY: f64 = 7.0 * 86_400_000.0;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Is this download a picture? PNG, JPEG, WebP, GIF, BMP or AVIF / HEIF, by its first bytes. A 200 that isn't one (a
/// Wi-Fi sign-in page, a CDN error page) is never saved as a cover: a saved picture is trusted for good.
pub fn is_image(b: &[u8]) -> bool {
    b.len() >= 12 && (b.starts_with(&[0x89, 0x50]) || b.starts_with(&[0xff, 0xd8]) || (b.starts_with(b"RIFF") && &b[8..12] == b"WEBP") || b.starts_with(b"GIF") || b.starts_with(b"BM") || &b[4..8] == b"ftyp")
}

fn asset(id: &str, name: &str) -> [String; 2] {
    [format!("https://shared.cloudflare.steamstatic.com/store_item_assets/steam/apps/{id}/{name}"), format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{id}/{name}")]
}

enum Saved {
    Ok,
    /// really not on the CDN: worth remembering
    Missing,
    /// offline and the like: tried again next time, nothing remembered
    Error,
}

fn save(urls: &[String], dest: &Path) -> Saved {
    let mut err = false;
    for u in urls {
        match crate::net::get_bytes(u, Duration::from_secs(20), 64 << 20) {
            // a stub counts as "not here"
            Ok(Some(b)) if b.len() > 2048 => {
                if !is_image(&b) {
                    err = true;
                    continue;
                }
                let part = PathBuf::from(format!("{}.part", dest.display()));
                let written = dest.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&part, &b)).and_then(|()| std::fs::rename(&part, dest));
                match written {
                    Ok(()) => return Saved::Ok,
                    Err(_) => {
                        let _ = std::fs::remove_file(&part);
                        err = true;
                    }
                }
            }
            Ok(_) => {}
            Err(_) => err = true,
        }
    }
    if err {
        Saved::Error
    } else {
        Saved::Missing
    }
}

/// A release date as the store writes it ("21 Nov, 2023", "Nov 21, 2023", "Nov 2023", "2023") as YYYY-MM-DD; else the
/// year in it, else nothing ("Coming soon").
// The Electron app reads the date in local time and writes it in UTC, a day early east of Greenwich; the day the
// store gives is kept here.
fn release_date(rel: &str) -> (String, String) {
    static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new("([0-9]{4})").unwrap());
    static DMY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]{1,2})\s+([A-Za-z]{3,})\.?,?\s+([0-9]{4})$").unwrap());
    static MDY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z]{3,})\.?\s+([0-9]{1,2}),?\s+([0-9]{4})$").unwrap());
    static MY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z]{3,})\.?,?\s+([0-9]{4})$").unwrap());
    let year = YEAR.captures(rel).map(|m| m[1].to_string()).unwrap_or_default();
    let month = |name: &str| ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"].iter().position(|m| name.to_lowercase().starts_with(m)).map(|i| i + 1);
    let t = js::trim(rel);
    let parts = if let Some(m) = DMY.captures(t) {
        month(&m[2]).map(|mo| (m[3].to_string(), mo, m[1].parse::<u32>().unwrap_or(0)))
    } else if let Some(m) = MDY.captures(t) {
        month(&m[1]).map(|mo| (m[3].to_string(), mo, m[2].parse().unwrap_or(0)))
    } else if let Some(m) = MY.captures(t) {
        month(&m[1]).map(|mo| (m[2].to_string(), mo, 1))
    } else if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
        Some((t.to_string(), 1, 1))
    } else {
        None
    };
    match parts.filter(|(_, _, d)| (1..=31).contains(d)) {
        Some((y, m, d)) => (format!("{y}-{m:02}-{d:02}"), year),
        None => (year.clone(), year),
    }
}

/// The store's page for an app as the entry kept for it; None when the store couldn't be asked.
/// Newer games keep their art under a hashed folder (steam/apps/<id>/<hash>/library_capsule.jpg): the plain
/// addresses answer 404 for them. Steam's public store API names the real files: the addresses of the first of `keys`
/// it has.
fn store_assets(id: &str, keys: &[&str]) -> Vec<String> {
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w./-]+$").unwrap());
    let Ok(n) = id.parse::<u64>() else { return Vec::new() };
    let q = js::encode_uri_component(&json!({ "ids": [{ "appid": n }], "data_request": { "include_assets": true } }).to_string());
    let Ok(j) = crate::net::get_json(&format!("https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={q}"), Duration::from_secs(15)) else { return Vec::new() };
    let a = &j["response"]["store_items"][0]["assets"];
    keys.iter().filter_map(|k| a[*k].as_str()).filter(|n| NAME.is_match(n)).flat_map(|n| asset(id, n)).collect()
}

/// The store's HTML as text, a paragraph, heading or list item a line. Keep in sync with main/steamart.js plain.
fn plain(html: &str) -> String {
    static BLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</(p|li|h\d|ul|ol)>|<br\s*/?>").unwrap());
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new("<[^>]+>").unwrap());
    static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\n\s*").unwrap());
    let s = TAG.replace_all(&BLOCK.replace_all(html, "\n"), "").replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");
    LINE.replace_all(&SPACE.replace_all(&s, " "), "\n").trim().to_string()
}

fn fetch_info(appid: &str) -> Option<Value> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new("<[^>]+>").unwrap());
    let j = crate::net::get_json(&format!("https://store.steampowered.com/api/appdetails?appids={appid}&l=english"), Duration::from_secs(20)).ok()?;
    let app = j.get(appid);
    let Some(d) = app.filter(|a| js::truthy(a.get("success"))).and_then(|a| a.get("data")).filter(|d| js::truthy(Some(d))) else {
        return Some(json!({ "ok": false, "at": now() }));
    };
    let (iso, year) = release_date(&js::text(d.get("release_date").and_then(|r| r.get("date"))));
    let joined = |k: &str, sep: &str, of: &dyn Fn(&Value) -> String| d.get(k).and_then(Value::as_array).map(|a| a.iter().map(of).collect::<Vec<_>>().join(sep)).unwrap_or_default();
    let text = |v: &Value| js::string(Some(v));
    let overview = TAG.replace_all(&js::text(d.get("short_description")), "").replace("&amp;", "&").replace("&quot;", "\"").replace("&#39;", "'");
    let about = plain(&js::text(d.get("about_the_game")));
    let shots: Vec<Value> = d
        .get("screenshots")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|s| s.get("path_full").and_then(Value::as_str))
        .filter(|u| u.starts_with("https://"))
        .map(Value::from)
        .collect();
    let mut e = Map::new();
    e.insert("ok".into(), json!(true));
    e.insert("at".into(), json!(now()));
    if let Some(name) = d.get("name") {
        e.insert("name".into(), name.clone());
    }
    let fields = [
        ("release", iso),
        ("year", year),
        ("dev", joined("developers", ", ", &text)),
        ("pub", joined("publishers", ", ", &text)),
        ("genres", joined("genres", "; ", &|g| js::string(g.get("description")))),
    ];
    for (k, v) in fields.into_iter().filter(|(_, v)| !v.is_empty()) {
        e.insert(k.into(), json!(v));
    }
    if let Some(score) = d.get("metacritic").and_then(|m| m.get("score")).filter(|s| js::truthy(Some(s))) {
        e.insert("metacritic".into(), score.clone());
    }
    if !overview.is_empty() {
        e.insert("overview".into(), json!(js::slice(&overview, 1200)));
    }
    if !about.is_empty() {
        e.insert("about".into(), json!(js::slice(&about, 2000)));
    }
    e.insert("v".into(), json!(2));
    e.insert("shots".into(), Value::Array(shots));
    e.insert("bg".into(), js::str_of(d.get("background_raw")).filter(|u| u.starts_with("https://")).map_or(Value::Null, Value::from));
    Some(Value::Object(e))
}

/// Where a Steam game's made back is kept.
pub fn back_file(data_dir: &Path, appid: &str) -> PathBuf {
    data_dir.join("steam-art").join(format!("{appid}_backmade.jpg"))
}

/// Backs for these Steam games (app id, title, cover) from their store screenshots, artwork and description, for the
/// ones with store info and no made back yet (Steam has no box back to download). How many were made; it stops at
/// the first game none of whose screenshots came (offline).
pub fn make_backs(data_dir: &Path, games: &[(String, String, PathBuf)]) -> u32 {
    let infos = crate::info::steam_infos(data_dir);
    let fetch = |u: &str| crate::net::get_bytes(u, Duration::from_secs(10), 15_000_000).ok().flatten().and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8());
    let band = crate::looks::Band { bg: [0x11 as f32 / 255.0, 0x14 as f32 / 255.0, 0x1a as f32 / 255.0, 1.0], fg: [0x8f as f32 / 255.0, 0xb4 as f32 / 255.0, 0xd9 as f32 / 255.0, 1.0], label: "PC".into() };
    let mut made = 0;
    for (appid, title, front) in games {
        let dest = back_file(data_dir, appid);
        let Some(info) = infos.get(appid).filter(|i| !i.shots.is_empty()) else { continue };
        if is_file(&dest) {
            continue;
        }
        let Some(cover) = image::open(front).ok().map(|i| i.to_rgba8()) else { continue };
        let shots: Vec<_> = info.shots.iter().take(3).filter_map(|u| fetch(u)).collect();
        if shots.is_empty() {
            break;
        }
        let art = info.bg.as_deref().and_then(fetch);
        let (about, overview) = (info.about.as_deref().unwrap_or(""), info.overview.as_deref().unwrap_or(""));
        let text = crate::drawn::BackText {
            title,
            overview: if about.chars().count() > overview.chars().count() { about } else { overview },
            esrb: "",
            players: "",
            publisher: info.publisher.as_deref().or(info.dev.as_deref()).unwrap_or(""),
            year: info.year.as_deref().unwrap_or(""),
        };
        let Some(p) = crate::drawn::made_back(&cover, &shots, art.as_ref(), &band, &text) else { continue };
        let Some(img) = image::RgbaImage::from_raw(p.width, p.height, p.rgba) else { continue };
        let mut jpg = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90);
        if image::DynamicImage::ImageRgba8(img).to_rgb8().write_with_encoder(enc).is_ok() && std::fs::create_dir_all(dest.parent().unwrap_or(data_dir)).and_then(|()| std::fs::write(&dest, &jpg)).is_ok() {
            made += 1;
        }
    }
    made
}

/// A Steam game on the shelf, and what it already has.
pub struct Wanted {
    pub appid: String,
    pub has_front: bool,
    pub has_hero: bool,
}

/// The apps already looked at this session.
static TRIED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// Fills in covers, heroes and store info for these Steam games. Returns the new covers, and the heroes and info
/// entries that are new.
pub fn fill(data_dir: &Path, games: &[Wanted]) -> (u32, u32) {
    let dir = data_dir.join("steam-art");
    let info_file = data_dir.join("steam-info.json");
    let mut info = match std::fs::read(&info_file).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    };
    let need: Vec<&Wanted> = {
        let mut tried = TRIED.lock().unwrap();
        games.iter().filter(|g| tried.insert(g.appid.clone())).collect()
    };
    // (".nocover": a ".missing" mark was set before the store API was asked, and isn't trusted)
    let missed = |name: &str| dir.join(format!("{name}.nocover"));
    let recently_missed = |name: &str| {
        let at = missed(name).metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok());
        at.is_some_and(|t| (now() as f64 - t.as_millis() as f64) < RETRY)
    };
    let remember = |name: &str| {
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(missed(name), b""));
    };
    let (mut covers, mut other) = (0, 0);
    for g in &need {
        let id = g.appid.as_str();
        let (cover, hero) = (dir.join(format!("{id}_cover.jpg")), dir.join(format!("{id}_hero.jpg")));
        if !g.has_front && !is_file(&cover) && !recently_missed(id) {
            let urls = [asset(id, "library_600x900_2x.jpg"), asset(id, "library_600x900.jpg")].concat();
            let mut got = save(&urls, &cover);
            if matches!(got, Saved::Missing) {
                let more = store_assets(id, &["library_capsule_2x", "library_capsule"]);
                if !more.is_empty() {
                    got = save(&more, &cover);
                }
            }
            match got {
                Saved::Ok => covers += 1,
                Saved::Missing => remember(id),
                // offline: nothing is marked, the CDN isn't hammered
                Saved::Error => break,
            }
        }
        let hero_mark = format!("{id}_hero");
        if !g.has_hero && !is_file(&hero) && !recently_missed(&hero_mark) {
            let mut got = save(&asset(id, "library_hero.jpg"), &hero);
            if matches!(got, Saved::Missing) {
                let more = store_assets(id, &["library_hero_2x", "library_hero"]);
                if !more.is_empty() {
                    got = save(&more, &hero);
                }
            }
            match got {
                Saved::Ok => other += 1,
                Saved::Missing => remember(&hero_mark),
                Saved::Error => break,
            }
        }
        let due = match info.get(id) {
            // (before v2 an entry had no long description)
            Some(old) => {
                let ok = js::truthy(old.get("ok"));
                now() as f64 - js::number(old.get("at")) > (if ok { MONTH } else { RETRY }) || (ok && old.get("v").is_none())
            }
            None => true,
        };
        if due {
            // a refresh that fails leaves the old entry, which isn't a change
            if let Some(entry) = fetch_info(id) {
                if js::truthy(entry.get("ok")) {
                    other += 1;
                }
                info.insert(id.to_string(), entry);
            }
            // the store API rate-limits bursts
            std::thread::sleep(Duration::from_millis(350));
        }
    }
    if !need.is_empty() {
        let tmp = data_dir.join("steam-info.json.tmp");
        if std::fs::create_dir_all(data_dir).and_then(|()| std::fs::write(&tmp, Value::Object(info).to_string())).is_ok() {
            let _ = std::fs::rename(&tmp, &info_file);
        }
    }
    (covers, other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_pictures() {
        let d = |s: &str| release_date(s);
        assert_eq!(d("21 Nov, 2023"), ("2023-11-21".into(), "2023".into()));
        assert_eq!(d("Nov 21, 2023"), ("2023-11-21".into(), "2023".into()));
        assert_eq!(d("Nov 2023"), ("2023-11-01".into(), "2023".into()));
        assert_eq!(d("2 mars 2020"), ("2020-03-02".into(), "2020".into()));
        assert_eq!(d("2023"), ("2023-01-01".into(), "2023".into()));
        assert_eq!(d("Q1 2024"), ("2024".into(), "2024".into()));
        assert_eq!(d("Coming soon"), (String::new(), String::new()));
        assert!(is_image(b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01") && is_image(b"RIFF\x00\x00\x00\x00WEBP") && !is_image(b"<!doctype html>") && !is_image(b"\xff\xd8"));
    }
}

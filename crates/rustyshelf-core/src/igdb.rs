// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/igdb.js.
//! IGDB: the game info LaunchBox's database is missing (description, genres, developer, publisher, release date,
//! score), and its screenshots' addresses. Needs a Twitch developer app's Client ID and Secret (`keys.igdbId`,
//! `keys.igdbSecret`); the app token Twitch gives for them is kept in memory only, and neither it nor the secret is
//! ever logged or put in an address. IGDB allows 4 requests a second: they go one at a time, `GAP` apart.
//! The info found is kept in `<data>\igdb-info\index.json` (`{ v: 1, games: { key: { at, i } } }`, the Electron app's
//! file) and asked for again after `RETRY`. It only ever fills gaps: a value LaunchBox has is never replaced.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

use crate::info::Info;
use crate::js;

/// ms between two requests
const GAP: u64 = 300;
/// a game asked about twice within this long shares one answer (ms)
const MEMO: f64 = 30.0 * 60e3;
/// a game's info is asked for again after this long (ms)
pub const RETRY: f64 = 30.0 * 864e5;
/// keys Twitch turned down aren't tried again for this long (ms), unless they change
const REFUSED: f64 = 10.0 * 60e3;
const FIELDS: &str = "name,summary,first_release_date,total_rating,genres.name,involved_companies.company.name,involved_companies.developer,involved_companies.publisher,franchise.name,franchises.name,screenshots.image_id";

/// IGDB's platform for a console (the search is narrowed to it).
pub fn platform(sys: &str) -> Option<u32> {
    Some(match sys {
        "gc" => 21,
        "wii" => 5,
        "wiiu" => 41,
        "sw" => 130,
        "psx" => 7,
        "ps2" => 8,
        "ps3" => 9,
        "ps4" => 48,
        "psp" => 38,
        "psvita" => 46,
        "n64" => 4,
        "snes" => 19,
        "nes" => 18,
        "gba" => 24,
        "gb" => 33,
        "gbc" => 22,
        "nds" => 20,
        "n3ds" => 37,
        "megadrive" => 29,
        "saturn" => 32,
        "dreamcast" => 23,
        "mastersystem" => 64,
        "gamegear" => 35,
        "pcengine" => 86,
        "x360" => 12,
        "xbox" => 11,
        "segacd" => 78,
        "sega32x" => 30,
        "atari2600" => 59,
        "atarilynx" => 61,
        "wonderswan" => 57,
        "neogeo" => 80,
        "dos" => 13,
        _ => return None,
    })
}

/// A game's title as it's searched for: without its bracketed parts and quotes.
fn title_of(title: &str) -> String {
    static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[(\[].*?[)\]]").unwrap());
    BRACKETS.replace_all(title, "").replace(['"', '\\'], "").trim().to_string()
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// How a request goes out: the address, its headers and body → the status and the answer.
pub type Fetch = Box<dyn Fn(&str, &[(&str, &str)], &str) -> Result<(u16, Vec<u8>), String> + Send + Sync>;

fn over_the_net() -> Fetch {
    Box::new(|url, headers, body| crate::net::post(url, headers, body, Duration::from_secs(15), 4 << 20))
}

/// Every client shares IGDB's limit: when the last request went.
static LAST: Mutex<u64> = Mutex::new(0);

#[derive(Default)]
struct State {
    /// the token, the keys it's for, and until when (ms)
    token: Option<(String, String, f64)>,
    /// the keys turned down, and when
    refused: Option<(String, f64)>,
    /// recent answers by console and title, and when
    memo: HashMap<String, (f64, Option<Found>)>,
}

/// What a search found: the best match, and the best match that has screenshots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub hit: Option<Value>,
    pub shot_hit: Option<Value>,
}

/// The client: the token, the queue, one lookup per game.
pub struct Igdb {
    id: String,
    secret: String,
    state: Mutex<State>,
    fetch: Fetch,
}

/// What went wrong: `refused` when Twitch or IGDB turned the keys down (asked again only with new keys), else busy.
#[derive(Debug, Clone, PartialEq)]
pub struct Trouble {
    pub refused: bool,
    pub why: String,
}

fn busy(why: &str) -> Trouble {
    Trouble { refused: false, why: why.into() }
}

fn refused() -> Trouble {
    Trouble { refused: true, why: "IGDB: Twitch didn't accept the Client ID and Secret".into() }
}

impl Igdb {
    pub fn new(id: &str, secret: &str) -> Igdb {
        Igdb::with(id, secret, over_the_net())
    }

    /// The same, asking through `fetch` (the tests' stand-in for the sites).
    pub fn with(id: &str, secret: &str, fetch: Fetch) -> Igdb {
        Igdb { id: id.trim().into(), secret: secret.trim().into(), state: Mutex::default(), fetch }
    }

    /// With the keys kept in Settings (`data/config.json`); None when there are none.
    pub fn from_settings(data_dir: &Path) -> Option<Igdb> {
        let cfg = crate::config::Config::load(data_dir).ok()?;
        let seal = crate::seal::key(&crate::paths::env_var);
        let c = Igdb::new(&cfg.api_key("igdbId", seal.as_ref()), &cfg.api_key("igdbSecret", seal.as_ref()));
        c.ready().then_some(c)
    }

    pub fn ready(&self) -> bool {
        !self.id.is_empty() && !self.secret.is_empty()
    }

    fn pair(&self) -> String {
        Sha1::digest(format!("{}\n{}", self.id, self.secret).as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The keys were turned down a moment ago.
    pub fn is_refused(&self) -> bool {
        self.state.lock().unwrap().refused.as_ref().is_some_and(|(pair, at)| now_ms() - at < REFUSED && *pair == self.pair())
    }

    fn refuse(&self) -> Trouble {
        let mut s = self.state.lock().unwrap();
        (s.token, s.refused) = (None, Some((self.pair(), now_ms())));
        refused()
    }

    fn token(&self, fresh: bool) -> Result<String, Trouble> {
        let pair = self.pair();
        if let Some((t, _, _)) = self.state.lock().unwrap().token.as_ref().filter(|(_, p, until)| !fresh && *p == pair && now_ms() < *until) {
            return Ok(t.clone());
        }
        if self.is_refused() {
            return Err(refused());
        }
        let body = js::query(&[("client_id", self.id.clone()), ("client_secret", self.secret.clone()), ("grant_type", "client_credentials".into())]);
        let (status, b) = (self.fetch)("https://id.twitch.tv/oauth2/token", &[("Content-Type", "application/x-www-form-urlencoded")], &body).map_err(|e| busy(&e))?;
        if status >= 500 || status == 429 {
            return Err(busy("Twitch busy"));
        }
        if !(200..300).contains(&status) {
            return Err(self.refuse());
        }
        let j: Value = serde_json::from_slice(&b).unwrap_or_default();
        let tok = j["access_token"].as_str().filter(|t| !t.is_empty()).ok_or_else(|| busy("Twitch gave no token"))?.to_string();
        let life = j["expires_in"].as_f64().filter(|s| *s > 0.0).unwrap_or(3600.0);
        let mut s = self.state.lock().unwrap();
        (s.token, s.refused) = (Some((tok.clone(), pair, now_ms() + (life - 300.0).max(60.0) * 1000.0)), None);
        Ok(tok)
    }

    /// IGDB's answer (a list) to a query on one of its endpoints.
    pub fn query(&self, endpoint: &str, body: &str) -> Result<Vec<Value>, Trouble> {
        if self.is_refused() {
            return Err(refused());
        }
        // one request at a time, at least GAP apart
        let mut last = LAST.lock().unwrap();
        let wait = (*last + GAP).saturating_sub(now_ms() as u64);
        if wait > 0 {
            std::thread::sleep(Duration::from_millis(wait));
        }
        let asked = (|| {
            let mut answer = (0, Vec::new());
            for i in 0..2 {
                let tok = self.token(i > 0)?;
                let auth = format!("Bearer {tok}");
                answer = (self.fetch)(&format!("https://api.igdb.com/v4/{endpoint}"), &[("Client-ID", &self.id), ("Authorization", &auth), ("Accept", "application/json")], body).map_err(|e| busy(&e))?;
                if answer.0 != 401 {
                    break;
                }
            }
            Ok(answer)
        })();
        *last = now_ms() as u64;
        drop(last);
        let (status, b) = asked?;
        if status == 429 || status >= 500 {
            return Err(busy("IGDB busy"));
        }
        if status == 401 || status == 403 {
            return Err(self.refuse());
        }
        if !(200..300).contains(&status) {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_slice::<Value>(&b).ok().and_then(|v| v.as_array().cloned()).unwrap_or_default())
    }

    /// A game on a console → the best match and the best match with screenshots; None without keys or a title.
    pub fn lookup(&self, sys: &str, title: &str) -> Result<Option<Found>, Trouble> {
        let t = title_of(title);
        if t.is_empty() || !self.ready() {
            return Ok(None);
        }
        let (now, key) = (now_ms(), format!("{sys}|{}", t.to_lowercase()));
        {
            let mut s = self.state.lock().unwrap();
            s.memo.retain(|_, (at, _)| now - *at < MEMO);
            if let Some((_, found)) = s.memo.get(&key) {
                return Ok(found.clone());
            }
        }
        let q = format!("search \"{t}\"; fields {FIELDS}; {}limit 15;", platform(sys).map_or(String::new(), |p| format!("where platforms = ({p}); ")));
        let list = self.query("games", &q)?;
        let named: Vec<&Value> = list.iter().filter(|x| x["name"].as_str().is_some_and(|n| !n.is_empty())).collect();
        let pick = |of: Vec<&Value>| -> Option<Value> {
            let names: Vec<String> = of.iter().map(|x| crate::lbindex::norm(x["name"].as_str().unwrap_or(""))).collect();
            crate::covers::match_title(&names.iter().map(String::as_str).collect::<Vec<_>>(), title, &|_| 1, false).map(|i| of[i].clone())
        };
        let found = Found { hit: pick(named.clone()), shot_hit: pick(named.into_iter().filter(|x| x["screenshots"].as_array().is_some_and(|s| !s.is_empty())).collect()) };
        self.state.lock().unwrap().memo.insert(key, (now, Some(found.clone())));
        Ok(Some(found))
    }

    /// Whether Twitch gives a token for these keys: for Accounts' check.
    pub fn test(&self) -> Result<(), String> {
        if !self.ready() {
            return Err("Add both the Client ID and the Client Secret first.".into());
        }
        self.token(true).map(|_| ()).map_err(|t| if t.refused { "Twitch didn't accept the Client ID and Secret".into() } else { format!("Couldn't reach Twitch: {}", t.why) })
    }
}

/// A match's screenshots, as addresses of their large size.
pub fn shots_of(hit: &Value) -> Vec<String> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-zA-Z0-9]+$").unwrap());
    hit["screenshots"].as_array().map_or(Vec::new(), |s| s.iter().filter_map(|x| x["image_id"].as_str()).filter(|id| ID.is_match(id)).map(|id| format!("https://images.igdb.com/igdb/image/upload/t_screenshot_huge/{id}.jpg")).collect())
}

/// The fields Details shows, in LaunchBox's shapes: genres "A; B", release "YYYY-MM-DD", year a number, rating out of 5.
pub fn info_of(h: &Value) -> Option<Value> {
    let h = h.as_object()?;
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::trim).unwrap_or("").to_string();
    let mut o = Map::new();
    let summary = s(h.get("summary"));
    if !summary.is_empty() {
        o.insert("overview".into(), json!(if summary.chars().count() > 1500 {
            static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+\S*$").unwrap());
            format!("{}…", TAIL.replace(&summary.chars().take(1499).collect::<String>(), ""))
        } else {
            summary
        }));
    }
    let uniq = |l: Vec<String>| {
        let mut out: Vec<String> = Vec::new();
        for x in l.into_iter().filter(|x| !x.is_empty()) {
            if !out.contains(&x) {
                out.push(x);
            }
        }
        out
    };
    let genres = uniq(h.get("genres").and_then(Value::as_array).map_or(Vec::new(), |g| g.iter().map(|x| s(x.get("name"))).collect()));
    if !genres.is_empty() {
        o.insert("genres".into(), json!(genres[..genres.len().min(6)].join("; ")));
    }
    let companies: Vec<&Value> = h.get("involved_companies").and_then(Value::as_array).map_or(Vec::new(), |c| c.iter().filter(|x| !s(x["company"].get("name")).is_empty()).collect());
    let who = |role: &str| uniq(companies.iter().filter(|x| x[role] == true).map(|x| s(x["company"].get("name"))).collect());
    let (dev, publisher) = (who("developer"), who("publisher"));
    if !dev.is_empty() {
        o.insert("dev".into(), json!(dev[..dev.len().min(3)].join(", ")));
    }
    if !publisher.is_empty() {
        o.insert("pub".into(), json!(publisher[..publisher.len().min(3)].join(", ")));
    }
    if let Some(t) = h.get("first_release_date").and_then(Value::as_f64).filter(|t| t.is_finite() && *t > 0.0) {
        let iso = js::iso_string((t * 1000.0) as i64);
        o.insert("year".into(), json!(iso[..4].parse::<u32>().unwrap_or(0)));
        o.insert("release".into(), json!(&iso[..10]));
    }
    if let Some(r) = h.get("total_rating").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0) {
        o.insert("rating".into(), json!((r / 20.0 * 10.0).round() / 10.0));
    }
    let series = Some(s(h.get("franchise").and_then(|f| f.get("name")))).filter(|f| !f.is_empty()).or_else(|| h.get("franchises").and_then(Value::as_array).and_then(|l| l.iter().map(|x| s(x.get("name"))).find(|n| !n.is_empty())));
    if let Some(series) = series {
        o.insert("series".into(), json!(series));
    }
    (!o.is_empty()).then_some(Value::Object(o))
}

fn gap(v: &Option<String>) -> bool {
    v.as_deref().is_none_or(|s| s.trim().is_empty())
}

/// What IGDB gave (`x`, as `info_of` writes it) filling a game's gaps; a value already there is never replaced.
/// Whether anything was added.
pub fn fill(info: &mut Info, x: &Value) -> bool {
    let text = |k: &str| x[k].as_str().filter(|s| !s.is_empty()).map(String::from);
    let mut added = false;
    for (have, k) in [(&mut info.overview, "overview"), (&mut info.genres, "genres"), (&mut info.dev, "dev"), (&mut info.publisher, "pub")] {
        if let (true, Some(v)) = (gap(have), text(k)) {
            (*have, added) = (Some(v), true);
        }
    }
    if let (None, Some(r)) = (info.rating, x["rating"].as_f64()) {
        (info.rating, added) = (Some(r), true);
    }
    // the release date only when it doesn't disagree with the year LaunchBox gives
    let year = x["year"].as_u64().map(|y| y.to_string());
    if let Some(release) = text("release").filter(|_| gap(&info.release) && (gap(&info.year) || info.year.as_deref().map(|y| &y[..y.len().min(4)]) == year.as_deref())) {
        (info.release, added) = (Some(release), true);
    }
    if let Some(y) = year.filter(|_| gap(&info.year) && gap(&info.release)) {
        (info.year, added) = (Some(y), true);
    }
    added
}

/// Some of what Details shows is missing: worth a lookup.
pub fn gaps(info: Option<&Info>) -> bool {
    info.is_none_or(|i| gap(&i.overview) || gap(&i.genres) || gap(&i.dev) || (gap(&i.release) && gap(&i.year)))
}

/// IGDB's info kept per game: `<data>\igdb-info\index.json`.
pub struct Store {
    file: PathBuf,
    games: Map<String, Value>,
}

impl Store {
    pub fn load(data_dir: &Path) -> Store {
        let file = data_dir.join("igdb-info").join("index.json");
        let j: Value = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let games = if j["v"] == 1 { j["games"].as_object().cloned().unwrap_or_default() } else { Map::new() };
        Store { file, games }
    }

    /// The info kept for a game, if IGDB had some.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.games.get(key).and_then(|e| e.get("i"))
    }

    /// Not asked yet, or asked more than `RETRY` ago.
    pub fn due(&self, key: &str, now: f64) -> bool {
        self.games.get(key).is_none_or(|e| now - e["at"].as_f64().unwrap_or(0.0) >= RETRY)
    }

    /// An answer for a game, its match or none (IGDB has nothing for it): kept either way, so it isn't asked again
    /// before `RETRY`.
    pub fn put(&mut self, key: &str, hit: Option<&Value>, now: f64) {
        let e = match hit.and_then(info_of) {
            Some(i) => json!({ "at": now, "i": i }),
            None => json!({ "at": now }),
        };
        self.games.insert(key.into(), e);
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(d) = self.file.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = self.file.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, json!({ "v": 1, "games": self.games }).to_string())?;
        std::fs::rename(&tmp, &self.file)
    }
}

/// A game's info with what IGDB gave filling its gaps, looked up first when it's due and there are keys. Whether
/// anything was added.
pub fn lookup_info(client: Option<&Igdb>, data_dir: &Path, sys: &str, file: &Path, title: &str, info: &mut Info) -> bool {
    let key = crate::gamekey::game_key(sys, file, title);
    let mut store = Store::load(data_dir);
    if let Some(c) = client.filter(|c| c.ready() && !c.is_refused() && store.due(&key, now_ms())) {
        if let Ok(Some(found)) = c.lookup(sys, title) {
            store.put(&key, found.hit.as_ref(), now_ms());
            let _ = store.save();
        }
    }
    store.get(&key).is_some_and(|x| fill(info, x))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A pretend Twitch and IGDB: the token for "id"/"good" only; the search answers two games.
    fn pretend(tokens: Arc<AtomicUsize>) -> Fetch {
        Box::new(move |url, headers, body| {
            if url.starts_with("https://id.twitch.tv/") {
                tokens.fetch_add(1, Ordering::Relaxed);
                return Ok(if body.contains("client_secret=good") { (200, br#"{"access_token":"tok","expires_in":5000}"#.to_vec()) } else { (400, Vec::new()) });
            }
            assert!(headers.contains(&("Authorization", "Bearer tok")) && body.contains("where platforms = (19);"));
            Ok((200, json!([
                { "name": "Super Metroid Redesign" },
                { "name": "Super Metroid", "summary": "Samus again.", "first_release_date": 765158400, "total_rating": 92.4, "genres": [{ "name": "Platform" }, { "name": "Adventure" }],
                  "involved_companies": [{ "company": { "name": "Nintendo R&D1" }, "developer": true }, { "company": { "name": "Nintendo" }, "publisher": true }],
                  "franchise": { "name": "Metroid" }, "screenshots": [{ "image_id": "abc123" }] }
            ]).to_string().into_bytes()))
        })
    }

    #[test]
    fn a_game_s_gaps_are_filled_from_its_match_and_kept() {
        let tokens = Arc::new(AtomicUsize::new(0));
        let c = Igdb::with("id", "good", pretend(tokens.clone()));
        let found = c.lookup("snes", "Super Metroid (USA)").unwrap().unwrap();
        let hit = found.hit.unwrap();
        assert_eq!(hit["name"], "Super Metroid");
        assert_eq!(shots_of(&hit), ["https://images.igdb.com/igdb/image/upload/t_screenshot_huge/abc123.jpg"]);
        let x = info_of(&hit).unwrap();
        assert_eq!(x, json!({ "overview": "Samus again.", "genres": "Platform; Adventure", "dev": "Nintendo R&D1", "pub": "Nintendo", "year": 1994, "release": "1994-04-01", "rating": 4.6, "series": "Metroid" }));
        // asked again soon: the same answer, no new request
        c.lookup("snes", "Super Metroid").unwrap();
        assert_eq!(tokens.load(Ordering::Relaxed), 1);

        let mut info = Info { dev: Some("Intelligent Systems".into()), year: Some("1994".into()), ..Info::default() };
        assert!(fill(&mut info, &x));
        assert_eq!((info.dev.as_deref(), info.publisher.as_deref(), info.release.as_deref(), info.overview.as_deref()), (Some("Intelligent Systems"), Some("Nintendo"), Some("1994-04-01"), Some("Samus again.")));
        assert!(!fill(&mut info, &x));
        // a release date that disagrees with LaunchBox's year isn't taken
        let mut other = Info { year: Some("1995".into()), ..Info::default() };
        fill(&mut other, &x);
        assert!(other.release.is_none());

        let tmp = tempfile::tempdir().unwrap();
        let mut info = Info::default();
        assert!(lookup_info(Some(&c), tmp.path(), "snes", Path::new("Super Metroid.sfc"), "Super Metroid", &mut info));
        let store = Store::load(tmp.path());
        assert!(store.get("snes|super metroid.sfc").is_some() && !store.due("snes|super metroid.sfc", now_ms()));
        // kept: found again without keys
        let mut again = Info::default();
        assert!(lookup_info(None, tmp.path(), "snes", Path::new("Super Metroid.sfc"), "", &mut again) && again.genres.is_some());
    }

    #[test]
    fn keys_turned_down_are_not_tried_again_for_a_while() {
        let tokens = Arc::new(AtomicUsize::new(0));
        let c = Igdb::with("id", "bad", pretend(tokens.clone()));
        assert!(c.lookup("snes", "Super Metroid").unwrap_err().refused);
        assert!(c.is_refused());
        assert!(c.lookup("snes", "Super Metroid").unwrap_err().refused);
        assert_eq!(tokens.load(Ordering::Relaxed), 1);
        assert!(Igdb::with("id", "bad", pretend(tokens)).test().is_err());
        assert!(!Igdb::new("", "x").ready());
    }
}

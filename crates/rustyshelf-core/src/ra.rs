// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/ra.js, as RustyShelf has it (with its Neo Geo Pocket shelves).
//! RetroAchievements: progress and achievement lists for the shelves it covers. Achievements are earned inside
//! the emulator; this reads the account through the site's web API and matches games by title.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use unicode_normalization::UnicodeNormalization;

const API: &str = "https://retroachievements.org/API/";
const MEDIA: &str = "https://media.retroachievements.org";
/// how long a failed look-up is answered from before the site is asked again, in milliseconds
const RETRY_MS: u64 = 60_000;

/// The site's number for a console's shelf (rcheevos' rc_consoles.h); Neo Geo sets are in its Arcade list, and
/// both Neo Geo Pocket shelves share one.
pub fn console_id(shelf: &str) -> Option<u32> {
    Some(match shelf {
        "megadrive" => 1,
        "n64" => 2,
        "snes" => 3,
        "gb" => 4,
        "gba" => 5,
        "gbc" => 6,
        "nes" | "fds" => 7,
        "pcengine" => 8,
        "segacd" => 9,
        "sega32x" => 10,
        "mastersystem" => 11,
        "psx" => 12,
        "atarilynx" => 13,
        "ngp" | "ngpc" => 14,
        "gamegear" => 15,
        "gc" => 16,
        "nds" => 18,
        "wii" => 19,
        "ps2" => 21,
        "atari2600" => 25,
        "arcade" | "neogeo" => 27,
        "saturn" => 39,
        "dreamcast" => 40,
        "psp" => 41,
        "wonderswan" => 53,
        _ => return None,
    })
}

/// A title as it's matched: lower case, accents and apostrophes off, "&" as "and", an alternate after "|" and a
/// "~Hack~" tag dropped, anything else that isn't a letter or digit a space, and no "the", "a" or "an".
pub fn norm(title: &str) -> String {
    let plain: String = title.to_lowercase().nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c)).collect::<String>().replace('&', " and ").replace(['\'', '’'], "");
    let plain = plain.find('|').map_or(plain.as_str(), |i| plain[..i].trim_end());
    // a tag between two tildes, with the spaces round it
    let mut out = String::new();
    let mut rest = plain;
    while let Some(open) = rest.find('~') {
        let Some(len) = rest[open + 1..].find('~') else { break };
        out.push_str(rest[..open].trim_end());
        out.push(' ');
        rest = rest[open + len + 2..].trim_start();
    }
    out.push_str(rest);
    out.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit())).filter(|w| !w.is_empty() && !matches!(*w, "the" | "a" | "an")).collect::<Vec<_>>().join(" ")
}

/// A game of a console's list on the site.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed {
    pub id: u64,
    pub title: String,
    folded: String,
    /// how many achievements its set has
    pub count: u64,
}

/// The game of the list a title is: the same title, or one that only adds a subtitle (never a number: a sequel),
/// or one that shares three quarters of its words.
pub fn match_game<'a>(list: &'a [Listed], title: &str) -> Option<&'a Listed> {
    let want = norm(title);
    if want.is_empty() {
        return None;
    }
    if let Some(g) = list.iter().find(|x| x.folded == want) {
        return Some(g);
    }
    let words: HashSet<&str> = want.split(' ').collect();
    let (mut best, mut most) = (None, 0.0);
    for x in list {
        let mut score = 0.0;
        if x.folded.starts_with(&format!("{want} ")) || want.starts_with(&format!("{} ", x.folded)) {
            let extra = if x.folded.len() > want.len() { &x.folded[want.len()..] } else { &want[x.folded.len()..] };
            let numbered = extra.split(' ').any(|w| !w.is_empty() && (w.chars().all(|c| c.is_ascii_digit()) || w.chars().all(|c| "ivx".contains(c))));
            if !numbered {
                score = 0.9 - x.folded.len().abs_diff(want.len()) as f64 / 200.0;
            }
        } else {
            let theirs: Vec<&str> = x.folded.split(' ').collect();
            let shared = theirs.iter().filter(|w| words.contains(*w)).count();
            let all = words.iter().chain(theirs.iter()).collect::<HashSet<_>>().len();
            let part = shared as f64 / all as f64;
            if part >= 0.75 {
                score = part;
            }
        }
        if score > most {
            (most, best) = (score, Some(x));
        }
    }
    best
}

/// How the site is asked: a URL in, its JSON out, or why not in words for the screen.
pub type Fetch = Box<dyn Fn(&str) -> Result<Value, String> + Send + Sync>;

fn over_the_net(url: &str) -> Result<Value, String> {
    let agent = format!("RustyShelf/{}", crate::VERSION);
    let (status, body) = crate::net::get(url, Duration::from_secs(20), 8_000_000, &agent)?;
    match status {
        200 => serde_json::from_slice(&body).map_err(|_| "RetroAchievements sent an answer RustyShelf couldn't read. Try again in a moment.".to_string()),
        429 => Err("RetroAchievements is busy (too many requests). Try again in a minute.".into()),
        401 | 403 => Err("RetroAchievements didn't accept your web API key. Check it in Settings → Accounts.".into()),
        // (never the URL: it carries the API key)
        code => Err(format!("RetroAchievements didn't answer (HTTP {code}). Try again in a moment.")),
    }
}

/// The site's answer when it should be a list: some failures come with 200 and an object that says why.
fn need(v: &Value) -> Result<&Vec<Value>, String> {
    v.as_array().ok_or_else(|| match v.get("message").and_then(Value::as_str).filter(|m| !m.is_empty()) {
        Some(m) => format!("RetroAchievements: {}", m.chars().take(200).collect::<String>()),
        None => "RetroAchievements sent an unexpected answer. Try again in a moment.".into(),
    })
}

/// What JavaScript takes for true.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

fn number(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// A whole number as JSON has it: 5, not 5.0.
fn whole(f: f64) -> Value {
    if f.fract() == 0.0 && f.abs() < 9e15 { json!(f as i64) } else { json!(f) }
}

struct Failed {
    at: u64,
    why: String,
    old: Option<Value>,
}

/// The account in use and what's been read through it.
pub struct Ra {
    dir: PathBuf,
    /// the username and the web API key
    account: Mutex<(String, String)>,
    kept: Mutex<HashMap<String, (u64, Value)>>,
    failed: Mutex<HashMap<String, Failed>>,
    fetch: Fetch,
}

impl Ra {
    /// With its cache in the data folder's `ra`.
    pub fn new(data_dir: &Path) -> Ra {
        Ra::with(data_dir, Box::new(over_the_net))
    }

    /// The same, asking through `fetch` (the tests' stand-in for the site).
    pub fn with(data_dir: &Path, fetch: Fetch) -> Ra {
        let dir = data_dir.join("ra");
        let _ = fs::create_dir_all(&dir);
        Ra { dir, account: Mutex::default(), kept: Mutex::default(), failed: Mutex::default(), fetch }
    }

    pub fn set_credentials(&self, user: &str, key: &str) {
        let next = (user.trim().to_string(), key.trim().to_string());
        let mut account = self.account.lock().unwrap();
        if *account != next {
            // a key put right is tried at once, not after the pause
            self.failed.lock().unwrap().clear();
            *account = next;
        }
    }

    pub fn enabled(&self) -> bool {
        let a = self.account.lock().unwrap();
        !a.0.is_empty() && !a.1.is_empty()
    }

    fn user(&self) -> String {
        self.account.lock().unwrap().0.clone()
    }

    /// The username as it goes into a cache file's name: itself when it's plain, else a hash of it (an odd one
    /// could name a file outside the folder).
    fn user_key(&self) -> String {
        let user = self.user();
        if (1..=64).contains(&user.len()) && user.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            user
        } else {
            format!("u{}", Sha1::digest(user.as_bytes()).iter().map(|b| format!("{b:02x}")).collect::<String>().chars().take(16).collect::<String>())
        }
    }

    fn call(&self, endpoint: &str, params: &[(&str, String)], who: Option<(&str, &str)>) -> Result<Value, String> {
        let account = self.account.lock().unwrap().clone();
        let (user, key) = who.unwrap_or((account.0.as_str(), account.1.as_str()));
        let mut pairs = vec![("z", user.to_string()), ("y", key.to_string())];
        pairs.extend(params.iter().map(|(k, v)| (*k, v.clone())));
        (self.fetch)(&format!("{API}{endpoint}.php?{}", crate::js::query(&pairs)))
    }

    /// What `fetch` gives, kept in memory and on disk for `max_age_ms`. When the site can't be reached the last
    /// copy answers, and for a minute it isn't asked again (a forced look-up, age 0, always asks).
    fn cached(&self, name: &str, max_age_ms: u64, fetch: impl FnOnce() -> Result<Value, String>) -> Result<Value, String> {
        let name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        let file = self.dir.join(format!("{name}.json"));
        let now = crate::now_ms();
        let fresh = |at: u64| now.saturating_sub(at) < max_age_ms;
        if let Some((_, data)) = self.kept.lock().unwrap().get(&name).filter(|k| fresh(k.0)) {
            return Ok(data.clone());
        }
        let on_disk = || fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(j) = on_disk() {
            if let (Some(at), Some(data)) = (j.get("at").and_then(Value::as_u64), j.get("data")) {
                if fresh(at) {
                    self.kept.lock().unwrap().insert(name, (at, data.clone()));
                    return Ok(data.clone());
                }
            }
        }
        if max_age_ms > 0 {
            if let Some(f) = self.failed.lock().unwrap().get(&name).filter(|f| now.saturating_sub(f.at) < RETRY_MS) {
                return f.old.clone().ok_or_else(|| f.why.clone());
            }
        }
        match fetch() {
            Ok(data) => {
                self.kept.lock().unwrap().insert(name.clone(), (now, data.clone()));
                self.failed.lock().unwrap().remove(&name);
                let _ = fs::write(&file, json!({ "at": now, "data": data }).to_string());
                Ok(data)
            }
            Err(why) => {
                let old = on_disk().and_then(|j| j.get("data").cloned());
                let mut failed = self.failed.lock().unwrap();
                if failed.len() >= 200 {
                    failed.clear();
                }
                failed.insert(name, Failed { at: now, why: why.clone(), old: old.clone() });
                old.ok_or(why)
            }
        }
    }

    /// Every game with achievements on a console's shelf (kept a day).
    pub fn game_list(&self, shelf: &str) -> Result<Vec<Listed>, String> {
        let Some(cid) = console_id(shelf).filter(|_| self.enabled()) else { return Ok(Vec::new()) };
        let list = self.cached(&format!("list-{cid}"), 86_400_000, || {
            let all = self.call("API_GetGameList", &[("i", cid.to_string()), ("f", "1".into()), ("h", "0".into())], None)?;
            Ok(Value::Array(need(&all)?.iter().map(|g| json!({ "id": g["ID"], "title": g["Title"], "count": g["NumAchievements"], "points": g["Points"], "icon": g["ImageIcon"] })).collect()))
        })?;
        Ok(list
            .as_array()
            .map(|l| l.iter().map(|g| { let title = g["title"].as_str().unwrap_or("").to_string(); Listed { id: g["id"].as_u64().unwrap_or(0), folded: norm(&title), title, count: g["count"].as_u64().unwrap_or(0) } }).collect())
            .unwrap_or_default())
    }

    /// One game's progress: its totals and every achievement, in the set's order; None for a game the site has
    /// no set for. `force` asks the site again now.
    pub fn progress(&self, shelf: &str, title: &str, force: bool) -> Result<Option<Value>, String> {
        self.progress_within(shelf, title, if force { 0 } else { 600_000 })
    }

    /// Whether the site has a set for this game, from its game list as last downloaded (kept a day).
    pub fn has_set(&self, shelf: &str, title: &str) -> bool {
        self.game_list(shelf).is_ok_and(|list| match_game(&list, title).is_some())
    }

    /// `progress`, with a kept copy no older than `max_age_ms`.
    pub fn progress_within(&self, shelf: &str, title: &str, max_age_ms: u64) -> Result<Option<Value>, String> {
        let list = self.game_list(shelf)?;
        let Some(g) = match_game(&list, title) else { return Ok(None) };
        let user = self.user();
        let data = self.cached(&format!("game-{}-{}", g.id, self.user_key()), max_age_ms, || self.call("API_GetGameInfoAndUserProgress", &[("g", g.id.to_string()), ("u", user.clone()), ("a", "1".into())], None))?;
        let Some(all) = data.get("Achievements").filter(|a| truthy(Some(a))) else { return Err("RetroAchievements sent no achievement list for this game. Try again in a moment.".into()) };
        // (as JavaScript walks an object: its whole-number keys in rising order first)
        let mut listed: Vec<&Value> = match all {
            Value::Object(o) => {
                let mut keys: Vec<(&String, &Value)> = o.iter().collect();
                keys.sort_by_key(|(k, _)| k.parse::<u64>().ok().map_or((1, 0), |n| (0, n)));
                keys.into_iter().map(|(_, v)| v).collect()
            }
            Value::Array(a) => a.iter().collect(),
            _ => Vec::new(),
        };
        listed.sort_by(|a, b| number(a.get("DisplayOrder")).total_cmp(&number(b.get("DisplayOrder"))));
        let players = number(data.get("NumDistinctPlayers"));
        let achievements: Vec<Value> = listed
            .iter()
            .map(|a| {
                let (soft, hard) = (truthy(a.get("DateEarned")), truthy(a.get("DateEarnedHardcore")));
                let awarded = number(a.get("NumAwarded"));
                json!({
                    "id": a["ID"],
                    "title": a["Title"],
                    "desc": a["Description"],
                    "points": whole(number(a.get("Points"))),
                    "type": a.get("Type").filter(|t| truthy(Some(t))).or(a.get("type").filter(|t| truthy(Some(t)))).cloned().unwrap_or(json!("")),
                    "badge": format!("{MEDIA}/Badge/{}{}.png", a["BadgeName"].as_str().map_or_else(|| a["BadgeName"].to_string(), str::to_string), if soft || hard { "" } else { "_lock" }),
                    "earned": soft || hard,
                    "hardcore": hard,
                    "date": if hard { a["DateEarnedHardcore"].clone() } else if soft { a["DateEarned"].clone() } else { Value::Null },
                    "rarity": if awarded != 0.0 && players != 0.0 { whole((100.0 * awarded / players + 0.5).floor()) } else { Value::Null },
                })
            })
            .collect();
        let earned: Vec<&Value> = achievements.iter().filter(|a| a["earned"] == true).collect();
        let points = |list: &mut dyn Iterator<Item = &Value>| whole(list.map(|a| number(a.get("points"))).sum());
        let media = |key: &str| data.get(key).filter(|v| truthy(Some(v))).and_then(Value::as_str).map_or(Value::Null, |p| json!(format!("{MEDIA}{p}")));
        let or_null = |key: &str| data.get(key).filter(|v| truthy(Some(v))).cloned().unwrap_or(Value::Null);
        let mut out = Map::new();
        out.insert("id".into(), json!(g.id));
        out.insert("title".into(), data.get("Title").filter(|t| truthy(Some(t))).cloned().unwrap_or(json!(g.title)));
        if let Some(console) = data.get("ConsoleName") {
            out.insert("console".into(), console.clone());
        }
        out.insert("icon".into(), media("ImageIcon"));
        out.insert("box".into(), media("ImageBoxArt"));
        out.insert("total".into(), json!(achievements.len()));
        out.insert("earned".into(), json!(earned.len()));
        out.insert("points".into(), points(&mut achievements.iter()));
        out.insert("pointsEarned".into(), points(&mut earned.iter().copied()));
        out.insert("completion".into(), or_null("UserCompletion"));
        out.insert("completionHardcore".into(), or_null("UserCompletionHardcore"));
        out.insert("mastered".into(), or_null("HighestAwardKind"));
        out.insert("url".into(), json!(format!("https://retroachievements.org/game/{}", g.id)));
        out.insert("achievements".into(), Value::Array(achievements));
        Ok(Some(Value::Object(out)))
    }

    /// How far each of a shelf's games is, by title, for the marks on the shelf: its set's size, how many are
    /// earned, and whether it's beaten or mastered. Games without a set are left out.
    pub fn shelf_progress(&self, shelf: &str, titles: &[String]) -> Result<Value, String> {
        if console_id(shelf).is_none() || !self.enabled() {
            return Ok(json!({}));
        }
        let list = self.game_list(shelf)?;
        let user = self.user();
        let done = self.cached(&format!("completed-{}", self.user_key()), 600_000, || {
            let r = self.call("API_GetUserCompletionProgress", &[("u", user.clone()), ("c", "500".into())], None)?;
            Ok(Value::Array(need(r.get("Results").unwrap_or(&Value::Null))?.clone()))
        })?;
        let by_id: HashMap<u64, &Value> = done.as_array().map(|d| d.iter().filter_map(|r| Some((r.get("GameID")?.as_u64()?, r))).collect()).unwrap_or_default();
        let mut out = Map::new();
        for t in titles {
            let Some(g) = match_game(&list, t) else { continue };
            let r = by_id.get(&g.id);
            let earned = r.map_or(0.0, |r| number(r.get("NumAwarded")).max(number(r.get("NumAwardedHardcore"))));
            out.insert(t.clone(), json!({ "id": g.id, "total": g.count, "earned": whole(earned), "mastered": r.and_then(|r| r.get("HighestAwardKind")).filter(|k| truthy(Some(k))).cloned().unwrap_or(Value::Null) }));
        }
        Ok(Value::Object(out))
    }

    /// The account: its name, points and picture. None without an account.
    pub fn profile(&self) -> Result<Option<Value>, String> {
        if !self.enabled() {
            return Ok(None);
        }
        let user = self.user();
        self.cached(&format!("profile-{}", self.user_key()), 300_000, || {
            let p = self.call("API_GetUserProfile", &[("u", user.clone())], None)?;
            if !truthy(p.get("User")) {
                return Err("No profile came back. Check the username and web API key.".into());
            }
            let pic = p.get("UserPic").filter(|v| truthy(Some(v))).and_then(Value::as_str).map_or(Value::Null, |u| json!(format!("{MEDIA}{u}")));
            Ok(json!({ "user": p["User"], "points": p["TotalPoints"], "softcore": p["TotalSoftcorePoints"], "pic": pic, "rich": p.get("RichPresenceMsg").filter(|v| truthy(Some(v))).cloned().unwrap_or(json!("")) }))
        })
        .map(Some)
    }

    /// The achievements unlocked in the last `minutes`: shown when a game is come back from.
    pub fn recent(&self, minutes: f64) -> Result<Vec<Value>, String> {
        if !self.enabled() {
            return Ok(Vec::new());
        }
        let r = self.call("API_GetUserRecentAchievements", &[("u", self.user()), ("m", format!("{}", minutes.ceil().max(1.0)))], None)?;
        Ok(r.as_array().map(|list| list.iter().map(|a| json!({ "id": a["AchievementID"], "title": a["Title"], "desc": a["Description"], "points": a["Points"], "badge": format!("{MEDIA}/Badge/{}.png", a["BadgeName"].as_str().unwrap_or("")), "game": a["GameTitle"], "hardcore": a["HardcoreMode"] == 1, "date": a["Date"] })).collect()).unwrap_or_default())
    }

    /// The whole account for the Trophies screen: every game with achievements (latest first), the last month's
    /// unlocks with how rare each is, and the totals. None without an account; `force` asks the site again now.
    pub fn trophies(&self, force: bool) -> Result<Option<Value>, String> {
        if !self.enabled() {
            return Ok(None);
        }
        let (user, key) = (self.user(), self.user_key());
        let prof = self.profile().ok().flatten();
        let done = self.cached(&format!("completed-{key}"), if force { 0 } else { 600_000 }, || {
            let r = self.call("API_GetUserCompletionProgress", &[("u", user.clone()), ("c", "500".into())], None)?;
            Ok(Value::Array(need(r.get("Results").unwrap_or(&Value::Null))?.clone()))
        })?;
        let rec = self.cached(&format!("recent30-{key}"), if force { 0 } else { 300_000 }, || {
            let r = self.call("API_GetUserRecentAchievements", &[("u", user.clone()), ("m", "43200".into())], None)?;
            Ok(Value::Array(need(&r)?.clone()))
        })?;
        let text = |v: Option<&Value>| match v {
            Some(Value::String(s)) => s.clone(),
            Some(v) if truthy(Some(v)) => v.to_string(),
            _ => String::new(),
        };
        let or_null = |v: Option<&Value>| v.filter(|v| truthy(Some(v))).cloned().unwrap_or(Value::Null);
        let mut games: Vec<Value> = done
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let total = number(r.get("MaxPossible"));
                let earned = number(r.get("NumAwarded")).max(number(r.get("NumAwardedHardcore")));
                let icon = r.get("ImageIcon").filter(|v| truthy(Some(v))).map_or(Value::Null, |p| json!(format!("{MEDIA}{}", text(Some(p)))));
                json!({ "id": r["GameID"], "title": text(r.get("Title")), "console": text(r.get("ConsoleName")), "icon": icon, "total": whole(total), "earned": whole(earned),
                    "pct": if total != 0.0 { whole((100.0 * earned / total + 0.5).floor()) } else { json!(0) }, "award": or_null(r.get("HighestAwardKind")), "last": or_null(r.get("MostRecentAwardedDate")) })
            })
            .filter(|g| number(g.get("total")) > 0.0)
            .collect();
        let by = |k: &'static str| move |a: &Value, b: &Value| text(b.get(k)).cmp(&text(a.get(k)));
        games.sort_by(by("last"));
        let mut recent: Vec<Value> = rec
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| {
                let mut o = json!({ "id": a["AchievementID"], "title": text(a.get("Title")), "desc": text(a.get("Description")), "points": whole(number(a.get("Points"))),
                    "badge": format!("{MEDIA}/Badge/{}.png", a.get("BadgeName").map_or("undefined".into(), |b| text(Some(b)))), "game": text(a.get("GameTitle")) });
                if let Some(g) = a.get("GameID").filter(|g| !g.is_null()) {
                    o["gameId"] = g.clone();
                }
                o["console"] = json!(text(a.get("ConsoleName")));
                o["hardcore"] = json!(a.get("HardcoreMode").is_some_and(|h| h == 1 || h == "1"));
                o["date"] = or_null(a.get("Date"));
                o["rarity"] = Value::Null;
                o
            })
            .collect();
        recent.sort_by(by("date"));
        recent.truncate(60);
        let mut ids: Vec<Value> = Vec::new();
        for g in recent.iter().take(15).filter_map(|a| a.get("gameId").filter(|g| truthy(Some(g)))) {
            if !ids.contains(g) {
                ids.push(g.clone());
            }
        }
        ids.truncate(5);
        for gid in ids {
            let id = text(Some(&gid));
            let Ok(d) = self.cached(&format!("game-{id}-{key}"), 600_000, || self.call("API_GetGameInfoAndUserProgress", &[("g", id.clone()), ("u", user.clone()), ("a", "1".into())], None)) else { continue };
            let players = number(d.get("NumDistinctPlayers"));
            let list: Vec<&Value> = match d.get("Achievements") {
                Some(Value::Object(o)) => o.values().collect(),
                Some(Value::Array(a)) => a.iter().collect(),
                _ => continue,
            };
            if players == 0.0 {
                continue;
            }
            let by_id: HashMap<String, &Value> = list.into_iter().map(|x| (text(x.get("ID")), x)).collect();
            for a in recent.iter_mut().filter(|a| a.get("gameId") == Some(&gid)) {
                if let Some(n) = by_id.get(&text(a.get("id"))).and_then(|x| x.get("NumAwarded")).filter(|n| !n.is_null()) {
                    a["rarity"] = whole(((1000.0 * number(Some(n)) / players + 0.5).floor() / 10.0).max(0.1));
                }
            }
        }
        let award = |kinds: &[&str]| games.iter().filter(|g| g["award"].as_str().is_some_and(|k| kinds.contains(&k))).count();
        let p = |k: &str| prof.as_ref().and_then(|p| p.get(k)).filter(|v| truthy(Some(v))).cloned();
        let avg = if games.is_empty() { 0.0 } else { (games.iter().map(|g| number(g.get("pct"))).sum::<f64>() / games.len() as f64 + 0.5).floor() };
        let totals = json!({ "games": games.len(), "unlocked": whole(games.iter().map(|g| number(g.get("earned"))).sum()), "mastered": award(&["mastered", "completed"]),
            "beaten": award(&["beaten-hardcore", "beaten-softcore"]), "avg": whole(avg) });
        Ok(Some(json!({ "user": p("user").unwrap_or(json!(user)), "pic": p("pic").unwrap_or(Value::Null), "points": p("points").unwrap_or(json!(0)), "softcore": p("softcore").unwrap_or(json!(0)),
            "games": games, "recent": recent, "totals": totals })))
    }

    /// Checks a username and key without switching the account in use: the account's name and points.
    pub fn test(&self, user: &str, key: &str) -> Result<Value, String> {
        let (user, key) = (user.trim(), key.trim());
        if user.is_empty() || key.is_empty() {
            return Err("Fill in the username and web API key first.".into());
        }
        let p = self.call("API_GetUserProfile", &[("u", user.to_string())], Some((user, key)))?;
        if !truthy(p.get("User")) {
            return Err("No profile came back. Check the username and web API key.".into());
        }
        Ok(json!({ "user": p["User"], "points": p["TotalPoints"] }))
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/steamgrid.js.
//! SteamGridDB (steamgriddb.com, API v2): community-made posters and logos. Optional: nothing is asked without the
//! user's own key, which goes only into the Authorization header to www.steamgriddb.com. What the site answers is
//! kept in the data folder's `sgdb-cache.json`, the file the Electron app keeps too.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::{covers, js, lbindex};

const BASE: &str = "https://www.steamgriddb.com/api/v2";
const DAY: f64 = 86_400_000.0;
/// which SteamGridDB game a title is, in milliseconds; a title with no match is asked again sooner (the site grows)
const ID_TTL: f64 = 7.0 * DAY;
const MISS_TTL: f64 = DAY;
const LIST_TTL: f64 = DAY;
const MAX_LISTS: usize = 4000;
const MAX_IDS: usize = 20000;
/// Portrait posters only, as a box's front is: Steam's library capsule and the two older poster sizes.
pub const DIMS: &str = "600x900,342x482,660x930";
const STYLES: &str = "alternate,white_logo,no_logo,material,blurred";

/// The site's answer to one request.
pub struct Reply {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

/// How the site is asked: an address and the key in, its answer out. An error is the network's: `true` when it
/// took too long.
pub type Fetch = Box<dyn Fn(&str, &str) -> Result<Reply, bool> + Send + Sync>;

fn over_the_net(url: &str, key: &str) -> Result<Reply, bool> {
    let (agent, bearer) = (format!("RustyShelf/{}", crate::VERSION), format!("Bearer {key}"));
    let headers = [("Authorization", bearer.as_str()), ("User-Agent", agent.as_str()), ("Accept", "application/json")];
    let (status, retry_after, body) = crate::net::ask(url, &headers, Duration::from_secs(8), 4_000_000)?;
    Ok(Reply { status, retry_after, body })
}

/// Why the site gave nothing, with words for the screen (never the key or the site's own text).
#[derive(Debug, Clone, PartialEq)]
pub enum Trouble {
    NoKey,
    /// the site refused the key
    Auth,
    /// the site asked for a pause
    Rate,
    Net(String),
}

impl Trouble {
    pub fn words(&self) -> &str {
        match self {
            Trouble::NoKey => "No SteamGridDB key",
            Trouble::Auth => "The SteamGridDB key was not accepted",
            Trouble::Rate => "SteamGridDB asked us to slow down",
            Trouble::Net(why) => why,
        }
    }
}

/// The game to look up: a Steam app by its number, anything else by its title and then its other names (the most
/// specific first, as `covers::lookup_names` gives them).
#[derive(Debug, Clone, Copy, Default)]
pub struct Game<'a> {
    pub appid: &'a str,
    pub title: &'a str,
    pub names: &'a [String],
}

/// A poster offered for a game's front; `rank` counts from the site's best voted.
#[derive(Debug, Clone, PartialEq)]
pub struct Poster {
    pub url: String,
    pub thumb: Option<String>,
    /// ".png", ".jpg" or ".webp"
    pub ext: &'static str,
    pub id: u64,
    pub w: u32,
    pub h: u32,
    pub style: String,
    pub author: String,
    pub rank: usize,
}

impl Poster {
    pub fn style_name(&self) -> &'static str {
        match self.style.as_str() {
            "alternate" => "Alternate",
            "white_logo" => "White logo",
            "no_logo" => "No logo",
            "material" => "Material",
            "blurred" => "Blurred",
            _ => "",
        }
    }

    /// As the Electron app's Art Studio lists it.
    pub fn to_json(&self) -> Value {
        json!({ "url": self.url, "thumbUrl": self.thumb, "ext": self.ext, "source": "steamgriddb", "type": "grid", "sgId": self.id, "w": self.w, "h": self.h,
            "style": self.style, "styleName": self.style_name(), "author": self.author, "rank": self.rank })
    }
}

/// A game's title art on a clear background (a PNG).
#[derive(Debug, Clone, PartialEq)]
pub struct Logo {
    pub url: String,
    pub w: u32,
    pub h: u32,
}

#[derive(Default)]
struct Cache {
    /// "steam:<appid>" or "title:<title in lower case>" → [the site's game id or 0, when]
    ids: Map<String, Value>,
    /// "<game id or steam:appid>|<sizes or logos>" → [when, the list]
    lists: Map<String, Value>,
}

#[derive(Default)]
struct State {
    key: String,
    /// a key the site refused: not sent again until it changes
    bad: String,
    paused_until: f64,
    cache: Option<Cache>,
}

pub struct SteamGrid {
    file: PathBuf,
    state: Mutex<State>,
    /// requests on their way: two at once at most
    busy: (Mutex<usize>, Condvar),
    fetch: Fetch,
}

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

fn appid_of(game: &Game) -> Option<String> {
    let id = game.appid;
    ((1..=10).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())).then(|| id.to_string())
}

/// An https address on SteamGridDB's own hosts.
fn ok_url(v: Option<&Value>, png_only: bool) -> Option<&str> {
    static ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^(?i)https://([a-z0-9-]+\.)*steamgriddb\.com/[^\s"<>]+$"#).unwrap());
    static PNG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^(?i)https://([a-z0-9-]+\.)*steamgriddb\.com/[^\s"<>]+\.png$"#).unwrap());
    let u = v?.as_str()?;
    (if png_only { &PNG } else { &ANY }.is_match(u) && u.encode_utf16().count() < 600).then_some(u)
}

/// The answer's list, when it is one.
fn rows(d: Option<&Value>) -> &[Value] {
    d.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn ext_of(url: &str, mime: &str) -> &'static str {
    static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(png|jpe?g|webp)(?:$|\?)").unwrap());
    let named = EXT.captures(url).map(|c| c[1].to_lowercase());
    match named.as_deref() {
        Some("png") => ".png",
        Some("webp") => ".webp",
        Some(_) => ".jpg",
        None if mime.contains("png") => ".png",
        None if mime.contains("webp") => ".webp",
        None => ".jpg",
    }
}

impl SteamGrid {
    /// With its cache in the data folder.
    pub fn new(data_dir: &Path) -> SteamGrid {
        SteamGrid::with(data_dir, Box::new(over_the_net))
    }

    /// The same, asking through `fetch` (the tests' stand-in for the site).
    pub fn with(data_dir: &Path, fetch: Fetch) -> SteamGrid {
        SteamGrid { file: data_dir.join("sgdb-cache.json"), state: Mutex::default(), busy: Default::default(), fetch }
    }

    pub fn set_key(&self, key: &str) {
        self.state.lock().unwrap().key = key.trim().to_string();
    }

    /// With the key kept in Settings (`data/config.json`), or None when there's none.
    pub fn from_settings(data_dir: &Path) -> Option<SteamGrid> {
        let cfg = crate::config::Config::load(data_dir).ok()?;
        let key = cfg.api_key("steamgriddb", crate::seal::key(&crate::paths::env_var).as_ref());
        let sg = SteamGrid::new(data_dir);
        sg.set_key(&key);
        sg.enabled().then_some(sg)
    }

    pub fn enabled(&self) -> bool {
        let s = self.state.lock().unwrap();
        !s.key.is_empty() && s.key != s.bad
    }

    fn cache<T>(&self, with: impl FnOnce(&mut Cache) -> T) -> T {
        let mut s = self.state.lock().unwrap();
        let cache = s.cache.get_or_insert_with(|| {
            let read = fs::read(&self.file).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
            match read.as_ref().map(|j| (&j["ids"], &j["lists"])) {
                Some((Value::Object(ids), Value::Object(lists))) => Cache { ids: ids.clone(), lists: lists.clone() },
                _ => Cache::default(),
            }
        });
        with(cache)
    }

    /// Writes the cache, without what's gone stale and the oldest of too many.
    fn save(&self) {
        let text = self.cache(|c| {
            let now = now_ms();
            c.ids.retain(|_, v| now - js::number(v.get(1)) <= ID_TTL);
            c.lists.retain(|_, v| now - js::number(v.get(0)) <= LIST_TTL);
            let trim = |m: &mut Map<String, Value>, most: usize, at: usize| {
                if m.len() > most {
                    let mut keys: Vec<(f64, String)> = m.iter().map(|(k, v)| (js::number(v.get(at)), k.clone())).collect();
                    keys.sort_by(|a, b| a.0.total_cmp(&b.0));
                    for (_, k) in keys.iter().take(m.len() - most) {
                        m.remove(k);
                    }
                }
            };
            trim(&mut c.ids, MAX_IDS, 1);
            trim(&mut c.lists, MAX_LISTS, 0);
            json!({ "ids": c.ids, "lists": c.lists }).to_string()
        });
        let tmp = self.file.with_extension("json.tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &self.file);
        }
    }

    /// One API path's "data"; None when there's no such game or nothing was found. `key`: the one to use instead
    /// of the account's (the one being tested).
    fn api(&self, path: &str, key: Option<&str>) -> Result<Option<Value>, Trouble> {
        let (mine, paused_until) = {
            let s = self.state.lock().unwrap();
            (s.key.clone(), s.paused_until)
        };
        let key = key.unwrap_or(&mine);
        if key.is_empty() {
            return Err(Trouble::NoKey);
        }
        if now_ms() < paused_until {
            return Err(Trouble::Rate);
        }
        {
            let mut busy = self.busy.1.wait_while(self.busy.0.lock().unwrap(), |n| *n >= 2).unwrap();
            *busy += 1;
        }
        let got = (self.fetch)(&format!("{BASE}{path}"), key);
        *self.busy.0.lock().unwrap() -= 1;
        self.busy.1.notify_one();
        let r = got.map_err(|slow| Trouble::Net(if slow { "SteamGridDB took too long to answer" } else { "Couldn't reach SteamGridDB" }.into()))?;
        match r.status {
            401 | 403 => {
                let mut s = self.state.lock().unwrap();
                if key == s.key {
                    s.bad = key.to_string();
                }
                Err(Trouble::Auth)
            }
            429 => {
                // the pause the site asks for, in seconds: half a minute to ten minutes
                let asked = r.retry_after.as_deref().map_or(f64::NAN, |a| js::number(Some(&json!(a))));
                let secs = if asked.is_nan() || asked == 0.0 { 60.0 } else { asked }.clamp(30.0, 600.0);
                self.state.lock().unwrap().paused_until = now_ms() + secs * 1000.0;
                Err(Trouble::Rate)
            }
            404 => Ok(None),
            200..=299 => {
                let j: Value = serde_json::from_slice(&r.body).map_err(|_| Trouble::Net("SteamGridDB sent something unexpected".into()))?;
                if !js::truthy(Some(&j)) || j.get("success") == Some(&Value::Bool(false)) {
                    return Ok(None);
                }
                Ok(j.get("data").filter(|d| !d.is_null()).cloned())
            }
            code => Err(Trouble::Net(format!("SteamGridDB answered {code}"))),
        }
    }

    /// Which SteamGridDB game this is, 0 when it has none: a Steam app by its number, anything else by title
    /// through the site's search, taken only on an exact or near match (the box art lookup's own matching), its
    /// other names on an exact one. At most three searches a game.
    pub fn game_id(&self, game: &Game) -> Result<u64, Trouble> {
        let (appid, title) = (appid_of(game), js::trim(game.title));
        if appid.is_none() && title.is_empty() {
            return Ok(0);
        }
        let ck = appid.as_ref().map_or_else(|| format!("title:{}", title.to_lowercase()), |a| format!("steam:{a}"));
        let now = now_ms();
        let hit = self.cache(|c| c.ids.get(&ck).map(|h| (js::number(h.get(0)), js::number(h.get(1)))));
        if let Some((id, at)) = hit {
            if now - at < if id != 0.0 { ID_TTL } else { MISS_TTL } {
                return Ok(id as u64);
            }
        }
        let whole = |v: Option<&Value>| Some(js::number(v)).filter(|n| n.is_finite() && *n > 0.0).map_or(0, |n| n as u64);
        let mut id = 0;
        if let Some(a) = &appid {
            id = whole(self.api(&format!("/games/steam/{a}"), None)?.as_ref().and_then(|d| d.get("id")));
        } else {
            let own = [title.to_string()];
            let names = if game.names.is_empty() { &own[..] } else { &game.names[..game.names.len().min(3)] };
            for (i, name) in names.iter().enumerate() {
                let d = self.api(&format!("/search/autocomplete/{}", js::encode_uri_component(js::slice(name, 120))), None)?;
                let list: Vec<&Value> = rows(d.as_ref()).iter().filter(|x| js::truthy(x.get("id")) && js::truthy(x.get("name"))).collect();
                let folded: Vec<String> = list.iter().map(|x| lbindex::norm(&js::string(x.get("name")))).collect();
                let hit = covers::match_title(&folded.iter().map(String::as_str).collect::<Vec<_>>(), name, &|_| 1, i > 0);
                id = hit.map_or(0, |n| whole(list[n].get("id")));
                if id != 0 {
                    break;
                }
            }
        }
        self.cache(|c| c.ids.insert(ck, json!([id, now_ms()])));
        self.save();
        Ok(id)
    }

    /// A list of the cache, or what `ask` gives for it, kept a day.
    fn list(&self, ck: String, ask: impl FnOnce() -> Result<Vec<Value>, Trouble>) -> Result<Vec<Value>, Trouble> {
        let now = now_ms();
        if let Some(Value::Array(list)) = self.cache(|c| c.lists.get(&ck).filter(|h| now - js::number(h.get(0)) < LIST_TTL).and_then(|h| h.get(1).cloned())) {
            return Ok(list);
        }
        let list = ask()?;
        self.cache(|c| c.lists.insert(ck, json!([now_ms(), list])));
        self.save();
        Ok(list)
    }

    /// The poster grids of a game (or of a Steam app), in the site's own order: the best voted first.
    fn grids(&self, id: u64, appid: Option<&str>) -> Result<Vec<Value>, Trouble> {
        let of = appid.map_or_else(|| id.to_string(), |a| format!("steam:{a}"));
        self.list(format!("{of}|{DIMS}"), || {
            let path = appid.map_or_else(|| format!("/grids/game/{id}"), |a| format!("/grids/steam/{a}"));
            let d = self.api(&format!("{path}?dimensions={DIMS}&styles={STYLES}&nsfw=false&humor=false&epilepsy=false"), None)?;
            let size = |g: &Value, k: &str| js::number(g.get(k));
            Ok(rows(d.as_ref())
                .iter()
                .filter(|g| js::truthy(g.get("id")) && ok_url(g.get("url"), false).is_some() && !js::truthy(g.get("nsfw")) && !js::truthy(g.get("humor")))
                .filter(|g| size(g, "width") > 0.0 && size(g, "height") > 0.0 && size(g, "height") >= size(g, "width"))
                .take(40)
                .map(|g| {
                    let text = |v: Option<&Value>| if js::truthy(v) { js::string(v) } else { String::new() };
                    let score = js::number(g.get("score"));
                    json!({ "id": js::num(js::number(g.get("id"))), "url": g["url"], "thumb": ok_url(g.get("thumb"), false), "w": js::num(size(g, "width")), "h": js::num(size(g, "height")), "style": text(g.get("style")),
                        "author": js::slice(&text(g.get("author").and_then(|a| a.get("name"))), 40), "score": js::num(if score.is_nan() { 0.0 } else { score }), "mime": text(g.get("mime")) })
                })
                .collect())
        })
    }

    /// Posters for a game's front. Nothing when there's no key, no match, no connection, a pause or a refused key.
    pub fn posters(&self, game: &Game) -> Vec<Poster> {
        if !self.enabled() {
            return Vec::new();
        }
        let list = match appid_of(game) {
            Some(a) => self.grids(0, Some(&a)),
            None => self.game_id(game).and_then(|id| if id == 0 { Ok(Vec::new()) } else { self.grids(id, None) }),
        };
        let text = |v: &Value| v.as_str().unwrap_or("").to_string();
        list.unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(rank, g)| Poster {
                thumb: g["thumb"].as_str().map(str::to_string),
                ext: ext_of(g["url"].as_str().unwrap_or(""), g["mime"].as_str().unwrap_or("")),
                url: text(&g["url"]),
                id: js::number(g.get("id")) as u64,
                w: js::number(g.get("w")) as u32,
                h: js::number(g.get("h")) as u32,
                style: text(&g["style"]),
                author: text(&g["author"]),
                rank,
            })
            .collect()
    }

    /// A game's logos, the best voted first: static PNGs only. Nothing whenever `posters` gives nothing.
    pub fn logos(&self, game: &Game) -> Vec<Logo> {
        if !self.enabled() {
            return Vec::new();
        }
        let list = self.game_id(game).and_then(|id| {
            if id == 0 {
                return Ok(Vec::new());
            }
            self.list(format!("{id}|logos"), || {
                let d = self.api(&format!("/logos/game/{id}?types=static&nsfw=false&humor=false&epilepsy=false"), None)?;
                let size = |g: &Value, k: &str| js::number(g.get(k));
                Ok(rows(d.as_ref())
                    .iter()
                    .filter(|g| ok_url(g.get("url"), true).is_some() && !js::truthy(g.get("nsfw")) && !js::truthy(g.get("humor")) && size(g, "width") > 0.0 && size(g, "height") > 0.0)
                    .take(12)
                    .map(|g| json!({ "url": g["url"], "ext": ".png", "w": js::num(size(g, "width")), "h": js::num(size(g, "height")) }))
                    .collect())
            })
        });
        list.unwrap_or_default().iter().map(|g| Logo { url: g["url"].as_str().unwrap_or("").to_string(), w: js::number(g.get("w")) as u32, h: js::number(g.get("h")) as u32 }).collect()
    }

    /// Whether the site takes this key; if not, why in a word for the screen.
    pub fn test(&self, key: &str) -> Result<(), String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("no key".into());
        }
        match self.api("/search/autocomplete/zelda", Some(key)) {
            Ok(_) => {
                let mut s = self.state.lock().unwrap();
                if s.bad == key {
                    s.bad.clear();
                }
                Ok(())
            }
            Err(Trouble::Auth) => Err("not accepted".into()),
            Err(other) => Err(other.words().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pause_lasts_what_the_site_asks_within_limits() {
        for (asked, secs) in [(Some("5"), 30.0), (Some("120"), 120.0), (Some("9999"), 600.0), (Some("soon"), 60.0), (None, 60.0)] {
            let tmp = tempfile::tempdir().unwrap();
            let sg = SteamGrid::with(tmp.path(), Box::new(move |_, _| Ok(Reply { status: 429, retry_after: asked.map(str::to_string), body: Vec::new() })));
            sg.set_key("made-up-key");
            assert_eq!(sg.api("/search/autocomplete/x", None), Err(Trouble::Rate));
            let left = (sg.state.lock().unwrap().paused_until - now_ms()) / 1000.0;
            assert!((left - secs).abs() < 2.0, "{asked:?}: {left}");
        }
    }
}

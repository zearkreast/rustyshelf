//! Steam achievements for the Trophies screen, with the player's own Steam Web API key, for the account signed in to
//! Steam on this PC. No error may carry a URL: it holds the key.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use regex::Regex;
use serde_json::{json, Value};

use crate::js::{number, round, text, truthy};

const API: &str = "https://api.steampowered.com";
const MAX_GAMES: usize = 60;
const RECENT_DAYS: f64 = 30.0;
const CONCURRENCY: usize = 4;

/// A GET's status and its body as JSON (None when it isn't JSON); an error is the network's.
pub type Fetch = Box<dyn Fn(&str) -> Result<(u16, Option<Value>), String> + Send + Sync>;

fn online(url: &str) -> Result<(u16, Option<Value>), String> {
    match crate::net::ask(url, &[("User-Agent", "RustyShelf")], Duration::from_secs(20), 32 << 20) {
        Ok((status, _, body)) => Ok((status, serde_json::from_slice(&body).ok())),
        Err(true) => Err("Steam took too long to answer.".into()),
        Err(false) => Err("Steam couldn't be reached.".into()),
    }
}

/// "2026-01-02 10:00:00" (UTC) for seconds since 1970; null for 0.
fn iso(t: f64) -> Value {
    if t == 0.0 || !t.is_finite() {
        return Value::Null;
    }
    crate::pcach::stamp(t).map_or(Value::Null, Value::from)
}

/// The same file on cdn.akamai.steamstatic.com, the only Steam image host the Electron page may load from.
fn cdn(u: Option<&Value>) -> Value {
    static HOST: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^https?://[^/]+(/steamcommunity/)").unwrap());
    if !truthy(u) {
        return Value::Null;
    }
    json!(HOST.replace(&text(u), "https://cdn.akamai.steamstatic.com$1"))
}

fn valid(key: &str) -> bool {
    key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The account signed in to Steam here: loginusers.vdf's MostRecent user, else the newest userdata folder (its name is
/// the 32-bit account id; the 64-bit id is that plus 76561197960265728).
pub fn steam_id(root: &Path) -> Option<String> {
    static ID: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"^7656\d{13}$").unwrap());
    if let Ok(t) = fs::read_to_string(root.join("config").join("loginusers.vdf")) {
        let d = crate::vdf::parse(t.trim_start_matches('\u{feff}'));
        let mut best: Option<(&str, f64)> = None;
        for (id, u) in d.get(&["users"]).map(|u| u.entries()).unwrap_or_default() {
            if !ID.is_match(id) {
                continue;
            }
            let t = if u.text(&["MostRecent"]) == Some("1") { f64::INFINITY } else { u.num(&["Timestamp"]) };
            if best.is_none_or(|b| t > b.1) {
                best = Some((id, t));
            }
        }
        if let Some((id, _)) = best {
            return Some(id.to_string());
        }
    }
    let mut best: Option<(u64, SystemTime)> = None;
    for e in fs::read_dir(root.join("userdata")).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(n) = name.parse::<u64>().ok().filter(|n| *n != 0 && name.bytes().all(|b| b.is_ascii_digit())) else { continue };
        let Ok(t) = fs::metadata(e.path().join("config").join("localconfig.vdf")).and_then(|m| m.modified()) else { continue };
        if best.is_none_or(|b| t > b.1) {
            best = Some((n, t));
        }
    }
    best.map(|(n, _)| (76_561_197_960_265_728u64 + n).to_string())
}

/// The account keeps its game details private.
struct Private;

pub struct SteamAch {
    dir: PathBuf,
    key: String,
    root: Option<PathBuf>,
    fetch: Fetch,
    mem: Mutex<HashMap<String, (u64, Value)>>,
}

impl SteamAch {
    pub fn new(data_dir: &Path, key: &str, steam_root: Option<PathBuf>) -> SteamAch {
        SteamAch::with(data_dir, key, steam_root, Box::new(online))
    }

    pub fn with(data_dir: &Path, key: &str, steam_root: Option<PathBuf>, fetch: Fetch) -> SteamAch {
        let dir = data_dir.join("steam-ach");
        let _ = fs::create_dir_all(&dir);
        SteamAch { dir, key: key.trim().to_string(), root: steam_root, fetch, mem: Mutex::new(HashMap::new()) }
    }

    /// The key and Steam's folder from the settings; None without a usable key.
    pub fn from_settings(data_dir: &Path) -> Option<SteamAch> {
        let cfg = crate::config::Config::load(data_dir).ok()?;
        let key = cfg.api_key("steamweb", crate::seal::key(&crate::paths::env_var).as_ref());
        let root = crate::steam::main_root(&cfg.steam().1);
        let s = SteamAch::new(data_dir, &key, root);
        s.enabled().then_some(s)
    }

    pub fn enabled(&self) -> bool {
        valid(&self.key)
    }

    fn get(&self, url: &str) -> Result<(u16, Value), String> {
        let (status, j) = (self.fetch)(url)?;
        match (status, j) {
            (401, _) | (403, None) => Err("Steam didn't accept the Web API key. Check it in Settings → Accounts & services.".into()),
            (429, _) => Err("Steam is busy (too many requests). Try again in a minute.".into()),
            // 400 / 403 with a body: "no stats", "profile is not public"
            (s, Some(j)) => Ok((s, j)),
            (s, None) => Err(format!("Steam didn't answer (HTTP {s}).")),
        }
    }

    fn q(&self, pairs: &[(&str, String)]) -> String {
        crate::js::query(pairs)
    }

    fn cached<E>(&self, name: &str, max_age_ms: u64, fetch: impl FnOnce() -> Result<Value, E>) -> Result<Value, E> {
        let name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
        let file = self.dir.join(format!("{name}.json"));
        let now = crate::now_ms();
        let fresh = |at: u64| now.saturating_sub(at) < max_age_ms;
        if let Some((_, d)) = self.mem.lock().unwrap().get(&name).filter(|m| fresh(m.0)) {
            return Ok(d.clone());
        }
        let on_disk = || fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        if let Some(j) = on_disk() {
            if let (Some(at), Some(d)) = (j.get("at").and_then(Value::as_u64), j.get("data")) {
                if fresh(at) {
                    self.mem.lock().unwrap().insert(name, (at, d.clone()));
                    return Ok(d.clone());
                }
            }
        }
        match fetch() {
            Ok(d) => {
                self.mem.lock().unwrap().insert(name, (now, d.clone()));
                let _ = fs::write(&file, json!({ "at": now, "data": d }).to_string());
                Ok(d)
            }
            // offline: the last copy
            Err(e) => on_disk().and_then(|j| j.get("data").cloned()).ok_or(e),
        }
    }

    fn summary(&self, sid: &str, age: u64) -> Result<Value, String> {
        self.cached(&format!("player-{sid}"), age, || {
            let (_, j) = self.get(&format!("{API}/ISteamUser/GetPlayerSummaries/v2/?{}", self.q(&[("key", self.key.clone()), ("steamids", sid.into())])))?;
            let p = j.pointer("/response/players/0").filter(|p| truthy(Some(p))).ok_or("Steam sent no profile for this account.")?;
            let pic = [p.get("avatarfull"), p.get("avatarmedium")].into_iter().find(|v| truthy(*v)).flatten().cloned().unwrap_or(Value::Null);
            Ok(json!({ "name": text(p.get("personaname")), "pic": pic, "visible": p.get("communityvisibilitystate") == Some(&json!(3)) }))
        })
    }

    fn owned(&self, sid: &str, age: u64) -> Result<Value, String> {
        self.cached(&format!("owned-{sid}"), age, || {
            let (_, j) = self.get(&format!("{API}/IPlayerService/GetOwnedGames/v1/?{}", self.q(&[("key", self.key.clone()), ("steamid", sid.into()), ("include_appinfo", "1".into()), ("include_played_free_games", "1".into())])))?;
            let g = j.pointer("/response/games").and_then(Value::as_array).ok_or("Steam sent no game list. In Steam, set Profile → Privacy → Game details to Public, then reload.")?;
            // without Steam's stats flag, every played game is asked
            let flagged = g.iter().any(|x| truthy(x.get("has_community_visible_stats")));
            Ok(Value::Array(
                g.iter()
                    .filter(|x| number(x.get("playtime_forever")) > 0.0 && (!flagged || truthy(x.get("has_community_visible_stats"))))
                    .map(|x| {
                        let icon = if truthy(x.get("img_icon_url")) { json!(format!("https://media.steampowered.com/steamcommunity/public/images/apps/{}/{}.jpg", text(x.get("appid")), text(x.get("img_icon_url")))) } else { Value::Null };
                        let last = number(x.get("rtime_last_played"));
                        json!({ "appid": x["appid"], "name": text(x.get("name")), "last": crate::js::num(if last.is_nan() { 0.0 } else { last }), "icon": icon })
                    })
                    .collect(),
            ))
        })
    }

    /// One game's achievements for the account; null when the game has none (or keeps them hidden).
    fn player(&self, sid: &str, appid: &str, age: u64) -> Result<Value, Result<Private, String>> {
        self.cached(&format!("game-{appid}-{sid}"), age, || {
            let (_, j) = self.get(&format!("{API}/ISteamUserStats/GetPlayerAchievements/v1/?{}", self.q(&[("key", self.key.clone()), ("steamid", sid.into()), ("appid", appid.into()), ("l", "english".into())]))).map_err(Err)?;
            let ps = j.get("playerstats").filter(|p| truthy(Some(p)));
            let Some(ps) = ps.filter(|p| p.get("success") != Some(&json!(false))) else {
                if text(ps.and_then(|p| p.get("error"))).to_lowercase().contains("not public") {
                    return Err(Ok(Private));
                }
                return Ok(Value::Null);
            };
            Ok(Value::Array(
                crate::js::items(ps.get("achievements"))
                    .into_iter()
                    .map(|a| {
                        let title = if truthy(a.get("name")) { text(a.get("name")) } else { crate::js::string(a.get("apiname")) };
                        let at = number(a.get("unlocktime"));
                        json!({ "api": a.get("apiname").cloned().unwrap_or(Value::Null), "title": title, "desc": text(a.get("description")), "earned": a.get("achieved") == Some(&json!(1)), "at": crate::js::num(if at.is_nan() { 0.0 } else { at }) })
                    })
                    .collect(),
            ))
        })
    }

    fn icons(&self, appid: &str) -> Value {
        self.cached(&format!("schema-{appid}"), 7 * 86_400_000, || -> Result<Value, String> {
            let (_, j) = self.get(&format!("{API}/ISteamUserStats/GetSchemaForGame/v2/?{}", self.q(&[("key", self.key.clone()), ("appid", appid.into()), ("l", "english".into())])))?;
            Ok(Value::Object(crate::js::items(j.pointer("/game/availableGameStats/achievements")).into_iter().map(|a| (crate::js::string(a.get("name")), a.get("icon").filter(|i| truthy(Some(i))).cloned().unwrap_or(Value::Null))).collect()))
        })
        .unwrap_or_else(|_| json!({}))
    }

    fn world(&self, appid: &str) -> Value {
        self.cached(&format!("world-{appid}"), 86_400_000, || -> Result<Value, String> {
            let (_, j) = self.get(&format!("{API}/ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?{}", self.q(&[("gameid", appid.into())])))?;
            Ok(Value::Object(crate::js::items(j.pointer("/achievementpercentages/achievements")).into_iter().map(|a| (crate::js::string(a.get("name")), crate::js::num(number(a.get("percent"))))).collect()))
        })
        .unwrap_or_else(|_| json!({}))
    }

    /// One game's achievements for the account signed in to Steam here, as [`crate::ra::Ra::progress`] gives them:
    /// pictures, how rare each is, steamhunters.com's points, and the release `year`. None without a key, or for a
    /// game Steam keeps none for.
    pub fn game(&self, appid: &str, force: bool) -> Result<Option<Value>, String> {
        if !self.enabled() || appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(None);
        }
        let sid = self.root.as_deref().and_then(steam_id).ok_or("RustyShelf couldn't find the account signed in to Steam on this PC.")?;
        let list = match self.player(&sid, appid, if force { 0 } else { 600_000 }) {
            Ok(l) => l,
            Err(Ok(Private)) => return Err("Your Steam game details are private. In Steam, set Profile → Privacy → Game details to Public, then reload.".into()),
            Err(Err(e)) => return Err(e),
        };
        let Some(list) = list.as_array().filter(|l| !l.is_empty()) else { return Ok(None) };
        let (icons, rarity, points) = (self.icons(appid), self.rarity(appid), self.hunter_points(appid));
        let mut total = (0i64, 0i64);
        let achievements: Vec<Value> = list
            .iter()
            .map(|x| {
                let api = crate::js::string(x.get("api"));
                let earned = x["earned"] == true;
                let pts = points.get(&api).map(|n| n.round() as i64);
                if let Some(n) = pts {
                    total.0 += n;
                    total.1 += if earned { n } else { 0 };
                }
                json!({ "id": api, "api": api, "title": x["title"], "desc": x["desc"], "points": pts.map_or(json!(0), |n| json!(n)), "badge": cdn(icons.get(&api)),
                    "earned": earned, "hardcore": false, "date": if earned { iso(number(x.get("at"))) } else { Value::Null },
                    "rarity": rarity.get(&api).map_or(Value::Null, |r| crate::js::num((round(r * 10.0) / 10.0).max(0.1))) })
            })
            .collect();
        let earned = achievements.iter().filter(|a| a["earned"] == true).count();
        let mut out = json!({ "id": appid, "title": self.app_name(appid), "console": "Steam", "source": "steam", "total": achievements.len(), "earned": earned,
            "points": total.0, "pointsEarned": total.1, "mastered": if earned == achievements.len() { "completed" } else { "" }, "achievements": achievements,
            "url": format!("https://store.steampowered.com/app/{appid}") });
        if let Some(y) = self.release_year(appid) {
            out["year"] = json!(y);
        }
        Ok(Some(out))
    }

    fn app_name(&self, appid: &str) -> Value {
        let sid = self.root.as_deref().and_then(steam_id).unwrap_or_default();
        let owned = self.owned(&sid, 86_400_000).unwrap_or(Value::Null);
        crate::js::items(Some(&owned)).into_iter().find(|g| crate::js::string(g.get("appid")) == appid).and_then(|g| g.get("name")).cloned().unwrap_or(json!(""))
    }

    /// The year the Steam store says the game came out, kept a month.
    fn release_year(&self, appid: &str) -> Option<String> {
        static YEAR: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| Regex::new(r"\b(19[5-9]\d|20\d\d)\b").unwrap());
        let got = self.cached(&format!("year-{appid}"), 30 * 86_400_000, || -> Result<Value, String> {
            let (_, j) = self.get(&format!("https://store.steampowered.com/api/appdetails?appids={appid}&filters=release_date"))?;
            Ok(j.pointer(&format!("/{appid}/data/release_date/date")).cloned().unwrap_or(json!("")))
        });
        got.ok().and_then(|d| YEAR.find(d.as_str()?).map(|m| m.as_str().to_string()))
    }

    /// How many players have each of a game's achievements, in percent, by API name; Steam answers this without a key.
    pub fn rarity(&self, appid: &str) -> HashMap<String, f64> {
        let w = self.world(appid);
        w.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect()).unwrap_or_default()
    }

    /// steamhunters.com's points for each of a game's achievements, by API name (its public JSON, kept a week).
    pub fn hunter_points(&self, appid: &str) -> HashMap<String, f64> {
        if appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
            return HashMap::new();
        }
        let got = self.cached(&format!("hunters-{appid}"), 7 * 86_400_000, || -> Result<Value, String> {
            let (_, j) = self.get(&format!("https://steamhunters.com/api/apps/{appid}/achievements"))?;
            Ok(Value::Object(crate::js::items(Some(&j)).into_iter().filter_map(|a| Some((a.get("apiName")?.as_str()?.to_string(), a.get("points").cloned().filter(Value::is_number)?))).collect()))
        });
        got.ok().and_then(|v| v.as_object().map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect())).unwrap_or_default()
    }

    /// The Trophies screen's Steam half: { user, pic, games, recent }, None without a key. `now` is seconds since 1970.
    pub fn trophies(&self, force: bool, now: f64) -> Result<Option<Value>, String> {
        if !self.enabled() {
            return Ok(None);
        }
        let sid = self.root.as_deref().and_then(steam_id).ok_or("RustyShelf couldn't find the account signed in to Steam on this PC.")?;
        let age = if force { 0 } else { 600_000 };
        let me = self.summary(&sid, if force { 0 } else { 86_400_000 }).ok();
        let list = self.owned(&sid, if force { 0 } else { 3_600_000 })?;
        let mut pick: Vec<Value> = crate::js::items(Some(&list)).into_iter().cloned().collect();
        pick.sort_by(|a, b| number(b.get("last")).total_cmp(&number(a.get("last"))));
        pick.truncate(MAX_GAMES);
        let private = std::sync::atomic::AtomicBool::new(false);
        let per: Vec<Mutex<Value>> = pick.iter().map(|_| Mutex::new(Value::Null)).collect();
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..pick.len().min(CONCURRENCY) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(g) = pick.get(i) else { break };
                    match self.player(&sid, &crate::js::string(g.get("appid")), age) {
                        Ok(a) => *per[i].lock().unwrap() = a,
                        Err(Ok(Private)) => private.store(true, Ordering::Relaxed),
                        Err(Err(_)) => {}
                    }
                });
            }
        });
        let per: Vec<Value> = per.into_iter().map(|m| m.into_inner().unwrap()).collect();
        if private.load(Ordering::Relaxed) && !per.iter().any(|a| truthy(Some(a))) {
            return Err("Your Steam game details are private. In Steam, set Profile → Privacy → Game details to Public, then reload.".into());
        }
        let (mut games, mut recent) = (Vec::new(), Vec::new());
        let since = now - RECENT_DAYS * 86_400.0;
        for (g, a) in pick.iter().zip(&per) {
            let Some(a) = a.as_array().filter(|a| !a.is_empty()) else { continue };
            let earned: Vec<&Value> = a.iter().filter(|x| x["earned"] == true).collect();
            let last = earned.iter().map(|x| number(x.get("at"))).fold(0.0, f64::max);
            let pct = round(100.0 * earned.len() as f64 / a.len() as f64);
            let appid = crate::js::string(g.get("appid"));
            games.push(json!({ "id": format!("steam-{appid}"), "appid": g["appid"], "title": g["name"], "console": "Steam", "icon": cdn(g.get("icon")), "total": a.len(), "earned": earned.len(),
                "pct": crate::js::num(pct), "award": if pct == 100.0 { json!("mastered") } else { Value::Null }, "last": iso(last), "src": "steam" }));
            for x in earned.iter().filter(|x| number(x.get("at")) >= since) {
                recent.push(json!({ "id": format!("{appid}:{}", crate::js::string(x.get("api"))), "api": x["api"], "appid": g["appid"], "title": x["title"], "desc": x["desc"], "points": null, "badge": null,
                    "game": g["name"], "console": "Steam", "hardcore": false, "date": iso(number(x.get("at"))), "rarity": null, "src": "steam" }));
            }
        }
        recent.sort_by_key(|a| std::cmp::Reverse(crate::js::string(a.get("date"))));
        recent.truncate(60);
        // icons and world percentages for the games with recent unlocks only
        let mut seen = HashSet::new();
        let apps: Vec<Value> = recent.iter().map(|r| r["appid"].clone()).filter(|a| seen.insert(a.to_string())).take(8).collect();
        for appid in apps {
            let id = crate::js::string(Some(&appid));
            let (ic, w) = (self.icons(&id), self.world(&id));
            for r in recent.iter_mut().filter(|r| r["appid"] == appid) {
                let api = crate::js::string(r.get("api"));
                r["badge"] = cdn(ic.get(&api));
                if let Some(p) = w.get(&api).filter(|p| !p.is_null()) {
                    r["rarity"] = crate::js::num((round(number(Some(p)) * 10.0) / 10.0).max(0.1));
                }
            }
        }
        // some newer games' achievement icons are gone from Steam's servers: the game's icon instead
        for r in &mut recent {
            let fallback = games.iter().find(|g| g["appid"] == r["appid"]).map(|g| g["icon"].clone()).filter(|i| truthy(Some(i))).unwrap_or(Value::Null);
            if !truthy(r.get("badge")) {
                r["badge"] = fallback.clone();
            }
            r["fallback"] = fallback;
        }
        let me = me.as_ref();
        Ok(Some(json!({ "user": text(me.and_then(|m| m.get("name"))), "pic": me.and_then(|m| m.get("pic")).filter(|p| truthy(Some(p))).cloned().unwrap_or(Value::Null), "games": games, "recent": recent })))
    }

    /// Settings' "Test the key": the profile of the account signed in here, with `key` in place of the saved one.
    pub fn test(&self, key: &str) -> Value {
        let key = key.trim();
        if !valid(key) {
            return json!({ "ok": false, "error": "A Steam Web API key is 32 letters and digits." });
        }
        let Some(sid) = self.root.as_deref().and_then(steam_id) else { return json!({ "ok": false, "error": "No Steam account found on this PC (sign in to Steam once)." }) };
        let url = format!("{API}/ISteamUser/GetPlayerSummaries/v2/?{}", crate::js::query(&[("key", key.to_string()), ("steamids", sid)]));
        match self.get(&url) {
            Ok((_, j)) => match j.pointer("/response/players/0").filter(|p| truthy(Some(p))) {
                Some(p) => json!({ "ok": true, "user": text(p.get("personaname")) }),
                None => json!({ "ok": false, "error": "Steam answered without this account." }),
            },
            Err(e) => json!({ "ok": false, "error": e }),
        }
    }
}

/// The Trophies screen: RetroAchievements and Steam side by side, either one alone enough; `errs` are why a side
/// that was asked didn't answer.
/// RPCS3's trophy sets put in with the rest of the Trophies screen (`combine`'s answer, or an empty one), totals
/// counted again. `games` and `recent` as `rpcs3troph::all` gives them.
pub fn add_rpcs3(d: Option<Value>, games: Vec<Value>, recent: Vec<Value>) -> Value {
    let mut d = d.unwrap_or_else(|| json!({ "user": "", "pic": null, "points": 0, "softcore": 0, "games": [], "recent": [], "sources": { "ra": false, "steam": false, "steamUser": "" }, "warn": "" }));
    if games.is_empty() {
        return d;
    }
    let date = |v: &Value| -> Value { v.as_str().map_or(Value::Null, |t| json!(t.replacen('T', " ", 1).chars().take(19).collect::<String>())) };
    let mut all: Vec<Value> = crate::js::items(d.get("games")).into_iter().cloned().collect();
    all.extend(games.into_iter().map(|mut g| {
        g["last"] = date(&g["last"]);
        g
    }));
    all.sort_by_key(|a| std::cmp::Reverse(text(a.get("last"))));
    let mut rec: Vec<Value> = crate::js::items(d.get("recent")).into_iter().cloned().collect();
    rec.extend(recent.into_iter().map(|mut r| {
        r["date"] = date(&r["date"]);
        r
    }));
    rec.sort_by_key(|a| std::cmp::Reverse(text(a.get("date"))));
    rec.truncate(100);
    let award = |kinds: &[&str]| all.iter().filter(|g| g["award"].as_str().is_some_and(|k| kinds.contains(&k))).count();
    let unlocked: f64 = all.iter().map(|g| number(g.get("earned"))).sum();
    let avg = if all.is_empty() { 0.0 } else { round(all.iter().map(|g| number(g.get("pct"))).sum::<f64>() / all.len() as f64) };
    d["totals"] = json!({ "games": all.len(), "unlocked": crate::js::num(unlocked), "mastered": award(&["mastered", "completed", "platinum"]), "beaten": award(&["beaten-hardcore", "beaten-softcore"]), "avg": crate::js::num(avg) });
    d["games"] = Value::Array(all);
    d["recent"] = Value::Array(rec);
    d["sources"]["rpcs3"] = json!(true);
    d
}

pub fn combine(ra: Option<Value>, steam: Option<Value>, errs: &[String]) -> Result<Value, String> {
    if ra.is_none() && steam.is_none() {
        return Err(errs.first().cloned().unwrap_or_else(|| "Neither RetroAchievements nor Steam answered.".into()));
    }
    let date = |v: Option<&Value>| -> String { if truthy(v) { crate::js::string(v).replacen('T', " ", 1).chars().take(19).collect() } else { String::new() } };
    let tagged = |side: Option<&Value>, list: &str, src: Option<&str>, when: &str| -> Vec<Value> {
        crate::js::items(side.and_then(|s| s.get(list)))
            .into_iter()
            .map(|x| {
                let mut x = x.clone();
                if let Some(src) = src {
                    x["src"] = json!(src);
                }
                x[when] = json!(date(x.get(when)));
                x
            })
            .collect()
    };
    let mut games = tagged(ra.as_ref(), "games", Some("ra"), "last");
    games.extend(tagged(steam.as_ref(), "games", None, "last"));
    games.sort_by_key(|a| std::cmp::Reverse(text(a.get("last"))));
    let mut recent = tagged(ra.as_ref(), "recent", Some("ra"), "date");
    recent.extend(tagged(steam.as_ref(), "recent", None, "date"));
    recent.sort_by_key(|a| std::cmp::Reverse(text(a.get("date"))));
    recent.truncate(80);
    let award = |kinds: &[&str]| games.iter().filter(|g| g["award"].as_str().is_some_and(|k| kinds.contains(&k))).count();
    let pick = |k: &str| [ra.as_ref(), steam.as_ref()].into_iter().flatten().map(|s| s.get(k)).find(|v| truthy(*v)).flatten().cloned();
    let unlocked: f64 = games.iter().map(|g| number(g.get("earned"))).sum();
    let avg = if games.is_empty() { 0.0 } else { round(games.iter().map(|g| number(g.get("pct"))).sum::<f64>() / games.len() as f64) };
    let side = |k: &str| ra.as_ref().and_then(|r| r.get(k)).cloned().unwrap_or(json!(0));
    Ok(json!({ "user": pick("user").unwrap_or(json!("")), "pic": pick("pic").unwrap_or(Value::Null), "points": side("points"), "softcore": side("softcore"),
        "totals": { "games": games.len(), "unlocked": crate::js::num(unlocked), "mastered": award(&["mastered", "completed"]), "beaten": award(&["beaten-hardcore", "beaten-softcore"]), "avg": crate::js::num(avg) },
        "games": games, "recent": recent,
        "sources": { "ra": ra.is_some(), "steam": steam.is_some(), "steamUser": steam.as_ref().map_or(json!(""), |s| s.get("user").cloned().unwrap_or(json!(""))) },
        "warn": errs.first().cloned().unwrap_or_default() }))
}

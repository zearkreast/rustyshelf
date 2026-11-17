// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/screenscraper.js.
//! ScreenScraper (screenscraper.fr, API v2): box fronts, backs, spines, full wraps and disc or cartridge art by
//! region, for a game matched by its file. Optional: nothing is asked without the user's own account.
//!
//! The credentials go into an address only as it's sent. The media addresses the site answers with carry them
//! too: they're kept without (`clean`) and signed again for the download. No address, body or raw error is ever
//! given back in words. What's found is kept in the data folder's `ss-cache.json`, the Electron app's own file.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::Digest;
use url::Url;

use crate::{covers, js, lbindex};

const BASE: &str = "https://api.screenscraper.fr/api2";
const SOFT: &str = "RustyShelf";
const DAY: f64 = 86_400_000.0;
/// how long a game found, and one not found, are answered from the cache, in milliseconds
const HIT_TTL: f64 = 7.0 * DAY;
const MISS_TTL: f64 = DAY;
/// The biggest file whose checksums are taken; a bigger one (a disc image) is matched on its name and size.
pub const HASH_MAX: u64 = 64 * 1_048_576;
const MAX_GAMES: usize = 20000;
const MAX_HASH: usize = 20000;

/// ScreenScraper's number for a console's shelf (its systemesListe); PS4 and Steam aren't covered.
pub fn system_id(shelf: &str) -> Option<u32> {
    Some(match shelf {
        "gc" => 13,
        "wii" => 16,
        "wiiu" => 18,
        "sw" => 225,
        "n3ds" => 17,
        "psp" => 61,
        "psvita" => 62,
        "ps2" => 58,
        "ps3" => 59,
        "x360" => 33,
        "xbox" => 32,
        "megadrive" => 1,
        "mastersystem" => 2,
        "nes" => 3,
        "fds" => 106,
        "snes" => 4,
        "gb" => 9,
        "gbc" => 10,
        "gba" => 12,
        "n64" => 14,
        "nds" => 15,
        "gamegear" => 21,
        "dreamcast" => 23,
        "saturn" => 22,
        "pcengine" => 31,
        "neogeo" => 142,
        "arcade" => 75,
        "segacd" => 20,
        "sega32x" => 19,
        "atari2600" => 26,
        "atarilynx" => 28,
        "wonderswan" => 45,
        "dos" => 135,
        "psx" => 57,
        "ngp" => 25,
        "ngpc" => 82,
        "gamecom" => 121,
        _ => return None,
    })
}

/// The media types a part of the box is made from. "box-texture" is the whole wrap (back, spine, front): the
/// spine is cut from it.
/// Details' screenshots: the in-game screenshot and the title screen, and what kind each is.
const SHOT_MEDIA: [(&str, &str); 2] = [("ss", "play"), ("sstitle", "title")];
/// Details' trailer: the gameplay capture, its "normalized" copy first.
const VIDEO_MEDIA: [&str; 2] = ["video-normalized", "video"];

fn part_media(part: &str) -> &'static [&'static str] {
    match part {
        "front" => &["box-2D"],
        "back" => &["box-2D-back"],
        "spine" => &["box-2D-side", "box-texture"],
        "disc" => &["support-2D"],
        _ => &[],
    }
}

/// The shelves whose games come on a cartridge or a card, not a disc.
const CART: [&str; 25] =
    ["n3ds", "psvita", "sw", "sw2", "nes", "fds", "snes", "n64", "gb", "gbc", "gba", "nds", "megadrive", "mastersystem", "gamegear", "pcengine", "neogeo", "arcade", "sega32x", "atari2600", "atarilynx", "wonderswan", "ngp", "ngpc", "gamecom"];

/// A media's region as the box art's regions are named: "usa", "eur", "jpn", "world" or "".
fn region_of(r: &str) -> &'static str {
    match r.to_lowercase().as_str() {
        "us" | "cus" => "usa",
        "jp" => "jpn",
        "wor" => "world",
        "eu" | "fr" | "de" | "es" | "it" | "uk" | "nl" | "pt" | "se" | "dk" | "fi" | "no" | "pl" | "gr" | "ru" | "au" | "nz" => "eur",
        _ => "",
    }
}

/// The account as Settings has it: the user's login, and a developer login of their own if they have one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Creds {
    pub user: String,
    pub pass: String,
    pub dev_id: String,
    pub dev_pass: String,
}

impl Creds {
    /// As they're sent: a developer password alone goes with the username, a developer login alone signs in
    /// too, and without one this build's own is used, when it was built with one.
    fn settled(&self) -> Creds {
        let mut c = Creds { user: js::trim(&self.user).into(), pass: self.pass.clone(), dev_id: js::trim(&self.dev_id).into(), dev_pass: self.dev_pass.clone() };
        if !c.dev_pass.is_empty() && c.dev_id.is_empty() && !c.user.is_empty() {
            c.dev_id = c.user.clone();
        }
        if c.user.is_empty() && !c.dev_id.is_empty() {
            c.user = c.dev_id.clone();
        }
        if c.pass.is_empty() && !c.dev_pass.is_empty() {
            c.pass = c.dev_pass.clone();
        }
        if c.dev_id.is_empty() {
            if let (Some(id), Some(pass)) = (option_env!("RUSTYSHELF_SS_DEV_ID").filter(|v| !v.is_empty()), option_env!("RUSTYSHELF_SS_DEV_PASS").filter(|v| !v.is_empty())) {
                (c.dev_id, c.dev_pass) = (id.into(), pass.into());
            }
        }
        c
    }

    /// A fingerprint of the sign-in, never its values.
    fn sig(&self) -> String {
        let hash = sha2::Sha256::digest([self.user.as_str(), &self.pass, &self.dev_id, &self.dev_pass].join("\0").as_bytes());
        hash.iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    fn has_dev(&self) -> bool {
        !self.dev_id.is_empty() && !self.dev_pass.is_empty()
    }

    /// What every request carries, in the order it's written.
    fn params(&self) -> Vec<(&'static str, String)> {
        let mut p = vec![("softname", SOFT.to_string()), ("output", "json".into()), ("ssid", self.user.clone()), ("sspassword", self.pass.clone())];
        if !self.dev_id.is_empty() {
            p.extend([("devid", self.dev_id.clone()), ("devpassword", self.dev_pass.clone())]);
        }
        p
    }
}

fn api_url(file: &str, q: &[(&str, String)], c: &Creds) -> String {
    let mut pairs = c.params();
    pairs.extend(q.iter().map(|(k, v)| (*k, v.clone())));
    pairs.retain(|(_, v)| !v.is_empty());
    format!("{BASE}/{file}?{}", js::query(&pairs))
}

fn own_host(x: &Url) -> bool {
    static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(^|\.)screenscraper\.fr$").unwrap());
    x.host_str().is_some_and(|h| HOST.is_match(h))
}

/// A media address from an answer, without its credentials; None when it isn't ScreenScraper's.
pub fn clean(u: &str) -> Option<String> {
    static CRED: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?i)(devid|devpassword|softname|ssid|sspassword|output)$").unwrap());
    let mut x = Url::parse(u).ok()?;
    if !matches!(x.scheme(), "http" | "https") || !own_host(&x) || u.encode_utf16().count() > 1500 {
        return None;
    }
    let all: Vec<(String, String)> = x.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    let kept: Vec<&(String, String)> = all.iter().filter(|(k, _)| !CRED.is_match(k)).collect();
    if kept.is_empty() && !all.is_empty() {
        x.set_query(None);
    } else if kept.len() < all.len() {
        x.query_pairs_mut().clear().extend_pairs(kept);
    }
    x.set_scheme("https").ok()?;
    Some(x.to_string())
}

/// Whether an address is one of ScreenScraper's media files.
pub fn is_media(u: &str) -> bool {
    Url::parse(u).is_ok_and(|x| own_host(&x) && x.path().to_lowercase().ends_with("mediajeu.php"))
}

fn rom_type(file: &str, is_dir: bool) -> &'static str {
    static DISC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(iso|gcm|gcz|rvz|wbfs|wia|ciso|cso|chd|cue|bin|img|nkit\.iso)$").unwrap());
    if is_dir {
        "dossier"
    } else if DISC.is_match(file) {
        "iso"
    } else {
        "rom"
    }
}

/// How the site is asked: an address in (`true` with it for a media file, which may take longer), the status and
/// the body out. An error is the network's: `true` when it took too long.
pub type Fetch = Box<dyn Fn(&str, bool) -> Result<(u16, Vec<u8>), bool> + Send + Sync>;

fn over_the_net(url: &str, media: bool) -> Result<(u16, Vec<u8>), bool> {
    let agent = format!("RustyShelf/{}", crate::VERSION);
    let (status, _, body) = if media {
        crate::net::ask(url, &[("User-Agent", agent.as_str())], Duration::from_secs(45), 30_000_000)?
    } else {
        crate::net::ask(url, &[("User-Agent", agent.as_str()), ("Accept", "application/json")], Duration::from_secs(15), 8_000_000)?
    };
    Ok((status, body))
}

/// Why the site gave nothing: "nocreds", "auth", "rate", "quota", "closed", "blocked" or "net", with words for
/// the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Trouble {
    pub code: &'static str,
    pub words: String,
}

fn trouble(code: &'static str, words: &str) -> Trouble {
    Trouble { code, words: words.to_string() }
}

fn pause_words(why: &str) -> &'static str {
    match why {
        "quota" => "Today's ScreenScraper lookups are used up",
        "closed" => "ScreenScraper is closed to apps right now",
        "auth" => "The ScreenScraper login was not accepted",
        "blocked" => "ScreenScraper has blocked this version of RustyShelf",
        _ => "ScreenScraper asked us to slow down",
    }
}

/// The game a file is.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rom<'a> {
    pub file: &'a str,
    pub title: &'a str,
    /// its size when the file isn't one that can be measured here (a folder)
    pub size_bytes: u64,
}

/// One picture of a game on the site; its address is kept clean.
#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub kind: String,
    pub region: String,
    pub url: String,
    pub ext: String,
}

impl Media {
    /// As the cache keeps it.
    pub fn to_json(&self) -> Value {
        json!({ "type": self.kind, "region": self.region, "url": self.url, "ext": self.ext })
    }

    fn from_json(m: &Value) -> Option<Media> {
        let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        // cleaned again: a file edited by hand, or an older one, never carries a credential onward
        Some(Media { kind: text("type"), region: text("region"), url: clean(m.get("url")?.as_str()?)?, ext: text("ext") })
    }
}

/// The site's game a file was matched to, and its pictures.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// the site's number for the game
    pub id: Value,
    pub media: Vec<Media>,
}

/// A picture offered for one part of a game's box.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub url: String,
    /// the extension it's saved with
    pub ext: String,
    /// "spine" when the part is to be cut from this full wrap, whose own extension is `src`
    pub cut: Option<&'static str>,
    pub src: Option<String>,
    pub kind: String,
    pub region: &'static str,
    pub label: String,
}

impl Candidate {
    /// As the Electron app's Art Studio lists it.
    pub fn to_json(&self) -> Value {
        let mut c = json!({ "url": self.url, "ext": self.ext, "source": "screenscraper", "type": self.kind, "region": self.region, "edition": "original", "kind": "scan", "label": self.label });
        if let (Some(cut), Some(src)) = (self.cut, &self.src) {
            c["cut"] = json!(cut);
            c["src"] = json!(src);
        }
        c
    }
}

/// How a download went when the site answered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Got {
    Ok,
    /// the site has no such picture
    Missing,
    /// not now: no account, a pause or a refusal (worth trying another time)
    Busy,
}

/// Where the account stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub configured: bool,
    pub enabled: bool,
    /// why nothing is asked for now, or ""
    pub paused: &'static str,
    pub threads: usize,
    pub dev_login: bool,
}

#[derive(Default)]
struct Cache {
    /// "<shelf>|<file>|<size>" → [the site's game or 0, when, its media]
    games: Map<String, Value>,
    /// "<file>|<size>|<changed>" → [crc, md5, sha1]
    hashes: Map<String, Value>,
}

struct State {
    creds: Creds,
    paused_until: f64,
    pause_why: &'static str,
    /// a sign-in the site refused: not sent again until it changes
    bad_sig: String,
    /// what the account allows: requests at once, and the time between two, in milliseconds
    max_threads: usize,
    spacing: f64,
    last_at: f64,
    cache: Option<Cache>,
}

pub struct ScreenScraper {
    file: PathBuf,
    state: Mutex<State>,
    busy: (Mutex<usize>, Condvar),
    fetch: Fetch,
}

/// Until a minute into the next day by UTC, when the day's quota starts again.
fn next_utc_day() -> f64 {
    let now = crate::js::date_now();
    ((now / DAY).floor() + 1.0) * DAY - now + 60_000.0
}

/// JS `parseInt(v, 10)`.
fn parse_int(v: Option<&Value>) -> Option<i64> {
    static LEAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[+-]?[0-9]+").unwrap());
    let text = match v? {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    LEAD.find(js::trim(&text)).and_then(|m| m.as_str().parse().ok())
}

fn object_like(text: &str) -> bool {
    text.trim_start_matches(js_space).starts_with('{')
}

fn js_space(c: char) -> bool {
    js::trim(c.encode_utf8(&mut [0; 4])).is_empty()
}

fn dev_word(t: &str) -> bool {
    static DEV: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)d[ée]veloppeur|developer|devid").unwrap());
    DEV.is_match(t)
}

impl ScreenScraper {
    /// With its cache in the data folder.
    pub fn new(data_dir: &Path) -> ScreenScraper {
        ScreenScraper::with(data_dir, Box::new(over_the_net))
    }

    /// The same, asking through `fetch` (the tests' stand-in for the site).
    pub fn with(data_dir: &Path, fetch: Fetch) -> ScreenScraper {
        let state = State { creds: Creds::default(), paused_until: 0.0, pause_why: "", bad_sig: String::new(), max_threads: 1, spacing: 0.0, last_at: 0.0, cache: None };
        ScreenScraper { file: data_dir.join("ss-cache.json"), state: Mutex::new(state), busy: Default::default(), fetch }
    }

    /// The account to use. A sign-in that was refused is tried again once it has changed.
    pub fn set_creds(&self, creds: Creds) {
        let mut s = self.state.lock().unwrap();
        if s.creds != creds {
            s.creds = creds;
            s.bad_sig.clear();
            if s.pause_why == "auth" {
                (s.paused_until, s.pause_why) = (0.0, "");
            }
        }
    }

    fn creds(&self) -> Creds {
        self.state.lock().unwrap().creds.settled()
    }

    /// The account kept in Settings (`data/config.json`), or None when there's none.
    pub fn from_settings(data_dir: &Path) -> Option<ScreenScraper> {
        let cfg = crate::config::Config::load(data_dir).ok()?;
        let seal = crate::seal::key(&crate::paths::env_var);
        let k = |n: &str| cfg.api_key(n, seal.as_ref());
        let site = ScreenScraper::new(data_dir);
        site.set_creds(Creds { user: k("ssUser"), pass: k("ssPass"), dev_id: k("ssDevId"), dev_pass: k("ssDevPass") });
        site.configured().then_some(site)
    }

    pub fn configured(&self) -> bool {
        let c = self.creds();
        !c.user.is_empty() && !c.pass.is_empty()
    }

    pub fn enabled(&self) -> bool {
        self.configured() && self.creds().sig() != self.state.lock().unwrap().bad_sig
    }

    pub fn standing(&self) -> Standing {
        let (c, s) = (self.creds(), self.state.lock().unwrap());
        let configured = !c.user.is_empty() && !c.pass.is_empty();
        Standing { configured, enabled: configured && c.sig() != s.bad_sig, paused: if crate::js::date_now() < s.paused_until { s.pause_why } else { "" }, threads: s.max_threads, dev_login: c.has_dev() }
    }

    /// A clean media address with the account in it, for the download.
    fn sign(&self, u: &str) -> Option<String> {
        let mut x = Url::parse(u).ok()?;
        let mut pairs: Vec<(String, String)> = x.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        for (k, v) in self.creds().params().into_iter().filter(|(k, v)| *k != "output" && !v.is_empty()) {
            match pairs.iter().position(|p| p.0 == k) {
                Some(at) => {
                    pairs[at].1 = v;
                    let mut n = 0;
                    pairs.retain(|p| p.0 != k || { n += 1; n == 1 });
                }
                None => pairs.push((k.to_string(), v)),
            }
        }
        x.query_pairs_mut().clear().extend_pairs(pairs);
        Some(x.to_string())
    }

    fn cache<T>(&self, with: impl FnOnce(&mut Cache) -> T) -> T {
        let mut s = self.state.lock().unwrap();
        let cache = s.cache.get_or_insert_with(|| {
            let read = crate::files::read_json(&self.file);
            match read.as_ref().map(|j| (&j["games"], &j["hashes"])) {
                Some((Value::Object(games), Value::Object(hashes))) => Cache { games: games.clone(), hashes: hashes.clone() },
                _ => Cache::default(),
            }
        });
        with(cache)
    }

    /// Writes the cache, without what's gone stale and the first written of too many.
    fn save(&self) {
        let text = self.cache(|c| {
            let now = crate::js::date_now();
            c.games.retain(|_, v| now - js::number(v.get(1)) <= if js::truthy(v.get(0)) { HIT_TTL } else { MISS_TTL });
            let trim = |m: &mut Map<String, Value>, most: usize| {
                if m.len() > most {
                    *m = std::mem::take(m).into_iter().skip(m.len() - most).collect();
                }
            };
            trim(&mut c.games, MAX_GAMES);
            trim(&mut c.hashes, MAX_HASH);
            json!({ "games": c.games, "hashes": c.hashes }).to_string()
        });
        let _ = crate::files::write_atomic(&self.file, text);
    }

    fn pause(&self, ms: f64, why: &'static str) {
        let mut s = self.state.lock().unwrap();
        s.paused_until = s.paused_until.max(crate::js::date_now() + ms);
        s.pause_why = why;
    }

    /// As many requests at once as the account allows, each spaced to its rate a minute.
    fn limited<T>(&self, ask: impl FnOnce() -> T) -> T {
        {
            let mut busy = self.busy.1.wait_while(self.busy.0.lock().unwrap(), |n| *n >= self.state.lock().unwrap().max_threads).unwrap();
            *busy += 1;
        }
        let wait = {
            let (mut s, now) = (self.state.lock().unwrap(), crate::js::date_now());
            let at = now.max(s.last_at + s.spacing);
            s.last_at = at;
            at - now
        };
        if wait > 0.0 {
            std::thread::sleep(Duration::from_millis(wait as u64));
        }
        let got = ask();
        *self.busy.0.lock().unwrap() -= 1;
        self.busy.1.notify_all();
        got
    }

    /// The account's limits, as each answer reports them: how many lookups are left today, when it says.
    fn learn(&self, user: Option<&Value>) -> Option<Option<i64>> {
        let user = user.filter(|u| u.is_object())?;
        let n = |k: &str| parse_int(user.get(k));
        let (today, max) = (n("requeststoday"), n("maxrequestsperday").filter(|m| *m != 0));
        {
            let mut s = self.state.lock().unwrap();
            if let Some(threads) = n("maxthreads").filter(|t| *t != 0) {
                s.max_threads = threads.clamp(1, 8) as usize;
            }
            if let Some(a_minute) = n("maxrequestspermin").filter(|m| *m != 0) {
                s.spacing = (60_000.0 / a_minute as f64).ceil();
            }
        }
        if let (Some(today), Some(max)) = (today, max) {
            if today >= max {
                self.pause(next_utc_day(), "quota");
            }
        }
        Some(today.zip(max).map(|(today, max)| (max - today).max(0)))
    }

    /// What a status and a body's first words mean, when they mean the answer is no. The body is read for those
    /// words only.
    fn failure(&self, status: u16, text: &str, c: Option<&Creds>) -> Option<Trouble> {
        static LOGIN: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)erreur de login|identifiants|login error").unwrap());
        let t = js::slice(text, 400);
        let minutes = 60_000.0;
        Some(match status {
            401 => {
                self.pause(10.0 * minutes, "closed");
                trouble("closed", pause_words("closed"))
            }
            _ if status == 403 || LOGIN.is_match(t) => {
                if let Some(c) = c.filter(|c| c.sig() == self.creds().sig()) {
                    self.state.lock().unwrap().bad_sig = c.sig();
                }
                trouble("auth", if dev_word(t) { "The ScreenScraper developer login was not accepted" } else { pause_words("auth") })
            }
            423 => {
                self.pause(10.0 * minutes, "closed");
                trouble("closed", "ScreenScraper is down for now")
            }
            426 => {
                self.pause(DAY, "blocked");
                trouble("blocked", pause_words("blocked"))
            }
            429 => {
                self.pause(minutes, "rate");
                trouble("rate", pause_words("rate"))
            }
            _ if status == 430 || t.to_lowercase().contains("quota") => {
                self.pause(next_utc_day(), "quota");
                trouble("quota", pause_words("quota"))
            }
            431 => {
                self.pause(60.0 * minutes, "quota");
                trouble("quota", "ScreenScraper: too many unknown games today")
            }
            500.. => trouble("net", &format!("ScreenScraper answered {status}")),
            _ => return None,
        })
    }

    /// One API file's "response" and the lookups left today; None when the site has no such thing.
    fn api(&self, file: &str, q: &[(&str, String)], c: &Creds) -> Result<Option<Answer>, Trouble> {
        if c.user.is_empty() || c.pass.is_empty() {
            return Err(trouble("nocreds", "No ScreenScraper account"));
        }
        {
            let s = self.state.lock().unwrap();
            if crate::js::date_now() < s.paused_until {
                return Err(trouble(if s.pause_why.is_empty() { "rate" } else { s.pause_why }, pause_words(s.pause_why)));
            }
        }
        let (status, body) = self
            .limited(|| (self.fetch)(&api_url(file, q, c), false))
            .map_err(|slow| trouble("net", if slow { "ScreenScraper took too long to answer" } else { "Couldn't reach ScreenScraper" }))?;
        if status == 404 {
            return Ok(None);
        }
        let (text, ok) = (String::from_utf8_lossy(&body), (200..300).contains(&status));
        if let Some(f) = self.failure(status, if ok && object_like(&text) { "" } else { &text }, Some(c)) {
            return Err(f);
        }
        // 400: a request it couldn't use, such as a name with nothing to go on
        if status == 400 {
            return Ok(None);
        }
        if !ok {
            return Err(trouble("net", &format!("ScreenScraper answered {status}")));
        }
        static NOT_FOUND: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)non trouv|not found").unwrap());
        if NOT_FOUND.is_match(js::slice(&text, 200)) && !object_like(&text) {
            return Ok(None);
        }
        let j: Value = serde_json::from_str(&text).map_err(|_| trouble("net", "ScreenScraper sent something unexpected"))?;
        let Some(resp) = j.get("response").filter(|r| r.is_object() || r.is_array()) else { return Ok(None) };
        let left = self.learn(resp.get("ssuser"));
        Ok(Some((resp.clone(), left)))
    }

    /// A file's CRC32, MD5 and SHA1 when it's no bigger than `HASH_MAX`, remembered by its path, size and date.
    fn hashes(&self, file: &str, meta: &fs::Metadata) -> Option<[String; 3]> {
        if !meta.is_file() || meta.len() > HASH_MAX || meta.len() < 1 {
            return None;
        }
        let changed = meta.modified().ok().and_then(crate::ms_of).unwrap_or(0);
        let key = format!("{}|{}|{changed}", file.to_lowercase(), meta.len());
        let text = |v: &Value| v.as_str().unwrap_or("").to_string();
        if let Some(hit) = self.cache(|c| c.hashes.get(&key).map(|h| [text(&h[0]), text(&h[1]), text(&h[2])])) {
            return Some(hit);
        }
        let (mut crc, mut md5, mut sha1) = (crc32fast::Hasher::new(), <md5::Md5 as md5::Digest>::new(), sha1::Sha1::new());
        let (mut f, mut buf) = (fs::File::open(file).ok()?, vec![0u8; 1 << 20]);
        loop {
            let n = f.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            md5::Digest::update(&mut md5, &buf[..n]);
            sha1.update(&buf[..n]);
        }
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<String>();
        let out = [format!("{:08X}", crc.finalize()), hex(&md5::Digest::finalize(md5)), hex(&sha1.finalize())];
        self.cache(|c| c.hashes.insert(key, json!(out)));
        self.save();
        Some(out)
    }

    /// Which of the site's games a file is, and its pictures: by the file's checksums or its name and size (and
    /// a GameCube or Wii disc's id), then by title through the site's search, taken only on an exact or near
    /// match. `cache_only`: nothing is asked.
    pub fn lookup(&self, shelf: &str, game: &Rom, cache_only: bool) -> Result<Option<Found>, Trouble> {
        self.lookup_with(shelf, game, cache_only, false)
    }

    /// `lookup`; with `shots`, a match kept from before screenshots were kept (no 4th field) is asked again.
    pub fn lookup_with(&self, shelf: &str, game: &Rom, cache_only: bool, shots: bool) -> Result<Option<Found>, Trouble> {
        let Some(sys) = system_id(shelf).filter(|_| !game.file.is_empty()) else { return Ok(None) };
        let meta = fs::metadata(game.file).ok();
        let size = meta.as_ref().filter(|m| m.is_file()).map_or(game.size_bytes, fs::Metadata::len);
        let ck = format!("{shelf}|{}|{size}", game.file.to_lowercase());
        let now = crate::js::date_now();
        let stale = |h: &Value| shots && js::truthy(h.get(0)) && !js::truthy(h.get(3)) && !cache_only;
        let hit = self.cache(|c| c.games.get(&ck).filter(|h| now - js::number(h.get(1)) < if js::truthy(h.get(0)) { HIT_TTL } else { MISS_TTL } && !stale(h)).cloned());
        if let Some(hit) = hit {
            return Ok(js::truthy(hit.get(0)).then(|| Found { id: hit[0].clone(), media: hit.get(2).and_then(Value::as_array).map_or(Vec::new(), |l| l.iter().filter_map(Media::from_json).collect()) }));
        }
        if cache_only || !self.enabled() {
            return Ok(None);
        }
        let c = self.creds();
        let mut q = vec![("systemeid", sys.to_string()), ("romtype", rom_type(game.file, meta.as_ref().is_some_and(fs::Metadata::is_dir)).to_string()), ("romnom", js::basename(game.file)), ("romtaille", if size > 0 { size.to_string() } else { String::new() })];
        if let Some([crc, md5, sha1]) = meta.as_ref().and_then(|m| self.hashes(game.file, m)) {
            q.extend([("crc", crc), ("md5", md5), ("sha1", sha1)]);
        }
        if matches!(shelf, "gc" | "wii") {
            if let Some(id) = covers::disc_id(Path::new(game.file)) {
                q.push(("serialnum", id));
            }
        }
        let numbered = |jeu: &Value| js::truthy(jeu.get("id")) && js::string(jeu.get("id")) != "0";
        let mut jeu = self.api("jeuInfos.php", &q, &c)?.and_then(|(r, _)| r.get("jeu").filter(|j| numbered(j)).cloned());
        if jeu.is_none() && !game.title.is_empty() {
            let s = self.api("jeuRecherche.php", &[("systemeid", sys.to_string()), ("recherche", js::slice(game.title, 120).to_string())], &c)?;
            let list: Vec<Value> = s.as_ref().and_then(|(r, _)| r.get("jeux")).and_then(Value::as_array).map_or(Vec::new(), |l| l.iter().filter(|x| js::truthy(x.get("id")) && x.get("noms").is_some_and(Value::is_array)).take(30).cloned().collect());
            let named: Vec<(&Value, String)> = list.iter().flat_map(|x| x["noms"].as_array().unwrap().iter().filter(|n| js::truthy(n.get("text"))).map(move |n| (x, lbindex::norm(&js::string(n.get("text")))))).collect();
            if let Some(n) = covers::match_name(&named.iter().map(|n| n.1.as_str()).collect::<Vec<_>>(), game.title, &|_| 1) {
                let found = named[n].0;
                jeu = Some(found.clone());
                if found.get("medias").and_then(Value::as_array).is_none_or(Vec::is_empty) {
                    if let Some(whole) = self.api("jeuInfos.php", &[("systemeid", sys.to_string()), ("gameid", js::string(found.get("id")))], &c)?.and_then(|(r, _)| r.get("jeu").filter(|j| js::truthy(Some(j))).cloned()) {
                        jeu = Some(whole);
                    }
                }
            }
        }
        let out = jeu.map(|jeu| {
            let n = js::number(jeu.get("id"));
            Found { id: if n.is_nan() || n == 0.0 { json!(js::string(jeu.get("id"))) } else { js::num(n) }, media: media_of(&jeu) }
        });
        let kept = match &out {
            // 2: screenshots and videos kept too
            Some(f) => json!([f.id, crate::js::date_now(), f.media.iter().map(Media::to_json).collect::<Vec<_>>(), 2]),
            None => json!([0, crate::js::date_now()]),
        };
        self.cache(|c| c.games.insert(ck, kept));
        self.save();
        Ok(out)
    }

    /// The pictures on offer for one part of a game's box ("front", "back", "spine", "disc"). Nothing when
    /// there's no account, no match, no connection, a used-up day or a refused login. `can_cut`: a spine can be
    /// cut from a full wrap here.
    pub fn candidates(&self, shelf: &str, game: &Rom, part: &str, cache_only: bool, can_cut: bool) -> Vec<Candidate> {
        let types = part_media(part);
        if types.is_empty() || !self.configured() {
            return Vec::new();
        }
        let Ok(Some(hit)) = self.lookup(shelf, game, cache_only) else { return Vec::new() };
        let mut out = Vec::new();
        for m in hit.media.iter().filter(|m| types.contains(&m.kind.as_str())) {
            let region = region_of(&m.region);
            let place = match region {
                "usa" => "USA".to_string(),
                "eur" => "Europe".into(),
                "jpn" => "Japan".into(),
                "world" => "World".into(),
                _ if m.region != "ss" => m.region.to_uppercase(),
                _ => String::new(),
            };
            let what = match m.kind.as_str() {
                "box-2D" => "Box front",
                "box-2D-back" => "Box back",
                "box-2D-side" => "Spine",
                "box-texture" => "Cut from full wrap",
                _ if CART.contains(&shelf) => "Cartridge",
                _ => "Disc",
            };
            let label = [what, place.as_str()].into_iter().filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" · ");
            let wrap = m.kind == "box-texture";
            if wrap && !can_cut {
                continue;
            }
            out.push(Candidate {
                url: m.url.clone(),
                ext: if wrap { ".jpg".into() } else { m.ext.clone() },
                cut: wrap.then_some("spine"),
                src: wrap.then(|| m.ext.clone()),
                kind: m.kind.clone(),
                region,
                label,
            });
        }
        out
    }

    /// Details' screenshots: one of each kind (the in-game screenshot, the title screen), the best region first, with
    /// what kind it is ("play", "title"). No account, or one that isn't accepted: none. Busy (a quota, a pause,
    /// offline): an Err, to be asked again another time.
    pub fn shots(&self, shelf: &str, game: &Rom) -> Result<Vec<(String, &'static str)>, String> {
        const REGIONS: [&str; 5] = ["us", "wor", "eu", "ss", "jp"];
        if !self.configured() || system_id(shelf).is_none() {
            return Ok(Vec::new());
        }
        let hit = match self.lookup_with(shelf, game, false, true) {
            Ok(Some(hit)) => hit,
            Ok(None) => return Ok(Vec::new()),
            Err(t) if matches!(t.code, "nocreds" | "auth") => return Ok(Vec::new()),
            Err(t) => return Err(t.words),
        };
        let rank = |m: &&Media| REGIONS.iter().position(|r| *r == m.region).unwrap_or(REGIONS.len());
        Ok(SHOT_MEDIA.iter().filter_map(|(kind, what)| hit.media.iter().filter(|m| m.kind == *kind).min_by_key(rank).map(|m| (m.url.clone(), *what))).collect())
    }

    /// Fetches a media file (a clean address from `candidates`) to `dest`. An error is the network's, in words.
    pub fn download(&self, url: &str, dest: &Path) -> Result<Got, String> {
        if !is_media(url) {
            return Ok(Got::Missing);
        }
        // without an account, or with a refused one: not "missing", which would hide the picture for a week
        if !self.enabled() || crate::js::date_now() < self.state.lock().unwrap().paused_until {
            return Ok(Got::Busy);
        }
        let Some(signed) = self.sign(url) else { return Ok(Got::Missing) };
        let (status, b) = self.limited(|| (self.fetch)(&signed, true)).map_err(|slow| if slow { "ScreenScraper took too long to answer" } else { "Couldn't reach ScreenScraper" }.to_string())?;
        if status == 404 {
            return Ok(Got::Missing);
        }
        if !(200..300).contains(&status) {
            self.failure(status, "", None);
            return Ok(Got::Busy);
        }
        // a picture by its first bytes: the site answers a missing one with a short text and 200
        let picture = b.len() > 64 && (b.starts_with(&[0x89, 0x50]) || b.starts_with(&[0xff, 0xd8]) || (b.starts_with(b"RIFF") && &b[8..12] == b"WEBP"));
        if !picture {
            return Ok(Got::Missing);
        }
        crate::files::write_atomic(dest, &b).map_err(|e| e.to_string())?;
        Ok(Got::Ok)
    }

    /// Signs in: the lookups left today when the account says, or why not in a word for the screen.
    pub fn test(&self) -> Result<Option<i64>, String> {
        let c = self.creds();
        if c.user.is_empty() || c.pass.is_empty() {
            return Err("no account".into());
        }
        {
            // a fresh try, except for a day that's used up
            let mut s = self.state.lock().unwrap();
            if !matches!(s.pause_why, "quota" | "blocked") {
                (s.paused_until, s.pause_why) = (0.0, "");
            }
        }
        // refused with no developer login at all: that's what's missing, not the account
        let refused = |dev: bool| if !c.has_dev() { "no developer login" } else if dev { "developer login not accepted" } else { "not accepted" }.to_string();
        match self.api("ssuserInfos.php", &[], &c) {
            Ok(Some((r, left))) if js::truthy(r.get("ssuser")) => {
                let mut s = self.state.lock().unwrap();
                if s.bad_sig == c.sig() {
                    s.bad_sig.clear();
                }
                Ok(left.flatten())
            }
            Ok(_) => Err(refused(false)),
            Err(t) if t.code == "auth" => Err(refused(t.words.contains("developer"))),
            Err(t) => Err(t.words),
        }
    }
}

/// An answer's "response", and what its account part says is left of today's lookups (None without that
/// part, Some(None) when it doesn't say).
type Answer = (Value, Option<Option<i64>>);

/// The pictures an answer lists that a box is made from, at ScreenScraper's own addresses.
fn media_of(jeu: &Value) -> Vec<Media> {
    let (mut out, mut vids, mut shot_n) = (Vec::new(), Vec::<Media>::new(), HashMap::<String, usize>::new());
    for m in jeu.get("medias").and_then(Value::as_array).map_or(&[][..], Vec::as_slice) {
        let kind = js::string(m.get("type"));
        if !js::truthy(Some(m)) || (js::truthy(m.get("parent")) && m.get("parent") != Some(&json!("jeu"))) {
            continue;
        }
        // one of each video type, kept apart so the 80 pictures never crowd it out
        if VIDEO_MEDIA.contains(&kind.as_str()) {
            if let Some(url) = vids.iter().all(|v| v.kind != kind).then(|| clean(&js::string(m.get("url")))).flatten() {
                vids.push(Media { kind, region: String::new(), url, ext: ".mp4".into() });
            }
            continue;
        }
        let shot = SHOT_MEDIA.iter().any(|s| s.0 == kind);
        let wanted = shot || ["front", "back", "spine", "disc"].iter().any(|p| part_media(p).contains(&kind.as_str()));
        if !wanted || out.len() >= 80 {
            continue;
        }
        // a few regions of each screenshot: never crowding out the box art
        if shot {
            let n = shot_n.entry(kind.clone()).or_default();
            *n += 1;
            if *n > 4 {
                continue;
            }
        }
        let Some(url) = m.get("url").filter(|u| js::truthy(Some(u))).and_then(|u| clean(&js::string(Some(u)))) else { continue };
        let text = |k: &str| if js::truthy(m.get(k)) { js::string(m.get(k)).to_lowercase() } else { String::new() };
        let ext = match text("format").as_str() {
            "jpg" | "jpeg" => ".jpg",
            "webp" => ".webp",
            _ => ".png",
        };
        out.push(Media { kind, region: js::slice(&text("region"), 8).to_string(), url, ext: ext.into() });
    }
    out.extend(vids);
    out
}

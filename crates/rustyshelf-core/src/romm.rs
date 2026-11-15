// From sassiestrhihno's 1.82.0-beta.12 source (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0), brought into
// RustyShelf: ported from main/romm.js.
//! A RomM server's client: signing in (the device flow's QR, or an 8-digit code), its library (only what changed
//! since the last sync, kept in `romm-library.json` for when the server can't be reached), covers (downloaded once
//! into `romm-covers/`) and downloads (resumable, checked with RomM's hashes, a game of several files unpacked from
//! its zip). The files are the Electron app's.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::js;

const PAGE: usize = 1000;
const PAGE_PAUSE: Duration = Duration::from_millis(40);
const COVER_MAX_BYTES: u64 = 8 * 1024 * 1024;
const JSON_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub api: Duration,
    pub page: Duration,
    pub cover: Duration,
    pub connect: Duration,
    /// a download that receives nothing for this long is given up
    pub idle: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        let s = Duration::from_secs;
        Timeouts { api: s(15), page: s(30), cover: s(20), connect: s(20), idle: s(30) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoServer,
    NoToken,
    BadUrl,
    BadGame,
    BadCode,
    Unsupported,
    RateLimited,
    Offline(String),
    Aborted,
    Auth,
    NotFound,
    Http(u16),
    HashMismatch(String),
    NoSpace,
    Range(String),
    ZipBad(String),
    ZipUnsupported(String),
    SignedOut,
    Io(String),
}

impl Error {
    /// The Electron app's name for it ("offline", "http-404", ...).
    pub fn code(&self) -> String {
        match self {
            Error::NoServer => "no-server".into(),
            Error::NoToken => "no-token".into(),
            Error::BadUrl => "bad-url".into(),
            Error::BadGame => "bad-game".into(),
            Error::BadCode => "bad-code".into(),
            Error::Unsupported => "unsupported".into(),
            Error::RateLimited => "rate-limited".into(),
            Error::Offline(_) => "offline".into(),
            Error::Aborted => "aborted".into(),
            Error::Auth => "auth".into(),
            Error::NotFound => "not-found".into(),
            Error::Http(s) => format!("http-{s}"),
            Error::HashMismatch(_) => "hash-mismatch".into(),
            Error::NoSpace => "no-space".into(),
            Error::Range(_) => "range".into(),
            Error::ZipBad(_) => "zip-bad".into(),
            Error::ZipUnsupported(_) => "zip-unsupported".into(),
            Error::SignedOut => "signed-out".into(),
            Error::Io(_) => "error".into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Offline(why) => write!(f, "server not reachable: {why}"),
            Error::HashMismatch(m) | Error::Range(m) | Error::ZipBad(m) | Error::ZipUnsupported(m) | Error::Io(m) => f.write_str(m),
            Error::NoSpace => f.write_str("the disk is full"),
            Error::Aborted => f.write_str("cancelled"),
            e => f.write_str(&e.code()),
        }
    }
}

fn io_error(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::StorageFull {
        Error::NoSpace
    } else {
        Error::Io(e.to_string())
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn is_http(u: &str) -> bool {
    u.len() >= 7 && (u[..7].eq_ignore_ascii_case("http://") || u.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://")))
}

/// JS `num`: a finite number, or a string of digits.
fn num(v: Option<&Value>) -> Option<f64> {
    match v? {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
        _ => None,
    }
}

fn unum(v: Option<&Value>) -> Option<u64> {
    num(v).filter(|n| *n >= 0.0).map(|n| n as u64)
}

/// A string field's text, `null` for an empty one (JS `v || null`).
fn opt_text(v: Option<&Value>) -> Option<String> {
    js::truthy(v).then(|| js::string(v))
}

// ---------- dates

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `Date.parse` of an ISO 8601 date (RomM's `updated_at`), in ms; 0 when it isn't one. A time without a zone is
/// taken as UTC.
fn parse_iso(s: &str) -> i64 {
    static ISO: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?(Z|[+-]\d{2}:?\d{2})?)?$").unwrap()
    });
    let Some(c) = ISO.captures(s.trim()) else { return 0 };
    let n = |i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
    let ms = c.get(7).map_or(0, |f| {
        let d = f.as_str();
        (d.to_string() + "00").get(..3).and_then(|x| x.parse().ok()).unwrap_or(0)
    });
    let offset = c.get(8).map_or(0, |z| {
        let z = z.as_str();
        if z == "Z" {
            return 0;
        }
        let digits: String = z[1..].chars().filter(char::is_ascii_digit).collect();
        let mins = digits[..2].parse::<i64>().unwrap_or(0) * 60 + digits[2..].parse::<i64>().unwrap_or(0);
        if z.starts_with('-') { -mins } else { mins }
    });
    let days = days_from_civil(n(1), n(2), n(3));
    ((days * 86_400 + n(4) * 3600 + n(5) * 60 + n(6)) - offset * 60) * 1000 + ms
}

/// `new Date(ms).toISOString()`.
fn iso(ms: u64) -> String {
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// A release date as "1990-01-02": RomM gives ms, seconds, or a date.
fn date_of(v: Option<&Value>) -> Option<String> {
    let mut t = match num(v) {
        Some(t) => t,
        None => parse_iso(v.and_then(Value::as_str)?) as f64,
    };
    if t == 0.0 || !t.is_finite() {
        return None;
    }
    if t < 1e11 {
        t *= 1000.0;
    }
    let (y, m, d) = civil_from_days((t / 86_400_000.0).floor() as i64);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

// ---------- server addresses

/// An address typed without http:// or https://: https, except a server on the home network (a private or loopback
/// IP, localhost, a .local / .lan / .home / .internal name or a one-word name), which is usually plain http.
pub fn with_scheme(u: &str) -> String {
    static TAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[/?#].*$").unwrap());
    static PORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":\d+$").unwrap());
    static PRIVATE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(10\.|127\.|192\.168\.|172\.(1[6-9]|2\d|3[01])\.|169\.254\.)").unwrap());
    static HOME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.(local|lan|home|internal)$").unwrap());
    let u = u.trim();
    if u.is_empty() || is_http(u) {
        return u.to_string();
    }
    let host = TAIL.replace(u, "");
    let host = PORT.replace(&host, "");
    let host = host.trim_start_matches('[').trim_end_matches(']').to_lowercase();
    let home = PRIVATE.is_match(&host) || host == "localhost" || host == "::1" || HOME.is_match(&host) || !host.contains('.');
    format!("{}{u}", if home { "http://" } else { "https://" })
}

/// The origin an address leads to ("" for none): the sign-in token is only ever sent there.
pub fn origin_of(url: &str) -> String {
    let u = with_scheme(url);
    if u.is_empty() {
        return String::new();
    }
    url::Url::parse(&u).map(|p| p.origin().ascii_serialization()).unwrap_or_default()
}

// ---------- RomM platform -> RustyShelf console

/// RomM's own slugs (IGDB-style) and the usual folder names. Anything not here is matched against the consoles'
/// ROM folder names.
pub const SLUGS: &[(&str, &[&str])] = &[
    ("gc", &["ngc", "gamecube", "gc", "gcn", "nintendo-gamecube"]),
    ("wii", &["wii"]),
    ("wiiu", &["wiiu", "wii-u"]),
    ("sw", &["switch", "nintendo-switch", "nsw"]),
    ("sw2", &["switch-2", "switch2", "nintendo-switch-2"]),
    ("psp", &["psp", "playstation-portable"]),
    ("ps2", &["ps2", "playstation-2"]),
    ("ps3", &["ps3", "playstation-3"]),
    ("ps4", &["ps4", "ps4--1", "playstation-4"]),
    ("ps5", &["ps5", "playstation-5"]),
    ("x360", &["xbox360", "xbox-360", "x360"]),
    ("n3ds", &["3ds", "n3ds", "nintendo-3ds", "new-nintendo-3ds"]),
    ("psx", &["ps", "psx", "ps1", "playstation", "playstation-1"]),
    ("nes", &["nes", "famicom"]),
    ("fds", &["fds", "famicom-disk-system", "famicomdisk"]),
    ("snes", &["snes", "sfam", "super-famicom", "super-nintendo"]),
    ("n64", &["n64", "nintendo-64"]),
    ("gb", &["gb", "gameboy", "game-boy"]),
    ("gbc", &["gbc", "gameboy-color", "game-boy-color"]),
    ("gba", &["gba", "gameboy-advance", "game-boy-advance"]),
    ("nds", &["nds", "nintendo-ds", "nintendo-dsi", "dsi"]),
    ("megadrive", &["genesis-slash-megadrive", "genesis", "megadrive", "mega-drive", "md", "sega-genesis"]),
    ("mastersystem", &["sms", "mastersystem", "master-system", "sega-master-system"]),
    ("gamegear", &["gamegear", "game-gear", "gg", "sega-game-gear"]),
    ("dreamcast", &["dc", "dreamcast", "sega-dreamcast"]),
    ("saturn", &["saturn", "sega-saturn"]),
    (
        "pcengine",
        &["turbografx16--1", "turbografx-16-slash-pc-engine-cd", "tg16", "turbografx16", "turbografx-16", "pce", "pcengine", "pc-engine", "pc-engine-cd", "pcenginecd", "tg-cd"],
    ),
    ("neogeo", &["neogeoaes", "neogeomvs", "neo-geo", "neogeo", "neo-geo-aes", "neo-geo-mvs"]),
    ("arcade", &["arcade", "mame", "fbneo", "fba", "cps1", "cps2", "cps3"]),
    ("segacd", &["segacd", "sega-cd", "mega-cd", "megacd", "sega-cd-32x"]),
    ("sega32x", &["sega32", "sega32x", "sega-32x", "32x"]),
    ("atari2600", &["atari2600", "atari-2600", "a2600"]),
    ("atarilynx", &["lynx", "atari-lynx", "atarilynx"]),
    ("wonderswan", &["wonderswan", "wonderswan-color", "wonderswancolor", "wsc", "ws"]),
    ("dos", &["dos", "ms-dos", "msdos", "pc-dos"]),
];

fn norm_folder(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '_' | '-' | '.') && !c.is_whitespace()).collect()
}

/// The console a RomM platform's games go on (its `slug`, else its `fs_slug`); None for one RustyShelf has no shelf for.
pub fn map_slug(slug: &str, fs_slug: &str) -> Option<String> {
    static SLUG: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
        let mut m = HashMap::new();
        for (id, list) in SLUGS {
            for s in *list {
                m.entry(*s).or_insert(*id);
            }
        }
        m
    });
    static FOLDER: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
        let mut m = HashMap::new();
        for c in &crate::systems::systems().consoles {
            for n in std::iter::once(&c.id).chain(&c.rom_folder_names) {
                let k = norm_folder(n);
                if !k.is_empty() {
                    m.entry(k).or_insert_with(|| c.id.clone());
                }
            }
        }
        m
    });
    let (slug, fs_slug) = (slug.trim().to_lowercase(), fs_slug.trim().to_lowercase());
    SLUG.get(slug.as_str())
        .or_else(|| SLUG.get(fs_slug.as_str()))
        .map(|s| s.to_string())
        .or_else(|| FOLDER.get(&norm_folder(&slug)).cloned())
        .or_else(|| FOLDER.get(&norm_folder(&fs_slug)).cloned())
}

fn map_platform(p: &Value) -> Option<String> {
    map_slug(&js::text(p.get("slug")), &js::text(p.get("fs_slug")))
}

// ---------- the library

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RomFile {
    pub name: String,
    pub size: Option<u64>,
    pub crc: Option<String>,
    pub md5: Option<String>,
    pub sha1: Option<String>,
}

/// A RomM game as `romm-library.json` keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Game {
    pub id: String,
    pub romm_id: u64,
    pub platform_id: u64,
    pub console_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub size: Option<u64>,
    pub files: Vec<RomFile>,
    pub multi: bool,
    pub summary: Option<String>,
    pub released: Option<String>,
    /// "Adventure;Puzzle", as every other game's genres
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<String>,
    pub regions: Vec<Value>,
    pub cover_url: Option<String>,
    pub updated_at: Option<String>,
    /// the cover downloaded into romm-covers (never saved: the folder says)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PlatformRec {
    id: u64,
    slug: Option<String>,
    fs_slug: Option<String>,
    name: Option<String>,
    console_id: String,
    since: Option<String>,
    count: usize,
}

/// The platforms RustyShelf has no shelf for, and their games.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skipped {
    pub platforms: u64,
    pub games: u64,
    pub slugs: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cache {
    v: u32,
    server: String,
    at: Option<String>,
    platforms: BTreeMap<u64, PlatformRec>,
    games: BTreeMap<u64, Game>,
    skipped: Option<Skipped>,
}

/// The library as the shelf takes it: each console's games by title.
#[derive(Debug, Clone, Default)]
pub struct Shown {
    pub consoles: BTreeMap<String, Vec<Game>>,
    pub skipped: Skipped,
    /// when it was last synced (ISO)
    pub at: Option<String>,
    pub count: usize,
    /// the server couldn't be reached: this is the saved copy
    pub offline: bool,
    pub error: Option<Error>,
    pub fetched: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncProgress {
    Platform { platform: String, index: usize, of: usize, fetched: usize },
    Roms { platform: String, index: usize, of: usize, fetched: usize },
    Done { fetched: usize, removed: usize },
}

fn cover_url_of(it: &Value, base: &str) -> Option<String> {
    static TS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[?&]ts=").unwrap());
    let p = js::text(js::first(&[it.get("path_cover_large"), it.get("path_cover_small")]));
    if !p.is_empty() {
        let mut p = if is_http(&p) {
            p
        } else {
            let rel = p.trim_start_matches('/');
            let rel = if rel.get(..7).is_some_and(|a| a.eq_ignore_ascii_case("assets/")) { rel.to_string() } else { format!("assets/romm/resources/{rel}") };
            format!("{base}/{rel}")
        };
        let ts = parse_iso(&js::text(it.get("updated_at")));
        if ts != 0 && !TS.is_match(&p) {
            p.push_str(if p.contains('?') { "&" } else { "?" });
            p.push_str(&format!("ts={ts}"));
        }
        return Some(p);
    }
    let u = js::text(it.get("url_cover"));
    if is_http(&u) {
        Some(u)
    } else if u.starts_with("//") {
        Some(format!("https:{u}"))
    } else {
        None
    }
}

fn to_game(it: &Value, platform_id: u64, console_id: &str, base: &str) -> Game {
    static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.[a-z0-9]{1,5}$").unwrap());
    let empty = json!({});
    let md = it.get("metadatum").filter(|m| js::truthy(Some(m))).unwrap_or(&empty);
    let mut files: Vec<RomFile> = js::items(it.get("files"))
        .into_iter()
        .map(|f| RomFile {
            name: js::text(js::first(&[f.get("file_name"), f.get("name")])),
            size: unum(f.get("file_size_bytes")),
            crc: opt_text(f.get("crc_hash")),
            md5: opt_text(f.get("md5_hash")),
            sha1: opt_text(f.get("sha1_hash")),
        })
        .collect();
    let multi = it.get("has_multiple_files").and_then(Value::as_bool).unwrap_or(files.len() > 1);
    if !multi && files.is_empty() {
        files.push(RomFile {
            name: js::text(it.get("fs_name")),
            size: unum(it.get("fs_size_bytes")),
            crc: opt_text(it.get("crc_hash")),
            md5: opt_text(it.get("md5_hash")),
            sha1: opt_text(it.get("sha1_hash")),
        });
    }
    let title = match opt_text(it.get("name")) {
        Some(n) => n,
        None => EXT.replace(&js::text(js::first(&[it.get("fs_name_no_ext"), it.get("fs_name")])), "").into_owned(),
    };
    let size = unum(it.get("fs_size_bytes")).filter(|s| *s != 0).or_else(|| Some(files.iter().filter_map(|f| f.size).sum::<u64>()).filter(|s| *s != 0));
    let genres = [md.get("genres"), it.get("genres")].into_iter().flatten().find(|g| g.is_array());
    let genres: Vec<String> = genres.map(|g| js::items(Some(g)).into_iter().map(|x| js::string(Some(x))).filter(|s| !s.is_empty()).collect()).unwrap_or_default();
    let romm_id = unum(it.get("id")).unwrap_or(0);
    Game {
        id: format!("romm-{romm_id}"),
        romm_id,
        platform_id,
        console_id: console_id.to_string(),
        title,
        file: js::str_of(it.get("fs_name")).map(str::to_string),
        size,
        files,
        multi,
        summary: opt_text(it.get("summary")),
        released: date_of(if md.get("first_release_date").is_some_and(|v| !v.is_null()) { md.get("first_release_date") } else { it.get("first_release_date") }),
        genres: (!genres.is_empty()).then(|| genres.join(";")),
        regions: it.get("regions").and_then(Value::as_array).cloned().unwrap_or_default(),
        cover_url: cover_url_of(it, base),
        updated_at: opt_text(it.get("updated_at")),
        cover: None,
    }
}

// ---------- hashes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algo {
    Sha1,
    Md5,
    Crc,
}

impl Algo {
    pub fn name(self) -> &'static str {
        match self {
            Algo::Sha1 => "sha1",
            Algo::Md5 => "md5",
            Algo::Crc => "crc",
        }
    }
}

/// The strongest hash RomM gave for a file: sha1, else md5, else crc32.
fn algo_for(want: Option<&RomFile>) -> Option<Algo> {
    let w = want?;
    let has = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.is_empty());
    if has(&w.sha1) {
        Some(Algo::Sha1)
    } else if has(&w.md5) {
        Some(Algo::Md5)
    } else if has(&w.crc) {
        Some(Algo::Crc)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hashed {
    pub size: u64,
    pub algo: Option<Algo>,
    pub value: Option<String>,
}

enum HashState {
    Sha1(sha1::Sha1),
    Md5(md5::Md5),
    Crc(crc32fast::Hasher),
    None,
}

struct Hasher {
    size: u64,
    algo: Option<Algo>,
    st: HashState,
}

impl Hasher {
    fn new(algo: Option<Algo>) -> Self {
        let st = match algo {
            Some(Algo::Sha1) => HashState::Sha1(<sha1::Sha1 as sha1::Digest>::new()),
            Some(Algo::Md5) => HashState::Md5(<md5::Md5 as md5::Digest>::new()),
            Some(Algo::Crc) => HashState::Crc(crc32fast::Hasher::new()),
            None => HashState::None,
        };
        Hasher { size: 0, algo, st }
    }

    fn update(&mut self, b: &[u8]) {
        self.size += b.len() as u64;
        match &mut self.st {
            HashState::Sha1(h) => sha1::Digest::update(h, b),
            HashState::Md5(h) => md5::Digest::update(h, b),
            HashState::Crc(h) => h.update(b),
            HashState::None => {}
        }
    }

    fn result(self) -> Hashed {
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let value = match self.st {
            HashState::Sha1(h) => Some(hex(&sha1::Digest::finalize(h))),
            HashState::Md5(h) => Some(hex(&md5::Digest::finalize(h))),
            HashState::Crc(h) => Some(format!("{:08x}", h.finalize())),
            HashState::None => None,
        };
        Hashed { size: self.size, algo: self.algo, value }
    }
}

/// JS `parseInt(s, 16) >>> 0`.
fn hex_u32(s: &str) -> u32 {
    let digits: String = s.trim().chars().take_while(char::is_ascii_hexdigit).collect();
    digits.chars().fold(0u32, |a, c| a.wrapping_mul(16).wrapping_add(c.to_digit(16).unwrap_or(0)))
}

fn matches(want: &RomFile, got: &Hashed) -> bool {
    if want.size.is_some_and(|s| s != got.size) {
        return false;
    }
    let Some(algo) = got.algo else { return true };
    let w = match algo {
        Algo::Sha1 => &want.sha1,
        Algo::Md5 => &want.md5,
        Algo::Crc => &want.crc,
    };
    let w = w.as_deref().unwrap_or("").trim().to_lowercase();
    let v = got.value.as_deref().unwrap_or("");
    if algo == Algo::Crc {
        hex_u32(&w) == hex_u32(v)
    } else {
        w == v
    }
}

fn hash_file(file: &Path, algo: Option<Algo>, cancel: Option<&AtomicBool>) -> Result<Hashed, Error> {
    let mut f = fs::File::open(file).map_err(io_error)?;
    let (mut h, mut buf) = (Hasher::new(algo), vec![0u8; 1 << 20]);
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Aborted);
        }
        let n = f.read(&mut buf).map_err(io_error)?;
        if n == 0 {
            return Ok(h.result());
        }
        h.update(&buf[..n]);
    }
}

// ---------- zip (RomM sends a game made of several files as one zip)

#[derive(Debug)]
struct ZipEntry {
    name: String,
    method: u16,
    comp: u64,
    unc: u64,
    data: u64,
    encrypted: bool,
}

fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn le64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn read_at(f: &mut fs::File, pos: u64, len: usize) -> Result<Vec<u8>, Error> {
    let mut b = vec![0u8; len];
    f.seek(SeekFrom::Start(pos)).map_err(io_error)?;
    f.read_exact(&mut b).map_err(|_| Error::ZipBad("zip: short read".into()))?;
    Ok(b)
}

fn zip_entries(zip: &Path) -> Result<Vec<ZipEntry>, Error> {
    let bad = |m: &str| Error::ZipBad(m.into());
    let mut f = fs::File::open(zip).map_err(io_error)?;
    let size = f.metadata().map_err(io_error)?.len();
    let tail_len = size.min(65_557);
    let tail = read_at(&mut f, size - tail_len, tail_len as usize)?;
    let e = (0..tail.len().saturating_sub(21)).rev().find(|&i| le32(&tail, i) == 0x0605_4b50).ok_or_else(|| bad("zip: no directory"))?;
    let (mut count, mut cd_size, mut cd_off) = (u64::from(le16(&tail, e + 10)), u64::from(le32(&tail, e + 12)), u64::from(le32(&tail, e + 16)));
    if count == 0xffff || cd_size == 0xffff_ffff || cd_off == 0xffff_ffff {
        if e < 20 || le32(&tail, e - 20) != 0x0706_4b50 {
            return Err(bad("zip64: no locator"));
        }
        let z64 = read_at(&mut f, le64(&tail, e - 20 + 8), 56)?;
        if le32(&z64, 0) != 0x0606_4b50 {
            return Err(bad("zip64: no directory"));
        }
        (count, cd_size, cd_off) = (le64(&z64, 32), le64(&z64, 40), le64(&z64, 48));
    }
    if cd_off.saturating_add(cd_size) > size {
        return Err(bad("zip: short read"));
    }
    let cd = read_at(&mut f, cd_off, cd_size as usize)?;
    let mut list = Vec::new();
    let mut p = 0usize;
    for _ in 0..count {
        if p + 46 > cd.len() || le32(&cd, p) != 0x0201_4b50 {
            return Err(bad("zip: bad entry"));
        }
        let (flags, method) = (le16(&cd, p + 8), le16(&cd, p + 10));
        let (mut comp, mut unc) = (u64::from(le32(&cd, p + 20)), u64::from(le32(&cd, p + 24)));
        let (n_len, x_len, c_len) = (le16(&cd, p + 28) as usize, le16(&cd, p + 30) as usize, le16(&cd, p + 32) as usize);
        let mut local = u64::from(le32(&cd, p + 42));
        if p + 46 + n_len + x_len > cd.len() {
            return Err(bad("zip: bad entry"));
        }
        let raw = &cd[p + 46..p + 46 + n_len];
        let name = if flags & 0x800 != 0 { String::from_utf8_lossy(raw).into_owned() } else { raw.iter().map(|&b| b as char).collect() };
        let (mut x, end) = (p + 46 + n_len, p + 46 + n_len + x_len);
        while x + 4 <= end {
            let (id, len) = (le16(&cd, x), le16(&cd, x + 2) as usize);
            if id == 0x0001 {
                let mut q = x + 4;
                for v in [&mut unc, &mut comp, &mut local] {
                    if *v == 0xffff_ffff && q + 8 <= end {
                        *v = le64(&cd, q);
                        q += 8;
                    }
                }
            }
            x += 4 + len;
        }
        list.push(ZipEntry { name, method, comp, unc, data: local, encrypted: flags & 1 != 0 });
        p += 46 + n_len + x_len + c_len;
    }
    for en in &mut list {
        let lh = read_at(&mut f, en.data, 30)?;
        if le32(&lh, 0) != 0x0403_4b50 {
            return Err(bad("zip: bad local header"));
        }
        en.data += 30 + u64::from(le16(&lh, 26)) + u64::from(le16(&lh, 28));
    }
    Ok(list)
}

/// A file unpacked from a zip, hashed on the way.
#[derive(Debug, Clone)]
pub struct Unpacked {
    /// its path in the zip, with `/`
    pub name: String,
    pub file: PathBuf,
    pub got: Hashed,
}

/// Unpacks `zip` into `out_dir` (nothing outside it), hashing each file with the hash its RomM entry has.
pub fn unzip(zip: &Path, out_dir: &Path, want_for: &dyn Fn(&str) -> Option<RomFile>, on_progress: &mut dyn FnMut(DownloadProgress), cancel: Option<&AtomicBool>) -> Result<Vec<Unpacked>, Error> {
    let list = zip_entries(zip)?;
    let total: u64 = list.iter().map(|e| e.unc).sum();
    let mut done = 0u64;
    let mut tick = Instant::now() - Duration::from_secs(1);
    let mut out = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    for en in &list {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(Error::Aborted);
        }
        let rel = en.name.replace('\\', "/");
        if rel.ends_with('/') {
            continue;
        }
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        let drive = parts.first().is_some_and(|p| p.len() >= 2 && p.as_bytes()[0].is_ascii_alphabetic() && p.as_bytes()[1] == b':');
        if parts.is_empty() || parts.contains(&"..") || drive {
            return Err(Error::ZipBad("zip: unsafe path".into()));
        }
        let dest = parts.iter().fold(out_dir.to_path_buf(), |d, p| d.join(safe_name(p)));
        if en.encrypted || (en.method != 0 && en.method != 8) {
            return Err(Error::ZipUnsupported(format!("zip: method {}", en.method)));
        }
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).map_err(io_error)?;
        }
        let want = want_for(parts[parts.len() - 1]);
        let mut h = Hasher::new(algo_for(want.as_ref()).or(Some(Algo::Crc)));
        let mut src = fs::File::open(zip).map_err(io_error)?;
        src.seek(SeekFrom::Start(en.data)).map_err(io_error)?;
        let raw = src.take(en.comp);
        let mut reader: Box<dyn Read> = if en.method == 8 && en.comp > 0 { Box::new(flate2::read::DeflateDecoder::new(raw)) } else { Box::new(raw) };
        let mut w = std::io::BufWriter::new(fs::File::create(&dest).map_err(io_error)?);
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Err(Error::Aborted);
            }
            let n = reader.read(&mut buf).map_err(|e| Error::ZipBad(format!("zip: {e}")))?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
            w.write_all(&buf[..n]).map_err(io_error)?;
            done += n as u64;
            if tick.elapsed() > Duration::from_millis(200) {
                tick = Instant::now();
                on_progress(DownloadProgress { phase: Phase::Extract, received: done, total: Some(total) });
            }
        }
        w.flush().map_err(io_error)?;
        let got = h.result();
        if got.size != en.unc {
            return Err(Error::ZipBad("zip: size differs".into()));
        }
        out.push(Unpacked { name: parts.join("/"), file: dest, got });
    }
    on_progress(DownloadProgress { phase: Phase::Extract, received: total, total: Some(total) });
    Ok(out)
}

// ---------- downloads

/// A file name that's safe on every system, from RomM's.
pub fn safe_name(n: &str) -> String {
    let n = n.replace('\\', "/");
    let base = n.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let s: String = base.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20 { '_' } else { c }).collect();
    let s = s.trim_end_matches(['.', ' ']);
    if s.is_empty() || s == "." || s == ".." {
        "game".into()
    } else {
        s.to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Download,
    Verify,
    Extract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub phase: Phase,
    pub received: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Downloaded {
    /// the file to start
    pub file: PathBuf,
    /// a game of several files: the folder it was unpacked into
    pub dir: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub multi: bool,
    pub already: bool,
    pub extracted: bool,
    /// "sha1", "md5", "crc", "size", "files" or "zip"
    pub verified: Option<String>,
    pub resumed: bool,
    /// kept as a zip, and why
    pub note: Option<String>,
}

fn size_of(f: &Path) -> Option<u64> {
    fs::metadata(f).ok().filter(|m| m.is_file()).map(|m| m.len())
}

struct LocalFile {
    name: String,
    file: PathBuf,
    size: u64,
}

fn launch_file(files: &[LocalFile]) -> PathBuf {
    const ORDER: [&str; 12] = ["m3u", "cue", "gdi", "ccd", "cdi", "chd", "iso", "rvz", "wbfs", "wua", "nsp", "xci"];
    let ext = |f: &LocalFile| f.file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    for e in ORDER {
        if let Some(hit) = files.iter().find(|f| ext(f) == e && !f.name.contains('/')) {
            return hit.file.clone();
        }
    }
    let mut by_size: Vec<&LocalFile> = files.iter().collect();
    by_size.sort_by_key(|f| std::cmp::Reverse(f.size));
    by_size.first().map(|f| f.file.clone()).unwrap_or_default()
}

fn multi_done(game: &Game, out_dir: &Path) -> Option<Vec<LocalFile>> {
    if game.files.is_empty() {
        return None;
    }
    game.files
        .iter()
        .map(|f| {
            let p = out_dir.join(safe_name(&f.name));
            let s = size_of(&p)?;
            (f.size.is_none_or(|w| w == s)).then_some(LocalFile { name: f.name.clone(), file: p, size: s })
        })
        .collect()
}

fn free_space_ok(dir: &Path, need: u64) -> bool {
    if need == 0 {
        return true;
    }
    crate::storage::space(&dir.to_string_lossy()).0.is_none_or(|free| free > need + 64 * 1024 * 1024)
}

fn stem_ext(name: &str) -> (String, String) {
    let ext = js::extname(name);
    (name[..name.len() - ext.len()].to_string(), ext)
}

// ---------- TLS without a certificate check ("Skip certificate check", for a home server's own certificate)

#[derive(Debug)]
struct AnyCert(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn agent(skip_tls: bool, t: &Timeouts) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new()
        .user_agent("RustyShelf")
        .timeout_connect(t.connect)
        .timeout_read(t.idle)
        .redirects(5)
        .redirect_auth_headers(ureq::RedirectAuthHeaders::SameHost);
    if skip_tls {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        if let Ok(cfg) = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions() {
            b = b.tls_config(Arc::new(cfg.dangerous().with_custom_certificate_verifier(Arc::new(AnyCert(provider))).with_no_client_auth()));
        }
    }
    b.build()
}

// ---------- requests

struct Opts<'a> {
    method: &'a str,
    json: Option<Value>,
    auth: bool,
    timeout: Duration,
    buffer: bool,
    max: u64,
    headers: Vec<(&'a str, String)>,
    cancel: Option<&'a AtomicBool>,
}

impl<'a> Opts<'a> {
    fn get(timeout: Duration) -> Self {
        Opts { method: "GET", json: None, auth: true, timeout, buffer: false, max: JSON_MAX_BYTES, headers: Vec::new(), cancel: None }
    }
    fn post(timeout: Duration, body: Value) -> Self {
        Opts { method: "POST", json: Some(body), auth: false, ..Opts::get(timeout) }
    }
}

struct Resp {
    status: u16,
    json: Option<Value>,
    bytes: Vec<u8>,
    too_big: bool,
}

impl Resp {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status) && !self.too_big
    }
    fn json_or_empty(&self) -> Value {
        self.json.clone().unwrap_or_else(|| json!({}))
    }
}

fn send(req: ureq::Request, body: Option<&Value>) -> Result<ureq::Response, Error> {
    let r = match body {
        Some(j) => req.set("Content-Type", "application/json").send_string(&j.to_string()),
        None => req.call(),
    };
    match r {
        Ok(r) | Err(ureq::Error::Status(_, r)) => Ok(r),
        Err(ureq::Error::Transport(t)) => Err(Error::Offline(t.to_string())),
    }
}

fn header_u64(r: &ureq::Response, name: &str) -> Option<u64> {
    r.header(name).and_then(|v| v.trim().parse().ok())
}

// ---------- the client

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairStart {
    pub user_code: String,
    /// the page the QR leads to, on the server
    pub qr_url: String,
    /// ms since 1970
    pub expires_at: u64,
    /// seconds between polls
    pub interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// not yet: ask again in `wait` ms
    Pending { wait: u64, error: Option<Error> },
    Signed { token: String },
    Denied,
    Expired,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Test {
    pub version: Option<String>,
    /// the number of platforms the signed-in call saw
    pub outcome: Result<usize, Error>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub server: String,
    pub has_token: bool,
    pub pairing: bool,
    pub syncing: bool,
    pub downloads: Vec<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CoverRun {
    /// game id -> its cover
    pub paths: HashMap<String, PathBuf>,
    pub downloaded: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
struct Pair {
    serial: u64,
    device_code: String,
    interval: u64,
    next_at: u64,
    expires_at: u64,
}

struct State {
    base: String,
    origin: String,
    skip_tls: bool,
    token: Option<String>,
    pair: Option<Pair>,
    cache: Option<Cache>,
    device_id: Option<String>,
    bad_covers: HashSet<String>,
    t: Timeouts,
    time_scale: f64,
    agent: Option<ureq::Agent>,
}

pub struct Romm {
    data_dir: PathBuf,
    s: Mutex<State>,
    sync_lock: Mutex<()>,
    syncing: AtomicBool,
    active: Mutex<HashSet<u64>>,
    active_done: Condvar,
    sign_outs: AtomicU64,
    pairs: AtomicU64,
}

fn scaled(ms: u64, scale: f64) -> u64 {
    (ms as f64 * scale) as u64
}

fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// The server's reason in an error answer: {"error":"slow_down"} (OAuth style) or {"detail":"slow_down"} (FastAPI).
fn reason_of(j: &Value) -> String {
    if !j.is_object() {
        return String::new();
    }
    let d = j.get("detail");
    let v = if js::truthy(j.get("error")) {
        j.get("error")
    } else if d.is_some_and(Value::is_object) {
        js::first(&[d.and_then(|d| d.get("error")), d.and_then(|d| d.get("code"))])
    } else {
        d
    };
    js::text(v).to_lowercase()
}

impl Romm {
    pub fn new(data_dir: &Path) -> Self {
        Romm {
            data_dir: data_dir.to_path_buf(),
            s: Mutex::new(State {
                base: String::new(),
                origin: String::new(),
                skip_tls: false,
                token: None,
                pair: None,
                cache: None,
                device_id: None,
                bad_covers: HashSet::new(),
                t: Timeouts::default(),
                time_scale: 1.0,
                agent: None,
            }),
            sync_lock: Mutex::new(()),
            syncing: AtomicBool::new(false),
            active: Mutex::new(HashSet::new()),
            active_done: Condvar::new(),
            sign_outs: AtomicU64::new(0),
            pairs: AtomicU64::new(0),
        }
    }

    fn st(&self) -> MutexGuard<'_, State> {
        self.s.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[doc(hidden)]
    pub fn set_timeouts(&self, t: Timeouts) {
        let mut s = self.st();
        s.t = t;
        s.agent = None;
    }

    /// For the tests: the pairing's waits scaled down.
    #[doc(hidden)]
    pub fn set_time_scale(&self, f: f64) {
        self.st().time_scale = f;
    }

    fn cache_file(&self) -> PathBuf {
        self.data_dir.join("romm-library.json")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.data_dir.join("romm-covers")
    }

    /// The server's address, cleaned ("" for none: RomM off); a trailing /api is dropped.
    pub fn set_server(&self, url: &str, skip_tls_verify: bool) -> Result<String, Error> {
        let mut s = self.st();
        let u = url.trim();
        if u.is_empty() {
            (s.base, s.origin, s.pair, s.cache) = (String::new(), String::new(), None, None);
            return Ok(String::new());
        }
        let p = url::Url::parse(&with_scheme(u)).map_err(|_| Error::BadUrl)?;
        let origin = p.origin().ascii_serialization();
        static API: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)/api$").unwrap());
        let path = API.replace(p.path().trim_end_matches('/'), "").into_owned();
        let nb = format!("{origin}{path}");
        if nb != s.base {
            s.pair = None;
            s.cache = None;
        }
        s.base = nb.clone();
        s.origin = origin;
        s.skip_tls = skip_tls_verify;
        s.agent = None;
        Ok(nb)
    }

    /// A pairing in progress is only dropped when the token really changes.
    pub fn set_token(&self, t: Option<&str>) {
        let mut s = self.st();
        let nt = t.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        if nt != s.token {
            s.pair = None;
        }
        s.token = nt;
    }

    pub fn status(&self) -> Status {
        let s = self.st();
        let mut downloads: Vec<u64> = self.active.lock().map(|a| a.iter().copied().collect()).unwrap_or_default();
        downloads.sort_unstable();
        Status { server: s.base.clone(), has_token: s.token.is_some(), pairing: s.pair.is_some(), syncing: self.syncing.load(Ordering::Relaxed), downloads }
    }

    fn timeouts(&self) -> Timeouts {
        self.st().t
    }

    fn the_agent(&self) -> ureq::Agent {
        let mut s = self.st();
        if s.agent.is_none() {
            s.agent = Some(agent(s.skip_tls, &s.t));
        }
        s.agent.clone().unwrap()
    }

    fn same_server(origin: &str, url: &str) -> bool {
        !origin.is_empty() && (url == origin || url.starts_with(&format!("{origin}/")))
    }

    /// One API call: JSON in and out (or the bytes, with `buffer`). An HTTP error is an answer; offline is an Err.
    fn call(&self, p: &str, o: Opts) -> Result<Resp, Error> {
        let (base, origin, token) = {
            let s = self.st();
            (s.base.clone(), s.origin.clone(), s.token.clone())
        };
        if base.is_empty() {
            return Err(Error::NoServer);
        }
        let cancelled = || o.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        if cancelled() {
            return Err(Error::Aborted);
        }
        let url = if is_http(p) { p.to_string() } else { format!("{base}{p}") };
        let mut req = self.the_agent().request(o.method, &url).timeout(o.timeout).set("Accept", if o.buffer { "*/*" } else { "application/json" });
        for (k, v) in &o.headers {
            req = req.set(k, v);
        }
        if o.auth && Self::same_server(&origin, &url) {
            if let Some(t) = &token {
                req = req.set("Authorization", &format!("Bearer {t}"));
            }
        }
        let res = send(req, o.json.as_ref()).map_err(|e| if cancelled() { Error::Aborted } else { e })?;
        let status = res.status();
        if o.buffer && header_u64(&res, "content-length").is_some_and(|l| l > o.max) {
            return Ok(Resp { status, json: None, bytes: Vec::new(), too_big: true });
        }
        let mut bytes = Vec::new();
        res.into_reader().take(o.max + 1).read_to_end(&mut bytes).map_err(|e| if cancelled() { Error::Aborted } else { Error::Offline(e.to_string()) })?;
        if bytes.len() as u64 > o.max {
            return Ok(Resp { status, json: None, bytes: Vec::new(), too_big: true });
        }
        let json = if o.buffer { None } else { serde_json::from_slice(&bytes).ok() };
        Ok(Resp { status, json, bytes, too_big: false })
    }

    // ---------- pairing

    fn device_id(&self) -> String {
        if let Some(id) = self.st().device_id.clone() {
            return id;
        }
        let f = self.data_dir.join("romm-device.json");
        let saved = fs::read(&f).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|o| opt_text(o.get("id")));
        let id = saved.unwrap_or_else(|| {
            let mut b = [0u8; 16];
            let _ = getrandom::getrandom(&mut b);
            b[6] = (b[6] & 0x0f) | 0x40;
            b[8] = (b[8] & 0x3f) | 0x80;
            let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
            let id = format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]);
            let _ = fs::create_dir_all(&self.data_dir).and_then(|_| fs::write(&f, json!({ "id": id }).to_string()));
            id
        });
        self.st().device_id = Some(id.clone());
        id
    }

    /// Starts the device-flow sign-in (RomM 5.0+): a code to show and a page for the QR. `Unsupported` is an older
    /// RomM, which takes the 8-digit code instead.
    pub fn pair_start(&self, name: &str) -> Result<PairStart, Error> {
        let base = self.st().base.clone();
        if base.is_empty() {
            return Err(Error::NoServer);
        }
        let body = json!({
            "client_device_identifier": self.device_id(), "name": if name.is_empty() { "RustyShelf" } else { name }, "client": "RustyShelf",
            "platform": os_name(), "client_version": env!("CARGO_PKG_VERSION"), "requested_scopes": ["roms.read", "platforms.read"]
        });
        let r = self.call("/api/auth/device/init", Opts::post(self.timeouts().api, body))?;
        match r.status {
            404 | 405 => return Err(Error::Unsupported),
            429 => return Err(Error::RateLimited),
            _ => {}
        }
        let j = r.json_or_empty();
        if !r.ok() || !js::truthy(j.get("device_code")) || !js::truthy(j.get("user_code")) {
            return Err(Error::Http(r.status));
        }
        let interval = (num(j.get("interval")).filter(|n| *n != 0.0).unwrap_or(5.0).max(1.0) * 1000.0) as u64;
        let now = now_ms();
        let vp = js::text(js::first(&[j.get("verification_path_complete"), j.get("verification_uri_complete"), j.get("verification_path"), j.get("verification_uri")]));
        let expires_at = now + (num(j.get("expires_in")).filter(|n| *n != 0.0).unwrap_or(600.0).max(30.0) * 1000.0) as u64;
        let serial = self.pairs.fetch_add(1, Ordering::Relaxed) + 1;
        let mut s = self.st();
        s.pair = Some(Pair { serial, device_code: js::string(j.get("device_code")), interval, next_at: now + scaled(interval, s.time_scale), expires_at });
        let qr_url = if is_http(&vp) { vp } else { format!("{base}{}{vp}", if vp.starts_with('/') { "" } else { "/" }) };
        Ok(PairStart { user_code: js::string(j.get("user_code")), qr_url, expires_at, interval: interval / 1000 })
    }

    /// One poll, never sooner than the server's interval.
    pub fn pair_poll(&self) -> Poll {
        let (p, scale) = {
            let mut s = self.st();
            let Some(p) = s.pair.clone() else { return Poll::Expired };
            let now = now_ms();
            if now >= p.expires_at {
                s.pair = None;
                return Poll::Expired;
            }
            if now < p.next_at {
                return Poll::Pending { wait: p.next_at - now, error: None };
            }
            (p, s.time_scale)
        };
        let r = match self.call("/api/auth/device/token", Opts::post(self.timeouts().api, json!({ "device_code": p.device_code }))) {
            Ok(r) => r,
            Err(e) => {
                let mut s = self.st();
                if let Some(cur) = s.pair.as_mut().filter(|c| c.serial == p.serial) {
                    cur.next_at = now_ms() + scaled(cur.interval, scale);
                }
                return Poll::Pending { wait: scaled(p.interval, scale), error: Some(e) };
            }
        };
        let mut s = self.st();
        let Some(cur) = s.pair.as_mut().filter(|c| c.serial == p.serial) else { return Poll::Expired };
        let j = r.json_or_empty();
        let tok = js::first(&[j.get("access_token"), j.get("token"), j.get("raw_token")]);
        if r.ok() && tok.is_some() {
            let token = js::string(tok);
            s.pair = None;
            s.token = Some(token.clone());
            return Poll::Signed { token };
        }
        let why = reason_of(&j);
        if why == "slow_down" || r.status == 429 {
            cur.interval += 5000;
        }
        if why == "access_denied" {
            s.pair = None;
            return Poll::Denied;
        }
        if why == "expired_token" || why == "invalid_grant" {
            s.pair = None;
            return Poll::Expired;
        }
        cur.next_at = now_ms() + scaled(cur.interval, scale);
        Poll::Pending { wait: scaled(cur.interval, scale), error: None }
    }

    /// Polls until the pairing ends, or `cancel` stops it.
    pub fn pair_wait(&self, cancel: Option<&AtomicBool>) -> Poll {
        loop {
            if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                return Poll::Cancelled;
            }
            let r = self.pair_poll();
            let Poll::Pending { wait, .. } = r else { return r };
            let until = Instant::now() + Duration::from_millis(if wait == 0 { 1000 } else { wait.min(60_000) });
            while Instant::now() < until {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return Poll::Cancelled;
                }
                std::thread::sleep((until - Instant::now()).min(Duration::from_millis(50)));
            }
        }
    }

    pub fn pair_cancel(&self) {
        self.st().pair = None;
    }

    /// Signs in with the 8-digit code RomM 4.8-4.9 shows; the token on success.
    pub fn pair_with_code(&self, code: &str) -> Result<String, Error> {
        if self.st().base.is_empty() {
            return Err(Error::NoServer);
        }
        let c: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
        if c.is_empty() {
            return Err(Error::BadCode);
        }
        let r = self.call("/api/client-tokens/exchange", Opts::post(self.timeouts().api, json!({ "code": c })))?;
        if matches!(r.status, 404 | 405) {
            return Err(Error::Unsupported);
        }
        let j = r.json_or_empty();
        let tok = match &j {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            _ => js::first(&[j.get("access_token"), j.get("token"), j.get("raw_token")]).map(|t| js::string(Some(t))),
        };
        match tok {
            Some(t) if r.ok() => {
                self.st().token = Some(t.clone());
                Ok(t)
            }
            _ => Err(if r.status == 429 { Error::RateLimited } else { Error::BadCode }),
        }
    }

    /// A cheap check: the server's version (no sign-in needed) and one signed-in call.
    pub fn test(&self) -> Test {
        let (base, token) = {
            let s = self.st();
            (s.base.clone(), s.token.clone())
        };
        if base.is_empty() {
            return Test { version: None, outcome: Err(Error::NoServer) };
        }
        let api = self.timeouts().api;
        let h = match self.call("/api/heartbeat", Opts { auth: false, ..Opts::get(api) }) {
            Ok(h) => h,
            Err(e) => return Test { version: None, outcome: Err(e) },
        };
        let j = h.json_or_empty();
        let v = js::first(&[j.get("SYSTEM").and_then(|s| s.get("VERSION")), j.get("VERSION"), j.get("version")]);
        let version = v.map(|v| js::string(Some(v)));
        if token.is_none() {
            return Test { version, outcome: Err(Error::NoToken) };
        }
        let outcome = match self.call("/api/platforms", Opts::get(api)) {
            Ok(r) if matches!(r.status, 401 | 403) => Err(Error::Auth),
            Ok(r) if !r.ok() => Err(Error::Http(r.status)),
            Ok(r) => Ok(r.json.as_ref().and_then(Value::as_array).map_or(0, Vec::len)),
            Err(e) => Err(e),
        };
        Test { version, outcome }
    }

    // ---------- the library

    fn load_cache(&self) -> Cache {
        let base = self.st().base.clone();
        if let Some(c) = self.st().cache.clone().filter(|c| c.server == base) {
            return c;
        }
        let read = fs::read(self.cache_file()).ok().and_then(|b| serde_json::from_slice::<Cache>(&b).ok()).filter(|c| c.v == 1 && c.server == base);
        let c = read.unwrap_or_else(|| Cache { v: 1, server: base, ..Cache::default() });
        self.st().cache = Some(c.clone());
        c
    }

    fn save_cache(&self, c: &Cache) {
        let f = self.cache_file();
        let tmp = f.with_file_name(format!("romm-library.json.{}.tmp", std::process::id()));
        let _ = fs::create_dir_all(&self.data_dir)
            .and_then(|_| fs::write(&tmp, serde_json::to_string(c).unwrap_or_default()))
            .and_then(|_| fs::rename(&tmp, &f));
    }

    /// romm-covers' files by RomM id.
    fn cover_index(&self) -> HashMap<String, String> {
        static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(\d+)\.(png|jpe?g|webp|gif)$").unwrap());
        let mut m = HashMap::new();
        for e in fs::read_dir(self.covers_dir()).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if let Some(c) = NAME.captures(&n) {
                m.insert(c[1].to_string(), n.clone());
            }
        }
        m
    }

    fn out(&self, c: &Cache) -> Shown {
        let covers = self.cover_index();
        let mut consoles: BTreeMap<String, Vec<Game>> = BTreeMap::new();
        for g in c.games.values() {
            let mut g = g.clone();
            g.cover = covers.get(&g.romm_id.to_string()).map(|f| self.covers_dir().join(f));
            consoles.entry(g.console_id.clone()).or_default().push(g);
        }
        for list in consoles.values_mut() {
            list.sort_by(|a, b| crate::collate::compare_exact(&a.title, &b.title));
        }
        Shown { consoles, skipped: c.skipped.clone().unwrap_or_default(), at: c.at.clone(), count: c.games.len(), ..Shown::default() }
    }

    /// The saved library, without asking the server.
    pub fn cached(&self) -> Shown {
        if self.st().base.is_empty() {
            return Shown { error: Some(Error::NoServer), ..Shown::default() };
        }
        self.out(&self.load_cache())
    }

    /// Syncs with the server (only what changed since the last time) and gives the library; the saved copy when
    /// the server can't be reached.
    pub fn library(&self, on_progress: &mut dyn FnMut(SyncProgress), cancel: Option<&AtomicBool>) -> Shown {
        let (base, token) = {
            let s = self.st();
            (s.base.clone(), s.token.is_some())
        };
        if base.is_empty() {
            return Shown { error: Some(Error::NoServer), ..Shown::default() };
        }
        let _one = self.sync_lock.lock().unwrap_or_else(|e| e.into_inner());
        let c = self.load_cache();
        if !token {
            return Shown { error: Some(Error::NoToken), ..self.out(&c) };
        }
        self.syncing.store(true, Ordering::Relaxed);
        let r = self.sync(c, on_progress, cancel);
        self.syncing.store(false, Ordering::Relaxed);
        r
    }

    /// Every page of a platform's ROMs (after `since` when given), one after another.
    fn pages(&self, pid: u64, since: Option<&str>, cancel: Option<&AtomicBool>, on_page: &mut dyn FnMut(&[Value])) -> Result<(), Error> {
        let mut offset = 0usize;
        let mut first = true;
        loop {
            let mut q = vec![
                ("platform_ids", pid.to_string()),
                ("limit", PAGE.to_string()),
                ("offset", offset.to_string()),
                ("with_char_index", "false".into()),
                ("with_filter_values", "false".into()),
                ("with_rom_id_index", "false".into()),
                ("with_total", if first { "true" } else { "false" }.into()),
            ];
            if let Some(s) = since {
                q.push(("updated_after", s.to_string()));
            }
            first = false;
            let r = self.call(&format!("/api/roms?{}", js::query(&q)), Opts { cancel, ..Opts::get(self.timeouts().page) })?;
            if matches!(r.status, 401 | 403) {
                return Err(Error::Auth);
            }
            if !r.ok() {
                return Err(Error::Http(r.status));
            }
            let j = r.json.unwrap_or(Value::Null);
            let items: Vec<Value> = match &j {
                Value::Array(a) => a.clone(),
                _ => j.get("items").and_then(Value::as_array).cloned().unwrap_or_default(),
            };
            on_page(&items);
            offset += items.len();
            let total = j.get("total").and_then(Value::as_f64);
            if items.is_empty() || items.len() < PAGE || total.is_some_and(|t| offset as f64 >= t) || j.is_array() {
                return Ok(());
            }
            std::thread::sleep(PAGE_PAUSE);
        }
    }

    /// All ROM ids of a platform in one light request; None when the server doesn't give them.
    fn id_sweep(&self, pid: u64, cancel: Option<&AtomicBool>) -> Result<Option<HashSet<u64>>, Error> {
        let q = js::query(&[
            ("platform_ids", pid.to_string()),
            ("limit", "1".into()),
            ("offset", "0".into()),
            ("with_char_index", "false".into()),
            ("with_filter_values", "false".into()),
            ("with_rom_id_index", "true".into()),
            ("with_total", "false".into()),
        ]);
        let r = self.call(&format!("/api/roms?{q}"), Opts { cancel, ..Opts::get(self.timeouts().page) })?;
        if matches!(r.status, 401 | 403) {
            return Err(Error::Auth);
        }
        let ids = r.json.as_ref().filter(|_| r.ok()).and_then(|j| j.get("rom_id_index")).and_then(Value::as_array);
        Ok(ids.map(|ids| ids.iter().filter_map(|v| unum(Some(v))).collect()))
    }

    fn sync(&self, mut c: Cache, on_progress: &mut dyn FnMut(SyncProgress), cancel: Option<&AtomicBool>) -> Shown {
        let so = self.sign_outs.load(Ordering::Relaxed);
        let base = self.st().base.clone();
        let plats = match self.call("/api/platforms", Opts { cancel, ..Opts::get(self.timeouts().api) }) {
            Ok(r) if matches!(r.status, 401 | 403) => return Shown { error: Some(Error::Auth), ..self.out(&c) },
            Ok(r) => match r.json.as_ref().and_then(Value::as_array).filter(|_| r.ok()) {
                Some(a) => a.clone(),
                None => return Shown { offline: true, error: Some(Error::Http(r.status)), ..self.out(&c) },
            },
            Err(Error::Aborted) => return Shown { error: Some(Error::Aborted), ..self.out(&c) },
            Err(e) => return Shown { offline: true, error: Some(e), ..self.out(&c) },
        };
        let mut acc = Acc { games: c.games.clone(), seen: None, fetched: 0, removed: 0, since: None };
        let mut platforms = BTreeMap::new();
        let mut skipped = Skipped::default();
        let mut mapped = Vec::new();
        for p in &plats {
            let Some(id) = unum(p.get("id")) else { continue };
            match map_platform(p) {
                Some(cid) => mapped.push((id, cid, p)),
                None => {
                    skipped.platforms += 1;
                    skipped.games += unum(p.get("rom_count")).unwrap_or(0);
                    skipped.slugs.push(js::first(&[p.get("slug"), p.get("fs_slug")]).map_or_else(|| id.to_string(), |s| js::string(Some(s))));
                }
            }
        }
        let of = mapped.len();
        let run = (|| -> Result<(), Error> {
            for (i, (pid, cid, p)) in mapped.iter().enumerate() {
                let (pid, cid) = (*pid, cid.as_str());
                let name = |keys: &[&str]| js::text(js::first(&keys.iter().map(|k| p.get(*k)).collect::<Vec<_>>()));
                let prev = c.platforms.get(&pid);
                let full = prev.is_none_or(|pr| pr.console_id != cid || pr.since.as_deref().is_none_or(str::is_empty));
                let prev_since = if full { None } else { prev.and_then(|pr| pr.since.clone()) };
                on_progress(SyncProgress::Platform { platform: name(&["display_name", "custom_name", "name", "slug"]), index: i, of, fetched: acc.fetched });
                acc.since = prev_since.clone();
                acc.seen = full.then(HashSet::new);
                let roms_name = name(&["name", "slug"]);
                let rom_count = unum(p.get("rom_count"));
                let mut take = |acc: &mut Acc, items: &[Value]| {
                    acc.take(items, pid, cid, &base);
                    on_progress(SyncProgress::Roms { platform: roms_name.clone(), index: i, of, fetched: acc.fetched });
                };
                if full && rom_count == Some(0) {
                    for k in acc.mine(pid) {
                        acc.games.remove(&k);
                        acc.removed += 1;
                    }
                } else {
                    self.pages(pid, prev_since.as_deref(), cancel, &mut |items| take(&mut acc, items))?;
                    if full {
                        let seen = acc.seen.take().unwrap_or_default();
                        for k in acc.mine(pid) {
                            if !seen.contains(&k) {
                                acc.games.remove(&k);
                                acc.removed += 1;
                            }
                        }
                    } else {
                        std::thread::sleep(PAGE_PAUSE);
                        match self.id_sweep(pid, cancel)? {
                            Some(ids) => {
                                for k in acc.mine(pid) {
                                    if !ids.contains(&k) {
                                        acc.games.remove(&k);
                                        acc.removed += 1;
                                    }
                                }
                            }
                            None if rom_count.is_some_and(|n| n as usize != acc.mine(pid).len()) => {
                                let mut all = HashSet::new();
                                self.pages(pid, None, cancel, &mut |items| {
                                    all.extend(items.iter().filter_map(|it| unum(it.get("id"))));
                                    take(&mut acc, items);
                                })?;
                                for k in acc.mine(pid) {
                                    if !all.contains(&k) {
                                        acc.games.remove(&k);
                                        acc.removed += 1;
                                    }
                                }
                            }
                            None => {}
                        }
                    }
                }
                let count = acc.mine(pid).len();
                let opt = |k: &str| opt_text(p.get(k));
                platforms.insert(pid, PlatformRec { id: pid, slug: opt("slug"), fs_slug: opt("fs_slug"), name: Some(name(&["display_name", "custom_name", "name"])).filter(|n| !n.is_empty()), console_id: cid.to_string(), since: acc.since.clone(), count });
                if i + 1 < of {
                    std::thread::sleep(PAGE_PAUSE);
                }
            }
            Ok(())
        })();
        if let Err(e) = run {
            let offline = matches!(e, Error::Offline(_));
            return Shown { offline, error: Some(e), ..self.out(&c) };
        }
        let Acc { mut games, fetched, mut removed, .. } = acc;
        games.retain(|_, g| {
            let keep = platforms.contains_key(&g.platform_id);
            removed += usize::from(!keep);
            keep
        });
        c.games = games;
        c.platforms = platforms;
        c.skipped = Some(skipped);
        c.at = Some(iso(now_ms()));
        if so != self.sign_outs.load(Ordering::Relaxed) {
            return Shown { error: Some(Error::SignedOut), ..self.out(&c) };
        }
        self.save_cache(&c);
        self.st().cache = Some(c.clone());
        on_progress(SyncProgress::Done { fetched, removed });
        Shown { fetched, removed, ..self.out(&c) }
    }

    // ---------- covers

    /// Downloads the covers not in romm-covers yet (at most `max`, `concurrency` at a time); every game's cover.
    pub fn covers(&self, games: &[Game], max: usize, concurrency: usize, cancel: Option<&AtomicBool>) -> CoverRun {
        let dir = self.covers_dir();
        let mut run = CoverRun::default();
        if fs::create_dir_all(&dir).is_err() {
            return run;
        }
        let have = self.cover_index();
        let mut todo = Vec::new();
        for g in games {
            if let Some(f) = have.get(&g.romm_id.to_string()) {
                run.paths.insert(g.id.clone(), dir.join(f));
            } else if let Some(u) = g.cover_url.as_ref().filter(|u| !self.st().bad_covers.contains(*u)) {
                if todo.len() < max {
                    todo.push((g, u.clone()));
                }
            }
        }
        let so = self.sign_outs.load(Ordering::Relaxed);
        let next = AtomicU64::new(0);
        let out = Mutex::new(&mut run);
        let cover_t = self.timeouts().cover;
        std::thread::scope(|sc| {
            for _ in 0..concurrency.clamp(1, 6).min(todo.len()) {
                sc.spawn(|| loop {
                    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) || so != self.sign_outs.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                    let Some((g, url)) = todo.get(i) else { return };
                    let r = self.call(url, Opts { auth: false, buffer: true, max: COVER_MAX_BYTES, cancel, ..Opts::get(cover_t) });
                    if so != self.sign_outs.load(Ordering::Relaxed) {
                        return;
                    }
                    let r = match r {
                        Ok(r) => r,
                        Err(Error::Aborted) => return,
                        Err(_) => {
                            out.lock().unwrap().failed += 1;
                            continue;
                        }
                    };
                    let Some(ext) = image_ext(&r.bytes).filter(|_| r.ok()) else {
                        out.lock().unwrap().failed += 1;
                        self.st().bad_covers.insert(url.clone());
                        continue;
                    };
                    let file = dir.join(format!("{}.{ext}", g.romm_id));
                    let tmp = dir.join(format!("{}.{ext}.{}.tmp", g.romm_id, std::process::id()));
                    let mut o = out.lock().unwrap();
                    if fs::write(&tmp, &r.bytes).and_then(|_| fs::rename(&tmp, &file)).is_ok() {
                        o.paths.insert(g.id.clone(), file);
                        o.downloaded += 1;
                    } else {
                        o.failed += 1;
                    }
                });
            }
        });
        run
    }

    // ---------- downloads

    /// Downloads a game into `dest_dir` (a game of several files unpacked into a folder of its name), carrying on
    /// from an earlier try and checked with RomM's hashes. The same game asked for twice waits for the first.
    pub fn download(&self, game: &Game, dest_dir: &Path, on_progress: &mut dyn FnMut(DownloadProgress), cancel: Option<&AtomicBool>) -> Result<Downloaded, Error> {
        if game.romm_id == 0 || game.file.as_deref().is_none_or(str::is_empty) {
            return Err(Error::BadGame);
        }
        {
            let s = self.st();
            if s.base.is_empty() {
                return Err(Error::NoServer);
            }
            if s.token.is_none() {
                return Err(Error::NoToken);
            }
        }
        let key = game.romm_id;
        {
            let mut a = self.active.lock().unwrap_or_else(|e| e.into_inner());
            while a.contains(&key) {
                a = self.active_done.wait(a).unwrap_or_else(|e| e.into_inner());
            }
            a.insert(key);
        }
        let r = self.do_download(game, dest_dir, on_progress, cancel);
        self.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
        self.active_done.notify_all();
        r
    }

    fn head_size(&self, url: &str, token: Option<&str>, origin: &str) -> Option<u64> {
        let mut req = self.the_agent().head(url).timeout(self.timeouts().api).set("Accept-Encoding", "identity");
        if let Some(t) = token.filter(|_| Self::same_server(origin, url)) {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let r = send(req, None).ok()?;
        (200..300).contains(&r.status()).then(|| header_u64(&r, "content-length")).flatten()
    }

    /// Downloads `url` into `part`, carrying on from what's already there (Range); true when it resumed.
    fn fetch_to_part(&self, url: &str, part: &Path, expect: Option<u64>, on_progress: &mut dyn FnMut(DownloadProgress), cancel: Option<&AtomicBool>) -> Result<(u64, bool), Error> {
        let cancelled = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let mut have = size_of(part).unwrap_or(0);
        if expect.is_some_and(|e| have > e) {
            let _ = fs::remove_file(part);
            have = 0;
        }
        if let Some(e) = expect.filter(|e| *e == have && *e > 0) {
            return Ok((e, true));
        }
        let (token, origin) = {
            let s = self.st();
            (s.token.clone(), s.origin.clone())
        };
        let mut req = self.the_agent().get(url).set("Accept", "*/*").set("Accept-Encoding", "identity");
        if let Some(t) = token.as_deref().filter(|_| Self::same_server(&origin, url)) {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        if have > 0 {
            req = req.set("Range", &format!("bytes={have}-"));
        }
        let res = send(req, None).map_err(|e| if cancelled() { Error::Aborted } else { e })?;
        let status = res.status();
        if status == 416 && have > 0 {
            if self.head_size(url, token.as_deref(), &origin) == Some(have) {
                return Ok((have, true));
            }
            let _ = fs::remove_file(part);
            return Err(Error::Range("resume refused".into()));
        }
        match status {
            401 | 403 => return Err(Error::Auth),
            404 => return Err(Error::NotFound),
            s if !(200..300).contains(&s) => return Err(Error::Http(s)),
            _ => {}
        }
        static RANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)bytes\s+(\d+)-(\d+)/(\d+|\*)").unwrap());
        let (append, total) = if status == 206 && have > 0 {
            let m = res.header("content-range").and_then(|h| RANGE.captures(h).map(|c| (c[1].parse::<u64>().ok(), c[3].parse::<u64>().ok())));
            match m {
                Some((Some(start), total)) if start == have => (true, total),
                _ => {
                    let _ = fs::remove_file(part);
                    return Err(Error::Range("resume answered with other bytes".into()));
                }
            }
        } else {
            have = 0;
            (false, header_u64(&res, "content-length").filter(|l| *l != 0).or(expect))
        };
        let file = fs::OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(part).map_err(io_error)?;
        let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
        let mut body = res.into_reader();
        let mut received = have;
        let mut tick = Instant::now();
        on_progress(DownloadProgress { phase: Phase::Download, received, total });
        let mut buf = vec![0u8; 64 << 10];
        loop {
            if cancelled() {
                let _ = out.flush();
                return Err(Error::Aborted);
            }
            let n = match body.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    let _ = out.flush();
                    if cancelled() {
                        return Err(Error::Aborted);
                    }
                    let why = if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) { "stalled".to_string() } else { e.to_string() };
                    return Err(Error::Offline(why));
                }
            };
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).map_err(io_error)?;
            received += n as u64;
            if tick.elapsed() > Duration::from_millis(200) {
                tick = Instant::now();
                on_progress(DownloadProgress { phase: Phase::Download, received, total });
            }
        }
        out.flush().map_err(io_error)?;
        drop(out);
        on_progress(DownloadProgress { phase: Phase::Download, received, total });
        if let Some(t) = total.filter(|t| *t != received) {
            return Err(Error::Offline(format!("short download ({received} of {t})")));
        }
        Ok((received, append))
    }

    fn do_download(&self, game: &Game, dest_dir: &Path, on_progress: &mut dyn FnMut(DownloadProgress), cancel: Option<&AtomicBool>) -> Result<Downloaded, Error> {
        let aborted = || cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        fs::create_dir_all(dest_dir).map_err(io_error)?;
        let file = game.file.clone().unwrap_or_default();
        let name = safe_name(&file);
        let multi = game.multi;
        let fin = dest_dir.join(if multi { format!("{name}.zip") } else { name.clone() });
        let out_dir = dest_dir.join(&name);
        let size = game.size.filter(|s| *s != 0);
        if !multi && size.is_some() && size_of(&fin) == size {
            return Ok(Downloaded { file: fin.clone(), files: vec![fin], already: true, ..Downloaded::default() });
        }
        // a different file of yours with that name is never replaced: RomM's copy goes beside it as "Name (RomM).ext"
        let (stem, ext) = stem_ext(&name);
        let alt = dest_dir.join(format!("{stem} (RomM){ext}"));
        if !multi && size.is_some() && size_of(&alt) == size {
            return Ok(Downloaded { file: alt.clone(), files: vec![alt], already: true, ..Downloaded::default() });
        }
        if multi {
            if let Some(done) = multi_done(game, &out_dir) {
                return Ok(Downloaded { file: launch_file(&done), dir: Some(out_dir), files: done.into_iter().map(|f| f.file).collect(), multi: true, extracted: true, already: true, ..Downloaded::default() });
            }
        }
        if !free_space_ok(dest_dir, size.unwrap_or(0) * if multi { 2 } else { 1 }) {
            return Err(Error::NoSpace);
        }
        let base = self.st().base.clone();
        let url = format!("{base}/api/roms/{}/content/{}", game.romm_id, js::encode_uri_component(&file));
        let part = fin.with_file_name(format!("{}.part", fin.file_name().unwrap_or_default().to_string_lossy()));
        let expect = if multi { None } else { size };
        let mut resumed = false;
        for attempt in 0.. {
            match self.fetch_to_part(&url, &part, expect, on_progress, cancel) {
                Ok((_, r)) => {
                    resumed |= r;
                    break;
                }
                Err(_) if aborted() => return Err(Error::Aborted),
                Err(e @ (Error::Offline(_) | Error::Range(_))) if attempt < 2 => {
                    let _ = e;
                    std::thread::sleep(Duration::from_millis(scaled(1000, self.st().time_scale)));
                }
                Err(e) => return Err(e),
            }
        }
        if !multi {
            let want = match game.files.as_slice() {
                [one] => RomFile { size: one.size.or(game.size), ..one.clone() },
                _ => RomFile { size: game.size, ..RomFile::default() },
            };
            on_progress(DownloadProgress { phase: Phase::Verify, received: 0, total: None });
            let got = hash_file(&part, algo_for(Some(&want)), cancel)?;
            if !matches(&want, &got) {
                let _ = fs::remove_file(&part);
                return Err(Error::HashMismatch(format!("the download doesn't match RomM's {}", got.algo.map_or("size", Algo::name))));
            }
            let dest = if fin.exists() {
                let _ = fs::remove_file(&alt);
                alt
            } else {
                fin
            };
            fs::rename(&part, &dest).map_err(io_error)?;
            return Ok(Downloaded { file: dest.clone(), files: vec![dest], verified: Some(got.algo.map_or("size", Algo::name).into()), resumed, ..Downloaded::default() });
        }
        let _ = fs::remove_file(&fin);
        fs::rename(&part, &fin).map_err(io_error)?;
        let by_name: HashMap<String, RomFile> = game.files.iter().map(|f| (safe_name(&f.name).to_lowercase(), f.clone())).collect();
        let tmp = dest_dir.join(format!("{name}.unpacking"));
        let _ = fs::remove_dir_all(&tmp);
        let got = match unzip(&fin, &tmp, &|n| by_name.get(&safe_name(n).to_lowercase()).cloned(), on_progress, cancel) {
            Ok(g) => g,
            Err(e) => {
                let _ = fs::remove_dir_all(&tmp);
                if e == Error::Aborted || aborted() {
                    return Err(Error::Aborted);
                }
                return Ok(Downloaded {
                    file: fin.clone(),
                    files: vec![fin],
                    multi: true,
                    note: Some(format!("kept as a zip: it could not be unpacked ({})", e.code())),
                    resumed,
                    ..Downloaded::default()
                });
            }
        };
        for f in &got {
            if let Some(want) = by_name.get(&safe_name(&f.name).to_lowercase()) {
                if !matches(want, &f.got) {
                    let _ = fs::remove_dir_all(&tmp);
                    let _ = fs::remove_file(&fin);
                    return Err(Error::HashMismatch(format!("{} doesn't match RomM's {}", f.name, f.got.algo.map_or("size", Algo::name))));
                }
            }
        }
        let mut files = Vec::new();
        for f in &got {
            let dest = out_dir.join(f.file.strip_prefix(&tmp).unwrap_or(&f.file));
            if let Some(d) = dest.parent() {
                fs::create_dir_all(d).map_err(io_error)?;
            }
            let _ = fs::remove_file(&dest);
            fs::rename(&f.file, &dest).map_err(io_error)?;
            files.push(LocalFile { name: f.name.clone(), file: dest, size: f.got.size });
        }
        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_file(&fin);
        let verified = if got.iter().any(|f| by_name.contains_key(&safe_name(&f.name).to_lowercase())) { "files" } else { "zip" };
        Ok(Downloaded {
            file: launch_file(&files),
            dir: Some(out_dir),
            files: files.into_iter().map(|f| f.file).collect(),
            multi: true,
            extracted: true,
            verified: Some(verified.into()),
            resumed,
            ..Downloaded::default()
        })
    }

    // ---------- sign out

    /// Forgets the sign-in, the saved library and the covers. A sync or cover download still running doesn't
    /// write them back.
    pub fn forget(&self) {
        self.sign_outs.fetch_add(1, Ordering::Relaxed);
        {
            let mut s = self.st();
            s.token = None;
            s.pair = None;
            s.cache = None;
            s.bad_covers.clear();
        }
        let _ = fs::remove_file(self.cache_file());
        let _ = fs::remove_dir_all(self.covers_dir());
    }
}

struct Acc {
    games: BTreeMap<u64, Game>,
    seen: Option<HashSet<u64>>,
    fetched: usize,
    removed: usize,
    since: Option<String>,
}

impl Acc {
    fn mine(&self, pid: u64) -> Vec<u64> {
        self.games.iter().filter(|(_, g)| g.platform_id == pid).map(|(k, _)| *k).collect()
    }

    fn take(&mut self, items: &[Value], pid: u64, cid: &str, base: &str) {
        for it in items {
            let Some(id) = unum(it.get("id")) else { continue };
            if let Some(seen) = &mut self.seen {
                seen.insert(id);
            }
            if it.get("missing_from_fs") == Some(&Value::Bool(true)) {
                if self.games.remove(&id).is_some() {
                    self.removed += 1;
                }
                continue;
            }
            self.games.insert(id, to_game(it, pid, cid, base));
            self.fetched += 1;
            if let Some(u) = opt_text(it.get("updated_at")) {
                if self.since.as_deref().is_none_or(|s| parse_iso(&u) > parse_iso(s)) {
                    self.since = Some(u);
                }
            }
        }
    }
}

fn image_ext(b: &[u8]) -> Option<&'static str> {
    if b.len() < 12 {
        return None;
    }
    if b.starts_with(&[0x89, 0x50, 0x4e, 0x47]) {
        Some("png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("webp")
    } else if &b[..4] == b"GIF8" {
        Some("gif")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(with_scheme("192.168.1.5:8080"), "http://192.168.1.5:8080");
        assert_eq!(with_scheme("nas"), "http://nas");
        assert_eq!(with_scheme("romm.lan/x"), "http://romm.lan/x");
        assert_eq!(with_scheme("romm.example.com"), "https://romm.example.com");
        assert_eq!(with_scheme("HTTP://a.b"), "HTTP://a.b");
        assert_eq!(origin_of("nas:3000/api"), "http://nas:3000");
        assert_eq!(origin_of(""), "");
    }

    #[test]
    fn dates() {
        assert_eq!(parse_iso("2026-01-01T00:00:00.000Z"), 1_767_225_600_000);
        assert_eq!(parse_iso("2026-01-01T02:00:00.123456+02:00"), 1_767_225_600_123);
        assert_eq!(iso(1_767_225_600_123), "2026-01-01T00:00:00.123Z");
        assert_eq!(date_of(Some(&json!(631_152_000_000u64 + 86_400_000))).as_deref(), Some("1990-01-02"));
        assert_eq!(date_of(Some(&json!(631_152_000))).as_deref(), Some("1990-01-01"));
        assert_eq!(date_of(Some(&json!(null))), None);
    }

    #[test]
    fn names_and_hashes() {
        assert_eq!(safe_name("a/b:c?.iso. "), "b_c_.iso");
        assert_eq!(safe_name(".."), "game");
        assert_eq!(hex_u32("CBF43926"), 0xcbf4_3926);
        let mut h = Hasher::new(Some(Algo::Crc));
        h.update(b"123456789");
        assert_eq!(h.result().value.as_deref(), Some("cbf43926"));
        assert!(matches(&RomFile { crc: Some("CBF43926".into()), size: Some(9), ..RomFile::default() }, &Hashed { size: 9, algo: Some(Algo::Crc), value: Some("cbf43926".into()) }));
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/companion.js, as RustyShelf has it.
//! The phone companion: a small web server on this PC's home network, so a phone on the same Wi-Fi can browse
//! the library, start a game on the TV, read its manual, and work as a remote and a keyboard. Off until it's
//! switched on.
//!
//! Safety: it listens on the home-network address only and answers private-range addresses only; every API
//! call needs the long random token a phone gets when it pairs with the six-digit PIN (five wrong PINs lock
//! pairing for ten minutes, longer each time); tokens are kept only as hashes; pictures and manual pages go out
//! under random ids, never file paths.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::js;

const DEFAULT_PORT: u16 = 8730;
const MAX_PHONES: usize = 12;
const PIN_TRIES: u32 = 5;
/// the first lockout after too many wrong PINs, in milliseconds
const LOCK_MS: f64 = 600_000.0;
const COOKIE: &str = "bs_phone";
const BUTTONS: [&str; 13] = ["up", "down", "left", "right", "a", "b", "x", "y", "lb", "rb", "start", "view", "home"];
/// the heights a picture is made smaller to: any height at all would be one more decode for each one asked
const THUMB_H: [u32; 6] = [180, 360, 480, 720, 1080, 1600];

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    getrandom::getrandom(&mut b).expect("the system's random numbers");
    b
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn base64url(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn sha(s: &str) -> String {
    hex(&Sha256::digest(s.as_bytes()))
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    block[..key.len().min(64)].copy_from_slice(&key[..key.len().min(64)]);
    let inner = Sha256::new().chain_update(block.map(|b| b ^ 0x36)).chain_update(message).finalize();
    Sha256::new().chain_update(block.map(|b| b ^ 0x5c)).chain_update(inner).finalize().to_vec()
}

/// Whether two texts are the same, in a time that doesn't tell where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

// ---------- the pairing QR code: byte mode, error correction M, versions 1 to 10 ----------

fn gf_mul(x: u32, y: u32) -> u32 {
    let mut z = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11d);
        z ^= ((y >> i) & 1) * x;
    }
    z & 0xff
}

fn raw_modules(v: usize) -> usize {
    let mut r = (16 * v + 128) * v + 64;
    if v >= 2 {
        let n = v / 7 + 2;
        r -= (25 * n - 10) * n - 55;
        if v >= 7 {
            r -= 36;
        }
    }
    r
}

const QR_ECC: [usize; 11] = [0, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26];
const QR_BLOCKS: [usize; 11] = [0, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5];

/// A text as a QR code: its modules, row by row, true for a dark one. An error for a text too long for one.
pub fn qr_matrix(text: &str) -> Result<Vec<Vec<bool>>, String> {
    let bytes = text.as_bytes();
    let data_codewords = |v: usize| raw_modules(v) / 8 - QR_ECC[v] * QR_BLOCKS[v];
    let v = (1..=10).find(|&v| 4 + if v < 10 { 8 } else { 16 } + bytes.len() * 8 <= data_codewords(v) * 8).ok_or("Too long for a QR code.")?;
    let mut bits: Vec<u8> = Vec::new();
    let put = |bits: &mut Vec<u8>, val: usize, n: usize| bits.extend((0..n).rev().map(|i| ((val >> i) & 1) as u8));
    put(&mut bits, 4, 4);
    put(&mut bits, bytes.len(), if v < 10 { 8 } else { 16 });
    bytes.iter().for_each(|b| put(&mut bits, *b as usize, 8));
    let cap = data_codewords(v) * 8;
    let end = 4.min(cap - bits.len());
    put(&mut bits, 0, end);
    let fill = (8 - bits.len() % 8) % 8;
    put(&mut bits, 0, fill);
    let mut pad = 0xec;
    while bits.len() < cap {
        put(&mut bits, pad, 8);
        pad ^= 0xec ^ 0x11;
    }
    let data: Vec<u32> = bits.chunks(8).map(|c| c.iter().fold(0, |n, b| (n << 1) | u32::from(*b))).collect();
    // error correction a block, then the blocks interleaved
    let (nb, ecl, raw) = (QR_BLOCKS[v], QR_ECC[v], raw_modules(v) / 8);
    let (n_short, short_len) = (nb - raw % nb, raw / nb);
    let divisor = {
        let mut r = vec![0u32; ecl];
        r[ecl - 1] = 1;
        let mut root = 1;
        for _ in 0..ecl {
            for j in 0..ecl {
                r[j] = gf_mul(r[j], root);
                if j + 1 < ecl {
                    r[j] ^= r[j + 1];
                }
            }
            root = gf_mul(root, 2);
        }
        r
    };
    let mut blocks: Vec<Vec<u32>> = Vec::new();
    let mut k = 0;
    for i in 0..nb {
        let len = short_len - ecl + usize::from(i >= n_short);
        let mut d = data[k..k + len].to_vec();
        k += len;
        let mut rem = vec![0u32; ecl];
        for b in &d {
            let f = b ^ rem.remove(0);
            rem.push(0);
            for (r, c) in rem.iter_mut().zip(&divisor) {
                *r ^= gf_mul(*c, f);
            }
        }
        if i < n_short {
            d.push(0);
        }
        d.extend(rem);
        blocks.push(d);
    }
    let mut cw: Vec<u32> = Vec::new();
    for i in 0..blocks[0].len() {
        for (j, b) in blocks.iter().enumerate() {
            if i != short_len - ecl || j >= n_short {
                cw.push(b[i]);
            }
        }
    }
    let size = v * 4 + 17;
    let (mut m, mut fixed) = (vec![vec![false; size]; size], vec![vec![false; size]; size]);
    fn set(m: &mut [Vec<bool>], fixed: &mut [Vec<bool>], x: usize, y: usize, dark: bool) {
        m[y][x] = dark;
        fixed[y][x] = true;
    }
    for i in 0..size {
        set(&mut m, &mut fixed, 6, i, i % 2 == 0);
        set(&mut m, &mut fixed, i, 6, i % 2 == 0);
    }
    for (cx, cy) in [(3i32, 3i32), (size as i32 - 4, 3), (3, size as i32 - 4)] {
        for dy in -4..=4i32 {
            for dx in -4..=4i32 {
                let (x, y, dist) = (cx + dx, cy + dy, dx.abs().max(dy.abs()));
                if x >= 0 && (x as usize) < size && y >= 0 && (y as usize) < size {
                    set(&mut m, &mut fixed, x as usize, y as usize, dist != 2 && dist != 4);
                }
            }
        }
    }
    let align: Vec<usize> = if v == 1 {
        Vec::new()
    } else {
        let n = v / 7 + 2;
        let step = ((v * 4 + 4) as f64 / (n * 2 - 2) as f64).ceil() as usize * 2;
        let mut r = vec![6];
        let mut p = v * 4 + 10;
        while r.len() < n {
            r.insert(1, p);
            p -= step;
        }
        r
    };
    let na = align.len();
    for i in 0..na {
        for j in 0..na {
            if (i == 0 && j == 0) || (i == 0 && j == na - 1) || (i == na - 1 && j == 0) {
                continue;
            }
            for dy in -2..=2i32 {
                for dx in -2..=2i32 {
                    set(&mut m, &mut fixed, (align[i] as i32 + dx) as usize, (align[j] as i32 + dy) as usize, dx.abs().max(dy.abs()) != 1);
                }
            }
        }
    }
    // the format's bits for a mask, at error correction M
    let format = |m: &mut [Vec<bool>], fixed: &mut [Vec<bool>], mask: u32| {
        let mut r = mask;
        for _ in 0..10 {
            r = (r << 1) ^ ((r >> 9) * 0x537);
        }
        let b = ((mask << 10) | r) ^ 0x5412;
        let bit = |i: usize| (b >> i) & 1 == 1;
        for i in 0..=5 {
            set(m, fixed, 8, i, bit(i));
        }
        set(m, fixed, 8, 7, bit(6));
        set(m, fixed, 8, 8, bit(7));
        set(m, fixed, 7, 8, bit(8));
        for i in 9..15 {
            set(m, fixed, 14 - i, 8, bit(i));
        }
        for i in 0..8 {
            set(m, fixed, size - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            set(m, fixed, 8, size - 15 + i, bit(i));
        }
        set(m, fixed, 8, size - 8, true);
    };
    format(&mut m, &mut fixed, 0);
    if v >= 7 {
        let mut r = v as u32;
        for _ in 0..12 {
            r = (r << 1) ^ ((r >> 11) * 0x1f25);
        }
        let b = ((v as u32) << 12) | r;
        for i in 0..18 {
            let (dark, a, k) = ((b >> i) & 1 == 1, size - 11 + i % 3, i / 3);
            set(&mut m, &mut fixed, a, k, dark);
            set(&mut m, &mut fixed, k, a, dark);
        }
    }
    let mut i = 0;
    let mut right = size as i32 - 1;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let y = if (right + 1) & 2 == 0 { size - 1 - vert } else { vert };
                if !fixed[y][x] && i < cw.len() * 8 {
                    m[y][x] = (cw[i >> 3] >> (7 - (i & 7))) & 1 == 1;
                    i += 1;
                }
            }
        }
        right -= 2;
    }
    let masked = |mask: usize, x: usize, y: usize| match mask {
        0 => (x + y).is_multiple_of(2),
        1 => y.is_multiple_of(2),
        2 => x.is_multiple_of(3),
        3 => (x + y).is_multiple_of(3),
        4 => (x / 3 + y / 2).is_multiple_of(2),
        5 => x * y % 2 + x * y % 3 == 0,
        6 => (x * y % 2 + x * y % 3).is_multiple_of(2),
        _ => ((x + y) % 2 + x * y % 3).is_multiple_of(2),
    };
    let apply = |m: &mut [Vec<bool>], fixed: &[Vec<bool>], mask: usize| {
        for y in 0..size {
            for x in 0..size {
                if !fixed[y][x] && masked(mask, x, y) {
                    m[y][x] = !m[y][x];
                }
            }
        }
    };
    // the mask with the fewest long runs, 2×2 blocks, finder-like stretches and imbalance
    let penalty = |m: &[Vec<bool>]| {
        const FINDER: [bool; 11] = [true, false, true, true, true, false, true, false, false, false, false];
        let (mut p, mut dark) = (0usize, 0usize);
        for y in 0..size {
            let (mut rx, mut ry) = (1, 1);
            for x in 0..size {
                dark += usize::from(m[y][x]);
                if x > 0 {
                    if m[y][x] == m[y][x - 1] {
                        rx += 1;
                        p += if rx == 5 { 3 } else { usize::from(rx > 5) };
                    } else {
                        rx = 1;
                    }
                    if m[x][y] == m[x - 1][y] {
                        ry += 1;
                        p += if ry == 5 { 3 } else { usize::from(ry > 5) };
                    } else {
                        ry = 1;
                    }
                }
                if x > 0 && y > 0 && m[y][x] == m[y][x - 1] && m[y][x] == m[y - 1][x] && m[y][x] == m[y - 1][x - 1] {
                    p += 3;
                }
            }
        }
        let finders = |get: &dyn Fn(usize, usize) -> bool| {
            let mut k = 0;
            for a in 0..size {
                for b in 0..size.saturating_sub(10) {
                    k += usize::from((0..11).all(|t| get(a, b + t) == FINDER[t])) + usize::from((0..11).all(|t| get(a, b + t) == FINDER[10 - t]));
                }
            }
            k
        };
        p += 40 * (finders(&|a, b| m[a][b]) + finders(&|a, b| m[b][a]));
        p + (dark as i64 * 20 - (size * size) as i64 * 10).unsigned_abs() as usize / (size * size) * 10
    };
    let (mut best, mut least) = (0, usize::MAX);
    for mask in 0..8 {
        apply(&mut m, &fixed, mask);
        format(&mut m, &mut fixed, mask as u32);
        let p = penalty(&m);
        if p < least {
            (least, best) = (p, mask);
        }
        apply(&mut m, &fixed, mask);
    }
    apply(&mut m, &fixed, best);
    format(&mut m, &mut fixed, best as u32);
    Ok(m)
}

// ---------- addresses ----------

/// Whether an address is of a home network: 10/8, 172.16/12, 192.168/16, link-local fe80::/10 or this PC itself.
/// An IPv4 address seen through IPv6 (::ffff:a.b.c.d) counts as IPv4.
pub fn is_private(addr: &str) -> bool {
    static V4: LazyLock<Regex> = LazyLock::new(|| re(r"^([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})\.([0-9]{1,3})$"));
    static LINK_LOCAL: LazyLock<Regex> = LazyLock::new(|| re("^fe[89ab][0-9a-f]:"));
    let low = addr.to_lowercase();
    let a = low.split('%').next().unwrap_or("");
    let a = a.strip_prefix("::ffff:").filter(|rest| V4.is_match(rest)).unwrap_or(a);
    if let Some(m) = V4.captures(a) {
        let part = |i: usize| m[i].parse::<u32>().unwrap_or(999);
        if (1..=4).any(|i| part(i) > 255) {
            return false;
        }
        return part(1) == 10 || (part(1) == 172 && (16..=31).contains(&part(2))) || (part(1) == 192 && part(2) == 168) || part(1) == 127;
    }
    a == "::1" || LINK_LOCAL.is_match(a)
}

/// This PC's address on the home network. Nothing is sent: each address asked about only says which adapter
/// traffic to it would leave by.
pub fn lan_address() -> Option<String> {
    // (a test's own address)
    if let Some(ip) = std::env::var("RUSTYSHELF_LAN_ADDRESS").ok().filter(|ip| is_private(ip)) {
        return Some(ip);
    }
    let leaves_by = |target: String| -> Option<Ipv4Addr> {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect(target).ok()?;
        match socket.local_addr().ok()?.ip() {
            IpAddr::V4(ip) => Some(ip),
            IpAddr::V6(_) => None,
        }
    };
    let usual = leaves_by("10.255.255.255:9".into()).filter(|ip| is_private(&ip.to_string()) && !ip.is_loopback());
    if usual.is_some_and(|ip| ip.octets()[..2] == [192, 168]) {
        return usual.map(|ip| ip.to_string());
    }
    // the usual way out is something else (a VPN, or none): an adapter that's on a 192.168 network itself. One
    // that is that network's first address is a virtual machine's host side, not a home network
    let direct = (0..=255u8).find_map(|n| leaves_by(format!("192.168.{n}.1:9")).filter(|ip| ip.octets()[..3] == [192, 168, n] && ip.octets()[3] != 1));
    direct.or(usual).map(|ip| ip.to_string())
}

/// A phone's name: the one it gave, without anything odd, else what its browser says it is.
pub fn phone_name(given: &str, agent: &str) -> String {
    static MODEL: LazyLock<Regex> = LazyLock::new(|| re(r";\s*([^;)]+?)\s+Build/"));
    let plain: String = given.chars().filter(|c| c.is_alphabetic() || c.is_numeric() || " '’()._-".contains(*c)).collect();
    let plain = js::slice(js::trim(&plain), 40);
    if !plain.is_empty() {
        return plain.to_string();
    }
    if agent.contains("iPad") {
        "iPad".into()
    } else if agent.contains("iPhone") {
        "iPhone".into()
    } else if agent.contains("Android") {
        js::slice(MODEL.captures(agent).map_or("Android phone", |m| m.get(1).unwrap().as_str()), 40).to_string()
    } else {
        "Phone".into()
    }
}

/// The file a picture's address names: the Electron app's own addresses, a file: address, or a path as it is.
pub fn file_of(u: &str) -> Option<String> {
    let plain = u.split('?').next().unwrap_or("");
    if let Some(rest) = u.strip_prefix("bsfile://local/") {
        return js::decode_uri_component(rest.split('?').next().unwrap_or(""));
    }
    if u.starts_with("file:") {
        let path = url::Url::parse(plain).ok()?.to_file_path().ok()?;
        return Some(path.to_string_lossy().into_owned());
    }
    (!u.is_empty() && !u.contains("://")).then(|| u.to_string())
}

fn image_type(file: &str) -> Option<&'static str> {
    Some(match js::extname(file).to_lowercase().as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        ".gif" => "image/gif",
        ".bmp" => "image/bmp",
        ".avif" => "image/avif",
        // (no SVG: it could carry script on this origin)
        _ => return None,
    })
}

// ---------- what the app hands over ----------

/// What the phone does on the TV.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// a button of the remote going down or coming up
    Button(&'static str, bool),
    /// "Backspace", "Enter" or "Escape"
    Key(&'static str),
    Text(String),
}

/// What's running, as the app knows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Running {
    pub id: String,
    pub title: String,
    pub kind: String,
}

/// The app the companion serves.
pub trait Host: Send + Sync {
    /// The shelves as last read: `[{ id, name, kind, games: [{ id, title, kind, file, appid, region, sizeBytes,
    /// art: { front, icon, hero, back, manual: [], manualPdf }, info }] }]`, pictures as files.
    fn library(&self) -> Arc<Value>;
    fn hidden(&self) -> Vec<String>;
    /// Whose letters the pad's buttons carry ("xbox", "ps", "nintendo"), and whether the right one confirms.
    fn glyphs(&self) -> (String, bool);
    fn home_menu(&self) -> bool;
    fn running(&self) -> Option<Running>;
    /// Starts a game as pressing A on its case would; false when that can't be done now.
    fn play(&self, shelf: &str, id: &str, title: &str) -> bool;
    fn quit(&self);
    fn input(&self, input: Input);
    /// A game's achievements: `{ earned, total, points, pointsEarned, mastered }`.
    fn achievements(&self, shelf: &str, title: &str) -> Option<Value>;
    fn has_achievements(&self, shelf: &str) -> bool;
    fn version(&self) -> String;
}

// ---------- requests and answers ----------

/// A request as it came in.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// the address it came from
    pub ip: String,
    pub method: String,
    /// the path and the query
    pub url: String,
    /// by name, in lower case
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// The answer to one.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

const HEAD: [(&str, &str); 4] = [("X-Content-Type-Options", "nosniff"), ("Referrer-Policy", "no-referrer"), ("X-Frame-Options", "DENY"), ("Cross-Origin-Resource-Policy", "same-origin")];
const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self'; script-src 'self'; font-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

fn send(status: u16, body: Vec<u8>, kind: &str, extra: &[(&str, String)]) -> Response {
    let mut headers: Vec<(String, String)> = HEAD.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    headers.push(("Content-Type".into(), kind.into()));
    headers.push(("Content-Length".into(), body.len().to_string()));
    headers.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
    Response { status, headers, body }
}

fn answer(status: u16, body: Value, extra: &[(&str, String)]) -> Response {
    let mut all = vec![("Cache-Control", "no-store".to_string())];
    all.extend(extra.iter().cloned());
    send(status, body.to_string().into_bytes(), "application/json; charset=utf-8", &all)
}

fn refuse(status: u16, why: &str) -> Response {
    answer(status, json!({ "error": why }), &[])
}

/// A request's body as the API takes it: JSON, an object, no bigger than `most`.
fn body_of(req: &Request, most: usize) -> Result<Map<String, Value>, Response> {
    let kind = req.headers.get("content-type").map_or(String::new(), |c| js::trim(c.split(';').next().unwrap_or("")).to_lowercase());
    if kind != "application/json" {
        return Err(refuse(415, "JSON only"));
    }
    if req.body.len() > most {
        return Err(refuse(413, "Too big"));
    }
    let text = String::from_utf8_lossy(&req.body);
    match serde_json::from_str::<Value>(if text.is_empty() { "{}" } else { &text }) {
        Ok(Value::Object(o)) => Ok(o),
        Ok(_) => Ok(Map::new()),
        Err(_) => Err(refuse(400, "Bad JSON")),
    }
}

/// A phone that has paired.
#[derive(Debug, Clone, PartialEq)]
pub struct Phone {
    pub id: String,
    pub name: String,
    /// its token's hash: the token itself is only the phone's
    hash: String,
    pub added: f64,
    pub seen: f64,
}

/// Where the companion stands, for Settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub enabled: bool,
    pub on: bool,
    /// its address on the home network, "" while it's off
    pub url: String,
    pub port: u16,
    pub want_port: u16,
    pub pin: String,
    /// why it couldn't start; and something to know though it did
    pub error: String,
    pub note: String,
    /// the minutes pairing stays locked after too many wrong PINs
    pub locked: u32,
    pub phones: Vec<Phone>,
}

struct Inner {
    enabled: bool,
    port: u16,
    phones: Vec<Phone>,
    pin: String,
    bad: u32,
    lock_until: f64,
    locks: u32,
    bound: Option<(String, u16)>,
    error: String,
    note: String,
    /// by random id: the pictures and the games the phone may ask for, from the library as it was last told
    files: HashMap<String, String>,
    games: HashMap<String, (Value, Value)>,
    told: Option<(Arc<Value>, String, Value)>,
    /// what each address may still ask, and when it last did
    buckets: HashMap<String, (f64, f64)>,
    /// pictures made smaller, by file, when it changed and height
    thumbs: HashMap<(String, u64, u32), Arc<Vec<u8>>>,
    stop: Option<Arc<AtomicBool>>,
}

pub struct Companion {
    store: PathBuf,
    /// new every run: yesterday's ids lead nowhere
    secret: Vec<u8>,
    host: Arc<dyn Host>,
    inner: Mutex<Inner>,
}

// the phone's page, as the Electron app serves it
static PAGE: [(&str, &str, &[u8]); 5] = [
    ("/", "text/html; charset=utf-8", include_bytes!("../assets/companion/index.html")),
    ("/app.js", "text/javascript; charset=utf-8", include_bytes!("../assets/companion/app.js")),
    ("/app.css", "text/css; charset=utf-8", include_bytes!("../assets/companion/app.css")),
    ("/icon.svg", "image/svg+xml", include_bytes!("../assets/companion/icon.svg")),
    ("/manifest.json", "application/manifest+json", include_bytes!("../assets/companion/manifest.json")),
];
static FONTS: [(&str, &[u8]); 3] = [
    ("/font/lexend-400.woff2", include_bytes!("../assets/companion/font/lexend-latin-400-normal.woff2")),
    ("/font/lexend-600.woff2", include_bytes!("../assets/companion/font/lexend-latin-600-normal.woff2")),
    ("/font/lexend-800.woff2", include_bytes!("../assets/companion/font/lexend-latin-800-normal.woff2")),
];

fn truthy_text(v: Option<&Value>) -> String {
    if js::truthy(v) { js::string(v) } else { String::new() }
}

impl Companion {
    /// With its pairings in the data folder's `companion.json` (not in the settings: backups never carry them).
    pub fn new(data_dir: &Path, host: Arc<dyn Host>) -> Arc<Companion> {
        static HASH: LazyLock<Regex> = LazyLock::new(|| re("^[0-9a-f]{64}$"));
        let store = data_dir.join("companion.json");
        let saved = crate::files::read_json(&store).unwrap_or(Value::Null);
        let mut phones: Vec<Phone> = saved["phones"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["hash"].as_str().is_some_and(|h| HASH.is_match(h)))
            .map(|p| Phone { id: js::string(p.get("id")), name: js::string(p.get("name")), hash: js::string(p.get("hash")), added: js::number(p.get("added")), seen: js::number(p.get("seen")) })
            .collect();
        let extra = phones.len().saturating_sub(MAX_PHONES);
        phones.drain(..extra);
        let inner = Inner {
            enabled: js::truthy(saved.get("enabled")),
            port: valid_port(js::number(saved.get("port"))).unwrap_or(DEFAULT_PORT),
            phones,
            pin: String::new(),
            bad: 0,
            lock_until: 0.0,
            locks: 0,
            bound: None,
            error: String::new(),
            note: String::new(),
            files: HashMap::new(),
            games: HashMap::new(),
            told: None,
            buckets: HashMap::new(),
            thumbs: HashMap::new(),
            stop: None,
        };
        Arc::new(Companion { store, secret: random(16), host, inner: Mutex::new(inner) })
    }

    fn save(&self, inner: &Inner) {
        let phones: Vec<Value> = inner.phones.iter().map(|p| json!({ "id": p.id, "name": p.name, "hash": p.hash, "added": js::num(p.added), "seen": js::num(p.seen) })).collect();
        let _ = crate::files::write_atomic(&self.store, json!({ "enabled": inner.enabled, "port": inner.port, "phones": phones }).to_string());
    }

    fn oid(&self, kind: &str, of: &str) -> String {
        let id = base64url(&hmac(&self.secret, format!("{kind}\0{of}").as_bytes()));
        format!("{kind}{}", &id[..22])
    }

    /// The id a picture goes out under; None for what isn't a picture.
    fn image_id(&self, inner: &mut Inner, u: Option<&Value>) -> Value {
        let Some(file) = file_of(&truthy_text(u)).filter(|f| image_type(f).is_some()) else { return Value::Null };
        let id = self.oid("i", &file);
        inner.files.insert(id.clone(), file);
        json!(id)
    }

    /// The library as the phone lists it, built again only when the library or the hidden games changed.
    fn library(&self, inner: &mut Inner) -> Value {
        let (src, hidden) = (self.host.library(), self.host.hidden());
        let key = hidden.join("\n");
        if let Some((_, _, told)) = inner.told.as_ref().filter(|t| Arc::ptr_eq(&t.0, &src) && t.1 == key) {
            return told.clone();
        }
        // rebuilt from this library only: nothing else can be named
        inner.files.clear();
        inner.games.clear();
        let mut shelves = Vec::new();
        for s in src.as_array().into_iter().flatten() {
            let Some(games) = s.get("games").and_then(Value::as_array).filter(|_| s["kind"] != "home") else { continue };
            let mut list = Vec::new();
            for g in games {
                let kind = g["kind"].as_str().unwrap_or("");
                if !js::truthy(Some(g)) || hidden.iter().any(|h| g["id"] == h.as_str()) || !matches!(kind, "rom" | "steam" | "app") {
                    continue;
                }
                let id = self.oid("g", &format!("{}\0{}", js::string(s.get("id")), js::string(g.get("id"))));
                inner.games.insert(id.clone(), (s.clone(), g.clone()));
                let art = &g["art"];
                let cover = [art.get("front"), art.get("icon")].into_iter().map(|u| self.image_id(inner, u)).find(|c| !c.is_null()).unwrap_or(Value::Null);
                let mut entry = json!({ "id": id, "t": truthy_text(g.get("title")), "c": cover, "m": u8::from(art["manual"].as_array().is_some_and(|m| !m.is_empty())) });
                if kind != "rom" {
                    entry["k"] = json!(kind);
                }
                list.push(entry);
            }
            if !list.is_empty() {
                let name = if js::truthy(s.get("name")) { js::string(s.get("name")) } else { js::string(s.get("id")) };
                shelves.push(json!({ "id": js::string(s.get("id")), "name": name, "kind": s["kind"], "games": list }));
            }
        }
        let told = json!({ "shelves": shelves });
        inner.told = Some((src, key, told.clone()));
        told
    }

    fn game(&self, inner: &mut Inner, id: &str) -> Option<(Value, Value)> {
        self.library(inner);
        inner.games.get(id).cloned()
    }

    fn game_view(&self, inner: &mut Inner, (s, g): &(Value, Value)) -> Value {
        let (art, info) = (&g["art"], &g["info"]);
        let cover = [art.get("front"), art.get("icon")].into_iter().map(|u| self.image_id(inner, u)).find(|c| !c.is_null()).unwrap_or(Value::Null);
        let manual: Vec<Value> = art["manual"].as_array().into_iter().flatten().map(|u| self.image_id(inner, Some(u))).filter(|i| !i.is_null()).collect();
        let kept: Map<String, Value> = ["overview", "release", "year", "dev", "pub", "genres", "players", "coop", "rating", "esrb"].iter().filter_map(|k| info.get(*k).filter(|v| !v.is_null() && **v != "").map(|v| (k.to_string(), v.clone()))).collect();
        let mut view = json!({ "id": self.oid("g", &format!("{}\0{}", js::string(s.get("id")), js::string(g.get("id")))), "t": g["title"], "shelf": s["name"], "shelfId": s["id"], "kind": g["kind"],
            "cover": cover, "hero": self.image_id(inner, art.get("hero")), "back": self.image_id(inner, art.get("back")), "manual": manual,
            "manualPdf": js::truthy(art.get("manualPdf")) && !art["manual"].as_array().is_some_and(|m| !m.is_empty()), "info": kept,
            "ra": g["kind"] == "rom" && self.host.has_achievements(&js::string(s.get("id"))) });
        for (name, from) in [("region", "region"), ("size", "sizeBytes")] {
            if js::truthy(g.get(from)) {
                view[name] = g[from].clone();
            }
        }
        view
    }

    /// What's running, by the title its case has.
    fn running(&self, inner: &mut Inner) -> Value {
        let Some(r) = self.host.running() else { return Value::Null };
        self.library(inner);
        let own = inner.games.values().find(|(s, g)| !r.id.is_empty() && (g["id"] == r.id.as_str() || format!("{}:{}", js::string(s.get("id")), js::string(g.get("file"))) == r.id || format!("steam-{}", js::string(g.get("appid"))) == r.id));
        match own {
            Some((_, g)) => json!({ "title": g["title"], "kind": if r.kind.is_empty() { g["kind"].clone() } else { json!(r.kind) } }),
            None => json!({ "title": r.title, "kind": r.kind }),
        }
    }

    fn glyphs(&self) -> Value {
        let (style, swap) = self.host.glyphs();
        json!({ "style": if matches!(style.as_str(), "ps" | "nintendo") { style.as_str() } else { "xbox" }, "swap": swap })
    }

    /// The phone a request comes from, by its token's hash.
    fn phone_of(&self, inner: &Inner, req: &Request) -> Option<usize> {
        static BETWEEN: LazyLock<Regex> = LazyLock::new(|| re(r";\s*"));
        let cookies = req.headers.get("cookie").map_or("", String::as_str);
        let token = BETWEEN.split(cookies).find_map(|c| c.strip_prefix(COOKIE).and_then(|rest| rest.strip_prefix('=')))?;
        if !(20..=200).contains(&token.encode_utf16().count()) {
            return None;
        }
        let hash = sha(token);
        inner.phones.iter().position(|p| same(p.hash.as_bytes(), hash.as_bytes()))
    }

    /// A few requests a second from an address is plenty for a remote; bursts are let through (opening the
    /// library loads many covers).
    fn allow(&self, inner: &mut Inner, ip: &str, cost: f64) -> bool {
        let now = crate::js::date_now();
        if !inner.buckets.contains_key(ip) && inner.buckets.len() > 256 {
            inner.buckets.clear();
        }
        let b = inner.buckets.entry(ip.to_string()).or_insert((now, 200.0));
        b.1 = (b.1 + (now - b.0) * 0.06).min(200.0);
        b.0 = now;
        if b.1 < cost {
            return false;
        }
        b.1 -= cost;
        true
    }

    fn new_pin(inner: &mut Inner) {
        let n = u32::from_le_bytes(random(4).try_into().expect("four bytes"));
        inner.pin = format!("{:06}", (u64::from(n) * 1_000_000) >> 32);
    }

    /// Answers a request; None for one that gets no answer at all (it isn't from the home network).
    pub fn handle(&self, req: &Request) -> Option<Response> {
        static NUMBERS: LazyLock<Regex> = LazyLock::new(|| re(r"^(?i)([0-9]{1,3}(\.[0-9]{1,3}){3}|\[[0-9a-f:.%]+\])(:[0-9]+)?$"));
        if !is_private(&req.ip) {
            return None;
        }
        let url = url::Url::parse("http://x").ok()?.join(if req.url.is_empty() { "/" } else { &req.url }).ok()?;
        let path = url.path().to_string();
        let query = |name: &str| url.query_pairs().find(|p| p.0 == name).map(|p| p.1.into_owned());
        let (get, post) = (req.method == "GET", req.method == "POST");
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        if !self.allow(inner, &req.ip, if path.starts_with("/img/") { 0.5 } else { 1.0 }) {
            return Some(answer(429, json!({ "error": "Slow down a little." }), &[("Retry-After", "2".into())]));
        }
        // an address typed as numbers only (what the QR code and Settings show): a web page under some other name
        // can't reach here by renaming itself to this PC
        let host = req.headers.get("host").cloned().unwrap_or_default();
        if !NUMBERS.is_match(&host) {
            return Some(refuse(421, "Use the address shown in RustyShelf."));
        }
        if post && req.headers.get("origin").is_some_and(|o| !o.is_empty() && *o != format!("http://{host}")) {
            return Some(refuse(403, "Wrong origin."));
        }
        if get {
            if let Some((_, kind, bytes)) = PAGE.iter().find(|p| p.0 == path) {
                let mut extra = vec![("Cache-Control", "no-cache".to_string())];
                if path == "/" {
                    extra.push(("Content-Security-Policy", CSP.into()));
                }
                return Some(send(200, bytes.to_vec(), kind, &extra));
            }
            if let Some((_, bytes)) = FONTS.iter().find(|f| f.0 == path) {
                return Some(send(200, bytes.to_vec(), "font/woff2", &[("Cache-Control", "max-age=604800".into())]));
            }
        }
        if post && path == "/api/pair" {
            return Some(self.pair(inner, req));
        }
        if !path.starts_with("/api/") && !path.starts_with("/img/") {
            return Some(refuse(404, "Not found."));
        }
        let Some(phone) = self.phone_of(inner, req) else { return Some(refuse(401, "Pair this phone first.")) };
        if crate::js::date_now() - inner.phones[phone].seen > 600_000.0 {
            inner.phones[phone].seen = crate::js::date_now();
            self.save(inner);
        }
        if get {
            return Some(match path.as_str() {
                p if p.starts_with("/img/") => self.picture(inner, &p[5..], query("h").as_deref()),
                "/api/me" => answer(200, json!({ "name": inner.phones[phone].name, "version": self.host.version() }), &[]),
                "/api/status" => answer(200, json!({ "running": self.running(inner), "glyphs": self.glyphs(), "homeMenu": self.host.home_menu() }), &[]),
                "/api/library" => {
                    let mut all = self.library(inner);
                    all["glyphs"] = self.glyphs();
                    answer(200, all, &[])
                }
                "/api/game" => match self.game(inner, &query("id").unwrap_or_default()) {
                    Some(e) => answer(200, self.game_view(inner, &e), &[]),
                    None => refuse(404, "That game is gone. Pull to refresh."),
                },
                "/api/ach" => {
                    let e = self.game(inner, &query("id").unwrap_or_default()).filter(|e| e.1["kind"] == "rom");
                    drop(guard);
                    let r = e.and_then(|(s, g)| self.host.achievements(&js::string(s.get("id")), &js::string(g.get("title"))));
                    // numbers only: the phone puts these into its page
                    let n = |v: &Value| { let x = js::number(Some(v)); if x.is_nan() { 0.0 } else { js::round(x).max(0.0) } };
                    let ach = r.filter(|r| n(&r["total"]) > 0.0).map(|r| json!({ "earned": js::num(n(&r["earned"])), "total": js::num(n(&r["total"])), "points": js::num(n(&r["points"])), "pointsEarned": js::num(n(&r["pointsEarned"])), "mastered": js::truthy(r.get("mastered")) }));
                    return Some(answer(200, json!({ "ach": ach }), &[]));
                }
                _ => refuse(404, "Not found."),
            });
        }
        if !post {
            return Some(refuse(405, "Not allowed."));
        }
        let o = match body_of(req, 4096) {
            Ok(o) => o,
            Err(no) => return Some(no),
        };
        Some(match path.as_str() {
            "/api/input" => {
                let Some(button) = BUTTONS.iter().find(|b| o.get("b").is_some_and(|x| js::truthy(Some(x)) && js::string(Some(x)) == **b)) else { return Some(refuse(400, "Unknown button.")) };
                self.host.input(Input::Button(button, js::truthy(o.get("d"))));
                answer(200, json!({ "ok": true }), &[])
            }
            "/api/text" => {
                if let Some(key) = o.get("key").and_then(Value::as_str) {
                    let Some(key) = ["Backspace", "Enter", "Escape"].into_iter().find(|k| *k == key) else { return Some(refuse(400, "Unknown key.")) };
                    self.host.input(Input::Key(key));
                    return Some(answer(200, json!({ "ok": true }), &[]));
                }
                let typed = match o.get("text") {
                    None | Some(Value::Null) => String::new(),
                    Some(t) => js::string(Some(t)),
                };
                let typed: String = typed.chars().filter(|c| !matches!(*c as u32, 0..=0x1f | 0x7f)).collect();
                let typed = js::slice(&typed, 200);
                if typed.is_empty() {
                    return Some(refuse(400, "Nothing to type."));
                }
                self.host.input(Input::Text(typed.to_string()));
                answer(200, json!({ "ok": true }), &[])
            }
            "/api/play" => {
                let Some((s, g)) = self.game(inner, &truthy_text(o.get("id"))) else { return Some(refuse(404, "That game is gone. Pull to refresh.")) };
                let running = self.running(inner);
                // the phone asks "Quit <game>?" and comes back with quit: true
                if !running.is_null() && !js::truthy(o.get("quit")) {
                    return Some(answer(409, json!({ "running": running }), &[]));
                }
                drop(guard);
                if !running.is_null() {
                    self.host.quit();
                    let since = Instant::now();
                    while self.host.running().is_some() && since.elapsed() < Duration::from_secs(15) {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    if self.host.running().is_some() {
                        let still = self.running(&mut self.inner.lock().unwrap());
                        return Some(answer(409, json!({ "running": still, "stuck": true }), &[]));
                    }
                }
                if self.host.play(&js::string(s.get("id")), &js::string(g.get("id")), &js::string(g.get("title"))) {
                    answer(200, json!({ "ok": true }), &[])
                } else {
                    refuse(503, "RustyShelf is busy. Try again in a moment.")
                }
            }
            "/api/quit" => {
                if self.running(inner).is_null() {
                    return Some(answer(200, json!({ "ok": true, "running": null }), &[]));
                }
                drop(guard);
                self.host.quit();
                answer(200, json!({ "ok": true }), &[])
            }
            "/api/unpair" => {
                inner.phones.remove(phone);
                self.save(inner);
                answer(200, json!({ "ok": true }), &[("Set-Cookie", format!("{COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict"))])
            }
            _ => refuse(404, "Not found."),
        })
    }

    fn pair(&self, inner: &mut Inner, req: &Request) -> Response {
        let o = match body_of(req, 1024) {
            Ok(o) => o,
            Err(no) => return no,
        };
        let now = crate::js::date_now();
        if now < inner.lock_until {
            return answer(429, json!({ "error": "Too many wrong PINs. Try again in a few minutes.", "retry": js::num(((inner.lock_until - now) / 1000.0).ceil()) }), &[]);
        }
        let digits: String = match o.get("pin") {
            None | Some(Value::Null) => String::new(),
            Some(p) => js::string(Some(p)),
        }
        .chars()
        .filter(char::is_ascii_digit)
        .take(6)
        .collect();
        let (given, want) = (format!("{digits:x<6}"), format!("{:-<6}", inner.pin));
        if inner.pin.is_empty() || !same(given.as_bytes(), want.as_bytes()) {
            inner.bad += 1;
            if inner.bad >= PIN_TRIES {
                // each lockout in a row lasts twice as long: someone guessing on the Wi-Fi gets about five tries an
                // hour; a pairing, or a new PIN asked for on the TV, starts again from ten minutes
                inner.bad = 0;
                let ms = LOCK_MS * f64::from(1u32 << inner.locks.min(5));
                inner.locks += 1;
                inner.lock_until = now + ms;
                return answer(429, json!({ "error": format!("Too many wrong PINs. Pairing is locked for {} minutes.", js::round(ms / 60_000.0)), "retry": js::num(ms / 1000.0) }), &[]);
            }
            let left = PIN_TRIES - inner.bad;
            return refuse(403, &format!("That PIN isn't right. {left} {} left.", if left == 1 { "try" } else { "tries" }));
        }
        (inner.bad, inner.locks) = (0, 0);
        let token = base64url(&random(32));
        let given_name = match o.get("name") {
            Some(n) if js::truthy(Some(n)) => js::string(Some(n)),
            _ => String::new(),
        };
        let phone = Phone { id: hex(&random(6)), name: phone_name(&given_name, req.headers.get("user-agent").map_or("", String::as_str)), hash: sha(&token), added: now, seen: now };
        let name = phone.name.clone();
        inner.phones.push(phone);
        let extra = inner.phones.len().saturating_sub(MAX_PHONES);
        inner.phones.drain(..extra);
        self.save(inner);
        // a PIN pairs one phone: someone who saw it over a shoulder can't use it again
        Self::new_pin(inner);
        answer(200, json!({ "ok": true, "name": name }), &[("Set-Cookie", format!("{COOKIE}={token}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict"))])
    }

    /// A picture by its id, made as small as the phone asked (`h`, its height) when it can be.
    fn picture(&self, inner: &mut Inner, id: &str, h: Option<&str>) -> Response {
        let Some(file) = inner.files.get(id).cloned() else { return refuse(404, "Not found.") };
        let (Some(kind), Ok(meta)) = (image_type(&file), fs::metadata(&file)) else { return refuse(404, "Not found.") };
        if !meta.is_file() {
            return refuse(404, "Not found.");
        }
        let cache = [("Cache-Control", "private, max-age=86400".to_string())];
        let asked = h.map_or(f64::NAN, |h| js::number(Some(&json!(h))));
        let asked = if asked.is_nan() { 0 } else { js::round(asked).max(0.0) as u32 };
        let tall = if asked >= 64 { Some(THUMB_H.into_iter().find(|x| *x >= asked).unwrap_or(1600)) } else { None };
        if let Some(tall) = tall {
            let changed = meta.modified().ok().and_then(crate::ms_of).unwrap_or(0);
            let key = (file.clone(), changed, tall);
            let made = inner.thumbs.get(&key).cloned().or_else(|| {
                let small = smaller(Path::new(&file), tall).map(Arc::new)?;
                if inner.thumbs.len() >= 96 {
                    inner.thumbs.clear();
                }
                inner.thumbs.insert(key, small.clone());
                Some(small)
            });
            if let Some(bytes) = made {
                return send(200, bytes.to_vec(), "image/jpeg", &cache);
            }
        }
        if meta.len() > 40 * 1024 * 1024 {
            return refuse(413, "Too big.");
        }
        match fs::read(&file) {
            Ok(bytes) => send(200, bytes, kind, &cache),
            Err(_) => refuse(404, "Not found."),
        }
    }

    // ---------- what the app calls ----------

    pub fn state(&self) -> State {
        let inner = self.inner.lock().unwrap();
        let on = inner.bound.is_some();
        let lock = (inner.lock_until - crate::js::date_now()).max(0.0);
        State {
            enabled: inner.enabled,
            on,
            url: inner.bound.as_ref().map_or(String::new(), |(ip, port)| format!("http://{ip}:{port}")),
            port: inner.bound.as_ref().map_or(inner.port, |b| b.1),
            want_port: inner.port,
            pin: if on { inner.pin.clone() } else { String::new() },
            error: inner.error.clone(),
            note: inner.note.clone(),
            locked: (lock / 60_000.0).ceil() as u32,
            phones: inner.phones.clone(),
        }
    }

    /// What the phone's camera reads to pair: the address with the PIN.
    pub fn pairing_link(&self) -> Option<String> {
        let s = self.state();
        (s.on && !s.pin.is_empty()).then(|| format!("{}/#pin={}", s.url, s.pin))
    }

    /// Switches it on or off, or moves it to another port (1024 to 65535).
    pub fn set(self: &Arc<Self>, enabled: Option<bool>, port: Option<u16>) -> Result<(), String> {
        let restart = {
            let mut inner = self.inner.lock().unwrap();
            let mut restart = false;
            if let Some(p) = port {
                let p = valid_port(f64::from(p)).ok_or("Choose a port from 1024 to 65535.")?;
                restart |= std::mem::replace(&mut inner.port, p) != p;
            }
            if let Some(on) = enabled {
                restart |= std::mem::replace(&mut inner.enabled, on) != on;
            }
            self.save(&inner);
            restart.then_some(inner.enabled)
        };
        match restart {
            Some(true) => self.start(),
            Some(false) => {
                self.stop();
                let mut inner = self.inner.lock().unwrap();
                (inner.error, inner.note) = (String::new(), String::new());
            }
            None => {}
        }
        Ok(())
    }

    /// Forgets a phone by its id, or all of them ("all").
    pub fn forget(&self, id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.phones.retain(|p| id != "all" && p.id != id);
        self.save(&inner);
    }

    /// A new PIN, asked for on the TV itself: a lockout from wrong guesses ends with the old one.
    pub fn renew_pin(&self) {
        let mut inner = self.inner.lock().unwrap();
        if inner.bound.is_some() {
            Self::new_pin(&mut inner);
            (inner.bad, inner.locks, inner.lock_until) = (0, 0, 0.0);
        }
    }

    /// Starts it when it's switched on (at the app's start).
    pub fn begin(self: &Arc<Self>) {
        if self.inner.lock().unwrap().enabled {
            self.start();
        }
    }

    /// Follows the PC to another address on the network (the router handed it a new one): to be asked now and
    /// then while it's on.
    pub fn follow_network(self: &Arc<Self>) {
        let (enabled, bound) = {
            let inner = self.inner.lock().unwrap();
            (inner.enabled, inner.bound.as_ref().map(|b| b.0.clone()))
        };
        if enabled && lan_address() != bound {
            self.start();
        }
    }

    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(stop) = inner.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        (inner.bound, inner.pin) = (None, String::new());
    }

    fn start(self: &Arc<Self>) {
        self.start_on(lan_address());
    }

    /// Listens on `ip`, at the port chosen or one of the nine after it.
    pub fn start_on(self: &Arc<Self>, ip: Option<String>) {
        self.stop();
        let mut inner = self.inner.lock().unwrap();
        (inner.error, inner.note) = (String::new(), String::new());
        let Some(ip) = ip else {
            inner.error = "No home network found. Connect this PC to your router (Wi-Fi or cable).".into();
            return;
        };
        let want = inner.port;
        for port in want..=want.saturating_add(9) {
            let Ok(addr) = format!("{ip}:{port}").parse::<SocketAddr>() else { break };
            match TcpListener::bind(addr) {
                Ok(listener) => {
                    if port != want {
                        inner.note = format!("Port {want} is used by another program, so {port} is used instead.");
                    }
                    let stop = Arc::new(AtomicBool::new(false));
                    (inner.bound, inner.stop, inner.bad) = (Some((ip.clone(), port)), Some(stop.clone()), 0);
                    Self::new_pin(&mut inner);
                    let me = self.clone();
                    std::thread::spawn(move || me.serve(listener, stop));
                    return;
                }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied) => {}
                Err(e) => {
                    inner.error = format!("Couldn't start: {e}");
                    return;
                }
            }
        }
        inner.error = format!("Ports {want}–{} are all in use. Choose another port.", want.saturating_add(9));
    }

    fn serve(self: Arc<Self>, listener: TcpListener, stop: Arc<AtomicBool>) {
        let _ = listener.set_nonblocking(true);
        let busy = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        while !stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, from)) => {
                    // from the home network only, and no more at once than a household's phones need
                    if !is_private(&from.ip().to_string()) || busy.load(Ordering::Relaxed) >= 64 {
                        continue;
                    }
                    let (me, busy) = (self.clone(), busy.clone());
                    busy.fetch_add(1, Ordering::Relaxed);
                    std::thread::spawn(move || {
                        me.talk(stream, from.ip());
                        busy.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(25)),
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    }

    /// One request and its answer; the connection closes after.
    fn talk(&self, mut stream: TcpStream, from: IpAddr) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));
        let Some(req) = read_request(&mut stream, &from.to_string()) else { return };
        let Some(res) = self.handle(&req) else { return };
        let mut head = format!("HTTP/1.1 {} {}\r\n", res.status, reason(res.status));
        for (name, value) in &res.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&res.body)).and_then(|()| stream.flush());
        // what's left unread of a body too big would reset the connection under the answer
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let (mut rest, mut buf) = (0, [0u8; 4096]);
        while let Ok(n @ 1..) = stream.read(&mut buf) {
            rest += n;
            if rest > 1 << 20 {
                break;
            }
        }
    }
}

fn valid_port(p: f64) -> Option<u16> {
    let p = js::round(p);
    (1024.0..=65535.0).contains(&p).then_some(p as u16)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Reads a request: its head (16 kB and forty headers at most) and a body of 8 kB at most.
fn read_request(stream: &mut TcpStream, from: &str) -> Option<Request> {
    let mut seen: Vec<u8> = Vec::new();
    let mut buf = [0u8; 2048];
    let head_end = loop {
        if let Some(at) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        if seen.len() > 16_384 {
            return None;
        }
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return None,
            Ok(n) => seen.extend_from_slice(&buf[..n]),
        }
    };
    let head = String::from_utf8_lossy(&seen[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, url) = (first.next()?.to_string(), first.next()?.to_string());
    let headers: HashMap<String, String> = lines.take(40).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
    let length: usize = headers.get("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut body = seen[head_end + 4..].to_vec();
    // a body too big is cut short here and refused by its length
    while body.len() < length.min(8192) {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&buf[..n]),
        }
    }
    if length > 8192 {
        body.resize(8193, 0);
    }
    Some(Request { ip: from.to_string(), method, url, headers, body })
}

/// A picture as a JPEG `tall` pixels high at most; None for one that can't be read here, or is small already.
fn smaller(file: &Path, tall: u32) -> Option<Vec<u8>> {
    let img = image::open(file).ok()?;
    if img.height() <= tall {
        return None;
    }
    let small = img.resize(u32::MAX, tall, image::imageops::FilterType::Triangle).into_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&small).ok()?;
    Some(out)
}

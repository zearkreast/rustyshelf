// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/covers.js and main/spreads.js (sizeOf), as RustyShelf has them.
//! Downloading box art. Front covers come from the libretro thumbnail library (names match No-Intro file names);
//! backs, spines, discs, and the fronts libretro doesn't have, from the LaunchBox Games Database. Neither needs an
//! account. A picture is saved next to the games in `<games folder>\covers` (or the art library) as
//! `<game file name>.png`, `-back.jpg`, `-spine.jpg`, `-disc.png`. A picture that's there is kept; one that's
//! replaced moves to `replaced\`, never deleted. The caches are in `data/boxart-cache`, shared with the Electron app.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::art::{art_base, ArtLibrary};
use crate::config::SystemConfig;
use crate::covers::{self, LibretroNames};
use crate::js;
use crate::lbindex::{self, GameImage, ShardFile, ShardsMeta};
use crate::paths::is_file;
use crate::scanner::Game;
use crate::steamart::is_image;
use crate::systems::Console;

/// The index format: a different one is built again (main/covers.js IDX_V).
const IDX_V: u32 = 8;
/// The XML is deleted once indexed, so a rebuild means the ~110 MB download again.
const IDX_DAYS: f64 = 60.0;
const LB_ZIP: &str = "https://gamesdb.launchbox-app.com/Metadata.zip";
const DAY: f64 = 86_400_000.0;
/// The picture types, as the scanner reads them; Linux tells "Game.PNG" from "Game.png".
const ART_EXT: [&str; 4] = ["png", "jpg", "jpeg", "webp"];
/// The parts of a game's box and the end of each one's file name.
pub const KINDS: [(&str, &str); 4] = [("front", ""), ("back", "-back"), ("spine", "-spine"), ("disc", "-disc")];
const REPLACED: &str = "replaced";

fn user_agent() -> String {
    format!("RustyShelf/{}", env!("CARGO_PKG_VERSION"))
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn ext_forms() -> Vec<String> {
    ART_EXT.iter().flat_map(|e| [e.to_string(), e.to_uppercase()]).collect()
}

// ---------- pictures on disk ----------

fn jpeg_size(h: &[u8]) -> Option<(u32, u32)> {
    let be16 = |o: usize| u32::from(u16::from_be_bytes([h[o], h[o + 1]]));
    let mut o = 2;
    while o + 9 < h.len() {
        if h[o] != 0xff {
            o += 1;
            continue;
        }
        let m = h[o + 1];
        if m == 0xd8 || m == 0x01 || (0xd0..=0xd7).contains(&m) {
            o += 2;
            continue;
        }
        // a frame header (not a Huffman table, JPEG extension or arithmetic conditioning)
        if (0xc0..=0xcf).contains(&m) && !matches!(m, 0xc4 | 0xc8 | 0xcc) {
            return Some((be16(o + 7), be16(o + 5)));
        }
        o += 2 + be16(o + 2) as usize;
    }
    None
}

/// A picture's width and height from its header (PNG, JPEG); None for other formats.
pub fn size_of(file: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut f = std::fs::File::open(file).ok()?;
    let mut read = |n: u64| {
        let mut b = Vec::new();
        std::io::Seek::rewind(&mut f).ok()?;
        (&mut f).take(n).read_to_end(&mut b).ok()?;
        Some(b)
    };
    let h = read(4096)?;
    if h.len() >= 24 && h.starts_with(b"\x89PNG") {
        let be32 = |o: usize| u32::from_be_bytes(h[o..o + 4].try_into().unwrap());
        return Some((be32(16), be32(20)));
    }
    if !h.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    // a frame header behind big EXIF / ICC blocks gets a second, longer look
    match jpeg_size(&h) {
        None if h.len() == 4096 => jpeg_size(&read(65536)?),
        z => z,
    }
}

/// The picture a game has for one part, whichever type it is.
pub fn art_file(dir: &Path, base: &str, suffix: &str) -> Option<PathBuf> {
    let forms = if cfg!(target_os = "linux") { ext_forms() } else { ART_EXT.iter().map(|e| e.to_string()).collect() };
    forms.iter().map(|e| dir.join(format!("{base}{suffix}.{e}"))).find(|f| is_file(f))
}

fn same_path(a: &Path, b: &Path) -> bool {
    crate::art::resolve(a).to_string_lossy().to_lowercase() == crate::art::resolve(b).to_string_lossy().to_lowercase()
}

/// A stamp for a replaced picture's name: the time, to the millisecond, with dashes.
pub(crate) fn art_stamp() -> String {
    let ms = now();
    let (days, rest) = ((ms / 86_400_000) as i64, ms % 86_400_000);
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}-{:02}-{:02}-{:02}-{:03}", rest / 3_600_000, rest / 60_000 % 60, rest / 1000 % 60, rest % 1000)
}

/// Puts a new picture in place. It's already on disk (`src`, under a temporary name the scanner ignores, in the same
/// folder). Every older picture of that kind moves to `replaced\` as `<base><suffix> (<stamp>).<ext>`, then `src` is
/// renamed in; if a step fails the moved ones go back, so a game is never left without its picture. `replaced\`
/// holds each distinct picture once: an older copy with the very bytes of one just moved there goes. Returns the
/// picture's file and the files moved.
pub fn place_art(dir: &Path, base: &str, suffix: &str, src: &Path, ext: &str) -> std::io::Result<(PathBuf, Vec<PathBuf>)> {
    place_art_as(dir, base, suffix, src, ext, "auto")
}

/// [`place_art`], with the picture on record (`crate::provenance`) as from `origin`.
pub fn place_art_as(dir: &Path, base: &str, suffix: &str, src: &Path, ext: &str, origin: &str) -> std::io::Result<(PathBuf, Vec<PathBuf>)> {
    let (dest, old, stamp) = (dir.join(format!("{base}{suffix}{ext}")), dir.join(REPLACED), art_stamp());
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut put = || -> std::io::Result<()> {
        for e in ext_forms() {
            let f = dir.join(format!("{base}{suffix}.{e}"));
            // on Windows "Game.PNG" is the same file as "Game.png", already moved
            if !is_file(&f) || same_path(&f, src) {
                continue;
            }
            let mut to = old.join(format!("{base}{suffix} ({stamp}).{e}"));
            let mut i = 2;
            // two in one millisecond: never over an earlier one
            while to.symlink_metadata().is_ok() {
                to = old.join(format!("{base}{suffix} ({stamp}-{i}).{e}"));
                i += 1;
            }
            std::fs::create_dir_all(&old)?;
            std::fs::rename(&f, &to)?;
            moved.push((f, to));
        }
        if !same_path(src, &dest) {
            std::fs::rename(src, &dest)?;
        }
        Ok(())
    };
    if let Err(e) = put() {
        for (f, to) in moved.iter().rev() {
            let _ = std::fs::rename(to, f);
        }
        return Err(e);
    }
    // a fresh date: cached thumbnails of the old picture aren't reused
    if let Ok(f) = std::fs::File::options().write(true).open(&dest) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    crate::provenance::record(&dest, origin);
    if !moved.is_empty() {
        let names: Vec<String> = std::fs::read_dir(&old).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        let others: Vec<PathBuf> = covers::replaced_for(&names, base, suffix).into_iter().map(|(n, _)| old.join(n)).filter(|f| !moved.iter().any(|m| same_path(&m.1, f))).collect();
        for (_, to) in &moved {
            let Ok(bytes) = std::fs::read(to) else { continue };
            let twin = others.iter().find(|o| o.metadata().is_ok_and(|m| m.len() == bytes.len() as u64) && std::fs::read(o).is_ok_and(|b| b == bytes));
            if let Some(twin) = twin {
                let _ = std::fs::remove_file(twin);
            }
        }
    }
    Ok((dest, moved.into_iter().map(|m| m.1).collect()))
}

// ---------- downloads ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// it isn't there (a 404, or too small to be a picture)
    Missing,
    /// worth another day: the server is busy, or answered with something that isn't a picture
    Busy,
}

/// Downloads a picture (`zip`: a .zip instead). A timeout, a dropped connection or a server error is tried once more
/// after a moment; a second network error is an Err (offline).
pub fn download_ex(url: &str, dest: &Path, zip: bool) -> Result<Outcome, String> {
    let mut tries = 0;
    loop {
        tries += 1;
        let (status, buf) = match crate::net::get(url, Duration::from_secs(45), 256 << 20, &user_agent()) {
            Ok(r) => r,
            Err(_) if tries == 1 => {
                std::thread::sleep(Duration::from_millis(1200));
                continue;
            }
            Err(e) => return Err(e),
        };
        if (200..300).contains(&status) {
            if buf.len() < 1024 {
                return Ok(Outcome::Missing);
            }
            // a 200 that isn't a picture (a Wi-Fi sign-in page, a CDN error page) is never saved as the cover
            if if zip { !buf.starts_with(b"PK\x03\x04") } else { !is_image(&buf) } {
                return Ok(Outcome::Busy);
            }
            let part = PathBuf::from(format!("{}.part", dest.display()));
            let saved = dest.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&part, &buf)).and_then(|()| std::fs::rename(&part, dest));
            return match saved {
                Ok(()) => Ok(Outcome::Ok),
                Err(e) => {
                    let _ = std::fs::remove_file(&part);
                    Err(if e.kind() == std::io::ErrorKind::StorageFull { "disk full".into() } else { e.to_string() })
                }
            };
        }
        if status >= 500 || status == 429 {
            if tries == 1 {
                std::thread::sleep(Duration::from_millis(1200));
                continue;
            }
            return Ok(Outcome::Busy);
        }
        // a 403 / 401 is often a CDN's passing block: not remembered as "not there"
        return Ok(if status == 404 || status == 410 { Outcome::Missing } else { Outcome::Busy });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetched {
    Placed,
    /// no sharper than the picture it would replace
    Same,
    /// the drive is full
    Full,
    Failed,
}

/// Downloads into a temporary name and keeps it only if it's a picture and, when replacing one (`compare`), clearly
/// sharper than it.
pub fn fetch_into(url: &str, dir: &Path, base: &str, suffix: &str, ext: &str, old: Option<&Path>, compare: bool) -> Fetched {
    fetch_into_as(url, dir, base, suffix, ext, old, compare, "auto")
}

/// [`fetch_into`], with the picture on record as from `origin`.
#[allow(clippy::too_many_arguments)]
fn fetch_into_as(url: &str, dir: &Path, base: &str, suffix: &str, ext: &str, old: Option<&Path>, compare: bool, origin: &str) -> Fetched {
    let tmp = dir.join(format!("{base}{suffix}.incoming{ext}"));
    let longest = |f: &Path| size_of(f).map(|z| z.0.max(z.1));
    let result = (|| -> Result<Fetched, String> {
        if download_ex(url, &tmp, false)? != Outcome::Ok {
            return Ok(Fetched::Failed);
        }
        if let Some(old) = old.filter(|_| compare) {
            let (a, b) = (longest(old), longest(&tmp));
            if b.is_none() || a.is_some_and(|a| f64::from(b.unwrap_or(0)) < f64::from(a) * 1.15) {
                let _ = std::fs::remove_file(&tmp);
                return Ok(Fetched::Same);
            }
        }
        place_art_as(dir, base, suffix, &tmp, ext, origin).map_err(|e| if e.kind() == std::io::ErrorKind::StorageFull { "disk full".to_string() } else { e.to_string() })?;
        Ok(Fetched::Placed)
    })();
    result.unwrap_or_else(|e| {
        let _ = std::fs::remove_file(&tmp);
        if e == "disk full" {
            Fetched::Full
        } else {
            Fetched::Failed
        }
    })
}

// ---------- libretro's list ----------

/// The names of a console's box art in libretro's thumbnail library, kept for a week in
/// `boxart-cache/libretro-<console>.json`. None when the console isn't there or the list can't be fetched.
pub fn libretro_names(cache_dir: &Path, console_id: &str) -> Option<LibretroNames> {
    static HREF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r#"(?i)href="([^"]+?\.png)""#).unwrap());
    let sys = covers::libretro_system(console_id)?;
    let file = cache_dir.join(format!("libretro-{console_id}.json"));
    let strings = |v: &Value| v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>());
    let cached: Option<Value> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(names) = cached.as_ref().filter(|c| now() as f64 - js::number(c.get("at")) < 7.0 * DAY).and_then(|c| strings(&c["names"])) {
        return Some(LibretroNames::new(names));
    }
    let url = format!("https://thumbnails.libretro.com/{}/Named_Boxarts/", js::encode_uri_component(sys));
    let (status, html) = crate::net::get(&url, Duration::from_secs(30), 64 << 20, &user_agent()).ok()?;
    if !(200..300).contains(&status) {
        return None;
    }
    let html = String::from_utf8_lossy(&html);
    let names: Vec<String> = HREF.captures_iter(&html).filter_map(|m| js::decode_uri_component(&m[1])).map(|n| n[..n.len() - 4].to_string()).collect();
    // an empty page (a layout change, an error page) isn't worth remembering for a week
    if !names.is_empty() {
        let _ = std::fs::create_dir_all(cache_dir).and_then(|()| std::fs::write(&file, json!({ "at": now(), "names": names }).to_string()));
    }
    Some(LibretroNames::new(names))
}

// ---------- the LaunchBox index ----------

/// The LaunchBox index, read one console at a time from its part (`launchbox-<platform>.json`, a few MB each).
pub struct Index {
    cache_dir: PathBuf,
    shards: HashMap<String, Option<ShardFile>>,
}

impl Index {
    /// One platform's games and their pictures.
    pub fn platform(&mut self, plat: &str) -> Option<&ShardFile> {
        let dir = &self.cache_dir;
        self.shards.entry(plat.to_string()).or_insert_with(|| std::fs::read(lbindex::shard_file_name(dir, plat)).ok().and_then(|b| serde_json::from_slice(&b).ok())).as_ref()
    }
}

fn all_platforms() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let consoles = crate::systems::systems().consoles.iter().filter_map(|c| covers::launchbox_platform(&c.id));
    for p in consoles.chain(covers::pc_box_platforms().iter().map(|p| p.1.as_str())) {
        if !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

fn index_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join("launchbox-index.json")
}

/// The split's details, when it's the split of the index file that's there (its stamp is that file's date).
fn shard_meta(cache_dir: &Path) -> Option<ShardsMeta> {
    let stamp = index_file(cache_dir).metadata().and_then(|m| m.modified()).ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64;
    let m: ShardsMeta = serde_json::from_slice(&std::fs::read(cache_dir.join("launchbox-shards.json")).ok()?).ok()?;
    (m.stamp != 0 && m.stamp == stamp).then_some(m)
}

/// The split of the index by console, made from the single file when it isn't there yet.
fn ensure_shards(cache_dir: &Path) -> Option<ShardsMeta> {
    shard_meta(cache_dir).or_else(|| {
        let f = index_file(cache_dir);
        is_file(&f).then(|| lbindex::split_index_file(&f, cache_dir).ok()).flatten()?;
        shard_meta(cache_dir)
    })
}

/// The index if it's on disk already: never downloads or builds it. An older one past its 60 days still serves.
pub fn ready_index(cache_dir: &Path) -> Option<Index> {
    ensure_shards(cache_dir).filter(|m| m.v >= 8).map(|_| Index { cache_dir: cache_dir.to_path_buf(), shards: HashMap::new() })
}

/// A Metadata.xml that's all there: it ends with `</LaunchBox>`.
fn xml_whole(xml: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(xml) else { return false };
    let n = f.metadata().map_or(0, |m| m.len());
    let mut b = Vec::new();
    f.seek(SeekFrom::Start(n.saturating_sub(512))).is_ok() && f.read_to_end(&mut b).is_ok() && String::from_utf8_lossy(&b).trim_end().ends_with("</LaunchBox>")
}

fn run(cmd: &str, args: &[&str], env: &[(&str, &str)]) -> bool {
    let mut c = std::process::Command::new(cmd);
    c.args(args).envs(env.iter().copied()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0800_0000); // CREATE_NO_WINDOW
    c.status().is_ok_and(|s| s.success())
}

/// Unpacks Metadata.xml into a folder of its own and moves it in only when complete: a cut-off unpack leaves nothing
/// that's later trusted as a fresh one.
fn unpack(zip: &Path, cache_dir: &Path) -> Result<(), String> {
    let tmp = cache_dir.join(format!(".unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let (z, t) = (zip.to_string_lossy().into_owned(), tmp.to_string_lossy().into_owned());
    let done = if cfg!(windows) {
        // paths go in through the environment, never into the command text
        run("tar", &["-xf", &z, "-C", &t, "Metadata.xml"], &[])
            || run("powershell", &["-NoProfile", "-Command", "Expand-Archive -LiteralPath $env:BS_ZIP -DestinationPath $env:BS_DIR -Force"], &[("BS_ZIP", &z), ("BS_DIR", &t)])
    } else {
        // GNU tar can't read a .zip
        run("unzip", &["-o", "-q", &z, "Metadata.xml", "-d", &t], &[])
            || run("bsdtar", &["-xf", &z, "-C", &t, "Metadata.xml"], &[])
            || run("python3", &["-c", "import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extract('Metadata.xml', sys.argv[2])", &z, &t], &[])
    };
    let from = tmp.join("Metadata.xml");
    let result = if done && is_file(&from) { std::fs::rename(&from, cache_dir.join("Metadata.xml")).map_err(|e| e.to_string()) } else { Err("Couldn't unpack the LaunchBox database.".to_string()) };
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// The index, downloaded and built if there's none, it's of another format, misses a platform or is over 60 days
/// old. `progress`: what's going on, for the screen.
pub fn launchbox_index(cache_dir: &Path, progress: &mut dyn FnMut(&str)) -> Result<Index, String> {
    let (zip, xml, idx) = (cache_dir.join("Metadata.zip"), cache_dir.join("Metadata.xml"), index_file(cache_dir));
    let plats = all_platforms();
    let fresh = |m: &ShardsMeta| m.v == IDX_V && now() as f64 - (m.at as f64) < IDX_DAYS * DAY && plats.iter().all(|p| m.plats.contains(p));
    if ensure_shards(cache_dir).is_some_and(|m| fresh(&m)) {
        let _ = std::fs::remove_file(&xml);
        return Ok(Index { cache_dir: cache_dir.to_path_buf(), shards: HashMap::new() });
    }
    let age = |f: &Path| f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_millis() as f64);
    // a Metadata.xml left by a build that was cut short: read it rather than download it again, unless it's cut off
    if !(is_file(&xml) && age(&xml).is_some_and(|a| a < 30.0 * DAY) && xml_whole(&xml)) {
        progress("Downloading the LaunchBox game database (one time, about a minute)…");
        std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
        let part = cache_dir.join("Metadata.zip.part");
        let got = crate::net::download(LB_ZIP, &part, Duration::from_secs(600), &mut |pct| progress(&format!("Downloading the LaunchBox game database… {pct}%"))).and_then(|()| std::fs::rename(&part, &zip).map_err(|e| e.to_string()));
        if let Err(e) = got {
            let _ = std::fs::remove_file(&part);
            return Err(format!("LaunchBox database: {e}"));
        }
        progress("Unpacking the game database…");
        let unpacked = unpack(&zip, cache_dir);
        let _ = std::fs::remove_file(&zip);
        unpacked?;
    }
    progress("Reading the game database… 0%");
    let built = lbindex::build_index_file(&xml, &idx, &plats, IDX_V, Some(cache_dir), |pct| progress(&format!("Reading the game database… {pct}%"))).map_err(|e| e.to_string());
    // the 500 MB XML is only needed while the index is built
    let _ = std::fs::remove_file(&xml);
    built?;
    ready_index(cache_dir).ok_or_else(|| "Couldn't read the LaunchBox index.".to_string())
}

// ---------- a download run ----------

/// Where a run's pictures go and what it fetches.
pub struct Run<'a> {
    pub art: &'a ArtLibrary,
    pub app_root: &'a Path,
    /// `data/boxart-cache`
    pub cache_dir: &'a Path,
    /// "front", "back", "spine", "disc"; "redo-<part>" fetches a part again
    pub kinds: &'a [&'a str],
    /// Settings → Box art region
    pub region: &'a str,
    /// sharpest first, and a low-res picture RustyShelf placed is replaced by a clearly sharper one
    pub hd: bool,
    /// started by the user (who's told it replaces low-res art): pictures from before the first look may be
    /// replaced too
    pub explicit: bool,
    /// the settings (`config.json`): the consoles' Art Studio styles and the parts locked by hand
    pub cfg: &'a Value,
    /// only a LaunchBox index already built: none is downloaded for this run
    pub index_ready_only: bool,
}

/// A console and the games of it to look at.
pub struct Wanted<'a> {
    pub console: &'a Console,
    pub config: &'a SystemConfig,
    pub games: Vec<&'a Game>,
}

/// What a run did for one console.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsoleReport {
    pub id: String,
    pub name: String,
    pub total: usize,
    pub front: usize,
    pub back: usize,
    pub spine: usize,
    pub disc: usize,
    /// low-res pictures replaced by sharper ones
    pub upgraded: usize,
    /// games that had everything asked for already
    pub had: usize,
    /// games still missing a part
    pub missed: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub systems: Vec<ConsoleReport>,
    pub cancelled: bool,
    pub launchbox_error: Option<String>,
    /// the art folders on a drive that ran out of space: nothing more was tried there
    pub full: Vec<PathBuf>,
}

/// The picture types auto-fetch falls back on when a game has no scan of the part itself (main/covers.js
/// OLD_FALLBACK).
fn fallback(kind: &str) -> &'static [&'static str] {
    match kind {
        "Box - Front" => &["Fanart - Box - Front", "Steam Poster"],
        "Box - Back" => &["Fanart - Box - Back"],
        "Disc" => &["Cart - Front", "Fanart - Cart - Front", "Fanart - Disc"],
        _ => &[],
    }
}

// (main/covers.js LB_FALLBACK: keep in sync)
fn styled_fallback(kind: &str) -> &'static [&'static str] {
    match kind {
        "Box - Front" => &["Box - Front - Reconstructed", "Fanart - Box - Front", "Steam Poster"],
        "Box - Back" => &["Box - Back - Reconstructed", "Fanart - Box - Back"],
        _ => fallback(kind),
    }
}

fn ext_of(file: &str) -> String {
    let e = js::extname(file).to_lowercase();
    if matches!(e.as_str(), ".png" | ".jpg" | ".jpeg" | ".webp") {
        e
    } else {
        ".jpg".into()
    }
}

/// A game's pictures of one LaunchBox type and its fallbacks, the best first, at most six. `styled`: for a console with an
/// Art Studio style, whose fallbacks take the clean rebuilds too.
fn launchbox_images(shard: &ShardFile, title: &str, kind: &str, region: &str, styled: bool) -> Vec<GameImage> {
    static BUNDLE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)\s(?:\+|&|and)\s").unwrap());
    let types: Vec<&str> = std::iter::once(kind).chain(if styled { styled_fallback(kind) } else { fallback(kind) }.iter().copied()).collect();
    let of = |title: &str| -> Vec<&GameImage> {
        let Some(g) = covers::launchbox_match(&shard.games, &shard.images, title) else { return Vec::new() };
        let imgs = shard.images.get(&g.id).map_or(&[][..], Vec::as_slice);
        types.iter().flat_map(|t| covers::pick_images(imgs, t, region)).collect()
    };
    let mut c = of(title);
    // compilations ("A + B"): borrow from the first game in the bundle
    if c.is_empty() && BUNDLE.is_match(title) {
        c = of(BUNDLE.split(title).next().unwrap_or(""));
    }
    c.into_iter().take(6).cloned().collect()
}

fn lb_url(im: &GameImage) -> (String, String) {
    (format!("{}{}", covers::LB_IMG, js::encode_uri(&im.file)), ext_of(&im.file))
}

/// A case's front and its spine, each for its height, by console: what a full cover scan is cut by (the Electron
/// app's main.js CASE, keep in sync).
fn case_of(console: &str) -> (f32, f32) {
    match console {
        "wiiu" => (0.711, 0.058),
        "ps3" | "ps4" | "ps5" => (0.789, 0.07),
        "sw" | "sw2" => (0.618, 0.059),
        "psp" => (0.578, 0.078),
        "n3ds" => (1.127, 0.112),
        _ => (0.711, 0.074),
    }
}

/// The spine or the back ("back") cut out of a full cover scan (back | spine | front), as a JPEG; None when the
/// picture isn't one or the fold isn't found.
pub fn cut_wrap(file: &Path, console: &str, part: &str) -> Option<Vec<u8>> {
    let img = image::ImageReader::open(file).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let (front, spine) = case_of(console);
    let cut = rustyshelf_imgtools::spine::cut_spine(&img, Some(front), Some(spine), if part == "back" { "back" } else { "spine" })?;
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).encode_image(&cut).ok()?;
    Some(out)
}

/// A picture, and clearly sharper than the one it would replace.
fn sharper(new: &Path, replacing: Option<&Path>) -> bool {
    let longest = |f: &Path| size_of(f).map(|z| f64::from(z.0.max(z.1)));
    longest(new).is_some_and(|b| replacing.and_then(longest).is_none_or(|a| b >= a * 1.15))
}

/// Whether a picture is low-res: its longest side under what a sharp one of that part has (pixels). A format whose
/// size can't be read counts as fine.
fn is_low(file: &Path, part: &str) -> bool {
    // (main/covers.js HD_MIN: keep in sync)
    let min = match part {
        "spine" => 700,
        "disc" => 480,
        _ => 900,
    };
    size_of(file).is_some_and(|z| z.0.max(z.1) < min)
}

/// Consoles whose disc part is a cartridge that can be drawn from the front cover (main/covers.js CART_SYS: keep in
/// sync).
pub const CART_SYS: [&str; 3] = ["sw", "sw2", "n3ds"];

/// A Switch or 3DS cartridge drawn from the game's front cover, as a PNG.
pub fn make_cart(front: &Path, console: &str) -> Option<Vec<u8>> {
    if !CART_SYS.contains(&console) {
        return None;
    }
    let img = image::ImageReader::open(front).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let cart = rustyshelf_imgtools::cart::make_cart(&img, console);
    let mut out = std::io::Cursor::new(Vec::new());
    cart.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner()).filter(|b| b.len() > 2000)
}

/// Puts a part cut from a full cover in its place, unless it's too small to be one or no sharper than `replacing`.
fn place_cut(bytes: &[u8], dir: &Path, base: &str, suffix: &str, replacing: Option<&Path>) -> bool {
    if bytes.len() <= 1500 || fs::create_dir_all(dir).is_err() {
        return false;
    }
    let incoming = dir.join(format!("{base}{suffix}.incoming-tdb.jpg"));
    let placed = fs::write(&incoming, bytes).is_ok() && sharper(&incoming, replacing) && place_art(dir, base, suffix, &incoming, ".jpg").is_ok();
    if !placed {
        let _ = fs::remove_file(&incoming);
    }
    placed
}

/// A GameCube or Wii game's spine, cut from GameTDB's full cover scan of its disc (found by the id in the disc).
fn gametdb_spine(console: &str, file: &Path, dir: &Path, base: &str, replacing: Option<&Path>, cache_dir: &Path, region: &str) -> bool {
    let Some(id) = covers::disc_id(file) else { return false };
    for r in covers::tdb_regions(&id, region) {
        for kind in ["coverfullHQ", "coverfull"] {
            let tmp = cache_dir.join(format!(".tdb-{}-{id}-{r}-{kind}.png", std::process::id()));
            let got = matches!(download_ex(&format!("https://art.gametdb.com/wii/{kind}/{r}/{id}.png"), &tmp, false), Ok(Outcome::Ok));
            let placed = got && cut_wrap(&tmp, console, "spine").is_some_and(|cut| place_cut(&cut, dir, base, "-spine", replacing));
            let _ = fs::remove_file(&tmp);
            if placed {
                return true;
            }
        }
    }
    false
}

/// A game's full box wraps on LaunchBox (back | spine | front), the best three for the region.
fn launchbox_wraps(shard: &ShardFile, title: &str, region: &str) -> Vec<String> {
    let Some(g) = covers::launchbox_match(&shard.games, &shard.images, title) else { return Vec::new() };
    let imgs = shard.images.get(&g.id).map_or(&[][..], Vec::as_slice);
    covers::pick_images(imgs, "Box - Full", region).into_iter().take(3).map(|im| im.file.clone()).collect()
}

/// A spine cut from one of these wraps: usually far sharper than a spine scan.
fn wrap_spine(wraps: &[String], console: &str, dir: &Path, base: &str, replacing: Option<&Path>, cache_dir: &Path, cancel: &AtomicBool) -> bool {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    for file in wraps {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let ext = Some(js::extname(file)).filter(|e| !e.is_empty()).unwrap_or(".jpg".into());
        let tmp = cache_dir.join(format!(".wrap-{}-{}{ext}", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
        let got = matches!(download_ex(&format!("{}{}", covers::LB_IMG, js::encode_uri(file)), &tmp, false), Ok(Outcome::Ok));
        // (wider than tall, or it isn't a wrap)
        let placed = got && !size_of(&tmp).is_some_and(|(w, h)| (w as f32) < h as f32 * 1.2) && cut_wrap(&tmp, console, "spine").is_some_and(|cut| place_cut(&cut, dir, base, "-spine", replacing));
        let _ = fs::remove_file(&tmp);
        if placed {
            return true;
        }
    }
    false
}

/// A Switch or 3DS game's full cover scan from GameTDB (found by its title), downloaded to a file in the cache:
/// its back and its spine are both cut from it.
fn gametdb_wrap(map: &serde_json::Map<String, serde_json::Value>, console: &str, title: &str, cache_dir: &Path, region: &str, cancel: &AtomicBool) -> Option<PathBuf> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let prefer = covers::art_region(region).map_or(&[][..], |r| r.tdb);
    for c in crate::gametdb::full_covers(map, console, &covers::lookup_names(title), prefer) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let tmp = cache_dir.join(format!(".tdbfull-{}-{}.jpg", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)));
        // (wide enough to be a whole wrap, not a front on its own)
        if matches!(download_ex(&c.url, &tmp, false), Ok(Outcome::Ok)) && size_of(&tmp).is_some_and(|(w, h)| w as f32 >= h as f32 * 1.2) {
            return Some(tmp);
        }
        let _ = fs::remove_file(&tmp);
    }
    None
}

/// A game in the box art databases a game's art can be matched to.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtMatch {
    pub name: String,
    /// LaunchBox's (else libretro's, a front cover only)
    pub launchbox: bool,
    /// its box and disc pictures
    pub pictures: usize,
}

/// A console's games in the box art databases whose names hold every word of `term`, the closest names first (24 at
/// most), and whether the LaunchBox index is built. The index is never built for this; libretro's list is fetched
/// once a week.
pub fn art_matches(cache_dir: &Path, sys: &str, term: &str) -> (Vec<ArtMatch>, bool) {
    let mut idx = ready_index(cache_dir);
    let ready = idx.is_some();
    let want = crate::lbindex::norm(term);
    if want.is_empty() {
        return (Vec::new(), ready);
    }
    let words: Vec<&str> = want.split(' ').filter(|w| !w.is_empty()).collect();
    let fits = |n: &str| words.iter().all(|w| n.contains(w));
    let mut out: Vec<(String, ArtMatch)> = Vec::new();
    if let Some(shard) = idx.as_mut().zip(covers::launchbox_platform(sys)).and_then(|(i, p)| i.platform(p)) {
        for g in shard.games.iter().filter(|g| !g.n.is_empty() && fits(&g.n)) {
            let pictures = shard.images.get(&g.id).map_or(0, |l| l.iter().filter(|i| i.r#type.starts_with("Box - ") || i.r#type == "Disc").count());
            let m = ArtMatch { name: g.name.clone(), launchbox: true, pictures };
            match out.iter_mut().find(|x| x.0 == g.n) {
                Some(prev) if prev.1.pictures >= pictures => {}
                Some(prev) => prev.1 = m,
                None => out.push((g.n.clone(), m)),
            }
        }
    }
    for n in libretro_names(cache_dir, sys).as_ref().map_or(&[][..], LibretroNames::names) {
        let title = crate::titles::clean_title(&format!("{n}.png"), true).title;
        let k = crate::lbindex::norm(&title);
        if !k.is_empty() && fits(&k) && !out.iter().any(|x| x.0 == k) {
            out.push((k, ArtMatch { name: title, launchbox: false, pictures: 1 }));
        }
    }
    let rank = |m: &ArtMatch| {
        let k = crate::lbindex::norm(&m.name);
        (if k == want { 3 } else if k.starts_with(&want) { 2 } else { 1 }) * 1_000_000 - k.chars().count() as i64 * 10 + m.pictures.min(9) as i64
    };
    let mut list: Vec<ArtMatch> = out.into_iter().map(|x| x.1).collect();
    list.sort_by(|a, b| rank(b).cmp(&rank(a)).then_with(|| a.name.cmp(&b.name)));
    list.truncate(24);
    (list, ready)
}

/// Fetches the parts these games don't have. `progress(console name or what's going on, done, total)` is called as
/// games finish; setting `cancel` stops after the games in flight.
pub fn download(run: &Run, systems: &[Wanted], progress: &(dyn Fn(&str, usize, usize) + Sync), cancel: &AtomicBool) -> Report {
    let Run { art, app_root, cache_dir, kinds, region, hd, explicit, cfg, index_ready_only } = *run;
    let region = if covers::art_region(region).is_some() { region } else { "auto" };
    let want = |k: &str| kinds.contains(&k) || kinds.contains(&format!("redo-{k}").as_str());
    let redo = |k: &str| kinds.contains(&format!("redo-{k}").as_str());
    let mut report = Report::default();
    let mut index: Option<Mutex<Index>> = None;
    let full: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    for sys in systems.iter().filter(|s| !s.games.is_empty()) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let id = sys.console.id.as_str();
        let posters = if want("front") { cache_dir.parent().and_then(crate::steamgrid::SteamGrid::from_settings) } else { None };
        // a console with an Art Studio style: each part follows the style's look
        let style = cfg["art"]["styles"].get(id).filter(|s| js::truthy(Some(s))).map(|_| crate::artstudio::style_for(cfg, id));
        progress(&sys.console.name, 0, sys.games.len());
        let names = if want("front") { libretro_names(cache_dir, id) } else { None };
        if index.is_none() && index_ready_only {
            index = ready_index(cache_dir).map(Mutex::new);
        } else if index.is_none() && report.launchbox_error.is_none() {
            match launchbox_index(cache_dir, &mut |stage| progress(stage, 0, 0)) {
                Ok(i) => index = Some(Mutex::new(i)),
                Err(e) => report.launchbox_error = Some(e),
            }
        }
        let platform = covers::launchbox_platform(id);
        // Switch / 3DS: GameTDB's games by title, for the backs and spines no other source has
        let tdb = if crate::gametdb::has(id) && (want("back") || want("spine")) { crate::gametdb::index(cache_dir, id) } else { None };
        let rep = Mutex::new(ConsoleReport { id: id.to_string(), name: sys.console.name.clone(), total: sys.games.len(), ..ConsoleReport::default() });
        let (next, done) = (AtomicUsize::new(0), AtomicUsize::new(0));
        let one = |game: &Game| {
            // looked up under the name picked in Details → Art match
            let renamed = covers::art_name(cfg, &game.id).filter(|n| *n != game.title).map(|n| Game { title: n.to_string(), ..game.clone() });
            let game = renamed.as_ref().unwrap_or(game);
            let (dir, base) = (art.dir_for(sys.config, &game.file, app_root), art_base(&game.file));
            let (mut touched, mut missing) = (false, false);
            // (downloaded once for both the back and the spine, deleted after)
            let mut wrap: Option<Option<PathBuf>> = None;
            // parts chosen or kept by hand in the Art Studio are left alone
            let locks = crate::artstudio::locked_parts(cfg, id, &game.file);
            for (key, suffix) in KINDS {
                if !want(key) || locks.contains(&key) || cancel.load(Ordering::Relaxed) || full.lock().unwrap().contains(&dir) {
                    continue;
                }
                let look = style.as_ref().map(|s| crate::artstudio::look_for(s, key));
                // "sharper" (kept inside the default style) and the HD look both mean: sharpest first, low-res replaced
                let sharp = hd || look.as_deref() == Some("hd") || style.as_ref().zip(look.as_deref()).is_some_and(|(s, l)| s["sharper"] == true && l != "special" && !l.starts_with("region:"));
                let reg = style.as_ref().zip(look.as_deref()).map_or(region.to_string(), |(s, l)| crate::artstudio::region_for_look(s, l, &game.region));
                let reg = reg.as_str();
                let old = art_file(&dir, &base, suffix);
                let upgrade = sharp && !redo(key) && old.as_deref().is_some_and(|f| is_low(f, key) && crate::provenance::may_replace(f, explicit));
                // a SteamGridDB poster standing in for a front no source had: a real scan replaces it whenever one is found
                let poster_old = key == "front" && !redo(key) && !upgrade && old.as_deref().is_some_and(crate::provenance::is_poster);
                if old.is_some() && !redo(key) && !upgrade && !poster_old {
                    continue;
                }
                let kind = match key {
                    "front" => "Box - Front",
                    "back" => "Box - Back",
                    "spine" => "Box - Spine",
                    _ => "Disc",
                };
                // libretro first for a front (its names match file names best), or last for HD: LaunchBox's scans are
                // usually 1000–2500 px, libretro's thumbnails smaller
                let lb: Vec<GameImage> = match (&index, platform) {
                    (Some(index), Some(plat)) => index.lock().unwrap().platform(plat).map(|shard| launchbox_images(shard, &game.title, kind, reg, style.is_some())).unwrap_or_default(),
                    _ => Vec::new(),
                };
                let cands: Vec<(String, String)> = match (&style, &look) {
                    // every version the sources have, in the order the look ranks them (as the Art Studio's picker)
                    (Some(style), Some(look)) => {
                        let lr = if key == "front" { names.as_ref().map(|n| n.all(&game.file, &game.title)).unwrap_or_default() } else { Vec::new() };
                        let mut all: Vec<crate::artstudio::Cand> = lb.iter().map(crate::artstudio::lb_cand).collect();
                        all.extend(lr.iter().map(|n| crate::artstudio::lr_cand(id, n, &base)));
                        let ctx = crate::artstudio::Ctx { style, game_region: &game.region, sharp: Some(sharp) };
                        crate::artstudio::rank_list(&all, look, &ctx).into_iter().filter(|c| c.cut.is_none()).map(|c| (c.url.clone(), c.ext.clone())).collect()
                    }
                    // libretro first for a front (its names match file names best), or last for HD: LaunchBox's
                    // scans are usually 1000–2500 px, libretro's thumbnails smaller
                    _ => {
                        let hit = if key == "front" { names.as_ref().and_then(|n| n.find(&game.file, &game.title, &game.region, region)) } else { None };
                        let lr = hit.and_then(|h| covers::libretro_url(id, &h)).map(|u| (u, ".png".to_string()));
                        let lb = lb.iter().map(lb_url);
                        if hd { lb.chain(lr).collect() } else { lr.into_iter().chain(lb).collect() }
                    }
                };
                let wraps = || match (&index, platform) {
                    (Some(index), Some(plat)) => index.lock().unwrap().platform(plat).map(|shard| launchbox_wraps(shard, &game.title, reg)).unwrap_or_default(),
                    _ => Vec::new(),
                };
                // a full box wrap gives a far sharper spine than most spine scans: tried first for HD, after them
                // otherwise (never both: a wrap turned down here would only be downloaded and turned down again)
                let wrap_first = key == "spine" && sharp;
                let mut ok = wrap_first && !cancel.load(Ordering::Relaxed) && wrap_spine(&wraps(), id, &dir, &base, old.as_deref(), cache_dir, cancel);
                for (url, ext) in cands.iter().filter(|_| !ok) {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    match fetch_into(url, &dir, &base, suffix, ext, old.as_deref(), upgrade) {
                        Fetched::Placed => {
                            ok = true;
                            break;
                        }
                        Fetched::Full => {
                            full.lock().unwrap().push(dir.clone());
                            break;
                        }
                        _ => {}
                    }
                }
                let stopped = || cancel.load(Ordering::Relaxed) || full.lock().unwrap().contains(&dir);
                // nothing from the databases: the Super Mario Wiki's official box scans (Switch 2)
                if !ok && (old.is_none() || poster_old) && matches!(key, "front" | "back") && crate::wikiart::has(id) && !stopped() {
                    let lower = reg.to_lowercase();
                    let wiki_region = if lower.contains("eur") { "eur" } else if lower.contains("jpn") || lower.contains("japan") { "jpn" } else { "usa" };
                    if let Some(w) = crate::wikiart::find(cache_dir, id, &covers::lookup_names(&game.title), key, wiki_region, now() as f64, &crate::wikiart::ask) {
                        ok = matches!(fetch_into(&w.url, &dir, &base, suffix, &format!(".{}", w.ext), None, false), Fetched::Placed);
                    }
                }
                // no box scan anywhere: SteamGridDB's best-voted poster (only with the user's key), on record as a poster
                if let (false, None, "front", Some(sg), false) = (ok, &old, key, &posters, stopped()) {
                    let names = covers::lookup_names(&game.title);
                    for p in sg.posters(&crate::steamgrid::Game { appid: "", title: &game.title, names: &names }).into_iter().take(3) {
                        if matches!(fetch_into_as(&p.url, &dir, &base, suffix, p.ext, None, false, "poster"), Fetched::Placed) {
                            ok = true;
                            break;
                        }
                    }
                }
                // still no real scan: the poster stays
                if poster_old && !ok {
                    continue;
                }
                if !ok && !wrap_first && key == "spine" && !stopped() {
                    ok = wrap_spine(&wraps(), id, &dir, &base, old.as_deref(), cache_dir, cancel);
                }
                let against = old.as_deref().filter(|_| upgrade);
                // GameCube / Wii spines: GameTDB has a full cover scan for nearly every disc, by its game id
                if !ok && (old.is_none() || upgrade) && key == "spine" && matches!(id, "gc" | "wii") && !stopped() {
                    ok = gametdb_spine(id, &game.file, &dir, &base, against, cache_dir, reg);
                }
                // Switch / 3DS: the spine and the back cut from GameTDB's full cover, found by the game's title
                if let (false, true, "spine" | "back", Some(map), false) = (ok, old.is_none() || upgrade, key, &tdb, stopped()) {
                    let full_cover = wrap.get_or_insert_with(|| gametdb_wrap(map, id, &game.title, cache_dir, reg, cancel));
                    ok = full_cover.as_ref().and_then(|f| cut_wrap(f, id, key)).is_some_and(|cut| place_cut(&cut, &dir, &base, suffix, against));
                }
                // a Switch / 3DS game no source has a cartridge photo of: one drawn from its front cover
                if !ok && old.is_none() && key == "disc" && CART_SYS.contains(&id) && !stopped() {
                    if let Some(png) = art_file(&dir, &base, "").and_then(|front| make_cart(&front, id)) {
                        let tmp = dir.join(format!("{base}{suffix}.incoming.png"));
                        ok = fs::write(&tmp, png).is_ok() && place_art(&dir, &base, suffix, &tmp, ".png").is_ok();
                        if !ok {
                            let _ = fs::remove_file(&tmp);
                        }
                    }
                }
                let mut rep = rep.lock().unwrap();
                match (ok, key) {
                    (true, _) if upgrade => rep.upgraded += 1,
                    (true, "front") => rep.front += 1,
                    (true, "back") => rep.back += 1,
                    (true, "spine") => rep.spine += 1,
                    (true, _) => rep.disc += 1,
                    (false, _) => missing |= old.is_none(),
                }
                touched |= ok;
            }
            if let Some(Some(f)) = wrap {
                let _ = fs::remove_file(f);
            }
            let mut rep = rep.lock().unwrap();
            if missing {
                rep.missed.push(game.title.clone());
            } else if !touched {
                rep.had += 1;
            }
            progress(&sys.console.name, done.fetch_add(1, Ordering::Relaxed) + 1, sys.games.len());
        };
        // four games at a time
        std::thread::scope(|s| {
            for _ in 0..sys.games.len().min(4) {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= sys.games.len() || cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    one(sys.games[i]);
                });
            }
        });
        report.systems.push(rep.into_inner().unwrap());
    }
    report.cancelled = cancel.load(Ordering::Relaxed);
    report.full = full.into_inner().unwrap();
    report
}

#[cfg(test)]
mod wrap_tests {
    use super::*;

    #[test]
    fn a_low_res_picture_is_only_replaced_by_a_clearly_sharper_one() {
        let tmp = tempfile::tempdir().unwrap();
        let pic = |name: &str, w: u32, h: u32| {
            let f = tmp.path().join(name);
            image::RgbImage::new(w, h).save(&f).unwrap();
            f
        };
        let (small, big, bit_bigger) = (pic("small.png", 400, 560), pic("big.png", 1000, 1400), pic("near.png", 420, 600));
        assert!(is_low(&small, "front") && !is_low(&big, "front") && !is_low(&pic("spine.png", 60, 800), "spine") && is_low(&pic("disc.png", 400, 400), "disc"));
        assert!(!is_low(&tmp.path().join("missing.png"), "front"));
        assert!(sharper(&big, Some(&small)) && !sharper(&bit_bigger, Some(&small)) && sharper(&small, None) && !sharper(&tmp.path().join("missing.png"), None));
    }

    /// A made-up full cover: a blue back, a dark spine and a red front, in a GameCube case's proportions.
    #[test]
    fn a_spine_and_a_back_are_cut_from_a_full_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, front, spine) = (400u32, 284u32, 30u32);
        let wrap = image::RgbImage::from_fn(front * 2 + spine, h, |x, y| {
            let band = if (y / 20) % 2 == 0 { 30 } else { 0 };
            if x < front {
                image::Rgb([20, 40 + band, 200])
            } else if x < front + spine {
                // (speckled, as print is: a flat one packs too small to be taken for a picture)
                let speck = ((x * 7919 + y * 104_729) % 97) as u8;
                image::Rgb([15 + speck / 2, 15 + speck / 3, 20 + speck / 2])
            } else {
                image::Rgb([200, 30 + band, 30])
            }
        });
        let file = tmp.path().join("full.png");
        wrap.save(&file).unwrap();
        let size = |bytes: &[u8]| image::load_from_memory(bytes).map(|i| (i.width(), i.height())).unwrap();
        let cut = cut_wrap(&file, "gc", "spine").expect("a spine");
        let (w, tall) = size(&cut);
        assert!(tall >= h - 4 && (spine - 8..=spine + 8).contains(&w), "the spine came out {w} × {tall}");
        let back = cut_wrap(&file, "gc", "back").expect("a back");
        assert!((front - 12..=front + 12).contains(&size(&back).0), "the back came out {} wide", size(&back).0);
        // a front on its own isn't a wrap; a cut too small isn't kept
        image::RgbImage::new(284, 400).save(tmp.path().join("front.png")).unwrap();
        assert!(cut_wrap(&tmp.path().join("front.png"), "gc", "spine").is_none());
        let art = tmp.path().join("art");
        assert!(place_cut(&cut, &art, "Zeta Run (USA)", "-spine", None) && art_file(&art, "Zeta Run (USA)", "-spine").is_some());
        assert!(!place_cut(&[0; 100], &art, "Other", "-spine", None) && art_file(&art, "Other", "-spine").is_none());
        assert_eq!((case_of("n3ds"), case_of("sw2"), case_of("nothing")), ((1.127, 0.112), (0.618, 0.059), (0.711, 0.074)));
    }
}

#[cfg(test)]
mod match_tests {
    use super::*;
    use crate::lbindex::{norm, shard_file_name, Game as LbGame, GameImage, ShardFile, ShardsMeta};

    #[test]
    fn art_matches_from_both_databases() {
        let cache = std::env::temp_dir().join(format!("rs-artmatch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cache);
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("libretro-ps2.json"), json!({ "at": now(), "names": ["Ratchet Clank (USA)", "Jak and Daxter (USA)"] }).to_string()).unwrap();
        let (list, ready) = art_matches(&cache, "ps2", "ratchet");
        assert!(!ready);
        assert_eq!(list.iter().map(|m| (m.name.as_str(), m.launchbox)).collect::<Vec<_>>(), [("Ratchet Clank", false)]);

        let plat = covers::launchbox_platform("ps2").unwrap();
        fs::write(cache.join("launchbox-index.json"), "{}").unwrap();
        let stamp = cache.join("launchbox-index.json").metadata().unwrap().modified().unwrap().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        fs::write(cache.join("launchbox-shards.json"), serde_json::to_vec(&ShardsMeta { v: IDX_V, at: 0, plats: vec![plat.to_string()], stamp }).unwrap()).unwrap();
        let game = |id: &str, name: &str| LbGame { id: id.into(), name: name.into(), n: norm(name), overview: None, release: None, year: None, dev: None, pub_: None, genres: None, esrb: None, players: None, coop: None, rating: None, wiki: None, video: None };
        let img = |kind: &str| GameImage { r#type: kind.into(), region: "North America".into(), file: "x.jpg".into() };
        let shard = ShardFile {
            v: IDX_V,
            plat: plat.into(),
            games: vec![game("1", "Ratchet & Clank"), game("2", "Ratchet: Deadlocked"), game("3", "Jak II")],
            images: [("1".to_string(), vec![img("Box - Front"), img("Box - Back"), img("Fanart - Background")])].into(),
        };
        fs::write(shard_file_name(&cache, plat), serde_json::to_vec(&shard).unwrap()).unwrap();
        let (list, ready) = art_matches(&cache, "ps2", "Ratchet");
        assert!(ready);
        // the closest names first: the shortest of those that start with the words
        assert_eq!(list.iter().map(|m| (m.name.as_str(), m.pictures)).collect::<Vec<_>>(), [("Ratchet Clank", 1), ("Ratchet & Clank", 2), ("Ratchet: Deadlocked", 0)]);
        assert_eq!(art_matches(&cache, "ps2", "clank ratchet").0.len(), 2, "every word, in any order");
        assert!(art_matches(&cache, "ps2", "  ").0.is_empty());
        let _ = fs::remove_dir_all(&cache);
    }
}

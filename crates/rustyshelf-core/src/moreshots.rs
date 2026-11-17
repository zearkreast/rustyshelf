// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/moreshots.js.
//! More screenshots for Details: a console game with fewer than `WANT` is topped up from ScreenScraper (with the
//! account), libretro's thumbnails and IGDB (with keys), gameplay before title screens before fan art. Saved under
//! `more-shots/<console>/` and listed in `more-shots/index.json` (the Electron app's format: keep in sync with
//! main/moreshots.js); never downloaded again. The same picture from two sources is dropped.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};

pub const WANT: usize = 10;
pub const MAX: usize = 12;
const RETRY_MS: f64 = 30.0 * 86_400_000.0;
const MAX_CANDS: usize = 16;
/// two difference hashes this many bits apart or fewer are the same picture
const SAME_BITS: u32 = 8;

/// A picture a source has for a game: its address and what it shows ("play", "title" or "fan").
#[derive(Debug, Clone, PartialEq)]
pub struct Cand {
    pub url: String,
    pub kind: &'static str,
}

/// How a download went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Got {
    Ok,
    Missing,
    /// asked again next time
    Busy,
}

/// The game a source is asked about.
pub struct Game<'a> {
    pub file: &'a Path,
    pub title: &'a str,
    pub region: &'a str,
}

pub trait Source: Send + Sync {
    /// as the index keeps it: "ss", "lr", "igdb"
    fn id(&self) -> &'static str;
    fn ready(&self) -> bool;
    /// What it has for the game. An Err: busy, asked again next time.
    fn find(&self, sys: &str, game: &Game) -> Result<Vec<Cand>, String>;
    fn get(&self, url: &str, dest: &Path) -> Got;
}

fn rank(kind: &str) -> Option<usize> {
    ["play", "title", "fan"].iter().position(|k| *k == kind)
}

/// `more-shots/index.json`: `{ v: 1, games: { key: { s: [[file (relative), source, kind, sha1, dhash]], t: { source:
/// at }, b: { url: [sha1, dhash] }, w } } }`.
pub struct Index {
    dir: PathBuf,
    games: Map<String, Value>,
}

impl Index {
    pub fn load(data_dir: &Path) -> Index {
        let dir = data_dir.join("more-shots");
        let read = crate::files::read_json(&dir.join("index.json"));
        let games = read.filter(|j| j["v"] == 1).and_then(|j| j["games"].as_object().cloned()).unwrap_or_default();
        Index { dir, games }
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::files::write_atomic(&self.dir.join("index.json"), json!({ "v": 1, "games": self.games }).to_string())
    }

    /// The game's added screenshots, as files.
    fn ours(&self, key: &str) -> Vec<PathBuf> {
        self.games.get(key).and_then(|e| e["s"].as_array()).map_or(Vec::new(), |s| s.iter().filter_map(|x| x[0].as_str()).map(|r| self.dir.join(r)).collect())
    }

    /// A game's whole list: its LaunchBox shots (`base`, without ones of ours a list from the shelf already has),
    /// then the added ones, at most `MAX`.
    pub fn list(&self, sys: &str, file: &Path, title: &str, base: &[String]) -> Vec<String> {
        let ours: Vec<String> = self.ours(&crate::gamekey::game_key(sys, file, title)).iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let mut out: Vec<String> = base.iter().filter(|u| !u.is_empty() && !ours.contains(u)).cloned().collect();
        out.extend(ours);
        out.truncate(MAX);
        out
    }

    /// Worth a look: short of `WANT`, with a source set up that hasn't been asked (lately) for this game.
    pub fn need(&self, sys: &str, file: &Path, title: &str, base: &[String], sources: &[&dyn Source]) -> bool {
        let key = crate::gamekey::game_key(sys, file, title);
        if self.list(sys, file, title, base).len() >= WANT {
            return false;
        }
        let e = self.games.get(&key);
        sources.iter().any(|s| s.ready() && due(e, s.id(), crate::js::date_now()))
    }
}

/// (a game whose sources were asked when the aim was lower is due again)
fn due(e: Option<&Value>, src: &str, now: f64) -> bool {
    let Some(e) = e else { return true };
    let at = e["t"][src].as_f64();
    at.is_none_or(|t| now - t >= RETRY_MS) || e["w"].as_u64().unwrap_or(6) < WANT as u64
}

fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.len() < 64 {
        return None;
    }
    if b.starts_with(b"\x89PNG") {
        Some(".png")
    } else if b.starts_with(&[0xff, 0xd8]) {
        Some(".jpg")
    } else if b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Some(".webp")
    } else {
        None
    }
}

/// A 9x8 copy of the picture → 64 bits as 16 hex digits: each pixel brighter than the one to its right. None for
/// a picture that can't be read here (WebP: compared by its bytes only).
pub fn dhash(b: &[u8]) -> Option<String> {
    let small = image::load_from_memory(b).ok()?.resize_exact(9, 8, image::imageops::FilterType::Triangle).to_rgb8();
    let mut h = String::new();
    for y in 0..8 {
        let mut nib = 0u8;
        for x in 0..8 {
            let sum = |x: u32| small.get_pixel(x, y).0.iter().map(|c| u32::from(*c)).sum::<u32>();
            nib = (nib << 1) | u8::from(sum(x) > sum(x + 1));
            if x == 3 || x == 7 {
                h.push(char::from_digit(u32::from(nib), 16)?);
                nib = 0;
            }
        }
    }
    Some(h)
}

fn hex16(h: &str) -> Option<u64> {
    (h.len() == 16).then(|| u64::from_str_radix(h, 16).ok()).flatten()
}

/// Two pictures' marks ([sha1, dhash]) are the same picture.
fn same(a: &[String; 2], b: &[String; 2]) -> bool {
    a[0] == b[0] || matches!((hex16(&a[1]), hex16(&b[1])), (Some(x), Some(y)) if (x ^ y).count_ones() <= SAME_BITS)
}

fn marks(b: &[u8]) -> [String; 2] {
    let sha = crate::files::hex(&Sha1::digest(b));
    [sha, dhash(b).unwrap_or_default()]
}

/// Fetches what's missing for a game from the sources that are set up and due, and keeps it in the index (saved).
/// `fetch_base`: a LaunchBox shot's bytes, so a new picture can be told apart from it.
pub fn top_up(index: &mut Index, sys: &str, game: &Game, base: &[String], sources: &[&dyn Source], fetch_base: &dyn Fn(&str) -> Option<Vec<u8>>) {
    let key = crate::gamekey::game_key(sys, game.file, game.title);
    let mut e = index.games.get(&key).cloned().unwrap_or_else(|| json!({ "s": [], "t": {} }));
    let ours: HashSet<String> = index.ours(&key).iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let base: Vec<&String> = base.iter().filter(|u| !u.is_empty() && !ours.contains(*u)).collect();
    let count = |e: &Value| e["s"].as_array().map_or(0, Vec::len);
    if base.len() + count(&e) >= WANT {
        return;
    }
    let now = crate::js::date_now();
    let (mut cands, mut asked): (Vec<(Cand, &dyn Source)>, Vec<&'static str>) = (Vec::new(), Vec::new());
    for src in sources {
        if !src.ready() || !due(Some(&e), src.id(), now) {
            continue;
        }
        let Ok(got) = src.find(sys, game) else { continue };
        asked.push(src.id());
        cands.extend(got.into_iter().filter(|c| rank(c.kind).is_some()).map(|c| (c, *src)));
    }
    if asked.is_empty() {
        return;
    }
    cands.sort_by_key(|c| rank(c.0.kind));
    cands.truncate(MAX_CANDS);
    let mut seen: Vec<[String; 2]> = e["s"].as_array().into_iter().flatten().map(|x| [x[3].as_str().unwrap_or("").to_string(), x[4].as_str().unwrap_or("").to_string()]).collect();
    let (mut busy, mut urls, mut base_marked) = (HashSet::new(), HashSet::new(), false);
    let sub_name: String = sys.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let sub = index.dir.join(&sub_name);
    let name = crate::files::hex(&Sha1::digest(key.as_bytes())[..8]);
    for (c, src) in cands {
        if base.len() + count(&e) >= WANT || base.len() + count(&e) >= MAX {
            break;
        }
        if !urls.insert(c.url.clone()) {
            continue;
        }
        // the LaunchBox shots' marks, once, before the first new picture is compared with them
        if !base_marked {
            base_marked = true;
            for u in &base {
                if e["b"].get(u.as_str()).is_none() {
                    if let Some(b) = fetch_base(u).filter(|b| sniff(b).is_some()) {
                        e["b"][u.as_str()] = json!(marks(&b));
                    }
                }
                if let Some(m) = e["b"].get(u.as_str()).and_then(|m| Some([m[0].as_str()?.to_string(), m[1].as_str().unwrap_or("").to_string()])) {
                    seen.push(m);
                }
            }
        }
        let tmp = sub.join(format!("{name}.{}.incoming", std::process::id()));
        let got = if std::fs::create_dir_all(&sub).is_ok() { src.get(&c.url, &tmp) } else { Got::Busy };
        if got != Got::Ok {
            if got == Got::Busy {
                busy.insert(src.id());
            }
            let _ = std::fs::remove_file(&tmp);
            continue;
        }
        let buf = std::fs::read(&tmp).unwrap_or_default();
        let Some(ext) = sniff(&buf) else {
            let _ = std::fs::remove_file(&tmp);
            continue;
        };
        let m = marks(&buf);
        if seen.iter().any(|s| same(s, &m)) {
            let _ = std::fs::remove_file(&tmp);
            continue;
        }
        let rel = format!("{sub_name}/{name}-{}{ext}", &m[0][..10]);
        if std::fs::rename(&tmp, index.dir.join(&rel)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            continue;
        }
        if let Some(s) = e["s"].as_array_mut() {
            s.push(json!([rel, src.id(), c.kind, m[0], m[1]]));
        }
        seen.push(m);
    }
    for id in asked.iter().filter(|id| !busy.contains(*id)) {
        e["t"][*id] = json!(now as u64);
    }
    if busy.is_empty() {
        e["w"] = json!(WANT);
    }
    index.games.insert(key, e);
    let _ = index.save();
}

// ---------- the sources ----------

/// ScreenScraper: its in-game screenshot and title screen (only with the user's account).
pub struct Ss<'a>(pub &'a crate::screenscraper::ScreenScraper);

impl Source for Ss<'_> {
    fn id(&self) -> &'static str {
        "ss"
    }
    fn ready(&self) -> bool {
        self.0.enabled()
    }
    fn find(&self, sys: &str, game: &Game) -> Result<Vec<Cand>, String> {
        let file = game.file.to_string_lossy();
        let rom = crate::screenscraper::Rom { file: &file, title: game.title, size_bytes: 0 };
        Ok(self.0.shots(sys, &rom)?.into_iter().map(|(url, kind)| Cand { url, kind }).collect())
    }
    fn get(&self, url: &str, dest: &Path) -> Got {
        match self.0.download(url, dest) {
            Ok(crate::screenscraper::Got::Ok) => Got::Ok,
            Ok(crate::screenscraper::Got::Missing) => Got::Missing,
            _ => Got::Busy,
        }
    }
}

fn download(url: &str, dest: &Path) -> Got {
    match crate::boxart::download_ex(url, dest, false) {
        Ok(crate::boxart::Outcome::Ok) => Got::Ok,
        Ok(crate::boxart::Outcome::Missing) => Got::Missing,
        _ => Got::Busy,
    }
}

/// libretro's thumbnails: the game's name in the console's box art list, then that name's snap and title screen.
pub struct Libretro<'a> {
    pub cache_dir: &'a Path,
    /// the region Settings asks box art in
    pub region: &'a str,
}

impl Source for Libretro<'_> {
    fn id(&self) -> &'static str {
        "lr"
    }
    fn ready(&self) -> bool {
        true
    }
    fn find(&self, sys: &str, game: &Game) -> Result<Vec<Cand>, String> {
        let Some(lr) = crate::covers::libretro_system(sys) else { return Ok(Vec::new()) };
        let names = crate::boxart::libretro_names(self.cache_dir, sys).ok_or("no list")?;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        let Some(name) = names.find(game.file, game.title, game.region, self.region) else { return Ok(Vec::new()) };
        let u = |d: &str| format!("{}{}/{d}/{}", crate::covers::LIBRETRO_THUMBS, crate::js::encode_uri_component(lr), crate::js::encode_uri_component(&format!("{name}.png")));
        Ok(vec![Cand { url: u("Named_Snaps"), kind: "play" }, Cand { url: u("Named_Titles"), kind: "title" }])
    }
    fn get(&self, url: &str, dest: &Path) -> Got {
        download(url, dest)
    }
}

/// IGDB's screenshots (only with a Twitch Client ID and Secret). The same answer carries the game's info, kept in
/// IGDB's store so a game costs one question for both.
pub struct Igdb<'a> {
    pub client: &'a crate::igdb::Igdb,
    pub data_dir: &'a Path,
}

impl Source for Igdb<'_> {
    fn id(&self) -> &'static str {
        "igdb"
    }
    fn ready(&self) -> bool {
        self.client.ready()
    }
    fn find(&self, sys: &str, game: &Game) -> Result<Vec<Cand>, String> {
        let Some(found) = self.client.lookup(sys, game.title).map_err(|t| t.why)? else { return Ok(Vec::new()) };
        let mut store = crate::igdb::Store::load(self.data_dir);
        store.put(&crate::gamekey::game_key(sys, game.file, game.title), found.hit.as_ref(), crate::js::date_now());
        let _ = store.save();
        Ok(found.shot_hit.as_ref().map_or(Vec::new(), crate::igdb::shots_of).into_iter().take(MAX).map(|url| Cand { url, kind: "play" }).collect())
    }
    fn get(&self, url: &str, dest: &Path) -> Got {
        download(url, dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn png(shade: u8, stripes: bool) -> Vec<u8> {
        let img = image::RgbImage::from_fn(64, 48, |x, _| if stripes && (x / 8) % 2 == 0 { image::Rgb([255 - shade, 0, 0]) } else { image::Rgb([shade, shade, shade]) });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    struct Fake {
        id: &'static str,
        cands: Vec<Cand>,
        files: Vec<(&'static str, Vec<u8>)>,
        asked: Mutex<usize>,
    }

    impl Source for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn ready(&self) -> bool {
            true
        }
        fn find(&self, _: &str, _: &Game) -> Result<Vec<Cand>, String> {
            *self.asked.lock().unwrap() += 1;
            Ok(self.cands.clone())
        }
        fn get(&self, url: &str, dest: &Path) -> Got {
            match self.files.iter().find(|f| f.0 == url) {
                Some((_, b)) => {
                    std::fs::write(dest, b).unwrap();
                    Got::Ok
                }
                None => Got::Missing,
            }
        }
    }

    #[test]
    fn a_game_is_topped_up_once_without_the_same_picture_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let file = Path::new("D:/Roms/SNES/Zeta Run (USA).sfc");
        let game = Game { file, title: "Zeta Run", region: "USA" };
        let a = Fake {
            id: "ss",
            cands: vec![Cand { url: "ss/title".into(), kind: "title" }, Cand { url: "ss/play".into(), kind: "play" }],
            files: vec![("ss/title", png(40, true)), ("ss/play", png(200, false))],
            asked: Mutex::new(0),
        };
        // libretro's snap is ScreenScraper's title screen again, a shade apart: the same picture
        let b = Fake { id: "lr", cands: vec![Cand { url: "lr/snap".into(), kind: "play" }, Cand { url: "lr/x".into(), kind: "nope" }], files: vec![("lr/snap", png(44, true))], asked: Mutex::new(0) };
        let mut index = Index::load(tmp.path());
        let base = vec!["https://images.launchbox-app.com/one.jpg".to_string()];
        assert!(index.need("snes", file, "Zeta Run", &base, &[&a, &b]));
        top_up(&mut index, "snes", &game, &base, &[&a, &b], &|_| None);
        let list = Index::load(tmp.path()).list("snes", file, "Zeta Run", &base);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0], base[0]);
        let kept: Vec<(&str, &str)> = index.games.values().next().unwrap()["s"].as_array().unwrap().iter().map(|x| (x[1].as_str().unwrap(), x[2].as_str().unwrap())).collect();
        // gameplay before title screens: libretro's snap comes before ScreenScraper's title screen, which is then the
        // same picture again
        assert_eq!(kept, [("ss", "play"), ("lr", "play")]);
        assert!(list[1..].iter().all(|f| Path::new(f).is_file() && f.contains("more-shots")));
        // asked once: not again until it's due
        assert!(!index.need("snes", file, "Zeta Run", &base, &[&a, &b]));
        top_up(&mut index, "snes", &game, &base, &[&a, &b], &|_| None);
        assert_eq!((*a.asked.lock().unwrap(), *b.asked.lock().unwrap()), (1, 1));
        // a list from the shelf that already has ours isn't doubled
        assert_eq!(index.list("snes", file, "Zeta Run", &list), list);
    }

    #[test]
    fn a_difference_hash() {
        let (dark, light) = (dhash(&png(40, true)).unwrap(), dhash(&png(200, false)).unwrap());
        assert_eq!(dark.len(), 16);
        assert!(same(&["a".into(), dark.clone()], &["b".into(), dhash(&png(44, true)).unwrap()]));
        assert!(!same(&["a".into(), dark], &["b".into(), light]));
    }
}

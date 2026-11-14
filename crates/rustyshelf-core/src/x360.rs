// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/x360.js, as RustyShelf has it.
//! Xbox 360: the title id inside a game (a disc image, a default.xex, a Games on Demand / Arcade package), and its real
//! name from the title list kept in `data/x360-titles.json` (Xenia's compatibility trackers), a file the Electron app
//! shares; it's fetched again every week.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

const SECTOR: u64 = 2048;
/// extracted / XGD3 / XGD2 / XGD1
const PARTITIONS: [u64; 4] = [0, 0x208_0000, 0xFD9_0000, 0x1830_0000];
const MAGIC: &[u8] = b"MICROSOFT*XBOX*MEDIA";

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_at(f: &mut File, pos: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(pos)).ok()?;
    let mut b = Vec::with_capacity(len);
    f.take(len as u64).read_to_end(&mut b).ok()?;
    Some(b)
}

fn hex_id(n: u32) -> String {
    format!("{n:08X}")
}

/// The title id in a XEX2 header (its optional headers are plain even when the program is encrypted).
pub fn xex_title_id(buf: &[u8]) -> Option<String> {
    if buf.len() < 0x18 || &buf[..4] != b"XEX2" {
        return None;
    }
    let count = be32(buf, 0x14)? as usize;
    if count > 256 {
        return None;
    }
    for i in 0..count {
        let at = 0x18 + i * 8;
        if at + 8 > buf.len() {
            break;
        }
        // the execution id: media id, version, base version, title id …
        if be32(buf, at)? == 0x0004_0006 {
            let val = be32(buf, at + 4)? as usize;
            if val + 16 > buf.len() {
                return None;
            }
            return be32(buf, val + 12).map(hex_id);
        }
    }
    None
}

/// The first bytes of an .xex, enough for its headers (a big game's run past 16 KB).
fn xex_head(f: &mut File, at: u64, limit: u64) -> Option<Vec<u8>> {
    let mut head = read_at(f, at, limit.min(0x4000) as usize)?;
    let size = if head.len() >= 12 { be32(&head, 8)? as u64 } else { 0 };
    if size > head.len() as u64 && size <= 0x10_0000 {
        head = read_at(f, at, limit.min(size) as usize)?;
    }
    Some(head)
}

struct Entry {
    sector: u32,
    size: u32,
    dir: bool,
}

/// A name in a GDFX directory table, any case.
fn find_entry(f: &mut File, base: u64, sector: u32, size: u32, name: &str) -> Option<Entry> {
    let table = read_at(f, base + sector as u64 * SECTOR, (size as usize).min(256 * 1024))?;
    let mut off = 0usize;
    while off + 14 <= table.len() {
        if table[off] == 0xFF && table[off + 1] == 0xFF {
            off = (off / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let start = le32(&table, off + 4)?;
        let fsize = le32(&table, off + 8)?;
        let (attr, nlen) = (table[off + 12], table[off + 13] as usize);
        if nlen == 0 || off + 14 + nlen > table.len() {
            break;
        }
        let n: String = table[off + 14..off + 14 + nlen].iter().map(|&c| c as char).collect();
        if n.eq_ignore_ascii_case(name) {
            return Some(Entry { sector: start, size: fsize, dir: attr & 0x10 != 0 });
        }
        off += (14 + nlen + 3) & !3;
    }
    None
}

/// The title id of a disc image's default.xex.
pub fn iso_title_id(file: &Path) -> Option<String> {
    let mut f = File::open(file).ok()?;
    let total = f.metadata().ok()?.len();
    for base in PARTITIONS {
        if base + 0x10000 + SECTOR > total {
            continue;
        }
        let vol = read_at(&mut f, base + 0x10000, SECTOR as usize)?;
        if vol.get(..20) != Some(MAGIC) {
            continue;
        }
        let e = find_entry(&mut f, base, le32(&vol, 20)?, le32(&vol, 24)?, "default.xex")?;
        if e.dir {
            return None;
        }
        return xex_title_id(&xex_head(&mut f, base + e.sector as u64 * SECTOR, e.size as u64)?);
    }
    None
}

/// What kind of package a Games on Demand / Arcade / indie file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    GamesOnDemand,
    Arcade,
    Title,
    Indie,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub title_id: String,
    pub title: String,
    pub kind: PackageKind,
}

/// A file with no extension in a content-type folder (`<title id>\00007000\<hash>`): maybe an STFS package.
pub fn is_stfs_candidate(file: &Path) -> bool {
    let folder = file.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    no_extname(file) && folder.len() == 8 && folder.chars().all(|c| c.is_ascii_hexdigit())
}

/// node's `!path.extname(f)`: no dot past the first character ("name." has one, ".hidden" hasn't).
pub fn no_extname(file: &Path) -> bool {
    let n = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    n.rfind('.').is_none_or(|i| i == 0)
}

/// An STFS package ("CON " / "LIVE" / "PIRS") that's a game; None for saves, DLC, title updates, themes, demos …
pub fn stfs_info(file: &Path) -> Option<Package> {
    let mut f = File::open(file).ok()?;
    let h = read_at(&mut f, 0, 0x1712)?;
    if h.len() < 0x1712 || !matches!(&h[..4], b"CON " | b"LIVE" | b"PIRS") {
        return None;
    }
    let kind = match be32(&h, 0x344)? {
        0x0000_7000 => PackageKind::GamesOnDemand,
        0x000D_0000 => PackageKind::Arcade,
        0x0000_1000 => PackageKind::Title,
        0x0200_0000 => PackageKind::Indie,
        _ => return None,
    };
    let u16be = |at: usize| {
        let units: Vec<u16> = h[at..at + 0x80].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        let s = String::from_utf16_lossy(&units);
        let s = s.split('\0').next().unwrap_or("");
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    // the display name, else the title name
    let title = Some(u16be(0x411)).filter(|s| !s.is_empty()).unwrap_or_else(|| u16be(0x1691));
    Some(Package { title_id: hex_id(be32(&h, 0x360)?), title, kind })
}

/// The title id of a game file: an .xex, a disc image, or a package (already read by the caller when `stfs` is given).
pub fn title_id_of(file: &Path, stfs: Option<&Package>) -> Option<String> {
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase());
    match ext.as_deref() {
        Some("xex") => {
            let mut f = File::open(file).ok()?;
            let len = f.metadata().ok()?.len();
            xex_title_id(&xex_head(&mut f, 0, len)?)
        }
        Some("iso") => iso_title_id(file),
        None => match stfs {
            Some(s) => Some(s.title_id.clone()),
            None => stfs_info(file).map(|s| s.title_id),
        },
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Known {
    pub title: String,
    /// Xenia's compatibility state ("playable", "ingame" …)
    #[serde(default)]
    pub state: String,
}

/// The title list: title id (upper case) → its name and how well it runs.
#[derive(Debug, Clone, Default)]
pub struct TitleList {
    titles: HashMap<String, Known>,
}

#[derive(Deserialize)]
struct TitleFile {
    #[serde(default)]
    titles: HashMap<String, Known>,
}

impl TitleList {
    /// An empty list when the Electron app hasn't downloaded one yet.
    pub fn load(data_dir: &Path) -> TitleList {
        let text = std::fs::read_to_string(data_dir.join("x360-titles.json")).unwrap_or_default();
        let titles = serde_json::from_str::<TitleFile>(&text).map(|f| f.titles).unwrap_or_default();
        TitleList { titles }
    }

    pub fn get(&self, title_id: &str) -> Option<&Known> {
        self.titles.get(title_id)
    }
}

/// Xenia Manager's compatibility lists, Canary's first (main/x360.js DB_URLS).
const DB_URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/xenia-manager/Database/main/data/game-compatibility/canary.json",
    "https://raw.githubusercontent.com/xenia-manager/Database/main/data/game-compatibility/stable.json",
];

/// The titles in one compatibility list (`[{ id, title, state }]`) that `titles` doesn't have yet.
fn add_titles(list: &serde_json::Value, titles: &mut serde_json::Map<String, serde_json::Value>) {
    use crate::js;
    for g in list.as_array().into_iter().flatten() {
        let id = js::text(g.get("id")).to_uppercase();
        if !id.is_empty() && js::truthy(g.get("title")) && !titles.contains_key(&id) {
            let state = js::first(&[g.get("state")]).cloned().unwrap_or_else(|| "".into());
            titles.insert(id, serde_json::json!({ "title": g["title"], "state": state }));
        }
    }
}

/// Fetches the title list when there's none or it's older than a week; true when a new one was saved. A list that
/// comes back nearly empty isn't kept: the old one stays.
pub fn refresh(data_dir: &Path) -> Result<bool, String> {
    const WEEK: f64 = 7.0 * 86_400_000.0;
    let file = data_dir.join("x360-titles.json");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let saved: Option<serde_json::Value> = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if saved.as_ref().is_some_and(|j| j.get("titles").is_some_and(|t| t.is_object()) && now as f64 - crate::js::number(j.get("at").or(Some(&serde_json::Value::Null))) < WEEK) {
        return Ok(false);
    }
    let mut titles = serde_json::Map::new();
    let mut failure = String::new();
    for u in DB_URLS {
        match crate::net::get_bytes(u, std::time::Duration::from_secs(15), 32 << 20) {
            Ok(Some(b)) => match serde_json::from_slice(&b) {
                Ok(list) => add_titles(&list, &mut titles),
                Err(_) => failure = "bad reply".into(),
            },
            Ok(None) => failure = "HTTP 404".into(),
            Err(e) => failure = e,
        }
    }
    if titles.len() <= 100 {
        return Err(format!("x360 title list: {}", if failure.is_empty() { "too few games in it" } else { &failure }));
    }
    std::fs::create_dir_all(data_dir).and_then(|()| std::fs::write(&file, serde_json::json!({ "at": now, "titles": titles }).to_string())).map_err(|e| e.to_string())?;
    Ok(true)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub title_id: String,
    pub title: Option<String>,
    pub compat: Option<String>,
}

pub fn lookup(file: &Path, stfs: Option<&Package>, list: &TitleList) -> Option<Lookup> {
    let title_id = title_id_of(file, stfs)?;
    let hit = list.get(&title_id);
    Some(Lookup {
        title: hit.map(|k| k.title.clone()),
        compat: hit.map(|k| k.state.clone()).filter(|s| !s.is_empty()),
        title_id,
    })
}

/// Test images: a XEX2 header with a title id, a package header, a disc image holding a default.xex.
#[cfg(test)]
pub mod build {
    pub fn xex(title_id: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x40];
        b[..4].copy_from_slice(b"XEX2");
        b[8..12].copy_from_slice(&0x40u32.to_be_bytes());
        b[0x14..0x18].copy_from_slice(&1u32.to_be_bytes());
        b[0x18..0x1C].copy_from_slice(&0x0004_0006u32.to_be_bytes());
        b[0x1C..0x20].copy_from_slice(&0x20u32.to_be_bytes());
        b[0x2C..0x30].copy_from_slice(&title_id.to_be_bytes());
        b
    }

    pub fn stfs(content_type: u32, title_id: u32, name: &str) -> Vec<u8> {
        let mut b = vec![0u8; 0x1712];
        b[..4].copy_from_slice(b"LIVE");
        b[0x344..0x348].copy_from_slice(&content_type.to_be_bytes());
        b[0x360..0x364].copy_from_slice(&title_id.to_be_bytes());
        for (i, u) in name.encode_utf16().enumerate() {
            b[0x411 + i * 2..0x413 + i * 2].copy_from_slice(&u.to_be_bytes());
        }
        b
    }

    pub fn iso(title_id: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x10000 + 2048 * 4];
        b[0x10000..0x10014].copy_from_slice(super::MAGIC);
        let root = 0x10000 / 2048 + 1;
        b[0x10014..0x10018].copy_from_slice(&(root as u32).to_le_bytes());
        b[0x10018..0x1001C].copy_from_slice(&2048u32.to_le_bytes());
        let t = root * 2048;
        let name = b"default.xex";
        b[t + 4..t + 8].copy_from_slice(&((root + 1) as u32).to_le_bytes());
        b[t + 8..t + 12].copy_from_slice(&0x40u32.to_le_bytes());
        b[t + 13] = name.len() as u8;
        b[t + 14..t + 14 + name.len()].copy_from_slice(name);
        let x = (root + 1) * 2048;
        b[x..x + 0x40].copy_from_slice(&xex(title_id));
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_ids() {
        assert_eq!(xex_title_id(&build::xex(0x4D53_07E6)).as_deref(), Some("4D5307E6"));
        assert_eq!(xex_title_id(b"XEX1"), None);
        let dir = tempfile::tempdir().unwrap();
        let iso = dir.path().join("DVD.iso");
        std::fs::write(&iso, build::iso(0x4156_07D1)).unwrap();
        assert_eq!(iso_title_id(&iso).as_deref(), Some("415607D1"));
    }

    #[test]
    fn packages() {
        let dir = tempfile::tempdir().unwrap();
        let ct = dir.path().join("00007000");
        std::fs::create_dir(&ct).unwrap();
        let game = ct.join("0123456789ABCDEF0123456789ABCDEF01234567");
        std::fs::write(&game, build::stfs(0x7000, 0x5841_0912, "Some  Game\u{0}junk")).unwrap();
        assert!(is_stfs_candidate(&game));
        assert!(!is_stfs_candidate(&dir.path().join("x.iso")));
        let p = stfs_info(&game).unwrap();
        assert_eq!((p.title_id.as_str(), p.title.as_str(), p.kind), ("58410912", "Some Game", PackageKind::GamesOnDemand));
        let save = ct.join("save");
        std::fs::write(&save, build::stfs(0x1, 0x5841_0912, "Save")).unwrap();
        assert!(stfs_info(&save).is_none());
    }
}

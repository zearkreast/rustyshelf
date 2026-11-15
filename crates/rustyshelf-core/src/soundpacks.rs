// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/soundpacks.js (the packs; not Kenney's downloads or the real instruments).
//! Your own sounds: a pack is a folder `<RustyShelf>\sounds\<id>\` with the sound files and a pack.json
//! (`{ name, matches: { move, open, back, play, problem, music }, where: "all" | [console ids], files, unsupported }`),
//! the Electron app's own layout. Packs come in as a ZIP, a folder or loose files; files are matched to the menu's
//! moments by their names. Only WAV, OGG, MP3 and FLAC are copied; anything else is listed as unsupported. A ZIP's
//! entries are read from its own index, and an entry that would land outside the pack, too many files or too many
//! bytes turn it away before anything is written.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

pub const MOMENTS: [&str; 6] = ["move", "open", "back", "play", "problem", "music"];
const MAX_BYTES: u64 = 50 << 20;
const MAX_FILES: usize = 300;
const MAX_ZIP: u64 = 200 << 20;
const MAX_FILE: u64 = 20 << 20;

static AUDIO: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(wav|ogg|mp3|flac)$").unwrap());
// files that are plainly not sounds (a readme, a licence, pictures): skipped without a word
static IGNORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(txt|md|url|pdf|png|jpe?g|gif|webp|bmp|json|nfo|html?|ini|db|ds_store|lnk|sfv|xml|csv|rtf|doc|docx)$").unwrap());
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-z0-9-]{1,48}$").unwrap());

/// Where the packs are kept.
pub fn packs_dir(app_root: &Path) -> PathBuf {
    app_root.join("sounds")
}

/// A file's name as lower-case words ("SelectMove_02.wav" → select, move).
fn words(name: &str) -> Vec<String> {
    static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.[a-z0-9]{1,5}$").unwrap());
    static CAMEL: LazyLock<Regex> = LazyLock::new(|| Regex::new("([a-z])([A-Z])").unwrap());
    let base = EXT.replace(name, "");
    CAMEL.replace_all(&base, "$1 $2").to_lowercase().split(|c: char| !c.is_ascii_lowercase()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// The moment a file's name suggests, or "".
pub fn match_name(name: &str) -> &'static str {
    let w = words(name);
    let has = |l: &[&str]| w.iter().any(|x| l.iter().any(|p| if *p == "ok" { x == "ok" } else { x.starts_with(p) }));
    let joined = w.join(" ");
    if has(&["music", "bgm", "theme", "loop", "song"]) {
        "music"
    } else if has(&["error", "buzz", "fail", "wrong", "denied", "invalid"]) {
        "problem"
    } else if has(&["cancel", "back", "close", "return", "exit"]) {
        "back"
    } else if has(&["start", "launch", "boot"]) {
        "play"
    } else if Regex::new(r"\bselect ?move\b").unwrap().is_match(&joined) || has(&["cursor", "move", "click", "hover", "rollover", "tick", "scroll", "nav"]) {
        "move"
    } else if has(&["decide", "select", "ok", "open", "confirm", "accept", "enter"]) {
        "open"
    } else {
        ""
    }
}

/// JavaScript's `localeCompare` with numeric order, near enough: digits as numbers, case left out.
fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let n = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s.parse::<u128>().unwrap_or(0)
                };
                match n(&mut x).cmp(&n(&mut y)) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            (Some(c), Some(d)) => {
                match c.to_lowercase().cmp(d.to_lowercase()) {
                    Ordering::Equal => {}
                    o => return o,
                }
                x.next();
                y.next();
            }
        }
    }
}

/// Every moment's first matching file, in natural order (so _001 before _002).
pub fn match_all(files: &[String]) -> Map<String, Value> {
    let mut list = files.to_vec();
    list.sort_by(|a, b| natural(a, b));
    let mut m: Map<String, Value> = MOMENTS.iter().map(|k| (k.to_string(), json!(""))).collect();
    for f in &list {
        let k = match_name(f);
        if !k.is_empty() && m[k] == "" {
            m[k] = json!(f);
        }
    }
    m
}

/// A console named in the pack's name ("PS2 menu sounds"): only on it (it can be changed).
pub fn guess_where(name: &str) -> Value {
    static GUESS: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"(?i)\bps ?2\b|playstation ?2", "ps2"),
            (r"(?i)\bps ?3\b|playstation ?3", "ps3"),
            (r"(?i)\bps ?4\b|playstation ?4", "ps4"),
            (r"(?i)\bps ?5\b|playstation ?5", "ps5"),
            (r"(?i)\bpsp\b", "psp"),
            (r"(?i)\bps ?1\b|\bpsx\b|\bpsone\b", "psx"),
            (r"(?i)\bwii ?u\b", "wiiu"),
            (r"(?i)\bwii\b", "wii"),
            (r"(?i)gamecube|\bngc\b", "gc"),
            (r"(?i)switch ?2", "sw2"),
            (r"(?i)\bswitch\b", "sw"),
            (r"(?i)\b3ds\b", "n3ds"),
            (r"(?i)xbox ?360|\bx360\b", "x360"),
            (r"(?i)dreamcast", "dreamcast"),
        ]
        .into_iter()
        .map(|(r, id)| (Regex::new(r).unwrap(), id))
        .collect()
    });
    GUESS.iter().find(|(re, _)| re.is_match(name)).map_or(json!("all"), |(_, id)| json!([id]))
}

// ---------- a ZIP's own index ----------

fn le16(b: &[u8], at: usize) -> usize {
    b.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn le32(b: &[u8], at: usize) -> usize {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

struct Entry {
    name: String,
    size: usize,
    method: usize,
    csize: usize,
    local: usize,
}

fn zip_index(b: &[u8]) -> Result<Vec<Entry>, String> {
    let last = b.len().checked_sub(22).ok_or("That file isn't a ZIP.")?;
    let end = (last.saturating_sub(65_535)..=last).rev().find(|&i| le32(b, i) == 0x0605_4b50).ok_or("That file isn't a ZIP.")?;
    let (count, cd_size, cd_off) = (le16(b, end + 10), le32(b, end + 12), le32(b, end + 16));
    if count == 0xffff || cd_off == 0xffff_ffff {
        return Err("That ZIP is too big.".into());
    }
    if cd_size > 8 << 20 || cd_off + cd_size > b.len() {
        return Err("That ZIP looks damaged.".into());
    }
    let mut out = Vec::new();
    let mut p = cd_off;
    for _ in 0..count {
        if p + 46 > b.len() || le32(b, p) != 0x0201_4b50 {
            return Err("That ZIP looks damaged.".into());
        }
        let (flags, method, csize, size, nl, xl, cl, local) = (le16(b, p + 8), le16(b, p + 10), le32(b, p + 20), le32(b, p + 24), le16(b, p + 28), le16(b, p + 30), le16(b, p + 32), le32(b, p + 42));
        let raw = b.get(p + 46..p + 46 + nl).unwrap_or_default();
        let name = if flags & 0x800 != 0 { String::from_utf8_lossy(raw).into_owned() } else { raw.iter().map(|c| *c as char).collect() };
        out.push(Entry { name, size, method, csize, local });
        p += 46 + nl + xl + cl;
    }
    Ok(out)
}

fn zip_read(b: &[u8], e: &Entry) -> Option<Vec<u8>> {
    if le32(b, e.local) != 0x0403_4b50 {
        return None;
    }
    let start = e.local + 30 + le16(b, e.local + 26) + le16(b, e.local + 28);
    let raw = b.get(start..start.checked_add(e.csize)?)?;
    match e.method {
        0 => Some(raw.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, e.size.max(1)).ok(),
        _ => None,
    }
}

/// A ZIP entry's name that stays inside the folder it's unpacked into.
fn safe_entry(name: &str) -> bool {
    let n = name.replace('\\', "/");
    !(n.is_empty() || n.starts_with('/') || n.as_bytes().get(1) == Some(&b':') || n.contains('\0') || n.split('/').any(|s| s == ".."))
}

// ---------- adding a pack ----------

fn slug(s: &str) -> String {
    static NOT: LazyLock<Regex> = LazyLock::new(|| Regex::new("[^a-z0-9]+").unwrap());
    let s = NOT.replace_all(&s.to_lowercase(), "-").trim_matches('-').chars().take(40).collect::<String>();
    if s.is_empty() { "pack".into() } else { s }
}

fn clean_name(s: &str) -> String {
    static ZIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.zip$").unwrap());
    static KENNEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^kenney[_ -]*").unwrap());
    let s = KENNEY.replace(&ZIP.replace(s, ""), "").replace('_', " ");
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
    if s.is_empty() { "My sounds".into() } else { s }
}

fn safe_file(s: &str) -> String {
    let s: String = s.chars().map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c }).collect();
    let s = if s.starts_with('.') { format!("_{}", s.trim_start_matches('.')) } else { s };
    s.chars().take(120).collect()
}

/// A sound to copy in: from a file on disk, or a ZIP entry's bytes; and where it was (for a name clash).
enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
}

fn walk(dir: &Path, depth: usize, base: &Path, out: &mut Vec<(String, Source, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut ents: Vec<_> = rd.flatten().collect();
    ents.sort_by_key(|e| e.file_name());
    for e in ents {
        let name = e.file_name().to_string_lossy().into_owned();
        if out.len() > MAX_FILES * 4 || name.starts_with('.') || name == "__MACOSX" {
            continue;
        }
        let Ok(kind) = e.file_type() else { continue };
        let p = e.path();
        if kind.is_dir() {
            if depth < 6 {
                walk(&p, depth + 1, base, out);
            }
        } else if kind.is_file() {
            let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().into_owned();
            let size = e.metadata().map_or(0, |m| m.len());
            out.push((rel, Source::File(p), size));
        }
    }
}

/// A pack as it's listed: its id and what pack.json says, the moments only naming files that are there.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    /// moment → file ("" for none)
    pub matches: Map<String, Value>,
    /// None: every console; else only these
    pub only_on: Option<Vec<String>>,
    pub files: Vec<String>,
    pub unsupported: Vec<String>,
}

impl Pack {
    /// The file a moment plays, if the pack has one for it.
    pub fn file_for(&self, moment: &str) -> Option<&str> {
        self.matches.get(moment).and_then(Value::as_str).filter(|f| !f.is_empty())
    }

    /// Its id as a sound is chosen by in the settings (`ui.soundTheme`, `ui.soundByShelf`).
    pub fn sound_id(&self) -> String {
        if Regex::new("^kenney-[a-z]+$").unwrap().is_match(&self.id) { self.id.clone() } else { format!("pack:{}", self.id) }
    }
}

/// The pack a sound id names (`pack:<id>`, or a Kenney pack's own id).
pub fn pack_id_of(sound: &str) -> Option<&str> {
    sound.strip_prefix("pack:").or_else(|| sound.starts_with("kenney-").then_some(sound)).filter(|id| ID.is_match(id))
}

/// Adds the sounds in `sources` (one ZIP, one folder, or loose files; several of any make one pack) as a pack.
pub fn import_from(app_root: &Path, sources: &[PathBuf], name: Option<&str>) -> Result<Pack, String> {
    if sources.is_empty() {
        return Err("Nothing to add.".into());
    }
    let root = packs_dir(app_root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let (mut cands, mut guess): (Vec<(String, Source, u64)>, String) = (Vec::new(), name.unwrap_or("").to_string());
    let mut unsupported: Vec<String> = Vec::new();
    for src in sources {
        let base = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let meta = std::fs::metadata(src).map_err(|_| format!("Couldn't find {base}."))?;
        if meta.is_dir() {
            if guess.is_empty() {
                guess = base;
            }
            walk(src, 0, src, &mut cands);
        } else if base.to_lowercase().ends_with(".zip") {
            if meta.len() > MAX_ZIP {
                return Err("That ZIP is too big (over 200 MB).".into());
            }
            if guess.is_empty() {
                guess = base.clone();
            }
            let b = std::fs::read(src).map_err(|e| e.to_string())?;
            let (mut pick, mut total) = (Vec::new(), 0u64);
            for e in zip_index(&b)?.into_iter().filter(|e| !e.name.ends_with('/')) {
                let last = e.name.rsplit('/').next().unwrap_or("").to_string();
                if !safe_entry(&e.name) || e.name.starts_with("__MACOSX/") || last.starts_with('.') {
                    continue;
                }
                if !AUDIO.is_match(&last) {
                    if !IGNORE.is_match(&last) {
                        unsupported.push(last);
                    }
                    continue;
                }
                total += e.size as u64;
                pick.push(e);
            }
            if pick.len() > MAX_FILES {
                return Err(format!("That ZIP has too many sounds (over {MAX_FILES})."));
            }
            if total > MAX_BYTES {
                return Err("The sounds in that ZIP are too big (over 50 MB in all).".into());
            }
            for e in pick {
                if let Some(data) = zip_read(&b, &e) {
                    let n = data.len() as u64;
                    cands.push((e.name.clone(), Source::Bytes(data), n));
                }
            }
        } else {
            let size = meta.len();
            cands.push((base.clone(), Source::File(src.clone()), size));
            if guess.is_empty() {
                guess = if sources.len() == 1 { base.rsplit_once('.').map_or(base.as_str(), |(s, _)| s).to_string() } else { "My sounds".into() };
            }
        }
    }
    let (mut keep, mut names, mut total): (Vec<(String, Source)>, Vec<String>, u64) = (Vec::new(), Vec::new(), 0);
    for (rel, src, size) in cands {
        let rel = rel.replace('\\', "/");
        let base = rel.rsplit('/').next().unwrap_or(&rel).to_string();
        if !AUDIO.is_match(&base) {
            if !IGNORE.is_match(&base) {
                unsupported.push(base);
            }
            continue;
        }
        if size > MAX_FILE {
            unsupported.push(base);
            continue;
        }
        total += size;
        if total > MAX_BYTES {
            return Err("Those sounds are too big (over 50 MB in all).".into());
        }
        if keep.len() >= MAX_FILES {
            return Err(format!("That's too many sounds (over {MAX_FILES})."));
        }
        let mut name = safe_file(&base);
        if names.contains(&name.to_lowercase()) {
            let dir = rel.rsplit('/').nth(1).unwrap_or("");
            name = safe_file(&format!("{}-{base}", if dir.is_empty() { "x" } else { dir }));
            let mut k = 2;
            while names.contains(&name.to_lowercase()) {
                name = safe_file(&format!("{k}-{base}"));
                k += 1;
            }
        }
        names.push(name.to_lowercase());
        keep.push((name, src));
    }
    unsupported.dedup();
    if keep.is_empty() {
        return Err(if unsupported.is_empty() {
            "No sound files in there (WAV, OGG, MP3 or FLAC).".into()
        } else {
            let some = unsupported.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            format!("No sounds RustyShelf can play: {some}{} {} not WAV, OGG, MP3 or FLAC.", if unsupported.len() > 3 { "…" } else { "" }, if unsupported.len() == 1 { "is" } else { "are" })
        });
    }
    let name = clean_name(&guess);
    let (base_id, mut id, mut k) = (slug(&name), slug(&name), 2);
    while root.join(&id).exists() {
        id = format!("{base_id}-{k}");
        k += 1;
    }
    let dest = root.join(&id);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    for (file, src) in &keep {
        let done = match src {
            Source::File(p) => std::fs::copy(p, dest.join(file)).map(|_| ()),
            Source::Bytes(b) => std::fs::write(dest.join(file), b),
        };
        done.map_err(|e| e.to_string())?;
    }
    let mut files: Vec<String> = keep.into_iter().map(|(f, _)| f).collect();
    files.sort_by(|a, b| natural(a, b));
    unsupported.sort();
    unsupported.dedup();
    let pack = json!({ "name": name, "matches": match_all(&files), "where": guess_where(&name), "files": files, "unsupported": unsupported.into_iter().take(40).collect::<Vec<_>>(), "added": crate::js::iso_string(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)) });
    let (f, tmp) = (dest.join("pack.json"), dest.join("pack.json.tmp"));
    std::fs::write(&tmp, crate::js::stringify_indented(&pack, 1)).and_then(|()| std::fs::rename(&tmp, &f)).map_err(|e| e.to_string())?;
    read_pack(app_root, &id).ok_or_else(|| "The pack couldn't be read back.".into())
}

/// One pack, as its folder holds it now.
pub fn read_pack(app_root: &Path, id: &str) -> Option<Pack> {
    if !ID.is_match(id) {
        return None;
    }
    let dir = packs_dir(app_root).join(id);
    let p: Value = serde_json::from_slice(&std::fs::read(dir.join("pack.json")).ok()?).ok()?;
    let mut files: Vec<String> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| AUDIO.is_match(f)).collect();
    files.sort_by(|a, b| natural(a, b));
    let matches = MOMENTS.iter().map(|k| (k.to_string(), json!(p["matches"][k].as_str().filter(|f| files.iter().any(|x| x == f)).unwrap_or("")))).collect();
    static CON: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-z0-9]{1,16}$").unwrap());
    let only_on = p["where"].as_array().map(|w| w.iter().filter_map(Value::as_str).filter(|x| CON.is_match(x)).take(60).map(String::from).collect::<Vec<_>>()).filter(|w| !w.is_empty());
    Some(Pack {
        id: id.into(),
        name: p["name"].as_str().filter(|n| !n.is_empty()).unwrap_or(id).chars().take(60).collect(),
        matches,
        only_on,
        files,
        unsupported: p["unsupported"].as_array().map_or(Vec::new(), |u| u.iter().filter_map(Value::as_str).take(40).map(String::from).collect()),
    })
}

/// Every pack, by name.
pub fn list(app_root: &Path) -> Vec<Pack> {
    let Ok(rd) = std::fs::read_dir(packs_dir(app_root)) else { return Vec::new() };
    let mut out: Vec<Pack> = rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| read_pack(app_root, &e.file_name().to_string_lossy())).collect();
    out.sort_by(|a, b| natural(&a.name, &b.name).then_with(|| natural(&a.id, &b.id)));
    out
}

/// A pack's file, as its folder holds it.
pub fn file_of(app_root: &Path, pack: &Pack, file: &str) -> Option<PathBuf> {
    pack.files.iter().any(|f| f == file).then(|| packs_dir(app_root).join(&pack.id).join(file))
}

/// Takes a pack away: its folder, and only when it really is one under the packs folder.
pub fn remove(app_root: &Path, id: &str) -> bool {
    read_pack(app_root, id).is_some() && std::fs::remove_dir_all(packs_dir(app_root).join(id)).is_ok()
}

/// The cue a pack plays a moment for (renderer/sound.js MOMENT; keep in sync).
pub fn moment_of(cue: &str) -> Option<&'static str> {
    Some(match cue {
        "tick" | "flip" | "shelf" => "move",
        "open" | "panel" | "caseOpen" => "open",
        "back" => "back",
        "go" | "boot" => "play",
        "error" => "problem",
        _ => return None,
    })
}

/// The sound a shelf plays: its own (`by_shelf`), else a pack made for that console, else the main one.
pub fn sound_for(shelf: &str, main: &str, by_shelf: &std::collections::HashMap<String, String>, packs: &[Pack]) -> String {
    if let Some(own) = by_shelf.get(shelf) {
        return own.clone();
    }
    packs.iter().find(|p| p.only_on.as_ref().is_some_and(|w| w.iter().any(|x| x == shelf))).map_or_else(|| main.to_string(), Pack::sound_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP of stored entries.
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data) in entries {
            let off = out.len() as u32;
            let crc = crc32fast::hash(data);
            let mut lh = vec![0u8; 30];
            lh[..4].copy_from_slice(&0x0403_4b50u32.to_le_bytes());
            lh[14..18].copy_from_slice(&crc.to_le_bytes());
            lh[18..22].copy_from_slice(&(data.len() as u32).to_le_bytes());
            lh[22..26].copy_from_slice(&(data.len() as u32).to_le_bytes());
            lh[26..28].copy_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend(lh);
            out.extend(name.as_bytes());
            out.extend(*data);
            let mut ch = vec![0u8; 46];
            ch[..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
            ch[8..10].copy_from_slice(&0x800u16.to_le_bytes());
            ch[16..20].copy_from_slice(&crc.to_le_bytes());
            ch[20..24].copy_from_slice(&(data.len() as u32).to_le_bytes());
            ch[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
            ch[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
            ch[42..46].copy_from_slice(&off.to_le_bytes());
            cd.extend(ch);
            cd.extend(name.as_bytes());
        }
        let (cd_off, cd_len) = (out.len() as u32, cd.len() as u32);
        out.extend(cd);
        let mut end = vec![0u8; 22];
        end[..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
        end[8..10].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        end[10..12].copy_from_slice(&(entries.len() as u16).to_le_bytes());
        end[12..16].copy_from_slice(&cd_len.to_le_bytes());
        end[16..20].copy_from_slice(&cd_off.to_le_bytes());
        out.extend(end);
        out
    }

    #[test]
    fn a_file_s_name_says_its_moment() {
        assert_eq!(["SelectMove_02.wav", "cursor.ogg", "Decide.mp3", "cancel1.wav", "error_004.ogg", "BGM loop.ogg", "start game.flac", "other.wav"].map(match_name), ["move", "move", "open", "back", "problem", "music", "play", ""]);
        let m = match_all(&["click_010.ogg".into(), "click_002.ogg".into(), "back.wav".into()]);
        assert_eq!((m["move"].as_str(), m["back"].as_str(), m["open"].as_str()), (Some("click_002.ogg"), Some("back.wav"), Some("")));
        assert_eq!((guess_where("PS2 menu sounds"), guess_where("Wii U sounds"), guess_where("My sounds")), (json!(["ps2"]), json!(["wiiu"]), json!("all")));
    }

    #[test]
    fn packs_come_in_from_a_folder_or_a_zip_and_go() {
        let tmp = tempfile::tempdir().unwrap();
        let (app, src) = (tmp.path().join("app"), tmp.path().join("PS2 menu sounds"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        for f in ["cursor.wav", "decide.ogg", "readme.txt", "notes.docx", "sub/cursor.wav", "sound.aiff"] {
            std::fs::write(src.join(f), b"RIFF").unwrap();
        }
        let p = import_from(&app, std::slice::from_ref(&src), None).unwrap();
        assert_eq!((p.id.as_str(), p.name.as_str(), p.only_on.clone()), ("ps2-menu-sounds", "PS2 menu sounds", Some(vec!["ps2".to_string()])));
        assert_eq!(p.files, ["cursor.wav", "decide.ogg", "sub-cursor.wav"]);
        assert_eq!((p.file_for("move"), p.file_for("open"), p.file_for("back")), (Some("cursor.wav"), Some("decide.ogg"), None));
        assert_eq!(p.unsupported, ["sound.aiff"]);
        assert_eq!(p.sound_id(), "pack:ps2-menu-sounds");
        assert_eq!(pack_id_of("pack:ps2-menu-sounds"), Some("ps2-menu-sounds"));
        // the same name again: its own id
        assert_eq!(import_from(&app, &[src], None).unwrap().id, "ps2-menu-sounds-2");

        let z = tmp.path().join("kenney_ui-audio.zip");
        std::fs::write(&z, zip(&[("Audio/click1.ogg", b"OggS"), ("../evil.wav", b"x"), ("Audio/License.txt", b"cc0"), ("Audio/error.mp3", b"ID3")])).unwrap();
        let p = import_from(&app, &[z], None).unwrap();
        assert_eq!((p.name.as_str(), p.files.clone(), p.only_on.clone()), ("ui-audio", vec!["click1.ogg".to_string(), "error.mp3".to_string()], None));
        assert!(!tmp.path().join("evil.wav").exists() && !app.join("evil.wav").exists());
        assert_eq!(list(&app).len(), 3);
        let by: std::collections::HashMap<String, String> = [("gc".to_string(), "disc".to_string())].into();
        let packs = list(&app);
        assert_eq!((sound_for("gc", "aero", &by, &packs), sound_for("ps2", "aero", &by, &packs), sound_for("snes", "aero", &by, &packs)), ("disc".into(), "pack:ps2-menu-sounds".into(), "aero".into()));
        assert!(remove(&app, "ui-audio") && list(&app).len() == 2 && !remove(&app, "../app"));
        let bad = tmp.path().join("bad.zip");
        std::fs::write(&bad, b"not a zip at all, sorry, not at all").unwrap();
        assert!(import_from(&app, &[bad], None).is_err());
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("a.txt"), b"x").unwrap();
        assert_eq!(import_from(&app, &[docs], None).unwrap_err(), "No sound files in there (WAV, OGG, MP3 or FLAC).");
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/nx2.js (the games, not the emulator builds), as RustyShelf has it.
//! Switch 2 games. Most come unpacked: a folder with exefs/main (+ main.npdm) and romfs, or the same split as base/ and
//! updates/<version>/ (each update a full copy), or just <version>/ with no base. The shelf's file for such a game is
//! its base exefs/main (or its only version's), so favourites and history stay put when an update is added; the newest
//! version's is the one started.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::paths::is_file;

static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(exefs|romfs|updates?|dlc|mods|patches|base)$").unwrap());
static ID_IN_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\[((?:0100|0400)[0-9A-F]{12})\]").unwrap());
static EXEFS_MAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(^|[\\/])exefs[\\/]main$").unwrap());

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn dirs(d: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(d) else { return Vec::new() };
    rd.filter_map(Result::ok)
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn is_version(n: &str) -> bool {
    !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

fn version_number(n: &str) -> f64 {
    n.parse().unwrap_or(f64::INFINITY)
}

fn main_of(d: &Path) -> Option<PathBuf> {
    let f = d.join("exefs").join("main");
    (is_file(&f) && is_file(&d.join("exefs").join("main.npdm"))).then_some(f)
}

/// Whether a shelf file is an unpacked game's exefs/main.
pub fn is_unpacked(file: &Path) -> bool {
    EXEFS_MAIN.is_match(&file.to_string_lossy())
}

/// The unpacked games under a games folder: each one's base exefs/main, or its lowest version's.
pub fn find(root: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    for n in dirs(root) {
        if n.starts_with('.') {
            continue;
        }
        let d = root.join(&n);
        if let Some(m) = main_of(&d).or_else(|| main_of(&d.join("base"))) {
            out.push(m);
            continue;
        }
        let mut vers: Vec<String> = dirs(&d).into_iter().filter(|v| is_version(v)).collect();
        vers.sort_by(|a, b| version_number(a).total_cmp(&version_number(b)));
        if let Some(m) = vers.iter().find_map(|v| main_of(&d.join(v))) {
            out.push(m);
        } else if depth > 0 && !SKIP.is_match(&n) {
            find(&d, depth - 1, out);
        }
    }
}

/// The game's folder for a shelf file: …/Game/exefs/main, …/Game/base/exefs/main, …/Game/131072/exefs/main or
/// …/Game/updates/131072/exefs/main → …/Game.
pub fn game_root(file: &Path) -> PathBuf {
    let up = |p: &Path| p.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut d = up(&up(file));
    if name_of(&d).eq_ignore_ascii_case("base") {
        return up(&d);
    }
    if is_version(&name_of(&d)) {
        d = up(&d);
        let n = name_of(&d).to_lowercase();
        if n == "update" || n == "updates" {
            d = up(&d);
        }
    }
    d
}

/// The file to start: the newest full version's exefs/main (updates/<version> or <version> beside base).
pub fn boot_file(file: &Path) -> PathBuf {
    if !is_unpacked(file) {
        return file.to_path_buf();
    }
    let root = game_root(file);
    let mut list: Vec<(f64, PathBuf)> = Vec::new();
    for d in [root.join("updates"), root.clone()] {
        for n in dirs(&d) {
            if is_version(&n) {
                list.push((version_number(&n), d.join(&n)));
            }
        }
    }
    list.sort_by(|a, b| b.0.total_cmp(&a.0));
    list.iter().find_map(|(_, d)| main_of(d)).unwrap_or_else(|| file.to_path_buf())
}

/// The program id in main.npdm's ACI0 block, e.g. 0400C3F00006E000; empty when there's none.
pub fn title_id(file: &Path) -> String {
    let read = || -> Option<String> {
        let b = fs::read(file.parent()?.join("main.npdm")).ok()?;
        if b.get(..4)? != b"META" {
            return None;
        }
        let aci = u32::from_le_bytes(b.get(0x70..0x74)?.try_into().ok()?) as usize;
        if b.get(aci..aci + 4)? != b"ACI0" {
            return None;
        }
        let id = u64::from_le_bytes(b.get(aci + 0x10..aci + 0x18)?.try_into().ok()?);
        Some(format!("{id:016X}"))
    };
    read().unwrap_or_default()
}

fn id_in_name(s: &str) -> String {
    ID_IN_NAME.captures(s).map(|c| c[1].to_uppercase()).unwrap_or_default()
}

/// Unpacked games carry no name (it's in the control data, not dumped): known title ids first, so a folder called
/// "SR ROM" still shows the real title.
fn known(id: &str) -> Option<&'static str> {
    Some(match id {
        "0400C3F00006E000" => "Mario Kart World",
        "0400380020056000" => "Splatoon Raiders",
        "040074A01BF12000" => "Donkey Kong Bananza",
        "04007EF01F8AE000" => "Fire Emblem: Fortune's Weave",
        "0400D4201C6EA000" => "Mario Tennis Fever",
        "04008E001F098000" => "Nintendo Switch Sports Resort",
        "0400A3A01EE4C000" => "Pokémon Pokopia",
        "0100F43008C44000" => "Pokémon Legends: Z-A - Nintendo Switch 2 Edition",
        // Switch 2 Editions keep their Switch game's id: the folder or file name carries it
        "010015100B514000" => "Super Mario Bros. Wonder – Nintendo Switch 2 Edition",
        _ => return None,
    })
}

/// Known games without a readable id (a .nup carries none in its name), by their tidied name.
fn known_name(tidied: &str) -> Option<&'static str> {
    match tidied.to_lowercase().as_str() {
        "hyrule warriors age of imprisonment" => Some("Hyrule Warriors: Age of Imprisonment"),
        _ => None,
    }
}

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(TAGS, r"\[[^\]]*\]|\([^)]*\)");
re!(DOT_VERSION, r"(?i)[_.]v?[0-9]+(\.[0-9]+)+(?-u:\b)");
re!(SEPARATORS, r"[._]+");
re!(CAMEL, r"([a-z])([A-Z])");
re!(SWITCH2, r"(?i)(?-u:\b)Switch2(?-u:\b)");
re!(DUMP_WORDS, r"(?i)(?-u:\b)(base|rom|unpacked|extracted|exefs|romfs|nsw2?)(?-u:\b)");
re!(VERSION, r"(?i)(?-u:\b)v[0-9]+(\.[0-9]+)*(?-u:\b)");
re!(SPACES, r"\s+");
re!(ENDS, r"^[\s-]+|[\s-]+$");

/// A readable name from a folder or file name: dump words go ("base", "[BASE]", "ROM", version and id tags) and
/// joined words split ("MarioTennisFever_v1.1.1" → "Mario Tennis Fever").
pub fn tidy(n: &str) -> String {
    let n = TAGS.replace_all(n, " ");
    let n = DOT_VERSION.replace_all(&n, " ");
    let n = SEPARATORS.replace_all(&n, " ");
    let n = CAMEL.replace_all(&n, "$1 $2");
    let n = SWITCH2.replace_all(&n, "Switch 2");
    let n = DUMP_WORDS.replace_all(&n, " ");
    let n = VERSION.replace_all(&n, " ");
    let n = SPACES.replace_all(&n, " ");
    let n = ENDS.replace_all(&n, "").trim().to_string();
    known_name(&n).map(String::from).unwrap_or(n)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub title: String,
    /// empty when unknown
    pub title_id: String,
    /// the game's folder (an unpacked game) or the folder the file is in
    pub folder: PathBuf,
}

/// An unpacked game, from its exefs/main.
pub fn info(file: &Path) -> Info {
    let root = game_root(file);
    let id = title_id(file);
    let folder = name_of(&root);
    let title = known(&id)
        .or_else(|| known(&id_in_name(&folder)))
        .map(String::from)
        .unwrap_or_else(|| Some(tidy(&folder)).filter(|t| !t.is_empty()).unwrap_or(folder));
    Info { title, title_id: id, folder: root }
}

/// A packed game (.nsp / .xci / .nup): its id from the name when there is one, a known title, else the tidied name.
pub fn file_info(file: &Path) -> Info {
    let name = name_of(file);
    let base = match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[..i],
        _ => &name,
    };
    let id = id_in_name(base);
    let title = known(&id).map(String::from).unwrap_or_else(|| Some(tidy(base)).filter(|t| !t.is_empty()).unwrap_or(base.to_string()));
    Info { title, title_id: id, folder: file.parent().map(Path::to_path_buf).unwrap_or_default() }
}

/// main.npdm with a program id, for tests.
#[cfg(test)]
pub fn build_npdm(id: u64) -> Vec<u8> {
    let mut b = vec![0u8; 0x100];
    b[..4].copy_from_slice(b"META");
    b[0x70..0x74].copy_from_slice(&0x80u32.to_le_bytes());
    b[0x80..0x84].copy_from_slice(b"ACI0");
    b[0x90..0x98].copy_from_slice(&id.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpacked(dir: &Path, id: Option<u64>) {
        fs::create_dir_all(dir.join("exefs")).unwrap();
        fs::write(dir.join("exefs/main"), b"x").unwrap();
        fs::write(dir.join("exefs/main.npdm"), id.map(build_npdm).unwrap_or_default()).unwrap();
    }

    #[test]
    fn names() {
        assert_eq!(tidy("MarioTennisFever_v1.1.1"), "Mario Tennis Fever");
        assert_eq!(tidy("NintendoSwitch2_WelcomeTour [BASE]"), "Nintendo Switch 2 Welcome Tour");
        assert_eq!(tidy("Hyrule.Warriors.Age.of.Imprisonment.ROM"), "Hyrule Warriors: Age of Imprisonment");
        assert_eq!(file_info(Path::new("/g/x [0400C3F00006E000].nsp")).title, "Mario Kart World");
    }

    #[test]
    fn versions() {
        let t = tempfile::tempdir().unwrap();
        let g = t.path().join("SR ROM");
        unpacked(&g.join("base"), Some(0x0400_C3F0_0006_E000));
        unpacked(&g.join("updates/131072"), None);
        unpacked(&g.join("updates/65536"), None);
        let mut found = Vec::new();
        find(t.path(), 3, &mut found);
        assert_eq!(found, vec![g.join("base/exefs/main")]);
        assert_eq!(game_root(&found[0]), g);
        assert_eq!(boot_file(&found[0]), g.join("updates/131072/exefs/main"));
        let i = info(&found[0]);
        assert_eq!((i.title.as_str(), i.title_id.as_str()), ("Mario Kart World", "0400C3F00006E000"));

        let v = t.path().join("Group/Other Game");
        unpacked(&v.join("200"), None);
        unpacked(&v.join("30"), None);
        let mut found = Vec::new();
        find(&t.path().join("Group"), 3, &mut found);
        assert_eq!(found, vec![v.join("30/exefs/main")]);
        assert_eq!(info(&found[0]).title, "Other Game");
    }
}

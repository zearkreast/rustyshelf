// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/pcfolder.js, as RustyShelf has it.
//! The PC games folder (Settings → PC games): every folder in it is a game, named after the folder. On Linux a
//! program made for Linux wins (an AppRun, a start script, an AppImage, a Linux program), else the game's Windows
//! program; on Windows its Windows program. Folders an Epic Games install made (they hold `.egstore`) are left out.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::art::resolve;
use crate::js;
use crate::pcgames::{slug, PcGame};
use crate::scanner::read_dir_sorted;

const LINUX: bool = cfg!(target_os = "linux");

static HELPER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(unins.*|uninstall.*|setup.*|install.*|update.*|crash.*|.*crashhandler.*|vc_?redist.*|.*\.so(\.[0-9]+)*)$").unwrap());
static SKIP_DIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(squashfs-root|lib|lib32|lib64|libs|plugins|share|_?redist|_commonredist|directx|.*_original|backups?|old|mods?|saves?|logs?|cache|\.egstore)$").unwrap()
});

/// A folder's game id: readable, from its name; a name the readable part can't tell apart (letters beyond a–z,
/// "Half-Life" next to "Half Life") gets a short fingerprint of the full name too.
fn id_of(folder: &str, used: &mut HashSet<String>) -> String {
    let mut id = format!("pc-{}", slug(folder));
    if !folder.is_ascii() || used.contains(&id) {
        let hex: String = Sha1::digest(folder.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        id = format!("{id}-{}", &hex[..8]);
    }
    used.insert(id.clone());
    id
}

/// A name's words, for telling a game's own program from helpers ("Hollow Knight" → hollow, knight).
fn words(s: &str) -> Vec<String> {
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new("[^a-z0-9]+").unwrap());
    SPLIT.split(&s.to_lowercase()).filter(|w| w.len() > 1).map(String::from).collect()
}

fn shares_word(name: &str, folder: &str) -> bool {
    let w = words(folder);
    words(name).iter().any(|x| w.contains(x))
}

#[cfg(unix)]
fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_exec(_: &Path) -> bool {
    false
}

fn is_elf(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p).and_then(|mut f| f.read_exact(&mut b)).is_ok() && b == *b"\x7fELF"
}

fn name_of(e: &std::fs::DirEntry) -> String {
    e.file_name().to_string_lossy().into_owned()
}

fn is_dir_entry(e: &std::fs::DirEntry) -> bool {
    e.file_type().is_ok_and(|t| t.is_dir())
}

/// The Linux programs in a game's folder, best first: an AppRun (an unpacked AppImage), a start script (the shortest
/// name first), an AppImage, a Linux program at the top (one sharing a word with the folder's name first), then one
/// in a subfolder sharing a word.
fn linux_programs(dir: &Path, folder: &str) -> Vec<(String, bool)> {
    static SH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.sh$").unwrap());
    static APPIMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.appimage$").unwrap());
    static X86: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.x86_64$").unwrap());
    static EXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.[a-z0-9]{1,4}$").unwrap());
    let len = |n: &str| n.encode_utf16().count();
    let mut ranked: Vec<(u8, usize, String)> = Vec::new();
    let ents = read_dir_sorted(dir);
    for e in &ents {
        let (p, n) = (e.path(), name_of(e));
        if is_dir_entry(e) || HELPER.is_match(&n) {
            continue;
        }
        if n == "AppRun" && is_exec(&p) {
            ranked.push((0, 0, n));
        } else if SH.is_match(&n) && is_exec(&p) {
            ranked.push((1, len(&n), n));
        } else if APPIMAGE.is_match(&n) && is_exec(&p) {
            ranked.push((2, len(&n), n));
        } else if is_exec(&p) && !EXT.is_match(&X86.replace(&n, "")) && is_elf(&p) {
            ranked.push((if shares_word(&n, folder) { 3 } else { 4 }, len(&n), n));
        }
    }
    for e in ents.iter().filter(|e| is_dir_entry(e) && !SKIP_DIR.is_match(&name_of(e))) {
        for f in read_dir_sorted(&e.path()) {
            let (p, n) = (f.path(), name_of(&f));
            if f.file_type().is_ok_and(|t| t.is_file()) && shares_word(&n, folder) && !HELPER.is_match(&n) && is_exec(&p) && is_elf(&p) {
                ranked.push((5, len(&n), js::join(&[&name_of(e), &n])));
            }
        }
    }
    ranked.sort_by_key(|r| (r.0, r.1));
    ranked.into_iter().map(|r| (r.2, true)).collect()
}

struct WinProgram {
    rel: String,
    bat: bool,
    depth: u32,
    other_word: bool,
    size: u64,
}

/// The Windows programs in a game's folder (two folders deep), best first: the shallowest, then one sharing a word
/// with the folder's name, then the biggest. On Linux a batch file can be chosen too (Wine runs it), after the programs.
fn windows_programs(dir: &Path, folder: &str) -> Vec<(String, bool)> {
    static PROGRAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(if LINUX { r"(?i)\.(exe|bat)$" } else { r"(?i)\.exe$" }).unwrap());
    static BAT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.bat$").unwrap());
    fn walk(d: &Path, rel: &str, depth: u32, folder: &str, found: &mut Vec<WinProgram>) {
        for e in read_dir_sorted(d) {
            let (Ok(t), n) = (e.file_type(), name_of(&e)) else { continue };
            let inside = if rel.is_empty() { n.clone() } else { js::join(&[rel, &n]) };
            if t.is_file() && PROGRAM.is_match(&n) && !HELPER.is_match(&n) {
                let size = e.path().metadata().map_or(0, |m| m.len());
                found.push(WinProgram { rel: inside, bat: BAT.is_match(&n), depth, other_word: !shares_word(&n, folder), size });
            } else if t.is_dir() && depth < 2 && !SKIP_DIR.is_match(&n) {
                walk(&e.path(), &inside, depth + 1, folder, found);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, "", 0, folder, &mut found);
    found.sort_by(|a, b| (a.bat, a.depth, a.other_word).cmp(&(b.bat, b.depth, b.other_word)).then(b.size.cmp(&a.size)));
    found.into_iter().take(40).map(|f| (f.rel, false)).collect()
}

/// Every program a game could start with (its path inside the folder, and whether it's made for this system), the
/// one RustyShelf would pick first.
pub fn programs_in(dir: &Path, folder: &str) -> Vec<(String, bool)> {
    let mut out = if LINUX { linux_programs(dir, folder) } else { Vec::new() };
    out.extend(windows_programs(dir, folder));
    out
}

fn real(p: &str) -> String {
    std::fs::canonicalize(p).unwrap_or_else(|_| resolve(Path::new(p))).to_string_lossy().into_owned()
}

/// The games in the PC games folder. `own_dir`: a folder of the player's own copies, used instead of the shared one
/// of the same name. `programs`: the program chosen in Settings for a folder (`{ "<folder>": "<path inside it>" }`).
/// `skip`: folders already on the shelf another way (Epic's installs).
pub fn scan(games_dir: &str, own_dir: &str, programs: Option<&Value>, skip: &[String]) -> Vec<PcGame> {
    if games_dir.is_empty() {
        return Vec::new();
    }
    let dirs_in = |d: &str| -> Vec<String> { read_dir_sorted(Path::new(d)).iter().filter(|e| is_dir_entry(e)).map(name_of).collect() };
    let own: HashSet<String> = if own_dir.is_empty() { HashSet::new() } else { dirs_in(own_dir).into_iter().collect() };
    let taken: HashSet<String> = skip.iter().filter(|s| !s.is_empty()).map(|s| real(s)).collect();
    let mut folders = dirs_in(games_dir);
    // in name order, as JS compares strings: ids stay the same between scans
    folders.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    let (mut out, mut used) = (Vec::new(), HashSet::new());
    for folder in folders.into_iter().filter(|f| !f.starts_with('.')) {
        let shared = js::join(&[games_dir, &folder]);
        let dir = if own.contains(&folder) { js::join(&[own_dir, &folder]) } else { shared.clone() };
        if Path::new(&js::join(&[&shared, ".egstore"])).metadata().is_ok() || taken.contains(&real(&shared)) {
            continue;
        }
        let progs = programs_in(Path::new(&dir), &folder);
        let wanted = js::str_of(programs.and_then(|p| p.get(&folder)));
        let Some(chosen) = progs.iter().find(|p| Some(p.0.as_str()) == wanted).or(progs.first()) else { continue };
        out.push(PcGame {
            id: id_of(&folder, &mut used),
            source: "folder",
            title: folder,
            exe: js::join(&[&dir, &chosen.0]),
            native: chosen.1,
            dir,
            programs: progs,
            ..PcGame::default()
        });
    }
    out.sort_by(|a, b| crate::collate::compare_plain(&a.title, &b.title));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        let mut used = HashSet::new();
        assert_eq!(id_of("Half-Life", &mut used), "pc-half-life");
        assert!(id_of("Half Life", &mut used).starts_with("pc-half-life-"));
        assert_eq!(id_of("Öde", &mut used).len(), "pc-de-".len() + 8);
        assert!(!shares_word("HollowKnight.exe", "Hollow Knight") && shares_word("hollow_knight.exe", "Hollow Knight"));
    }
}

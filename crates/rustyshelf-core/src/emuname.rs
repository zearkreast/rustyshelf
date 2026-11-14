// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's shared/emuname.js, as RustyShelf has it.
//! A readable name for an emulator program, "<emulator> · <build>", from its path alone:
//! `C:\Switch 2 emu\lichii pgo\lichii.exe` → "Lichii · pgo", `/home/me/Applications/dolphin-emu.AppImage` → "Dolphin".
//! The emulator is a known name found in the program's name (else its folder's, else the program's first word); the
//! build is what's left of the program's name, or of the folder's when that folder is named after the emulator too.

use std::sync::LazyLock;

use regex::Regex;

/// What to look for in a name, and how to write it: "citron" before "citra", so the longer name wins
/// (shared/emuname.js KNOWN, keep in sync).
const KNOWN: &[(&str, &str)] = &[
    ("eglantine", "Eglantine"),
    ("lichii", "Lichii"),
    ("orchid", "Orchid"),
    ("citron", "Citron"),
    ("sudachi", "Sudachi"),
    ("ryujinx", "Ryujinx"),
    ("ryubing", "Ryubing"),
    ("eden", "Eden"),
    ("yuzu", "yuzu"),
    ("suyu", "Suyu"),
    ("dolphin", "Dolphin"),
    ("cemu", "Cemu"),
    ("pcsx2", "PCSX2"),
    ("rpcs3", "RPCS3"),
    ("shadps4", "shadPS4"),
    ("ppsspp", "PPSSPP"),
    ("xenia", "Xenia"),
    ("azahar", "Azahar"),
    ("lime3ds", "Lime3DS"),
    ("citra", "Citra"),
    ("duckstation", "DuckStation"),
    ("retroarch", "RetroArch"),
    ("melonds", "melonDS"),
    ("mgba", "mGBA"),
    ("desmume", "DeSmuME"),
    ("visualboyadvance", "VisualBoyAdvance-M"),
    ("vbam", "VisualBoyAdvance-M"),
    ("flycast", "Flycast"),
    ("rmg", "RMG"),
    ("kyty", "Kyty"),
    ("xemu", "xemu"),
    ("emuhawk", "BizHawk"),
    ("bizhawk", "BizHawk"),
    ("gopher64", "Gopher64"),
    ("rap64", "RAProject64"),
    ("raproject64", "RAProject64"),
    ("project64", "Project64"),
    ("vita3k", "Vita3K"),
    ("mame", "MAME"),
    ("blastem", "BlastEm"),
    ("stremio", "Stremio"),
    ("nuvio", "Nuvio"),
];

/// Words that say nothing about the build: platforms, packaging, the program's role.
static NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(windows?|win(32|64)?|x86|x86_64|64|x64|amd64|arm64|aarch64|linux|appimage|exe|qt|qt6|sdl2?|setup|portable|release|stable|bin|emu|emulator|cli|msvc|mingw|emuhawk|rap64)$").unwrap()
});

/// A name's words: split at spaces, `_ - ( ) [ ]` and dots that aren't inside a version number (2.1.0 stays whole); a
/// platform glued to a word goes ("x86_64mariotennis" → "mariotennis").
fn tokens(s: &str) -> Vec<String> {
    static PLATFORM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)x86_64|x86-64|win64|windows").unwrap());
    static SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s_\-()\[\]]+").unwrap());
    let mut unglued = String::with_capacity(s.len());
    let mut at = 0;
    while let Some(m) = PLATFORM.find_at(s, at) {
        if s[m.end()..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
            unglued.push_str(&s[at..m.start()]);
            unglued.push(' ');
            at = m.end();
        } else {
            let step = m.start() + s[m.start()..].chars().next().map_or(1, char::len_utf8);
            unglued.push_str(&s[at..step]);
            at = step;
        }
    }
    unglued.push_str(&s[at..]);
    let chars: Vec<char> = unglued.chars().collect();
    let digit = |i: Option<usize>| i.and_then(|i| chars.get(i)).is_some_and(char::is_ascii_digit);
    let spaced: String = chars.iter().enumerate().map(|(i, &c)| if c == '.' && !(digit(i.checked_sub(1)) && digit(Some(i + 1))) { ' ' } else { c }).collect();
    SPLIT.split(&spaced).filter(|t| !t.is_empty()).map(String::from).collect()
}

fn known(s: &str) -> &'static str {
    let l = s.to_lowercase();
    KNOWN.iter().find(|(id, _)| l.contains(id)).map_or("", |k| k.1)
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The build words of a name: the emulator's own word, platform noise and repeats left out; a commit hash shortened.
fn rest(s: &str, emu: &str) -> String {
    static HASH: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?i:[0-9a-f]{40})$").unwrap());
    let e = emu.to_lowercase();
    let n = e.chars().count();
    let mut out: Vec<String> = Vec::new();
    for t in tokens(s) {
        let l = t.to_lowercase();
        if NOISE.is_match(&t) || l == e {
            continue;
        }
        let len = t.chars().count();
        let mut t = if l.starts_with(&e) && len > n {
            t.chars().skip(n).collect()
        } else if l.ends_with(&e) && len > n {
            t.chars().take(len - n).collect()
        } else {
            t
        };
        if HASH.is_match(&t) {
            t.truncate(7);
        }
        if !t.is_empty() && !out.iter().any(|x| x.to_lowercase() == t.to_lowercase()) {
            out.push(t);
        }
    }
    out.join(" ")
}

fn segments(p: &str) -> Vec<&str> {
    p.split(['\\', '/']).filter(|s| !s.is_empty()).collect()
}

/// The emulator and its build ("" when nothing tells builds apart).
pub fn parts(p: &str) -> (String, String) {
    static PROGRAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(exe|appimage|sh|bin|bat|cmd|app)$").unwrap());
    static HASH: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)[0-9a-f]{40}").unwrap());
    let mut segs = segments(p);
    let (file, dir) = (segs.pop().unwrap_or(""), segs.pop().unwrap_or(""));
    let stem = PROGRAM.replace(file, "").into_owned();
    let emu = [known(&stem).to_string(), known(dir).to_string(), cap(tokens(&stem).first().map_or(stem.as_str(), String::as_str))]
        .into_iter()
        .find(|e| !e.is_empty())
        .unwrap_or_else(|| "Emulator".into());
    let mut build = rest(&stem, &emu);
    if build.is_empty() && !dir.is_empty() && (known(dir) == emu || HASH.is_match(dir)) {
        build = rest(dir, &emu);
    }
    (emu, build)
}

/// "Lichii · K code AI"; just "Dolphin" when nothing tells builds apart.
pub fn label(p: &str) -> String {
    let (emu, build) = parts(p);
    if build.is_empty() {
        emu
    } else {
        format!("{emu} · {build}")
    }
}

/// A known emulator's name in the program's name or folder ("" for a program it doesn't recognise).
pub fn known_name(p: &str) -> &'static str {
    let mut segs = segments(p);
    match known(segs.pop().unwrap_or("")) {
        "" => known(segs.pop().unwrap_or("")),
        k => k,
    }
}

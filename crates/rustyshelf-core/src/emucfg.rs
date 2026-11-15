// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/emucfg.js, as RustyShelf has it.
//! Emulator settings: small, safe changes so the controller's quit combo closes an emulator without a question. They
//! are made right before a launch (the emulator rewrites its settings when it exits, keeping what was set), and the
//! player's own files can be put back (Settings → Restore emulator settings).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde_json::{json, Value};

use crate::js;
use crate::paths::is_file;
use crate::scanner::read_dir_sorted;

/// An environment variable by name.
pub type Var<'a> = &'a dyn Fn(&str) -> Option<String>;

pub(crate) fn get(var: Var, k: &str) -> Option<String> {
    var(k).filter(|v| !v.is_empty())
}

pub(crate) fn home(var: Var) -> String {
    get(var, if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default()
}

fn exists(p: &str) -> bool {
    Path::new(p).symlink_metadata().is_ok()
}

static SECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new("^\\[([^\n\r\u{2028}\u{2029}]+)\\]\\s*$").unwrap());

fn section(line: &str) -> Option<&str> {
    SECTION.captures(line).map(|m| m.get(1).unwrap().as_str())
}

/// A line's key: what's before its first `=`.
fn key(line: &str) -> &str {
    js::trim(line.split('=').next().unwrap_or(""))
}

fn lines_of(src: &str) -> (Vec<String>, &'static str) {
    static NL: LazyLock<Regex> = LazyLock::new(|| Regex::new("\r?\n").unwrap());
    (NL.split(src).map(String::from).collect(), if src.contains("\r\n") { "\r\n" } else { "\n" })
}

/// A settings file read for editing.
struct Ini {
    src: String,
    /// its byte-order mark, kept aside so sections match, put back on write
    bom: bool,
    /// no file yet
    is_new: bool,
}

/// None when the file must be left alone: it can't be read (locked, cloud-only: read as empty it would be overwritten
/// with one key), or it's UTF-16, binary, or not UTF-8 (written back, an ANSI accented path would break).
/// `read_only`: only looked at, so bytes that aren't UTF-8 are fine.
fn read_ini(f: &str, read_only: bool) -> Option<Ini> {
    let buf = match std::fs::read(f) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Ini { src: String::new(), bom: false, is_new: true }),
        Err(_) => return None,
    };
    if buf.starts_with(&[0xff, 0xfe]) || buf.starts_with(&[0xfe, 0xff]) || buf.contains(&0) {
        return None;
    }
    if !read_only && std::str::from_utf8(&buf).is_err() {
        return None;
    }
    let src = String::from_utf8_lossy(&buf).into_owned();
    match src.strip_prefix('\u{feff}') {
        Some(rest) => Some(Ini { src: rest.to_string(), bom: true, is_new: false }),
        None => Some(Ini { src, bom: false, is_new: false }),
    }
}

/// The one-time backup of the player's own file; `.trustyshelf-bak` is the same, made under the app's old name.
pub fn bak_of(f: &str) -> String {
    let old = format!("{f}.trustyshelf-bak");
    if exists(&old) {
        old
    } else {
        format!("{f}.rustyshelf-bak")
    }
}

/// The backup is made once and never refreshed, so it stays the file from before RustyShelf; then a temporary file
/// takes the real one's place. No backup, no write.
fn write_ini(f: &str, text: &str, ini: &Ini) -> std::io::Result<()> {
    if !ini.is_new {
        let bak = bak_of(f);
        if !exists(&bak) {
            std::fs::copy(f, &bak)?;
        }
    }
    if let Some(d) = Path::new(f).parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = format!("{f}.tmp");
    let body = if ini.bom { format!("\u{feff}{text}") } else { text.to_string() };
    std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, f)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// A settings file of an emulator's own written again: its one-time backup made first, then a temporary file takes
/// its place.
pub(crate) fn write_keeping_copy(f: &Path, text: &str) -> Result<(), String> {
    let f = f.to_string_lossy();
    write_ini(&f, text, &Ini { src: String::new(), bom: false, is_new: false }).map_err(|e| e.to_string())
}

// ---------- Eden and its forks (Switch, Switch 2) ----------

/// Each fork keeps its own settings folder (main/emucfg.js EDEN_FORKS).
const EDEN_FORKS: &[&str] = &["eden", "yuzu", "citron", "sudachi", "suyu"];

/// Eglantine, the Switch 2 fork, keeps Eden's own settings folder.
fn is_eden(name: &str) -> bool {
    let l = name.to_lowercase();
    ["eden", "yuzu", "sudachi", "citron", "suyu", "eglantine"].iter().any(|n| l.contains(n))
}

/// Where qt-config.ini can be: in `user\config` next to the program (portable), or in the fork's settings folder,
/// the one named like the program first.
fn eden_inis(exe: &str, var: Var) -> Vec<String> {
    let mut c = vec![js::join(&[&js::dirname(exe), "user", "config", "qt-config.ini"])];
    let n0 = js::basename(exe).to_lowercase();
    let names: Vec<&str> = EDEN_FORKS.iter().copied().filter(|n| n0.contains(n)).chain(EDEN_FORKS.iter().copied().filter(|n| !n0.contains(n))).collect();
    if let Some(appdata) = get(var, "APPDATA") {
        c.extend(names.iter().map(|n| js::join(&[&appdata, n, "config", "qt-config.ini"])));
    }
    let xdg = get(var, "XDG_CONFIG_HOME").or_else(|| Some(home(var)).filter(|h| !h.is_empty()).map(|h| js::join(&[&h, ".config"])));
    if let Some(xdg) = xdg {
        for n in &names {
            c.push(js::join(&[&xdg, n, "qt-config.ini"]));
            c.push(js::join(&[&xdg, n, "config", "qt-config.ini"]));
        }
    }
    c
}

fn eden_ini(exe: &str, var: Var) -> Option<String> {
    eden_inis(exe, var).into_iter().find(|f| is_file(Path::new(f)))
}

/// Citron's game folders beyond the third, taken out of its settings for a launch: its size line's number, and the
/// lines.
#[derive(Clone)]
struct Gamedirs {
    original_size: u64,
    lines: Vec<String>,
}

static SAVED_GAMEDIRS: LazyLock<Mutex<HashMap<String, Gamedirs>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Beside the settings file, so the folders come back even after a crash: `{ originalSize, lines }`.
fn citron_marker(f: &str) -> String {
    js::join(&[&js::dirname(f), ".rustyshelf-citron-gamedirs.json"])
}

static GAMEDIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Paths\\gamedirs\\([0-9]+)\\").unwrap());
static GAMEDIR_SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^Paths\\gamedirs\\size=([0-9]+)$").unwrap());

fn number_in(m: &regex::Captures) -> u64 {
    m[1].parse().unwrap_or(u64::MAX)
}

/// Puts back the game folders `quiet_eden` took out of Citron's settings.
fn restore_citron_gamedirs(f: &str) -> bool {
    let marker = citron_marker(f);
    let saved = SAVED_GAMEDIRS.lock().unwrap().get(f).cloned().or_else(|| {
        let j: Value = serde_json::from_slice(&std::fs::read(&marker).ok()?).ok()?;
        let lines = j.get("lines")?.as_array()?.iter().map(|l| js::string(Some(l))).collect();
        Some(Gamedirs { original_size: js::number(j.get("originalSize")) as u64, lines })
    });
    let Some(data) = saved.filter(|d| !d.lines.is_empty()) else {
        let _ = std::fs::remove_file(&marker);
        SAVED_GAMEDIRS.lock().unwrap().remove(f);
        return false;
    };
    let Some(ini) = read_ini(f, false).filter(|i| !i.is_new) else { return false };
    let (mut lines, nl) = lines_of(&ini.src);
    let (mut sec, mut ui_at, mut last, mut size_at) = (String::new(), None, None, None);
    let mut there = std::collections::HashSet::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(s) = section(line) {
            sec = s.to_string();
            if sec == "UI" {
                ui_at = Some(i);
            }
            continue;
        }
        if sec != "UI" {
            continue;
        }
        if line.starts_with("Paths\\gamedirs\\size=") {
            size_at = Some(i);
        }
        if line.starts_with("Paths\\gamedirs\\") {
            last = Some(i);
            there.insert(line.clone());
        }
    }
    let mut changed = false;
    if let Some(i) = size_at {
        let want = format!("Paths\\gamedirs\\size={}", data.original_size);
        if lines[i] != want {
            lines[i] = want;
            changed = true;
        }
    }
    let insert: Vec<String> = data.lines.iter().filter(|l| !there.contains(*l)).cloned().collect();
    if !insert.is_empty() {
        changed = true;
        let at = last.or(ui_at).map_or(lines.len(), |i| i + 1);
        lines.splice(at..at, insert);
    }
    if changed && write_ini(f, &lines.join(nl), &ini).is_err() {
        return false;
    }
    let _ = std::fs::remove_file(&marker);
    SAVED_GAMEDIRS.lock().unwrap().remove(f);
    true
}

/// Eden / yuzu ask "Are you sure you want to stop the emulation?" when the window is asked to close: turned off. A
/// Citron with more than three game folders gets them capped to three for the launch (its scanner crashes), and
/// back afterwards.
pub fn quiet_eden(exe: &str, var: Var) -> bool {
    let Some(f) = eden_ini(exe, var) else { return false };
    restore_citron_gamedirs(&f);
    let Some(ini) = read_ini(&f, false).filter(|i| !i.is_new) else { return false };
    let mut want: Vec<(&str, &str)> = vec![("confirmClose", "false"), ("confirmClose\\default", "false"), ("confirmStop", "2"), ("confirmStop\\default", "false")];
    let (mut lines, nl) = lines_of(&ini.src);
    let citron = js::basename(exe).to_lowercase().contains("citron") || f.to_lowercase().contains("citron");
    let (mut original_size, mut removed) = (0, Vec::new());
    if citron {
        for line in &lines {
            if let Some(m) = GAMEDIR_SIZE.captures(line) {
                original_size = number_in(&m);
            }
            if GAMEDIR.captures(line).is_some_and(|m| number_in(&m) >= 4) {
                removed.push(line.clone());
            }
        }
    }
    let cap = citron && original_size > 3 && !removed.is_empty();
    if cap {
        want.push(("Paths\\gamedirs\\size", "3"));
    }
    let (mut sec, mut ui_at, mut changed) = (String::new(), None, false);
    let mut seen = Vec::new();
    for (i, line) in lines.iter_mut().enumerate() {
        if let Some(s) = section(line) {
            sec = s.to_string();
            if sec == "UI" {
                ui_at = Some(i);
            }
            continue;
        }
        if sec != "UI" {
            continue;
        }
        if let Some((k, v)) = want.iter().find(|(k, _)| *k == key(line)) {
            seen.push(*k);
            let set = format!("{k}={v}");
            if *line != set {
                *line = set;
                changed = true;
            }
        }
    }
    let missing: Vec<String> = want.iter().filter(|(k, _)| !seen.contains(k)).map(|(k, v)| format!("{k}={v}")).collect();
    if !missing.is_empty() {
        changed = true;
        match ui_at {
            Some(i) => {
                lines.splice(i + 1..i + 1, missing);
            }
            None => {
                lines.push("[UI]".into());
                lines.extend(missing);
            }
        }
    }
    if cap {
        lines.retain(|l| !GAMEDIR.captures(l).is_some_and(|m| number_in(&m) >= 4));
        let data = Gamedirs { original_size, lines: removed };
        let _ = std::fs::write(citron_marker(&f), json!({ "originalSize": data.original_size, "lines": data.lines }).to_string());
        SAVED_GAMEDIRS.lock().unwrap().insert(f.clone(), data);
        changed = true;
    }
    !changed || write_ini(&f, &lines.join(nl), &ini).is_ok()
}

// ---------- settings in sections ----------

/// Sets keys in one `[section]` of an ini (`Key = Value`); true if the file changed.
pub fn set_ini(f: &str, sect: &str, want: &[(&str, &str)]) -> std::io::Result<bool> {
    let want: Vec<(&str, Option<&str>)> = want.iter().map(|(k, v)| (*k, Some(*v))).collect();
    set_ini_or_drop(f, sect, &want)
}

/// `set_ini` where a key set to None goes from the section (an undo of "it wasn't there").
pub fn set_ini_or_drop(f: &str, sect: &str, want: &[(&str, Option<&str>)]) -> std::io::Result<bool> {
    let Some(ini) = read_ini(f, false) else { return Ok(false) };
    let (mut lines, nl) = if ini.src.is_empty() { (Vec::new(), "\n") } else { lines_of(&ini.src) };
    let (mut sec, mut at, mut changed) = (String::new(), None, false);
    let mut seen = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(s) = section(&lines[i]) {
            sec = s.to_string();
            if sec == sect {
                at = Some(i);
            }
            i += 1;
            continue;
        }
        if sec == sect {
            if let Some((k, v)) = want.iter().find(|(k, _)| *k == key(&lines[i])) {
                let Some(v) = v else {
                    lines.remove(i);
                    changed = true;
                    continue;
                };
                seen.push(*k);
                let value = lines[i].split_once('=').map_or("", |x| x.1);
                if js::trim(value) != *v {
                    lines[i] = format!("{k} = {v}");
                    changed = true;
                }
            }
        }
        i += 1;
    }
    let missing: Vec<String> = want.iter().filter(|(k, _)| !seen.contains(k)).filter_map(|(k, v)| v.map(|v| format!("{k} = {v}"))).collect();
    if !missing.is_empty() {
        changed = true;
        match at {
            Some(i) => {
                lines.splice(i + 1..i + 1, missing);
            }
            None => {
                if lines.last().is_some_and(|l| !l.is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{sect}]"));
                lines.extend(missing);
            }
        }
    }
    if !changed {
        return Ok(false);
    }
    let end = if lines.last().is_some_and(String::is_empty) { "" } else { nl };
    write_ini(f, &format!("{}{end}", lines.join(nl)), &ini)?;
    Ok(true)
}

// ---------- where emulators keep their settings ----------

/// What Windows calls the Documents folder in other languages, for OneDrive's backup of it (main/emucfg.js DOC_NAMES).
const DOC_NAMES: &[&str] = &["Documents", "Documenti", "Dokumente", "Documentos", "Documenten", "Dokumenty", "Mes documents"];

/// Where Windows really keeps Documents: it can be moved anywhere (its Location tab, another drive).
fn real_docs(var: Var) -> Option<String> {
    if let Some(d) = get(var, "BS_DOCUMENTS") {
        return Some(d);
    }
    #[cfg(windows)]
    {
        static ASKED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        ASKED
            .get_or_init(|| {
                let mut cmd = std::process::Command::new("reg");
                cmd.args(["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders", "/v", "Personal"]);
                std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
                let out = cmd.output().ok().filter(|o| o.status.success())?;
                static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)Personal\s+REG_\w+\s+(.+)").unwrap());
                let dir = LINE.captures(&String::from_utf8_lossy(&out.stdout)).map(|c| c[1].trim().to_string())?;
                Some(crate::paths::expand(&dir, Path::new(""))).filter(|d| !d.is_empty())
            })
            .clone()
    }
    #[cfg(not(windows))]
    None
}

/// The Documents folder(s): where Windows keeps it, the usual one, and the OneDrive ones Windows moves it to when
/// OneDrive backs it up (a business "OneDrive - Name" folder, a translated name).
pub fn docs_dirs(var: Var) -> Vec<String> {
    static ONEDRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^(?i:OneDrive( - .+)?)$").unwrap());
    let home = home(var);
    let mut l: Vec<String> = real_docs(var).into_iter().collect();
    l.push(js::join(&[&home, "Documents"]));
    l.push(js::join(&[&home, "OneDrive", "Documents"]));
    for e in read_dir_sorted(Path::new(&home)).into_iter().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
        let name = e.file_name().to_string_lossy().into_owned();
        if ONEDRIVE.is_match(&name) {
            l.extend(DOC_NAMES.iter().map(|n| js::join(&[&home, &name, n])));
        }
    }
    let mut seen = std::collections::HashSet::new();
    l.retain(|d| seen.insert(d.clone()));
    l
}

/// Dolphin's user folder, the way Dolphin itself picks it: `portable.txt` next to the program, else
/// `Documents\Dolphin Emulator` (older installs), else `%APPDATA%\Dolphin Emulator`.
pub fn dolphin_user_dirs(exe: &str, var: Var) -> Vec<String> {
    let dir = js::dirname(exe);
    if !exe.is_empty() && exists(&js::join(&[&dir, "portable.txt"])) {
        return vec![js::join(&[&dir, "User"])];
    }
    let appdata = get(var, "APPDATA").unwrap_or_else(|| js::join(&[&home(var), "AppData", "Roaming"]));
    docs_dirs(var).iter().map(|d| js::join(&[d, "Dolphin Emulator"])).chain([js::join(&[&appdata, "Dolphin Emulator"])]).collect()
}

/// PCSX2's `inis\PCSX2.ini`: next to the program (portable: only there) or in `Documents\PCSX2`; on Linux in
/// `~/.config/PCSX2` and the Flatpak's own folder. `every`: the Documents ones too even when portable (backups made
/// before it was).
pub fn pcsx2_inis(exe: &str, var: Var, every: bool) -> Vec<String> {
    let dir = js::dirname(exe);
    let mut c = Vec::new();
    if !exe.is_empty() && ["portable.ini", "portable.txt"].iter().any(|f| exists(&js::join(&[&dir, f]))) {
        c.push(js::join(&[&dir, "inis", "PCSX2.ini"]));
        if !every {
            return c;
        }
    }
    c.extend(docs_dirs(var).iter().map(|d| js::join(&[d, "PCSX2", "inis", "PCSX2.ini"])));
    if cfg!(target_os = "linux") || get(var, "BS_LINUX").is_some() {
        let home = home(var);
        let xdg = get(var, "XDG_CONFIG_HOME").unwrap_or_else(|| js::join(&[&home, ".config"]));
        c.push(js::join(&[&xdg, "PCSX2", "inis", "PCSX2.ini"]));
        c.push(js::join(&[&home, ".var", "app", "net.pcsx2.PCSX2", "config", "PCSX2", "inis", "PCSX2.ini"]));
    }
    c
}

/// PCSX2 asks "Are you sure you want to shut down the virtual machine?" when its window is closed: turned off.
pub fn pcsx2_no_confirm(exe: &str, var: Var) -> bool {
    let Some(f) = pcsx2_inis(exe, var, false).into_iter().find(|f| is_file(Path::new(f))) else { return false };
    set_ini(&f, "UI", &[("ConfirmShutdown", "false")]).is_ok()
}

fn rpcs3_ini(exe: &str) -> String {
    js::join(&[&js::dirname(exe), "GuiConfigs", "CurrentSettings.ini"])
}

/// RPCS3 shows "Exit game?" when its window is closed: turned off.
pub fn rpcs3_no_confirm(exe: &str) -> bool {
    static KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^confirmationBoxExitGame\s*=").unwrap());
    const SET: &str = "confirmationBoxExitGame=false";
    let f = rpcs3_ini(exe);
    if !exists(&f) {
        return false;
    }
    let Some(ini) = read_ini(&f, false).filter(|i| !i.is_new) else { return false };
    let (mut lines, nl) = lines_of(&ini.src);
    let (mut sec, mut at, mut seen, mut changed) = (String::new(), None, false, false);
    for (i, line) in lines.iter_mut().enumerate() {
        if let Some(s) = section(line) {
            sec = s.to_string();
            if sec == "Meta" {
                at = Some(i);
            }
            continue;
        }
        if sec == "Meta" && KEY.is_match(line) {
            seen = true;
            if *line != SET {
                *line = SET.into();
                changed = true;
            }
        }
    }
    if !seen {
        changed = true;
        match at {
            Some(i) => lines.insert(i + 1, SET.into()),
            None => lines.extend(["[Meta]".to_string(), SET.to_string()]),
        }
    }
    !changed || write_ini(&f, &lines.join(nl), &ini).is_ok()
}

// ---------- Dolphin: the quit combo for every pad ----------

/// The quit combos as Dolphin names their inputs (main/emucfg.js DOLPHIN_COMBO).
const DOLPHIN_COMBO: &[(&str, &[&str])] =
    &[("l3+r3", &["Thumb L", "Thumb R"]), ("start+back", &["Start", "Back"]), ("lb+rb+start", &["Shoulder L", "Shoulder R", "Start"]), ("back+b", &["Back", "Button E"])];

/// Dolphin names the east face button per backend: SDL "Button E", XInput / Windows.Gaming.Input "Button B".
fn input_name<'a>(device: &str, input: &'a str) -> &'a str {
    if input == "Button E" && !device.to_lowercase().starts_with("sdl/") {
        "Button B"
    } else {
        input
    }
}

/// An expression's terms: split at each `|` that's outside quotes and brackets.
fn terms(s: &str) -> Vec<String> {
    let (mut out, mut depth, mut quoted, mut cur) = (Vec::new(), 0i32, false, String::new());
    for ch in s.chars() {
        match ch {
            '`' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth -= 1,
            _ => {}
        }
        if !quoted && depth == 0 && ch == '|' {
            out.push(js::trim(&cur).to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    out.push(js::trim(&cur).to_string());
    out.retain(|t| !t.is_empty());
    out
}

/// The inputs of a term that's only quoted inputs joined by `&` in brackets: "(`a`&`b`)".
fn quoted_inputs(t: &str) -> Option<Vec<String>> {
    static TERM: LazyLock<Regex> = LazyLock::new(|| Regex::new("^\\(((?:`[^`]+`&)*`[^`]+`)\\)$").unwrap());
    let m = TERM.captures(t)?;
    Some(m[1].split('&').map(|x| x.chars().skip(1).take(x.chars().count().saturating_sub(2)).collect()).collect())
}

fn is_combo(device: &str, got: &str) -> bool {
    DOLPHIN_COMBO.iter().any(|(_, l)| l.iter().map(|i| input_name(device, i)).collect::<Vec<_>>().join("&") == got)
}

/// One bracketed expression from end to end.
fn wrapped(b: &str) -> bool {
    if !b.starts_with('(') {
        return false;
    }
    let (mut depth, mut quoted) = (0, false);
    let n = b.chars().count();
    for (i, c) in b.chars().enumerate() {
        match c {
            '`' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return i == n - 1;
                }
            }
            _ => {}
        }
    }
    false
}

/// Dolphin's Stop hotkey listens to one device (its Hotkeys controller). A Wii game played with another controller
/// then had no way back to the shelf, so the quit combo is added to Stop for every other controller its Wii Remote /
/// GameCube settings use, as fully named inputs (`Device:Input`). The player's own terms stay as they are.
pub fn dolphin_quit_pads(exe: &str, combo: &str, var: Var) -> bool {
    static DEVICE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mR)^\s*Device\s*=\s*(.+?)\s*$").unwrap());
    static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mR)^\s*General/Stop\s*=\s*(.*?)\s*$").unwrap());
    static NOT_PAD: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)keyboard|mouse|^Android|^Bluetooth|^XInput2?/").unwrap());
    let Some((_, inputs)) = DOLPHIN_COMBO.iter().find(|(k, _)| *k == combo) else { return false };
    let has = |d: &str, n: &str| exists(&js::join(&[d, "Config", n]));
    let Some(pick) = dolphin_user_dirs(exe, var).into_iter().filter(|d| exists(&js::join(&[d, "Config"]))).find(|d| has(d, "WiimoteNew.ini") || has(d, "GCPadNew.ini")) else {
        return false;
    };
    let cfg_dir = js::join(&[&pick, "Config"]);
    let hotkeys = js::join(&[&cfg_dir, "Hotkeys.ini"]);
    // no Hotkeys.ini yet: left alone. Dolphin uses its default hotkeys only while the file is missing; a new one
    // holding just Stop would blank all the others
    let Some(hot) = read_ini(&hotkeys, false).filter(|h| !h.is_new) else { return false };
    let first = |re: &Regex| re.captures(&hot.src).map_or(String::new(), |m| m[1].to_string());
    let (hot_dev, stop) = (first(&DEVICE), first(&STOP));
    let mut devs: Vec<String> = Vec::new();
    for n in ["WiimoteNew.ini", "GCPadNew.ini"] {
        let Some(ini) = read_ini(&js::join(&[&cfg_dir, n]), true) else { continue };
        for m in DEVICE.captures_iter(&ini.src) {
            if !devs.iter().any(|d| *d == m[1]) {
                devs.push(m[1].to_string());
            }
        }
    }
    let pad_ok = |d: &str| !d.is_empty() && d != hot_dev && !NOT_PAD.is_match(d) && !d.contains('`');
    // RustyShelf's own terms ("(`<pad>:<input>`&…)": one such pad, exactly one quit combo's inputs) are rebuilt every
    // time, so a changed combo reaches a pad that's already there
    let is_own = |t: &str| {
        let Some(ins) = quoted_inputs(t) else { return false };
        let dev = match ins[0].rfind(':') {
            Some(at) if at > 0 => &ins[0][..at],
            _ => "",
        };
        let prefix = format!("{dev}:");
        if !pad_ok(dev) || !ins.iter().all(|x| x.starts_with(&prefix)) {
            return false;
        }
        is_combo(dev, &ins.iter().map(|x| &x[prefix.len()..]).collect::<Vec<_>>().join("&"))
    };
    // the main pad's term RustyShelf writes when Stop was empty ("(`Back`&`Button B`)": no device) is its own too when
    // it's the only other term beside its pad terms
    let is_base = |t: &str| quoted_inputs(t).is_some_and(|ins| !ins.iter().any(|x| x.contains(':')) && is_combo(&hot_dev, &ins.join("&")));
    let all = terms(&stop);
    let (mine, rest): (Vec<&String>, Vec<&String>) = all.iter().partition(|t| is_own(t));
    let user: Vec<&String> = if !mine.is_empty() && rest.len() == 1 && is_base(rest[0]) { Vec::new() } else { rest };
    let extra: Vec<&String> = devs.iter().filter(|d| pad_ok(d)).collect();
    if extra.is_empty() && mine.is_empty() {
        return false;
    }
    let base = match user.as_slice() {
        [] => inputs.iter().map(|i| format!("`{}`", input_name(&hot_dev, i))).collect::<Vec<_>>().join("&"),
        [one] => one.to_string(),
        many => format!("({})", many.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(" | ")),
    };
    let base = if (base.contains('|') || base.contains('&')) && !wrapped(&base) { format!("({base})") } else { base };
    let pads = extra.iter().map(|d| format!("({})", inputs.iter().map(|i| format!("`{d}:{}`", input_name(d, i))).collect::<Vec<_>>().join("&")));
    let next = std::iter::once(base).chain(pads).collect::<Vec<_>>().join(" | ");
    let _ = set_ini(&hotkeys, "Hotkeys", &[("General/Stop", &next)]);
    true
}

// ---------- around a launch ----------

/// At startup: game folders a crash left out of Citron's settings go back in.
pub fn init(var: Var) {
    for f in eden_inis("citron", var) {
        restore_citron_gamedirs(&f);
    }
}

/// Before a game starts. `quit_combo`: the controller's quit combo (Settings → Controller), None when it's off.
pub fn before_launch(console_id: &str, exe: &str, quit_combo: Option<&str>, var: Var) {
    let name = js::basename(exe).to_lowercase();
    if (console_id == "sw" && is_eden(&name)) || console_id == "sw2" {
        quiet_eden(exe, var);
    }
    // the other changes look for the emulators' Windows settings folders, next to the program or in %APPDATA%: beside
    // a Flatpak launcher or in /usr/bin that would only make stray folders
    if cfg!(target_os = "linux") {
        return;
    }
    if let Some(combo) = quit_combo.filter(|_| matches!(console_id, "gc" | "wii") && name.contains("dolphin")) {
        dolphin_quit_pads(exe, combo, var);
    }
    if name.contains("pcsx2") {
        pcsx2_no_confirm(exe, var);
    }
    if name.contains("rpcs3") {
        rpcs3_no_confirm(exe);
    }
}

/// After the game closed.
pub fn after_exit(console_id: &str, exe: &str, var: Var) {
    if (console_id == "sw" && is_eden(&js::basename(exe))) || console_id == "sw2" {
        if let Some(f) = eden_ini(exe, var) {
            restore_citron_gamedirs(&f);
        }
    }
}

/// Puts back every backup these changes made (the player's file from before the first one) and drops the backup, so
/// a later change backs the file up as it is then. `emulators`: each console's id and program. Returns the files
/// restored.
pub fn restore_backups(emulators: &[(String, String)], var: Var) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut add = |f: String| {
        if !files.contains(&f) {
            files.push(f);
        }
    };
    for (sys, exe) in emulators.iter().filter(|(_, exe)| !exe.is_empty()) {
        let name = js::basename(exe).to_lowercase();
        // GFX.ini and Dolphin.ini: backups from older versions, which changed those
        if matches!(sys.as_str(), "gc" | "wii") || name.contains("dolphin") {
            for d in dolphin_user_dirs(exe, var) {
                for n in ["GFX.ini", "Dolphin.ini", "Hotkeys.ini"] {
                    add(js::join(&[&d, "Config", n]));
                }
            }
        }
        if matches!(sys.as_str(), "sw" | "sw2") || is_eden(&name) {
            for f in eden_inis(exe, var) {
                restore_citron_gamedirs(&f);
                add(f);
            }
        }
        if name.contains("pcsx2") {
            pcsx2_inis(exe, var, true).into_iter().for_each(&mut add);
        }
        if name.contains("rpcs3") {
            add(rpcs3_ini(exe));
        }
    }
    let mut done = Vec::new();
    for f in files {
        let bak = bak_of(&f);
        if !exists(&bak) {
            continue;
        }
        let tmp = format!("{f}.tmp");
        match std::fs::copy(&bak, &tmp).and_then(|_| std::fs::rename(&tmp, &f)).and_then(|()| std::fs::remove_file(&bak)) {
            Ok(()) => done.push(f),
            Err(_) => {
                let _ = std::fs::remove_file(&tmp);
            }
        }
    }
    done
}

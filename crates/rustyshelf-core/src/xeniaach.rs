// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/xeniaach.js.
//! Xbox 360 achievements from Xenia Canary. Xenia keeps each game's achievements for the signed-in profile in a file
//! named after the game's title id (`<content>\<XUID>\FFFE07D1\00010000\<XUID>\<TITLEID>.gpd`, an "XDBF" database):
//! every achievement's name, both descriptions, gamerscore and, once earned, when. Only read. The achievement pictures
//! aren't in it (they live inside the game); the game's icon is. Also whether a profile is signed in (achievements are
//! only recorded for one), whether Xenia's own pop-up is on, and a switch for that pop-up.

use std::path::{Path, PathBuf};

use base64::Engine;
use regex::Regex;
use serde_json::{json, Value};

use crate::emucfg::{home, Var};

const CONFIGS: [&str; 2] = ["xenia-canary.config.toml", "xenia.config.toml"];

/// Where this Xenia keeps its settings and content, and what they say.
pub struct Places {
    pub config: Option<PathBuf>,
    text: String,
    pub content: PathBuf,
    pub xuid: String,
    popups: String,
}

/// Beside the program when portable.txt is there, else Documents\Xenia.
pub fn places(exe: &Path, var: Var) -> Places {
    let dir = exe.parent().unwrap_or(Path::new("")).to_path_buf();
    let home = PathBuf::from(home(var));
    let roots = if dir.join("portable.txt").is_file() { vec![dir] } else { vec![home.join("Documents").join("Xenia"), home.join("OneDrive").join("Documents").join("Xenia")] };
    let found = roots.iter().flat_map(|r| CONFIGS.iter().map(move |n| r.join(n))).find_map(|f| crate::files::read_utf8(&f).map(|t| (f, t)));
    let root = found.as_ref().and_then(|(f, _)| f.parent().map(Path::to_path_buf)).unwrap_or_else(|| roots[0].clone());
    let text = found.as_ref().map_or(String::new(), |(_, t)| t.clone());
    let val = |k: &str| {
        let re = Regex::new(&format!(r#"(?m)^\s*{k}\s*=\s*("([^"]*)"|[^\s#]+)"#)).unwrap();
        re.captures(&text).map_or(String::new(), |c| c.get(2).or(c.get(1)).map_or(String::new(), |m| m.as_str().to_string()))
    };
    let content_root = val("content_root");
    Places {
        content: if content_root.is_empty() { root.join("content") } else { root.join(content_root) },
        xuid: val("logged_profile_slot_0_xuid").trim().to_string(),
        popups: val("show_achievement_notification"),
        config: found.map(|(f, _)| f),
        text,
    }
}

fn signed_in(p: &Places) -> bool {
    p.xuid.len() == 16 && p.xuid.bytes().all(|b| b.is_ascii_hexdigit())
}

/// For Accounts → Your emulators: the state ("ok", "signed-out", "unknown"), whether Xenia's pop-up is on, and why
/// it isn't ready.
pub fn status(exe: &Path, var: Var) -> (&'static str, bool, String) {
    let p = places(exe, var);
    if p.config.is_none() {
        return ("unknown", false, "Xenia settings not found (run Xenia once)".into());
    }
    if !signed_in(&p) {
        return ("signed-out", false, "No Xenia profile signed in".into());
    }
    ("ok", p.popups.eq_ignore_ascii_case("true"), String::new())
}

/// One achievement in a .gpd.
#[derive(Debug, Clone, PartialEq)]
pub struct Achievement {
    pub id: u32,
    pub title: String,
    pub desc: String,
    pub points: i32,
    pub earned: bool,
    /// ms since 1970, when earned and known
    pub at: Option<i64>,
}

/// What a game's .gpd holds.
#[derive(Debug, Default)]
pub struct Gpd {
    pub achievements: Vec<Achievement>,
    pub title: String,
    pub icon: Option<Vec<u8>>,
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// A UTF-16BE string ending in a 0 or at `end`, and where the next one starts.
fn utf16(b: &[u8], mut p: usize, end: usize) -> (String, usize) {
    let mut units = Vec::new();
    while p + 1 < end {
        let c = be16(b, p).unwrap_or(0);
        p += 2;
        if c == 0 {
            break;
        }
        units.push(c);
    }
    (String::from_utf16_lossy(&units), p)
}

/// One XDBF database; None when it isn't one.
pub fn read_gpd(b: &[u8]) -> Option<Gpd> {
    if b.len() < 24 || &b[..4] != b"XDBF" {
        return None;
    }
    let (ent_len, ent_cnt, free_len) = (be32(b, 8)? as usize, be32(b, 12)? as usize, be32(b, 16)? as usize);
    let base = 24 + ent_len * 18 + free_len * 8;
    if ent_cnt > ent_len || base > b.len() {
        return None;
    }
    let mut out = Gpd::default();
    for i in 0..ent_cnt {
        let o = 24 + i * 18;
        let (Some(ns), Some(id), Some(off), Some(len)) = (be16(b, o), be64(b, o + 2), be32(b, o + 10), be32(b, o + 14)) else { continue };
        let (d, len) = (base + off as usize, len as usize);
        if d >= b.len() || len == 0 {
            continue;
        }
        let end = b.len().min(d + len);
        if ns == 1 && len >= 28 {
            let (Some(size), Some(aid), Some(points), Some(flags), Some(t)) = (be32(b, d), be32(b, d + 4), be32(b, d + 12), be32(b, d + 16), be64(b, d + 20)) else { continue };
            let p = d + (size as usize).max(28);
            let (name, p) = utf16(b, p, end);
            let (got, p) = utf16(b, p, end);
            let (locked, _) = utf16(b, p, end);
            let earned = flags & 0x20000 != 0;
            // a Windows file time: 100 ns steps since 1601
            let ms = if t == 0 { 0 } else { (t / 10_000) as i64 - 11_644_473_600_000 };
            let desc = if earned || locked.is_empty() { got } else { locked };
            out.achievements.push(Achievement { id: aid, title: name.trim().into(), desc: desc.trim().into(), points: points as i32, earned, at: (earned && ms > 0).then_some(ms) });
        } else if ns == 5 && id == 0x8000 {
            out.title = utf16(b, d, end).0.trim().into();
        } else if ns == 2 && id == 0x8000 && b.get(d + 1..d + 4) == Some(b"PNG") {
            out.icon = Some(b[d..end].to_vec());
        }
    }
    out.achievements.sort_by_key(|a| a.id);
    Some(out)
}

/// The progress a .gpd holds, in the shape `ra::progress` gives (Details and the achievements list show it the same
/// way); None without achievements.
pub fn progress_of(g: &Gpd, title_id: &str) -> Option<Value> {
    if g.achievements.is_empty() {
        return None;
    }
    let list: Vec<Value> = g
        .achievements
        .iter()
        .map(|a| json!({ "id": a.id, "title": a.title, "desc": a.desc, "points": a.points, "type": "", "badge": "", "earned": a.earned, "hardcore": false, "date": a.at.map(crate::js::iso_string), "rarity": null }))
        .collect();
    let earned: Vec<&Achievement> = g.achievements.iter().filter(|a| a.earned).collect();
    let icon = g.icon.as_ref().map(|i| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(i)));
    let all = g.achievements.len();
    Some(json!({
        "id": title_id.to_uppercase(), "title": g.title, "console": "Xbox 360", "source": "xenia", "unit": "G", "icon": icon, "box": null,
        "total": all, "earned": earned.len(), "points": g.achievements.iter().map(|a| a.points).sum::<i32>(), "pointsEarned": earned.iter().map(|a| a.points).sum::<i32>(),
        "completion": null, "completionHardcore": null, "mastered": if earned.len() == all { json!("completed") } else { Value::Null }, "achievements": list, "url": null,
    }))
}

/// One game's progress; None when Xenia has nothing for it yet (it writes the file the first time the game runs).
pub fn progress(exe: &Path, title_id: &str, var: Var) -> Option<Value> {
    if title_id.len() != 8 || !title_id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let p = places(exe, var);
    if !signed_in(&p) {
        return None;
    }
    let file = p.content.join(&p.xuid).join("FFFE07D1").join("00010000").join(&p.xuid).join(format!("{}.gpd", title_id.to_uppercase()));
    progress_of(&read_gpd(&std::fs::read(file).ok()?)?, title_id)
}

/// Xenia's own pop-up on or off: that one line in its settings, the rest of the file untouched, a copy of the file
/// kept the first time. Xenia must be closed (it writes its settings back when it quits). Whether it changed.
pub fn set_popups(exe: &Path, on: bool, var: Var) -> Result<bool, String> {
    let p = places(exe, var);
    let Some(config) = p.config else { return Err("Xenia's settings file wasn't found. Open Xenia once first.".into()) };
    let want = if on { "true" } else { "false" };
    let line = Regex::new(r"(?m)^(\s*show_achievement_notification\s*=\s*)(true|false)").unwrap();
    let ui = Regex::new(r"(?m)^\[UI\][ \t]*\r?$").unwrap();
    let t = &p.text;
    let next = if line.is_match(t) {
        line.replace(t, |c: &regex::Captures| format!("{}{want}", &c[1])).into_owned()
    } else if let Some(m) = ui.find(t) {
        let nl = if t.contains("\r\n") { "\r\n" } else { "\n" };
        let head = m.as_str().trim_end_matches('\r');
        format!("{}{head}{nl}show_achievement_notification = {want}{}", &t[..m.start()], &t[m.start() + head.len()..])
    } else {
        format!("{t}{}\n[UI]\nshow_achievement_notification = {want}\n", if t.ends_with('\n') { "" } else { "\n" })
    };
    if next == *t {
        return Ok(false);
    }
    crate::emucfg::write_keeping_copy(&config, &next)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16be(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(u16::to_be_bytes).collect()
    }

    /// A .gpd with the game's name and two achievements, the second earned at `at` (ms).
    fn gpd(at: i64) -> Vec<u8> {
        let ach = |id: u32, points: u32, flags: u32, time: u64, name: &str, got: &str, locked: &str| {
            let mut d = Vec::new();
            for v in [28u32, id, 0, points, flags] {
                d.extend(v.to_be_bytes());
            }
            d.extend(time.to_be_bytes());
            d.extend(utf16be(name));
            d.extend(utf16be(got));
            d.extend(utf16be(locked));
            d
        };
        let time = ((at + 11_644_473_600_000) * 10_000) as u64;
        let data = [ach(2, 20, 0x20000, time, "Second", "You did it", "Do it"), ach(1, 10, 0, 0, "First", "Done", "Still to do"), utf16be("A Game")];
        let entries = [(1u16, 2u64), (1, 1), (5, 0x8000)];
        let mut b = b"XDBF".to_vec();
        for v in [0x10000u32, 3, 3, 0, 0] {
            b.extend(v.to_be_bytes());
        }
        let mut off = 0u32;
        for ((ns, id), d) in entries.iter().zip(&data) {
            b.extend(ns.to_be_bytes());
            b.extend(id.to_be_bytes());
            b.extend(off.to_be_bytes());
            b.extend((d.len() as u32).to_be_bytes());
            off += d.len() as u32;
        }
        for d in &data {
            b.extend(d);
        }
        b
    }

    #[test]
    fn a_gpd_gives_the_game_s_achievements_and_gamerscore() {
        let g = read_gpd(&gpd(1_700_000_000_000)).unwrap();
        assert_eq!(g.title, "A Game");
        assert_eq!(g.achievements.iter().map(|a| (a.id, a.title.as_str(), a.desc.as_str(), a.earned)).collect::<Vec<_>>(), [(1, "First", "Still to do", false), (2, "Second", "You did it", true)]);
        let p = progress_of(&g, "4d5307e6").unwrap();
        assert_eq!((p["id"].as_str(), p["earned"].as_u64(), p["total"].as_u64(), p["points"].as_i64(), p["pointsEarned"].as_i64(), p["unit"].as_str()), (Some("4D5307E6"), Some(1), Some(2), Some(30), Some(20), Some("G")));
        assert_eq!(p["achievements"][1]["date"], "2023-11-14T22:13:20.000Z");
        assert!(read_gpd(b"not a gpd at all, not at all").is_none());
        assert_eq!(crate::i18n::tr("tr", "1 of 2 · 20 of 30 G · Completed"), "1/2 · 20/30 G · Tamamlandı");
        assert_eq!(crate::i18n::tr("fr", "1 of 2 · 20 of 30 G"), "1 sur 2 · 20 sur 30 G");
    }

    #[test]
    fn xenia_s_profile_and_pop_up_are_read_and_switched() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("xenia_canary.exe");
        std::fs::write(tmp.path().join("portable.txt"), "").unwrap();
        let none = |_: &str| None;
        assert_eq!(status(&exe, &none).0, "unknown");
        let config = tmp.path().join("xenia-canary.config.toml");
        std::fs::write(&config, "[Profiles]\nlogged_profile_slot_0_xuid = \"\"\n").unwrap();
        assert_eq!(status(&exe, &none).0, "signed-out");
        std::fs::write(&config, "[Profiles]\nlogged_profile_slot_0_xuid = \"E03000ABCDEF0123\"\n[UI]\nfoo = 1\n").unwrap();
        assert_eq!(status(&exe, &none), ("ok", false, String::new()));
        assert_eq!(set_popups(&exe, true, &none), Ok(true));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), "[Profiles]\nlogged_profile_slot_0_xuid = \"E03000ABCDEF0123\"\n[UI]\nshow_achievement_notification = true\nfoo = 1\n");
        assert!(status(&exe, &none).1);
        assert_eq!(set_popups(&exe, true, &none), Ok(false));
        assert!(crate::emucfg::bak_of(&config.to_string_lossy()).ends_with(".rustyshelf-bak"));
        let dir = tmp.path().join("content").join("E03000ABCDEF0123").join("FFFE07D1").join("00010000").join("E03000ABCDEF0123");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("4D5307E6.gpd"), gpd(0)).unwrap();
        assert_eq!(progress(&exe, "4d5307e6", &none).map(|p| p["total"].clone()), Some(serde_json::json!(2)));
        assert!(progress(&exe, "nothex!!", &none).is_none());
    }
}

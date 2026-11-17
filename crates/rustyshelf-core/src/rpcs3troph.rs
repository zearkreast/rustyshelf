// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/rpcs3troph.js.
//! PS3 trophies from RPCS3. RPCS3 keeps each game's trophies, as a real PS3 does, in
//! `dev_hdd0\home\<user>\trophy\<NP communication id>\` beside the program: TROP.SFM (the list: names, details,
//! grades), TROPUSR.DAT (which are earned, and when) and TROPnnn.PNG (each trophy's picture). A game finds its folder
//! by its own TROPDIR (`<game>\PS3_GAME\TROPDIR\<id>\`, or `<title id>\TROPDIR\` for a digital one); an .iso, whose
//! files can't be read, by the game's name. A game not played yet shows its own pack's list, all locked. Only read,
//! apart from RPCS3's trophy pop-up switch (config.yml → Miscellaneous → "Show trophy popups", on unless off).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde_json::{json, Value};
use unicode_normalization::UnicodeNormalization;

/// A grade's type, name and PSN's points for it.
fn grade(g: &str) -> (&'static str, &'static str, u32) {
    match g {
        "P" => ("platinum", "Platinum", 180),
        "G" => ("gold", "Gold", 90),
        "S" => ("silver", "Silver", 30),
        _ => ("bronze", "Bronze", 15),
    }
}

fn unxml(s: &str) -> String {
    static CDATA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<!\[CDATA\[([\s\S]*?)\]\]>").unwrap());
    static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&#(\d+);").unwrap());
    let s = CDATA.replace_all(s, "$1").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
    let s = NUM.replace_all(&s, |c: &regex::Captures| c[1].parse::<u32>().ok().and_then(char::from_u32).map_or(String::new(), String::from));
    s.replace("&amp;", "&").trim().to_string()
}

/// A game's name as names are matched: lower case, no accents or marks, words apart.
fn norm(s: &str) -> String {
    static NOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());
    let s: String = s.to_lowercase().nfd().filter(|c| !('\u{300}'..='\u{36f}').contains(c) && !matches!(c, '™' | '®' | '©')).collect::<String>().replace('&', " and ");
    NOT.replace_all(&s, " ").trim().to_string()
}

fn read_text(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok().map(|t| t.trim_start_matches('\u{feff}').to_string())
}

/// One trophy in a TROP.SFM.
#[derive(Debug, Clone, PartialEq)]
pub struct Trophy {
    pub id: u32,
    pub name: String,
    pub detail: String,
    /// P, G, S or B
    pub grade: String,
    pub hidden: bool,
}

/// TROP.SFM: the game's name and its trophies, by id.
pub fn read_sfm(t: &str) -> (String, Vec<Trophy>) {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<title-name>([\s\S]*?)</title-name>").unwrap());
    static TROPHY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<trophy\s+([^>]*)>([\s\S]*?)</trophy>").unwrap());
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bid="(\d+)""#).unwrap());
    static TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bttype="([PGSB])""#).unwrap());
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<name>([\s\S]*?)</name>").unwrap());
    static DETAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<detail>([\s\S]*?)</detail>").unwrap());
    let inner = |re: &Regex, s: &str| re.captures(s).map_or(String::new(), |c| unxml(&c[1]));
    let mut list: Vec<Trophy> = TROPHY
        .captures_iter(t)
        .filter_map(|m| {
            let (a, body) = (&m[1], &m[2]);
            Some(Trophy {
                id: ID.captures(a)?[1].parse().ok()?,
                name: inner(&NAME, body),
                detail: inner(&DETAIL, body),
                grade: TYPE.captures(a).map_or("B".into(), |c| c[1].to_string()),
                hidden: a.contains("hidden=\"yes\""),
            })
        })
        .collect();
    list.sort_by_key(|t| t.id);
    (inner(&TITLE, t), list)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// TROPUSR.DAT: each earned trophy's id and when, in ms since 1970 (0 when the time isn't known). `now`: ms now.
pub fn read_usr(b: &[u8], now: i64) -> HashMap<u32, i64> {
    let mut out = HashMap::new();
    if b.len() < 48 || be32(b, 0) != Some(0x818F_54AD) {
        return out;
    }
    let tables = be32(b, 8).unwrap_or(0).min(16) as usize;
    for i in 0..tables {
        let h = 48 + i * 32;
        if h + 32 > b.len() {
            break;
        }
        // table 6: each trophy's state and time
        if be32(b, h) != Some(6) {
            continue;
        }
        let (Some(count), Some(mut p)) = (be32(b, h + 12), be64(b, h + 16).map(|p| p as usize)) else { continue };
        for _ in 0..count {
            if p + 0x30 > b.len() {
                break;
            }
            let (size, id, state, t) = (be32(b, p + 4).unwrap_or(0), be32(b, p + 16).unwrap_or(0), be32(b, p + 20).unwrap_or(0), be64(b, p + 32).unwrap_or(0));
            if state == 1 {
                // microseconds since the year 1
                let ms = if t == 0 { 0 } else { (t / 1000) as i64 - 62_135_596_800_000 };
                out.insert(id, if ms > 0 && ms < now + 86_400_000 { ms } else { 0 });
            }
            p += 16 + if size == 0 { 0x60 } else { size as usize };
        }
    }
    out
}

/// RPCS3's trophy folders, one per user, the default user first.
fn trophy_roots(exe: &Path) -> Vec<PathBuf> {
    let home = exe.parent().unwrap_or(Path::new("")).join("dev_hdd0").join("home");
    let mut users: Vec<String> = std::fs::read_dir(&home)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.len() == 8 && n.bytes().all(|b| b.is_ascii_digit())).collect())
        .unwrap_or_default();
    users.sort_by(|a, b| (a != "00000001").cmp(&(b != "00000001")).then(a.cmp(b)));
    users.into_iter().map(|u| home.join(u).join("trophy")).collect()
}

/// A trophy folder's game names (as matched) → their folders, and the folder's state they were read in.
type Titles = (String, HashMap<String, String>);

/// Each trophy set's game name (as matched) → its folder, kept until the trophy folder changes.
fn titles_of(root: &Path, dirs: &[String]) -> HashMap<String, String> {
    static MEMO: LazyLock<Mutex<HashMap<PathBuf, Titles>>> = LazyLock::new(Default::default);
    let stamp = crate::files::mtime_ms(root).unwrap_or(0);
    let key = format!("{stamp}|{}", dirs.len());
    if let Some((k, map)) = MEMO.lock().unwrap().get(root) {
        if *k == key {
            return map.clone();
        }
    }
    let mut map = HashMap::new();
    for n in dirs {
        if let Some(t) = read_text(&root.join(n).join("TROP.SFM")) {
            let k = norm(&read_sfm(&t).0);
            if !k.is_empty() {
                map.entry(k).or_insert_with(|| n.clone());
            }
        }
    }
    MEMO.lock().unwrap().insert(root.to_path_buf(), (key, map.clone()));
    map
}

/// The game's own NP communication ids, from its TROPDIR (EBOOT.BIN → USRDIR → PS3_GAME or the title id's folder).
fn own_ids(file: &Path) -> Vec<String> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^NP[A-Z]{2}\d{5}_\d{2}$").unwrap());
    if !file.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("eboot.bin")) {
        return Vec::new();
    }
    let Some(d) = file.parent().and_then(Path::parent).map(|p| p.join("TROPDIR")) else { return Vec::new() };
    std::fs::read_dir(d).map(|rd| rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().to_uppercase()).filter(|n| ID.is_match(n)).collect()).unwrap_or_default()
}

/// A trophy folder (RPCS3's, or one unpacked from the game's pack) in the shape `ra::progress` gives. `earned`: what's
/// earned (none for a game not played yet).
pub fn from_dir(dir: &Path, id: &str, earned: &HashMap<u32, i64>) -> Option<Value> {
    let text = read_text(&dir.join("TROP.SFM")).or_else(|| read_text(&dir.join("TROPCONF.SFM")))?;
    let (title, trophies) = read_sfm(&text);
    if trophies.is_empty() {
        return None;
    }
    let file = |p: PathBuf| p.is_file().then(|| p.to_string_lossy().into_owned());
    let list: Vec<Value> = trophies
        .iter()
        .map(|t| {
            let (kind, name, points) = grade(&t.grade);
            let got = earned.get(&t.id);
            let secret = t.hidden && got.is_none();
            json!({
                "id": t.id, "title": if secret { "Hidden trophy" } else { &t.name }, "desc": if secret { "Keep playing to find out" } else { &t.detail },
                "points": points, "type": kind, "grade": name, "badge": file(dir.join(format!("TROP{:03}.PNG", t.id))).unwrap_or_default(),
                "earned": got.is_some(), "hardcore": false, "date": got.filter(|ms| **ms > 0).map(|ms| crate::js::iso_string(*ms)), "rarity": null,
            })
        })
        .collect();
    let won = |a: &&Value| a["earned"] == true;
    let count = |g: &str| format!("{}/{}", list.iter().filter(|a| a["type"] == g).filter(won).count(), list.iter().filter(|a| a["type"] == g).count());
    let plat = list.iter().find(|a| a["type"] == "platinum");
    let grades = format!("{}Gold {} · Silver {} · Bronze {}", plat.map_or("", |p| if p["earned"] == true { "Platinum ✓ · " } else { "Platinum · " }), count("gold"), count("silver"), count("bronze"));
    let got = list.iter().filter(won).count();
    let sum = |only: bool| list.iter().filter(|a| !only || a["earned"] == true).map(|a| a["points"].as_u64().unwrap_or(0)).sum::<u64>();
    let mastered = if plat.is_some_and(|p| p["earned"] == true) { json!("platinum") } else if got == list.len() { json!("completed") } else { Value::Null };
    Some(json!({
        "id": id.to_uppercase(), "title": title, "console": "PS3", "source": "rpcs3", "unit": "trophy", "icon": file(dir.join("ICON0.PNG")), "box": null,
        "total": list.len(), "earned": got, "points": sum(false), "pointsEarned": sum(true), "grades": grades,
        "completion": null, "completionHardcore": null, "mastered": mastered, "achievements": list, "url": null,
    }))
}

/// The game's own trophy pack (TROPDIR\<id>\TROPHY.TRP, magic DCA24D00): its list and pictures unpacked once into
/// `out`. Whether `out` holds a list.
pub fn unpack_trp(trp: &Path, out: &Path) -> bool {
    static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[A-Z0-9_]{1,24}\.(SFM|PNG)$").unwrap());
    if out.join("TROP.SFM").is_file() || out.join("TROPCONF.SFM").is_file() {
        return true;
    }
    let Ok(b) = std::fs::read(trp) else { return false };
    if b.len() < 64 || be32(&b, 0) != Some(0xDCA2_4D00) {
        return false;
    }
    let n = be32(&b, 16).unwrap_or(0).min(512) as usize;
    let es = match be32(&b, 20).unwrap_or(0) {
        0 => 64,
        s => s as usize,
    };
    if std::fs::create_dir_all(out).is_err() {
        return false;
    }
    let mut wrote = 0;
    for i in 0..n {
        let e = 64 + i * es;
        if e + 64 > b.len() {
            break;
        }
        let raw = &b[e..e + 32];
        let name = String::from_utf8_lossy(&raw[..raw.iter().position(|c| *c == 0).unwrap_or(32)]).into_owned();
        let (Some(off), Some(size)) = (be64(&b, e + 32).map(|v| v as usize), be64(&b, e + 40).map(|v| v as usize)) else { continue };
        // only the list and the pictures, never a path
        if !NAME.is_match(&name) || off.checked_add(size).is_none_or(|end| end > b.len()) || size > 8 * 1_048_576 {
            continue;
        }
        if std::fs::write(out.join(name.to_uppercase()), &b[off..off + size]).is_ok() {
            wrote += 1;
        }
    }
    wrote > 0
}

/// Every trophy set RPCS3 keeps for its first user that has any, for the Trophies screen: `{ id, title, console,
/// icon, total, earned, pct, award, last, src: "rpcs3" }` each, the last played first; and the trophies won in the
/// last `days`.
pub fn all(exe: &Path, now: i64, days: i64) -> (Vec<Value>, Vec<Value>) {
    let (mut games, mut recent) = (Vec::new(), Vec::new());
    let since = now - days * 86_400_000;
    for root in trophy_roots(exe) {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            let id = e.file_name().to_string_lossy().into_owned();
            let earned = std::fs::read(e.path().join("TROPUSR.DAT")).map(|b| read_usr(&b, now)).unwrap_or_default();
            let Some(p) = from_dir(&e.path(), &id, &earned) else { continue };
            let last = earned.values().copied().max().filter(|t| *t > 0);
            let total = p["total"].as_u64().unwrap_or(0);
            let got = p["earned"].as_u64().unwrap_or(0);
            for (t, at) in earned.iter().filter(|(_, at)| **at >= since) {
                if let Some(a) = p["achievements"].as_array().and_then(|l| l.iter().find(|a| a["id"] == *t)) {
                    recent.push(json!({ "id": format!("{id}:{t}"), "title": a["title"], "desc": a["desc"], "points": a["points"], "grade": a["grade"], "badge": a["badge"],
                        "game": p["title"], "gameId": id.to_uppercase(), "console": "PS3", "hardcore": false, "date": crate::js::iso_string(*at), "rarity": null, "src": "rpcs3" }));
                }
            }
            games.push(json!({ "id": id.to_uppercase(), "title": p["title"], "console": "PS3", "icon": p["icon"], "total": total, "earned": got,
                "pct": (100 * got + total / 2).checked_div(total).unwrap_or(0), "award": p["mastered"], "last": last.map(crate::js::iso_string), "src": "rpcs3" }));
        }
        if !games.is_empty() {
            break;
        }
    }
    games.sort_by_key(|g| std::cmp::Reverse(g["last"].as_str().unwrap_or("").to_string()));
    recent.sort_by_key(|g| std::cmp::Reverse(g["date"].as_str().unwrap_or("").to_string()));
    (games, recent)
}

/// One trophy set by its folder's name (an NP communication id), as [`progress`] gives it.
pub fn set(exe: &Path, id: &str, now: i64) -> Option<Value> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    trophy_roots(exe).into_iter().find_map(|root| {
        let dir = std::fs::read_dir(&root).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(id))?.path();
        let earned = std::fs::read(dir.join("TROPUSR.DAT")).map(|b| read_usr(&b, now)).unwrap_or_default();
        from_dir(&dir, id, &earned)
    })
}

/// One game's trophies: RPCS3's own folder once the game has run (what's earned, and when), else the game's own
/// pack (all locked, unpacked into `cache`), else None.
pub fn progress(exe: &Path, title: &str, file: &Path, cache: Option<&Path>, now: i64) -> Option<Value> {
    let (ids, want) = (own_ids(file), norm(title));
    for root in trophy_roots(exe) {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        let dirs: Vec<String> = rd.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let pick = dirs.iter().find(|n| ids.contains(&n.to_uppercase())).cloned().or_else(|| (!want.is_empty()).then(|| titles_of(&root, &dirs).get(&want).cloned()).flatten());
        let Some(pick) = pick else { continue };
        let earned = std::fs::read(root.join(&pick).join("TROPUSR.DAT")).map(|b| read_usr(&b, now)).unwrap_or_default();
        if let Some(p) = from_dir(&root.join(&pick), &pick, &earned) {
            return Some(p);
        }
    }
    let (cache, game) = (cache?, file.parent()?.parent()?);
    ids.iter().find_map(|id| {
        let out = cache.join(id);
        unpack_trp(&game.join("TROPDIR").join(id).join("TROPHY.TRP"), &out).then(|| from_dir(&out, id, &HashMap::new())).flatten()
    })
}

fn config_file(exe: &Path) -> Option<(PathBuf, String)> {
    let dir = exe.parent().unwrap_or(Path::new(""));
    [dir.join("config").join("config.yml"), dir.join("config.yml")].into_iter().find_map(|f| read_text(&f).map(|t| (f, t)))
}

static POP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(\s*Show trophy popups:\s*)(true|false)[ \t]*\r?$").unwrap());

/// For Accounts → Your emulators: RPCS3 needs no sign-in (trophies are kept on this PC); whether its own pop-up is on,
/// and why it may not be ready.
pub fn status(exe: &Path) -> (&'static str, bool, String) {
    match config_file(exe) {
        Some((_, t)) => ("ok", POP.captures(&t).is_none_or(|c| &c[2] == "true"), String::new()),
        None => ("ok", true, "Open RPCS3 once to make its settings".into()),
    }
}

/// RPCS3's pop-up on or off: that one line in config.yml, the rest untouched, a copy kept the first time. RPCS3 must
/// be closed (it writes its settings when it quits). Whether it changed.
pub fn set_popups(exe: &Path, on: bool) -> Result<bool, String> {
    let Some((f, t)) = config_file(exe) else { return Err("RPCS3's settings file wasn't found. Open RPCS3 once first.".into()) };
    let (want, nl) = (if on { "true" } else { "false" }, if t.contains("\r\n") { "\r\n" } else { "\n" });
    static MISC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Miscellaneous:[ \t]*$").unwrap());
    let next = if let Some(c) = POP.captures(&t) {
        let m = c.get(2).unwrap();
        format!("{}{want}{}", &t[..m.start()], &t[m.end()..])
    } else if let Some(m) = MISC.find(&t) {
        format!("{}{nl}  Show trophy popups: {want}{}", &t[..m.end()], &t[m.end()..])
    } else {
        format!("{t}{}Miscellaneous:{nl}  Show trophy popups: {want}{nl}", if t.ends_with('\n') { "" } else { nl })
    };
    if next == t {
        return Ok(false);
    }
    crate::emucfg::write_keeping_copy(&f, &next)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SFM: &str = r#"<?xml version="1.0"?><trophyconf><title-name>Ratchet &amp; Clank</title-name>
<trophy id="001" hidden="no" ttype="G"><name>Gold one</name><detail>Do the gold thing</detail></trophy>
<trophy id="000" hidden="no" ttype="P"><name>All of them</name><detail>Every trophy</detail></trophy>
<trophy id="002" hidden="yes" ttype="B"><name><![CDATA[Secret]]></name><detail>Shh</detail></trophy></trophyconf>"#;

    /// A TROPUSR.DAT with trophy 1 earned at `ms`.
    fn usr(ms: i64) -> Vec<u8> {
        let mut b = vec![0u8; 48 + 32];
        b[..4].copy_from_slice(&0x818F_54ADu32.to_be_bytes());
        b[8..12].copy_from_slice(&1u32.to_be_bytes());
        b[48..52].copy_from_slice(&6u32.to_be_bytes());
        b[60..64].copy_from_slice(&1u32.to_be_bytes());
        let at = b.len() as u64;
        b[64..72].copy_from_slice(&at.to_be_bytes());
        let mut e = vec![0u8; 16 + 0x60];
        e[4..8].copy_from_slice(&0x60u32.to_be_bytes());
        e[16..20].copy_from_slice(&1u32.to_be_bytes());
        e[20..24].copy_from_slice(&1u32.to_be_bytes());
        e[32..40].copy_from_slice(&(((ms + 62_135_596_800_000) * 1000) as u64).to_be_bytes());
        b.extend(e);
        b
    }

    fn trp(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = vec![0u8; 64];
        b[..4].copy_from_slice(&0xDCA2_4D00u32.to_be_bytes());
        b[16..20].copy_from_slice(&(files.len() as u32).to_be_bytes());
        b[20..24].copy_from_slice(&64u32.to_be_bytes());
        let mut off = 64 + 64 * files.len();
        let mut body: Vec<u8> = Vec::new();
        for (name, data) in files {
            let mut e = vec![0u8; 64];
            e[..name.len()].copy_from_slice(name.as_bytes());
            e[32..40].copy_from_slice(&(off as u64).to_be_bytes());
            e[40..48].copy_from_slice(&(data.len() as u64).to_be_bytes());
            b.extend(e);
            body.extend(*data);
            off += data.len();
        }
        b.extend(body);
        b
    }

    #[test]
    fn a_trophy_list_and_what_s_earned_are_read() {
        let (title, list) = read_sfm(SFM);
        assert_eq!(title, "Ratchet & Clank");
        assert_eq!(list.iter().map(|t| (t.id, t.grade.as_str(), t.hidden)).collect::<Vec<_>>(), [(0, "P", false), (1, "G", false), (2, "B", true)]);
        assert_eq!(list[2].name, "Secret");
        let got = read_usr(&usr(1_700_000_000_000), 1_800_000_000_000);
        assert_eq!(got.get(&1), Some(&1_700_000_000_000));
        assert!(read_usr(b"nope", 0).is_empty());
    }

    #[test]
    fn rpcs3_s_folder_then_the_game_s_own_pack() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("rpcs3").join("rpcs3.exe");
        let set = tmp.path().join("rpcs3").join("dev_hdd0").join("home").join("00000001").join("trophy").join("NPWR00001_00");
        std::fs::create_dir_all(&set).unwrap();
        std::fs::write(set.join("TROP.SFM"), SFM).unwrap();
        std::fs::write(set.join("TROPUSR.DAT"), usr(1_700_000_000_000)).unwrap();
        std::fs::write(set.join("TROP001.PNG"), b"png").unwrap();
        // an .iso: found by the game's name
        let p = progress(&exe, "Ratchet and Clank", &tmp.path().join("game.iso"), None, 1_800_000_000_000).unwrap();
        assert_eq!((p["earned"].as_u64(), p["total"].as_u64(), p["points"].as_u64(), p["pointsEarned"].as_u64()), (Some(1), Some(3), Some(285), Some(90)));
        assert_eq!(p["grades"], "Platinum · Gold 1/1 · Silver 0/0 · Bronze 0/1");
        assert_eq!((p["achievements"][2]["title"].as_str(), p["achievements"][1]["badge"].as_str().is_some_and(|b| b.ends_with("TROP001.PNG"))), (Some("Hidden trophy"), true));
        // a folder game not played yet: its own pack, every trophy locked
        let game = tmp.path().join("Game").join("PS3_GAME");
        let eboot = game.join("USRDIR").join("EBOOT.BIN");
        std::fs::create_dir_all(eboot.parent().unwrap()).unwrap();
        std::fs::write(&eboot, b"").unwrap();
        let pack = game.join("TROPDIR").join("NPWR99999_00");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("TROPHY.TRP"), trp(&[("TROPCONF.SFM", SFM.as_bytes()), ("../evil.png", b"x"), ("ICON0.PNG", b"i")])).unwrap();
        let cache = tmp.path().join("packs");
        let p = progress(&exe, "Another Game", &eboot, Some(&cache), 0).unwrap();
        assert_eq!((p["id"].as_str(), p["earned"].as_u64(), p["total"].as_u64()), (Some("NPWR99999_00"), Some(0), Some(3)));
        assert!(cache.join("NPWR99999_00").join("ICON0.PNG").is_file() && !tmp.path().join("evil.png").exists());
    }

    #[test]
    fn every_set_and_one_by_its_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("rpcs3").join("rpcs3.exe");
        let set_dir = tmp.path().join("rpcs3").join("dev_hdd0").join("home").join("00000001").join("trophy").join("NPWR00001_00");
        std::fs::create_dir_all(&set_dir).unwrap();
        std::fs::write(set_dir.join("TROP.SFM"), SFM).unwrap();
        std::fs::write(set_dir.join("TROPUSR.DAT"), usr(1_700_000_000_000)).unwrap();
        let (games, recent) = all(&exe, 1_700_000_100_000, 30);
        assert_eq!(games.len(), 1);
        assert_eq!((games[0]["id"].as_str(), games[0]["earned"].as_u64(), games[0]["pct"].as_u64(), games[0]["src"].as_str()), (Some("NPWR00001_00"), Some(1), Some(33), Some("rpcs3")));
        assert_eq!((recent.len(), recent[0]["title"].as_str(), recent[0]["grade"].as_str()), (1, Some("Gold one"), Some("Gold")));
        assert!(all(&exe, 1_700_000_000_000 + 40 * 86_400_000, 30).1.is_empty(), "older than 30 days");
        assert_eq!(set(&exe, "npwr00001_00", 0).unwrap()["total"].as_u64(), Some(3));
        assert!(set(&exe, "../x", 0).is_none());
    }

    #[test]
    fn rpcs3_s_pop_up_switch() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("rpcs3.exe");
        assert!(set_popups(&exe, false).is_err());
        let cfg = tmp.path().join("config").join("config.yml");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "Core:\n  x: 1\nMiscellaneous:\n  Other: 2\n").unwrap();
        assert!(status(&exe).1);
        assert_eq!(set_popups(&exe, false), Ok(true));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "Core:\n  x: 1\nMiscellaneous:\n  Show trophy popups: false\n  Other: 2\n");
        assert!(!status(&exe).1);
        assert_eq!(set_popups(&exe, true), Ok(true));
        assert!(std::fs::read_to_string(&cfg).unwrap().contains("Show trophy popups: true\n"));
    }
}

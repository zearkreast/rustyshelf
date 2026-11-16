// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/steamdeck.js, as RustyShelf has it.
//! Linux / Steam Deck: where RustyShelf runs (a Deck? started by Steam? Game Mode?) and Add RustyShelf to Steam: an
//! entry in Steam's list of non-Steam games (userdata/<user>/config/shortcuts.vdf, binary KeyValues), so it starts from
//! Steam and Game Mode and its emulators get a real gamepad. Steam rewrites shortcuts.vdf when it exits, so it's only
//! written while Steam is closed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;

type Var<'a> = &'a dyn Fn(&str) -> Option<String>;

fn read(f: &str) -> String {
    fs::read_to_string(f).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// A Steam Deck (Valve's board; LCD "Jupiter", OLED "Galileo"). `BS_FAKE_DECK=1`: a pretend Deck, for trying it out.
pub fn is_deck(var: Var) -> bool {
    if var("BS_FAKE_DECK").as_deref() == Some("1") {
        return true;
    }
    cfg!(target_os = "linux") && (read("/sys/devices/virtual/dmi/id/board_vendor") == "Valve" || matches!(read("/sys/devices/virtual/dmi/id/product_name").to_lowercase().as_str(), "jupiter" | "galileo"))
}

pub fn started_by_steam(var: Var) -> bool {
    ["SteamGameId", "SteamClientLaunch", "SteamEnv", "SteamGamepadUI", "SteamAppId"].iter().any(|k| var(k).is_some_and(|v| !v.is_empty()))
}

pub fn game_mode(var: Var) -> bool {
    var("XDG_CURRENT_DESKTOP").as_deref() == Some("gamescope") || var("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) || var("SteamGamepadUI").is_some_and(|v| !v.is_empty())
}

/// Where RustyShelf runs, for the Steam Deck card and Settings → System.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Info {
    pub linux: bool,
    pub deck: bool,
    pub steam: bool,
    pub game_mode: bool,
    /// the program Steam would start: the AppImage when RustyShelf runs from one, else its own program
    pub exe: String,
    pub can_add: bool,
    pub in_steam: bool,
}

pub fn info(var: Var) -> Info {
    let linux = cfg!(target_os = "linux") || var("BS_FAKE_DECK").as_deref() == Some("1");
    let exe = var("APPIMAGE").filter(|a| !a.is_empty()).or_else(|| std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned())).unwrap_or_default();
    let home = PathBuf::from(var("HOME").unwrap_or_default());
    Info { linux, deck: is_deck(var), steam: started_by_steam(var), game_mode: game_mode(var), can_add: linux && !exe.is_empty(), in_steam: linux && !exe.is_empty() && in_steam(&home, &exe), exe }
}

// ---------- Steam's folders ----------

pub fn steam_roots(home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in [home.join(".local/share/Steam"), home.join(".steam/steam"), home.join(".steam/root"), home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam")] {
        let Ok(r) = fs::canonicalize(&d) else { continue };
        if !out.contains(&r) && r.join("userdata").exists() {
            out.push(r);
        }
    }
    out
}

/// Every Steam account that has signed in on this PC: its config folder.
pub fn user_dirs(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in steam_roots(home) {
        for e in crate::scanner::read_dir_sorted(&root.join("userdata")) {
            let id = e.file_name().to_string_lossy().into_owned();
            let dir = e.path().join("config");
            if !id.is_empty() && id != "0" && id.bytes().all(|b| b.is_ascii_digit()) && dir.exists() {
                out.push(dir);
            }
        }
    }
    out
}

// ---------- binary KeyValues (shortcuts.vdf) ----------

/// One entry, in the file's order: every value is kept as it was, so writing it back changes nothing not meant to.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Map(Vec<Entry>),
    Str(String),
    Int(i32),
    Float(f32),
    U64(u64),
}

/// Types: 0 a map, 1 a string, 2 an int32, 3 a float32, 7 a uint64, 8 the end of a map.
pub fn parse(buf: &[u8]) -> Result<Vec<Entry>, String> {
    fn text(buf: &[u8], p: &mut usize) -> Result<String, String> {
        let end = buf[*p..].iter().position(|&b| b == 0).ok_or("bad shortcuts.vdf")? + *p;
        let s = String::from_utf8_lossy(&buf[*p..end]).into_owned();
        *p = end + 1;
        Ok(s)
    }
    fn take<const N: usize>(buf: &[u8], p: &mut usize) -> Result<[u8; N], String> {
        let b = buf.get(*p..*p + N).ok_or("bad shortcuts.vdf (ends early)")?.try_into().unwrap();
        *p += N;
        Ok(b)
    }
    fn map(buf: &[u8], p: &mut usize) -> Result<Vec<Entry>, String> {
        let mut out = Vec::new();
        loop {
            let t = *buf.get(*p).ok_or("bad shortcuts.vdf (ends early)")?;
            *p += 1;
            if t == 8 {
                return Ok(out);
            }
            let key = text(buf, p)?;
            let value = match t {
                0 => Value::Map(map(buf, p)?),
                1 => Value::Str(text(buf, p)?),
                2 => Value::Int(i32::from_le_bytes(take(buf, p)?)),
                3 => Value::Float(f32::from_le_bytes(take(buf, p)?)),
                7 => Value::U64(u64::from_le_bytes(take(buf, p)?)),
                t => return Err(format!("bad shortcuts.vdf (type {t})")),
            };
            out.push(Entry { key, value });
        }
    }
    map(buf, &mut 0)
}

pub fn write(list: &[Entry]) -> Vec<u8> {
    fn map(list: &[Entry], out: &mut Vec<u8>) {
        for e in list {
            let t = match e.value {
                Value::Map(_) => 0,
                Value::Str(_) => 1,
                Value::Int(_) => 2,
                Value::Float(_) => 3,
                Value::U64(_) => 7,
            };
            out.push(t);
            out.extend_from_slice(e.key.as_bytes());
            out.push(0);
            match &e.value {
                Value::Map(m) => {
                    map(m, out);
                    out.push(8);
                }
                Value::Str(s) => {
                    out.extend_from_slice(s.as_bytes());
                    out.push(0);
                }
                Value::Int(n) => out.extend_from_slice(&n.to_le_bytes()),
                Value::Float(n) => out.extend_from_slice(&n.to_le_bytes()),
                Value::U64(n) => out.extend_from_slice(&n.to_le_bytes()),
            }
        }
    }
    let mut out = Vec::new();
    map(list, &mut out);
    out.push(8);
    out
}

/// Steam's id for a non-Steam game: CRC32 of exe + name with the top bit set (its artwork in config/grid uses it too).
pub fn shortcut_id(exe: &str, name: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

fn get<'a>(list: &'a [Entry], key: &str) -> Option<&'a Value> {
    list.iter().find(|e| e.key.eq_ignore_ascii_case(key)).map(|e| &e.value)
}

fn get_str<'a>(list: &'a [Entry], key: &str) -> &'a str {
    match get(list, key) {
        Some(Value::Str(s)) => s,
        _ => "",
    }
}

static OURS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(rustyshelf|boxshelf)[^/]*\.appimage"?$"#).unwrap());

/// Puts RustyShelf's entry in (`exe`: the program Steam starts): the list, Steam's id for it, and whether it was there.
/// An entry that's ours is the same program, a RustyShelf AppImage of another version or place, or one by our name.
pub fn with_rustyshelf(mut root: Vec<Entry>, exe: &str, name: &str, icon: &str) -> (Vec<Entry>, u32, bool) {
    let q = |s: &str| format!("\"{s}\"");
    let (qexe, dir) = (q(exe), q(&crate::js::dirname(exe)));
    let id = shortcut_id(&qexe, name);
    let fields: [(&str, Value); 16] = [
        ("appid", Value::Int(id as i32)),
        ("AppName", Value::Str(name.into())),
        ("Exe", Value::Str(qexe.clone())),
        ("StartDir", Value::Str(dir)),
        ("icon", Value::Str(icon.into())),
        ("ShortcutPath", Value::Str(String::new())),
        ("LaunchOptions", Value::Str(String::new())),
        ("IsHidden", Value::Int(0)),
        ("AllowDesktopConfig", Value::Int(1)),
        ("AllowOverlay", Value::Int(1)),
        ("OpenVR", Value::Int(0)),
        ("Devkit", Value::Int(0)),
        ("DevkitGameID", Value::Str(String::new())),
        ("DevkitOverrideAppID", Value::Int(0)),
        ("LastPlayTime", Value::Int(0)),
        ("FlatpakAppID", Value::Str(String::new())),
    ];
    if !root.iter().any(|e| e.key.eq_ignore_ascii_case("shortcuts") && matches!(e.value, Value::Map(_))) {
        root.push(Entry { key: "shortcuts".into(), value: Value::Map(Vec::new()) });
    }
    let top = root.iter_mut().find(|e| e.key.eq_ignore_ascii_case("shortcuts") && matches!(e.value, Value::Map(_))).unwrap();
    let Value::Map(list) = &mut top.value else { unreachable!() };
    let mine = |v: &[Entry]| {
        let x = get_str(v, "Exe");
        let n = get_str(v, "AppName");
        x == qexe || OURS.is_match(x) || n == name || ["(T)rustyShelf", "TrustyShelf", "Boxshelf"].contains(&n)
    };
    let at = list.iter().position(|e| matches!(&e.value, Value::Map(v) if mine(v)));
    let had = at.is_some();
    let at = at.unwrap_or_else(|| {
        list.push(Entry { key: list.len().to_string(), value: Value::Map(Vec::new()) });
        list.len() - 1
    });
    let Value::Map(ent) = &mut list[at].value else { unreachable!() };
    for (k, v) in fields {
        match ent.iter_mut().find(|e| e.key.eq_ignore_ascii_case(k)) {
            Some(e) => e.value = v,
            None => ent.push(Entry { key: k.into(), value: v }),
        }
    }
    if !ent.iter().any(|e| e.key.eq_ignore_ascii_case("tags")) {
        ent.push(Entry { key: "tags".into(), value: Value::Map(Vec::new()) });
    }
    (root, id, had)
}

/// Whether any Steam account's list of non-Steam games has this program or a RustyShelf AppImage.
pub fn in_steam(home: &Path, exe: &str) -> bool {
    static APPIMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)rustyshelf[^/]*\.appimage"?$"#).unwrap());
    let qexe = format!("\"{exe}\"");
    user_dirs(home).iter().any(|d| {
        let Ok(list) = fs::read(d.join("shortcuts.vdf")).map_err(|_| ()).and_then(|b| parse(&b).map_err(|_| ())) else { return false };
        let Some(Value::Map(top)) = get(&list, "shortcuts") else { return false };
        top.iter().any(|e| matches!(&e.value, Value::Map(v) if get_str(v, "Exe") == qexe || APPIMAGE.is_match(get_str(v, "Exe"))))
    })
}

// ---------- Steam running? close it, start it again ----------

pub fn steam_running() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Ok(rd) = fs::read_dir("/proc") else { return false };
    rd.flatten().filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit())).any(|e| matches!(read(&e.path().join("comm").to_string_lossy()).as_str(), "steam" | "steamwebhelper"))
}

fn steam_cmd() -> &'static str {
    ["/usr/bin/steam", "/usr/games/steam"].into_iter().find(|c| Path::new(c).exists()).unwrap_or("steam")
}

fn close_steam() -> bool {
    if let Ok(mut c) = std::process::Command::new(steam_cmd()).arg("-shutdown").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        let until = Instant::now() + Duration::from_secs(15);
        while matches!(c.try_wait(), Ok(None)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    for _ in 0..60 {
        if !steam_running() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    !steam_running()
}

fn start_steam() {
    let mut cmd = std::process::Command::new(steam_cmd());
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let _ = cmd.spawn();
}

// ---------- Add RustyShelf to Steam ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NotLinux,
    NoProgram,
    /// already in Steam, and Steam can't be closed from under its own session
    GameMode,
    NoSteam,
    SteamRunning,
    SteamWouldNotClose,
    WriteFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub users: usize,
    pub updated: bool,
    pub reopened: bool,
    /// for the log: what was done for each Steam account, by its place in the list, never its number
    pub notes: Vec<String>,
}

/// `exe`: the program Steam starts; `icon`: RustyShelf's icon, copied into `data_dir/steam`; `close`: Steam is closed
/// first and started again after.
pub fn add_to_steam(var: Var, home: &Path, exe: &str, icon: Option<&[u8]>, data_dir: &Path, close: bool) -> Result<Added, Refusal> {
    if !cfg!(target_os = "linux") && var("BS_FAKE_DECK").as_deref() != Some("1") {
        return Err(Refusal::NotLinux);
    }
    if exe.is_empty() {
        return Err(Refusal::NoProgram);
    }
    if game_mode(var) {
        return Err(Refusal::GameMode);
    }
    let users = user_dirs(home);
    if users.is_empty() {
        return Err(Refusal::NoSteam);
    }
    let mut reopened = false;
    if steam_running() {
        if !close {
            return Err(Refusal::SteamRunning);
        }
        if !close_steam() {
            return Err(Refusal::SteamWouldNotClose);
        }
        reopened = true;
    }
    let icon_file = icon
        .and_then(|b| {
            let f = data_dir.join("steam").join("rustyshelf-icon.png");
            fs::create_dir_all(f.parent()?).ok()?;
            fs::write(&f, b).ok()?;
            Some(f.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let (mut done, mut updated, mut notes) = (0, false, Vec::new());
    for (i, dir) in users.iter().enumerate() {
        let f = dir.join("shortcuts.vdf");
        let result = (|| -> Result<bool, String> {
            let mut root = Vec::new();
            if f.exists() {
                let buf = fs::read(&f).map_err(|e| e.kind().to_string())?;
                if !buf.is_empty() {
                    root = parse(&buf)?;
                }
                fs::copy(&f, f.with_extension("vdf.rustyshelf-bak")).map_err(|e| e.kind().to_string())?;
            }
            let (root, id, had) = with_rustyshelf(root, exe, "RustyShelf", &icon_file);
            let tmp = f.with_extension("vdf.part");
            fs::write(&tmp, write(&root)).and_then(|()| fs::rename(&tmp, &f)).map_err(|e| e.kind().to_string())?;
            // the Library's artwork: RustyShelf's icon, until a proper one is chosen in Steam
            if !icon_file.is_empty() {
                let grid = dir.join("grid");
                let _ = fs::create_dir_all(&grid).and_then(|()| fs::copy(&icon_file, grid.join(format!("{id}_icon.png"))));
            }
            Ok(had)
        })();
        // the account number stays out of the log, which goes into problem reports
        let who = format!("{} of {}", i + 1, users.len());
        notes.push(match result {
            Ok(had) => {
                done += 1;
                updated |= had;
                format!("Steam: RustyShelf {} the shortcuts of Steam user {who}", if had { "updated in" } else { "added to" })
            }
            Err(e) => format!("Steam: shortcuts for user {who} not changed: {}", e.split(':').next().unwrap_or("error")),
        });
    }
    if reopened {
        start_steam();
    }
    if done == 0 {
        return Err(Refusal::WriteFailed);
    }
    Ok(Added { users: done, updated, reopened, notes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(k: &str, v: &str) -> Entry {
        Entry { key: k.into(), value: Value::Str(v.into()) }
    }

    #[test]
    fn crc() {
        assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
        assert!(shortcut_id("\"/a\"", "B") >= 0x8000_0000);
    }

    #[test]
    fn add_to_steam_twice() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let cfg = home.join(".local/share/Steam/userdata/12345/config");
        fs::create_dir_all(&cfg).unwrap();
        let other = vec![Entry {
            key: "shortcuts".into(),
            value: Value::Map(vec![Entry {
                key: "0".into(),
                value: Value::Map(vec![
                    Entry { key: "appid".into(), value: Value::Int(-123) },
                    s("AppName", "Some Game"),
                    s("Exe", "\"/x/game\""),
                    Entry { key: "LastPlayTime".into(), value: Value::Int(5) },
                    Entry { key: "tags".into(), value: Value::Map(vec![s("0", "favorite")]) },
                ]),
            }]),
        }];
        fs::write(cfg.join("shortcuts.vdf"), write(&other)).unwrap();
        assert_eq!(parse(&write(&other)).unwrap(), other);
        let img = home.join("Applications/RustyShelf-linux.AppImage").to_string_lossy().into_owned();
        let fake = |k: &str| (k == "BS_FAKE_DECK").then(|| "1".to_string());
        let data = home.join("data");
        let r1 = add_to_steam(&fake, home, &img, Some(b"png"), &data, false);
        let r2 = add_to_steam(&fake, home, &img, Some(b"png"), &data, false);
        assert!(r1.as_ref().is_ok_and(|r| !r.updated), "{r1:?}");
        assert!(r2.as_ref().is_ok_and(|r| r.updated), "{r2:?}");
        let root = parse(&fs::read(cfg.join("shortcuts.vdf")).unwrap()).unwrap();
        let Some(Value::Map(list)) = get(&root, "shortcuts") else { panic!() };
        assert_eq!(list.len(), 2);
        let Value::Map(game) = &list[0].value else { panic!() };
        assert_eq!(game, match &other[0].value {
            Value::Map(l) => match &l[0].value {
                Value::Map(g) => g,
                _ => panic!(),
            },
            _ => panic!(),
        });
        let Value::Map(ours) = &list[1].value else { panic!() };
        assert_eq!(get_str(ours, "AppName"), "RustyShelf");
        assert_eq!(get_str(ours, "Exe"), format!("\"{img}\""));
        assert_eq!(get(ours, "appid"), Some(&Value::Int(shortcut_id(&format!("\"{img}\""), "RustyShelf") as i32)));
        assert!(cfg.join("shortcuts.vdf.rustyshelf-bak").exists());
        assert!(in_steam(home, &img));
        assert!(!in_steam(&home.join("nobody"), &img));
        assert!(cfg.join("grid").join(format!("{}_icon.png", shortcut_id(&format!("\"{img}\""), "RustyShelf"))).exists());
    }

    #[test]
    fn where_it_runs() {
        let env = |pairs: &'static [(&'static str, &'static str)]| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string());
        assert!(!started_by_steam(&env(&[])));
        assert!(started_by_steam(&env(&[("SteamClientLaunch", "1")])));
        assert!(game_mode(&env(&[("XDG_CURRENT_DESKTOP", "gamescope")])));
        assert!(!game_mode(&env(&[("XDG_CURRENT_DESKTOP", "KDE")])));
        let gm = env(&[("SteamGamepadUI", "1"), ("BS_FAKE_DECK", "1")]);
        assert_eq!(add_to_steam(&gm, Path::new("/nowhere"), "/x", None, Path::new("/tmp"), false), Err(Refusal::GameMode));
    }
}

// Based on ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf by
// sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source): ported from its main/moonlight.js, the moonlight
// rules of its main/config.js and its linux/ci/moonlight-test.js.
//! Game streaming: games from another PC, played through Moonlight (moonlight-qt) from a Sunshine or GeForce
//! Experience host. RustyShelf never streams anything itself: it asks Moonlight's command line for the host's apps
//! (`moonlight list <host> --csv`), shows each as a case on the Game streaming shelf, and starts one with
//! `moonlight stream <host> "<app>"` like any other program, so the quit combo and the Home menu work the same.
//! The host must already be paired in Moonlight's own window; a MAC address wakes it first (Wake-on-LAN).

use std::io::Read;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Value};

use crate::js;
use crate::paths::is_file;
use crate::pcgames::PcGame;

pub const FLATPAK_ID: &str = "com.moonlight_stream.Moonlight";

/// One of the host's apps, as Moonlight lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct App {
    pub name: String,
    pub id: i64,
    pub hidden: bool,
    /// Moonlight's cached box art, a file:// address
    pub boxart: String,
}

impl App {
    pub fn to_value(&self) -> Value {
        json!({ "name": self.name, "id": self.id, "hidden": self.hidden, "boxart": self.boxart })
    }
}

/// Settings → Game streaming (the `moonlight` section).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Setup {
    pub enabled: bool,
    /// Moonlight's program
    pub path: String,
    /// the paired PC's name or IP address
    pub host: String,
    /// its network card, to wake it
    pub mac: String,
    /// "WxH", empty for Moonlight's own setting; 0 is Moonlight's own too
    pub resolution: String,
    pub fps: u32,
    pub bitrate: u32,
    pub quit_after: bool,
    /// the host's list from the last time it was asked
    pub apps: Vec<App>,
}

impl Setup {
    /// The section of a settings file that went through `sanitize`; the defaults when there's none.
    pub fn of(v: Option<&Value>) -> Setup {
        let get = |k: &str| v.and_then(|m| m.get(k));
        let text = |k: &str| js::str_of(get(k)).unwrap_or("").to_string();
        let count = |k: &str| get(k).and_then(Value::as_u64).map_or(0, |n| n as u32);
        Setup {
            enabled: get("enabled") == Some(&Value::Bool(true)),
            path: text("path"),
            host: text("host"),
            mac: text("mac"),
            resolution: text("resolution"),
            fps: count("fps"),
            bitrate: count("bitrate"),
            quit_after: get("quitAfter") == Some(&Value::Bool(true)),
            apps: get("apps").and_then(Value::as_array).into_iter().flatten().map(|a| App {
                name: js::str_of(a.get("name")).unwrap_or("").to_string(),
                id: a.get("id").and_then(Value::as_i64).unwrap_or(0),
                hidden: a.get("hidden") == Some(&Value::Bool(true)),
                boxart: js::str_of(a.get("boxart")).unwrap_or("").to_string(),
            })
            .collect(),
        }
    }
}

/// The apps as the settings keep them, by the same rules as the rest of the section.
pub fn apps_value(apps: &[App]) -> Value {
    sanitize(Some(&json!({ "apps": apps.iter().map(App::to_value).collect::<Vec<_>>() })))["apps"].clone()
}

/// The section brought back to plain strings and small numbers, the app list as Moonlight gave it, capped.
pub fn sanitize(m: Option<&Value>) -> Value {
    static MAC: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^([0-9a-f]{2}[:-]?){5}[0-9a-f]{2}$").unwrap());
    static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]{3,5}x[0-9]{3,5}$").unwrap());
    let m = m.filter(|v| v.is_object());
    let get = |k: &str| m.and_then(|m| m.get(k));
    let s = |v: Option<&Value>, n: usize| js::str_of(v).map(|t| js::slice(js::trim(t), n).to_string()).unwrap_or_default();
    let n = |v: Option<&Value>, lo: f64, hi: f64| {
        let x = js::number(v);
        js::num(if x.is_finite() && x >= lo && x <= hi { js::round(x) } else { 0.0 })
    };
    let (mac, size) = (s(get("mac"), 17), s(get("resolution"), 11));
    let apps: Vec<Value> = get("apps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|a| a.is_object() && !s(a.get("name"), 200).is_empty())
        .take(500)
        .map(|a| json!({ "name": s(a.get("name"), 200), "id": n(a.get("id"), 0.0, 2_147_483_647.0), "hidden": a.get("hidden") == Some(&Value::Bool(true)), "boxart": s(a.get("boxart"), 2048) }))
        .collect();
    json!({
        "enabled": get("enabled") == Some(&Value::Bool(true)),
        "path": s(get("path"), 1024),
        "host": s(get("host"), 255).replace('"', ""),
        "mac": if MAC.is_match(&mac) { mac } else { String::new() },
        "resolution": if SIZE.is_match(&size) { size } else { String::new() },
        "fps": n(get("fps"), 0.0, 240.0),
        "bitrate": n(get("bitrate"), 0.0, 500_000.0),
        "quitAfter": get("quitAfter") == Some(&Value::Bool(true)),
        "apps": apps,
    })
}

// ---------- finding Moonlight ----------

/// Where Moonlight usually is: its installer's folder (Windows); a package, the Flatpak's exported launcher or an
/// AppImage in ~/Applications (Linux); the app bundle (macOS).
pub fn find() -> Option<PathBuf> {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    find_on(os, &var, Path::new(&var("HOME").unwrap_or_default()))
}

/// `find` for `os` ("windows", "macos" or "linux"), with the environment `var` and the home folder `home`.
pub fn find_on(os: &str, var: &dyn Fn(&str) -> Option<String>, home: &Path) -> Option<PathBuf> {
    let path = var("PATH").unwrap_or_default();
    match os {
        "windows" => {
            let roots = [var("ProgramFiles").map(PathBuf::from), var("ProgramFiles(x86)").map(PathBuf::from), var("LOCALAPPDATA").map(|d| Path::new(&d).join("Programs"))];
            roots.into_iter().flatten().map(|r| r.join("Moonlight Game Streaming").join("Moonlight.exe")).find(|f| is_file(f)).or_else(|| {
                path.split(';').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("Moonlight.exe")).find(|f| is_file(f))
            })
        }
        "macos" => Some(PathBuf::from("/Applications/Moonlight.app/Contents/MacOS/Moonlight")).filter(|f| is_file(f)),
        _ => {
            static APPIMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^moonlight.*\.appimage$").unwrap());
            let package = ["moonlight", "moonlight-qt"].into_iter().find_map(|n| crate::linuxpc::which_in(n, &path));
            let flatpak = || [home.join(".local/share/flatpak/exports/bin"), PathBuf::from("/var/lib/flatpak/exports/bin")].into_iter().map(|d| d.join(FLATPAK_ID)).find(|f| is_file(f));
            let appimage = || {
                [home.join("Applications"), home.join(".local/bin"), home.join("Downloads")].into_iter().find_map(|d| {
                    let mut names: Vec<String> = std::fs::read_dir(&d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                    names.sort();
                    names.into_iter().find(|n| APPIMAGE.is_match(n)).map(|n| d.join(n)).filter(|f| is_file(f))
                })
            };
            package.or_else(flatpak).or_else(appimage)
        }
    }
}

// ---------- the host's apps ----------

/// `moonlight list <host> --csv`: a header line, then `"Name",ID,hdr,collector,hidden,directLaunch,"Boxart URL"`.
/// Moonlight doesn't escape quotes inside a name, so each line is read from both ends. A name comes once.
pub fn parse_csv(text: &str) -> Vec<App> {
    static ROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^"(.*)",(-?[0-9]+),(true|false),(true|false),(true|false),(true|false),"(.*)"\s*$"#).unwrap());
    let (mut out, mut seen) = (Vec::new(), std::collections::HashSet::new());
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let Some(m) = ROW.captures(js::trim(line)) else { continue };
        let name = js::trim(&m[1]).to_string();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        out.push(App { name, id: m[2].parse().unwrap_or(0), hidden: &m[5] == "true", boxart: m[7].to_string() });
    }
    out
}

/// Moonlight's cached box art: a file:// address as a path, when the file is there.
pub fn boxart_file(url: &str) -> Option<PathBuf> {
    if !url.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        return None;
    }
    url::Url::parse(url).ok()?.to_file_path().ok().filter(|f| is_file(f))
}

/// The host's apps, asked through Moonlight's program (20 s at most; on Linux with the environment a game gets).
pub fn list(exe: &str, host: &str) -> Result<Vec<App>, String> {
    list_with(exe, host, Duration::from_secs(20), cfg!(target_os = "linux").then(|| crate::launch::clean_env(None)))
}

pub fn list_with(exe: &str, host: &str, timeout: Duration, env: Option<Vec<(String, String)>>) -> Result<Vec<App>, String> {
    if exe.is_empty() || !is_file(Path::new(exe)) {
        return Err("Can't find Moonlight. Install it, or choose Moonlight.exe / moonlight in Settings → Game streaming.".into());
    }
    if host.is_empty() {
        return Err("Choose your host PC (its name or IP address) in Settings → Game streaming.".into());
    }
    let mut cmd = Command::new(exe);
    cmd.args(["list", host, "--csv"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(env) = env {
        cmd.env_clear().envs(env);
    }
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    let (out, err, failed) = match cmd.spawn() {
        Err(e) => (String::new(), String::new(), Some(format!("spawn {exe} {e}"))),
        Ok(mut child) => {
            let read = |r: Option<Box<dyn Read + Send>>| {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let mut b = Vec::new();
                    if let Some(r) = r {
                        let mut r = r.take(4 << 20);
                        let _ = r.read_to_end(&mut b);
                        let _ = std::io::copy(&mut r.into_inner(), &mut std::io::sink());
                    }
                    let _ = tx.send(String::from_utf8_lossy(&b).into_owned());
                });
                rx
            };
            let out = read(child.stdout.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
            let err = read(child.stderr.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
            let until = Instant::now() + timeout;
            let failed = loop {
                match child.try_wait() {
                    Ok(Some(st)) if st.success() => break None,
                    Ok(Some(_)) => break Some(format!("Command failed: {exe} list {host} --csv")),
                    Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Some("no answer from the host".into());
                    }
                }
            };
            // (what it started can keep the pipes open after it's killed)
            let got = |rx: std::sync::mpsc::Receiver<String>| rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
            (got(out), got(err), failed)
        }
    };
    let apps = parse_csv(&out);
    // a full list still counts after an exit that isn't 0
    if !apps.is_empty() {
        return Ok(apps);
    }
    let why = js::trim(&err).split('\n').next_back().map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !l.is_empty()).map(String::from).or(failed).unwrap_or_else(|| "no apps".into());
    if why.to_lowercase().contains("not been paired") {
        Err(format!("{host} isn't paired yet: open Moonlight once, add it and pair it, then refresh."))
    } else {
        Err(format!("Couldn't get the games from {host}: {why}"))
    }
}

// ---------- shelf entries ----------

pub fn slug(s: &str) -> String {
    crate::pcgames::slug_or(s, "app")
}

/// The arguments after Moonlight's program: a stream of one app, fullscreen, with the settings chosen.
pub fn stream_args(c: &Setup, app: &str) -> String {
    static SIZE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[0-9]{3,5}x[0-9]{3,5}$").unwrap());
    let q = |s: &str| format!("\"{}\"", s.replace('"', ""));
    let mut a = vec!["stream".to_string(), q(&c.host), q(app), "--display-mode".into(), "fullscreen".into()];
    if SIZE.is_match(&c.resolution) {
        a.extend(["--resolution".into(), c.resolution.clone()]);
    }
    if c.fps > 0 {
        a.extend(["--fps".into(), c.fps.to_string()]);
    }
    if c.bitrate > 0 {
        a.extend(["--bitrate".into(), c.bitrate.to_string()]);
    }
    if c.quit_after {
        a.push("--quit-after".into());
    }
    a.join(" ")
}

/// The host's apps from the last refresh as the shelf's games, each with Moonlight's box art when it's there.
/// Sunshine's own entries that aren't games stay off (the desktop itself; Steam Big Picture is kept), and so do
/// hidden apps and names with a quote in them.
pub fn games(c: &Setup) -> Vec<(PcGame, Option<PathBuf>)> {
    static NOT_GAMES: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^(desktop|remote desktop|terminate|virtual desktop)$").unwrap());
    if !c.enabled || c.host.is_empty() || c.path.is_empty() || !is_file(Path::new(&c.path)) {
        return Vec::new();
    }
    c.apps
        .iter()
        .filter(|a| !a.name.is_empty() && !a.hidden && !a.name.contains('"') && !NOT_GAMES.is_match(js::trim(&a.name)))
        .take(500)
        .map(|a| {
            let g = PcGame { id: format!("moonlight-{}", slug(&a.name)), source: "moonlight", title: js::slice(js::trim(&a.name), 120).to_string(), exe: c.path.clone(), args: stream_args(c, &a.name), ..PcGame::default() };
            (g, boxart_file(&a.boxart))
        })
        .collect()
}

// ---------- Wake-on-LAN ----------

/// "AA:BB:CC:DD:EE:FF", "AA-BB-…" or "AABBCCDDEEFF" as its 6 bytes.
pub fn mac_bytes(mac: &str) -> Option<[u8; 6]> {
    let h: Vec<u8> = mac.chars().filter_map(|c| c.to_digit(16)).map(|d| d as u8).collect();
    if h.len() != 12 {
        return None;
    }
    let mut out = [0u8; 6];
    for (b, p) in out.iter_mut().zip(h.chunks(2)) {
        *b = (p[0] << 4) | p[1];
    }
    Some(out)
}

/// 6 × 0xFF, then the MAC 16 times.
pub fn magic_packet(mac: &str) -> Option<Vec<u8>> {
    let m = mac_bytes(mac)?;
    Some([0xff; 6].into_iter().chain((0..16).flat_map(|_| m)).collect())
}

/// Wakes the host: the packet broadcast a few times, as a sleeping network card can miss one. False for a MAC
/// that isn't one, or when it couldn't be sent.
pub fn wake(mac: &str) -> bool {
    wake_to(mac, "255.255.255.255", 9, 3).is_ok()
}

pub fn wake_to(mac: &str, address: &str, port: u16, times: u32) -> Result<(), String> {
    let p = magic_packet(mac).ok_or("not a MAC address")?;
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    let _ = s.set_broadcast(true);
    for i in 0..times {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
        s.send_to(&p, (address, port)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_read_from_both_ends() {
        let csv = "Name, ID, HDR Support, App Collection Game, Hidden, Direct Launch, Boxart URL\r\n\"A \"quoted\" one\",-3,false,false,false,false,\"\"\r\n\"Same\",1,false,false,false,false,\"x\"\n\"same\",2,false,false,true,false,\"\"\n\"  \",3,false,false,false,false,\"\"";
        let apps = parse_csv(csv);
        assert_eq!(apps.iter().map(|a| (a.name.as_str(), a.id)).collect::<Vec<_>>(), [("A \"quoted\" one", -3), ("Same", 1)]);
        assert_eq!(apps[1].boxart, "x");
    }

    #[test]
    fn a_slug_never_comes_out_empty() {
        assert_eq!((slug("Elden Ring, Deluxe"), slug("!!!")), ("elden-ring-deluxe".to_string(), "app".to_string()));
    }

    #[test]
    fn stream_arguments_only_carry_what_is_chosen() {
        let c = Setup { host: "my\"pc".into(), ..Setup::default() };
        assert_eq!(stream_args(&c, "A \"B\""), r#"stream "mypc" "A B" --display-mode fullscreen"#);
        let c = Setup { host: "pc".into(), resolution: "4k".into(), bitrate: 20000, ..Setup::default() };
        assert_eq!(stream_args(&c, "G"), r#"stream "pc" "G" --display-mode fullscreen --bitrate 20000"#);
    }

    #[test]
    fn settings_keep_a_capped_app_list() {
        let apps: Vec<Value> = (0..600).map(|i| json!({ "name": format!(" Game {i} "), "id": i, "hidden": i == 1, "boxart": 5 })).chain([json!({ "name": "" }), json!(7)]).collect();
        let m = sanitize(Some(&json!({ "enabled": true, "mac": "AABBCCDDEEFF", "resolution": "1280x720", "fps": "59.6", "bitrate": -1, "apps": apps })));
        assert_eq!((m["enabled"].clone(), m["mac"].clone(), m["resolution"].clone(), m["fps"].clone(), m["bitrate"].clone()), (json!(true), json!("AABBCCDDEEFF"), json!("1280x720"), json!(60), json!(0)));
        let list = m["apps"].as_array().unwrap();
        assert_eq!((list.len(), &list[1]), (500, &json!({ "name": "Game 1", "id": 1, "hidden": true, "boxart": "" })));
        assert_eq!(sanitize(Some(&json!(5))), sanitize(None));
        assert_eq!(Setup::of(Some(&m)).apps[1], App { name: "Game 1".into(), id: 1, hidden: true, boxart: String::new() });
        assert_eq!(Setup::of(None), Setup::default());
    }

    #[test]
    fn the_section_is_kept_only_when_it_is_there() {
        let loaded = |text: &str| crate::settings::read(text).unwrap();
        assert!(loaded("{}").get("moonlight").is_none());
        let m = &loaded(r#"{"moonlight":{"host":" pc ","fps":60}}"#)["moonlight"];
        assert_eq!((m["host"].clone(), m["fps"].clone(), m["apps"].clone(), m["enabled"].clone()), (json!("pc"), json!(60), json!([]), json!(false)));
        let cfg = crate::config::Config::parse(r#"{"moonlight":{"enabled":true,"host":"pc","quitAfter":true}}"#).unwrap();
        assert_eq!((cfg.moonlight().enabled, cfg.moonlight().host, cfg.moonlight().quit_after), (true, "pc".to_string(), true));
        assert_eq!(crate::config::Config::default().moonlight(), Setup::default());
    }

    #[test]
    fn a_found_moonlight_on_the_path_or_in_applications() {
        let t = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        assert_eq!(find_on("linux", &none, t.path()), None);
        let img = t.path().join("Applications/Moonlight-6.1.0-x86_64.AppImage");
        std::fs::create_dir_all(img.parent().unwrap()).unwrap();
        std::fs::write(&img, "").unwrap();
        assert_eq!(find_on("linux", &none, t.path()), Some(img));
        let exe = t.path().join("Prog/Moonlight Game Streaming/Moonlight.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "").unwrap();
        let prog = t.path().join("Prog").to_string_lossy().into_owned();
        assert_eq!(find_on("windows", &|k: &str| (k == "ProgramFiles(x86)").then(|| prog.clone()), t.path()), Some(exe));
    }

    #[test]
    fn a_bad_mac_makes_no_packet() {
        assert_eq!(mac_bytes("aa-bb-cc-dd-ee-0f"), Some([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0x0f]));
        assert_eq!((magic_packet("12:34"), magic_packet("AA:BB:CC:DD:EE:FF:00")), (None, None));
        assert!(!wake("zz"));
    }

    /// linux/ci/moonlight-test.js: a pretend Moonlight Flatpak in a made-up home folder answers `list --csv`.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_pretend_moonlight_end_to_end() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let mk = |p: &str, body: &str, exec: bool| {
            let f = home.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, body).unwrap();
            if exec {
                std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            f
        };
        let art = mk(".cache/Moonlight/boxart/1234.png", "png", false);
        let csv = [
            "Name, ID, HDR Support, App Collection Game, Hidden, Direct Launch, Boxart URL".to_string(),
            r#""Desktop",1,false,false,false,false,"""#.into(),
            format!(r#""Elden Ring, Deluxe",2,true,false,false,false,"file://{}""#, art.display()),
            r#""Hidden Game",3,false,false,true,false,"""#.into(),
            r#""Yuzu ""Test""",4,false,false,false,false,"""#.into(),
            r#""Steam Big Picture",5,false,false,false,false,"file:///nope.png""#.into(),
        ]
        .join("\n");
        let flatpak = mk(
            &format!(".local/share/flatpak/exports/bin/{FLATPAK_ID}"),
            &format!("#!/bin/sh\nif [ \"$1\" = list ] && [ \"$2\" = mypc ]; then cat <<'EOF'\n{csv}\nEOF\nexit 0; fi\nif [ \"$1\" = list ]; then echo \"Computer x has not been paired. Please open Moonlight to pair\" >&2; exit 255; fi\nexit 0\n"),
            true,
        );
        let no_path = |k: &str| (k == "PATH").then(|| "/nonexistent".to_string());
        assert_eq!(find_on("linux", &no_path, home), Some(flatpak.clone()), "finds the Flatpak launcher");
        assert_eq!(find_on("linux", &no_path, &home.join("none")), None, "nothing found in an empty home");

        let apps = parse_csv(&csv);
        assert_eq!(apps.len(), 5, "reads every app row");
        assert_eq!(apps[1].name, "Elden Ring, Deluxe", "a comma inside a name");
        assert!(apps[2].hidden, "hidden flag");

        let exe = flatpak.to_string_lossy().into_owned();
        let listed = list(&exe, "mypc").unwrap();
        assert_eq!(listed.len(), 5, "list through the program");
        let msg = list(&exe, "other").unwrap_err();
        assert!(msg.contains("isn't paired yet"), "an unpaired host says so: {msg}");
        let msg = list(&home.join("nope").to_string_lossy(), "mypc").unwrap_err();
        assert!(msg.contains("Can't find Moonlight"), "a missing Moonlight says so: {msg}");

        let apps: Vec<Value> = listed.iter().map(App::to_value).collect();
        let cfg = crate::settings::normalize(&json!({ "moonlight": { "enabled": true, "path": exe, "host": "mypc", "mac": "aa-bb-cc-dd-ee-ff", "fps": 60, "resolution": "1920x1080", "quitAfter": true, "apps": apps } })).unwrap();
        let c = Setup::of(cfg.get("moonlight"));
        let shelf = games(&c);
        assert_eq!(shelf.iter().map(|g| g.0.title.as_str()).collect::<Vec<_>>().join("|"), "Elden Ring, Deluxe|Steam Big Picture", "Desktop, hidden and quoted names left out");
        assert_eq!((shelf[0].1.as_deref(), shelf[1].1.as_deref()), (Some(art.as_path()), None), "local box art used, a missing one dropped");
        assert_eq!(shelf[0].0.id, "moonlight-elden-ring-deluxe", "ids are stable slugs");
        assert_eq!(crate::launch::build_args(&shelf[0].0.args, ""), ["stream", "mypc", "Elden Ring, Deluxe", "--display-mode", "fullscreen", "--resolution", "1920x1080", "--fps", "60", "--quit-after"]);
        assert!(games(&Setup { enabled: false, ..c }).is_empty(), "off: no games");

        let bad = crate::settings::normalize(&json!({ "moonlight": { "enabled": "yes", "host": "my \"pc\"", "mac": "zz", "fps": 9999, "resolution": "4k", "apps": "x" } })).unwrap()["moonlight"].clone();
        assert_eq!((bad["enabled"].clone(), bad["host"].clone(), bad["mac"].clone(), bad["fps"].clone(), bad["resolution"].clone(), bad["apps"].clone()), (json!(false), json!("my pc"), json!(""), json!(0), json!(""), json!([])), "settings clean-up");

        let p = magic_packet("AA:BB:CC:DD:EE:FF").unwrap();
        assert!(p.len() == 102 && p[..6].iter().all(|b| *b == 0xff) && p[96..] == [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff], "magic packet: 6 x FF then 16 x the MAC");
        assert_eq!(magic_packet("12:34"), None, "a bad MAC makes no packet");
        let got = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = got.local_addr().unwrap().port();
        wake_to("AA:BB:CC:DD:EE:FF", "127.0.0.1", port, 1).unwrap();
        got.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut b = [0u8; 200];
        assert_eq!(got.recv(&mut b).unwrap(), 102, "wake: sent to the loopback");
    }
}

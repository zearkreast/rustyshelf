// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/config.js, as RustyShelf has it.
//! The settings file's rules: today's defaults, a saved file merged over them, older versions' values moved to
//! today's, and every value brought back to something Settings can show. The Electron app applies the same rules to
//! the same `data/config.json` (main/config.js, keep in sync).

use std::collections::HashSet;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::js;
use crate::systems::systems;

fn is_obj(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::Object(_)))
}

fn len16(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Runs of white space to one space, the ends off, the first `n` characters.
fn tidy(s: &str, n: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars() {
        let space = (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}';
        if space && !gap {
            out.push(' ');
        } else if !space {
            out.push(c);
        }
        gap = space;
    }
    js::slice(js::trim(&out), n).to_string()
}

/// A string's ends off and its first `n` characters; "" for anything that isn't a string.
fn s(v: Option<&Value>, n: usize) -> String {
    js::str_of(v).map(|t| js::slice(js::trim(t), n).to_string()).unwrap_or_default()
}

/// The strings of a list that aren't empty or longer than `max`, each once, the first `n` of them.
fn strings(v: Option<&Value>, max: usize, n: usize) -> Vec<Value> {
    let mut seen = HashSet::new();
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|x| !x.is_empty() && len16(x) <= max && seen.insert(x.to_string()))
        .take(n)
        .map(Value::from)
        .collect()
}

/// Today's settings for a new install.
pub fn defaults() -> Value {
    let mut sys = Map::new();
    for c in &systems().consoles {
        sys.insert(
            c.id.clone(),
            json!({
                "enabled": true, "emulator": "", "gameArgs": c.game_args, "emulatorArgs": c.emulator_args, "coverSeconds": js::num(c.cover_seconds),
                "romDirs": [], "artDir": "", "bootOnLaunch": true, "useIpl": false, "iplCoverSeconds": 2.5, "hideDolphinWarnings": false, "insertAnim": true
            }),
        );
    }
    let apps: Map<String, Value> = systems().apps.iter().map(|a| (a.id.clone(), json!({ "enabled": true, "path": "" }))).collect();
    let mut d: Value = serde_json::from_str(DEFAULTS).expect("the default settings");
    d["systems"] = Value::Object(sys);
    d["apps"] = Value::Object(apps);
    d
}

/// main/config.js defaults(), keep in sync; `systems` and `apps` are filled in from the consoles and the apps.
const DEFAULTS: &str = r#"{
  "setupDone": false,
  "retroarch": "",
  "systems": null,
  "steam": { "enabled": true, "path": "", "hideTools": true },
  "pc": { "epic": true, "quiver": true, "quiverPath": "", "custom": [], "gog": true, "ubisoft": true, "ea": true, "xbox": true, "hoyoplay": true, "consoleBox": false,
    "gamesDir": "", "userGamesDir": "", "programs": {},
    "runner": "", "prefixDir": "", "homeDir": "", "legendaryPath": "", "epicDir": "",
    "run": {} },
  "games": {},
  "emuSeen": [],
  "emuSetup": { "auto": true, "graphics": "auto", "resume": false, "pending": [] },
  "apps": null,
  "ra": { "user": "", "key": "", "enabled": true },
  "rocket": { "exe": "", "mode": "off", "frontend": "" },
  "romm": { "server": "", "skipTls": false, "cloud": true },
  "keys": { "fanart": "", "steamgriddb": "", "ssUser": "", "ssPass": "", "ssDevId": "", "ssDevPass": "", "rommToken": "", "rommTokenServer": "", "steamweb": "" },
  "art": { "styles": {}, "locks": {} },
  "ui": { "sound": true, "startFullscreen": true, "buttons": "auto", "performance": "auto", "quitCombo": true, "quitComboKeys": "l3+r3", "soundTheme": "aero", "volume": 0.7,
    "language": "en", "discordPresence": false, "discordAppId": "", "ambient": false, "ambientLevel": 0.02, "music": "off", "musicLevel": 0.4, "rumble": true, "soundRev": 4,
    "autoCovers": false, "autoManuals": false, "artLibrary": "", "attract": true, "attractMinutes": 3, "attractPace": "normal", "attractWander": true, "shuffleAll": true, "favorites": [], "collections": [],
    "background": "blur", "theme": "spectrum", "sort": "az", "filter": "all", "museumLight": "gallery", "homeMenu": true, "homeLayout": "side", "showSize": true,
    "hideTaskbar": true, "uiScale": 1, "previews": "off", "previewSound": false, "previewYoutube": true, "previewClips": true, "previewDirs": [], "previewDelay": 1.5,
    "previewHint": false, "clipHint": false, "deckHint": false, "lastSeenVersion": "", "autoHideHints": false, "allShelf": false, "customSort": {}, "boxScale": 1,
    "shelfScale": {}, "gameScale": {},
    "updateCheck": true,
    "autoUpdate": false,
    "openInto": "last", "display": "", "safeArea": 100, "clock": "full", "clock24": false, "reduceMotion": false, "dimMinutes": 0, "confirmButton": "auto",
    "scrollSpeed": "normal", "artRegion": "auto", "audioOut": "", "hidden": [],
    "museumStyle": { "walkIn": true, "grouping": "manufacturer" }, "arcadeCap": 24 }
}"#;

/// Saved values over the defaults, so new options appear after an update. Where the default is a section or a list,
/// a saved value of another shape is dropped for the default: the app reads `ui.x` everywhere.
fn merge(base: &Value, saved: &Value) -> Value {
    let Value::Object(saved) = saved else { return saved.clone() };
    let mut out = match base {
        Value::Object(b) => b.clone(),
        _ => Map::new(),
    };
    for (k, sv) in js::entries(saved) {
        let v = match base.get(k) {
            Some(b @ Value::Object(_)) => {
                if sv.is_object() {
                    merge(b, sv)
                } else {
                    b.clone()
                }
            }
            Some(b @ Value::Array(_)) if !sv.is_array() => b.clone(),
            _ => sv.clone(),
        };
        out.insert(k.clone(), v);
    }
    Value::Object(out)
}

/// Older default launch options that move to today's by themselves (main/config.js OLD_DEFAULTS).
const OLD_DEFAULTS: &[(&str, &[&str])] = &[("gc", &["-b -e \"{rom}\""]), ("wii", &["-b -e \"{rom}\""])];

const THEMES: &[&str] = &["spectrum", "classic", "keys", "arcade", "aurora", "midnight", "artwork", "blur", "cube", "blueshift", "swirl", "pocket", "oled", "walnut", "forest", "crt", "vhs", "synth"];

fn one_of(v: Option<&Value>, list: &[&str]) -> bool {
    js::str_of(v).is_some_and(|x| list.contains(&x))
}

/// `list.includes(v || fallback)`: a value that isn't set counts as the fallback.
fn one_of_or(v: Option<&Value>, list: &[&str], fallback: &str) -> bool {
    if js::truthy(v) {
        one_of(v, list)
    } else {
        list.contains(&fallback)
    }
}

/// Values of older versions moved to today's.
fn upgrade(cfg: &mut Value) {
    static CUSTOM: LazyLock<Regex> = LazyLock::new(|| Regex::new("^custom-[a-z0-9-]{1,48}$").unwrap());
    if let Some(Value::Object(ui)) = cfg.get_mut("ui") {
        let set = |ui: &mut Map<String, Value>, k: &str, v: Value| {
            ui.insert(k.to_string(), v);
        };
        if ui.get("quitComboKeys").and_then(Value::as_str) == Some("start+back") && !js::truthy(ui.get("quitComboChosen")) {
            set(ui, "quitComboKeys", json!("l3+r3"));
        }
        let rev = |ui: &Map<String, Value>| js::number(ui.get("soundRev"));
        // 0.7.3 turned the ambient bed way down
        if (if js::truthy(ui.get("soundRev")) { rev(ui) } else { 1.0 }) < 2.0 {
            if matches!(ui.get("ambientLevel"), None | Some(Value::Null)) || js::number(ui.get("ambientLevel")) >= 0.05 {
                set(ui, "ambientLevel", json!(0.02));
            }
            set(ui, "soundRev", json!(2));
        }
        // 0.7.4: the ambient bed is opt-in
        if rev(ui) < 3.0 {
            set(ui, "ambient", json!(false));
            set(ui, "soundRev", json!(3));
        }
        // 1.75: menu music replaced the ambient bed; whoever had the bed on gets a track at about the same loudness
        if rev(ui) < 4.0 {
            let (on, lv) = (ui.get("ambient") == Some(&Value::Bool(true)), js::number(ui.get("ambientLevel")));
            set(ui, "music", json!(if on { "calm" } else { "off" }));
            set(ui, "musicLevel", if on && lv.is_finite() && lv > 0.0 { js::num((js::round(lv / 0.02 * 0.4 * 20.0) / 20.0).min(1.0)) } else { json!(0.4) });
            set(ui, "soundRev", json!(4));
        }
        // 1.28: older themes and sounds move to their nearest new one
        let moved = |v: Option<&Value>, table: &[(&str, &str)]| js::str_of(v).and_then(|x| table.iter().find(|(old, _)| *old == x)).map(|(_, new)| json!(new));
        if let Some(t) = moved(ui.get("theme"), &[("sunset", "spectrum"), ("gallery", "midnight"), ("synthwave", "arcade")]) {
            set(ui, "theme", t);
        }
        if let Some(t) = moved(ui.get("soundTheme"), &[("glass", "aero"), ("wood", "aero"), ("pulse", "musicbox"), ("arcade", "musicbox"), ("xylo", "melody"), ("jazz", "melody")]) {
            set(ui, "soundTheme", t);
        }
        // 1.76: Background → Game artwork became the Artwork theme
        if ui.get("background").and_then(Value::as_str) == Some("art") {
            set(ui, "theme", json!("artwork"));
            set(ui, "background", json!("blur"));
        }
        // custom-…: a picture theme from Custom themes\
        if !one_of_or(ui.get("theme"), THEMES, "spectrum") && !CUSTOM.is_match(&js::string(ui.get("theme"))) {
            set(ui, "theme", json!("spectrum"));
        }
        if !one_of_or(ui.get("soundTheme"), &["aero", "melody", "musicbox"], "aero") {
            set(ui, "soundTheme", json!("aero"));
        }
        // 1.77: the ambient tracks took over from Calm / Lounge / Chime
        if let Some(t) = moved(ui.get("music"), &[("calm", "drift"), ("lounge", "night"), ("chime", "glow")]) {
            set(ui, "music", t);
        }
        if !one_of_or(ui.get("music"), &["drift", "night", "glow", "own", "off"], "drift") {
            set(ui, "music", json!("drift"));
        }
        if !one_of_or(ui.get("bootSound"), &["chime", "crystal", "pulse", "horizon", "keys"], "chime") {
            set(ui, "bootSound", json!("chime"));
        }
        if !ui.get("discordPresence").is_some_and(Value::is_boolean) {
            set(ui, "discordPresence", json!(false));
        }
        let digits: String = js::text(ui.get("discordAppId")).chars().filter(char::is_ascii_digit).take(25).collect();
        set(ui, "discordAppId", json!(digits));
        if !one_of(ui.get("language"), &["en", "pt", "fr", "tr"]) {
            set(ui, "language", json!("en"));
        }
    }
    for c in &systems().consoles {
        let old = OLD_DEFAULTS.iter().find(|(id, _)| *id == c.id).map_or(&[][..], |o| o.1);
        if let Some(Value::Object(sc)) = cfg.get_mut("systems").and_then(|s| s.get_mut(&c.id)) {
            if one_of(sc.get("gameArgs"), old) {
                sc.insert("gameArgs".into(), json!(c.game_args));
            }
        }
    }
}

/// Downloads from the Internet are opt-in for new installs, and stay on for a file saved before the keys existed.
fn existing_user(cfg: &mut Value, saved: Option<&Value>) {
    let ui = saved.and_then(|s| s.get("ui")).filter(|u| u.is_object());
    for k in ["autoCovers", "autoManuals"] {
        if ui.and_then(|u| u.get(k)).is_none() {
            cfg["ui"][k] = json!(true);
        }
    }
    // the museum's themed rooms are for new installs: a file saved before the choice existed keeps the Classic wall
    if ui.and_then(|u| u.get("museumStyle")).filter(|m| is_obj(Some(m))).and_then(|m| m.get("grouping")).is_none() {
        cfg["ui"]["museumStyle"]["grouping"] = json!("classic");
    }
}

/// Details' Status per game (main/config.js GAME_STATUS).
pub const GAME_STATUS: &[&str] = &["playing", "finished", "notstarted", "abandoned"];

/// What Settings chooses from: any other value falls back to the default (main/config.js ONE_OF).
const ONE_OF: &[(&str, &[&str])] = &[
    ("openInto", &["last", "home", "museum"]),
    ("clock", &["full", "time", "off"]),
    ("confirmButton", &["auto", "a", "b"]),
    ("scrollSpeed", &["relaxed", "normal", "fast"]),
    ("artRegion", &["auto", "usa", "eur", "jpn"]),
    ("buttons", &["auto", "xbox", "ps", "nintendo"]),
    ("quitComboKeys", &["l3+r3", "lb+rb+start", "back+b", "start+back"]),
    ("performance", &["auto", "high", "balanced", "low"]),
    ("background", &["blur", "art"]),
    ("homeLayout", &["side", "center"]),
];

static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new("^#(?i:[0-9a-f]{6})$").unwrap());

fn bool_or(m: &mut Map<String, Value>, k: &str, default: bool) {
    if !m.get(k).is_some_and(Value::is_boolean) {
        m.insert(k.to_string(), json!(default));
    }
}

/// A number written as JS reads it, kept when it's really one (not null or "").
fn written(v: Option<&Value>) -> Option<f64> {
    let n = js::number(v);
    (n.is_finite() && !matches!(v, Some(Value::Null)) && v.and_then(Value::as_str) != Some("")).then_some(n)
}

/// `{ "<name>": scale }` with the scales between 0.5 and 2, to two decimals.
fn scales(v: Option<&Value>, max_entries: usize, max_key: usize) -> Value {
    let mut out = Map::new();
    if let Some(Value::Object(o)) = v {
        for (k, x) in js::entries(o).into_iter().take(max_entries) {
            let n = js::number(Some(x));
            if len16(k) <= max_key && n.is_finite() && (0.5..=2.0).contains(&n) {
                out.insert(k.clone(), js::num(js::round(n * 100.0) / 100.0));
            }
        }
    }
    Value::Object(out)
}

fn sanitize_pc(pc: &mut Map<String, Value>) {
    static ENV_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Za-z_][A-Za-z0-9_]{0,80}$").unwrap());
    // which shelf a game added by hand sits on: Custom (none), PC, Ports & Recomps, or a console's
    let shelf_of = |c: &Value| [c.get("shelf"), c.get("platform")].into_iter().filter_map(js::str_of).find(|x| *x == "pc" || *x == "ports" || systems().console(x).is_some()).map(String::from);
    let custom: Vec<Value> = pc
        .get("custom")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| c.is_object() && !s(c.get("exe"), 1024).is_empty())
        .take(2000)
        .map(|c| {
            let mut o = Map::new();
            let id = [s(c.get("id"), 80), s(c.get("title"), 80), s(c.get("exe"), 80)].into_iter().find(|x| !x.is_empty()).unwrap_or_default();
            o.insert("id".into(), json!(id));
            o.insert("title".into(), json!(s(c.get("title"), 120)));
            o.insert("exe".into(), json!(s(c.get("exe"), 1024)));
            o.insert("args".into(), json!(s(c.get("args"), 1000)));
            for (k, n) in [("src", 20), ("runner", 1024)] {
                let v = s(c.get(k), n);
                if !v.is_empty() {
                    o.insert(k.into(), json!(v));
                }
            }
            if let Some(shelf) = shelf_of(c) {
                o.insert("shelf".into(), json!(shelf));
            }
            if let Some(from) = js::str_of(c.get("portOf")).filter(|x| systems().console(x).is_some()) {
                o.insert("portOf".into(), json!(from));
            }
            Value::Object(o)
        })
        .collect();
    pc.insert("custom".into(), Value::Array(custom));
    // the games folders a Playnite import added
    let mut pn = Map::new();
    if let Some(Value::Object(o)) = pc.get("pnDirs") {
        for (k, v) in js::entries(o).into_iter().take(60) {
            let dirs: Vec<Value> = v.as_array().into_iter().flatten().filter(|x| x.as_str().is_some_and(|x| len16(x) <= 1024)).take(50).cloned().collect();
            pn.insert(js::slice(k, 20).to_string(), Value::Array(dirs));
        }
    }
    pc.insert("pnDirs".into(), Value::Object(pn));
    for k in ["gamesDir", "userGamesDir", "runner", "prefixDir", "homeDir", "legendaryPath", "epicDir"] {
        let v = s(pc.get(k), 1024);
        pc.insert(k.into(), json!(v));
    }
    // the native app's own: the ports and recompilations folder (kept only when it's set)
    match s(pc.get("portsDir"), 1024) {
        v if v.is_empty() => {
            pc.remove("portsDir");
        }
        v => {
            pc.insert("portsDir".into(), json!(v));
        }
    }
    let mut run = Map::new();
    if let Some(Value::Object(o)) = pc.get("run") {
        for (id, v) in js::entries(o).into_iter().filter(|(_, v)| v.is_object()).take(2000) {
            let mut e = Map::new();
            let (runner, prefix) = (s(v.get("runner"), 1024), s(v.get("prefix"), 1024));
            if !runner.is_empty() {
                e.insert("runner".into(), json!(runner));
            }
            if !prefix.is_empty() && js::is_absolute(&prefix) {
                e.insert("prefix".into(), json!(prefix));
            }
            if let Some(Value::Object(env)) = v.get("env") {
                let vars: Map<String, Value> =
                    js::entries(env).into_iter().filter(|(k, x)| ENV_NAME.is_match(k) && x.as_str().is_some_and(|x| len16(x) <= 4096)).take(50).map(|(k, x)| (k.clone(), x.clone())).collect();
                e.insert("env".into(), Value::Object(vars));
            }
            run.insert(js::slice(id, 200).to_string(), Value::Object(e));
        }
    }
    pc.insert("run".into(), Value::Object(run));
    let mut programs = Map::new();
    if let Some(Value::Object(o)) = pc.get("programs") {
        let inside = |v: &str| !v.is_empty() && len16(v) <= 1024 && !js::is_absolute(v) && !v.split(['\\', '/']).any(|p| p == "..");
        for (k, v) in js::entries(o).into_iter().filter(|(_, v)| v.as_str().is_some_and(inside)).take(2000) {
            programs.insert(js::slice(k, 200).to_string(), v.clone());
        }
    }
    pc.insert("programs".into(), Value::Object(programs));
    let color = |v: Option<&Value>| js::str_of(v).is_some_and(|c| COLOR.is_match(c));
    if !color(pc.get("boxColor")) {
        pc.insert("boxColor".into(), json!(""));
    }
    let mut colors = Map::new();
    if let Some(Value::Object(o)) = pc.get("boxColors") {
        for (k, v) in js::entries(o).into_iter().filter(|(k, v)| ["ps5", "ps4", "xsx", "xone", "sw"].contains(&k.as_str()) && color(Some(v))) {
            colors.insert(k.clone(), v.clone());
        }
    }
    pc.insert("boxColors".into(), Value::Object(colors));
    let quiver = s(pc.get("quiverPath"), 1024);
    pc.insert("quiverPath".into(), json!(quiver));
    bool_or(pc, "consoleBox", false);
    for k in ["epic", "quiver", "gog", "ubisoft", "ea", "xbox", "hoyoplay"] {
        bool_or(pc, k, true);
    }
}

/// Every value brought back to one the app can use: anything else (a hand-edited file, an older or newer version)
/// falls back to its default.
pub fn sanitize(cfg: &mut Value) {
    static LOOK_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Za-z][A-Za-z0-9]{0,30}$").unwrap());
    if !is_obj(Some(cfg)) || !is_obj(cfg.get("ui")) {
        return;
    }
    let d = defaults();
    let d = d["ui"].as_object().unwrap();
    // per-game choices: plain values only, a sane number of them
    let mut games = Map::new();
    if let Some(Value::Object(g)) = cfg.get("games") {
        for (id, v) in js::entries(g).into_iter().take(5000) {
            if len16(id) > 2048 || !v.is_object() {
                continue;
            }
            let pick = |k: &str, list: &[&str]| js::str_of(v.get(k)).filter(|x| list.contains(x)).unwrap_or("").to_string();
            let name = |k: &str| js::str_of(v.get(k)).map(|t| tidy(t, 120)).unwrap_or_default();
            let fields = [
                ("emulator", s(v.get("emulator"), 1024)),
                ("title", name("title")),
                ("window", pick("window", &["borderless", "windowed"])),
                ("install", pick("install", &["installed", "notinstalled"])),
                ("artName", name("artName")),
                ("status", pick("status", GAME_STATUS)),
            ];
            let o: Map<String, Value> = fields.into_iter().filter(|(_, x)| !x.is_empty()).map(|(k, x)| (k.to_string(), json!(x))).collect();
            if !o.is_empty() {
                games.insert(id.clone(), Value::Object(o));
            }
        }
    }
    cfg["games"] = Value::Object(games);
    cfg["retroarch"] = json!(s(cfg.get("retroarch"), 1024));

    {
        let ui = cfg["ui"].as_object_mut().unwrap();
        let v = js::str_of(ui.get("lastSeenVersion")).map(|t| js::slice(t, 40).to_string()).unwrap_or_default();
        ui.insert("lastSeenVersion".into(), json!(v));
        if !one_of(ui.get("previews"), &["off", "details", "shelf"]) {
            ui.insert("previews".into(), json!("off"));
        }
        for k in ["previewSound", "previewYoutube", "previewClips"] {
            if !ui.get(k).is_some_and(Value::is_boolean) {
                ui.insert(k.into(), d[k].clone());
            }
        }
        let n = js::number(ui.get("previewDelay"));
        ui.insert("previewDelay".into(), js::num(if [0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 8.0, 10.0, 15.0].contains(&n) { n } else { 1.5 }));
        for k in ["clipHint", "deckHint", "leaveHint", "deckScaled"] {
            bool_or(ui, k, false);
        }
        let dirs = strings(ui.get("previewDirs"), 1024, 20);
        ui.insert("previewDirs".into(), Value::Array(dirs));
        // your collections (shelves of your own): [{ id, name, ids: [game ids], tag? }]
        let collections: Vec<Value> = ui
            .get("collections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|c| c.is_object() && js::filled(c.get("id")).is_some() && c.get("name").is_some_and(Value::is_string))
            .take(50)
            .map(|c| {
                let name = Some(tidy(c["name"].as_str().unwrap_or(""), 40)).filter(|n| !n.is_empty()).unwrap_or_else(|| "Collection".into());
                let mut o = json!({ "id": js::slice(c["id"].as_str().unwrap_or(""), 40), "name": name, "ids": strings(c.get("ids"), 1024, 5000) });
                if let Some(tag) = c.get("tag").and_then(Value::as_str).filter(|t| !js::trim(t).is_empty()) {
                    o["tag"] = json!(tidy(tag, 40));
                }
                // a shelf that fills itself (beta.14): { kind, value }
                if let Some(kind) = c.get("rule").and_then(|r| r.get("kind")).and_then(Value::as_str).filter(|k| ["genre", "status", "never", "console"].contains(k)) {
                    let v = match c["rule"].get("value") {
                        Some(Value::String(x)) => js::slice(x, 80).to_string(),
                        Some(Value::Number(n)) => js::slice(&n.to_string(), 80).to_string(),
                        _ => String::new(),
                    };
                    o["rule"] = json!({ "kind": kind, "value": if kind == "never" { String::new() } else { v } });
                }
                o
            })
            .collect();
        ui.insert("collections".into(), Value::Array(collections));
        // Looks (Settings → Display → Looks): named sets of look settings, plain values only
        let looks: Vec<Value> = ui
            .get("looks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|l| l.is_object() && l.get("name").is_some_and(Value::is_string))
            .take(20)
            .map(|l| {
                let mut set = Map::new();
                if let Some(Value::Object(o)) = l.get("set") {
                    for (k, v) in js::entries(o).into_iter().take(30) {
                        let plain = match v {
                            Value::String(t) => len16(t) <= 80,
                            Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
                            Value::Bool(_) => true,
                            _ => false,
                        };
                        if LOOK_KEY.is_match(k) && plain {
                            set.insert(k.clone(), v.clone());
                        }
                    }
                }
                let name = Some(tidy(l["name"].as_str().unwrap_or(""), 40)).filter(|n| !n.is_empty()).unwrap_or_else(|| "Look".into());
                json!({ "name": name, "set": set })
            })
            .collect();
        ui.insert("looks".into(), Value::Array(looks));
    }

    if let Some(Value::Object(pc)) = cfg.get_mut("pc") {
        sanitize_pc(pc);
    }
    let e = cfg.get("emuSetup").filter(|e| e.is_object());
    let get = |k: &str| e.and_then(|e| e.get(k));
    cfg["emuSetup"] = json!({
        "auto": get("auto") != Some(&Value::Bool(false)),
        "graphics": js::str_of(get("graphics")).filter(|g| ["auto", "fast", "balanced", "pretty"].contains(g)).unwrap_or("auto"),
        "resume": get("resume") == Some(&Value::Bool(true)),
        "pending": strings(get("pending"), 1024, 40),
    });
    cfg["emuSeen"] = Value::Array(strings(cfg.get("emuSeen"), 1024, 20));

    let ui = cfg["ui"].as_object_mut().unwrap();
    for (k, list) in ONE_OF {
        if !one_of(ui.get(*k), list) {
            ui.insert(k.to_string(), d[*k].clone());
        }
    }
    if !ui.get("clock24").is_some_and(Value::is_boolean) {
        ui.insert("clock24".into(), d["clock24"].clone());
    }
    // Reduce motion: false Off, true On, "system" Follow Windows
    if !ui.get("reduceMotion").is_some_and(|v| v.is_boolean() || v == "system") {
        ui.insert("reduceMotion".into(), d["reduceMotion"].clone());
    }
    bool_or(ui, "seamless", true);
    let vol = written(ui.get("trailerVol")).filter(|n| (0.0..=1.0).contains(n)).unwrap_or(0.7);
    ui.insert("trailerVol".into(), js::num(vol));
    if !one_of(ui.get("screenMode"), &["", "full", "window"]) {
        ui.insert("screenMode".into(), json!(""));
    }
    // Window mode's last place and size
    let bounds = ui.get("windowBounds").filter(|b| b.is_object()).and_then(|b| {
        let n = |k: &str| b.get(k).and_then(Value::as_f64).filter(|n| n.is_finite() && n.abs() < 100_000.0);
        let (x, y, w, h) = (n("x")?, n("y")?, n("width")?, n("height")?);
        (w >= 200.0 && h >= 150.0).then(|| json!({ "x": js::num(js::round(x)), "y": js::num(js::round(y)), "width": js::num(js::round(w)), "height": js::num(js::round(h)) }))
    });
    ui.insert("windowBounds".into(), bounds.unwrap_or(Value::Null));
    let display = match ui.get("display") {
        v @ Some(Value::String(_) | Value::Number(_)) => js::slice(&js::string(v), 64).to_string(),
        _ => String::new(),
    };
    ui.insert("display".into(), json!(display));
    let audio = js::str_of(ui.get("audioOut")).map(|a| js::slice(a, 512).to_string()).unwrap_or_default();
    ui.insert("audioOut".into(), json!(audio));
    let safe = written(ui.get("safeArea")).map_or(100.0, |n| js::round(n).clamp(90.0, 100.0));
    ui.insert("safeArea".into(), js::num(safe));
    let attract = Some(js::number(ui.get("attractMinutes"))).filter(|n| (1.0..=60.0).contains(n)).map_or(d["attractMinutes"].clone(), js::num);
    ui.insert("attractMinutes".into(), attract);
    if !matches!(ui.get("attractPace").and_then(Value::as_str), Some("slow" | "normal" | "fast")) {
        ui.insert("attractPace".into(), json!("normal"));
    }
    if !ui.get("attractWander").is_some_and(Value::is_boolean) {
        ui.insert("attractWander".into(), json!(true));
    }
    let dim = written(ui.get("dimMinutes")).filter(|n| [0.0, 10.0, 20.0, 30.0].contains(n)).unwrap_or(0.0);
    ui.insert("dimMinutes".into(), js::num(dim));
    let scale = Some(js::number(ui.get("uiScale"))).filter(|n| [1.0, 1.15, 1.3].contains(n)).unwrap_or(1.0);
    ui.insert("uiScale".into(), js::num(scale));
    // the Wii remote, and made backs: on unless turned off
    for k in ["wiimote", "makeBacks"] {
        if ui.get(k).is_some_and(|v| !v.is_boolean()) {
            ui.shift_remove(k);
        }
    }
    if ui.contains_key("wmHold") && !one_of(ui.get("wmHold"), &["upright", "sideways"]) {
        ui.shift_remove("wmHold");
    }
    if ui.contains_key("wmSpeed") {
        let n = js::number(ui.get("wmSpeed"));
        if n.is_finite() && (0.8..=3.0).contains(&n) {
            ui.insert("wmSpeed".into(), js::num(js::round(n * 10.0) / 10.0));
        } else {
            ui.shift_remove("wmSpeed");
        }
    }
    // Switch 2: the build each game was last started in ({ 'sw2:<file>': exe })
    if ui.contains_key("sw2Last") {
        let mut o = Map::new();
        if let Some(Value::Object(last)) = ui.get("sw2Last") {
            let ents = js::entries(last);
            for (k, v) in ents.iter().skip(ents.len().saturating_sub(300)) {
                if v.as_str().is_some_and(|v| len16(v) <= 600) && len16(k) <= 900 {
                    o.insert((*k).clone(), (*v).clone());
                }
            }
        }
        ui.insert("sw2Last".into(), Value::Object(o));
    }
    for (k, lo, hi) in [("volume", 0.0, 1.0), ("ambientLevel", 0.0, 0.06), ("musicLevel", 0.0, 1.0)] {
        let given = match ui.get(k) {
            Some(Value::Number(_)) => true,
            Some(Value::String(t)) => !js::trim(t).is_empty(),
            _ => false,
        };
        let n = js::number(ui.get(k));
        let v = if given && n.is_finite() { js::num(n.clamp(lo, hi)) } else { d[k].clone() };
        ui.insert(k.into(), v);
    }
    if !is_obj(ui.get("museumStyle")) {
        ui.insert("museumStyle".into(), json!({}));
    }
    let cap = js::number(ui.get("arcadeCap"));
    ui.insert("arcadeCap".into(), if cap.is_finite() { js::num(js::round(cap.clamp(20.0, 30.0))) } else { json!(24) });
    if ui.contains_key("museumLight") && !one_of(ui.get("museumLight"), &["gallery", "daylight", "night", "neon", "spotlit", "lightsout"]) {
        ui.insert("museumLight".into(), json!("gallery"));
    }
    // the museum's Retro TV: anything unknown is dropped, so the room falls back to its default
    let ms = ui["museumStyle"].as_object_mut().unwrap();
    bool_or(ms, "walkIn", true);
    if ms.contains_key("grouping") && !one_of(ms.get("grouping"), &["classic", "manufacturer", "genre"]) {
        ms.insert("grouping".into(), json!("classic"));
    }
    for (k, list) in [("tvStand", &["cart", "cabinet", "floor", "wall"][..]), ("tvSize", &["s", "m", "l"]), ("tvPos", &["left", "centre", "right"]), ("tvCh", &["ads", "trailers", "recent", "mix"]), ("tvModel", &["crt", "wood", "flat"])] {
        if ms.contains_key(k) && !one_of(ms.get(k), list) {
            ms.shift_remove(k);
        }
    }
    for k in ["tv", "tvMute", "tvNear", "tvAnim", "tvSpatial"] {
        if ms.get(k).is_some_and(|v| !v.is_boolean()) {
            ms.shift_remove(k);
        }
    }
    for k in ["tvVol", "tvCrt"] {
        match ms.get(k).map(|v| v.as_f64().filter(|n| n.is_finite())) {
            Some(Some(n)) => {
                ms.insert(k.into(), js::num(n.clamp(0.0, 1.0)));
            }
            Some(None) => {
                ms.shift_remove(k);
            }
            None => {}
        }
    }
    // the TV guide: favourites and the last thing on per channel
    if ui.contains_key("tvFavs") {
        let favs = strings(ui.get("tvFavs"), 600, 500);
        ui.insert("tvFavs".into(), Value::Array(favs));
    }
    if ui.contains_key("tvLast") {
        let mut o = Map::new();
        for k in ["ads", "trailers", "recent", "mix"] {
            if let Some(v) = js::filled(ui.get("tvLast").and_then(|t| t.get(k))).filter(|v| len16(v) <= 600) {
                o.insert(k.into(), json!(v));
            }
        }
        ui.insert("tvLast".into(), Value::Object(o));
    }
    let (mut seen, mut hidden) = (HashSet::new(), Vec::new());
    for v in ui.get("hidden").and_then(Value::as_array).into_iter().flatten().filter(|v| v.is_string() || v.is_number()) {
        let id = js::slice(&js::string(Some(v)), 2048).to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        hidden.push(json!(id));
        if hidden.len() >= 5000 {
            break;
        }
    }
    ui.insert("hidden".into(), Value::Array(hidden));
    bool_or(ui, "autoHideHints", false);
    bool_or(ui, "allShelf", false);
    let bs = Some(js::number(ui.get("boxScale"))).filter(|n| (0.6..=1.6).contains(n)).map_or(1.0, |n| js::round(n * 100.0) / 100.0);
    ui.insert("boxScale".into(), js::num(bs));
    let shelf_scale = scales(ui.get("shelfScale"), 100, 40);
    ui.insert("shelfScale".into(), shelf_scale);
    let game_scale = scales(ui.get("gameScale"), 1000, 120);
    ui.insert("gameScale".into(), game_scale);
    let mut sort = Map::new();
    if let Some(Value::Object(o)) = ui.get("customSort") {
        for (k, v) in js::entries(o).into_iter().take(100) {
            if let (true, Some(list)) = (len16(k) <= 80, v.as_array()) {
                let ids: Vec<Value> = list.iter().filter(|x| x.as_str().is_some_and(|x| len16(x) <= 1024)).take(5000).cloned().collect();
                sort.insert(k.clone(), Value::Array(ids));
            }
        }
    }
    ui.insert("customSort".into(), Value::Object(sort));
    if let Some(Value::Object(steam)) = cfg.get_mut("steam") {
        bool_or(steam, "hideTools", true);
    }
    cfg["art"] = sanitize_art(cfg.get("art"), cfg.get("ui"));
}

// ---------- Art Studio ----------

const PARTS: &[&str] = &["front", "back", "spine", "disc"];
const LOOKS: &[&str] = &["authentic", "hd", "region:usa", "region:eur", "region:jpn", "special"];
const PRESETS: &[&str] = &["authentic", "hd", "region", "special", "sets", "mix"];
const REGIONS: &[&str] = &["usa", "eur", "jpn"];

/// The style a console has before anyone picks one: authentic, in the region the old "Box art region" chose
/// (auto → usa), which then keeps beating each game's own region; the old "Prefer HD scans" stays a tie-breaker.
pub fn default_art_style(ui: Option<&Value>) -> Value {
    let region = js::str_of(ui.and_then(|u| u.get("artRegion"))).filter(|r| REGIONS.contains(r));
    let mut st = json!({ "preset": "authentic", "region": region.unwrap_or("usa"), "parts": {}, "sharper": ui.and_then(|u| u.get("artHD")) != Some(&Value::Bool(false)) });
    if region.is_some() {
        st["regionFirst"] = json!(true);
    }
    st
}

pub fn clean_art_style(st: &Value, base: &Value) -> Value {
    if !st.is_object() {
        return base.clone();
    }
    let pick = |k: &str, list: &[&str]| js::str_of(st.get(k)).filter(|x| list.contains(x)).map_or(base[k].clone(), Value::from);
    let mut out = Map::new();
    out.insert("preset".into(), pick("preset", PRESETS));
    out.insert("region".into(), pick("region", REGIONS));
    let mut parts = Map::new();
    if let Some(Value::Object(p)) = st.get("parts") {
        for part in PARTS {
            // a part's look: an Art Studio look, or 'region' (the style's own region)
            if let Some(l) = js::str_of(p.get(*part)).filter(|l| LOOKS.contains(l) || *l == "region") {
                parts.insert(part.to_string(), json!(l));
            }
        }
    }
    out.insert("parts".into(), Value::Object(parts));
    if let Some(sharper) = st.get("sharper").filter(|v| v.is_boolean()).or(base.get("sharper").filter(|v| v.is_boolean())) {
        out.insert("sharper".into(), sharper.clone());
    }
    let first = match st.get("regionFirst") {
        Some(Value::Bool(b)) => *b,
        _ => js::truthy(base.get("regionFirst")) && out["region"] == base["region"],
    };
    if first {
        out.insert("regionFirst".into(), json!(true));
    }
    // Complete sets: the HD sets rather than one region's
    if out["preset"] == "sets" && st.get("setsHd") == Some(&Value::Bool(true)) {
        out.insert("setsHd".into(), json!(true));
    }
    Value::Object(out)
}

pub fn sanitize_art(a: Option<&Value>, ui: Option<&Value>) -> Value {
    static KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[a-z0-9]+\\|[^\n\r\u{2028}\u{2029}]").unwrap());
    let (mut styles, mut locks) = (Map::new(), Map::new());
    if let Some(Value::Object(st)) = a.and_then(|a| a.get("styles")) {
        for (id, v) in js::entries(st).into_iter().filter(|(id, v)| systems().console(id).is_some() && v.is_object()) {
            styles.insert(id.clone(), clean_art_style(v, &default_art_style(ui)));
        }
    }
    if let Some(Value::Object(l)) = a.and_then(|a| a.get("locks")) {
        for (k, v) in js::entries(l) {
            if locks.len() >= 20000 {
                break;
            }
            if len16(k) > 4200 || !KEY.is_match(k) || !v.is_object() {
                continue;
            }
            let mut e = Map::new();
            for p in PARTS {
                let Some(l) = v.get(*p).filter(|l| l.is_object()) else { continue };
                let Some(look) = js::str_of(l.get("look")).filter(|x| LOOKS.contains(x) || *x == "exact") else { continue };
                let mut part = Map::new();
                part.insert("look".into(), json!(look));
                if let Some(key) = js::filled(l.get("key")).filter(|k| len16(k) <= 64) {
                    part.insert("key".into(), json!(key));
                }
                let at = js::number(l.get("at"));
                if at.is_finite() && at > 0.0 {
                    part.insert("at".into(), js::num(at));
                }
                e.insert(p.to_string(), Value::Object(part));
            }
            // all: the whole game locked as it was (sig: the look each part followed then)
            if let Some(all) = v.get("all").filter(|a| a.is_object()) {
                let mut sig = Map::new();
                for p in PARTS {
                    if let Some(x) = js::str_of(all.get("sig").filter(|s| s.is_object()).and_then(|s| s.get(*p))).filter(|x| len16(x) <= 120) {
                        sig.insert(p.to_string(), json!(x));
                    }
                }
                let at = js::number(all.get("at"));
                e.insert("all".into(), json!({ "at": if at.is_finite() { js::num(at) } else { json!(0) }, "sig": sig }));
            }
            if !e.is_empty() {
                locks.insert(k.clone(), Value::Object(e));
            }
        }
    }
    json!({ "styles": styles, "locks": locks })
}

/// Settings as they come from a screen: the same shape rules as a loaded file.
pub fn normalize(c: &Value) -> Option<Value> {
    if !c.is_object() {
        return None;
    }
    let mut cfg = merge(&defaults(), c);
    sanitize(&mut cfg);
    Some(cfg)
}

/// A saved file's text as today's settings; None when it isn't a settings object.
pub fn read(text: &str) -> Option<Value> {
    let saved = serde_json::from_str::<Value>(text).ok().filter(Value::is_object)?;
    let mut cfg = merge(&defaults(), &saved);
    existing_user(&mut cfg, Some(&saved));
    // merge fills soundRev with today's, which would hide an old file from the sound migrations: one without it is rev 1
    if matches!(saved.get("ui").filter(|u| u.is_object()).and_then(|u| u.get("soundRev")), None | Some(Value::Null)) {
        cfg["ui"]["soundRev"] = json!(1);
    }
    upgrade(&mut cfg);
    sanitize(&mut cfg);
    Some(cfg)
}

/// The settings when the file is damaged or locked: the defaults, as an existing user's.
pub fn fallback() -> Value {
    let mut cfg = defaults();
    existing_user(&mut cfg, None);
    cfg
}

/// Writes the settings through a temporary file, flushed before it takes the real one's place: a crash or a power cut
/// leaves the old file or the new one, never a cut-off one.
pub fn write(file: &Path, cfg: &Value) -> std::io::Result<()> {
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = file.with_file_name(format!("{}.tmp", file.file_name().unwrap_or_default().to_string_lossy()));
    let put = || -> std::io::Result<()> {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let mut f = opts.open(&tmp)?;
        f.write_all(js::stringify_pretty(cfg).as_bytes())?;
        f.sync_all()?;
        drop(f);
        // Windows: antivirus or indexing can hold the file for a moment
        let mut tries = 0;
        loop {
            match std::fs::rename(&tmp, file) {
                Ok(()) => return Ok(()),
                Err(e) if tries < 20 && e.kind() == std::io::ErrorKind::PermissionDenied => {
                    tries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(e),
            }
        }
    };
    put().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

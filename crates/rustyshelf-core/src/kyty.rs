// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/kyty.js, as RustyShelf has it.
//! PS5 games through Kyty: folders with eboot.bin and sce_sys/param.json, started straight in kyty_emulator.exe the way
//! Kyty's own launcher does, so a game boots with the settings chosen there.
//!   - the launcher's settings (Kyty.ini beside it in a portable setup, else %ProgramData%\Kyty\Kyty.ini on Windows
//!     and ~/.config/Kyty/Kyty.ini on Linux) become kyty_emulator's options; fullscreen and the hidden cursor are
//!     always on
//!   - a u59-preset.json beside the emulator (KytyJets) is applied as environment variables, after clearing inherited
//!     KYTY_* / TRACY_* ones, as its launcher does
//!   - the game's patch file: _Patches\<title id>.json beside the emulator

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::paths::{is_dir, is_file};

/// The game folder for a shelf file (…/Game/eboot.bin → …/Game).
pub fn game_dir(file: &Path) -> PathBuf {
    let is_eboot = file.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("eboot.bin"));
    match file.parent() {
        Some(p) if is_eboot => p.to_path_buf(),
        _ => file.to_path_buf(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    /// in the game's default language, else US English
    pub title: String,
    pub title_id: String,
}

/// sce_sys/param.json of the game a shelf file (or folder) belongs to.
pub fn read_param(file: &Path) -> Option<Param> {
    let j = crate::files::read_json(&game_dir(file).join("sce_sys").join("param.json"))?;
    let l = j.get("localizedParameters");
    let lang = l.and_then(|l| l.get("defaultLanguage")).and_then(Value::as_str).unwrap_or("en-US");
    let name = |lang: &str| l.and_then(|l| l.get(lang)).and_then(|o| o.get("titleName")).filter(|v| truthy(v)).map(text_of);
    let title = name(lang).or_else(|| name("en-US")).unwrap_or_default();
    let title_id = j.get("titleId").filter(|v| truthy(v)).map(text_of).unwrap_or_default();
    Some(Param { title: title.split_whitespace().collect::<Vec<_>>().join(" "), title_id: title_id.trim().to_string() })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// JS String(v)
fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A game's title id from its param.json, when it's a proper one (PPSA01234).
pub fn title_id(file: &Path) -> String {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{4}[0-9]{5}$").unwrap());
    read_param(file).map(|p| p.title_id).filter(|id| ID.is_match(id)).unwrap_or_default()
}

/// A Qt settings file: section → key → value (Qt quotes some strings, writes lists as "a, b" and @Invalid() for none).
pub fn read_ini(file: &Path) -> Option<HashMap<String, HashMap<String, String>>> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut sec = String::new();
    out.insert(sec.clone(), HashMap::new());
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')).filter(|n| !n.is_empty()) {
            sec = name.to_string();
            out.entry(sec.clone()).or_default();
            continue;
        }
        let Some(i) = line.find('=') else { continue };
        let mut v = line[i + 1..].trim().to_string();
        if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
            v = v[1..v.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\");
        }
        out.entry(sec.clone()).or_default().insert(line[..i].trim().to_string(), v);
    }
    Some(out)
}

/// Where the launcher beside `exe` keeps its settings, as it looks for them.
fn ini_path(exe: &Path) -> PathBuf {
    let portable = exe.with_file_name("Kyty.ini");
    if is_file(&portable) {
        return portable;
    }
    let var = |n: &str| std::env::var_os(n).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        var("ProgramData").unwrap_or_else(|| PathBuf::from("C:\\ProgramData")).join("Kyty").join("Kyty.ini")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))).unwrap_or_default().join("Kyty").join("Kyty.ini")
    }
}

/// option → what it takes ("" for a plain switch)
type Options = HashMap<String, String>;

static OPTIONS_MEMO: LazyLock<Mutex<HashMap<String, Option<Options>>>> = LazyLock::new(Default::default);

/// The options in kyty_emulator's help text; None when there's too little of it to trust.
pub fn parse_options(help: &str) -> Option<Options> {
    static OPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s{2}(--[a-z0-9][a-z0-9-]*)(?:\s+<([^>]*)>)?").unwrap());
    let o: Options = OPT.captures_iter(help).map(|c| (c[1].to_string(), c.get(2).map_or("", |m| m.as_str()).to_string())).collect();
    (o.len() > 5).then_some(o)
}

/// The options this build understands, from its own help: builds differ, and one handed an option it doesn't know
/// prints its help and quits. Kept per program and date.
fn options_of(exe: &Path) -> Option<Options> {
    let name = exe.file_stem()?.to_string_lossy().to_lowercase();
    // only the emulator prints its options; the launcher would open its window
    if name != "kyty_emulator" {
        return None;
    }
    let mtime = std::fs::metadata(exe).ok()?.modified().ok()?;
    let key = format!("{}|{:?}", exe.display(), mtime);
    if let Some(o) = OPTIONS_MEMO.lock().unwrap().get(&key) {
        return o.clone();
    }
    let o = parse_options(&probe(exe));
    OPTIONS_MEMO.lock().unwrap().insert(key, o.clone());
    o
}

/// An unknown option: the build prints its help and quits with an error. Its output then, else nothing.
fn probe(exe: &Path) -> String {
    let mut cmd = Command::new(exe);
    cmd.arg("--rustyshelf-probe").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(d) = exe.parent() {
        cmd.current_dir(d);
    }
    let Ok(mut child) = cmd.spawn() else { return String::new() };
    let read = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut r) = r {
                let mut b = Vec::new();
                let _ = r.read_to_end(&mut b);
                s = String::from_utf8_lossy(&b).into_owned();
            }
            s
        })
    };
    let out = read(child.stdout.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let until = Instant::now() + Duration::from_secs(8);
    let failed = loop {
        match child.try_wait() {
            Ok(Some(st)) => break !st.success(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break true;
            }
        }
    };
    let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
    if failed {
        format!("{out}\n{err}")
    } else {
        String::new()
    }
}

fn on(v: &str) -> bool {
    matches!(v.trim().to_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

/// JS parseInt(v, 10) within lo..=hi, as text.
fn num(v: Option<&String>, lo: i64, hi: i64) -> Option<String> {
    let s = v?.trim_start();
    let (sign, digits) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    let n = sign * digits[..end].parse::<i64>().ok()?;
    (lo..=hi).contains(&n).then(|| n.to_string())
}

/// What one PS5 launch takes: the options before the game, the environment (None: RustyShelf's own), and the game's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub args: Vec<String>,
    pub env: Option<Vec<(String, String)>>,
    pub dir: PathBuf,
}

pub fn spec(exe: &Path, file: &Path) -> Spec {
    let ini = read_ini(&ini_path(exe)).unwrap_or_default();
    let opts = options_of(exe);
    spec_with(exe, file, &ini, opts.as_ref())
}

/// `spec` with the launcher's settings and the build's options given (tests).
pub fn spec_with(exe: &Path, file: &Path, ini: &HashMap<String, HashMap<String, String>>, opts: Option<&Options>) -> Spec {
    let empty = HashMap::new();
    let (g, l) = (ini.get("GlobalConfiguration").unwrap_or(&empty), ini.get("Launcher").unwrap_or(&empty));
    let fallback: Options = [("--screen-width", "num"), ("--screen-height", "num"), ("--fullscreen", ""), ("--hide-cursor", ""), ("--addcont-dir", "dir"), ("--game-patch", "json")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let o = opts.unwrap_or(&fallback);
    let mut a: Vec<String> = Vec::new();
    // a value option, a switch, or a yes / no in the form this build wants (true|false, on|off, or the switch alone)
    let val = |a: &mut Vec<String>, n: &str, v: Option<String>| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            if o.get(n).is_some_and(|s| !s.is_empty()) {
                a.extend([n.to_string(), v]);
            }
        }
    };
    let bool_ = |a: &mut Vec<String>, n: &str, yes: bool| {
        let Some(s) = o.get(n) else { return };
        if s.is_empty() {
            if yes {
                a.push(n.to_string());
            }
        } else if s.contains("on|off") {
            a.extend([n.to_string(), if yes { "on" } else { "off" }.to_string()]);
        } else {
            a.extend([n.to_string(), if yes { "true" } else { "false" }.to_string()]);
        }
    };
    let get = |k: &str| g.get(k);
    static RES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^R?([0-9]{3,5})X([0-9]{3,5})$").unwrap());
    if let Some(c) = RES.captures(get("screen_resolution").map_or("", |s| s.as_str())) {
        val(&mut a, "--screen-width", Some(c[1].to_string()));
        val(&mut a, "--screen-height", Some(c[2].to_string()));
    }
    bool_(&mut a, "--fullscreen", true);
    bool_(&mut a, "--hide-cursor", true);
    if o.get("--shader-cache").is_some_and(|s| s.is_empty()) {
        a.push("--shader-cache".into());
    }
    if let Some(m) = get("present_mode").filter(|m| matches!(m.as_str(), "Fifo" | "Mailbox" | "Immediate")) {
        val(&mut a, "--present-mode", Some(m.clone()));
    }
    if num(get("gpu_index"), 0, 15).is_some() && get("gpu_index").is_some_and(|v| v != "0") {
        val(&mut a, "--gpu", get("gpu_index").cloned());
    }
    let user = get("user_name").map_or("", |s| s.trim());
    if !user.is_empty() && user.len() <= 16 {
        val(&mut a, "--user-name", Some(user.to_string()));
    }
    val(&mut a, "--user-id", num(get("user_id"), 1, 99_999_999));
    let mic = get("audio_input_device").map_or("", |s| s.trim());
    if !mic.is_empty() && mic != "@Invalid()" {
        val(&mut a, "--mic", Some(mic.to_string()));
    }
    static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^#[0-9a-f]{6}$").unwrap());
    if let Some(c) = get("controller_color").filter(|c| COLOR.is_match(c)) {
        val(&mut a, "--controller-color", Some(c.clone()));
    }
    val(&mut a, "--controller-volume", num(get("controller_speaker_volume"), 0, 100));
    val(&mut a, "--controller-vibration", num(get("controller_vibration_intensity"), 0, 100));
    val(&mut a, "--keyboard-player", num(get("controller_keyboard_player"), 0, 4));
    val(&mut a, "--audio-master-volume", num(get("audio_master_volume"), 0, 200));
    val(&mut a, "--audio-main-volume", num(get("audio_main_volume"), 0, 200));
    val(&mut a, "--audio-music-volume", num(get("audio_music_volume"), 0, 200));
    val(&mut a, "--audio-pad-speaker-volume", num(get("audio_pad_speaker_main_volume"), 0, 200));
    val(&mut a, "--audio-objects-volume", num(get("audio_objects_volume"), 0, 200));
    if let Some(v) = get("audio_objects_enabled") {
        bool_(&mut a, "--audio-objects", on(v));
    }
    val(&mut a, "--vblank-frequency", num(get("vblank_frequency"), 1, 240));
    val(&mut a, "--console-language", num(get("console_language"), 0, 29));
    if let Some(t) = get("shader_optimization_type").filter(|t| matches!(t.as_str(), "None" | "Size" | "Performance")) {
        val(&mut a, "--shader-optimization-type", Some(t.clone()));
    }
    for (k, n) in [
        ("hdr_enabled", "--hdr"),
        ("network_connected", "--network-connected"),
        ("perf_overlay_enabled", "--perf-overlay"),
        ("skip_notice_screen", "--skip-notice-screen"),
        ("tessellation_enabled", "--tessellation"),
        ("readback_linear_images", "--readback-linear-images"),
        ("readback_tiled_images", "--readback-tiled-images"),
        ("sync_raw_image_buffers", "--sync-raw-image-buffers"),
        ("red_zone_protection_enabled", "--redzone"),
        ("gpu_occlusion_accurate", "--gpu-occlusion"),
    ] {
        // a plain switch only when on; a yes / no as set
        if let Some(v) = get(k) {
            if on(v) || o.get(n).is_some_and(|s| !s.is_empty()) {
                bool_(&mut a, n, on(v));
            }
        }
    }
    let trophies = get("trophy_enabled");
    if let Some(t) = trophies {
        if o.contains_key("--trophy-notifications") {
            bool_(&mut a, "--trophy-notifications", on(t));
        } else if !on(t) {
            bool_(&mut a, "--no-trophy-notifications", true);
        }
    }
    if !trophies.is_some_and(|t| !on(t)) {
        val(&mut a, "--trophy-duration", num(get("trophy_duration_seconds"), 5, 10));
    }
    // only when set on or off in the launcher; "auto" leaves it to the build, which knows what it means (KytyJets: an
    // Intel compatibility mode; older builds: AMD patches)
    let amd = get("amd_cpu_mode").map_or(String::new(), |s| s.to_lowercase().trim().to_string());
    if on(&amd) {
        bool_(&mut a, "--amd-cpu", true);
    } else if matches!(amd.as_str(), "off" | "false" | "0" | "no") {
        bool_(&mut a, "--no-amd-cpu", true);
    }
    let add = l.get("addons_dir").map_or("", |s| s.trim());
    if !add.is_empty() && is_dir(Path::new(add)) {
        val(&mut a, "--addcont-dir", Some(add.to_string()));
    }
    let dir = exe.parent().unwrap_or(Path::new(""));
    let id = title_id(file);
    if !id.is_empty() {
        let p = dir.join("_Patches").join(format!("{id}.json"));
        if is_file(&p) {
            val(&mut a, "--game-patch", Some(p.to_string_lossy().into_owned()));
        }
    }
    Spec { args: a, env: preset_env(&dir.join("u59-preset.json")), dir: game_dir(file) }
}

fn preset_env(preset: &Path) -> Option<Vec<(String, String)>> {
    static KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z0-9_]+$").unwrap());
    if !is_file(preset) {
        return None;
    }
    let p = crate::files::read_json(preset)?;
    let mut env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
        .filter(|(k, _)| {
            let up = k.to_uppercase();
            !up.starts_with("KYTY_") && !up.starts_with("TRACY_")
        })
        .collect();
    for (k, v) in p.as_object().into_iter().flatten() {
        let v = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => js_number(n),
            _ => continue,
        };
        if KEY.is_match(k) {
            env.retain(|(e, _)| e != k);
            env.push((k.clone(), v));
        }
    }
    Some(env)
}

/// JS String(n): 1, not 1.0
fn js_number(n: &serde_json::Number) -> String {
    match n.as_f64() {
        Some(f) if n.is_f64() && f.fract() == 0.0 && f.abs() < 1e21 => format!("{f:.0}"),
        _ => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_portable_launchers_settings_come_first() {
        let t = tempfile::tempdir().unwrap();
        let emu = t.path().join(if cfg!(windows) { "kyty_emulator.exe" } else { "kyty_emulator" });
        assert!(ini_path(&emu).ends_with("Kyty/Kyty.ini"));
        std::fs::write(t.path().join("Kyty.ini"), "").unwrap();
        assert_eq!(ini_path(&emu), t.path().join("Kyty.ini"));
    }

    #[test]
    fn param() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("sce_sys")).unwrap();
        std::fs::write(
            t.path().join("sce_sys/param.json"),
            r#"{"titleId":"PPSA01234 ","localizedParameters":{"defaultLanguage":"ja-JP","ja-JP":{"titleName":""},"en-US":{"titleName":"Some  Game"}}}"#,
        )
        .unwrap();
        let p = read_param(&t.path().join("eboot.bin")).unwrap();
        assert_eq!((p.title.as_str(), p.title_id.as_str()), ("Some Game", "PPSA01234"));
        assert_eq!(game_dir(&t.path().join("EBOOT.BIN")), t.path());
    }

    const HELP: &str = "Usage:\n  --game <dir>\n  --fullscreen <true|false>\n  --hide-cursor\n  --screen-width <num>\n  --screen-height <num>\n  \
        --skip-notice-screen <true|false>\n  --hdr <on|off>\n  --no-trophy-notifications\n  --amd-cpu\n";

    #[test]
    fn options_for_this_build() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("Game");
        std::fs::create_dir_all(game.join("sce_sys")).unwrap();
        std::fs::write(game.join("sce_sys/param.json"), r#"{"titleId":"PPSA01234"}"#).unwrap();
        let emu = t.path().join("Kyty/kyty_emulator.exe");
        std::fs::create_dir_all(emu.parent().unwrap().join("_Patches")).unwrap();
        std::fs::write(emu.parent().unwrap().join("_Patches/PPSA01234.json"), "{}").unwrap();
        std::fs::write(emu.parent().unwrap().join("u59-preset.json"), r#"{"KYTY_A":1.0,"lower":"x","B":"2"}"#).unwrap();
        let ini_file = t.path().join("Kyty.ini");
        std::fs::write(
            &ini_file,
            "[GlobalConfiguration]\nscreen_resolution=R1920X1080\nhdr_enabled=false\nskip_notice_screen=true\ntrophy_enabled=false\namd_cpu_mode=auto\nuser_id=0\n",
        )
        .unwrap();
        let opts = parse_options(HELP).unwrap();
        let s = spec_with(&emu, &game.join("eboot.bin"), &read_ini(&ini_file).unwrap(), Some(&opts));
        assert_eq!(
            s.args,
            ["--screen-width", "1920", "--screen-height", "1080", "--fullscreen", "true", "--hide-cursor", "--hdr", "off", "--skip-notice-screen", "true", "--no-trophy-notifications"]
        );
        assert_eq!(s.dir, game);
        let env = s.env.unwrap();
        assert!(env.contains(&("KYTY_A".to_string(), "1".to_string())) && env.contains(&("B".to_string(), "2".to_string())));
        assert!(!env.iter().any(|(k, _)| k == "lower"));

        // no help text: the few options every build takes
        let s = spec_with(&emu, &game.join("eboot.bin"), &HashMap::new(), None);
        assert_eq!(s.args, ["--fullscreen", "--hide-cursor", "--game-patch", &emu.parent().unwrap().join("_Patches/PPSA01234.json").to_string_lossy()]);
    }

    #[test]
    fn numbers_as_js_reads_them() {
        assert_eq!(num(Some(&" 12px".to_string()), 0, 99).as_deref(), Some("12"));
        assert_eq!(num(Some(&"x".to_string()), 0, 99), None);
        assert_eq!(num(Some(&"200".to_string()), 0, 99), None);
    }
}

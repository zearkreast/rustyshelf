// Ported from main/linuxpc.js, as RustyShelf has it.
//! (Linux) PC games without Steam: Windows games through Wine or Proton, and Epic Games through legendary.
//!
//! - Runners: the Proton and Wine builds already on the PC, where Steam, Lutris and the distro put them. Proton runs
//!   through umu-run when it's installed, else through Proton's own script. Each game gets a Wine prefix of its own,
//!   named after the game, in the prefixes folder.
//! - Epic: legendary keeps RustyShelf's own Epic sign-in and install list in data/legendary; the account's whole
//!   library is kept in data/epic-library.json so games that aren't installed can be shown too. The files are the
//!   Electron app's.

use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Value};

use crate::js;
use crate::pcgames::slug;

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

fn is_dir(p: &Path) -> bool {
    p.is_dir()
}

fn is_exec(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

fn read_json(f: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(f).ok()?).ok()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn write_atomic(f: &Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", f.display()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, f)
}

/// A program on the PATH.
pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var("PATH").unwrap_or_default().split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|f| is_exec(f))
}

/// The folders (and links to them) in `d`.
fn ls(d: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir() || p.is_symlink()).collect();
    out.sort();
    out
}

/// An Epic app name: the only thing the screen may name.
pub fn is_app_name(s: &str) -> bool {
    static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w.-]{1,120}$").unwrap());
    APP.is_match(s)
}

pub fn tools_dir(app_root: &Path) -> PathBuf {
    app_root.join("tools")
}

// ---------- Wine and Proton builds

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
    /// "proton:<folder>" or "wine:<program>", as Settings keeps it
    pub id: String,
    pub proton: bool,
    pub name: String,
    pub path: PathBuf,
}

fn proton_dirs(app_root: &Path) -> Vec<PathBuf> {
    let h = home();
    vec![
        h.join(".steam/root/compatibilitytools.d"),
        h.join(".local/share/Steam/compatibilitytools.d"),
        h.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"),
        "/usr/share/steam/compatibilitytools.d".into(),
        "/usr/local/share/steam/compatibilitytools.d".into(),
        tools_dir(app_root).join("proton"),
    ]
}

fn steam_common() -> Vec<PathBuf> {
    let h = home();
    vec![h.join(".steam/steam/steamapps/common"), h.join(".local/share/Steam/steamapps/common")]
}

/// "GE-Proton11-5" sorts before "GE-Proton10-4", "Proton 9.0" before "Proton 8.0".
fn ver_key(n: &str) -> String {
    static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
    DIGITS.find_iter(n).map(|m| format!("{:0>6}", m.as_str())).collect::<Vec<_>>().join(".")
}

fn newest_first(list: &mut [PathBuf], name: impl Fn(&Path) -> String) {
    list.sort_by(|a, b| crate::collate::compare(&ver_key(&name(b)), &ver_key(&name(a))));
}

fn base_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The Proton and Wine builds found: Proton's, newest first, then Wine's.
pub fn runners(app_root: &Path) -> Vec<Runner> {
    let mut out: Vec<Runner> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut add = |r: Runner| {
        let real = std::fs::canonicalize(&r.path).unwrap_or_else(|_| r.path.clone());
        if seen.insert(real) {
            out.push(r);
        }
    };
    let mut protons: Vec<PathBuf> = proton_dirs(app_root).iter().flat_map(|d| ls(d)).filter(|p| p.join("proton").is_file()).collect();
    for d in steam_common() {
        protons.extend(ls(&d).into_iter().filter(|p| base_name(p).to_lowercase().starts_with("proton") && p.join("proton").is_file()));
    }
    newest_first(&mut protons, base_name);
    for p in protons {
        add(Runner { id: format!("proton:{}", p.display()), proton: true, name: base_name(&p), path: p });
    }
    let wine_name = |w: &Path| w.parent().and_then(Path::parent).map(base_name).unwrap_or_default();
    let h = home();
    let mut wines: Vec<PathBuf> = [h.join(".local/share/lutris/runners/wine"), tools_dir(app_root).join("wine")].iter().flat_map(|d| ls(d)).map(|p| p.join("bin/wine")).filter(|w| is_exec(w)).collect();
    newest_first(&mut wines, wine_name);
    for w in wines {
        add(Runner { id: format!("wine:{}", w.display()), proton: false, name: wine_name(&w), path: w });
    }
    if let Some(sys) = which("wine") {
        add(Runner { id: format!("wine:{}", sys.display()), proton: false, name: "Wine (system)".into(), path: sys });
    }
    out
}

/// A runner from its id (Settings keeps the id), or the best one found: Proton first (it brings DXVK and VKD3D), the
/// newest GE-Proton or UMU-Proton before Valve's builds, then Wine.
pub fn pick_runner(id: &str, list: &[Runner]) -> Option<Runner> {
    static GE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)ge-proton|umu-proton").unwrap());
    if !id.is_empty() {
        if let Some(r) = list.iter().find(|r| r.id == id) {
            return Some(r.clone());
        }
    }
    list.iter().find(|r| r.proton && GE.is_match(&r.name)).or_else(|| list.iter().find(|r| r.proton)).or_else(|| list.first()).cloned()
}

pub fn umu_run() -> Option<PathBuf> {
    which("umu-run").or_else(|| [home().join(".local/bin/umu-run"), "/usr/bin/umu-run".into(), "/usr/local/bin/umu-run".into()].into_iter().find(|p| is_exec(p)))
}

fn steam_root() -> Option<PathBuf> {
    let h = home();
    [h.join(".steam/steam"), h.join(".local/share/Steam")].into_iter().find(|p| is_dir(p))
}

// ---------- prefixes

pub fn prefix_root(custom: &str, app_root: &Path) -> PathBuf {
    if !custom.is_empty() && Path::new(custom).is_absolute() {
        PathBuf::from(custom)
    } else {
        app_root.join("Prefixes")
    }
}

/// A game's prefix folder: its name, readable ("Rocket League®" → "Rocket League"), safe as a folder name.
pub fn prefix_name(title: &str) -> String {
    static MARKS: LazyLock<Regex> = LazyLock::new(|| Regex::new("[®™©]").unwrap());
    static BAD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[\\/:*?"<>|]+"#).unwrap());
    static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
    let t = MARKS.replace_all(title, "");
    let t = BAD.replace_all(&t, " ");
    let t = SPACES.replace_all(&t, " ");
    let t = js::slice(js::trim(&t), 100);
    if t.is_empty() {
        "Game".into()
    } else {
        t.to_string()
    }
}

pub fn prefix_for(title: &str, custom: &str, app_root: &Path) -> PathBuf {
    prefix_root(custom, app_root).join(prefix_name(title))
}

// ---------- the environment a game starts with

/// A native game's own home (Settings → PC games → Game data folder): its saves and settings go there, the XDG folders
/// with it. Empty when there's none.
pub fn game_home(dir: &str) -> Vec<(String, String)> {
    if dir.is_empty() || !Path::new(dir).is_absolute() {
        return Vec::new();
    }
    let d = Path::new(dir);
    let x: Vec<(String, String)> = [
        ("HOME", d.to_path_buf()),
        ("XDG_CONFIG_HOME", d.join(".config")),
        ("XDG_DATA_HOME", d.join(".local/share")),
        ("XDG_CACHE_HOME", d.join(".cache")),
        ("XDG_STATE_HOME", d.join(".local/state")),
    ]
    .into_iter()
    .map(|(k, p)| (k.to_string(), p.to_string_lossy().into_owned()))
    .collect();
    for (_, p) in &x {
        let _ = std::fs::create_dir_all(p);
    }
    x
}

fn runtime_dir(app_root: &Path, name: &str, steam_name: &str) -> Option<PathBuf> {
    std::iter::once(tools_dir(app_root).join("runtimes").join(name)).chain(steam_common().into_iter().map(|c| c.join(steam_name))).find(|p| is_dir(p))
}

/// What every Proton start gets: the anti-cheat runtimes (without them a game such as Rocket League waits on an empty
/// Wine window), NVAPI, LD_PRELOAD emptied (the Steam overlay's library from the session) and Steam's ids at 0.
pub fn proton_env(title: &str, app_root: &Path) -> Vec<(String, String)> {
    let mut e: Vec<(String, String)> = [
        ("LD_PRELOAD", String::new()),
        ("STEAM_COMPAT_APP_ID", "0".into()),
        ("SteamAppId", "0".into()),
        ("SteamGameId", format!("rustyshelf-{}", slug(title).replace('-', ""))),
        ("PROTON_ENABLE_NVAPI", "1".into()),
        ("DXVK_NVAPI_ALLOW_OTHER_DRIVERS", "1".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    if let Some(d) = runtime_dir(app_root, "eac_runtime", "Proton EasyAntiCheat Runtime") {
        e.push(("PROTON_EAC_RUNTIME".into(), d.to_string_lossy().into_owned()));
    }
    if let Some(d) = runtime_dir(app_root, "battleye_runtime", "Proton BattlEye Runtime") {
        e.push(("PROTON_BATTLEYE_RUNTIME".into(), d.to_string_lossy().into_owned()));
    }
    e
}

/// The command that runs a Windows program, with the whole environment it gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinCommand {
    pub cmd: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// what runs it, for the log ("GE-Proton10-4 (umu-run)")
    pub how: String,
}

pub struct WinProgram<'a> {
    pub exe: &'a Path,
    pub args: Vec<String>,
    pub cwd: Option<&'a Path>,
    pub runner: Option<&'a Runner>,
    pub prefix: &'a Path,
    pub env: Vec<(String, String)>,
    /// umu's id for the game (protonfixes), and its store ("egs", "none")
    pub game_id: Option<String>,
    pub store: Option<&'a str>,
}

///   proton + umu-run: WINEPREFIX / STEAM_COMPAT_DATA_PATH = the prefix, PROTONPATH = the build
///   proton alone:     <build>/proton waitforexitandrun <exe>
///   wine:             <wine> <exe>, WINEPREFIX = the prefix
pub fn windows_command(p: WinProgram) -> Result<WinCommand, String> {
    let runner = p.runner.ok_or("No Wine or Proton found. Install Wine, or a Proton build (GE-Proton) with Steam or ProtonUp-Qt.")?;
    std::fs::create_dir_all(p.prefix).map_err(|e| format!("Can't make the Wine prefix {}: {e}", p.prefix.display()))?;
    let dir = p.cwd.map(Path::to_path_buf).unwrap_or_else(|| p.exe.parent().map(Path::to_path_buf).unwrap_or_default());
    let set = |env: &mut Vec<(String, String)>, k: &str, v: String| {
        env.retain(|(e, _)| e != k);
        env.push((k.to_string(), v));
    };
    let mut env = crate::launch::clean_env(Some(p.env));
    let exe = p.exe.to_string_lossy().into_owned();
    let prefix = p.prefix.to_string_lossy().into_owned();
    if runner.proton {
        set(&mut env, "WINEPREFIX", prefix.clone());
        set(&mut env, "STEAM_COMPAT_DATA_PATH", prefix.clone());
        set(&mut env, "STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root().map_or(prefix.clone(), |s| s.to_string_lossy().into_owned()));
        set(&mut env, "STEAM_COMPAT_INSTALL_PATH", dir.to_string_lossy().into_owned());
        set(&mut env, "PROTONPATH", runner.path.to_string_lossy().into_owned());
        if let Some(umu) = umu_run() {
            set(&mut env, "GAMEID", p.game_id.unwrap_or_else(|| "umu-0".into()));
            set(&mut env, "STORE", p.store.unwrap_or("none").to_string());
            if !env.iter().any(|(k, v)| k == "UMU_RUNTIME_UPDATE" && !v.is_empty()) {
                set(&mut env, "UMU_RUNTIME_UPDATE", "1".into());
            }
            return Ok(WinCommand { cmd: umu, args: std::iter::once(exe).chain(p.args).collect(), cwd: dir, env, how: format!("{} (umu-run)", runner.name) });
        }
        let args = ["waitforexitandrun".to_string(), exe].into_iter().chain(p.args).collect();
        return Ok(WinCommand { cmd: runner.path.join("proton"), args, cwd: dir, env, how: runner.name.clone() });
    }
    set(&mut env, "WINEPREFIX", prefix);
    Ok(WinCommand { cmd: runner.path.clone(), args: std::iter::once(exe).chain(p.args).collect(), cwd: dir, env, how: runner.name.clone() })
}

/// umu-run's Steam Linux Runtime isn't there yet: the first Proton start downloads it (about 700 MB) before the game
/// shows.
pub fn umu_needs_runtime() -> bool {
    if umu_run().is_none() {
        return false;
    }
    match std::fs::read_dir(home().join(".local/share/umu")) {
        Ok(rd) => !rd.flatten().any(|e| e.path().is_dir() && e.file_name().to_string_lossy().starts_with("steamrt")),
        Err(_) => true,
    }
}

/// The wineserver that ends what's left in a prefix, and the prefix it's told.
pub fn wineserver(runner: &Runner, prefix: &Path) -> Option<(PathBuf, PathBuf)> {
    let ws = if runner.proton { runner.path.join("files/bin/wineserver") } else { runner.path.parent()?.join("wineserver") };
    let pfx = if runner.proton && prefix.join("pfx").is_dir() { prefix.join("pfx") } else { prefix.to_path_buf() };
    is_exec(&ws).then_some((ws, pfx))
}

/// Ends whatever is left in a prefix (wineserver) once its game has closed or been forced.
pub fn end_prefix(runner: &Runner, prefix: &Path) {
    let Some((ws, pfx)) = wineserver(runner, prefix) else { return };
    if let Ok(mut c) = Command::new(&ws).arg("-k").env("WINEPREFIX", pfx).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        std::thread::spawn(move || {
            let end = Instant::now() + Duration::from_secs(10);
            while Instant::now() < end && matches!(c.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = c.kill();
            let _ = c.wait();
        });
    }
}

pub fn is_windows_program(exe: &str) -> bool {
    let l = exe.to_lowercase();
    [".exe", ".msi", ".bat", ".lnk"].iter().any(|e| l.ends_with(e))
}

/// umu's game id for protonfixes, asked once per game and kept in data/umu-ids.json (a miss is asked again after a
/// week).
pub fn umu_id(data_dir: &Path, store: &str, codename: &str) -> Option<String> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^umu-[\w.-]{1,40}$").unwrap());
    let file = data_dir.join("umu-ids.json");
    let mut ids = read_json(&file).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let k = format!("{store}:{codename}");
    if let Some(e) = ids.get(&k) {
        let id = js::filled(e.get("id")).map(str::to_string);
        if id.is_some() || now_ms().saturating_sub(e.get("at").and_then(Value::as_u64).unwrap_or(0)) < 7 * 86_400_000 {
            return id;
        }
    }
    let url = format!("https://umu.openwinecomponents.org/umu_api.php?{}", js::query(&[("store", store.to_string()), ("codename", codename.to_string())]));
    let id = crate::net::ask(&url, &[("User-Agent", "RustyShelf")], Duration::from_secs(4), 100_000).ok().and_then(|(_, _, b)| serde_json::from_slice::<Value>(&b).ok()).and_then(|j| {
        let x = if j.is_array() { j.get(0).cloned() } else { Some(j) }?;
        js::str_of(x.get("umu_id")).filter(|i| ID.is_match(i)).map(str::to_string)
    });
    ids[&k] = json!({ "id": id, "at": now_ms() });
    let _ = std::fs::write(&file, ids.to_string());
    id
}

// ---------- legendary (Epic Games)

pub const LEGENDARY_VERSION: &str = "0.21.1";
const LEGENDARY_URL: &str = "https://github.com/derrod/legendary/releases/download/0.21.1/legendary_linux_x64";
const LEGENDARY_SHA256: &str = "dbed33bbe96031e65858233e4badf0a1d401bb6cb0cc995d1237cf3a0c826a45";

/// legendary itself: the one chosen in Settings, one on the PATH, then RustyShelf's own tools/legendary.
pub fn legendary_bin(custom: &str, app_root: &Path) -> Option<PathBuf> {
    if !custom.is_empty() && is_exec(Path::new(custom)) {
        return Some(PathBuf::from(custom));
    }
    which("legendary").or_else(|| Some(tools_dir(app_root).join("legendary")).filter(|p| is_exec(p)))
}

/// Settings → PC games → Get legendary: its release from GitHub, checked against the SHA-256 GitHub publishes for it.
pub fn get_legendary(app_root: &Path) -> Result<PathBuf, String> {
    use sha2::Digest;
    let buf = crate::net::get_bytes(LEGENDARY_URL, Duration::from_secs(300), 200 << 20).ok().flatten().ok_or("Couldn't download legendary. Check the internet connection.")?;
    let hash: String = sha2::Sha256::digest(&buf).iter().map(|b| format!("{b:02x}")).collect();
    if hash != LEGENDARY_SHA256 {
        return Err("The legendary download was damaged. Try again.".into());
    }
    let dest = tools_dir(app_root).join("legendary");
    let tmp = tools_dir(app_root).join("legendary.tmp");
    std::fs::create_dir_all(tools_dir(app_root)).and_then(|_| std::fs::write(&tmp, &buf)).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// legendary's folder: RustyShelf's Epic sign-in and install list.
pub fn legendary_dir(data_dir: &Path) -> PathBuf {
    match std::env::var("LEGENDARY_CONFIG_PATH") {
        Ok(d) if Path::new(&d).is_dir() => PathBuf::from(d),
        _ => data_dir.join("legendary"),
    }
}

/// A legendary run that can be stopped from another thread.
#[derive(Clone)]
pub struct Proc(Arc<Mutex<Child>>);

impl Proc {
    /// SIGTERM, so legendary keeps what it downloaded and goes on from there next time.
    pub fn stop(&self) {
        let pid = self.0.lock().map(|c| c.id()).unwrap_or(0);
        if pid == 0 {
            return;
        }
        #[cfg(unix)]
        {
            let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        #[cfg(not(unix))]
        {
            let _ = self.0.lock().map(|mut c| c.kill());
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ran {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// Runs legendary with RustyShelf's own folder and waits for it; `limit`: it's stopped after that long. Each line it
/// writes (on either stream, split at a carriage return too) goes to `on_line` as it comes.
pub fn run_legendary(bin: &Path, args: &[String], data_dir: &Path, limit: Option<Duration>, on_line: &mut dyn FnMut(&str), on_start: &mut dyn FnMut(Proc)) -> Ran {
    let dir = legendary_dir(data_dir);
    let _ = std::fs::create_dir_all(&dir);
    let env = crate::launch::clean_env(Some(vec![("LEGENDARY_CONFIG_PATH".into(), dir.to_string_lossy().into_owned()), ("PYTHONUNBUFFERED".into(), "1".into())]));
    let mut cmd = Command::new(bin);
    cmd.args(args).env_clear().envs(env).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Ran { code: -1, out: String::new(), err: e.to_string() },
    };
    let (tx, rx) = channel::<(bool, String)>();
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter()
        .enumerate()
        .filter_map(|(i, s)| Some((i == 1, s?)))
        .map(|(is_err, s)| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut r = std::io::BufReader::new(s);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    // a line ends at \n; legendary's progress also ends at \r
                    match r.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let text = String::from_utf8_lossy(&buf).into_owned();
                            for part in text.split('\r') {
                                let _ = tx.send((is_err, part.trim_end_matches('\n').to_string()));
                            }
                        }
                    }
                }
            })
        })
        .collect();
    drop(tx);
    let proc = Proc(Arc::new(Mutex::new(child)));
    on_start(proc.clone());
    let started = Instant::now();
    let (mut out, mut err) = (String::new(), String::new());
    let mut stopped = false;
    let code = loop {
        while let Ok((is_err, line)) = rx.recv_timeout(Duration::from_millis(50)) {
            let into = if is_err { &mut err } else { &mut out };
            if into.len() < if is_err { 2_000_000 } else { 32_000_000 } {
                into.push_str(&line);
                into.push('\n');
            }
            if !line.is_empty() {
                on_line(&line);
            }
        }
        let status = proc.0.lock().ok().and_then(|mut c| c.try_wait().ok().flatten());
        if let Some(s) = status {
            break s.code().unwrap_or(-1);
        }
        if !stopped && limit.is_some_and(|l| started.elapsed() > l) {
            stopped = true;
            proc.stop();
        }
    };
    for r in readers {
        let _ = r.join();
    }
    while let Ok((is_err, line)) = rx.try_recv() {
        let into = if is_err { &mut err } else { &mut out };
        into.push_str(&line);
        into.push('\n');
        if !line.is_empty() {
            on_line(&line);
        }
    }
    Ran { code, out, err }
}

fn legendary(bin: &Path, args: &[&str], data_dir: &Path, limit: Duration) -> Ran {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    run_legendary(bin, &args, data_dir, Some(limit), &mut |_| {}, &mut |_| {})
}

/// The signed-in Epic account's name (never its tokens).
pub fn epic_account(data_dir: &Path) -> Option<String> {
    let u = read_json(&legendary_dir(data_dir).join("user.json"))?;
    if !js::truthy(u.get("displayName")) && !js::truthy(u.get("account_id")) {
        return None;
    }
    let name = js::text(u.get("displayName"));
    Some(js::slice(if name.is_empty() { "Epic account" } else { &name }, 80).to_string())
}

/// Epic's own box art among a game's store pictures: the tall box, else the wide one; "" when there's none.
pub fn box_art(key_images: Option<&Value>) -> String {
    let imgs = key_images.and_then(Value::as_array).cloned().unwrap_or_default();
    let pick = |t: &str| imgs.iter().find(|i| i.get("type").and_then(Value::as_str) == Some(t) && i.get("url").and_then(Value::as_str).is_some_and(|u| u.starts_with("https://"))).and_then(|i| i["url"].as_str()).map(str::to_string);
    pick("DieselGameBoxTall").or_else(|| pick("OfferImageTall")).or_else(|| pick("DieselGameBox")).unwrap_or_default()
}

fn epic_cover(data_dir: &Path, app: &str) -> String {
    box_art(read_json(&legendary_dir(data_dir).join("metadata").join(format!("{app}.json"))).as_ref().and_then(|m| m.get("metadata")).and_then(|m| m.get("keyImages")))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpicGame {
    pub id: String,
    pub installed: bool,
    pub title: String,
    pub app_name: String,
    pub dir: String,
    pub exe: String,
    pub platform: String,
    pub cover_url: String,
    pub install_size: u64,
}

/// The installed Epic games: legendary's installed.json (read, never written); DLC and games whose folder is gone left
/// out.
pub fn epic_installed(data_dir: &Path) -> Vec<EpicGame> {
    let Some(Value::Object(j)) = read_json(&legendary_dir(data_dir).join("installed.json")) else { return Vec::new() };
    let mut out = Vec::new();
    for (app, g) in j.iter().take(2000) {
        if !g.is_object() || js::truthy(g.get("is_dlc")) || !is_app_name(app) {
            continue;
        }
        let dir = js::text(g.get("install_path"));
        if dir.is_empty() || !is_dir(Path::new(&dir)) {
            continue;
        }
        let exe = js::text(g.get("executable"));
        out.push(EpicGame {
            id: format!("epic-{}", slug(app)),
            installed: true,
            title: js::slice(js::trim(&js::text(js::first(&[g.get("title"), Some(&json!(app))]))), 120).to_string(),
            app_name: app.clone(),
            exe: if exe.is_empty() { String::new() } else { Path::new(&dir).join(&exe).to_string_lossy().into_owned() },
            dir,
            platform: js::filled(g.get("platform")).unwrap_or("Windows").to_string(),
            cover_url: epic_cover(data_dir, app),
            install_size: js::number(g.get("install_size")).max(0.0) as u64,
        });
    }
    out.sort_by(|a, b| crate::collate::compare_exact(&a.title, &b.title));
    out
}

fn library_file(data_dir: &Path) -> PathBuf {
    data_dir.join("epic-library.json")
}

/// How long ago the Epic library was last read (ms); None for never.
pub fn library_age(data_dir: &Path) -> Option<u64> {
    let at = read_json(&library_file(data_dir))?.get("at")?.as_u64()?;
    Some(now_ms().saturating_sub(at))
}

/// Every game the account owns, as last read from Epic, installed or not.
pub fn epic_owned(data_dir: &Path) -> Vec<EpicGame> {
    let inst = epic_installed(data_dir);
    let have: std::collections::HashSet<String> = inst.iter().map(|g| g.app_name.clone()).collect();
    let saved = read_json(&library_file(data_dir));
    let rest = saved.as_ref().and_then(|c| c.get("games")).and_then(Value::as_array).cloned().unwrap_or_default().into_iter().filter_map(|g| {
        let app = js::text(g.get("appName"));
        (is_app_name(&app) && !have.contains(&app)).then(|| EpicGame {
            id: format!("epic-{}", slug(&app)),
            installed: false,
            title: js::slice(&js::text(js::first(&[g.get("title"), Some(&json!(app))])), 120).to_string(),
            cover_url: js::str_of(g.get("cover")).unwrap_or("").to_string(),
            app_name: app,
            ..EpicGame::default()
        })
    });
    let mut all: Vec<EpicGame> = inst.into_iter().chain(rest).collect();
    all.sort_by(|a, b| crate::collate::compare_exact(&a.title, &b.title));
    all
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owned {
    pub app_name: String,
    pub title: String,
    pub cover: String,
    pub installed: bool,
}

/// Asks Epic for the account's library (legendary list) and keeps it.
pub fn epic_library(bin: &Path, data_dir: &Path) -> Result<Vec<Owned>, String> {
    let r = legendary(bin, &["list", "--json"], data_dir, Duration::from_secs(120));
    let signed_out = || "Sign in to Epic Games first.".to_string();
    if r.code != 0 {
        return Err(if epic_account(data_dir).is_some() { "Couldn't get your Epic library. Check the internet connection.".into() } else { signed_out() });
    }
    let list: Value = serde_json::from_str(r.out.trim()).map_err(|_| "Couldn't read the Epic library.".to_string())?;
    let mut games: Vec<Value> = Vec::new();
    for g in list.as_array().map(|a| a.iter().take(5000).collect::<Vec<_>>()).unwrap_or_default() {
        let app = js::text(g.get("app_name"));
        if !is_app_name(&app) {
            continue;
        }
        let title = js::slice(&js::text(js::first(&[g.get("app_title"), Some(&json!(app))])), 120).to_string();
        games.push(json!({ "appName": app, "title": title, "cover": box_art(g.get("metadata").and_then(|m| m.get("keyImages"))) }));
    }
    games.sort_by(|a, b| crate::collate::compare_exact(a["title"].as_str().unwrap_or(""), b["title"].as_str().unwrap_or("")));
    if epic_account(data_dir).is_none() {
        return Err(signed_out());
    }
    let _ = write_atomic(&library_file(data_dir), &json!({ "at": now_ms(), "games": games }).to_string());
    let have: std::collections::HashSet<String> = epic_installed(data_dir).into_iter().map(|g| g.app_name).collect();
    Ok(games
        .iter()
        .map(|g| {
            let app = g["appName"].as_str().unwrap_or("").to_string();
            Owned { installed: have.contains(&app), app_name: app, title: g["title"].as_str().unwrap_or("").into(), cover: g["cover"].as_str().unwrap_or("").into() }
        })
        .collect())
}

/// Signs in with the code from Epic's sign-in page; the account's name.
pub fn epic_sign_in(bin: &Path, data_dir: &Path, code: &str) -> Result<String, String> {
    static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\w-]{8,200}$").unwrap());
    if !CODE.is_match(code) {
        return Err("Epic didn't give a sign-in code. Try again.".into());
    }
    let r = legendary(bin, &["auth", "--code", code], data_dir, Duration::from_secs(60));
    match epic_account(data_dir) {
        Some(name) if r.code == 0 => Ok(name),
        _ => Err("Couldn't sign in to Epic Games. Try again in a moment.".into()),
    }
}

/// Signs out and forgets the library; true when no account is left.
pub fn epic_sign_out(bin: &Path, data_dir: &Path) -> bool {
    legendary(bin, &["auth", "--delete"], data_dir, Duration::from_secs(30));
    let _ = std::fs::remove_file(library_file(data_dir));
    epic_account(data_dir).is_none()
}

fn unfinished_file(data_dir: &Path) -> PathBuf {
    data_dir.join("epic-unfinished.json")
}

/// The folders of Epic installs stopped half way (legendary picks them up next time), kept in
/// data/epic-unfinished.json: `{ "<folder>": "<app name>" }`. They aren't games of the PC games folder. `running`: the
/// folders of the install going on now.
pub fn epic_unfinished(data_dir: &Path, running: Vec<String>) -> Vec<String> {
    let m = read_json(&unfinished_file(data_dir)).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    let done: std::collections::HashSet<PathBuf> = epic_installed(data_dir).into_iter().map(|g| crate::art::resolve(Path::new(&g.dir))).collect();
    m.keys().filter(|d| !done.contains(&crate::art::resolve(Path::new(d)))).cloned().chain(running).collect()
}

/// An install stopped (its new folders are noted) or finished (its folders are forgotten).
pub fn note_unfinished(data_dir: &Path, dirs: &[String], app_name: &str, finished: bool) {
    let f = unfinished_file(data_dir);
    let mut m = read_json(&f).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    if finished {
        m.retain(|_, a| a.as_str() != Some(app_name));
    } else {
        for d in dirs {
            m.insert(d.clone(), json!(app_name));
        }
    }
    let _ = std::fs::write(&f, Value::Object(m).to_string());
}

/// Where Epic games install: the folder chosen for them, else the PC games folder, else <RustyShelf>/Epic Games.
pub fn epic_install_dir(custom: &str, games_dir: &str, app_root: &Path) -> PathBuf {
    [custom, games_dir].into_iter().find(|d| !d.is_empty() && Path::new(d).is_absolute()).map_or_else(|| app_root.join("Epic Games"), PathBuf::from)
}

pub fn epic_uninstall(bin: &Path, data_dir: &Path, app_name: &str) -> Result<bool, String> {
    if !is_app_name(app_name) {
        return Err("Unknown Epic game.".into());
    }
    Ok(legendary(bin, &["uninstall", app_name, "-y"], data_dir, Duration::from_secs(300)).code == 0)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpicLaunch {
    pub exe: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    pub offline: bool,
    pub outdated: bool,
}

/// How to start an installed Epic game: legendary hands over its program and Epic's sign-in arguments (online; an
/// update waiting is skipped for now; offline when Epic can't be reached and the game allows it).
pub fn epic_launch_info(bin: &Path, data_dir: &Path, app_name: &str) -> Result<EpicLaunch, String> {
    static OUTDATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)out of date").unwrap());
    static VAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{0,80}$").unwrap());
    let outdated = std::cell::Cell::new(false);
    let ask = |off: bool, skip: bool| -> Option<Value> {
        let mut args = vec!["launch", app_name, "--json"];
        if off {
            args.push("--offline");
        }
        if skip {
            args.push("--skip-version-check");
        }
        let r = legendary(bin, &args, data_dir, Duration::from_secs(45));
        if r.code != 0 {
            outdated.set(outdated.get() || OUTDATED.is_match(&r.err));
            return None;
        }
        serde_json::from_str(r.out.trim()).ok()
    };
    let mut off = epic_account(data_dir).is_none();
    let mut j = ask(off, false);
    if j.is_none() && !off && outdated.get() {
        j = ask(false, true);
    }
    if j.is_none() && !off {
        off = true;
        j = ask(true, false);
    }
    let j = j.filter(|j| js::truthy(j.get("game_executable")) && js::truthy(j.get("game_directory"))).ok_or_else(|| {
        if epic_account(data_dir).is_some() { "Couldn't get the game ready with Epic. Check the internet connection, or that the game is still installed.".to_string() } else { "Sign in to Epic Games in Settings → PC games first.".to_string() }
    })?;
    let strs = |k: &str| js::items(j.get(k)).into_iter().map(|v| js::string(Some(v))).filter(|s| !s.is_empty()).take(200).collect::<Vec<_>>();
    let env: HashMap<String, String> = j.get("environment").and_then(Value::as_object).map(|o| o.iter().filter(|(k, _)| VAR.is_match(k)).map(|(k, v)| (k.clone(), js::string(Some(v)))).collect()).unwrap_or_default();
    let mut env: Vec<(String, String)> = env.into_iter().collect();
    env.sort();
    let dir = js::string(j.get("game_directory"));
    Ok(EpicLaunch {
        exe: Path::new(&dir).join(js::string(j.get("game_executable"))),
        args: [strs("game_parameters"), strs("egl_parameters"), strs("user_parameters")].concat(),
        cwd: PathBuf::from(js::text(js::first(&[j.get("working_directory"), j.get("game_directory")]))),
        env,
        offline: off,
        outdated: outdated.get(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fake(dir: &Path, script: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let f = dir.join("legendary");
        std::fs::write(&f, format!("#!/bin/sh\n{script}")).unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        f
    }

    #[test]
    fn names() {
        assert_eq!(prefix_name("Rocket League®: Deluxe / \"x\""), "Rocket League Deluxe x");
        assert_eq!(prefix_name("™"), "Game");
        assert!(ver_key("GE-Proton11-5") > ver_key("GE-Proton10-4"));
        assert!(is_app_name("Fortnite.v2-x") && !is_app_name("bad name; rm"));
        assert!(is_windows_program("a/Game.EXE") && !is_windows_program("game.sh"));
    }

    #[test]
    fn picks_a_runner() {
        let r = |id: &str, proton: bool, name: &str| Runner { id: id.into(), proton, name: name.into(), path: PathBuf::new() };
        let list = [r("wine:/w", false, "Wine (system)"), r("proton:/p", true, "Proton 9.0"), r("proton:/g", true, "GE-Proton10-4")];
        assert_eq!(pick_runner("", &list).unwrap().id, "proton:/g");
        assert_eq!(pick_runner("wine:/w", &list).unwrap().id, "wine:/w");
        assert_eq!(pick_runner("gone", &list[..2]).unwrap().id, "proton:/p");
    }

    #[test]
    fn commands() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("g/Game.exe");
        let prefix = tmp.path().join("Prefixes/Game");
        let wine = Runner { id: "wine:/usr/bin/wine".into(), proton: false, name: "Wine".into(), path: "/usr/bin/wine".into() };
        let c = windows_command(WinProgram { exe: &exe, args: vec!["-x".into()], cwd: None, runner: Some(&wine), prefix: &prefix, env: vec![("A".into(), "1".into())], game_id: None, store: None }).unwrap();
        assert_eq!((c.cmd, c.args, c.cwd), (PathBuf::from("/usr/bin/wine"), vec![exe.to_string_lossy().into_owned(), "-x".into()], tmp.path().join("g")));
        assert!(c.env.contains(&("WINEPREFIX".into(), prefix.to_string_lossy().into_owned())) && c.env.contains(&("A".into(), "1".into())) && prefix.is_dir());
        assert!(windows_command(WinProgram { exe: &exe, args: vec![], cwd: None, runner: None, prefix: &prefix, env: vec![], game_id: None, store: None }).is_err());
        let env = proton_env("Test Epic Game", tmp.path());
        assert!(env.contains(&("SteamGameId".into(), "rustyshelf-testepicgame".into())) && env.contains(&("LD_PRELOAD".into(), String::new())));
    }

    #[test]
    fn legendary_through_a_pretend_one() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let leg = legendary_dir(&data);
        let game = tmp.path().join("Games/Test");
        std::fs::create_dir_all(game.join("Binaries")).unwrap();
        let bin = fake(
            tmp.path(),
            &format!(
                r#"echo "$LEGENDARY_CONFIG_PATH" > "{t}/cfg.txt"
case "$1" in
  list) echo '[{{"app_name":"FakeApp","app_title":"Test Epic Game","metadata":{{"keyImages":[{{"type":"DieselGameBox","url":"https://x/wide.jpg"}},{{"type":"DieselGameBoxTall","url":"https://x/tall.jpg"}}]}}}},{{"app_name":"Other","app_title":"Not Installed Game"}},{{"app_name":"bad name"}}]' ;;
  auth) [ "$2" = --delete ] && rm -f "$LEGENDARY_CONFIG_PATH/user.json" || echo '{{"displayName":"Tester"}}' > "$LEGENDARY_CONFIG_PATH/user.json" ;;
  launch) [ "$4" = "" ] && {{ echo "ERROR: game is out of date" >&2; exit 1; }}; echo '{{"game_executable":"Binaries/Game.exe","game_directory":"{g}","working_directory":"{g}/Binaries","game_parameters":["-windowed"],"egl_parameters":["-AUTH_PASSWORD=fake-code"],"environment":{{"FROM_LEGENDARY":"1","bad var":"x"}}}}' ;;
esac"#,
                t = tmp.path().display(),
                g = game.display()
            ),
        );
        assert_eq!(epic_account(&data), None);
        assert!(epic_sign_in(&bin, &data, "short").is_err());
        assert_eq!(epic_sign_in(&bin, &data, "abcdefgh1234"), Ok("Tester".into()));
        assert_eq!(std::fs::read_to_string(tmp.path().join("cfg.txt")).unwrap().trim(), leg.to_string_lossy());
        std::fs::write(leg.join("installed.json"), json!({ "FakeApp": { "title": "Test Epic Game", "install_path": game, "executable": "Binaries/Game.exe", "install_size": 1234 }, "Dlc": { "is_dlc": true, "install_path": game } }).to_string()).unwrap();
        let lib = epic_library(&bin, &data).unwrap();
        assert_eq!(lib.iter().map(|g| (g.app_name.as_str(), g.installed)).collect::<Vec<_>>(), vec![("Other", false), ("FakeApp", true)]);
        assert_eq!(lib.iter().find(|g| g.app_name == "FakeApp").unwrap().cover, "https://x/tall.jpg");
        assert!(library_age(&data).is_some_and(|a| a < 60_000));
        let owned = epic_owned(&data);
        assert_eq!(owned.iter().map(|g| (g.title.as_str(), g.installed)).collect::<Vec<_>>(), vec![("Not Installed Game", false), ("Test Epic Game", true)]);
        assert_eq!((owned[1].id.as_str(), owned[1].install_size), ("epic-fakeapp", 1234));
        let l = epic_launch_info(&bin, &data, "FakeApp").unwrap();
        assert_eq!(l.exe, game.join("Binaries/Game.exe"));
        assert_eq!(l.args, vec!["-windowed".to_string(), "-AUTH_PASSWORD=fake-code".into()]);
        assert_eq!((l.cwd, l.env, l.offline, l.outdated), (game.join("Binaries"), vec![("FROM_LEGENDARY".into(), "1".into())], false, true));
        note_unfinished(&data, &["/x/half".into()], "Half", false);
        assert_eq!(epic_unfinished(&data, vec!["/x/now".into()]), vec!["/x/half".to_string(), "/x/now".into()]);
        note_unfinished(&data, &[], "Half", true);
        assert!(epic_unfinished(&data, vec![]).is_empty());
        assert!(epic_sign_out(&bin, &data) && !library_file(&data).exists());
        assert_eq!(epic_install_dir("", "/games", tmp.path()), PathBuf::from("/games"));
        assert_eq!(epic_install_dir("rel", "", tmp.path()), tmp.path().join("Epic Games"));
    }
}

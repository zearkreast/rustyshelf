// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/launcher.js, the game launch in main.js, main/emucfg.js (resolveExe, launchArgs), main/linux.js
// (retroCore) and main/linuxpc.js (cleanEnv), as RustyShelf has them; main/vita3k.js is RustyShelf's. Waking a Game
// streaming host is from ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork
// of Boxshelf: ported from its main.js.
//! Starting a game: which emulator, with which options, and the per-console steps the Electron app takes.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use crate::config::{Config, SystemConfig};
use crate::paths::{expand, is_file};
use crate::pcgames::PcGame;
use crate::scanner::read_dir_sorted;
use crate::systems::{systems, Console};
use crate::{kyty, nx2};

static ARG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)"|(\S+)"#).unwrap());

/// Splits launch options like `-b -e "{rom}"` into arguments, `{rom}` filled in; `{romname}` is the game's file name
/// without its extension, `{romdir}` its folder.
pub fn build_args(template: &str, rom: &str) -> Vec<String> {
    let p = Path::new(rom);
    let name = if rom.is_empty() { String::new() } else { p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
    let dir = if rom.is_empty() { String::new() } else { p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default() };
    ARG.captures_iter(template)
        .map(|c| c.get(1).or(c.get(2)).map_or("", |m| m.as_str()).replace("{romname}", &name).replace("{romdir}", &dir).replace("{rom}", rom))
        .filter(|a| !a.is_empty())
        .collect()
}

#[derive(Debug)]
pub enum LaunchError {
    /// the game can't be started as things are, and why (shown as it is)
    Refused(String),
    Spawn(std::io::Error),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::Refused(why) => f.write_str(why),
            LaunchError::Spawn(e) => write!(f, "The emulator didn't start: {e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

fn refuse<T>(why: impl Into<String>) -> Result<T, LaunchError> {
    Err(LaunchError::Refused(why.into()))
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A program as Settings has it: `%VAR%`, `{app}`, and "quotes" pasted around a path.
pub fn emu_path(exe: &str, app_root: &Path) -> PathBuf {
    let t = exe.trim();
    let t = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(t);
    PathBuf::from(expand(t, app_root))
}

/// What a launch runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub exe: PathBuf,
    pub args: Vec<String>,
    /// the whole environment the program gets; None for RustyShelf's own
    pub env: Option<Vec<(String, String)>>,
}

const PS4_PKG: &str = "This PS4 game is still a .pkg package, which shadPS4 can't run. Extract it to a folder first (current \
shadPS4 builds no longer install packages themselves) with a PKG extractor such as PKG Editor or the ShadPS4Plus \
pkg_extractor, into a folder inside your PS4 games folder. Once that folder has an eboot.bin in it, the game shows up here as \
installed.";

/// The emulator a game runs in: its own build (Details → Emulator), else its console's, else RetroArch for a retro
/// console.
fn choose_emulator(console: &Console, sc: &SystemConfig, game_id: &str, cfg: &Config, app_root: &Path) -> Result<PathBuf, LaunchError> {
    if let Some(own) = cfg.game_emulator(game_id) {
        let own = PathBuf::from(expand(own, app_root));
        if !is_file(&own) {
            return refuse(format!(
                "This game's own emulator isn't there any more ({}). Choose another in its Details → Emulator, or go back to the shelf's.",
                own.display()
            ));
        }
        return Ok(own);
    }
    let mut e = sc.emulator.clone();
    if e.is_empty() && console.retro {
        e = cfg.retroarch();
    }
    Ok(if e.is_empty() { PathBuf::new() } else { emu_path(&e, app_root) })
}

/// Xenia Manager is a launcher around Xenia that can't take a game on its command line: the real emulator beside it,
/// Canary first.
pub fn find_xenia(exe: &Path) -> Option<PathBuf> {
    static XENIA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^xenia.*\.exe$").unwrap());
    static MANAGER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)manager").unwrap());
    static SKIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(cache|content|patches|config|logs?)").unwrap());
    fn walk(d: &Path, depth: u32, found: &mut Vec<PathBuf>) {
        for e in read_dir_sorted(d) {
            let (name, path) = (e.file_name().to_string_lossy().into_owned(), e.path());
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                if depth > 0 && !SKIP.is_match(&name) {
                    walk(&path, depth - 1, found);
                }
            } else if XENIA.is_match(&name) && !MANAGER.is_match(&name) {
                found.push(path);
            }
        }
    }
    let dir = exe.parent()?;
    let mut found = Vec::new();
    for r in [dir.to_path_buf(), dir.join("Emulators")] {
        walk(&r, 2, &mut found);
    }
    let prefer = ["xenia_canary.exe", "xenia.exe", "xenia_canary_mousehook.exe", "xenia_canary_netplay.exe"];
    let rank = |p: &PathBuf| prefer.iter().position(|n| *n == name_of(p).to_lowercase()).unwrap_or(9);
    found.sort_by_key(rank);
    found.into_iter().next()
}

/// The real Xenia when Settings names something else in its folder.
fn resolve_xenia(exe: PathBuf) -> PathBuf {
    static REAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^xenia(_canary)?(_mousehook|_netplay)?\.exe$").unwrap());
    if exe.as_os_str().is_empty() || REAL.is_match(&name_of(&exe)) {
        return exe;
    }
    find_xenia(&exe).unwrap_or(exe)
}

/// PS Vita in Vita3K: an installed game (ux0/app/<title id>) is started by its title id, a .vpk is installed and run.
pub fn vita3k_spec(file: &Path) -> Result<(String, &'static str), LaunchError> {
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{4}[0-9]{5}$").unwrap());
    static ID_IN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([A-Z]{4}[0-9]{5})").unwrap());
    if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("vpk")) {
        return Ok((file.to_string_lossy().into_owned(), r#"-F "{rom}""#));
    }
    let dir = file.parent().unwrap_or(Path::new(""));
    let folder = name_of(dir);
    let id = if ID.is_match(&folder) {
        Some(folder)
    } else {
        std::fs::read(dir.join("sce_sys").join("param.sfo"))
            .ok()
            .and_then(|b| ID_IN.captures(&b.iter().map(|&c| c as char).collect::<String>()).map(|c| c[1].to_string()))
    };
    match id {
        Some(id) => Ok((id, r#"-F -r "{rom}""#)),
        None => refuse("This PS Vita game's title id couldn't be read (its folder should be named like PCSE00000). Install it in Vita3K first."),
    }
}

/// The standalone emulators a retro console can use instead of the one its options are for, and the options each takes.
fn alt_args(exe_name: &str) -> Option<&'static str> {
    static ALT: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"(?i)desmume", r#""{rom}""#),
            (r"(?i)visualboyadvance|vbam", r#""{rom}""#),
            (r"(?i)mgba", r#"-f "{rom}""#),
            (r"(?i)melonds", r#"-f "{rom}""#),
            (r"(?i)duckstation", r#"-fullscreen -batch -nogui -- "{rom}""#),
            (r"(?i)emuhawk", r#"--fullscreen "{rom}""#),
            (r"(?i)gopher64", r#"--fullscreen "{rom}""#),
            (r"(?i)^(rap64|project64)", r#""{rom}""#),
            (r"(?i)flycast", r#""{rom}""#),
        ]
        .into_iter()
        .map(|(re, a)| (Regex::new(re).unwrap(), a))
        .collect()
    });
    ALT.iter().find(|(re, _)| re.is_match(exe_name)).map(|(_, a)| *a)
}

pub fn is_retroarch(exe: &Path) -> bool {
    static LINUX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(retroarch|org\.libretro\.retroarch|retroarch.*\.appimage)$").unwrap());
    let n = name_of(exe);
    if cfg!(target_os = "linux") {
        LINUX.is_match(&n)
    } else {
        n.eq_ignore_ascii_case("retroarch.exe")
    }
}

fn core_name(id: &str) -> String {
    const NAMES: [(&str, &str); 18] = [
        ("swanstation", "SwanStation"),
        ("mesen", "Mesen"),
        ("snes9x", "Snes9x"),
        ("mupen64plus_next", "Mupen64Plus-Next"),
        ("gambatte", "Gambatte"),
        ("mgba", "mGBA"),
        ("melondsds", "melonDS DS"),
        ("genesis_plus_gx", "Genesis Plus GX"),
        ("flycast", "Flycast"),
        ("mednafen_saturn", "Beetle Saturn"),
        ("mednafen_pce_fast", "Beetle PCE Fast"),
        ("fbneo", "FinalBurn Neo"),
        ("picodrive", "PicoDrive"),
        ("stella", "Stella"),
        ("handy", "Handy"),
        ("mednafen_wswan", "Beetle Cygne"),
        ("mednafen_ngp", "Beetle NeoPop"),
        ("dosbox_pure", "DOSBox Pure"),
    ];
    NAMES
        .iter()
        .find(|(k, _)| *k == id)
        .map(|(_, n)| n.to_string())
        .or_else(|| systems().consoles.iter().flat_map(|c| &c.ra_cores).find(|(k, _)| k == id).map(|(_, n)| n.clone()))
        .unwrap_or_else(|| id.to_string())
}

/// The core a console runs in: the one picked in Settings when the console offers it, else its own.
pub fn core_of(console: &Console, sc: &SystemConfig) -> Option<String> {
    if !sc.ra_core.is_empty() && console.ra_cores.iter().any(|(id, _)| *id == sc.ra_core) {
        return Some(sc.ra_core.clone());
    }
    console.ra_core.clone()
}

static LIBRETRO_DIR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?m)^\s*libretro_directory\s*=\s*"([^"]*)""#).unwrap());

fn home() -> PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default()
}

/// A RetroArch core's file: the folder its settings name, its usual folders, and (Linux) the distribution's packaged
/// cores.
pub fn retro_core(exe: &Path, core: &str) -> Option<PathBuf> {
    let dir = exe.parent().unwrap_or(Path::new("")).to_path_buf();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "linux") {
        let home = home();
        let cfg_dirs = [home.join(".config/retroarch"), home.join(".var/app/org.libretro.RetroArch/config/retroarch"), dir.clone()];
        for c in &cfg_dirs {
            let Ok(t) = std::fs::read_to_string(c.join("retroarch.cfg")) else { continue };
            if let Some(m) = LIBRETRO_DIR.captures(&t).map(|m| m[1].to_string()).filter(|m| !m.is_empty() && m != "default") {
                let m = match m.strip_prefix('~').filter(|r| r.is_empty() || r.starts_with('/')) {
                    Some(r) => format!("{}{r}", home.display()),
                    None => m,
                };
                let m = match m.strip_prefix(':') {
                    Some(r) => format!("{}/{}", dir.display(), r.strip_prefix('/').unwrap_or(r)),
                    None => m,
                };
                dirs.push(PathBuf::from(m));
            }
        }
        dirs.extend(cfg_dirs[..2].iter().map(|c| c.join("cores")));
        dirs.push(dir.join("cores"));
        dirs.extend(["/usr/lib/libretro", "/usr/lib/x86_64-linux-gnu/libretro", "/usr/lib64/libretro"].map(PathBuf::from));
        return dirs.into_iter().map(|d| d.join(format!("{core}_libretro.so"))).find(|f| is_file(f));
    }
    dirs.push(dir.join("cores"));
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    for cfg_file in [dir.join("retroarch.cfg"), appdata.join("RetroArch").join("retroarch.cfg")] {
        let Ok(t) = std::fs::read_to_string(&cfg_file) else { continue };
        if let Some(m) = LIBRETRO_DIR.captures(&t).map(|m| m[1].to_string()).filter(|m| !m.is_empty() && m != "default") {
            // ":" is RetroArch's own folder
            let m = match m.strip_prefix(':') {
                Some(r) => format!("{}{}{}", dir.display(), std::path::MAIN_SEPARATOR, r.strip_prefix(['\\', '/']).unwrap_or(r)),
                None => m,
            };
            dirs.insert(0, PathBuf::from(m));
        }
        break;
    }
    dirs.into_iter().map(|d| d.join(format!("{core}_libretro.dll"))).find(|f| is_file(f))
}

/// The environment a program gets on Linux: RustyShelf's own, without what an AppImage it runs from adds (its
/// libraries would be loaded into the emulator), then `extra`.
pub fn clean_env(extra: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    let vars = std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect();
    clean_env_of(vars, extra)
}

fn clean_env_of(vars: Vec<(String, String)>, extra: Option<Vec<(String, String)>>) -> Vec<(String, String)> {
    let own = vars.iter().find(|(k, _)| k == "APPDIR").map(|(_, v)| v.clone()).filter(|d| !d.is_empty());
    let mut env: Vec<(String, String)> = vars
        .into_iter()
        .filter(|(k, _)| !matches!(k.as_str(), "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD"))
        .filter_map(|(k, v)| match (&own, matches!(k.as_str(), "LD_LIBRARY_PATH" | "PATH" | "XDG_DATA_DIRS" | "GSETTINGS_SCHEMA_DIR")) {
            (Some(own), true) if !v.is_empty() => {
                let kept: Vec<&str> = v.split(':').filter(|p| !p.is_empty() && !p.starts_with(own.as_str())).collect();
                (!kept.is_empty()).then(|| (k, kept.join(":")))
            }
            _ => Some((k, v)),
        })
        .collect();
    for (k, v) in extra.unwrap_or_default() {
        env.retain(|(e, _)| *e != k);
        env.push((k, v));
    }
    env
}

/// Everything the Electron app decides before it starts a game: the emulator, its options, the file it's handed.
/// (Windows) The game through RocketLauncher, which picks and starts the emulator itself. None when RocketLauncher has
/// no system for this console and it's on for every console without this one chosen: the console keeps its emulator.
fn rocket_plan(console: &Console, file: &Path, cfg: &Config) -> Result<Option<Plan>, LaunchError> {
    let (exe, mode, frontend) = crate::rocket::settings(cfg);
    let exe = PathBuf::from(exe);
    if !is_file(&exe) {
        return refuse("RocketLauncher isn't where Settings → Consoles says. Find it again there, or start this console with its emulator.");
    }
    let list = crate::rocket::systems(&exe, false);
    let own = cfg.raw().get("systems").and_then(|s| s.get(&console.id)).and_then(|s| s.get("rocketSystem")).and_then(serde_json::Value::as_str).unwrap_or("").trim().to_string();
    let system = if own.is_empty() { crate::rocket::name_for(&console.id, &list) } else { own };
    let a = crate::rocket::args_for(&system, file, &list, &frontend);
    match a.ready {
        None if mode == "all" && crate::rocket::console_choice(cfg, &console.id) != Some(true) => Ok(None),
        Some(false) => refuse(format!("RocketLauncher has no emulator set for {system} yet. Set one in RocketLauncherUI, or start this console with its emulator (Settings → Consoles).")),
        _ => Ok(Some(Plan { exe, args: a.args, env: None })),
    }
}

/// Is this program RocketLauncher? (its game is closed through the emulator it started)
pub fn is_rocket(exe: &str) -> bool {
    Path::new(exe).file_name().is_some_and(|n| n.eq_ignore_ascii_case("RocketLauncher.exe"))
}

/// `build`: the Switch 2 build chosen at Play, over the game's own and the shelf's. `say`: what the launch screen says
/// while the game is got ready (a 3DS install, a RetroArch core's download).
#[allow(clippy::too_many_arguments)]
pub fn plan(console: &Console, sc: &SystemConfig, file: &Path, game_id: &str, cfg: &Config, app_root: &Path, build: Option<&Path>, say: &mut dyn FnMut(&str)) -> Result<Plan, LaunchError> {
    let id = console.id.as_str();
    if crate::rocket::use_for(id, cfg) {
        if let Some(p) = rocket_plan(console, file, cfg)? {
            return Ok(p);
        }
    }
    let mut exe = match build.filter(|_| id == "sw2") {
        Some(b) => nx2::chosen(cfg, app_root, &file.to_string_lossy(), b).map_err(LaunchError::Refused)?.exe,
        None => choose_emulator(console, sc, game_id, cfg, app_root)?,
    };
    if id == "ps4" && file.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkg")) {
        return refuse(PS4_PKG);
    }
    if id == "x360" {
        exe = resolve_xenia(exe);
    }
    let (con, emu) = (&console.name, if console.emulator_name.is_empty() { "its emulator" } else { &console.emulator_name });
    if exe.as_os_str().is_empty() {
        return refuse(format!("No emulator is set up for {con} yet. Choose {emu} in Settings → {con} → Emulator."));
    }
    if !is_file(&exe) {
        return refuse(format!("{emu} isn't at {} any more. Choose it again in Settings → {con} → Emulator.", exe.display()));
    }
    let mut game_args = sc.game_args.clone();
    let mut extra: Vec<String> = Vec::new();
    if id == "gc" && sc.use_ipl {
        extra.extend(["-C", "Dolphin.Core.SkipIPL=False"].map(String::from));
        if sc.hide_dolphin_warnings {
            extra.extend(["-C", "Dolphin.Interface.UsePanicHandlers=False"].map(String::from));
        }
    }
    let exe_name = name_of(&exe);
    // Dolphin takes settings for this session only, so its own files stay as they are: no "stop emulation?" question,
    // borderless so the Home menu can show over it
    if (id == "gc" || id == "wii") && exe_name.to_lowercase().contains("dolphin") {
        extra.extend(["-C", "Dolphin.Interface.ConfirmStop=False", "-C", "Graphics.Settings.BorderlessFullscreen=True"].map(String::from));
    }
    let mut rom = file.to_string_lossy().into_owned();
    if id == "n3ds" {
        let boot = crate::n3ds::boot_file(file, &exe.to_string_lossy(), &crate::paths::env_var, say).map_err(LaunchError::Refused)?;
        rom = boot.to_string_lossy().into_owned();
    }
    if id == "psvita" {
        let (r, args) = vita3k_spec(file)?;
        rom = r;
        if game_args.is_empty() || game_args == console.game_args {
            game_args = args.to_string();
        }
    }
    if id == "sw2" {
        rom = nx2::boot_file(file).to_string_lossy().into_owned();
    }
    let mut env = None;
    let mut via_launcher = sc.via_launcher;
    static KYTY_LAUNCHER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(kyty_)?launcher\.exe$").unwrap());
    if id == "ps5" && KYTY_LAUNCHER.is_match(&exe_name) {
        let emu = exe.with_file_name("kyty_emulator.exe");
        if is_file(&emu) {
            exe = emu;
        } else {
            via_launcher = true;
        }
    }
    if id == "ps5" && via_launcher {
        // the launcher itself, where the game is started; it takes no game to open
        let names: &[&str] = if cfg!(windows) { &["kyty_launcher.exe", "launcher.exe"] } else { &["kyty_launcher"] };
        let Some(ui) = names.iter().map(|n| exe.with_file_name(n)).find(|p| is_file(p)) else {
            return refuse("KytyPS5's launcher (kyty_launcher) isn't next to kyty_emulator. Turn off \"Start games through KytyPS5's launcher\" in Settings → PS5, or check the emulator path.");
        };
        exe = ui;
        game_args = String::new();
        extra.clear();
    } else if id == "ps5" {
        let k = kyty::spec(&exe, file);
        extra.extend(k.args);
        env = k.env;
        rom = k.dir.to_string_lossy().into_owned();
    }
    let exe_name = name_of(&exe);
    // Switch through Ryujinx: its own options, not Eden's "-f -g" (an edited command line is left as it is)
    static EDEN_ARGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*-f\s+-g\s+"\{rom\}"\s*$"#).unwrap());
    let low = exe_name.to_lowercase();
    if id == "sw" && (low.contains("ryujinx") || low.contains("ryubing")) && EDEN_ARGS.is_match(&game_args) {
        game_args = r#"--fullscreen "{rom}""#.into();
    }
    // a retro console in another standalone emulator than the one its options are for: that emulator's own options, as
    // long as the console's are still the default ones
    if console.retro && !low.contains("retroarch") && (game_args.is_empty() || game_args == console.game_args) {
        if let Some(alt) = alt_args(&exe_name) {
            game_args = alt.into();
        }
    }
    if console.ra_core.is_some() && is_retroarch(&exe) {
        let cid = core_of(console, sc).unwrap_or_default();
        let nm = core_name(&cid);
        let fix = format!("In RetroArch: Main Menu → Online Updater → Core Downloader → {nm}, then press Play again.");
        let core = match retro_core(&exe, &cid) {
            Some(core) => core,
            // a missing core is downloaded now, from the same place RetroArch's Online Updater gets it
            None if cfg!(windows) => {
                say(&format!("Getting RetroArch's {nm} core…"));
                crate::racores::get(&exe.to_string_lossy(), &cid, &crate::paths::env_var, &mut |p| say(&format!("Getting RetroArch's {nm} core… {p}%")))
                    .map_err(|e| LaunchError::Refused(format!("RetroArch doesn't have the {nm} core for {con} yet, and it couldn't be downloaded ({e}). {fix}")))?
            }
            None => return refuse(format!("RetroArch doesn't have the {nm} core for {con} yet. {fix}")),
        };
        game_args = format!(r#"-f -L "{}" "{{rom}}""#, core.display());
    }
    extra.extend(build_args(&game_args, &rom));
    let env = if cfg!(target_os = "linux") { Some(clean_env(env)) } else { env };
    Ok(Plan { exe, args: extra, env })
}

/// Starts the program in its own folder. It isn't tied to RustyShelf: closing the shelf leaves the game running.
pub fn start(plan: &Plan) -> Result<Child, LaunchError> {
    let mut cmd = Command::new(&plan.exe);
    cmd.args(&plan.args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(env) = &plan.env {
        cmd.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
    }
    if let Some(dir) = plan.exe.parent() {
        cmd.current_dir(dir);
    }
    // its own process group, as the Electron app starts it: closing the group reaches an AppImage's or a Flatpak's emulator
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.spawn().map_err(LaunchError::Spawn)
}

/// Hands an address to the program registered for it (steam://, a store's launch link).
fn open_external(url: &str) -> Result<(), LaunchError> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // the opener returns at once; reaped on a thread so it doesn't linger
    let mut child = cmd.spawn().map_err(LaunchError::Spawn)?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The emulator on its own, for changing its settings, with the console's emulator options. The PS5's is Kyty's
/// launcher: kyty_emulator needs a game.
pub fn emulator_plan(console_id: &str, emulator: &str, emulator_args: &str, app_root: &Path) -> Result<Plan, LaunchError> {
    if emulator.trim().is_empty() {
        return refuse("Choose this console's emulator first.");
    }
    let mut exe = emu_path(emulator, app_root);
    if console_id == "ps5" {
        let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        if let Some(ui) = ["kyty_launcher.exe", "launcher.exe"].iter().map(|n| dir.join(n)).find(|f| is_file(f)) {
            exe = ui;
        }
    }
    if !is_file(&exe) {
        return refuse(format!("The emulator isn't at {} any more. Choose it again in Settings.", exe.display()));
    }
    let env = if cfg!(target_os = "linux") { Some(clean_env(None)) } else { None };
    Ok(Plan { exe, args: build_args(emulator_args, ""), env })
}

/// Asks the programs `pid` started to close, not `pid` itself: RocketLauncher, asked directly, could end before its own
/// exit (fade out, taskbar and mouse back) and leave them hidden; once its emulator closes it ends by itself.
pub fn close_children(pid: u32) {
    if !cfg!(windows) {
        return close_program(pid, false);
    }
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &format!("Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}' | ForEach-Object {{ taskkill /PID $_.ProcessId | Out-Null }}")]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    if let Ok(mut c) = cmd.spawn() {
        std::thread::spawn(move || c.wait());
    }
}

/// Asks a running program to close, as a click on its X does (its saves and settings are written); `force` ends it
/// and everything it started, for one that didn't close when asked.
pub fn close_program(pid: u32, force: bool) {
    let mut cmd;
    if cfg!(windows) {
        cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string()]);
        if force {
            cmd.args(["/T", "/F"]);
        }
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    } else {
        // the process group it was started as: a Flatpak's launcher, an AppImage's runtime and the emulator itself
        cmd = Command::new("kill");
        cmd.args([if force { "-KILL" } else { "-TERM" }, "--", &format!("-{pid}")]);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Ok(mut child) = cmd.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

/// Shows a folder in the system's file manager.
pub fn open_folder(dir: &Path) -> Result<(), LaunchError> {
    let mut cmd = Command::new(if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" });
    cmd.arg(dir).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(LaunchError::Spawn)?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Opens a web page in the system's browser.
pub fn open_link(url: &str) -> Result<(), LaunchError> {
    if !url.starts_with("https://") {
        return refuse("Not a web address.");
    }
    open_folder(Path::new(url))
}

/// Hands a Steam game to Steam (steam://rungameid), which starts it.
pub fn launch_steam(appid: &str) -> Result<(), LaunchError> {
    if appid.is_empty() || !appid.bytes().all(|b| b.is_ascii_digit()) {
        return refuse("Unknown Steam game.");
    }
    open_external(&format!("steam://rungameid/{appid}"))
}

/// A program whose manifest asks for administrator rights can't be started by a normal process: Windows refuses it.
fn needs_admin(exe: &Path) -> bool {
    const MARK: &[u8] = b"requireAdministrator";
    exe.metadata().is_ok_and(|m| m.len() <= 600_000_000) && std::fs::read(exe).is_ok_and(|b| b.windows(MARK.len()).any(|w| w == MARK))
}

/// The PowerShell lines that start a program with Windows' own administrator prompt, and wait for it.
fn elevated_script(exe: &str, args: &[String]) -> String {
    // one PowerShell string: a quote inside is doubled
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    // the program's folder, as Windows names it: a drive's root keeps its separator
    let dir = match exe.rfind(['\\', '/']) {
        Some(0) => &exe[..1],
        Some(2) if exe.as_bytes()[1] == b':' => &exe[..3],
        Some(i) => &exe[..i],
        None if exe.as_bytes().get(1) == Some(&b':') => &exe[..2],
        None => ".",
    };
    let list = if args.is_empty() { String::new() } else { format!("-ArgumentList @({}) ", args.iter().map(|a| q(a)).collect::<Vec<_>>().join(",")) };
    format!("$p = Start-Process -FilePath {} -WorkingDirectory {} -Verb RunAs {list}-PassThru; $p.WaitForExit()", q(exe), q(dir))
}

/// The game stays "running" as long as the elevated program does: PowerShell waits for it.
fn start_elevated(exe: &str, args: &[String]) -> std::io::Result<Child> {
    use base64::Engine;
    let utf16: Vec<u8> = elevated_script(exe, args).encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-EncodedCommand", &base64::engine::general_purpose::STANDARD.encode(utf16)]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    cmd.spawn()
}

/// Starts a PC game that isn't Steam's. A store's game goes through its store (Epic, Ubisoft Connect, the EA app:
/// their sign-in, cloud saves, overlay) and an Xbox game is started as the Windows app it is: None then, the store
/// looks after it. The others start their own program in its own folder.
pub fn launch_pc(g: &PcGame) -> Result<Option<Child>, LaunchError> {
    static STORE_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(com\.epicgames\.launcher|uplay|origin2)://").unwrap());
    if g.installed == Some(false) {
        return refuse(format!("{} isn't installed. Install it first.", g.title));
    }
    if STORE_LINK.is_match(&g.uri) {
        return open_external(&g.uri).map(|()| None);
    }
    if !g.shell_app.is_empty() {
        if !crate::pcgames::is_shell_app(&g.shell_app) {
            return refuse("Couldn't start that Xbox game.");
        }
        let mut cmd = Command::new("explorer.exe");
        cmd.arg(format!("shell:AppsFolder\\{}", g.shell_app)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(LaunchError::Spawn)?;
        std::thread::spawn(move || child.wait());
        return Ok(None);
    }
    let exe = PathBuf::from(&g.exe);
    if !is_file(&exe) {
        let fix = match g.source {
            "quiver" => "Check it in Quiver Launcher.",
            "gog" => "Check it in GOG Galaxy.",
            "hoyoplay" => "Check it in HoYoPlay.",
            "folder" => "Check its folder in the PC games folder.",
            "moonlight" => "Install Moonlight, or choose it in Settings → Game streaming.",
            _ => "Remove it in Settings → Custom games and add it again.",
        };
        return refuse(format!("{} isn't at {} any more. {fix}", g.title, g.exe));
    }
    start_program(&g.exe, build_args(&g.args, ""), &g.title).map(Some)
}

/// A PC game as `launch_pc` starts it, a Game streaming app's host woken first when it has a MAC address, and on
/// Linux an Epic game, a PC games folder's game or a Windows program through legendary, Wine or Proton. The text is
/// something to tell the player (an update waiting, Proton's runtime still downloading).
pub fn launch_pc_with(g: &PcGame, cfg: &Config, app_root: &Path) -> Result<(Option<Child>, Option<String>), LaunchError> {
    // the wake-up isn't waited for: Moonlight waits for the host to show up
    if g.source == "moonlight" {
        let mac = cfg.moonlight().mac;
        if !mac.is_empty() {
            std::thread::spawn(move || crate::moonlight::wake(&mac));
        }
    }
    if is_linux_pc(g) {
        return launch_linux_pc(g, cfg, app_root).map(|(c, n)| (Some(c), n));
    }
    if cfg!(target_os = "linux") && g.source == "epic" {
        return refuse(format!("{} isn't installed yet: Settings → PC games → Epic library installs it.", g.title));
    }
    launch_pc(g).map(|c| (c, None))
}

/// (Linux) A game RustyShelf starts itself through legendary, Wine or Proton: asking Epic can take a while, so it's
/// started off the shelf's thread.
pub fn is_linux_pc(g: &PcGame) -> bool {
    cfg!(target_os = "linux") && g.installed != Some(false) && (g.source == "epic" || g.source == "folder" || crate::linuxpc::is_windows_program(&g.exe))
}

/// (Linux) An Epic game (legendary gives its program and Epic's sign-in arguments), one in the PC games folder, or a
/// Windows program added by hand. A Linux program runs as it is, with the game data folder as its home; a Windows one
/// in a Wine prefix named after the game. Its output goes to data/logs/last-pc-game.log, and what's left in its prefix
/// is ended once it closes.
fn launch_linux_pc(g: &PcGame, cfg: &Config, app_root: &Path) -> Result<(Child, Option<String>), LaunchError> {
    use crate::linuxpc as lp;
    let pc = cfg.raw().get("pc");
    let text = |k: &str| crate::js::text(pc.and_then(|p| p.get(k)));
    let data_dir = crate::paths::data_dir(app_root);
    let mut notice = None;
    let (exe, args, cwd, env, store, game_id) = if g.source == "epic" {
        let bin = lp::legendary_bin(&text("legendaryPath"), app_root).ok_or_else(|| LaunchError::Refused("Epic games need legendary: Settings → PC games → Get legendary.".into()))?;
        let info = lp::epic_launch_info(&bin, &data_dir, &g.app_name).map_err(LaunchError::Refused)?;
        if info.outdated {
            notice = Some(format!("An update for {} is waiting: Settings → PC games → Epic library updates it.", g.title));
        }
        let id = lp::is_windows_program(&info.exe.to_string_lossy()).then(|| lp::umu_id(&data_dir, "egs", &g.app_name)).flatten();
        (info.exe, info.args, info.cwd, info.env, "egs", id)
    } else {
        if !is_file(Path::new(&g.exe)) {
            let fix = if g.source == "folder" { "Check its folder in the PC games folder." } else { "Remove it in Settings → Custom games and add it again." };
            return refuse(format!("{} isn't at {} any more. {fix}", g.title, g.exe));
        }
        let exe = PathBuf::from(&g.exe);
        let cwd = exe.parent().map(Path::to_path_buf).unwrap_or_default();
        (exe, if g.source == "folder" { Vec::new() } else { build_args(&g.args, "") }, cwd, Vec::new(), "none", None)
    };
    // the game's own way of running (pc.run, by id: its runner, prefix and variables), over what the PC games settings say
    let own = pc.and_then(|p| p.get("run")).and_then(|r| r.get(&g.id));
    let own_text = |k: &str| crate::js::text(own.and_then(|o| o.get(k)));
    let own_env: Vec<(String, String)> = own.and_then(|o| o.get("env")).and_then(serde_json::Value::as_object).map(|e| e.iter().map(|(k, v)| (k.clone(), crate::js::string(Some(v)))).collect()).unwrap_or_default();
    let mut ends = None;
    let (cmd, args, cwd, env) = if !lp::is_windows_program(&exe.to_string_lossy()) {
        (exe.clone(), args, cwd, clean_env(Some([lp::game_home(&text("homeDir")), own_env, env].concat())))
    } else {
        let list = lp::runners(app_root);
        let wanted = [own_text("runner"), text("runner")].into_iter().find(|r| !r.is_empty()).unwrap_or_default();
        let runner = lp::pick_runner(&wanted, &list);
        // named after the game's own title, not a name given in Details → Rename: renaming keeps its saves
        let prefix = Some(own_text("prefix")).filter(|p| !p.is_empty()).map_or_else(|| lp::prefix_for(&g.title, &text("prefixDir"), app_root), PathBuf::from);
        if runner.as_ref().is_some_and(|r| r.proton) && lp::umu_needs_runtime() {
            notice = Some("First start with Proton: its runtime is downloading (about 700 MB), so the game takes a minute or two to appear.".into());
        }
        let c = lp::windows_command(lp::WinProgram { exe: &exe, args, cwd: Some(&cwd), runner: runner.as_ref(), prefix: &prefix, env: [lp::proton_env(&g.title, app_root), own_env, env].concat(), game_id, store: Some(store) }).map_err(LaunchError::Refused)?;
        ends = runner.and_then(|r| lp::wineserver(&r, &prefix));
        (c.cmd, c.args, c.cwd, c.env)
    };
    let log = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&log);
    let out = std::fs::File::create(log.join("last-pc-game.log")).ok();
    // a shell around the game ends what's left in its prefix once it closes; the whole group is closed together
    let mut c = Command::new("/bin/sh");
    let script = r#"prog=$1; ws=$2; pfx=$3; shift 3; "$prog" "$@"; s=$?; [ -n "$ws" ] && WINEPREFIX="$pfx" "$ws" -k; exit $s"#;
    let (ws, pfx) = ends.map_or((String::new(), String::new()), |(w, p)| (w.to_string_lossy().into_owned(), p.to_string_lossy().into_owned()));
    c.arg("-c").arg(script).arg("rustyshelf").arg(&cmd).arg(ws).arg(pfx).args(&args).current_dir(&cwd).env_clear().envs(env).stdin(Stdio::null());
    match out.as_ref().and_then(|f| Some((f.try_clone().ok()?, f.try_clone().ok()?))) {
        Some((a, b)) => c.stdout(a).stderr(b),
        None => c.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let child = c.spawn().map_err(|e| LaunchError::Refused(format!("{} didn't start: {e}", g.title)))?;
    Ok((child, notice))
}

/// Starts a program that isn't an emulator in its own folder; one that asks for administrator rights through
/// Windows' own prompt.
fn start_program(exe: &str, args: Vec<String>, name: &str) -> Result<Child, LaunchError> {
    let plan = Plan { exe: PathBuf::from(exe), args, env: cfg!(target_os = "linux").then(|| clean_env(None)) };
    match start(&plan) {
        Ok(child) => Ok(child),
        Err(LaunchError::Spawn(_)) if cfg!(windows) && needs_admin(&plan.exe) => {
            start_elevated(exe, &plan.args).map_err(|e| LaunchError::Refused(format!("{name} didn't start: {e}")))
        }
        Err(LaunchError::Spawn(e)) => refuse(format!("{name} didn't start: {e}")),
        Err(e) => Err(e),
    }
}

/// Starts a media app (the Media shelf) by the program Settings has for it.
pub fn launch_app(name: &str, path: &str, app_root: &Path) -> Result<Child, LaunchError> {
    let exe = emu_path(path, app_root);
    if path.trim().is_empty() || !is_file(&exe) {
        let what = if path.trim().is_empty() { "the program".to_string() } else { exe.display().to_string() };
        return refuse(format!("Can't find {what}. Check its path in Settings."));
    }
    start_program(&exe.to_string_lossy(), Vec::new(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_options_take_the_games_name_and_folder() {
        assert_eq!(build_args(r#"-r "{romname}" -d "{romdir}" "{rom}""#, "/g/Beta Kart (Europe).iso"), ["-r", "Beta Kart (Europe)", "-d", "/g", "/g/Beta Kart (Europe).iso"]);
        assert_eq!(build_args("{romname} {romdir}", ""), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn a_started_program_closes_when_asked() {
        let plan = Plan { exe: PathBuf::from("/bin/sleep"), args: vec!["30".into()], env: None };
        let mut child = start(&plan).unwrap();
        close_program(child.id(), false);
        let asked = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            assert!(asked.elapsed().as_secs() < 10, "still running");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn args() {
        assert_eq!(
            build_args(r#"-b -C Dolphin.Display.Fullscreen=True -e "{rom}""#, "/g/Ca$$h (USA).iso"),
            ["-b", "-C", "Dolphin.Display.Fullscreen=True", "-e", "/g/Ca$$h (USA).iso"]
        );
        assert_eq!(build_args(r#""" x"#, ""), ["x"]);
    }

    struct Setup {
        dir: tempfile::TempDir,
    }

    impl Setup {
        fn new() -> Setup {
            Setup { dir: tempfile::tempdir().unwrap() }
        }
        fn file(&self, rel: &str) -> PathBuf {
            let p = self.dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"").unwrap();
            p
        }
        fn plan(&self, id: &str, cfg: &str, file: &Path) -> Result<Plan, LaunchError> {
            let console = systems().console(id).unwrap();
            let cfg = Config::parse(cfg).unwrap();
            let game_id = format!("{id}:{}", file.display());
            plan(console, &cfg.system(console), file, &game_id, &cfg, self.dir.path(), None, &mut |_| {})
        }
    }

    fn refused(r: Result<Plan, LaunchError>) -> String {
        match r {
            Err(LaunchError::Refused(why)) => why,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn which_emulator() {
        let s = Setup::new();
        let game = s.file("g/Some Game (USA).iso");
        let gc = systems().console("gc").unwrap();
        let why = refused(s.plan("gc", "{}", &game));
        assert!(why.starts_with(&format!("No emulator is set up for {} yet.", gc.name)), "{why}");
        let why = refused(s.plan("gc", r#"{"systems":{"gc":{"emulator":"{app}/none/Dolphin.exe"}}}"#, &game));
        assert!(why.contains("isn't at") && why.contains("none"), "{why}");

        let dolphin = s.file("Dolphin/Dolphin.exe");
        let own = s.file("Other/Dolphin.exe");
        let cfg = format!(
            r#"{{"systems":{{"gc":{{"emulator":"\"{{app}}/Dolphin/Dolphin.exe\"","useIpl":true,"hideDolphinWarnings":true}}}},"games":{{"gc:{}":{{"emulator":"{}"}}}}}}"#,
            game.display(),
            own.display()
        );
        let p = s.plan("gc", &cfg, &game).unwrap();
        assert_eq!(p.exe, own);
        assert_eq!(p.args[..4], ["-C", "Dolphin.Core.SkipIPL=False", "-C", "Dolphin.Interface.UsePanicHandlers=False"]);
        assert!(p.args.contains(&"Graphics.Settings.BorderlessFullscreen=True".to_string()));
        assert_eq!(p.args.last().unwrap(), &game.to_string_lossy());
        let p = s.plan("gc", r#"{"systems":{"gc":{"emulator":"{app}/Dolphin/Dolphin.exe"}}}"#, &game).unwrap();
        assert_eq!(p.exe, dolphin);
    }

    #[test]
    fn xenia_behind_its_manager() {
        let s = Setup::new();
        s.file("Xenia/XeniaManager.exe");
        let canary = s.file("Xenia/Emulators/Xenia Canary/xenia_canary.exe");
        s.file("Xenia/Emulators/Xenia Stable/xenia.exe");
        let game = s.file("g/Game.iso");
        let p = s.plan("x360", r#"{"systems":{"x360":{"emulator":"{app}/Xenia/XeniaManager.exe"}}}"#, &game).unwrap();
        assert_eq!(p.exe, canary);
    }

    #[test]
    fn retro_consoles() {
        let s = Setup::new();
        let game = s.file("g/Some Game.gba");
        let gba = systems().console("gba").unwrap();
        let mgba = s.file("mGBA/mGBA.exe");
        let p = s.plan("gba", r#"{"systems":{"gba":{"emulator":"{app}/mGBA/mGBA.exe"}}}"#, &game).unwrap();
        assert_eq!((p.exe, p.args), (mgba, vec!["-f".to_string(), game.to_string_lossy().into_owned()]));
        // RetroArch, from Settings → RetroArch, without the core yet
        let ra = s.file(if cfg!(target_os = "linux") { "RA/retroarch" } else { "RA/retroarch.exe" });
        let why = refused(s.plan("gba", &format!(r#"{{"retroarch":"{}"}}"#, ra.display()), &game));
        let core = gba.ra_core.clone().unwrap();
        assert!(why.contains(&core_name(&core)) && why.contains(&gba.name), "{why}");
        let ext = if cfg!(target_os = "linux") { "so" } else { "dll" };
        let lib = s.file(&format!("RA/cores/{core}_libretro.{ext}"));
        let p = s.plan("gba", &format!(r#"{{"retroarch":"{}"}}"#, ra.display()), &game).unwrap();
        assert_eq!(p.args, ["-f", "-L", &lib.to_string_lossy(), &game.to_string_lossy()]);
    }

    #[test]
    fn per_console_files() {
        let s = Setup::new();
        let emu = s.file("Emu/emu.exe");
        let cfg = |id: &str| format!(r#"{{"systems":{{"{id}":{{"emulator":"{}"}}}}}}"#, emu.display());
        let pkg = s.file("ps4/Game.pkg");
        assert!(refused(s.plan("ps4", &cfg("ps4"), &pkg)).contains("still a .pkg"));
        let vita = s.file("ux0/app/PCSE00000/eboot.bin");
        assert_eq!(s.plan("psvita", &cfg("psvita"), &vita).unwrap().args, ["-F", "-r", "PCSE00000"]);
        let ryu = s.file("Ryujinx/Ryujinx.exe");
        let nsp = s.file("sw/Game.nsp");
        let p = s.plan("sw", &format!(r#"{{"systems":{{"sw":{{"emulator":"{}"}}}}}}"#, ryu.display()), &nsp).unwrap();
        assert_eq!(p.args, ["--fullscreen", &nsp.to_string_lossy()]);
    }

    #[cfg(unix)]
    #[test]
    fn starts_the_emulator() {
        use std::os::unix::fs::PermissionsExt;
        let s = Setup::new();
        let out = s.dir.path().join("got.txt");
        let emu = s.dir.path().join("Emu/emu.sh");
        std::fs::create_dir_all(emu.parent().unwrap()).unwrap();
        std::fs::write(&emu, format!("#!/bin/sh\nprintf '%s|' \"$PWD\" \"$@\" \"$RS_T\" > '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&emu, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plan = Plan { exe: emu.clone(), args: vec!["-f".into(), "/g/A Game.iso".into()], env: Some(clean_env(Some(vec![("RS_T".into(), "x".into())]))) };
        let status = start(&plan).unwrap().wait().unwrap();
        assert!(status.success());
        let got = std::fs::read_to_string(&out).unwrap();
        assert_eq!(got, format!("{}|-f|/g/A Game.iso|x|", emu.parent().unwrap().display()));
    }

    #[cfg(unix)]
    #[test]
    fn starts_a_pc_game() {
        use std::os::unix::fs::PermissionsExt;
        let s = Setup::new();
        let out = s.dir.path().join("got.txt");
        let exe = s.dir.path().join("Tool/tool.sh");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, format!("#!/bin/sh\nprintf '%s|' \"$PWD\" \"$@\" > '{}'\n", out.display())).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let g = PcGame { id: "custom-tool".into(), source: "custom", title: "Tool".into(), exe: exe.to_string_lossy().into_owned(), args: "-a \"b c\"".into(), ..PcGame::default() };
        assert!(launch_pc(&g).unwrap().unwrap().wait().unwrap().success());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), format!("{}|-a|b c|", exe.parent().unwrap().display()));

        let refused = |g: PcGame| launch_pc(&g).unwrap_err().to_string();
        assert_eq!(refused(PcGame { installed: Some(false), title: "Zeta".into(), ..g.clone() }), "Zeta isn't installed. Install it first.");
        assert_eq!(refused(PcGame { source: "gog", exe: "/nope/zeta".into(), title: "Zeta".into(), ..g.clone() }), "Zeta isn't at /nope/zeta any more. Check it in GOG Galaxy.");
        assert_eq!(refused(PcGame { shell_app: "not an app".into(), ..g }), "Couldn't start that Xbox game.");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn starts_a_linux_pc_game() {
        use std::os::unix::fs::PermissionsExt;
        let s = Setup::new();
        let root = s.dir.path();
        let exe = root.join("Games/Native/run.sh");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "#!/bin/sh\necho \"in $PWD home $HOME\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let home = root.join("GameHome");
        let cfg = Config::parse(&serde_json::json!({ "pc": { "homeDir": home } }).to_string()).unwrap();
        let g = PcGame { id: "folder-native".into(), source: "folder", title: "Native".into(), exe: exe.to_string_lossy().into_owned(), native: true, ..PcGame::default() };
        assert!(is_linux_pc(&g));
        let (child, notice) = launch_pc_with(&g, &cfg, root).unwrap();
        assert!(child.unwrap().wait().unwrap().success() && notice.is_none());
        let log = std::fs::read_to_string(crate::paths::data_dir(root).join("logs/last-pc-game.log")).unwrap();
        assert_eq!(log.trim(), format!("in {} home {}", exe.parent().unwrap().display(), home.display()));
        let gone = PcGame { installed: Some(false), source: "epic", ..g };
        assert!(!is_linux_pc(&gone));
        assert_eq!(launch_pc_with(&gone, &cfg, root).unwrap_err().to_string(), "Native isn't installed yet: Settings → PC games → Epic library installs it.");
    }

    #[test]
    fn administrator_prompt() {
        assert_eq!(
            elevated_script("C:\\Games\\It's\\a.exe", &["-x".into(), "it's".into()]),
            "$p = Start-Process -FilePath 'C:\\Games\\It''s\\a.exe' -WorkingDirectory 'C:\\Games\\It''s' -Verb RunAs -ArgumentList @('-x','it''s') -PassThru; $p.WaitForExit()"
        );
        assert!(elevated_script("D:\\game.exe", &[]).contains("-WorkingDirectory 'D:\\' "));
        assert_eq!(elevated_script("a.exe", &[]), "$p = Start-Process -FilePath 'a.exe' -WorkingDirectory '.' -Verb RunAs -PassThru; $p.WaitForExit()");
    }

    #[test]
    fn appimage_paths_left_out() {
        let vars = [("APPDIR", "/tmp/.mount_rs"), ("PATH", "/tmp/.mount_rs/usr/bin:/usr/bin"), ("LD_LIBRARY_PATH", "/tmp/.mount_rs/lib"), ("HOME", "/h")];
        let env = clean_env_of(vars.map(|(k, v)| (k.to_string(), v.to_string())).to_vec(), Some(vec![("X".into(), "1".into())]));
        let want = [("PATH", "/usr/bin"), ("HOME", "/h"), ("X", "1")].map(|(k, v)| (k.to_string(), v.to_string()));
        assert_eq!(env, want);
    }
}

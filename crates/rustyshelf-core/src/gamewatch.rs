// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/gamewatch.js, as RustyShelf has it.
//! Watches a game RustyShelf didn't start itself: a Steam game (started with steam://rungameid) or a store's game
//! (Epic, Ubisoft Connect, the EA app, Xbox), so it gets what a game RustyShelf started gets: the return to the
//! shelf when it ends, its playtime, the quit combo.
//!   - Steam: `HKCU\Software\Valve\Steam\RunningAppID` says which Steam game is running.
//!   - Every store: the game's process is one whose program sits inside the game's install folder.
//!
//! Polling: every 2 s while waiting for the game to show up (90 s at most, then `Timeout`); once it runs, a cheap
//! check that its processes are still alive every half second, and a fresh listing only when they're all gone (a
//! game's own launcher may hand over to the real game) or every 15 s to be sure.
//! Closing: the game's own processes are asked to close, forced after a wait. Steam, the store launchers and their
//! helpers are never touched.

use std::sync::LazyLock;

use regex::Regex;

/// Launchers, their helpers and Windows' own: never the game, never closed.
static LAUNCHERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(steam|steamwebhelper|steamservice|steamerrorreporter(64)?|gameoverlayui(64)?|steamtours|epicgameslauncher|epicwebhelper|epiconlineservices(host|userhelper|uihelper)?|galaxyclient( helper)?|galaxycommunication|gogcomposer|upc|uplaywebcore|uplayservice|ubisoftconnect|ubisoftgamelauncher(64)?|uplaycrashreporter|eadesktop|eabackgroundservice|ealocalhostsvc|ealauncher|eaconnect_microsoft|origin|originwebhelperservice|originclientservice|xboxpcapp|xboxapp|xboxappservices|gamingservices(net)?|gamebar(ftserver)?|explorer|boxshelf|powershell|cmd|conhost|taskkill)\.exe$").unwrap()
});
/// Helpers inside a game's folder: closed with the game, but never taken for the game itself.
static HELPERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(unitycrashhandler(32|64)?|crashreportclient|crashpad_handler|.*crash.?report(er)?.*|easyanticheat(_eos)?(_setup)?|start_protected_game|beservice(_x64)?|battleye.*|vc_?redist.*|dxsetup|ue4prereqsetup.*|uninstall.*|unins\d*)\.exe$").unwrap()
});

pub const WAIT_POLL: u64 = 2000;
pub const ALIVE_POLL: u64 = 500;
const VERIFY: u64 = 15_000;
const GIVE_UP: u64 = 90_000;
const APPID_ONLY: u64 = 15_000;
const STEAM_GRACE: u64 = 10_000;

/// A path as two are compared: Windows' separators, `.` and `..` resolved, no trailing separator, lower case.
fn norm(p: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for (i, part) in p.split(['\\', '/']).enumerate() {
        match part {
            "" if i > 0 => {}
            "." => {}
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            x => parts.push(x),
        }
    }
    parts.join("\\").to_lowercase()
}

fn base(p: &str) -> &str {
    p.rsplit(['\\', '/']).next().unwrap_or(p)
}

/// Whether a program is inside a folder (the folder itself excluded).
pub fn under(dir: &str, exe: &str) -> bool {
    if dir.is_empty() || exe.is_empty() {
        return false;
    }
    let (d, e) = (norm(dir), norm(exe));
    e.len() > d.len() + 1 && e.starts_with(&format!("{d}\\"))
}

pub fn is_launcher(name: &str) -> bool {
    LAUNCHERS.is_match(name)
}

/// A running process: its program's name and path ("" when it can't be read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub exe: String,
}

/// What the watcher asks of the computer.
pub trait World {
    /// Steam's running game: 0 when none, None when it can't be read.
    fn running_app_id(&mut self, want: &str) -> Option<u64>;
    /// Every process with its program; empty when the listing failed.
    fn list(&mut self) -> Vec<Proc>;
    /// A process that may not be opened (an elevated game) still counts as running.
    fn alive(&mut self, pid: u32) -> bool;
    /// Asks a process to close (like a click on its X), or forces it and whatever it started.
    fn kill(&mut self, pid: u32, force: bool);
}

#[derive(Debug, Clone, Default)]
pub struct Spec {
    /// "steam", or a store's name
    pub kind: String,
    pub appid: String,
    /// the game's install folder, when it's known
    pub dir: String,
    /// the program the store names
    pub exe: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `pid` 0: Steam says it runs, and no process was found in its folder
    Started { pid: u32, exe: String, name: String },
    /// the game goes on in another of its programs (its own launcher handed over)
    Handover { pid: u32, exe: String },
    /// `cancelled`: closed before the game showed up
    Ended { ms: u64, cancelled: bool },
    /// no game process after 90 s: no longer watched
    Timeout,
    /// these didn't close when asked and were ended
    Forced(Vec<u32>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Waiting,
    Running,
    Ended,
    Stopped,
}

pub struct Watch {
    spec: Spec,
    own_pid: u32,
    pub state: State,
    t0: u64,
    pids: Vec<u32>,
    /// the process followed for its window
    pub pid: u32,
    started_at: u64,
    verified_at: u64,
    /// since when Steam has said the game runs with nothing running from its folder
    appid_at: u64,
    /// since when its processes are gone while Steam still says it runs
    gone_at: u64,
    misses: u32,
    list_fails: u32,
    closing: bool,
    poll_at: u64,
    force_at: Option<u64>,
}

impl Watch {
    /// `now`: ms; every call takes the time the same way. `own_pid`: RustyShelf's process, never the game.
    pub fn new(spec: Spec, now: u64, own_pid: u32) -> Watch {
        Watch {
            spec,
            own_pid,
            state: State::Waiting,
            t0: now,
            pids: Vec::new(),
            pid: 0,
            started_at: 0,
            verified_at: 0,
            appid_at: 0,
            gone_at: 0,
            misses: 0,
            list_fails: 0,
            closing: false,
            poll_at: now + WAIT_POLL,
            force_at: None,
        }
    }

    fn over(&self) -> bool {
        matches!(self.state, State::Ended | State::Stopped)
    }

    /// When `run` has something to do next; None once the watch is over.
    pub fn due(&self) -> Option<u64> {
        (!self.over()).then(|| self.force_at.map_or(self.poll_at, |f| f.min(self.poll_at)))
    }

    /// The game's processes in a listing: inside its folder, not a launcher, not RustyShelf.
    fn game_procs(&self, list: &[Proc]) -> Vec<Proc> {
        let named = |p: &Proc| if p.name.is_empty() { base(&p.exe).to_string() } else { p.name.clone() };
        list.iter().filter(|p| p.pid > 0 && p.pid != self.own_pid && !is_launcher(&named(p)) && under(&self.spec.dir, &p.exe)).cloned().collect()
    }

    /// The one to follow: the program the store names, else the first that isn't a helper.
    fn pick<'a>(&self, found: &'a [Proc]) -> &'a Proc {
        let want = base(&self.spec.exe).to_lowercase();
        found.iter().find(|p| !want.is_empty() && base(&p.exe).to_lowercase() == want).or_else(|| found.iter().find(|p| !HELPERS.is_match(base(&p.exe)))).unwrap_or(&found[0])
    }

    fn appid(&self) -> u64 {
        self.spec.appid.parse().unwrap_or(0)
    }

    fn started(&mut self, found: &[Proc], now: u64, out: &mut Vec<Event>) {
        self.state = State::Running;
        self.started_at = now;
        self.verified_at = now;
        self.pids = found.iter().map(|p| p.pid).collect();
        let p = (!found.is_empty()).then(|| self.pick(found).clone());
        self.pid = p.as_ref().map_or(0, |p| p.pid);
        out.push(Event::Started { pid: self.pid, exe: p.as_ref().map_or(String::new(), |p| p.exe.clone()), name: p.map_or(String::new(), |p| p.name) });
    }

    fn step(&mut self, w: &mut dyn World, now: u64, out: &mut Vec<Event>) {
        let steam = self.spec.kind == "steam";
        if self.state == State::Waiting {
            if now - self.t0 >= GIVE_UP {
                self.state = State::Stopped;
                out.push(Event::Timeout);
                return;
            }
            let id = if steam { w.running_app_id(&self.spec.appid) } else { None };
            if !self.spec.dir.is_empty() {
                let found = self.game_procs(&w.list());
                if !found.is_empty() {
                    return self.started(&found, now, out);
                }
            }
            // Steam says the game runs but nothing runs from its folder (a game that runs from elsewhere): followed
            // by Steam's word alone
            if steam && id == Some(self.appid()) {
                if self.appid_at == 0 {
                    self.appid_at = now;
                }
                if self.spec.dir.is_empty() || now - self.appid_at >= APPID_ONLY {
                    self.started(&[], now, out);
                }
            }
            return;
        }
        if self.state != State::Running {
            return;
        }
        // followed by Steam's word alone: it ends when Steam says no game runs, twice in a row
        if self.pids.is_empty() {
            let id = w.running_app_id(&self.spec.appid);
            self.misses = if id.is_none() || id == Some(self.appid()) { 0 } else { self.misses + 1 };
            if self.misses >= 2 {
                self.state = State::Ended;
                out.push(Event::Ended { ms: now - self.started_at, cancelled: false });
            }
            return;
        }
        let living: Vec<u32> = self.pids.iter().copied().filter(|p| w.alive(*p)).collect();
        if !living.is_empty() && now - self.verified_at < VERIFY {
            self.pids = living;
            return;
        }
        // all gone (or time to make sure it's still the game): a fresh look in its folder
        self.verified_at = now;
        let list = w.list();
        // no listing at all is no proof the game ended; three in a row with its processes gone is
        if list.is_empty() {
            if !living.is_empty() {
                return;
            }
            self.list_fails += 1;
            if self.list_fails < 3 {
                return;
            }
        } else {
            self.list_fails = 0;
        }
        let found = self.game_procs(&list);
        // a process whose program can't be read (one running as administrator) that's still alive stays the game
        if found.is_empty() && !living.is_empty() {
            let still: Vec<u32> = living.iter().copied().filter(|pid| list.iter().any(|p| p.pid == *pid && p.exe.is_empty())).collect();
            if !still.is_empty() {
                self.pids = still;
                return;
            }
        }
        if !found.is_empty() {
            self.pids = found.iter().map(|p| p.pid).collect();
            if !self.pids.contains(&self.pid) {
                let p = self.pick(&found);
                self.pid = p.pid;
                out.push(Event::Handover { pid: p.pid, exe: p.exe.clone() });
            }
            self.gone_at = 0;
            return;
        }
        // Steam still says it runs: a game restarting itself gets a few more seconds. Still Steam's after that: an
        // outside launcher's hand-off (a sign-in, an update); back to waiting for the real game
        if steam && !self.spec.appid.is_empty() && w.running_app_id(&self.spec.appid) == Some(self.appid()) {
            if self.gone_at == 0 {
                self.gone_at = now;
            }
            if now - self.gone_at < STEAM_GRACE {
                return;
            }
            if !self.closing {
                self.state = State::Waiting;
                self.t0 = now;
                self.pids.clear();
                self.pid = 0;
                self.gone_at = 0;
                self.appid_at = 0;
                return;
            }
        }
        self.state = State::Ended;
        out.push(Event::Ended { ms: now - self.started_at, cancelled: false });
    }

    /// Does what's due at `now`: a poll, the forced close.
    pub fn run(&mut self, w: &mut dyn World, now: u64) -> Vec<Event> {
        let mut out = Vec::new();
        if self.force_at.is_some_and(|f| f <= now) && !self.over() {
            self.force_at = None;
            let left: Vec<u32> = self.targets(w).into_iter().filter(|p| w.alive(*p)).collect();
            if !left.is_empty() {
                for p in &left {
                    w.kill(*p, true);
                }
                out.push(Event::Forced(left));
            }
        }
        if self.poll_at <= now && !self.over() {
            self.step(w, now, &mut out);
            self.poll_at = now + if self.state == State::Running && !self.pids.is_empty() { ALIVE_POLL } else { WAIT_POLL };
        }
        out
    }

    fn targets(&self, w: &mut dyn World) -> Vec<u32> {
        let mut set = self.pids.clone();
        if !self.spec.dir.is_empty() {
            for p in self.game_procs(&w.list()) {
                if !set.contains(&p.pid) {
                    set.push(p.pid);
                }
            }
        }
        set.retain(|p| *p > 0 && *p != self.own_pid);
        set
    }

    /// Closes the game: its processes are asked to close, and forced `force_after` ms later if still there.
    /// Launchers are never touched. Still waiting for it to show up: the watch just ends (`Ended`, cancelled).
    pub fn close(&mut self, w: &mut dyn World, now: u64, force_after: u64) -> Vec<Event> {
        if self.over() {
            return Vec::new();
        }
        let list = self.targets(w);
        // asked again: asked again, the first deadline kept
        if self.closing {
            for p in list {
                w.kill(p, false);
            }
            return Vec::new();
        }
        self.closing = true;
        if list.is_empty() && self.state == State::Waiting {
            self.state = State::Ended;
            return vec![Event::Ended { ms: 0, cancelled: true }];
        }
        // it showed up just now: followed until it has closed
        if !list.is_empty() && self.state == State::Waiting {
            self.state = State::Running;
            self.started_at = now;
            self.verified_at = now;
            self.pids = list.clone();
            self.pid = list[0];
        }
        for p in &list {
            w.kill(*p, false);
        }
        self.force_at = Some(now + force_after);
        Vec::new()
    }

    /// Stops watching, without a word.
    pub fn stop(&mut self) {
        if !self.over() {
            self.state = State::Stopped;
        }
    }
}

/// The process list as the Windows helper prints it, a `pid|name|program` a line.
pub fn parse_list(out: &str) -> Vec<Proc> {
    static LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\|([^|]*)\|(.*)$").unwrap());
    out.lines()
        .filter_map(|l| {
            let m = LINE.captures(l.trim())?;
            let pid: u32 = m[1].parse().ok().filter(|p| *p > 0)?;
            Some(Proc { pid, name: m[2].trim().to_string(), exe: m[3].trim().to_string() })
        })
        .collect()
}

// ---------- the real readers ----------

/// Steam's own helpers and Wine's (Linux): never taken for the game.
static LINUX_SKIP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(steam|steamwebhelper|reaper|steam-launch-wr.*|pressure-vessel.*|pv-bwrap|pv-adverb|fossilize_repla.*|bwrap|srt-.*|steam-runtime.*|gameoverlayui|wineserver|services\.exe|winedevice\.exe|explorer\.exe|plugplay\.exe|rpcss\.exe|tabtip\.exe|conhost\.exe|boxshelf.*)$").unwrap()
});

/// A process as `/proc` shows it; `appid`: the Steam game it says it is (0: none), read when `env` is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxProc {
    pub pid: u32,
    pub name: String,
    pub exe: String,
    pub appid: u64,
    pub skip: bool,
}

/// The processes under `proc_dir` (`/proc`). Steam starts each game with SteamAppId (or SteamGameId) in its
/// environment; a Proton game runs as wine with the game's Windows path (`Z:\home\...`) on its command line, which
/// is turned back into the Linux path.
pub fn linux_procs(env: bool, proc_dir: &std::path::Path) -> Vec<LinuxProc> {
    static WIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z]:\\.+\.exe$").unwrap());
    static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|\x00)Steam(?:App|Game)Id=(\d+)").unwrap());
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(proc_dir) else { return out };
    let mut pids: Vec<u32> = dir.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.parse().ok())).collect();
    pids.sort_unstable();
    for pid in pids {
        let d = proc_dir.join(pid.to_string());
        let Ok(comm) = std::fs::read_to_string(d.join("comm")) else { continue };
        let name = comm.trim().to_string();
        if LINUX_SKIP.is_match(&name) {
            out.push(LinuxProc { pid, name, exe: String::new(), appid: 0, skip: true });
            continue;
        }
        let read = |f: &str| std::fs::read(d.join(f)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let cmdline = read("cmdline");
        let exe = std::fs::read_link(d.join("exe")).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let unix = cmdline.split('\0').find(|a| WIN.is_match(a)).filter(|a| a.to_lowercase().starts_with("z:\\")).map(|a| format!("/{}", a[3..].replace('\\', "/")));
        let appid = if env { ID.captures(&read("environ")).and_then(|m| m[1].parse().ok()).unwrap_or(0) } else { 0 };
        out.push(LinuxProc { pid, name, exe: unix.unwrap_or(exe), appid, skip: false });
    }
    out
}

/// This computer, for the watcher.
pub struct System;

fn hidden(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(cmd, 0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(not(windows))]
    let _ = cmd;
}

fn output_of(mut cmd: std::process::Command) -> String {
    hidden(&mut cmd);
    cmd.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

impl World for System {
    fn running_app_id(&mut self, want: &str) -> Option<u64> {
        if cfg!(target_os = "linux") {
            // RustyShelf's own id, when Steam started RustyShelf, never counts
            let own: Vec<u64> = ["SteamAppId", "SteamGameId"].iter().filter_map(|k| std::env::var(k).ok()?.parse().ok()).filter(|n| *n != 0).collect();
            let ids: Vec<u64> = linux_procs(true, std::path::Path::new("/proc")).into_iter().filter(|p| p.appid != 0 && !p.skip && p.pid != std::process::id() && !own.contains(&p.appid)).map(|p| p.appid).collect();
            let want: u64 = want.parse().unwrap_or(0);
            return Some(if want != 0 && ids.contains(&want) { want } else { ids.first().copied().unwrap_or(0) });
        }
        if !cfg!(windows) {
            return None;
        }
        static VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)RunningAppID\s+REG_DWORD\s+0x([0-9a-f]+)").unwrap());
        let mut cmd = std::process::Command::new("reg");
        cmd.args(["query", r"HKCU\Software\Valve\Steam", "/v", "RunningAppID"]);
        VALUE.captures(&output_of(cmd)).and_then(|m| u64::from_str_radix(&m[1], 16).ok())
    }

    fn list(&mut self) -> Vec<Proc> {
        if cfg!(target_os = "linux") {
            return linux_procs(false, std::path::Path::new("/proc")).into_iter().filter(|p| !p.skip).map(|p| Proc { pid: p.pid, name: p.name, exe: p.exe }).collect();
        }
        if !cfg!(windows) {
            return Vec::new();
        }
        const LIST: &str = "[Console]::OutputEncoding = [Text.Encoding]::UTF8; Get-CimInstance Win32_Process -Property ProcessId,Name,ExecutablePath | ForEach-Object { [string]$_.ProcessId + '|' + $_.Name + '|' + $_.ExecutablePath }";
        let mut cmd = std::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", LIST]);
        parse_list(&output_of(cmd))
    }

    fn alive(&mut self, pid: u32) -> bool {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void;
                fn GetExitCodeProcess(process: *mut std::ffi::c_void, code: *mut u32) -> i32;
                fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                fn GetLastError() -> u32;
            }
            const QUERY_LIMITED: u32 = 0x1000;
            const ACCESS_DENIED: u32 = 5;
            const STILL_ACTIVE: u32 = 259;
            // SAFETY: a process opened only to ask whether it has ended; its handle is closed again
            unsafe {
                let h = OpenProcess(QUERY_LIMITED, 0, pid);
                if h.is_null() {
                    return GetLastError() == ACCESS_DENIED;
                }
                let mut code = 0;
                let ok = GetExitCodeProcess(h, &mut code);
                CloseHandle(h);
                ok != 0 && code == STILL_ACTIVE
            }
        }
        #[cfg(not(windows))]
        {
            std::path::Path::new("/proc").join(pid.to_string()).exists()
        }
    }

    fn kill(&mut self, pid: u32, force: bool) {
        let mut cmd;
        if cfg!(windows) {
            cmd = std::process::Command::new("taskkill");
            cmd.args(["/PID", &pid.to_string()]);
            if force {
                cmd.args(["/T", "/F"]);
            }
        } else {
            cmd = std::process::Command::new("kill");
            cmd.args([if force { "-KILL" } else { "-TERM" }, &pid.to_string()]);
        }
        hidden(&mut cmd);
        cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        if let Ok(mut child) = cmd.spawn() {
            std::thread::spawn(move || child.wait());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEAM: &str = r"C:\Program Files (x86)\Steam";
    const GAME: &str = r"C:\Program Files (x86)\Steam\steamapps\common\Cobalt Drift";

    /// A pretend computer and clock.
    #[derive(Default)]
    struct Fake {
        t: u64,
        appid: u64,
        list: Vec<Proc>,
        dead: Vec<u32>,
        kills: Vec<(u32, bool)>,
        fail_list: bool,
        reads: u32,
        lists: u32,
        alives: u32,
        events: Vec<Event>,
    }

    impl World for Fake {
        fn running_app_id(&mut self, _: &str) -> Option<u64> {
            self.reads += 1;
            Some(self.appid)
        }
        fn list(&mut self) -> Vec<Proc> {
            self.lists += 1;
            if self.fail_list {
                return Vec::new();
            }
            self.list.iter().filter(|p| !self.dead.contains(&p.pid)).cloned().collect()
        }
        fn alive(&mut self, pid: u32) -> bool {
            self.alives += 1;
            self.list.iter().any(|p| p.pid == pid) && !self.dead.contains(&pid)
        }
        fn kill(&mut self, pid: u32, force: bool) {
            self.kills.push((pid, force));
        }
    }

    fn proc(pid: u32, name: &str, exe: String) -> Proc {
        Proc { pid, name: name.to_string(), exe }
    }

    fn world() -> Fake {
        Fake {
            t: 1_000_000,
            list: vec![
                proc(10, "steam.exe", format!(r"{STEAM}\steam.exe")),
                proc(11, "steamwebhelper.exe", format!(r"{STEAM}\bin\cef\cef.win7x64\steamwebhelper.exe")),
                proc(12, "explorer.exe", r"C:\Windows\explorer.exe".into()),
            ],
            ..Fake::default()
        }
    }

    fn advance(f: &mut Fake, w: &mut Watch, ms: u64) {
        let end = f.t + ms;
        while let Some(at) = w.due().filter(|at| *at <= end) {
            f.t = f.t.max(at);
            let now = f.t;
            let ev = w.run(f, now);
            f.events.extend(ev);
        }
        f.t = end;
    }

    fn watch(f: &Fake, kind: &str, appid: &str, dir: &str, exe: &str) -> Watch {
        Watch::new(Spec { kind: kind.into(), appid: appid.into(), dir: dir.into(), exe: exe.into(), title: String::new() }, f.t, 999)
    }

    #[test]
    fn steam_game_starts_and_ends() {
        let mut f = world();
        let mut w = watch(&f, "steam", "400000", GAME, "");
        advance(&mut f, &mut w, 4100);
        assert!(w.state == State::Waiting && f.events.is_empty());
        f.appid = 400_000;
        f.list.push(proc(501, "UnityCrashHandler64.exe", format!(r"{GAME}\UnityCrashHandler64.exe")));
        f.list.push(proc(500, "Cobalt.exe", format!(r"{GAME}\Cobalt.exe")));
        advance(&mut f, &mut w, 2000);
        // the game's own process, not its crash handler, not Steam
        assert_eq!(f.events, [Event::Started { pid: 500, exe: format!(r"{GAME}\Cobalt.exe"), name: "Cobalt.exe".into() }]);
        let lists = f.lists;
        advance(&mut f, &mut w, 5000);
        assert!(f.lists == lists && f.alives > 0, "only alive checks while it runs");
        f.dead.extend([500, 501]);
        f.appid = 0;
        advance(&mut f, &mut w, 600);
        assert!(matches!(f.events[1], Event::Ended { ms, cancelled: false } if ms >= 5000), "{:?}", f.events);
        let before = (f.reads, f.lists, f.alives);
        advance(&mut f, &mut w, 30_000);
        assert!(w.due().is_none() && before == (f.reads, f.lists, f.alives), "nothing polls after the end");
    }

    #[test]
    fn gives_up_on_a_game_that_never_shows() {
        let mut f = world();
        let mut w = watch(&f, "epic", "", r"D:\Epic Games\Ember", r"D:\Epic Games\Ember\Ember.exe");
        advance(&mut f, &mut w, 88_000);
        assert!(f.events.is_empty());
        advance(&mut f, &mut w, 4000);
        assert_eq!(f.events, [Event::Timeout]);
        assert!(w.state == State::Stopped && w.due().is_none());
    }

    #[test]
    fn follows_a_games_own_launcher_to_the_game() {
        let mut f = world();
        let dir = r"E:\Ubisoft\Harbor Run";
        f.list.push(proc(50, "upc.exe", r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\upc.exe".into()));
        f.list.push(proc(600, "HarborLauncher.exe", format!(r"{dir}\HarborLauncher.exe")));
        let mut w = watch(&f, "ubisoft", "", dir, "");
        advance(&mut f, &mut w, 2100);
        f.dead.push(600);
        f.list.push(proc(601, "HarborRun.exe", format!(r"{dir}\bin\HarborRun.exe")));
        advance(&mut f, &mut w, 1000);
        assert_eq!(f.events.len(), 2);
        assert!(matches!(f.events[0], Event::Started { pid: 600, .. }) && matches!(f.events[1], Event::Handover { pid: 601, .. }));
        assert!(w.state == State::Running && w.pid == 601);
        // a process listing that fails while the game runs is no end
        f.fail_list = true;
        advance(&mut f, &mut w, 16_000);
        assert_eq!(w.state, State::Running);
        f.fail_list = false;
        f.dead.push(601);
        advance(&mut f, &mut w, 600);
        assert!(matches!(f.events.last(), Some(Event::Ended { .. })));
    }

    #[test]
    fn close_asks_then_forces_and_never_touches_launchers() {
        let mut f = world();
        f.list.push(proc(20, "EpicGamesLauncher.exe", format!(r"{GAME}\EpicGamesLauncher.exe")));
        f.list.push(proc(999, "RustyShelf.exe", format!(r"{GAME}\RustyShelf.exe")));
        f.list.push(proc(500, "Cobalt.exe", format!(r"{GAME}\Cobalt.exe")));
        f.list.push(proc(502, "CobaltHelper.exe", format!(r"{GAME}\tools\CobaltHelper.exe")));
        let mut w = watch(&f, "steam", "400000", GAME, "");
        advance(&mut f, &mut w, 2100);
        let now = f.t;
        w.close(&mut f, now, 8000);
        let mut gentle = f.kills.clone();
        gentle.sort();
        assert_eq!(gentle, [(500, false), (502, false)]);
        f.dead.push(502);
        advance(&mut f, &mut w, 8100);
        assert!(f.kills.contains(&(500, true)) && !f.kills.contains(&(502, true)));
        assert!(f.events.contains(&Event::Forced(vec![500])));
        assert!(![10, 11, 12, 20, 999].iter().any(|p| f.kills.iter().any(|k| k.0 == *p)), "{:?}", f.kills);
        f.dead.push(500);
        advance(&mut f, &mut w, 600);
        assert!(matches!(f.events.last(), Some(Event::Ended { .. })) && w.due().is_none());
    }

    #[test]
    fn close_before_the_game_showed_up() {
        let mut f = world();
        let mut w = watch(&f, "epic", "", r"D:\Epic Games\Ember", "");
        advance(&mut f, &mut w, 2100);
        let now = f.t;
        let ev = w.close(&mut f, now, 8000);
        advance(&mut f, &mut w, 10_000);
        assert_eq!(ev, [Event::Ended { ms: 0, cancelled: true }]);
        assert!(f.kills.is_empty() && w.due().is_none());
    }

    #[test]
    fn steam_without_an_install_folder() {
        let mut f = world();
        let mut w = watch(&f, "steam", "400001", "", "");
        advance(&mut f, &mut w, 2100);
        f.appid = 400_001;
        advance(&mut f, &mut w, 2000);
        assert_eq!(f.events, [Event::Started { pid: 0, exe: String::new(), name: String::new() }]);
        f.appid = 0;
        advance(&mut f, &mut w, 2000);
        assert_eq!(f.events.len(), 1, "Steam must say so twice");
        advance(&mut f, &mut w, 2000);
        assert!(matches!(f.events[1], Event::Ended { .. }));
        assert_eq!(f.lists, 0);
    }

    #[test]
    fn parsers_and_folders() {
        let l = parse_list("12|steam.exe|C:\\Steam\\steam.exe\r\n500|Cobalt.exe|D:\\Games\\Cobalt\\Cobalt.exe\r\n4||\r\njunk\r\n");
        assert_eq!(l.len(), 3);
        assert_eq!((l[1].pid, l[1].exe.as_str(), l[2].exe.as_str()), (500, r"D:\Games\Cobalt\Cobalt.exe", ""));
        assert!(under(r"D:\Games\Cobalt", r"d:\games\cobalt\bin\x.exe"));
        assert!(!under(r"D:\Games\Cobalt", r"D:\Games\Cobalt Drift\x.exe") && !under("", r"D:\x.exe"));
        assert!(under("/home/u/Games/Cobalt/", "/home/u/Games/Cobalt/bin/../cobalt.x86_64"));
        assert!(["steam.exe", "EpicGamesLauncher.exe", "upc.exe", "EADesktop.exe", "GalaxyClient.exe", "XboxPcApp.exe"].iter().all(|n| is_launcher(n)));
        assert!(!is_launcher("Cobalt.exe"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn watches_a_real_process() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        std::fs::create_dir_all(&game).unwrap();
        let exe = game.join("cobalt");
        std::fs::copy("/bin/sleep", &exe).unwrap();
        let mut child = std::process::Command::new(&exe).arg("30").spawn().unwrap();
        let mut sys = System;
        let spec = Spec { kind: "epic".into(), dir: game.to_string_lossy().into_owned(), ..Spec::default() };
        let mut w = Watch::new(spec, 0, std::process::id());
        let started = w.run(&mut sys, WAIT_POLL);
        assert!(matches!(&started[..], [Event::Started { pid, .. }] if *pid == child.id()), "{started:?}");
        assert!(w.run(&mut sys, WAIT_POLL + ALIVE_POLL).is_empty());
        w.close(&mut sys, WAIT_POLL + ALIVE_POLL, 8000);
        child.wait().unwrap();
        let ended = w.run(&mut sys, WAIT_POLL + 2 * ALIVE_POLL);
        assert!(matches!(&ended[..], [Event::Ended { cancelled: false, .. }]), "{ended:?}");
    }

    #[test]
    fn reads_proc() {
        let tmp = tempfile::tempdir().unwrap();
        let put = |pid: &str, f: &str, text: &str| {
            std::fs::create_dir_all(tmp.path().join(pid)).unwrap();
            std::fs::write(tmp.path().join(pid).join(f), text).unwrap();
        };
        put("40", "comm", "steam\n");
        put("41", "comm", "Cobalt.exe\n");
        put("41", "cmdline", "wine64\0Z:\\home\\u\\Games\\Cobalt\\Cobalt.exe\0-fullscreen\0");
        put("41", "environ", "HOME=/home/u\0SteamAppId=400000\0");
        put("self", "comm", "x\n");
        let l = linux_procs(true, tmp.path());
        assert_eq!(l.len(), 2);
        assert!(l[0].skip && l[0].pid == 40);
        assert_eq!((l[1].exe.as_str(), l[1].appid, l[1].skip), ("/home/u/Games/Cobalt/Cobalt.exe", 400_000, false));
        assert_eq!(linux_procs(false, tmp.path())[1].appid, 0);
    }
}

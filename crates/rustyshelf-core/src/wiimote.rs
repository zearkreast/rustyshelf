// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/wiimote.js, as RustyShelf has it.
//! The Wii remote (Windows): wiimote.cs, run in a hidden Windows PowerShell (compiled once, cached as a DLL in
//! data/wiimote-cache, as the Electron app caches it), reports what a remote paired to a Mayflash DolphinBar in its
//! Dolphin mode does. `pause` lets go of it before Dolphin starts a game, `resume` takes it back after, and `watch`
//! listens to it alongside Dolphin (read-only) for its buttons.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::Duration;

const SOURCE: &str = include_str!("wiimote.cs");

/// The remote's buttons, as the reader's bits.
pub mod btn {
    pub const LEFT: u16 = 0x0100;
    pub const RIGHT: u16 = 0x0200;
    pub const DOWN: u16 = 0x0400;
    pub const UP: u16 = 0x0800;
    pub const PLUS: u16 = 0x1000;
    pub const TWO: u16 = 0x0001;
    pub const ONE: u16 = 0x0002;
    pub const B: u16 = 0x0004;
    pub const A: u16 = 0x0008;
    pub const MINUS: u16 = 0x0010;
    pub const HOME: u16 = 0x0080;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Ready,
    Connected(bool),
    Paused(bool),
    /// the buttons, and where it points: 0..1 of the camera's view; `seen`: the bar's lights are in view
    State { buttons: u16, x: f32, y: f32, seen: bool },
    /// during a Dolphin game (`watch`): the buttons from the game's own reports
    Watch(u16),
    /// the reader ended on its own, and what it said
    Ended(String),
}

/// One line the reader prints.
pub fn parse_line(line: &str) -> Option<Event> {
    let l = line.trim();
    match l {
        "READY" => return Some(Event::Ready),
        "P 1" => return Some(Event::Paused(true)),
        "P 0" => return Some(Event::Paused(false)),
        "C 1" => return Some(Event::Connected(true)),
        "C 0" => return Some(Event::Connected(false)),
        _ => {}
    }
    let parts: Vec<&str> = l.split(' ').collect();
    match parts.as_slice() {
        ["H", b] => b.parse().ok().map(Event::Watch),
        ["S", b, x, y, seen @ ("0" | "1")] => Some(Event::State { buttons: b.parse().ok()?, x: x.parse().ok()?, y: y.parse().ok()?, seen: *seen == "1" }),
        _ => None,
    }
}

#[derive(Default)]
struct Run {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stopping: bool,
}

pub struct Wiimote {
    run: Mutex<Run>,
    tx: Sender<Event>,
    rx: Mutex<Receiver<Event>>,
    want_paused: AtomicBool,
    paused_now: Mutex<bool>,
    paused_cv: Condvar,
}

/// The one reader RustyShelf runs.
pub static REMOTE: LazyLock<Wiimote> = LazyLock::new(|| {
    let (tx, rx) = channel();
    Wiimote { run: Mutex::new(Run::default()), tx, rx: Mutex::new(rx), want_paused: AtomicBool::new(false), paused_now: Mutex::new(false), paused_cv: Condvar::new() }
});

fn dll_path(cache_dir: &Path) -> PathBuf {
    use sha1::Digest;
    let h = crate::files::hex(&sha1::Sha1::digest(SOURCE.as_bytes()));
    cache_dir.join(format!("wm-{}.dll", &h[..12]))
}

/// PowerShell's single-quoted string.
fn psq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

impl Wiimote {
    /// Starts the reader (Windows); false when it can't be.
    pub fn start(&'static self, cache_dir: &Path) -> bool {
        let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
        if !cfg!(windows) || run.child.is_some() {
            return false;
        }
        run.stopping = false;
        let dll = dll_path(cache_dir);
        let cs = std::env::temp_dir().join(format!("rustyshelf-wm-{}.cs", std::process::id()));
        let _ = std::fs::create_dir_all(cache_dir);
        // old builds of the reader (one in use can't be deleted: skipped)
        for e in std::fs::read_dir(cache_dir).into_iter().flatten().flatten() {
            if e.path() != dll {
                let _ = std::fs::remove_file(e.path());
            }
        }
        if !dll.exists() && std::fs::write(&cs, SOURCE).is_err() {
            return false;
        }
        let d = dll.to_string_lossy();
        let script = format!(
            "$ErrorActionPreference = 'Stop'\n$dll = {}\nif (-not (Test-Path -LiteralPath $dll)) {{ Add-Type -Path {} -OutputAssembly $dll -OutputType Library; Remove-Item -LiteralPath {} -ErrorAction SilentlyContinue }}\nAdd-Type -Path $dll\n[WMH]::Run()",
            psq(&d),
            psq(&cs.to_string_lossy()),
            psq(&cs.to_string_lossy())
        );
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-WindowStyle", "Hidden", "-Command", &script]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
        let Ok(mut child) = cmd.spawn() else { return false };
        let (out, err) = (child.stdout.take(), child.stderr.take());
        run.stdin = child.stdin.take();
        run.child = Some(child);
        let said = std::sync::Arc::new(Mutex::new(String::new()));
        if let Some(err) = err {
            let said = said.clone();
            std::thread::spawn(move || {
                for l in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                    let mut s = said.lock().unwrap();
                    if s.len() < 2000 {
                        s.push_str(&l);
                        s.push('\n');
                    }
                }
            });
        }
        if let Some(out) = out {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(e) = parse_line(&l) {
                        if let Event::Paused(p) = e {
                            *REMOTE.paused_now.lock().unwrap() = p;
                            REMOTE.paused_cv.notify_all();
                        }
                        let _ = tx.send(e);
                    }
                }
                REMOTE.ended(&said.lock().unwrap());
            });
        }
        drop(run);
        if self.want_paused.load(Ordering::Relaxed) {
            self.command("PAUSE");
        }
        true
    }

    fn ended(&self, said: &str) {
        let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut c) = run.child.take() {
            let _ = c.wait();
        }
        run.stdin = None;
        *self.paused_now.lock().unwrap() = false;
        self.paused_cv.notify_all();
        let _ = self.tx.send(Event::Connected(false));
        if !run.stopping {
            let _ = self.tx.send(Event::Ended(said.lines().take(3).collect::<Vec<_>>().join(" | ")));
        }
    }

    /// What the reader said since the last call.
    pub fn events(&self) -> Vec<Event> {
        self.rx.lock().map(|rx| rx.try_iter().collect()).unwrap_or_default()
    }

    fn command(&self, c: &str) {
        let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stdin) = run.stdin.as_mut() {
            let _ = writeln!(stdin, "{c}").and_then(|_| stdin.flush());
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.lock().is_ok_and(|r| r.child.is_some())
    }

    /// Lets go of the remote, waiting until it has (a second at the most); Dolphin can then connect to it.
    pub fn pause(&self) {
        self.want_paused.store(true, Ordering::Relaxed);
        if !self.is_running() {
            return;
        }
        self.command("PAUSE");
        let now = self.paused_now.lock().unwrap();
        let _ = self.paused_cv.wait_timeout_while(now, Duration::from_secs(1), |p| !*p);
    }

    pub fn resume(&self) {
        self.want_paused.store(false, Ordering::Relaxed);
        self.command("RESUME");
    }

    pub fn is_paused(&self) -> bool {
        self.want_paused.load(Ordering::Relaxed)
    }

    /// While paused (a Dolphin game): listens to the remote alongside Dolphin, read-only, for its buttons.
    pub fn watch(&self) {
        if self.is_paused() {
            self.command("WATCH");
        }
    }

    pub fn rumble(&self, ms: u32) {
        self.command(&format!("RUMBLE {ms}"));
    }

    pub fn stop(&self) {
        let child = {
            let mut run = self.run.lock().unwrap_or_else(|e| e.into_inner());
            run.stopping = true;
            run.child.take()
        };
        self.command("QUIT");
        if let Some(mut c) = child {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(800));
                let _ = c.kill();
                let _ = c.wait();
            });
        }
    }
}

/// Dolphin has the remote for its own games: a GameCube or Wii game, or one a Dolphin build starts.
pub fn for_dolphin(console: &str, exe: &Path) -> bool {
    matches!(console, "gc" | "wii") || exe.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase().contains("dolphin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        assert_eq!(parse_line("READY"), Some(Event::Ready));
        assert_eq!(parse_line("C 1"), Some(Event::Connected(true)));
        assert_eq!(parse_line("P 0"), Some(Event::Paused(false)));
        assert_eq!(parse_line("H 128"), Some(Event::Watch(btn::HOME)));
        assert_eq!(parse_line("S 8 0.5000 0.2500 1"), Some(Event::State { buttons: btn::A, x: 0.5, y: 0.25, seen: true }));
        assert_eq!(parse_line("S 8 0.5 0.25 2"), None);
        assert_eq!(parse_line("noise"), None);
        assert!(for_dolphin("wii", Path::new("x")) && for_dolphin("nes", Path::new("C:/Dolphin/Dolphin.exe")) && !for_dolphin("nes", Path::new("retroarch.exe")));
        assert_eq!(psq("it's"), "'it''s'");
    }
}

// Ported from main/epicdl.js, as RustyShelf has it. The idea of a queue with pause / resume / stop comes from
// Playnite's Unified Download Manager add-on (MIT) and the TyroShelf fork's download manager.
//! (Linux) The Epic download manager: a queue of `legendary install` runs, one at a time. Each item shows how far it
//! is, its speed and the time left; it can be paused (legendary is stopped: it keeps what it downloaded and goes on
//! from there when resumed), stopped, moved up the queue or removed. The queue is kept in data/epic-downloads.json,
//! as the Electron app keeps it, so a restart finds it as it was (an install that was running comes back paused).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde_json::{json, Value};

use crate::js;
use crate::linuxpc::{self, Proc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Installing,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Installing => "installing",
            State::Paused => "paused",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    fn from_name(s: &str) -> Option<State> {
        [State::Queued, State::Installing, State::Paused, State::Done, State::Failed, State::Cancelled].into_iter().find(|x| x.name() == s)
    }

    /// waiting, running or paused: not over
    pub fn live(self) -> bool {
        matches!(self, State::Queued | State::Installing | State::Paused)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u64,
    pub app_name: String,
    pub title: String,
    pub base_dir: String,
    pub update: bool,
    pub state: State,
    pub pct: f64,
    pub speed: String,
    pub eta: String,
    pub error: String,
    /// ms since 1970
    pub added: u64,
}

type Tell<T> = Box<dyn Fn(&T) + Send + Sync>;

/// What a manager is handed: where legendary is, and who to tell.
pub struct Hooks {
    pub bin: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    /// the list after a change
    pub on_change: Tell<[Item]>,
    /// an install ended (finished, failed or stopped)
    pub on_settled: Tell<Item>,
}

struct Running {
    id: u64,
    proc: Option<Proc>,
    /// the folders in its games folder before it began: the new ones are its own
    before: Option<HashSet<String>>,
    base_dir: String,
}

struct Inner {
    items: Vec<Item>,
    seq: u64,
    cur: Option<Running>,
    saved_at: Option<Instant>,
}

#[derive(Clone)]
pub struct Downloads(Arc<Shared>);

struct Shared {
    file: PathBuf,
    data_dir: PathBuf,
    hooks: Hooks,
    inner: Mutex<Inner>,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

fn list_dir(d: &str) -> HashSet<String> {
    std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
}

impl Downloads {
    /// The queue kept in `data_dir/epic-downloads.json`.
    pub fn new(data_dir: &Path, hooks: Hooks) -> Downloads {
        let file = data_dir.join("epic-downloads.json");
        let items = load(&file);
        let seq = items.iter().map(|i| i.id).max().unwrap_or(0);
        let d = Downloads(Arc::new(Shared { file, data_dir: data_dir.to_path_buf(), hooks, inner: Mutex::new(Inner { items, seq, cur: None, saved_at: None }) }));
        if d.lock().items.iter().any(|i| i.state == State::Paused) {
            d.save(&mut d.lock(), true);
        }
        d
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Progress arrives many times a second: it's written at most every 300 ms, the rest at once.
    fn save(&self, i: &mut Inner, force: bool) {
        if !force && i.saved_at.is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        i.saved_at = Some(Instant::now());
        let d: Vec<Value> = i
            .items
            .iter()
            .map(|x| json!({ "id": x.id, "appName": x.app_name, "title": x.title, "baseDir": x.base_dir, "update": x.update, "status": x.state.name(), "pct": js::num(x.pct), "error": x.error, "added": x.added }))
            .collect();
        let tmp = PathBuf::from(format!("{}.tmp", self.0.file.display()));
        let _ = self.0.file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(&tmp, Value::Array(d).to_string()).and_then(|_| std::fs::rename(&tmp, &self.0.file));
    }

    fn changed(&self, mut i: MutexGuard<'_, Inner>, force: bool) {
        self.save(&mut i, force);
        let list = i.items.clone();
        drop(i);
        (self.0.hooks.on_change)(&list);
    }

    pub fn list(&self) -> Vec<Item> {
        self.lock().items.clone()
    }

    /// The install that's running.
    pub fn current(&self) -> Option<Item> {
        let i = self.lock();
        let id = i.cur.as_ref()?.id;
        i.items.iter().find(|x| x.id == id).cloned()
    }

    /// The folders of the install going on now.
    pub fn unfinished_dirs(&self) -> Vec<String> {
        let i = self.lock();
        i.cur.as_ref().map(new_dirs).unwrap_or_default()
    }

    fn pump(&self) {
        let next = {
            let i = self.lock();
            if i.cur.is_some() {
                return;
            }
            i.items.iter().find(|x| x.state == State::Queued).map(|x| x.id)
        };
        if let Some(id) = next {
            self.begin(id);
        }
    }

    fn begin(&self, id: u64) {
        let bin = (self.0.hooks.bin)();
        let mut i = self.lock();
        let Some(it) = i.items.iter_mut().find(|x| x.id == id) else { return };
        let Some(bin) = bin else {
            it.state = State::Failed;
            it.error = "legendary not found: Settings → PC games → Get legendary.".into();
            self.changed(i, true);
            return self.pump();
        };
        let (update, base_dir, app) = (it.update, it.base_dir.clone(), it.app_name.clone());
        if !update {
            if let Err(e) = std::fs::create_dir_all(&base_dir) {
                it.state = State::Failed;
                it.error = js::slice(&format!("Can't use {base_dir} for games: {e}"), 200).to_string();
                self.changed(i, true);
                return self.pump();
            }
        }
        (it.state, it.error, it.speed, it.eta) = (State::Installing, String::new(), String::new(), String::new());
        i.cur = Some(Running { id, proc: None, before: (!update).then(|| list_dir(&base_dir)), base_dir: base_dir.clone() });
        self.changed(i, true);
        let args: Vec<String> = if update {
            ["install", &app, "--update-only", "--skip-dlcs", "--skip-sdl", "-y"].iter().map(|s| s.to_string()).collect()
        } else {
            ["install", &app, "--base-path", &base_dir, "--platform", "Windows", "--skip-dlcs", "--skip-sdl", "-y"].iter().map(|s| s.to_string()).collect()
        };
        let me = self.clone();
        std::thread::spawn(move || {
            let on_line = |l: &str| me.progress(id, l);
            let r = linuxpc::run_legendary(&bin, &args, &me.0.data_dir, None, &mut { on_line }, &mut |p| {
                if let Some(c) = me.lock().cur.as_mut().filter(|c| c.id == id) {
                    c.proc = Some(p);
                }
            });
            me.ended(id, &app, r);
        });
    }

    fn progress(&self, id: u64, line: &str) {
        static PROGRESS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"Progress:\s*([\d.]+)%.*?ETA:\s*([\d:]+)").unwrap());
        static SPEED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\+ Download\s*-\s*([\d.]+ \w+/s)").unwrap());
        let mut i = self.lock();
        let Some(it) = i.items.iter_mut().find(|x| x.id == id) else { return };
        if let Some(m) = PROGRESS.captures(line) {
            it.pct = m[1].parse::<f64>().unwrap_or(0.0).min(100.0);
            it.eta = m[2].to_string();
            self.changed(i, false);
        } else if let Some(m) = SPEED.captures(line) {
            it.speed = m[1].to_string();
        }
    }

    fn ended(&self, id: u64, app: &str, r: linuxpc::Ran) {
        static ERROR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ERROR: (.+)").unwrap());
        let mut i = self.lock();
        let run = i.cur.take().filter(|c| c.id == id);
        let dirs = run.as_ref().map(new_dirs).unwrap_or_default();
        let was = i.items.iter().find(|x| x.id == id).map_or(State::Cancelled, |x| x.state);
        let finished = was == State::Installing && r.code == 0;
        linuxpc::note_unfinished(&self.0.data_dir, &dirs, app, finished);
        let settled = i.items.iter_mut().find(|x| x.id == id).map(|it| {
            if was == State::Installing {
                if r.code == 0 {
                    (it.state, it.pct) = (State::Done, 100.0);
                } else {
                    it.state = State::Failed;
                    it.error = ERROR.captures_iter(&r.err).last().map_or_else(|| "The install stopped.".to_string(), |m| js::slice(m[1].trim_end(), 200).to_string());
                }
            }
            (it.speed, it.eta) = (String::new(), String::new());
            it.clone()
        });
        self.changed(i, true);
        if let Some(it) = settled {
            (self.0.hooks.on_settled)(&it);
        }
        self.pump();
    }

    /// A game to install (or update), at the end of the queue; one already waiting or running isn't added twice, a
    /// paused one goes on.
    pub fn add(&self, app_name: &str, title: &str, base_dir: &str, update: bool) -> Result<Item, String> {
        if !linuxpc::is_app_name(app_name) {
            return Err("Unknown Epic game.".into());
        }
        let mut i = self.lock();
        if let Some(live) = i.items.iter().find(|x| x.app_name == app_name && x.state.live()).cloned() {
            drop(i);
            if live.state == State::Paused {
                self.resume(live.id);
            }
            return Ok(live);
        }
        i.items.retain(|x| x.app_name != app_name);
        i.seq += 1;
        let title = if title.is_empty() { app_name } else { title };
        let it = Item { id: i.seq, app_name: app_name.into(), title: js::slice(title, 120).into(), base_dir: base_dir.into(), update, state: State::Queued, pct: 0.0, speed: String::new(), eta: String::new(), error: String::new(), added: now_ms() };
        i.items.push(it.clone());
        self.changed(i, true);
        self.pump();
        Ok(self.list().into_iter().find(|x| x.id == it.id).unwrap_or(it))
    }

    pub fn pause(&self, id: u64) -> bool {
        let mut i = self.lock();
        let Some(it) = i.items.iter_mut().find(|x| x.id == id) else { return false };
        match it.state {
            State::Installing => {
                it.state = State::Paused;
                let p = i.cur.as_ref().filter(|c| c.id == id).and_then(|c| c.proc.clone());
                self.changed(i, true);
                if let Some(p) = p {
                    p.stop();
                }
                true
            }
            State::Queued => {
                it.state = State::Paused;
                self.changed(i, true);
                true
            }
            _ => false,
        }
    }

    pub fn resume(&self, id: u64) -> bool {
        let mut i = self.lock();
        let Some(it) = i.items.iter_mut().find(|x| x.id == id && matches!(x.state, State::Paused | State::Failed | State::Cancelled)) else { return false };
        (it.state, it.error) = (State::Queued, String::new());
        self.changed(i, true);
        self.pump();
        true
    }

    /// Like Cancel: it stays in the list, and can be resumed (legendary goes on from where it stopped).
    pub fn stop(&self, id: u64) -> bool {
        let mut i = self.lock();
        let Some(it) = i.items.iter_mut().find(|x| x.id == id && x.state.live()) else { return false };
        let was = it.state;
        it.state = State::Cancelled;
        let p = (was == State::Installing).then(|| i.cur.as_ref().filter(|c| c.id == id).and_then(|c| c.proc.clone())).flatten();
        self.changed(i, true);
        if let Some(p) = p {
            p.stop();
        }
        true
    }

    pub fn cancel_current(&self) -> bool {
        self.current().is_some_and(|c| self.stop(c.id))
    }

    pub fn remove(&self, id: u64) -> bool {
        let mut i = self.lock();
        let Some(at) = i.items.iter().position(|x| x.id == id) else { return false };
        let p = (i.items[at].state == State::Installing).then(|| i.cur.as_ref().filter(|c| c.id == id).and_then(|c| c.proc.clone())).flatten();
        i.items.remove(at);
        self.changed(i, true);
        if let Some(p) = p {
            p.stop();
        }
        true
    }

    /// Among the ones that haven't started: up (-1) or down (+1).
    pub fn move_by(&self, id: u64, delta: i32) -> bool {
        let mut i = self.lock();
        let Some(idx) = i.items.iter().position(|x| x.id == id && !matches!(x.state, State::Installing | State::Done)) else { return false };
        let step: isize = if delta < 0 { -1 } else { 1 };
        let mut j = idx as isize + step;
        while i.items.get(j as usize).is_some_and(|x| j >= 0 && matches!(x.state, State::Installing | State::Done)) {
            j += step;
        }
        if j < 0 || j as usize >= i.items.len() {
            return false;
        }
        i.items.swap(idx, j as usize);
        self.changed(i, true);
        true
    }

    /// Takes the finished, failed and stopped ones off; how many went.
    pub fn clear_done(&self) -> usize {
        let mut i = self.lock();
        let n = i.items.len();
        i.items.retain(|x| x.state.live());
        let gone = n - i.items.len();
        if gone > 0 {
            self.changed(i, true);
        }
        gone
    }

    /// Starts the first waiting install, when none runs (the queue as it was found at the start).
    pub fn start(&self) {
        self.pump();
    }
}

fn new_dirs(c: &Running) -> Vec<String> {
    let Some(before) = &c.before else { return Vec::new() };
    let mut d: Vec<String> = list_dir(&c.base_dir).into_iter().filter(|n| !before.contains(n)).map(|n| Path::new(&c.base_dir).join(n).to_string_lossy().into_owned()).collect();
    d.sort();
    d
}

fn load(file: &Path) -> Vec<Item> {
    let Some(Value::Array(j)) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()) else { return Vec::new() };
    let kept: Vec<&Value> = j.iter().filter(|i| i.is_object() && linuxpc::is_app_name(&js::string(i.get("appName")))).collect();
    let mut seq = 0;
    kept[kept.len().saturating_sub(200)..]
        .iter()
        .map(|i| {
            let app = js::string(i.get("appName"));
            let id = Some(js::number(i.get("id"))).filter(|n| *n > 0.0 && n.is_finite()).map(|n| n as u64).unwrap_or_else(|| {
                seq += 1;
                seq
            });
            let state = match State::from_name(&js::string(i.get("status"))) {
                Some(State::Installing) => State::Paused,
                Some(s) => s,
                None => State::Failed,
            };
            let title = js::text(js::first(&[i.get("title"), Some(&json!(app))]));
            Item {
                id,
                title: js::slice(&title, 120).into(),
                base_dir: js::text(i.get("baseDir")),
                update: js::truthy(i.get("update")),
                state,
                pct: js::number(i.get("pct")).clamp(0.0, 100.0).max(0.0),
                speed: String::new(),
                eta: String::new(),
                error: js::slice(&js::text(i.get("error")), 200).into(),
                added: js::number(i.get("added")).max(0.0) as u64,
                app_name: app,
            }
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn until(f: impl Fn() -> bool, secs: u64) -> bool {
        let end = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(40));
        }
        false
    }

    /// The same as linux/ci/epicdl-test.js runs main/epicdl.js through: a pretend legendary that installs slowly
    /// and prints legendary's progress lines.
    #[test]
    fn the_queue() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path();
        let fake = t.join("legendary");
        std::fs::write(
            &fake,
            format!(
                r#"#!/bin/sh
cmd=$1; app=$2
if [ "$cmd" = install ]; then
  base=""; prev=""; for a in "$@"; do [ "$prev" = "--base-path" ] && base="$a"; prev="$a"; done
  [ -n "$base" ] && mkdir -p "$base/$app-folder"
  echo "$app $*" >> "{t}/runs.txt"
  if [ "$app" = FailApp ]; then echo "ERROR: boom, no space left" >&2; exit 1; fi
  i=1; while [ $i -le 10 ]; do echo "[DLManager] INFO: = Progress: $((i*10)).00% (1/2), Running for 00:00:0$i, ETA: 00:00:0$((10-i))"; echo " + Download	- 5.00 MiB/s (raw) / 4.00 MiB/s (decompressed)"; sleep 0.2; i=$((i+1)); done
  exit 0
fi
exit 0
"#,
                t = t.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let data = t.join("data");
        let make = || {
            let fake = fake.clone();
            Downloads::new(&data, Hooks { bin: Box::new(move || Some(fake.clone())), on_change: Box::new(|_| {}), on_settled: Box::new(|_| {}) })
        };
        let base = t.join("games").to_string_lossy().into_owned();
        let m = make();
        let st = |m: &Downloads, app: &str| m.list().into_iter().find(|x| x.app_name == app).map(|x| x.state);
        let a = m.add("AppA", "Game A", &base, false).unwrap();
        let b = m.add("AppB", "Game B", &base, false).unwrap();
        m.add("AppC", "Game C", &base, false).unwrap();
        assert_eq!((st(&m, "AppA"), st(&m, "AppB"), st(&m, "AppC")), (Some(State::Installing), Some(State::Queued), Some(State::Queued)));
        assert_eq!((m.add("AppB", "Game B", &base, false).unwrap().id, m.list().len()), (b.id, 3));
        assert!(until(|| std::fs::read_to_string(t.join("runs.txt")).is_ok_and(|r| r.contains(&format!("AppA install AppA --base-path {base} --platform Windows --skip-dlcs --skip-sdl -y"))), 10));
        assert!(until(|| m.list()[0].pct >= 20.0 && m.list()[0].pct < 100.0 && !m.list()[0].eta.is_empty(), 10));
        assert!(m.list()[0].speed.contains("MiB/s") || until(|| m.list()[0].speed.contains("MiB/s"), 3));
        assert_eq!(m.current().map(|c| c.app_name), Some("AppA".into()));
        assert!(m.pause(a.id));
        assert_eq!(st(&m, "AppA"), Some(State::Paused));
        assert!(m.list()[0].pct > 0.0);
        assert!(until(|| st(&m, "AppB") == Some(State::Installing), 10));
        let unfinished = std::fs::read_to_string(data.join("epic-unfinished.json")).unwrap();
        assert!(unfinished.contains("AppA-folder"), "{unfinished}");
        assert!(m.resume(a.id));
        assert_eq!((st(&m, "AppA"), st(&m, "AppB")), (Some(State::Queued), Some(State::Installing)));
        assert!(until(|| ["AppA", "AppB", "AppC"].iter().all(|x| st(&m, x) == Some(State::Done)), 40));
        let order: Vec<String> = std::fs::read_to_string(t.join("runs.txt")).unwrap().lines().map(|l| l.split(' ').next().unwrap().to_string()).collect();
        assert_eq!(order, ["AppA", "AppB", "AppA", "AppC"]);
        assert!(!std::fs::read_to_string(data.join("epic-unfinished.json")).unwrap().contains("AppA"));

        m.add("FailApp", "Fail Game", &base, false).unwrap();
        assert!(until(|| st(&m, "FailApp") == Some(State::Failed), 10));
        assert!(m.list().iter().find(|x| x.app_name == "FailApp").unwrap().error.contains("boom, no space left"));
        let fail = m.list().iter().find(|x| x.app_name == "FailApp").unwrap().id;
        assert!(m.resume(fail));
        assert!(until(|| st(&m, "FailApp") == Some(State::Failed), 10));

        m.clear_done();
        m.add("AppD", "D", &base, false).unwrap();
        let e = m.add("AppE", "E", &base, false).unwrap();
        let f = m.add("AppF", "F", &base, false).unwrap();
        assert!(m.move_by(f.id, -1));
        assert_eq!(m.list().iter().map(|x| x.app_name.as_str()).collect::<Vec<_>>(), ["AppD", "AppF", "AppE"]);
        assert!(!m.move_by(m.list()[0].id, 1), "a running install can't be moved");
        assert!(m.stop(e.id) && st(&m, "AppE") == Some(State::Cancelled));
        assert!(m.resume(e.id) && st(&m, "AppE") == Some(State::Queued));
        let running = m.current().unwrap().id;
        assert!(m.remove(running));
        assert!(!m.list().iter().any(|x| x.id == running));
        assert!(until(|| ["AppF", "AppE"].iter().any(|x| st(&m, x) == Some(State::Installing)), 10));
        std::thread::sleep(Duration::from_millis(400));
        let now = m.current();
        let again = make();
        assert_eq!(again.list().len(), m.list().len());
        if let Some(now) = now {
            assert_eq!(again.list().iter().find(|x| x.app_name == now.app_name).map(|x| x.state), Some(State::Paused));
        }
        m.clear_done();
        assert!(m.list().iter().all(|x| x.state.live()));
        assert_eq!(m.add("bad name; rm -rf", "x", &base, false).unwrap_err(), "Unknown Epic game.");
        for x in m.list() {
            m.remove(x.id);
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// play history in Boxshelf's main.js, as RustyShelf has it.
//! The play history, `data/history.json`: per game its launches, when it was last played and the time in it. The
//! Electron app reads and writes the same file.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::js;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Played {
    pub count: u64,
    pub ms: u64,
    /// ms since 1970
    pub last: u64,
}

#[derive(Debug)]
struct Open {
    id: String,
    at: u64,
    /// the launches and last play before this one; None for a game that's only timed
    prev: Option<(u64, u64)>,
}

#[derive(Debug, Default)]
pub struct History {
    file: PathBuf,
    games: Vec<(String, Played)>,
    open: Option<Open>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// JS `+v || 0`, as a whole number.
fn num(v: Option<&Value>) -> u64 {
    let n = match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => js::trim(s).parse().unwrap_or(0.0),
        Some(Value::Bool(true)) => 1.0,
        _ => 0.0,
    };
    if n.is_finite() && n > 0.0 {
        n as u64
    } else {
        0
    }
}

impl History {
    /// A file that can't be read as JSON is kept as a copy (`history.json.corrupt-<time>`) and the history starts empty.
    pub fn load(data_dir: &Path) -> History {
        let file = data_dir.join("history.json");
        let mut games = Vec::new();
        if let Ok(raw) = std::fs::read(&file) {
            match serde_json::from_str::<Value>(&String::from_utf8_lossy(&raw)) {
                Ok(Value::Object(o)) => {
                    for (k, v) in js::entries(&o) {
                        if v.is_object() || v.is_array() {
                            games.push((k.clone(), Played { count: num(v.get("count")), ms: num(v.get("ms")), last: num(v.get("last")) }));
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = std::fs::copy(&file, format!("{}.corrupt-{}", file.display(), now()));
                }
            }
        }
        History { file, games, open: None }
    }

    pub fn get(&self, id: &str) -> Option<Played> {
        self.games.iter().find(|(k, _)| k == id).map(|(_, p)| *p)
    }

    fn entry(&mut self, id: &str) -> &mut Played {
        let i = match self.games.iter().position(|(k, _)| k == id) {
            Some(i) => i,
            None => {
                self.games.push((id.to_string(), Played::default()));
                self.games.len() - 1
            }
        };
        &mut self.games[i].1
    }

    /// Through a temporary file: a crash or a power cut mid-write can't wipe the history.
    fn save(&self) {
        let o: Map<String, Value> = self.games.iter().map(|(k, p)| (k.clone(), json!({ "count": p.count, "ms": p.ms, "last": p.last }))).collect();
        let tmp = PathBuf::from(format!("{}.tmp", self.file.display()));
        if let Some(d) = self.file.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if std::fs::write(&tmp, Value::Object(o).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
    }

    fn count(&mut self, id: &str, at: u64) -> (String, (u64, u64)) {
        let id = js::slice(id, 1024).to_string();
        let e = self.entry(&id);
        let prev = (e.count, e.last);
        e.count += 1;
        e.last = at;
        self.save();
        (id, prev)
    }

    /// A game the shelf starts and waits for: the launch is counted now, its time when it ends.
    pub fn start(&mut self, id: &str) {
        self.start_at(id, now());
    }

    fn start_at(&mut self, id: &str, at: u64) {
        let (id, prev) = self.count(id, at);
        self.open = Some(Open { id, at, prev: Some(prev) });
    }

    /// A game its store looks after (Steam, a store's launcher) or that isn't waited for: the launch, no time.
    pub fn touch(&mut self, id: &str) {
        self.count(id, now());
    }

    /// The launch never started, or the emulator closed right away: the play is taken back off, "last played" too.
    pub fn undo(&mut self) {
        let Some(Open { id, prev: Some((count, last)), .. }) = self.open.take() else { return };
        let Some(i) = self.games.iter().position(|(k, _)| *k == id) else { return };
        if count == 0 && self.games[i].1.ms == 0 {
            self.games.remove(i);
        } else {
            self.games[i].1.count = count;
            self.games[i].1.last = last;
        }
        self.save();
    }

    /// The game closed: its time is added.
    pub fn end(&mut self) {
        self.end_at(now());
    }

    fn end_at(&mut self, at: u64) {
        let Some(open) = self.open.take() else { return };
        if let Some(i) = self.games.iter().position(|(k, _)| *k == open.id) {
            self.games[i].1.ms += at.saturating_sub(open.at);
            self.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("history.json"), r#"{"gc:/g/a.iso":{"count":"2","ms":1500.7,"last":9},"junk":3,"7":{}}"#).unwrap();
        let mut h = History::load(t.path());
        assert_eq!(h.get("gc:/g/a.iso"), Some(Played { count: 2, ms: 1500, last: 9 }));
        assert_eq!((h.get("junk"), h.get("7")), (None, Some(Played::default())));
        h.start_at("gc:/g/a.iso", 100);
        h.end_at(350);
        assert_eq!(h.get("gc:/g/a.iso"), Some(Played { count: 3, ms: 1750, last: 100 }));
        h.start_at("new", 400);
        h.undo();
        assert_eq!(h.get("new"), None);
        h.start_at("gc:/g/a.iso", 500);
        h.undo();
        assert_eq!(h.get("gc:/g/a.iso"), Some(Played { count: 3, ms: 1750, last: 100 }));
        h.touch("steam-10");
        h.end_at(900);
        let saved = std::fs::read_to_string(t.path().join("history.json")).unwrap();
        assert!(saved.starts_with(r#"{"7":{"count":0,"ms":0,"last":0},"gc:/g/a.iso":{"count":3,"ms":1750,"last":100},"steam-10":{"count":1,"ms":0,"last":"#), "{saved}");
        assert_eq!(History::load(t.path()).get("steam-10").unwrap().count, 1);

        std::fs::write(t.path().join("history.json"), "{broken").unwrap();
        assert_eq!(History::load(t.path()).games.len(), 0);
        assert!(std::fs::read_dir(t.path()).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("history.json.corrupt-")));
    }
}

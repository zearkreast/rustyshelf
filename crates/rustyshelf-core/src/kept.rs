//! A memory-and-disk cache of JSON answers: `<dir>/<name>.json` holding `{ "at": <ms>, "data": … }`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};

pub(crate) struct Kept {
    dir: PathBuf,
    mem: Mutex<HashMap<String, (u64, Value)>>,
}

impl Kept {
    /// Kept in `dir`, made if it isn't there.
    pub fn new(dir: PathBuf) -> Kept {
        let _ = fs::create_dir_all(&dir);
        Kept { dir, mem: Mutex::default() }
    }

    /// A name made safe as a file name.
    pub fn name(name: &str) -> String {
        name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    /// The copy kept under `name` (made safe) younger than `max_age_ms`, from memory, else from disk.
    pub fn fresh(&self, name: &str, max_age_ms: u64, now: u64) -> Option<Value> {
        let fresh = |at: u64| now.saturating_sub(at) < max_age_ms;
        if let Some((_, d)) = self.mem.lock().unwrap().get(name).filter(|m| fresh(m.0)) {
            return Some(d.clone());
        }
        let j = crate::files::read_json(&self.file(name))?;
        let (at, d) = (j.get("at").and_then(Value::as_u64)?, j.get("data")?);
        if !fresh(at) {
            return None;
        }
        self.mem.lock().unwrap().insert(name.to_string(), (at, d.clone()));
        Some(d.clone())
    }

    pub fn keep(&self, name: &str, data: &Value, now: u64) {
        self.mem.lock().unwrap().insert(name.to_string(), (now, data.clone()));
        let _ = fs::write(self.file(name), json!({ "at": now, "data": data }).to_string());
    }

    /// The last copy on disk, however old.
    pub fn last(&self, name: &str) -> Option<Value> {
        crate::files::read_json(&self.file(name)).and_then(|j| j.get("data").cloned())
    }
}

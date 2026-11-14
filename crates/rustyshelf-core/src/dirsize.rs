//! How much room a game's folder takes (the PC games folder's games, Epic's installs, unpacked Switch 2 games), for
//! the size under a game's name. Ported from RustyShelf's main/dirsize.js. A folder is measured off the shelf's
//! thread: with `du` where there is one, else by walking it. The sizes are kept in `data/dir-sizes.json`
//! (`{ "<folder>": { bytes, mtime, at } }`, a file the Electron app shares) and measured again when the folder
//! changes (its own modification time) or after a week.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{json, Map, Value};

use crate::js;

const WEEK: f64 = 7.0 * 24.0 * 3_600_000.0;

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// A folder's own modification time in ms; None when it isn't there.
fn mtime_of(dir: &str) -> Option<u64> {
    Path::new(dir).metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_millis() as u64)
}

/// `du` prints bytes with -b (GNU, Linux); BSD / macOS du counts kilobytes with -k.
#[cfg(unix)]
fn via_du(dir: &str) -> Option<u64> {
    let gnu = cfg!(target_os = "linux");
    let out = std::process::Command::new("du").args([if gnu { "-sb" } else { "-sk" }, dir]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    // a file it couldn't read still gives the total of the rest
    let n: u64 = String::from_utf8_lossy(&out.stdout).split_whitespace().next()?.parse().ok()?;
    Some(if gnu { n } else { n * 1024 })
}

#[cfg(not(unix))]
fn via_du(_: &str) -> Option<u64> {
    None
}

fn via_walk(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => via_walk(&e.path()),
            Ok(t) if t.is_file() => e.path().metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// The folder's size in bytes, however long it takes.
pub fn measure(dir: &str) -> u64 {
    via_du(dir).unwrap_or_else(|| via_walk(Path::new(dir)))
}

pub struct DirSizes {
    file: PathBuf,
    cache: Map<String, Value>,
}

impl DirSizes {
    pub fn load(data_dir: &Path) -> DirSizes {
        let file = data_dir.join("dir-sizes.json");
        let cache = match std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(Value::Object(m)) => m,
            _ => Map::new(),
        };
        DirSizes { file, cache }
    }

    /// Whether a folder has to be measured: never was, changed since, or was measured over a week ago.
    pub fn stale(&self, dir: &str) -> bool {
        if dir.is_empty() || !js::is_absolute(dir) {
            return false;
        }
        let Some(m) = mtime_of(dir) else { return false };
        match self.cache.get(dir) {
            Some(c) => js::number(c.get("mtime")) != m as f64 || now() as f64 - js::number(c.get("at")) > WEEK,
            None => true,
        }
    }

    /// Measures the folders that need it, one at a time, and keeps the sizes; true when a size changed.
    pub fn measure_stale(&mut self, dirs: &[String]) -> bool {
        let mut changed = false;
        for dir in dirs {
            if !self.stale(dir) {
                continue;
            }
            let Some(m) = mtime_of(dir) else { continue };
            let bytes = measure(dir);
            changed |= self.cache.get(dir).and_then(|c| c.get("bytes")).and_then(Value::as_f64) != Some(bytes as f64);
            self.cache.insert(dir.clone(), json!({ "bytes": bytes, "mtime": m, "at": now() }));
        }
        let tmp = self.file.with_extension("json.tmp");
        if std::fs::write(&tmp, Value::Object(self.cache.clone()).to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.file);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders() {
        let t = tempfile::tempdir().unwrap();
        let game = t.path().join("games/Zeta");
        std::fs::create_dir_all(game.join("data")).unwrap();
        std::fs::write(game.join("a.bin"), vec![0u8; 1000]).unwrap();
        std::fs::write(game.join("data/b.bin"), vec![0u8; 500]).unwrap();
        assert_eq!(via_walk(&game), 1500);
        let dir = game.to_string_lossy().into_owned();
        let mut sizes = DirSizes::load(t.path());
        assert!(sizes.stale(&dir) && !sizes.stale("relative") && !sizes.stale(&format!("{dir}/nope")));
        assert!(sizes.measure_stale(&[dir.clone(), "relative".into()]));
        assert!(!sizes.stale(&dir) && !sizes.measure_stale(std::slice::from_ref(&dir)));
        let kept = DirSizes::load(t.path());
        assert!(kept.cache[&dir]["bytes"].as_u64().unwrap() >= 1500 && !kept.stale(&dir));
    }
}

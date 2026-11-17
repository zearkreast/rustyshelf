//! Where Vita3K keeps its installed games (RustyShelf's main/vita3k.js): `ux0/app/<title id>` in the folder its
//! config.yml names (`pref-path`), else beside the program (older and portable builds), else in its user folder.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::art::path_key;
use crate::js;
use crate::paths::is_dir;

/// Vita3K's user folder: `%APPDATA%\Vita3K\Vita3K` on Windows, `~/.local/share/Vita3K/Vita3K` on Linux.
fn user_dir(var: &dyn Fn(&str) -> Option<String>) -> String {
    let get = |k: &str| var(k).filter(|v| !v.is_empty());
    let home = get(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
    let base = if cfg!(windows) {
        get("APPDATA").unwrap_or_else(|| js::join(&[&home, "AppData", "Roaming"]))
    } else {
        get("XDG_DATA_HOME").unwrap_or_else(|| js::join(&[&home, ".local", "share"]))
    };
    js::join(&[&base, "Vita3K", "Vita3K"])
}

/// The folders that can hold ux0 for this Vita3K program (`exe` may be empty: only the usual ones).
pub fn pref_dirs(exe: &str, var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    static PREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^pref-path:\s*(.*?)\s*$").unwrap());
    let (beside, home) = (if exe.is_empty() { String::new() } else { js::dirname(exe) }, user_dir(var));
    let mut out = Vec::new();
    for dir in [&beside, &home].into_iter().filter(|d| !d.is_empty()) {
        let Ok(t) = std::fs::read(js::join(&[dir, "config.yml"])).map(|b| String::from_utf8_lossy(&b).into_owned()) else { continue };
        let Some(m) = PREF.captures(&t) else { continue };
        let p = &m[1];
        let quoted = p.len() >= 2 && (p.starts_with('"') && p.ends_with('"') || p.starts_with('\'') && p.ends_with('\''));
        let p = js::trim(if quoted { &p[1..p.len() - 1] } else { p });
        if !p.is_empty() {
            out.push(if js::is_absolute(p) { p.to_string() } else { js::join(&[dir, p]) });
        }
    }
    if !beside.is_empty() {
        out.push(beside.clone());
        out.push(js::join(&[&beside, "portable"]));
    }
    out.push(home);
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(path_key(Path::new(d))));
    out
}

/// The `ux0/app` folders that exist: the games folders for the Vita shelf.
pub fn app_dirs(exe: &str, var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    pref_dirs(exe, var).iter().map(|d| js::join(&[d, "ux0", "app"])).filter(|d| is_dir(Path::new(d))).collect()
}

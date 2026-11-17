//! Settings → Accounts as a `.env` file in the data folder, one `NAME=value` line per sign-in or key, in plain text.
//! A value there wins over the one in `config.json`, so the file can be edited by hand or carried to another PC
//! (a sealed key in `config.json` only opens for the Windows user that sealed it).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The settings' name for each value (`ra.user`, `ra.key`, else under `keys`) and its name in the file.
pub const NAMES: [(&str, &str); 10] = [
    ("ra.user", "RA_USER"),
    ("ra.key", "RA_API_KEY"),
    ("steamgriddb", "STEAMGRIDDB_API_KEY"),
    ("steamweb", "STEAM_WEB_API_KEY"),
    ("igdbId", "IGDB_CLIENT_ID"),
    ("igdbSecret", "IGDB_CLIENT_SECRET"),
    ("ssUser", "SCREENSCRAPER_USER"),
    ("ssPass", "SCREENSCRAPER_PASSWORD"),
    ("ssDevId", "SCREENSCRAPER_DEV_ID"),
    ("ssDevPass", "SCREENSCRAPER_DEV_PASSWORD"),
];

const HEAD: &str = "# RustyShelf's accounts (Settings → Accounts), in plain text: don't share this file.\n";

pub fn file(data_dir: &Path) -> PathBuf {
    data_dir.join(".env")
}

fn env_name(name: &str) -> Option<&'static str> {
    NAMES.iter().find(|n| n.0 == name).map(|n| n.1)
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    match v.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        Some(inner) => {
            let mut out = String::with_capacity(inner.len());
            let mut it = inner.chars();
            while let Some(c) = it.next() {
                out.push(if c == '\\' { it.next().unwrap_or('\\') } else { c });
            }
            out
        }
        None => v.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')).unwrap_or(v).to_string(),
    }
}

fn quote(v: &str) -> String {
    if v.chars().all(|c| c.is_ascii_alphanumeric() || "-_.@+/:".contains(c)) {
        v.to_string()
    } else {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn line_name(line: &str) -> Option<&str> {
    let l = line.trim_start();
    let l = l.strip_prefix("export ").unwrap_or(l);
    (!l.starts_with('#')).then(|| l.split_once('=').map(|(k, _)| k.trim())).flatten()
}

/// The values the file has, by the settings' name; empty ones are left out.
pub fn read(data_dir: &Path) -> HashMap<String, String> {
    let text = std::fs::read_to_string(file(data_dir)).unwrap_or_default();
    let mut out = HashMap::new();
    for line in text.lines() {
        let Some(k) = line_name(line) else { continue };
        let Some((name, _)) = NAMES.iter().find(|n| n.1 == k) else { continue };
        let v = unquote(line.split_once('=').map_or("", |(_, v)| v));
        if !v.is_empty() {
            out.insert(name.to_string(), v);
        }
    }
    out
}

/// Puts one value in the file (an empty one takes its line out), keeping the other lines as they are.
pub fn write(data_dir: &Path, name: &str, value: &str) -> std::io::Result<()> {
    let Some(key) = env_name(name) else { return Ok(()) };
    let f = file(data_dir);
    let text = std::fs::read_to_string(&f).unwrap_or_default();
    let value = value.trim();
    let mut lines: Vec<String> = Vec::new();
    let mut put = false;
    for line in text.lines() {
        if line_name(line) == Some(key) {
            if !put && !value.is_empty() {
                lines.push(format!("{key}={}", quote(value)));
            }
            put = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !put && !value.is_empty() {
        lines.push(format!("{key}={}", quote(value)));
    }
    if lines.iter().all(|l| l.trim().is_empty() || HEAD.trim_end() == l) {
        return match std::fs::remove_file(&f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let mut out = if text.is_empty() { HEAD.to_string() } else { String::new() };
    for l in lines {
        out.push_str(&l);
        out.push('\n');
    }
    crate::files::write_atomic(&f, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn written_read_and_removed() {
        let dir = std::env::temp_dir().join(format!("rustyshelf-accountenv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(file(&dir), "# mine\nOTHER=1\nexport IGDB_CLIENT_ID='abc'\n").unwrap();
        assert_eq!(read(&dir).get("igdbId").map(String::as_str), Some("abc"));
        write(&dir, "igdbId", "xyz").unwrap();
        write(&dir, "ssPass", "p a\"ss#").unwrap();
        let r = read(&dir);
        assert_eq!((r["igdbId"].as_str(), r["ssPass"].as_str()), ("xyz", "p a\"ss#"));
        let text = std::fs::read_to_string(file(&dir)).unwrap();
        assert!(text.starts_with("# mine\nOTHER=1\nIGDB_CLIENT_ID=xyz\n"));
        write(&dir, "igdbId", "").unwrap();
        assert!(!read(&dir).contains_key("igdbId"));
        write(&dir, "nothing", "x").unwrap();
        assert!(!std::fs::read_to_string(file(&dir)).unwrap().contains("nothing"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

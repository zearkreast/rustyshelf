//! Reading and writing the app's own JSON files.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

/// A JSON file's value (a byte-order mark allowed, bytes that aren't UTF-8 replaced); None when it's missing or isn't
/// JSON.
pub fn read_json(f: &Path) -> Option<Value> {
    read_json_as(f)
}

/// [`read_json`] into a type; None also when the JSON doesn't fit it.
pub fn read_json_as<T: DeserializeOwned>(f: &Path) -> Option<T> {
    let bytes = std::fs::read(f).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()
}

/// [`read_json`]'s object, or an empty one when the file is missing or holds anything else.
pub fn read_object(f: &Path) -> Map<String, Value> {
    match read_json(f) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// A text file without its byte-order mark, bytes that aren't UTF-8 replaced; None when it can't be read.
pub fn read_text(f: &Path) -> Option<String> {
    let text = String::from_utf8_lossy(&std::fs::read(f).ok()?).into_owned();
    Some(text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text))
}

/// A UTF-8 text file without its byte-order mark; None when it can't be read or isn't UTF-8.
pub fn read_utf8(f: &Path) -> Option<String> {
    std::fs::read_to_string(f).ok().map(|t| t.trim_start_matches('\u{feff}').to_string())
}

/// A file's modification time in ms since 1970; None when it isn't there.
pub fn mtime_ms(f: &Path) -> Option<u64> {
    crate::ms_of(std::fs::metadata(f).ok()?.modified().ok()?)
}

/// Writes a file whole or not at all: a temporary file beside it, then moved over it. The temporary file's name has
/// the process id, as another RustyShelf may be writing the same file.
pub fn write_atomic(f: &Path, data: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(d) = f.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d)?;
    }
    let tmp = tmp_of(f);
    std::fs::write(&tmp, data).and_then(|()| std::fs::rename(&tmp, f)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn tmp_of(f: &Path) -> PathBuf {
    let mut name = f.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.tmp", std::process::id()));
    f.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_written_whole_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("deep/er/index.json");
        write_atomic(&f, r#"{"v":1}"#).unwrap();
        assert_eq!(read_json(&f), Some(serde_json::json!({ "v": 1 })));
        write_atomic(&f, "\u{feff}[2]").unwrap();
        assert_eq!(read_json(&f), Some(serde_json::json!([2])));
        assert!(!tmp_of(&f).exists());
        assert_eq!(std::fs::read_dir(f.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn what_isnt_json_reads_as_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.json");
        assert_eq!(read_json(&f), None);
        std::fs::write(&f, "{cut off").unwrap();
        assert_eq!(read_json(&f), None);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary_one() {
        let dir = tempfile::tempdir().unwrap();
        // a folder where the file should be: the move over it fails
        let f = dir.path().join("taken");
        std::fs::create_dir(&f).unwrap();
        std::fs::write(f.join("inside"), "kept").unwrap();
        assert!(write_atomic(&f, "new").is_err());
        assert!(!tmp_of(&f).exists());
        assert_eq!(std::fs::read_to_string(f.join("inside")).unwrap(), "kept");
    }
}

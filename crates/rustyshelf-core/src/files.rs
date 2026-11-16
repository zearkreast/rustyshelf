//! Reading and writing the app's own JSON files.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A JSON file's value (a byte-order mark allowed, bytes that aren't UTF-8 replaced); None when it's missing or isn't
/// JSON.
pub fn read_json(f: &Path) -> Option<Value> {
    let text = String::from_utf8_lossy(&std::fs::read(f).ok()?).into_owned();
    serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(&text)).ok()
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

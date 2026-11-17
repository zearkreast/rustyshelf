//! Achievement badges and game icons from the web, kept in `data/ach-icons` so a list shows them again without asking.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha1::{Digest, Sha1};

fn file_for(data_dir: &Path, url: &str) -> PathBuf {
    let hex: String = Sha1::digest(url.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    data_dir.join("ach-icons").join(&hex[..2]).join(hex)
}

/// The picture's bytes: kept ones, else downloaded (at most 4 MB) and kept.
pub fn get(data_dir: &Path, url: &str) -> Option<Vec<u8>> {
    let f = file_for(data_dir, url);
    if let Ok(b) = std::fs::read(&f) {
        return Some(b);
    }
    let b = crate::net::get_bytes(url, Duration::from_secs(15), 4 << 20).ok().flatten()?;
    let _ = crate::files::write_atomic(&f, &b);
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_picture_is_read_without_asking() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.invalid/badge.png";
        let f = file_for(dir.path(), url);
        crate::files::write_atomic(&f, b"png").unwrap();
        assert_eq!(get(dir.path(), url).as_deref(), Some(&b"png"[..]));
        assert_ne!(file_for(dir.path(), "https://example.invalid/other.png"), f);
    }
}

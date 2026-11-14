// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/scanner.js (pkgInfo, pkgTitleFromName), as RustyShelf has it.
//! A PS4 .pkg that hasn't been installed yet: its param.sfo sits unencrypted in the package's entry table (id 0x1000),
//! which gives the real title, the CUSA id and whether it's a game (gd), a patch (gp) or an add-on (ac).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::sfo::{self, Sfo};

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_exact_at(f: &mut File, pos: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(pos)).ok()?;
    let mut b = vec![0u8; len];
    f.read_exact(&mut b).ok()?;
    Some(b)
}

/// The package's param.sfo. The header is big-endian.
pub fn info(file: &Path) -> Option<Sfo> {
    let mut f = File::open(file).ok()?;
    let h = read_exact_at(&mut f, 0, 32)?;
    if be32(&h, 0)? != 0x7F43_4E54 {
        return None;
    }
    let (count, table_off) = (be32(&h, 0x10)? as usize, be32(&h, 0x18)?);
    if count > 10_000 {
        return None;
    }
    let t = read_exact_at(&mut f, table_off as u64, count * 32)?;
    for i in 0..count {
        if be32(&t, i * 32)? != 0x1000 {
            continue;
        }
        let (off, size) = (be32(&t, i * 32 + 16)?, be32(&t, i * 32 + 20)? as usize);
        if size > 1 << 20 {
            return None;
        }
        return sfo::parse(&read_exact_at(&mut f, off as u64, size)?);
    }
    None
}

static BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\]]*\]").unwrap());
static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[_ ]?(?:CUSA|PLAS|PCAS|PLJM|PCJS)[0-9]{5}").unwrap());
static SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[._]+").unwrap());
static ENDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\s-]+|[\s-]+$").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
/// a package's id in its file name
pub static NAME_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(CUSA|PLAS|PCAS|PLJM|PCJS)[0-9]{5}").unwrap());

/// A readable title from a download-style name: "[Site]-The.Last.of.Us.Remastered_CUSA00556_v1.00_UPLOADER.pkg".
pub fn title_from_name(file: &Path) -> String {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let b = if name.to_lowercase().ends_with(".pkg") { &name[..name.len() - 4] } else { &name };
    let b = BRACKETS.replace_all(b, " ");
    let b = ID.split(&b).next().unwrap_or("").to_string();
    let b = SEPARATORS.replace_all(&b, " ");
    let b = ENDS.replace_all(&b, "");
    SPACES.replace_all(&b, " ").trim().to_string()
}

/// A package around a param.sfo, for tests.
#[cfg(test)]
pub fn build(sfo: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 0x60];
    b[..4].copy_from_slice(&0x7F43_4E54u32.to_be_bytes());
    b[0x10..0x14].copy_from_slice(&1u32.to_be_bytes());
    b[0x18..0x1C].copy_from_slice(&0x20u32.to_be_bytes());
    b[0x20..0x24].copy_from_slice(&0x1000u32.to_be_bytes());
    b[0x30..0x34].copy_from_slice(&0x60u32.to_be_bytes());
    b[0x34..0x38].copy_from_slice(&(sfo.len() as u32).to_be_bytes());
    b.extend_from_slice(sfo);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(title_from_name(Path::new("[Site]-The.Last.of.Us.Remastered_CUSA00556_v1.00_UPLOADER.pkg")), "The Last of Us Remastered");
        assert_eq!(title_from_name(Path::new("Bloodborne.PKG")), "Bloodborne");
    }

    #[test]
    fn header() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("g.pkg");
        std::fs::write(&f, build(&sfo::build(&[("TITLE", "Bloodborne"), ("TITLE_ID", "CUSA00900"), ("CATEGORY", "gd")]))).unwrap();
        let s = info(&f).unwrap();
        assert_eq!(sfo::text(&s, "TITLE_ID"), Some("CUSA00900"));
        std::fs::write(&f, b"nope").unwrap();
        assert!(info(&f).is_none());
    }
}

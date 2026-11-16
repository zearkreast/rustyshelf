//! ZIP files: what's inside one, and an entry's bytes (stored or deflated), either from the whole file in memory or
//! read from disk a piece at a time. No ZIP64: a ZIP that needs it (over 4 GB, or 65,535 entries) is refused.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};

/// One file inside a ZIP.
pub struct Entry {
    pub name: String,
    /// 0 stored, 8 deflated; anything else can't be read here
    pub method: u16,
    pub size: u64,
    pub csize: u64,
    local: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotZip,
    TooBig,
    Damaged,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // the interface's dictionaries translate these exact texts
        f.write_str(match self {
            Error::NotZip => "That file isn't a ZIP.",
            Error::TooBig => "That ZIP is too big.",
            Error::Damaged => "That ZIP looks damaged.",
        })
    }
}

const END: u32 = 0x0605_4b50;
const DIR: u32 = 0x0201_4b50;
const LOCAL: u32 = 0x0403_4b50;
/// The end record and the longest comment after it.
const TAIL: u64 = 22 + 65_535;
const MAX_DIR: u64 = 8 << 20;

fn le16(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 2).map_or(0, |s| u16::from_le_bytes([s[0], s[1]]) as u64)
}

fn le32(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64)
}

/// The central directory's entry count, size and offset, from the file's last bytes (`tail`).
fn directory(tail: &[u8], len: u64) -> Result<(u64, u64, u64), Error> {
    let last = tail.len().checked_sub(22).ok_or(Error::NotZip)?;
    let end = (last.saturating_sub(65_535)..=last).rev().find(|&i| le32(tail, i) == END as u64).ok_or(Error::NotZip)?;
    let (count, size, offset) = (le16(tail, end + 10), le32(tail, end + 12), le32(tail, end + 16));
    if count == 0xffff || offset == 0xffff_ffff || size == 0xffff_ffff {
        return Err(Error::TooBig);
    }
    if size > MAX_DIR || offset + size > len {
        return Err(Error::Damaged);
    }
    Ok((count, size, offset))
}

fn parse(dir: &[u8], count: u64) -> Result<Vec<Entry>, Error> {
    let mut out = Vec::new();
    let mut p = 0;
    for _ in 0..count {
        if p + 46 > dir.len() || le32(dir, p) != DIR as u64 {
            return Err(Error::Damaged);
        }
        let (flags, method, csize, size, local) = (le16(dir, p + 8), le16(dir, p + 10), le32(dir, p + 20), le32(dir, p + 24), le32(dir, p + 42));
        let (nl, xl, cl) = (le16(dir, p + 28) as usize, le16(dir, p + 30) as usize, le16(dir, p + 32) as usize);
        if csize == 0xffff_ffff || size == 0xffff_ffff || local == 0xffff_ffff {
            return Err(Error::TooBig);
        }
        let raw = dir.get(p + 46..p + 46 + nl).unwrap_or_default();
        // bit 11: the name is UTF-8; without it, older tools wrote it in the PC's code page (ASCII almost always)
        let name = if flags & 0x800 != 0 { String::from_utf8_lossy(raw).into_owned() } else { raw.iter().map(|c| *c as char).collect() };
        out.push(Entry { name, method: method as u16, size, csize, local });
        p += 46 + nl + xl + cl;
    }
    Ok(out)
}

/// The entries of a ZIP that's all in memory.
pub fn entries(b: &[u8]) -> Result<Vec<Entry>, Error> {
    let (count, size, offset) = directory(b, b.len() as u64)?;
    parse(&b[offset as usize..(offset + size) as usize], count)
}

/// An entry's bytes from a ZIP that's all in memory; at most `limit` of them once inflated.
pub fn read(b: &[u8], e: &Entry, limit: usize) -> Option<Vec<u8>> {
    let local = usize::try_from(e.local).ok()?;
    if le32(b, local) != LOCAL as u64 {
        return None;
    }
    let start = local + 30 + le16(b, local + 26) as usize + le16(b, local + 28) as usize;
    let raw = b.get(start..start.checked_add(usize::try_from(e.csize).ok()?)?)?;
    match e.method {
        0 => Some(raw.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, limit.max(1)).ok(),
        _ => None,
    }
}

fn damaged(_: std::io::Error) -> Error {
    Error::Damaged
}

/// The entries of a ZIP on disk, reading only its end.
pub fn entries_of(f: &mut File) -> Result<Vec<Entry>, Error> {
    let len = f.metadata().map_err(damaged)?.len();
    let from = len.saturating_sub(TAIL);
    let mut tail = Vec::new();
    f.seek(SeekFrom::Start(from)).map_err(damaged)?;
    Read::take(&mut *f, TAIL).read_to_end(&mut tail).map_err(damaged)?;
    let (count, size, offset) = directory(&tail, len)?;
    let mut dir = vec![0; size as usize];
    f.seek(SeekFrom::Start(offset)).map_err(damaged)?;
    f.read_exact(&mut dir).map_err(damaged)?;
    parse(&dir, count)
}

/// Writes an entry of a ZIP on disk to `out`, inflated as it's read; how many bytes that was. Fails on a method
/// other than stored or deflated, or when what comes out isn't the entry's size.
pub fn unpack(f: &mut File, e: &Entry, out: &mut impl Write) -> std::io::Result<u64> {
    let bad = |why: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string());
    let mut head = [0u8; 30];
    f.seek(SeekFrom::Start(e.local))?;
    f.read_exact(&mut head)?;
    if le32(&head, 0) != LOCAL as u64 {
        return Err(bad("not a ZIP entry"));
    }
    f.seek(SeekFrom::Current((le16(&head, 26) + le16(&head, 28)) as i64))?;
    let mut raw = std::io::BufReader::with_capacity(1 << 20, Read::take(&mut *f, e.csize));
    let n = match e.method {
        0 => std::io::copy(&mut raw, out)?,
        8 => std::io::copy(&mut flate2::bufread::DeflateDecoder::new(raw), out)?,
        _ => return Err(bad("compressed in a way RustyShelf can't read")),
    };
    if n != e.size {
        return Err(bad("cut off"));
    }
    Ok(n)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ZIP of these files, each stored or deflated.
    pub(crate) fn make(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut out, mut dir) = (Vec::new(), Vec::new());
        for (name, data, deflate) in files {
            let body = if *deflate { miniz_oxide::deflate::compress_to_vec(data, 6) } else { data.to_vec() };
            let crc = crc32fast::hash(data);
            let local = out.len() as u32;
            let method: u16 = if *deflate { 8 } else { 0 };
            let common = |v: &mut Vec<u8>| {
                v.extend_from_slice(&[20, 0, 0, 0]);
                v.extend_from_slice(&method.to_le_bytes());
                v.extend_from_slice(&[0, 0, 0, 0]);
                v.extend_from_slice(&crc.to_le_bytes());
                v.extend_from_slice(&(body.len() as u32).to_le_bytes());
                v.extend_from_slice(&(data.len() as u32).to_le_bytes());
                v.extend_from_slice(&(name.len() as u16).to_le_bytes());
                v.extend_from_slice(&[0, 0]);
            };
            out.extend_from_slice(&LOCAL.to_le_bytes());
            common(&mut out);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&body);
            dir.extend_from_slice(&DIR.to_le_bytes());
            dir.extend_from_slice(&[20, 0]);
            common(&mut dir);
            dir.extend_from_slice(&[0; 10]);
            dir.extend_from_slice(&local.to_le_bytes());
            dir.extend_from_slice(name.as_bytes());
        }
        let at = out.len() as u32;
        out.extend_from_slice(&dir);
        out.extend_from_slice(&END.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&(dir.len() as u32).to_le_bytes());
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn stored_and_deflated_entries_read_from_memory_and_from_disk() {
        let big = "a line of a database\n".repeat(5000);
        let zip = make(&[("one.txt", b"plain", false), ("dir/two.xml", big.as_bytes(), true)]);
        let list = entries(&zip).unwrap();
        assert_eq!(list.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["one.txt", "dir/two.xml"]);
        assert_eq!(read(&zip, &list[0], 100).unwrap(), b"plain");
        assert_eq!(read(&zip, &list[1], 1 << 20).unwrap(), big.as_bytes());
        assert!(read(&zip, &list[1], 10).is_none(), "more than the limit");

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.zip");
        std::fs::write(&file, &zip).unwrap();
        let mut f = File::open(&file).unwrap();
        let list = entries_of(&mut f).unwrap();
        let mut out = Vec::new();
        assert_eq!(unpack(&mut f, &list[1], &mut out).unwrap(), big.len() as u64);
        assert_eq!(out, big.as_bytes());
        out.clear();
        unpack(&mut f, &list[0], &mut out).unwrap();
        assert_eq!(out, b"plain");
    }

    #[test]
    fn what_isnt_a_zip_or_is_cut_off_is_refused() {
        assert_eq!(entries(b"not a zip at all, just some text").err(), Some(Error::NotZip));
        let zip = make(&[("a.txt", b"hello", true)]);
        // the end record says there's more directory than the file has
        let mut short = zip.clone();
        let n = short.len();
        short[n - 6..n - 2].copy_from_slice(&(n as u32).to_le_bytes());
        assert_eq!(entries(&short).err(), Some(Error::Damaged));
        // the entry's data cut off
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.zip");
        std::fs::write(&file, &zip).unwrap();
        let mut f = File::open(&file).unwrap();
        let mut e = entries_of(&mut f).unwrap().remove(0);
        e.size += 1;
        assert!(unpack(&mut f, &e, &mut Vec::new()).is_err());
    }
}

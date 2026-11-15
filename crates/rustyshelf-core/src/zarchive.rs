// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/zarchive.js, as RustyShelf has it.
//! Single files out of a ZArchive (.wua, Cemu's Wii U format) without unpacking the whole thing. Big-endian; a
//! 144-byte footer at the end points at the sections: zstd-compressed data in 64 KiB blocks, offset records (a 64-bit
//! base and 16 sizes each), a name table and a flat file tree.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const BLOCK: usize = 64 * 1024;
const PER_RECORD: usize = 16;
const FOOTER: u64 = 144;
const MAGIC: u32 = 0x169f_52d6;
// the archive is untrusted: real manuals and icons are a few MB, the tables of a 30 GB game about 2 MB
const MAX_FILE: u64 = 256 << 20;
const MAX_TABLE: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub file: bool,
    /// a file's place in the uncompressed data, and its size
    pub offset: u64,
    pub size: u64,
    /// a folder's first child in the tree, and how many it has
    pub start: u32,
    pub count: u32,
    name_off: u32,
}

struct Record {
    base: u64,
    sizes: [u16; PER_RECORD],
}

pub struct ZArchive {
    file: File,
    data_offset: u64,
    records: Vec<Record>,
    names: Vec<u8>,
    nodes: Vec<Node>,
    cache: HashMap<usize, Vec<u8>>,
}

fn be32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn be64(b: &[u8], o: usize) -> u64 {
    (u64::from(be32(b, o)) << 32) | u64::from(be32(b, o + 4))
}

/// Up to `len` bytes at `pos` (fewer at the end of the file).
fn read_at(f: &mut File, pos: u64, len: usize) -> std::io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(pos))?;
    let mut out = Vec::with_capacity(len);
    f.take(len as u64).read_to_end(&mut out)?;
    Ok(out)
}

impl ZArchive {
    pub fn open(path: &Path) -> Result<ZArchive, String> {
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        if size < FOOTER {
            return Err("too small".into());
        }
        let f = read_at(&mut file, size - FOOTER, FOOTER as usize).map_err(|e| e.to_string())?;
        if f.len() < FOOTER as usize || be32(&f, FOOTER as usize - 4) != MAGIC {
            return Err("not a ZArchive".into());
        }
        let sec = |i: usize| (be64(&f, i * 16), be64(&f, i * 16 + 8));
        let (data, recs, names, tree) = (sec(0), sec(1), sec(2), sec(3));
        for (off, len) in [data, recs, names, tree] {
            if off.checked_add(len).is_none_or(|end| end > size) {
                return Err("bad section".into());
            }
        }
        for (_, len) in [recs, names, tree] {
            if len > MAX_TABLE {
                return Err(format!("ZArchive table too large ({len} bytes)"));
            }
        }
        let mut table = |(off, len): (u64, u64)| read_at(&mut file, off, len as usize).map_err(|e| e.to_string());
        let (rec_buf, name_buf, tree_buf) = (table(recs)?, table(names)?, table(tree)?);
        let records = rec_buf
            .chunks_exact(40)
            .map(|r| Record { base: be64(r, 0), sizes: std::array::from_fn(|i| u16::from_be_bytes([r[8 + i * 2], r[9 + i * 2]])) })
            .collect();
        let nodes = tree_buf
            .chunks_exact(16)
            .map(|n| {
                let (a, b, c, d) = (be32(n, 0), be32(n, 4), be32(n, 8), be32(n, 12));
                let name_off = a & 0x7FFF_FFFF;
                if a & 0x8000_0000 != 0 {
                    let offset = u64::from(b) + (u64::from(d & 0xFFFF) << 32);
                    let size = u64::from(c) + (u64::from(d >> 16) << 32);
                    Node { name: String::new(), file: true, offset, size, start: 0, count: 0, name_off }
                } else {
                    Node { name: String::new(), file: false, offset: 0, size: 0, start: b, count: c, name_off }
                }
            })
            .collect();
        Ok(ZArchive { file, data_offset: data.0, records, names: name_buf, nodes, cache: HashMap::new() })
    }

    fn name(&self, off: u32) -> String {
        let nb = &self.names;
        let off = off as usize;
        if off == 0x7FFF_FFFF || off >= nb.len() {
            return String::new();
        }
        let mut len = usize::from(nb[off] & 0x7F);
        let mut p = off + 1;
        if nb[off] & 0x80 != 0 {
            len |= usize::from(nb.get(off + 1).copied().unwrap_or(0)) << 7;
            p = off + 2;
        }
        let (p, end) = (p.min(nb.len()), (p + len).min(nb.len()));
        nb[p..end].iter().map(|&c| char::from(c)).collect()
    }

    fn block(&mut self, i: usize) -> Result<&[u8], String> {
        if !self.cache.contains_key(&i) {
            let r = self.records.get(i / PER_RECORD).ok_or("block out of range")?;
            let sub = i % PER_RECORD;
            let off = r.base + r.sizes[..sub].iter().map(|&s| u64::from(s) + 1).sum::<u64>();
            let csize = usize::from(r.sizes[sub]) + 1;
            let raw = read_at(&mut self.file, self.data_offset + off, csize).map_err(|e| format!("ZArchive block {i}: {e}"))?;
            let out = if csize == BLOCK {
                raw
            } else {
                let dec = ruzstd::decoding::StreamingDecoder::new(&raw[..]).map_err(|e| format!("ZArchive block {i}: {e}"))?;
                let mut out = Vec::new();
                dec.take(BLOCK as u64 + 1).read_to_end(&mut out).map_err(|e| format!("ZArchive block {i}: {e}"))?;
                if out.len() > BLOCK {
                    return Err(format!("ZArchive block {i} unpacks past {BLOCK} bytes"));
                }
                out
            };
            if self.cache.len() > 64 {
                self.cache.clear();
            }
            self.cache.insert(i, out);
        }
        Ok(&self.cache[&i])
    }

    pub fn read(&mut self, node: &Node) -> Result<Vec<u8>, String> {
        if node.size > MAX_FILE {
            return Err(format!("ZArchive file too large to read ({} bytes, limit {MAX_FILE})", node.size));
        }
        let size = node.size as usize;
        let mut out = vec![0u8; size];
        let (mut pos, mut w) = (node.offset, 0usize);
        while w < size {
            let (bi, bo) = ((pos / BLOCK as u64) as usize, (pos % BLOCK as u64) as usize);
            let n = (size - w).min(BLOCK - bo);
            let b = self.block(bi)?;
            let have = b.len().saturating_sub(bo).min(n);
            out[w..w + have].copy_from_slice(&b[bo..bo + have]);
            w += n;
            pos += n as u64;
        }
        Ok(out)
    }

    pub fn children(&self, dir: &Node) -> Vec<Node> {
        (0..dir.count as usize)
            .filter_map(|i| self.nodes.get(dir.start as usize + i))
            .map(|n| Node { name: self.name(n.name_off), ..n.clone() })
            .collect()
    }

    pub fn root(&self) -> Result<Vec<Node>, String> {
        self.nodes.first().map(|r| self.children(r)).ok_or_else(|| "ZArchive without a file tree".into())
    }

    /// A path like "0005000010101a00_v0/meta/Manual.bfma", any case.
    pub fn lookup(&self, path: &str) -> Option<Node> {
        let mut cur = self.nodes.first()?.clone();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            if cur.file {
                return None;
            }
            let want = part.to_lowercase();
            cur = self.children(&cur).into_iter().find(|c| c.name.to_lowercase() == want)?;
        }
        Some(cur)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A ZArchive holding `files` (paths with '/'), its data in zstd blocks as Cemu writes them.
    pub(crate) fn build(files: &[(&str, &[u8])]) -> Vec<u8> {
        struct Dir {
            name: String,
            dirs: Vec<Dir>,
            files: Vec<(String, u64, u64)>,
        }
        let mut data = Vec::new();
        let mut root = Dir { name: String::new(), dirs: Vec::new(), files: Vec::new() };
        for (path, bytes) in files {
            let parts: Vec<&str> = path.split('/').collect();
            let mut d = &mut root;
            for p in &parts[..parts.len() - 1] {
                let i = match d.dirs.iter().position(|x| x.name == *p) {
                    Some(i) => i,
                    None => {
                        d.dirs.push(Dir { name: p.to_string(), dirs: Vec::new(), files: Vec::new() });
                        d.dirs.len() - 1
                    }
                };
                d = &mut d.dirs[i];
            }
            d.files.push((parts[parts.len() - 1].to_string(), data.len() as u64, bytes.len() as u64));
            data.extend_from_slice(bytes);
        }
        let (mut blocks, mut records) = (Vec::new(), Vec::new());
        for (i, chunk) in data.chunks(BLOCK).enumerate() {
            if i % PER_RECORD == 0 {
                records.extend_from_slice(&(blocks.len() as u64).to_be_bytes());
                records.extend_from_slice(&[0u8; PER_RECORD * 2]);
            }
            let packed = if i % 2 == 1 && chunk.len() == BLOCK { chunk.to_vec() } else { ruzstd::encoding::compress_to_vec(chunk, ruzstd::encoding::CompressionLevel::Fastest) };
            let at = records.len() - PER_RECORD * 2 + (i % PER_RECORD) * 2;
            records[at..at + 2].copy_from_slice(&((packed.len() - 1) as u16).to_be_bytes());
            blocks.extend_from_slice(&packed);
        }
        let mut names = Vec::new();
        let mut name = |s: &str| {
            let off = names.len() as u32;
            names.push(s.len() as u8);
            names.extend_from_slice(s.as_bytes());
            off
        };
        // breadth first: each folder's children sit together
        let mut tree: Vec<[u32; 4]> = vec![[0x7FFF_FFFF, 0, 0, 0]];
        let mut queue: Vec<(&Dir, usize)> = vec![(&root, 0)];
        while let Some((d, at)) = queue.pop() {
            let start = tree.len() as u32;
            tree[at][1] = start;
            tree[at][2] = (d.dirs.len() + d.files.len()) as u32;
            for (n, off, len) in &d.files {
                tree.push([0x8000_0000 | name(n), *off as u32, *len as u32, 0]);
            }
            for sub in &d.dirs {
                tree.push([name(&sub.name), 0, 0, 0]);
                queue.push((sub, tree.len() - 1));
            }
        }
        let mut out = blocks;
        let mut secs = Vec::new();
        for part in [records, names, tree.iter().flat_map(|n| n.iter().flat_map(|v| v.to_be_bytes())).collect()] {
            secs.push((out.len() as u64, part.len() as u64));
            out.extend_from_slice(&part);
        }
        let mut footer = Vec::new();
        for (off, len) in [(0, secs[0].0)].into_iter().chain(secs) {
            footer.extend_from_slice(&off.to_be_bytes());
            footer.extend_from_slice(&len.to_be_bytes());
        }
        footer.resize(FOOTER as usize - 4, 0);
        footer.extend_from_slice(&MAGIC.to_be_bytes());
        out.extend_from_slice(&footer);
        out
    }

    #[test]
    fn reads_files_across_blocks() {
        let big: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
        let bytes = build(&[("0005000010101A00/meta/Manual.bfma", b"SARC manual"), ("0005000010101A00/code/game.rpx", &big), ("0005000010101A00/meta/iconTex.tga", b"icon")]);
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("game.wua");
        std::fs::write(&f, &bytes).unwrap();
        let mut z = ZArchive::open(&f).unwrap();
        let root = z.root().unwrap();
        assert_eq!(root.iter().map(|n| (n.name.as_str(), n.file)).collect::<Vec<_>>(), [("0005000010101A00", false)]);
        let m = z.lookup("0005000010101a00/META/manual.bfma").unwrap();
        assert_eq!(z.read(&m).unwrap(), b"SARC manual");
        let rpx = z.lookup("0005000010101A00/code/game.rpx").unwrap();
        assert_eq!(z.read(&rpx).unwrap(), big);
        assert_eq!(z.read(&z.lookup("0005000010101A00/meta/iconTex.tga").unwrap()).unwrap(), b"icon");
        assert!(z.lookup("0005000010101A00/meta/nope").is_none());
        assert!(z.lookup("0005000010101A00/meta/Manual.bfma/x").is_none());
    }

    #[test]
    fn refuses_what_isnt_one() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.wua");
        std::fs::write(&f, [0u8; 200]).unwrap();
        assert_eq!(ZArchive::open(&f).err().as_deref(), Some("not a ZArchive"));
        std::fs::write(&f, [0u8; 20]).unwrap();
        assert_eq!(ZArchive::open(&f).err().as_deref(), Some("too small"));
    }
}


//! A Windows program's own icon, read from its .exe (its PE resources): the largest picture of its first icon group,
//! for the case of a console's "Open <emulator>".

use std::path::Path;

/// The picture: width, height and RGBA.
pub type Icon = (u32, u32, Vec<u8>);

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

struct Pe<'a> {
    b: &'a [u8],
    /// (virtual address, virtual size, file offset) of each section
    sections: Vec<(u32, u32, u32)>,
    /// the resource directory's file offset
    base: usize,
}

impl<'a> Pe<'a> {
    fn read(b: &'a [u8]) -> Option<Pe<'a>> {
        if b.get(..2) != Some(b"MZ") {
            return None;
        }
        let pe = u32_at(b, 0x3c)? as usize;
        if b.get(pe..pe + 4) != Some(b"PE\0\0") {
            return None;
        }
        let (n, opt_size) = (u16_at(b, pe + 6)? as usize, u16_at(b, pe + 20)? as usize);
        let opt = pe + 24;
        let dirs = opt + if u16_at(b, opt)? == 0x20b { 112 } else { 96 };
        let rsrc = u32_at(b, dirs + 2 * 8)?;
        let sections = (0..n.min(96)).map(|i| opt + opt_size + i * 40).map(|s| Some((u32_at(b, s + 12)?, u32_at(b, s + 8)?, u32_at(b, s + 20)?))).collect::<Option<Vec<_>>>()?;
        let mut pe = Pe { b, sections, base: 0 };
        pe.base = pe.offset(rsrc)?;
        Some(pe)
    }

    fn offset(&self, rva: u32) -> Option<usize> {
        self.sections.iter().find(|(va, vs, _)| rva >= *va && rva < va + (*vs).max(1)).map(|(va, _, raw)| (rva - va + raw) as usize)
    }

    /// A resource directory's entries: (id, the entry's offset field).
    fn entries(&self, at: usize) -> Vec<(u32, u32)> {
        let n = u16_at(self.b, at + 12).unwrap_or(0) as usize + u16_at(self.b, at + 14).unwrap_or(0) as usize;
        (0..n.min(4096)).filter_map(|i| Some((u32_at(self.b, at + 16 + i * 8)?, u32_at(self.b, at + 20 + i * 8)?))).collect()
    }

    /// The data a directory entry ends in, following its first child at each level.
    fn data(&self, mut field: u32) -> Option<&'a [u8]> {
        for _ in 0..4 {
            if field & 0x8000_0000 == 0 {
                let at = self.base + field as usize;
                let (rva, size) = (u32_at(self.b, at)?, u32_at(self.b, at + 4)? as usize);
                let start = self.offset(rva)?;
                return self.b.get(start..start + size);
            }
            field = self.entries(self.base + (field & 0x7fff_ffff) as usize).first()?.1;
        }
        None
    }

    fn of_type(&self, kind: u32) -> Vec<(u32, u32)> {
        let Some(&(_, field)) = self.entries(self.base).iter().find(|e| e.0 == kind) else { return Vec::new() };
        if field & 0x8000_0000 == 0 {
            return Vec::new();
        }
        self.entries(self.base + (field & 0x7fff_ffff) as usize)
    }
}

/// A 32-bit icon bitmap (a DIB without its file header, twice its height for the mask) as RGBA.
fn dib(b: &[u8]) -> Option<Icon> {
    let (w, h2, bits) = (u32_at(b, 4)? as i32, u32_at(b, 8)? as i32, u16_at(b, 14)?);
    let (w, h, head) = (w.unsigned_abs(), (h2 / 2).unsigned_abs(), u32_at(b, 0)? as usize);
    if bits != 32 || w == 0 || h == 0 || w > 1024 || h > 1024 {
        return None;
    }
    let row = w as usize * 4;
    let mut out = vec![0u8; row * h as usize];
    for y in 0..h as usize {
        let src = b.get(head + (h as usize - 1 - y) * row..head + (h as usize - y) * row)?;
        for (o, p) in out[y * row..(y + 1) * row].as_chunks_mut::<4>().0.iter_mut().zip(src.as_chunks::<4>().0) {
            o.copy_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    Some((w, h, out))
}

/// The largest picture of the program's first icon group, None for a file that isn't a Windows program or has no icon.
pub fn largest(exe: &Path) -> Option<Icon> {
    let b = std::fs::read(exe).ok()?;
    let pe = Pe::read(&b)?;
    let icons: Vec<(u32, &[u8])> = pe.of_type(RT_ICON).into_iter().filter_map(|(id, f)| Some((id, pe.data(f)?))).collect();
    let group = pe.data(pe.of_type(RT_GROUP_ICON).first()?.1)?;
    let count = u16_at(group, 4)? as usize;
    let mut sizes: Vec<(u32, u16)> = (0..count.min(64))
        .filter_map(|i| {
            let at = 6 + i * 14;
            let w = u32::from(*group.get(at)?);
            Some((if w == 0 { 256 } else { w }, u16_at(group, at + 12)?))
        })
        .collect();
    sizes.sort_by_key(|s| std::cmp::Reverse(s.0));
    sizes.into_iter().find_map(|(_, id)| {
        let data = icons.iter().find(|i| i.0 == u32::from(id))?.1;
        if data.starts_with(b"\x89PNG") {
            let img = image::load_from_memory_with_format(data, image::ImageFormat::Png).ok()?.into_rgba8();
            Some((img.width(), img.height(), img.into_raw()))
        } else {
            dib(data)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A smallest PE with one icon group of one 32-bit 2×2 bitmap.
    fn exe() -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        b[0x94..0x96].copy_from_slice(&224u16.to_le_bytes());
        let opt = 0x98;
        b[opt..opt + 2].copy_from_slice(&0x10bu16.to_le_bytes());
        b[opt + 96 + 16..opt + 96 + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        let sec = opt + 224;
        b[sec + 8..sec + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sec + 12..sec + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        b[sec + 20..sec + 24].copy_from_slice(&0x200u32.to_le_bytes());
        // the resources at 0x200 (RVA 0x1000)
        let mut r = vec![0u8; 0x200];
        let dir = |r: &mut Vec<u8>, at: usize, entries: &[(u32, u32)]| {
            r[at + 14..at + 16].copy_from_slice(&(entries.len() as u16).to_le_bytes());
            for (i, (id, f)) in entries.iter().enumerate() {
                r[at + 16 + i * 8..at + 20 + i * 8].copy_from_slice(&id.to_le_bytes());
                r[at + 20 + i * 8..at + 24 + i * 8].copy_from_slice(&f.to_le_bytes());
            }
        };
        let sub = 0x8000_0000;
        dir(&mut r, 0, &[(3, sub | 0x20), (14, sub | 0x40)]);
        dir(&mut r, 0x20, &[(1, 0x60)]);
        dir(&mut r, 0x40, &[(1, 0x70)]);
        // data entries: (rva, size)
        let icon_at = 0x100usize;
        let mut icon = vec![0u8; 40 + 16];
        icon[0..4].copy_from_slice(&40u32.to_le_bytes());
        icon[4..8].copy_from_slice(&2u32.to_le_bytes());
        icon[8..12].copy_from_slice(&4u32.to_le_bytes());
        icon[14..16].copy_from_slice(&32u16.to_le_bytes());
        // bottom row blue, top row red (BGRA)
        icon[40..48].copy_from_slice(&[255, 0, 0, 255, 255, 0, 0, 255]);
        icon[48..56].copy_from_slice(&[0, 0, 255, 255, 0, 0, 255, 255]);
        let group_at = 0x180usize;
        let mut group = vec![0u8; 6 + 14];
        group[4..6].copy_from_slice(&1u16.to_le_bytes());
        group[6] = 2;
        group[7] = 2;
        group[18..20].copy_from_slice(&1u16.to_le_bytes());
        r[0x60..0x64].copy_from_slice(&(0x1000 + icon_at as u32).to_le_bytes());
        r[0x64..0x68].copy_from_slice(&(icon.len() as u32).to_le_bytes());
        r[0x70..0x74].copy_from_slice(&(0x1000 + group_at as u32).to_le_bytes());
        r[0x74..0x78].copy_from_slice(&(group.len() as u32).to_le_bytes());
        r[icon_at..icon_at + icon.len()].copy_from_slice(&icon);
        r[group_at..group_at + group.len()].copy_from_slice(&group);
        b[0x200..0x400].copy_from_slice(&r);
        b
    }

    #[test]
    fn a_program_s_icon_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("emu.exe");
        std::fs::write(&f, exe()).unwrap();
        let (w, h, rgba) = largest(&f).unwrap();
        assert_eq!((w, h), (2, 2));
        assert_eq!(&rgba[..4], &[255, 0, 0, 255], "the top row first");
        assert_eq!(&rgba[8..12], &[0, 0, 255, 255]);
        std::fs::write(&f, b"#!/bin/sh").unwrap();
        assert!(largest(&f).is_none());
    }
}

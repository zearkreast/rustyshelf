// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/gx2.js, as RustyShelf has it.
//! Wii U textures (BFLIM, as e-manual layouts use them): the GX2 GPU tiling undone and the pixel format decoded to
//! RGBA. Tiling follows AMD's AddrLib for the Wii U's R7xx-class GPU (2 pipes, 4 banks, 256-byte pipe interleave),
//! only the modes these files use: 1 linear-aligned, 2 1D-tiled-thin1, 4 2D-tiled-thin1.

const PIPES: usize = 2;
const BANKS: usize = 4;
const GROUP_BITS: usize = 8;
const PIPE_BITS: usize = 1;
const BANK_BITS: usize = 2;

/// e-manual textures are 1024-2048 px at most: past this the header is damaged or hostile
pub const MAX_SIDE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    L8,
    A8,
    La4,
    La8,
    Hilo8,
    Rgb565,
    Rgbx8,
    Rgb5a1,
    Rgba4,
    Rgba8,
    Bc1,
    Bc2,
    Bc3,
    Bc4L,
    Bc4A,
    Bc5,
}

/// A BFLIM format code: its format, bits per element, block size.
pub fn format(code: u8) -> Option<(Format, usize, usize)> {
    use Format::*;
    Some(match code {
        0 => (L8, 8, 1),
        1 => (A8, 8, 1),
        2 => (La4, 8, 1),
        3 => (La8, 16, 1),
        4 => (Hilo8, 16, 1),
        5 => (Rgb565, 16, 1),
        6 => (Rgbx8, 32, 1),
        7 => (Rgb5a1, 16, 1),
        8 => (Rgba4, 16, 1),
        9 | 20 => (Rgba8, 32, 1),
        12 | 21 => (Bc1, 64, 4),
        13 | 22 => (Bc2, 128, 4),
        14 | 23 => (Bc3, 128, 4),
        15 => (Bc4L, 64, 4),
        16 => (Bc4A, 64, 4),
        17 => (Bc5, 128, 4),
        _ => return None,
    })
}

fn pixel_index(x: usize, y: usize, bpp: usize) -> usize {
    let (b0, b1, b2, b3, b4, b5) = match bpp {
        8 => (x & 1, (x & 2) >> 1, (x & 4) >> 2, (y & 2) >> 1, y & 1, (y & 4) >> 2),
        16 => (x & 1, (x & 2) >> 1, (x & 4) >> 2, y & 1, (y & 2) >> 1, (y & 4) >> 2),
        64 => (x & 1, y & 1, (x & 2) >> 1, (x & 4) >> 2, (y & 2) >> 1, (y & 4) >> 2),
        128 => (y & 1, x & 1, (x & 2) >> 1, (x & 4) >> 2, (y & 2) >> 1, (y & 4) >> 2),
        _ => (x & 1, (x & 2) >> 1, y & 1, (x & 4) >> 2, (y & 2) >> 1, (y & 4) >> 2),
    };
    32 * b5 + 16 * b4 + 8 * b3 + 4 * b2 + 2 * b1 + b0
}

fn addr_micro(x: usize, y: usize, bpp: usize, pitch: usize) -> usize {
    let micro_tile_bytes = (64 * bpp) >> 3;
    ((bpp * pixel_index(x, y, bpp)) >> 3) + micro_tile_bytes * ((x >> 3) + (y >> 3) * (pitch >> 3))
}

fn addr_macro(x: usize, y: usize, bpp: usize, pitch: usize, pipe_swizzle: usize, bank_swizzle: usize) -> usize {
    let elem_offset = (bpp * pixel_index(x, y, bpp)) >> 3;
    let pipe = ((y >> 3) ^ (x >> 3)) & 1;
    let bank = (((y / (16 * PIPES)) ^ (x >> 3)) & 1) | (2 * (((y / (8 * PIPES)) ^ (x >> 4)) & 1));
    let bank_pipe = ((pipe + PIPES * bank) ^ (pipe_swizzle + PIPES * bank_swizzle)) % (PIPES * BANKS);
    let (pipe, bank) = (bank_pipe % PIPES, bank_pipe / PIPES);
    let (mt_w, mt_h) = (8 * BANKS, 8 * PIPES);
    let macro_tile_bytes = (bpp * mt_h * mt_w + 7) >> 3;
    let macro_tile_offset = (x / mt_w + (pitch / mt_w) * (y / mt_h)) * macro_tile_bytes;
    let total = elem_offset + (macro_tile_offset >> (BANK_BITS + PIPE_BITS));
    let group_mask = (1 << GROUP_BITS) - 1;
    let (high, low) = ((total & !group_mask) << (BANK_BITS + PIPE_BITS), total & group_mask);
    (bank << (PIPE_BITS + GROUP_BITS)) + (pipe << GROUP_BITS) + low + high
}

fn align(v: usize, a: usize) -> usize {
    v.div_ceil(a) * a
}

/// Tiled → linear elements (pixels, or 4×4 blocks for BCn).
pub fn untile(data: &[u8], w: usize, h: usize, bpp: usize, tile_mode: u8, swizzle: usize) -> Result<Vec<u8>, String> {
    if !(w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE) {
        return Err(format!("texture size {w}×{h} is out of range"));
    }
    let bytes = bpp >> 3;
    let mut out = vec![0u8; w * h * bytes];
    let (pipe_swizzle, bank_swizzle) = ((swizzle >> 8) & 1, (swizzle >> 9) & 3);
    let pitch = match tile_mode {
        4 => align(w, 32.max(32 * (256 / bpp / 8))),
        2 => align(w, 8.max(256 / bpp)),
        _ => align(w, 64.max(2048 / bpp)),
    };
    for y in 0..h {
        for x in 0..w {
            let a = match tile_mode {
                4 => addr_macro(x, y, bpp, pitch, pipe_swizzle, bank_swizzle),
                2 => addr_micro(x, y, bpp, pitch),
                _ => (y * pitch + x) * bytes,
            };
            if let Some(src) = data.get(a..a + bytes) {
                let d = (y * w + x) * bytes;
                out[d..d + bytes].copy_from_slice(src);
            }
        }
    }
    Ok(out)
}

fn rgb565(v: u16) -> [u8; 3] {
    let v = u32::from(v);
    [(((v >> 11) & 31) * 255 / 31) as u8, (((v >> 5) & 63) * 255 / 63) as u8, ((v & 31) * 255 / 31) as u8]
}

fn le16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn bc1_block(src: &[u8], o: usize, out: &mut [u8], w: usize, bx: usize, by: usize, alpha_ok: bool) {
    let (c0, c1) = (le16(src, o), le16(src, o + 2));
    let bits = u32::from_le_bytes([src[o + 4], src[o + 5], src[o + 6], src[o + 7]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |f: &dyn Fn(u32, u32) -> u32| -> [u8; 4] { [f(a[0].into(), b[0].into()) as u8, f(a[1].into(), b[1].into()) as u8, f(a[2].into(), b[2].into()) as u8, 255] };
    let pal: [[u8; 4]; 4] = if c0 > c1 || !alpha_ok {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(&|p, q| (2 * p + q) / 3), mix(&|p, q| (p + 2 * q) / 3)]
    } else {
        [[a[0], a[1], a[2], 255], [b[0], b[1], b[2], 255], mix(&|p, q| (p + q) / 2), [0, 0, 0, 0]]
    };
    for i in 0..16 {
        let p = pal[((bits >> (2 * i)) & 3) as usize];
        let d = ((by + (i >> 2)) * w + bx + (i & 3)) * 4;
        out[d..d + 4].copy_from_slice(&p);
    }
}

fn bc4_block(src: &[u8], o: usize) -> [u8; 16] {
    let (a0, a1) = (u32::from(src[o]), u32::from(src[o + 1]));
    let mut pal = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for i in 1..7 {
            pal[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1) / 7;
        }
    } else {
        for i in 1..5 {
            pal[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1) / 5;
        }
        (pal[6], pal[7]) = (0, 255);
    }
    let bits = (2..8).rev().fold(0u64, |v, k| (v << 8) | u64::from(src[o + k]));
    std::array::from_fn(|i| pal[((bits >> (3 * i)) & 7) as usize] as u8)
}

pub fn decode(fmt: Format, lin: &[u8], w: usize, h: usize) -> Result<Vec<u8>, String> {
    use Format::*;
    if !(w > 0 && h > 0 && w <= MAX_SIDE && h <= MAX_SIDE) {
        return Err(format!("texture size {w}×{h} is out of range"));
    }
    let mut out = vec![0u8; w * h * 4];
    if matches!(fmt, Bc1 | Bc2 | Bc3 | Bc4L | Bc4A | Bc5) {
        let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
        let big_w = bw * 4;
        let mut tmp = vec![0u8; big_w * bh * 4 * 4];
        let es = if matches!(fmt, Bc1 | Bc4L | Bc4A) { 8 } else { 16 };
        if lin.len() < bw * bh * es {
            return Err("texture data cut short".into());
        }
        for by in 0..bh {
            for bx in 0..bw {
                let o = (by * bw + bx) * es;
                let at = |i: usize| ((by * 4 + (i >> 2)) * big_w + bx * 4 + (i & 3)) * 4;
                let put = |tmp: &mut [u8], i: usize, p: [u8; 4]| tmp[at(i)..at(i) + 4].copy_from_slice(&p);
                match fmt {
                    Bc1 => bc1_block(lin, o, &mut tmp, big_w, bx * 4, by * 4, true),
                    Bc2 => {
                        bc1_block(lin, o + 8, &mut tmp, big_w, bx * 4, by * 4, false);
                        for i in 0..16 {
                            tmp[at(i) + 3] = ((lin[o + (i >> 1)] >> ((i & 1) * 4)) & 15) * 17;
                        }
                    }
                    Bc3 => {
                        bc1_block(lin, o + 8, &mut tmp, big_w, bx * 4, by * 4, false);
                        let al = bc4_block(lin, o);
                        for (i, a) in al.iter().enumerate() {
                            tmp[at(i) + 3] = *a;
                        }
                    }
                    Bc4L => {
                        for (i, v) in bc4_block(lin, o).into_iter().enumerate() {
                            put(&mut tmp, i, [v, v, v, 255]);
                        }
                    }
                    Bc4A => {
                        for (i, v) in bc4_block(lin, o).into_iter().enumerate() {
                            put(&mut tmp, i, [255, 255, 255, v]);
                        }
                    }
                    // layouts use BC5 as luminance + alpha
                    _ => {
                        let (l, a) = (bc4_block(lin, o), bc4_block(lin, o + 8));
                        for i in 0..16 {
                            put(&mut tmp, i, [l[i], l[i], l[i], a[i]]);
                        }
                    }
                }
            }
        }
        for y in 0..h {
            out[y * w * 4..(y + 1) * w * 4].copy_from_slice(&tmp[y * big_w * 4..y * big_w * 4 + w * 4]);
        }
        return Ok(out);
    }
    let per = match fmt {
        Rgba8 | Rgbx8 => 4,
        La8 | Hilo8 | Rgb565 | Rgb5a1 | Rgba4 => 2,
        _ => 1,
    };
    if lin.len() < w * h * per {
        return Err("texture data cut short".into());
    }
    for i in 0..w * h {
        let d = i * 4;
        let v = || u32::from(le16(lin, i * 2));
        let p: [u8; 4] = match fmt {
            Rgba8 => [lin[d], lin[d + 1], lin[d + 2], lin[d + 3]],
            Rgbx8 => [lin[d], lin[d + 1], lin[d + 2], 255],
            L8 => [lin[i], lin[i], lin[i], 255],
            A8 => [255, 255, 255, lin[i]],
            La4 => {
                let l = (lin[i] >> 4) * 17;
                [l, l, l, (lin[i] & 15) * 17]
            }
            La8 | Hilo8 => [lin[i * 2], lin[i * 2], lin[i * 2], lin[i * 2 + 1]],
            Rgb565 => {
                let c = rgb565(le16(lin, i * 2));
                [c[0], c[1], c[2], 255]
            }
            Rgb5a1 => {
                let v = v();
                [((v & 31) * 255 / 31) as u8, (((v >> 5) & 31) * 255 / 31) as u8, (((v >> 10) & 31) * 255 / 31) as u8, if v >> 15 != 0 { 255 } else { 0 }]
            }
            Rgba4 => {
                let v = v();
                [((v & 15) * 17) as u8, (((v >> 4) & 15) * 17) as u8, (((v >> 8) & 15) * 17) as u8, ((v >> 12) * 17) as u8]
            }
            _ => [0, 0, 0, 255],
        };
        out[d..d + 4].copy_from_slice(&p);
    }
    Ok(out)
}

/// A decoded texture, RGBA8.
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub format: Format,
}

/// A BFLIM's picture; None when its format isn't one known here.
pub fn read_flim(buf: &[u8]) -> Result<Option<Texture>, String> {
    let n = buf.len();
    if n < 0x28 || &buf[n - 0x28..n - 0x24] != b"FLIM" {
        return Ok(None);
    }
    let im = n - 0x14;
    if &buf[im..im + 4] != b"imag" {
        return Ok(None);
    }
    let be16 = |o: usize| usize::from(u16::from_be_bytes([buf[o], buf[o + 1]]));
    let (width, height, fmt, flags) = (be16(im + 8), be16(im + 10), buf[im + 14], buf[im + 15]);
    let size = u32::from_be_bytes([buf[im + 16], buf[im + 17], buf[im + 18], buf[im + 19]]) as usize;
    let Some((format, bpp, blk)) = format(fmt) else { return Ok(None) };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("texture is {width}×{height}: bigger than {MAX_SIDE}×{MAX_SIDE}, not reading it"));
    }
    let (tile_mode, swizzle) = (flags & 0x1F, usize::from((flags >> 5) & 7) << 8);
    let lin = untile(&buf[..size.min(n)], width.div_ceil(blk), height.div_ceil(blk), bpp, tile_mode, swizzle)?;
    Ok(Some(Texture { width, height, rgba: decode(format, &lin, width, height)?, format }))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A BFLIM around `data` (already tiled as `flags` says).
    pub(crate) fn flim(data: &[u8], w: u16, h: u16, fmt: u8, flags: u8) -> Vec<u8> {
        let mut b = data.to_vec();
        b.extend_from_slice(b"FLIM");
        b.extend_from_slice(&[0xFE, 0xFF, 0, 0x14, 2, 2, 0, 0]);
        b.extend_from_slice(&((data.len() + 0x28) as u32).to_be_bytes());
        b.extend_from_slice(&[0, 1, 0, 0]);
        b.extend_from_slice(b"imag");
        b.extend_from_slice(&0x10u32.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[0, 0x20, fmt, flags]);
        b.extend_from_slice(&(data.len() as u32).to_be_bytes());
        b
    }

    fn fnv(b: &[u8]) -> u32 {
        b.iter().fold(0x811c_9dc5u32, |h, &c| (h ^ u32::from(c)).wrapping_mul(0x0100_0193))
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| ((i * 37 + i / 7) % 256) as u8).collect()
    }

    #[test]
    fn linear_rgba8_and_small_formats() {
        // 2×2 RGBA8, linear-aligned: rows 64 pixels apart
        let mut data = vec![0u8; 64 * 4 * 2];
        data[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        data[256..264].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let t = read_flim(&flim(&data, 2, 2, 9, 1)).unwrap().unwrap();
        assert_eq!((t.width, t.height, t.format), (2, 2, Format::Rgba8));
        assert_eq!(t.rgba, (1..=16).collect::<Vec<u8>>());
        assert_eq!(decode(Format::La4, &[0xF0], 1, 1).unwrap(), [255, 255, 255, 0]);
        assert_eq!(decode(Format::A8, &[7], 1, 1).unwrap(), [255, 255, 255, 7]);
        assert_eq!(decode(Format::Rgb565, &0xF800u16.to_le_bytes(), 1, 1).unwrap(), [255, 0, 0, 255]);
        assert_eq!(decode(Format::Rgba4, &0x8421u16.to_le_bytes(), 1, 1).unwrap(), [17, 34, 68, 136]);
        assert_eq!(decode(Format::Rgb5a1, &0x801Fu16.to_le_bytes(), 1, 1).unwrap(), [255, 0, 0, 255]);
        assert!(read_flim(&flim(&data, 2, 2, 99, 1)).unwrap().is_none(), "a format not known");
        assert!(read_flim(&flim(&data, 5000, 2, 9, 1)).is_err());
        assert!(read_flim(b"short").unwrap().is_none());
    }

    #[test]
    fn block_formats() {
        // BC1, red and blue end points; texels 0,1,2,3 in turn
        let mut b = Vec::new();
        b.extend_from_slice(&0xF800u16.to_le_bytes());
        b.extend_from_slice(&0x001Fu16.to_le_bytes());
        b.extend_from_slice(&0xE4E4_E4E4u32.to_le_bytes());
        let out = decode(Format::Bc1, &b, 4, 4).unwrap();
        assert_eq!(&out[..16], &[255, 0, 0, 255, 0, 0, 255, 255, 170, 0, 85, 255, 85, 0, 170, 255]);
        // c0 <= c1: the fourth colour is transparent
        let mut b2 = b.clone();
        b2.swap(0, 2);
        b2.swap(1, 3);
        let out = decode(Format::Bc1, &b2, 4, 4).unwrap();
        assert_eq!(&out[8..16], &[127, 0, 127, 255, 0, 0, 0, 0]);
        // BC4: 8-step ramp, and the 6-step one with 0 and 255
        let mut b4 = vec![255u8, 0];
        let idx: u64 = (0..16).map(|i| ((i % 8) as u64) << (3 * i)).sum();
        b4.extend_from_slice(&idx.to_le_bytes()[..6]);
        let out = decode(Format::Bc4L, &b4, 4, 4).unwrap();
        assert_eq!((0..8).map(|i| out[i * 4]).collect::<Vec<_>>(), [255, 0, 218, 182, 145, 109, 72, 36]);
        b4.swap(0, 1);
        let out = decode(Format::Bc4A, &b4, 4, 4).unwrap();
        assert_eq!((0..8).map(|i| out[i * 4 + 3]).collect::<Vec<_>>(), [0, 255, 51, 102, 153, 204, 0, 255]);
        // a 6×6 picture is two blocks wide and high, cut to size
        let out = decode(Format::Bc5, &pattern(4 * 16), 6, 6).unwrap();
        assert_eq!(out.len(), 6 * 6 * 4);
    }

    // the same bytes through main/gx2.js (untile + decode, fnv-1a of the RGBA)
    #[test]
    fn tiled_modes_match_the_electron_app() {
        for (fmt, w, h, mode, swz, want) in [
            (9u8, 40usize, 20usize, 4u8, 0usize, 0x2fbb_c1e4u32),
            (9, 40, 20, 4, 3 << 8, 0x353d_abfd),
            (9, 40, 20, 2, 0, 0x042a_5a90),
            (0, 70, 33, 4, 0, 0x5f01_053f),
            (5, 33, 17, 2, 0, 0xf2d9_7c4e),
            (12, 64, 64, 4, 1 << 8, 0xf632_3eb2),
            (14, 32, 32, 4, 0, 0x6cbb_9558),
            (17, 20, 12, 2, 0, 0x92ee_b6e9),
        ] {
            let (f, bpp, blk) = format(fmt).unwrap();
            let lin = untile(&pattern(256 * 1024), w.div_ceil(blk), h.div_ceil(blk), bpp, mode, swz).unwrap();
            assert_eq!(fnv(&decode(f, &lin, w, h).unwrap()), want, "format {fmt} {w}×{h} mode {mode} swizzle {swz}");
        }
    }
}

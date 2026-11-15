// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/bflyt.js, as RustyShelf has it.
//! BFLYT (Wii U / Cafe layout, big-endian): the pane tree with what a page needs to be drawn — pictures, windows
//! (frames + content), text boxes (position only: e-manual text comes laid out in a file of its own), user data.
//! Layout space: y points up, a pane's translation is from its parent's position, its rect hangs off its origin.

pub type Rgba = [u8; 4];
pub type Uv = [[f64; 2]; 4];

#[derive(Debug, Clone, PartialEq)]
pub struct TexRef {
    pub index: u16,
    pub wrap_s: u8,
    pub wrap_t: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub name: String,
    pub black: Rgba,
    pub white: Rgba,
    pub tex: Vec<TexRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub mat: u16,
    /// 1 horizontal, 2 vertical, 4 turned half round
    pub flip: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UserValue {
    Str(String),
    Nums(Vec<f64>),
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub struct User {
    pub name: String,
    pub value: UserValue,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pane {
    /// "pan", "pic", "wnd", "txt", "bnd" or "prt"
    pub kind: &'static str,
    pub name: String,
    pub visible: bool,
    pub origin: u8,
    pub alpha: u8,
    pub x: f64,
    pub y: f64,
    pub sx: f64,
    pub sy: f64,
    pub w: f64,
    pub h: f64,
    pub colors: Option<[Rgba; 4]>,
    pub mat: u16,
    pub uv: Vec<Uv>,
    /// a window's content inflation: left, right, top, bottom
    pub inflate: Option<[f64; 4]>,
    pub frames: Vec<Frame>,
    pub text: Option<String>,
    pub user: Vec<User>,
    /// indexes into `Layout::panes`
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    pub textures: Vec<String>,
    pub materials: Vec<Material>,
    pub panes: Vec<Pane>,
    pub root: Option<usize>,
}

impl Layout {
    pub fn pane(&self, i: usize) -> &Pane {
        &self.panes[i]
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn bytes(&self, o: usize, n: usize) -> Result<&[u8], String> {
        o.checked_add(n).and_then(|e| self.0.get(o..e)).ok_or_else(|| format!("layout cut short at {o:#x}"))
    }
    fn u8(&self, o: usize) -> Result<u8, String> {
        Ok(self.bytes(o, 1)?[0])
    }
    fn u16(&self, o: usize) -> Result<u16, String> {
        let b = self.bytes(o, 2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&self, o: usize) -> Result<usize, String> {
        let b = self.bytes(o, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize)
    }
    fn i32(&self, o: usize) -> Result<i32, String> {
        Ok(self.u32(o)? as u32 as i32)
    }
    fn f32(&self, o: usize) -> Result<f64, String> {
        Ok(f64::from(f32::from_bits(self.u32(o)? as u32)))
    }
    fn rgba(&self, o: usize) -> Result<Rgba, String> {
        let b = self.bytes(o, 4)?;
        Ok([b[0], b[1], b[2], b[3]])
    }
    /// A NUL-ended Latin-1 string, cut at the end of the file and at `max` bytes.
    fn cstr(&self, o: usize, max: usize) -> String {
        let end = self.0.len().min(o.saturating_add(max));
        let s = self.0.get(o.min(end)..end).unwrap_or(&[]);
        s.iter().take_while(|&&c| c != 0).map(|&c| char::from(c)).collect()
    }
}

/// UTF-16BE text with any NULs at its end dropped (an odd last byte is ignored).
pub fn utf16be(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units).trim_end_matches('\0').to_string()
}

fn str_table(b: &R, o: usize) -> Result<Vec<String>, String> {
    let (n, base) = (b.u16(o + 8)?, o + 12);
    (0..usize::from(n)).map(|i| Ok(b.cstr(base + b.u32(base + i * 4)?, usize::MAX))).collect()
}

fn materials(b: &R, o: usize) -> Result<Vec<Material>, String> {
    let n = b.u16(o + 8)?;
    let mut out = Vec::new();
    for i in 0..usize::from(n) {
        let mut m = o + b.u32(o + 12 + i * 4)?;
        let mut mat = Material { name: b.cstr(m, 28), black: b.rgba(m + 28)?, white: b.rgba(m + 32)?, tex: Vec::new() };
        let flags = b.u32(m + 36)?;
        m += 40;
        let (tex_count, srt_count) = (flags & 3, (flags >> 2) & 3);
        for _ in 0..tex_count {
            mat.tex.push(TexRef { index: b.u16(m)?, wrap_s: b.u8(m + 2)? & 3, wrap_t: b.u8(m + 3)? & 3 });
            m += 4;
        }
        if srt_count > 0 {
            b.bytes(m, 20)?;
        }
        out.push(mat);
    }
    Ok(out)
}

fn uv_sets(b: &R, mut o: usize, count: u8) -> Result<Vec<Uv>, String> {
    let mut out = Vec::new();
    for _ in 0..count {
        let f = |i: usize| b.f32(o + i * 4);
        out.push([[f(0)?, f(1)?], [f(2)?, f(3)?], [f(4)?, f(5)?], [f(6)?, f(7)?]]);
        o += 32;
    }
    Ok(out)
}

fn pane(b: &R, o: usize, kind: &'static str) -> Result<Pane, String> {
    let f = |i: usize| b.f32(o + 0x2C + i * 4);
    let mut p = Pane {
        kind,
        name: b.cstr(o + 12, 24),
        visible: b.u8(o + 8)? & 1 != 0,
        origin: b.u8(o + 9)?,
        alpha: b.u8(o + 10)?,
        x: f(0)?,
        y: f(1)?,
        sx: f(6)?,
        sy: f(7)?,
        w: f(8)?,
        h: f(9)?,
        ..Pane::default()
    };
    let d = o + 0x54;
    let colors = |c: usize| -> Result<[Rgba; 4], String> { Ok([b.rgba(c)?, b.rgba(c + 4)?, b.rgba(c + 8)?, b.rgba(c + 12)?]) };
    match kind {
        "pic" => {
            p.colors = Some(colors(d)?);
            p.mat = b.u16(d + 16)?;
            p.uv = uv_sets(b, d + 20, b.u8(d + 18)?)?;
        }
        "wnd" => {
            p.inflate = Some([b.f32(d)?, b.f32(d + 4)?, b.f32(d + 8)?, b.f32(d + 12)?]);
            let (frames, c, ft) = (b.u8(d + 16)?, o + b.u32(d + 20)?, o + b.u32(d + 24)?);
            p.colors = Some(colors(c)?);
            p.mat = b.u16(c + 16)?;
            p.uv = uv_sets(b, c + 20, b.u8(c + 18)?)?;
            for k in 0..usize::from(frames) {
                let fo = o + b.u32(ft + k * 4)?;
                p.frames.push(Frame { mat: b.u16(fo)?, flip: b.u8(fo + 2)? });
            }
        }
        "txt" => {
            b.u16(d + 4)?;
            let (str_bytes, to) = (usize::from(b.u16(d + 2)?), b.u32(d + 16)?);
            if str_bytes > 2 && o + to + str_bytes <= b.0.len() {
                p.text = Some(utf16be(&b.0[o + to..o + to + str_bytes - 2]));
            }
        }
        _ => {}
    }
    Ok(p)
}

/// e-manual pictures name their JPEG here (JpegFileName)
fn user_data(b: &R, o: usize) -> Result<Vec<User>, String> {
    let n = b.u16(o + 8)?;
    let mut out = Vec::new();
    for i in 0..usize::from(n) {
        let e = o + 12 + i * 12;
        let name = b.cstr(e + b.u32(e)?, usize::MAX);
        let (data, len, kind) = (e + b.u32(e + 4)?, usize::from(b.u16(e + 8)?), b.u8(e + 10)?);
        let value = match kind {
            0 => UserValue::Str(b.cstr(data, len)),
            1 => UserValue::Nums((0..len).map(|k| b.i32(data + k * 4).map(f64::from)).collect::<Result<_, _>>()?),
            2 => UserValue::Nums((0..len).map(|k| b.f32(data + k * 4)).collect::<Result<_, _>>()?),
            _ => UserValue::None,
        };
        out.push(User { name, value });
    }
    Ok(out)
}

pub fn parse(bytes: &[u8]) -> Result<Layout, String> {
    let b = R(bytes);
    if bytes.get(0..4) != Some(b"FLYT") {
        return Err("not a layout".into());
    }
    let mut l = Layout::default();
    let mut o = usize::from(b.u16(6)?);
    let count = b.u16(16)?;
    let (mut stack, mut last, mut parent): (Vec<Option<usize>>, Option<usize>, Option<usize>) = (Vec::new(), None, None);
    for _ in 0..count {
        if o + 8 > bytes.len() {
            break;
        }
        let magic = &bytes[o..o + 4];
        let size = b.u32(o + 4)?;
        match magic {
            b"lyt1" => (l.width, l.height) = (b.f32(o + 12)?, b.f32(o + 16)?),
            b"txl1" => l.textures = str_table(&b, o)?,
            b"fnl1" => {
                str_table(&b, o)?;
            }
            b"mat1" => l.materials = materials(&b, o)?,
            b"pan1" | b"pic1" | b"wnd1" | b"txt1" | b"bnd1" | b"prt1" => {
                let kind = match magic {
                    b"pan1" => "pan",
                    b"pic1" => "pic",
                    b"wnd1" => "wnd",
                    b"txt1" => "txt",
                    b"bnd1" => "bnd",
                    _ => "prt",
                };
                let p = pane(&b, o, kind)?;
                let i = l.panes.len();
                l.panes.push(p);
                if let Some(pi) = parent {
                    l.panes[pi].children.push(i);
                } else if l.root.is_none() {
                    l.root = Some(i);
                }
                last = Some(i);
            }
            b"pas1" => {
                stack.push(parent);
                parent = last;
            }
            b"pae1" => parent = stack.pop().flatten(),
            b"usd1" => {
                if let Some(i) = last {
                    l.panes[i].user = user_data(&b, o)?;
                }
            }
            _ => {}
        }
        o = o.saturating_add(size);
    }
    Ok(l)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) type Mat<'a> = (Rgba, Rgba, &'a [(u16, u8, u8)]);

    /// Writes BFLYT sections, for tests here and in wiiumanual.
    #[derive(Default)]
    pub(crate) struct Writer {
        sections: Vec<Vec<u8>>,
    }

    fn f(v: f32) -> [u8; 4] {
        v.to_be_bytes()
    }

    pub(crate) struct P<'a> {
        pub kind: &'a [u8; 4],
        pub name: &'a str,
        pub origin: u8,
        pub alpha: u8,
        pub xy: (f32, f32),
        pub scale: (f32, f32),
        pub wh: (f32, f32),
    }

    impl Default for P<'_> {
        fn default() -> Self {
            P { kind: b"pan1", name: "", origin: 0, alpha: 255, xy: (0.0, 0.0), scale: (1.0, 1.0), wh: (0.0, 0.0) }
        }
    }

    impl Writer {
        fn section(&mut self, magic: &[u8; 4], body: &[u8]) {
            let mut s = magic.to_vec();
            s.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
            s.extend_from_slice(body);
            self.sections.push(s);
        }

        pub(crate) fn lyt(&mut self, w: f32, h: f32) -> &mut Self {
            let mut b = vec![0u8; 4];
            b.extend_from_slice(&f(w));
            b.extend_from_slice(&f(h));
            self.section(b"lyt1", &b);
            self
        }

        fn table(&mut self, magic: &[u8; 4], names: &[&str]) -> &mut Self {
            let mut b = (names.len() as u16).to_be_bytes().to_vec();
            b.extend_from_slice(&[0, 0]);
            let mut strs = Vec::new();
            let mut offs = Vec::new();
            for n in names {
                offs.extend_from_slice(&((names.len() * 4 + strs.len()) as u32).to_be_bytes());
                strs.extend_from_slice(n.as_bytes());
                strs.push(0);
            }
            b.extend_from_slice(&offs);
            b.extend_from_slice(&strs);
            self.section(magic, &b);
            self
        }

        pub(crate) fn textures(&mut self, names: &[&str]) -> &mut Self {
            self.table(b"txl1", names)
        }

        /// Materials: (black, white, textures as (index, wrap s, wrap t)).
        pub(crate) fn materials(&mut self, mats: &[Mat]) -> &mut Self {
            let mut b = (mats.len() as u16).to_be_bytes().to_vec();
            b.extend_from_slice(&[0, 0]);
            let mut body = Vec::new();
            let mut offs = Vec::new();
            for (k, (black, white, tex)) in mats.iter().enumerate() {
                offs.extend_from_slice(&((12 + mats.len() * 4 + body.len()) as u32).to_be_bytes());
                let mut name = format!("mat{k}").into_bytes();
                name.resize(28, 0);
                body.extend_from_slice(&name);
                body.extend_from_slice(black);
                body.extend_from_slice(white);
                body.extend_from_slice(&(tex.len() as u32).to_be_bytes());
                for (i, s, t) in tex.iter() {
                    body.extend_from_slice(&i.to_be_bytes());
                    body.extend_from_slice(&[*s, *t]);
                }
            }
            b.extend_from_slice(&offs);
            b.extend_from_slice(&body);
            self.section(b"mat1", &b);
            self
        }

        fn head(p: &P) -> Vec<u8> {
            let mut b = vec![1, p.origin, p.alpha, 0];
            let mut name = p.name.as_bytes().to_vec();
            name.resize(24, 0);
            b.extend_from_slice(&name);
            b.resize(0x2C - 8, 0);
            for v in [p.xy.0, p.xy.1, 0.0, 0.0, 0.0, 0.0, p.scale.0, p.scale.1, p.wh.0, p.wh.1] {
                b.extend_from_slice(&f(v));
            }
            b
        }

        pub(crate) fn pane(&mut self, p: &P) -> &mut Self {
            let b = Self::head(p);
            self.section(p.kind, &b);
            self
        }

        pub(crate) fn pic(&mut self, p: &P, colors: [Rgba; 4], mat: u16, uv: Option<Uv>) -> &mut Self {
            let mut b = Self::head(p);
            for c in colors {
                b.extend_from_slice(&c);
            }
            b.extend_from_slice(&mat.to_be_bytes());
            b.extend_from_slice(&[u8::from(uv.is_some()), 0]);
            for c in uv.iter().flatten().flatten() {
                b.extend_from_slice(&f(*c as f32));
            }
            self.section(b"pic1", &b);
            self
        }

        pub(crate) fn wnd(&mut self, p: &P, inflate: [f32; 4], mat: u16, frames: &[(u16, u8)]) -> &mut Self {
            let mut b = Self::head(p);
            for v in inflate {
                b.extend_from_slice(&f(v));
            }
            let content = b.len() + 8 + 12;
            b.push(frames.len() as u8);
            b.extend_from_slice(&[0, 0, 0]);
            b.extend_from_slice(&(content as u32).to_be_bytes());
            let ft = content + 16 + 4;
            b.extend_from_slice(&(ft as u32).to_be_bytes());
            for _ in 0..4 {
                b.extend_from_slice(&[255, 255, 255, 255]);
            }
            b.extend_from_slice(&mat.to_be_bytes());
            b.extend_from_slice(&[0, 0]);
            for k in 0..frames.len() {
                b.extend_from_slice(&((ft + frames.len() * 4 + k * 4) as u32).to_be_bytes());
            }
            for (m, flip) in frames {
                b.extend_from_slice(&m.to_be_bytes());
                b.extend_from_slice(&[*flip, 0]);
            }
            self.section(b"wnd1", &b);
            self
        }

        pub(crate) fn txt(&mut self, p: &P, text: &str) -> &mut Self {
            let mut b = Self::head(p);
            let mut s: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            s.extend_from_slice(&[0, 0]);
            b.extend_from_slice(&[0, 0]);
            b.extend_from_slice(&(s.len() as u16).to_be_bytes());
            b.extend_from_slice(&[0; 12]);
            let to = b.len() + 8 + 4;
            b.extend_from_slice(&(to as u32).to_be_bytes());
            b.extend_from_slice(&s);
            self.section(b"txt1", &b);
            self
        }

        /// User data: (name, value) with Str or Nums (ints).
        pub(crate) fn user(&mut self, entries: &[(&str, UserValue)]) -> &mut Self {
            let mut b = (entries.len() as u16).to_be_bytes().to_vec();
            b.extend_from_slice(&[0, 0]);
            let mut extra = Vec::new();
            let base = entries.len() * 12;
            let mut table = Vec::new();
            for (i, (name, value)) in entries.iter().enumerate() {
                let here = i * 12;
                let name_off = base + extra.len() - here;
                extra.extend_from_slice(name.as_bytes());
                extra.push(0);
                while extra.len() % 4 != 0 {
                    extra.push(0);
                }
                let data_off = base + extra.len() - here;
                let (len, kind) = match value {
                    UserValue::Str(s) => {
                        extra.extend_from_slice(s.as_bytes());
                        extra.push(0);
                        (s.len(), 0u8)
                    }
                    UserValue::Nums(v) => {
                        for n in v {
                            extra.extend_from_slice(&(*n as i32).to_be_bytes());
                        }
                        (v.len(), 1)
                    }
                    UserValue::None => (0, 3),
                };
                while extra.len() % 4 != 0 {
                    extra.push(0);
                }
                table.extend_from_slice(&(name_off as u32).to_be_bytes());
                table.extend_from_slice(&(data_off as u32).to_be_bytes());
                table.extend_from_slice(&(len as u16).to_be_bytes());
                table.extend_from_slice(&[kind, 0]);
            }
            b.extend_from_slice(&table);
            b.extend_from_slice(&extra);
            self.section(b"usd1", &b);
            self
        }

        pub(crate) fn open(&mut self) -> &mut Self {
            self.section(b"pas1", &[]);
            self
        }

        pub(crate) fn close(&mut self) -> &mut Self {
            self.section(b"pae1", &[]);
            self
        }

        pub(crate) fn bytes(&self) -> Vec<u8> {
            let mut b = b"FLYT".to_vec();
            b.extend_from_slice(&[0xFE, 0xFF, 0, 0x14, 0x02, 0x02, 0, 0, 0, 0, 0, 0]);
            b.extend_from_slice(&(self.sections.len() as u16).to_be_bytes());
            b.extend_from_slice(&[0, 0]);
            for s in &self.sections {
                b.extend_from_slice(s);
            }
            let len = b.len() as u32;
            b[12..16].copy_from_slice(&len.to_be_bytes());
            b
        }
    }

    pub(crate) const WHITE: Rgba = [255, 255, 255, 255];

    #[test]
    fn parses_a_layout() {
        let mut w = Writer::default();
        w.lyt(688.0, 1200.0).textures(&["paper.bflim", "frame.bflim"]).materials(&[([0, 0, 0, 0], WHITE, &[(0, 1, 1)]), ([10, 20, 30, 0], [200, 210, 220, 255], &[(1, 0, 2)])]);
        w.pane(&P { name: "RootPane", ..P::default() }).open();
        w.pic(&P { name: "P_bg", origin: 5, xy: (-344.0, 600.0), wh: (688.0, 1200.0), ..P::default() }, [WHITE; 4], 0, Some([[0.0, 0.0], [4.0, 0.0], [0.0, 6.0], [4.0, 6.0]]));
        w.user(&[("JpegFileName", UserValue::Str("photo01.jpg".into())), ("PageID_0", UserValue::Nums(vec![3.0, 4.0]))]);
        w.pane(&P { name: "N_group", xy: (10.0, -20.0), scale: (2.0, 0.5), ..P::default() }).open();
        w.wnd(&P { name: "W_frame", wh: (300.0, 200.0), ..P::default() }, [1.0, 2.0, 3.0, 4.0], 1, &[(1, 0), (1, 1), (1, 2), (1, 4)]);
        w.txt(&P { name: "T_body", alpha: 128, ..P::default() }, "Hello Wii U");
        w.close().close();
        w.pane(&P { name: "Stray", ..P::default() });
        let l = parse(&w.bytes()).unwrap();
        assert_eq!((l.width, l.height), (688.0, 1200.0));
        assert_eq!(l.textures, ["paper.bflim", "frame.bflim"]);
        assert_eq!(l.materials.len(), 2);
        assert_eq!(l.materials[1].black, [10, 20, 30, 0]);
        assert_eq!(l.materials[1].tex, [TexRef { index: 1, wrap_s: 0, wrap_t: 2 }]);
        let root = l.pane(l.root.unwrap());
        assert_eq!(root.name, "RootPane");
        assert_eq!(root.children.len(), 2);
        let bg = l.pane(root.children[0]);
        assert_eq!((bg.kind, bg.origin, bg.x, bg.y, bg.w), ("pic", 5, -344.0, 600.0, 688.0));
        assert_eq!(bg.uv[0][3], [4.0, 6.0]);
        assert_eq!(bg.user[0], User { name: "JpegFileName".into(), value: UserValue::Str("photo01.jpg".into()) });
        assert_eq!(bg.user[1].value, UserValue::Nums(vec![3.0, 4.0]));
        let group = l.pane(root.children[1]);
        assert_eq!((group.sx, group.sy), (2.0, 0.5));
        let wnd = l.pane(group.children[0]);
        assert_eq!(wnd.inflate, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(wnd.frames.iter().map(|f| (f.mat, f.flip)).collect::<Vec<_>>(), [(1, 0), (1, 1), (1, 2), (1, 4)]);
        assert_eq!(wnd.mat, 1);
        let txt = l.pane(group.children[1]);
        assert_eq!((txt.text.as_deref(), txt.alpha), (Some("Hello Wii U"), 128));
        assert_eq!(l.panes.len(), 6, "the pane after the root's end is read but hangs nowhere");
        assert!(parse(b"FLYX").is_err());
    }

    #[test]
    fn a_cut_layout() {
        let mut w = Writer::default();
        w.lyt(10.0, 10.0).pic(&P::default(), [WHITE; 4], 0, None);
        let b = w.bytes();
        assert!(parse(&b[..b.len() - 10]).is_err(), "a pane cut short");
        let whole_lyt = 0x14 + 20;
        let l = parse(&b[..whole_lyt + 4]).unwrap();
        assert!(l.root.is_none() && l.width == 10.0, "a section header past the end isn't read");
    }
}

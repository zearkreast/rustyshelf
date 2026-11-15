// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/spreads.js, as RustyShelf has it.
//! Manual scans uploaded as two-page spreads (the outside of the booklet is back cover | front cover, the inside
//! left | right) are cut into single pages: the front cover comes first, the back cover goes to the end, an
//! inside spread becomes two pages. The originals are moved aside, never deleted.

use std::fs;
use std::path::{Path, PathBuf};

use crate::boxart::size_of;
use crate::collate;

/// A booklet page is about 0.7 wide to tall and a spread about 1.4; screens as pages (16:9, 1.78) are left alone.
const SPREAD: f64 = 1.25;
const SPREAD_MAX: f64 = 1.62;
/// narrower than this, a wide page is a screen capture, not a booklet scanned open
const SPREAD_MIN_W: u32 = 1100;

type Size = Option<(u32, u32)>;

pub fn is_spread(size: Size) -> bool {
    size.is_some_and(|(w, h)| f64::from(w) > f64::from(h) * SPREAD && f64::from(w) < f64::from(h) * SPREAD_MAX)
}

/// How a manual is cut.
#[derive(Debug, Clone, Copy, Default)]
pub struct Split {
    /// pages known to be booklet scans (from a scan collection): any landscape page is a spread, squarish
    /// booklets included, even when every page is one
    pub scan: bool,
    /// a wide page narrower than this isn't cut (0: any)
    pub min_w: u32,
    /// the originals aren't kept
    pub discard: bool,
}

/// A manual's page files in `dir`, in page order.
fn pages(dir: &Path, base: &str) -> Vec<String> {
    let prefix = format!("{base}-manual-").to_lowercase();
    let numbered = |f: &str| {
        let l = f.to_lowercase();
        let Some((stem, ext)) = l.rsplit_once('.') else { return false };
        let digits = stem.len() - stem.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        matches!(ext, "jpg" | "jpeg" | "png") && digits > 0 && stem[..stem.len() - digits].ends_with("-manual-")
    };
    let mut files: Vec<String> = fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|f| f.to_lowercase().starts_with(&prefix) && numbered(f)).collect();
    files.sort_by(|a, b| collate::compare(a, b));
    files
}

/// A two-page scan's halves, as JPEGs.
fn halves(file: &Path) -> Option<(Vec<u8>, Vec<u8>)> {
    let img = image::open(file).ok()?;
    let (left, right) = rustyshelf_imgtools::crop::crop_both(&img);
    let jpeg = |half: image::DynamicImage| {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&half.into_rgb8()).ok()?;
        Some(out)
    };
    Some((jpeg(left)?, jpeg(right)?))
}

/// Cuts a manual's spreads into pages. Returns how many pages it has now, 0 when nothing was cut (it wasn't
/// scanned open). A failure puts the manual back exactly as it was.
pub fn split_manual(dir: &Path, base: &str, opts: Split) -> Result<usize, String> {
    let spread = |z: Size| if opts.scan { z.is_some_and(|(w, h)| f64::from(w) > f64::from(h) * 1.05 && f64::from(w) < f64::from(h) * SPREAD_MAX) } else { is_spread(z) };
    let files = pages(dir, base);
    let sizes: Vec<Size> = files.iter().map(|f| size_of(&dir.join(f))).collect();
    if !sizes.iter().any(|z| spread(*z)) {
        return Ok(0);
    }
    let shape = |z: (u32, u32)| f64::from(z.0) / f64::from(z.1);
    // a booklet that is landscape on every page (a digital manual laid out sideways) is left as it is
    if !opts.scan && files.len() > 2 && !sizes.iter().any(|z| z.is_some_and(|(w, h)| h >= w)) && sizes.iter().all(|z| z.is_some_and(|z| (shape(z) - sizes[0].map_or(0.0, shape)).abs() < 0.02)) {
        return Ok(0);
    }
    let wide: Vec<(u32, u32)> = sizes.iter().filter(|z| spread(**z)).flatten().copied().collect();
    if opts.min_w > 0 && wide.iter().any(|z| z.0 < opts.min_w) {
        return Ok(0);
    }
    // a manual from anywhere but a scan collection is cut only when it really is a booklet scanned open: wide
    // pages big enough to be a scan, and most of its pages that way. A single wide page among upright ones (a
    // box wrap or a poster) stays whole.
    if !opts.scan && (wide.iter().any(|z| z.0 < SPREAD_MIN_W) || wide.len() * 2 < sizes.iter().flatten().count()) {
        return Ok(0);
    }
    enum Page {
        Whole(PathBuf, &'static str),
        Half(Vec<u8>),
    }
    let (mut front, mut back, mut cut) = (Vec::new(), Vec::new(), 0);
    for (i, f) in files.iter().enumerate() {
        let full = dir.join(f);
        let ext = if f.to_lowercase().ends_with(".png") { ".png" } else { ".jpg" };
        match Some(()).filter(|()| spread(sizes[i])).and_then(|()| halves(&full)) {
            None => front.push(Page::Whole(full, ext)),
            Some((left, right)) => {
                cut += 1;
                // the outside: front cover first, back cover last
                if i == 0 {
                    front.push(Page::Half(right));
                    back.push(Page::Half(left));
                } else {
                    front.extend([Page::Half(left), Page::Half(right)]);
                }
            }
        }
    }
    if cut == 0 {
        return Ok(0);
    }
    front.append(&mut back);
    // the new pages go under other names first; the originals then move to their own backup folder, or to a
    // holding folder dropped at the end
    let hold = if opts.discard { dir.join(format!(".{base}-split-old")) } else { backup_dir(dir, base).map_err(|e| e.to_string())? };
    let (mut tmp, mut moved, mut placed) = (Vec::new(), Vec::new(), Vec::new());
    let done = (|| -> std::io::Result<()> {
        for (i, p) in front.iter().enumerate() {
            let (ext, bytes) = match p {
                Page::Whole(file, ext) => (*ext, fs::read(file)?),
                Page::Half(bytes) => (".jpg", bytes.clone()),
            };
            let t = dir.join(format!(".{base}-split-{i}{ext}"));
            tmp.push((t.clone(), ext));
            fs::write(&t, bytes)?;
        }
        if opts.discard {
            fs::create_dir_all(&hold)?;
        }
        for f in &files {
            fs::rename(dir.join(f), hold.join(f))?;
            moved.push(f.clone());
        }
        for (i, (t, ext)) in tmp.iter().enumerate() {
            let d = dir.join(format!("{base}-manual-{:02}{ext}", i + 1));
            fs::rename(t, &d)?;
            placed.push(d);
        }
        Ok(())
    })();
    if let Err(e) = done {
        for d in &placed {
            let _ = fs::remove_file(d);
        }
        for f in &moved {
            let _ = fs::rename(hold.join(f), dir.join(f));
        }
        for (t, _) in &tmp {
            let _ = fs::remove_file(t);
        }
        // only when empty: an original that couldn't go back stays in it
        if fs::remove_dir(&hold).is_ok() && !opts.discard {
            let _ = hold.parent().map(fs::remove_dir);
        }
        return Err(e.to_string());
    }
    if opts.discard {
        let _ = fs::remove_dir_all(&hold);
    }
    Ok(front.len())
}

/// A new, empty `.manual-originals/<game>/<time>` beside the pages, for pages about to be replaced: never one
/// that already holds a backup.
pub fn backup_dir(dir: &Path, base: &str) -> std::io::Result<PathBuf> {
    let root = dir.join(".manual-originals").join(base);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let (days, rest) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (d, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    let stamp = format!("{:04}-{m:02}-{d:02}T{:02}-{:02}-{:02}-{:03}", yoe + era * 400 + i64::from(m <= 2), rest / 3_600_000, rest % 3_600_000 / 60_000, rest % 60_000 / 1000, rest % 1000);
    fs::create_dir_all(&root)?;
    for i in 0.. {
        let d = root.join(if i == 0 { stamp.clone() } else { format!("{stamp}-{i}") });
        match fs::create_dir(&d) {
            Ok(()) => return Ok(d),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && i <= 99 => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

/// The width of a manual's middle page, 0 when it has none.
pub fn median_width(dir: &Path, base: &str) -> u32 {
    let prefix = format!("{base}-manual-").to_lowercase();
    let mut widths: Vec<u32> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let l = e.file_name().to_string_lossy().to_lowercase();
            l.starts_with(&prefix) && (l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png"))
        })
        .filter_map(|e| size_of(&e.path()).map(|z| z.0).filter(|w| *w > 0))
        .collect();
    widths.sort_unstable();
    widths.get(widths.len() / 2).copied().unwrap_or(0)
}

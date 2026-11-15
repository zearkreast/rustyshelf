// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/library.js, as RustyShelf has it.
//! Your magazines and strategy guides for the museum's reading corner: `<reading folder>/Magazines` and
//! `<reading folder>/Guides` (Settings → Magazines & guides; beside RustyShelf by default). Each issue or guide is a
//! PDF, a comic archive (.cbz / .cbr / .zip of page images) or a folder of page images; subfolders group them into
//! series. Covers and an archive's pages are made once and kept in `data/library`, named as the Electron app names
//! them, so the two apps share them.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha1::{Digest, Sha1};

/// The two kinds, with their folders.
pub const KINDS: [(&str, &str); 2] = [("magazine", "Magazines"), ("guide", "Guides")];
const MAX_ITEMS: usize = 2000;
const MAX_DEPTH: usize = 3;
/// the cache's size cap, and how long something just used is kept whatever its size
const CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const RECENT: Duration = Duration::from_secs(2 * 3600);
/// a PDF's cover: page 1 at most 900 pixels tall
const COVER_SIZE: u32 = 675;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Pdf,
    Archive,
    /// a folder of page images
    Dir,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    /// "magazine" or "guide"
    pub kind: &'static str,
    pub kind_of: Type,
    pub title: String,
    /// its folders under the kind's, joined with " · "
    pub series: String,
    pub path: PathBuf,
    /// last changed, in milliseconds since 1970
    pub mtime: u64,
    /// a folder's page count
    pub pages: Option<usize>,
}

/// What the reader opens: a PDF, or page images in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pages {
    Pdf(PathBuf),
    Images(Vec<PathBuf>),
}

/// The reading folder: Settings' (`ui.readingLibrary`) or beside RustyShelf.
pub fn root(reading_library: Option<&str>, app_root: &Path) -> PathBuf {
    match reading_library.filter(|d| !d.is_empty()) {
        Some(d) => std::path::absolute(crate::paths::expand(d, app_root)).unwrap_or_else(|_| PathBuf::from(d)),
        None => app_root.to_path_buf(),
    }
}

/// Where covers and unpacked archives are kept.
pub fn cache_dir(app_root: &Path) -> PathBuf {
    crate::paths::data_dir(app_root).join("library")
}

/// An item's id: the first 16 hex digits of the SHA-1 of its full path in lower case.
pub fn id_of(p: &Path) -> String {
    let full = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let hex: String = Sha1::digest(full.to_string_lossy().to_lowercase().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
    hex[..16].to_string()
}

/// "Nintendo_Power_Issue_042_(1992-11).pdf" → "Nintendo Power Issue 042 (1992-11)"
pub fn title_of(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(i) if (3..=5).contains(&(name.len() - i)) && name[i + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => &name[..i],
        _ => name,
    };
    stem.split(['_', '.', ' ', '\t', '\n', '\r']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

fn natural(a: &str, b: &str) -> Ordering {
    crate::collate::compare(a, b)
}

fn ext_is(name: &str, exts: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    exts.iter().any(|e| lower.ends_with(&format!(".{e}")))
}

fn is_image(name: &str) -> bool {
    ext_is(name, &["jpg", "jpeg", "png", "webp", "gif"])
}

fn is_archive(name: &str) -> bool {
    ext_is(name, &["cbz", "cbr", "zip", "cb7", "7z", "rar"])
}

fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

/// A folder's page images, in natural order, by name.
fn images_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|r| r.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_image(n)).collect())
        .unwrap_or_default();
    names.sort_by(|a, b| natural(a, b));
    names
}

/// Every issue and guide, both kinds: directory listings only. The kinds' folders are made when missing.
pub fn scan(root: &Path) -> Vec<Item> {
    let mut out = Vec::new();
    for (kind, folder) in KINDS {
        let dir = root.join(folder);
        let _ = fs::create_dir_all(&dir);
        walk(&dir, 0, "", kind, &mut out);
    }
    out
}

fn walk(dir: &Path, depth: usize, series: &str, kind: &'static str, out: &mut Vec<Item>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<(String, PathBuf)> = rd.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect();
    entries.sort_by(|a, b| natural(&a.0, &b.0));
    for (name, full) in entries {
        if out.len() >= MAX_ITEMS {
            return;
        }
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = fs::metadata(&full) else { continue };
        if meta.is_dir() {
            // a folder of page images is one issue; any other folder is a series (walked into)
            let imgs = images_in(&full);
            if imgs.len() >= 2 {
                out.push(Item { id: id_of(&full), kind, kind_of: Type::Dir, title: title_of(&name), series: series.to_string(), path: full, mtime: mtime_ms(&meta), pages: Some(imgs.len()) });
            } else if depth < MAX_DEPTH {
                let inner = if series.is_empty() { title_of(&name) } else { format!("{series} · {}", title_of(&name)) };
                walk(&full, depth + 1, &inner, kind, out);
            }
        } else if ext_is(&name, &["pdf"]) || is_archive(&name) {
            let kind_of = if ext_is(&name, &["pdf"]) { Type::Pdf } else { Type::Archive };
            out.push(Item { id: id_of(&full), kind, kind_of, title: title_of(&name), series: series.to_string(), path: full, mtime: mtime_ms(&meta), pages: None });
        }
    }
}

/// An archive entry worth taking out: a page image, never a path that climbs out of the folder or one tar would read
/// as an option.
fn safe_image(n: &str) -> bool {
    let climbs = n.split(['/', '\\']).any(|p| p == "..");
    let drive = n.len() > 1 && n.as_bytes()[1] == b':' && n.as_bytes()[0].is_ascii_alphabetic();
    is_image(n) && !n.starts_with(['-', '@', '/', '\\']) && !climbs && !drive && !n.to_ascii_lowercase().contains("__macosx")
}

// ---------- archives ----------

/// A zip's entries: name, compression method, compressed size, and where its local header is.
fn zip_entries(data: &[u8]) -> Option<Vec<(String, u16, usize, usize)>> {
    let u16_at = |i: usize| -> Option<u16> { Some(u16::from_le_bytes(data.get(i..i + 2)?.try_into().ok()?)) };
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(data.get(i..i + 4)?.try_into().ok()?)) };
    let from = data.len().saturating_sub(22 + 65_535);
    let end = (from..data.len().saturating_sub(21)).rev().find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let (count, mut at) = (u16_at(end + 10)? as usize, u32_at(end + 16)? as usize);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let (method, size) = (u16_at(at + 10)?, u32_at(at + 20)? as usize);
        let (name_len, extra, comment) = (u16_at(at + 28)? as usize, u16_at(at + 30)? as usize, u16_at(at + 32)? as usize);
        let local = u32_at(at + 42)? as usize;
        let name = String::from_utf8_lossy(data.get(at + 46..at + 46 + name_len)?).into_owned();
        out.push((name, method, size, local));
        at += 46 + name_len + extra + comment;
    }
    Some(out)
}

/// One zip entry's bytes: stored or deflated.
fn zip_read(data: &[u8], method: u16, size: usize, local: usize) -> Option<Vec<u8>> {
    let u16_at = |i: usize| -> Option<u16> { Some(u16::from_le_bytes(data.get(i..i + 2)?.try_into().ok()?)) };
    let start = local + 30 + u16_at(local + 26)? as usize + u16_at(local + 28)? as usize;
    let raw = data.get(start..start + size)?;
    match method {
        0 => Some(raw.to_vec()),
        8 => miniz_oxide::inflate::decompress_to_vec(raw).ok(),
        _ => None,
    }
}

fn is_zip(path: &Path) -> bool {
    let mut magic = [0u8; 2];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && &magic == b"PK"
}

/// tar reads zip, rar and 7z on Windows; on Linux GNU tar reads none of them, bsdtar all.
fn tar() -> &'static str {
    if cfg!(target_os = "linux") {
        "bsdtar"
    } else {
        "tar"
    }
}

fn run_tar(args: &[&std::ffi::OsStr]) -> Result<String, String> {
    let mut cmd = std::process::Command::new(tar());
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().map_err(|e| format!("{}: {e}", tar()))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// An archive's page images, by name inside it.
fn archive_names(path: &Path) -> Result<Vec<String>, String> {
    if is_zip(path) {
        let data = fs::read(path).map_err(|e| e.to_string())?;
        return zip_entries(&data).map(|e| e.into_iter().map(|e| e.0).filter(|n| safe_image(n)).collect()).ok_or_else(|| "not a zip that can be read".to_string());
    }
    Ok(run_tar(&["-tf".as_ref(), path.as_os_str()])?.lines().map(str::trim).filter(|n| safe_image(n)).map(String::from).collect())
}

/// Takes `names` out of the archive into `out`, keeping their folders.
fn extract(path: &Path, out: &Path, names: &[String]) -> Result<(), String> {
    if is_zip(path) {
        let data = fs::read(path).map_err(|e| e.to_string())?;
        let wanted: HashSet<&str> = names.iter().map(String::as_str).collect();
        for (name, method, size, local) in zip_entries(&data).ok_or("not a zip that can be read")? {
            if !wanted.contains(name.as_str()) || !safe_image(&name) {
                continue;
            }
            let Some(bytes) = zip_read(&data, method, size, local) else { continue };
            let to = out.join(name.replace('\\', "/"));
            if let Some(dir) = to.parent() {
                fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            fs::write(&to, bytes).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    // many names don't fit Windows' command line: they go to tar in a list file
    let list = out.join(".names.txt");
    fs::write(&list, names.join("\n") + "\n").map_err(|e| e.to_string())?;
    let r = run_tar(&["-xf".as_ref(), path.as_os_str(), "-C".as_ref(), out.as_os_str(), "-T".as_ref(), list.as_os_str()]);
    let _ = fs::remove_file(&list);
    r.map(|_| ())
}

/// Every page image under a folder, in natural order of their paths there.
fn images_under(dir: &Path) -> Vec<PathBuf> {
    fn walk(d: &Path, depth: usize, found: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() && depth < 4 {
                walk(&p, depth + 1, found);
            } else if t.is_file() && is_image(&e.file_name().to_string_lossy()) {
                found.push(p);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, 0, &mut found);
    let rel = |p: &Path| p.strip_prefix(dir).unwrap_or(p).to_string_lossy().into_owned();
    found.sort_by(|a, b| natural(&rel(a), &rel(b)));
    found
}

fn touch(p: &Path) {
    let _ = fs::File::options().write(true).open(p).and_then(|f| f.set_modified(SystemTime::now()));
    if p.is_dir() {
        let _ = fs::File::open(p).and_then(|f| f.set_modified(SystemTime::now()));
    }
}

/// An archive unpacked once into `<cache>/<id>-<mtime>/` (its older unpacks go).
fn unpack(it: &Item, cache: &Path) -> Result<PathBuf, String> {
    let out = cache.join(format!("{}-{}", it.id, it.mtime));
    let done = out.join(".done");
    if done.exists() {
        touch(&out);
        return Ok(out);
    }
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let names = archive_names(&it.path)?;
    if names.is_empty() {
        return Err("No page images inside.".into());
    }
    extract(&it.path, &out, &names[..names.len().min(1500)])?;
    fs::write(&done, "").map_err(|e| e.to_string())?;
    if let Ok(rd) = fs::read_dir(cache) {
        let mine = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&format!("{}-", it.id)) && n != mine {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(out)
}

/// What the reader opens for an item.
pub fn pages(it: &Item, cache: &Path) -> Result<Pages, String> {
    match it.kind_of {
        Type::Pdf => Ok(Pages::Pdf(it.path.clone())),
        Type::Dir => Ok(Pages::Images(images_in(&it.path).into_iter().map(|n| it.path.join(n)).collect())),
        Type::Archive => Ok(Pages::Images(images_under(&unpack(it, cache)?))),
    }
}

/// An item's cover (its first page) or, with `back`, its back cover (its last): a folder's own image, a page taken out
/// of an archive into `<cache>/covers/<id>-<mtime>[-back]/`, or a PDF's page drawn as `<cache>/covers/<id>-<mtime>[-back].jpg`.
pub fn cover(it: &Item, cache: &Path, back: bool) -> Result<Option<PathBuf>, String> {
    let tail = if back { "-back" } else { "" };
    match it.kind_of {
        Type::Dir => {
            let l = images_in(&it.path);
            Ok(if back { l.last() } else { l.first() }.map(|n| it.path.join(n)))
        }
        Type::Archive => {
            // already unpacked (it was read): from there; otherwise just the one page
            if cache.join(format!("{}-{}", it.id, it.mtime)).join(".done").exists() {
                let Pages::Images(files) = pages(it, cache)? else { return Ok(None) };
                return Ok(if back { files.last() } else { files.first() }.cloned());
            }
            let dir = cache.join("covers").join(format!("{}-{}{tail}", it.id, it.mtime));
            if dir.join(".done").exists() {
                touch(&dir);
                return Ok(images_under(&dir).into_iter().next());
            }
            let mut names = archive_names(&it.path)?;
            names.sort_by(|a, b| natural(&a.replace('\\', "/"), &b.replace('\\', "/")));
            let Some(pick) = (if back { names.last() } else { names.first() }).cloned() else { return Ok(None) };
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            extract(&it.path, &dir, &[pick])?;
            fs::write(dir.join(".done"), "").map_err(|e| e.to_string())?;
            Ok(images_under(&dir).into_iter().next())
        }
        Type::Pdf => {
            let out = cache.join("covers").join(format!("{}-{}{tail}.jpg", it.id, it.mtime));
            if out.exists() {
                touch(&out);
                return Ok(Some(out));
            }
            let pdf = fs::read(&it.path).map_err(|e| e.to_string())?;
            let Some(jpeg) = crate::pdfpages::pages(&pdf, COVER_SIZE, 1, back)?.into_iter().next() else { return Ok(None) };
            fs::create_dir_all(out.parent().unwrap_or(cache)).map_err(|e| e.to_string())?;
            // written aside and renamed: a cut-off write would otherwise stay as the cover
            let tmp = out.with_extension(format!("{}.tmp", std::process::id()));
            fs::write(&tmp, jpeg).map_err(|e| e.to_string())?;
            fs::rename(&tmp, &out).map_err(|e| {
                let _ = fs::remove_file(&tmp);
                e.to_string()
            })?;
            Ok(Some(out))
        }
    }
}

fn size_of(p: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(p) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(p).map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum()).unwrap_or(0)
}

/// Keeps the cache in bounds: the covers and unpacked archives of items no longer listed go (when `listed` is a full
/// listing that found something), then the least recently used until it's under its cap. Anything used in the last
/// two hours stays.
pub fn prune(cache: &Path, listed: Option<&[Item]>) {
    let ids: Option<HashSet<&str>> = listed.filter(|l| !l.is_empty() && l.len() < MAX_ITEMS).map(|l| l.iter().map(|i| i.id.as_str()).collect());
    let cached = |n: &str| {
        let core = n.strip_suffix(".jpg").unwrap_or(n);
        let core = core.strip_suffix("-back").unwrap_or(core);
        let (id, stamp) = core.split_at(core.len().min(16));
        id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) && stamp.len() > 1 && stamp.starts_with('-') && stamp[1..].bytes().all(|b| b.is_ascii_digit())
    };
    let now = SystemTime::now();
    let mut entries = Vec::new();
    for dir in [cache.to_path_buf(), cache.join("covers")] {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !cached(&n) {
                continue;
            }
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(now);
            entries.push((e.path(), n[..16].to_string(), t));
        }
    }
    let recent = |t: SystemTime| now.duration_since(t).unwrap_or_default() < RECENT;
    let mut keep = Vec::new();
    for (p, id, t) in entries {
        if ids.as_ref().is_some_and(|ids| !ids.contains(id.as_str())) && !recent(t) {
            let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        } else {
            keep.push((size_of(&p), p, t));
        }
    }
    let mut total: u64 = keep.iter().map(|k| k.0).sum();
    keep.sort_by_key(|k| k.2);
    for (size, p, t) in keep {
        if total <= CAP_BYTES {
            break;
        }
        if recent(t) {
            continue;
        }
        if (if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) }).is_ok() {
            total -= size;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip of the given files, stored.
    fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let local = out.len() as u32;
            let crc = crc32fast::hash(data);
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend(crc.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(*data);
            central.extend(0x0201_4b50u32.to_le_bytes());
            central.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend(crc.to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend(local.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let (at, n) = (out.len() as u32, files.len() as u16);
        out.extend(&central);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0, 0, 0, 0]);
        out.extend(n.to_le_bytes());
        out.extend(n.to_le_bytes());
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(at.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn titles_lose_their_underscores_and_extension() {
        assert_eq!(title_of("Nintendo_Power_Issue_042_(1992-11).pdf"), "Nintendo Power Issue 042 (1992-11)");
        assert_eq!(title_of("Guide.v2"), "Guide");
        assert_eq!(title_of("Vol.1 part.three.cbz"), "Vol 1 part three");
        assert_eq!(title_of("Pages"), "Pages");
    }

    #[test]
    fn a_reading_folder_is_scanned_into_series_and_issues() {
        let t = tempfile::tempdir().unwrap();
        let m = t.path().join("Magazines");
        fs::create_dir_all(m.join("Nintendo Power").join("Issue 10 scans")).unwrap();
        fs::write(m.join("Nintendo Power").join("Issue 2.pdf"), b"%PDF").unwrap();
        fs::write(m.join("Nintendo Power").join("Issue 10.cbz"), zip(&[("p1.png", b"a")])).unwrap();
        for p in ["01.jpg", "02.jpg"] {
            fs::write(m.join("Nintendo Power").join("Issue 10 scans").join(p), b"x").unwrap();
        }
        fs::write(m.join("notes.txt"), b"").unwrap();
        let items = scan(t.path());
        let got: Vec<(&str, &str, Type)> = items.iter().map(|i| (i.series.as_str(), i.title.as_str(), i.kind_of)).collect();
        assert_eq!(got, [("Nintendo Power", "Issue 2", Type::Pdf), ("Nintendo Power", "Issue 10 scans", Type::Dir), ("Nintendo Power", "Issue 10", Type::Archive)]);
        assert!(t.path().join("Guides").is_dir());
        assert_eq!(items[1].pages, Some(2));
    }

    #[test]
    fn a_comic_archive_opens_into_its_pages_and_cover() {
        let t = tempfile::tempdir().unwrap();
        let g = t.path().join("Guides");
        fs::create_dir_all(&g).unwrap();
        fs::write(g.join("Book.cbz"), zip(&[("b/02.png", b"two"), ("b/01.png", b"one"), ("../evil.png", b"no"), ("readme.txt", b"")])).unwrap();
        let cache = t.path().join("cache");
        let it = scan(t.path()).remove(0);
        let front = cover(&it, &cache, false).unwrap().unwrap();
        assert_eq!(fs::read(front).unwrap(), b"one");
        let back = cover(&it, &cache, true).unwrap().unwrap();
        assert_eq!(fs::read(back).unwrap(), b"two");
        let Pages::Images(files) = pages(&it, &cache).unwrap() else { panic!() };
        assert_eq!(files.iter().map(|f| fs::read(f).unwrap()).collect::<Vec<_>>(), [b"one".to_vec(), b"two".to_vec()]);
        assert!(!cache.join("evil.png").exists());
        // the unpacked pages are kept while the item is still listed
        prune(&cache, Some(std::slice::from_ref(&it)));
        assert!(cache.join(format!("{}-{}", it.id, it.mtime)).join(".done").exists());
    }
}

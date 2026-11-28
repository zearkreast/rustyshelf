// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/pccovers.js (cover, autoCover, fetchAuto, fillMissing, folderCover, consoleCover, consolePlat,
// options, use, reset), as RustyShelf has it. The back of the box (back, own_back, reset_back) is from
// ZakAuMiel/boxshelf-source (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf: its
// main/pccovers.js.
//! A PC game's cover files in the data folder, and a picture found in the game's own folder.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::js;
use crate::paths::is_file;
use crate::scanner::read_dir_sorted;

/// A PC game's id as a file name.
pub fn safe_id(id: &str) -> String {
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9._-]+").unwrap());
    js::slice(&UNSAFE.replace_all(&id.to_lowercase(), "-"), 120).to_string()
}

fn picture(dir: PathBuf, id: &str) -> Option<PathBuf> {
    let b = dir.join(safe_id(id));
    ["jpg", "png", "webp"].iter().map(|e| PathBuf::from(format!("{}.{e}", b.display()))).find(|f| is_file(f))
}

/// The cover the player chose for a game (`data/pc-covers`): it wins over every automatic one.
pub fn cover(data: &Path, id: &str) -> Option<PathBuf> {
    picture(data.join("pc-covers"), id)
}

/// Makes a picture of the player's own the cover chosen for a game: a JPG, PNG or WebP of 25 MB at most, copied
/// into `data/pc-covers` in place of any cover chosen before.
pub fn own(data: &Path, id: &str, file: &Path) -> Result<PathBuf, String> {
    let ext = file.extension().map_or(String::new(), |e| e.to_string_lossy().to_lowercase().replace("jpeg", "jpg"));
    if !["jpg", "png", "webp"].contains(&ext.as_str()) {
        return Err("Pick a JPG, PNG or WebP picture.".into());
    }
    if file.metadata().map_err(|e| e.to_string())?.len() > 25_000_000 {
        return Err("That picture is too big (over 25 MB).".into());
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    keep_as(data, id, &bytes, &ext)
}

/// Takes away the cover chosen for a game: its automatic one is shown again.
pub fn clear(data: &Path, id: &str) {
    clear_except(data, id, Path::new(""));
}

/// [`clear`], sparing `kept`.
fn clear_except(data: &Path, id: &str, kept: &Path) {
    let base = data.join("pc-covers").join(safe_id(id)).display().to_string();
    for gone in ["jpg", "png", "webp", "plat", "from"] {
        let f = PathBuf::from(format!("{base}.{gone}"));
        if f != kept {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// A picture's kind by its first bytes, else by the address it came from.
fn ext_of(url: &str, b: &[u8]) -> String {
    static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(png|webp|jpe?g)(?:$|\?)").unwrap());
    if b.starts_with(&[0x89, b'P']) {
        "png".into()
    } else if b.get(8..12) == Some(b"WEBP") {
        "webp".into()
    } else {
        NAMED.captures(url).map_or("jpg".into(), |c| c[1].to_lowercase().replace("jpeg", "jpg"))
    }
}

/// A cover offered for a game.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverOption {
    pub label: String,
    pub sub: String,
    /// "yours", "steam", "steamstore", "console" or "steamgriddb"
    pub source: &'static str,
    /// a small copy to show, on disk
    pub thumb: PathBuf,
    take: Take,
}

#[derive(Debug, Clone, PartialEq)]
enum Take {
    File(PathBuf),
    /// the sharpest first
    Urls(Vec<String>),
    Box(ConsoleBox),
}

/// A console release of a PC game's title, with its box's pictures in LaunchBox's image store.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleBox {
    /// "ps5", "xsx", "ps4", "xone" or "sw"
    pub plat: String,
    /// LaunchBox's name for the console
    pub platform: String,
    pub name: String,
    pub url: String,
    pub back: String,
    pub spine: String,
}

/// The boxes of a title's console releases, newest console first, from the LaunchBox index already built (never
/// downloaded for this).
pub fn console_boxes(data: &Path, title: &str, region: &str) -> Vec<ConsoleBox> {
    use crate::covers;
    let Some(mut idx) = crate::boxart::ready_index(&crate::paths::boxart_cache(data)).filter(|_| !title.is_empty()) else { return Vec::new() };
    let mut out = Vec::new();
    for (plat, platform) in covers::pc_box_platforms() {
        let Some(shard) = idx.platform(platform) else { continue };
        let names: Vec<&str> = shard.games.iter().map(|g| g.n.as_str()).collect();
        let fronts = |i: usize| shard.images.get(&shard.games[i].id).map_or(0, |l| l.iter().filter(|im| im.r#type == "Box - Front").count());
        let Some(hit) = covers::match_name(&names, title, &fronts).map(|i| &shard.games[i]) else { continue };
        let all = shard.images.get(&hit.id).map_or(&[][..], Vec::as_slice);
        let first = |kind: &str| covers::pick_images(all, kind, region).first().map(|im| covers::lb_url(&im.file));
        let Some(front) = first("Box - Front") else { continue };
        out.push(ConsoleBox { plat: plat.clone(), platform: platform.clone(), name: hit.name.clone(), url: front, back: first("Box - Back").unwrap_or_default(), spine: first("Box - Spine").unwrap_or_default() });
    }
    out
}

/// Keeps one part of a game's console box ("" the front, "back" or "spine") in `data/pc-console-covers`; the front
/// with where it came from and its console beside it.
pub fn save_console(data: &Path, id: &str, bytes: &[u8], from: &str, part: &str, plat: &str) -> Result<PathBuf, String> {
    let dir = data.join("pc-console-covers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = dir.join(safe_id(id)).display().to_string();
    let dash = if part.is_empty() { "" } else { "-" };
    let f = PathBuf::from(format!("{base}{dash}{part}.jpg"));
    crate::files::write_atomic(&f, bytes).map_err(|e| e.to_string())?;
    if part.is_empty() {
        std::fs::write(format!("{base}.from"), from).map_err(|e| e.to_string())?;
        if !plat.is_empty() {
            std::fs::write(format!("{base}.plat"), plat).map_err(|e| e.to_string())?;
        }
    }
    Ok(f)
}

/// A console box part with its console's band at the top painted over with a PC one, as a JPEG.
fn rebranded(bytes: &[u8], plat: &str, part: &str) -> Option<Vec<u8>> {
    let img = rustyshelf_imgtools::rebrand::rebrand(&image::load_from_memory(bytes).ok()?, plat, part)?;
    crate::art::jpeg(&img.into_rgb8(), 92)
}

pub(crate) fn download(url: &str) -> Option<Vec<u8>> {
    crate::net::get_picture(url, std::time::Duration::from_secs(15), 25_000_000)
}

/// Each of these PC games (id, title) without a box or a chosen cover in the box of its newest console release.
/// `go_on` is asked before each game. Returns how many were made.
pub fn make_console_boxes(data: &Path, games: &[(&str, &str)], region: &str, go_on: &dyn Fn() -> bool) -> usize {
    // the games with no console release, looked up once per run
    static NONE: LazyLock<std::sync::Mutex<std::collections::HashSet<String>>> = LazyLock::new(Default::default);
    let mut made = 0;
    for (id, title) in games {
        let none = NONE.lock().is_ok_and(|n| n.contains(*id));
        if id.is_empty() || title.is_empty() || none || cover(data, id).is_some() || console_cover(data, id, "").is_some() {
            continue;
        }
        if !go_on() {
            break;
        }
        let Some(b) = console_boxes(data, title, region).into_iter().next() else {
            if let Ok(mut n) = NONE.lock() {
                n.insert(id.to_string());
            }
            continue;
        };
        // the front last: it's what says the game is done
        for (part, url) in [("back", &b.back), ("spine", &b.spine), ("", &b.url)] {
            let Some(pic) = Some(url).filter(|u| !u.is_empty()).and_then(|u| download(u)).and_then(|raw| rebranded(&raw, &b.plat, part)) else { continue };
            if save_console(data, id, &pic, &format!("{} {}", b.platform, b.url), part, &b.plat).is_ok() && part.is_empty() {
                made += 1;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    made
}

/// The covers offered for a game, and whether there's a SteamGridDB key (posters come only with one).
#[derive(Debug, Default)]
pub struct Options {
    pub list: Vec<CoverOption>,
    pub steamgriddb: bool,
    pub chosen: bool,
}

/// A thumbnail fetched once into `data/pc-cover-cache` (a picture only, through a temporary name), by its address.
fn thumb(data: &Path, url: &str) -> Option<PathBuf> {
    use sha1::Digest;
    let dir = data.join("pc-cover-cache");
    let name = crate::files::hex(&sha1::Sha1::digest(url.as_bytes())[..12]);
    let base = dir.join(&name).display().to_string();
    if let Some(had) = picture(dir.clone(), &name) {
        return Some(had);
    }
    let b = crate::net::get_picture(url, std::time::Duration::from_secs(10), 15_000_000)?;
    std::fs::create_dir_all(&dir).ok()?;
    let dest = PathBuf::from(format!("{base}.{}", ext_of(url, &b)));
    crate::files::write_atomic(&dest, &b).ok()?;
    Some(dest)
}

/// The first of these addresses whose thumbnail could be had, for each, fetched a few at a time.
fn thumbs(data: &Path, wanted: Vec<Vec<String>>) -> Vec<Option<PathBuf>> {
    crate::pool::map(&wanted, 4, |urls| urls.iter().find_map(|u| thumb(data, u)))
}

/// The covers offered for a game (`id`; a Steam game's `appid`; its `title`, or a title typed in when `term`):
/// the one chosen now, Steam's own (a Steam game's, else the store's games of that name), and up to 24 SteamGridDB
/// posters with a key. Their thumbnails are fetched first, so this takes a while: off the frame.
pub fn options(data: &Path, id: &str, appid: Option<&str>, title: &str, term: bool, region: &str, sg: Option<&crate::steamgrid::SteamGrid>) -> Options {
    let mut list = Vec::new();
    let mine = cover(data, id);
    if let Some(f) = &mine {
        list.push(CoverOption { label: "Your choice now".into(), sub: "Chosen before".into(), source: "yours", thumb: f.clone(), take: Take::File(f.clone()) });
    }
    let appid = appid.filter(|_| !term);
    let local = appid.map(|a| data.join("steam-art").join(format!("{a}_cover.jpg"))).filter(|f| is_file(f));
    if let Some(f) = &local {
        list.push(CoverOption { label: "Steam's cover".into(), sub: "From your Steam library".into(), source: "steam", thumb: f.clone(), take: Take::File(f.clone()) });
    }
    // Steam's store cover: a Steam game's own when it isn't kept here, else the Steam games of that name
    let apps: Vec<(String, String)> = match appid {
        Some(a) if local.is_none() => vec![(a.to_string(), String::new())],
        Some(_) => Vec::new(),
        None => crate::pcgames::steam_search(title, !term),
    };
    let boxes = console_boxes(data, title, region);
    let posters = sg.filter(|g| g.enabled()).map(|g| g.posters(&crate::steamgrid::Game { appid: appid.unwrap_or(""), title, names: &[title.to_string()] })).unwrap_or_default();
    let posters: Vec<_> = posters.into_iter().take(24).collect();
    let wanted: Vec<Vec<String>> = apps
        .iter()
        .map(|(a, _)| crate::steamart::asset(a, "library_600x900.jpg")[..2].to_vec())
        .chain(boxes.iter().map(|b| vec![b.url.clone()]))
        .chain(posters.iter().map(|p| vec![p.thumb.clone().unwrap_or_else(|| p.url.clone())]))
        .collect();
    let got = thumbs(data, wanted);
    for ((a, name), t) in apps.iter().zip(&got) {
        let Some(t) = t else { continue };
        let urls = [crate::steamart::asset(a, "library_600x900_2x.jpg"), crate::steamart::asset(a, "library_600x900.jpg")].concat();
        let sub = if name.is_empty() { "From Steam's store".to_string() } else { format!("{name} on Steam") };
        list.push(CoverOption { label: "Steam's cover".into(), sub, source: "steamstore", thumb: t.clone(), take: Take::Urls(urls) });
    }
    for (b, t) in boxes.iter().zip(&got[apps.len()..]) {
        let Some(t) = t else { continue };
        list.push(CoverOption { label: format!("{} box", b.platform), sub: "Physical console release".into(), source: "console", thumb: t.clone(), take: Take::Box(b.clone()) });
    }
    for (p, t) in posters.iter().zip(&got[apps.len() + boxes.len()..]) {
        let Some(t) = t else { continue };
        let label = if p.style.is_empty() { "SteamGridDB".to_string() } else { format!("SteamGridDB · {}", p.style_name()) };
        let sub = if p.author.is_empty() { String::new() } else { format!("by {}", p.author) };
        list.push(CoverOption { label, sub, source: "steamgriddb", thumb: t.clone(), take: Take::Urls(vec![p.url.clone()]) });
    }
    Options { list, steamgriddb: sg.is_some_and(|g| g.enabled()), chosen: mine.is_some() }
}

/// Makes one of the offered covers the game's chosen one: downloaded (the sharpest that comes) and kept in
/// `data/pc-covers`. A console box brings its back and spine, and the case of its console.
pub fn choose(data: &Path, id: &str, option: &CoverOption) -> Result<PathBuf, String> {
    const FAILED: &str = "Couldn't download that cover. Check the connection and try again.";
    let (bytes, from) = match &option.take {
        Take::Box(b) => {
            let front = download(&b.url).ok_or(FAILED)?;
            let dest = keep(data, id, &front, &b.url)?;
            let base = data.join("pc-covers").join(safe_id(id)).display().to_string();
            let from = format!("{} {}", b.platform, b.url);
            std::fs::write(format!("{base}.plat"), &b.plat).and_then(|()| std::fs::write(format!("{base}.from"), &from)).map_err(|e| e.to_string())?;
            if save_console(data, id, &front, &from, "", &b.plat).is_ok() {
                for (part, url) in [("back", &b.back), ("spine", &b.spine)] {
                    let Some(raw) = Some(url).filter(|u| !u.is_empty()).and_then(|u| download(u)) else { continue };
                    let pic = if part == "spine" { rebranded(&raw, &b.plat, part).unwrap_or(raw) } else { raw };
                    let _ = save_console(data, id, &pic, &format!("{} {url}", b.platform), part, &b.plat);
                }
            }
            return Ok(dest);
        }
        Take::File(f) => (std::fs::read(f).map_err(|e| e.to_string())?, f.display().to_string()),
        Take::Urls(urls) => urls
            .iter()
            .find_map(|u| download(u).map(|b| (b, u.clone())))
            .ok_or(FAILED)?,
    };
    keep(data, id, &bytes, &from)
}

/// Keeps a picture as the game's chosen cover in place of any chosen before.
pub(crate) fn keep(data: &Path, id: &str, bytes: &[u8], from: &str) -> Result<PathBuf, String> {
    keep_as(data, id, bytes, &ext_of(from, bytes))
}

/// [`keep`], as `<id>.<ext>`.
fn keep_as(data: &Path, id: &str, bytes: &[u8], ext: &str) -> Result<PathBuf, String> {
    let dir = data.join("pc-covers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = dir.join(safe_id(id)).display().to_string();
    let dest = PathBuf::from(format!("{base}.{ext}"));
    crate::files::write_atomic(&dest, bytes).map_err(|e| e.to_string())?;
    clear_except(data, id, &dest);
    Ok(dest)
}

/// SteamGridDB's top poster for a game that downloads, made its chosen cover. None when there's none.
pub fn use_best(data: &Path, id: &str, title: &str, sg: &crate::steamgrid::SteamGrid) -> Option<PathBuf> {
    let names = [title.to_string()];
    let posters = sg.posters(&crate::steamgrid::Game { appid: "", title, names: &names });
    posters.iter().take(3).find_map(|p| download(&p.url).and_then(|b| keep(data, id, &b, &p.url).ok()))
}

fn back_id(id: &str) -> String {
    format!("{id}.back")
}

/// The back of the box the player chose for a game (`data/pc-covers/<id>.back.<ext>`, as Zak's fork keeps it).
pub fn back(data: &Path, id: &str) -> Option<PathBuf> {
    cover(data, &back_id(id))
}

/// A part of the case of a game on a console's shelf, put in by the Art Studio (`data/pc-covers/<id>.<part>.<ext>`):
/// "back", "spine" or "disc".
pub fn part(data: &Path, id: &str, part: &str) -> Option<PathBuf> {
    cover(data, &format!("{id}.{part}"))
}

/// The manual of a game on a console's shelf (`data/pc-covers/<id>-manual-01.jpg` …): its pages in order, else its
/// PDF.
pub fn manual(data: &Path, id: &str) -> (Vec<PathBuf>, Option<PathBuf>) {
    let dir = data.join("pc-covers");
    let (pdf, mut pages): (Vec<String>, Vec<String>) = crate::manualcoll::manual_files(&dir, &safe_id(id)).into_iter().partition(|f| f.to_lowercase().ends_with(".pdf"));
    pages.sort_by(|a, b| crate::collate::compare(a, b));
    let pdf = pdf.first().filter(|_| pages.is_empty()).map(|f| dir.join(f));
    (pages.into_iter().map(|f| dir.join(f)).collect(), pdf)
}

/// Makes a picture of the player's own the back of a game's box, as `own` does a cover.
pub fn own_back(data: &Path, id: &str, file: &Path) -> Result<PathBuf, String> {
    own(data, &back_id(id), file)
}

/// LaunchBox's back of a game's box, downloaded (`data/pc-covers-auto/<id>.back.<ext>`).
pub fn auto_back(data: &Path, id: &str) -> Option<PathBuf> {
    auto_cover(data, &back_id(id))
}

/// Downloads LaunchBox's back of a game's box, as `fetch_auto` does a cover.
pub fn fetch_auto_back(data: &Path, id: &str, url: &str) -> Option<PathBuf> {
    fetch_auto(data, &back_id(id), url)
}

fn made_up_marker(data: &Path, id: &str) -> PathBuf {
    data.join("pc-covers").join(format!("{}.made", safe_id(&back_id(id))))
}

/// "Made-up back" was chosen for a game: LaunchBox's back isn't put on it by itself.
pub fn made_up_back(data: &Path, id: &str) -> bool {
    is_file(&made_up_marker(data, id))
}

fn remove_pictures(dir: &Path, id: &str) {
    for e in ["jpg", "png", "webp"] {
        let _ = std::fs::remove_file(dir.join(format!("{}.{e}", safe_id(id))));
    }
}

/// "Made-up back": the player's back and LaunchBox's go, and LaunchBox's stays off.
pub fn reset_back(data: &Path, id: &str) -> Result<(), String> {
    remove_pictures(&data.join("pc-covers"), &back_id(id));
    remove_pictures(&data.join("pc-covers-auto"), &back_id(id));
    let _ = std::fs::remove_file(data.join("pc-covers-auto").join(format!("{}.from", safe_id(&back_id(id)))));
    std::fs::create_dir_all(data.join("pc-covers")).and_then(|()| std::fs::write(made_up_marker(data, id), "")).map_err(|e| e.to_string())
}

/// LaunchBox's back asked for by hand: it goes on the case in place of the player's.
pub fn prefer_auto_back(data: &Path, id: &str) {
    remove_pictures(&data.join("pc-covers"), &back_id(id));
    let _ = std::fs::remove_file(made_up_marker(data, id));
}

/// The store's own cover of a game, downloaded once (`data/pc-covers-auto`).
pub fn auto_cover(data: &Path, id: &str) -> Option<PathBuf> {
    picture(data.join("pc-covers-auto"), id)
}

/// Downloads a cover into the automatic ones (`data/pc-covers-auto`), with the address it came from beside it:
/// a cover already there from that address is left alone. None when it couldn't be had.
pub fn fetch_auto(data: &Path, id: &str, url: &str) -> Option<PathBuf> {
    if !url.starts_with("https://") {
        return None;
    }
    let dir = data.join("pc-covers-auto");
    let base = dir.join(safe_id(id)).display().to_string();
    let from = PathBuf::from(format!("{base}.from"));
    if let Some(had) = auto_cover(data, id).filter(|_| std::fs::read_to_string(&from).is_ok_and(|f| f == url)) {
        return Some(had);
    }
    // a picture of 1 kB to 15 MB
    let b = crate::net::get_picture(url, std::time::Duration::from_secs(10), 15_000_000).filter(|b| b.len() > 1000)?;
    std::fs::create_dir_all(&dir).ok()?;
    remove_pictures(&dir, id);
    let dest = PathBuf::from(format!("{base}.{}", ext_of(url, &b)));
    crate::files::write_atomic(&dest, &b).ok()?;
    let _ = std::fs::write(&from, url);
    Some(dest)
}

/// SteamGridDB's best poster for each of these games (id, title), which have no cover at all, kept with the
/// automatic covers: a cover the player chose, the store's own or Steam's always wins. Returns the games looked
/// up and the posters that came in.
pub fn fill_missing(data: &Path, games: &[(&str, &str)], sg: &crate::steamgrid::SteamGrid) -> (usize, usize) {
    let (mut checked, mut downloaded) = (0, 0);
    if !sg.enabled() {
        return (0, 0);
    }
    let mut seen = std::collections::HashSet::new();
    for (id, title) in games {
        if id.is_empty() || title.is_empty() || !seen.insert(*id) || cover(data, id).is_some() || auto_cover(data, id).is_some() {
            continue;
        }
        checked += 1;
        let names = [title.to_string()];
        let posters = sg.posters(&crate::steamgrid::Game { appid: "", title, names: &names });
        if posters.iter().take(3).any(|p| fetch_auto(data, id, &p.url).is_some()) {
            downloaded += 1;
        }
    }
    (checked, downloaded)
}

/// A game's console box (`data/pc-console-covers`). part: "" (the front), "back" or "spine".
pub fn console_cover(data: &Path, id: &str, part: &str) -> Option<PathBuf> {
    let dash = if part.is_empty() { "" } else { "-" };
    Some(data.join("pc-console-covers").join(format!("{}{dash}{part}.jpg", safe_id(id)))).filter(|f| is_file(f))
}

/// The console a game's box comes from ("ps5", "xone" …), if the cover in use is a console box: a cover the player
/// chose only when it was picked as one (its `.plat` beside it), the automatic console box only while none is chosen.
pub fn console_plat(data: &Path, id: &str) -> Option<String> {
    // by the platform name the .from note starts with, never by letters anywhere in the address
    // (main/pccovers.js FROM_PLAT, keep in sync)
    static FROM: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"^(sony )?playstation 5(?-u:\b)", "ps5"),
            (r"^(microsoft )?xbox series x", "xsx"),
            (r"^(sony )?playstation 4(?-u:\b)", "ps4"),
            (r"^(microsoft )?xbox one(?-u:\b)", "xone"),
            (r"^nintendo switch(?-u:\b)", "sw"),
        ]
        .into_iter()
        .map(|(re, plat)| (Regex::new(re).unwrap(), plat))
        .collect()
    });
    let chosen = cover(data, id).is_some();
    let d = data.join(if chosen { "pc-covers" } else { "pc-console-covers" });
    let read = |ext: &str| Some(d.join(format!("{}.{ext}", safe_id(id)))).filter(|p| is_file(p)).and_then(|p| std::fs::read(p).ok()).map(|b| js::trim(&String::from_utf8_lossy(&b)).to_string());
    if let Some(plat) = read("plat") {
        return Some(plat).filter(|p| !p.is_empty());
    }
    if chosen {
        return None;
    }
    let from = read("from")?.to_lowercase();
    FROM.iter().find(|(re, _)| re.is_match(&from)).map(|(_, plat)| plat.to_string())
}

/// A picture in the game's own folder (or its covers, art or media folder) named cover, folder, box, front, poster,
/// artwork or default, or after the program or the folder.
pub fn folder_cover(dir_or_exe: &str) -> Option<PathBuf> {
    let meta = Path::new(dir_or_exe).metadata().ok()?;
    let (d, base) = if meta.is_file() { (js::dirname(dir_or_exe), js::stem(dir_or_exe).to_lowercase()) } else { (dir_or_exe.to_string(), String::new()) };
    let mut names: Vec<String> = ["cover", "folder", "box", "front", "poster", "artwork", "default"].iter().map(|n| n.to_string()).collect();
    for n in [base, js::basename(&d).to_lowercase()] {
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    for s in [d.clone(), js::join(&[&d, "covers"]), js::join(&[&d, "art"]), js::join(&[&d, "media"])] {
        // TODO: a picture with its extension in capitals (Cover.JPG) is never matched, as in main/pccovers.js, which
        // takes the lower-cased extension off the name as written
        let pictures: Vec<(String, String)> = read_dir_sorted(Path::new(&s))
            .into_iter()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter_map(|n| {
                let ext = js::extname(&n).to_lowercase();
                [".jpg", ".jpeg", ".png", ".webp"].contains(&ext.as_str()).then(|| (n.strip_suffix(&ext).filter(|stem| !stem.is_empty()).unwrap_or(&n).to_lowercase(), n.clone()))
            })
            .collect();
        for name in &names {
            if let Some((_, file)) = pictures.iter().find(|(stem, _)| stem == name) {
                return Some(PathBuf::from(js::join(&[&s, file])));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers() {
        assert_eq!(safe_id("steam-12 34/X"), "steam-12-34-x");
        let t = tempfile::tempdir().unwrap();
        let d = t.path();
        for f in ["pc-covers/steam-10.png", "pc-console-covers/epic-a.jpg", "pc-console-covers/epic-a-back.jpg", "game/Zeta.exe", "game/art/ZETA.png", "game/Cover.JPG", "game/readme.txt"] {
            std::fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            std::fs::write(d.join(f), b"").unwrap();
        }
        assert_eq!(cover(d, "steam-10"), Some(d.join("pc-covers/steam-10.png")));
        assert_eq!(console_cover(d, "epic-a", "back"), Some(d.join("pc-console-covers/epic-a-back.jpg")));
        assert_eq!(console_cover(d, "epic-a", "spine"), None);
        assert_eq!(console_plat(d, "epic-a"), None);
        std::fs::write(d.join("pc-console-covers/epic-a.from"), "Sony PlayStation 5: https://example.invalid/xbox one").unwrap();
        assert_eq!(console_plat(d, "epic-a").as_deref(), Some("ps5"));
        std::fs::write(d.join("pc-covers/steam-10.plat"), " xone\n").unwrap();
        assert_eq!(console_plat(d, "steam-10").as_deref(), Some("xone"));
        let pic = d.join("game/art/ZETA.png");
        assert_eq!(own_back(d, "steam-10", &pic), Ok(d.join("pc-covers/steam-10.back.png")));
        assert_eq!((cover(d, "steam-10"), back(d, "steam-10")), (Some(d.join("pc-covers/steam-10.png")), Some(d.join("pc-covers/steam-10.back.png"))));
        std::fs::create_dir_all(d.join("pc-covers-auto")).unwrap();
        std::fs::write(d.join("pc-covers-auto/steam-10.back.jpg"), b"").unwrap();
        reset_back(d, "steam-10").unwrap();
        assert_eq!((back(d, "steam-10"), auto_back(d, "steam-10"), made_up_back(d, "steam-10")), (None, None, true));
        assert!(cover(d, "steam-10").is_some());
        std::fs::write(d.join("pc-covers/steam-10.back.jpg"), b"").unwrap();
        prefer_auto_back(d, "steam-10");
        assert_eq!((back(d, "steam-10"), made_up_back(d, "steam-10")), (None, false));
        for f in ["pc-covers/steam-10.spine.webp", "pc-covers/steam-10-manual-10.jpg", "pc-covers/steam-10-manual-2.jpg", "pc-covers/steam-10-manual.pdf", "pc-covers/epic-a-manual.pdf"] {
            std::fs::write(d.join(f), b"").unwrap();
        }
        assert_eq!((part(d, "steam-10", "spine"), part(d, "steam-10", "disc")), (Some(d.join("pc-covers/steam-10.spine.webp")), None));
        assert_eq!(manual(d, "steam-10"), (vec![d.join("pc-covers/steam-10-manual-2.jpg"), d.join("pc-covers/steam-10-manual-10.jpg")], None));
        assert_eq!(manual(d, "epic-a"), (Vec::new(), Some(d.join("pc-covers/epic-a-manual.pdf"))));
        let exe = d.join("game/Zeta.exe");
        assert_eq!(folder_cover(&exe.to_string_lossy()), Some(d.join("game/art/ZETA.png")));
        assert_eq!(folder_cover(&d.join("nope").to_string_lossy()), None);
    }
}

#[cfg(test)]
mod poster_tests {
    use super::*;
    use crate::steamgrid::{Reply, SteamGrid};

    #[test]
    fn only_games_without_a_cover_are_looked_up() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("pc-covers")).unwrap();
        std::fs::write(t.path().join("pc-covers/has-one.jpg"), b"x").unwrap();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let heard = asked.clone();
        let sg = SteamGrid::with(
            t.path(),
            Box::new(move |url: &str, _| {
                heard.lock().unwrap().push(url.rsplit('/').next().unwrap().to_string());
                Ok(Reply { status: 200, retry_after: None, body: br#"{"success":true,"data":[]}"#.to_vec() })
            }),
        );
        let games = [("Has One", "Zeta Run"), ("bare", "Omega Quest"), ("bare", "Omega Quest"), ("", "Nothing"), ("untitled", "")];
        assert_eq!(fill_missing(t.path(), &games, &sg), (0, 0));
        sg.set_key("made-up-key");
        assert_eq!(fill_missing(t.path(), &games, &sg), (1, 0));
        assert_eq!(*asked.lock().unwrap(), ["Omega%20Quest"]);
        assert_eq!(fetch_auto(t.path(), "bare", "http://example.org/a.jpg"), None);
    }
}

#[cfg(test)]
mod picker_tests {
    use super::*;

    #[test]
    fn choosing_and_going_back() {
        let data = std::env::temp_dir().join(format!("rs-pccover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(data.join("steam-art")).unwrap();
        // a JPEG's first bytes are enough to be taken for one
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0];
        std::fs::write(data.join("steam-art").join("1063660_cover.jpg"), jpeg).unwrap();
        // Steam's cover kept here: offered, and no store search for a Steam game that has it
        let o = options(&data, "steam-1063660", Some("1063660"), "Bendy and the Dark Revival", false, "auto", None);
        assert_eq!(o.list.iter().map(|c| (c.source, c.label.as_str())).collect::<Vec<_>>(), [("steam", "Steam's cover")]);
        assert!(!o.chosen && !o.steamgriddb);
        let dest = choose(&data, "steam-1063660", &o.list[0]).unwrap();
        assert_eq!(cover(&data, "steam-1063660"), Some(dest.clone()));
        assert_eq!(std::fs::read(&dest).unwrap(), jpeg);
        // the pick is offered first next time, and going back takes it away
        let o = options(&data, "steam-1063660", Some("1063660"), "Bendy and the Dark Revival", false, "auto", None);
        assert_eq!(o.list[0].source, "yours");
        assert!(o.chosen);
        clear(&data, "steam-1063660");
        assert_eq!(cover(&data, "steam-1063660"), None);
        let _ = std::fs::remove_dir_all(&data);
    }

    /// A LaunchBox index of one PS5 game, as `boxart::ready_index` reads it.
    fn index_with(data: &Path, name: &str) {
        use crate::lbindex::{norm, shard_file_name, Game, GameImage, ShardFile, ShardsMeta};
        let cache = crate::paths::boxart_cache(data);
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("launchbox-index.json"), "{}").unwrap();
        let stamp = cache.join("launchbox-index.json").metadata().unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let plats: Vec<String> = crate::covers::pc_box_platforms().iter().map(|p| p.1.clone()).collect();
        std::fs::write(cache.join("launchbox-shards.json"), serde_json::to_vec(&ShardsMeta { v: 8, at: 0, plats: plats.clone(), stamp }).unwrap()).unwrap();
        let img = |kind: &str, region: &str, file: &str| GameImage { r#type: kind.into(), region: region.into(), file: file.into() };
        let game = Game { id: "7".into(), name: name.into(), n: norm(name), overview: None, release: None, year: None, dev: None, pub_: None, genres: None, esrb: None, players: None, coop: None, rating: None, wiki: None, video: None };
        let images = [("7".to_string(), vec![img("Box - Front", "Japan", "jp front.jpg"), img("Box - Front", "North America", "na front.jpg"), img("Box - Back", "North America", "na back.jpg")])].into();
        let ps5 = &plats[0];
        let shard = ShardFile { v: 8, plat: ps5.clone(), games: vec![game], images };
        std::fs::write(shard_file_name(&cache, ps5), serde_json::to_vec(&shard).unwrap()).unwrap();
    }

    #[test]
    fn console_boxes_from_the_index() {
        let data = std::env::temp_dir().join(format!("rs-pcbox-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        assert!(console_boxes(&data, "Bendy and the Dark Revival", "auto").is_empty(), "no index, no boxes");
        index_with(&data, "Bendy and the Dark Revival");
        let found = console_boxes(&data, "Bendy and the Dark Revival", "auto");
        assert_eq!(found.len(), 1);
        let b = &found[0];
        assert_eq!((b.plat.as_str(), b.name.as_str()), ("ps5", "Bendy and the Dark Revival"));
        assert_eq!((b.url.as_str(), b.back.as_str(), b.spine.as_str()), ("https://images.launchbox-app.com/na%20front.jpg", "https://images.launchbox-app.com/na%20back.jpg", ""));
        assert_eq!(console_boxes(&data, "Bendy and the Dark Revival", "jpn")[0].url, "https://images.launchbox-app.com/jp%20front.jpg");
        assert!(console_boxes(&data, "Cuphead", "auto").is_empty());

        // a box kept gives the case of its console while no cover is chosen
        save_console(&data, "custom-bendy", b"front", &format!("{} {}", b.platform, b.url), "", &b.plat).unwrap();
        save_console(&data, "custom-bendy", b"back", "", "back", &b.plat).unwrap();
        assert_eq!(console_cover(&data, "custom-bendy", "back"), Some(data.join("pc-console-covers/custom-bendy-back.jpg")));
        assert_eq!(console_plat(&data, "custom-bendy").as_deref(), Some("ps5"));
        // made already, or a cover chosen: nothing is looked up or downloaded
        std::fs::create_dir_all(data.join("pc-covers")).unwrap();
        std::fs::write(data.join("pc-covers/custom-other.png"), b"x").unwrap();
        let none = make_console_boxes(&data, &[("custom-bendy", "Bendy and the Dark Revival"), ("custom-other", "Bendy and the Dark Revival"), ("custom-cup", "Cuphead")], "auto", &|| true);
        assert_eq!(none, 0);
        let _ = std::fs::remove_dir_all(&data);
    }
}

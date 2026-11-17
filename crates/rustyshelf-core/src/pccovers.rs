// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/pccovers.js (cover, autoCover, fetchAuto, fillMissing, folderCover, consoleCover, consolePlat), as
// RustyShelf has it. The back of the box (back, own_back, reset_back) is from ZakAuMiel/boxshelf-source
// (https://github.com/ZakAuMiel/boxshelf-source, GPL-3.0), Zak's fork of Boxshelf: its main/pccovers.js.
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
    let dir = data.join("pc-covers");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = dir.join(safe_id(id)).display().to_string();
    let (dest, tmp) = (PathBuf::from(format!("{base}.{ext}")), PathBuf::from(format!("{base}.{ext}.tmp")));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    for gone in ["jpg", "png", "webp", "plat", "from"] {
        let _ = std::fs::remove_file(format!("{base}.{gone}"));
    }
    std::fs::rename(&tmp, &dest).map_err(|e| e.to_string())?;
    Ok(dest)
}

fn back_id(id: &str) -> String {
    format!("{id}.back")
}

/// The back of the box the player chose for a game (`data/pc-covers/<id>.back.<ext>`, as Zak's fork keeps it).
pub fn back(data: &Path, id: &str) -> Option<PathBuf> {
    cover(data, &back_id(id))
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
    static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\.(png|webp|jpe?g)(?:$|\?)").unwrap());
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
    let b = crate::net::get_bytes(url, std::time::Duration::from_secs(10), 15_000_000).ok()??;
    if b.len() <= 1000 || !crate::steamart::is_image(&b) {
        return None;
    }
    let ext = if b.starts_with(&[0x89, b'P']) {
        "png".to_string()
    } else if b.get(8..12) == Some(b"WEBP") {
        "webp".to_string()
    } else {
        NAMED.captures(url).map_or("jpg".to_string(), |c| c[1].to_lowercase().replace("jpeg", "jpg"))
    };
    std::fs::create_dir_all(&dir).ok()?;
    for e in ["jpg", "png", "webp"] {
        let _ = std::fs::remove_file(format!("{base}.{e}"));
    }
    let (dest, tmp) = (PathBuf::from(format!("{base}.{ext}")), PathBuf::from(format!("{base}.{ext}.tmp")));
    std::fs::write(&tmp, &b).and_then(|()| std::fs::rename(&tmp, &dest)).ok()?;
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

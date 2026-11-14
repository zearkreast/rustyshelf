// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/pccovers.js (cover, autoCover, folderCover, consoleCover, consolePlat), as RustyShelf has it.
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

/// The store's own cover of a game, downloaded once (`data/pc-covers-auto`).
pub fn auto_cover(data: &Path, id: &str) -> Option<PathBuf> {
    picture(data.join("pc-covers-auto"), id)
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
        let exe = d.join("game/Zeta.exe");
        assert_eq!(folder_cover(&exe.to_string_lossy()), Some(d.join("game/art/ZETA.png")));
        assert_eq!(folder_cover(&d.join("nope").to_string_lossy()), None);
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/gamekey.js.
//! One key per game for what's kept about it (IGDB's info, more screenshots, trailers): `<console>|<file name>`,
//! lowercased. A generic file name (default.xex, DVD.iso, EBOOT.BIN, Nintendont's "Title [ID]/game.iso") is shared by
//! many games, so for those the game's folder comes first: `<console>|<folder>/<file name>`; folders that are part of
//! a game's layout (PS3_GAME\USRDIR, a Wii U game's code, a Switch game's exefs) are stepped over to the game's own.
//! A game without a file: its title. Keep in sync with main/gamekey.js: the Electron app reads the same files.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// The file part of a game's key.
pub fn file_key(file: &str) -> String {
    static GENERIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(game|default|eboot|disc|dvd|boot|main|rom|image|iso)\s*\d*$").unwrap());
    static LAYOUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(usrdir|ps3_game|code|exefs|content|meta|sce_sys|app|files|sys)$").unwrap());
    let base = crate::js::basename(file);
    let stem = base.rfind('.').map_or(base.as_str(), |i| &base[..i]);
    if !GENERIC.is_match(stem) {
        return base.to_lowercase();
    }
    let mut d = crate::js::dirname(file);
    for _ in 0..4 {
        if !LAYOUT.is_match(&crate::js::basename(&d)) {
            break;
        }
        let up = crate::js::dirname(&d);
        if up == d {
            break;
        }
        d = up;
    }
    let dn = crate::js::basename(&d);
    format!("{}{base}", if dn.is_empty() { String::new() } else { format!("{dn}/") }).to_lowercase()
}

/// A game's key: its console and file, or its title when it has no file.
pub fn game_key(sys: &str, file: &Path, title: &str) -> String {
    let f = file.to_string_lossy();
    if f.is_empty() {
        format!("{sys}|t:{}", title.to_lowercase())
    } else {
        format!("{sys}|{}", file_key(&f))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generic_file_name_takes_its_game_s_folder() {
        assert_eq!(game_key("snes", Path::new("D:/Roms/SNES/Super Metroid.sfc"), "Super Metroid"), "snes|super metroid.sfc");
        assert_eq!(game_key("x360", Path::new("D:/X360/Halo 3/default.xex"), ""), "x360|halo 3/default.xex");
        assert_eq!(game_key("ps3", Path::new("D:/PS3/BLUS30001/PS3_GAME/USRDIR/EBOOT.BIN"), ""), "ps3|blus30001/eboot.bin");
        assert_eq!(game_key("pc", Path::new(""), "Half-Life"), "pc|t:half-life");
    }
}

// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/dolphin.js, as RustyShelf has it.
//! Dolphin's user folder and the GameCube startup files (IPL.bin) in it. Dolphin plays the GameCube startup when it
//! finds `<its user folder>\GC\<USA|EUR|JAP>\IPL.bin` and "Skip Main Menu" is off, which RustyShelf turns off per
//! launch.

use std::path::Path;

use crate::emucfg::{dolphin_user_dirs, Var};
use crate::js;
use crate::paths::{is_dir, is_file};

const REGIONS: &[&str] = &["USA", "EUR", "JAP"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ipl {
    /// region and file of each IPL.bin found
    pub found: Vec<(&'static str, String)>,
    /// where a new IPL.bin should go: the folder Dolphin uses
    pub user_dir: String,
    pub usa_folder: String,
}

/// The folders Dolphin may really be using (the last is what a new install makes), then the others an IPL.bin could
/// have been left in: Documents and AppData for a portable Dolphin, and a `User` folder without portable.txt.
fn user_dirs(exe: &str, var: Var) -> (Vec<String>, Vec<String>) {
    let used = dolphin_user_dirs(exe, var);
    let mut all = used.clone();
    all.extend(dolphin_user_dirs("", var));
    let beside = js::join(&[&js::dirname(exe), "User"]);
    if !exe.is_empty() && is_dir(Path::new(&beside)) {
        all.push(beside);
    }
    let mut seen = std::collections::HashSet::new();
    all.retain(|d| seen.insert(d.clone()));
    (all, used)
}

pub fn find_ipl(exe: &str, var: Var) -> Ipl {
    let (all, used) = user_dirs(exe, var);
    let mut found = Vec::new();
    for d in &all {
        for r in REGIONS {
            let f = js::join(&[d, "GC", r, "IPL.bin"]);
            if is_file(Path::new(&f)) {
                found.push((*r, f));
            }
        }
    }
    // portable, else the first that exists, else where a fresh install will put it
    let user_dir = used.iter().find(|d| is_dir(Path::new(d))).or(used.last()).cloned().unwrap_or_default();
    Ipl { found, usa_folder: js::join(&[&user_dir, "GC", "USA"]), user_dir }
}

/// Makes the folder an IPL.bin of this region goes in, and gives it.
pub fn ensure_ipl_folder(exe: &str, region: &str, var: Var) -> std::io::Result<String> {
    let region = REGIONS.iter().find(|r| **r == region).unwrap_or(&"USA");
    let dir = js::join(&[&find_ipl(exe, var).user_dir, "GC", region]);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

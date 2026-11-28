// Ported from Spadfinn's boxshelf-source (https://github.com/Spadfinn/boxshelf-source, GPL-3.0), the flatpak-games-pr
// branch's flatpakGames in main/pcgames.js.
//! (Linux) Installed Flatpak games, for the PC shelf (Settings → PC → Flatpak games): every installed Flatpak whose
//! launcher says it's a game (`Categories` has Game), minus emulators (their games have shelves of their own) and game
//! launchers and stores (Lutris, Heroic: `PackageManager`). Recomps and ports from Flathub are this. Each is started
//! with the launcher Flatpak exports for it (`exports/bin/<app id>`, what the desktop's menu runs).

use std::collections::HashMap;
use std::path::PathBuf;

use crate::pcgames::PcGame;

/// Where Flatpak exports the apps: the user's own, then the system's.
pub fn roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    vec![home.join(".local/share/flatpak/exports"), PathBuf::from("/var/lib/flatpak/exports")]
}

/// A `.desktop` file's `[Desktop Entry]` keys; a key's plain form wins over its translations (`Name[de]` is a key of
/// its own).
pub fn desktop_entry(text: &str) -> HashMap<String, String> {
    let (mut o, mut main) = (HashMap::new(), false);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
            continue;
        }
        if let Some((k, v)) = line.split_once('=').filter(|(k, _)| main && !k.trim().is_empty()) {
            o.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    o
}

/// The Flatpak games under these roots, A to Z; an app in two roots once.
pub fn games(roots: &[PathBuf]) -> Vec<PcGame> {
    let mut out: Vec<PcGame> = Vec::new();
    for root in roots {
        let Ok(files) = std::fs::read_dir(root.join("share/applications")) else { continue };
        let mut files: Vec<PathBuf> = files.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "desktop")).collect();
        files.sort();
        for f in files {
            let Some(app_id) = f.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
            if !app_id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) || out.iter().any(|g| g.app_name == app_id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let e = desktop_entry(&text);
            let get = |k: &str| e.get(k).map_or("", String::as_str);
            let cats: Vec<String> = get("Categories").split(';').map(|c| c.trim().to_lowercase()).collect();
            let yes = |k: &str| get(k).eq_ignore_ascii_case("true");
            if (!get("Type").is_empty() && get("Type") != "Application") || yes("NoDisplay") || yes("Hidden") {
                continue;
            }
            if !cats.iter().any(|c| c == "game") || cats.iter().any(|c| c == "emulator" || c == "packagemanager") {
                continue;
            }
            let exe = root.join("bin").join(&app_id);
            if !exe.is_file() {
                continue;
            }
            let title: String = if get("Name").is_empty() { app_id.clone() } else { get("Name").chars().take(120).collect() };
            out.push(PcGame { id: format!("flatpak-{}", crate::pcgames::slug(&app_id)), source: "flatpak", title, exe: exe.display().to_string(), dir: root.join("bin").display().to_string(), app_name: app_id, ..PcGame::default() });
        }
    }
    out.sort_by(|a, b| crate::collate::compare(&a.title, &b.title));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_are_the_flatpaks_that_say_so_and_not_emulators_or_launchers() {
        let root = std::env::temp_dir().join(format!("rs-flatpak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("share/applications")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let app = |id: &str, desktop: &str| {
            std::fs::write(root.join("share/applications").join(format!("{id}.desktop")), desktop).unwrap();
            std::fs::write(root.join("bin").join(id), "#!/bin/sh\n").unwrap();
        };
        app("io.github.Zelda64Recomp", "[Desktop Entry]\nType=Application\nName=Zelda 64: Recompiled\nName[de]=Zelda 64\nCategories=Game;ActionGame;\n[Desktop Action x]\nName=Other\n");
        app("org.DolphinEmu.dolphin-emu", "[Desktop Entry]\nName=Dolphin\nCategories=Game;Emulator;\n");
        app("net.lutris.Lutris", "[Desktop Entry]\nName=Lutris\nCategories=Game;PackageManager;\n");
        app("org.gimp.GIMP", "[Desktop Entry]\nName=GIMP\nCategories=Graphics;\n");
        app("com.example.Hidden", "[Desktop Entry]\nName=Hidden\nCategories=Game;\nNoDisplay=true\n");
        std::fs::write(root.join("share/applications/com.example.NoLauncher.desktop"), "[Desktop Entry]\nName=Gone\nCategories=Game;\n").unwrap();
        let got = games(&[root.clone(), root.clone()]);
        assert_eq!(got.iter().map(|g| (g.title.as_str(), g.source, g.app_name.as_str())).collect::<Vec<_>>(), [("Zelda 64: Recompiled", "flatpak", "io.github.Zelda64Recomp")]);
        let _ = std::fs::remove_dir_all(&root);
    }
}

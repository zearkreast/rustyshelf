// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the shelves of
// Boxshelf's main.js library build, with main/pccovers.js (cover, consoleCover) and main/steamart.js (local), as
// RustyShelf has them.
//! Every shelf: each console's, read from the settings and the games folders, and the PC shelf's Steam games.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::art::{Art, ArtLibrary};
use crate::config::{Config, LoadError, SystemConfig};
use crate::paths::{self, is_file};
use crate::scanner::{self, Game, ScanData, Source};
use crate::steam::{self, SteamGame};
use crate::systems::{systems, Console};

#[derive(Debug, Clone)]
pub enum ShelfKind {
    Console { console: Box<Console>, config: SystemConfig },
    Pc,
}

#[derive(Debug, Clone)]
pub struct Shelf {
    /// the console's id, or "pc"
    pub id: String,
    pub name: String,
    pub kind: ShelfKind,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub app_root: PathBuf,
    pub config: Config,
    /// consoles that are on and have games, in the settings' console order, then the PC shelf
    pub shelves: Vec<Shelf>,
}

/// A PC game's id as a file name.
fn safe_id(id: &str) -> String {
    static UNSAFE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9._-]+").unwrap());
    UNSAFE.replace_all(&id.to_lowercase(), "-").chars().take(120).collect()
}

/// The cover the player chose for a PC game, in `data/pc-covers`.
fn chosen_cover(data: &Path, id: &str) -> Option<PathBuf> {
    let b = data.join("pc-covers").join(safe_id(id));
    ["jpg", "png", "webp"].iter().map(|e| PathBuf::from(format!("{}.{e}", b.display()))).find(|f| is_file(f))
}

/// The PC shelf's Steam games, from the Steam folder Settings names, else the one found in the usual place.
fn steam_shelf(cfg: &Config, data: &Path) -> Vec<Game> {
    let (enabled, path, hide_tools) = cfg.steam();
    if !enabled {
        return Vec::new();
    }
    // unset, wrong, or on a drive that doesn't answer: the Steam in the usual place
    let path = match steam::steam_root_within(&path) {
        Some(_) => path,
        None => steam::find_steam()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| steam::steam_root_within(p).is_some())
            .unwrap_or(path),
    };
    if path.is_empty() {
        return Vec::new();
    }
    // a library on another drive set as the Steam folder: Steam's own folder lists every library
    let main = steam::steam_root_within(&path)
        .filter(|r| !steam::is_main_steam_within(r))
        .and_then(|_| steam::find_steam())
        .filter(|p| steam::steam_root_within(&p.to_string_lossy()).is_some_and(|r| steam::is_main_steam_within(&r)))
        .map(|p| p.to_string_lossy().into_owned());
    let boxes = cfg.pc_console_box();
    steam::scan(&path, main.as_deref(), hide_tools).into_iter().map(|g| steam_game(g, cfg, data, boxes)).collect()
}

fn steam_game(g: SteamGame, cfg: &Config, data: &Path, boxes: bool) -> Game {
    let id = format!("steam-{}", g.appid);
    let art_dir = data.join("steam-art");
    let local = |name: String| Some(art_dir.join(name)).filter(|f| is_file(f));
    let console_box = || Some(data.join("pc-console-covers").join(format!("{}.jpg", safe_id(&id)))).filter(|f| is_file(f));
    // the player's pick, its console box, Steam's own cover, then one the Electron app fetched from Steam's store
    let front = chosen_cover(data, &id)
        .or_else(|| boxes.then(console_box).flatten())
        .or(g.front)
        .or_else(|| local(format!("{}_cover.jpg", g.appid)));
    let hero = g.hero.or_else(|| local(format!("{}_hero.jpg", g.appid)));
    Game {
        title: cfg.game_title(&id).map(String::from).unwrap_or(g.title),
        id,
        file: g.dir.unwrap_or_default(),
        size_bytes: g.size_bytes,
        art: Art { front, ..Art::default() },
        hero,
        source: Source::Steam { appid: g.appid, minutes: g.minutes, last_played: g.last_played },
        ..Game::default()
    }
}

impl Library {
    pub fn load() -> Result<Library, LoadError> {
        Self::load_from(&paths::app_root())
    }

    pub fn load_from(app_root: &Path) -> Result<Library, LoadError> {
        let data_dir = paths::data_dir(app_root);
        let config = Config::load(&data_dir)?;
        let lib = ArtLibrary::from_config(&config, app_root);
        let data = ScanData::load(&data_dir);
        let wanted: Vec<(&Console, SystemConfig)> = systems()
            .consoles
            .iter()
            .map(|c| (c, config.system(c)))
            .filter(|(_, sc)| sc.enabled && !sc.rom_dirs.is_empty())
            .collect();
        let (mut shelves, pc): (Vec<Shelf>, Vec<Game>) = std::thread::scope(|s| {
            let pc = s.spawn(|| steam_shelf(&config, &data_dir));
            let jobs: Vec<_> = wanted
                .into_iter()
                .map(|(c, sc)| {
                    let (dirs, data) = (lib.dirs_for(&sc, app_root), &data);
                    s.spawn(move || Shelf {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        games: scanner::scan(c, &sc, &dirs, data),
                        kind: ShelfKind::Console { console: Box::new(c.clone()), config: sc },
                    })
                })
                .collect();
            let shelves = jobs.into_iter().filter_map(|j| j.join().ok()).filter(|s| !s.games.is_empty()).collect();
            (shelves, pc.join().unwrap_or_default())
        });
        for g in shelves.iter_mut().flat_map(|s| s.games.iter_mut()) {
            if let Some(t) = config.game_title(&g.id) {
                g.title = t.to_string();
            }
        }
        if !pc.is_empty() {
            shelves.push(Shelf { id: "pc".into(), name: "PC".into(), kind: ShelfKind::Pc, games: pc });
        }
        Ok(Library { app_root: app_root.to_path_buf(), config, shelves })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pc_cover_names() {
        assert_eq!(safe_id("steam-12 34/X"), "steam-12-34-x");
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("pc-covers")).unwrap();
        std::fs::write(t.path().join("pc-covers/steam-10.png"), b"").unwrap();
        assert_eq!(chosen_cover(t.path(), "steam-10"), Some(t.path().join("pc-covers/steam-10.png")));
        let g = steam_game(
            SteamGame { appid: "10".into(), title: "Zeta".into(), size_bytes: 1, dir: None, minutes: 3, last_played: 0, front: Some("/s/f.jpg".into()), hero: None },
            &Config::parse(r#"{"games":{"steam-10":{"title":"Zeta Deluxe"}}}"#).unwrap(),
            t.path(),
            false,
        );
        assert_eq!((g.title.as_str(), g.art.front), ("Zeta Deluxe", Some(t.path().join("pc-covers/steam-10.png"))));
    }
}

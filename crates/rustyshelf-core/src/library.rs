//! Every console's shelf, read from the settings and the games folders.

use std::path::{Path, PathBuf};

use crate::art::ArtLibrary;
use crate::config::{Config, LoadError, SystemConfig};
use crate::paths;
use crate::scanner::{self, Game, ScanData};
use crate::systems::{systems, Console};

#[derive(Debug, Clone)]
pub struct Shelf {
    pub console: Console,
    pub config: SystemConfig,
    pub games: Vec<Game>,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub app_root: PathBuf,
    pub config: Config,
    /// consoles that are on and have games, in the settings' console order
    pub shelves: Vec<Shelf>,
}

impl Library {
    pub fn load() -> Result<Library, LoadError> {
        Self::load_from(&paths::app_root())
    }

    pub fn load_from(app_root: &Path) -> Result<Library, LoadError> {
        let config = Config::load(&paths::data_dir(app_root))?;
        let lib = ArtLibrary::from_config(&config, app_root);
        let data = ScanData::load(&paths::data_dir(app_root));
        let wanted: Vec<(&Console, SystemConfig)> = systems()
            .consoles
            .iter()
            .map(|c| (c, config.system(c)))
            .filter(|(_, sc)| sc.enabled && !sc.rom_dirs.is_empty())
            .collect();
        let mut shelves: Vec<Shelf> = std::thread::scope(|s| {
            let jobs: Vec<_> = wanted
                .into_iter()
                .map(|(c, sc)| {
                    let (dirs, data) = (lib.dirs_for(&sc, app_root), &data);
                    s.spawn(move || Shelf { games: scanner::scan(c, &sc, &dirs, data), console: c.clone(), config: sc })
                })
                .collect();
            jobs.into_iter().filter_map(|j| j.join().ok()).filter(|s| !s.games.is_empty()).collect()
        });
        for g in shelves.iter_mut().flat_map(|s| s.games.iter_mut()) {
            if let Some(t) = config.game_title(&g.id) {
                g.title = t.to_string();
            }
        }
        Ok(Library { app_root: app_root.to_path_buf(), config, shelves })
    }
}

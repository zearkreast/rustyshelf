// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the consoles of
// Boxshelf's main/systems.js, as RustyShelf has it.
//! Every shelf the launcher knows about. The definitions are main/systems.js's, exported to JSON by
//! scripts/export-systems.js (run it after changing systems.js).

use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Console {
    pub id: String,
    pub name: String,
    pub emulator_name: String,
    /// program names to look for; `re:` starts a regex
    pub exe_names: Vec<String>,
    /// may hold `%VAR%` and `{app}`
    pub emulator_folders: Vec<String>,
    pub rom_folder_names: Vec<String>,
    pub extensions: Vec<String>,
    /// launch options, `{rom}` is the game file
    pub game_args: String,
    pub emulator_args: String,
    pub cover_seconds: f64,
    #[serde(default)]
    pub deep: bool,
    #[serde(default)]
    pub hint: Option<String>,
    /// games are folders holding this file (eboot.bin)
    #[serde(default)]
    pub game_file: Option<String>,
    /// loose files taken besides `game_file` folders
    #[serde(default)]
    pub also_files: Vec<String>,
    #[serde(default)]
    pub ra_core: Option<String>,
    /// the RetroArch cores Settings offers: [id, name]
    #[serde(default)]
    pub ra_cores: Vec<(String, String)>,
    #[serde(default)]
    pub retro: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: String,
    pub name: String,
    pub exe_names: Vec<String>,
    pub folders: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Systems {
    pub consoles: Vec<Console>,
    pub apps: Vec<App>,
    pub search_roots: Vec<String>,
    pub rom_root_names: Vec<String>,
}

impl Systems {
    pub fn console(&self, id: &str) -> Option<&Console> {
        self.consoles.iter().find(|c| c.id == id)
    }
}

static WINDOWS: LazyLock<Systems> =
    LazyLock::new(|| serde_json::from_str(include_str!("../data/systems-windows.json")).expect("systems-windows.json"));
static LINUX: LazyLock<Systems> =
    LazyLock::new(|| serde_json::from_str(include_str!("../data/systems-linux.json")).expect("systems-linux.json"));

/// This platform's definitions (other Unix systems get Linux's).
pub fn systems() -> &'static Systems {
    if cfg!(windows) {
        &WINDOWS
    } else {
        &LINUX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_platforms_load() {
        assert_eq!(WINDOWS.consoles.len(), LINUX.consoles.len());
        let gc = WINDOWS.console("gc").unwrap();
        assert!(gc.game_args.contains("{rom}"));
        assert!(LINUX.console("ps4").unwrap().game_file.is_some());
    }
}

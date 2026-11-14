// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): reads the settings
// Boxshelf's main/config.js writes, as RustyShelf has it.
//! `data/config.json`, the Electron app's settings. Read only: the Electron app saves its whole copy over the file,
//! so a write from here would be lost.

use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use crate::systems::Console;

#[derive(Debug, Clone, Default)]
pub struct Config {
    raw: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemConfig {
    pub enabled: bool,
    /// the emulator program; may hold `%VAR%` and `{app}`
    pub emulator: String,
    pub game_args: String,
    pub emulator_args: String,
    pub rom_dirs: Vec<String>,
    /// one folder for this console's art instead of the ones beside the games
    pub art_dir: String,
    /// GameCube: show the console's own startup (the IPL), and keep Dolphin's warnings about it quiet
    pub use_ipl: bool,
    pub hide_dolphin_warnings: bool,
    /// the RetroArch core picked in Settings; empty for the console's own
    pub ra_core: String,
    /// PS5: games are started from Kyty's launcher, not straight in its emulator
    pub via_launcher: bool,
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    /// the file is there but isn't a settings object (truncated, hand-edited)
    Damaged(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "settings file could not be read: {e}"),
            LoadError::Damaged(e) => write!(f, "settings file is damaged: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Launch options an older version saved as the default, which load as today's default (main/config.js OLD_DEFAULTS:
/// keep in sync).
fn old_default(console_id: &str, args: &str) -> bool {
    matches!(console_id, "gc" | "wii") && args == r#"-b -e "{rom}""#
}

fn str_of<'a>(v: Option<&'a Value>, default: &'a str) -> &'a str {
    v.and_then(Value::as_str).unwrap_or(default)
}

impl Config {
    /// No file yet is a fresh setup: every value is its default.
    pub fn load(data_dir: &Path) -> Result<Config, LoadError> {
        let text = match fs::read_to_string(data_dir.join("config.json")) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(LoadError::Io(e)),
        };
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, LoadError> {
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(raw)) => Ok(Config { raw }),
            Ok(_) => Err(LoadError::Damaged("not a settings object".into())),
            Err(e) => Err(LoadError::Damaged(e.to_string())),
        }
    }

    pub fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }

    pub fn setup_done(&self) -> bool {
        self.raw.get("setupDone").and_then(Value::as_bool).unwrap_or(false)
    }

    fn systems_raw(&self) -> Option<&Map<String, Value>> {
        self.raw.get("systems").and_then(Value::as_object)
    }

    /// A value of the wrong type falls back to the default, as the Electron app's merge does for lists.
    pub fn system(&self, console: &Console) -> SystemConfig {
        let s = self.systems_raw().and_then(|m| m.get(&console.id)).and_then(Value::as_object);
        let get = |k: &str| s.and_then(|m| m.get(k));
        SystemConfig {
            enabled: get("enabled").and_then(Value::as_bool).unwrap_or(true),
            emulator: str_of(get("emulator"), "").to_string(),
            game_args: Some(str_of(get("gameArgs"), &console.game_args)).filter(|a| !old_default(&console.id, a)).unwrap_or(&console.game_args).to_string(),
            emulator_args: str_of(get("emulatorArgs"), &console.emulator_args).to_string(),
            rom_dirs: get("romDirs")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).filter(|d| !d.is_empty()).map(String::from).collect())
                .unwrap_or_default(),
            art_dir: str_of(get("artDir"), "").to_string(),
            use_ipl: get("useIpl").and_then(Value::as_bool).unwrap_or(false),
            hide_dolphin_warnings: get("hideDolphinWarnings").and_then(Value::as_bool).unwrap_or(false),
            ra_core: str_of(get("raCore"), "").to_string(),
            via_launcher: get("viaLauncher").and_then(Value::as_bool).unwrap_or(false),
        }
    }

    /// Settings → PC games → Steam: on, its folder, and whether its tools (Proton, runtimes …) are left off the shelf.
    pub fn steam(&self) -> (bool, String, bool) {
        let s = self.raw.get("steam").and_then(Value::as_object);
        let get = |k: &str| s.and_then(|m| m.get(k));
        (
            get("enabled").and_then(Value::as_bool).unwrap_or(true),
            str_of(get("path"), "").to_string(),
            get("hideTools").and_then(Value::as_bool).unwrap_or(true),
        )
    }

    /// Settings → PC games → Console box art: a PC game shown in its console box.
    pub fn pc_console_box(&self) -> bool {
        self.raw.get("pc").and_then(|p| p.get("consoleBox")).and_then(Value::as_bool).unwrap_or(false)
    }

    /// Settings → RetroArch: the program the retro consoles fall back on, unexpanded.
    pub fn retroarch(&self) -> String {
        str_of(self.raw.get("retroarch"), "").trim().to_string()
    }

    /// A game's own choices (Details), by its id (`<console>:<file>`): `emulator`, `title` …
    pub fn game(&self, id: &str) -> Option<&Map<String, Value>> {
        self.raw.get("games").and_then(Value::as_object).and_then(|g| g.get(id)).and_then(Value::as_object)
    }

    /// The name the player gave a game (Details → Rename).
    pub fn game_title(&self, id: &str) -> Option<&str> {
        self.game(id).and_then(|g| g.get("title")).and_then(Value::as_str).filter(|t| !t.is_empty())
    }

    /// The emulator build a game runs in instead of its console's, unexpanded.
    pub fn game_emulator(&self, id: &str) -> Option<&str> {
        self.game(id).and_then(|g| g.get("emulator")).and_then(Value::as_str).filter(|t| !t.is_empty())
    }

    /// Settings → Box art → the art library folder, unexpanded; empty when the art sits beside the games.
    pub fn art_library(&self) -> String {
        let ui = self.raw.get("ui").and_then(Value::as_object);
        str_of(ui.and_then(|u| u.get("artLibrary")), "").to_string()
    }

    /// Every games folder whose console has no art folder of its own: the art library has a folder for each.
    pub fn library_roots(&self) -> Vec<String> {
        let Some(systems) = self.systems_raw() else { return Vec::new() };
        systems
            .values()
            .filter_map(Value::as_object)
            .filter(|s| str_of(s.get("artDir"), "").is_empty())
            .flat_map(|s| s.get("romDirs").and_then(Value::as_array).into_iter().flatten())
            .filter_map(Value::as_str)
            .filter(|d| !d.is_empty())
            .map(String::from)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::systems;

    #[test]
    fn saved_values_over_defaults() {
        let gc = systems().console("gc").unwrap();
        let cfg = Config::parse(r#"{"setupDone":true,"systems":{"gc":{"emulator":"/bin/dolphin","romDirs":["/g",""],"enabled":false,"gameArgs":7}}}"#).unwrap();
        let s = cfg.system(gc);
        assert!(cfg.setup_done());
        assert!(!s.enabled);
        assert_eq!(s.emulator, "/bin/dolphin");
        assert_eq!(s.rom_dirs, vec!["/g"]);
        assert_eq!(s.game_args, gc.game_args);
        assert_eq!(cfg.library_roots(), vec!["/g"]);
    }

    #[test]
    fn old_default_launch_options() {
        let cfg = Config::parse(r#"{"systems":{"gc":{"gameArgs":"-b -e \"{rom}\""},"wii":{"gameArgs":"-e \"{rom}\""}}}"#).unwrap();
        let (gc, wii) = (systems().console("gc").unwrap(), systems().console("wii").unwrap());
        assert_eq!(cfg.system(gc).game_args, gc.game_args);
        assert_eq!(cfg.system(wii).game_args, r#"-e "{rom}""#);
    }

    #[test]
    fn damaged_file_is_an_error() {
        assert!(matches!(Config::parse("[1]"), Err(LoadError::Damaged(_))));
        assert!(matches!(Config::parse("{\"a\":"), Err(LoadError::Damaged(_))));
    }
}

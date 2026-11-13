// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/launcher.js, as RustyShelf has it.
//! Starting an emulator with a game.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use crate::config::SystemConfig;
use crate::paths::{expand, is_file};

static ARG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""([^"]*)"|(\S+)"#).unwrap());

/// Splits launch options like `-b -e "{rom}"` into arguments, `{rom}` filled in.
pub fn build_args(template: &str, rom: &str) -> Vec<String> {
    ARG.captures_iter(template)
        .map(|c| c.get(1).or(c.get(2)).map_or("", |m| m.as_str()).replace("{rom}", rom))
        .filter(|a| !a.is_empty())
        .collect()
}

#[derive(Debug)]
pub enum LaunchError {
    /// no emulator set, or it isn't where Settings says
    NotFound(String),
    Spawn(std::io::Error),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotFound(p) if p.is_empty() => write!(f, "No emulator is set for this console. Choose one in Settings."),
            LaunchError::NotFound(p) => write!(f, "Can't find {p}. Check its path in Settings."),
            LaunchError::Spawn(e) => write!(f, "The emulator didn't start: {e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// The program as Settings has it: `%VAR%`, `{app}`, and "quotes" pasted around a path.
pub fn resolve_exe(exe: &str, app_root: &Path) -> Result<PathBuf, LaunchError> {
    let t = exe.trim();
    let t = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(t);
    let p = PathBuf::from(expand(t, app_root));
    if t.is_empty() || !is_file(&p) {
        return Err(LaunchError::NotFound(if t.is_empty() { String::new() } else { p.display().to_string() }));
    }
    Ok(p)
}

/// Starts the program in its own folder. It isn't tied to RustyShelf: closing the shelf leaves the game running.
pub fn start(exe: &Path, args: &[String]) -> Result<Child, LaunchError> {
    let mut cmd = Command::new(exe);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.spawn().map_err(LaunchError::Spawn)
}

pub fn launch_game(sc: &SystemConfig, rom: &Path, app_root: &Path) -> Result<Child, LaunchError> {
    let exe = resolve_exe(&sc.emulator, app_root)?;
    start(&exe, &build_args(&sc.game_args, &rom.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args() {
        assert_eq!(
            build_args(r#"-b -C Dolphin.Display.Fullscreen=True -e "{rom}""#, "/g/Ca$$h (USA).iso"),
            ["-b", "-C", "Dolphin.Display.Fullscreen=True", "-e", "/g/Ca$$h (USA).iso"]
        );
        assert_eq!(build_args(r#""" x"#, ""), ["x"]);
    }

    #[test]
    fn missing_emulator() {
        assert!(matches!(resolve_exe("", Path::new("/")), Err(LaunchError::NotFound(p)) if p.is_empty()));
        assert!(matches!(resolve_exe("\"/no/such/emu\"", Path::new("/")), Err(LaunchError::NotFound(_))));
    }
}

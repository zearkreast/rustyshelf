// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/paths.js and main/linux.js (appRoot), as RustyShelf has them.
//! Where RustyShelf keeps its things, and the `%VAR%` / `{app}` placeholders in paths.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

static APP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\{app\}").unwrap());
static VAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%([^%]+)%").unwrap());

/// Fills in `{app}` and `%VAR%` (any case; an unknown variable stays as written).
pub fn expand(p: &str, app_root: &Path) -> String {
    expand_with(p, app_root, &env_var)
}

/// An environment variable by its name in any case, as Windows reads them.
pub fn env_var(name: &str) -> Option<String> {
    let want = name.to_lowercase();
    env::vars_os().find(|(k, _)| k.to_string_lossy().to_lowercase() == want).map(|(_, v)| v.to_string_lossy().into_owned())
}

/// `expand` with the variables looked up by `var`.
pub fn expand_with(p: &str, app_root: &Path, var: &dyn Fn(&str) -> Option<String>) -> String {
    let root = app_root.to_string_lossy();
    let p = APP.replace_all(p, |_: &Captures| root.to_string());
    VAR.replace_all(&p, |c: &Captures| var(&c[1]).unwrap_or_else(|| c[0].to_string())).into_owned()
}

/// Windows: a drive with nothing in it (an empty card reader, an optical drive) fails quietly when it's looked at,
/// instead of raising "There is no disk in the drive". Called once as the app starts.
pub fn quiet_drive_errors() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn SetErrorMode(mode: u32) -> u32;
        }
        const SEM_FAILCRITICALERRORS: u32 = 1;
        // SAFETY: a kernel32 call that only sets this process's error mode
        unsafe {
            SetErrorMode(SEM_FAILCRITICALERRORS);
        }
    }
}

pub fn is_dir(p: &Path) -> bool {
    p.metadata().map(|m| m.is_dir()).unwrap_or(false)
}

pub fn is_file(p: &Path) -> bool {
    p.metadata().map(|m| m.is_file()).unwrap_or(false)
}

/// The folder RustyShelf lives in: `data\`, `Custom themes\`, `Songs\` … The Electron app picks the same one, so both
/// share a setup.
///
/// `RUSTYSHELF_ROOT` wins. Then the program's own folder when it has a `data` folder (a portable setup; always on
/// Windows). On Linux otherwise `~/RustyShelf`, or the `~/TrustyShelf` / `~/Boxshelf` an earlier version made.
pub fn app_root() -> PathBuf {
    if let Some(r) = env::var_os("RUSTYSHELF_ROOT").filter(|r| !r.is_empty()) {
        return PathBuf::from(r);
    }
    let beside = env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    if let Some(b) = beside.as_ref().filter(|b| is_dir(&b.join("data"))) {
        return b.clone();
    }
    if cfg!(windows) {
        return beside.unwrap_or_else(|| PathBuf::from("."));
    }
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return PathBuf::from(".");
    };
    let root = home.join("RustyShelf");
    if !is_dir(&root) {
        for old in ["TrustyShelf", "Boxshelf"] {
            let d = home.join(old);
            if is_dir(&d.join("data")) {
                return d;
            }
        }
    }
    root
}

/// `<app root>/data`: config.json, history, caches.
pub fn data_dir(app_root: &Path) -> PathBuf {
    app_root.join("data")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders() {
        env::set_var("RS_TEST_VAR", "/x");
        let root = Path::new("/apps/rs");
        assert_eq!(expand("{APP}/Dolphin", root), "/apps/rs/Dolphin");
        assert_eq!(expand("%rs_test_var%/y", root), "/x/y");
        assert_eq!(expand("%NOPE_NOT_SET%/y", root), "%NOPE_NOT_SET%/y");
    }
}

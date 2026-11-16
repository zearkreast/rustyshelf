//! The system's own dialogs and clipboard. The desktop's come from rfd and arboard; Android has neither (its folders
//! are picked through the Storage Access Framework), so there they answer nothing.

use std::path::PathBuf;

/// One file; `programs`: Windows' programs only.
#[cfg(not(target_os = "android"))]
pub fn pick_file(title: &str, programs: bool) -> Option<PathBuf> {
    let mut d = rfd::FileDialog::new().set_title(title);
    if programs && cfg!(windows) {
        d = d.add_filter("Programs", &["exe"]);
    }
    d.pick_file()
}

#[cfg(not(target_os = "android"))]
pub fn pick_files(title: &str) -> Vec<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_files().unwrap_or_default()
}

#[cfg(not(target_os = "android"))]
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(not(target_os = "android"))]
pub fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

#[cfg(target_os = "android")]
pub fn pick_file(_title: &str, _programs: bool) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "android")]
pub fn pick_files(_title: &str) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(target_os = "android")]
pub fn pick_folder(_title: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "android")]
pub fn clipboard_text() -> Option<String> {
    None
}

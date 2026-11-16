//! The system's own dialogs and clipboard. The desktop's come from rfd and arboard; on Android a folder is picked
//! through the Storage Access Framework, and there's no file picker or clipboard yet.

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

/// Android's folder picker: the folder is kept with the others picked (data/folders.json), and the path it stands for
/// is answered.
#[cfg(target_os = "android")]
pub fn pick_folder(_title: &str) -> Option<PathBuf> {
    let uri = crate::android_saf::pick_folder()?;
    let data = rustyshelf_core::paths::data_dir(&rustyshelf_core::paths::app_root());
    rustyshelf_core::android::add_folder(&data, &uri).map(PathBuf::from)
}

#[cfg(target_os = "android")]
pub fn clipboard_text() -> Option<String> {
    None
}

// Ported from RustyShelf's main/music.js; the Songs folder's part is based on Boxshelf by sassiestrhihno
// (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from Boxshelf's main/songs.js, as RustyShelf
// has it.
//! Your own menu music and songs. Audio files in `<RustyShelf folder>/Music` (sub-folders too; the ones in the
//! Songs folder count as well) play, shuffled, as the menu music. MIDI files in `<RustyShelf folder>/Songs` are
//! songs for the Melody voice.

use std::path::{Path, PathBuf};

use crate::collate;
use crate::cues::{parse_midi, Song};

const MUSIC_FILES: usize = 500;
const MIDI_FILES: usize = 300;
/// MIDI files read into songs, and the biggest one that's read.
const SONGS: usize = 120;
const MIDI_BYTES: u64 = 2 << 20;
const AUDIO: [&str; 9] = ["mp3", "ogg", "oga", "opus", "wav", "flac", "m4a", "aac", "weba"];
const MIDI: [&str; 2] = ["mid", "midi"];

pub fn music_folder(app_root: &Path) -> PathBuf {
    app_root.join("Music")
}

pub fn songs_folder(app_root: &Path) -> PathBuf {
    app_root.join("Songs")
}

/// A file's name without its ending, when the ending is one of `endings` (whatever its case).
fn named(file: &str, endings: &[&str]) -> Option<String> {
    let (stem, end) = file.rsplit_once('.')?;
    endings.iter().any(|e| e.eq_ignore_ascii_case(end)).then(|| stem.to_string())
}

/// A track's or a song's name from its file's: underscores are spaces.
fn title(stem: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in stem.chars() {
        if c == '_' {
            if !run {
                out.push(' ');
            }
            run = true;
        } else {
            out.push(c);
            run = false;
        }
    }
    crate::js::trim(&out).to_string()
}

/// The files with one of `endings` under `root`, three folders deep at most, each folder in natural order;
/// hidden ones left out. Stops at `most`.
fn walk(root: &Path, endings: &[&str], most: usize, out: &mut Vec<(String, PathBuf)>) {
    fn step(dir: &Path, depth: u32, endings: &[&str], most: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(list) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<(String, bool)> = list.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.file_type().is_ok_and(|t| t.is_dir()))).collect();
        entries.sort_by(|a, b| collate::compare(&a.0, &b.0));
        for (name, is_dir) in entries {
            if out.len() >= most || name.starts_with('.') {
                continue;
            }
            let full = dir.join(&name);
            if is_dir {
                if depth < 3 {
                    step(&full, depth + 1, endings, most, out);
                }
            } else if let Some(stem) = named(&name, endings) {
                out.push((title(&stem), full));
            }
        }
    }
    step(root, 0, endings, most, out);
}

/// Your music: each track's name and file, the Music folder's first (made if it isn't there), then the Songs
/// folder's.
pub fn music_files(app_root: &Path) -> Vec<(String, PathBuf)> {
    let _ = std::fs::create_dir_all(music_folder(app_root));
    let mut out = Vec::new();
    for root in [music_folder(app_root), songs_folder(app_root)] {
        walk(&root, &AUDIO, MUSIC_FILES, &mut out);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|(_, f)| seen.insert(f.to_string_lossy().to_lowercase()));
    out
}

/// The MIDI files in the Songs folder (made if it isn't there): each one's name and file.
pub fn midi_files(app_root: &Path) -> Vec<(String, PathBuf)> {
    let _ = std::fs::create_dir_all(songs_folder(app_root));
    let mut out = Vec::new();
    walk(&songs_folder(app_root), &MIDI, MIDI_FILES, &mut out);
    out
}

/// Your songs for the Melody voice, read from the Songs folder; and the names of the files that couldn't be read
/// as a song.
pub fn songs(app_root: &Path) -> (Vec<Song>, Vec<String>) {
    let (mut songs, mut skipped) = (Vec::new(), Vec::new());
    for (name, file) in midi_files(app_root).into_iter().take(SONGS) {
        let bytes = file.metadata().ok().filter(|m| m.len() <= MIDI_BYTES).and_then(|_| std::fs::read(&file).ok());
        match bytes.and_then(|b| parse_midi(&b, &name).ok()) {
            Some(song) => songs.push(song),
            None => skipped.push(name),
        }
    }
    (songs, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let put = |p: &str| {
            let f = tmp.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, b"x").unwrap();
        };
        for f in ["Music/b_side__two.MP3", "Music/Track 10.ogg", "Music/Track 2.ogg", "Music/.hidden.mp3", "Music/notes.txt", "Music/a/b/c/deep.flac", "Music/a/b/c/d/too deep.flac", "Songs/theme.mid", "Songs/extra.wav", "Songs/Old_Tune.MIDI"] {
            put(f);
        }
        let names = |l: Vec<(String, PathBuf)>| l.into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        assert_eq!(names(music_files(tmp.path())), ["deep", "b side two", "Track 2", "Track 10", "extra"]);
        assert_eq!(names(midi_files(tmp.path())), ["Old Tune", "theme"]);
        // not MIDI inside: listed, and skipped as a song
        let (songs, skipped) = songs(tmp.path());
        assert!(songs.is_empty());
        assert_eq!(skipped, ["Old Tune", "theme"]);
        assert!(music_folder(&tmp.path().join("fresh")).parent().is_some() && music_files(&tmp.path().join("fresh")).is_empty() && tmp.path().join("fresh/Music").is_dir());
    }
}

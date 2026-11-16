//! Where the shelf was left, for Settings → Look → Open into's "Last shelf": `data/native-place.json` holds the shelf
//! and the game by id. (The Electron app keeps its own place in its page's storage, which this app can't read.)

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde_json::{json, Value};

use crate::loading::{LibraryRes, LibraryState};
use crate::shelf::Cursor;

/// seconds the cursor stays put before its place is written
const SETTLE: f32 = 2.0;

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("native-place.json")
}

/// The shelf and game left on last time, by id.
pub fn read(data_dir: &Path) -> Option<(String, String)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(file(data_dir)).ok()?).ok()?;
    Some((v["shelf"].as_str()?.to_string(), v["game"].as_str().unwrap_or_default().to_string()))
}

/// Puts the cursor where `openInto` says, on the library's first showing: Home, or the place left last time.
pub fn open_into(open_into: &str, data_dir: &Path, lib: &rustyshelf_core::library::Library, cursor: &mut Cursor) {
    let want = if open_into == "home" { Some(("home".to_string(), String::new())) } else { read(data_dir) };
    let Some((shelf, game)) = want else { return };
    let Some(s) = lib.shelves.iter().position(|s| s.id == shelf) else { return };
    cursor.shelf = s;
    if let Some(g) = lib.shelves[s].games.iter().position(|g| g.id == game) {
        (cursor.game, cursor.land) = (g, false);
    }
}

pub fn keep(time: Res<Time>, lib: Res<LibraryRes>, cursor: Res<Cursor>, mut seen: Local<Option<(String, String)>>, mut since: Local<f32>, mut kept: Local<Option<(String, String)>>) {
    let LibraryState::Ready(l) = &lib.0 else { return };
    let Some(shelf) = l.shelves.get(cursor.shelf) else { return };
    let here = (shelf.id.clone(), shelf.games.get(cursor.game).map(|g| g.id.clone()).unwrap_or_default());
    let now = time.elapsed_secs();
    if seen.as_ref() != Some(&here) {
        (*seen, *since) = (Some(here), now);
        return;
    }
    if now - *since < SETTLE || kept.as_ref() == Some(&here) {
        return;
    }
    let path = file(&rustyshelf_core::paths::data_dir(&l.app_root));
    if std::fs::write(&path, json!({ "shelf": here.0, "game": here.1 }).to_string()).is_ok() {
        *kept = Some(here);
    }
}

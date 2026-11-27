//! What's new: the first start of a new native build shows its highlights once, on the shelf (a fresh install only
//! remembers the build). The build last seen is kept in data/native-whatsnew.json: the Electron app keeps its own
//! version in the settings, and the two would undo each other there.

use bevy::prelude::*;
use rustyshelf_core::paths;

use crate::card::{Card, Content, Outcome};
use crate::loading::{LibraryRes, LibraryState};
use crate::play::Playing;
use crate::settings::Settings;

#[derive(Resource, Default)]
pub struct WhatsNew {
    checked: bool,
    showing: bool,
}

/// The shelf is up and nothing else is: a card may show.
pub fn quiet(lib: &LibraryRes, settings: &Settings, playing: &Playing, card: &Card) -> bool {
    matches!(lib.0, LibraryState::Ready(_)) && !settings.open && !settings.keys_taken && !settings.just_closed && playing.title.is_none() && !card.is_open() && !crate::tour::ON.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn run(time: Res<Time>, lib: Res<LibraryRes>, mut wn: ResMut<WhatsNew>, mut card: ResMut<Card>, mut settings: ResMut<Settings>, playing: Res<Playing>) {
    if wn.showing {
        if let Some(o) = card.take() {
            wn.showing = false;
            if o == Outcome::Picked(1) {
                settings.open_whatsnew();
            }
        }
        return;
    }
    if wn.checked || time.elapsed_secs() < 5.0 || !quiet(&lib, &settings, &playing, &card) {
        return;
    }
    wn.checked = true;
    let notes = crate::settings::notes();
    let Some(top) = notes.first() else { return };
    let file = paths::data_dir(&paths::app_root()).join("native-whatsnew.json");
    let seen = rustyshelf_core::files::read_json(&file).and_then(|v| v["seen"].as_str().map(str::to_string)).unwrap_or_default();
    if seen == top.version {
        return;
    }
    let _ = rustyshelf_core::files::write_atomic(&file, serde_json::json!({ "seen": top.version }).to_string());
    if !settings.setup_done() {
        return;
    }
    card.show(Content {
        kick: "Updated".into(),
        title: format!("What's new in {}", top.name),
        sub: top.title.clone(),
        sections: top.sections.iter().map(|s| (s.h.clone(), s.items.clone())).collect(),
        // (a version with no sections: its highlights)
        items: if top.sections.is_empty() { top.highlights.iter().take(5).cloned().collect() } else { Vec::new() },
        keys: Vec::new(),
        buttons: vec!["Got it".into(), "All the notes".into()],
        ..Default::default()
    });
    wn.showing = true;
}
